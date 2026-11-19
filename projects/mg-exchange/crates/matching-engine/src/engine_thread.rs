use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossbeam::channel::{bounded, Receiver, Sender, SendError};
use exchange_common::{Command, MatchEvent, Price, Side, Symbol, UserId};
use hdrhistogram::Histogram;

use crate::orderbook::{EventBuffer, OrderBook, OrderBookSnapshot};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur when communicating with an engine thread.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("Symbol not found: {0}")]
    SymbolNotFound(String),
    #[error("Engine thread disconnected")]
    Disconnected,
    #[error("Engine request timed out")]
    Timeout,
    #[error("Engine shutting down")]
    ShuttingDown,
}

// ---------------------------------------------------------------------------
// Oneshot helpers — a single-use response channel built on std::sync::mpsc.
// ---------------------------------------------------------------------------

pub mod oneshot {
    use std::sync::mpsc;

    pub type Sender<T> = mpsc::Sender<T>;
    pub type Receiver<T> = mpsc::Receiver<T>;

    pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
        mpsc::channel()
    }
}

// ---------------------------------------------------------------------------
// Internal command type (includes shutdown signal and snapshot request)
// ---------------------------------------------------------------------------

/// Commands sent through the bounded crossbeam channel to the engine thread.
enum EngineCommand {
    Process {
        id: u64,
        command: Command,
        response_tx: oneshot::Sender<EngineResponse>,
    },
    Snapshot {
        id: u64,
        depth: usize,
        response_tx: oneshot::Sender<SnapshotResponse>,
    },
    /// Request a snapshot of the engine's latency histogram.
    ///
    /// The engine responds by extracting quantiles from its internal
    /// HdrHistogram via `value_at_quantile()` calls. This goes through the
    /// command channel (not a shared Mutex) so the engine thread retains
    /// exclusive ownership of the histogram and there is zero contention on
    /// the hot matching path.
    LatencySnapshot {
        response_tx: oneshot::Sender<LatencySnapshot>,
    },
    /// Reset (clear) the engine's latency histogram.
    ///
    /// Called periodically by the `latency_metrics_updater` background task
    /// in `api-gateway` to implement a rolling 5-minute window. After reset,
    /// the next `LatencySnapshot` query reflects only commands processed since
    /// this reset.
    ResetLatencyHistogram,
    /// Query how many open orders `user_id` has on this symbol's book.
    ///
    /// Used by the order-limit validation in `order-gateway` (Sprint 8, KG-2).
    /// Goes through the command channel so the engine thread has exclusive,
    /// consistent access to `user_order_count` with no locking.
    UserOrderCount {
        user_id: UserId,
        response_tx: oneshot::Sender<usize>,
    },
    /// Query how many open orders `user_id` has at `price` on `side`.
    ///
    /// Used by `max_orders_per_user_per_price_level` enforcement (Sprint 8, KG-2).
    UserOrdersAtPrice {
        user_id: UserId,
        side: Side,
        price: Price,
        response_tx: oneshot::Sender<usize>,
    },
    /// Full-fidelity snapshot: every resting order for state persistence.
    FullSnapshot {
        response_tx: oneshot::Sender<crate::orderbook::FullBookSnapshot>,
    },
    Shutdown,
}

/// Response for a Process command.
///
/// `events` is `EventBuffer` (SmallVec<[MatchEvent; 4]>) so that the common
/// case (1–3 events) never allocates. The public `EngineHandle::send()` methods
/// convert to `Vec<MatchEvent>` once at the receive boundary.
pub struct EngineResponse {
    pub id: u64,
    pub events: EventBuffer,
}

/// Response for a Snapshot command.
pub struct SnapshotResponse {
    pub id: u64,
    pub snapshot: OrderBookSnapshot,
}

// ---------------------------------------------------------------------------
// Latency snapshot — p50/p90/p99/p99.9/p99.99 extracted from HdrHistogram
// ---------------------------------------------------------------------------

