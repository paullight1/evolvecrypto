//! Cross-collateral haircut system tests.
//!
//! Covers:
//! - `effective_collateral_usd`: correct haircut application, max-cap, price lookup
//! - `cross_margin_balance`: collateral + uPnL
//! - User collateral-coin toggle (PUT /account/collateral-coin)
//! - Collateral info endpoint (GET /account/collateral-info)
//! - Admin config endpoints (GET/PUT /admin/collateral/configs)

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::response::Response;
use exchange_api_gateway::exchange::Exchange;
use exchange_api_gateway::server::build_router;
use exchange_common::*;
use rust_decimal_macros::dec;
use tower::ServiceExt;
use uuid::Uuid;

// ── Test helpers ──────────────────────────────────────────────────────────────

fn setup() -> axum::Router {
    let exchange = Exchange::new();

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

    let user = Uuid::nil();
    exchange.create_account(user);
    // Deposit several assets — will be used as multi-asset collateral.
    exchange.deposit(user, "USDT", dec!(10_000)).unwrap();
    exchange.deposit(user, "BTC", dec!(1)).unwrap();
    exchange.deposit(user, "ETH", dec!(5)).unwrap();

    build_router(exchange)
}

fn get(path: &str) -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(path)
        .body(Body::empty())
        .unwrap()
}

fn put(path: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(Method::PUT)
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap()
}

async fn parse_json(resp: Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| panic!("response is not JSON: {:?}", bytes))
}

// ── Unit-level: effective_collateral_usd ─────────────────────────────────────

/// USDT is a stablecoin with haircut 1.0 — 10 000 USDT should contribute
/// exactly 10 000 USD of effective collateral.
#[test]
fn effective_collateral_usdt_no_discount() {
    let exchange = Exchange::new();
    let user = Uuid::new_v4();
    exchange.create_account(user);
    exchange.deposit(user, "USDT", dec!(10_000)).unwrap();

    let collateral = exchange.effective_collateral_usd(user);
    assert_eq!(collateral, dec!(10_000));
}

/// USDC is also a stablecoin (haircut 1.0).
#[test]
fn effective_collateral_usdc_no_discount() {
    let exchange = Exchange::new();
    let user = Uuid::new_v4();
    exchange.create_account(user);
    exchange.deposit(user, "USDC", dec!(5_000)).unwrap();

    let collateral = exchange.effective_collateral_usd(user);
    assert_eq!(collateral, dec!(5_000));
}

/// BTC haircut is 0.95. Without an index price the BTC contribution should be
/// zero (conservative fallback). Only USDT contributes.
#[test]
fn effective_collateral_btc_no_index_price_excluded() {
    let exchange = Exchange::new();
    let user = Uuid::new_v4();
    exchange.create_account(user);
    exchange.deposit(user, "USDT", dec!(1_000)).unwrap();
    exchange.deposit(user, "BTC", dec!(2)).unwrap();

    // No index price feed → BTC is excluded, only USDT counts.
    let collateral = exchange.effective_collateral_usd(user);
    assert_eq!(collateral, dec!(1_000));
}

/// After feeding an index price for BTCUSDT, BTC should contribute
/// `balance * price * haircut = 2 * 50000 * 0.95 = 95000`.
#[test]
fn effective_collateral_btc_with_index_price() {
    let exchange = Exchange::new();
    let user = Uuid::new_v4();
    exchange.create_account(user);
    exchange.deposit(user, "BTC", dec!(2)).unwrap();

    // Feed three sources so the aggregator has quorum.
    // `IndexPriceAggregator::with_defaults` uses `min_sources = 3`
    // (raised from 2 in commit 68072a7 as DDoS hardening — an attacker
    // who knocks out 2 of 4 sources must not control the median).
    let btcusdt = exchange_common::Symbol::new("BTCUSDT");
    let now = chrono::Utc::now();
    {
        let mut agg = exchange
            .index_aggregator
            .lock().unwrap_or_else(|e| e.into_inner());
        agg.update_source_price(&btcusdt, "Binance", dec!(50_000), now);
        agg.update_source_price(&btcusdt, "Coinbase", dec!(50_000), now);
        agg.update_source_price(&btcusdt, "Kraken", dec!(50_000), now);
    }

    let collateral = exchange.effective_collateral_usd(user);
    // 2 BTC * 50 000 * 0.95 = 95 000
    assert_eq!(collateral, dec!(95000.00));
}

