use std::collections::{HashMap, VecDeque};

use chrono::{DateTime, Duration, Utc};
use exchange_common::types::{Side, Symbol};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tracing::trace;

/// A 24-hour rolling-window ticker for a single symbol.
///
/// Fields are designed to match Binance/Bybit REST API quality:
/// all price/volume values are stored as `Decimal`, optional fields are `None`
/// until the exchange feeds the data in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ticker24h {
    pub symbol: Symbol,

    // --- Price ---
    pub last_price: Decimal,
    /// Size of the last individual trade.
    pub last_qty: Decimal,
    /// Mark price (from risk engine). `None` until injected via `update_futures_data`.
    pub mark_price: Option<Decimal>,
    /// Index price (from index aggregator). `None` until injected.
    pub index_price: Option<Decimal>,

    // --- 24h stats ---
    /// Price of the oldest trade in the 24h window (24h open reference).
    pub open_24h: Decimal,
    pub high_24h: Decimal,
    pub low_24h: Decimal,
    /// Previous session close (populated externally; `None` if unknown).
    pub prev_close: Option<Decimal>,
    /// Absolute price change: `last_price − open_24h`.
    pub price_change_24h: Decimal,
    /// Percentage price change: `(last_price − open_24h) / open_24h × 100`.
    pub price_change_pct_24h: Decimal,
    /// Volume-weighted average price over the 24h window: `quote_volume / volume`.
    pub weighted_avg_price: Decimal,
    /// Cumulative base-asset volume over the last 24 hours.
    pub volume_24h: Decimal,
    /// Cumulative quote-asset volume (price × qty) over the last 24 hours.
    pub quote_volume_24h: Decimal,
    /// Buy-side aggressor volume over the last 24h.
    pub taker_buy_volume_24h: Decimal,
    /// Sell-side aggressor volume over the last 24h.
    pub taker_sell_volume_24h: Decimal,
    pub trade_count_24h: u64,

    // --- 1h stats ---
    /// Price 1h ago (oldest trade in the last 1h window).
    pub prev_price_1h: Option<Decimal>,
    /// Percentage price change over the last 1h.
    pub price_change_pct_1h: Option<Decimal>,

    // --- Bid/Ask ---
    pub best_bid: Option<Decimal>,
    pub best_bid_size: Option<Decimal>,
    pub best_ask: Option<Decimal>,
    pub best_ask_size: Option<Decimal>,
    /// Absolute spread: `ask − bid`.
    pub spread: Option<Decimal>,
    /// Spread as a percentage of mid price: `spread / mid × 100`.
    pub spread_pct: Option<Decimal>,

    // --- Futures only ---
    /// Open interest in base asset units.
    pub open_interest: Option<Decimal>,
    /// Open interest value in quote asset.
    pub open_interest_value: Option<Decimal>,
    /// Current funding rate.
    pub funding_rate: Option<Decimal>,
    /// Unix milliseconds until the next funding settlement.
    pub next_funding_time: Option<i64>,

    // --- Meta ---
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct TradeRecord {
    price: Decimal,
    qty: Decimal,
    taker_side: Side,
    timestamp: DateTime<Utc>,
}

struct TickerState {
    /// Sliding 24h window of trades, oldest first.
    trades: VecDeque<TradeRecord>,
    last_price: Decimal,
    last_qty: Decimal,
    best_bid: Option<Decimal>,
    best_bid_size: Option<Decimal>,
    best_ask: Option<Decimal>,
    best_ask_size: Option<Decimal>,
    /// Futures data injected from the risk / index engines.
    mark_price: Option<Decimal>,
    index_price: Option<Decimal>,
    open_interest: Option<Decimal>,
    open_interest_value: Option<Decimal>,
    funding_rate: Option<Decimal>,
    next_funding_time: Option<i64>,
}

impl TickerState {
    fn new() -> Self {
        Self {
            trades: VecDeque::new(),
            last_price: Decimal::ZERO,
            last_qty: Decimal::ZERO,
            best_bid: None,
            best_bid_size: None,
            best_ask: None,
            best_ask_size: None,
            mark_price: None,
            index_price: None,
            open_interest: None,
            open_interest_value: None,
            funding_rate: None,
            next_funding_time: None,
        }
    }

    fn on_trade(&mut self, price: Decimal, qty: Decimal, taker_side: Side, timestamp: DateTime<Utc>) {
        self.last_price = price;
        self.last_qty = qty;
        self.trades.push_back(TradeRecord {
            price,
            qty,
            taker_side,
            timestamp,
        });
        self.prune(timestamp);
    }

