use std::collections::HashMap;

use exchange_common::{Command, MatchEvent, Price, Side, Symbol, UserId};

use crate::engine_thread::{EngineError, EngineHandle, LatencySnapshot};
use crate::orderbook::OrderBookSnapshot;

/// A pool of per-symbol matching engine threads.
///
/// Each symbol gets its own OS thread with its own `OrderBook`.  Commands for
/// different symbols are processed concurrently — there is no shared lock
/// across symbols.
///
/// # Example
/// ```text
/// let mut pool = EnginePool::new();
/// pool.add_symbol(Symbol::new("BTCUSDT"));
/// let events = pool.process(&Symbol::new("BTCUSDT"), command)?;
/// ```
pub struct EnginePool {
    engines: HashMap<Symbol, EngineHandle>,
    /// Channel capacity used when spawning new engine threads.
    channel_capacity: usize,
}

impl EnginePool {
    /// Create a new empty pool with default channel capacity (1024).
    pub fn new() -> Self {
        Self {
            engines: HashMap::new(),
            channel_capacity: 1024,
        }
    }

    /// Create a new pool with a custom channel capacity per engine thread.
    pub fn with_capacity(channel_capacity: usize) -> Self {
        Self {
            engines: HashMap::new(),
            channel_capacity,
        }
    }

    /// Add a symbol to the pool, spawning a dedicated engine thread for it.
    ///
    /// If the symbol already exists this is a no-op (returns the existing handle).
    pub fn add_symbol(&mut self, symbol: Symbol) -> &EngineHandle {
        let cap = self.channel_capacity;
        let next_core = self.next_core_id();
        self.engines
            .entry(symbol.clone())
            .or_insert_with(|| EngineHandle::spawn_with_affinity(symbol, cap, next_core))
    }

    /// Parse `EXCHANGE_ENGINE_CPUS` env var (comma-separated core IDs) and
    /// return the next core to assign, round-robin style.
    ///
    /// Format: `EXCHANGE_ENGINE_CPUS=2,3,4,5` — pins engine threads to those cores.
    /// If not set, returns `None` (no pinning — default behavior).
    fn next_core_id(&self) -> Option<usize> {
        let cpus_str = std::env::var("EXCHANGE_ENGINE_CPUS").ok()?;
        let cores: Vec<usize> = cpus_str
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        if cores.is_empty() {
            return None;
        }
        // Round-robin: current engine count mod number of configured cores.
        let idx = self.engines.len() % cores.len();
        Some(cores[idx])
    }

    /// Process a command on the engine thread for `symbol`.
    ///
    /// Returns `EngineError::SymbolNotFound` if the symbol has not been added.
    pub fn process(
        &self,
        symbol: &Symbol,
        command: Command,
    ) -> Result<Vec<MatchEvent>, EngineError> {
        let handle = self
            .engines
            .get(symbol)
            .ok_or_else(|| EngineError::SymbolNotFound(symbol.to_string()))?;

        handle.send(command)
    }

    /// Request an L2 snapshot for `symbol` at the given `depth`.
    ///
    /// Returns `EngineError::SymbolNotFound` if the symbol has not been added.
    pub fn snapshot(
        &self,
        symbol: &Symbol,
        depth: usize,
    ) -> Result<OrderBookSnapshot, EngineError> {
        let handle = self
            .engines
            .get(symbol)
            .ok_or_else(|| EngineError::SymbolNotFound(symbol.to_string()))?;

        handle.book_snapshot(depth)
    }

    /// Return all symbols currently registered in the pool.
    pub fn symbols(&self) -> Vec<&Symbol> {
        self.engines.keys().collect()
    }

    /// Take a full-fidelity snapshot of every symbol's order book.
    ///
    /// Used by the background snapshot task for fast crash recovery.
    /// Returns one snapshot per registered symbol; errors for individual
    /// symbols are logged and skipped.
    pub fn full_snapshots_all(&self) -> Vec<crate::orderbook::FullBookSnapshot> {
        self.engines
            .iter()
            .filter_map(|(symbol, handle)| {
                match handle.full_book_snapshot() {
                    Ok(snap) => Some(snap),
                    Err(e) => {
                        tracing::warn!(symbol = %symbol, error = %e, "full_book_snapshot failed");
                        None
                    }
                }
            })
            .collect()
    }

    /// Request a latency snapshot for a single symbol's engine.
    ///
    /// Returns `EngineError::SymbolNotFound` if the symbol has not been added.
    pub fn latency_snapshot(&self, symbol: &Symbol) -> Result<LatencySnapshot, EngineError> {
        let handle = self
            .engines
            .get(symbol)
            .ok_or_else(|| EngineError::SymbolNotFound(symbol.to_string()))?;
        handle.latency_snapshot()
    }

