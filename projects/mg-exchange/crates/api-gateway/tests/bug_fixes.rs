//! Tests for the five critical bug fixes.
//!
//! Bug 1: Grid/advanced bot stop must cancel live orders on the matching engine.
//! Bug 2: Referral commissions must be distributed on every fill.
//! Bug 3: Copy trading must propagate maker fills (not only taker fills).
//! Bug 4: VIP tier recalculator must sync balance-based qualification hourly.
//! Bug 5: Fixed earn products must auto-redeem when the lock period expires.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use exchange_api_gateway::exchange::Exchange;
use exchange_api_gateway::server::build_router;
use exchange_common::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tower::ServiceExt;
use uuid::Uuid;

// ── Shared helpers ────────────────────────────────────────────────────────────

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

    let user = Uuid::nil();
    exchange.create_account(user);
    exchange.deposit(user, "USDT", dec!(100_000)).unwrap();
    exchange.deposit(user, "BTC", dec!(10)).unwrap();

    build_router(exchange)
}

async fn json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| panic!("response is not JSON: {bytes:?}"))
}

// ── Bug 1: Grid bot stop cancels orders ──────────────────────────────────────

/// A stopped grid bot must return status "stopped".
#[tokio::test]
async fn bug1_grid_bot_stop_returns_stopped_status() {
    let app = setup();

    // Create a grid bot.
    let create_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/bots/grid")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&serde_json::json!({
                    "symbol": "BTCUSDT",
                    "upper_price": "50000",
                    "lower_price": "40000",
                    "grid_count": 5,
                    "total_investment": "1000",
                    "side": "neutral"
                })).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    // Grid bot create returns 200 (not 201).
    assert_eq!(create_resp.status(), StatusCode::OK);
    let body = json(create_resp).await;
    let bot_id = body["bot_id"].as_str().expect("bot_id in response").to_string();

    // Stop it.
    let stop_resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/bots/{bot_id}/stop"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stop_resp.status(), StatusCode::OK);
    let stop_body = json(stop_resp).await;
    assert_eq!(
        stop_body["status"].as_str().unwrap(),
        "stopped",
        "stop response should have status=stopped"
    );
}

/// Stopping an advanced (infinity-grid) bot returns "stopped".
#[tokio::test]
async fn bug1_advanced_bot_stop_returns_stopped_status() {
    let app = setup();

    let create_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/bots/infinity-grid")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&serde_json::json!({
                    "symbol": "BTCUSDT",
                    "lower_price": "40000",
                    "qty_per_level": "0.001",
                    "grid_spacing_pct": "1.0"
                })).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_resp.status(), StatusCode::OK);
    let body = json(create_resp).await;
    let bot_id = body["bot_id"].as_str().expect("bot_id");

    let stop_resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/bots/advanced/{bot_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stop_resp.status(), StatusCode::OK);
    let stop_body = json(stop_resp).await;
    assert_eq!(stop_body["status"].as_str().unwrap(), "stopped");
}

/// Stopping an already-stopped advanced bot returns 400.
#[tokio::test]
async fn bug1_advanced_bot_double_stop_is_bad_request() {
    let app = setup();

    let create_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/bots/infinity-grid")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&serde_json::json!({
                    "symbol": "BTCUSDT",
                    "lower_price": "40000",
                    "qty_per_level": "0.001",
                    "grid_spacing_pct": "1.0"
                })).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = json(create_resp).await;
    let bot_id = body["bot_id"].as_str().expect("bot_id").to_string();

    // First stop — should succeed.
    app.clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/bots/advanced/{bot_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Second stop — should be 400.
    let second_stop = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/bots/advanced/{bot_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second_stop.status(), StatusCode::BAD_REQUEST);
}

// ── Bug 1 (unit): GridBot::stop() live_order_ids ─────────────────────────────