/// Multi-asset: USDT + BTC + ETH all contribute with their respective haircuts.
#[test]
fn effective_collateral_multi_asset() {
    let exchange = Exchange::new();
    let user = Uuid::new_v4();
    exchange.create_account(user);
    exchange.deposit(user, "USDT", dec!(10_000)).unwrap();
    exchange.deposit(user, "BTC", dec!(1)).unwrap();
    exchange.deposit(user, "ETH", dec!(10)).unwrap();

    let now = chrono::Utc::now();
    {
        let mut agg = exchange
            .index_aggregator
            .lock().unwrap_or_else(|e| e.into_inner());
        let btcusdt = exchange_common::Symbol::new("BTCUSDT");
        let ethusdt = exchange_common::Symbol::new("ETHUSDT");
        // Register ETH so the aggregator tracks it.
        agg.add_symbol(ethusdt.clone());
        // Three sources required for quorum — see min_sources=3 note above.
        agg.update_source_price(&btcusdt, "Binance", dec!(60_000), now);
        agg.update_source_price(&btcusdt, "Coinbase", dec!(60_000), now);
        agg.update_source_price(&btcusdt, "Kraken", dec!(60_000), now);
        agg.update_source_price(&ethusdt, "Binance", dec!(3_000), now);
        agg.update_source_price(&ethusdt, "Coinbase", dec!(3_000), now);
        agg.update_source_price(&ethusdt, "Kraken", dec!(3_000), now);
    }

    let collateral = exchange.effective_collateral_usd(user);
    // USDT: 10 000 * 1.0  * 1.0  = 10 000
    // BTC:  1     * 60000 * 0.95 = 57 000
    // ETH:  10    * 3000  * 0.90 = 27 000
    // Total = 94 000
    assert_eq!(collateral, dec!(94000));
}

/// max_collateral_amount cap: only the capped portion is counted.
#[test]
fn effective_collateral_max_cap_applied() {
    let exchange = Exchange::new();
    let user = Uuid::new_v4();
    exchange.create_account(user);
    exchange.deposit(user, "BTC", dec!(10)).unwrap();

    let now = chrono::Utc::now();
    {
        let btcusdt = exchange_common::Symbol::new("BTCUSDT");
        let mut agg = exchange
            .index_aggregator
            .lock().unwrap_or_else(|e| e.into_inner());
        // Three sources required for quorum — see min_sources=3 note above.
        agg.update_source_price(&btcusdt, "Binance", dec!(50_000), now);
        agg.update_source_price(&btcusdt, "Coinbase", dec!(50_000), now);
        agg.update_source_price(&btcusdt, "Kraken", dec!(50_000), now);
    }
    // Set a cap of 2 BTC on the BTC collateral config.
    {
        let mut configs = exchange
            .collateral_configs
            .lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cfg) = configs.get_mut("BTC") {
            cfg.max_collateral_amount = Some(dec!(2));
        }
    }

    let collateral = exchange.effective_collateral_usd(user);
    // Only 2 BTC counted (cap), not 10.  2 * 50 000 * 0.95 = 95 000
    assert_eq!(collateral, dec!(95_000));
}

/// Ineligible coins are entirely excluded from collateral.
#[test]
fn effective_collateral_ineligible_coin_excluded() {
    let exchange = Exchange::new();
    let user = Uuid::new_v4();
    exchange.create_account(user);
    exchange.deposit(user, "USDT", dec!(5_000)).unwrap();

    // Mark USDT ineligible.
    {
        let mut configs = exchange
            .collateral_configs
            .lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cfg) = configs.get_mut("USDT") {
            cfg.is_eligible = false;
        }
    }

    let collateral = exchange.effective_collateral_usd(user);
    assert_eq!(collateral, dec!(0));
}

