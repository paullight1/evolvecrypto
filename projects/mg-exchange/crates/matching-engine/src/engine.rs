use std::collections::HashMap;

use exchange_common::*;

use crate::orderbook::{EventBuffer, OrderBook};

/// The matching engine — manages order books for all symbols.
///
/// Single-threaded by design. All commands are processed sequentially.
/// This eliminates all locking, all race conditions, all data races.
/// Throughput scales by sharding symbols across engine instances.
pub struct MatchingEngine {
    books: HashMap<Symbol, OrderBook>,
}

impl MatchingEngine {
    pub fn new() -> Self {
        Self {
            books: HashMap::new(),
        }
    }

    /// Register a new trading pair.
    pub fn add_symbol(&mut self, symbol: Symbol) {
        self.books
            .entry(symbol.clone())
            .or_insert_with(|| OrderBook::new(symbol));
    }

    /// Process a command and return resulting events.
    ///
    /// Returns `EventBuffer` (SmallVec<[MatchEvent; 4]>) — zero heap allocation
    /// for the typical 1–3-event response. The caller converts to `Vec<MatchEvent>`
    /// at the channel boundary if needed (see `engine_thread.rs`).
    pub fn process(&mut self, command: Command) -> EventBuffer {
        match command {
            Command::PlaceOrder(cmd) => {
                let symbol = cmd.symbol.clone();
                if let Some(book) = self.books.get_mut(&symbol) {
                    book.place_order(cmd)
                } else {
                    smallvec::smallvec![MatchEvent::OrderRejected {
                        order_id: cmd.order_id,
                        symbol,
                        reason: std::borrow::Cow::Borrowed("Symbol not found"),
                        seq: 0,
                    }]
                }
            }
            Command::CancelOrder(cmd) => {
                let symbol = cmd.symbol.clone();
                if let Some(book) = self.books.get_mut(&symbol) {
                    book.cancel_order(cmd)
                } else {
                    EventBuffer::new() // Silent ignore for unknown symbol
                }
            }
            // Balance events (deposit, withdrawal, funding) are WAL entries that are
            // applied by the account manager layer, not the matching engine.
            // Silently ignore them so the engine stays stateless w.r.t. balances.
            Command::BalanceDeposit { .. }
            | Command::BalanceWithdraw { .. }
            | Command::FundingPayment { .. }
            | Command::PositionSnapshot { .. } => EventBuffer::new(),
        }
    }

    /// Get the order book for a symbol.
    pub fn book(&self, symbol: &Symbol) -> Option<&OrderBook> {
        self.books.get(symbol)
    }

    /// Get mutable order book for a symbol.
    pub fn book_mut(&mut self, symbol: &Symbol) -> Option<&mut OrderBook> {
        self.books.get_mut(symbol)
    }

    /// Get all registered symbols.
    pub fn symbols(&self) -> Vec<&Symbol> {
        self.books.keys().collect()
    }
}

impl Default for MatchingEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_engine_multi_symbol() {
        let mut engine = MatchingEngine::new();
        engine.add_symbol(Symbol::new("BTCUSDT"));
        engine.add_symbol(Symbol::new("ETHUSDT"));

        // Place order on BTC
        let events = engine.process(Command::PlaceOrder(PlaceOrderCommand {
            order_id: uuid::Uuid::new_v4(),
            user_id: uuid::Uuid::new_v4(),
            symbol: Symbol::new("BTCUSDT"),
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
        }));

        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], MatchEvent::OrderAccepted { .. }));

        // BTC book has 1 order, ETH book has 0
        assert_eq!(engine.book(&Symbol::new("BTCUSDT")).unwrap().order_count(), 1);
        assert_eq!(engine.book(&Symbol::new("ETHUSDT")).unwrap().order_count(), 0);
    }

    #[test]
    fn test_unknown_symbol_rejected() {
        let mut engine = MatchingEngine::new();

        let events = engine.process(Command::PlaceOrder(PlaceOrderCommand {
            order_id: uuid::Uuid::new_v4(),
            user_id: uuid::Uuid::new_v4(),
            symbol: Symbol::new("XYZUSDT"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
            price: dec!(100),
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

        assert!(matches!(events[0], MatchEvent::OrderRejected { .. }));
    }
}
