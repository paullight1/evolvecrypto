//! Market-maker workload benchmarks.
//!
//! Motivation: `throughput.rs` exercises the engine with one order per price
//! level, which hides the O(n) cancel inside `PriceLevel::remove()`. Real MM
//! flow places hundreds of orders at the same price and sends a 20:1 cancel-
//! to-fill ratio, so cancellation cost dominates. These benches measure that
//! regime so we can prove (or disprove) the O(n) concern before optimising.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use exchange_common::*;
use exchange_matching_engine::MatchingEngine;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

const SYMBOL: &str = "BTCUSDT";
const BID_PRICE: Decimal = dec!(49999);

fn make_engine() -> MatchingEngine {
    let mut engine = MatchingEngine::new();
    engine.add_symbol(Symbol::new(SYMBOL));
    engine
}

fn limit_at(
    user_id: uuid::Uuid,
    order_id: uuid::Uuid,
    side: Side,
    price: Decimal,
    qty: Decimal,
) -> Command {
    Command::PlaceOrder(PlaceOrderCommand {
        order_id,
        user_id,
        symbol: Symbol::new(SYMBOL),
        side,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::Gtc,
        price,
        qty,
        reduce_only: false,
        timestamp: chrono::Utc::now(),
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

fn cancel(order_id: uuid::Uuid, user_id: uuid::Uuid) -> Command {
    Command::CancelOrder(CancelOrderCommand {
        order_id,
        user_id,
        symbol: Symbol::new(SYMBOL),
        timestamp: chrono::Utc::now(),
    })
}

/// Fill one price level with `depth` resting bids at BID_PRICE.
/// Returns (engine, vec of (order_id, user_id) in FIFO order).
fn seed_level(depth: usize) -> (MatchingEngine, Vec<(uuid::Uuid, uuid::Uuid)>) {
    let mut engine = make_engine();
    let mut ids = Vec::with_capacity(depth);
    for _ in 0..depth {
        let uid = uuid::Uuid::new_v4();
        let oid = uuid::Uuid::new_v4();
        engine.process(limit_at(uid, oid, Side::Buy, BID_PRICE, dec!(1)));
        ids.push((oid, uid));
    }
    (engine, ids)
}

// ---------------------------------------------------------------------------
// Cancel cost as a function of position within a deep price level.
//
// Worst case: cancel the MIDDLE order of a level with N resting orders.
// If `PriceLevel::remove()` is O(n), latency grows linearly with N.
// ---------------------------------------------------------------------------

fn bench_cancel_middle_of_deep_level(c: &mut Criterion) {
    let mut group = c.benchmark_group("mm/cancel_middle_of_level");

    for depth in [10usize, 50, 100, 500, 1000] {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::from_parameter(depth),
            &depth,
            |b, &depth| {
                b.iter_batched(
                    || {
                        let (engine, ids) = seed_level(depth);
                        // Cancel the middle order — worst-case for linear scan
                        // from either end of a VecDeque.
                        let mid = depth / 2;
                        let (oid, uid) = ids[mid];
                        (engine, oid, uid)
                    },
                    |(mut engine, oid, uid)| {
                        let events = engine.process(cancel(oid, uid));
                        std::hint::black_box(events);
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Cancel cost for front vs back vs middle at fixed depth=500.
// VecDeque::remove is O(min(pos, len-pos)) internally, but we linearly scan
// from .iter().position() so front is the one that matters for our path.
// This isolates scan cost from shift cost.
// ---------------------------------------------------------------------------

fn bench_cancel_position_in_level(c: &mut Criterion) {
    let mut group = c.benchmark_group("mm/cancel_position_depth_500");
    group.throughput(Throughput::Elements(1));

    let positions: [(&str, fn(usize) -> usize); 4] = [
        ("front", |_| 0),
        ("quarter", |d| d / 4),
        ("middle", |d| d / 2),
        ("back", |d| d - 1),
    ];

    for (label, pos_fn) in positions {
        group.bench_function(label, |b| {
            b.iter_batched(
                || {
                    let (engine, ids) = seed_level(500);
                    let (oid, uid) = ids[pos_fn(500)];
                    (engine, oid, uid)
                },
                |(mut engine, oid, uid)| {
                    let events = engine.process(cancel(oid, uid));
                    std::hint::black_box(events);
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Realistic MM flow: cancel-replace on a hot level.
// Each iteration: cancel one resting order, place a new one at the same price.
// Measures the steady-state cost an MM pays to keep a quote refreshed.
// ---------------------------------------------------------------------------

fn bench_cancel_replace_cycle(c: &mut Criterion) {
    let mut group = c.benchmark_group("mm/cancel_replace_cycle");
    group.throughput(Throughput::Elements(1));

    for depth in [10usize, 100, 500] {
        group.bench_with_input(
            BenchmarkId::from_parameter(depth),
            &depth,
            |b, &depth| {
                b.iter_batched(
                    || seed_level(depth),
                    |(mut engine, ids)| {
                        // Cancel the oldest order (front of queue) and place
                        // a fresh one. This is the canonical MM refresh.
                        let (oid, uid) = ids[0];
                        let c_events = engine.process(cancel(oid, uid));
                        let p_events = engine.process(limit_at(
                            uid,
                            uuid::Uuid::new_v4(),
                            Side::Buy,
                            BID_PRICE,
                            dec!(1),
                        ));
                        std::hint::black_box((c_events, p_events));
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Mixed flow: 95% cancel-replace, 5% fills.
// Simulates a market maker quoting both sides while a taker occasionally
// sweeps the top. Measures aggregate throughput, not single-op latency.
// ---------------------------------------------------------------------------

fn bench_mm_mixed_flow(c: &mut Criterion) {
    let mut group = c.benchmark_group("mm/mixed_flow");
    // 1000 operations per iteration, 95/5 cancel-replace / fill split.
    group.throughput(Throughput::Elements(1000));
    group.sample_size(20); // Heavier iterations — fewer samples.

    group.bench_function("depth_100_ops_1000", |b| {
        b.iter_batched(
            || seed_level(100),
            |(mut engine, mut ids)| {
                let mut next_fresh = 0usize;
                let mut fill_user = uuid::Uuid::new_v4();

                for i in 0..1000u32 {
                    if i % 20 == 0 {
                        // 5% — taker fill against the front of the book.
                        let ev = engine.process(limit_at(
                            fill_user,
                            uuid::Uuid::new_v4(),
                            Side::Sell,
                            BID_PRICE,
                            dec!(1),
                        ));
                        std::hint::black_box(ev);
                        // The fill consumed the front resting order; drop its id.
                        if !ids.is_empty() {
                            ids.remove(0);
                        }
                        fill_user = uuid::Uuid::new_v4();
                    } else {
                        // 95% — cancel oldest and re-place at the same level.
                        if let Some((oid, uid)) = ids.first().copied() {
                            engine.process(cancel(oid, uid));
                            ids.remove(0);
                            let new_oid = uuid::Uuid::new_v4();
                            engine.process(limit_at(
                                uid,
                                new_oid,
                                Side::Buy,
                                BID_PRICE,
                                dec!(1),
                            ));
                            ids.push((new_oid, uid));
                            next_fresh += 1;
                        }
                    }
                }
                std::hint::black_box(next_fresh);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Baseline: cancel on a level with exactly 1 order. Same as throughput.rs
// cancel_order — duplicated here so we can compare deep vs shallow in one run.
// ---------------------------------------------------------------------------

fn bench_cancel_shallow_baseline(c: &mut Criterion) {
    let mut group = c.benchmark_group("mm/cancel_shallow_baseline");
    group.throughput(Throughput::Elements(1));

    group.bench_function("depth_1", |b| {
        b.iter_batched(
            || seed_level(1),
            |(mut engine, ids)| {
                let (oid, uid) = ids[0];
                let events = engine.process(cancel(oid, uid));
                std::hint::black_box(events);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group!(
    mm_benches,
    bench_cancel_shallow_baseline,
    bench_cancel_middle_of_deep_level,
    bench_cancel_position_in_level,
    bench_cancel_replace_cycle,
    bench_mm_mixed_flow,
);
criterion_main!(mm_benches);
