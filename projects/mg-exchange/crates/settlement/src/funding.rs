//! Funding rate calculation and settlement for perpetual futures.
//!
//! Funding is exchanged between longs and shorts every interval (typically 8h).
//! Positive rate → longs pay shorts. Negative rate → shorts pay longs.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tracing::debug;

use exchange_common::{Position, PositionSide, Symbol, UserId};

use crate::error::SettlementError;

/// Calculate the funding rate for one interval.
///
/// rate = clamp((perp_price - index_price) / index_price, -max_rate, max_rate)
///
/// Returns an error if `index_price` is zero (division by zero guard).
pub fn calculate_funding_rate(
    perp_price: Decimal,
    index_price: Decimal,
    max_rate: Decimal,
) -> Result<Decimal, SettlementError> {
    if index_price.is_zero() {
        return Err(SettlementError::ZeroIndexPrice);
    }

    let raw = (perp_price - index_price) / index_price;
    let rate = raw.clamp(-max_rate, max_rate);

    debug!(
        perp_price = %perp_price,
        index_price = %index_price,
        raw_rate = %raw,
        clamped_rate = %rate,
        "calculated funding rate"
    );

    Ok(rate)
}

/// A single funding payment for one position in one settlement round.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundingPayment {
    pub user_id: UserId,
    pub symbol: Symbol,
    /// Signed amount in the settlement asset.
    ///
    /// For **linear** contracts the amount is in the quote asset (USDT).
    /// For **inverse** contracts the amount is in the base asset (BTC, ETH).
    ///
    /// Negative → user pays (outflow). Positive → user receives (inflow).
    pub amount: Decimal,
    /// The funding rate applied.
    pub rate: Decimal,
    /// Mark price used for the notional calculation.
    pub mark_price: Decimal,
    /// Settlement asset coin (e.g. "USDT" for linear, "BTC" for BTCUSD inverse).
    #[serde(default = "default_settle_asset")]
    pub settle_asset: String,
    pub settled_at: DateTime<Utc>,
}

fn default_settle_asset() -> String {
    "USDT".to_string()
}

/// Settles funding across a set of open positions.
pub struct FundingSettlement;

impl FundingSettlement {
    /// Compute funding payments for every position in the slice.
    ///
    /// **Linear** settlement:
    ///   `payment = size * mark_price * funding_rate` (in quote asset, e.g. USDT)
    ///
    /// **Inverse** settlement:
    ///   `payment = size * contract_size * funding_rate / mark_price` (in base asset, e.g. BTC)
    ///
    /// Direction (same for both types):
    /// - Rate > 0: longs pay (negative amount), shorts receive (positive amount).
    /// - Rate < 0: shorts pay (negative amount), longs receive (positive amount).
    /// - Rate = 0: no payment (zero amount, still recorded for audit trail).
    pub fn settle(
        positions: &[Position],
        funding_rate: Decimal,
        mark_price: Decimal,
        now: DateTime<Utc>,
    ) -> Vec<FundingPayment> {
        Self::settle_with_config(
            positions,
            funding_rate,
            mark_price,
            now,
            false,
            Decimal::ONE,
            "USDT",
        )
    }

    /// Extended funding settlement with explicit inverse/contract-size/asset parameters.
    ///
    /// Use this overload when settling inverse contracts.
    pub fn settle_with_config(
        positions: &[Position],
        funding_rate: Decimal,
        mark_price: Decimal,
        now: DateTime<Utc>,
        is_inverse: bool,
        contract_size: Decimal,
        settle_asset: &str,
    ) -> Vec<FundingPayment> {
        positions
            .iter()
            .map(|pos| {
                let raw = if is_inverse {
                    // Inverse: payment in base asset.
                    // notional_base = size * contract_size / mark_price
                    // payment = notional_base * funding_rate
                    if mark_price.is_zero() {
                        Decimal::ZERO
                    } else {
                        pos.size * contract_size / mark_price * funding_rate
                    }
                } else {
                    // Linear: payment in quote asset.
                    let notional = pos.size * mark_price;
                    notional * funding_rate
                };

                // Long pays when rate > 0, receives when rate < 0.
                // Short receives when rate > 0, pays when rate < 0.
                let amount = match pos.side {
                    PositionSide::Long => -raw,
                    PositionSide::Short => raw,
                };

                FundingPayment {
                    user_id: pos.user_id,
                    symbol: pos.symbol.clone(),
                    amount,
                    rate: funding_rate,
                    mark_price,
                    settle_asset: settle_asset.to_string(),
                    settled_at: now,
                }
            })
            .collect()
    }
}

