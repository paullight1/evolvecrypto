//! TradFi price aggregator.
//!
//! Applies the same weighted-median algorithm used by `exchange-index-price`
//! to a set of TradFi price sources (Pyth, forex REST, commodity REST, etc.).
//!
//! # Staleness detection
//! A source whose last update is older than `stale_timeout_secs` is excluded
//! from aggregation. This prevents a slow or dead feed from contributing a
//! stale price to the median.
//!
//! # Deviation circuit breaker
//! When the min/max spread across healthy sources exceeds `max_deviation_pct`
//! (default 2%), the aggregated price is flagged as unreliable via
//! [`TradFiPrice::circuit_breaker_triggered`]. Callers should halt order
//! acceptance for the affected symbol until the feeds re-converge.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, warn};

use crate::{
    feeds::FeedSourceConfig,
    weighted_median::weighted_median,
};

// ── Errors ────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum AggregatorError {
    /// Not enough healthy (non-stale) sources to produce a reliable price.
    #[error(
        "Insufficient healthy sources for {symbol}: need {needed}, have {available} (stale: {stale})"
    )]
    InsufficientSources {
        symbol: String,
        needed: usize,
        available: usize,
        stale: usize,
    },

    /// Symbol is not tracked by this aggregator.
    #[error("Symbol not tracked: {0}")]
    UnknownSymbol(String),
}

// ── Output types ──────────────────────────────────────────────────────────────

/// A computed, aggregated TradFi price.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradFiPrice {
    /// Normalised symbol, e.g. `"XAUUSD"`.
    pub symbol: String,
    /// Weighted-median price across all healthy sources.
    pub price: Decimal,
    /// Number of healthy sources that contributed to this price.
    pub sources_used: usize,
    /// True when the cross-source deviation exceeds the configured threshold.
    ///
    /// Callers should reject new orders while this flag is set.
    pub circuit_breaker_triggered: bool,
    pub timestamp: DateTime<Utc>,
}

// ── Internal source state ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct SourceState {
    /// Feed adapter name — used in `Debug` output and future tracing spans.
    #[allow(dead_code)]
    name: String,
    weight: Decimal,
    last_price: Option<Decimal>,
    last_update: DateTime<Utc>,
    /// True after at least one price update has been received.
    is_healthy: bool,
    stale_timeout_secs: u64,
}

impl SourceState {
    fn from_config(cfg: &FeedSourceConfig) -> Self {
        Self {
            name: cfg.name.clone(),
            weight: cfg.weight,
            last_price: None,
            last_update: DateTime::UNIX_EPOCH,
            is_healthy: false,
            stale_timeout_secs: cfg.stale_timeout_secs,
        }
    }

    fn is_stale(&self, now: DateTime<Utc>) -> bool {
        let age = now.signed_duration_since(self.last_update);
        age.num_seconds() as u64 >= self.stale_timeout_secs
    }

    fn update(&mut self, price: Decimal, timestamp: DateTime<Utc>) {
        self.last_price = Some(price);
        self.last_update = timestamp;
        self.is_healthy = true;
    }
}

// ── Aggregator ────────────────────────────────────────────────────────────────

/// Aggregates prices from multiple TradFi sources into a single
/// manipulation-resistant price via the weighted median.
///
/// Pure computation — no I/O, no networking. The caller feeds price updates
/// via [`update_source_price`][Self::update_source_price].
pub struct TradFiPriceAggregator {
    /// `sources[symbol][source_name]` → `SourceState`
    sources: HashMap<String, HashMap<String, SourceState>>,
    /// Symbols tracked, in insertion order.
    symbols: Vec<String>,
    /// Minimum number of healthy sources required for a successful aggregation.
    min_sources: usize,
    /// Maximum fractional spread between min and max healthy source prices
    /// before the circuit breaker fires. Default: 2% (dec!(0.02)).
    max_deviation: Decimal,
}

