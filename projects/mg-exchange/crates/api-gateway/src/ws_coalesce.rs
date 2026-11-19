use std::collections::HashMap;

use crate::exchange::MarketEvent;

/// Accumulated order-book delta for one symbol within a flush window.
///
/// When multiple book updates arrive for the same symbol in a window, only
/// the latest quantity for each price level is kept. A quantity of `"0"`
/// signals that the level should be removed from the client's local book.
#[derive(Debug, Clone)]
pub struct BookDelta {
    pub symbol: String,
    /// price → qty  ("0" means the level was removed)
    pub bids: HashMap<String, String>,
    /// price → qty  ("0" means the level was removed)
    pub asks: HashMap<String, String>,
    /// Highest sequence number seen across all merged updates.
    pub sequence: u64,
}

impl BookDelta {
    fn new(symbol: String) -> Self {
        Self {
            symbol,
            bids: HashMap::new(),
            asks: HashMap::new(),
            sequence: 0,
        }
    }

    /// Merge a book snapshot into this delta.
    ///
    /// For each price level the *latest* quantity wins. If `qty` is zero the
    /// level is recorded as `"0"` so the client knows to remove it.
    fn merge(&mut self, bids: &[(rust_decimal::Decimal, rust_decimal::Decimal)], asks: &[(rust_decimal::Decimal, rust_decimal::Decimal)], sequence: u64) {
        for (price, qty) in bids {
            self.bids.insert(price.to_string(), qty.to_string());
        }
        for (price, qty) in asks {
            self.asks.insert(price.to_string(), qty.to_string());
        }
        if sequence > self.sequence {
            self.sequence = sequence;
        }
    }

    /// Convert back to a `MarketEvent::BookUpdate` for broadcasting.
    fn into_market_event(self) -> MarketEvent {
        use rust_decimal::Decimal;
        use std::str::FromStr;

        let bids: Vec<(Decimal, Decimal)> = self
            .bids
            .into_iter()
            .filter_map(|(p, q)| {
                let price = Decimal::from_str(&p).ok()?;
                let qty = Decimal::from_str(&q).ok()?;
                Some((price, qty))
            })
            .collect();

        let asks: Vec<(Decimal, Decimal)> = self
            .asks
            .into_iter()
            .filter_map(|(p, q)| {
                let price = Decimal::from_str(&p).ok()?;
                let qty = Decimal::from_str(&q).ok()?;
                Some((price, qty))
            })
            .collect();

        MarketEvent::BookUpdate {
            symbol: self.symbol,
            bids,
            asks,
            sequence: self.sequence,
        }
    }
}

/// Coalesces market data updates within a time window.
///
/// Instead of forwarding every individual book change to subscribers, the
/// coalescer accumulates all changes in a configurable window (default 50ms)
/// and emits a single combined delta per symbol on flush.
///
/// Trades and other non-book events pass through unchanged because they carry
/// discrete economic information (price discovery, fills) that must not be
/// dropped or merged.
///
/// # Usage
///
/// ```text
/// // Typical integration in the WS fanout path:
/// let mut coalescer = MessageCoalescer::new(50);
/// coalescer.push(event);
/// // … after the window expires …
/// let batch = coalescer.flush();
/// for event in batch { /* send to subscriber */ }
/// ```
pub struct MessageCoalescer {
    /// Pending book updates per symbol (accumulated deltas).
    pending_book: HashMap<String, BookDelta>,
    /// Pending trades / liquidations / funding events (pass-through).
    pending_passthrough: Vec<MarketEvent>,
    /// Flush interval in milliseconds (informational; callers drive the clock).
    pub interval_ms: u64,
}

impl MessageCoalescer {
    /// Create a new coalescer with the given flush interval in milliseconds.
    pub fn new(interval_ms: u64) -> Self {
        Self {
            pending_book: HashMap::new(),
            pending_passthrough: Vec::new(),
            interval_ms,
        }
    }

    /// Accumulate a market event.
    ///
    /// `BookUpdate` events are merged into a per-symbol delta (last-write wins
    /// per price level). All other events are queued for pass-through.
    pub fn push(&mut self, event: MarketEvent) {
        match event {
            MarketEvent::BookUpdate { ref symbol, ref bids, ref asks, sequence } => {
                let delta = self
                    .pending_book
                    .entry(symbol.clone())
                    .or_insert_with(|| BookDelta::new(symbol.clone()));
                delta.merge(bids, asks, sequence);
            }
            other => {
                self.pending_passthrough.push(other);
            }
        }
    }

