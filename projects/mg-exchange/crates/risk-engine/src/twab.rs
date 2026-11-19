//! Time-Weighted Average Balance (TWAB) for earn/staking yield calculation.
//!
//! Instead of using a snapshot balance at a single point in time to compute
//! yield, TWAB integrates the balance over the entire yield period. A user
//! who deposits 1 minute before the snapshot gets proportional yield (not
//! the full period).
//!
//! # Formula
//! TWAB = Σ(balance_i × duration_i) / total_duration
//!
//! # Reference
//! - PoolTogether TWAB implementation (ERC-4626)
//! - Celsius/BlockFi insolvency: unsustainable yields from snapshot-based calc

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

/// A balance change event for TWAB tracking.
#[derive(Debug, Clone)]
pub struct BalanceEvent {
    pub balance: Decimal,
    pub timestamp: DateTime<Utc>,
}

/// Calculate the time-weighted average balance over a period.
///
/// `events` — chronologically ordered balance change events within the period.
/// `period_start` — start of the yield period.
/// `period_end` — end of the yield period.
///
/// Returns the TWAB. If no events, returns zero.
pub fn calculate_twab(
    events: &[BalanceEvent],
    period_start: DateTime<Utc>,
    period_end: DateTime<Utc>,
) -> Decimal {
    let total_duration = (period_end - period_start).num_seconds();
    if total_duration <= 0 || events.is_empty() {
        return Decimal::ZERO;
    }

    let mut weighted_sum = Decimal::ZERO;
    let mut prev_balance = Decimal::ZERO;
    let mut prev_time = period_start;

    for event in events {
        // Clamp event time to the period boundaries.
        let event_time = if event.timestamp < period_start {
            // Event before period — just update the starting balance.
            prev_balance = event.balance;
            continue;
        } else if event.timestamp > period_end {
            period_end
        } else {
            event.timestamp
        };

        // Add the weighted contribution of the previous balance.
        let duration = (event_time - prev_time).num_seconds();
        if duration > 0 {
            weighted_sum += prev_balance * Decimal::from(duration);
        }

        prev_balance = event.balance;
        prev_time = event_time;

        if event.timestamp >= period_end {
            break;
        }
    }

    // Account for the final segment (from last event to period_end).
    let remaining = (period_end - prev_time).num_seconds();
    if remaining > 0 {
        weighted_sum += prev_balance * Decimal::from(remaining);
    }

    weighted_sum / Decimal::from(total_duration)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use rust_decimal_macros::dec;

    fn base_time() -> DateTime<Utc> {
        Utc::now()
    }

    #[test]
    fn constant_balance_entire_period() {
        let start = base_time();
        let end = start + Duration::hours(24);
        let events = vec![BalanceEvent {
            balance: dec!(1000),
            timestamp: start,
        }];

        let twab = calculate_twab(&events, start, end);
        assert_eq!(twab, dec!(1000));
    }

    #[test]
    fn deposit_at_95_percent_gets_5_percent_yield() {
        let start = base_time();
        let end = start + Duration::hours(100);
        let deposit_time = start + Duration::hours(95); // 95% through

        let events = vec![
            BalanceEvent { balance: dec!(0), timestamp: start },
            BalanceEvent { balance: dec!(1000), timestamp: deposit_time },
        ];

        let twab = calculate_twab(&events, start, end);
        // 0 for 95 hours + 1000 for 5 hours = 5000 / 100 = 50
        // That's 5% of 1000.
        assert_eq!(twab, dec!(50));
    }

    #[test]
    fn deposit_at_start_gets_full_yield() {
        let start = base_time();
        let end = start + Duration::hours(24);

        let events = vec![BalanceEvent {
            balance: dec!(5000),
            timestamp: start,
        }];

        let twab = calculate_twab(&events, start, end);
        assert_eq!(twab, dec!(5000));
    }

    #[test]
    fn no_events_returns_zero() {
        let start = base_time();
        let end = start + Duration::hours(24);
        let twab = calculate_twab(&[], start, end);
        assert_eq!(twab, Decimal::ZERO);
    }

    #[test]
    fn partial_withdrawal_mid_period() {
        let start = base_time();
        let end = start + Duration::hours(10);

        let events = vec![
            BalanceEvent { balance: dec!(1000), timestamp: start },
            BalanceEvent { balance: dec!(500), timestamp: start + Duration::hours(5) },
        ];

        let twab = calculate_twab(&events, start, end);
        // 1000 * 5h + 500 * 5h = 7500 / 10 = 750
        assert_eq!(twab, dec!(750));
    }
}
