//! End-to-end integration tests for complete trading scenarios.
//!
//! Each test creates a fresh exchange, sets up users with deposits, and
//! exercises the real trading code path — no mocks.

use exchange_api_gateway::exchange::Exchange;
use exchange_common::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

// ── Constants ───────────────────────────────────────────────────────────────

const ALICE: Uuid = Uuid::from_u128(0xAAAA_AAAA_0000_0000_0000_0000_0000_0001);
const BOB: Uuid = Uuid::from_u128(0xBBBB_BBBB_0000_0000_0000_0000_0000_0002);
const CAROL: Uuid = Uuid::from_u128(0xCCCC_CCCC_0000_0000_0000_0000_0000_0003);

/// Treasury account ID (must match the constant in exchange.rs).
const TREASURY: Uuid = Uuid::from_u128(0xFEE0_FEE0_FEE0_FEE0_FEE0_FEE0_FEE0_FEE0);

// ── Helpers ─────────────────────────────────────────────────────────────────

fn make_order(
    user_id: Uuid,
    symbol: &str,
    side: Side,
    order_type: OrderType,
    price: Decimal,
    qty: Decimal,
) -> PlaceOrderCommand {
    PlaceOrderCommand {
        order_id: Uuid::new_v4(),
        user_id,
        symbol: Symbol::new(symbol),
        side,
        order_type,
        time_in_force: if order_type == OrderType::Market {
            TimeInForce::Ioc
        } else {
            TimeInForce::Gtc
        },
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
        stp_mode: StpMode::default(),
        request_expire_time: None,
    }
}

fn make_reduce_order(
    user_id: Uuid,
    symbol: &str,
    side: Side,
    order_type: OrderType,
    price: Decimal,
    qty: Decimal,
) -> PlaceOrderCommand {
    let mut cmd = make_order(user_id, symbol, side, order_type, price, qty);
    cmd.reduce_only = true;
    cmd
}

/// Create a spot exchange with XBTUSDC pair.
fn setup_spot_exchange() -> Arc<Exchange> {
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

    exchange
}

/// Create a futures exchange with BTCUSDT perpetual.
fn setup_futures_exchange() -> Arc<Exchange> {
    let exchange = Exchange::new();

    let product = Product {
        symbol: Symbol::new("BTCUSDT"),
        base_asset: "BTC".to_string(),
        quote_asset: "USDT".to_string(),
        product_type: ProductType::Perpetual,
        tick_size: dec!(0.01),
        lot_size: dec!(0.00001),
        min_notional: dec!(10),
        maker_fee: dec!(0.0002),
        taker_fee: dec!(0.0005),
    max_order_qty: None,
    max_order_notional: None,
        max_leverage: None,
    };
    exchange.add_product(product);

    exchange
}

fn get_balance(exchange: &Arc<Exchange>, user: Uuid, asset: &str) -> (Decimal, Decimal) {
    match exchange.accounts.get_balance(user, asset) {
        Some(b) => (b.available(), b.locked()),
        None => (dec!(0), dec!(0)),
    }
}

fn total_balance(exchange: &Arc<Exchange>, user: Uuid, asset: &str) -> Decimal {
    let (avail, locked) = get_balance(exchange, user, asset);
    avail + locked
}

// =============================================================================
// Scenario 1: Full Spot Trade Lifecycle
// =============================================================================

