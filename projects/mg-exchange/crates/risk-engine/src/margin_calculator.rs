//! Margin calculations for perpetual futures positions.
//!
//! All values use [`rust_decimal::Decimal`] — never `f64` for money.
//!
//! # Linear vs Inverse
//!
//! **Linear (USDT-margined)**: margin, PnL, and notional all expressed in USDT.
//! - `initial_margin = size * price / leverage`
//! - `notional = size * price`
//!
//! **Inverse (coin-margined)**: margin and PnL expressed in the base asset (BTC/ETH).
//! The contract is quoted in USD but settled in the base coin.
//! - `initial_margin_btc = (contracts * contract_size) / (price * leverage)`
//!   e.g. 100 contracts of BTCUSD at $50,000 with 10x → 100/(50000*10) = 0.0002 BTC
//! - `notional_usd = contracts * contract_size` (fixed per contract, independent of price)

use exchange_common::{
    futures::{ContractType, PerpetualConfig, Position},
    types::{Price, Qty},
};
use rust_decimal::Decimal;

/// Required initial margin to open a **linear** (USDT-margined) position.
///
/// `initial_margin = notional / leverage = size * price / leverage`
pub fn required_initial_margin(size: Qty, price: Price, leverage: Decimal) -> Decimal {
    debug_assert!(!leverage.is_zero(), "leverage must not be zero");
    size * price / leverage
}

/// Required initial margin to open an **inverse** (coin-margined) position.
///
/// For inverse perps each contract is worth `contract_size` USD. Margin is held
/// in the base asset (BTC, ETH, etc.).
///
/// `initial_margin_base = (contracts * contract_size) / (price * leverage)`
///
/// Example: 100 BTCUSD contracts, price = $50,000, 10x leverage, contract_size = $1:
///   = (100 * 1) / (50,000 * 10) = 0.0002 BTC
pub fn required_initial_margin_inverse(
    contracts: Qty,
    price: Price,
    leverage: Decimal,
    contract_size: Decimal,
) -> Decimal {
    debug_assert!(!leverage.is_zero(), "leverage must not be zero");
    debug_assert!(!price.is_zero(), "price must not be zero");
    (contracts * contract_size) / (price * leverage)
}

/// Required initial margin based on contract type.
///
/// Delegates to `required_initial_margin` or `required_initial_margin_inverse`
/// depending on `config.contract_type`.
pub fn required_initial_margin_for_config(
    size: Qty,
    price: Price,
    leverage: Decimal,
    config: &PerpetualConfig,
) -> Decimal {
    match config.contract_type {
        ContractType::Linear => required_initial_margin(size, price, leverage),
        ContractType::Inverse => {
            required_initial_margin_inverse(size, price, leverage, config.contract_size)
        }
    }
}

/// Required maintenance margin to keep a **linear** position open.
///
/// `maintenance_margin = notional * mmr = size * mark_price * mmr`
pub fn required_maintenance_margin(size: Qty, mark_price: Price, mmr: Decimal) -> Decimal {
    size * mark_price * mmr
}

/// Required maintenance margin for an **inverse** (coin-margined) position.
///
/// The margin and PnL for inverse contracts are denominated in the base asset.
/// Maintenance margin in base asset is:
///   `mm_base = (contracts * contract_size) / mark_price * mmr`
///
/// This reflects the mark-price-denominated value expressed back in base coin.
pub fn required_maintenance_margin_inverse(
    contracts: Qty,
    mark_price: Price,
    mmr: Decimal,
    contract_size: Decimal,
) -> Decimal {
    if mark_price.is_zero() {
        return Decimal::ZERO;
    }
    (contracts * contract_size) / mark_price * mmr
}

