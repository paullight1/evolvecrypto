//! Profit-sharing settlement between followers and master traders.
//!
//! When a follower closes a profitable copy trade, the master is entitled to a
//! performance fee (`profit_share_rate`) of the gross profit.  Losses are never
//! shared — the follower absorbs them in full.
//!
//! # Example
//! ```
//! # use rust_decimal_macros::dec;
//! # use exchange_copy_trading::settlement::calculate_profit_share;
//! let (master_share, follower_net) = calculate_profit_share(dec!(100), dec!(0.10)).unwrap();
//! assert_eq!(master_share, dec!(10));
//! assert_eq!(follower_net, dec!(90));
//! ```

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::Zero;
use tracing::debug;
use uuid::Uuid;

use exchange_common::types::UserId;

use crate::error::CopyTradingError;

// ──────────────────────────────────────────────────────────────────────────────
// Types
// ──────────────────────────────────────────────────────────────────────────────

/// A completed profit-sharing event for a single follow relation.
#[derive(Debug, Clone)]
pub struct ProfitSettlement {
    pub relation_id: Uuid,
    pub follower_id: UserId,
    pub master_id: UserId,
    /// Total profit before the performance fee is deducted.
    pub gross_profit: Decimal,
    /// Amount transferred to the master (performance fee).
    pub share_amount: Decimal,
    /// Amount retained by the follower after the fee.
    pub net_profit: Decimal,
    pub settled_at: DateTime<Utc>,
}

// ──────────────────────────────────────────────────────────────────────────────
// Pure calculation
// ──────────────────────────────────────────────────────────────────────────────

/// Split `profit` between master and follower according to `share_rate`.
///
/// * If `profit` ≤ 0, returns `(0, profit)` — losses are not shared.
/// * `share_rate` must be in \[0, 1\]; returns an error otherwise.
///
/// Returns `(master_share, follower_net)`.
pub fn calculate_profit_share(
    profit: Decimal,
    share_rate: Decimal,
) -> Result<(Decimal, Decimal), CopyTradingError> {
    if share_rate < Decimal::zero() || share_rate > Decimal::ONE {
        return Err(CopyTradingError::InvalidShareRate(share_rate.to_string()));
    }

    if profit <= Decimal::zero() {
        debug!(%profit, "no profit to share");
        return Ok((Decimal::zero(), profit));
    }

    let master_share = profit * share_rate;
    let follower_net = profit - master_share;
    Ok((master_share, follower_net))
}

/// Build a [`ProfitSettlement`] record for the given relation and profit.
///
/// Delegates to [`calculate_profit_share`] for the arithmetic.
pub fn settle(
    relation_id: Uuid,
    follower_id: UserId,
    master_id: UserId,
    gross_profit: Decimal,
    share_rate: Decimal,
) -> Result<ProfitSettlement, CopyTradingError> {
    let (share_amount, net_profit) = calculate_profit_share(gross_profit, share_rate)?;

    Ok(ProfitSettlement {
        relation_id,
        follower_id,
        master_id,
        gross_profit,
        share_amount,
        net_profit,
        settled_at: Utc::now(),
    })
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    use super::*;

    fn uid() -> UserId {
        Uuid::new_v4()
    }

    // ── calculate_profit_share ────────────────────────────────────────────────

    #[test]
    fn profit_split_at_ten_percent() {
        let (master, follower) = calculate_profit_share(dec!(100), dec!(0.10)).unwrap();
        assert_eq!(master, dec!(10));
        assert_eq!(follower, dec!(90));
    }

    #[test]
    fn profit_split_at_zero_rate_all_to_follower() {
        let (master, follower) = calculate_profit_share(dec!(200), dec!(0)).unwrap();
        assert_eq!(master, dec!(0));
        assert_eq!(follower, dec!(200));
    }

    #[test]
    fn profit_split_at_full_rate_all_to_master() {
        let (master, follower) = calculate_profit_share(dec!(150), dec!(1)).unwrap();
        assert_eq!(master, dec!(150));
        assert_eq!(follower, dec!(0));
    }

    #[test]
    fn no_share_on_zero_profit() {
        let (master, follower) = calculate_profit_share(dec!(0), dec!(0.20)).unwrap();
        assert_eq!(master, dec!(0));
        assert_eq!(follower, dec!(0));
    }

    #[test]
    fn no_share_on_loss() {
        let (master, follower) = calculate_profit_share(dec!(-75), dec!(0.15)).unwrap();
        assert_eq!(master, dec!(0));
        assert_eq!(follower, dec!(-75)); // follower absorbs the full loss
    }

    #[test]
    fn rate_above_one_returns_error() {
        let err = calculate_profit_share(dec!(100), dec!(1.01)).unwrap_err();
        assert!(matches!(err, CopyTradingError::InvalidShareRate(_)));
    }

    #[test]
    fn rate_negative_returns_error() {
        let err = calculate_profit_share(dec!(100), dec!(-0.01)).unwrap_err();
        assert!(matches!(err, CopyTradingError::InvalidShareRate(_)));
    }

    #[test]
    fn profit_split_sums_to_gross() {
        let gross = dec!(333.33);
        let rate = dec!(0.17);
        let (master, follower) = calculate_profit_share(gross, rate).unwrap();
        assert_eq!(master + follower, gross);
    }

    // ── settle ────────────────────────────────────────────────────────────────

    #[test]
    fn settle_builds_correct_record() {
        let rid = Uuid::new_v4();
        let f = uid();
        let m = uid();

        let settlement = settle(rid, f, m, dec!(500), dec!(0.20)).unwrap();
        assert_eq!(settlement.relation_id, rid);
        assert_eq!(settlement.follower_id, f);
        assert_eq!(settlement.master_id, m);
        assert_eq!(settlement.gross_profit, dec!(500));
        assert_eq!(settlement.share_amount, dec!(100));
        assert_eq!(settlement.net_profit, dec!(400));
    }

    #[test]
    fn settle_on_loss_gives_zero_share() {
        let rid = Uuid::new_v4();
        let settlement = settle(rid, uid(), uid(), dec!(-200), dec!(0.10)).unwrap();
        assert_eq!(settlement.share_amount, dec!(0));
        assert_eq!(settlement.net_profit, dec!(-200));
    }

    #[test]
    fn settle_invalid_rate_propagates_error() {
        let err = settle(Uuid::new_v4(), uid(), uid(), dec!(100), dec!(2)).unwrap_err();
        assert!(matches!(err, CopyTradingError::InvalidShareRate(_)));
    }
}