#[tokio::test]
async fn scenario_spot_trade_lifecycle() {
    let exchange = setup_spot_exchange();

    // Create users
    exchange.create_account(ALICE);
    exchange.create_account(BOB);

    // Deposit: Alice gets 10,000 USDC, Bob gets 1 XBT
    exchange.deposit(ALICE, "USDC", dec!(10000)).unwrap();
    exchange.deposit(BOB, "XBT", dec!(1)).unwrap();

    // Snapshot total system balance before trade
    let sys_usdc_before = total_balance(&exchange, ALICE, "USDC")
        + total_balance(&exchange, BOB, "USDC")
        + total_balance(&exchange, TREASURY, "USDC");
    let sys_xbt_before = total_balance(&exchange, ALICE, "XBT")
        + total_balance(&exchange, BOB, "XBT")
        + total_balance(&exchange, TREASURY, "XBT");

    // Alice places limit buy 0.1 XBT @ 65,000
    exchange
        .place_order(make_order(
            ALICE,
            "XBTUSDC",
            Side::Buy,
            OrderType::Limit,
            dec!(65000),
            dec!(0.1),
        ))
        .unwrap();

    // Verify: Alice's USDC locked = 0.1 * 65000 = 6500
    let (alice_usdc_avail, alice_usdc_locked) = get_balance(&exchange, ALICE, "USDC");
    assert_eq!(alice_usdc_locked, dec!(6500), "Alice must have 6500 USDC locked");
    assert_eq!(
        alice_usdc_avail,
        dec!(3500),
        "Alice must have 3500 USDC available"
    );

    // Bob places market sell 0.1 XBT
    exchange
        .place_order(make_order(
            BOB,
            "XBTUSDC",
            Side::Sell,
            OrderType::Market,
            dec!(0), // market order price
            dec!(0.1),
        ))
        .unwrap();

    // Verify fills at maker price (65,000)
    // Alice: gets 0.1 XBT minus taker-receives-nothing fee model
    // Actually: Alice is maker (limit buy was resting), Bob is taker (market sell)
    // Maker fee = 0.1% on quote for Alice's buy = charged on XBT received
    // Taker fee = 0.2% on quote for Bob's sell = charged on USDC received
    //
    // Alice (maker, buy):
    //   - Receives: 0.1 XBT * (1 - 0.001) = 0.0999 XBT (maker fee on base for buyer)
    //   - Pays: 6500 USDC (locked amount released on fill)
    //
    // Bob (taker, sell):
    //   - Receives: 6500 USDC * (1 - 0.002) = 6500 - 13 = 6487 USDC (taker fee)
    //   - Pays: 0.1 XBT
    //
    // Verify Alice's balances
    let alice_xbt = total_balance(&exchange, ALICE, "XBT");
    let alice_usdc = total_balance(&exchange, ALICE, "USDC");

    // Alice: started with 0 XBT, got 0.1 - maker fee
    // maker fee on buy = received_base * maker_fee_rate = 0.1 * 0.001 = 0.0001 XBT
    assert_eq!(alice_xbt, dec!(0.0999), "Alice XBT after buy = 0.1 - 0.0001 maker fee");
    // Alice USDC: 10000 - 6500 = 3500
    assert_eq!(alice_usdc, dec!(3500), "Alice USDC after buy = 10000 - 6500");

    // Verify Bob's balances
    let bob_xbt = total_balance(&exchange, BOB, "XBT");
    let bob_usdc = total_balance(&exchange, BOB, "USDC");

    // Bob: started with 1 XBT, sold 0.1 → 0.9 XBT remaining
    assert_eq!(bob_xbt, dec!(0.9), "Bob XBT after sell = 1 - 0.1");
    // Bob USDC: 0 + 6500 - taker fee
    // taker fee on sell = received_quote * taker_fee_rate = 6500 * 0.002 = 13 USDC
    assert_eq!(bob_usdc, dec!(6487), "Bob USDC after sell = 6500 - 13 taker fee");

    // Verify: total system balance unchanged (conservation)
    let sys_usdc_after = total_balance(&exchange, ALICE, "USDC")
        + total_balance(&exchange, BOB, "USDC")
        + total_balance(&exchange, TREASURY, "USDC");
    let sys_xbt_after = total_balance(&exchange, ALICE, "XBT")
        + total_balance(&exchange, BOB, "XBT")
        + total_balance(&exchange, TREASURY, "XBT");

    assert_eq!(
        sys_usdc_before, sys_usdc_after,
        "Total USDC in system must be conserved"
    );
    assert_eq!(
        sys_xbt_before, sys_xbt_after,
        "Total XBT in system must be conserved"
    );
}

// =============================================================================
// Scenario 2: Futures Position Lifecycle
// =============================================================================