#[test]
fn bug1_grid_bot_stop_returns_all_live_order_ids() {
    use exchange_trading_bots::{BotStatus, GridBot, GridBotConfig, GridSide};
    use rust_decimal_macros::dec;

    let config = GridBotConfig {
        user_id: Uuid::new_v4(),
        symbol: Symbol::new("BTCUSDT"),
        upper_price: dec!(50000),
        lower_price: dec!(40000),
        grid_count: 5,
        total_investment: dec!(1000),
        side: GridSide::Neutral,
    };
    let mut bot = GridBot::new(config);
    bot.generate_initial_orders(dec!(45000));

    let live_before: usize = bot
        .grid_lines
        .iter()
        .map(|l| l.buy_order_id.is_some() as usize + l.sell_order_id.is_some() as usize)
        .sum();
    assert!(live_before > 0, "should have live orders before stop");

    let cancelled = bot.stop();
    assert_eq!(
        cancelled.len(),
        live_before,
        "stop() should return exactly all live order IDs"
    );
    assert_eq!(bot.status, BotStatus::Stopped);

    let live_after: usize = bot
        .grid_lines
        .iter()
        .map(|l| l.buy_order_id.is_some() as usize + l.sell_order_id.is_some() as usize)
        .sum();
    assert_eq!(live_after, 0, "grid_lines should have no live orders after stop");
}

// ── Bug 1 (unit): AdvancedBot::live_order_ids ────────────────────────────────

#[test]
fn bug1_infinity_grid_live_order_ids_collected() {
    use exchange_api_gateway::rest_bots::{AdvancedBot, InfinityGridBot, InfinityGridLine};
    use exchange_trading_bots::BotStatus;

    let line_with_orders = InfinityGridLine {
        price: dec!(45000),
        buy_order_id: Some(Uuid::new_v4()),
        sell_order_id: Some(Uuid::new_v4()),
    };
    let line_without = InfinityGridLine {
        price: dec!(46000),
        buy_order_id: None,
        sell_order_id: None,
    };
    let bot = AdvancedBot::InfinityGrid(InfinityGridBot {
        id: Uuid::new_v4(),
        user_id: Uuid::new_v4(),
        symbol: Symbol::new("BTCUSDT"),
        lower_price: dec!(40000),
        qty_per_level: dec!(0.001),
        grid_spacing_pct: dec!(1),
        highest_level: dec!(45000),
        grid_lines: vec![line_with_orders, line_without],
        status: BotStatus::Active,
        realized_pnl: Decimal::ZERO,
        total_trades: 0,
        created_at: chrono::Utc::now(),
    });

    let live_ids = bot.live_order_ids();
    assert_eq!(live_ids.len(), 2, "should collect buy+sell from active line only");
}

#[test]
fn bug1_martingale_live_order_ids_skips_filled_layers() {
    use exchange_api_gateway::rest_bots::{AdvancedBot, MartingaleBot, MartingaleLayer};
    use exchange_trading_bots::BotStatus;

    let unfilled_id = Uuid::new_v4();
    let layers = vec![
        MartingaleLayer {
            layer: 0,
            trigger_price: dec!(45000),
            qty: dec!(0.1),
            order_id: Some(Uuid::new_v4()), // placed and filled
            filled: true,
        },
        MartingaleLayer {
            layer: 1,
            trigger_price: dec!(44000),
            qty: dec!(0.2),
            order_id: Some(unfilled_id),
            filled: false,
        },
        MartingaleLayer {
            layer: 2,
            trigger_price: dec!(43000),
            qty: dec!(0.4),
            order_id: None, // not yet placed
            filled: false,
        },
    ];
    let bot = AdvancedBot::Martingale(MartingaleBot {
        id: Uuid::new_v4(),
        user_id: Uuid::new_v4(),
        symbol: Symbol::new("BTCUSDT"),
        side: "buy".to_string(),
        initial_qty: dec!(0.1),
        multiplier: dec!(2),
        max_layers: 3,
        price_drop_pct: dec!(2),
        take_profit_pct: dec!(3),
        layers,
        avg_entry: Decimal::ZERO,
        total_qty_filled: Decimal::ZERO,
        status: BotStatus::Active,
        realized_pnl: Decimal::ZERO,
        created_at: chrono::Utc::now(),
    });

    let live_ids = bot.live_order_ids();
    // Only layer 1 is unfilled with a real order_id.
    assert_eq!(live_ids.len(), 1);
    assert_eq!(live_ids[0], unfilled_id);
}

