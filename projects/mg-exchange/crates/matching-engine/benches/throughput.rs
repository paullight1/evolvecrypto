use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use exchange_common::*;
use exchange_matching_engine::MatchingEngine;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

const SYMBOL: &str = "BTCUSDT";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_engine() -> MatchingEngine {
    let mut engine = MatchingEngine::new();
    engine.add_symbol(Symbol::new(SYMBOL));
    engine
}

fn limit_cmd(side: Side, price: Decimal, qty: Decimal) -> Command {
    Command::PlaceOrder(PlaceOrderCommand {
        order_id: uuid::Uuid::new_v4(),
        user_id: uuid::Uuid::new_v4(),
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

fn market_cmd(side: Side, qty: Decimal) -> Command {
    Command::PlaceOrder(PlaceOrderCommand {
        order_id: uuid::Uuid::new_v4(),
        user_id: uuid::Uuid::new_v4(),
        symbol: Symbol::new(SYMBOL),
        side,
        order_type: OrderType::Market,
        time_in_force: TimeInForce::Gtc,
        price: Decimal::ZERO,
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

fn cancel_cmd(order_id: uuid::Uuid, user_id: uuid::Uuid) -> Command {
    Command::CancelOrder(CancelOrderCommand {
        order_id,
        user_id,
        symbol: Symbol::new(SYMBOL),
        timestamp: chrono::Utc::now(),
    })
}

// ---------------------------------------------------------------------------
// Benchmark: place a single limit order on an empty book
// ---------------------------------------------------------------------------

fn bench_place_limit_order(c: &mut Criterion) {
    let mut group = c.benchmark_group("matching_engine");
    group.throughput(Throughput::Elements(1));

    group.bench_function("place_limit_order", |b| {
        b.iter_batched(
            make_engine,
            |mut engine| {
                let events = engine.process(limit_cmd(Side::Buy, dec!(50000), dec!(1)));
                // Prevent the compiler from optimising the call away.
                std::hint::black_box(events);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Benchmark: place an order that immediately matches (1 fill)
// ---------------------------------------------------------------------------

fn bench_match_order(c: &mut Criterion) {
    let mut group = c.benchmark_group("matching_engine");
    group.throughput(Throughput::Elements(1));

    group.bench_function("match_order_1_fill", |b| {
        b.iter_batched(
            || {
                let mut engine = make_engine();
                // Pre-seed a resting sell so the next buy matches immediately.
                engine.process(limit_cmd(Side::Sell, dec!(50000), dec!(1)));
                engine
            },
            |mut engine| {
                let events = engine.process(limit_cmd(Side::Buy, dec!(50000), dec!(1)));
                std::hint::black_box(events);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Benchmark: place 1000 limit orders (build a deep book)
// ---------------------------------------------------------------------------

fn bench_place_1000_orders(c: &mut Criterion) {
    let mut group = c.benchmark_group("matching_engine");
    group.throughput(Throughput::Elements(1_000));

    group.bench_function("place_1000_limit_orders", |b| {
        b.iter_batched(
            make_engine,
            |mut engine| {
                // Spread 500 bids and 500 asks across distinct price levels.
                for i in 0..500u64 {
                    let bid_price = Decimal::new(49_000_i64 - i as i64, 0);
                    let ask_price = Decimal::new(51_000_i64 + i as i64, 0);
                    engine.process(limit_cmd(Side::Buy, bid_price, dec!(1)));
                    engine.process(limit_cmd(Side::Sell, ask_price, dec!(1)));
                }
                std::hint::black_box(&engine);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Benchmark: market order that sweeps 100 price levels
// ---------------------------------------------------------------------------

fn bench_match_against_deep_book(c: &mut Criterion) {
    let mut group = c.benchmark_group("matching_engine");
    group.throughput(Throughput::Elements(100));

    group.bench_function("market_order_sweep_100_levels", |b| {
        b.iter_batched(
            || {
                let mut engine = make_engine();
                // Build a sell-side ladder with 100 distinct price levels.
                for i in 0..100u64 {
                    let price = Decimal::new(50_000_i64 + i as i64, 0);
                    engine.process(limit_cmd(Side::Sell, price, dec!(1)));
                }
                engine
            },
            |mut engine| {
                // Market buy for 100 units — sweeps all 100 levels.
                let events = engine.process(market_cmd(Side::Buy, dec!(100)));
                std::hint::black_box(events);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Benchmark: cancel a resting order
// ---------------------------------------------------------------------------

fn bench_cancel_order(c: &mut Criterion) {
    let mut group = c.benchmark_group("matching_engine");
    group.throughput(Throughput::Elements(1));

    group.bench_function("cancel_order", |b| {
        b.iter_batched(
            || {
                let mut engine = make_engine();
                let user_id = uuid::Uuid::new_v4();
                let order_id = uuid::Uuid::new_v4();

                engine.process(Command::PlaceOrder(PlaceOrderCommand {
                    order_id,
                    user_id,
                    symbol: Symbol::new(SYMBOL),
                    side: Side::Buy,
                    order_type: OrderType::Limit,
                    time_in_force: TimeInForce::Gtc,
                    price: dec!(49000),
                    qty: dec!(1),
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
                }));

                (engine, order_id, user_id)
            },
            |(mut engine, order_id, user_id)| {
                let events = engine.process(cancel_cmd(order_id, user_id));
                std::hint::black_box(events);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Benchmark: full cycle — place, match, cancel the leftover
// ---------------------------------------------------------------------------

fn bench_full_cycle(c: &mut Criterion) {
    let mut group = c.benchmark_group("matching_engine");
    // Each iteration represents one complete order lifecycle.
    group.throughput(Throughput::Elements(1));

    group.bench_function("full_cycle_place_match_cancel", |b| {
        b.iter_batched(
            || {
                let mut engine = make_engine();
                // Place a resting sell for 2 units so the market buy only partially fills.
                engine.process(limit_cmd(Side::Sell, dec!(50000), dec!(2)));
                engine
            },
            |mut engine| {
                // 1. Place a limit buy that partially matches (fills 1, rests 1).
                let place_events = engine.process(limit_cmd(Side::Buy, dec!(50000), dec!(1)));

                // The buy fully fills the first unit from the resting sell.
                // Now place a second buy that will rest on the book.
                let resting_cmd = Command::PlaceOrder(PlaceOrderCommand {
                    order_id: uuid::Uuid::new_v4(),
                    user_id: uuid::Uuid::new_v4(),
                    symbol: Symbol::new(SYMBOL),
                    side: Side::Buy,
                    order_type: OrderType::Limit,
                    time_in_force: TimeInForce::Gtc,
                    price: dec!(49000), // below ask — will rest
                    qty: dec!(1),
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
                });

                let accepted_events = engine.process(resting_cmd.clone());

                // Extract the resting order ID from the accepted event.
                let order_id = if let Command::PlaceOrder(ref p) = resting_cmd {
                    p.order_id
                } else {
                    unreachable!()
                };
                let user_id = if let Command::PlaceOrder(ref p) = resting_cmd {
                    p.user_id
                } else {
                    unreachable!()
                };

                // 2. Cancel the resting order.
                let cancel_events = engine.process(cancel_cmd(order_id, user_id));

                std::hint::black_box((place_events, accepted_events, cancel_events));
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Benchmark: place a non-crossing limit order against a deep book (fast path)
//
// T4.1: The fast path skips match_order entirely when the incoming GTC limit
// would not cross.  This bench measures the optimised insert-only path.
// Expected: ≤ 800 ns (vs ~2 µs for the slow path on empty book).
// ---------------------------------------------------------------------------

fn bench_place_limit_nocross(c: &mut Criterion) {
    let mut group = c.benchmark_group("matching_engine");
    group.throughput(Throughput::Elements(1));

    group.bench_function("place_limit_nocross", |b| {
        b.iter_batched(
            || {
                let mut engine = make_engine();
                // Seed a far-from-market ask so the book is non-empty (cache is hot)
                // but the incoming bid will clearly not cross.
                engine.process(limit_cmd(Side::Sell, dec!(51000), dec!(10)));
                engine
            },
            |mut engine| {
                // Bid at 50000: well below ask at 51000 — guaranteed non-crossing.
                let events = engine.process(limit_cmd(Side::Buy, dec!(50000), dec!(1)));
                std::hint::black_box(events);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Parametric: throughput at varying book depths
// ---------------------------------------------------------------------------

fn bench_sweep_varying_depths(c: &mut Criterion) {
    let mut group = c.benchmark_group("matching_engine/sweep_depth");

    for depth in [1usize, 10, 50, 100, 500] {
        group.throughput(Throughput::Elements(depth as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(depth),
            &depth,
            |b, &depth| {
                b.iter_batched(
                    || {
                        let mut engine = make_engine();
                        for i in 0..depth {
                            let price = Decimal::new(50_000_i64 + i as i64, 0);
                            engine.process(limit_cmd(Side::Sell, price, dec!(1)));
                        }
                        engine
                    },
                    |mut engine| {
                        let qty = Decimal::new(depth as i64, 0);
                        let events = engine.process(market_cmd(Side::Buy, qty));
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
// Registration
// ---------------------------------------------------------------------------

criterion_group!(
    benches,
    bench_place_limit_order,
    bench_match_order,
    bench_place_1000_orders,
    bench_match_against_deep_book,
    bench_cancel_order,
    bench_full_cycle,
    bench_sweep_varying_depths,
    bench_place_limit_nocross,
);
criterion_main!(benches);