impl TradFiPriceAggregator {
    /// Create an aggregator for the given symbols and source configurations.
    ///
    /// `min_sources` is the floor of healthy sources required before
    /// [`calculate_price`][Self::calculate_price] succeeds. Typically 1–2 for
    /// TradFi symbols where fewer high-quality sources exist.
    pub fn new(
        symbols: Vec<String>,
        source_configs: Vec<FeedSourceConfig>,
        min_sources: usize,
    ) -> Self {
        let mut sources: HashMap<String, HashMap<String, SourceState>> = HashMap::new();

        for symbol in &symbols {
            let mut source_map = HashMap::new();
            for cfg in &source_configs {
                source_map.insert(cfg.name.clone(), SourceState::from_config(cfg));
            }
            sources.insert(symbol.clone(), source_map);
        }

        Self {
            sources,
            symbols,
            min_sources,
            max_deviation: dec!(0.02), // 2% default
        }
    }

    /// Convenience constructor using the default three Phase-1 sources with
    /// `min_sources = 1`.
    ///
    /// `min_sources = 1` is intentional for TradFi: we have fewer sources than
    /// for crypto, and blocking all trading because one of two feeds is
    /// temporarily stale would be disruptive during market hours.
    pub fn with_defaults(symbols: Vec<String>) -> Self {
        Self::new(symbols, crate::feeds::defaults::all(), 1)
    }

    /// Override the maximum inter-source deviation before the circuit breaker
    /// fires.  `threshold` is a fraction (e.g. `dec!(0.02)` = 2%).
    pub fn set_max_deviation(&mut self, threshold: Decimal) {
        self.max_deviation = threshold;
    }

    /// Feed a new price observation for a `(symbol, source_name)` pair.
    ///
    /// Unknown symbols or source names are silently ignored — the caller does
    /// not need to coordinate exact naming at startup.
    pub fn update_source_price(
        &mut self,
        symbol: &str,
        source_name: &str,
        price: Decimal,
        timestamp: DateTime<Utc>,
    ) {
        let Some(source_map) = self.sources.get_mut(symbol) else {
            debug!(
                symbol,
                source = source_name,
                "Ignoring price update for untracked TradFi symbol"
            );
            return;
        };

        let Some(source) = source_map.get_mut(source_name) else {
            debug!(
                symbol,
                source = source_name,
                "Ignoring price update for unknown TradFi source"
            );
            return;
        };

        source.update(price, timestamp);
        debug!(
            symbol,
            source = source_name,
            %price,
            "TradFi source price updated"
        );
    }

    /// Compute the aggregated price for `symbol`.
    ///
    /// Returns [`AggregatorError::InsufficientSources`] when fewer healthy
    /// sources than `min_sources` are available, and
    /// [`AggregatorError::UnknownSymbol`] when the symbol is not tracked.
    ///
    /// The returned [`TradFiPrice`] carries a `circuit_breaker_triggered` flag
    /// that is set when the spread between the lowest and highest healthy
    /// source prices exceeds `max_deviation`.
    pub fn calculate_price(&self, symbol: &str) -> Result<TradFiPrice, AggregatorError> {
        let source_map = self
            .sources
            .get(symbol)
            .ok_or_else(|| AggregatorError::UnknownSymbol(symbol.to_string()))?;

        let now = Utc::now();

        let (healthy, stale_count) = Self::partition_healthy(source_map, now);

        if healthy.len() < self.min_sources {
            debug!(
                symbol,
                healthy = healthy.len(),
                stale = stale_count,
                min_sources = self.min_sources,
                "Insufficient healthy TradFi sources"
            );
            return Err(AggregatorError::InsufficientSources {
                symbol: symbol.to_string(),
                needed: self.min_sources,
                available: healthy.len(),
                stale: stale_count,
            });
        }

        let price_weights: Vec<(Decimal, Decimal)> = healthy
            .iter()
            .filter_map(|src| src.last_price.map(|p| (p, src.weight)))
            .collect();

        let price = weighted_median(&price_weights)
            .expect("weighted_median returned None despite non-empty healthy sources");

        // Circuit breaker: flag if spread across healthy sources exceeds threshold.
        let circuit_breaker_triggered =
            self.deviation_exceeds_threshold(&price_weights);

        if circuit_breaker_triggered {
            warn!(
                symbol,
                %price,
                sources = healthy.len(),
                max_deviation = %self.max_deviation,
                "TradFi circuit breaker triggered: inter-source deviation too large"
            );
        } else {
            debug!(
                symbol,
                %price,
                sources_used = healthy.len(),
                "TradFi price calculated"
            );
        }

        Ok(TradFiPrice {
            symbol: symbol.to_string(),
            price,
            sources_used: healthy.len(),
            circuit_breaker_triggered,
            timestamp: now,
        })
    }