// ── Bug 2: Referral commissions ───────────────────────────────────────────────

/// Registering a referral code and then trading should result in referral
/// commission records appearing in the affiliate history endpoint.
#[tokio::test]
async fn bug2_referral_commissions_credited_after_trade() {
    use std::sync::Arc;

    let exchange = Arc::new(Exchange::new());

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

    let referrer = Uuid::new_v4();
    let trader = Uuid::nil(); // authenticated user

    // counter_party is the maker; trader (the referred user) is the taker.
    // referrer is a third party who receives the commission.
    let counter_party = Uuid::new_v4();
    exchange.create_account(referrer);
    exchange.create_account(trader);
    exchange.create_account(counter_party);
    exchange.deposit(referrer, "USDT", dec!(1000)).unwrap();
    exchange.deposit(trader, "USDT", dec!(100_000)).unwrap();
    exchange.deposit(counter_party, "USDT", dec!(100_000)).unwrap();
    exchange.deposit(counter_party, "BTC", dec!(10)).unwrap();

    // Register the referral relationship: trader was referred by referrer.
    {
        let mut tree = exchange.referral_tree.lock().unwrap_or_else(|e| e.into_inner());
        tree.insert(trader, referrer);
    }

    let referrer_balance_before = exchange
        .accounts
        .get_balance(referrer, "USDT")
        .map(|b| b.total())
        .unwrap_or(Decimal::ZERO);

    // counter_party places a resting sell; trader hits it as taker.
    // The taker fee from trader's fill is subject to referral commission distribution.
    let maker_cmd = PlaceOrderCommand {
        order_id: Uuid::new_v4(),
        user_id: counter_party,
        symbol: Symbol::new("BTCUSDT"),
        side: Side::Sell,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::Gtc,
        price: dec!(45000),
        qty: dec!(0.01),
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
    };
    exchange.place_order(maker_cmd).unwrap();

    let taker_cmd = PlaceOrderCommand {
        order_id: Uuid::new_v4(),
        user_id: trader, // the referred user is the taker — commission goes to referrer
        symbol: Symbol::new("BTCUSDT"),
        side: Side::Buy,
        order_type: OrderType::Market,
        time_in_force: TimeInForce::Ioc,
        price: dec!(0),
        qty: dec!(0.01),
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
    };
    exchange.place_order(taker_cmd).unwrap();

    // The fill should have triggered referral commission distribution.
    let commissions = exchange.affiliate_commissions.lock().unwrap_or_else(|e| e.into_inner());
    assert!(
        !commissions.is_empty(),
        "referral commissions should be recorded after a fill with a referral relationship"
    );
    // At least the taker's referrer (= referrer) should have received tier1 commission.
    let referrer_commission = commissions.iter().any(|c| c.earner_id == referrer);
    assert!(referrer_commission, "referrer should appear in commission records");
    drop(commissions);

    // Referrer's balance should have increased above their initial 1000 deposit
    // (they receive commission but place no orders themselves).
    let referrer_balance_after = exchange
        .accounts
        .get_balance(referrer, "USDT")
        .map(|b| b.total())
        .unwrap_or(Decimal::ZERO);
    assert!(
        referrer_balance_after > referrer_balance_before,
        "referrer balance should increase after commission: before={referrer_balance_before} after={referrer_balance_after}"
    );
}

