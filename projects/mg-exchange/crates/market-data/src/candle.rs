use std::collections::{HashMap, VecDeque};

use chrono::{DateTime, Datelike, DurationRound, TimeDelta, Utc};
use exchange_common::types::{Side, Symbol};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tracing::trace;

/// Candle aggregation intervals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CandleInterval {
    S1,   // 1 second
    M1,   // 1 minute
    M3,   // 3 minutes
    M5,   // 5 minutes
    M15,  // 15 minutes
    M30,  // 30 minutes
    H1,   // 1 hour
    H2,   // 2 hours
    H4,   // 4 hours
    H6,   // 6 hours
    H8,   // 8 hours
    H12,  // 12 hours
    D1,   // 1 day
    D3,   // 3 days
    W1,   // 1 week
    MN1,  // 1 month
}

impl CandleInterval {
    /// Duration of the interval in seconds (for sub-month intervals).
    pub fn duration_secs(&self) -> i64 {
        match self {
            Self::S1  => 1,
            Self::M1  => 60,
            Self::M3  => 3 * 60,
            Self::M5  => 5 * 60,
            Self::M15 => 15 * 60,
            Self::M30 => 30 * 60,
            Self::H1  => 60 * 60,
            Self::H2  => 2 * 60 * 60,
            Self::H4  => 4 * 60 * 60,
            Self::H6  => 6 * 60 * 60,
            Self::H8  => 8 * 60 * 60,
            Self::H12 => 12 * 60 * 60,
            Self::D1  => 24 * 60 * 60,
            Self::D3  => 3 * 24 * 60 * 60,
            // W1 and MN1 use calendar logic — return a representative value for buffer sizing.
            Self::W1  => 7 * 24 * 60 * 60,
            Self::MN1 => 30 * 24 * 60 * 60,
        }
    }

    /// Align a timestamp down to the start of its enclosing interval boundary.
    pub fn floor_timestamp(&self, ts: DateTime<Utc>) -> DateTime<Utc> {
        match self {
            Self::W1 => {
                // Floor to the Monday 00:00 UTC of the current week.
                let days_since_monday = ts.weekday().num_days_from_monday();
                let day_start = ts
                    .date_naive()
                    .and_hms_opt(0, 0, 0)
                    .expect("valid time")
                    .and_utc();
                day_start - TimeDelta::days(days_since_monday as i64)
            }
            Self::MN1 => {
                // Floor to the 1st of the current month 00:00 UTC.
                ts.date_naive()
                    .with_day(1)
                    .expect("day 1 always exists")
                    .and_hms_opt(0, 0, 0)
                    .expect("valid time")
                    .and_utc()
            }
            _ => {
                let delta = TimeDelta::seconds(self.duration_secs());
                ts.duration_trunc(delta)
                    .expect("timestamp within reasonable range; duration_trunc only fails on overflow")
            }
        }
    }

    /// Maximum history buffer size for this interval.
    pub fn max_history(&self) -> usize {
        match self {
            Self::S1  => 3600,  // 1 hour of seconds
            Self::M1  => 1500,
            Self::M3  => 1000,
            Self::M5  => 1000,
            Self::M15 => 1000,
            Self::M30 => 1000,
            Self::H1  => 1000,
            Self::H2  => 1000,
            Self::H4  => 1000,
            Self::H6  => 1000,
            Self::H8  => 1000,
            Self::H12 => 1000,
            Self::D1  => 1000,
            Self::D3  => 500,
            Self::W1  => 500,
            Self::MN1 => 120,
        }
    }
}

/// A single OHLCV candle with taker volume breakdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candle {
    pub symbol: Symbol,
    pub interval: CandleInterval,
    pub open_time: DateTime<Utc>,
    pub close_time: DateTime<Utc>,
    /// First trade price in the interval.
    pub open: Decimal,
    /// Highest trade price in the interval.
    pub high: Decimal,
    /// Lowest trade price in the interval.
    pub low: Decimal,
    /// Last trade price in the interval.
    pub close: Decimal,
    /// Cumulative base-asset volume.
    pub volume: Decimal,
    /// Cumulative quote-asset volume (price × qty).
    pub quote_volume: Decimal,
    pub trade_count: u64,
    /// Volume from buy-side aggressors (taker buys).
    pub taker_buy_volume: Decimal,
    /// Quote volume from buy-side aggressors.
    pub taker_buy_quote_volume: Decimal,
    /// Volume from sell-side aggressors (taker sells).
    pub taker_sell_volume: Decimal,
    /// Quote volume from sell-side aggressors.
    pub taker_sell_quote_volume: Decimal,
    /// `true` once the interval has elapsed and no further trades will be applied.
    pub is_closed: bool,
}

