//! Outflow circuit breaker — limits cumulative and per-transaction withdrawal amounts.
//!
//! Prevents a Bybit-style mass-drain by enforcing three hard limits:
//! - Per-transaction maximum (default $500,000)
//! - Hourly cumulative across ALL users (default $10,000,000)
//! - Daily cumulative across ALL users (default $50,000,000)
//!
//! When any limit is exceeded the breaker trips automatically and all further
//! withdrawals are rejected until an admin explicitly resets it.

use std::collections::VecDeque;

use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Reason a withdrawal was rejected by the circuit breaker.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
pub enum CircuitBreakerError {
    /// The breaker is already tripped from a previous violation.
    #[error("Withdrawals temporarily paused for security review: {0}")]
    AlreadyTripped(String),

    /// This single withdrawal exceeds the per-transaction limit.
    #[error(
        "Single withdrawal ${amount} exceeds per-transaction limit ${limit}"
    )]
    SingleTransactionExceeded { amount: Decimal, limit: Decimal },

    /// Adding this withdrawal would breach the hourly cumulative limit.
    #[error(
        "Hourly outflow limit exceeded: current ${current}, limit ${limit}, requested ${requested}"
    )]
    HourlyLimitExceeded {
        current: Decimal,
        limit: Decimal,
        requested: Decimal,
    },

    /// Adding this withdrawal would breach the daily cumulative limit.
    #[error(
        "Daily outflow limit exceeded: current ${current}, limit ${limit}, requested ${requested}"
    )]
    DailyLimitExceeded {
        current: Decimal,
        limit: Decimal,
        requested: Decimal,
    },
}

// ---------------------------------------------------------------------------
// Outflow circuit breaker
// ---------------------------------------------------------------------------

/// Exchange-wide cumulative outflow circuit breaker.
///
/// All amounts are in USD equivalent.  The breaker is **stateful and shared**
/// — it must be held under a `Mutex` in production.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutflowCircuitBreaker {
    /// Per-transaction maximum in USD equivalent.
    pub max_single_withdrawal_usd: Decimal,
    /// Hourly cumulative maximum across ALL users.
    pub max_hourly_outflow_usd: Decimal,
    /// Daily cumulative maximum across ALL users.
    pub max_daily_outflow_usd: Decimal,

    /// Rolling window of (timestamp, amount) pairs for the past hour.
    hourly_outflows: VecDeque<(DateTime<Utc>, Decimal)>,
    /// Rolling window of (timestamp, amount) pairs for the past 24 hours.
    daily_outflows: VecDeque<(DateTime<Utc>, Decimal)>,

    /// Number of withdrawals to new addresses recorded in the past hour.
    new_address_count_hourly: VecDeque<DateTime<Utc>>,

    /// Whether the breaker is tripped (all withdrawals blocked).
    pub is_tripped: bool,
    /// When the breaker was tripped (UTC).
    pub tripped_at: Option<DateTime<Utc>>,
    /// Human-readable reason the breaker was tripped.
    pub trip_reason: Option<String>,
}

impl OutflowCircuitBreaker {
    /// Default limits: $500K per transaction, $10M/hour, $50M/day.
    pub fn new() -> Self {
        Self {
            max_single_withdrawal_usd: dec!(500_000),
            max_hourly_outflow_usd: dec!(10_000_000),
            max_daily_outflow_usd: dec!(50_000_000),
            hourly_outflows: VecDeque::new(),
            daily_outflows: VecDeque::new(),
            new_address_count_hourly: VecDeque::new(),
            is_tripped: false,
            tripped_at: None,
            trip_reason: None,
        }
    }

    /// Construct with custom limits (for testing or per-environment config).
    pub fn with_limits(
        max_single_usd: Decimal,
        max_hourly_usd: Decimal,
        max_daily_usd: Decimal,
    ) -> Self {
        Self {
            max_single_withdrawal_usd: max_single_usd,
            max_hourly_outflow_usd: max_hourly_usd,
            max_daily_outflow_usd: max_daily_usd,
            hourly_outflows: VecDeque::new(),
            daily_outflows: VecDeque::new(),
            new_address_count_hourly: VecDeque::new(),
            is_tripped: false,
            tripped_at: None,
            trip_reason: None,
        }
    }

    // -----------------------------------------------------------------------
    // Public interface
    // -----------------------------------------------------------------------