    /// Compute aggregated prices for all tracked symbols.
    ///
    /// Symbols with insufficient data are skipped with a warning.
    pub fn get_all_prices(&self) -> HashMap<String, TradFiPrice> {
        self.symbols
            .iter()
            .filter_map(|symbol| match self.calculate_price(symbol) {
                Ok(p) => Some((symbol.clone(), p)),
                Err(e) => {
                    warn!(symbol = %symbol, error = %e, "Skipping symbol in get_all_prices");
                    None
                }
            })
            .collect()
    }

    /// Return the list of tracked symbols.
    pub fn symbols(&self) -> &[String] {
        &self.symbols
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    /// Separate sources for a symbol into healthy (price present and fresh) vs
    /// stale/unhealthy.
    fn partition_healthy<'a>(
        source_map: &'a HashMap<String, SourceState>,
        now: DateTime<Utc>,
    ) -> (Vec<&'a SourceState>, usize) {
        let mut healthy = Vec::new();
        let mut stale_count = 0usize;

        for state in source_map.values() {
            let stale = state.is_stale(now);
            let has_price = state.last_price.is_some();

            if state.is_healthy && !stale && has_price {
                healthy.push(state);
            } else if stale || !has_price {
                stale_count += 1;
            }
        }

        (healthy, stale_count)
    }