#[tokio::test]
async fn scenario_futures_position_lifecycle() {
    let exchange = setup_futures_exchange();

    exchange.create_account(ALICE);
    exchange.create_account(BOB);
    exchange.deposit(ALICE, "USDT", dec!(10000)).unwrap();
    exchange.deposit(BOB, "USDT", dec!(100000)).unwrap();

    let symbol = Symbol::new("BTCUSDT");

    // Set up index prices so mark price works
    {
        let mut agg = exchange.index_aggregator.lock().unwrap_or_else(|e| e.into_inner());
        agg.update_source_price(&symbol, "Binance", dec!(65000), chrono::Utc::now());
        agg.update_source_price(&symbol, "Coinbase", dec!(65000), chrono::Utc::now());
        agg.update_source_price(&symbol, "Kraken", dec!(65000), chrono::Utc::now());
    }

    // Bob rests sell @ 65,000 (maker)
    exchange
        .place_order(make_order(
            BOB,
            "BTCUSDT",
            Side::Sell,
            OrderType::Limit,
            dec!(65000),
            dec!(0.1),
        ))
        .unwrap();

    let alice_before = total_balance(&exchange, ALICE, "USDT");

    // Alice buys 0.1 BTC @ 65,000, leverage 10x
    // margin = 0.1 * 65000 / 10 = 650 USDT
    // taker fee = 0.1 * 65000 * 0.0005 = 3.25 USDT
    exchange
        .place_order(make_order(
            ALICE,
            "BTCUSDT",
            Side::Buy,
            OrderType::Limit,
            dec!(65000),
            dec!(0.1),
        ))
        .unwrap();

    let alice_after_open = total_balance(&exchange, ALICE, "USDT");
    let expected_deduction = dec!(650) + dec!(3.25); // margin + fee
    assert_eq!(
        alice_before - alice_after_open,
        expected_deduction,
        "Alice must be debited margin ({}) + taker fee ({})",
        dec!(650),
        dec!(3.25)
    );

    // Verify position created
    {
        let pm = exchange.positions.lock();
        let pos = pm.get_position(ALICE, &symbol, 0).expect("Alice must have a position");
        assert_eq!(pos.size, dec!(0.1));
        assert_eq!(pos.entry_price, dec!(65000));
        assert!(!pos.is_closed());
    }

    // Verify unrealized PnL at mark price 66,000
    {
        let pm = exchange.positions.lock();
        let pos = pm.get_position(ALICE, &symbol, 0).unwrap();
        let upnl = pos.unrealized_pnl(dec!(66000));
        // Long 0.1 BTC: (66000 - 65000) * 0.1 = 100
        assert_eq!(upnl, dec!(100), "Unrealized PnL at 66000 should be +100");
    }

    // Close position: Bob rests buy @ 66,000, Alice sells @ 66,000
    // Update circuit breaker so 66000 is accepted
    {
        let mut cbs = exchange.circuit_breakers.lock();
        if let Some(cb) = cbs.get_mut(&symbol) {
            cb.update_reference(dec!(66000));
        }
    }

    exchange
        .place_order(make_order(
            BOB,
            "BTCUSDT",
            Side::Buy,
            OrderType::Limit,
            dec!(66000),
            dec!(0.1),
        ))
        .unwrap();

    exchange
        .place_order(make_order(
            ALICE,
            "BTCUSDT",
            Side::Sell,
            OrderType::Limit,
            dec!(66000),
            dec!(0.1),
        ))
        .unwrap();

    let alice_after_close = total_balance(&exchange, ALICE, "USDT");

    // Position must be closed
    {
        let pm = exchange.positions.lock();
        let pos = pm.get_position(ALICE, &symbol, 0).unwrap();
        assert!(pos.is_closed(), "Position must be fully closed");
    }

    // Realized PnL = (66000 - 65000) * 0.1 = 100
    // Close fee = 0.1 * 66000 * 0.0005 = 3.3
    // Net credit on close = released_margin (650) + PnL (100) - fee (3.3) = 746.7
    // Total net change = -653.25 (open) + 746.7 (close) = +93.45
    // So alice_after_close = 10000 + 100 - 3.25 - 3.3 = 10093.45
    let expected_final = dec!(10000) + dec!(100) - dec!(3.25) - dec!(3.3);
    assert_eq!(
        alice_after_close, expected_final,
        "Final balance = initial + PnL - open_fee - close_fee"
    );
}