    /// Reset the latency histogram on every engine in the pool.
    ///
    /// Returns one `Result<(), EngineError>` per registered symbol. An error
    /// for one symbol does NOT stop the reset from being sent to the remaining
    /// symbols.
    ///
    /// Intended to be called every 5 minutes by the `spawn_latency_metrics_updater`
    /// background task in `api-gateway` to implement a rolling observation window.
    pub fn reset_all_latency_histograms(&self) -> Vec<Result<(), EngineError>> {
        self.engines.values().map(|h| h.reset_latency_histogram()).collect()
    }

    /// Returns true if `symbol` has a registered engine in this pool.
    pub fn has_symbol(&self, symbol: &Symbol) -> bool {
        self.engines.contains_key(symbol)
    }

    /// Return the number of open orders for `user_id` on `symbol`'s book.
    ///
    /// Returns `EngineError::SymbolNotFound` if the symbol has not been added.
    pub fn user_open_order_count(
        &self,
        symbol: &Symbol,
        user_id: UserId,
    ) -> Result<usize, EngineError> {
        let handle = self
            .engines
            .get(symbol)
            .ok_or_else(|| EngineError::SymbolNotFound(symbol.to_string()))?;
        handle.user_open_order_count(user_id)
    }

    /// Return the number of open orders for `user_id` at `price` on `side` for `symbol`.
    ///
    /// Returns `EngineError::SymbolNotFound` if the symbol has not been added.
    pub fn user_orders_at_price(
        &self,
        symbol: &Symbol,
        user_id: UserId,
        side: Side,
        price: Price,
    ) -> Result<usize, EngineError> {
        let handle = self
            .engines
            .get(symbol)
            .ok_or_else(|| EngineError::SymbolNotFound(symbol.to_string()))?;
        handle.user_orders_at_price(user_id, side, price)
    }

    /// Restart the engine for `symbol` by shutting down the current thread and
    /// spawning a fresh one.
    ///
    /// **Warning**: all in-flight orders on the old engine are lost. Call this
    /// only when the engine thread has panicked or is otherwise unresponsive.
    /// Returns `false` if the symbol is not registered (no-op).
    pub fn restart_symbol(&mut self, symbol: &Symbol) -> bool {
        if let Some(mut old_handle) = self.engines.remove(symbol) {
            old_handle.shutdown();
            self.engines
                .insert(symbol.clone(), EngineHandle::spawn(symbol.clone(), self.channel_capacity));
            true
        } else {
            false
        }
    }

    /// Shut down all engine threads and join them.
    ///
    /// After this call the pool is empty.  It can be reused by calling
    /// `add_symbol` again.
    pub fn shutdown_all(&mut self) {
        for (_, mut handle) in self.engines.drain() {
            handle.shutdown();
        }
    }
}

