//! Smoke tests for S13 G.5 — per-stage place_order latency metrics.
//!
//! Verifies that after a successful place_order call all 7 stage counters
//! have been incremented at least once and that the max_ns values are > 0.
//!
//! This is purely an observability correctness check — it does not test
//! performance values, only that the instrumentation fires.

use exchange_api_gateway::exchange::Exchange;
use exchange_common::*;
use rust_decimal_macros::dec;
use std::sync::Arc;
use uuid::Uuid;

/// All 7 stage names wired in G.1.
const STAGES: &[&str] = &[
    "auth",
    "rate_limit",
    "validation",
    "balance_lock",
    "wal_write",
    "matching",
    "process_events",
];

fn make_exchange_with_spot() -> Arc<Exchange> {
    let exchange = Exchange::new();
    let product = Product {
        symbol: Symbol::new("BTCUSDT"),
        base_asset: "BTC".to_string(),
        quote_asset: "USDT".to_string(),
        product_type: ProductType::Spot,
        tick_size: dec!(0.01),
        lot_size: dec!(0.00001),
        min_notional: dec!(10),
        maker_fee: dec!(0.001),
        taker_fee: dec!(0.002),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
    };
    exchange.add_product(product);
    let user = Uuid::nil();
    exchange.create_account(user);
    exchange.deposit(user, "USDT", dec!(1_000_000)).unwrap();
    exchange.deposit(user, "BTC", dec!(100)).unwrap();
    exchange
}

fn make_cmd(price: rust_decimal::Decimal) -> PlaceOrderCommand {
    PlaceOrderCommand {
        order_id: Uuid::new_v4(),
        user_id: Uuid::nil(),
        symbol: Symbol::new("BTCUSDT"),
        side: Side::Buy,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::Gtc,
        price,
        qty: dec!(0.1),
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

/// Placing a single spot limit order must increment all 7 stage counters and
/// set all 7 max_ns values to > 0.
#[test]
fn test_stage_metrics_all_fire_on_spot_limit_order() {
    let exchange = make_exchange_with_spot();
    let result = exchange.place_order(make_cmd(dec!(50_000)));
    assert!(result.is_ok(), "place_order failed: {:?}", result.err());

    // All 7 stages must have been recorded.
    for stage in STAGES {
        let count = exchange.metrics.place_order_stage_count(stage);
        assert!(
            count > 0,
            "stage '{}' count is 0 — instrumentation did not fire",
            stage
        );

        let max_ns = exchange.metrics.place_order_stage_max_ns(stage);
        assert!(
            max_ns > 0,
            "stage '{}' max_ns is 0 — instrumentation did not record any time",
            stage
        );
    }
}

/// Placing N orders accumulates the count correctly for all stages.
#[test]
fn test_stage_metrics_accumulate_across_orders() {
    let exchange = make_exchange_with_spot();

    let n = 3u32;
    for i in 0..n {
        let price = dec!(50_000) - rust_decimal::Decimal::from(i);
        exchange.place_order(make_cmd(price)).expect("place_order failed");
    }

    for stage in STAGES {
        let count = exchange.metrics.place_order_stage_count(stage);
        assert_eq!(
            count,
            u64::from(n),
            "stage '{}' expected count={} after {} orders, got {}",
            stage,
            n,
            n,
            count
        );
    }
}

/// Unknown stage names must not panic and must return 0.
#[test]
fn test_stage_metrics_unknown_stage_returns_zero() {
    let exchange = make_exchange_with_spot();
    assert_eq!(exchange.metrics.place_order_stage_max_ns("nonexistent"), 0);
    assert_eq!(exchange.metrics.place_order_stage_count("nonexistent"), 0);
}
