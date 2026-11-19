//! E2E tests that require a PostgreSQL database.
//!
//! Run with: `TEST_DATABASE_URL=postgres://... cargo test -- --ignored`
//!
//! These tests are `#[ignore]`d by default so `cargo test` runs without a DB.

use std::sync::Arc;
use exchange_api_gateway::exchange::Exchange;
use exchange_common::*;
use rust_decimal_macros::dec;
use uuid::Uuid;

/// Full flow with PostgreSQL persistence: create state → verify DB.
#[tokio::test]
#[ignore]
async fn e2e_db_user_persists_across_restart() {
    let db_url = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must be set for DB tests");

    let db = exchange_db::Database::connect(&db_url).await.unwrap();
    db.migrate().await.unwrap();

    let user_id = Uuid::new_v4();
    let email = format!("e2e_db_{}@test.com", user_id.simple());

    // Phase 1: Create exchange, add user, deposit
    {
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
        exchange.set_db_pool(db.pool.clone());

        // Create user in DB
        exchange_db::user_repo::UserRepo::create(&db.pool, user_id, Some(&email))
            .await
            .unwrap();
        exchange_db::balance_repo::BalanceRepo::deposit(&db.pool, user_id, "USDT", dec!(10_000))
            .await
            .unwrap();
    }

    // Phase 2: New exchange, load_from_db → user should be there
    {
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
        exchange.set_db_pool(db.pool.clone());
        exchange.load_from_db(&db.pool).await.unwrap();

        // Verify user was loaded
        let has_balance = exchange
            .accounts
            .get_balance(user_id, "USDT")
            .is_some();
        assert!(
            has_balance,
            "user balance must exist after load_from_db"
        );

        // Verify email mapping was loaded
        let emails = exchange.user_emails.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(
            emails.get(&email),
            Some(&user_id),
            "email mapping must be restored from DB"
        );
    }
}

/// Verify terminal_order_ids returns filled/cancelled orders.
#[tokio::test]
#[ignore]
async fn e2e_db_terminal_order_ids() {
    let db_url = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must be set for DB tests");

    let db = exchange_db::Database::connect(&db_url).await.unwrap();
    db.migrate().await.unwrap();

    let user_id = Uuid::new_v4();
    let order_id = Uuid::new_v4();

    // Setup: create user and product
    sqlx::query("INSERT INTO users (id) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(user_id)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO products (symbol, base_asset, quote_asset, tick_size, lot_size, min_notional, maker_fee, taker_fee)
         VALUES ('TSTTST', 'TST', 'TST', 0.01, 0.01, 1, 0.001, 0.001)
         ON CONFLICT DO NOTHING"
    )
    .execute(&db.pool)
    .await
    .unwrap();

    // Create an order and mark it as Filled
    exchange_db::order_repo::OrderRepo::create(
        &db.pool, order_id, user_id, "TSTTST", "Buy", "Limit", "Gtc", Some(dec!(100)), dec!(1),
    )
    .await
    .unwrap();
    exchange_db::order_repo::OrderRepo::update_status(&db.pool, order_id, "Filled", dec!(1))
        .await
        .unwrap();

    // Verify it appears in terminal set
    let terminal = exchange_db::order_repo::OrderRepo::terminal_order_ids(&db.pool)
        .await
        .unwrap();
    assert!(
        terminal.contains(&order_id),
        "filled order must appear in terminal set"
    );
}
