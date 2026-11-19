//! Deep-book benchmark — Sprint 1 (T3.4).
//!
//! Purpose: prove and measure the cost of the `keys().cloned().collect()` call
//! in `match_order` (orderbook.rs:210-213). That call allocates a `Vec<Price>`
//! proportional to ALL levels in the opposite side, even when the incoming
//! market order only touches the best level.
//!
//! This bench will be used as the BEFORE measurement. Sprint 2 replaces the
//! Vec with a lazy iterator and we expect depth=5000 to drop from the measured
//! cost (after Fix 1 — see BENCHMARKS.md) to ≤ 500 ns.
//!
//! Parametric depths: [100, 500, 1000, 5000].
//! Setup: sparse ask book where each level holds exactly 1 order of qty=1.
//! Measured operation: a single market buy of qty=1 that fills ONLY the best
//! (lowest) ask level and leaves the rest of the book untouched.
//!
//! ## Methodology (Sprint 1 fix-up)
//!
//! The original Sprint 1 pass used `BatchSize::SmallInput` which included the
//! drop cost of the `MatchingEngine` (with 5000 BTreeMap entries) inside the
//! measured window. At depth=5000 the teardown alone dominated the 239 µs
//! number. We now use `iter_custom` to time ONLY `engine.process(cmd)`,
//! dropping the engine outside the timed region entirely.

use criterion::{Criterion, BenchmarkId, Throughput, criterion_group, criterion_main};
use exchange_common::*;
use exchange_matching_engine::MatchingEngine;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::time::Instant;

const SYMBOL: &str = "BTCUSDT";

/// Build an engine with `depth` resting ask orders.
///
/// Prices span [50_000.00 .. 50_000.00 + depth * 0.01] in 0.01 steps.
/// Each level holds exactly 1 order of qty=1, so a market buy of qty=1
/// fills ONLY the best (cheapest) level and terminates.
fn build_sparse_ask_book(depth: usize) -> MatchingEngine {
    let mut engine = MatchingEngine::new();
    engine.add_symbol(Symbol::new(SYMBOL));

    // Prices: 50_000.00, 50_000.01, 50_000.02, …
    // Represented as integers with 2 decimal places: 5_000_000 + i (scale=2).
    for i in 0..depth {
        let price = Decimal::new(5_000_000_i64 + i as i64, 2); // e.g. 50000.00 + i*0.01
        engine.process(Command::PlaceOrder(PlaceOrderCommand {
            order_id: uuid::Uuid::from_u128(i as u128 + 1),
            user_id: uuid::Uuid::from_u128(i as u128 + 1_000_000),
            symbol: Symbol::new(SYMBOL),
            side: Side::Sell,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
            price,
            qty: dec!(1),
            reduce_only: false,
            timestamp: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            expire_at: None,
            position_idx: 0,
            iceberg_qty: None,
            order_link_id: None,
            slippage_tolerance: None,
            close_position: false,
            stp_mode: StpMode::default(),
            request_expire_time: None,
        }));
    }

    engine
}

/// Market buy command that fills exactly the top-of-book level (qty=1).
/// Uses deterministic IDs so both caller sites produce the same command bytes.
fn touch_top_buy(cmd_idx: u128) -> Command {
    Command::PlaceOrder(PlaceOrderCommand {
        order_id: uuid::Uuid::from_u128(0xDEAD_BEEF_0000_0000_u128 | cmd_idx),
        user_id: uuid::Uuid::from_u128(0xCAFE_BABE_0000_0000_u128 | cmd_idx),
        symbol: Symbol::new(SYMBOL),
        side: Side::Buy,
        order_type: OrderType::Market,
        time_in_force: TimeInForce::Gtc,
        price: Decimal::ZERO,
        qty: dec!(1),
        reduce_only: false,
        timestamp: chrono::DateTime::from_timestamp(1_700_000_001, 0).unwrap(),
        expire_at: None,
        position_idx: 0,
        iceberg_qty: None,
        order_link_id: None,
        slippage_tolerance: None,
        close_position: false,
        stp_mode: StpMode::default(),
        request_expire_time: None,
    })
}

/// Parametric benchmark: for each depth in [100, 500, 1000, 5000], measure
/// the cost of a single market buy that touches only the best ask level.
///
/// Uses `iter_custom` to time ONLY `engine.process(cmd)` — engine construction
/// and drop are outside the measured window. This avoids inflating the number
/// with BTreeMap teardown cost (which at depth=5000 is >100 µs).
///
/// PRE-FIX EXPECTATION (Sprint 1 baseline, corrected methodology):
///   depth=100  → sub-µs to low µs (small Vec alloc, short scan)
///   depth=5000 → in the 5-50 µs range (Vec<Price> allocation of 5000 elements)
///
/// POST-FIX TARGET (Sprint 2, after T1.1 lazy iterator fix):
///   All depths → ≤ 500 ns (lazy iterator, no allocation)
fn bench_deep_book_touch_top(c: &mut Criterion) {
    let mut group = c.benchmark_group("matching_engine/deep_book");

    for &depth in &[100usize, 500, 1000, 5000] {
        // Throughput is 1 match event (1 fill at top of book).
        group.throughput(Throughput::Elements(1));

        group.bench_with_input(
            BenchmarkId::from_parameter(depth),
            &depth,
            |b, &depth| {
                // iter_custom: we control the loop and the timer ourselves.
                // Engine is built BEFORE the timer starts and dropped AFTER it stops.
                b.iter_custom(|iters| {
                    let mut total = std::time::Duration::ZERO;
                    for i in 0..iters {
                        // Build the engine outside the timed region.
                        let mut engine = build_sparse_ask_book(depth);
                        let cmd = touch_top_buy(i.into());

                        // Time ONLY the process() call.
                        let start = Instant::now();
                        let events = engine.process(cmd);
                        total += start.elapsed();

                        // Prevent compiler from eliding the call.
                        std::hint::black_box(events);
                        // engine drops here — AFTER the timer has already stopped.
                    }
                    total
                });
            },
        );
    }

    group.finish();
}

criterion_group!(deep_book_benches, bench_deep_book_touch_top);
criterion_main!(deep_book_benches);