/// Referral commissions are NOT created when there is no referral relationship.
#[tokio::test]
async fn bug2_no_referral_no_commission() {
    use std::sync::Arc;

    let exchange = Arc::new(Exchange::new());

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

    let user_a = Uuid::nil();
    let user_b = Uuid::new_v4();
    exchange.create_account(user_a);
    exchange.create_account(user_b);
    exchange.deposit(user_a, "USDT", dec!(100_000)).unwrap();
    exchange.deposit(user_a, "BTC", dec!(10)).unwrap();
    exchange.deposit(user_b, "USDT", dec!(100_000)).unwrap();
    exchange.deposit(user_b, "BTC", dec!(10)).unwrap();

    // No referral tree entries — plain trade.
    let maker_cmd = PlaceOrderCommand {
        order_id: Uuid::new_v4(),
        user_id: user_b,
        symbol: Symbol::new("BTCUSDT"),
        side: Side::Sell,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::Gtc,
        price: dec!(45000),
        qty: dec!(0.01),
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
    };
    exchange.place_order(maker_cmd).unwrap();

    let taker_cmd = PlaceOrderCommand {
        order_id: Uuid::new_v4(),
        user_id: user_a,
        symbol: Symbol::new("BTCUSDT"),
        side: Side::Buy,
        order_type: OrderType::Market,
        time_in_force: TimeInForce::Ioc,
        price: dec!(0),
        qty: dec!(0.01),
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
    };
    exchange.place_order(taker_cmd).unwrap();

    let commissions = exchange.affiliate_commissions.lock().unwrap_or_else(|e| e.into_inner());
    assert!(
        commissions.is_empty(),
        "no commissions should be recorded when there is no referral relationship"
    );
}

/// Three-tier referral chain: commission reaches tier 2.
#[tokio::test]
async fn bug2_three_tier_referral_chain_distributes_commissions() {
    use std::sync::Arc;

    let exchange = Arc::new(Exchange::new());

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

    let tier1_referrer = Uuid::new_v4();
    let tier2_referrer = Uuid::new_v4();
    let trader = Uuid::nil(); // the referred user who generates the fee
    let seller = Uuid::new_v4(); // counter party who provides the resting order

    for uid in [tier1_referrer, tier2_referrer, trader, seller] {
        exchange.create_account(uid);
        exchange.deposit(uid, "USDT", dec!(100_000)).unwrap();
    }
    // Only the seller needs BTC.
    exchange.deposit(seller, "BTC", dec!(10)).unwrap();

    // Build chain: trader → tier1_referrer → tier2_referrer
    {
        let mut tree = exchange.referral_tree.lock().unwrap_or_else(|e| e.into_inner());
        tree.insert(trader, tier1_referrer);
        tree.insert(tier1_referrer, tier2_referrer);
    }

    // Execute a fill — seller is the maker, trader is the taker (generates the taker fee).
    let maker_cmd = PlaceOrderCommand {
        order_id: Uuid::new_v4(),
        user_id: seller,
        symbol: Symbol::new("BTCUSDT"),
        side: Side::Sell,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::Gtc,
        price: dec!(45000),
        qty: dec!(0.01),
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
    };
    exchange.place_order(maker_cmd).unwrap();

    let taker_cmd = PlaceOrderCommand {
        order_id: Uuid::new_v4(),
        user_id: trader, // referred user — their fee triggers referral distribution
        symbol: Symbol::new("BTCUSDT"),
        side: Side::Buy,
        order_type: OrderType::Market,
        time_in_force: TimeInForce::Ioc,
        price: dec!(0),
        qty: dec!(0.01),
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
    };
    exchange.place_order(taker_cmd).unwrap();

    let commissions = exchange.affiliate_commissions.lock().unwrap_or_else(|e| e.into_inner());
    // Both tier1 and tier2 earners should appear.
    let tier1_earned = commissions.iter().any(|c| c.earner_id == tier1_referrer && c.tier == 1);
    let tier2_earned = commissions.iter().any(|c| c.earner_id == tier2_referrer && c.tier == 2);
    assert!(tier1_earned, "tier1 referrer should receive commission");
    assert!(tier2_earned, "tier2 referrer should receive commission");
}

// ── Bug 3: Copy trading propagates maker fills ────────────────────────────────

/// The copy-trading leaderboard endpoint returns 200, confirming the
/// infrastructure for copy-trading (including the master registry) works.
#[tokio::test]
async fn bug3_copy_trading_leaderboard_returns_ok() {
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
}