    /// Remove trades older than 24 hours relative to `now`.
    fn prune(&mut self, now: DateTime<Utc>) {
        let cutoff = now - Duration::hours(24);
        while let Some(front) = self.trades.front() {
            if front.timestamp <= cutoff {
                self.trades.pop_front();
            } else {
                break;
            }
        }
    }

    fn compute(&self, symbol: &Symbol, updated_at: DateTime<Utc>) -> Option<Ticker24h> {
        if self.trades.is_empty() {
            return None;
        }

        let mut high = Decimal::MIN;
        let mut low = Decimal::MAX;
        let mut volume = Decimal::ZERO;
        let mut quote_volume = Decimal::ZERO;
        let mut taker_buy_volume = Decimal::ZERO;
        let mut taker_sell_volume = Decimal::ZERO;
        let mut count: u64 = 0;

        // Collect 1h ago boundary
        let cutoff_1h = updated_at - Duration::hours(1);
        let mut prev_price_1h: Option<Decimal> = None;

        for tr in &self.trades {
            if tr.price > high {
                high = tr.price;
            }
            if tr.price < low {
                low = tr.price;
            }
            volume += tr.qty;
            quote_volume += tr.price * tr.qty;
            match tr.taker_side {
                Side::Buy => taker_buy_volume += tr.qty,
                Side::Sell => taker_sell_volume += tr.qty,
            }
            count += 1;

            // Track the oldest trade inside the 1h window as our 1h-ago reference.
            if tr.timestamp <= cutoff_1h && prev_price_1h.is_none() {
                prev_price_1h = Some(tr.price);
            }
        }

        // Oldest trade in the window is the 24h open reference.
        let open_24h = self
            .trades
            .front()
            .expect("non-empty checked above")
            .price;

        let price_change_24h = self.last_price - open_24h;
        let price_change_pct_24h = if open_24h.is_zero() {
            Decimal::ZERO
        } else {
            (price_change_24h / open_24h) * Decimal::ONE_HUNDRED
        };

        let weighted_avg_price = if volume.is_zero() {
            Decimal::ZERO
        } else {
            quote_volume / volume
        };

        // 1h price change
        let price_change_pct_1h = prev_price_1h.map(|p1h| {
            if p1h.is_zero() {
                Decimal::ZERO
            } else {
                (self.last_price - p1h) / p1h * Decimal::ONE_HUNDRED
            }
        });

        // Spread computation
        let (spread, spread_pct) = match (self.best_bid, self.best_ask) {
            (Some(bid), Some(ask)) if ask > bid => {
                let sp = ask - bid;
                let mid = (bid + ask) / Decimal::TWO;
                let sp_pct = if mid.is_zero() {
                    Decimal::ZERO
                } else {
                    sp / mid * Decimal::ONE_HUNDRED
                };
                (Some(sp), Some(sp_pct))
            }
            _ => (None, None),
        };

        trace!(
            symbol = %symbol,
            last = %self.last_price,
            high = %high,
            low = %low,
            volume = %volume,
            trades = count,
            "ticker computed"
        );

        Some(Ticker24h {
            symbol: symbol.clone(),
            last_price: self.last_price,
            last_qty: self.last_qty,
            mark_price: self.mark_price,
            index_price: self.index_price,
            open_24h,
            high_24h: high,
            low_24h: low,
            prev_close: None,
            price_change_24h,
            price_change_pct_24h,
            weighted_avg_price,
            volume_24h: volume,
            quote_volume_24h: quote_volume,
            taker_buy_volume_24h: taker_buy_volume,
            taker_sell_volume_24h: taker_sell_volume,
            trade_count_24h: count,
            prev_price_1h,
            price_change_pct_1h,
            best_bid: self.best_bid,
            best_bid_size: self.best_bid_size,
            best_ask: self.best_ask,
            best_ask_size: self.best_ask_size,
            spread,
            spread_pct,
            open_interest: self.open_interest,
            open_interest_value: self.open_interest_value,
            funding_rate: self.funding_rate,
            next_funding_time: self.next_funding_time,
            updated_at,
        })
    }
}

// ---------------------------------------------------------------------------
// Public engine
// ---------------------------------------------------------------------------

/// Maintains per-symbol 24h rolling-window tickers.
pub struct TickerEngine {
    tickers: HashMap<Symbol, TickerState>,
}

impl TickerEngine {
    pub fn new() -> Self {
        Self {
            tickers: HashMap::new(),
        }
    }

