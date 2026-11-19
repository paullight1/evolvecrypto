//! HTTP handler benchmarks.
//!
//! Measures round-trip latency for common API operations through the full
//! axum router stack (no network — tower ServiceExt::oneshot).
//!
//! Run with:
//!   cargo bench -p exchange-api-gateway
//!
//! S13 benchmarks added:
//!   bench_place_limit_order_full_rest   — G.2: single-user REST round-trip (iter_custom)
//!   bench_place_order_concurrent_10_users — G.3: 10 concurrent users × 100 orders throughput
//!
//! S17b benchmarks added:
//!   bench_contended_100_tasks_true_parallel   — B.1: 100 truly concurrent tasks hit Exchange simultaneously
//!   bench_place_order_with_portfolio_margin   — B.2: portfolio-margin path (large perp_configs HashMap)
//!   bench_admin_churn_with_concurrent_orders  — B.3: ArcSwap vs Mutex writer pressure

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use exchange_api_gateway::exchange::Exchange;
use exchange_api_gateway::server::{build_router, build_router_unlimited};
use exchange_common::*;
use exchange_order_gateway::{OrderLimitConfig, RateLimitConfig};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tower::ServiceExt;
use uuid::Uuid;

// ── Setup ─────────────────────────────────────────────────────────────────────

fn make_product(symbol: &str, product_type: ProductType) -> Product {
    Product {
        symbol: Symbol::new(symbol),
        base_asset: "BTC".to_string(),
        quote_asset: "USDT".to_string(),
        product_type,
        tick_size: dec!(0.01),
        lot_size: dec!(0.00001),
        min_notional: dec!(10),
        maker_fee: dec!(0.001),
        taker_fee: dec!(0.002),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
    }
}

fn make_app() -> axum::Router {
    let exchange = Exchange::new();
    exchange.add_product(make_product("BTCUSDT", ProductType::Perpetual));

    let user = Uuid::nil();
    exchange.create_account(user);
    exchange.deposit(user, "USDT", dec!(1_000_000)).unwrap();
    exchange.deposit(user, "BTC", dec!(100)).unwrap();

    build_router(exchange)
}

/// Build a shared app with `n_users` distinct accounts, each funded with
/// 10M USDT and 1000 BTC.  Returns `(router, [user_id; n_users])`.
///
/// The first user is always `Uuid::nil()` because the dev-mode auth bypass
/// injects `nil` as the `AuthenticatedUser` for all requests without real
/// auth headers.  The REST order handler uses this `nil` UUID.
fn make_shared_app(n_users: usize) -> (axum::Router, Vec<Uuid>) {
    let mut exchange = Exchange::new();
    // Use Perpetual to match the existing bench setup; futures margin is
    // simpler to set up (no base-asset balance needed for buys).
    exchange.add_product(make_product("BTCUSDT", ProductType::Perpetual));

    // Rate limit config: 0 = unlimited (benchmarks must not be capped by the rate limiter).
    let bench_rate_limit = RateLimitConfig {
        max_per_second: 0,
        max_per_minute: 0,
    };

    // First user is nil — dev-mode auth bypass injects this for all REST calls.
    // Deposit a large amount so many thousands of small orders never exhaust the balance.
    let nil_uid = Uuid::nil();
    exchange.create_account(nil_uid);
    exchange.deposit(nil_uid, "USDT", dec!(100_000_000_000)).unwrap();
    exchange.deposit(nil_uid, "BTC", dec!(1_000_000)).unwrap();
    exchange.rate_limiter.set_override(nil_uid, bench_rate_limit.clone());

    let mut users = vec![nil_uid];
    for _ in 1..n_users {
        let uid = Uuid::new_v4();
        exchange.create_account(uid);
        exchange.deposit(uid, "USDT", dec!(100_000_000_000)).unwrap();
        exchange.deposit(uid, "BTC", dec!(1_000_000)).unwrap();
        exchange.rate_limiter.set_override(uid, bench_rate_limit.clone());
        users.push(uid);
    }

    // Bypass the per-user open-order cap (default 500/symbol) so the benchmark
    // can accumulate thousands of resting limit orders without hitting HTTP 429.
    // This is benchmark-only — production always uses the default 500 cap.
    {
        let unlimited_order_cap = OrderLimitConfig {
            max_open_orders_per_user_per_symbol: usize::MAX,
            max_orders_per_user_per_price_level: usize::MAX,
            max_open_orders_per_user_total: usize::MAX,
        };
        // Arc::get_mut succeeds here because no other Arc clones have been
        // created yet (add_product/create_account/deposit all use Mutex fields).
        let exchange_inner =
            Arc::get_mut(&mut exchange).expect("arc should be unique before router construction");
        for uid in &users {
            exchange_inner.order_limit_overrides.insert(*uid, unlimited_order_cap.clone());
        }
    }

    // Always use unlimited-rate router in benchmarks so the HTTP token-bucket
    // middleware does not cap throughput during warmup/iterations. `required-features`
    // in Cargo.toml guarantees this compiles — cargo bench always enables `bench`.
    let router = build_router_unlimited(exchange);

    (router, users)
}

