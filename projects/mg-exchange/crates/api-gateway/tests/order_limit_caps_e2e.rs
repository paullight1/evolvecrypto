/// End-to-end integration tests for per-user open-order caps (Sprint 8, KG-2).
///
/// These tests use a real `Exchange` (including engine threads) to verify:
///   - The per-symbol cap is enforced end-to-end (REST → exchange → engine).
///   - Rejected orders return HTTP 429 (not 400).
///   - After cancelling one order, the next placement succeeds.
///   - The per-price-level cap works with the real engine's per-level index.
///   - Counter consistency after fills (maker order leaves the book).
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use exchange_api_gateway::exchange::Exchange;
use exchange_api_gateway::server::build_router;
use exchange_common::*;
use exchange_order_gateway::OrderLimitConfig;
use uuid::Uuid;
use rust_decimal_macros::dec;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
    }
}

/// Build a test router with a 3-order-per-symbol, 2-order-per-price-level cap.
///
/// Returns both the exchange Arc (for direct engine queries) and the router
/// (for REST calls).  The order_limit_config is set via `Arc::get_mut` before
/// the router takes a second reference — no unsafe needed.
fn setup_with_low_caps() -> (Arc<Exchange>, axum::Router) {
    let mut exchange = Exchange::new();

    // `Exchange::new()` returns `Arc<Self>`.  Since we have the only reference,
    // `Arc::get_mut` succeeds and lets us configure the caps before sharing.
    {
        let ex_mut = Arc::get_mut(&mut exchange)
            .expect("exclusive Arc reference required for test setup");
        ex_mut.order_limit_config = OrderLimitConfig {
            max_open_orders_per_user_per_symbol: 3,
            max_orders_per_user_per_price_level: 2,
            // Set a high total cap so existing tests are not affected by
            // the KG-7 cross-symbol check (they operate on one symbol only).
            max_open_orders_per_user_total: 1000,
        };
    }

    exchange.add_product(btc_product());

    let user_a = uuid::Uuid::nil();
    exchange.create_account(user_a);
    exchange.deposit(user_a, "USDT", dec!(1_000_000)).unwrap();
    exchange.deposit(user_a, "BTC", dec!(100)).unwrap();

    let router = build_router(Arc::clone(&exchange));
    (exchange, router)
}

/// Place a single limit order and return the response.
async fn place_limit_order(
    router: &axum::Router,
    price: &str,
    qty: &str,
) -> axum::response::Response {
    let body = serde_json::json!({
        "symbol": "BTCUSDT",
        "side": "buy",
        "order_type": "limit",
        "price": price,
        "qty": qty
    });
    router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/orders")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap()
}

/// Cancel an order by ID and return the response.
async fn cancel_order(router: &axum::Router, order_id: &str) -> axum::response::Response {
    router
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/orders/{order_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

/// Parse a response body as JSON.
async fn json_body(resp: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| panic!("response is not JSON: {:?}", bytes))
}

// ---------------------------------------------------------------------------
// Test 1: 4th order on same symbol returns 429
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_symbol_cap_429_on_excess() {
    let (_, router) = setup_with_low_caps(); // cap = 3 per symbol

    // Place 3 orders at different prices (each succeeds).
    for i in 1..=3u32 {
        let price = format!("{}.00", 50000 + i);
        let resp = place_limit_order(&router, &price, "0.001").await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "order {i} should succeed (cap not reached yet)"
        );
    }

    // 4th order must return 429.
    let resp = place_limit_order(&router, "50005.00", "0.001").await;
    assert_eq!(
        resp.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "4th order must return 429"
    );

    let body = json_body(resp).await;
    let error_msg = body["error"].as_str().unwrap_or("");
    assert!(
        error_msg.contains("Max open orders per symbol"),
        "error message must describe the cap: {error_msg}"
    );
    assert!(
        error_msg.contains("3"),
        "error message must include the limit value: {error_msg}"
    );
    assert!(
        body.get("retCode").and_then(|r| r.as_u64()) == Some(429),
        "body must have retCode=429"
    );
}

