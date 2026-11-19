//! M24-5: End-to-end security pipeline integration test.

use std::sync::Arc;

use exchange_api_gateway::exchange::Exchange;
use exchange_common::*;
use rust_decimal_macros::dec;
use uuid::Uuid;

fn setup_exchange() -> Arc<Exchange> {
    let exchange = Exchange::new();
    let product = Product {
        symbol: Symbol::new("BTCUSDT"),
        base_asset: "BTC".to_string(),
        quote_asset: "USDT".to_string(),
        product_type: ProductType::Perpetual,
        tick_size: dec!(0.01),
        lot_size: dec!(0.001),
        min_notional: dec!(10),
        maker_fee: dec!(0.001),
        taker_fee: dec!(0.002),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: Some(dec!(100)),
    };
    exchange.add_product(product);
    exchange
}

fn limit_order(user: Uuid, side: Side, price: rust_decimal::Decimal, qty: rust_decimal::Decimal) -> PlaceOrderCommand {
    PlaceOrderCommand {
        order_id: Uuid::new_v4(),
        user_id: user,
        symbol: Symbol::new("BTCUSDT"),
        side,
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
        stp_mode: StpMode::ExpireTaker,
        request_expire_time: None,
    }
}

#[test]
fn e2e_spoofing_attack_detected_and_frozen() {
    let exchange = setup_exchange();
    let attacker = Uuid::new_v4();
    let victim = Uuid::new_v4();
    exchange.create_account(attacker);
    exchange.create_account(victim);
    exchange.deposit(attacker, "USDT", dec!(100_000)).unwrap();
    exchange.deposit(victim, "USDT", dec!(100_000)).unwrap();

    // Attacker spoofs: 20 orders placed and cancelled.
    let now = chrono::Utc::now();
    {
        let mut surv = exchange.surveillance.lock();
        for _ in 0..20 {
            let oid = Uuid::new_v4();
            surv.on_order_placed(attacker, oid, Side::Buy, dec!(1), now);
            surv.on_order_cancelled(attacker, oid, now + chrono::Duration::seconds(1));
        }
        let alerts = surv.check_all(
            &[(attacker, Some(Symbol::new("BTCUSDT")))],
            now + chrono::Duration::seconds(2),
        );
        assert!(!alerts.is_empty(), "spoofing should trigger alerts");
    }

    // Freeze attacker.
    exchange.freeze_account(attacker);
    assert!(!exchange.get_account_status(attacker).can_trade());

    // Attacker order rejected.
    assert!(exchange.place_order(limit_order(attacker, Side::Buy, dec!(50000), dec!(0.1))).is_err());

    // Victim can still trade.
    assert!(exchange.place_order(limit_order(victim, Side::Sell, dec!(60000), dec!(0.1))).is_ok());

    // Admin reviews alert — account stays frozen.
    let alert_id = {
        let surv = exchange.surveillance.lock();
        surv.unreviewed_alerts().first().map(|a| a.id)
    };
    if let Some(id) = alert_id {
        let mut surv = exchange.surveillance.lock();
        assert!(surv.review_alert(id));
    }
    assert!(!exchange.get_account_status(attacker).can_trade());
}

#[test]
fn e2e_rate_limit_override_blocks_spam() {
    let exchange = setup_exchange();
    let user = Uuid::new_v4();
    exchange.create_account(user);
    exchange.deposit(user, "USDT", dec!(1_000_000)).unwrap();

    exchange.set_reduced_rate_limit(user);

    let mut rejected = 0;
    for i in 0..15 {
        let cmd = limit_order(user, Side::Buy, dec!(50000) + rust_decimal::Decimal::from(i), dec!(0.001));
        if exchange.place_order(cmd).is_err() {
            rejected += 1;
        }
    }
    assert!(rejected > 0, "rate-limited user should have rejections");
}

#[test]
fn e2e_cancel_all_clears_open_orders() {
    let exchange = setup_exchange();
    let user = Uuid::new_v4();
    exchange.create_account(user);
    exchange.deposit(user, "USDT", dec!(1_000_000)).unwrap();

    for i in 0..5 {
        let cmd = limit_order(user, Side::Buy, dec!(49000) + rust_decimal::Decimal::from(i * 100), dec!(0.01));
        let _ = exchange.place_order(cmd);
    }

    let before = exchange.pending_orders.iter().filter(|r| r.user_id == user).count();
    assert!(before > 0);

    let (cancelled, _) = exchange.cancel_all_orders_for_user(user);
    assert!(cancelled > 0);

    let after = exchange.pending_orders.iter().filter(|r| r.user_id == user).count();
    assert_eq!(after, 0);
}

#[test]
fn e2e_unfreeze_restores_trading() {
    let exchange = setup_exchange();
    let user = Uuid::new_v4();
    exchange.create_account(user);
    exchange.deposit(user, "USDT", dec!(100_000)).unwrap();

    exchange.freeze_account(user);
    assert!(exchange.place_order(limit_order(user, Side::Buy, dec!(50000), dec!(0.1))).is_err());

    exchange.unfreeze_account(user);
    assert!(exchange.place_order(limit_order(user, Side::Buy, dec!(50000), dec!(0.1))).is_ok());
}
