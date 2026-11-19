//! Margin requirements for short (written) option positions.
//!
//! Selling an option exposes the writer to potentially unlimited loss (calls)
//! or strike-bounded loss (puts). This module computes the required margin to
//! hold a short option position open.
//!
//! # Formula (Bybit / Deribit style)
//!
//! ```text
//! Short Call margin = max(factor × S − OTM, min_factor × S) + mark_premium
//! Short Put  margin = max(factor × S − OTM, min_factor × K) + mark_premium
//!
//! where:
//!   S           = underlying spot / index price
//!   K           = strike price
//!   OTM         = max(0, K − S) for calls, max(0, S − K) for puts
//!   mark_premium = current option mark price (Black-Scholes fair value)
//!   factor      = 0.15 (covers ~3σ daily move for crypto)
//!   min_factor  = 0.10 (floor to prevent near-zero margin for deep OTM)
//! ```
//!
//! The `+ mark_premium` term ensures that the writer always has at least enough
//! margin to buy back the option at its current fair value.
//!
//! # References
//!
//! - Bybit Options Margin: https://www.bybit.com/en/help-center/s/article/Options-Margin
//! - Deribit Portfolio Margin: similar structure with factor = 0.15
//! - OKX Options Margin: https://www.okx.com/support/hc (cross-margin docs)

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::options_instruments::OptionType;

/// Default factor for the primary margin component (15% of underlying).
///
/// Covers approximately a 3-standard-deviation daily move for major crypto
/// assets. Matches Bybit's factor for USDC-settled options.
pub const MARGIN_FACTOR: Decimal = dec!(0.15);

/// Minimum factor floor (10% of underlying for calls, 10% of strike for puts).
///
/// Prevents near-zero margin for deep out-of-the-money options where the
/// primary component (factor × S − OTM) would be negligible.
pub const MIN_MARGIN_FACTOR: Decimal = dec!(0.10);

/// Compute the required margin for a short (written) option position.
///
/// Returns the per-unit margin requirement in the settlement currency (USDT).
///
/// # Arguments
/// - `option_type` — Call or Put
/// - `underlying_price` — current spot/index price of the underlying (S)
/// - `strike_price` — option strike price (K)
/// - `mark_premium` — current option mark price (Black-Scholes value)
/// - `qty` — number of contracts (absolute, always positive)
///
/// # Returns
/// Total margin requirement = per_unit_margin × qty
///
/// # Panics
/// Debug-asserts that `qty > 0` and prices are non-negative.
pub fn required_short_margin(
    option_type: OptionType,
    underlying_price: Decimal,
    strike_price: Decimal,
    mark_premium: Decimal,
    qty: Decimal,
) -> Decimal {
    debug_assert!(qty > Decimal::ZERO, "qty must be positive");
    debug_assert!(underlying_price >= Decimal::ZERO, "underlying_price must be non-negative");
    debug_assert!(strike_price >= Decimal::ZERO, "strike_price must be non-negative");
    debug_assert!(mark_premium >= Decimal::ZERO, "mark_premium must be non-negative");

    let per_unit = match option_type {
        OptionType::Call => {
            // OTM amount for call = max(0, K - S)
            let otm = (strike_price - underlying_price).max(Decimal::ZERO);
            let primary = MARGIN_FACTOR * underlying_price - otm;
            let floor = MIN_MARGIN_FACTOR * underlying_price;
            primary.max(floor) + mark_premium
        }
        OptionType::Put => {
            // OTM amount for put = max(0, S - K)
            let otm = (underlying_price - strike_price).max(Decimal::ZERO);
            let primary = MARGIN_FACTOR * underlying_price - otm;
            let floor = MIN_MARGIN_FACTOR * strike_price;
            primary.max(floor) + mark_premium
        }
    };

    per_unit * qty
}