// ---------------------------------------------------------------------------
// Test 2: After cancel, next placement succeeds
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_symbol_cap_accept_after_cancel() {
    let (_, router) = setup_with_low_caps(); // cap = 3

    let mut order_ids: Vec<String> = Vec::new();

    // Fill up to cap.
    for i in 1..=3u32 {
        let price = format!("{}.00", 50000 + i);
        let resp = place_limit_order(&router, &price, "0.001").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = json_body(resp).await;
        let oid = body["order_id"].as_str().unwrap().to_string();
        order_ids.push(oid);
    }

    // 4th must fail.
    let fail_resp = place_limit_order(&router, "50010.00", "0.001").await;
    assert_eq!(fail_resp.status(), StatusCode::TOO_MANY_REQUESTS);

    // Cancel the first order.
    let cancel_resp = cancel_order(&router, &order_ids[0]).await;
    assert_eq!(cancel_resp.status(), StatusCode::OK, "cancel must succeed");

    // Now 4th placement should succeed (count is 2, below cap of 3).
    let retry_resp = place_limit_order(&router, "50010.00", "0.001").await;
    assert_eq!(
        retry_resp.status(),
        StatusCode::OK,
        "placement after cancel must succeed"
    );
}

// ---------------------------------------------------------------------------
// Test 3: Per-price-level cap — 3rd order at same price returns 429
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_price_level_cap_429() {
    let (_, router) = setup_with_low_caps(); // price-level cap = 2

    // Two orders at the same price succeed.
    for _ in 0..2u32 {
        let resp = place_limit_order(&router, "50000.00", "0.001").await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "first two orders at same price must succeed"
        );
    }

    // Third order at the SAME price must return 429.
    let resp = place_limit_order(&router, "50000.00", "0.001").await;
    assert_eq!(
        resp.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "3rd order at same price must return 429"
    );

    let body = json_body(resp).await;
    let error_msg = body["error"].as_str().unwrap_or("");
    assert!(
        error_msg.contains("Max orders per price level"),
        "error must mention price level cap: {error_msg}"
    );
}

// ---------------------------------------------------------------------------
// Test 4: Different price levels are independent
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_price_level_cap_independent_per_price() {
    let (_, router) = setup_with_low_caps(); // price-level cap = 2, symbol cap = 3

    // Two orders at $50001.
    for _ in 0..2u32 {
        let resp = place_limit_order(&router, "50001.00", "0.001").await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // Order at $50002 (different price) should succeed — different level quota.
    // Symbol cap is 3, so this 3rd order is still under symbol cap too.
    let resp = place_limit_order(&router, "50002.00", "0.001").await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "order at different price level must not be blocked by another level's cap"
    );
}

// ---------------------------------------------------------------------------
// Test 5: Counter consistency after fill (engine-level query)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_symbol_cap_decrements_on_fill() {
    let (exchange, _router) = setup_with_low_caps(); // symbol cap = 3
    let symbol = Symbol::new("BTCUSDT");
    let buyer = uuid::Uuid::nil();

    // Place 3 buy limit orders directly through the engine pool.
    {
        let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
        for i in 0..3u32 {
            let price = dec!(50000) + rust_decimal::Decimal::from(i);
            pool.process(
                &symbol,
                Command::PlaceOrder(PlaceOrderCommand {
                    order_id: uuid::Uuid::new_v4(),
                    user_id: buyer,
                    symbol: symbol.clone(),
                    side: Side::Buy,
                    order_type: OrderType::Limit,
                    time_in_force: TimeInForce::Gtc,
                    price,
                    qty: dec!(0.001),
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
                }),
            )
            .unwrap();
        }

        // Engine must report 3 open orders for buyer.
        let count = pool.user_open_order_count(&symbol, buyer).unwrap();
        assert_eq!(count, 3, "engine should have 3 open orders for buyer before fill");

        // Crossing sell that fills the best bid (highest buy = $50002).
        pool.process(
            &symbol,
            Command::PlaceOrder(PlaceOrderCommand {
                order_id: uuid::Uuid::new_v4(),
                user_id: uuid::Uuid::new_v4(), // different seller
                symbol: symbol.clone(),
                side: Side::Sell,
                order_type: OrderType::Market,
                time_in_force: TimeInForce::Gtc,
                price: dec!(0),
                qty: dec!(0.001),
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
            }),
        )
        .unwrap();

        // After fill, buyer should have 2 open orders.
        let count_after = pool.user_open_order_count(&symbol, buyer).unwrap();
        assert_eq!(count_after, 2, "engine should have 2 open orders after one fill");
    }
}