impl Candle {
    fn new(symbol: Symbol, interval: CandleInterval, open_time: DateTime<Utc>) -> Self {
        let close_time =
            open_time + TimeDelta::seconds(interval.duration_secs()) - TimeDelta::milliseconds(1);
        Self {
            symbol,
            interval,
            open_time,
            close_time,
            open: Decimal::ZERO,
            high: Decimal::ZERO,
            low: Decimal::ZERO,
            close: Decimal::ZERO,
            volume: Decimal::ZERO,
            quote_volume: Decimal::ZERO,
            trade_count: 0,
            taker_buy_volume: Decimal::ZERO,
            taker_buy_quote_volume: Decimal::ZERO,
            taker_sell_volume: Decimal::ZERO,
            taker_sell_quote_volume: Decimal::ZERO,
            is_closed: false,
        }
    }

    fn apply_trade(&mut self, price: Decimal, qty: Decimal, taker_side: Side) {
        if self.trade_count == 0 {
            self.open = price;
            self.high = price;
            self.low = price;
        } else {
            if price > self.high {
                self.high = price;
            }
            if price < self.low {
                self.low = price;
            }
        }
        self.close = price;
        self.volume += qty;
        let quote_qty = price * qty;
        self.quote_volume += quote_qty;
        self.trade_count += 1;

        match taker_side {
            Side::Buy => {
                self.taker_buy_volume += qty;
                self.taker_buy_quote_volume += quote_qty;
            }
            Side::Sell => {
                self.taker_sell_volume += qty;
                self.taker_sell_quote_volume += quote_qty;
            }
        }
    }

    /// Volume-weighted average price for this candle.
    ///
    /// Returns `Decimal::ZERO` if there are no trades yet.
    pub fn vwap(&self) -> Decimal {
        if self.volume.is_zero() {
            Decimal::ZERO
        } else {
            self.quote_volume / self.volume
        }
    }

    /// Net delta: taker buy volume minus taker sell volume.
    pub fn delta(&self) -> Decimal {
        self.taker_buy_volume - self.taker_sell_volume
    }

    /// Signed delta as a fraction of total volume, in the range [-1, +1].
    ///
    /// Returns `Decimal::ZERO` if there are no trades yet.
    pub fn delta_pct(&self) -> Decimal {
        if self.volume.is_zero() {
            Decimal::ZERO
        } else {
            self.delta() / self.volume
        }
    }
}

/// Ring buffer holding the current open candle and up to `max_history` closed candles
/// (newest-first order).
struct CandleBuffer {
    current: Option<Candle>,
    /// Closed candles, index 0 is the most recent.
    history: VecDeque<Candle>,
    max_history: usize,
}

impl CandleBuffer {
    fn new(max_history: usize) -> Self {
        Self {
            current: None,
            history: VecDeque::new(),
            max_history,
        }
    }

    /// Apply a trade at `timestamp`. Closes the open candle if the timestamp has
    /// crossed the interval boundary, then opens a new one.
    fn on_trade(
        &mut self,
        symbol: &Symbol,
        interval: CandleInterval,
        price: Decimal,
        qty: Decimal,
        taker_side: Side,
        timestamp: DateTime<Utc>,
    ) {
        let open_time = interval.floor_timestamp(timestamp);

        // Close any candles whose interval has elapsed.
        loop {
            match &mut self.current {
                Some(candle) if open_time > candle.open_time => {
                    // Finalize and rotate into history.
                    let mut closed = self.current.take().expect("checked above");
                    closed.is_closed = true;
                    trace!(
                        symbol = %closed.symbol,
                        interval = ?closed.interval,
                        open_time = %closed.open_time,
                        "candle closed"
                    );
                    self.history.push_front(closed);
                    if self.history.len() > self.max_history {
                        self.history.pop_back();
                    }
                }
                _ => break,
            }
        }

        // Ensure there is a current open candle for this interval.
        if self.current.is_none() {
            self.current = Some(Candle::new(symbol.clone(), interval, open_time));
        }

        self.current
            .as_mut()
            .expect("just inserted above")
            .apply_trade(price, qty, taker_side);
    }
}