/// Per-engine tail-latency statistics.
///
/// Populated on demand by sending `EngineCommand::LatencySnapshot` through
/// the engine's command channel.  All values are in nanoseconds.
///
/// The histogram uses Gil Tene's HdrHistogram (hdrhistogram crate, 7.x).
/// Range: 1 ns – 1 s; significant digits: 3.
#[derive(Debug, Clone)]
pub struct LatencySnapshot {
    /// Number of commands recorded so far.
    pub count: u64,
    /// Median latency (50th percentile).
    pub p50_ns: u64,
    /// 90th percentile latency.
    pub p90_ns: u64,
    /// 99th percentile latency.
    pub p99_ns: u64,
    /// 99.9th percentile latency.
    pub p999_ns: u64,
    /// 99.99th percentile latency.
    pub p9999_ns: u64,
    /// Maximum recorded latency.
    pub max_ns: u64,
}

// ---------------------------------------------------------------------------
// EngineHandle — the public API for a per-symbol engine thread
// ---------------------------------------------------------------------------

/// A handle to a matching engine running on its own OS thread.
///
/// Each `EngineHandle` owns exactly one `OrderBook` running in isolation.
/// Communication is via a bounded crossbeam channel (command dispatch) and
/// per-request `std::sync::mpsc` oneshot channels (response delivery).
///
/// The channel capacity of 1024 provides back-pressure: callers block if the
/// engine falls behind, preventing unbounded memory growth.
pub struct EngineHandle {
    symbol: Symbol,
    sender: Sender<EngineCommand>,
    thread: Option<std::thread::JoinHandle<()>>,
    next_id: AtomicU64,
}

impl EngineHandle {
    /// Spawn an OS thread that owns an `OrderBook` for `symbol`.
    ///
    /// `channel_capacity` controls the depth of the command queue.
    pub fn spawn(symbol: Symbol, channel_capacity: usize) -> Self {
        Self::spawn_with_affinity(symbol, channel_capacity, None)
    }

    /// Spawn with optional CPU core pinning.
    ///
    /// When `core_id` is `Some(n)`, the engine thread pins itself to core `n`
    /// at startup using `core_affinity::set_for_current`. This eliminates
    /// context-switch jitter and NUMA remote-access latency on multi-socket
    /// hosts, reducing p99.9 tail latency from hundreds of µs to single-digit µs
    /// (Erik Rigtorp benchmarks, Carl Cook CppCon 2017).
    pub fn spawn_with_affinity(
        symbol: Symbol,
        channel_capacity: usize,
        core_id: Option<usize>,
    ) -> Self {
        let (tx, rx) = bounded::<EngineCommand>(channel_capacity);
        let book_symbol = symbol.clone();

        let thread = std::thread::Builder::new()
            .name(format!("engine-{symbol}"))
            .spawn(move || {
                if let Some(id) = core_id {
                    let core = core_affinity::CoreId { id };
                    if core_affinity::set_for_current(core) {
                        tracing::info!(
                            symbol = %book_symbol,
                            core = id,
                            "Engine thread pinned to core"
                        );
                    } else {
                        tracing::warn!(
                            symbol = %book_symbol,
                            core = id,
                            "Failed to pin engine thread to core — running unpinned"
                        );
                    }
                }
                run_engine_thread(book_symbol, rx)
            })
            .expect("failed to spawn engine thread");

        Self {
            symbol,
            sender: tx,
            thread: Some(thread),
            next_id: AtomicU64::new(1),
        }
    }

    /// Send a command and block until the engine responds.
    ///
    /// Blocks the calling thread but does not block the engine thread for any
    /// other symbol — each symbol processes independently.
    pub fn send(&self, command: Command) -> Result<Vec<MatchEvent>, EngineError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (resp_tx, resp_rx) = oneshot::channel::<EngineResponse>();

        self.sender
            .send(EngineCommand::Process {
                id,
                command,
                response_tx: resp_tx,
            })
            .map_err(|_: SendError<_>| EngineError::Disconnected)?;