/// Whether a position should be liquidated at the given mark price.
///
/// A position becomes liquidatable when its effective margin (margin + unrealised PnL)
/// falls below the required maintenance margin.
///
/// For **inverse** contracts the notional used to look up the tiered MMR is the USD
/// notional (`contracts * contract_size`), which is price-independent. The effective
/// margin and maintenance margin are both denominated in the base asset.
///
/// Uses the tiered maintenance margin rate from `config.effective_mmr(notional)`,
/// so large positions are held to a stricter standard than small ones.
pub fn is_liquidatable(position: &Position, mark_price: Price, config: &PerpetualConfig) -> bool {
    match config.contract_type {
        ContractType::Linear => {
            let notional = position.notional(mark_price);
            let mmr = config.effective_mmr(notional);
            let effective = position.effective_margin(mark_price);
            let maintenance = required_maintenance_margin(position.size, mark_price, mmr);
            effective < maintenance
        }
        ContractType::Inverse => {
            // USD notional (price-independent) used for tier selection.
            let notional_usd = position.notional_inverse(config.contract_size);
            let mmr = config.effective_mmr(notional_usd);
            // Effective margin and maintenance are both in base asset (BTC, ETH).
            let effective = position.effective_margin_inverse(mark_price);
            let maintenance = required_maintenance_margin_inverse(
                position.size,
                mark_price,
                mmr,
                config.contract_size,
            );
            effective < maintenance
        }
    }
}

/// Available margin for new positions.
///
/// `available = equity - sum(initial_margins_of_all_open_positions)`
pub fn available_margin(equity: Decimal, total_initial_margin: Decimal) -> Decimal {
    (equity - total_initial_margin).max(Decimal::ZERO)
}

/// Maximum position size (in base asset) given available margin, price, and leverage.
///
/// For **linear** perps this gives base asset units:
///   `max_size = available_margin_usdt * leverage / price`
///
/// For **inverse** perps use `max_position_size_inverse`.
pub fn max_position_size(available_margin: Decimal, price: Price, leverage: Decimal) -> Qty {
    debug_assert!(!price.is_zero(), "price must not be zero");
    debug_assert!(!leverage.is_zero(), "leverage must not be zero");
    available_margin * leverage / price
}

/// Maximum number of contracts for an **inverse** (coin-margined) position.
///
/// `max_contracts = available_margin_base * price * leverage / contract_size`
///
/// Example: 0.1 BTC available, price $50,000, 10x leverage, contract_size $1:
///   = 0.1 * 50,000 * 10 / 1 = 50,000 contracts ($50,000 notional)
pub fn max_position_size_inverse(
    available_margin_base: Decimal,
    price: Price,
    leverage: Decimal,
    contract_size: Decimal,
) -> Qty {
    debug_assert!(!price.is_zero(), "price must not be zero");
    debug_assert!(!leverage.is_zero(), "leverage must not be zero");
    debug_assert!(!contract_size.is_zero(), "contract_size must not be zero");
    available_margin_base * price * leverage / contract_size
}

#[cfg(test)]
mod tests {
    use super::*;
    use exchange_common::futures::{MarginMode, PerpetualConfig, Position, PositionSide};
    use rust_decimal_macros::dec;
    use uuid::Uuid;
    use chrono::Utc;