    /// Return `true` when the fractional spread between min and max prices in
    /// `price_weights` exceeds `self.max_deviation`.
    ///
    /// The spread is defined as `(max - min) / min`.
    fn deviation_exceeds_threshold(&self, price_weights: &[(Decimal, Decimal)]) -> bool {
        if price_weights.len() < 2 {
            return false; // Cannot compute spread with fewer than 2 sources.
        }

        let prices: Vec<Decimal> = price_weights.iter().map(|(p, _)| *p).collect();
        let min_price = prices.iter().copied().min().expect("non-empty");
        let max_price = prices.iter().copied().max().expect("non-empty");

        if min_price == Decimal::ZERO {
            return true; // Guard against division by zero.
        }

        let spread = (max_price - min_price) / min_price;
        spread > self.max_deviation
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::feeds::FeedSourceConfig;

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    fn three_source_configs() -> Vec<FeedSourceConfig> {
        vec![
            FeedSourceConfig::new("PythSim",   dec!(0.50), 30),
            FeedSourceConfig::new("ForexFeed", dec!(0.30), 30),
            FeedSourceConfig::new("CmdtyFeed", dec!(0.20), 30),
        ]
    }

    fn xau_agg() -> TradFiPriceAggregator {
        TradFiPriceAggregator::new(
            vec!["XAUUSD".to_string()],
            three_source_configs(),
            1,
        )
    }

    // ── Basic aggregation ────────────────────────────────────────────────────

    #[test]
    fn single_healthy_source_returns_price() {
        let mut agg = xau_agg();
        agg.update_source_price("XAUUSD", "PythSim", dec!(2350), now());
        let p = agg.calculate_price("XAUUSD").unwrap();
        assert_eq!(p.price, dec!(2350));
        assert_eq!(p.sources_used, 1);
        assert!(!p.circuit_breaker_triggered);
    }

    #[test]
    fn three_healthy_sources_uses_weighted_median() {
        let mut agg = xau_agg();
        let t = now();
        agg.update_source_price("XAUUSD", "PythSim",   dec!(2350), t);
        agg.update_source_price("XAUUSD", "ForexFeed", dec!(2351), t);
        agg.update_source_price("XAUUSD", "CmdtyFeed", dec!(2349), t);
        let p = agg.calculate_price("XAUUSD").unwrap();
        assert_eq!(p.sources_used, 3);
        // Weighted median with weights 0.5, 0.3, 0.2 on prices 2349, 2350, 2351:
        // Sorted: (2349, 0.2), (2350, 0.5), (2351, 0.3). Half = 0.5.
        // Cum at 2349 = 0.2 < 0.5; cum at 2350 = 0.7 >= 0.5 → 2350.
        assert_eq!(p.price, dec!(2350));
    }

    // ── Staleness detection ──────────────────────────────────────────────────

    #[test]
    fn stale_source_excluded_from_aggregation() {
        let configs = vec![
            FeedSourceConfig::new("Fresh1", dec!(0.5), 30),
            FeedSourceConfig::new("Fresh2", dec!(0.5), 30),
            FeedSourceConfig::new("Stale",  dec!(0.5), 30),
        ];
        let mut agg = TradFiPriceAggregator::new(
            vec!["XAUUSD".to_string()],
            configs,
            1,
        );
        let t = now();
        // Stale source: timestamp is 60 s in the past (beyond 30 s timeout).
        agg.update_source_price("XAUUSD", "Stale",  dec!(9999), t - Duration::seconds(60));
        agg.update_source_price("XAUUSD", "Fresh1", dec!(2350), t);
        agg.update_source_price("XAUUSD", "Fresh2", dec!(2351), t);

        let p = agg.calculate_price("XAUUSD").unwrap();
        assert_eq!(p.sources_used, 2);
        // Price must not be near the stale outlier at 9999.
        assert!(p.price < dec!(3000));
    }

    #[test]
    fn all_sources_stale_returns_error_when_min_is_1() {
        let configs = vec![FeedSourceConfig::new("A", dec!(1.0), 10)];
        let mut agg = TradFiPriceAggregator::new(
            vec!["EURUSD".to_string()],
            configs,
            1,
        );
        // Update with a timestamp older than the 10 s timeout.
        agg.update_source_price("EURUSD", "A", dec!(1.085), now() - Duration::seconds(20));
        assert!(matches!(
            agg.calculate_price("EURUSD").unwrap_err(),
            AggregatorError::InsufficientSources { available: 0, .. }
        ));
    }

    // ── Unknown symbol / source ──────────────────────────────────────────────

    #[test]
    fn unknown_symbol_returns_error() {
        let agg = xau_agg();
        assert!(matches!(
            agg.calculate_price("UNKNOWNSYM").unwrap_err(),
            AggregatorError::UnknownSymbol(_)
        ));
    }

    #[test]
    fn update_unknown_symbol_is_silently_ignored() {
        let mut agg = xau_agg();
        // Must not panic.
        agg.update_source_price("NOSUCHSYM", "PythSim", dec!(2350), now());
    }

    #[test]
    fn update_unknown_source_is_silently_ignored() {
        let mut agg = xau_agg();
        // Must not panic.
        agg.update_source_price("XAUUSD", "NonExistentFeed", dec!(2350), now());
    }

    // ── Circuit breaker (deviation) ──────────────────────────────────────────

    #[test]
    fn circuit_breaker_not_triggered_when_feeds_agree() {
        let mut agg = xau_agg();
        let t = now();
        // Prices differ by <0.1% — well within the 2% default threshold.
        agg.update_source_price("XAUUSD", "PythSim",   dec!(2350.00), t);
        agg.update_source_price("XAUUSD", "ForexFeed", dec!(2350.50), t);
        agg.update_source_price("XAUUSD", "CmdtyFeed", dec!(2349.80), t);

        let p = agg.calculate_price("XAUUSD").unwrap();
        assert!(!p.circuit_breaker_triggered);
    }

    #[test]
    fn circuit_breaker_triggered_when_feeds_diverge() {
        let mut agg = xau_agg();
        let t = now();
        // One feed reports a price 5% above the others — exceeds 2% threshold.
        agg.update_source_price("XAUUSD", "PythSim",   dec!(2350.00), t);
        agg.update_source_price("XAUUSD", "ForexFeed", dec!(2350.00), t);
        agg.update_source_price("XAUUSD", "CmdtyFeed", dec!(2467.50), t); // +5%

        let p = agg.calculate_price("XAUUSD").unwrap();
        assert!(p.circuit_breaker_triggered);
    }

    #[test]
    fn circuit_breaker_uses_custom_threshold() {
        let mut agg = xau_agg();
        // Tighten the threshold to 0.1% so a 0.5% spread triggers it.
        agg.set_max_deviation(dec!(0.001));
        let t = now();
        agg.update_source_price("XAUUSD", "PythSim",   dec!(2350.00), t);
        agg.update_source_price("XAUUSD", "ForexFeed", dec!(2361.75), t); // +0.5%

        let p = agg.calculate_price("XAUUSD").unwrap();
        assert!(p.circuit_breaker_triggered);
    }

    #[test]
    fn single_source_never_triggers_circuit_breaker() {
        let mut agg = xau_agg();
        agg.update_source_price("XAUUSD", "PythSim", dec!(2350), now());
        let p = agg.calculate_price("XAUUSD").unwrap();
        // Cannot compute spread with 1 source — circuit breaker must remain off.
        assert!(!p.circuit_breaker_triggered);
    }

    // ── Multi-symbol ─────────────────────────────────────────────────────────

    #[test]
    fn multi_symbol_tracked_independently() {
        let mut agg = TradFiPriceAggregator::with_defaults(vec![
            "XAUUSD".to_string(),
            "EURUSD".to_string(),
        ]);
        let t = now();
        agg.update_source_price("XAUUSD", "PythSim", dec!(2350), t);
        agg.update_source_price("EURUSD", "PythSim", dec!(1.085), t);

        let xau = agg.calculate_price("XAUUSD").unwrap();
        let eur = agg.calculate_price("EURUSD").unwrap();

        assert!(xau.price > dec!(2000));
        assert!(eur.price > dec!(1.0) && eur.price < dec!(2.0));
    }

    #[test]
    fn get_all_prices_skips_symbols_with_no_data() {
        let mut agg = TradFiPriceAggregator::with_defaults(vec![
            "XAUUSD".to_string(),
            "EURUSD".to_string(),
        ]);
        // Only feed XAUUSD — EURUSD has no data.
        agg.update_source_price("XAUUSD", "PythSim", dec!(2350), now());
        let all = agg.get_all_prices();
        assert!(all.contains_key("XAUUSD"));
        assert!(!all.contains_key("EURUSD"));
    }

    // ── Insufficient sources (min > 1) ────────────────────────────────────────

    #[test]
    fn min_sources_two_requires_two_healthy() {
        let mut agg = TradFiPriceAggregator::new(
            vec!["SPXUSD".to_string()],
            three_source_configs(),
            2,
        );
        // Only one source fed — below minimum.
        agg.update_source_price("SPXUSD", "PythSim", dec!(5400), now());
        assert!(matches!(
            agg.calculate_price("SPXUSD").unwrap_err(),
            AggregatorError::InsufficientSources { needed: 2, available: 1, .. }
        ));
        // Feed a second source — now meets the minimum.
        agg.update_source_price("SPXUSD", "CmdtyFeed", dec!(5398), now());
        assert!(agg.calculate_price("SPXUSD").is_ok());
    }
}
