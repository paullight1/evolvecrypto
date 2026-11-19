use std::collections::HashMap;

use chrono::Utc;
use exchange_common::{Price, Symbol};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::aggregator::IndexPriceAggregator;

/// Deterministic mock price feed for use in tests and backtests.
///
/// Allows manual price injection and simple random-walk simulation without
/// requiring network access or real exchange connections.
pub struct MockPriceFeed {
    /// `prices[symbol][source_name]` → current price.
    prices: HashMap<Symbol, HashMap<String, Price>>,
    /// Simple LCG state for deterministic pseudo-random steps.
    rng_state: u64,
}

impl MockPriceFeed {
    pub fn new() -> Self {
        Self {
            prices: HashMap::new(),
            rng_state: 0x1234_5678_DEAD_BEEF,
        }
    }

    /// Manually set the price for a (symbol, source) pair.
    pub fn set_price(&mut self, symbol: Symbol, source: impl Into<String>, price: Price) {
        self.prices
            .entry(symbol)
            .or_default()
            .insert(source.into(), price);
    }

    /// Retrieve the current stored price for a (symbol, source) pair.
    pub fn get_price(&self, symbol: &Symbol, source: &str) -> Option<Price> {
        self.prices.get(symbol)?.get(source).copied()
    }

    /// Apply a random-walk tick around `base_price` for all named sources.
    ///
    /// `volatility` is the maximum fractional step per tick (e.g., `dec!(0.001)` = 0.1%).
    /// Each source receives an independent step so prices naturally diverge slightly,
    /// mimicking real cross-exchange basis.
    ///
    /// Uses a simple LCG for determinism — no `rand` dependency needed.
    pub fn simulate_tick(
        &mut self,
        symbol: &Symbol,
        sources: &[&str],
        base_price: Price,
        volatility: Decimal,
    ) {
        let source_prices = self.prices.entry(symbol.clone()).or_default();

        for source in sources {
            let current = source_prices
                .get(*source)
                .copied()
                .unwrap_or(base_price);

            // LCG step: produces a u64 in [0, u64::MAX].
            self.rng_state = self
                .rng_state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);

            // Map to [-1.0, 1.0] range as a Decimal.
            // We use the top 31 bits to keep numbers manageable.
            let bits = (self.rng_state >> 33) as i64; // [-2^30, 2^30)
            let max = (1i64 << 30) as i64;
            // sign: top bit of the lower 31 bits
            let signed = if bits & (1 << 29) != 0 {
                bits - max
            } else {
                bits
            };

            // Scale to [-volatility, +volatility].
            // Use integer arithmetic to stay in Decimal territory.
            // step = (signed / max) * volatility * current
            let numerator = Decimal::from(signed);
            let denominator = Decimal::from(max);
            let step = (numerator / denominator) * volatility * current;

            let new_price = (current + step).max(dec!(0.000001));
            source_prices.insert(source.to_string(), new_price);
        }
    }

    /// Push all currently stored prices into an aggregator.
    ///
    /// Useful in tests to quickly populate an aggregator without manual
    /// `update_source_price` calls for every source.
    pub fn flush_to_aggregator(&self, aggregator: &mut IndexPriceAggregator) {
        let now = Utc::now();
        for (symbol, source_map) in &self.prices {
            for (source_name, price) in source_map {
                aggregator.update_source_price(symbol, source_name, *price, now);
            }
        }
    }
}

impl Default for MockPriceFeed {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;
    use crate::aggregator::IndexPriceAggregator;

    fn btc() -> Symbol {
        Symbol::new("BTCUSDT")
    }

    // ── set/get ─────────────────────────────────────────────────────────────

    #[test]
    fn set_and_get_price() {
        let mut feed = MockPriceFeed::new();
        feed.set_price(btc(), "Binance", dec!(50000));
        assert_eq!(feed.get_price(&btc(), "Binance"), Some(dec!(50000)));
    }

    #[test]
    fn get_price_missing_returns_none() {
        let feed = MockPriceFeed::new();
        assert_eq!(feed.get_price(&btc(), "Binance"), None);
    }

    #[test]
    fn set_price_overwrites() {
        let mut feed = MockPriceFeed::new();
        feed.set_price(btc(), "Binance", dec!(50000));
        feed.set_price(btc(), "Binance", dec!(51000));
        assert_eq!(feed.get_price(&btc(), "Binance"), Some(dec!(51000)));
    }

    // ── simulate_tick ────────────────────────────────────────────────────────

    #[test]
    fn simulate_tick_stays_near_base_price() {
        let mut feed = MockPriceFeed::new();
        let sources = ["Binance", "Coinbase", "Kraken", "OKX"];
        let base = dec!(50000);
        let volatility = dec!(0.001); // 0.1%

        // Run 10 ticks and check prices stay within ±5% of base (very conservative).
        for _ in 0..10 {
            feed.simulate_tick(&btc(), &sources, base, volatility);
        }

        for source in &sources {
            let price = feed.get_price(&btc(), source).unwrap();
            let pct_diff = ((price - base) / base).abs();
            assert!(
                pct_diff < dec!(0.05),
                "Price {price} drifted more than 5% from base {base}"
            );
        }
    }

    #[test]
    fn simulate_tick_price_is_positive() {
        let mut feed = MockPriceFeed::new();
        let sources = ["Binance"];

        for _ in 0..100 {
            feed.simulate_tick(&btc(), &sources, dec!(1), dec!(2.0));
        }

        let price = feed.get_price(&btc(), "Binance").unwrap();
        assert!(price > dec!(0), "Price must remain positive, got {price}");
    }

    #[test]
    fn simulate_tick_different_sources_diverge() {
        let mut feed = MockPriceFeed::new();
        let sources = ["ExchangeA", "ExchangeB"];

        for _ in 0..20 {
            feed.simulate_tick(&btc(), &sources, dec!(100), dec!(0.01));
        }

        let a = feed.get_price(&btc(), "ExchangeA").unwrap();
        let b = feed.get_price(&btc(), "ExchangeB").unwrap();
        // After 20 independent ticks, prices should have diverged (probabilistically).
        // With high confidence: not exactly equal.
        assert_ne!(a, b, "Independent sources should diverge after multiple ticks");
    }

    // ── flush_to_aggregator ──────────────────────────────────────────────────

    #[test]
    fn flush_to_aggregator_enables_index_calculation() {
        let mut feed = MockPriceFeed::new();
        feed.set_price(btc(), "Binance", dec!(50000));
        feed.set_price(btc(), "Coinbase", dec!(50100));
        feed.set_price(btc(), "Kraken", dec!(49950));

        let mut agg = IndexPriceAggregator::with_defaults(vec![btc()]);
        feed.flush_to_aggregator(&mut agg);

        let index = agg.calculate_index(&btc()).unwrap();
        assert!(index.sources_used >= 2);
    }
}