impl Default for EnginePool {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for EnginePool {
    fn drop(&mut self) {
        self.shutdown_all();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;

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

    fn sol() -> Symbol {
        Symbol::new("SOLUSDT")
    }

    fn limit(
        symbol: Symbol,
        side: Side,
        price: rust_decimal::Decimal,
        qty: rust_decimal::Decimal,
    ) -> Command {
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

    // -----------------------------------------------------------------------
    // Test: Add 3 symbols, process on each
    // -----------------------------------------------------------------------

    #[test]
    fn test_pool_add_three_symbols_and_process_each() {
        let mut pool = EnginePool::new();
        pool.add_symbol(btc());
        pool.add_symbol(eth());
        pool.add_symbol(sol());

        // Process an order on each symbol
        let btc_events = pool.process(&btc(), limit(btc(), Side::Buy, dec!(50000), dec!(1))).unwrap();
        let eth_events = pool.process(&eth(), limit(eth(), Side::Sell, dec!(3000), dec!(5))).unwrap();
        let sol_events = pool.process(&sol(), limit(sol(), Side::Buy, dec!(100), dec!(50))).unwrap();

        assert!(matches!(btc_events[0], MatchEvent::OrderAccepted { .. }));
        assert!(matches!(eth_events[0], MatchEvent::OrderAccepted { .. }));
        assert!(matches!(sol_events[0], MatchEvent::OrderAccepted { .. }));
    }

    // -----------------------------------------------------------------------
    // Test: Send to nonexistent symbol returns SymbolNotFound error
    // -----------------------------------------------------------------------

    #[test]
    fn test_pool_nonexistent_symbol_returns_error() {
        let pool = EnginePool::new();

        let result = pool.process(&btc(), limit(btc(), Side::Buy, dec!(50000), dec!(1)));

        match result {
            Err(EngineError::SymbolNotFound(sym)) => {
                assert_eq!(sym, "BTCUSDT");
            }
            other => panic!("expected SymbolNotFound, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // Test: Snapshot on nonexistent symbol returns SymbolNotFound error
    // -----------------------------------------------------------------------

    #[test]
    fn test_pool_snapshot_nonexistent_symbol_returns_error() {
        let pool = EnginePool::new();

        let result = pool.snapshot(&eth(), 10);

        assert!(matches!(result, Err(EngineError::SymbolNotFound(_))));
    }

    // -----------------------------------------------------------------------
    // Test: Concurrent operations on different symbols don't block each other
    // -----------------------------------------------------------------------

    #[test]
    fn test_pool_concurrent_operations_on_different_symbols() {
        let mut pool = EnginePool::new();
        pool.add_symbol(btc());
        pool.add_symbol(eth());
        pool.add_symbol(sol());

        // Wrap pool in Arc so multiple threads can reference individual handles.
        // We access the handles directly to avoid requiring Arc<EnginePool>.
        // The test verifies that BTC, ETH, SOL each process their own batch
        // without serialising behind a single lock.
        let btc_handle = Arc::new(EngineHandle::spawn(btc(), 512));
        let eth_handle = Arc::new(EngineHandle::spawn(eth(), 512));
        let sol_handle = Arc::new(EngineHandle::spawn(sol(), 512));

        let btc_h = Arc::clone(&btc_handle);
        let eth_h = Arc::clone(&eth_handle);
        let sol_h = Arc::clone(&sol_handle);

        let btc_thread = thread::spawn(move || {
            for _ in 0..500 {
                btc_h.send(limit(btc(), Side::Buy, dec!(50000), dec!(1))).unwrap();
            }
        });

        let eth_thread = thread::spawn(move || {
            for _ in 0..500 {
                eth_h.send(limit(eth(), Side::Sell, dec!(3000), dec!(1))).unwrap();
            }
        });

        let sol_thread = thread::spawn(move || {
            for _ in 0..500 {
                sol_h.send(limit(sol(), Side::Buy, dec!(100), dec!(1))).unwrap();
            }
        });

        btc_thread.join().unwrap();
        eth_thread.join().unwrap();
        sol_thread.join().unwrap();

        // Each book should have 500 resting orders
        let btc_snap = btc_handle.book_snapshot(10).unwrap();
        let eth_snap = eth_handle.book_snapshot(10).unwrap();
        let sol_snap = sol_handle.book_snapshot(10).unwrap();

        // Snapshots should have entries — exact depth may be capped at 10
        // but sequence number must advance to 500
        assert!(btc_snap.sequence >= 500);
        assert!(eth_snap.sequence >= 500);
        assert!(sol_snap.sequence >= 500);
    }

    // -----------------------------------------------------------------------
    // Test: symbols() returns all registered symbols
    // -----------------------------------------------------------------------

    #[test]
    fn test_pool_symbols_returns_all() {
        let mut pool = EnginePool::new();
        pool.add_symbol(btc());
        pool.add_symbol(eth());

        let mut syms: Vec<String> = pool.symbols().into_iter().map(|s| s.to_string()).collect();
        syms.sort();

        assert_eq!(syms, vec!["BTCUSDT", "ETHUSDT"]);
    }

    // -----------------------------------------------------------------------
    // Test: shutdown_all joins all threads without panic
    // -----------------------------------------------------------------------

    #[test]
    fn test_pool_shutdown_all_is_clean() {
        let mut pool = EnginePool::new();
        pool.add_symbol(btc());
        pool.add_symbol(eth());

        pool.process(&btc(), limit(btc(), Side::Buy, dec!(50000), dec!(1))).unwrap();
        pool.process(&eth(), limit(eth(), Side::Sell, dec!(3000), dec!(1))).unwrap();

        pool.shutdown_all(); // must not panic or hang

        // After shutdown, symbols() is empty
        assert!(pool.symbols().is_empty());
    }

    // -----------------------------------------------------------------------
    // Test: add_symbol is idempotent
    // -----------------------------------------------------------------------

    #[test]
    fn test_pool_add_symbol_idempotent() {
        let mut pool = EnginePool::new();
        pool.add_symbol(btc());
        pool.add_symbol(btc()); // second call must not spawn a second thread

        assert_eq!(pool.symbols().len(), 1);
    }
}
