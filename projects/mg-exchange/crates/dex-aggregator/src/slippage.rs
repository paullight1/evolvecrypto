//! Slippage protection logic.
//!
//! Slippage is expressed in basis points throughout this crate
//! (1 bps = 0.01%). The protection layer validates that a quote's
//! expected price impact does not exceed the caller's tolerance, and
//! computes the minimum acceptable output for a given slippage setting.

use anyhow::{bail, Result};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::types::DexQuote;

/// Maximum slippage we will ever accept (5% = 500 bps).
///
/// Quotes that would move the price more than this are rejected regardless
/// of the caller-specified tolerance, to protect against sandwich attacks
/// or extreme thin-liquidity pools.
pub const MAX_SLIPPAGE_BPS: u16 = 500;

/// Minimum slippage we can request (1 bps = 0.01%).
///
/// Providers reject zero-slippage requests because rounding at the EVM level
/// means even a 1-wei deviation would revert the transaction.
pub const MIN_SLIPPAGE_BPS: u16 = 1;

/// Validates `slippage_bps` is within the safe range.
///
/// Returns `Err` if the value is out of the `[MIN_SLIPPAGE_BPS, MAX_SLIPPAGE_BPS]`
/// window, so callers can surface a useful error before sending a quote request.
pub fn validate_slippage_bps(slippage_bps: u16) -> Result<()> {
    if slippage_bps < MIN_SLIPPAGE_BPS {
        bail!(
            "slippage_bps ({slippage_bps}) is below the minimum allowed ({MIN_SLIPPAGE_BPS})"
        );
    }
    if slippage_bps > MAX_SLIPPAGE_BPS {
        bail!(
            "slippage_bps ({slippage_bps}) exceeds the maximum allowed ({MAX_SLIPPAGE_BPS})"
        );
    }
    Ok(())
}

/// Converts a basis-point slippage value to a `Decimal` fraction.
///
/// e.g. `50 bps → 0.005` (0.5%).
pub fn bps_to_fraction(bps: u16) -> Decimal {
    Decimal::from(bps) / dec!(10_000)
}

/// Computes the minimum acceptable output amount for a swap given the
/// expected `amount_out` from a quote and the caller's slippage tolerance.
///
/// `min_out = amount_out * (1 - slippage_bps / 10_000)`
pub fn min_amount_out(amount_out: Decimal, slippage_bps: u16) -> Decimal {
    let tolerance = bps_to_fraction(slippage_bps);
    amount_out * (Decimal::ONE - tolerance)
}

/// Validates that a quote's `price_impact` does not exceed the tolerance.
///
/// `price_impact` on the quote is a percentage (e.g. `0.5` means 0.5%).
/// We convert `slippage_bps` to a percentage and compare.
///
/// Returns `Err` if the quote should be rejected.
pub fn check_price_impact(quote: &DexQuote) -> Result<()> {
    let tolerance_pct = bps_to_fraction(quote.slippage_bps) * dec!(100);

    if quote.price_impact > tolerance_pct {
        bail!(
            "price impact {:.4}% exceeds slippage tolerance {:.4}% ({} bps) for {} on {}",
            quote.price_impact,
            tolerance_pct,
            quote.slippage_bps,
            quote.provider,
            quote.chain
        );
    }
    Ok(())
}

/// Rejects a quote whose `price_impact` exceeds the absolute cap regardless
/// of the caller's per-quote tolerance.
pub fn check_absolute_cap(quote: &DexQuote) -> Result<()> {
    let cap_pct = bps_to_fraction(MAX_SLIPPAGE_BPS) * dec!(100);
    if quote.price_impact > cap_pct {
        bail!(
            "price impact {:.4}% exceeds absolute cap {:.4}% — quote rejected",
            quote.price_impact,
            cap_pct
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rust_decimal_macros::dec;

    use crate::types::Chain;

    fn make_quote(price_impact: Decimal, slippage_bps: u16) -> DexQuote {
        DexQuote {
            provider: "test".into(),
            chain: Chain::Arbitrum,
            token_in: "USDC".into(),
            token_out: "WETH".into(),
            amount_in: dec!(1000),
            amount_out: dec!(0.45),
            price: dec!(0.00045),
            price_impact,
            gas_estimate: dec!(0.00004),
            gas_usd: dec!(0.10),
            route: vec![],
            expires_at: Utc::now() + chrono::Duration::seconds(30),
            slippage_bps,
        }
    }

    #[test]
    fn validate_slippage_too_low() {
        assert!(validate_slippage_bps(0).is_err());
    }

    #[test]
    fn validate_slippage_too_high() {
        assert!(validate_slippage_bps(501).is_err());
    }

    #[test]
    fn validate_slippage_boundary_values() {
        assert!(validate_slippage_bps(MIN_SLIPPAGE_BPS).is_ok());
        assert!(validate_slippage_bps(MAX_SLIPPAGE_BPS).is_ok());
    }

    #[test]
    fn bps_to_fraction_50() {
        assert_eq!(bps_to_fraction(50), dec!(0.005));
    }

    #[test]
    fn min_amount_out_50bps() {
        // 1000 USDC out, 0.5% slippage → min = 995
        let min = min_amount_out(dec!(1000), 50);
        assert_eq!(min, dec!(995.0));
    }

    #[test]
    fn check_price_impact_ok() {
        // 0.3% impact, 0.5% tolerance — should pass
        let quote = make_quote(dec!(0.3), 50);
        assert!(check_price_impact(&quote).is_ok());
    }

    #[test]
    fn check_price_impact_exceeds_tolerance() {
        // 1.0% impact, 0.5% (50 bps) tolerance — should fail
        let quote = make_quote(dec!(1.0), 50);
        assert!(check_price_impact(&quote).is_err());
    }

    #[test]
    fn check_absolute_cap_enforced() {
        // 6% impact exceeds 5% cap regardless of slippage_bps
        let quote = make_quote(dec!(6.0), MAX_SLIPPAGE_BPS);
        assert!(check_absolute_cap(&quote).is_err());
    }

    #[test]
    fn check_absolute_cap_passes_within_limit() {
        let quote = make_quote(dec!(4.9), MAX_SLIPPAGE_BPS);
        assert!(check_absolute_cap(&quote).is_ok());
    }
}