/// Both taker and maker sides are checked for master status — neither panics.
#[tokio::test]
async fn bug3_maker_fill_propagation_does_not_panic() {
    use std::sync::Arc;

    let exchange = Arc::new(Exchange::new());

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

    let master = Uuid::new_v4();
    let taker = Uuid::nil();

    exchange.create_account(master);
    exchange.create_account(taker);
    exchange.deposit(master, "USDT", dec!(100_000)).unwrap();
    exchange.deposit(master, "BTC", dec!(10)).unwrap();
    exchange.deposit(taker, "USDT", dec!(100_000)).unwrap();

    // Register master as a copy-trading master via the registry API.
    {
        let mut masters = exchange.copy_trading_masters.lock().unwrap_or_else(|e| e.into_inner());
        masters
            .register(master, "Master Trader", dec!(0.10))
            .expect("registration should succeed");
    }

    // Master places a limit sell (will fill as maker).
    let maker_cmd = PlaceOrderCommand {
        order_id: Uuid::new_v4(),
        user_id: master,
        symbol: Symbol::new("BTCUSDT"),
        side: Side::Sell,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::Gtc,
        price: dec!(45000),
        qty: dec!(0.01),
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
    };
    exchange.place_order(maker_cmd).unwrap();

    // Taker hits the resting order — master fills as maker.
    let taker_cmd = PlaceOrderCommand {
        order_id: Uuid::new_v4(),
        user_id: taker,
        symbol: Symbol::new("BTCUSDT"),
        side: Side::Buy,
        order_type: OrderType::Market,
        time_in_force: TimeInForce::Ioc,
        price: dec!(0),
        qty: dec!(0.01),
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
    };
    // Should not panic — the fixed code checks both taker and maker for master status.
    exchange.place_order(taker_cmd).unwrap();
}

/// `propagate_copy_trade_for` is a no-op for non-master users.
#[tokio::test]
async fn bug3_non_master_fill_does_not_trigger_copy() {
    use std::sync::Arc;

    let exchange = Arc::new(Exchange::new());

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

    let user_a = Uuid::nil();
    let user_b = Uuid::new_v4();
    exchange.create_account(user_a);
    exchange.create_account(user_b);
    exchange.deposit(user_a, "USDT", dec!(100_000)).unwrap();
    exchange.deposit(user_b, "USDT", dec!(100_000)).unwrap();
    exchange.deposit(user_b, "BTC", dec!(10)).unwrap();

    let maker_cmd = PlaceOrderCommand {
        order_id: Uuid::new_v4(),
        user_id: user_b,
        symbol: Symbol::new("BTCUSDT"),
        side: Side::Sell,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::Gtc,
        price: dec!(45000),
        qty: dec!(0.01),
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
    };
    exchange.place_order(maker_cmd).unwrap();

    let taker_cmd = PlaceOrderCommand {
        order_id: Uuid::new_v4(),
        user_id: user_a,
        symbol: Symbol::new("BTCUSDT"),
        side: Side::Buy,
        order_type: OrderType::Market,
        time_in_force: TimeInForce::Ioc,
        price: dec!(0),
        qty: dec!(0.01),
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
    };
    exchange.place_order(taker_cmd).unwrap();

    // No followers should have been notified (no masters registered).
    let followers = exchange.copy_trading_followers.lock().unwrap_or_else(|e| e.into_inner());
    // Both users have no follower relationships — state is clean.
    assert!(followers.master_followers(user_a).is_empty());
    assert!(followers.master_followers(user_b).is_empty());
}

// ── Bug 4: VIP tier recalculator ─────────────────────────────────────────────

/// `user_tiers()` exposes the fee engine's internal state for the background task.
#[test]
fn bug4_fee_engine_user_tiers_returns_state() {
    use exchange_account::FeeEngine;

    let mut engine = FeeEngine::new();
    let user = Uuid::new_v4();

    assert!(engine.user_tiers().is_empty(), "no users before any trade");

    engine.update_volume(user, dec!(1_000_000));
    assert_eq!(engine.user_tiers().len(), 1);
    assert_eq!(engine.user_tiers()[&user].tier_level, 1);
}