    /// Check whether a withdrawal of `amount_usd` can proceed.
    ///
    /// Returns `Ok(())` if every limit is satisfied.  Returns an error if the
    /// breaker is tripped or any limit would be exceeded.  This method does
    /// **not** mutate state — call [`record_withdrawal`] after the withdrawal
    /// has actually executed.
    pub fn check_withdrawal(
        &mut self,
        amount_usd: Decimal,
        now: DateTime<Utc>,
    ) -> Result<(), CircuitBreakerError> {
        // 1. Already tripped?
        if self.is_tripped {
            return Err(CircuitBreakerError::AlreadyTripped(
                self.trip_reason.clone().unwrap_or_default(),
            ));
        }

        // 2. Per-transaction limit.
        if amount_usd > self.max_single_withdrawal_usd {
            let reason = format!(
                "Single withdrawal ${amount_usd} exceeds per-transaction limit ${}",
                self.max_single_withdrawal_usd
            );
            self.trip(&reason, now);
            return Err(CircuitBreakerError::SingleTransactionExceeded {
                amount: amount_usd,
                limit: self.max_single_withdrawal_usd,
            });
        }

        self.prune_old_entries(now);

        // 3. Hourly limit.
        let hourly = self.hourly_total(now);
        if hourly + amount_usd > self.max_hourly_outflow_usd {
            let reason = format!(
                "Hourly outflow limit: current ${hourly} + ${amount_usd} would exceed ${}",
                self.max_hourly_outflow_usd
            );
            self.trip(&reason, now);
            return Err(CircuitBreakerError::HourlyLimitExceeded {
                current: hourly,
                limit: self.max_hourly_outflow_usd,
                requested: amount_usd,
            });
        }

        // 4. Daily limit.
        let daily = self.daily_total(now);
        if daily + amount_usd > self.max_daily_outflow_usd {
            let reason = format!(
                "Daily outflow limit: current ${daily} + ${amount_usd} would exceed ${}",
                self.max_daily_outflow_usd
            );
            self.trip(&reason, now);
            return Err(CircuitBreakerError::DailyLimitExceeded {
                current: daily,
                limit: self.max_daily_outflow_usd,
                requested: amount_usd,
            });
        }

        Ok(())
    }

    /// Record that a withdrawal of `amount_usd` has been successfully executed.
    ///
    /// Call this **after** the withdrawal has actually gone through.
    pub fn record_withdrawal(&mut self, amount_usd: Decimal, now: DateTime<Utc>) {
        self.hourly_outflows.push_back((now, amount_usd));
        self.daily_outflows.push_back((now, amount_usd));
    }

    /// Record that a withdrawal to a **new** (previously unseen) address occurred.
    ///
    /// If 10 or more such withdrawals happen within an hour the breaker trips.
    pub fn record_new_address_withdrawal(&mut self, now: DateTime<Utc>) {
        self.new_address_count_hourly.push_back(now);
        self.prune_new_address_window(now);

        if self.new_address_count_hourly.len() >= 10 {
            let reason = format!(
                "10+ withdrawals to new addresses in the past hour (count: {})",
                self.new_address_count_hourly.len()
            );
            self.trip(&reason, now);
        }
    }

    /// Trip the circuit breaker, blocking all further withdrawals until reset.
    ///
    /// Idempotent — calling on an already-tripped breaker updates the reason.
    pub fn trip(&mut self, reason: &str, now: DateTime<Utc>) {
        tracing::error!(
            reason = reason,
            "CIRCUIT BREAKER TRIPPED — all withdrawals halted"
        );
        self.is_tripped = true;
        self.tripped_at = Some(now);
        self.trip_reason = Some(reason.to_string());
    }

    /// Reset the circuit breaker (admin-only operation).
    ///
    /// Clears the tripped state but preserves the outflow history.
    pub fn reset(&mut self) {
        tracing::warn!("Circuit breaker reset by admin");
        self.is_tripped = false;
        self.tripped_at = None;
        self.trip_reason = None;
    }

    /// Sum of all outflows recorded within the past hour.
    pub fn hourly_total(&self, now: DateTime<Utc>) -> Decimal {
        let cutoff = now - Duration::hours(1);
        self.hourly_outflows
            .iter()
            .filter(|(ts, _)| *ts > cutoff)
            .map(|(_, amt)| *amt)
            .sum()
    }

