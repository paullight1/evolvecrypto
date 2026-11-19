//! Exchange market-data service.
//!
//! Provides real-time candle aggregation and 24h rolling-window tickers built
//! from a stream of trade events.

pub mod candle;
pub mod ticker;
pub mod provider;

pub use candle::{Candle, CandleAggregator, CandleInterval};
pub use ticker::{Ticker24h, TickerEngine};
pub use provider::InProcessMarketData;

use chrono::{DateTime, Utc};
use exchange_common::types::{Side, Symbol};
use rust_decimal::Decimal;

/// Top-level service that wires together [`CandleAggregator`] and [`TickerEngine`].
///
/// Feed trades via [`on_trade`](MarketDataService::on_trade) and orderbook
/// snapshots via [`update_book`](MarketDataService::update_book). Query
/// candles and tickers at any time — all reads are synchronous and cheap.
pub struct MarketDataService {
    candles: CandleAggregator,
    tickers: TickerEngine,
}

impl MarketDataService {
    pub fn new() -> Self {
        Self {
            candles: CandleAggregator::new(),
            tickers: TickerEngine::new(),
        }
    }

    /// Process a single trade, updating all candle intervals and the 24h ticker.
    ///
    /// `taker_side` is the aggressor side: `Side::Buy` means the buyer hit the ask,
    /// `Side::Sell` means the seller hit the bid.
    pub fn on_trade(
        &mut self,
        symbol: &Symbol,
        price: Decimal,
        qty: Decimal,
        taker_side: Side,
        timestamp: DateTime<Utc>,
    ) {
        self.candles.on_trade(symbol, price, qty, taker_side, timestamp);
        self.tickers.on_trade(symbol, price, qty, taker_side, timestamp);
    }

    /// Update the best bid/ask (with sizes) for a symbol from an orderbook snapshot.
    pub fn update_book(
        &mut self,
        symbol: &Symbol,
        best_bid: Option<Decimal>,
        best_bid_size: Option<Decimal>,
        best_ask: Option<Decimal>,
        best_ask_size: Option<Decimal>,
    ) {
        self.tickers.update_book(symbol, best_bid, best_bid_size, best_ask, best_ask_size);
    }

    /// Inject futures-specific data into the ticker for a symbol.
    ///
    /// Call this after each mark-price or funding-rate update from the risk engine.
    pub fn update_futures_data(
        &mut self,
        symbol: &Symbol,
        mark_price: Decimal,
        index_price: Decimal,
        open_interest: Decimal,
        open_interest_value: Decimal,
        funding_rate: Decimal,
        next_funding_time: i64,
    ) {
        self.tickers.update_futures_data(
            symbol,
            mark_price,
            index_price,
            open_interest,
            open_interest_value,
            funding_rate,
            next_funding_time,
        );
    }

    /// Return up to `limit` closed candles (newest first) for the given symbol and interval.
    pub fn candles(
        &self,
        symbol: &Symbol,
        interval: CandleInterval,
        limit: usize,
    ) -> Vec<&Candle> {
        self.candles.history(symbol, interval, limit)
    }

    /// Return the current (still-open) candle for a symbol and interval, if any trades have been seen.
    pub fn current_candle(&self, symbol: &Symbol, interval: CandleInterval) -> Option<&Candle> {
        self.candles.current_candle(symbol, interval)
    }

    /// Return the current 24h ticker for a symbol, or `None` if no trades seen.
    pub fn ticker(&self, symbol: &Symbol) -> Option<Ticker24h> {
        self.tickers.get_ticker(symbol)
    }

    /// Return tickers for all symbols that have received at least one trade.
    pub fn all_tickers(&self) -> Vec<Ticker24h> {
        self.tickers.all_tickers()
    }
}

impl Default for MarketDataService {
    fn default() -> Self {
        Self::new()
    }
}

/// Service boundary trait for market data. In-process now, remote later.
///
/// All methods are `&self` (shared reference). Interior mutability is the
/// responsibility of the implementor (e.g. `InProcessMarketData` uses a
/// `Mutex<MarketDataService>`).
pub trait MarketDataProvider: Send + Sync {
    fn on_trade(
        &self,
        symbol: &Symbol,
        price: Decimal,
        qty: Decimal,
        taker_side: Side,
        ts: DateTime<Utc>,
    );

    fn update_book(
        &self,
        symbol: &Symbol,
        best_bid: Option<Decimal>,
        best_bid_size: Option<Decimal>,
        best_ask: Option<Decimal>,
        best_ask_size: Option<Decimal>,
    );

    fn update_futures_data(
        &self,
        symbol: &Symbol,
        mark_price: Decimal,
        index_price: Decimal,
        open_interest: Decimal,
        open_interest_value: Decimal,
        funding_rate: Decimal,
        next_funding_time: i64,
    );

    fn candles(&self, symbol: &Symbol, interval: CandleInterval, limit: usize) -> Vec<Candle>;

    fn current_candle(&self, symbol: &Symbol, interval: CandleInterval) -> Option<Candle>;

    fn ticker(&self, symbol: &Symbol) -> Option<Ticker24h>;

    fn all_tickers(&self) -> Vec<Ticker24h>;
}
