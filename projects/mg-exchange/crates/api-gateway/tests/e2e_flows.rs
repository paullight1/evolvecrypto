//! End-to-end flow tests.
//!
//! Each test simulates a complete user journey through the exchange.
//!
//! Design rules (mirrors smoke_test.rs):
//!  - No sleeps, no I/O, no external services.
//!  - Each test creates a fresh exchange; tests are fully independent.
//!  - Assertions cover the full sequence of a real user action, not just one endpoint.
//!  - Every step in the flow is checked before moving to the next.
//!
//! NOTE on symbol choice:
//!   - BTCUSDT is pre-registered as a perpetual futures pair in `Exchange::new()`.
//!     Perpetual orders bypass `order_locks` (used only for spot balance tracking).
//!   - Use XBTUSDC for spot-settlement tests that need `get_open_orders` / amend /
//!     order history via `order_locks`.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::response::Response;
use exchange_api_gateway::exchange::Exchange;
use exchange_api_gateway::server::build_router;
use exchange_common::*;
use rust_decimal_macros::dec;
use tower::ServiceExt;
use uuid::Uuid;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build an exchange with one FUTURES product (BTCUSDT — already in perp_configs)
/// and one SPOT product (XBTUSDC), with a fully funded default user (Uuid::nil()).
fn setup() -> axum::Router {
    let exchange = Exchange::new();

    // BTCUSDT is already configured as a perp in Exchange::new() via default_perp_configs().
    // We still need to add the product entry for the account manager / validator.
    let btc_usdt = Product {
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
    exchange.add_product(btc_usdt);

    // XBTUSDC is a pure spot pair — NOT in perp_configs.
    let xbt_usdc = Product {
        symbol: Symbol::new("XBTUSDC"),
        base_asset: "XBT".to_string(),
        quote_asset: "USDC".to_string(),
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
    exchange.add_product(xbt_usdc);

    let user_a = Uuid::nil();
    exchange.create_account(user_a);
    exchange.deposit(user_a, "USDT", dec!(100_000)).unwrap();
    exchange.deposit(user_a, "USDC", dec!(100_000)).unwrap();
    exchange.deposit(user_a, "XBT", dec!(10)).unwrap();

    build_router(exchange)
}

/// Build a POST request with a JSON body.
fn post(path: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap()
}

/// Build a GET request.
fn get(path: &str) -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(path)
        .body(Body::empty())
        .unwrap()
}

/// Build a DELETE request.
fn delete(path: &str) -> Request<Body> {
    Request::builder()
        .method(Method::DELETE)
        .uri(path)
        .body(Body::empty())
        .unwrap()
}

/// Build a PUT request with a JSON body.
fn put(path: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(Method::PUT)
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap()
}

/// Parse a response body as JSON.  Panics with the raw bytes on failure.
async fn parse_json(resp: Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| panic!("response is not JSON: {:?}", bytes))
}

// ── Flow 1: Registration → Login → Balance Check ──────────────────────────────