/// User disables a specific coin — that coin no longer contributes.
#[test]
fn effective_collateral_user_disabled_coin_excluded() {
    let exchange = Exchange::new();
    let user = Uuid::new_v4();
    exchange.create_account(user);
    exchange.deposit(user, "USDT", dec!(5_000)).unwrap();
    exchange.deposit(user, "USDC", dec!(3_000)).unwrap();

    // Disable USDC for this user while keeping USDT enabled.
    {
        let mut settings = exchange
            .user_collateral_settings
            .lock().unwrap_or_else(|e| e.into_inner());
        let mut enabled = std::collections::HashSet::new();
        enabled.insert("USDT".to_string());
        // USDC is NOT inserted — effectively disabled.
        settings.insert(user, enabled);
    }

    let collateral = exchange.effective_collateral_usd(user);
    // Only USDT (5 000 * 1.0 * 1.0 = 5 000) counted; USDC excluded.
    assert_eq!(collateral, dec!(5_000));
}

/// cross_margin_balance = effective_collateral + uPnL (uPnL=0 when no positions).
#[test]
fn cross_margin_balance_equals_collateral_without_positions() {
    let exchange = Exchange::new();
    let user = Uuid::new_v4();
    exchange.create_account(user);
    exchange.deposit(user, "USDT", dec!(8_000)).unwrap();

    let cmb = exchange.cross_margin_balance(user);
    assert_eq!(cmb, dec!(8_000));
}

// ── HTTP: PUT /api/v1/account/collateral-coin ─────────────────────────────────

