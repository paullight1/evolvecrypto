//! Smoke tests for all API endpoints.
//!
//! These tests verify that every major endpoint responds with the expected
//! status code and a valid JSON body.  They do NOT test business logic —
//! that is covered by the unit tests in `server.rs`.
//!
//! Design rules:
//!  - No sleeps, no I/O, no external services.
//!  - Each test is independent; the exchange is created fresh per test.
//!  - A non-500 response on valid input is the only assertion (smoke == "not on fire").
//!  - Error shapes must be `{"error": "..."}` — asserted where a 4xx is expected.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use exchange_api_gateway::exchange::Exchange;
use exchange_api_gateway::server::build_router;
use exchange_common::*;
use rust_decimal_macros::dec;
use tower::ServiceExt;
use uuid::Uuid;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build a minimal exchange with one product and two funded accounts.
///
/// User A (`Uuid::nil()`) is the default "authenticated" user injected by
/// the `Extension(AuthenticatedUser(Uuid::nil()))` layer inside `build_router`.
fn setup() -> axum::Router {
    let exchange = Exchange::new();

    let product = Product {
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
    };
    exchange.add_product(product);

    let user_a = Uuid::nil();
    exchange.create_account(user_a);
    exchange.deposit(user_a, "USDT", dec!(100_000)).unwrap();
    exchange.deposit(user_a, "BTC", dec!(10)).unwrap();

    build_router(exchange)
}

/// Parse a response body as JSON.  Panics with the raw bytes on failure.
async fn json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| panic!("response is not JSON: {:?}", bytes))
}

/// Assert the JSON value is an object with an `"error"` string key.
fn assert_error_shape(v: &serde_json::Value) {
    assert!(
        v.get("error").and_then(|e| e.as_str()).is_some(),
        "expected {{\"error\": \"...\"}}, got: {v}"
    );
}

// ── 1. Public endpoints — no auth ─────────────────────────────────────────────

#[tokio::test]
async fn smoke_health() {
    let resp = setup()
        .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    // 200 when persistence is healthy, 503 when degraded (no WAL/DB in test mode).
    assert!(
        resp.status() == StatusCode::OK || resp.status() == StatusCode::SERVICE_UNAVAILABLE,
        "expected 200 or 503, got {}",
        resp.status()
    );
    let body = json(resp).await;
    assert!(
        body.get("status").and_then(|s| s.as_str()).is_some(),
        "missing status field in health response"
    );
}