// =============================================================================
// Scenario 3: Liquidation Flow
// =============================================================================

#[tokio::test]
async fn scenario_liquidation_flow() {
    use exchange_common::futures::PositionSide;
    use exchange_risk_engine::LiquidationEngine;

    let exchange = setup_futures_exchange();

    exchange.create_account(ALICE);
    exchange.create_account(BOB);
    // Alice gets 10,000 USDT — enough for position + margin on close
    // (reduce_only orders still pass through the margin pre-flight check)
    exchange.deposit(ALICE, "USDT", dec!(10000)).unwrap();
    exchange.deposit(BOB, "USDT", dec!(100000)).unwrap();

    let symbol = Symbol::new("BTCUSDT");

    // Set index price to 50,000 for mark price
    {
        let mut agg = exchange.index_aggregator.lock().unwrap_or_else(|e| e.into_inner());
        agg.update_source_price(&symbol, "Binance", dec!(50000), chrono::Utc::now());
        agg.update_source_price(&symbol, "Coinbase", dec!(50000), chrono::Utc::now());
        agg.update_source_price(&symbol, "Kraken", dec!(50000), chrono::Utc::now());
    }

    // Bob rests sell @ 50,000
    exchange
        .place_order(make_order(
            BOB,
            "BTCUSDT",
            Side::Sell,
            OrderType::Limit,
            dec!(50000),
            dec!(1),
        ))
        .unwrap();

    // Alice opens long 1 BTC @ 50,000, leverage 10x
    // margin = 1 * 50000 / 10 = 5000
    // taker fee = 1 * 50000 * 0.0005 = 25
    // Alice has 10000 USDT, so after open: 10000 - 5000 - 25 = 4975
    exchange
        .place_order(make_order(
            ALICE,
            "BTCUSDT",
            Side::Buy,
            OrderType::Limit,
            dec!(50000),
            dec!(1),
        ))
        .unwrap();

    // Verify position
    {
        let pm = exchange.positions.lock();
        let pos = pm.get_position(ALICE, &symbol, 0).unwrap();
        assert_eq!(pos.size, dec!(1));
    }

    // Drop price to liquidation territory: 45,200
    // effective margin = margin + upnl = 5000 + 1*(45200 - 50000) = 5000 - 4800 = 200
    // maintenance = 1 * 45200 * 0.005 = 226
    // 200 < 226 → liquidatable
    {
        let mut agg = exchange.index_aggregator.lock().unwrap_or_else(|e| e.into_inner());
        agg.update_source_price(&symbol, "Binance", dec!(45200), chrono::Utc::now());
        agg.update_source_price(&symbol, "Coinbase", dec!(45200), chrono::Utc::now());
        agg.update_source_price(&symbol, "Kraken", dec!(45200), chrono::Utc::now());
    }

    // Update circuit breaker reference
    {
        let mut cbs = exchange.circuit_breakers.lock();
        if let Some(cb) = cbs.get_mut(&symbol) {
            cb.update_reference(dec!(45200));
        }
    }

    // Place resting buy from Bob at 45,200 so liquidation sell can fill
    exchange
        .place_order(make_order(
            BOB,
            "BTCUSDT",
            Side::Buy,
            OrderType::Limit,
            dec!(45200),
            dec!(2), // plenty of liquidity
        ))
        .unwrap();

    // Run liquidation engine
    let mark_prices_full = exchange.get_mark_prices();
    let mark_prices: HashMap<_, _> = mark_prices_full
        .iter()
        .map(|(s, mp)| (s.clone(), mp.price))
        .collect();

    let positions: Vec<_> = {
        let pm = exchange.positions.lock();
        pm.all_positions().into_iter().cloned().collect()
    };

    // S17: load_full() returns Arc<HashMap> — no mutex, no clone.
    let perp_configs = exchange.perp_configs.load_full();

    let engine = LiquidationEngine::new(50);
    let batch = engine.check_all_positions(positions.iter(), &mark_prices, &perp_configs);

    assert!(
        !batch.orders.is_empty(),
        "Alice's position must be flagged for liquidation"
    );

    // Execute liquidation
    for order in &batch.orders {
        assert_eq!(order.user_id, ALICE);
        let close_side = match order.side {
            PositionSide::Long => Side::Sell,
            PositionSide::Short => Side::Buy,
        };
        let cmd = make_reduce_order(
            order.user_id,
            &order.symbol.to_string(),
            close_side,
            OrderType::Market,
            dec!(0),
            order.size,
        );
        let result = exchange.place_order(cmd);
        assert!(result.is_ok(), "Liquidation close order must succeed: {:?}", result.err());
    }

    // Position must be closed
    {
        let pm = exchange.positions.lock();
        let pos = pm.get_position(ALICE, &symbol, 0).unwrap();
        assert!(pos.is_closed(), "Position must be closed after liquidation");
    }
}

