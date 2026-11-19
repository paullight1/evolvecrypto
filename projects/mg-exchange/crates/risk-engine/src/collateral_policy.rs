//! Collateral concentration and PnL haircut policies.
//!
//! # M26-2: Collateral concentration limit
//! Single non-stable asset cannot exceed 40% of cross-margin collateral.
//! Prevents over-reliance on a manipulable asset (Mango Markets defense).
//!
//! # M26-10: Unrealized PnL haircut
//! Unrealized PnL used as collateral gets a 50% haircut (not 100% face value).
//! Prevents correlated collapse where position A's paper profit margins position B.
//!
//! # References
//! - Mango Markets / Eisenberg ($114M, 2022): attacker's collateral was 100% MNGO
//! - FTX/FTT circular collateral ($8B, 2022): exchange token as collateral

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Check if adding `additional_usd` of `asset` collateral would violate
/// the concentration limit.
///
/// - `asset_collateral_usd`: current USD value of this asset in collateral
/// - `total_collateral_usd`: total USD value of all collateral
/// - `additional_usd`: USD value being added
/// - `is_stable`: true for USDT/USDC (exempt from concentration limit)
/// - `max_concentration_pct`: maximum fraction (default 0.40 = 40%)
///
/// Returns `Ok(())` if allowed, `Err(current_pct)` if would exceed limit.
pub fn check_concentration(
    asset_collateral_usd: Decimal,
    total_collateral_usd: Decimal,
    additional_usd: Decimal,
    is_stable: bool,
    max_concentration_pct: Decimal,
) -> Result<(), Decimal> {
    if is_stable {
        return Ok(()); // Stables exempt
    }

    // If there's no existing collateral, any first deposit is fine.
    if total_collateral_usd.is_zero() {
        return Ok(());
    }

    let new_total = total_collateral_usd + additional_usd;
    if new_total.is_zero() {
        return Ok(());
    }

    let new_asset_total = asset_collateral_usd + additional_usd;
    let concentration = new_asset_total / new_total;

    if concentration > max_concentration_pct {
        Err(concentration)
    } else {
        Ok(())
    }
}

/// Default maximum concentration: 40%.
pub const DEFAULT_MAX_CONCENTRATION: Decimal = dec!(0.40);

/// Apply a haircut to unrealized PnL when used as cross-margin collateral.
///
/// Returns the adjusted equity (balance + haircut × unrealized_pnl).
///
/// - `balance`: cash/settled balance
/// - `unrealized_pnl`: paper profit/loss from open positions
/// - `haircut`: fraction of PnL to count (default 0.50 = 50%)
///
/// If PnL is negative, it's applied at full value (losses are always real).
pub fn apply_pnl_haircut(
    balance: Decimal,
    unrealized_pnl: Decimal,
    haircut: Decimal,
) -> Decimal {
    if unrealized_pnl > Decimal::ZERO {
        // Profit: apply haircut (count only a fraction)
        balance + unrealized_pnl * haircut
    } else {
        // Loss: count in full (conservative)
        balance + unrealized_pnl
    }
}

/// Default PnL haircut: 50%.
pub const DEFAULT_PNL_HAIRCUT: Decimal = dec!(0.50);

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    // --- Concentration limit tests ---

    #[test]
    fn stable_coin_exempt_from_concentration() {
        // Even at 100% concentration, stables are allowed.
        let result = check_concentration(
            dec!(100000), dec!(100000), dec!(50000), true, dec!(0.40),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn below_limit_allowed() {
        // BTC is 30% of collateral — adding more keeps it under 40%.
        let result = check_concentration(
            dec!(30000), dec!(100000), dec!(5000), false, dec!(0.40),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn at_limit_rejected() {
        // BTC is 39% of $100K. Adding $5K → $44K / $105K = 41.9% → rejected.
        let result = check_concentration(
            dec!(39000), dec!(100000), dec!(5000), false, dec!(0.40),
        );
        assert!(result.is_err());
    }

    #[test]
    fn zero_total_allowed() {
        let result = check_concentration(
            Decimal::ZERO, Decimal::ZERO, dec!(1000), false, dec!(0.40),
        );
        assert!(result.is_ok());
    }

    // --- PnL haircut tests ---

    #[test]
    fn positive_pnl_haircutted() {
        let equity = apply_pnl_haircut(dec!(10000), dec!(5000), dec!(0.50));
        // 10000 + 5000 * 0.5 = 12500
        assert_eq!(equity, dec!(12500));
    }

    #[test]
    fn negative_pnl_full_deduction() {
        let equity = apply_pnl_haircut(dec!(10000), dec!(-3000), dec!(0.50));
        // 10000 + (-3000) = 7000 (full loss, no haircut)
        assert_eq!(equity, dec!(7000));
    }

    #[test]
    fn zero_pnl_no_effect() {
        let equity = apply_pnl_haircut(dec!(10000), Decimal::ZERO, dec!(0.50));
        assert_eq!(equity, dec!(10000));
    }

    #[test]
    fn full_haircut_zero_means_no_pnl_counted() {
        let equity = apply_pnl_haircut(dec!(10000), dec!(5000), Decimal::ZERO);
        // 10000 + 5000 * 0 = 10000
        assert_eq!(equity, dec!(10000));
    }

    #[test]
    fn no_haircut_means_full_pnl() {
        let equity = apply_pnl_haircut(dec!(10000), dec!(5000), Decimal::ONE);
        // 10000 + 5000 * 1.0 = 15000
        assert_eq!(equity, dec!(15000));
    }

    // ── M26-12: Adversarial tests ─────────────────────────────────────────

    #[test]
    fn adversarial_mango_attack_single_asset_collateral() {
        // Mango Markets scenario: attacker has 100% of collateral in one manipulable asset.
        // With 40% concentration limit, this should be blocked.
        let result = check_concentration(
            dec!(100000), // already 100K in MNGO
            dec!(100000), // total = 100K
            dec!(50000),  // adding 50K more MNGO
            false,        // not a stablecoin
            dec!(0.40),
        );
        assert!(result.is_err(), "100% concentration in single asset should be blocked");
    }

    #[test]
    fn adversarial_ftx_exchange_token_collateral() {
        // FTX/FTT: exchange token should have 0% weight (Tier4).
        // With PnL haircut: $10K unrealized profit → only $5K counted.
        let equity = apply_pnl_haircut(dec!(1000), dec!(10000), dec!(0.50));
        // 1000 + 10000 * 0.5 = 6000 (not 11000)
        assert_eq!(equity, dec!(6000));
        assert!(equity < dec!(11000), "unrealized PnL must be haircut");
    }

    #[test]
    fn adversarial_correlated_collapse_both_positions_crash() {
        // Position A: +$10K unrealized, Position B: margined using A's profit.
        // With 50% haircut, only $5K of A's profit counts for B's margin.
        let effective = apply_pnl_haircut(dec!(5000), dec!(10000), dec!(0.50));
        assert_eq!(effective, dec!(10000)); // 5000 + 10000*0.5
        // Without haircut it would be $15K — 50% more.
        let without = apply_pnl_haircut(dec!(5000), dec!(10000), Decimal::ONE);
        assert_eq!(without, dec!(15000));
        assert!(effective < without, "haircut must reduce effective margin");
    }
}