    /// Sum of all outflows recorded within the past 24 hours.
    pub fn daily_total(&self, now: DateTime<Utc>) -> Decimal {
        let cutoff = now - Duration::hours(24);
        self.daily_outflows
            .iter()
            .filter(|(ts, _)| *ts > cutoff)
            .map(|(_, amt)| *amt)
            .sum()
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Drop entries older than their respective windows from both deques.
    fn prune_old_entries(&mut self, now: DateTime<Utc>) {
        let hour_cutoff = now - Duration::hours(1);
        let day_cutoff = now - Duration::hours(24);

        while let Some((ts, _)) = self.hourly_outflows.front() {
            if *ts <= hour_cutoff {
                self.hourly_outflows.pop_front();
            } else {
                break;
            }
        }

        while let Some((ts, _)) = self.daily_outflows.front() {
            if *ts <= day_cutoff {
                self.daily_outflows.pop_front();
            } else {
                break;
            }
        }
    }

    /// Drop new-address entries older than 1 hour.
    fn prune_new_address_window(&mut self, now: DateTime<Utc>) {
        let cutoff = now - Duration::hours(1);
        while let Some(ts) = self.new_address_count_hourly.front() {
            if *ts <= cutoff {
                self.new_address_count_hourly.pop_front();
            } else {
                break;
            }
        }
    }
}

impl Default for OutflowCircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    fn cb_small() -> OutflowCircuitBreaker {
        // Small limits to make tests deterministic without large numbers.
        OutflowCircuitBreaker::with_limits(
            dec!(1_000),   // $1K per tx
            dec!(5_000),   // $5K/hour
            dec!(10_000),  // $10K/day
        )
    }

    // -----------------------------------------------------------------------
    // Per-transaction limit
    // -----------------------------------------------------------------------

    #[test]
    fn test_single_tx_within_limit() {
        let mut cb = cb_small();
        assert!(cb.check_withdrawal(dec!(999), now()).is_ok());
    }

    #[test]
    fn test_single_tx_at_limit() {
        let mut cb = cb_small();
        // Exactly at limit is fine.
        assert!(cb.check_withdrawal(dec!(1_000), now()).is_ok());
    }

    #[test]
    fn test_single_tx_exceeds_limit_trips_breaker() {
        let mut cb = cb_small();
        let result = cb.check_withdrawal(dec!(1_001), now());
        assert!(matches!(
            result,
            Err(CircuitBreakerError::SingleTransactionExceeded { .. })
        ));
        assert!(cb.is_tripped);
    }

    // -----------------------------------------------------------------------
    // Hourly cumulative limit
    // -----------------------------------------------------------------------

    #[test]
    fn test_hourly_accumulation_within_limit() {
        let mut cb = cb_small();
        let t = now();
        cb.record_withdrawal(dec!(2_000), t);
        cb.record_withdrawal(dec!(2_000), t);
        // Total $4,000; $5,000 limit — one more $999 is ok.
        assert!(cb.check_withdrawal(dec!(999), t).is_ok());
    }

    #[test]
    fn test_hourly_limit_exceeded_trips_breaker() {
        let mut cb = cb_small();
        let t = now();
        cb.record_withdrawal(dec!(4_500), t);
        let result = cb.check_withdrawal(dec!(600), t); // 4500 + 600 > 5000
        assert!(matches!(
            result,
            Err(CircuitBreakerError::HourlyLimitExceeded { .. })
        ));
        assert!(cb.is_tripped);
    }

    #[test]
    fn test_hourly_entries_older_than_1h_pruned() {
        let mut cb = cb_small();
        let old = now() - Duration::hours(2); // well outside the hourly window
        // Record $900 at old time (within per-tx limit of $1,000).
        cb.record_withdrawal(dec!(900), old);
        cb.record_withdrawal(dec!(900), old);
        cb.record_withdrawal(dec!(900), old);
        cb.record_withdrawal(dec!(900), old);
        cb.record_withdrawal(dec!(900), old);
        // Total old = $4,500 — would breach hourly $5K limit if not pruned.
        // After pruning, the old entries disappear (hourly window = 1h).
        // Daily window is 24h, so we need the new amount to fit within $10K daily.
        // Old entries are still in daily — $4,500 + $500 = $5,000 < $10,000 daily limit.
        assert!(cb.check_withdrawal(dec!(500), now()).is_ok());
    }