// =============================================================================
// Scenario 4: Funding Rate Settlement
// =============================================================================

#[tokio::test]
async fn scenario_funding_rate_settlement() {
    use exchange_settlement::FundingSettlement;

    let exchange = setup_futures_exchange();

    exchange.create_account(ALICE);
    exchange.create_account(BOB);
    exchange.deposit(ALICE, "USDT", dec!(100000)).unwrap();
    exchange.deposit(BOB, "USDT", dec!(100000)).unwrap();

    let symbol = Symbol::new("BTCUSDT");

    // Set index prices
    {
        let mut agg = exchange.index_aggregator.lock().unwrap_or_else(|e| e.into_inner());
        agg.update_source_price(&symbol, "Binance", dec!(65000), chrono::Utc::now());
        agg.update_source_price(&symbol, "Coinbase", dec!(65000), chrono::Utc::now());
        agg.update_source_price(&symbol, "Kraken", dec!(65000), chrono::Utc::now());
    }

    // Alice opens long (buys from Bob's resting sell)
    exchange
        .place_order(make_order(
            BOB,
            "BTCUSDT",
            Side::Sell,
            OrderType::Limit,
            dec!(65000),
            dec!(0.1),
        ))
        .unwrap();

    exchange
        .place_order(make_order(
            ALICE,
            "BTCUSDT",
            Side::Buy,
            OrderType::Limit,
            dec!(65000),
            dec!(0.1),
        ))
        .unwrap();

    // Bob is now short (he sold and got position opened)
    // Alice is now long

    // Verify positions exist
    {
        let pm = exchange.positions.lock();
        let alice_pos = pm.get_position(ALICE, &symbol, 0);
        let bob_pos = pm.get_position(BOB, &symbol, 0);
        assert!(alice_pos.is_some(), "Alice must have a position");
        assert!(bob_pos.is_some(), "Bob must have a position");
    }

    // Capture balances before funding
    let alice_before = total_balance(&exchange, ALICE, "USDT");
    let bob_before = total_balance(&exchange, BOB, "USDT");

    // Collect positions for funding settlement
    let positions: Vec<_> = {
        let pm = exchange.positions.lock();
        pm.all_positions()
            .into_iter()
            .filter(|p| p.symbol == symbol)
            .cloned()
            .collect()
    };

    // Positive funding rate: longs pay, shorts receive
    // funding_rate = 0.01% = 0.0001
    let funding_rate = dec!(0.0001);
    let mark_price = dec!(65000);
    let now = chrono::Utc::now();

    let payments = FundingSettlement::settle(&positions, funding_rate, mark_price, now);

    assert!(
        !payments.is_empty(),
        "Funding settlement must produce payments"
    );

    // Apply payments to balances (replicating what the funding task does)
    for payment in &payments {
        if payment.amount > Decimal::ZERO {
            exchange
                .accounts
                .deposit(payment.user_id, "USDT", payment.amount)
                .unwrap();
        } else if payment.amount < Decimal::ZERO {
            let debit = payment.amount.abs();
            exchange
                .accounts
                .withdraw(payment.user_id, "USDT", debit)
                .unwrap();
        }
    }

    let alice_after = total_balance(&exchange, ALICE, "USDT");
    let bob_after = total_balance(&exchange, BOB, "USDT");

    // Positive funding rate → longs pay, shorts receive
    // Payment = size * mark_price * rate = 0.1 * 65000 * 0.0001 = 0.65 USDT
    let expected_payment = dec!(0.1) * dec!(65000) * dec!(0.0001); // = 0.65

    // Alice (long) pays: balance decreased
    assert_eq!(
        alice_before - alice_after,
        expected_payment,
        "Alice (long) must pay funding"
    );

    // Bob (short) receives: balance increased
    assert_eq!(
        bob_after - bob_before,
        expected_payment,
        "Bob (short) must receive funding"
    );

    // Zero-sum: total payments sum to zero
    let total: Decimal = payments.iter().map(|p| p.amount).sum();
    assert_eq!(total, dec!(0), "Funding payments must sum to zero");
}