    /// Flush and return all coalesced events, clearing internal buffers.
    ///
    /// Book deltas are emitted first (one per symbol), followed by
    /// pass-through events in arrival order.
    pub fn flush(&mut self) -> Vec<MarketEvent> {
        let mut out: Vec<MarketEvent> = self
            .pending_book
            .drain()
            .map(|(_, delta)| delta.into_market_event())
            .collect();

        out.extend(self.pending_passthrough.drain(..));
        out
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;
    use crate::exchange::MarketEvent;

    fn book_update(symbol: &str, bids: Vec<(rust_decimal::Decimal, rust_decimal::Decimal)>, asks: Vec<(rust_decimal::Decimal, rust_decimal::Decimal)>, sequence: u64) -> MarketEvent {
        MarketEvent::BookUpdate {
            symbol: symbol.to_string(),
            bids,
            asks,
            sequence,
        }
    }

    fn trade(symbol: &str) -> MarketEvent {
        MarketEvent::Trade {
            symbol: symbol.to_string(),
            price: dec!(50000),
            qty: dec!(0.1),
            taker_side: "Buy".to_string(),
            trade_id: 1,
            timestamp: 0,
        }
    }

    #[test]
    fn single_event_passes_through() {
        let mut c = MessageCoalescer::new(50);
        c.push(book_update("BTCUSDT", vec![(dec!(50000), dec!(1))], vec![], 1));
        let out = c.flush();
        assert_eq!(out.len(), 1);
        match &out[0] {
            MarketEvent::BookUpdate { symbol, sequence, .. } => {
                assert_eq!(symbol, "BTCUSDT");
                assert_eq!(*sequence, 1);
            }
            _ => panic!("expected BookUpdate"),
        }
    }

    #[test]
    fn multiple_book_updates_same_level_latest_wins() {
        let mut c = MessageCoalescer::new(50);
        // First update: bid 50000 = 1.0
        c.push(book_update("BTCUSDT", vec![(dec!(50000), dec!(1.0))], vec![], 1));
        // Second update: bid 50000 = 2.5 (should overwrite)
        c.push(book_update("BTCUSDT", vec![(dec!(50000), dec!(2.5))], vec![], 2));
        let out = c.flush();
        // One coalesced event, not two
        assert_eq!(out.len(), 1);
        match &out[0] {
            MarketEvent::BookUpdate { bids, sequence, .. } => {
                assert_eq!(bids.len(), 1);
                assert_eq!(bids[0].1, dec!(2.5), "latest qty should win");
                assert_eq!(*sequence, 2, "highest sequence retained");
            }
            _ => panic!("expected BookUpdate"),
        }
    }

    #[test]
    fn trades_not_coalesced() {
        let mut c = MessageCoalescer::new(50);
        c.push(trade("BTCUSDT"));
        c.push(trade("BTCUSDT"));
        c.push(trade("ETHUSDT"));
        let out = c.flush();
        assert_eq!(out.len(), 3, "all three trades must be emitted");
        for event in &out {
            assert!(matches!(event, MarketEvent::Trade { .. }), "must all be Trade events");
        }
    }

    #[test]
    fn flush_clears_buffers() {
        let mut c = MessageCoalescer::new(50);
        c.push(book_update("BTCUSDT", vec![(dec!(50000), dec!(1))], vec![], 1));
        let first = c.flush();
        assert_eq!(first.len(), 1);
        // Second flush must be empty
        let second = c.flush();
        assert!(second.is_empty(), "buffers must be cleared after flush");
    }

    #[test]
    fn different_symbols_coalesced_independently() {
        let mut c = MessageCoalescer::new(50);
        c.push(book_update("BTCUSDT", vec![(dec!(50000), dec!(1))], vec![], 1));
        c.push(book_update("ETHUSDT", vec![(dec!(3000), dec!(5))], vec![], 1));
        c.push(book_update("BTCUSDT", vec![(dec!(50000), dec!(3))], vec![], 2));
        let out = c.flush();
        // Two symbols → two coalesced events
        assert_eq!(out.len(), 2);
        let btc = out.iter().find(|e| matches!(e, MarketEvent::BookUpdate { symbol, .. } if symbol == "BTCUSDT"));
        match btc {
            Some(MarketEvent::BookUpdate { bids, sequence, .. }) => {
                assert_eq!(bids[0].1, dec!(3), "BTC latest qty");
                assert_eq!(*sequence, 2);
            }
            _ => panic!("expected BTC book update"),
        }
    }
}
