//! Integration tests for the OperatingMode-based graceful degradation.
//!
//! MiFID II RTS 6 requires the kill switch to be testable and the test logged.
//! These tests verify that mode transitions correctly gate order placement and
//! cancellation at the HTTP layer.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use exchange_api_gateway::exchange::Exchange;
use exchange_api_gateway::server::build_router;
use exchange_common::*;
use rust_decimal_macros::dec;
use std::sync::Arc;
use tower::ServiceExt;

fn setup() -> (Arc<Exchange>, axum::Router) {
    let exchange = Exchange::new();

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

    let user = uuid::Uuid::nil();
    exchange.accounts.create_account(user);
    exchange.accounts.deposit(user, "USDT", dec!(100_000)).unwrap();

    let router = build_router(exchange.clone());
    (exchange, router)
}

fn place_order_req() -> Request<Body> {
    let body = serde_json::json!({
        "symbol": "BTCUSDT",
        "side": "Buy",
        "order_type": "Limit",
        "price": "50000",
        "qty": "0.001",
        "time_in_force": "GTC",
    });
    Request::builder()
        .method("POST")
        .uri("/api/v1/orders")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn cancel_order_req(order_id: &str) -> Request<Body> {
    Request::builder()
        .method("DELETE")
        .uri(&format!("/api/v1/orders/{order_id}"))
        .body(Body::empty())
        .unwrap()
}

fn set_mode_req(mode: &str) -> Request<Body> {
    let body = serde_json::json!({"mode": mode});
    Request::builder()
        .method("POST")
        .uri("/admin/mode")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn halt_req() -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/admin/halt")
        .body(Body::empty())
        .unwrap()
}

fn resume_req() -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/admin/resume")
        .body(Body::empty())
        .unwrap()
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}

// ── Test 1: halt rejects orders, resume allows them ─────────────────────────

#[tokio::test]
async fn test_halt_rejects_orders_resume_allows() {
    let (_ex, router) = setup();

    // Normal: order should succeed.
    let resp = router.clone().oneshot(place_order_req()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "order should succeed in Normal mode");

    // Halt.
    let resp = router.clone().oneshot(halt_req()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["status"], "Halted");

    // Order should be rejected.
    let resp = router.clone().oneshot(place_order_req()).await.unwrap();
    assert_ne!(resp.status(), StatusCode::OK, "order should be rejected when halted");

    // Resume.
    let resp = router.clone().oneshot(resume_req()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Order should succeed again.
    let resp = router.clone().oneshot(place_order_req()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "order should succeed after resume");
}

// ── Test 2: CancelOnly mode — orders rejected, cancels accepted ─────────────

#[tokio::test]
async fn test_cancel_only_mode() {
    let (_ex, router) = setup();

    // Place an order in Normal mode so we have something to cancel.
    let resp = router.clone().oneshot(place_order_req()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    let order_id = json["order_id"].as_str().unwrap_or("").to_string();
    assert!(!order_id.is_empty(), "order_id should be returned");

    // Switch to CancelOnly.
    let resp = router.clone().oneshot(set_mode_req("CancelOnly")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["mode"], "CancelOnly");

    // New order should be rejected.
    let resp = router.clone().oneshot(place_order_req()).await.unwrap();
    assert_ne!(resp.status(), StatusCode::OK, "new order should be rejected in CancelOnly");

    // Cancel should succeed.
    let resp = router.clone().oneshot(cancel_order_req(&order_id)).await.unwrap();
    // Cancel may return 200 (cancelled) or 400 (already filled/not found) — but NOT rejected for mode.
    // The key assertion: it should not return the mode-rejection error.
    let json = body_json(resp).await;
    let err_msg = json["error"].as_str().unwrap_or("");
    assert!(
        !err_msg.contains("mode"),
        "cancel should not be rejected for operating mode in CancelOnly: {err_msg}",
    );

    // Resume to Normal.
    let resp = router.clone().oneshot(set_mode_req("Normal")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Order should succeed again.
    let resp = router.clone().oneshot(place_order_req()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// ── Test 3: Halted mode rejects cancels too ─────────────────────────────────

#[tokio::test]
async fn test_halted_rejects_cancels() {
    let (_ex, router) = setup();

    // Place an order.
    let resp = router.clone().oneshot(place_order_req()).await.unwrap();
    let json = body_json(resp).await;
    let order_id = json["order_id"].as_str().unwrap_or("").to_string();

    // Halt.
    router.clone().oneshot(halt_req()).await.unwrap();

    // Cancel should be rejected in Halted mode.
    let resp = router.clone().oneshot(cancel_order_req(&order_id)).await.unwrap();
    let status = resp.status();
    let json = body_json(resp).await;
    let err_msg = json["error"].as_str().unwrap_or("");
    assert!(
        err_msg.contains("Halted") || err_msg.contains("mode") || status != StatusCode::OK,
        "cancel should be rejected in Halted mode, got status={status} body={json}",
    );
}

// ── Test 4: /admin/mode rejects invalid mode string ─────────────────────────

#[tokio::test]
async fn test_invalid_mode_rejected() {
    let (_ex, router) = setup();

    let resp = router.clone().oneshot(set_mode_req("SuperMode")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