/// Check whether a user has sufficient available margin to open a short option position.
///
/// Returns `Ok(required)` if the user has enough margin, or `Err(deficit)` if not.
pub fn check_short_margin(
    option_type: OptionType,
    underlying_price: Decimal,
    strike_price: Decimal,
    mark_premium: Decimal,
    qty: Decimal,
    available_margin: Decimal,
) -> Result<Decimal, Decimal> {
    let required = required_short_margin(option_type, underlying_price, strike_price, mark_premium, qty);
    if available_margin >= required {
        Ok(required)
    } else {
        Err(required - available_margin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    // ── Short Call ───────────────────────────────────────────────────────

    #[test]
    fn short_call_atm() {
        // ATM call: S = K = 50,000. OTM = 0.
        // primary = 0.15 × 50,000 − 0 = 7,500
        // floor   = 0.10 × 50,000     = 5,000
        // margin  = max(7500, 5000) + 500 (premium) = 8,000 per unit
        let m = required_short_margin(
            OptionType::Call,
            dec!(50000),
            dec!(50000),
            dec!(500),
            dec!(1),
        );
        assert_eq!(m, dec!(8000));
    }

    #[test]
    fn short_call_otm() {
        // OTM call: S = 50,000, K = 55,000. OTM = 5,000.
        // primary = 0.15 × 50,000 − 5,000 = 2,500
        // floor   = 0.10 × 50,000          = 5,000
        // margin  = max(2500, 5000) + 100 = 5,100
        let m = required_short_margin(
            OptionType::Call,
            dec!(50000),
            dec!(55000),
            dec!(100),
            dec!(1),
        );
        assert_eq!(m, dec!(5100));
    }

    #[test]
    fn short_call_itm() {
        // ITM call: S = 50,000, K = 45,000. OTM = 0 (ITM).
        // primary = 0.15 × 50,000 − 0 = 7,500
        // floor   = 0.10 × 50,000     = 5,000
        // margin  = 7,500 + 5,500 (high premium) = 13,000
        let m = required_short_margin(
            OptionType::Call,
            dec!(50000),
            dec!(45000),
            dec!(5500),
            dec!(1),
        );
        assert_eq!(m, dec!(13000));
    }

    #[test]
    fn short_call_deep_otm_uses_floor() {
        // Deep OTM: S = 50,000, K = 80,000. OTM = 30,000.
        // primary = 7,500 − 30,000 = −22,500 (negative!)
        // floor   = 5,000
        // margin  = max(−22500, 5000) + 10 = 5,010
        let m = required_short_margin(
            OptionType::Call,
            dec!(50000),
            dec!(80000),
            dec!(10),
            dec!(1),
        );
        assert_eq!(m, dec!(5010));
    }

    // ── Short Put ────────────────────────────────────────────────────────

    #[test]
    fn short_put_atm() {
        // ATM put: S = K = 50,000. OTM = 0.
        // primary = 0.15 × 50,000 = 7,500
        // floor   = 0.10 × 50,000 = 5,000  (uses strike for puts)
        // margin  = 7,500 + 400 = 7,900
        let m = required_short_margin(
            OptionType::Put,
            dec!(50000),
            dec!(50000),
            dec!(400),
            dec!(1),
        );
        assert_eq!(m, dec!(7900));
    }

    #[test]
    fn short_put_otm() {
        // OTM put: S = 50,000, K = 45,000. OTM = 5,000.
        // primary = 7,500 − 5,000 = 2,500
        // floor   = 0.10 × 45,000 = 4,500
        // margin  = max(2500, 4500) + 50 = 4,550
        let m = required_short_margin(
            OptionType::Put,
            dec!(50000),
            dec!(45000),
            dec!(50),
            dec!(1),
        );
        assert_eq!(m, dec!(4550));
    }

    #[test]
    fn short_put_deep_otm_uses_floor() {
        // Deep OTM put: S = 50,000, K = 20,000. OTM = 30,000.
        // primary = 7,500 − 30,000 = −22,500
        // floor   = 0.10 × 20,000 = 2,000
        // margin  = max(−22500, 2000) + 5 = 2,005
        let m = required_short_margin(
            OptionType::Put,
            dec!(50000),
            dec!(20000),
            dec!(5),
            dec!(1),
        );
        assert_eq!(m, dec!(2005));
    }

    // ── Quantity scaling ─────────────────────────────────────────────────

    #[test]
    fn margin_scales_with_quantity() {
        let m1 = required_short_margin(
            OptionType::Call,
            dec!(50000),
            dec!(50000),
            dec!(500),
            dec!(1),
        );
        let m10 = required_short_margin(
            OptionType::Call,
            dec!(50000),
            dec!(50000),
            dec!(500),
            dec!(10),
        );
        assert_eq!(m10, m1 * dec!(10));
    }

    // ── check_short_margin ──────────────────────────────────────────────

    #[test]
    fn check_margin_sufficient() {
        let result = check_short_margin(
            OptionType::Call,
            dec!(50000),
            dec!(50000),
            dec!(500),
            dec!(1),
            dec!(10000), // more than 8,000 required
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), dec!(8000));
    }

    #[test]
    fn check_margin_insufficient() {
        let result = check_short_margin(
            OptionType::Call,
            dec!(50000),
            dec!(50000),
            dec!(500),
            dec!(1),
            dec!(5000), // less than 8,000 required
        );
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), dec!(3000)); // deficit
    }

    // ── ETH-scale options ───────────────────────────────────────────────

    #[test]
    fn short_call_eth() {
        // ETH: S = 3,000, K = 3,000 (ATM), premium = 80
        // primary = 0.15 × 3,000 = 450
        // floor   = 0.10 × 3,000 = 300
        // margin  = 450 + 80 = 530
        let m = required_short_margin(
            OptionType::Call,
            dec!(3000),
            dec!(3000),
            dec!(80),
            dec!(1),
        );
        assert_eq!(m, dec!(530));
    }
}
