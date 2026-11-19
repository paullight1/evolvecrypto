//! Property-based tests for order validation invariants.

use exchange_common::*;
use exchange_order_gateway::{OrderValidator, ValidationError};
use proptest::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;

fn btc_product() -> Product {
    Product {
        symbol: Symbol::new("BTCUSDT"),
        base_asset: "BTC".to_string(),
        quote_asset: "USDT".to_string(),
        product_type: ProductType::Perpetual,
        tick_size: dec!(0.01),
        lot_size: dec!(0.00001),
        min_notional: dec!(10),
        maker_fee: dec!(0.001),
        taker_fee: dec!(0.002),
        max_order_qty: Some(dec!(1000)),
        max_order_notional: Some(dec!(50_000_000)),
        max_leverage: None,
    }
}

fn make_cmd(price: Decimal, qty: Decimal) -> PlaceOrderCommand {
    PlaceOrderCommand {
        order_id: Uuid::new_v4(),
        user_id: Uuid::new_v4(),
        symbol: Symbol::new("BTCUSDT"),
        side: Side::Buy,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::Gtc,
        price,
        qty,
        timestamp: chrono::Utc::now(),
        reduce_only: false,
        expire_at: None,
        position_idx: 0,
        iceberg_qty: None,
        order_link_id: None,
        slippage_tolerance: None,
        close_position: false,
        stp_mode: StpMode::default(),
        request_expire_time: None,
    }
}

// ── Property 1: Valid orders always pass ──────────────────────────────────

proptest! {
    #[test]
    fn prop_valid_orders_accepted(
        price_cents in 5_000_000u64..10_000_000u64, // $50,000 - $100,000
        qty_units in 100u64..100_000u64,            // 0.00100 - 1.00000
    ) {
        // Construct price/qty that are on tick/lot.
        let price = Decimal::new(price_cents as i64, 2); // cents → dollars, on 0.01 tick
        let qty = Decimal::new(qty_units as i64, 5);     // on 0.00001 lot

        let product = btc_product();
        let cmd = make_cmd(price, qty);

        // If notional >= min_notional and qty <= max, should pass.
        let notional = price * qty;
        if notional >= product.min_notional && qty <= dec!(1000) && notional <= dec!(50_000_000) {
            prop_assert!(
                OrderValidator::validate(&cmd, &product).is_ok(),
                "valid order rejected: price={}, qty={}, notional={}", price, qty, notional
            );
        }
    }
}

// ── Property 2: Zero/negative qty always rejected ────────────────────────

proptest! {
    #[test]
    fn prop_invalid_qty_rejected(
        bad_qty in -1000i64..=0i64,
    ) {
        let qty = Decimal::new(bad_qty, 2);
        let cmd = make_cmd(dec!(50000), qty);
        let product = btc_product();
        let result = OrderValidator::validate(&cmd, &product);
        prop_assert!(result.is_err(), "zero/negative qty {} should be rejected", qty);
    }
}

// ── Property 3: Price off-tick always rejected ───────────────────────────

proptest! {
    #[test]
    fn prop_off_tick_price_rejected(
        base in 4_000_000u64..10_000_000u64,
    ) {
        // Add 0.001 (not on 0.01 tick) to an on-tick price.
        let price = Decimal::new(base as i64, 2) + dec!(0.001);
        let cmd = make_cmd(price, dec!(1));
        let product = btc_product();
        let result = OrderValidator::validate(&cmd, &product);
        prop_assert!(
            matches!(result, Err(ValidationError::PriceNotOnTick { .. })),
            "off-tick price {} should be rejected", price
        );
    }
}

// ── Property 4: Max qty exceeded always rejected ─────────────────────────

proptest! {
    #[test]
    fn prop_max_qty_rejected(
        excess in 1u64..1000u64,
    ) {
        let qty = dec!(1000) + Decimal::new(excess as i64, 5); // > 1000 (on lot)
        let cmd = make_cmd(dec!(50000), qty);
        let product = btc_product();
        let result = OrderValidator::validate(&cmd, &product);
        prop_assert!(
            matches!(result, Err(ValidationError::ExceedsMaxQty { .. })),
            "qty {} exceeding max should be rejected", qty
        );
    }
}
