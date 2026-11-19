//! PnL (profit and loss) calculation for perpetual futures.
//!
//! All arithmetic uses `Decimal` — never `f64` for money values.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use exchange_common::PositionSide;

use crate::error::SettlementError;

/// Realized PnL when closing (or partially closing) a position.
///
/// Long:  pnl = (exit_price - entry_price) * qty_closed
/// Short: pnl = (entry_price - exit_price) * qty_closed
pub fn calculate_realized_pnl(
    side: PositionSide,
    qty_closed: Decimal,
    entry_price: Decimal,
    exit_price: Decimal,
) -> Result<Decimal, SettlementError> {
    if qty_closed < Decimal::ZERO {
        return Err(SettlementError::NegativeQuantity(qty_closed));
    }
    if entry_price <= Decimal::ZERO {
        return Err(SettlementError::NonPositivePrice(entry_price));
    }
    if exit_price <= Decimal::ZERO {
        return Err(SettlementError::NonPositivePrice(exit_price));
    }

    let pnl = side.sign() * qty_closed * (exit_price - entry_price);
    Ok(pnl)
}

/// Return on equity as a fraction.
///
/// roe = pnl / margin
///
/// Returns an error if `margin` is zero.
pub fn calculate_roe(pnl: Decimal, margin: Decimal) -> Result<Decimal, SettlementError> {
    if margin.is_zero() {
        return Err(SettlementError::ZeroMargin);
    }
    Ok(pnl / margin)
}

/// Effective leverage = notional / equity.
///
/// Returns an error if `equity` is zero or negative.
pub fn calculate_effective_leverage(
    notional: Decimal,
    equity: Decimal,
) -> Result<Decimal, SettlementError> {
    if equity <= Decimal::ZERO {
        return Err(SettlementError::NonPositiveEquity(equity));
    }
    Ok(notional / equity)
}

/// Summary of a closed (or partially closed) trade.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PnlSummary {
    pub side: PositionSide,
    pub qty_closed: Decimal,
    pub entry_price: Decimal,
    pub exit_price: Decimal,
    pub realized_pnl: Decimal,
    /// PnL as a fraction of the margin used (e.g., 0.05 = 5%).
    pub roe: Decimal,
}

impl PnlSummary {
    pub fn compute(
        side: PositionSide,
        qty_closed: Decimal,
        entry_price: Decimal,
        exit_price: Decimal,
        margin: Decimal,
    ) -> Result<Self, SettlementError> {
        let realized_pnl = calculate_realized_pnl(side, qty_closed, entry_price, exit_price)?;
        let roe = calculate_roe(realized_pnl, margin)?;
        Ok(Self {
            side,
            qty_closed,
            entry_price,
            exit_price,
            realized_pnl,
            roe,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    // --- calculate_realized_pnl ---

    #[test]
    fn test_long_profit() {
        // Long 1 BTC, entry 50000, exit 52000 → +2000
        let pnl =
            calculate_realized_pnl(PositionSide::Long, dec!(1), dec!(50000), dec!(52000)).unwrap();
        assert_eq!(pnl, dec!(2000));
    }

    #[test]
    fn test_long_loss() {
        // Long 1 BTC, entry 50000, exit 48000 → -2000
        let pnl =
            calculate_realized_pnl(PositionSide::Long, dec!(1), dec!(50000), dec!(48000)).unwrap();
        assert_eq!(pnl, dec!(-2000));
    }

    #[test]
    fn test_short_profit() {
        // Short 1 BTC, entry 50000, exit 48000 → +2000
        let pnl =
            calculate_realized_pnl(PositionSide::Short, dec!(1), dec!(50000), dec!(48000)).unwrap();
        assert_eq!(pnl, dec!(2000));
    }

    #[test]
    fn test_short_loss() {
        // Short 1 BTC, entry 50000, exit 52000 → -2000
        let pnl =
            calculate_realized_pnl(PositionSide::Short, dec!(1), dec!(50000), dec!(52000)).unwrap();
        assert_eq!(pnl, dec!(-2000));
    }

    #[test]
    fn test_partial_close() {
        // Close 0.5 of a long, entry 50000, exit 51000 → 0.5 * 1000 = 500
        let pnl =
            calculate_realized_pnl(PositionSide::Long, dec!(0.5), dec!(50000), dec!(51000))
                .unwrap();
        assert_eq!(pnl, dec!(500));
    }

    #[test]
    fn test_zero_qty_returns_zero() {
        let pnl =
            calculate_realized_pnl(PositionSide::Long, Decimal::ZERO, dec!(50000), dec!(52000))
                .unwrap();
        assert_eq!(pnl, Decimal::ZERO);
    }

    #[test]
    fn test_negative_qty_errors() {
        let result =
            calculate_realized_pnl(PositionSide::Long, dec!(-1), dec!(50000), dec!(52000));
        assert!(matches!(result, Err(SettlementError::NegativeQuantity(_))));
    }

    #[test]
    fn test_zero_entry_price_errors() {
        let result =
            calculate_realized_pnl(PositionSide::Long, dec!(1), Decimal::ZERO, dec!(52000));
        assert!(matches!(result, Err(SettlementError::NonPositivePrice(_))));
    }

    #[test]
    fn test_zero_exit_price_errors() {
        let result =
            calculate_realized_pnl(PositionSide::Long, dec!(1), dec!(50000), Decimal::ZERO);
        assert!(matches!(result, Err(SettlementError::NonPositivePrice(_))));
    }

    // --- calculate_roe ---

    #[test]
    fn test_roe_basic() {
        // PnL = 500, margin = 5000 → ROE = 10%
        let roe = calculate_roe(dec!(500), dec!(5000)).unwrap();
        assert_eq!(roe, dec!(0.1));
    }

    #[test]
    fn test_roe_negative_pnl() {
        let roe = calculate_roe(dec!(-500), dec!(5000)).unwrap();
        assert_eq!(roe, dec!(-0.1));
    }

    #[test]
    fn test_roe_zero_margin_errors() {
        let result = calculate_roe(dec!(100), Decimal::ZERO);
        assert!(matches!(result, Err(SettlementError::ZeroMargin)));
    }

    // --- calculate_effective_leverage ---

    #[test]
    fn test_effective_leverage() {
        // notional = 50000, equity = 5000 → 10x
        let lev = calculate_effective_leverage(dec!(50000), dec!(5000)).unwrap();
        assert_eq!(lev, dec!(10));
    }

    #[test]
    fn test_effective_leverage_zero_equity_errors() {
        let result = calculate_effective_leverage(dec!(50000), Decimal::ZERO);
        assert!(matches!(result, Err(SettlementError::NonPositiveEquity(_))));
    }

    #[test]
    fn test_effective_leverage_negative_equity_errors() {
        let result = calculate_effective_leverage(dec!(50000), dec!(-100));
        assert!(matches!(result, Err(SettlementError::NonPositiveEquity(_))));
    }

    // --- PnlSummary ---

    #[test]
    fn test_pnl_summary_long_win() {
        let summary = PnlSummary::compute(
            PositionSide::Long,
            dec!(1),
            dec!(50000),
            dec!(55000),
            dec!(5000),
        )
        .unwrap();

        assert_eq!(summary.realized_pnl, dec!(5000));
        assert_eq!(summary.roe, dec!(1)); // 100% return on margin
    }
}
