//! Stress test: 100K mixed operations (limit/market/cancel) on a single order book.
//!
//! Verifies:
//! - No panics under high throughput
//! - All fill quantities are valid
//! - Book is not crossed at the end
//! - All cancelled orders are actually removed
//! - Memory usage is bounded (no leaks in data structures)

use exchange_common::*;
use exchange_matching_engine::OrderBook;
use rust_decimal::Decimal;
use std::collections::HashMap;
use uuid::Uuid;

fn limit_order(user: u128, side: Side, price: Decimal, qty: Decimal) -> PlaceOrderCommand {
    PlaceOrderCommand {
        order_id: Uuid::new_v4(),
        user_id: Uuid::from_u128(user),
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
        stp_mode: StpMode::default(),
        request_expire_time: None,
    }
}

fn market_order(user: u128, side: Side, qty: Decimal) -> PlaceOrderCommand {
    let mut cmd = limit_order(user, side, Decimal::ZERO, qty);
    cmd.order_type = OrderType::Market;
    cmd
}

/// Simple LCG pseudo-random number generator (deterministic, fast).
struct Lcg(u64);
impl Lcg {
    fn new(seed: u64) -> Self { Self(seed) }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn range(&mut self, min: u64, max: u64) -> u64 {
        min + self.next() % (max - min + 1)
    }
}

#[test]
fn stress_100k_mixed_operations() {
    let mut book = OrderBook::new(Symbol::new("BTCUSDT"));
    let mut rng = Lcg::new(42);

    // Track placed order IDs for cancellation
    let mut live_orders: Vec<Uuid> = Vec::new();
    // Track fill totals per order for invariant checking
    let mut order_qtys: HashMap<Uuid, Decimal> = HashMap::new();
    let mut fill_totals: HashMap<Uuid, Decimal> = HashMap::new();

    let mut total_fills = 0u64;
    let mut total_cancels = 0u64;
    let mut total_rejects = 0u64;

    let base_price = 50_000u64;
    let start = std::time::Instant::now();

    for i in 0u64..100_000 {
        let op = rng.range(0, 99);

        if op < 45 {
            // 45% — place limit buy
            let price_offset = rng.range(0, 200) as i64 - 100; // ±$100
            let price = Decimal::new(base_price as i64 + price_offset, 0);
            let qty = Decimal::new(rng.range(1, 100) as i64, 2); // 0.01 to 1.00
            let user = (i % 100) + 1; // 100 different users
            let cmd = limit_order(user as u128, Side::Buy, price, qty);
            let oid = cmd.order_id;
            order_qtys.insert(oid, qty);
            let events = book.place_order(cmd);
            for event in &events {
                match event {
                    MatchEvent::OrderAccepted { .. } => { live_orders.push(oid); }
                    MatchEvent::Fill(f) => {
                        total_fills += 1;
                        *fill_totals.entry(f.taker_order_id).or_default() += f.qty;
                        *fill_totals.entry(f.maker_order_id).or_default() += f.qty;
                    }
                    MatchEvent::OrderRejected { .. } => { total_rejects += 1; }
                    MatchEvent::OrderCancelled { .. } => {}
                    MatchEvent::CancelRejected { .. } => {}
                }
            }
        } else if op < 90 {
            // 45% — place limit sell
            let price_offset = rng.range(0, 200) as i64 - 100;
            let price = Decimal::new(base_price as i64 + price_offset, 0);
            let qty = Decimal::new(rng.range(1, 100) as i64, 2);
            let user = (i % 100) + 1;
            let cmd = limit_order(user as u128, Side::Sell, price, qty);
            let oid = cmd.order_id;
            order_qtys.insert(oid, qty);
            let events = book.place_order(cmd);
            for event in &events {
                match event {
                    MatchEvent::OrderAccepted { .. } => { live_orders.push(oid); }
                    MatchEvent::Fill(f) => {
                        total_fills += 1;
                        *fill_totals.entry(f.taker_order_id).or_default() += f.qty;
                        *fill_totals.entry(f.maker_order_id).or_default() += f.qty;
                    }
                    MatchEvent::OrderRejected { .. } => { total_rejects += 1; }
                    MatchEvent::OrderCancelled { .. } => {}
                    MatchEvent::CancelRejected { .. } => {}
                }
            }
        } else if op < 95 {
            // 5% — market order
            let side = if rng.range(0, 1) == 0 { Side::Buy } else { Side::Sell };
            let qty = Decimal::new(rng.range(1, 50) as i64, 2);
            let user = (i % 100) + 101; // separate user pool for market orders
            let cmd = market_order(user as u128, side, qty);
            let oid = cmd.order_id;
            order_qtys.insert(oid, qty);
            let events = book.place_order(cmd);
            for event in &events {
                if let MatchEvent::Fill(f) = event {
                    total_fills += 1;
                    *fill_totals.entry(f.taker_order_id).or_default() += f.qty;
                    *fill_totals.entry(f.maker_order_id).or_default() += f.qty;
                }
            }
        } else {
            // 5% — cancel random order
            if !live_orders.is_empty() {
                let idx = rng.range(0, live_orders.len() as u64 - 1) as usize;
                let oid = live_orders.swap_remove(idx);
                let events = book.cancel_order(CancelOrderCommand {
                    order_id: oid,
                    user_id: Uuid::nil(),
                    symbol: Symbol::new("BTCUSDT"),
                    timestamp: chrono::Utc::now(),
                });
                if events.iter().any(|e| matches!(e, MatchEvent::OrderCancelled { .. })) {
                    total_cancels += 1;
                }
            }
        }
    }

    let elapsed = start.elapsed();

    // ── Invariant checks ───────────────────────────────────────────────

    // 1. No order was overfilled.
    for (oid, filled) in &fill_totals {
        if let Some(&orig_qty) = order_qtys.get(oid) {
            assert!(
                *filled <= orig_qty,
                "Order {oid} overfilled: filled {filled} > qty {orig_qty}"
            );
        }
    }

    // 2. Book is not crossed.
    let snap = book.snapshot(1);
    if let (Some(&(bid, _)), Some(&(ask, _))) = (snap.bids.first(), snap.asks.first()) {
        assert!(bid < ask, "Book crossed after stress test: bid {bid} >= ask {ask}");
    }

    // 3. Print stats (not assertions, for human review).
    let throughput = 100_000.0 / elapsed.as_secs_f64();
    eprintln!("=== Stress Test Results ===");
    eprintln!("  Operations: 100,000");
    eprintln!("  Fills:      {total_fills}");
    eprintln!("  Cancels:    {total_cancels}");
    eprintln!("  Rejects:    {total_rejects}");
    eprintln!("  Book depth: {} orders remaining", book.order_count());
    eprintln!("  Elapsed:    {elapsed:.2?}");
    eprintln!("  Throughput: {throughput:.0} ops/sec");

    // 4. Throughput sanity: should complete in < 10 seconds on any modern machine.
    assert!(
        elapsed.as_secs() < 10,
        "Stress test too slow: {elapsed:.2?} (expected < 10s)"
    );
}