/// Aggregates raw trades into OHLCV candles for any number of symbols and intervals.
pub struct CandleAggregator {
    candles: HashMap<(Symbol, CandleInterval), CandleBuffer>,
}

impl CandleAggregator {
    pub fn new() -> Self {
        Self {
            candles: HashMap::new(),
        }
    }

    /// Feed a trade into every interval for the given symbol.
    pub fn on_trade(
        &mut self,
        symbol: &Symbol,
        price: Decimal,
        qty: Decimal,
        taker_side: Side,
        timestamp: DateTime<Utc>,
    ) {
        for interval in [
            CandleInterval::S1,
            CandleInterval::M1,
            CandleInterval::M3,
            CandleInterval::M5,
            CandleInterval::M15,
            CandleInterval::M30,
            CandleInterval::H1,
            CandleInterval::H2,
            CandleInterval::H4,
            CandleInterval::H6,
            CandleInterval::H8,
            CandleInterval::H12,
            CandleInterval::D1,
            CandleInterval::D3,
            CandleInterval::W1,
            CandleInterval::MN1,
        ] {
            let max_hist = interval.max_history();
            self.candles
                .entry((symbol.clone(), interval))
                .or_insert_with(|| CandleBuffer::new(max_hist))
                .on_trade(symbol, interval, price, qty, taker_side, timestamp);
        }
    }

    /// Return the current (open, not yet closed) candle for a symbol+interval, if any.
    pub fn current_candle(&self, symbol: &Symbol, interval: CandleInterval) -> Option<&Candle> {
        self.candles
            .get(&(symbol.clone(), interval))
            .and_then(|b| b.current.as_ref())
    }

    /// Return up to `limit` of the most recent *closed* candles (newest first).
    pub fn history(&self, symbol: &Symbol, interval: CandleInterval, limit: usize) -> Vec<&Candle> {
        self.candles
            .get(&(symbol.clone(), interval))
            .map(|b| b.history.iter().take(limit).collect())
            .unwrap_or_default()
    }
}

