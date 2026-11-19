//! Integration test for POST /admin/orders/prune-stale.
//!
//! Verifies that the prune endpoint cancels only resting orders that are
//! BOTH old (older than the age cutoff) AND far from mid-price (distance ≥
//! threshold). Near-mid orders and recent orders must be left intact, even
//! if the other condition is met.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use exchange_api_gateway::exchange::Exchange;
use exchange_api_gateway::server::build_router;
use exchange_common::*;
use rust_decimal_macros::dec;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

fn add_btc(exchange: &Arc<Exchange>) {
    exchange.add_product(Product {
        symbol: Symbol::new("BTCUSDT"),
        base_asset: "BTC".to_string(),
        quote_asset: "USDT".to_string(),
        product_type: ProductType::Perpetual,
        tick_size: dec!(0.01),
        lot_size: dec!(0.00001),
        min_notional: dec!(10),
        maker_fee: dec!(0.001),
        taker_fee: dec!(0.002),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
    });
}

fn fund(exchange: &Arc<Exchange>, user: Uuid) {
    exchange.accounts.create_account(user);
    exchange.accounts.deposit(user, "USDT", dec!(10_000_000)).unwrap();
    exchange.accounts.deposit(user, "BTC", dec!(1_000)).unwrap();
}

fn place_limit(
    exchange: &Arc<Exchange>,
    user: Uuid,
    side: Side,
    price: rust_decimal::Decimal,
    qty: rust_decimal::Decimal,
) -> Uuid {
    let order_id = Uuid::new_v4();
    let cmd = PlaceOrderCommand {
        order_id,
        user_id: user,
        symbol: Symbol::new("BTCUSDT"),
        side,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::Gtc,
        price,
        qty,
        reduce_only: false,
        timestamp: chrono::Utc::now(),
        expire_at: None,
        position_idx: 0,
        iceberg_qty: None,
        order_link_id: None,
        slippage_tolerance: None,
        close_position: false,
        stp_mode: StpMode::default(),
        request_expire_time: None,
    };
    exchange
        .place_order(cmd)
        .expect("place_order must succeed in test setup");
    order_id
}

/// Backdate every entry in `pending_orders` that matches `predicate` by
/// `hours` hours, so prune-stale sees them as old.
fn backdate<F: Fn(&Uuid) -> bool>(exchange: &Arc<Exchange>, hours: i64, predicate: F) {
    for mut entry in exchange.pending_orders.iter_mut() {
        if predicate(entry.key()) {
            entry.value_mut().created_at = chrono::Utc::now() - chrono::Duration::hours(hours);
        }
    }
}

#[tokio::test]
async fn prune_stale_cancels_only_old_and_far_orders() {
    let exchange = Exchange::new();
    add_btc(&exchange);

    let maker_a = Uuid::nil(); // default authenticated user in tests
    let maker_b = Uuid::from_u128(0xB);
    fund(&exchange, maker_a);
    fund(&exchange, maker_b);

    // Build a two-sided book at mid ≈ 50_005 so we have a real mid-price.
    let _bbo_bid = place_limit(&exchange, maker_a, Side::Buy, dec!(50_000), dec!(0.1));
    let _bbo_ask = place_limit(&exchange, maker_b, Side::Sell, dec!(50_010), dec!(0.1));

    // 47_000 is ~6% below mid (≥ 5% threshold) — prunable when also old.
    let far_old = place_limit(&exchange, maker_a, Side::Buy, dec!(47_000), dec!(0.01));
    // 49_900 is ~0.2% below mid (< 5% threshold) — must survive even if old.
    let near_old = place_limit(&exchange, maker_a, Side::Buy, dec!(49_900), dec!(0.01));
    // 46_000 is far but NOT backdated — must survive the age filter.
    let far_fresh = place_limit(&exchange, maker_a, Side::Buy, dec!(46_000), dec!(0.01));

    // Backdate only the two "_old" orders; leave far_fresh and the BBO alone.
    backdate(&exchange, 25, |id| *id == far_old || *id == near_old);

    // Hit the endpoint.
    let router = build_router(exchange.clone());
    let body = serde_json::json!({
        "symbol": "BTCUSDT",
        "older_than_hours": 24,
        "distance_from_mid_pct": "5"
    });
    let req = Request::builder()
        .method("POST")
        .uri("/admin/orders/prune-stale")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "admin endpoint must return 200");

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        json["cancelled"].as_u64(),
        Some(1),
        "exactly one order should be cancelled (far_old), got: {json}"
    );
    assert_eq!(json["failed"].as_u64(), Some(0));

    // Verify: far_old is gone, near_old and far_fresh still resting.
    assert!(
        !exchange.pending_orders.contains_key(&far_old),
        "far_old must have been pruned"
    );
    assert!(
        exchange.pending_orders.contains_key(&near_old),
        "near_old must remain (inside distance threshold)"
    );
    assert!(
        exchange.pending_orders.contains_key(&far_fresh),
        "far_fresh must remain (inside age window)"
    );
}

#[tokio::test]
async fn prune_stale_refuses_when_no_two_sided_book() {
    let exchange = Exchange::new();
    add_btc(&exchange);

    let maker = Uuid::nil();
    fund(&exchange, maker);

    // Only one side of the book — there is no mid-price.
    place_limit(&exchange, maker, Side::Buy, dec!(50_000), dec!(0.1));

    let router = build_router(exchange.clone());
    let body = serde_json::json!({"symbol": "BTCUSDT"});
    let req = Request::builder()
        .method("POST")
        .uri("/admin/orders/prune-stale")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "must refuse to prune when mid-price cannot be computed"
    );
}