/// Enabling a known eligible coin succeeds with 200 and the coin is recorded.
#[tokio::test]
async fn http_set_collateral_coin_enable() {
    let app = setup();
    let resp = app
        .oneshot(put(
            "/api/v1/account/collateral-coin",
            serde_json::json!({"coin": "BTC", "enabled": true}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json(resp).await;
    assert_eq!(body["coin"].as_str().unwrap(), "BTC");
    assert_eq!(body["enabled"].as_bool().unwrap(), true);
}

/// Disabling a coin succeeds.
#[tokio::test]
async fn http_set_collateral_coin_disable() {
    let app = setup();
    let resp = app
        .oneshot(put(
            "/api/v1/account/collateral-coin",
            serde_json::json!({"coin": "ETH", "enabled": false}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json(resp).await;
    assert_eq!(body["coin"].as_str().unwrap(), "ETH");
    assert_eq!(body["enabled"].as_bool().unwrap(), false);
}

/// Unknown coin returns 400.
#[tokio::test]
async fn http_set_collateral_coin_unknown_coin() {
    let app = setup();
    let resp = app
        .oneshot(put(
            "/api/v1/account/collateral-coin",
            serde_json::json!({"coin": "FAKECOIN99", "enabled": true}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = parse_json(resp).await;
    assert!(body["error"].as_str().is_some());
}

/// Missing `enabled` field returns 400.
#[tokio::test]
async fn http_set_collateral_coin_missing_enabled() {
    let app = setup();
    let resp = app
        .oneshot(put(
            "/api/v1/account/collateral-coin",
            serde_json::json!({"coin": "BTC"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// Missing `coin` field returns 400.
#[tokio::test]
async fn http_set_collateral_coin_missing_coin() {
    let app = setup();
    let resp = app
        .oneshot(put(
            "/api/v1/account/collateral-coin",
            serde_json::json!({"enabled": true}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// Ineligible coin (after marking it ineligible via admin) returns 400.
#[tokio::test]
async fn http_set_collateral_coin_ineligible() {
    let exchange = Exchange::new();
    let user = Uuid::nil();
    exchange.create_account(user);
    exchange.deposit(user, "USDT", dec!(1_000)).unwrap();

    // Mark DOGE ineligible.
    {
        let mut configs = exchange
            .collateral_configs
            .lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cfg) = configs.get_mut("DOGE") {
            cfg.is_eligible = false;
        }
    }

    let app = build_router(exchange);
    let resp = app
        .oneshot(put(
            "/api/v1/account/collateral-coin",
            serde_json::json!({"coin": "DOGE", "enabled": true}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = parse_json(resp).await;
    assert!(
        body["error"]
            .as_str()
            .unwrap_or("")
            .contains("not currently eligible"),
        "error message should mention eligibility"
    );
}

// ── HTTP: GET /api/v1/account/collateral-info ─────────────────────────────────

/// The collateral-info endpoint returns a list of eligible coins and a total.
#[tokio::test]
async fn http_get_collateral_info_structure() {
    let app = setup();
    let resp = app
        .oneshot(get("/api/v1/account/collateral-info"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = parse_json(resp).await;
    assert!(
        body["coins"].is_array(),
        "response must include 'coins' array"
    );
    assert!(
        body["totalEffectiveCollateralUsd"].is_string(),
        "response must include 'totalEffectiveCollateralUsd' as string"
    );

    let coins = body["coins"].as_array().unwrap();
    // Default config has 10 eligible assets.
    assert!(!coins.is_empty(), "coins array must not be empty");

    // Verify each entry has the expected keys.
    for coin in coins {
        assert!(coin["coin"].is_string());
        assert!(coin["haircutRate"].is_string());
        assert!(coin["availableBalance"].is_string());
        assert!(coin["effectiveCollateral"].is_string());
        assert!(coin["enabled"].is_boolean());
    }
}

/// USDT available balance contributes 1:1 to effectiveCollateral.
#[tokio::test]
async fn http_get_collateral_info_usdt_value() {
    let app = setup();
    let resp = app
        .oneshot(get("/api/v1/account/collateral-info"))
        .await
        .unwrap();
    let body = parse_json(resp).await;

    let coins = body["coins"].as_array().unwrap();
    let usdt = coins.iter().find(|c| c["coin"] == "USDT").expect("USDT must appear");

    // We deposited 10 000 USDT in setup().
    let available: rust_decimal::Decimal = usdt["availableBalance"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(available, dec!(10_000));

    // haircut = 1.0 so effective == available (usd price = 1).
    let effective: rust_decimal::Decimal = usdt["effectiveCollateral"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(effective, dec!(10_000));
    // Haircut for USDT is 1.0 (stored as Decimal::new(100,2) → serialises as "1.00").
    let haircut: rust_decimal::Decimal = usdt["haircutRate"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(haircut, dec!(1));
}

/// After disabling ETH via PUT, the collateral-info endpoint shows it as disabled
/// and its effectiveCollateral is zero.
#[tokio::test]
async fn http_get_collateral_info_disabled_coin_is_zero() {
    let app = setup();

    // Disable ETH.
    app.clone()
        .oneshot(put(
            "/api/v1/account/collateral-coin",
            serde_json::json!({"coin": "ETH", "enabled": false}),
        ))
        .await
        .unwrap();

    // Now fetch collateral info.
    let resp = app
        .oneshot(get("/api/v1/account/collateral-info"))
        .await
        .unwrap();
    let body = parse_json(resp).await;

    let coins = body["coins"].as_array().unwrap();
    let eth = coins.iter().find(|c| c["coin"] == "ETH").expect("ETH must appear");
    assert_eq!(eth["enabled"].as_bool().unwrap(), false);
    let effective: rust_decimal::Decimal = eth["effectiveCollateral"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(effective, dec!(0));
}

// ── HTTP: Admin collateral config endpoints ───────────────────────────────────

/// GET /admin/collateral/configs returns all configs sorted by coin.
#[tokio::test]
async fn http_admin_list_collateral_configs() {
    let app = setup();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/admin/collateral/configs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = parse_json(resp).await;
    let configs = body["configs"].as_array().unwrap();
    assert!(!configs.is_empty());

    // Must be alphabetically sorted.
    let names: Vec<&str> = configs
        .iter()
        .map(|c| c["coin"].as_str().unwrap())
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "coins must be alphabetically sorted");

    // Each entry has the required fields.
    for cfg in configs {
        assert!(cfg["coin"].is_string());
        assert!(cfg["haircutRate"].is_string());
        assert!(cfg["isEligible"].is_boolean());
    }
}

/// PUT /admin/collateral/configs/{coin} — update haircut rate.
#[tokio::test]
async fn http_admin_update_collateral_config_haircut() {
    let app = setup();
    let resp = app
        .clone()
        .oneshot(put(
            "/admin/collateral/configs/BTC",
            serde_json::json!({"haircutRate": "0.88"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json(resp).await;
    assert_eq!(body["coin"].as_str().unwrap(), "BTC");
    assert_eq!(body["haircutRate"].as_str().unwrap(), "0.88");

    // Confirm the change is reflected in the list.
    let list_resp = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/admin/collateral/configs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let list_body = parse_json(list_resp).await;
    let btc = list_body["configs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["coin"] == "BTC")
        .unwrap();
    assert_eq!(btc["haircutRate"].as_str().unwrap(), "0.88");
}

/// PUT /admin/collateral/configs/{coin} — disable a coin (isEligible: false).
#[tokio::test]
async fn http_admin_update_collateral_config_disable_coin() {
    let app = setup();
    let resp = app
        .oneshot(put(
            "/admin/collateral/configs/DOGE",
            serde_json::json!({"isEligible": false}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json(resp).await;
    assert_eq!(body["isEligible"].as_bool().unwrap(), false);
}

/// PUT /admin/collateral/configs/{coin} — invalid haircut out of range returns 400.
#[tokio::test]
async fn http_admin_update_collateral_config_invalid_haircut() {
    let app = setup();
    // Haircut > 1 is invalid.
    let resp = app
        .clone()
        .oneshot(put(
            "/admin/collateral/configs/BTC",
            serde_json::json!({"haircutRate": "1.5"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Zero haircut is also invalid.
    let resp = app
        .oneshot(put(
            "/admin/collateral/configs/BTC",
            serde_json::json!({"haircutRate": "0"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// PUT /admin/collateral/configs/{coin} — create a brand-new coin config.
#[tokio::test]
async fn http_admin_update_collateral_config_new_coin() {
    let app = setup();
    let resp = app
        .clone()
        .oneshot(put(
            "/admin/collateral/configs/NEWTOKEN",
            serde_json::json!({"haircutRate": "0.65", "isEligible": true}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json(resp).await;
    assert_eq!(body["coin"].as_str().unwrap(), "NEWTOKEN");
    assert_eq!(body["haircutRate"].as_str().unwrap(), "0.65");
    assert_eq!(body["isEligible"].as_bool().unwrap(), true);

    // The new coin must appear in the config list.
    let list_resp = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/admin/collateral/configs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let list_body = parse_json(list_resp).await;
    let found = list_body["configs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["coin"] == "NEWTOKEN");
    assert!(found, "newly added coin must appear in the configs list");
}

/// PUT /admin/collateral/configs/{coin} — set and clear maxCollateralAmount.
#[tokio::test]
async fn http_admin_update_collateral_config_max_amount() {
    let app = setup();

    // Set a cap.
    let resp = app
        .clone()
        .oneshot(put(
            "/admin/collateral/configs/ETH",
            serde_json::json!({"maxCollateralAmount": "100"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json(resp).await;
    assert_eq!(
        body["maxCollateralAmount"].as_str().unwrap(),
        "100"
    );

    // Clear the cap by sending null.
    let resp = app
        .oneshot(put(
            "/admin/collateral/configs/ETH",
            serde_json::json!({"maxCollateralAmount": null}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = parse_json(resp).await;
    assert!(
        body["maxCollateralAmount"].is_null(),
        "maxCollateralAmount should be null after clearing"
    );
}