fn post(path: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap()
}

fn get(path: &str) -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(path)
        .body(Body::empty())
        .unwrap()
}

// ── Benchmarks ────────────────────────────────────────────────────────────────

/// Round-trip latency for placing a single limit order.
///
/// Each iteration creates a fresh exchange so there is no shared state
/// between runs and no accumulated order-book depth skewing results.
fn bench_place_limit_order(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("api");
    group.throughput(Throughput::Elements(1));

    group.bench_function("place_limit_order", |b| {
        b.iter_batched(
            make_app,
            |app| {
                rt.block_on(async {
                    let resp = app
                        .oneshot(post(
                            "/api/v1/orders",
                            serde_json::json!({
                                "symbol": "BTCUSDT",
                                "side": "buy",
                                "order_type": "limit",
                                "price": "50000",
                                "qty": "0.1"
                            }),
                        ))
                        .await
                        .unwrap();
                    assert_eq!(resp.status(), StatusCode::OK);
                    std::hint::black_box(resp)
                })
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

/// Round-trip latency for fetching a 25-level orderbook snapshot.
fn bench_get_orderbook(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    // Build a single app once and reuse across all iterations — the orderbook
    // read path is read-only so sharing state is safe and representative of
    // real production load (many reads against a stable book).
    let app = make_app();

    let mut group = c.benchmark_group("api");
    group.throughput(Throughput::Elements(1));

    group.bench_function("get_orderbook_depth25", |b| {
        b.iter(|| {
            rt.block_on(async {
                let resp = app
                    .clone()
                    .oneshot(get("/api/v1/markets/BTCUSDT/orderbook?depth=25"))
                    .await
                    .unwrap();
                assert_eq!(resp.status(), StatusCode::OK);
                std::hint::black_box(resp)
            })
        });
    });

    group.finish();
}

/// Round-trip latency for fetching account balances.
fn bench_get_balances(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let app = make_app();

    let mut group = c.benchmark_group("api");
    group.throughput(Throughput::Elements(1));

    group.bench_function("get_balances", |b| {
        b.iter(|| {
            rt.block_on(async {
                let resp = app
                    .clone()
                    .oneshot(get("/api/v1/account/balances"))
                    .await
                    .unwrap();
                assert_eq!(resp.status(), StatusCode::OK);
                std::hint::black_box(resp)
            })
        });
    });

    group.finish();
}

/// Round-trip latency for listing open orders (empty book).
fn bench_get_open_orders(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let app = make_app();

    let mut group = c.benchmark_group("api");
    group.throughput(Throughput::Elements(1));

    group.bench_function("get_open_orders_empty", |b| {
        b.iter(|| {
            rt.block_on(async {
                let resp = app
                    .clone()
                    .oneshot(get("/api/v1/orders"))
                    .await
                    .unwrap();
                assert_eq!(resp.status(), StatusCode::OK);
                std::hint::black_box(resp)
            })
        });
    });

    group.finish();
}

/// Throughput: place-then-immediately-cancel an order (two HTTP round-trips per iteration).
///
/// Tests the combined cost of order ingress + book insertion + cancel + balance unlock.
fn bench_place_then_cancel(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("api");
    // Two HTTP calls per logical operation.
    group.throughput(Throughput::Elements(2));

    group.bench_function("place_and_cancel_order", |b| {
        b.iter_batched(
            make_app,
            |app| {
                rt.block_on(async {
                    // Place.
                    let resp = app
                        .clone()
                        .oneshot(post(
                            "/api/v1/orders",
                            serde_json::json!({
                                "symbol": "BTCUSDT",
                                "side": "buy",
                                "order_type": "limit",
                                "price": "45000",
                                "qty": "0.1"
                            }),
                        ))
                        .await
                        .unwrap();
                    assert_eq!(resp.status(), StatusCode::OK);
                    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
                        .await
                        .unwrap();
                    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                    let order_id = body["order_id"].as_str().unwrap().to_string();

                    // Cancel.
                    let cancel_resp = app
                        .oneshot(
                            axum::http::Request::builder()
                                .method(Method::DELETE)
                                .uri(format!("/api/v1/orders/{order_id}"))
                                .body(Body::empty())
                                .unwrap(),
                        )
                        .await
                        .unwrap();
                    assert_eq!(cancel_resp.status(), StatusCode::OK);
                    std::hint::black_box(cancel_resp)
                })
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ── S13 G.2 — Full REST round-trip benchmark (iter_custom) ────────────────────

/// Full REST round-trip: POST /api/v1/orders through axum → spawn_blocking →
/// Exchange::place_order (all stages) → JSON response.
///
/// Uses `iter_custom` so criterion does NOT include the setup or drop cost
/// in the measured time — only the `router.oneshot().await` call.
///
/// A single shared Exchange instance is used across all iterations to match
/// realistic conditions (non-empty order book, reused account state).
fn bench_place_limit_order_full_rest(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (app, _users) = make_shared_app(1);

    let mut group = c.benchmark_group("api_s13");
    group.throughput(Throughput::Elements(1));

    // Use a monotonically increasing price offset to avoid "already on book"
    // rejections from the matching engine across iterations.
    let counter = std::sync::atomic::AtomicU32::new(0);

    group.bench_function("place_limit_order_full_rest", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                // Vary price so each order is a distinct price level on the book.
                let offset = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let price = 50_000u32 + (offset % 10_000); // stays in [50000, 60000)
                let body = serde_json::json!({
                    "symbol": "BTCUSDT",
                    "side": "buy",
                    "order_type": "limit",
                    "price": price.to_string(),
                    "qty": "0.001"
                });
                let req = Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/orders")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap();
                let app_clone = app.clone();
                let t0 = Instant::now();
                let resp = rt.block_on(async move { app_clone.oneshot(req).await.unwrap() });
                total += t0.elapsed();
                // Verify the request succeeded — if it didn't we're not measuring what we think.
                assert_eq!(
                    resp.status(),
                    StatusCode::OK,
                    "unexpected status: {}",
                    resp.status()
                );
                std::hint::black_box(resp);
            }
            total
        });
    });

    group.finish();
}

// ── S13 G.3 — Concurrency stress benchmark ────────────────────────────────────

/// Throughput benchmark: 10 concurrent users × 100 limit orders each = 1000 orders.
///
/// Measures total elapsed time for all 1000 orders to complete, then derives
/// orders/sec.  Uses `tokio::spawn` fan-out inside a single `Runtime::block_on`
/// call so all 10 tasks compete for the same set of Mutexes in Exchange.
///
/// This is the baseline that S14 (WAL group commit) must beat.
/// Expected pre-S14: ~200–600 orders/sec (WAL fsync serialises everything).
fn bench_place_order_concurrent_10_users(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();

    const N_USERS: usize = 10;
    const ORDERS_PER_USER: usize = 100;
    const TOTAL_ORDERS: usize = N_USERS * ORDERS_PER_USER;

    let (app, users) = make_shared_app(N_USERS);
    let app = Arc::new(app);
    let users = Arc::new(users);

    let mut group = c.benchmark_group("api_s13");
    group.throughput(Throughput::Elements(TOTAL_ORDERS as u64));
    // Fewer samples because each iteration is expensive (1000 HTTP round-trips).
    group.sample_size(10);

    // Shared price counter so concurrent orders land at distinct prices.
    let price_counter = Arc::new(std::sync::atomic::AtomicU32::new(0));

    group.bench_function("concurrent_10_users_100_orders", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let app = Arc::clone(&app);
                let users = Arc::clone(&users);
                let price_counter = Arc::clone(&price_counter);

                let elapsed = rt.block_on(async move {
                    let t0 = Instant::now();

                    let handles: Vec<_> = (0..N_USERS)
                        .map(|u| {
                            let app = Arc::clone(&app);
                            let user_id = users[u];
                            let price_counter = Arc::clone(&price_counter);
                            tokio::spawn(async move {
                                for _ in 0..ORDERS_PER_USER {
                                    let offset = price_counter
                                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                    // Spread prices across a wide range to avoid excessive
                                    // contention at a single price level.
                                    let price = 40_000u32 + (offset % 20_000);
                                    let body = serde_json::json!({
                                        "symbol": "BTCUSDT",
                                        "side": "buy",
                                        "order_type": "limit",
                                        "price": price.to_string(),
                                        "qty": "0.001",
                                        "user_id": user_id.to_string()
                                    });
                                    let req = Request::builder()
                                        .method(Method::POST)
                                        .uri("/api/v1/orders")
                                        .header("content-type", "application/json")
                                        .body(Body::from(serde_json::to_string(&body).unwrap()))
                                        .unwrap();
                                    // Dereference Arc<Router> to get Router, then clone it
                                    // (Router implements Clone and oneshot consumes self).
                                    let resp = (*app).clone().oneshot(req).await.unwrap();
                                    std::hint::black_box(resp);
                                }
                            })
                        })
                        .collect();

                    for h in handles {
                        h.await.unwrap();
                    }

                    t0.elapsed()
                });

                total += elapsed;
            }
            total
        });
    });

    group.finish();
}

// ── S17b helpers ──────────────────────────────────────────────────────────────

/// Like `make_shared_app` but also returns the `Arc<Exchange>` so S17b benches
/// can drive admin write operations (add_perp_config, enable_portfolio_margin)
/// directly without going through the HTTP API.
///
/// Ownership sequence:
/// 1. `Arc::get_mut` mutations happen while the count is 1 (order_limit_overrides).
/// 2. We `Arc::clone` to keep a reference for benchmarks.
/// 3. We call `build_router_unlimited(exchange)` — the router takes ownership of
///    the original Arc and internally clones it.  After this the strong count is ≥2.
fn make_shared_app_with_exchange(
    n_users: usize,
) -> (axum::Router, Vec<Uuid>, Arc<Exchange>) {
    let mut exchange = Exchange::new();
    exchange.add_product(make_product("BTCUSDT", ProductType::Perpetual));

    let bench_rate_limit = RateLimitConfig {
        max_per_second: 0,
        max_per_minute: 0,
    };

    let nil_uid = Uuid::nil();
    exchange.create_account(nil_uid);
    exchange.deposit(nil_uid, "USDT", dec!(100_000_000_000)).unwrap();
    exchange.deposit(nil_uid, "BTC", dec!(1_000_000)).unwrap();
    exchange.rate_limiter.set_override(nil_uid, bench_rate_limit.clone());

    let mut users = vec![nil_uid];
    for _ in 1..n_users {
        let uid = Uuid::new_v4();
        exchange.create_account(uid);
        exchange.deposit(uid, "USDT", dec!(100_000_000_000)).unwrap();
        exchange.deposit(uid, "BTC", dec!(1_000_000)).unwrap();
        exchange.rate_limiter.set_override(uid, bench_rate_limit.clone());
        users.push(uid);
    }

    {
        let unlimited_order_cap = OrderLimitConfig {
            max_open_orders_per_user_per_symbol: usize::MAX,
            max_orders_per_user_per_price_level: usize::MAX,
            max_open_orders_per_user_total: usize::MAX,
        };
        let exchange_inner =
            Arc::get_mut(&mut exchange).expect("arc should be unique before router construction");
        for uid in &users {
            exchange_inner.order_limit_overrides.insert(*uid, unlimited_order_cap.clone());
        }
    }

    // Keep a clone BEFORE moving exchange into the router.
    let exchange_ref = Arc::clone(&exchange);
    let router = build_router_unlimited(exchange);
    (router, users, exchange_ref)
}

/// Build a minimal PerpetualConfig for a synthetic symbol (COIN{n}USDT).
/// Uses BTC-equivalent parameters: 100x max leverage, 0.5% maintenance margin.
fn make_perp_config(symbol: &str) -> PerpetualConfig {
    PerpetualConfig {
        symbol: Symbol::new(symbol),
        max_leverage: dec!(100),
        initial_margin_rate: dec!(0.01),
        maintenance_margin_rate: dec!(0.005),
        funding_interval_hours: 8,
        max_funding_rate: dec!(0.01),
        insurance_fund: dec!(0),
        tiers: vec![LeverageTier {
            min_notional: dec!(0),
            max_notional: Decimal::MAX,
            max_leverage: dec!(100),
            maintenance_margin_rate: dec!(0.005),
        }],
        contract_type: ContractType::Linear,
        settle_coin: "USDT".to_string(),
        contract_size: dec!(1),
    }
}

// ── S17b B.1 — 100 truly parallel tasks ──────────────────────────────────────

/// Contention benchmark: 100 `tokio::spawn` tasks all fire ONE order each, then
/// we await ALL of them simultaneously. This creates genuine multi-core contention
/// on shared fields (`accounts`, `rate_limiter`, `engine_pool`, etc.) that the
/// S13 "10 tasks × 100 sequential orders" bench cannot exercise.
///
/// Each criterion sample submits 100 orders in parallel; 30 samples = 3000 orders.
/// The measurement is the wall-clock time for all 100 to complete per sample.
///
/// Under parking_lot's contention model (spin-then-park), this is where the
/// S16 win is expected to show: the spin phase catches short critical sections
/// without an OS context switch.
fn bench_contended_100_tasks_true_parallel(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(8)
        .enable_all()
        .build()
        .unwrap();

    let (app, _users) = make_shared_app(1);
    let app = Arc::new(app);

    let mut group = c.benchmark_group("api_s17b");
    // Each sample is 100 parallel orders; 30 samples = 3000 orders total.
    // Fewer samples than the default 100 because each is more expensive.
    group.sample_size(30);
    group.warm_up_time(Duration::from_secs(5));
    group.measurement_time(Duration::from_secs(15));
    group.noise_threshold(0.02);
    // Report throughput as individual orders (100 per sample).
    group.throughput(Throughput::Elements(100));

    let price_counter = Arc::new(AtomicU32::new(0));

    group.bench_function("contended_100_tasks_true_parallel", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let app = Arc::clone(&app);
                let price_counter = Arc::clone(&price_counter);

                let elapsed = rt.block_on(async move {
                    // Spawn all 100 tasks BEFORE awaiting any — true parallel start.
                    let mut handles = Vec::with_capacity(100);
                    for _ in 0..100u32 {
                        let app = Arc::clone(&app);
                        let offset = price_counter.fetch_add(1, Ordering::Relaxed);
                        let price = 50_000u32 + (offset % 10_000);
                        handles.push(tokio::spawn(async move {
                            let body = serde_json::json!({
                                "symbol": "BTCUSDT",
                                "side": "buy",
                                "order_type": "limit",
                                "price": price.to_string(),
                                "qty": "0.001"
                            });
                            let req = Request::builder()
                                .method(Method::POST)
                                .uri("/api/v1/orders")
                                .header("content-type", "application/json")
                                .body(Body::from(serde_json::to_string(&body).unwrap()))
                                .unwrap();
                            (*app).clone().oneshot(req).await.unwrap()
                        }));
                    }

                    let t0 = Instant::now();
                    for h in handles {
                        let _ = h.await;
                    }
                    t0.elapsed()
                });

                total += elapsed;
            }
            total
        });
    });

    group.finish();
}