    fn long_position(size: Decimal, entry: Decimal, margin: Decimal) -> Position {
        Position {
            user_id: Uuid::nil(),
            symbol: exchange_common::types::Symbol::new("BTCUSDT"),
            side: PositionSide::Long,
            size,
            entry_price: entry,
            leverage: dec!(10),
            margin_mode: MarginMode::Isolated,
            margin,
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

    // --- required_initial_margin ---

    #[test]
    fn test_initial_margin_10x() {
        // 1 BTC at 50_000 USDT, 10x leverage → margin = 5_000
        let margin = required_initial_margin(dec!(1), dec!(50000), dec!(10));
        assert_eq!(margin, dec!(5000));
    }

    #[test]
    fn test_initial_margin_100x() {
        // 1 BTC at 50_000 USDT, 100x leverage → margin = 500
        let margin = required_initial_margin(dec!(1), dec!(50000), dec!(100));
        assert_eq!(margin, dec!(500));
    }

    #[test]
    fn test_initial_margin_fractional_size() {
        // 0.5 BTC at 40_000 USDT, 20x → notional = 20_000 → margin = 1_000
        let margin = required_initial_margin(dec!(0.5), dec!(40000), dec!(20));
        assert_eq!(margin, dec!(1000));
    }

    // --- required_maintenance_margin ---

    #[test]
    fn test_maintenance_margin_btc() {
        // 1 BTC mark 50_000, mmr = 0.005 → maintenance = 250
        let mm = required_maintenance_margin(dec!(1), dec!(50000), dec!(0.005));
        assert_eq!(mm, dec!(250));
    }

    #[test]
    fn test_maintenance_margin_scales_with_price() {
        // If mark price halves, maintenance halves
        let mm_high = required_maintenance_margin(dec!(1), dec!(50000), dec!(0.01));
        let mm_low = required_maintenance_margin(dec!(1), dec!(25000), dec!(0.01));
        assert_eq!(mm_high, dec!(500));
        assert_eq!(mm_low, dec!(250));
        assert_eq!(mm_high, mm_low * dec!(2));
    }

    // --- is_liquidatable ---

    #[test]
    fn test_not_liquidatable_at_entry() {
        // Long 1 BTC, entry 50_000, margin 5_000 (10x).
        // At entry: effective_margin = 5_000, notional = 50_000, mmr = 0.005 → mm = 250.
        // 5_000 > 250 → not liquidatable.
        let pos = long_position(dec!(1), dec!(50000), dec!(5000));
        let config = PerpetualConfig::btc_default();
        assert!(!is_liquidatable(&pos, dec!(50000), &config));
    }

    #[test]
    fn test_liquidatable_when_price_crashes() {
        // Long 1 BTC, entry 50_000, margin 5_000 (10x).
        // Liq price ≈ 45_000. At 44_000:
        //   effective_margin = 5_000 + (44_000 - 50_000) * 1 = -1_000
        //   maintenance = 44_000 * 0.005 = 220
        //   -1_000 < 220 → liquidatable
        let pos = long_position(dec!(1), dec!(50000), dec!(5000));
        let config = PerpetualConfig::btc_default();
        assert!(is_liquidatable(&pos, dec!(44000), &config));
    }

    #[test]
    fn test_liquidatable_boundary() {
        // Effective margin exactly equals maintenance margin → NOT liquidatable
        // (strictly less than triggers liquidation)
        // At mark = liq_price: effective = 0, maintenance > 0 → liquidatable
        let pos = long_position(dec!(1), dec!(50000), dec!(5000));
        let config = PerpetualConfig::btc_default();
        // At mark = 45_000: effective = 5_000 - 5_000 = 0, maintenance = 225 → liquidatable
        assert!(is_liquidatable(&pos, dec!(45000), &config));
    }

    // --- available_margin ---

    #[test]
    fn test_available_margin_normal() {
        // equity 10_000, locked 3_000 → available 7_000
        assert_eq!(available_margin(dec!(10000), dec!(3000)), dec!(7000));
    }

    #[test]
    fn test_available_margin_fully_locked() {
        assert_eq!(available_margin(dec!(5000), dec!(5000)), dec!(0));
    }

    #[test]
    fn test_available_margin_never_negative() {
        // Unrealised losses can push equity below locked margin — clamp to 0
        assert_eq!(available_margin(dec!(2000), dec!(5000)), dec!(0));
    }

    // --- max_position_size ---

    #[test]
    fn test_max_position_size_basic() {
        // 1_000 available, 10x, price 50_000 → max = 1_000 * 10 / 50_000 = 0.2 BTC
        let size = max_position_size(dec!(1000), dec!(50000), dec!(10));
        assert_eq!(size, dec!(0.2));
    }

    #[test]
    fn test_max_position_size_high_leverage() {
        // 500 available, 100x, price 50_000 → max = 500 * 100 / 50_000 = 1 BTC
        let size = max_position_size(dec!(500), dec!(50000), dec!(100));
        assert_eq!(size, dec!(1));
    }

    #[test]
    fn test_max_position_size_zero_margin() {
        assert_eq!(max_position_size(dec!(0), dec!(50000), dec!(10)), dec!(0));
    }

    // --- tiered MMR in is_liquidatable ---

    #[test]
    fn test_liquidatable_uses_tiered_mmr_small_position() {
        // Small BTC position (notional << 2M) → MMR = 0.5%.
        // Long 1 BTC at entry 50_000, margin 5_000, mark 49_750.
        //   effective_margin = 5_000 - 250 = 4_750
        //   notional = 49_750, mmr = 0.5%
        //   maintenance = 49_750 * 0.005 = 248.75
        //   4_750 > 248.75 → NOT liquidatable
        let pos = long_position(dec!(1), dec!(50000), dec!(5000));
        let config = PerpetualConfig::btc_default();
        assert!(!is_liquidatable(&pos, dec!(49750), &config));
    }

    #[test]
    fn test_liquidatable_large_btc_position_stricter_mmr() {
        // Position with notional ≥25M → MMR = 5% (tier 4).
        // 500 BTC at entry 50_000 each → notional = 25_000_000 exactly (tier 4 lower bound).
        // margin = 1_250_000 (at 10x leverage).
        // At mark = 50_000: effective = 1_250_000, maintenance = 25_000_000 * 0.05 = 1_250_000.
        // effective == maintenance → strictly less? No — NOT liquidatable (strictly less triggers).
        let pos = long_position(dec!(500), dec!(50000), dec!(1250000));
        let config = PerpetualConfig::btc_default();
        assert!(!is_liquidatable(&pos, dec!(50000), &config));

        // Tiny price drop: mark = 49_999 → notional = 24_999_500 → still tier 4 (≥25M? No.)
        // 500 * 49_999 = 24_999_500 < 25_000_000 → falls back to tier 3 (12M–25M): MMR 2.5%.
        // effective = 1_250_000 + 500 * (49_999 - 50_000) = 1_250_000 - 500 = 1_249_500
        // maintenance = 24_999_500 * 0.025 = 624_987.5
        // 1_249_500 > 624_987.5 → NOT liquidatable (position was well-capitalized)
        assert!(!is_liquidatable(&pos, dec!(49999), &config));
    }

    #[test]
    fn test_is_liquidatable_tiered_mmr_high_tier_triggers_earlier() {
        // Compare: same position, different configs (small vs large tier MMR).
        // Custom config: two tiers — tier 0: MMR 0.5%, tier 1 (≥100k notional): MMR 20%.
        use exchange_common::futures::LeverageTier;
        let custom_config = PerpetualConfig {
            symbol: exchange_common::types::Symbol::new("TESTUSDT"),
            max_leverage: dec!(100),
            initial_margin_rate: dec!(0.01),
            maintenance_margin_rate: dec!(0.005),
            funding_interval_hours: 8,
            max_funding_rate: dec!(0.01),
            insurance_fund: Decimal::ZERO,
            contract_type: exchange_common::futures::ContractType::default(),
            settle_coin: "USDT".to_string(),
            contract_size: Decimal::ONE,
            tiers: vec![
                LeverageTier {
                    min_notional: Decimal::ZERO,
                    max_notional: dec!(100000),
                    max_leverage: dec!(100),
                    maintenance_margin_rate: dec!(0.005),
                },
                LeverageTier {
                    min_notional: dec!(100000),
                    max_notional: Decimal::MAX,
                    max_leverage: dec!(10),
                    maintenance_margin_rate: dec!(0.20), // very high MMR for large positions
                },
            ],
        };

        // Position: 10 BTC at entry 15_000, mark 15_000 → notional = 150_000 (tier 1).
        // margin = 15_000 (10% of notional — notional / 10).
        // effective = 15_000, maintenance = 150_000 * 0.20 = 30_000.
        // 15_000 < 30_000 → liquidatable.
        let pos = long_position(dec!(10), dec!(15000), dec!(15000));
        assert!(is_liquidatable(&pos, dec!(15000), &custom_config));

        // Same position but under the tier boundary (notional = 90_000 < 100_000 → tier 0 MMR 0.5%).
        // 6 BTC at entry 15_000, margin 9_000. effective = 9_000, maintenance = 90_000 * 0.005 = 450.
        // 9_000 > 450 → NOT liquidatable.
        let small_pos = long_position(dec!(6), dec!(15000), dec!(9000));
        assert!(!is_liquidatable(&small_pos, dec!(15000), &custom_config));
    }
}