    /// Feed a trade into the engine. Prunes trades older than 24 h automatically.
    pub fn on_trade(
        &mut self,
        symbol: &Symbol,
        price: Decimal,
        qty: Decimal,
        taker_side: Side,
        timestamp: DateTime<Utc>,
    ) {
        self.tickers
            .entry(symbol.clone())
            .or_insert_with(TickerState::new)
            .on_trade(price, qty, taker_side, timestamp);
    }

    /// Update the best bid/ask (with sizes) from an orderbook snapshot.
    pub fn update_book(
        &mut self,
        symbol: &Symbol,
        best_bid: Option<Decimal>,
        best_bid_size: Option<Decimal>,
        best_ask: Option<Decimal>,
        best_ask_size: Option<Decimal>,
    ) {
        let state = self
            .tickers
            .entry(symbol.clone())
            .or_insert_with(TickerState::new);
        state.best_bid = best_bid;
        state.best_bid_size = best_bid_size;
        state.best_ask = best_ask;
        state.best_ask_size = best_ask_size;
    }

    /// Inject futures-specific market data (mark price, index price, OI, funding).
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
        let state = self
            .tickers
            .entry(symbol.clone())
            .or_insert_with(TickerState::new);
        state.mark_price = Some(mark_price);
        state.index_price = Some(index_price);
        state.open_interest = Some(open_interest);
        state.open_interest_value = Some(open_interest_value);
        state.funding_rate = Some(funding_rate);
        state.next_funding_time = Some(next_funding_time);
    }

    /// Compute and return the current 24h ticker for a symbol, or `None` if no
    /// trades have been seen.
    pub fn get_ticker(&self, symbol: &Symbol) -> Option<Ticker24h> {
        self.tickers
            .get(symbol)
            .and_then(|s| s.compute(symbol, Utc::now()))
    }

    /// Compute tickers for all known symbols that have at least one trade.
    pub fn all_tickers(&self) -> Vec<Ticker24h> {
        let now = Utc::now();
        self.tickers
            .iter()
            .filter_map(|(sym, state)| state.compute(sym, now))
            .collect()
    }
}