// ── S17b B.2 — Portfolio margin + large perp_configs HashMap ─────────────────

/// Portfolio-margin bench: exercises the `perp_configs.load_full()` path
/// (LOCK-19f-a) that S17 eliminated the Mutex clone from.
///
/// Setup:
/// - Register 20 additional synthetic perpetual products (total ≥21 entries in
///   the perp_configs HashMap, making the pre-S17 clone meaningfully expensive).
/// - Deposit ≥100,000 USDT and enable portfolio margin for the nil user
///   (requires equity ≥ $100,000 per `enable_portfolio_margin` gate).
/// - Place limit orders on BTCUSDT; the portfolio-margin branch calls
///   `required_portfolio_margin(...)` which loads `perp_configs.load_full()`.
///
/// The pre-S17 version cloned the entire HashMap under lock on every such order.
/// Post-S17, `load_full()` is one atomic pointer load + Arc refcount increment.
fn bench_place_order_with_portfolio_margin(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let (app, _users, exchange) = make_shared_app_with_exchange(1);

    // Register 20 synthetic perp products to make the HashMap non-trivial.
    // At 20+ entries the pre-S17 HashMap clone is measurably more expensive than
    // the 1-entry case (each entry is ~400 bytes: Symbol + Decimal fields + Vec<LeverageTier>).
    for i in 0..20u32 {
        let sym = format!("COIN{i:02}USDT");
        exchange.add_product(make_product(&sym, ProductType::Perpetual));
        exchange.add_perp_config(make_perp_config(&sym));
    }

    // Enable portfolio margin — requires ≥$100,000 USDT equity.
    // nil_uid already has 100B USDT from make_shared_app_with_exchange.
    exchange
        .enable_portfolio_margin(Uuid::nil())
        .expect("portfolio margin should enable with 100B USDT equity");

    let mut group = c.benchmark_group("api_s17b");
    group.sample_size(100);
    group.warm_up_time(Duration::from_secs(5));
    group.measurement_time(Duration::from_secs(20));
    group.noise_threshold(0.02);
    group.throughput(Throughput::Elements(1));

    let counter = AtomicU32::new(0);

    group.bench_function("place_order_with_portfolio_margin", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let offset = counter.fetch_add(1, Ordering::Relaxed);
                let price = 50_000u32 + (offset % 10_000);
                let body = serde_json::json!({
                    "symbol": "BTCUSDT",
                    "side": "buy",
                    "order_type": "limit",
                    "price": price.to_string(),
                    "qty": "0.001"
                });
                let req = Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/orders")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap();
                let app_clone = app.clone();
                let t0 = Instant::now();
                let resp = rt.block_on(async move { app_clone.oneshot(req).await.unwrap() });
                total += t0.elapsed();
                assert_eq!(
                    resp.status(),
                    StatusCode::OK,
                    "portfolio margin order failed: {}",
                    resp.status()
                );
                std::hint::black_box(resp);
            }
            total
        });
    });

    group.finish();
}