// ---------------------------------------------------------------------------
// Task 6 — E2E: cross-symbol total cap (KG-7, Sprint 10)
// ---------------------------------------------------------------------------

fn eth_product() -> Product {
    Product {
        symbol: Symbol::new("ETHUSDT"),
        base_asset: "ETH".to_string(),
        quote_asset: "USDT".to_string(),
        product_type: ProductType::Perpetual,
        tick_size: dec!(0.01),
        lot_size: dec!(0.001),
        min_notional: dec!(10),
        maker_fee: dec!(0.001),
        taker_fee: dec!(0.002),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
    }
}

/// Build a two-symbol exchange with a tight total cap for KG-7 testing.
///
/// Configuration:
///   - BTC and ETH symbols registered.
///   - `max_open_orders_per_user_per_symbol` = 10 (high enough to not interfere).
///   - `max_open_orders_per_user_total` = 2 (tight cap to keep test fast).
fn setup_two_symbols_total_cap_2() -> (Arc<Exchange>, axum::Router) {
    let mut exchange = Exchange::new();

    {
        let ex_mut = Arc::get_mut(&mut exchange)
            .expect("exclusive Arc reference required for test setup");
        ex_mut.order_limit_config = OrderLimitConfig {
            max_open_orders_per_user_per_symbol: 10,
            max_orders_per_user_per_price_level: 50,
            max_open_orders_per_user_total: 2,
        };
    }

    exchange.add_product(btc_product());
    exchange.add_product(eth_product());

    let user_a = uuid::Uuid::nil();
    exchange.create_account(user_a);
    exchange.deposit(user_a, "USDT", dec!(1_000_000)).unwrap();
    exchange.deposit(user_a, "BTC", dec!(100)).unwrap();
    exchange.deposit(user_a, "ETH", dec!(10_000)).unwrap();

    let router = build_router(Arc::clone(&exchange));
    (exchange, router)
}

/// Place a limit order on any symbol and return the response.
async fn place_limit_order_on(
    router: &axum::Router,
    symbol: &str,
    price: &str,
    qty: &str,
) -> axum::response::Response {
    let body = serde_json::json!({
        "symbol": symbol,
        "side": "buy",
        "order_type": "limit",
        "price": price,
        "qty": qty
    });
    router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/orders")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap()
}

/// KG-7: total cap across symbols — 3rd order (on any symbol) must return 429
/// with `MaxOpenOrdersPerUserTotal` when `max_open_orders_per_user_total` = 2.
///
/// Test plan:
///   1. Place 1 order on BTCUSDT → OK (total = 1).
///   2. Place 1 order on ETHUSDT → OK (total = 2).
///   3. Place any further order (BTCUSDT or ETHUSDT) → 429 MaxOpenOrdersPerUserTotal.
#[tokio::test]
async fn test_total_cap_across_symbols_rejects_at_429() {
    let (_, router) = setup_two_symbols_total_cap_2();

    // Step 1: one BTC order — under total cap.
    let resp = place_limit_order_on(&router, "BTCUSDT", "50000.00", "0.001").await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "first order (BTCUSDT) must succeed — total = 1"
    );

    // Step 2: one ETH order — reaches total cap.
    let resp = place_limit_order_on(&router, "ETHUSDT", "3000.00", "0.01").await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "second order (ETHUSDT) must succeed — total = 2"
    );

    // Step 3: any further order must be rejected (total = 2 = cap).
    let resp = place_limit_order_on(&router, "BTCUSDT", "50001.00", "0.001").await;
    assert_eq!(
        resp.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "third order must return 429 — total cap reached"
    );

    let body = json_body(resp).await;
    let error_msg = body["error"].as_str().unwrap_or("");
    assert!(
        error_msg.contains("Max total open orders per user"),
        "error must describe the total cap: {error_msg}"
    );
    assert!(
        error_msg.contains("2"),
        "error must include the limit value: {error_msg}"
    );
    assert!(
        body.get("retCode").and_then(|r| r.as_u64()) == Some(429),
        "body must have retCode=429"
    );
}