impl Default for TickerEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use rust_decimal_macros::dec;

    fn ts(h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 1, 2, h, mi, 0).unwrap()
    }

    fn sym() -> Symbol {
        Symbol::new("BTCUSDT")
    }

    fn sym2() -> Symbol {
        Symbol::new("ETHUSDT")
    }

    // T9 — single trade sets last_price
    #[test]
    fn single_trade_sets_last_price() {
        let mut eng = TickerEngine::new();
        eng.on_trade(&sym(), dec!(50_000), dec!(1), Side::Buy, ts(12, 0));
        let t = eng.get_ticker(&sym()).unwrap();
        assert_eq!(t.last_price, dec!(50_000));
        assert_eq!(t.last_qty, dec!(1));
    }

    // T10 — 24h high/low computed correctly
    #[test]
    fn high_low_computed_correctly() {
        let mut eng = TickerEngine::new();
        eng.on_trade(&sym(), dec!(100), dec!(1), Side::Buy, ts(12, 0));
        eng.on_trade(&sym(), dec!(150), dec!(1), Side::Buy, ts(12, 1));
        eng.on_trade(&sym(), dec!(80), dec!(1), Side::Sell, ts(12, 2));

        let t = eng.get_ticker(&sym()).unwrap();
        assert_eq!(t.high_24h, dec!(150));
        assert_eq!(t.low_24h, dec!(80));
    }

    // T11 — volume accumulates with taker breakdown
    #[test]
    fn volume_accumulates() {
        let mut eng = TickerEngine::new();
        eng.on_trade(&sym(), dec!(100), dec!(2), Side::Buy, ts(12, 0));
        eng.on_trade(&sym(), dec!(200), dec!(3), Side::Sell, ts(12, 1));

        let t = eng.get_ticker(&sym()).unwrap();
        assert_eq!(t.volume_24h, dec!(5));
        assert_eq!(t.quote_volume_24h, dec!(100) * dec!(2) + dec!(200) * dec!(3));
        assert_eq!(t.trade_count_24h, 2);
        assert_eq!(t.taker_buy_volume_24h, dec!(2));
        assert_eq!(t.taker_sell_volume_24h, dec!(3));
    }

    // T12 — old trades (>24h) are pruned
    #[test]
    fn old_trades_pruned() {
        let mut eng = TickerEngine::new();

        // Trade 25 hours ago — should be outside the window once newer trade arrives.
        let old = Utc.with_ymd_and_hms(2024, 1, 1, 11, 0, 0).unwrap(); // 25h before ts(12,0)
        eng.on_trade(&sym(), dec!(999), dec!(100), Side::Buy, old);

        // Trigger pruning by inserting a recent trade.
        eng.on_trade(&sym(), dec!(100), dec!(1), Side::Sell, ts(12, 0));

        let t = eng.get_ticker(&sym()).unwrap();
        // Only the recent trade should remain.
        assert_eq!(t.trade_count_24h, 1);
        assert_eq!(t.volume_24h, dec!(1));
        assert_eq!(t.last_price, dec!(100));
    }

    // T13 — price change calculated (current vs 24h-ago open)
    #[test]
    fn price_change_calculated() {
        let mut eng = TickerEngine::new();
        eng.on_trade(&sym(), dec!(1000), dec!(1), Side::Buy, ts(10, 0));
        eng.on_trade(&sym(), dec!(1100), dec!(1), Side::Buy, ts(12, 0));

        let t = eng.get_ticker(&sym()).unwrap();
        assert_eq!(t.open_24h, dec!(1000));
        assert_eq!(t.price_change_24h, dec!(100));
        // 10% change
        assert_eq!(t.price_change_pct_24h, dec!(10));
    }

    // T14 — empty ticker returns None
    #[test]
    fn empty_ticker_returns_none() {
        let eng = TickerEngine::new();
        assert!(eng.get_ticker(&sym()).is_none());
    }

    // T15 — multiple symbols are independent
    #[test]
    fn multiple_symbols_independent() {
        let mut eng = TickerEngine::new();
        eng.on_trade(&sym(), dec!(50_000), dec!(1), Side::Buy, ts(12, 0));
        eng.on_trade(&sym2(), dec!(3_000), dec!(10), Side::Sell, ts(12, 0));

        let t1 = eng.get_ticker(&sym()).unwrap();
        let t2 = eng.get_ticker(&sym2()).unwrap();

        assert_eq!(t1.last_price, dec!(50_000));
        assert_eq!(t2.last_price, dec!(3_000));
        assert_eq!(t1.volume_24h, dec!(1));
        assert_eq!(t2.volume_24h, dec!(10));
    }

    // T16 — spread computed correctly from book update
    #[test]
    fn spread_computed_from_book() {
        let mut eng = TickerEngine::new();
        eng.on_trade(&sym(), dec!(100), dec!(1), Side::Buy, ts(12, 0));
        eng.update_book(
            &sym(),
            Some(dec!(99)),
            Some(dec!(5)),
            Some(dec!(101)),
            Some(dec!(3)),
        );

        let t = eng.get_ticker(&sym()).unwrap();
        assert_eq!(t.best_bid, Some(dec!(99)));
        assert_eq!(t.best_bid_size, Some(dec!(5)));
        assert_eq!(t.best_ask, Some(dec!(101)));
        assert_eq!(t.best_ask_size, Some(dec!(3)));
        assert_eq!(t.spread, Some(dec!(2)));
        // mid = 100, spread_pct = 2/100 * 100 = 2%
        assert_eq!(t.spread_pct, Some(dec!(2)));
    }

    // T17 — futures data injection
    #[test]
    fn futures_data_injected() {
        let mut eng = TickerEngine::new();
        eng.on_trade(&sym(), dec!(50_000), dec!(1), Side::Buy, ts(12, 0));
        eng.update_futures_data(
            &sym(),
            dec!(50_100),
            dec!(49_900),
            dec!(1_000),
            dec!(50_000_000),
            dec!(0.0001),
            1_704_124_800_000,
        );

        let t = eng.get_ticker(&sym()).unwrap();
        assert_eq!(t.mark_price, Some(dec!(50_100)));
        assert_eq!(t.index_price, Some(dec!(49_900)));
        assert_eq!(t.open_interest, Some(dec!(1_000)));
        assert_eq!(t.funding_rate, Some(dec!(0.0001)));
        assert_eq!(t.next_funding_time, Some(1_704_124_800_000));
    }

    // T18 — weighted_avg_price equals quote_volume / volume
    #[test]
    fn weighted_avg_price_correct() {
        let mut eng = TickerEngine::new();
        eng.on_trade(&sym(), dec!(100), dec!(1), Side::Buy, ts(12, 0));
        eng.on_trade(&sym(), dec!(200), dec!(2), Side::Sell, ts(12, 1));

        let t = eng.get_ticker(&sym()).unwrap();
        // (100*1 + 200*2) / 3 = 500/3
        assert_eq!(t.weighted_avg_price, dec!(500) / dec!(3));
    }
}
