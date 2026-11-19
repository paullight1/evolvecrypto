//! Call auction benchmarks — Sprint 1 (T3.5).
//!
//! Measures the cost of `run_auction()` at different book sizes.
//! The call auction is the price-discovery mechanism used during the pre-market
//! session. It collects orders into a buffer and then, at a single point in time,
//! finds the equilibrium price that maximises matched volume and executes all
//! eligible orders at that one price.
//!
//! Parametric order counts: [10, 50, 100, 500] with balanced bid/ask books
//! where all bids cross all asks (every order is eligible). This measures the
//! O(n²) candidate-price evaluation loop in `run_auction`.
//!
//! ## Methodology (Sprint 1 fix-up)
//!
//! The original Sprint 1 pass used `BatchSize::SmallInput` which included the
//! drop cost of the `Vec<PlaceOrderCommand>` input inside the measured window.
//! We now use `iter_custom` to time ONLY the `run_auction()` call, dropping the
//! order list outside the timed region.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use exchange_common::*;
use exchange_matching_engine::run_auction;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::time::Instant;

const SYMBOL: &str = "BTCUSDT";

/// Build a balanced set of `n` bids and `n` asks that fully cross.
///
/// Bids: prices in [50100, 50100+n*0.01], qty=1 each (all willing to pay more
///       than the cheapest ask — full overlap).
/// Asks: prices in [49900, 49900+n*0.01], qty=1 each (all below all bids).
///
/// At equilibrium the clearing price will sit somewhere in the overlap band and
/// all orders will match, producing `n` fills.
fn build_crossing_orders(n: usize) -> Vec<PlaceOrderCommand> {
    let mut orders = Vec::with_capacity(n * 2);

    for i in 0..n {
        // Bid: 50100.00 + i * 0.01 — all bids are above all asks
        let bid_price = Decimal::new(5_010_000_i64 + i as i64, 2);
        orders.push(PlaceOrderCommand {
            order_id: uuid::Uuid::from_u128(i as u128 + 1),
            user_id: uuid::Uuid::from_u128((i + 1) as u128),
            symbol: Symbol::new(SYMBOL),
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
            price: bid_price,
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
        });

        // Ask: 49900.00 + i * 0.01 — all asks are below all bids
        let ask_price = Decimal::new(4_990_000_i64 + i as i64, 2);
        orders.push(PlaceOrderCommand {
            order_id: uuid::Uuid::from_u128((n + i + 1) as u128),
            user_id: uuid::Uuid::from_u128((n + i + 1) as u128),
            symbol: Symbol::new(SYMBOL),
            side: Side::Sell,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
            price: ask_price,
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
        });
    }

    orders
}

/// Parametric benchmark: auction cycle cost at varying order book sizes.
///
/// Uses `iter_custom` to time ONLY `run_auction()` — order list construction
/// and drop are outside the measured window.
fn bench_auction_cycle(c: &mut Criterion) {
    let mut group = c.benchmark_group("matching_engine/call_auction");

    // Reference price close to the midpoint of the bid/ask spread.
    let reference_price = Some(dec!(50000));

    for &n in &[10usize, 50, 100, 500] {
        // Throughput: n fills produced per auction run.
        group.throughput(Throughput::Elements(n as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(n),
            &n,
            |b, &n| {
                // iter_custom: build order list before timer, drop after timer.
                b.iter_custom(|iters| {
                    let mut total = std::time::Duration::ZERO;
                    for i in 0..iters {
                        // Build orders outside the timed region.
                        let orders = build_crossing_orders(n);
                        let mut tid: TradeId = 1 + (i * n as u64);

                        // Time ONLY run_auction().
                        let start = Instant::now();
                        let result = run_auction(&orders, reference_price, &mut tid);
                        total += start.elapsed();

                        // Prevent optimisation — result drops here, outside timer.
                        std::hint::black_box(result);
                    }
                    total
                });
            },
        );
    }

    group.finish();
}

criterion_group!(call_auction_benches, bench_auction_cycle);
criterion_main!(call_auction_benches);