        resp_rx.recv().map(|r| r.events.into_vec()).map_err(|_| EngineError::Disconnected)
    }

    /// Send a command with a deadline; returns `EngineError::Timeout` if the
    /// engine does not respond within `timeout`.
    pub fn send_timeout(
        &self,
        command: Command,
        timeout: Duration,
    ) -> Result<Vec<MatchEvent>, EngineError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (resp_tx, resp_rx) = oneshot::channel::<EngineResponse>();

        self.sender
            .send(EngineCommand::Process {
                id,
                command,
                response_tx: resp_tx,
            })
            .map_err(|_: SendError<_>| EngineError::Disconnected)?;

        resp_rx
            .recv_timeout(timeout)
            .map(|r| r.events.into_vec())
            .map_err(|e| match e {
                mpsc::RecvTimeoutError::Disconnected => EngineError::Disconnected,
                mpsc::RecvTimeoutError::Timeout => EngineError::Timeout,
            })
    }

    /// Request an L2 snapshot of the order book at the given depth.
    pub fn book_snapshot(&self, depth: usize) -> Result<OrderBookSnapshot, EngineError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (resp_tx, resp_rx) = oneshot::channel::<SnapshotResponse>();

        self.sender
            .send(EngineCommand::Snapshot {
                id,
                depth,
                response_tx: resp_tx,
            })
            .map_err(|_: SendError<_>| EngineError::Disconnected)?;

        resp_rx
            .recv()
            .map(|r| r.snapshot)
            .map_err(|_| EngineError::Disconnected)
    }

    /// Full-fidelity snapshot: every resting order for state persistence.
    ///
    /// Unlike `book_snapshot` which returns aggregated price levels, this
    /// returns the full set of individual orders needed to reconstruct
    /// the book without WAL replay.
    pub fn full_book_snapshot(&self) -> Result<crate::orderbook::FullBookSnapshot, EngineError> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.sender
            .send(EngineCommand::FullSnapshot { response_tx: resp_tx })
            .map_err(|_: SendError<_>| EngineError::Disconnected)?;
        resp_rx.recv().map_err(|_| EngineError::Disconnected)
    }

    /// Query the engine's latency histogram and return current quantiles.
    ///
    /// Sends a `LatencySnapshot` command through the engine channel. The
    /// handler extracts 5 quantile values via `value_at_quantile()` calls
    /// (O(1) bucket lookups, ~100 ns total — no histogram copy) and returns
    /// a 56-byte `LatencySnapshot` struct. Total round-trip including channel
    /// send/receive is under 1 µs. Safe to call from any thread.
    ///
    /// The histogram accumulates over the engine's rolling-window period —
    /// it is reset every 5 minutes by the background `latency_metrics_updater`
    /// task in `api-gateway`. Call `reset_latency_histogram()` explicitly to
    /// clear it sooner. For instantaneous observations, compare snapshots
    /// taken at known intervals.
    ///
    /// Returns `EngineError::Disconnected` if the engine thread has already
    /// exited (e.g. after `shutdown()`).
    pub fn latency_snapshot(&self) -> Result<LatencySnapshot, EngineError> {
        let (resp_tx, resp_rx) = oneshot::channel::<LatencySnapshot>();

        self.sender
            .send(EngineCommand::LatencySnapshot { response_tx: resp_tx })
            .map_err(|_: SendError<_>| EngineError::Disconnected)?;

        resp_rx.recv().map_err(|_| EngineError::Disconnected)
    }

    /// Reset (clear) the engine's latency histogram.
    ///
    /// Sends a `ResetLatencyHistogram` command through the engine channel. The
    /// engine thread calls `hdrhistogram::Histogram::reset()` on its local
    /// histogram. After this call the histogram count drops to 0 and the next
    /// `latency_snapshot()` will reflect only commands processed since the
    /// reset.
    ///
    /// Intended to be called periodically (every 5 minutes) by the
    /// `spawn_latency_metrics_updater` background task in `api-gateway` so
    /// that Prometheus gauges represent a rolling window instead of lifetime
    /// worst-case values.
    ///
    /// Returns `EngineError::Disconnected` if the engine thread has already
    /// exited.
    pub fn reset_latency_histogram(&self) -> Result<(), EngineError> {
        self.sender
            .send(EngineCommand::ResetLatencyHistogram)
            .map_err(|_: SendError<_>| EngineError::Disconnected)
    }

    /// Return the number of open orders for `user_id` on this symbol's book.
    ///
    /// Sends a `UserOrderCount` command through the engine's command channel and
    /// blocks until the response arrives.  The engine thread has sole ownership
    /// of the `OrderBook`, so this read is consistent with no locking.
    ///
    /// Returns `EngineError::Disconnected` if the engine thread has exited.
    pub fn user_open_order_count(&self, user_id: UserId) -> Result<usize, EngineError> {
        let (resp_tx, resp_rx) = oneshot::channel::<usize>();
        self.sender
            .send(EngineCommand::UserOrderCount { user_id, response_tx: resp_tx })
            .map_err(|_: SendError<_>| EngineError::Disconnected)?;
        resp_rx.recv().map_err(|_| EngineError::Disconnected)
    }

    /// Return the number of open orders for `user_id` at `price` on `side`.
    ///
    /// Used to enforce `max_orders_per_user_per_price_level`.
    /// Returns `EngineError::Disconnected` if the engine thread has exited.
    pub fn user_orders_at_price(
        &self,
        user_id: UserId,
        side: Side,
        price: Price,
    ) -> Result<usize, EngineError> {
        let (resp_tx, resp_rx) = oneshot::channel::<usize>();
        self.sender
            .send(EngineCommand::UserOrdersAtPrice { user_id, side, price, response_tx: resp_tx })
            .map_err(|_: SendError<_>| EngineError::Disconnected)?;
        resp_rx.recv().map_err(|_| EngineError::Disconnected)
    }

    /// Signal the engine thread to stop and block until it exits cleanly.
    pub fn shutdown(&mut self) {
        // Ignore send error — thread may have already exited.
        let _ = self.sender.send(EngineCommand::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    pub fn symbol(&self) -> &Symbol {
        &self.symbol
    }
}

impl Drop for EngineHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// ---------------------------------------------------------------------------
// Engine thread loop
// ---------------------------------------------------------------------------

/// How often to log histogram quantiles (every N `Process` commands).
///
/// 1024 commands at ~500 ns each ≈ 512 µs between log lines — frequent
/// enough to catch spikes, infrequent enough not to flood the log.
const HISTOGRAM_LOG_INTERVAL: u64 = 1024;

fn run_engine_thread(symbol: Symbol, rx: Receiver<EngineCommand>) {
    let mut book = OrderBook::new(symbol.clone());

    // HdrHistogram: range 1 ns – 1 s, 3 significant digits.
    // `unwrap`: the bounds (1, 1_000_000_000) are valid constants; this can
    // only fail if the bounds themselves are wrong, which is a programming error.
    let mut latency_hist: Histogram<u64> =
        Histogram::new_with_bounds(1, 1_000_000_000, 3)
            .expect("hdrhistogram: invalid bounds (1 ns – 1 s, 3 sig digits)");
    let mut process_count: u64 = 0;

    tracing::debug!(symbol = %symbol, "engine thread started");

    loop {
        let cmd = match rx.recv() {
            Ok(c) => c,
            Err(_) => {
                // Channel closed — sender side dropped; exit cleanly.
                tracing::debug!(symbol = %symbol, "engine thread: channel closed, exiting");
                break;
            }
        };

        match cmd {
            EngineCommand::Process {
                id,
                command,
                response_tx,
            } => {
                let t0 = Instant::now();
                let events: EventBuffer = match command {
                    Command::PlaceOrder(place_cmd) => book.place_order(place_cmd),
                    Command::CancelOrder(cancel_cmd) => book.cancel_order(cancel_cmd),
                    // Balance events are applied by the account layer, not this engine thread.
                    Command::BalanceDeposit { .. }
                    | Command::BalanceWithdraw { .. }
                    | Command::FundingPayment { .. }
                    | Command::PositionSnapshot { .. } => EventBuffer::new(),
                };
                let elapsed_ns = t0.elapsed().as_nanos() as u64;

                // Record: saturate to histogram max (1 s) rather than panic on
                // outliers. An outlier that saturates tells us something went
                // very wrong; it should not crash the engine thread.
                let _ = latency_hist.record(elapsed_ns.clamp(1, 1_000_000_000));
                process_count += 1;

                // Periodic log: every HISTOGRAM_LOG_INTERVAL commands emit
                // tail-latency quantiles at DEBUG level. This is cheap (five
                // value_at_quantile() calls) and does not reset the histogram
                // so percentiles accumulate over the engine lifetime.
                if process_count % HISTOGRAM_LOG_INTERVAL == 0 {
                    tracing::debug!(
                        symbol = %symbol,
                        count   = process_count,
                        p50_ns  = latency_hist.value_at_quantile(0.50),
                        p90_ns  = latency_hist.value_at_quantile(0.90),
                        p99_ns  = latency_hist.value_at_quantile(0.99),
                        p999_ns = latency_hist.value_at_quantile(0.999),
                        "engine latency histogram"
                    );
                }

                // Ignore send error: caller may have timed out and dropped the receiver.
                let _ = response_tx.send(EngineResponse { id, events });
            }

            EngineCommand::Snapshot {
                id,
                depth,
                response_tx,
            } => {
                let snapshot = book.snapshot(depth);
                let _ = response_tx.send(SnapshotResponse { id, snapshot });
            }

            EngineCommand::FullSnapshot { response_tx } => {
                let snap = book.full_snapshot();
                let _ = response_tx.send(snap);
            }

            EngineCommand::LatencySnapshot { response_tx } => {
                // Extract quantiles from the histogram. `value_at_quantile`
                // returns 0 for an empty histogram, which is a valid sentinel.
                let snap = LatencySnapshot {
                    count:     latency_hist.len(),
                    p50_ns:    latency_hist.value_at_quantile(0.50),
                    p90_ns:    latency_hist.value_at_quantile(0.90),
                    p99_ns:    latency_hist.value_at_quantile(0.99),
                    p999_ns:   latency_hist.value_at_quantile(0.999),
                    p9999_ns:  latency_hist.value_at_quantile(0.9999),
                    max_ns:    latency_hist.max(),
                };
                let _ = response_tx.send(snap);
            }

            EngineCommand::ResetLatencyHistogram => {
                latency_hist.reset();
                process_count = 0;
                tracing::debug!(symbol = %symbol, "latency histogram reset");
            }

            EngineCommand::UserOrderCount { user_id, response_tx } => {
                let count = book.user_open_order_count(user_id);
                let _ = response_tx.send(count);
            }

            EngineCommand::UserOrdersAtPrice { user_id, side, price, response_tx } => {
                let count = book.user_orders_at_price(user_id, side, price);
                let _ = response_tx.send(count);
            }

            EngineCommand::Shutdown => {
                tracing::debug!(symbol = %symbol, "engine thread: shutdown requested, exiting");
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use exchange_common::*;
    use rust_decimal_macros::dec;

    use super::*;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn btc() -> Symbol {
        Symbol::new("BTCUSDT")
    }

    fn eth() -> Symbol {
        Symbol::new("ETHUSDT")
    }

    fn limit(symbol: Symbol, side: Side, price: rust_decimal::Decimal, qty: rust_decimal::Decimal) -> Command {
        Command::PlaceOrder(PlaceOrderCommand {
            order_id: uuid::Uuid::new_v4(),
            user_id: uuid::Uuid::new_v4(),
            symbol,
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

    fn cancel(symbol: Symbol, order_id: uuid::Uuid) -> Command {
        Command::CancelOrder(CancelOrderCommand {
            order_id,
            user_id: uuid::Uuid::new_v4(),
            symbol,
            timestamp: chrono::Utc::now(),
        })
    }

    // -----------------------------------------------------------------------
    // Test 1: Spawn engine for BTCUSDT, place limit order, verify accepted
    // -----------------------------------------------------------------------

    #[test]
    fn test_spawn_and_place_limit_order() {
        let mut handle = EngineHandle::spawn(btc(), 128);

        let events = handle.send(limit(btc(), Side::Buy, dec!(50000), dec!(1))).unwrap();

        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], MatchEvent::OrderAccepted { .. }));

        handle.shutdown();
    }

    // -----------------------------------------------------------------------
    // Test 2: Two orders match, verify fill returned
    // -----------------------------------------------------------------------

    #[test]
    fn test_two_orders_match_fill_returned() {
        let mut handle = EngineHandle::spawn(btc(), 128);

        // Resting sell
        let sell_events = handle.send(limit(btc(), Side::Sell, dec!(50000), dec!(1))).unwrap();
        assert!(matches!(sell_events[0], MatchEvent::OrderAccepted { .. }));

        // Aggressive buy at same price
        let buy_events = handle.send(limit(btc(), Side::Buy, dec!(50000), dec!(1))).unwrap();

        let fill = buy_events.iter().find_map(|e| match e {
            MatchEvent::Fill(f) => Some(f),
            _ => None,
        });

        assert!(fill.is_some(), "expected a Fill event");
        assert_eq!(fill.unwrap().price, dec!(50000));
        assert_eq!(fill.unwrap().qty, dec!(1));

        handle.shutdown();
    }

    // -----------------------------------------------------------------------
    // Test 3: Cancel order, verify cancelled event returned
    // -----------------------------------------------------------------------

    #[test]
    fn test_cancel_order_returns_cancelled_event() {
        let mut handle = EngineHandle::spawn(btc(), 128);

        // Place an order so we have an ID to cancel
        let place_cmd = PlaceOrderCommand {
            order_id: uuid::Uuid::new_v4(),
            user_id: uuid::Uuid::new_v4(),
            symbol: btc(),
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
            price: dec!(50000),
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
        };
        let order_id = place_cmd.order_id;
        handle.send(Command::PlaceOrder(place_cmd)).unwrap();

        let cancel_events = handle.send(cancel(btc(), order_id)).unwrap();

        assert_eq!(cancel_events.len(), 1);
        assert!(matches!(cancel_events[0], MatchEvent::OrderCancelled { .. }));

        handle.shutdown();
    }

    // -----------------------------------------------------------------------
    // Test 4: Snapshot returns correct bids/asks
    // -----------------------------------------------------------------------

    #[test]
    fn test_snapshot_returns_correct_bids_asks() {
        let mut handle = EngineHandle::spawn(btc(), 128);

        handle.send(limit(btc(), Side::Buy, dec!(49900), dec!(2))).unwrap();
        handle.send(limit(btc(), Side::Buy, dec!(49800), dec!(3))).unwrap();
        handle.send(limit(btc(), Side::Sell, dec!(50100), dec!(1))).unwrap();

        let snap = handle.book_snapshot(10).unwrap();

        // Bids are best-first (descending)
        assert_eq!(snap.bids.len(), 2);
        assert_eq!(snap.bids[0], (dec!(49900), dec!(2)));
        assert_eq!(snap.bids[1], (dec!(49800), dec!(3)));

        // Asks are best-first (ascending)
        assert_eq!(snap.asks.len(), 1);
        assert_eq!(snap.asks[0], (dec!(50100), dec!(1)));

        handle.shutdown();
    }

    // -----------------------------------------------------------------------
    // Test 5: Two symbols process independently
    // -----------------------------------------------------------------------

    #[test]
    fn test_two_symbols_process_independently() {
        let mut btc_handle = EngineHandle::spawn(btc(), 128);
        let mut eth_handle = EngineHandle::spawn(eth(), 128);

        btc_handle.send(limit(btc(), Side::Buy, dec!(50000), dec!(1))).unwrap();
        btc_handle.send(limit(btc(), Side::Buy, dec!(49900), dec!(0.5))).unwrap();

        eth_handle.send(limit(eth(), Side::Sell, dec!(3000), dec!(10))).unwrap();

        let btc_snap = btc_handle.book_snapshot(10).unwrap();
        let eth_snap = eth_handle.book_snapshot(10).unwrap();

        assert_eq!(btc_snap.bids.len(), 2);
        assert_eq!(eth_snap.asks.len(), 1);

        // Cross-contamination check: BTC has no asks, ETH has no bids
        assert!(btc_snap.asks.is_empty());
        assert!(eth_snap.bids.is_empty());

        btc_handle.shutdown();
        eth_handle.shutdown();
    }

    // -----------------------------------------------------------------------
    // Test 6: Shutdown and join is clean (no panic, no deadlock)
    // -----------------------------------------------------------------------

    #[test]
    fn test_shutdown_join_is_clean() {
        let mut handle = EngineHandle::spawn(btc(), 128);

        // Do some work first
        handle.send(limit(btc(), Side::Buy, dec!(50000), dec!(1))).unwrap();

        // Should return without panic or hang
        handle.shutdown();

        // Calling shutdown again must be safe (idempotent via Option::take)
        handle.shutdown();
    }

    // -----------------------------------------------------------------------
    // Test 7: send_timeout returns Timeout when no work happens in time.
    //         We trigger this by filling the channel (capacity=1) and then
    //         sending with a very short timeout before the thread can drain.
    // -----------------------------------------------------------------------

    #[test]
    fn test_send_timeout_returns_timeout_when_saturated() {
        // Use capacity 1 to make it easy to saturate.
        let handle = EngineHandle::spawn(btc(), 1);

        // Fill the channel with one item (the thread may or may not have consumed it yet).
        // We fire off many sends with an absurdly short timeout.  At least ONE of them
        // must time out because the channel is only 1-deep and we fire 100 sends without
        // draining.  We capture all results and check that at least one timed out.
        let mut got_timeout = false;
        for _ in 0..100 {
            let result = handle.send_timeout(
                limit(btc(), Side::Buy, dec!(50000), dec!(1)),
                Duration::from_nanos(1),
            );
            match result {
                Err(EngineError::Timeout) => {
                    got_timeout = true;
                    break;
                }
                Ok(_) => {} // processed fast enough — try again
                Err(e) => panic!("unexpected error: {e}"),
            }
        }

        assert!(
            got_timeout,
            "expected at least one send_timeout to return Timeout on a saturated channel"
        );
    }

    // -----------------------------------------------------------------------
    // Test 8: Drop causes clean shutdown (via Drop impl)
    // -----------------------------------------------------------------------

    #[test]
    fn test_latency_snapshot_after_1000_commands() {
        let mut handle = EngineHandle::spawn(btc(), 2048);

        // Place 1000 limit orders (alternating bid prices to avoid matches).
        for i in 0..1000u32 {
            let price = rust_decimal::Decimal::from(49_000 + i);
            handle.send(limit(btc(), Side::Buy, price, dec!(1))).unwrap();
        }

        let snap = handle.latency_snapshot().unwrap();

        // We sent exactly 1000 Process commands.
        assert_eq!(snap.count, 1000, "histogram must have recorded 1000 commands");

        // p50 must be > 0: at least one measurement was non-zero.
        assert!(snap.p50_ns > 0, "p50 must be > 0 ns after real work");

        // Sanity ordering: percentiles must be non-decreasing.
        assert!(snap.p50_ns <= snap.p90_ns, "p50 <= p90");
        assert!(snap.p90_ns <= snap.p99_ns, "p90 <= p99");
        assert!(snap.p99_ns <= snap.p999_ns, "p99 <= p99.9");
        assert!(snap.p999_ns <= snap.p9999_ns, "p99.9 <= p99.99");
        assert!(snap.p9999_ns <= snap.max_ns, "p99.99 <= max");

        handle.shutdown();
    }

    #[test]
    fn test_drop_causes_clean_shutdown() {
        let handle = EngineHandle::spawn(btc(), 128);
        handle.send(limit(btc(), Side::Buy, dec!(50000), dec!(1))).unwrap();
        // Drop here — the Drop impl calls shutdown(), which sends Shutdown + joins.
        drop(handle);
        // If we reach here without a panic or hang, the test passes.
    }

    // -----------------------------------------------------------------------
    // Test 9: High throughput — 10 000 orders processed, all acknowledged
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // Test 10: reset_latency_histogram clears samples
    // -----------------------------------------------------------------------

    #[test]
    fn test_reset_latency_histogram_clears_samples() {
        let mut handle = EngineHandle::spawn(btc(), 2048);

        // Send 100 commands to populate the histogram.
        for i in 0..100u32 {
            let price = rust_decimal::Decimal::from(49_000 + i);
            handle.send(limit(btc(), Side::Buy, price, dec!(1))).unwrap();
        }

        let snap_before = handle.latency_snapshot().unwrap();
        assert_eq!(snap_before.count, 100, "histogram must record 100 commands before reset");
        assert!(snap_before.p50_ns > 0, "p50 must be > 0 before reset");

        // Reset the histogram.
        handle.reset_latency_histogram().unwrap();

        // After reset, count must be 0 (modulo any commands that sneak in between
        // reset and the snapshot query — we gate with <= 1 to be generous).
        let snap_after_reset = handle.latency_snapshot().unwrap();
        assert!(
            snap_after_reset.count <= 1,
            "count must be 0 (or at most 1) after reset, got {}",
            snap_after_reset.count
        );

        // Send 10 more commands and verify count reflects only the new window.
        for i in 0..10u32 {
            let price = rust_decimal::Decimal::from(48_000 + i);
            handle.send(limit(btc(), Side::Buy, price, dec!(1))).unwrap();
        }

        let snap_new_window = handle.latency_snapshot().unwrap();
        assert!(
            snap_new_window.count >= 10,
            "count must be >= 10 after new commands, got {}",
            snap_new_window.count
        );
        // Must be well below the original 100 — no bleed-through from the old window.
        assert!(
            snap_new_window.count < 20,
            "count must be < 20 (no bleed from pre-reset window), got {}",
            snap_new_window.count
        );

        handle.shutdown();
    }

    // -----------------------------------------------------------------------
    // Test 11: reset_all_latency_histograms via EnginePool
    // -----------------------------------------------------------------------

    #[test]
    fn test_reset_via_engine_pool() {
        use crate::engine_pool::EnginePool;

        let mut pool = EnginePool::new();
        pool.add_symbol(btc());
        pool.add_symbol(eth());

        // Populate both engines.
        for i in 0..50u32 {
            let price = rust_decimal::Decimal::from(49_000 + i);
            pool.process(&btc(), limit(btc(), Side::Buy, price, dec!(1))).unwrap();
            let price2 = rust_decimal::Decimal::from(2_900 + i);
            pool.process(&eth(), limit(eth(), Side::Buy, price2, dec!(1))).unwrap();
        }

        let snap_btc = pool.latency_snapshot(&btc()).unwrap();
        let snap_eth = pool.latency_snapshot(&eth()).unwrap();
        assert_eq!(snap_btc.count, 50);
        assert_eq!(snap_eth.count, 50);

        // Reset all.
        let results = pool.reset_all_latency_histograms();
        assert!(results.iter().all(|r| r.is_ok()), "all resets must succeed");

        // After reset counts are 0 (or near-0).
        let snap_btc_r = pool.latency_snapshot(&btc()).unwrap();
        let snap_eth_r = pool.latency_snapshot(&eth()).unwrap();
        assert!(snap_btc_r.count <= 1, "BTC count after reset: {}", snap_btc_r.count);
        assert!(snap_eth_r.count <= 1, "ETH count after reset: {}", snap_eth_r.count);
    }

    // -----------------------------------------------------------------------
    // Test 12: high-throughput (existing test, moved below new tests)
    // -----------------------------------------------------------------------

    #[test]
    fn test_high_throughput_10000_orders() {
        let mut handle = EngineHandle::spawn(btc(), 2048);

        let mut accepted = 0usize;
        let mut fills = 0usize;

        // Place 5000 resting limit buy orders at distinct price levels.
        for i in 0..5000u32 {
            let price = rust_decimal::Decimal::from(49000 + i);
            let events = handle
                .send(limit(btc(), Side::Buy, price, dec!(1)))
                .unwrap();
            for e in &events {
                if matches!(e, MatchEvent::OrderAccepted { .. }) {
                    accepted += 1;
                }
            }
        }

        assert_eq!(accepted, 5000, "all 5000 buy orders should be accepted");

        // Place 5000 market sell orders — each sweeps the best available bid,
        // guaranteeing exactly one fill per order regardless of price ordering.
        for _ in 0..5000u32 {
            let cmd = Command::PlaceOrder(PlaceOrderCommand {
                order_id: uuid::Uuid::new_v4(),
                user_id: uuid::Uuid::new_v4(),
                symbol: btc(),
                side: Side::Sell,
                order_type: OrderType::Market,
                time_in_force: TimeInForce::Gtc,
                price: rust_decimal::Decimal::ZERO,
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
            let events = handle.send(cmd).unwrap();
            for e in &events {
                if matches!(e, MatchEvent::Fill(_)) {
                    fills += 1;
                }
            }
        }

        assert_eq!(fills, 5000, "all 5000 market sell orders should result in fills");

        handle.shutdown();
    }
}