// =============================================================================
// Scenario 5: Cancel and Balance Restore
// =============================================================================

#[tokio::test]
async fn scenario_cancel_restores_balance() {
    let exchange = setup_spot_exchange();

    exchange.create_account(ALICE);
    exchange.deposit(ALICE, "USDC", dec!(10000)).unwrap();

    // Place 5 limit buy orders at different prices
    let prices = [dec!(60000), dec!(61000), dec!(62000), dec!(63000), dec!(64000)];
    let qty = dec!(0.01); // small qty so we don't exceed 10000

    let mut order_ids = Vec::new();
    let mut total_locked_expected = dec!(0);

    for &price in &prices {
        let cmd = make_order(ALICE, "XBTUSDC", Side::Buy, OrderType::Limit, price, qty);
        let oid = cmd.order_id;
        exchange.place_order(cmd).unwrap();
        order_ids.push(oid);
        total_locked_expected += price * qty;
    }

    // Verify locked amount
    let (_, locked) = get_balance(&exchange, ALICE, "USDC");
    assert_eq!(
        locked, total_locked_expected,
        "Total locked must equal sum of all order values"
    );

    // Cancel 2 orders
    let cancel_value_0 = prices[0] * qty;
    let cancel_value_1 = prices[1] * qty;

    exchange
        .cancel_order(CancelOrderCommand {
            order_id: order_ids[0],
            user_id: ALICE,
            symbol: Symbol::new("XBTUSDC"),
            timestamp: chrono::Utc::now(),
        })
        .unwrap();

    exchange
        .cancel_order(CancelOrderCommand {
            order_id: order_ids[1],
            user_id: ALICE,
            symbol: Symbol::new("XBTUSDC"),
            timestamp: chrono::Utc::now(),
        })
        .unwrap();

    // Verify locked decreased by exactly those 2 orders' values
    let (_, locked_after_2) = get_balance(&exchange, ALICE, "USDC");
    assert_eq!(
        locked_after_2,
        total_locked_expected - cancel_value_0 - cancel_value_1,
        "Locked must decrease by cancelled order values"
    );

    // Cancel remaining 3 orders
    for &oid in &order_ids[2..] {
        exchange
            .cancel_order(CancelOrderCommand {
                order_id: oid,
                user_id: ALICE,
                symbol: Symbol::new("XBTUSDC"),
                timestamp: chrono::Utc::now(),
            })
            .unwrap();
    }

    // Verify balance fully restored
    let (avail_final, locked_final) = get_balance(&exchange, ALICE, "USDC");
    assert_eq!(locked_final, dec!(0), "No balance should be locked");
    assert_eq!(
        avail_final,
        dec!(10000),
        "Full balance must be restored after all cancels"
    );
}

// =============================================================================
// Scenario 6: Partial Fill
// =============================================================================