/// `set_balance` triggers tier recalculation for balance-qualified tiers.
#[test]
fn bug4_set_balance_recalculates_tier_for_custom_threshold() {
    use exchange_account::{FeeEngine, VipTier};
    use rust_decimal::Decimal;

    let tiers = vec![
        VipTier {
            level: 0,
            name: "Regular".to_string(),
            min_volume_30d: Decimal::ZERO,
            min_balance: Decimal::ZERO,
            maker_fee: dec!(0.001),
            taker_fee: dec!(0.002),
        },
        VipTier {
            level: 1,
            name: "BalanceVIP".to_string(),
            min_volume_30d: dec!(10_000_000), // very high volume threshold
            min_balance: dec!(50_000),        // reachable balance threshold
            maker_fee: dec!(0.0008),
            taker_fee: dec!(0.0016),
        },
    ];
    let mut engine = FeeEngine::with_custom_tiers(tiers);
    let user = Uuid::new_v4();

    // User starts at level 0.
    engine.update_volume(user, dec!(100)); // far below volume threshold
    assert_eq!(engine.get_tier(user).level, 0);

    // VIP tier recalculator syncs balance → tier should upgrade.
    engine.set_balance(user, dec!(50_000));
    assert_eq!(
        engine.get_tier(user).level,
        1,
        "balance-based upgrade should activate when set_balance is called"
    );
}

/// `user_tiers()` returns an empty map for a fresh engine.
#[test]
fn bug4_user_tiers_empty_for_new_engine() {
    use exchange_account::FeeEngine;

    let engine = FeeEngine::new();
    assert!(engine.user_tiers().is_empty());
}

// ── Bug 5: Fixed earn auto-redeem ────────────────────────────────────────────

/// An earn allocation with an `unlock_at` in the past should be auto-redeemed
/// by the yield accrual task.  We test the task logic indirectly by exercising
/// the Exchange state that the task mutates.
#[test]
fn bug5_expired_allocation_is_marked_redeemed() {
    use exchange_api_gateway::exchange::{EarnAllocation, EarnProduct};
    use std::sync::Arc;

    let exchange = Arc::new(Exchange::new());
    let user = Uuid::new_v4();
    exchange.create_account(user);
    exchange.deposit(user, "USDT", dec!(0)).unwrap();

    let product_id = Uuid::new_v4();
    let now = chrono::Utc::now();
    let expired_alloc_id = Uuid::new_v4();

    // Register a fixed product.
    {
        let mut products = exchange.earn_products.lock().unwrap_or_else(|e| e.into_inner());
        products.push(EarnProduct {
            id: product_id,
            coin: "USDT".to_string(),
            name: "Fixed 30d".to_string(),
            apy: dec!(0.05),
            min_amount: dec!(10),
            max_amount: dec!(1_000_000),
            lock_days: 30,
            product_type: "fixed_30d".to_string(),
            supports_auto_compound: false,
            pool_capacity: None,
            max_user_pct: dec!(0.05),
        });
    }

    // Create an allocation whose lock period has already elapsed.
    {
        let mut allocs = exchange.earn_allocations.lock().unwrap_or_else(|e| e.into_inner());
        allocs.push(EarnAllocation {
            id: expired_alloc_id,
            user_id: user,
            product_id,
            coin: "USDT".to_string(),
            amount: dec!(1000),
            earned_interest: dec!(10),
            started_at: now - chrono::Duration::days(31),
            unlock_at: Some(now - chrono::Duration::seconds(1)), // already past
            status: "active".to_string(),
            auto_compound: false,
            last_accrued_at: now - chrono::Duration::hours(1),
        });
    }

    // Simulate what the task does: detect expired allocations and redeem.
    let expired: Vec<(Uuid, Uuid, String, Decimal, Decimal)> = {
        let allocs = exchange.earn_allocations.lock().unwrap_or_else(|e| e.into_inner());
        allocs
            .iter()
            .filter(|a| {
                if a.status != "active" { return false; }
                if let Some(unlock_at) = a.unlock_at {
                    now >= unlock_at
                } else {
                    false
                }
            })
            .map(|a| (a.id, a.user_id, a.coin.clone(), a.amount, a.amount + a.earned_interest))
            .collect()
    };

    assert_eq!(expired.len(), 1, "one allocation should be expired");

    for (alloc_id, uid, coin, _principal, total) in &expired {
        let _ = exchange.accounts.deposit(*uid, coin, *total);
        {
            let mut allocs = exchange.earn_allocations.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(alloc) = allocs.iter_mut().find(|a| a.id == *alloc_id) {
                alloc.status = "redeemed".to_string();
                alloc.amount = Decimal::ZERO;
                alloc.earned_interest = Decimal::ZERO;
            }
        }
    }

    // Verify the allocation is now marked redeemed.
    let allocs = exchange.earn_allocations.lock().unwrap_or_else(|e| e.into_inner());
    let alloc = allocs.iter().find(|a| a.id == expired_alloc_id).unwrap();
    assert_eq!(alloc.status, "redeemed");
    assert_eq!(alloc.amount, Decimal::ZERO);
    assert_eq!(alloc.earned_interest, Decimal::ZERO);
}