impl Default for CandleAggregator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Weekday};
    use rust_decimal_macros::dec;

    fn ts(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap()
    }

    fn sym() -> Symbol {
        Symbol::new("BTCUSDT")
    }

    // T1 — single trade creates an open candle
    #[test]
    fn single_trade_creates_open_candle() {
        let mut agg = CandleAggregator::new();
        agg.on_trade(&sym(), dec!(50_000), dec!(1), Side::Buy, ts(2024, 1, 1, 0, 0, 30));
        let c = agg.current_candle(&sym(), CandleInterval::M1).unwrap();
        assert_eq!(c.open, dec!(50_000));
        assert!(!c.is_closed);
        assert_eq!(c.trade_count, 1);
    }

    // T2 — multiple trades update OHLC correctly
    #[test]
    fn multiple_trades_update_ohlc() {
        let mut agg = CandleAggregator::new();
        let t = ts(2024, 1, 1, 0, 0, 0);
        agg.on_trade(&sym(), dec!(100), dec!(1), Side::Buy, t);
        agg.on_trade(&sym(), dec!(120), dec!(2), Side::Sell, t);
        agg.on_trade(&sym(), dec!(90), dec!(1), Side::Buy, t);
        agg.on_trade(&sym(), dec!(110), dec!(1), Side::Sell, t);

        let c = agg.current_candle(&sym(), CandleInterval::M1).unwrap();
        assert_eq!(c.open, dec!(100));
        assert_eq!(c.high, dec!(120));
        assert_eq!(c.low, dec!(90));
        assert_eq!(c.close, dec!(110));
        assert_eq!(c.volume, dec!(5));
        assert_eq!(c.quote_volume, dec!(100) + dec!(240) + dec!(90) + dec!(110));
        assert_eq!(c.trade_count, 4);
    }

    // T3 — candle closes at interval boundary
    #[test]
    fn candle_closes_at_boundary() {
        let mut agg = CandleAggregator::new();
        agg.on_trade(&sym(), dec!(100), dec!(1), Side::Buy, ts(2024, 1, 1, 0, 0, 30));
        // Trade in next minute → previous candle must close
        agg.on_trade(&sym(), dec!(200), dec!(1), Side::Sell, ts(2024, 1, 1, 0, 1, 0));

        let history = agg.history(&sym(), CandleInterval::M1, 10);
        assert_eq!(history.len(), 1);
        assert!(history[0].is_closed);
        assert_eq!(history[0].open, dec!(100));
    }

    // T4 — new candle starts after close
    #[test]
    fn new_candle_starts_after_close() {
        let mut agg = CandleAggregator::new();
        agg.on_trade(&sym(), dec!(100), dec!(1), Side::Buy, ts(2024, 1, 1, 0, 0, 0));
        agg.on_trade(&sym(), dec!(200), dec!(1), Side::Sell, ts(2024, 1, 1, 0, 1, 0));

        let c = agg.current_candle(&sym(), CandleInterval::M1).unwrap();
        assert_eq!(c.open, dec!(200));
        assert!(!c.is_closed);
    }

    // T5 — history preserves order (newest first)
    #[test]
    fn history_newest_first() {
        let mut agg = CandleAggregator::new();
        // Three consecutive 1-minute candles
        agg.on_trade(&sym(), dec!(100), dec!(1), Side::Buy, ts(2024, 1, 1, 0, 0, 0));
        agg.on_trade(&sym(), dec!(200), dec!(1), Side::Buy, ts(2024, 1, 1, 0, 1, 0));
        agg.on_trade(&sym(), dec!(300), dec!(1), Side::Buy, ts(2024, 1, 1, 0, 2, 0));
        agg.on_trade(&sym(), dec!(400), dec!(1), Side::Buy, ts(2024, 1, 1, 0, 3, 0));

        let h = agg.history(&sym(), CandleInterval::M1, 10);
        // History should be: 0:02, 0:01, 0:00 (newest first)
        assert_eq!(h.len(), 3);
        assert_eq!(h[0].open, dec!(300));
        assert_eq!(h[1].open, dec!(200));
        assert_eq!(h[2].open, dec!(100));
    }

    // T6 — history limit respected
    #[test]
    fn history_limit_respected() {
        let mut agg = CandleAggregator::new();
        // Push 5 closed candles by sending trades one minute apart
        for i in 0..6_u32 {
            agg.on_trade(
                &sym(),
                dec!(100),
                dec!(1),
                Side::Buy,
                ts(2024, 1, 1, 0, i, 0),
            );
        }
        // Request only 2
        let h = agg.history(&sym(), CandleInterval::M1, 2);
        assert_eq!(h.len(), 2);
    }

    // T7 — multiple intervals from same trade
    #[test]
    fn multiple_intervals_from_same_trade() {
        let mut agg = CandleAggregator::new();
        agg.on_trade(&sym(), dec!(42_000), dec!(0.5), Side::Buy, ts(2024, 1, 1, 0, 0, 0));

        for interval in [CandleInterval::M1, CandleInterval::M5, CandleInterval::H1] {
            let c = agg.current_candle(&sym(), interval).unwrap();
            assert_eq!(c.open, dec!(42_000), "interval {:?} open mismatch", interval);
            assert_eq!(c.volume, dec!(0.5), "interval {:?} volume mismatch", interval);
        }
    }

    // T8 — floor_timestamp for each interval
    #[test]
    fn floor_timestamp_each_interval() {
        let ts_raw = ts(2024, 1, 15, 13, 47, 33); // Monday 2024-01-15, 13:47:33

        assert_eq!(
            CandleInterval::M1.floor_timestamp(ts_raw),
            ts(2024, 1, 15, 13, 47, 0)
        );
        assert_eq!(
            CandleInterval::M5.floor_timestamp(ts_raw),
            ts(2024, 1, 15, 13, 45, 0)
        );
        assert_eq!(
            CandleInterval::M15.floor_timestamp(ts_raw),
            ts(2024, 1, 15, 13, 45, 0)
        );
        assert_eq!(
            CandleInterval::H1.floor_timestamp(ts_raw),
            ts(2024, 1, 15, 13, 0, 0)
        );
        assert_eq!(
            CandleInterval::H4.floor_timestamp(ts_raw),
            ts(2024, 1, 15, 12, 0, 0)
        );
        assert_eq!(
            CandleInterval::D1.floor_timestamp(ts_raw),
            ts(2024, 1, 15, 0, 0, 0)
        );
        // 2024-01-15 is a Monday, so W1 floor = 2024-01-15 00:00:00
        assert_eq!(
            CandleInterval::W1.floor_timestamp(ts_raw),
            ts(2024, 1, 15, 0, 0, 0)
        );
        // MN1 floor = first of month
        assert_eq!(
            CandleInterval::MN1.floor_timestamp(ts_raw),
            ts(2024, 1, 1, 0, 0, 0)
        );
    }

    // T9 — taker volume tracked separately
    #[test]
    fn taker_volume_tracked() {
        let mut agg = CandleAggregator::new();
        let t = ts(2024, 1, 1, 0, 0, 0);
        // 3 buy-side aggressors @ 100 qty 1 each
        agg.on_trade(&sym(), dec!(100), dec!(1), Side::Buy, t);
        agg.on_trade(&sym(), dec!(100), dec!(1), Side::Buy, t);
        agg.on_trade(&sym(), dec!(100), dec!(1), Side::Buy, t);
        // 1 sell-side aggressor @ 100 qty 2
        agg.on_trade(&sym(), dec!(100), dec!(2), Side::Sell, t);

        let c = agg.current_candle(&sym(), CandleInterval::M1).unwrap();
        assert_eq!(c.taker_buy_volume, dec!(3));
        assert_eq!(c.taker_sell_volume, dec!(2));
        assert_eq!(c.taker_buy_quote_volume, dec!(300));
        assert_eq!(c.taker_sell_quote_volume, dec!(200));
        assert_eq!(c.delta(), dec!(1));   // 3 - 2
        assert_eq!(c.volume, dec!(5));
    }

    // T10 — vwap computation
    #[test]
    fn vwap_computed_correctly() {
        let mut agg = CandleAggregator::new();
        let t = ts(2024, 1, 1, 0, 0, 0);
        // 1 unit at 100, 2 units at 200 → vwap = (100 + 400) / 3 = 166.666...
        agg.on_trade(&sym(), dec!(100), dec!(1), Side::Buy, t);
        agg.on_trade(&sym(), dec!(200), dec!(2), Side::Sell, t);

        let c = agg.current_candle(&sym(), CandleInterval::M1).unwrap();
        // quote_volume = 100*1 + 200*2 = 500, volume = 3
        assert_eq!(c.vwap(), dec!(500) / dec!(3));
    }

    // T11 — W1 floor lands on Monday
    #[test]
    fn w1_floor_lands_on_monday() {
        // 2024-01-17 is a Wednesday
        let wednesday = ts(2024, 1, 17, 10, 30, 0);
        let floored = CandleInterval::W1.floor_timestamp(wednesday);
        // Should floor back to Monday 2024-01-15
        assert_eq!(floored, ts(2024, 1, 15, 0, 0, 0));
        assert_eq!(floored.weekday(), Weekday::Mon);
    }

    // T12 — MN1 floor is always the 1st
    #[test]
    fn mn1_floor_is_first_of_month() {
        let mid_month = ts(2024, 3, 19, 14, 22, 0);
        assert_eq!(
            CandleInterval::MN1.floor_timestamp(mid_month),
            ts(2024, 3, 1, 0, 0, 0)
        );
    }

    // T13 — delta_pct range
    #[test]
    fn delta_pct_range() {
        let mut agg = CandleAggregator::new();
        let t = ts(2024, 1, 1, 0, 0, 0);
        // All buys → delta_pct should be +1
        agg.on_trade(&sym(), dec!(100), dec!(5), Side::Buy, t);
        let c = agg.current_candle(&sym(), CandleInterval::M1).unwrap();
        assert_eq!(c.delta_pct(), Decimal::ONE);
    }
}