#[tokio::test]
async fn scenario_partial_fill() {
    let exchange = setup_spot_exchange();

    exchange.create_account(ALICE);
    exchange.create_account(BOB);
    exchange.create_account(CAROL);
    exchange.deposit(ALICE, "USDC", dec!(100000)).unwrap();
    exchange.deposit(BOB, "XBT", dec!(10)).unwrap();
    exchange.deposit(CAROL, "XBT", dec!(10)).unwrap();

    // Alice places limit buy 1 XBT @ 65,000
    let alice_order = make_order(
        ALICE,
        "XBTUSDC",
        Side::Buy,
        OrderType::Limit,
        dec!(65000),
        dec!(1),
    );
    exchange.place_order(alice_order).unwrap();

    // Verify full lock: 65000 USDC
    let (_, locked) = get_balance(&exchange, ALICE, "USDC");
    assert_eq!(locked, dec!(65000), "Full order value must be locked");

    // Bob sells 0.3 XBT at market
    exchange
        .place_order(make_order(
            BOB,
            "XBTUSDC",
            Side::Sell,
            OrderType::Market,
            dec!(0),
            dec!(0.3),
        ))
        .unwrap();

    // Alice should have 0.3 XBT (minus maker fee)
    let alice_xbt = total_balance(&exchange, ALICE, "XBT");
    // maker fee on buy = 0.3 * 0.001 = 0.0003
    assert_eq!(
        alice_xbt,
        dec!(0.3) - dec!(0.3) * dec!(0.001),
        "Alice must have 0.3 XBT minus maker fee"
    );

    // Remaining locked = 0.7 * 65000 = 45500
    let (_, locked_after_partial) = get_balance(&exchange, ALICE, "USDC");
    assert_eq!(
        locked_after_partial,
        dec!(45500),
        "Remaining locked must be 0.7 * 65000 = 45500"
    );

    // Carol sells 0.7 XBT at market — fills the rest
    exchange
        .place_order(make_order(
            CAROL,
            "XBTUSDC",
            Side::Sell,
            OrderType::Market,
            dec!(0),
            dec!(0.7),
        ))
        .unwrap();

    // Alice's order should be fully filled now
    let (_, locked_final) = get_balance(&exchange, ALICE, "USDC");
    assert_eq!(locked_final, dec!(0), "No lock remaining after full fill");

    // Alice total XBT = (0.3 + 0.7) - maker fee on total
    // maker fee = 1.0 * 0.001 = 0.001
    let alice_xbt_final = total_balance(&exchange, ALICE, "XBT");
    assert_eq!(
        alice_xbt_final,
        dec!(0.999),
        "Alice must have 1 XBT minus 0.001 maker fee"
    );

    // Alice USDC: 100000 - 65000 = 35000
    let alice_usdc_final = total_balance(&exchange, ALICE, "USDC");
    assert_eq!(
        alice_usdc_final,
        dec!(35000),
        "Alice USDC = 100000 - 65000"
    );
}

// =============================================================================
// Scenario 7: Referral Commission
// =============================================================================

#[tokio::test]
async fn scenario_referral_commission() {
    use exchange_api_gateway::rest_engagement::distribute_affiliate_commissions;

    let exchange = setup_spot_exchange();

    exchange.create_account(ALICE); // referrer
    exchange.create_account(BOB); // referee
    exchange.deposit(BOB, "USDC", dec!(100000)).unwrap();
    exchange.deposit(BOB, "XBT", dec!(10)).unwrap();

    // Set up referral tree: Bob was referred by Alice
    {
        let mut tree = exchange.referral_tree.lock().unwrap_or_else(|e| e.into_inner());
        tree.insert(BOB, ALICE);
    }

    // Bob generates a trade with fee of 100 USDT
    // We simulate this by calling distribute_affiliate_commissions directly
    // (In production, this would be called from the trade settlement path if wired up)
    let fee_amount = dec!(100);
    distribute_affiliate_commissions(&exchange, BOB, fee_amount);

    // Verify: Alice receives tier-1 commission (30%)
    let alice_usdt = total_balance(&exchange, ALICE, "USDT");
    // TIER1_RATE = 0.30
    let expected_commission = dec!(100) * dec!(0.30);
    assert_eq!(
        alice_usdt, expected_commission,
        "Referrer (Alice) must receive 30% of fee as tier-1 commission"
    );

    // Verify commission log
    {
        let commissions = exchange.affiliate_commissions.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(commissions.len(), 1, "One commission entry must be recorded");
        let entry = commissions.front().unwrap();
        assert_eq!(entry.earner_id, ALICE);
        assert_eq!(entry.trader_id, BOB);
        assert_eq!(entry.amount, expected_commission);
        assert_eq!(entry.tier, 1);
    }
}