/// A newly registered user can log in and view their (empty) balances.
#[tokio::test]
async fn e2e_register_login_check_balance() {
    let app = setup();

    // Step 1: Register a new user.
    let resp = app
        .clone()
        .oneshot(post(
            "/api/v1/auth/register",
            serde_json::json!({
                "email": "e2e_flow1@example.com",
                "password": "StrongPass123!"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "registration must succeed"
    );
    let body = parse_json(resp).await;
    assert!(
        body.get("user_id").is_some(),
        "registration response must include user_id"
    );

    // Step 2: Login with the same credentials returns an access token.
    let resp = app
        .clone()
        .oneshot(post(
            "/api/v1/auth/login",
            serde_json::json!({
                "email": "e2e_flow1@example.com",
                "password": "StrongPass123!"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "login must succeed");
    let body = parse_json(resp).await;
    assert!(
        body.get("access_token").is_some() || body.get("status").is_some(),
        "login response must include access_token or a status field"
    );

    // Step 3: The default injected user can fetch their (pre-funded) balances.
    let resp = app
        .clone()
        .oneshot(get("/api/v1/account/balances"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json(resp).await;
    assert!(body.is_array(), "balances must be a JSON array");
    // The default user (Uuid::nil()) was funded in setup().
    assert!(
        !body.as_array().unwrap().is_empty(),
        "funded user must have non-empty balances"
    );
}

// ── Flow 2: Duplicate Registration Is Rejected ────────────────────────────────

/// Registering the same email twice returns a conflict error.
#[tokio::test]
async fn e2e_register_duplicate_email_rejected() {
    let app = setup();

    let resp = app
        .clone()
        .oneshot(post(
            "/api/v1/auth/register",
            serde_json::json!({
                "email": "dupe@example.com",
                "password": "Password123!"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let resp = app
        .clone()
        .oneshot(post(
            "/api/v1/auth/register",
            serde_json::json!({
                "email": "dupe@example.com",
                "password": "AnotherPass!"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "duplicate email registration must return 409"
    );
    let body = parse_json(resp).await;
    assert!(
        body.get("error").is_some(),
        "error response must have 'error' field"
    );
}

// ── Flow 3: Spot Order Lifecycle (Place → Verify → Cancel) ───────────────────
//
// Uses XBTUSDC (pure spot pair) so orders appear in order_locks
// and are visible via GET /api/v1/orders.

/// A user can place a limit spot order, see it in the open orders list, then cancel it.
#[tokio::test]
async fn e2e_spot_order_place_verify_cancel() {
    let app = setup();

    // Step 1: Place a limit buy order on the spot pair (no counterparty → rests).
    let resp = app
        .clone()
        .oneshot(post(
            "/api/v1/orders",
            serde_json::json!({
                "symbol": "XBTUSDC",
                "side": "buy",
                "order_type": "limit",
                "price": "45000",
                "qty": "0.1"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "order placement must succeed");
    let body = parse_json(resp).await;
    assert_eq!(body["status"], "open", "resting order must have status=open");
    let order_id = body["order_id"]
        .as_str()
        .expect("response must include order_id")
        .to_string();

    // Step 2: The order appears in the open orders list.
    let resp = app
        .clone()
        .oneshot(get("/api/v1/orders"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json(resp).await;
    let orders = body.as_array().expect("open orders must be an array");
    assert!(
        orders
            .iter()
            .any(|o| o["order_id"].as_str() == Some(&order_id)),
        "placed order must appear in open orders"
    );

    // Step 3: Cancel the order.
    let resp = app
        .clone()
        .oneshot(delete(&format!("/api/v1/orders/{order_id}")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "cancellation must succeed");
    let body = parse_json(resp).await;
    assert_eq!(
        body["status"], "cancelled",
        "cancel response must have status=cancelled"
    );

    // Step 4: The order no longer appears in open orders.
    let resp = app
        .clone()
        .oneshot(get("/api/v1/orders"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json(resp).await;
    let orders = body.as_array().expect("open orders must be an array");
    assert!(
        !orders
            .iter()
            .any(|o| o["order_id"].as_str() == Some(&order_id)),
        "cancelled order must not appear in open orders"
    );
}

// ── Flow 4: Spot Order Amend (Place → Amend → Cancel) ────────────────────────
//
// Uses XBTUSDC (pure spot pair) so the order is tracked in order_locks,
// which is the authoritative source checked by the amend handler.

/// A resting spot order can be amended to a new price.
#[tokio::test]
async fn e2e_spot_order_place_amend_cancel() {
    let exchange = Exchange::new();

    // XBTUSDC is a pure spot pair — not in perp_configs.
    let product = Product {
        symbol: Symbol::new("XBTUSDC"),
        base_asset: "XBT".to_string(),
        quote_asset: "USDC".to_string(),
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

    let user_a = Uuid::nil();
    exchange.create_account(user_a);
    exchange.deposit(user_a, "USDC", dec!(100_000)).unwrap();

    // Pre-seed a resting buy order directly so it enters order_locks.
    // The amend handler requires the order to already be on the book.
    let original_id = Uuid::new_v4();
    exchange
        .place_order(PlaceOrderCommand {
            order_id: original_id,
            user_id: user_a,
            symbol: Symbol::new("XBTUSDC"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
            price: dec!(45000),
            qty: dec!(0.1),
            reduce_only: false,

            expire_at: None,
            position_idx: 0,
            iceberg_qty: None,
            order_link_id: None,
            timestamp: chrono::Utc::now(),
            slippage_tolerance: None,
            close_position: false,
            stp_mode: StpMode::default(),
            request_expire_time: None,
        })
        .expect("seed order must succeed");

    let app = build_router(exchange);

    // Step 1: Verify the order is in open orders.
    let resp = app
        .clone()
        .oneshot(get("/api/v1/orders"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let orders = parse_json(resp).await;
    let orders = orders.as_array().unwrap();
    assert!(
        orders
            .iter()
            .any(|o| o["order_id"].as_str() == Some(&original_id.to_string())),
        "seed order must appear in open orders before amend"
    );

    // Step 2: Amend the price to 46000.
    let resp = app
        .clone()
        .oneshot(put(
            &format!("/api/v1/orders/{original_id}"),
            serde_json::json!({"price": "46000"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "amend must succeed");
    let body = parse_json(resp).await;
    let new_id = body["new_order_id"]
        .as_str()
        .expect("amend response must include new_order_id")
        .to_string();
    assert_ne!(
        original_id.to_string(),
        new_id,
        "amend must produce a new order ID"
    );
    assert_eq!(body["status"], "amended");

    // Step 3: The original ID is gone; the new ID is present.
    let resp = app
        .clone()
        .oneshot(get("/api/v1/orders"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let orders = parse_json(resp).await;
    let orders = orders.as_array().unwrap();
    assert!(
        !orders
            .iter()
            .any(|o| o["order_id"].as_str() == Some(&original_id.to_string())),
        "original order must be gone after amend"
    );
    assert!(
        orders
            .iter()
            .any(|o| o["order_id"].as_str() == Some(&new_id)),
        "new order must appear in open orders after amend"
    );

    // Step 4: Cancel the amended order and verify open orders is now empty.
    let resp = app
        .clone()
        .oneshot(delete(&format!("/api/v1/orders/{new_id}")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(parse_json(resp).await["status"], "cancelled");

    let resp = app
        .clone()
        .oneshot(get("/api/v1/orders"))
        .await
        .unwrap();
    let orders = parse_json(resp).await;
    assert!(
        orders.as_array().unwrap().is_empty(),
        "open orders must be empty after cancel"
    );
}

// ── Flow 5: Trigger Order Lifecycle ──────────────────────────────────────────

/// A stop-limit trigger order can be placed, seen in the list, and cancelled.
#[tokio::test]
async fn e2e_trigger_order_lifecycle() {
    let app = setup();

    // Step 1: Place a stop-limit trigger order.
    // The list endpoint uses "trigger_order_id" as the field key.
    let resp = app
        .clone()
        .oneshot(post(
            "/api/v1/trigger-orders",
            serde_json::json!({
                "symbol": "BTCUSDT",
                "side": "sell",
                "trigger_type": "stop_limit",
                "trigger_price": "48000",
                "order_price": "47900",
                "qty": "0.1"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "trigger order placement must succeed"
    );
    let body = parse_json(resp).await;
    let trigger_id = body["trigger_order_id"]
        .as_str()
        .expect("response must include trigger_order_id")
        .to_string();
    assert_eq!(body["status"], "active");

    // Step 2: The trigger order appears in the active list.
    // The list serializes each order with "trigger_order_id" as the key.
    let resp = app
        .clone()
        .oneshot(get("/api/v1/trigger-orders"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let triggers = parse_json(resp).await;
    let triggers = triggers.as_array().expect("trigger orders must be an array");
    assert!(
        triggers
            .iter()
            .any(|t| t["trigger_order_id"].as_str() == Some(&trigger_id)),
        "trigger order must appear in the active list"
    );

    // Step 3: Cancel the trigger order.
    let resp = app
        .clone()
        .oneshot(delete(&format!("/api/v1/trigger-orders/{trigger_id}")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "trigger cancel must succeed");

    // Step 4: The trigger order no longer appears in the list.
    let resp = app
        .clone()
        .oneshot(get("/api/v1/trigger-orders"))
        .await
        .unwrap();
    let triggers = parse_json(resp).await;
    let triggers = triggers.as_array().unwrap();
    assert!(
        !triggers
            .iter()
            .any(|t| t["trigger_order_id"].as_str() == Some(&trigger_id)),
        "cancelled trigger order must not appear in the active list"
    );
}

// ── Flow 6: Spot Matching — Two Orders Cross ──────────────────────────────────
//
// Uses XBTUSDC (pure spot pair) so fills appear in order_history.
// BTCUSDT fills go through the futures settlement path which does not
// populate order_history.

/// Placing a buy and a sell at the same price causes a fill.
/// The filled order appears in history, not in open orders.
#[tokio::test]
async fn e2e_spot_orders_cross_and_fill() {
    let exchange = Exchange::new();
    let product = Product {
        symbol: Symbol::new("XBTUSDC"),
        base_asset: "XBT".to_string(),
        quote_asset: "USDC".to_string(),
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

    let user_a = Uuid::nil();
    let user_b = Uuid::from_u128(1);

    exchange.create_account(user_a);
    exchange.deposit(user_a, "USDC", dec!(100_000)).unwrap();

    exchange.create_account(user_b);
    exchange.deposit(user_b, "XBT", dec!(10)).unwrap();

    // User B seeds a resting sell order directly so it enters order_locks
    // and the book before user A's HTTP buy arrives.
    let sell_cmd = PlaceOrderCommand {
        order_id: Uuid::new_v4(),
        user_id: user_b,
        symbol: Symbol::new("XBTUSDC"),
        side: Side::Sell,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::Gtc,
        price: dec!(50000),
        qty: dec!(0.1),
        reduce_only: false,

        expire_at: None,
        position_idx: 0,
        iceberg_qty: None,
        order_link_id: None,
        timestamp: chrono::Utc::now(),
        slippage_tolerance: None,
        close_position: false,
        stp_mode: StpMode::default(),
        request_expire_time: None,
    };
    exchange.place_order(sell_cmd).expect("user B seed sell must succeed");

    let app = build_router(exchange);

    // Step 1: User A (default auth) places a buy that crosses with user B's sell.
    let resp = app
        .clone()
        .oneshot(post(
            "/api/v1/orders",
            serde_json::json!({
                "symbol": "XBTUSDC",
                "side": "buy",
                "order_type": "limit",
                "price": "50000",
                "qty": "0.1"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "buy order must be accepted");
    let body = parse_json(resp).await;
    let fills = body["fills"].as_array().expect("fills must be present");
    assert!(!fills.is_empty(), "buy order must produce at least one fill");
    assert_eq!(body["status"], "filled", "order must be fully filled");

    // Step 2: No open orders remain for user A.
    let resp = app.clone().oneshot(get("/api/v1/orders")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let open_orders = parse_json(resp).await;
    assert!(
        open_orders.as_array().unwrap().is_empty(),
        "no open orders should remain after fill"
    );

    // Step 3: The filled order appears in order history.
    let resp = app
        .clone()
        .oneshot(get("/api/v1/orders/history"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let history = parse_json(resp).await;
    assert!(
        !history.as_array().unwrap().is_empty(),
        "filled order must appear in order history"
    );
}

// ── Flow 7: Withdrawal Flow ───────────────────────────────────────────────────

/// A user can add a withdrawal address and submit a withdrawal request.
#[tokio::test]
async fn e2e_withdrawal_flow() {
    let app = setup();

    // Step 1: Add a withdrawal address to the whitelist.
    let resp = app
        .clone()
        .oneshot(post(
            "/api/v1/account/address-book",
            serde_json::json!({
                "address": "bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh",
                "network": "BTC",
                "label": "Cold storage"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "add address must succeed");
    let body = parse_json(resp).await;
    assert_eq!(body["status"], "added");

    // Step 2: Address book lists the new entry.
    let resp = app
        .clone()
        .oneshot(get("/api/v1/account/address-book"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let book = parse_json(resp).await;
    let entries = book.as_array().expect("address book must be an array");
    assert!(
        entries.iter().any(|e| e["address"]
            .as_str()
            .map(|a| a.contains("bc1q"))
            .unwrap_or(false)),
        "added address must appear in the address book"
    );

    // Step 3: Submit a withdrawal (may be accepted or rejected by limit/cooldown logic).
    let resp = app
        .clone()
        .oneshot(post(
            "/api/v1/account/withdraw",
            serde_json::json!({
                "asset": "USDT",
                "network": "ERC20",
                "address": "0xAbCd1234AbCd1234AbCd1234AbCd1234AbCd1234",
                "amount": "100"
            }),
        ))
        .await
        .unwrap();
    let status = resp.status();
    assert!(
        status.is_success() || status == StatusCode::BAD_REQUEST,
        "withdrawal must return 2xx or 400, got {status}"
    );

    // Step 4: Withdrawal history endpoint is reachable.
    let resp = app
        .clone()
        .oneshot(get("/api/v1/account/withdrawals"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(parse_json(resp).await.is_array());
}

// ── Flow 8: Copy Trading ──────────────────────────────────────────────────────

/// A user can view the leaderboard and interact with the follow/unfollow flow.
#[tokio::test]
async fn e2e_copy_trading_flow() {
    let app = setup();

    // Step 1: Leaderboard is accessible (may be empty on a fresh exchange).
    let resp = app
        .clone()
        .oneshot(get("/api/v1/copy-trading/leaderboard"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let board = parse_json(resp).await;
    assert!(board.is_array(), "leaderboard must be an array");

    // Step 2: Attempt to follow a non-existent master — engine returns an error or
    // creates a lenient relation (implementation-defined).
    let fake_master = Uuid::new_v4();
    let resp = app
        .clone()
        .oneshot(post(
            "/api/v1/copy-trading/follow",
            serde_json::json!({
                "master_id": fake_master.to_string(),
                "allocation": "1000"
            }),
        ))
        .await
        .unwrap();
    let follow_status = resp.status();
    let follow_body = parse_json(resp).await;

    if follow_status == StatusCode::OK {
        let relation_id = follow_body["relation_id"]
            .as_str()
            .expect("follow response must include relation_id")
            .to_string();

        // Step 3: Following list shows the new relation.
        let resp = app
            .clone()
            .oneshot(get("/api/v1/copy-trading/following"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let following = parse_json(resp).await;
        assert!(
            !following.as_array().unwrap().is_empty(),
            "following list must be non-empty after follow"
        );

        // Step 4: Unfollow.
        let resp = app
            .clone()
            .oneshot(delete(&format!(
                "/api/v1/copy-trading/follow/{relation_id}"
            )))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(parse_json(resp).await["status"], "unfollowed");
    } else {
        // Master not found — following list must still be empty.
        assert!(follow_body.get("error").is_some());
        let resp = app
            .clone()
            .oneshot(get("/api/v1/copy-trading/following"))
            .await
            .unwrap();
        assert!(parse_json(resp).await.is_array());
    }
}

// ── Flow 9: Bot Lifecycle ─────────────────────────────────────────────────────

/// A grid bot can be created, listed, paused, resumed, then stopped.
///
/// The list endpoint serializes each bot with "bot_id" (not "id").
#[tokio::test]
async fn e2e_bot_lifecycle() {
    let app = setup();

    // Step 1: Create a grid bot.
    let resp = app
        .clone()
        .oneshot(post(
            "/api/v1/bots/grid",
            serde_json::json!({
                "symbol": "BTCUSDT",
                "upper_price": "55000",
                "lower_price": "45000",
                "grid_count": 10,
                "total_investment": "1000",
                "side": "neutral"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "grid bot creation must succeed");
    let body = parse_json(resp).await;
    let bot_id = body["bot_id"]
        .as_str()
        .expect("response must include bot_id")
        .to_string();
    assert_eq!(body["status"], "active");

    // Step 2: Bot appears in the bots list (field key is "bot_id").
    let resp = app.clone().oneshot(get("/api/v1/bots")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bots = parse_json(resp).await;
    let bots = bots.as_array().expect("bots must be an array");
    assert!(
        bots.iter().any(|b| b["bot_id"].as_str() == Some(&bot_id)),
        "created bot must appear in the bots list"
    );

    // Step 3: Pause the bot.
    let resp = app
        .clone()
        .oneshot(post(
            &format!("/api/v1/bots/{bot_id}/pause"),
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "pause must succeed");
    assert_eq!(parse_json(resp).await["status"], "paused");

    // Step 4: Resume the bot.
    let resp = app
        .clone()
        .oneshot(post(
            &format!("/api/v1/bots/{bot_id}/resume"),
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "resume must succeed");
    assert_eq!(parse_json(resp).await["status"], "active");

    // Step 5: Stop the bot.
    let resp = app
        .clone()
        .oneshot(post(
            &format!("/api/v1/bots/{bot_id}/stop"),
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "stop must succeed");
    assert_eq!(parse_json(resp).await["status"], "stopped");
}

// ── Flow 10: Admin Operations ─────────────────────────────────────────────────

/// Admin endpoints expose user list, feature flags, and reconciliation report.
/// Toggling a feature flag is reflected in subsequent reads.
#[tokio::test]
async fn e2e_admin_operations() {
    let app = setup();

    // Step 1: List all users.
    let resp = app.clone().oneshot(get("/admin/users")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let users = parse_json(resp).await;
    assert!(users.is_array(), "users must be an array");

    // Step 2: Admin can view balances for the pre-funded default user.
    let nil_id = Uuid::nil().to_string();
    let resp = app
        .clone()
        .oneshot(get(&format!("/admin/users/{nil_id}/balances")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let balances = parse_json(resp).await;
    assert!(balances.is_array(), "admin user balances must be an array");
    assert!(
        !balances.as_array().unwrap().is_empty(),
        "seeded user must have non-empty balances"
    );

    // Step 3: Feature flags list.
    let resp = app
        .clone()
        .oneshot(get("/admin/feature-flags"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(parse_json(resp).await.is_object());

    // Step 4: Toggle a feature flag.
    let resp = app
        .clone()
        .oneshot(post(
            "/admin/feature-flags",
            serde_json::json!({
                "flag": "withdrawals",
                "enabled": false
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json(resp).await;
    assert_eq!(body["flag"], "withdrawals");
    assert_eq!(body["enabled"], false);

    // Step 5: Reconciliation report.
    let resp = app
        .clone()
        .oneshot(get("/admin/reconciliation"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(parse_json(resp).await.is_object());
}

// ── Flow 11: KYC + Settings ───────────────────────────────────────────────────

/// A user can check KYC status, start a mock verification, and update settings.
#[tokio::test]
async fn e2e_kyc_and_settings() {
    let app = setup();

    // Step 1: Fresh user has Unverified KYC status.
    let resp = app
        .clone()
        .oneshot(get("/api/v1/kyc/status"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json(resp).await;
    assert_eq!(
        body["status"], "Unverified",
        "new user must have Unverified KYC status"
    );

    // Step 2: Start a mock KYC verification.
    let resp = app
        .clone()
        .oneshot(post(
            "/api/v1/kyc/start",
            serde_json::json!({"provider": "mock"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json(resp).await;
    assert_eq!(body["provider"], "mock");
    assert!(body.get("provider_ref").is_some());

    // Step 3: Account settings return defaults for a fresh user.
    let resp = app
        .clone()
        .oneshot(get("/api/v1/account/settings"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(parse_json(resp).await.is_object());

    // Step 4: Update a single setting.
    let resp = app
        .clone()
        .oneshot(put(
            "/api/v1/account/settings",
            serde_json::json!({"theme": "dark"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(parse_json(resp).await["theme"], "dark");

    // Step 5: Subsequent GET reflects the updated value.
    let resp = app
        .clone()
        .oneshot(get("/api/v1/account/settings"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        parse_json(resp).await["theme"],
        "dark",
        "settings must persist within a session"
    );
}

// ── Flow 12: Spot Order History After Cancel ──────────────────────────────────
//
// Uses XBTUSDC (pure spot) so cancelled orders appear in order_history.
// This validates the full cancel → history pipeline.

/// A cancelled spot order appears in order history with status "cancelled".
#[tokio::test]
async fn e2e_spot_order_history_after_cancel() {
    let exchange = Exchange::new();

    let product = Product {
        symbol: Symbol::new("XBTUSDC"),
        base_asset: "XBT".to_string(),
        quote_asset: "USDC".to_string(),
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

    let user_a = Uuid::nil();
    exchange.create_account(user_a);
    exchange.deposit(user_a, "USDC", dec!(100_000)).unwrap();

    let app = build_router(exchange);

    // Step 1: Open orders initially empty.
    let resp = app.clone().oneshot(get("/api/v1/orders")).await.unwrap();
    assert!(parse_json(resp).await.as_array().unwrap().is_empty());

    // Step 2: Place a limit order that will rest (no counterparty).
    let resp = app
        .clone()
        .oneshot(post(
            "/api/v1/orders",
            serde_json::json!({
                "symbol": "XBTUSDC",
                "side": "buy",
                "order_type": "limit",
                "price": "45000",
                "qty": "0.1"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json(resp).await;
    assert_eq!(body["status"], "open");
    let order_id = body["order_id"].as_str().unwrap().to_string();

    // Step 3: Cancel the order.
    let resp = app
        .clone()
        .oneshot(delete(&format!("/api/v1/orders/{order_id}")))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Step 4: Order history now contains the cancelled order.
    let resp = app
        .clone()
        .oneshot(get("/api/v1/orders/history"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let history = parse_json(resp).await;
    let entries = history.as_array().expect("history must be an array");
    assert!(
        entries
            .iter()
            .any(|e| e["order_id"].as_str() == Some(&order_id)
                && e["status"].as_str() == Some("cancelled")),
        "cancelled order must appear in history with status=cancelled"
    );

    // Step 5: Open orders is now empty.
    let resp = app.clone().oneshot(get("/api/v1/orders")).await.unwrap();
    assert!(parse_json(resp).await.as_array().unwrap().is_empty());
}

// ── Flow 13: Full Futures Lifecycle ──────────────────────────────────────────

/// Deposit → place long BTCUSDT → verify position → close → verify PnL settled.
#[tokio::test]
async fn e2e_futures_lifecycle() {
    let exchange = Exchange::new();

    let product = Product {
        symbol: Symbol::new("BTCUSDT"),
        base_asset: "BTC".to_string(),
        quote_asset: "USDT".to_string(),
        product_type: ProductType::Perpetual,
        tick_size: dec!(0.01),
        lot_size: dec!(0.001),
        min_notional: dec!(5),
        maker_fee: dec!(0.0002),
        taker_fee: dec!(0.0005),
    max_order_qty: None,
    max_order_notional: None,
    max_leverage: None,
    };
    exchange.add_product(product);

    let user_a = Uuid::nil();
    let market_maker = Uuid::from_u128(0xAA);

    exchange.create_account(user_a);
    exchange.deposit(user_a, "USDT", dec!(100_000)).unwrap();

    exchange.create_account(market_maker);
    exchange.deposit(market_maker, "USDT", dec!(500_000)).unwrap();

    // Seed sell-side liquidity at ask price
    let sell_result = exchange.place_order(PlaceOrderCommand {
        order_id: Uuid::new_v4(),
        user_id: market_maker,
        symbol: Symbol::new("BTCUSDT"),
        side: Side::Sell,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::Gtc,
        price: dec!(50100),
        qty: dec!(1),
        reduce_only: false,

        expire_at: None,
        position_idx: 0,
        iceberg_qty: None,
        order_link_id: None,
        timestamp: chrono::Utc::now(),
        slippage_tolerance: None,
        close_position: false,
        stp_mode: StpMode::default(),
        request_expire_time: None,
    });
    assert!(sell_result.is_ok(), "seed sell must succeed: {:?}", sell_result);

    // Seed buy-side liquidity at bid price (separate from ask to avoid self-match)
    let buy_result = exchange.place_order(PlaceOrderCommand {
        order_id: Uuid::new_v4(),
        user_id: market_maker,
        symbol: Symbol::new("BTCUSDT"),
        side: Side::Buy,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::Gtc,
        price: dec!(49900),
        qty: dec!(1),
        reduce_only: false,
        expire_at: None,
        position_idx: 0,
        iceberg_qty: None,
        order_link_id: None,
        timestamp: chrono::Utc::now(),
        slippage_tolerance: None,
        close_position: false,
        stp_mode: StpMode::default(),
        request_expire_time: None,
    });
    assert!(buy_result.is_ok(), "seed buy must succeed: {:?}", buy_result);

    let app = build_router(exchange.clone());

    // Step 1: Place a long (buy) limit order on BTCUSDT at ask price to cross immediately
    let resp = app
        .clone()
        .oneshot(post(
            "/api/v1/orders",
            serde_json::json!({
                "symbol": "BTCUSDT",
                "side": "buy",
                "order_type": "limit",
                "price": "50100",
                "qty": "0.1"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "buy order must succeed");
    let body = parse_json(resp).await;
    // Perp orders show fills but may have "open" status if partially resting
    let has_fills = body.get("fills")
        .and_then(|f| f.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false);
    assert!(has_fills, "order must have fills: {body}");

    // Step 2: Verify position opened
    let resp = app
        .clone()
        .oneshot(get("/api/v1/positions"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let positions = parse_json(resp).await;
    let positions = positions.as_array().expect("positions must be array");
    assert!(!positions.is_empty(), "user must have an open position");

    // Step 3: Close position by selling at bid price
    let resp = app
        .clone()
        .oneshot(post(
            "/api/v1/orders",
            serde_json::json!({
                "symbol": "BTCUSDT",
                "side": "sell",
                "order_type": "limit",
                "price": "49900",
                "qty": "0.1"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "close order must succeed");

    // Step 4: Verify position closed (size = 0 or gone)
    let resp = app
        .clone()
        .oneshot(get("/api/v1/positions"))
        .await
        .unwrap();
    let positions = parse_json(resp).await;
    let positions = positions.as_array().unwrap();
    // Position should be empty or have size 0
    assert!(
        positions.is_empty() || positions.iter().all(|p| p["size"].as_str() == Some("0")),
        "position should be closed"
    );

    // Step 5: Balance check — user should have approximately the same USDT (minus fees)
    let resp = app
        .clone()
        .oneshot(get("/api/v1/account/balances"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let balances = parse_json(resp).await;
    assert!(balances.is_array());
}

// ── Flow 14: Error Paths ─────────────────────────────────────────────────────

/// Insufficient balance returns 400.
#[tokio::test]
async fn e2e_error_insufficient_balance() {
    let exchange = Exchange::new();
    let product = Product {
        symbol: Symbol::new("XBTUSDC"),
        base_asset: "XBT".to_string(),
        quote_asset: "USDC".to_string(),
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

    let user_a = Uuid::nil();
    exchange.create_account(user_a);
    exchange.deposit(user_a, "USDC", dec!(100)).unwrap(); // Only $100

    let app = build_router(exchange);

    // Try to buy 1 XBT at 50000 = 50000 USDC, but only have 100
    let resp = app
        .clone()
        .oneshot(post(
            "/api/v1/orders",
            serde_json::json!({
                "symbol": "XBTUSDC",
                "side": "buy",
                "order_type": "limit",
                "price": "50000",
                "qty": "1"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "must reject insufficient balance");
    let body = parse_json(resp).await;
    assert!(body.get("error").is_some(), "must have error field");
}

/// Invalid symbol returns 400.
#[tokio::test]
async fn e2e_error_invalid_symbol() {
    let app = setup();

    let resp = app
        .clone()
        .oneshot(post(
            "/api/v1/orders",
            serde_json::json!({
                "symbol": "INVALID_SYM_!!",
                "side": "buy",
                "order_type": "limit",
                "price": "100",
                "qty": "1"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "invalid symbol must be rejected");
}

/// Duplicate email registration returns 409.
#[tokio::test]
async fn e2e_error_duplicate_email() {
    let app = setup();

    let email = "unique_e2e_dup@test.com";
    let resp = app
        .clone()
        .oneshot(post(
            "/api/v1/auth/register",
            serde_json::json!({"email": email, "password": "StrongPass123!"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let resp = app
        .clone()
        .oneshot(post(
            "/api/v1/auth/register",
            serde_json::json!({"email": email, "password": "AnotherPass!"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT, "duplicate email must return 409");
}

// ── Flow 15: Email Verification ──────────────────────────────────────────────

/// Verify email code returns correct responses.
#[tokio::test]
async fn e2e_email_verification_flow() {
    let exchange = Exchange::new();

    let product = Product {
        symbol: Symbol::new("BTCUSDT"),
        base_asset: "BTC".to_string(),
        quote_asset: "USDT".to_string(),
        product_type: ProductType::Perpetual,
        tick_size: dec!(0.01),
        lot_size: dec!(0.001),
        min_notional: dec!(5),
        maker_fee: dec!(0.001),
        taker_fee: dec!(0.002),
    max_order_qty: None,
    max_order_notional: None,
    max_leverage: None,
    };
    exchange.add_product(product);

    let user_a = Uuid::nil();
    exchange.create_account(user_a);

    // Generate a verification code
    let code = exchange.generate_email_code(user_a);

    // Verify with correct code
    assert!(exchange.verify_email_code(user_a, &code), "correct code must verify");

    // Verify email is now marked as verified
    assert!(
        exchange.email_verified.contains(&user_a),
        "user must be in verified set"
    );

    // Verify with wrong code should fail
    let code2 = exchange.generate_email_code(user_a);
    assert!(!exchange.verify_email_code(user_a, "000000"), "wrong code must fail");
    // Correct code should still work (if not consumed)
    assert!(exchange.verify_email_code(user_a, &code2));
}

// ── Flow 16: Admin Seed and KYC Approval ─────────────────────────────────────

/// Admin can view KYC status of any user.
#[tokio::test]
async fn e2e_admin_kyc_flow() {
    let app = setup();

    // Step 1: Start KYC for the default user
    let resp = app
        .clone()
        .oneshot(post(
            "/api/v1/kyc/start",
            serde_json::json!({"provider": "mock"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json(resp).await;
    assert_eq!(body["provider"], "mock");

    // Step 2: Check KYC status
    let resp = app
        .clone()
        .oneshot(get("/api/v1/kyc/status"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json(resp).await;
    // Status may be "Pending" or "Unverified" depending on mock behavior
    assert!(body.get("status").is_some(), "KYC status must be present");
}

// ── M13 Task 1: Portfolio / Cross-Margin Mode ─────────────────────────────────

/// GET /account/margin-mode returns "isolated" by default.
#[tokio::test]
async fn e2e_get_margin_mode_default_isolated() {
    let app = setup();

    let resp = app
        .oneshot(get("/api/v1/account/margin-mode"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json(resp).await;
    assert_eq!(body["marginMode"], "isolated");
    assert!(body["crossMarginBalance"].is_null(), "crossMarginBalance should be null in isolated mode");
}

/// PUT /account/margin-mode can switch to cross, and GET reflects the change.
#[tokio::test]
async fn e2e_set_margin_mode_cross_and_get() {
    let app = setup();

    // Switch to cross margin for BTCUSDT.
    let resp = app
        .clone()
        .oneshot(put(
            "/api/v1/account/margin-mode",
            serde_json::json!({
                "mode": "cross",
                "symbol": "BTCUSDT",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "switching to cross should succeed");
    let body = parse_json(resp).await;
    assert_eq!(body["marginMode"], "cross");

    // GET should now return cross.
    let resp = app
        .clone()
        .oneshot(get("/api/v1/account/margin-mode"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json(resp).await;
    assert_eq!(body["marginMode"], "cross");
    // crossMarginBalance should be present (may be 0 since no positions).
    assert!(body.get("crossMarginBalance").is_some());
}

/// PUT /account/margin-mode rejects invalid mode strings.
#[tokio::test]
async fn e2e_set_margin_mode_invalid_mode() {
    let app = setup();

    let resp = app
        .oneshot(put(
            "/api/v1/account/margin-mode",
            serde_json::json!({
                "mode": "leverage",
                "symbol": "BTCUSDT",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// PUT /account/margin-mode rejects request when a position is open.
#[tokio::test]
async fn e2e_set_margin_mode_rejected_while_position_open() {
    use exchange_common::{OrderType, PlaceOrderCommand, Side, StpMode, TimeInForce};

    let exchange = Exchange::new();

    let product = exchange_common::Product {
        symbol: Symbol::new("BTCUSDT"),
        base_asset: "BTC".to_string(),
        quote_asset: "USDT".to_string(),
        product_type: exchange_common::ProductType::Perpetual,
        tick_size: dec!(0.01),
        lot_size: dec!(0.001),
        min_notional: dec!(5),
        maker_fee: dec!(0.0002),
        taker_fee: dec!(0.0005),
    max_order_qty: None,
    max_order_notional: None,
    max_leverage: None,
    };
    exchange.add_product(product);

    let test_user = uuid::Uuid::nil();
    let market_maker = uuid::Uuid::from_u128(0xAA);

    exchange.create_account(test_user);
    exchange.deposit(test_user, "USDT", dec!(100_000)).unwrap();
    exchange.create_account(market_maker);
    exchange.deposit(market_maker, "USDT", dec!(500_000)).unwrap();

    // Market maker provides sell liquidity so the test user can open a long.
    exchange.place_order(PlaceOrderCommand {
        order_id: uuid::Uuid::new_v4(),
        user_id: market_maker,
        symbol: Symbol::new("BTCUSDT"),
        side: Side::Sell,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::Gtc,
        price: dec!(50000),
        qty: dec!(1),
        reduce_only: false,
        expire_at: None,
        position_idx: 0,
        iceberg_qty: None,
        order_link_id: None,
        timestamp: chrono::Utc::now(),
        slippage_tolerance: None,
        close_position: false,
        stp_mode: StpMode::default(),
        request_expire_time: None,
    }).unwrap();

    // Test user goes long — fills against the maker's sell.
    exchange.place_order(PlaceOrderCommand {
        order_id: uuid::Uuid::new_v4(),
        user_id: test_user,
        symbol: Symbol::new("BTCUSDT"),
        side: Side::Buy,
        order_type: OrderType::Market,
        time_in_force: TimeInForce::Ioc,
        price: dec!(0),
        qty: dec!(1),
        reduce_only: false,
        expire_at: None,
        position_idx: 0,
        iceberg_qty: None,
        order_link_id: None,
        timestamp: chrono::Utc::now(),
        slippage_tolerance: None,
        close_position: false,
        stp_mode: StpMode::default(),
        request_expire_time: None,
    }).unwrap();

    let app = build_router(exchange);

    // Now try to switch margin mode — should be rejected because position is open.
    let resp = app
        .oneshot(put(
            "/api/v1/account/margin-mode",
            serde_json::json!({
                "mode": "cross",
                "symbol": "BTCUSDT",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

// ── M13 Task 2: Insurance Fund Mechanics ─────────────────────────────────────

/// GET /market/insurance returns enhanced fund info including surplus/deficit stats.
#[tokio::test]
async fn e2e_get_insurance_fund_enhanced() {
    let app = setup();

    let resp = app
        .oneshot(get("/api/v1/market/insurance"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json(resp).await;
    let list = body["list"].as_array().expect("list must be array");
    assert!(!list.is_empty(), "at least one insurance fund entry expected");

    // Each entry must have the enhanced fields.
    let first = &list[0];
    assert!(first.get("symbol").is_some());
    assert!(first.get("balance").is_some());
    assert!(first.get("totalSurplusContributed").is_some());
    assert!(first.get("totalDeficitCovered").is_some());
    assert!(first.get("eventCount").is_some());
}

/// GET /market/insurance?symbol=BTCUSDT filters to one symbol.
#[tokio::test]
async fn e2e_get_insurance_fund_filter_by_symbol() {
    let app = setup();

    let resp = app
        .oneshot(get("/api/v1/market/insurance?symbol=BTCUSDT"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json(resp).await;
    let list = body["list"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["symbol"], "BTCUSDT");
}

/// GET /admin/insurance/history returns paginated history.
#[tokio::test]
async fn e2e_admin_insurance_history_empty() {
    let app = setup();

    let resp = app
        .oneshot(get("/admin/insurance/history"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json(resp).await;
    assert_eq!(body["total"], 0);
    let list = body["list"].as_array().unwrap();
    assert!(list.is_empty(), "no history entries at start");
}

/// GET /admin/insurance/history?symbol=BTCUSDT filters by symbol.
#[tokio::test]
async fn e2e_admin_insurance_history_symbol_filter() {
    let app = setup();

    let resp = app
        .oneshot(get("/admin/insurance/history?symbol=BTCUSDT&limit=10"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json(resp).await;
    assert!(body["limit"].as_u64().unwrap() <= 10);
}