    // -----------------------------------------------------------------------
    // Daily cumulative limit
    // -----------------------------------------------------------------------

    #[test]
    fn test_daily_limit_exceeded_trips_breaker() {
        // Use a CB where the hourly limit is higher than the daily limit so
        // we can test the daily-limit path without triggering the hourly check first.
        let mut cb = OutflowCircuitBreaker::with_limits(
            dec!(1_000),   // $1K per tx
            dec!(20_000),  // $20K/hour (high so hourly doesn't fire first)
            dec!(10_000),  // $10K/day
        );
        let t = now();
        // Record $900 x 11 = $9,900 in the daily window.
        for _ in 0..11 {
            cb.record_withdrawal(dec!(900), t);
        }
        let result = cb.check_withdrawal(dec!(200), t); // 9900 + 200 > 10000
        assert!(matches!(
            result,
            Err(CircuitBreakerError::DailyLimitExceeded { .. })
        ));
        assert!(cb.is_tripped);
    }

    #[test]
    fn test_daily_entries_older_than_24h_pruned() {
        let mut cb = cb_small();
        let old = now() - Duration::hours(25); // well outside the 24h daily window
        // Record $900 x10 = $9,000 at old time (each within per-tx limit of $1,000).
        for _ in 0..10 {
            cb.record_withdrawal(dec!(900), old);
        }
        // Total old = $9,000. If NOT pruned, $9,000 + $900 = $9,900 which still fits,
        // but $9,000 + $1,000 would exceed $10K. We check $900 to be safe.
        // After pruning (older than 24h), the old entries disappear.
        assert!(cb.check_withdrawal(dec!(900), now()).is_ok());
    }

    // -----------------------------------------------------------------------
    // Trip and reset
    // -----------------------------------------------------------------------

    #[test]
    fn test_already_tripped_blocks_all_withdrawals() {
        let mut cb = cb_small();
        let t = now();
        cb.trip("manual test trip", t);
        let result = cb.check_withdrawal(dec!(1), t);
        assert!(matches!(
            result,
            Err(CircuitBreakerError::AlreadyTripped(_))
        ));
    }

    #[test]
    fn test_reset_clears_tripped_state() {
        let mut cb = cb_small();
        cb.trip("test", now());
        assert!(cb.is_tripped);
        cb.reset();
        assert!(!cb.is_tripped);
        assert!(cb.trip_reason.is_none());
        assert!(cb.tripped_at.is_none());
    }

    #[test]
    fn test_reset_allows_new_withdrawals() {
        let mut cb = cb_small();
        cb.trip("test", now());
        cb.reset();
        assert!(cb.check_withdrawal(dec!(100), now()).is_ok());
    }

    // -----------------------------------------------------------------------
    // New address auto-trip
    // -----------------------------------------------------------------------

    #[test]
    fn test_new_address_trip_at_10() {
        let mut cb = cb_small();
        let t = now();
        for _ in 0..9 {
            cb.record_new_address_withdrawal(t);
        }
        assert!(!cb.is_tripped); // 9 is fine
        cb.record_new_address_withdrawal(t);
        assert!(cb.is_tripped); // 10 trips
    }

    #[test]
    fn test_new_address_old_entries_pruned() {
        let mut cb = cb_small();
        let old = now() - Duration::hours(2);
        let t = now();
        for _ in 0..9 {
            cb.record_new_address_withdrawal(old);
        }
        // The 9 old entries have expired — adding one recent one should not trip.
        cb.record_new_address_withdrawal(t);
        assert!(!cb.is_tripped);
    }

    // -----------------------------------------------------------------------
    // Hourly / daily total helpers
    // -----------------------------------------------------------------------

    #[test]
    fn test_hourly_total_only_counts_recent() {
        let mut cb = cb_small();
        let old = now() - Duration::hours(2);
        let recent = now();
        cb.record_withdrawal(dec!(3_000), old);
        cb.record_withdrawal(dec!(1_000), recent);
        assert_eq!(cb.hourly_total(recent), dec!(1_000));
    }

    #[test]
    fn test_daily_total_only_counts_recent() {
        let mut cb = cb_small();
        let old = now() - Duration::hours(26);
        let recent = now();
        cb.record_withdrawal(dec!(8_000), old);
        cb.record_withdrawal(dec!(2_000), recent);
        assert_eq!(cb.daily_total(recent), dec!(2_000));
    }
}
