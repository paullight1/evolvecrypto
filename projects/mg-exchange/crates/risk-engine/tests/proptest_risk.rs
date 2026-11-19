//! Property-based tests for risk engine invariants.

use exchange_common::futures::*;
use exchange_risk_engine::margin_calculator::*;
use proptest::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;
use chrono::Utc;

fn valid_price() -> impl Strategy<Value = Decimal> {
    (100u64..200_000u64).prop_map(Decimal::from)
}

fn valid_size() -> impl Strategy<Value = Decimal> {
    (1u64..10_000u64).prop_map(|v| Decimal::new(v as i64, 3))
}

fn valid_leverage() -> impl Strategy<Value = Decimal> {
    (1u32..100u32).prop_map(Decimal::from)
}

fn make_pos(side: PositionSide, size: Decimal, entry: Decimal, margin: Decimal) -> Position {
    Position {
        user_id: Uuid::nil(),
        symbol: exchange_common::Symbol::new("BTCUSDT"),
        side,
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

// ── Property 1: Initial margin is always positive for valid inputs ────────

proptest! {
    #[test]
    fn prop_initial_margin_positive(
        size in valid_size(),
        price in valid_price(),
        leverage in valid_leverage(),
    ) {
        let margin = required_initial_margin(size, price, leverage);
        prop_assert!(margin > Decimal::ZERO, "initial margin must be > 0, got {}", margin);
    }
}

// ── Property 2: Higher leverage → lower margin ───────────────────────────

proptest! {
    #[test]
    fn prop_higher_leverage_lower_margin(
        size in valid_size(),
        price in valid_price(),
    ) {
        let margin_10x = required_initial_margin(size, price, dec!(10));
        let margin_20x = required_initial_margin(size, price, dec!(20));
        prop_assert!(
            margin_20x < margin_10x,
            "20x margin {} should be less than 10x margin {}",
            margin_20x, margin_10x
        );
    }
}

// ── Property 3: Liquidation is consistent with effective margin ──────────

proptest! {
    #[test]
    fn prop_liquidation_consistent(
        entry in valid_price(),
        mark_offset in -5000i64..5000i64,
    ) {
        let mark = Decimal::from(entry.to_string().parse::<i64>().unwrap_or(50000) + mark_offset).max(Decimal::ONE);
        let margin = entry / dec!(10); // 10x leverage
        let pos = make_pos(PositionSide::Long, dec!(1), entry, margin);
        let config = PerpetualConfig::btc_default();

        let effective = pos.effective_margin(mark);
        let maint = required_maintenance_margin(pos.size, mark, config.maintenance_margin_rate);

        let should_liq = effective < maint;
        let is_liq = is_liquidatable(&pos, mark, &config);

        // These should agree (both use same formula path).
        prop_assert_eq!(is_liq, should_liq,
            "mismatch at mark={}: effective={}, maint={}", mark, effective, maint);
    }
}

// ── Property 4: Maintenance margin ≤ initial margin ──────────────────────

proptest! {
    #[test]
    fn prop_maintenance_le_initial(
        size in valid_size(),
        price in valid_price(),
        leverage in valid_leverage(),
    ) {
        let initial = required_initial_margin(size, price, leverage);
        let maintenance = required_maintenance_margin(size, price, dec!(0.005)); // 0.5% MMR
        prop_assert!(
            maintenance <= initial,
            "maintenance {} > initial {} at leverage {}",
            maintenance, initial, leverage
        );
    }
}

// ── Property 5: Available margin ≥ 0 ─────────────────────────────────────

proptest! {
    #[test]
    fn prop_available_margin_non_negative(
        equity in valid_price(),
        margin_used in valid_price(),
    ) {
        let avail = available_margin(equity, margin_used);
        prop_assert!(avail >= Decimal::ZERO, "available margin must be >= 0");
    }
}
