//! In-process implementation of [`MarketDataProvider`].
//!
//! `InProcessMarketData` wraps a `Mutex<MarketDataService>` so that callers
//! can hold a shared `Arc<dyn MarketDataProvider>` without needing to manage
//! the lock themselves.

use std::sync::Mutex;

use chrono::{DateTime, Utc};
use exchange_common::types::{Side, Symbol};
use rust_decimal::Decimal;

use crate::{Candle, CandleInterval, MarketDataProvider, MarketDataService, Ticker24h};

/// Concrete in-process implementation that wraps `MarketDataService` behind a
/// `Mutex`.  Every method acquires and releases the lock atomically.
pub struct InProcessMarketData {
    inner: Mutex<MarketDataService>,
}

impl InProcessMarketData {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(MarketDataService::new()),
        }
    }
}

impl Default for InProcessMarketData {
    fn default() -> Self {
        Self::new()
    }
}

impl MarketDataProvider for InProcessMarketData {
    fn on_trade(
        &self,
        symbol: &Symbol,
        price: Decimal,
        qty: Decimal,
        taker_side: Side,
        ts: DateTime<Utc>,
    ) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .on_trade(symbol, price, qty, taker_side, ts);
    }

    fn update_book(
        &self,
        symbol: &Symbol,
        best_bid: Option<Decimal>,
        best_bid_size: Option<Decimal>,
        best_ask: Option<Decimal>,
        best_ask_size: Option<Decimal>,
    ) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .update_book(symbol, best_bid, best_bid_size, best_ask, best_ask_size);
    }

    fn update_futures_data(
        &self,
        symbol: &Symbol,
        mark_price: Decimal,
        index_price: Decimal,
        open_interest: Decimal,
        open_interest_value: Decimal,
        funding_rate: Decimal,
        next_funding_time: i64,
    ) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .update_futures_data(
                symbol,
                mark_price,
                index_price,
                open_interest,
                open_interest_value,
                funding_rate,
                next_funding_time,
            );
    }

    fn candles(&self, symbol: &Symbol, interval: CandleInterval, limit: usize) -> Vec<Candle> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .candles(symbol, interval, limit)
            .into_iter()
            .cloned()
            .collect()
    }

    fn current_candle(&self, symbol: &Symbol, interval: CandleInterval) -> Option<Candle> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .current_candle(symbol, interval)
            .cloned()
    }

    fn ticker(&self, symbol: &Symbol) -> Option<Ticker24h> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .ticker(symbol)
    }

    fn all_tickers(&self) -> Vec<Ticker24h> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .all_tickers()
    }
}