// ---------------------------------------------------------------------------
// KG-8 (Sprint 12): per-user OrderLimitConfig overrides
// ---------------------------------------------------------------------------

/// Per-user override lifts the cap: user with override can exceed the
/// default per-symbol cap. Places 5 orders at distinct prices — without the
/// override these would fail at the 4th (default cap is 3), but with an
/// override of 10 they all succeed.
#[tokio::test]
async fn test_user_override_allows_higher_per_symbol_cap() {
    let user_a = Uuid::nil();
    let (_exchange, router) = setup_with_override(
        user_a,
        OrderLimitConfig {
            max_open_orders_per_user_per_symbol: 10,
            max_orders_per_user_per_price_level: 2,
            max_open_orders_per_user_total: 1000,
        },
    );

    // Place 5 orders at distinct prices — all must succeed under the override.
    for i in 0..5 {
        let price = format!("{}.00", 50000 + i);
        let resp = place_limit_order(&router, &price, "0.001").await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "order {i} for user with override must succeed (override cap = 10)"
        );
    }
}

/// `effective_order_limit_config()` returns the override when set, and the
/// default config for users without an override. Clear-override reverts.
#[test]
fn test_effective_config_returns_override_when_set() {
    let mut exchange = Exchange::new();
    let user_a = Uuid::nil();
    let override_cfg = OrderLimitConfig {
        max_open_orders_per_user_per_symbol: 999,
        max_orders_per_user_per_price_level: 99,
        max_open_orders_per_user_total: 9999,
    };
    {
        let ex_mut = Arc::get_mut(&mut exchange).expect("exclusive Arc");
        ex_mut.set_user_order_limit_override(user_a, override_cfg.clone());
    }

    // Default user (not user_a) sees the default config (cap = 500).
    let default_cfg = exchange.effective_order_limit_config(Uuid::from_u128(0xBEEF));
    assert_eq!(default_cfg.max_open_orders_per_user_per_symbol, 500);

    // user_a sees the override.
    let user_a_cfg = exchange.effective_order_limit_config(user_a);
    assert_eq!(user_a_cfg.max_open_orders_per_user_per_symbol, 999);
    assert_eq!(user_a_cfg.max_orders_per_user_per_price_level, 99);
    assert_eq!(user_a_cfg.max_open_orders_per_user_total, 9999);

    // Clear override; user_a reverts to default.
    {
        let ex_mut = Arc::get_mut(&mut exchange).expect("exclusive Arc");
        ex_mut.clear_user_order_limit_override(user_a);
    }
    let user_a_cfg_after = exchange.effective_order_limit_config(user_a);
    assert_eq!(user_a_cfg_after.max_open_orders_per_user_per_symbol, 500);
}

/// Helper: build an exchange with a per-user override pre-configured.
fn setup_with_override(
    user_id: Uuid,
    override_cfg: OrderLimitConfig,
) -> (Arc<Exchange>, axum::Router) {
    let mut exchange = Exchange::new();
    {
        let ex_mut = Arc::get_mut(&mut exchange)
            .expect("exclusive Arc reference required for test setup");
        ex_mut.order_limit_config = OrderLimitConfig {
            max_open_orders_per_user_per_symbol: 3,
            max_orders_per_user_per_price_level: 2,
            max_open_orders_per_user_total: 1000,
        };
        ex_mut.set_user_order_limit_override(user_id, override_cfg);
    }

    exchange.add_product(btc_product());
    exchange.create_account(user_id);
    exchange.deposit(user_id, "USDT", dec!(1_000_000)).unwrap();
    exchange.deposit(user_id, "BTC", dec!(100)).unwrap();

    let router = build_router(Arc::clone(&exchange));
    (exchange, router)
}