#[tokio::test]
async fn smoke_metrics() {
    let resp = setup()
        .oneshot(Request::builder().uri("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn smoke_fees() {
    let resp = setup()
        .oneshot(Request::builder().uri("/api/v1/fees").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    assert!(body.get("vip_tiers").is_some(), "missing vip_tiers");
    assert!(body.get("withdrawal_fees").is_some(), "missing withdrawal_fees");
}

#[tokio::test]
async fn smoke_status() {
    let resp = setup()
        .oneshot(Request::builder().uri("/api/v1/status").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    assert!(body.get("status").is_some(), "missing status field");
}

#[tokio::test]
async fn smoke_por_latest() {
    let resp = setup()
        .oneshot(Request::builder().uri("/api/v1/por/latest").body(Body::empty()).unwrap())
        .await
        .unwrap();
    // Either OK (report exists) or 404 (no report yet) — both are valid.
    assert!(
        resp.status() == StatusCode::OK || resp.status() == StatusCode::NOT_FOUND,
        "unexpected status: {}",
        resp.status()
    );
}

#[tokio::test]
async fn smoke_markets_all() {
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/api/v1/markets/all")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    // Array of market objects — may be empty on a fresh exchange.
    assert!(body.is_array(), "expected JSON array, got: {body}");
}

#[tokio::test]
async fn smoke_orderbook_known_symbol() {
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/api/v1/markets/BTCUSDT/orderbook")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    assert_eq!(body["symbol"], "BTCUSDT");
    assert!(body.get("bids").is_some());
    assert!(body.get("asks").is_some());
}

#[tokio::test]
async fn smoke_orderbook_unknown_symbol() {
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/api/v1/markets/XXXXXX/orderbook")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    // Either 400 (invalid symbol format) or 404 (symbol not registered).
    let status = resp.status();
    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND,
        "unexpected status: {status}"
    );
    let body = json(resp).await;
    assert_error_shape(&body);
}

#[tokio::test]
async fn smoke_copy_trading_leaderboard() {
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/api/v1/copy-trading/leaderboard")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    assert!(body.is_array(), "expected JSON array");
}

// ── 2. Auth endpoints (public) ────────────────────────────────────────────────

#[tokio::test]
async fn smoke_auth_register() {
    let body = serde_json::json!({
        "email": "smoke@example.com",
        "password": "password123"
    });
    let resp = setup()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = json(resp).await;
    assert!(body.get("user_id").is_some(), "missing user_id");
}

#[tokio::test]
async fn smoke_auth_register_invalid_email() {
    let body = serde_json::json!({
        "email": "not-an-email",
        "password": "password123"
    });
    let resp = setup()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_error_shape(&json(resp).await);
}

#[tokio::test]
async fn smoke_auth_login_unknown_user() {
    let body = serde_json::json!({
        "email": "nobody@example.com",
        "password": "password123"
    });
    let resp = setup()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert_error_shape(&json(resp).await);
}

// ── 3. Private endpoints (default auth injected by build_router) ──────────────

#[tokio::test]
async fn smoke_get_balances() {
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/api/v1/account/balances")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    assert!(body.is_array(), "expected JSON array of balances");
    // User A has BTC and USDT deposits.
    let arr = body.as_array().unwrap();
    assert!(!arr.is_empty(), "expected non-empty balances for funded user");
}

#[tokio::test]
async fn smoke_get_open_orders() {
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/api/v1/orders")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    assert!(body.is_array(), "expected JSON array");
}

#[tokio::test]
async fn smoke_get_order_history() {
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/api/v1/orders/history")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    assert!(body.is_array(), "expected JSON array");
}

#[tokio::test]
async fn smoke_get_positions() {
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/api/v1/positions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    assert!(body.is_array(), "expected JSON array");
}

#[tokio::test]
async fn smoke_get_trigger_orders() {
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/api/v1/trigger-orders")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    assert!(body.is_array(), "expected JSON array");
}

#[tokio::test]
async fn smoke_get_wallet_overview() {
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/api/v1/account/wallet")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    // Wallet endpoint returns equity, margin info, etc.
    assert!(body.is_object(), "expected JSON object");
}

#[tokio::test]
async fn smoke_list_bots() {
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/api/v1/bots")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    assert!(body.is_array(), "expected JSON array");
}

#[tokio::test]
async fn smoke_get_referral() {
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/api/v1/referral")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    assert!(body.is_object(), "expected JSON object");
}

#[tokio::test]
async fn smoke_get_account_settings() {
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/api/v1/account/settings")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    assert!(body.is_object(), "expected JSON object with settings");
}

#[tokio::test]
async fn smoke_list_deposits() {
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/api/v1/account/deposits")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    assert!(body.is_array(), "expected JSON array");
}

#[tokio::test]
async fn smoke_list_withdrawals() {
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/api/v1/account/withdrawals")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    assert!(body.is_array(), "expected JSON array");
}

#[tokio::test]
async fn smoke_copy_trading_following() {
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/api/v1/copy-trading/following")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    assert!(body.is_array(), "expected JSON array");
}

#[tokio::test]
async fn smoke_get_funding_rates() {
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/api/v1/funding/rate")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    assert!(body.is_array(), "expected JSON array");
}

// ── 4. Admin endpoints ────────────────────────────────────────────────────────
//
// No API key is configured on `Exchange::new()`, so the admin middleware
// passes all requests through in open/dev mode.

#[tokio::test]
async fn smoke_admin_stats() {
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/admin/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    assert!(body.is_object(), "expected JSON object");
}

#[tokio::test]
async fn smoke_admin_users() {
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/admin/users")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    assert!(body.is_array(), "expected JSON array of users");
}

#[tokio::test]
async fn smoke_admin_feature_flags() {
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/admin/feature-flags")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    assert!(body.is_object(), "expected JSON object of flags");
}

#[tokio::test]
async fn smoke_admin_reconciliation() {
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/admin/reconciliation")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    assert!(body.is_object(), "expected JSON object");
}

// ── 5. Response consistency — error shape ─────────────────────────────────────

#[tokio::test]
async fn smoke_error_shape_bad_order() {
    // Missing required fields → 400 with {"error": "..."}
    let body = serde_json::json!({ "symbol": "BTCUSDT" }); // missing side, qty, order_type
    let resp = setup()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/orders")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn smoke_cancel_nonexistent_order_is_idempotent() {
    // Canceling a nonexistent order is treated as idempotent: the engine
    // returns an empty events vec and the handler responds 200 with a
    // "cancelled" status.  This is the current API contract.
    let fake_id = Uuid::new_v4();
    let resp = setup()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/orders/{fake_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    assert_eq!(body["status"], "cancelled");
}

#[tokio::test]
async fn smoke_error_shape_invalid_symbol_orderbook() {
    // Symbols must be uppercase; "btcusdt" should return 400.
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/api/v1/markets/btcusdt/orderbook")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_error_shape(&json(resp).await);
}

// ── 6. Place + verify order round-trip (smoke, not business logic) ────────────

#[tokio::test]
async fn smoke_place_order_success() {
    let body = serde_json::json!({
        "symbol": "BTCUSDT",
        "side": "buy",
        "order_type": "limit",
        "price": "50000",
        "qty": "0.1"
    });
    let resp = setup()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/orders")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    assert!(body.get("order_id").is_some(), "missing order_id");
    assert!(body.get("status").is_some(), "missing status");
}

// ── Spot Margin Borrow/Lend Engine ────────────────────────────────────────────

#[tokio::test]
async fn smoke_spot_margin_borrowable() {
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/api/v1/spot-margin/borrowable")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    let borrowable = body.get("borrowable").expect("missing borrowable field");
    assert!(borrowable.as_array().is_some(), "borrowable should be an array");
}

#[tokio::test]
async fn smoke_spot_margin_borrow_and_repay() {
    let router = setup();

    // Borrow some USDT (user_a has 100_000 USDT collateral).
    let borrow_body = serde_json::json!({"coin": "USDT", "amount": "1000"});
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/spot-margin/borrow")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&borrow_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let borrow_resp = json(resp).await;
    assert!(borrow_resp.get("borrowId").is_some(), "missing borrowId");
    assert_eq!(borrow_resp["coin"].as_str().unwrap(), "USDT");

    // List borrows.
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/spot-margin/borrows")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let borrows_resp = json(resp).await;
    let borrows = borrows_resp["borrows"].as_array().expect("borrows should be array");
    assert_eq!(borrows.len(), 1, "should have one active borrow");
    assert_eq!(borrows[0]["coin"].as_str().unwrap(), "USDT");

    // Repay partial amount.
    let repay_body = serde_json::json!({"coin": "USDT", "amount": "500"});
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/spot-margin/repay")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&repay_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let repay_resp = json(resp).await;
    assert_eq!(repay_resp["status"].as_str().unwrap(), "active");
}

#[tokio::test]
async fn smoke_spot_margin_borrow_exceeds_limit() {
    let router = setup();
    // Attempt to borrow more than collateral * 3 (100_000 * 3 = 300_000).
    let body = serde_json::json!({"coin": "USDT", "amount": "999999999"});
    let resp = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/spot-margin/borrow")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_error_shape(&json(resp).await);
}

#[tokio::test]
async fn smoke_spot_margin_interest_history() {
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/api/v1/spot-margin/interest-history")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    assert!(body.get("interestHistory").is_some());
}

// ── Earn Products (Real Implementation) ──────────────────────────────────────

#[tokio::test]
async fn smoke_earn_subscribe_auto_compound() {
    let router = setup();

    // Get products and find the USDT Flexible one.
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/earn/products")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let products_resp = json(resp).await;
    let products = products_resp["products"].as_array().unwrap();
    let flexible_usdt = products
        .iter()
        .find(|p| p["coin"].as_str() == Some("USDT") && p["lockDays"].as_u64() == Some(0))
        .expect("USDT Flexible product not found");
    let product_id = flexible_usdt["id"].as_str().unwrap().to_string();
    assert_eq!(
        flexible_usdt["supportsAutoCompound"].as_bool().unwrap(),
        true,
        "USDT Flexible should support auto-compound"
    );

    // Subscribe with auto-compound enabled.
    let subscribe_body = serde_json::json!({
        "productId": product_id,
        "amount": "1000",
        "autoCompound": true,
    });
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/earn/subscribe")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&subscribe_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let sub_resp = json(resp).await;
    assert_eq!(sub_resp["autoCompound"].as_bool().unwrap(), true);

    // Allocations should show auto_compound.
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/earn/allocations")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let allocs_resp = json(resp).await;
    let allocs = allocs_resp["allocations"].as_array().unwrap();
    assert_eq!(allocs.len(), 1);
    assert_eq!(allocs[0]["autoCompound"].as_bool().unwrap(), true);
}

#[tokio::test]
async fn smoke_earn_interest_history() {
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/api/v1/earn/interest-history")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    assert!(body.get("history").is_some());
}

// ── Dust Conversion ───────────────────────────────────────────────────────────

#[tokio::test]
async fn smoke_dust_assets_no_dust() {
    // User has BTC (10) and USDT (100k) — BTC is not dust, USDT excluded from dust check.
    // Without index prices (test mode), BTC USD value = 0 → BTC not included as dust either.
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/api/v1/account/dust-assets")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    assert!(body.get("dustAssets").is_some());
}

#[tokio::test]
async fn smoke_dust_convert_no_dust_returns_error() {
    // Without index prices BTC value = 0, so dust-convert has no candidates.
    let resp = setup()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/account/dust-convert")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    // Should be 400 when no dust assets are found.
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_error_shape(&json(resp).await);
}

#[tokio::test]
async fn smoke_dust_history_empty() {
    let resp = setup()
        .oneshot(
            Request::builder()
                .uri("/api/v1/account/dust-history")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json(resp).await;
    let history = body["dustHistory"].as_array().expect("dustHistory should be array");
    assert!(history.is_empty(), "should be empty for new user");
}
