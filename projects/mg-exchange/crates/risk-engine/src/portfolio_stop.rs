//! Portfolio-level margin stop — prevents a user from opening new positions
//! when their total margin usage across all symbols exceeds available balance.
//!
//! # Purpose
//!
//! A user with 5 near-full cross-margin positions should not be able to open
//! a 6th that would exceed their total balance. The per-symbol margin check
//! passes (each position individually has sufficient margin) but the aggregate
//! exceeds capacity.
//!
//! # Reference
//!
//! Standard cross-margin risk control. Bybit and Binance both check aggregate
//! margin before allowing new positions under cross-margin mode.

use rust_decimal::Decimal;

/// Check whether opening a new position would cause the user's total margin
/// to exceed their available balance.
///
/// # Arguments
/// * `existing_total_margin` — sum of initial margin across all user's existing positions
/// * `new_position_margin` — initial margin required for the new position
/// * `available_balance` — user's total available balance (after locked amounts)
///
/// # Returns
/// `Ok(())` if the portfolio can absorb the new position,
/// `Err(PortfolioMarginError)` if it would exceed capacity.
pub fn check_portfolio_margin(
    existing_total_margin: Decimal,
    new_position_margin: Decimal,
    available_balance: Decimal,
) -> Result<(), PortfolioMarginError> {
    let total_after = existing_total_margin + new_position_margin;

    if total_after > available_balance {
        return Err(PortfolioMarginError {
            existing_margin: existing_total_margin,
            new_margin: new_position_margin,
            total_margin: total_after,
            available_balance,
            deficit: total_after - available_balance,
        });
    }

    Ok(())
}

/// Error when a new position would push total margin beyond available balance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortfolioMarginError {
    pub existing_margin: Decimal,
    pub new_margin: Decimal,
    pub total_margin: Decimal,
    pub available_balance: Decimal,
    pub deficit: Decimal,
}

impl std::fmt::Display for PortfolioMarginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Portfolio margin insufficient: total margin {} (existing {} + new {}) exceeds available balance {} by {}",
            self.total_margin, self.existing_margin, self.new_margin,
            self.available_balance, self.deficit,
        )
    }
}

impl std::error::Error for PortfolioMarginError {}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_within_portfolio_margin() {
        // Existing $3000 margin + $1000 new = $4000 < $5000 available.
        assert!(check_portfolio_margin(dec!(3000), dec!(1000), dec!(5000)).is_ok());
    }

    #[test]
    fn test_exactly_at_limit() {
        // $3000 + $2000 = $5000 == $5000 available.
        assert!(check_portfolio_margin(dec!(3000), dec!(2000), dec!(5000)).is_ok());
    }

    #[test]
    fn test_exceeds_portfolio_margin() {
        // $3000 + $2001 = $5001 > $5000.
        let err = check_portfolio_margin(dec!(3000), dec!(2001), dec!(5000)).unwrap_err();
        assert_eq!(err.deficit, dec!(1));
        assert_eq!(err.total_margin, dec!(5001));
    }

    #[test]
    fn test_no_existing_positions() {
        // First position: $1000 margin, $10000 balance → fine.
        assert!(check_portfolio_margin(dec!(0), dec!(1000), dec!(10000)).is_ok());
    }

    #[test]
    fn test_five_positions_then_sixth_blocked() {
        // 5 positions at $1800 each = $9000. Balance = $10000. 6th at $1800 = $10800 > $10000.
        let err = check_portfolio_margin(dec!(9000), dec!(1800), dec!(10000)).unwrap_err();
        assert_eq!(err.deficit, dec!(800));
    }

    #[test]
    fn test_error_display() {
        let err = check_portfolio_margin(dec!(4000), dec!(2000), dec!(5000)).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Portfolio margin insufficient"));
        assert!(msg.contains("6000")); // total
        assert!(msg.contains("1000")); // deficit
    }

    // ── M18-8: Adversarial tests ──────────────────────────────────────────

    #[test]
    fn adversarial_zero_balance_blocks_any_order() {
        let err = check_portfolio_margin(dec!(0), dec!(100), dec!(0)).unwrap_err();
        assert_eq!(err.deficit, dec!(100));
    }

    #[test]
    fn adversarial_exact_balance_allows_order() {
        // Exactly at the limit — should succeed.
        assert!(check_portfolio_margin(dec!(9000), dec!(1000), dec!(10000)).is_ok());
    }

    #[test]
    fn adversarial_one_dollar_over_blocks() {
        let err = check_portfolio_margin(dec!(9000), dec!(1001), dec!(10000)).unwrap_err();
        assert_eq!(err.deficit, dec!(1));
    }
}
