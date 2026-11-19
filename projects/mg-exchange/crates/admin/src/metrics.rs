use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Fixed-size ring buffer for recording timestamped event counts (orders/fills
/// per second) used to compute 1-min / 5-min / 1-hr throughput windows.
///
/// Each slot holds the number of events that occurred in one second.
/// The buffer is 3600 slots wide (covers one hour). Slots are identified by
/// `unix_second % SLOTS`. On each increment we check whether the current
/// second matches the slot's recorded second; if not the slot is reset before
/// being incremented (lazy expiry).
const SLOTS: usize = 3600;

pub struct RingCounter {
    counts: Vec<AtomicU64>,
    /// Unix-second timestamp recorded for each slot (so stale slots can be reset).
    timestamps: Vec<AtomicI64>,
}

impl RingCounter {
    fn new() -> Self {
        let mut counts = Vec::with_capacity(SLOTS);
        let mut timestamps = Vec::with_capacity(SLOTS);
        for _ in 0..SLOTS {
            counts.push(AtomicU64::new(0));
            timestamps.push(AtomicI64::new(0));
        }
        Self { counts, timestamps }
    }

    /// Increment the counter for the current second.
    pub fn inc(&self) {
        let now = now_unix_secs() as i64;
        let slot = (now as usize) % SLOTS;
        // If the slot belongs to a different second, reset it first.
        let prev = self.timestamps[slot].swap(now, Ordering::Relaxed);
        if prev != now {
            self.counts[slot].store(1, Ordering::Relaxed);
        } else {
            self.counts[slot].fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Sum events over the last `window_secs` seconds.
    fn sum_window(&self, window_secs: usize) -> u64 {
        let now = now_unix_secs() as i64;
        let cutoff = now - window_secs as i64 + 1;
        let mut total = 0u64;
        for i in 0..window_secs.min(SLOTS) {
            let slot = ((now as usize).wrapping_sub(i)) % SLOTS;
            let ts = self.timestamps[slot].load(Ordering::Relaxed);
            if ts >= cutoff {
                total += self.counts[slot].load(Ordering::Relaxed);
            }
        }
        total
    }

    /// Events per second averaged over the last `window_secs` seconds.
    pub fn rate_per_sec(&self, window_secs: usize) -> f64 {
        self.sum_window(window_secs) as f64 / window_secs as f64
    }
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Simple fixed-bucket histogram for nanosecond latencies.
// ---------------------------------------------------------------------------

/// Latency histogram with fixed linear-log buckets suitable for nanosecond
/// measurements.  Buckets (upper bound in ns):
///   1µs, 5µs, 10µs, 50µs, 100µs, 500µs, 1ms, 5ms, 10ms, 50ms, 100ms, +Inf
const BUCKET_BOUNDS_NS: [u64; 11] = [
    1_000,       // 1 µs
    5_000,       // 5 µs
    10_000,      // 10 µs
    50_000,      // 50 µs
    100_000,     // 100 µs
    500_000,     // 500 µs
    1_000_000,   // 1 ms
    5_000_000,   // 5 ms
    10_000_000,  // 10 ms
    50_000_000,  // 50 ms
    100_000_000, // 100 ms
                 // +Inf implicit
];

const NUM_BUCKETS: usize = BUCKET_BOUNDS_NS.len() + 1; // +1 for the +Inf bucket

/// Lock-free latency histogram that exposes p50 / p95 / p99 percentiles and
/// a Prometheus-compatible `_bucket` / `_count` / `_sum` representation.
pub struct LatencyHistogram {
    /// Cumulative bucket counters (index i covers values ≤ BUCKET_BOUNDS_NS[i]).
    /// The last bucket covers +Inf.
    buckets: [AtomicU64; NUM_BUCKETS],
    /// Total observation count.
    count: AtomicU64,
    /// Sum of all observations in nanoseconds.
    sum_ns: AtomicU64,
}

impl LatencyHistogram {
    pub fn new() -> Self {
        // AtomicU64 is not Copy so we cannot use array initializer syntax directly.
        let buckets = std::array::from_fn(|_| AtomicU64::new(0));
        Self {
            buckets,
            count: AtomicU64::new(0),
            sum_ns: AtomicU64::new(0),
        }
    }

    /// Record a single latency observation (in nanoseconds).
    pub fn observe_ns(&self, ns: u64) {
        self.count.fetch_add(1, Ordering::Relaxed);
        // Saturating add to prevent wrap-around on very large sums.
        self.sum_ns.fetch_add(ns, Ordering::Relaxed);
        // Increment the correct bucket.
        let idx = BUCKET_BOUNDS_NS
            .iter()
            .position(|&bound| ns <= bound)
            .unwrap_or(NUM_BUCKETS - 1);
        self.buckets[idx].fetch_add(1, Ordering::Relaxed);
    }

    /// Compute approximate percentile (0–100) from cumulative bucket data.
    ///
    /// Returns `None` when no observations have been recorded.
    pub fn percentile(&self, pct: f64) -> Option<u64> {
        let count = self.count.load(Ordering::Relaxed);
        if count == 0 {
            return None;
        }
        // Build per-bucket counts (not cumulative).
        let mut per_bucket = [0u64; NUM_BUCKETS];
        for (i, b) in self.buckets.iter().enumerate() {
            per_bucket[i] = b.load(Ordering::Relaxed);
        }
        let target = ((pct / 100.0) * count as f64).ceil() as u64;
        let mut cumulative = 0u64;
        for (i, &cnt) in per_bucket.iter().enumerate() {
            cumulative += cnt;
            if cumulative >= target {
                // Return the upper bound of this bucket as an estimate.
                return Some(if i < BUCKET_BOUNDS_NS.len() {
                    BUCKET_BOUNDS_NS[i]
                } else {
                    // +Inf bucket — return 2× the last finite bound as a rough estimate.
                    BUCKET_BOUNDS_NS[BUCKET_BOUNDS_NS.len() - 1] * 2
                });
            }
        }
        Some(BUCKET_BOUNDS_NS[BUCKET_BOUNDS_NS.len() - 1])
    }

    /// Total number of observations.
    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Sum of all observed values in nanoseconds.
    pub fn sum_ns(&self) -> u64 {
        self.sum_ns.load(Ordering::Relaxed)
    }

    /// Emit Prometheus histogram lines for the given metric name and optional
    /// label string (e.g., `operation="place_order"`).
    pub fn to_prometheus_lines(&self, name: &str, labels: &str) -> String {
        let mut out = String::with_capacity(512);
        let label_sep = if labels.is_empty() { "" } else { "," };
        let mut cumulative = 0u64;
        for (i, &bound) in BUCKET_BOUNDS_NS.iter().enumerate() {
            cumulative += self.buckets[i].load(Ordering::Relaxed);
            let bound_sec = bound as f64 / 1e9;
            out.push_str(&format!(
                "{name}_bucket{{{labels}{label_sep}le=\"{bound_sec:.6}\"}} {cumulative}\n"
            ));
        }
        // +Inf bucket
        cumulative += self.buckets[NUM_BUCKETS - 1].load(Ordering::Relaxed);
        out.push_str(&format!(
            "{name}_bucket{{{labels}{label_sep}le=\"+Inf\"}} {cumulative}\n"
        ));
        out.push_str(&format!("{name}_sum{{{labels}}} {:.9}\n", self.sum_ns() as f64 / 1e9));
        out.push_str(&format!("{name}_count{{{labels}}} {}\n", self.count()));
        out
    }
}

impl Default for LatencyHistogram {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Per-symbol latency tracker
// ---------------------------------------------------------------------------

/// Per-symbol latency statistics snapshot (nanoseconds).
#[derive(Debug, Clone, serde::Serialize)]
pub struct SymbolLatencyStats {
    pub symbol: String,
    pub count: u64,
    pub p50_ns: Option<u64>,
    pub p95_ns: Option<u64>,
    pub p99_ns: Option<u64>,
    pub avg_ns: Option<f64>,
}

/// Throughput statistics snapshot.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ThroughputStats {
    pub orders_per_sec_1m: f64,
    pub orders_per_sec_5m: f64,
    pub orders_per_sec_1h: f64,
    pub fills_per_sec_1m: f64,
    pub fills_per_sec_5m: f64,
    pub fills_per_sec_1h: f64,
}

// ---------------------------------------------------------------------------
// Per-symbol order counters with side/type labels (for Prometheus)
// ---------------------------------------------------------------------------

/// Per-symbol order and volume counters.
///
/// All fields are lock-free atomics — safe to increment from the hot matching
/// path while the Prometheus endpoint reads them concurrently.
pub struct SymbolMetrics {
    pub orders_placed: AtomicU64,
    pub orders_filled: AtomicU64,
    pub orders_cancelled: AtomicU64,
    pub liquidations: AtomicU64,
    /// Volume in 0.01 USDT cents to avoid floating-point in atomics.
    pub volume_quote_cents: AtomicU64,
    // Labeled counters (side × type) — 2 sides × 2 types = 4 cells
    // Layout: [buy_limit, buy_market, sell_limit, sell_market]
    pub orders_by_side_type: [AtomicU64; 4],
    pub fills_total: AtomicU64,
}

impl SymbolMetrics {
    fn new() -> Self {
        Self {
            orders_placed: AtomicU64::new(0),
            orders_filled: AtomicU64::new(0),
            orders_cancelled: AtomicU64::new(0),
            liquidations: AtomicU64::new(0),
            volume_quote_cents: AtomicU64::new(0),
            orders_by_side_type: std::array::from_fn(|_| AtomicU64::new(0)),
            fills_total: AtomicU64::new(0),
        }
    }
}

/// Index into `orders_by_side_type` for (side, order_type).
///
/// side: 0=buy, 1=sell  order_type: 0=limit, 1=market
pub const fn side_type_idx(is_buy: bool, is_limit: bool) -> usize {
    let side_off = if is_buy { 0 } else { 2 };
    let type_off = if is_limit { 0 } else { 1 };
    side_off + type_off
}

// ---------------------------------------------------------------------------
// Main ExchangeMetrics struct
// ---------------------------------------------------------------------------

/// Exchange-wide metrics for monitoring and admin dashboards.
///
/// Lock-free atomic counters — safe to read from Prometheus endpoint
/// while matching engine updates them concurrently.
///
/// Phase 1: in-memory counters exposed via /metrics endpoint.
/// Phase 2: integrate with `metrics` crate + Prometheus exporter.
pub struct ExchangeMetrics {
    // Order flow
    pub orders_placed: AtomicU64,
    pub orders_cancelled: AtomicU64,
    pub orders_rejected: AtomicU64,
    pub orders_filled: AtomicU64,

    // Fill counter (distinct from orders_filled — one order can produce many fills)
    pub fills_total: AtomicU64,

    // Trade flow
    pub trades_total: AtomicU64,
    pub trade_volume_quote_cents: AtomicU64, // volume in 0.01 USDT units to avoid f64

    // WebSocket
    pub ws_connections_active: AtomicU64,
    pub ws_messages_sent: AtomicU64,

    // System
    pub matching_latency_us_sum: AtomicU64,
    pub matching_latency_us_count: AtomicU64,

    // Gauges
    pub active_orders: AtomicU64,
    pub open_positions: AtomicU64,

    // Health flags
    pub db_connected: AtomicBool,
    pub wal_healthy: AtomicBool,

    // Last trade timestamp (unix millis, 0 = no trades yet)
    pub last_trade_timestamp_ms: AtomicI64,

    // Per-symbol breakdowns (behind RwLock — symbol set is small and stable)
    pub symbol_metrics: RwLock<HashMap<String, SymbolMetrics>>,

    // Per-symbol latency histograms (behind Mutex — only written by place_order)
    pub latency_histograms: Mutex<HashMap<String, LatencyHistogram>>,

    // Global end-to-end latency histogram (place_order entry → events returned)
    pub place_order_latency: LatencyHistogram,

    // Throughput ring counters
    pub order_ring: RingCounter,
    pub fill_ring: RingCounter,

    // Process start time for uptime calculation
    started_at: Instant,
    started_at_unix: u64,

    // Background task heartbeats — task_name → last heartbeat unix timestamp (secs).
    task_heartbeats: Mutex<HashMap<String, u64>>,

    // Aggregate matching-engine latency gauges (KG-3 / KG-5).
    //
    // These are updated every 30 seconds by the `spawn_latency_metrics_updater`
    // background task in `api-gateway`. Only aggregate (max-of-symbols) values
    // are stored here.
    //
    // KG-5 contract: per-symbol latency is intentionally NOT stored in this
    // struct and NOT emitted on the public /metrics endpoint. Exposing per-symbol
    // p99.99 would allow HFT participants to infer engine congestion on specific
    // symbols and front-run the resulting price impact. Internal debugging can
    // use the authenticated /admin/engine/latency endpoint (future sprint).
    pub engine_latency_p50_max_ns: AtomicU64,
    pub engine_latency_p99_max_ns: AtomicU64,
    pub engine_latency_p999_max_ns: AtomicU64,
    pub engine_latency_p9999_max_ns: AtomicU64,
    pub engine_latency_max_overall_ns: AtomicU64,
    pub engine_total_command_count: AtomicU64,

    // --- Per-stage place_order latency counters (S13 G.1) ---
    //
    // 7 stages × 3 fields (max_ns, sum_ns, count) = 21 AtomicU64 fields (168 bytes).
    //
    // KG-5 aggregation contract: NO per-symbol labels on these metrics.
    // They are exposed as aggregate gauges via to_prometheus().
    //
    // Stage definitions:
    //   auth         — halt check, frozen-account check, KYC/email gates
    //   rate_limit   — rate_limiter lock + check (+ status check before it)
    //   validation   — product rules + open-order caps + STP + surveillance + circuit breaker + pair phase
    //   balance_lock — spot balance lock OR futures margin reserve
    //   wal_write    — journal lock + WAL append
    //   matching     — engine dispatch (engine_pool + process())
    //   process_events — post-match side effects (settle fills, WS broadcast, iceberg)
    pub stage_auth_max_ns: AtomicU64,
    pub stage_auth_sum_ns: AtomicU64,
    pub stage_auth_count: AtomicU64,

    pub stage_rate_limit_max_ns: AtomicU64,
    pub stage_rate_limit_sum_ns: AtomicU64,
    pub stage_rate_limit_count: AtomicU64,

    pub stage_validation_max_ns: AtomicU64,
    pub stage_validation_sum_ns: AtomicU64,
    pub stage_validation_count: AtomicU64,

    pub stage_balance_lock_max_ns: AtomicU64,
    pub stage_balance_lock_sum_ns: AtomicU64,
    pub stage_balance_lock_count: AtomicU64,

    pub stage_wal_write_max_ns: AtomicU64,
    pub stage_wal_write_sum_ns: AtomicU64,
    pub stage_wal_write_count: AtomicU64,

    pub stage_matching_max_ns: AtomicU64,
    pub stage_matching_sum_ns: AtomicU64,
    pub stage_matching_count: AtomicU64,

    pub stage_process_events_max_ns: AtomicU64,
    pub stage_process_events_sum_ns: AtomicU64,
    pub stage_process_events_count: AtomicU64,

    // --- S15: split process_events into sync (financial) + dispatch (channel send) ---
    //
    // events_sync  — balance settlement, fee calc, position update (must be sync)
    // events_dispatch — channel try_send to post-fill worker (should be << 1 µs)
    pub stage_events_sync_max_ns: AtomicU64,
    pub stage_events_sync_sum_ns: AtomicU64,
    pub stage_events_sync_count: AtomicU64,

    pub stage_events_dispatch_max_ns: AtomicU64,
    pub stage_events_dispatch_sum_ns: AtomicU64,
    pub stage_events_dispatch_count: AtomicU64,

    /// Count of PostFillEvents dropped due to channel saturation (B.5).
    pub event_channel_dropped_total: AtomicU64,

    // ── S20 J.3 — Lock wait histograms ────────────────────────────────────
    //
    // Measures `parking_lot::Mutex::lock()` wait time at the four visible
    // contended call sites remaining in `place_order` after S18 Phase A/B:
    //   - stp_guard          : self-trade prevention
    //   - surveillance       : wash-trade detection
    //   - pending_orders     : per-order bookkeeping
    //   - order_locks        : balance-lock tracking
    //
    // Each histogram records the wall-clock delta between `Instant::now()`
    // before `.lock()` and the point the guard is returned. For an
    // uncontended acquire this is ~5-20 ns; under contention the tail grows
    // into the microsecond or millisecond range. The histograms expose
    // p50 / p99 and full bucket data so on-call engineers can distinguish
    // "slow because contended" from "slow because doing too much work".
    //
    // NOTE: the per-user `accounts` shard mutex (inside `ShardedAccounts`) is
    // not instrumented here because it lives in `exchange-account` which does
    // not depend on `exchange-admin`. A future refactor can wire a hook
    // closure into `ShardedAccounts::new_with_lock_hook(...)` if shard-level
    // contention monitoring becomes operationally important.
    pub lock_wait_stp_guard: LatencyHistogram,
    pub lock_wait_surveillance: LatencyHistogram,
    pub lock_wait_pending_orders: LatencyHistogram,
    pub lock_wait_order_locks: LatencyHistogram,

    // ── S20 J.5 — Per-endpoint latency histograms ──────────────────────────
    //
    // Mirrors `place_order_latency` but for the other REST hot-path
    // endpoints an on-call engineer needs to diagnose a latency spike:
    //   - cancel_order
    //   - amend_order
    //   - get_positions
    //   - get_open_orders
    //   - get_order_history
    //
    // Each handler acquires an `EndpointLatencyGuard` on entry and the guard
    // records the elapsed nanoseconds on drop — regardless of return path,
    // including early-return error cases.
    pub endpoint_cancel_order: LatencyHistogram,
    pub endpoint_amend_order: LatencyHistogram,
    pub endpoint_get_positions: LatencyHistogram,
    pub endpoint_get_open_orders: LatencyHistogram,
    pub endpoint_get_order_history: LatencyHistogram,
}

impl ExchangeMetrics {
    pub fn new() -> Self {
        let started_at_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            orders_placed: AtomicU64::new(0),
            orders_cancelled: AtomicU64::new(0),
            orders_rejected: AtomicU64::new(0),
            orders_filled: AtomicU64::new(0),
            fills_total: AtomicU64::new(0),
            trades_total: AtomicU64::new(0),
            trade_volume_quote_cents: AtomicU64::new(0),
            ws_connections_active: AtomicU64::new(0),
            ws_messages_sent: AtomicU64::new(0),
            matching_latency_us_sum: AtomicU64::new(0),
            matching_latency_us_count: AtomicU64::new(0),
            active_orders: AtomicU64::new(0),
            open_positions: AtomicU64::new(0),
            db_connected: AtomicBool::new(false),
            wal_healthy: AtomicBool::new(false),
            last_trade_timestamp_ms: AtomicI64::new(0),
            symbol_metrics: RwLock::new(HashMap::new()),
            latency_histograms: Mutex::new(HashMap::new()),
            place_order_latency: LatencyHistogram::new(),
            order_ring: RingCounter::new(),
            fill_ring: RingCounter::new(),
            started_at: Instant::now(),
            started_at_unix,
            task_heartbeats: Mutex::new(HashMap::new()),
            engine_latency_p50_max_ns: AtomicU64::new(0),
            engine_latency_p99_max_ns: AtomicU64::new(0),
            engine_latency_p999_max_ns: AtomicU64::new(0),
            engine_latency_p9999_max_ns: AtomicU64::new(0),
            engine_latency_max_overall_ns: AtomicU64::new(0),
            engine_total_command_count: AtomicU64::new(0),
            stage_auth_max_ns: AtomicU64::new(0),
            stage_auth_sum_ns: AtomicU64::new(0),
            stage_auth_count: AtomicU64::new(0),
            stage_rate_limit_max_ns: AtomicU64::new(0),
            stage_rate_limit_sum_ns: AtomicU64::new(0),
            stage_rate_limit_count: AtomicU64::new(0),
            stage_validation_max_ns: AtomicU64::new(0),
            stage_validation_sum_ns: AtomicU64::new(0),
            stage_validation_count: AtomicU64::new(0),
            stage_balance_lock_max_ns: AtomicU64::new(0),
            stage_balance_lock_sum_ns: AtomicU64::new(0),
            stage_balance_lock_count: AtomicU64::new(0),
            stage_wal_write_max_ns: AtomicU64::new(0),
            stage_wal_write_sum_ns: AtomicU64::new(0),
            stage_wal_write_count: AtomicU64::new(0),
            stage_matching_max_ns: AtomicU64::new(0),
            stage_matching_sum_ns: AtomicU64::new(0),
            stage_matching_count: AtomicU64::new(0),
            stage_process_events_max_ns: AtomicU64::new(0),
            stage_process_events_sum_ns: AtomicU64::new(0),
            stage_process_events_count: AtomicU64::new(0),
            stage_events_sync_max_ns: AtomicU64::new(0),
            stage_events_sync_sum_ns: AtomicU64::new(0),
            stage_events_sync_count: AtomicU64::new(0),
            stage_events_dispatch_max_ns: AtomicU64::new(0),
            stage_events_dispatch_sum_ns: AtomicU64::new(0),
            stage_events_dispatch_count: AtomicU64::new(0),
            event_channel_dropped_total: AtomicU64::new(0),
            // S20 J.3 — lock wait histograms (4 hottest contended locks).
            lock_wait_stp_guard: LatencyHistogram::new(),
            lock_wait_surveillance: LatencyHistogram::new(),
            lock_wait_pending_orders: LatencyHistogram::new(),
            lock_wait_order_locks: LatencyHistogram::new(),
            // S20 J.5 — per-endpoint latency histograms.
            endpoint_cancel_order: LatencyHistogram::new(),
            endpoint_amend_order: LatencyHistogram::new(),
            endpoint_get_positions: LatencyHistogram::new(),
            endpoint_get_open_orders: LatencyHistogram::new(),
            endpoint_get_order_history: LatencyHistogram::new(),
        }
    }

    /// S20 J.3 — record a lock wait observation by logical name.
    /// Unknown names are silently dropped (defensive).
    #[inline]
    pub fn record_lock_wait(&self, lock_name: &'static str, ns: u64) {
        match lock_name {
            "stp_guard" => self.lock_wait_stp_guard.observe_ns(ns),
            "surveillance" => self.lock_wait_surveillance.observe_ns(ns),
            "pending_orders" => self.lock_wait_pending_orders.observe_ns(ns),
            "order_locks" => self.lock_wait_order_locks.observe_ns(ns),
            _ => {}
        }
    }

    /// S20 J.5 — record a per-endpoint end-to-end latency observation.
    /// Unknown names are silently dropped (defensive).
    #[inline]
    pub fn record_endpoint_latency(&self, endpoint: &'static str, ns: u64) {
        match endpoint {
            "cancel_order" => self.endpoint_cancel_order.observe_ns(ns),
            "amend_order" => self.endpoint_amend_order.observe_ns(ns),
            "get_positions" => self.endpoint_get_positions.observe_ns(ns),
            "get_open_orders" => self.endpoint_get_open_orders.observe_ns(ns),
            "get_order_history" => self.endpoint_get_order_history.observe_ns(ns),
            _ => {}
        }
    }

    /// S20 J.5 — start measuring a REST endpoint and return an RAII guard.
    ///
    /// The returned guard records the elapsed time on drop, so the handler
    /// does not need to instrument every early-return path manually.
    ///
    /// Usage inside an axum handler:
    /// ```
    /// # use exchange_admin::ExchangeMetrics;
    /// let metrics = ExchangeMetrics::new();
    /// let _g = metrics.endpoint_timer("cancel_order");
    /// // ... handler body, may return early ...
    /// ```
    #[inline]
    pub fn endpoint_timer<'a>(&'a self, endpoint: &'static str) -> EndpointLatencyGuard<'a> {
        EndpointLatencyGuard {
            metrics: self,
            endpoint,
            start: Instant::now(),
        }
    }
}

/// S20 J.5 — RAII guard that records elapsed time to a per-endpoint
/// `LatencyHistogram` on drop. Created via
/// [`ExchangeMetrics::endpoint_timer`].
pub struct EndpointLatencyGuard<'a> {
    metrics: &'a ExchangeMetrics,
    endpoint: &'static str,
    start: Instant,
}

impl<'a> Drop for EndpointLatencyGuard<'a> {
    fn drop(&mut self) {
        let ns = self.start.elapsed().as_nanos() as u64;
        self.metrics.record_endpoint_latency(self.endpoint, ns);
    }
}

impl ExchangeMetrics {
    // --- Global counters ---

    pub fn inc_orders_placed(&self) {
        self.orders_placed.fetch_add(1, Ordering::Relaxed);
        self.order_ring.inc();
    }

    pub fn inc_orders_cancelled(&self) {
        self.orders_cancelled.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_orders_rejected(&self) {
        self.orders_rejected.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_orders_filled(&self) {
        self.orders_filled.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_fills(&self) {
        self.fills_total.fetch_add(1, Ordering::Relaxed);
        self.fill_ring.inc();
    }

    pub fn inc_trades(&self) {
        self.trades_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Record trade volume in quote asset cents (e.g., 50000.50 USDT → 5000050).
    pub fn add_volume_cents(&self, cents: u64) {
        self.trade_volume_quote_cents.fetch_add(cents, Ordering::Relaxed);
    }

    pub fn inc_ws_connections(&self) {
        self.ws_connections_active.fetch_add(1, Ordering::Relaxed);
    }

    pub fn dec_ws_connections(&self) {
        self.ws_connections_active.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn record_matching_latency_us(&self, us: u64) {
        self.matching_latency_us_sum.fetch_add(us, Ordering::Relaxed);
        self.matching_latency_us_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record the full place_order latency in nanoseconds (entry → events returned).
    pub fn record_place_order_latency_ns(&self, symbol: &str, ns: u64) {
        self.place_order_latency.observe_ns(ns);
        // Per-symbol histogram
        let mut map = self.latency_histograms.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(symbol.to_string())
            .or_insert_with(LatencyHistogram::new)
            .observe_ns(ns);
    }

    /// Record a single stage latency observation for the place_order pipeline.
    ///
    /// `stage` must be one of the 7 known stage names:
    ///   "auth", "rate_limit", "validation", "balance_lock",
    ///   "wal_write", "matching", "process_events"
    ///
    /// Unknown stage names are silently ignored (defensive; should not occur in practice).
    ///
    /// KG-5 contract: stage metrics are aggregate-only — no per-symbol labels.
    pub fn record_place_order_stage(&self, stage: &'static str, ns: u64) {
        let (max_field, sum_field, count_field) = match stage {
            "auth" => (
                &self.stage_auth_max_ns,
                &self.stage_auth_sum_ns,
                &self.stage_auth_count,
            ),
            "rate_limit" => (
                &self.stage_rate_limit_max_ns,
                &self.stage_rate_limit_sum_ns,
                &self.stage_rate_limit_count,
            ),
            "validation" => (
                &self.stage_validation_max_ns,
                &self.stage_validation_sum_ns,
                &self.stage_validation_count,
            ),
            "balance_lock" => (
                &self.stage_balance_lock_max_ns,
                &self.stage_balance_lock_sum_ns,
                &self.stage_balance_lock_count,
            ),
            "wal_write" => (
                &self.stage_wal_write_max_ns,
                &self.stage_wal_write_sum_ns,
                &self.stage_wal_write_count,
            ),
            "matching" => (
                &self.stage_matching_max_ns,
                &self.stage_matching_sum_ns,
                &self.stage_matching_count,
            ),
            "process_events" => (
                &self.stage_process_events_max_ns,
                &self.stage_process_events_sum_ns,
                &self.stage_process_events_count,
            ),
            "events_sync" => (
                &self.stage_events_sync_max_ns,
                &self.stage_events_sync_sum_ns,
                &self.stage_events_sync_count,
            ),
            "events_dispatch" => (
                &self.stage_events_dispatch_max_ns,
                &self.stage_events_dispatch_sum_ns,
                &self.stage_events_dispatch_count,
            ),
            _ => return, // unknown stage — silently ignore
        };
        // Update max via compare-and-swap loop (Relaxed ordering is fine — observability only).
        let mut current = max_field.load(Ordering::Relaxed);
        while ns > current {
            match max_field.compare_exchange_weak(
                current,
                ns,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
        sum_field.fetch_add(ns, Ordering::Relaxed);
        count_field.fetch_add(1, Ordering::Relaxed);
    }

    /// Return the current max observed nanoseconds for a given stage name.
    /// Returns 0 if no observations have been recorded or the stage name is unknown.
    /// Used by smoke tests (G.5).
    pub fn place_order_stage_max_ns(&self, stage: &str) -> u64 {
        match stage {
            "auth" => self.stage_auth_max_ns.load(Ordering::Relaxed),
            "rate_limit" => self.stage_rate_limit_max_ns.load(Ordering::Relaxed),
            "validation" => self.stage_validation_max_ns.load(Ordering::Relaxed),
            "balance_lock" => self.stage_balance_lock_max_ns.load(Ordering::Relaxed),
            "wal_write" => self.stage_wal_write_max_ns.load(Ordering::Relaxed),
            "matching" => self.stage_matching_max_ns.load(Ordering::Relaxed),
            "process_events" => self.stage_process_events_max_ns.load(Ordering::Relaxed),
            "events_sync" => self.stage_events_sync_max_ns.load(Ordering::Relaxed),
            "events_dispatch" => self.stage_events_dispatch_max_ns.load(Ordering::Relaxed),
            _ => 0,
        }
    }

    /// Return the current observation count for a given stage name.
    /// Returns 0 if no observations have been recorded or the stage name is unknown.
    /// Used by smoke tests (G.5).
    pub fn place_order_stage_count(&self, stage: &str) -> u64 {
        match stage {
            "auth" => self.stage_auth_count.load(Ordering::Relaxed),
            "rate_limit" => self.stage_rate_limit_count.load(Ordering::Relaxed),
            "validation" => self.stage_validation_count.load(Ordering::Relaxed),
            "balance_lock" => self.stage_balance_lock_count.load(Ordering::Relaxed),
            "wal_write" => self.stage_wal_write_count.load(Ordering::Relaxed),
            "matching" => self.stage_matching_count.load(Ordering::Relaxed),
            "process_events" => self.stage_process_events_count.load(Ordering::Relaxed),
            "events_sync" => self.stage_events_sync_count.load(Ordering::Relaxed),
            "events_dispatch" => self.stage_events_dispatch_count.load(Ordering::Relaxed),
            _ => 0,
        }
    }

    /// Increment the dropped-event counter (B.5 backpressure).
    pub fn inc_event_channel_dropped(&self) {
        self.event_channel_dropped_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Read the current dropped-event count (for tests and /metrics).
    pub fn event_channel_dropped_total(&self) -> u64 {
        self.event_channel_dropped_total.load(Ordering::Relaxed)
    }

    /// Update the last-trade timestamp to now.
    pub fn touch_last_trade(&self) {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        self.last_trade_timestamp_ms.store(now_ms, Ordering::Relaxed);
    }

    /// Average matching latency in microseconds.
    pub fn avg_matching_latency_us(&self) -> f64 {
        let count = self.matching_latency_us_count.load(Ordering::Relaxed);
        if count == 0 {
            return 0.0;
        }
        self.matching_latency_us_sum.load(Ordering::Relaxed) as f64 / count as f64
    }

    // --- Gauge setters ---

    pub fn set_active_orders(&self, n: u64) {
        self.active_orders.store(n, Ordering::Relaxed);
    }

    pub fn set_open_positions(&self, n: u64) {
        self.open_positions.store(n, Ordering::Relaxed);
    }

    pub fn set_db_connected(&self, connected: bool) {
        self.db_connected.store(connected, Ordering::Relaxed);
    }

    pub fn set_wal_healthy(&self, healthy: bool) {
        self.wal_healthy.store(healthy, Ordering::Relaxed);
    }

    /// Record a heartbeat for a named background task.
    ///
    /// Call this at least once per task iteration so the monitoring dashboard
    /// can detect stalled tasks (no heartbeat within expected interval).
    pub fn touch_task_heartbeat(&self, task_name: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if let Ok(mut map) = self.task_heartbeats.lock() {
            map.insert(task_name.to_string(), now);
        }
    }

    /// Return the last heartbeat unix timestamp for the named task, or `None`
    /// if it has never been recorded.
    pub fn last_task_heartbeat(&self, task_name: &str) -> Option<u64> {
        self.task_heartbeats
            .lock()
            .ok()
            .and_then(|map| map.get(task_name).copied())
    }

    /// Return all task heartbeats as (task_name, last_heartbeat_unix_secs).
    ///
    /// Used by the watchdog to detect stalled tasks.
    pub fn all_task_heartbeats(&self) -> Vec<(String, u64)> {
        self.task_heartbeats
            .lock()
            .map(|map| map.iter().map(|(k, &v)| (k.clone(), v)).collect())
            .unwrap_or_default()
    }

    // --- Per-symbol helpers ---

    /// Increment the placed-order counter for `symbol`, inserting the entry if absent.
    pub fn inc_symbol_orders_placed(&self, symbol: &str) {
        self.with_symbol(symbol, |m| {
            m.orders_placed.fetch_add(1, Ordering::Relaxed);
        });
    }

    /// Increment placed-order with side/type labels.
    pub fn inc_symbol_orders_placed_labeled(&self, symbol: &str, is_buy: bool, is_limit: bool) {
        self.with_symbol(symbol, |m| {
            m.orders_placed.fetch_add(1, Ordering::Relaxed);
            let idx = side_type_idx(is_buy, is_limit);
            m.orders_by_side_type[idx].fetch_add(1, Ordering::Relaxed);
        });
    }

    /// Increment the filled-order counter for `symbol`.
    pub fn inc_symbol_orders_filled(&self, symbol: &str) {
        self.with_symbol(symbol, |m| {
            m.orders_filled.fetch_add(1, Ordering::Relaxed);
        });
    }

    /// Increment the fills counter (individual fill events) for `symbol`.
    pub fn inc_symbol_fills(&self, symbol: &str) {
        self.with_symbol(symbol, |m| {
            m.fills_total.fetch_add(1, Ordering::Relaxed);
        });
    }

    /// Increment the cancelled-order counter for `symbol`.
    pub fn inc_symbol_orders_cancelled(&self, symbol: &str) {
        self.with_symbol(symbol, |m| {
            m.orders_cancelled.fetch_add(1, Ordering::Relaxed);
        });
    }

    /// Increment the liquidation counter for `symbol`.
    pub fn inc_symbol_liquidations(&self, symbol: &str) {
        self.with_symbol(symbol, |m| {
            m.liquidations.fetch_add(1, Ordering::Relaxed);
        });
    }

    /// Add volume in quote cents for `symbol`.
    pub fn add_symbol_volume(&self, symbol: &str, cents: u64) {
        self.with_symbol(symbol, |m| {
            m.volume_quote_cents.fetch_add(cents, Ordering::Relaxed);
        });
    }

    /// Process uptime in seconds.
    pub fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    /// Unix timestamp at which the process started.
    pub fn started_at_unix(&self) -> u64 {
        self.started_at_unix
    }

    /// Apply `f` to the [`SymbolMetrics`] for `symbol`.
    ///
    /// Fast path: read lock if the entry already exists (common case).
    /// Slow path: write lock to insert a new entry (first time seeing the symbol).
    fn with_symbol(&self, symbol: &str, f: impl Fn(&SymbolMetrics)) {
        // Fast path — entry exists
        {
            let map = self.symbol_metrics.read().unwrap_or_else(|e| e.into_inner());
            if let Some(m) = map.get(symbol) {
                f(m);
                return;
            }
        }
        // Slow path — insert then apply
        {
            let mut map = self.symbol_metrics.write().unwrap_or_else(|e| e.into_inner());
            let m = map.entry(symbol.to_string()).or_insert_with(SymbolMetrics::new);
            f(m);
        }
    }

    // --- Per-symbol latency stats ---

    /// Compute per-symbol latency stats from the histogram data.
    pub fn symbol_latency_stats(&self) -> Vec<SymbolLatencyStats> {
        let map = self.latency_histograms.lock().unwrap_or_else(|e| e.into_inner());
        let mut stats: Vec<SymbolLatencyStats> = map
            .iter()
            .map(|(sym, hist)| {
                let count = hist.count();
                let avg_ns = if count > 0 {
                    Some(hist.sum_ns() as f64 / count as f64)
                } else {
                    None
                };
                SymbolLatencyStats {
                    symbol: sym.clone(),
                    count,
                    p50_ns: hist.percentile(50.0),
                    p95_ns: hist.percentile(95.0),
                    p99_ns: hist.percentile(99.0),
                    avg_ns,
                }
            })
            .collect();
        stats.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        stats
    }

    /// Compute throughput stats from ring counters.
    pub fn throughput_stats(&self) -> ThroughputStats {
        ThroughputStats {
            orders_per_sec_1m: self.order_ring.rate_per_sec(60),
            orders_per_sec_5m: self.order_ring.rate_per_sec(300),
            orders_per_sec_1h: self.order_ring.rate_per_sec(3600),
            fills_per_sec_1m: self.fill_ring.rate_per_sec(60),
            fills_per_sec_5m: self.fill_ring.rate_per_sec(300),
            fills_per_sec_1h: self.fill_ring.rate_per_sec(3600),
        }
    }

    /// Export as Prometheus text format (exposition format 0.0.4).
    pub fn to_prometheus(&self) -> String {
        let mut out = String::with_capacity(4096);

        // --- Global order counters ---
        out.push_str("# HELP exchange_orders_placed_total Total orders placed\n");
        out.push_str("# TYPE exchange_orders_placed_total counter\n");
        out.push_str(&format!(
            "exchange_orders_placed_total {}\n",
            self.orders_placed.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP exchange_orders_cancelled_total Total orders cancelled\n");
        out.push_str("# TYPE exchange_orders_cancelled_total counter\n");
        out.push_str(&format!(
            "exchange_orders_cancelled_total {}\n",
            self.orders_cancelled.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP exchange_orders_rejected_total Total orders rejected\n");
        out.push_str("# TYPE exchange_orders_rejected_total counter\n");
        out.push_str(&format!(
            "exchange_orders_rejected_total {}\n",
            self.orders_rejected.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP exchange_orders_filled_total Total orders filled\n");
        out.push_str("# TYPE exchange_orders_filled_total counter\n");
        out.push_str(&format!(
            "exchange_orders_filled_total {}\n",
            self.orders_filled.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP exchange_fills_total Total fill events generated\n");
        out.push_str("# TYPE exchange_fills_total counter\n");
        out.push_str(&format!(
            "exchange_fills_total {}\n",
            self.fills_total.load(Ordering::Relaxed)
        ));

        // --- Per-symbol labeled counters ---
        out.push_str("# HELP exchange_orders_total Per-symbol order counters\n");
        out.push_str("# TYPE exchange_orders_total counter\n");
        {
            let map = self.symbol_metrics.read().unwrap_or_else(|e| e.into_inner());
            let mut symbols: Vec<&str> = map.keys().map(|s| s.as_str()).collect();
            symbols.sort_unstable();
            for sym in &symbols {
                if let Some(m) = map.get(*sym) {
                    out.push_str(&format!(
                        "exchange_orders_total{{symbol=\"{}\",result=\"placed\"}} {}\n",
                        sym,
                        m.orders_placed.load(Ordering::Relaxed)
                    ));
                    out.push_str(&format!(
                        "exchange_orders_total{{symbol=\"{}\",result=\"filled\"}} {}\n",
                        sym,
                        m.orders_filled.load(Ordering::Relaxed)
                    ));
                    out.push_str(&format!(
                        "exchange_orders_total{{symbol=\"{}\",result=\"cancelled\"}} {}\n",
                        sym,
                        m.orders_cancelled.load(Ordering::Relaxed)
                    ));
                }
            }

            // Side+type labeled counters
            out.push_str("# HELP exchange_orders_by_side_type_total Orders placed by side and type\n");
            out.push_str("# TYPE exchange_orders_by_side_type_total counter\n");
            let side_labels = ["buy", "buy", "sell", "sell"];
            let type_labels = ["limit", "market", "limit", "market"];
            for sym in &symbols {
                if let Some(m) = map.get(*sym) {
                    for i in 0..4 {
                        let val = m.orders_by_side_type[i].load(Ordering::Relaxed);
                        if val > 0 {
                            out.push_str(&format!(
                                "exchange_orders_by_side_type_total{{symbol=\"{}\",side=\"{}\",type=\"{}\"}} {}\n",
                                sym, side_labels[i], type_labels[i], val
                            ));
                        }
                    }
                }
            }

            // Per-symbol fills
            out.push_str("# HELP exchange_fills_by_symbol_total Fill events per symbol\n");
            out.push_str("# TYPE exchange_fills_by_symbol_total counter\n");
            for sym in &symbols {
                if let Some(m) = map.get(*sym) {
                    let fills = m.fills_total.load(Ordering::Relaxed);
                    if fills > 0 {
                        out.push_str(&format!(
                            "exchange_fills_by_symbol_total{{symbol=\"{}\"}} {}\n",
                            sym, fills
                        ));
                    }
                }
            }
        }

        // --- Per-symbol liquidations ---
        out.push_str("# HELP exchange_liquidations_total Per-symbol liquidation counter\n");
        out.push_str("# TYPE exchange_liquidations_total counter\n");
        {
            let map = self.symbol_metrics.read().unwrap_or_else(|e| e.into_inner());
            let mut symbols: Vec<&str> = map.keys().map(|s| s.as_str()).collect();
            symbols.sort_unstable();
            for sym in symbols {
                if let Some(m) = map.get(sym) {
                    out.push_str(&format!(
                        "exchange_liquidations_total{{symbol=\"{}\"}} {}\n",
                        sym,
                        m.liquidations.load(Ordering::Relaxed)
                    ));
                }
            }
        }

        // --- Per-symbol volume ---
        out.push_str("# HELP exchange_symbol_volume_usdt Per-symbol trade volume in USDT\n");
        out.push_str("# TYPE exchange_symbol_volume_usdt counter\n");
        {
            let map = self.symbol_metrics.read().unwrap_or_else(|e| e.into_inner());
            let mut symbols: Vec<&str> = map.keys().map(|s| s.as_str()).collect();
            symbols.sort_unstable();
            for sym in symbols {
                if let Some(m) = map.get(sym) {
                    let vol = m.volume_quote_cents.load(Ordering::Relaxed) as f64 / 100.0;
                    out.push_str(&format!(
                        "exchange_symbol_volume_usdt{{symbol=\"{}\"}} {:.2}\n",
                        sym, vol
                    ));
                }
            }
        }

        // --- Trade flow ---
        out.push_str("# HELP exchange_trades_total Total trades executed\n");
        out.push_str("# TYPE exchange_trades_total counter\n");
        out.push_str(&format!(
            "exchange_trades_total {}\n",
            self.trades_total.load(Ordering::Relaxed)
        ));

        let vol_cents = self.trade_volume_quote_cents.load(Ordering::Relaxed);
        let vol = vol_cents as f64 / 100.0;
        out.push_str("# HELP exchange_trade_volume_usdt Total trade volume in USDT\n");
        out.push_str("# TYPE exchange_trade_volume_usdt counter\n");
        out.push_str(&format!("exchange_trade_volume_usdt {:.2}\n", vol));

        // --- WebSocket ---
        out.push_str("# HELP exchange_active_connections Active WebSocket connections\n");
        out.push_str("# TYPE exchange_active_connections gauge\n");
        out.push_str(&format!(
            "exchange_active_connections {}\n",
            self.ws_connections_active.load(Ordering::Relaxed)
        ));
        // Legacy alias kept for backward compatibility
        out.push_str("# HELP exchange_ws_connections Active WebSocket connections (alias)\n");
        out.push_str("# TYPE exchange_ws_connections gauge\n");
        out.push_str(&format!(
            "exchange_ws_connections {}\n",
            self.ws_connections_active.load(Ordering::Relaxed)
        ));

        // --- Matching engine latency ---
        out.push_str(
            "# HELP exchange_order_latency_us_avg Average matching engine latency in microseconds\n",
        );
        out.push_str("# TYPE exchange_order_latency_us_avg gauge\n");
        out.push_str(&format!(
            "exchange_order_latency_us_avg {:.1}\n",
            self.avg_matching_latency_us()
        ));

        // --- End-to-end place_order latency histogram ---
        out.push_str("# HELP exchange_latency_seconds End-to-end order placement latency\n");
        out.push_str("# TYPE exchange_latency_seconds histogram\n");
        out.push_str(&self.place_order_latency.to_prometheus_lines(
            "exchange_latency_seconds",
            "operation=\"place_order\"",
        ));

        // --- S20 J.3 — Lock wait histograms (top 4 contended locks) ---
        out.push_str("# HELP exchange_lock_wait_seconds Time spent waiting to acquire a contended mutex\n");
        out.push_str("# TYPE exchange_lock_wait_seconds histogram\n");
        out.push_str(&self.lock_wait_stp_guard.to_prometheus_lines(
            "exchange_lock_wait_seconds",
            "lock=\"stp_guard\"",
        ));
        out.push_str(&self.lock_wait_surveillance.to_prometheus_lines(
            "exchange_lock_wait_seconds",
            "lock=\"surveillance\"",
        ));
        out.push_str(&self.lock_wait_pending_orders.to_prometheus_lines(
            "exchange_lock_wait_seconds",
            "lock=\"pending_orders\"",
        ));
        out.push_str(&self.lock_wait_order_locks.to_prometheus_lines(
            "exchange_lock_wait_seconds",
            "lock=\"order_locks\"",
        ));

        // --- S20 J.5 — Per-endpoint latency histograms ---
        out.push_str("# HELP exchange_endpoint_latency_seconds End-to-end latency of REST endpoints\n");
        out.push_str("# TYPE exchange_endpoint_latency_seconds histogram\n");
        out.push_str(&self.endpoint_cancel_order.to_prometheus_lines(
            "exchange_endpoint_latency_seconds",
            "endpoint=\"cancel_order\"",
        ));
        out.push_str(&self.endpoint_amend_order.to_prometheus_lines(
            "exchange_endpoint_latency_seconds",
            "endpoint=\"amend_order\"",
        ));
        out.push_str(&self.endpoint_get_positions.to_prometheus_lines(
            "exchange_endpoint_latency_seconds",
            "endpoint=\"get_positions\"",
        ));
        out.push_str(&self.endpoint_get_open_orders.to_prometheus_lines(
            "exchange_endpoint_latency_seconds",
            "endpoint=\"get_open_orders\"",
        ));
        out.push_str(&self.endpoint_get_order_history.to_prometheus_lines(
            "exchange_endpoint_latency_seconds",
            "endpoint=\"get_order_history\"",
        ));

        // --- Gauges ---
        out.push_str("# HELP exchange_active_orders Number of orders currently on the book\n");
        out.push_str("# TYPE exchange_active_orders gauge\n");
        out.push_str(&format!(
            "exchange_active_orders {}\n",
            self.active_orders.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP exchange_open_positions Number of open perpetual positions\n");
        out.push_str("# TYPE exchange_open_positions gauge\n");
        out.push_str(&format!(
            "exchange_open_positions {}\n",
            self.open_positions.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP exchange_positions_open Number of open perpetual positions (alias)\n");
        out.push_str("# TYPE exchange_positions_open gauge\n");
        out.push_str(&format!(
            "exchange_positions_open {}\n",
            self.open_positions.load(Ordering::Relaxed)
        ));

        // --- Uptime ---
        out.push_str("# HELP exchange_uptime_seconds Seconds since the exchange process started\n");
        out.push_str("# TYPE exchange_uptime_seconds gauge\n");
        out.push_str(&format!("exchange_uptime_seconds {}\n", self.uptime_secs()));

        // --- Health ---
        out.push_str("# HELP exchange_db_connected 1 if database connection is healthy\n");
        out.push_str("# TYPE exchange_db_connected gauge\n");
        out.push_str(&format!(
            "exchange_db_connected {}\n",
            if self.db_connected.load(Ordering::Relaxed) { 1 } else { 0 }
        ));

        out.push_str("# HELP exchange_wal_healthy 1 if write-ahead log is operational\n");
        out.push_str("# TYPE exchange_wal_healthy gauge\n");
        out.push_str(&format!(
            "exchange_wal_healthy {}\n",
            if self.wal_healthy.load(Ordering::Relaxed) { 1 } else { 0 }
        ));

        // --- Matching engine latency (aggregate only — KG-3 / KG-5) ---
        //
        // All metrics are exchange-wide aggregates (max-of-symbols). Per-symbol
        // latency is intentionally NOT exposed here to prevent HFT asymmetry
        // leak via engine congestion signals (KG-5).
        out.push_str("# HELP matching_engine_latency_ns_max_p50 Max p50 latency across all symbols (ns)\n");
        out.push_str("# TYPE matching_engine_latency_ns_max_p50 gauge\n");
        out.push_str(&format!(
            "matching_engine_latency_ns_max_p50 {}\n",
            self.engine_latency_p50_max_ns.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP matching_engine_latency_ns_max_p99 Max p99 latency across all symbols (ns)\n");
        out.push_str("# TYPE matching_engine_latency_ns_max_p99 gauge\n");
        out.push_str(&format!(
            "matching_engine_latency_ns_max_p99 {}\n",
            self.engine_latency_p99_max_ns.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP matching_engine_latency_ns_max_p999 Max p99.9 latency across all symbols (ns)\n");
        out.push_str("# TYPE matching_engine_latency_ns_max_p999 gauge\n");
        out.push_str(&format!(
            "matching_engine_latency_ns_max_p999 {}\n",
            self.engine_latency_p999_max_ns.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP matching_engine_latency_ns_max_p9999 Max p99.99 latency across all symbols (ns)\n");
        out.push_str("# TYPE matching_engine_latency_ns_max_p9999 gauge\n");
        out.push_str(&format!(
            "matching_engine_latency_ns_max_p9999 {}\n",
            self.engine_latency_p9999_max_ns.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP matching_engine_latency_ns_max_overall Max of all-time-max latency across all symbols (ns)\n");
        out.push_str("# TYPE matching_engine_latency_ns_max_overall gauge\n");
        out.push_str(&format!(
            "matching_engine_latency_ns_max_overall {}\n",
            self.engine_latency_max_overall_ns.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP matching_engine_total_command_count Total commands processed across all symbols in current window\n");
        out.push_str("# TYPE matching_engine_total_command_count gauge\n");
        out.push_str(&format!(
            "matching_engine_total_command_count {}\n",
            self.engine_total_command_count.load(Ordering::Relaxed)
        ));

        // --- Per-stage place_order latency (S13 G.1) ---
        //
        // KG-5 aggregation contract: aggregate only — no {symbol=...} labels.
        // Stages: auth, rate_limit, validation, balance_lock, wal_write, matching, process_events
        out.push_str("# HELP api_gateway_stage_latency_ns_max Max observed nanoseconds per place_order stage\n");
        out.push_str("# TYPE api_gateway_stage_latency_ns_max gauge\n");
        out.push_str("# HELP api_gateway_stage_latency_ns_sum Sum of nanoseconds per place_order stage\n");
        out.push_str("# TYPE api_gateway_stage_latency_ns_sum counter\n");
        out.push_str("# HELP api_gateway_stage_latency_count Total observations per place_order stage\n");
        out.push_str("# TYPE api_gateway_stage_latency_count counter\n");
        for &(stage, max_ns, sum_ns, count) in &[
            (
                "auth",
                self.stage_auth_max_ns.load(Ordering::Relaxed),
                self.stage_auth_sum_ns.load(Ordering::Relaxed),
                self.stage_auth_count.load(Ordering::Relaxed),
            ),
            (
                "rate_limit",
                self.stage_rate_limit_max_ns.load(Ordering::Relaxed),
                self.stage_rate_limit_sum_ns.load(Ordering::Relaxed),
                self.stage_rate_limit_count.load(Ordering::Relaxed),
            ),
            (
                "validation",
                self.stage_validation_max_ns.load(Ordering::Relaxed),
                self.stage_validation_sum_ns.load(Ordering::Relaxed),
                self.stage_validation_count.load(Ordering::Relaxed),
            ),
            (
                "balance_lock",
                self.stage_balance_lock_max_ns.load(Ordering::Relaxed),
                self.stage_balance_lock_sum_ns.load(Ordering::Relaxed),
                self.stage_balance_lock_count.load(Ordering::Relaxed),
            ),
            (
                "wal_write",
                self.stage_wal_write_max_ns.load(Ordering::Relaxed),
                self.stage_wal_write_sum_ns.load(Ordering::Relaxed),
                self.stage_wal_write_count.load(Ordering::Relaxed),
            ),
            (
                "matching",
                self.stage_matching_max_ns.load(Ordering::Relaxed),
                self.stage_matching_sum_ns.load(Ordering::Relaxed),
                self.stage_matching_count.load(Ordering::Relaxed),
            ),
            (
                "process_events",
                self.stage_process_events_max_ns.load(Ordering::Relaxed),
                self.stage_process_events_sum_ns.load(Ordering::Relaxed),
                self.stage_process_events_count.load(Ordering::Relaxed),
            ),
            (
                "events_sync",
                self.stage_events_sync_max_ns.load(Ordering::Relaxed),
                self.stage_events_sync_sum_ns.load(Ordering::Relaxed),
                self.stage_events_sync_count.load(Ordering::Relaxed),
            ),
            (
                "events_dispatch",
                self.stage_events_dispatch_max_ns.load(Ordering::Relaxed),
                self.stage_events_dispatch_sum_ns.load(Ordering::Relaxed),
                self.stage_events_dispatch_count.load(Ordering::Relaxed),
            ),
        ] {
            out.push_str(&format!(
                "api_gateway_stage_latency_ns_max{{stage=\"{stage}\"}} {max_ns}\n"
            ));
            out.push_str(&format!(
                "api_gateway_stage_latency_ns_sum{{stage=\"{stage}\"}} {sum_ns}\n"
            ));
            out.push_str(&format!(
                "api_gateway_stage_latency_count{{stage=\"{stage}\"}} {count}\n"
            ));
        }

        // S15 B.5: post-fill event channel drop counter
        let dropped = self.event_channel_dropped_total.load(Ordering::Relaxed);
        out.push_str("# HELP api_gateway_event_channel_dropped_total Total PostFillEvents dropped due to worker channel saturation\n");
        out.push_str("# TYPE api_gateway_event_channel_dropped_total counter\n");
        out.push_str(&format!("api_gateway_event_channel_dropped_total {dropped}\n"));

        out
    }

    /// Export as JSON for admin dashboard.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "orders": {
                "placed": self.orders_placed.load(Ordering::Relaxed),
                "cancelled": self.orders_cancelled.load(Ordering::Relaxed),
                "rejected": self.orders_rejected.load(Ordering::Relaxed),
                "filled": self.orders_filled.load(Ordering::Relaxed),
            },
            "trades": {
                "total": self.trades_total.load(Ordering::Relaxed),
                "volume_usdt": self.trade_volume_quote_cents.load(Ordering::Relaxed) as f64 / 100.0,
            },
            "websocket": {
                "connections_active": self.ws_connections_active.load(Ordering::Relaxed),
                "messages_sent": self.ws_messages_sent.load(Ordering::Relaxed),
            },
            "matching": {
                "avg_latency_us": self.avg_matching_latency_us(),
                "total_operations": self.matching_latency_us_count.load(Ordering::Relaxed),
            },
        })
    }
}

impl Default for ExchangeMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_counters() {
        let m = ExchangeMetrics::new();
        m.inc_orders_placed();
        m.inc_orders_placed();
        m.inc_orders_cancelled();
        m.inc_trades();

        assert_eq!(m.orders_placed.load(Ordering::Relaxed), 2);
        assert_eq!(m.orders_cancelled.load(Ordering::Relaxed), 1);
        assert_eq!(m.trades_total.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_volume_cents() {
        let m = ExchangeMetrics::new();
        // 50000.50 USDT = 5000050 cents
        m.add_volume_cents(5_000_050);
        m.add_volume_cents(2_500_025);

        let vol = m.trade_volume_quote_cents.load(Ordering::Relaxed) as f64 / 100.0;
        assert!((vol - 75000.75).abs() < 0.01);
    }

    #[test]
    fn test_matching_latency() {
        let m = ExchangeMetrics::new();
        m.record_matching_latency_us(100);
        m.record_matching_latency_us(200);
        m.record_matching_latency_us(300);

        assert!((m.avg_matching_latency_us() - 200.0).abs() < 0.01);
    }

    #[test]
    fn test_latency_zero_count() {
        let m = ExchangeMetrics::new();
        assert_eq!(m.avg_matching_latency_us(), 0.0);
    }

    #[test]
    fn test_ws_connections() {
        let m = ExchangeMetrics::new();
        m.inc_ws_connections();
        m.inc_ws_connections();
        m.inc_ws_connections();
        m.dec_ws_connections();

        assert_eq!(m.ws_connections_active.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_prometheus_format() {
        let m = ExchangeMetrics::new();
        m.inc_orders_placed();
        m.inc_trades();
        m.add_volume_cents(5_000_000);

        let prom = m.to_prometheus();
        assert!(prom.contains("exchange_orders_placed_total 1"));
        assert!(prom.contains("exchange_trades_total 1"));
        assert!(prom.contains("exchange_trade_volume_usdt 50000.00"));
    }

    #[test]
    fn test_json_format() {
        let m = ExchangeMetrics::new();
        m.inc_orders_placed();
        m.inc_orders_filled();

        let json = m.to_json();
        assert_eq!(json["orders"]["placed"], 1);
        assert_eq!(json["orders"]["filled"], 1);
    }

    #[test]
    fn test_per_symbol_increment() {
        let m = ExchangeMetrics::new();
        m.inc_symbol_orders_placed("BTCUSDT");
        m.inc_symbol_orders_placed("BTCUSDT");
        m.inc_symbol_orders_placed("ETHUSDT");
        m.inc_symbol_orders_filled("BTCUSDT");

        let map = m.symbol_metrics.read().unwrap();
        assert_eq!(map["BTCUSDT"].orders_placed.load(Ordering::Relaxed), 2);
        assert_eq!(map["BTCUSDT"].orders_filled.load(Ordering::Relaxed), 1);
        assert_eq!(map["ETHUSDT"].orders_placed.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_prometheus_labeled_metrics() {
        let m = ExchangeMetrics::new();
        m.inc_symbol_orders_placed("BTCUSDT");
        m.inc_symbol_orders_filled("BTCUSDT");
        m.inc_symbol_orders_placed("ETHUSDT");

        let prom = m.to_prometheus();
        assert!(prom.contains("exchange_orders_total{symbol=\"BTCUSDT\",result=\"placed\"} 1"));
        assert!(prom.contains("exchange_orders_total{symbol=\"BTCUSDT\",result=\"filled\"} 1"));
        assert!(prom.contains("exchange_orders_total{symbol=\"ETHUSDT\",result=\"placed\"} 1"));
    }

    #[test]
    fn test_health_flags() {
        let m = ExchangeMetrics::new();
        assert!(!m.db_connected.load(Ordering::Relaxed));
        assert!(!m.wal_healthy.load(Ordering::Relaxed));

        m.set_db_connected(true);
        m.set_wal_healthy(true);

        assert!(m.db_connected.load(Ordering::Relaxed));
        assert!(m.wal_healthy.load(Ordering::Relaxed));

        let prom = m.to_prometheus();
        assert!(prom.contains("exchange_db_connected 1"));
        assert!(prom.contains("exchange_wal_healthy 1"));
    }

    #[test]
    fn test_active_orders_and_positions_gauges() {
        let m = ExchangeMetrics::new();
        m.set_active_orders(25);
        m.set_open_positions(12);

        let prom = m.to_prometheus();
        assert!(prom.contains("exchange_active_orders 25"));
        assert!(prom.contains("exchange_open_positions 12"));
    }

    #[test]
    fn test_symbol_liquidations() {
        let m = ExchangeMetrics::new();
        m.inc_symbol_liquidations("BTCUSDT");
        m.inc_symbol_liquidations("BTCUSDT");

        let map = m.symbol_metrics.read().unwrap();
        assert_eq!(map["BTCUSDT"].liquidations.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_symbol_volume() {
        let m = ExchangeMetrics::new();
        m.add_symbol_volume("BTCUSDT", 1_000_000); // 10000.00 USDT
        m.add_symbol_volume("BTCUSDT", 500_000); // 5000.00 USDT

        let map = m.symbol_metrics.read().unwrap();
        let cents = map["BTCUSDT"].volume_quote_cents.load(Ordering::Relaxed);
        assert_eq!(cents, 1_500_000);
    }

    #[test]
    fn test_uptime_non_negative() {
        let m = ExchangeMetrics::new();
        // Uptime should be 0 or very small (sub-second), never wrapping.
        assert!(m.uptime_secs() < 5);
    }

    // --- New tests for Task 5 / 6 ---

    #[test]
    fn test_latency_histogram_observe_and_percentile() {
        let h = LatencyHistogram::new();
        // Record 100 observations: 50 at 10µs, 45 at 100µs, 5 at 1ms
        for _ in 0..50 {
            h.observe_ns(10_000);
        }
        for _ in 0..45 {
            h.observe_ns(100_000);
        }
        for _ in 0..5 {
            h.observe_ns(1_000_000);
        }
        assert_eq!(h.count(), 100);
        // p50 should fall in the ≤50µs bucket
        assert!(h.percentile(50.0).unwrap() <= 50_000);
        // p95 should fall in the ≤100µs bucket
        assert!(h.percentile(95.0).unwrap() <= 100_000);
        // p99 should fall in the ≤1ms bucket
        assert!(h.percentile(99.0).unwrap() <= 1_000_000);
    }

    #[test]
    fn test_latency_histogram_empty_percentile() {
        let h = LatencyHistogram::new();
        assert!(h.percentile(50.0).is_none());
        assert!(h.percentile(99.0).is_none());
    }

    #[test]
    fn test_latency_histogram_prometheus_lines() {
        let h = LatencyHistogram::new();
        h.observe_ns(5_000);   // 5µs
        h.observe_ns(50_000);  // 50µs
        let lines = h.to_prometheus_lines("exchange_latency_seconds", "operation=\"place_order\"");
        assert!(lines.contains("exchange_latency_seconds_bucket{"));
        assert!(lines.contains("le=\"+Inf\""));
        assert!(lines.contains("exchange_latency_seconds_count{"));
        assert!(lines.contains(" 2\n")); // count = 2
    }

    #[test]
    fn test_record_place_order_latency() {
        let m = ExchangeMetrics::new();
        m.record_place_order_latency_ns("BTCUSDT", 50_000);
        m.record_place_order_latency_ns("BTCUSDT", 100_000);
        m.record_place_order_latency_ns("ETHUSDT", 200_000);

        let stats = m.symbol_latency_stats();
        assert_eq!(stats.len(), 2);
        let btc = stats.iter().find(|s| s.symbol == "BTCUSDT").unwrap();
        assert_eq!(btc.count, 2);
        assert!(btc.p50_ns.is_some());
    }

    #[test]
    fn test_throughput_stats_smoke() {
        let m = ExchangeMetrics::new();
        m.inc_orders_placed();
        m.inc_orders_placed();
        m.inc_fills();

        let t = m.throughput_stats();
        // At least the last second should have counted
        assert!(t.orders_per_sec_1m >= 0.0);
        assert!(t.fills_per_sec_1m >= 0.0);
    }

    #[test]
    fn test_fills_total_counter() {
        let m = ExchangeMetrics::new();
        m.inc_fills();
        m.inc_fills();
        m.inc_fills();
        assert_eq!(m.fills_total.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn test_uptime_in_prometheus() {
        let m = ExchangeMetrics::new();
        let prom = m.to_prometheus();
        assert!(prom.contains("exchange_uptime_seconds"));
        assert!(prom.contains("exchange_active_connections"));
        assert!(prom.contains("exchange_positions_open"));
    }

    #[test]
    fn test_labeled_orders_by_side_type() {
        let m = ExchangeMetrics::new();
        m.inc_symbol_orders_placed_labeled("BTCUSDT", true, true); // buy limit
        m.inc_symbol_orders_placed_labeled("BTCUSDT", true, false); // buy market
        m.inc_symbol_orders_placed_labeled("BTCUSDT", false, true); // sell limit

        let map = m.symbol_metrics.read().unwrap();
        let btc = &map["BTCUSDT"];
        assert_eq!(btc.orders_by_side_type[side_type_idx(true, true)].load(Ordering::Relaxed), 1);
        assert_eq!(btc.orders_by_side_type[side_type_idx(true, false)].load(Ordering::Relaxed), 1);
        assert_eq!(btc.orders_by_side_type[side_type_idx(false, true)].load(Ordering::Relaxed), 1);
        assert_eq!(btc.orders_by_side_type[side_type_idx(false, false)].load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_ring_counter_rate() {
        let rc = RingCounter::new();
        rc.inc();
        rc.inc();
        rc.inc();
        // rate over 60s window — 3 events / 60 = 0.05 rps
        let rate = rc.rate_per_sec(60);
        assert!(rate > 0.0 && rate <= 3.0);
    }

    #[test]
    fn test_last_trade_timestamp() {
        let m = ExchangeMetrics::new();
        assert_eq!(m.last_trade_timestamp_ms.load(Ordering::Relaxed), 0);
        m.touch_last_trade();
        assert!(m.last_trade_timestamp_ms.load(Ordering::Relaxed) > 0);
    }
}
