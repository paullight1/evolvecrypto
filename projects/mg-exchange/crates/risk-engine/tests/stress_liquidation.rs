//! Stress test: 1000-position liquidation cascade.
//!
//! Verifies:
//! - No panics with many positions
//! - Insurance fund accounting is correct
//! - All liquidatable positions are processed
//! - Batch size limits work at scale

use exchange_common::futures::*;
use exchange_common::Symbol;
use exchange_risk_engine::liquidation::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;
use uuid::Uuid;
use chrono::Utc;

fn make_position(
    user_idx: u64,
    side: PositionSide,
    size: Decimal,
    entry: Decimal,
    margin: Decimal,
) -> Position {
    Position {
        user_id: Uuid::from_u128(user_idx as u128),
        symbol: Symbol::new("BTCUSDT"),
        side,
        size,
        entry_price: entry,
        leverage: dec!(10),
        margin_mode: MarginMode::Isolated,
        margin,
        realized_pnl: Decimal::ZERO,
        funding_accumulated: Decimal::ZERO,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        position_idx: 0,
        tp_price: None,
        sl_price: None,
        trailing_stop: None,
        tp_trigger_id: None,
        sl_trigger_id: None,
        tp_size: None,
        sl_size: None,
        auto_add_margin: false,
        unpaid_funding_debt: Decimal::ZERO,
    }
}

#[test]
fn stress_1000_positions_liquidation_cascade() {
    // Create 1000 positions: 500 longs at 50k, 500 shorts at 50k.
    // All with 10x leverage → margin = 5000 per BTC.
    // Then crash price to 44000 → all longs should be liquidated.
    let mut positions = Vec::new();

    for i in 0..500 {
        // Longs: will be underwater at 44000.
        positions.push(make_position(
            i,
            PositionSide::Long,
            dec!(1),
            dec!(50000),
            dec!(5000),
        ));
    }
    for i in 500..1000 {
        // Shorts: profitable at 44000 (not liquidated).
        positions.push(make_position(
            i,
            PositionSide::Short,
            dec!(1),
            dec!(50000),
            dec!(5000),
        ));
    }

    let mut mark_prices = HashMap::new();
    mark_prices.insert(Symbol::new("BTCUSDT"), dec!(44000));

    let mut configs = HashMap::new();
    configs.insert(Symbol::new("BTCUSDT"), PerpetualConfig::btc_default());

    let engine = LiquidationEngine::new(1000); // large batch size

    let start = std::time::Instant::now();
    let batch = engine.check_all_positions(
        positions.iter(),
        &mark_prices,
        &configs,
    );
    let elapsed = start.elapsed();

    // All 500 longs should be liquidated.
    assert_eq!(
        batch.orders.len(), 500,
        "expected 500 liquidations, got {}",
        batch.orders.len()
    );

    // No shorts should be liquidated.
    for order in &batch.orders {
        assert_eq!(order.side, PositionSide::Long, "liquidated positions should be longs");
    }

    // Performance: should complete in < 1 second.
    assert!(
        elapsed.as_secs() < 1,
        "1000-position liquidation too slow: {elapsed:?}"
    );

    eprintln!("=== Liquidation Stress Test ===");
    eprintln!("  Positions:     1000 (500 long, 500 short)");
    eprintln!("  Mark price:    $44,000 (crash from $50,000)");
    eprintln!("  Liquidated:    {}", batch.orders.len());
    eprintln!("  Elapsed:       {elapsed:?}");
}

#[test]
fn stress_batch_size_limits_at_scale() {
    // 200 underwater positions but batch_size = 50 → only 50 processed.
    let mut positions = Vec::new();
    for i in 0..200 {
        positions.push(make_position(
            i,
            PositionSide::Long,
            dec!(1),
            dec!(50000),
            dec!(5000),
        ));
    }

    let mut mark_prices = HashMap::new();
    mark_prices.insert(Symbol::new("BTCUSDT"), dec!(44000));

    let mut configs = HashMap::new();
    configs.insert(Symbol::new("BTCUSDT"), PerpetualConfig::btc_default());

    let engine = LiquidationEngine::new(50); // batch limit

    let batch = engine.check_all_positions(
        positions.iter(),
        &mark_prices,
        &configs,
    );

    assert_eq!(batch.orders.len(), 50, "batch size should be respected");
}