// ── S17b B.3 — Admin config churn while orders execute ───────────────────────

/// ArcSwap writer-pressure bench: a background task calls `add_perp_config`
/// every 10 ms (simulating admin config updates) while 50 concurrent order
/// tasks drive the read path.
///
/// With the pre-S17 `Mutex<HashMap>`, every writer acquisition blocks ALL 50
/// concurrent readers for the duration of the clone-and-insert.  With ArcSwap,
/// readers load the old pointer atomically — zero blocking.
///
/// The measurement captures the wall-clock time for 50 parallel orders to
/// complete while the writer is churning.  Under ArcSwap, this should be
/// stable (writer doesn't block readers).  Under a naive Mutex, order latency
/// would spike every 10 ms when the writer holds the lock.
fn bench_admin_churn_with_concurrent_orders(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(8)
        .enable_all()
        .build()
        .unwrap();

    let (app, _users, exchange) = make_shared_app_with_exchange(1);
    let app = Arc::new(app);

    // Pre-register 10 synthetic symbols so the HashMap is non-trivial before churn starts.
    for i in 0..10u32 {
        let sym = format!("CHURN{i:02}USDT");
        exchange.add_product(make_product(&sym, ProductType::Perpetual));
        exchange.add_perp_config(make_perp_config(&sym));
    }

    let mut group = c.benchmark_group("api_s17b");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(5));
    group.measurement_time(Duration::from_secs(20));
    group.noise_threshold(0.02);
    group.throughput(Throughput::Elements(50));

    let price_counter = Arc::new(AtomicU32::new(0));
    // Counter for the writer — cycles through 10 config variants to produce
    // realistic (non-trivial) ArcSwap store operations.
    let writer_counter = Arc::new(AtomicU32::new(0));

    group.bench_function("admin_churn_during_orders", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let exchange_writer = Arc::clone(&exchange);
                let stop = Arc::new(AtomicBool::new(false));
                let stop_clone = Arc::clone(&stop);
                let writer_counter = Arc::clone(&writer_counter);

                // Spawn writer task: calls add_perp_config every 10 ms.
                // add_perp_config = load_full() + HashMap::clone() + insert + ArcSwap::store().
                // This is exactly the writer path S17 optimised (readers never block).
                let writer_handle = rt.spawn(async move {
                    while !stop_clone.load(Ordering::Relaxed) {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        let n = writer_counter.fetch_add(1, Ordering::Relaxed) % 10;
                        let sym = format!("CHURN{n:02}USDT");
                        // Re-insert the same symbol with a slightly modified insurance_fund
                        // value — forces a real clone-and-swap on every call.
                        let mut cfg = make_perp_config(&sym);
                        cfg.insurance_fund = Decimal::new(n as i64 * 1000, 0);
                        exchange_writer.add_perp_config(cfg);
                    }
                });

                // Measure: 50 concurrent orders placed while writer is running.
                let app = Arc::clone(&app);
                let price_counter = Arc::clone(&price_counter);
                let elapsed = rt.block_on(async move {
                    let mut handles = Vec::with_capacity(50);
                    for _ in 0..50u32 {
                        let app = Arc::clone(&app);
                        let offset = price_counter.fetch_add(1, Ordering::Relaxed);
                        let price = 50_000u32 + (offset % 10_000);
                        handles.push(tokio::spawn(async move {
                            let body = serde_json::json!({
                                "symbol": "BTCUSDT",
                                "side": "buy",
                                "order_type": "limit",
                                "price": price.to_string(),
                                "qty": "0.001"
                            });
                            let req = Request::builder()
                                .method(Method::POST)
                                .uri("/api/v1/orders")
                                .header("content-type", "application/json")
                                .body(Body::from(serde_json::to_string(&body).unwrap()))
                                .unwrap();
                            (*app).clone().oneshot(req).await.unwrap()
                        }));
                    }
                    let t0 = Instant::now();
                    for h in handles {
                        let _ = h.await;
                    }
                    t0.elapsed()
                });

                stop.store(true, Ordering::Relaxed);
                let _ = rt.block_on(writer_handle);

                total += elapsed;
            }
            total
        });
    });

    group.finish();
}

// ── Registration ──────────────────────────────────────────────────────────────

criterion_group!(
    benches,
    bench_place_limit_order,
    bench_get_orderbook,
    bench_get_balances,
    bench_get_open_orders,
    bench_place_then_cancel,
    bench_place_limit_order_full_rest,
    bench_place_order_concurrent_10_users,
    bench_contended_100_tasks_true_parallel,
    bench_place_order_with_portfolio_margin,
    bench_admin_churn_with_concurrent_orders,
);
criterion_main!(benches);
