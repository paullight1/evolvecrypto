//! Position concentration limits — prevents any single user from holding
//! an outsized share of open interest in a symbol.
//!
//! # Purpose
//!
//! A user holding >20% of OI on a symbol creates systemic risk:
//! - Liquidation of that position would cascade through the order book.
//! - The user can manipulate funding rates.
//! - Insurance fund may be insufficient to cover the socialized loss.
//!
//! # Reference
//!
//! - OKEx 2018: single whale held $416M position, liquidation caused $9M clawback.
//! - Hunt Brothers 1980: cornered >50% of silver market, $1.7B loss on collapse.
//! - Bybit: imposes per-tier maximum position sizes.
//! - Default limit: 20% of total open interest per user per symbol.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Configuration for position concentration limits.
#[derive(Debug, Clone)]
pub struct ConcentrationLimitConfig {
    /// Maximum fraction of total open interest a single user may hold.
    /// Default: 0.20 (20%).
    pub max_pct_of_oi: Decimal,
}

impl Default for ConcentrationLimitConfig {
    fn default() -> Self {
        Self {
            max_pct_of_oi: dec!(0.20),
        }
    }
}

/// Error when a position would exceed the concentration limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConcentrationLimitExceeded {
    /// The user's current position size (absolute).
    pub current_position: Decimal,
    /// The size of the order being placed.
    pub order_size: Decimal,
    /// The resulting position if the order fills fully.
    pub resulting_position: Decimal,
    /// Total open interest for the symbol.
    pub total_open_interest: Decimal,
    /// Resulting fraction of OI.
    pub resulting_pct: Decimal,
    /// The configured maximum fraction.
    pub max_pct: Decimal,
}

impl std::fmt::Display for ConcentrationLimitExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Position concentration limit exceeded: resulting position would be {:.2}% of OI (max {:.2}%)",
            self.resulting_pct * dec!(100),
            self.max_pct * dec!(100),
        )
    }
}

impl std::error::Error for ConcentrationLimitExceeded {}

/// Check if a new order would cause the user's position to exceed the
/// concentration limit for the symbol.
///
/// # Arguments
/// * `current_position` — user's current absolute position size in the symbol
/// * `order_size` — the size of the new order (always positive)
/// * `total_open_interest` — total OI across all users for this symbol
/// * `config` — concentration limit configuration
///
/// # Returns
/// `Ok(())` if within limits, `Err(ConcentrationLimitExceeded)` otherwise.
///
/// # Note
/// For reduce-only orders or orders that decrease position size, this check
/// is not needed — it only applies to position-increasing orders.
pub fn check_concentration(
    current_position: Decimal,
    order_size: Decimal,
    total_open_interest: Decimal,
    config: &ConcentrationLimitConfig,
) -> Result<(), ConcentrationLimitExceeded> {
    // If total OI is zero (no positions in the market), allow the first position.
    if total_open_interest.is_zero() {
        return Ok(());
    }

    let resulting_position = current_position + order_size;
    let resulting_pct = resulting_position / total_open_interest;

    if resulting_pct > config.max_pct_of_oi {
        return Err(ConcentrationLimitExceeded {
            current_position,
            order_size,
            resulting_position,
            total_open_interest,
            resulting_pct,
            max_pct: config.max_pct_of_oi,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> ConcentrationLimitConfig {
        ConcentrationLimitConfig::default() // 20%
    }

    #[test]
    fn test_within_limit() {
        // User has 10 BTC, total OI is 100 BTC. Adding 5 → 15% < 20%.
        assert!(check_concentration(dec!(10), dec!(5), dec!(100), &default_config()).is_ok());
    }

    #[test]
    fn test_at_exact_limit() {
        // User has 15 BTC, adding 5 → 20/100 = 20% = limit. This is AT the limit.
        assert!(check_concentration(dec!(15), dec!(5), dec!(100), &default_config()).is_ok());
    }

    #[test]
    fn test_exceeds_limit() {
        // User has 15 BTC, adding 6 → 21/100 = 21% > 20%.
        let err = check_concentration(dec!(15), dec!(6), dec!(100), &default_config()).unwrap_err();
        assert_eq!(err.resulting_position, dec!(21));
        assert!(err.resulting_pct > dec!(0.20));
    }

    #[test]
    fn test_zero_oi_allows_first_position() {
        // No open interest yet — first order should be allowed.
        assert!(check_concentration(dec!(0), dec!(100), dec!(0), &default_config()).is_ok());
    }

    #[test]
    fn test_custom_limit() {
        let config = ConcentrationLimitConfig { max_pct_of_oi: dec!(0.10) }; // 10%
        // 11/100 = 11% > 10%.
        assert!(check_concentration(dec!(5), dec!(6), dec!(100), &config).is_err());
        // 9/100 = 9% < 10%.
        assert!(check_concentration(dec!(5), dec!(4), dec!(100), &config).is_ok());
    }

    #[test]
    fn test_whale_blocked() {
        // OKEx 2018 scenario: whale trying to hold 50% of OI.
        let err = check_concentration(dec!(0), dec!(500), dec!(1000), &default_config()).unwrap_err();
        assert_eq!(err.resulting_pct, dec!(0.5)); // 50%
    }

    #[test]
    fn test_error_display() {
        let err = check_concentration(dec!(15), dec!(6), dec!(100), &default_config()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("concentration limit exceeded"));
    }
}