/// Historical record of a funding rate for a given symbol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundingRateRecord {
    pub symbol: Symbol,
    pub rate: Decimal,
    pub mark_price: Decimal,
    pub index_price: Decimal,
    pub calculated_at: DateTime<Utc>,
}

/// Stores past funding rates and payments for a single symbol.
#[derive(Debug, Default)]
pub struct FundingHistory {
    pub rates: Vec<FundingRateRecord>,
    pub payments: Vec<FundingPayment>,
}

impl FundingHistory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a funding rate record.
    pub fn record_rate(&mut self, record: FundingRateRecord) {
        self.rates.push(record);
    }

    /// Append funding payments from a settlement round.
    pub fn record_payments(&mut self, payments: Vec<FundingPayment>) {
        self.payments.extend(payments);
    }

    /// Return the most recently recorded funding rate, if any.
    pub fn latest_rate(&self) -> Option<&FundingRateRecord> {
        self.rates.last()
    }

    /// Sum of all payments received/paid by a specific user.
    pub fn user_net(&self, user_id: UserId) -> Decimal {
        self.payments
            .iter()
            .filter(|p| p.user_id == user_id)
            .map(|p| p.amount)
            .sum()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Premium buffer (TWAP-based funding rate)
// ─────────────────────────────────────────────────────────────────────────────

/// Circular buffer of premium index samples for TWAP-based funding rate calc.
///
/// Bybit samples the premium index every minute over an 8-hour window (480
/// samples).  The TWAP of these samples replaces the instantaneous spread used
/// in the simple formula, preventing single-tick manipulation.
///
/// Reference: Bybit perpetual swap funding rate documentation, "Premium Index".
#[derive(Debug, Clone)]
pub struct PremiumBuffer {
    samples: Vec<Decimal>,
    cursor: usize,
    max_samples: usize,
}

impl Default for PremiumBuffer {
    fn default() -> Self {
        Self::new(480)
    }
}

impl PremiumBuffer {
    /// Create a new buffer.  `max_samples` = 480 for a standard 8h / 1-min window.
    pub fn new(max_samples: usize) -> Self {
        Self {
            samples: Vec::with_capacity(max_samples),
            cursor: 0,
            max_samples,
        }
    }

    /// Record a new premium index sample computed from raw prices.
    ///
    /// Computes `premium = (perp_price - index_price) / index_price` and appends it.
    /// Returns an error if `index_price` is zero (division by zero guard).
    pub fn record_sample(
        &mut self,
        perp_price: Decimal,
        index_price: Decimal,
    ) -> Result<(), crate::error::SettlementError> {
        if index_price.is_zero() {
            return Err(crate::error::SettlementError::ZeroIndexPrice);
        }
        let sample = (perp_price - index_price) / index_price;
        if self.samples.len() < self.max_samples {
            self.samples.push(sample);
        } else {
            self.samples[self.cursor] = sample;
            self.cursor = (self.cursor + 1) % self.max_samples;
        }
        Ok(())
    }

    /// Arithmetic mean of all buffered samples.  Returns `None` if empty.
    pub fn twap_premium(&self) -> Option<Decimal> {
        if self.samples.is_empty() {
            return None;
        }
        let sum: Decimal = self.samples.iter().sum();
        Some(sum / Decimal::from(self.samples.len() as u64))
    }

    /// Number of samples currently buffered.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Returns `true` if no samples have been recorded yet.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

/// Calculate the funding rate using a TWAP premium.
///
/// Follows the Bybit formula:
/// ```text
/// funding_rate = clamp(twap_premium + interest_rate, -max_rate, max_rate)
/// ```
/// where `interest_rate` is typically 0.01% per 8h interval (hardcoded to 0
/// here for simplicity unless the caller provides it in `twap_premium`).
///
/// If `twap_premium` is `None` (buffer empty), falls back to the instantaneous
/// `calculate_funding_rate` using `mark_price` and `index_price`.
///
/// Returns an error if `index_price` is zero and the buffer is also empty.
pub fn calculate_funding_rate_twap(
    mark_price: Decimal,
    index_price: Decimal,
    max_rate: Decimal,
    twap_premium: Option<Decimal>,
) -> Result<Decimal, crate::error::SettlementError> {
    let raw = if let Some(twap) = twap_premium {
        twap
    } else {
        // Fall back to instantaneous premium.
        calculate_funding_rate(mark_price, index_price, max_rate)?;
        if index_price.is_zero() {
            return Err(crate::error::SettlementError::ZeroIndexPrice);
        }
        (mark_price - index_price) / index_price
    };
    Ok(raw.clamp(-max_rate, max_rate))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use exchange_common::{MarginMode, PositionSide};
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn make_position(side: PositionSide, size: Decimal) -> Position {
        Position {
            user_id: Uuid::new_v4(),
            symbol: Symbol::new("BTCUSDT"),
            side,
            size,
            entry_price: dec!(50000),
            leverage: dec!(10),
            margin_mode: MarginMode::Isolated,
            margin: dec!(5000),
            realized_pnl: Decimal::ZERO,
            funding_accumulated: Decimal::ZERO,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            position_idx: 0,
            tp_price: None,
            sl_price: None,
            trailing_stop: None,
            tp_trigger_id: None,
            sl_trigger_id: None,
            tp_size: None,
            sl_size: None,
            auto_add_margin: false,
            unpaid_funding_debt: Decimal::ZERO,
        }
    }

    // --- calculate_funding_rate ---

    #[test]
    fn test_rate_positive_premium() {
        // perp trades above index → positive rate (longs pay)
        let rate = calculate_funding_rate(dec!(50500), dec!(50000), dec!(0.01)).unwrap();
        assert_eq!(rate, dec!(0.01)); // (50500-50000)/50000 = 0.01, at max
    }

    #[test]
    fn test_rate_negative_discount() {
        // perp trades below index → negative rate (shorts pay)
        let rate = calculate_funding_rate(dec!(49500), dec!(50000), dec!(0.01)).unwrap();
        assert_eq!(rate, dec!(-0.01)); // -0.01, at max negative
    }

    #[test]
    fn test_rate_zero_when_prices_equal() {
        let rate = calculate_funding_rate(dec!(50000), dec!(50000), dec!(0.01)).unwrap();
        assert_eq!(rate, Decimal::ZERO);
    }

    #[test]
    fn test_rate_clamped_at_max() {
        // Extreme deviation: would be 2% without cap, max is 1%
        let rate = calculate_funding_rate(dec!(51000), dec!(50000), dec!(0.01)).unwrap();
        assert_eq!(rate, dec!(0.01));
    }

    #[test]
    fn test_rate_clamped_at_min() {
        let rate = calculate_funding_rate(dec!(49000), dec!(50000), dec!(0.01)).unwrap();
        assert_eq!(rate, dec!(-0.01));
    }

    #[test]
    fn test_rate_unclamped_small_deviation() {
        // 0.2% deviation, within ±1% max
        let rate = calculate_funding_rate(dec!(50100), dec!(50000), dec!(0.01)).unwrap();
        assert_eq!(rate, dec!(0.002));
    }

    #[test]
    fn test_rate_zero_index_price_errors() {
        let result = calculate_funding_rate(dec!(50000), Decimal::ZERO, dec!(0.01));
        assert!(matches!(result, Err(SettlementError::ZeroIndexPrice)));
    }

    // --- FundingSettlement::settle ---

    #[test]
    fn test_settle_long_pays_positive_rate() {
        // Rate > 0 → long pays (negative amount)
        let long = make_position(PositionSide::Long, dec!(1));
        let payments = FundingSettlement::settle(&[long], dec!(0.001), dec!(50000), Utc::now());
        assert_eq!(payments.len(), 1);
        // payment = 1 * 50000 * 0.001 = 50 USDT paid → -50
        assert_eq!(payments[0].amount, dec!(-50));
    }

    #[test]
    fn test_settle_short_receives_positive_rate() {
        // Rate > 0 → short receives (positive amount)
        let short = make_position(PositionSide::Short, dec!(1));
        let payments = FundingSettlement::settle(&[short], dec!(0.001), dec!(50000), Utc::now());
        assert_eq!(payments[0].amount, dec!(50));
    }

    #[test]
    fn test_settle_long_receives_negative_rate() {
        // Rate < 0 → long receives
        let long = make_position(PositionSide::Long, dec!(1));
        let payments = FundingSettlement::settle(&[long], dec!(-0.001), dec!(50000), Utc::now());
        assert_eq!(payments[0].amount, dec!(50));
    }

    #[test]
    fn test_settle_short_pays_negative_rate() {
        // Rate < 0 → short pays
        let short = make_position(PositionSide::Short, dec!(1));
        let payments = FundingSettlement::settle(&[short], dec!(-0.001), dec!(50000), Utc::now());
        assert_eq!(payments[0].amount, dec!(-50));
    }

    #[test]
    fn test_settle_zero_rate_no_payment() {
        let long = make_position(PositionSide::Long, dec!(2));
        let payments = FundingSettlement::settle(&[long], Decimal::ZERO, dec!(50000), Utc::now());
        assert_eq!(payments[0].amount, Decimal::ZERO);
    }

    #[test]
    fn test_settle_multiple_positions() {
        let long = make_position(PositionSide::Long, dec!(1));
        let short = make_position(PositionSide::Short, dec!(1));
        let rate = dec!(0.001);
        let mark = dec!(50000);
        let payments = FundingSettlement::settle(&[long, short], rate, mark, Utc::now());
        assert_eq!(payments.len(), 2);
        // Net transfer: long pays 50, short receives 50 → net = 0
        let net: Decimal = payments.iter().map(|p| p.amount).sum();
        assert_eq!(net, Decimal::ZERO);
    }

    #[test]
    fn test_settle_empty_positions() {
        let payments = FundingSettlement::settle(&[], dec!(0.001), dec!(50000), Utc::now());
        assert!(payments.is_empty());
    }

    // --- FundingHistory ---

    #[test]
    fn test_history_user_net() {
        let user_a = Uuid::new_v4();
        let user_b = Uuid::new_v4();

        let mut history = FundingHistory::new();
        history.record_payments(vec![
            FundingPayment {
                user_id: user_a,
                symbol: Symbol::new("BTCUSDT"),
                amount: dec!(-50),
                rate: dec!(0.001),
                mark_price: dec!(50000),
                settle_asset: "USDT".to_string(),
                settled_at: Utc::now(),
            },
            FundingPayment {
                user_id: user_b,
                symbol: Symbol::new("BTCUSDT"),
                amount: dec!(50),
                rate: dec!(0.001),
                mark_price: dec!(50000),
                settle_asset: "USDT".to_string(),
                settled_at: Utc::now(),
            },
            FundingPayment {
                user_id: user_a,
                symbol: Symbol::new("BTCUSDT"),
                amount: dec!(20),
                rate: dec!(-0.001),
                mark_price: dec!(50000),
                settle_asset: "USDT".to_string(),
                settled_at: Utc::now(),
            },
        ]);

        assert_eq!(history.user_net(user_a), dec!(-30));
        assert_eq!(history.user_net(user_b), dec!(50));
    }
}