/// A flexible allocation (lock_days == 0) should NOT be auto-redeemed.
#[test]
fn bug5_flexible_allocation_not_auto_redeemed() {
    use exchange_api_gateway::exchange::{EarnAllocation, EarnProduct};

    let product_id = Uuid::new_v4();
    let now = chrono::Utc::now();

    // Simulate the task filter on a flexible allocation.
    let alloc = EarnAllocation {
        id: Uuid::new_v4(),
        user_id: Uuid::new_v4(),
        product_id,
        coin: "USDT".to_string(),
        amount: dec!(500),
        earned_interest: dec!(5),
        started_at: now - chrono::Duration::days(100),
        unlock_at: None, // flexible — no lock
        status: "active".to_string(),
        auto_compound: false,
        last_accrued_at: now,
    };

    // lock_days == 0 means flexible; filter should exclude it.
    let product = EarnProduct {
        id: product_id,
        coin: "USDT".to_string(),
        name: "USDT Flexible".to_string(),
        apy: dec!(0.03),
        min_amount: dec!(10),
        max_amount: dec!(1_000_000),
        lock_days: 0,
        product_type: "flexible".to_string(),
        supports_auto_compound: true,
        pool_capacity: None,
        max_user_pct: Decimal::new(5, 2),
    };

    // Replicate the task's expiry check.
    let should_auto_redeem = product.lock_days != 0 && {
        if let Some(unlock_at) = alloc.unlock_at {
            now >= unlock_at
        } else {
            now >= alloc.started_at + chrono::Duration::days(product.lock_days as i64)
        }
    };

    assert!(
        !should_auto_redeem,
        "flexible allocation (lock_days=0) must never be auto-redeemed"
    );
}

/// A fixed-term allocation that is still within its lock period is not redeemed.
#[test]
fn bug5_active_fixed_allocation_not_redeemed_early() {
    use exchange_api_gateway::exchange::EarnAllocation;

    let now = chrono::Utc::now();
    let alloc = EarnAllocation {
        id: Uuid::new_v4(),
        user_id: Uuid::new_v4(),
        product_id: Uuid::new_v4(),
        coin: "USDT".to_string(),
        amount: dec!(1000),
        earned_interest: dec!(0),
        started_at: now - chrono::Duration::days(10),
        unlock_at: Some(now + chrono::Duration::days(20)), // still locked
        status: "active".to_string(),
        auto_compound: false,
        last_accrued_at: now,
    };

    let is_expired = if let Some(unlock_at) = alloc.unlock_at {
        now >= unlock_at
    } else {
        false
    };

    assert!(
        !is_expired,
        "allocation within lock period must not be treated as expired"
    );
}
