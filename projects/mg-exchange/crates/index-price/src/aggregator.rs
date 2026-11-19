use std::collections::HashMap;

use chrono::{DateTime, Utc};
use exchange_common::{Price, Symbol};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, warn};

use crate::{
    source::{PriceSource, PriceSourceConfig, SourceStatus},
    weighted_median::weighted_median,
};

/// Errors that can occur during index price calculation.
#[derive(Debug, Error)]
pub enum IndexError {
    /// Fewer healthy (non-stale) sources than `min_sources` are available.
    #[error(
        "Insufficient healthy sources for {symbol}: need {needed}, have {available} (stale: {stale})"
    )]
    InsufficientSources {
        symbol: String,
        needed: usize,
        available: usize,
        stale: usize,
    },

    /// The requested symbol is not tracked by this aggregator.
    #[error("Symbol not tracked: {0}")]
    UnknownSymbol(String),
}

/// A computed index price along with metadata about how it was derived.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexPrice {
    pub symbol: Symbol,
    /// The weighted-median price across all healthy sources.
    pub price: Price,
    /// Number of healthy sources that contributed to this price.
    pub sources_used: usize,
    pub timestamp: DateTime<Utc>,
}

/// Aggregates spot prices from multiple external sources into a single
/// manipulation-resistant index price via the weighted median.
///
/// This struct is pure computation — no I/O, no networking.
/// The caller is responsible for feeding price updates via [`update_source_price`].
pub struct IndexPriceAggregator {
    /// `sources[symbol][source_name]` → `PriceSource`
    sources: HashMap<Symbol, HashMap<String, PriceSource>>,
    /// Symbols being tracked, in insertion order.
    symbols: Vec<Symbol>,
    /// Minimum number of healthy sources required to compute an index price.
    min_sources: usize,
}

impl IndexPriceAggregator {
    /// Create an aggregator tracking `symbols`, using the provided source configs for each.
    ///
    /// `min_sources` is the floor of healthy sources required before `calculate_index` succeeds.
    /// Defaults to 2 when not explicitly set.
    pub fn new(
        symbols: Vec<Symbol>,
        source_configs: Vec<PriceSourceConfig>,
        min_sources: usize,
    ) -> Self {
        let mut sources: HashMap<Symbol, HashMap<String, PriceSource>> = HashMap::new();

        for symbol in &symbols {
            let mut source_map = HashMap::new();
            for cfg in &source_configs {
                source_map.insert(cfg.name.clone(), PriceSource::from_config(cfg));
            }
            sources.insert(symbol.clone(), source_map);
        }

        Self {
            sources,
            symbols,
            min_sources,
        }
    }

    /// Convenience constructor using the four default exchange configs with `min_sources = 3`.
    ///
    /// With `min_sources = 2`, an attacker who DDoSes 2 of 4 sources controls the
    /// weighted median. Raising to 3 requires compromising 3 sources simultaneously,
    /// which is significantly harder.
    pub fn with_defaults(symbols: Vec<Symbol>) -> Self {
        Self::new(
            symbols,
            crate::source::defaults::all(),
            3,
        )
    }

    /// Feed a new price for a (symbol, source) pair.
    ///
    /// Silently ignores updates for unknown symbols or source names so that
    /// callers don't need to coordinate exact naming at startup.
    pub fn update_source_price(
        &mut self,
        symbol: &Symbol,
        source_name: &str,
        price: Price,
        timestamp: DateTime<Utc>,
    ) {
        let Some(source_map) = self.sources.get_mut(symbol) else {
            debug!(
                symbol = %symbol,
                source = source_name,
                "Ignoring price update for untracked symbol"
            );
            return;
        };

        let Some(source) = source_map.get_mut(source_name) else {
            debug!(
                symbol = %symbol,
                source = source_name,
                "Ignoring price update for unknown source"
            );
            return;
        };

        source.update_price(price, timestamp);
        debug!(
            symbol = %symbol,
            source = source_name,
            %price,
            "Price source updated"
        );
    }

    /// Compute the index price for `symbol`.
    ///
    /// Filters stale sources, checks the minimum source requirement, then
    /// applies the weighted median algorithm.
    pub fn calculate_index(&self, symbol: &Symbol) -> Result<IndexPrice, IndexError> {
        let source_map = self
            .sources
            .get(symbol)
            .ok_or_else(|| IndexError::UnknownSymbol(symbol.to_string()))?;

        let now = Utc::now();

        let (healthy, stale_count) = self.partition_sources(source_map, now);

        if healthy.len() < self.min_sources {
            debug!(
                symbol = %symbol,
                healthy = healthy.len(),
                stale = stale_count,
                min_sources = self.min_sources,
                "Insufficient healthy sources"
            );
            return Err(IndexError::InsufficientSources {
                symbol: symbol.to_string(),
                needed: self.min_sources,
                available: healthy.len(),
                stale: stale_count,
            });
        }

        // Build the (price, weight) pairs for the weighted median.
        let price_weights: Vec<(Decimal, Decimal)> = healthy
            .iter()
            .filter_map(|(src, _)| src.last_price.map(|p| (p, src.weight)))
            .collect();

        // `healthy` already excludes sources with no price, but guard anyway.
        let price = weighted_median(&price_weights)
            .expect("weighted_median returned None despite non-empty healthy sources");

        debug!(
            symbol = %symbol,
            %price,
            sources_used = healthy.len(),
            "Index price calculated"
        );

        Ok(IndexPrice {
            symbol: symbol.clone(),
            price,
            sources_used: healthy.len(),
            timestamp: now,
        })
    }

    /// Compute index prices for all tracked symbols, returning only the successful ones.
    ///
    /// Symbols with insufficient data are skipped and a warning is logged.
    pub fn get_all_indices(&self) -> HashMap<Symbol, IndexPrice> {
        self.symbols
            .iter()
            .filter_map(|symbol| match self.calculate_index(symbol) {
                Ok(index) => Some((symbol.clone(), index)),
                Err(e) => {
                    warn!(symbol = %symbol, error = %e, "Skipping symbol in get_all_indices");
                    None
                }
            })
            .collect()
    }

    /// Return health status for every source of a given symbol.
    pub fn source_status(&self, symbol: &Symbol) -> Vec<SourceStatus> {
        let now = Utc::now();
        match self.sources.get(symbol) {
            None => vec![],
            Some(source_map) => source_map.values().map(|s| s.status(now)).collect(),
        }
    }

    /// Returns the list of symbols this aggregator tracks.
    pub fn symbols(&self) -> &[Symbol] {
        &self.symbols
    }

    /// Register a new symbol for index price tracking at runtime.
    ///
    /// Initialises source entries using the default exchange configs (Binance,
    /// Coinbase, Kraken, OKX).  Prices will flow in from the next feed update.
    /// No-op if the symbol is already tracked.
    pub fn add_symbol(&mut self, symbol: Symbol) {
        if self.sources.contains_key(&symbol) {
            return;
        }
        let configs = crate::source::defaults::all();
        let mut source_map = HashMap::new();
        for cfg in &configs {
            source_map.insert(cfg.name.clone(), PriceSource::from_config(cfg));
        }
        self.sources.insert(symbol.clone(), source_map);
        self.symbols.push(symbol);
    }

    /// Maximum deviation fraction from the median before a source is excluded.
    /// 5% = 0.05. A source deviating >5% from the median of other sources is
    /// treated as potentially compromised and excluded from the index calculation.
    const MAX_DEVIATION_FRACTION: Decimal = Decimal::from_parts(5, 0, 0, false, 2); // 0.05

    /// Separates sources for a symbol into (healthy_with_price, stale_count).
    ///
    /// A source is considered healthy when:
    /// - `is_healthy` flag is set (received at least one update), AND
    /// - not currently stale (within `stale_timeout_secs`), AND
    /// - has a price value, AND
    /// - not deviating >5% from the median of other sources (outlier exclusion).
    fn partition_sources<'a>(
        &self,
        source_map: &'a HashMap<String, PriceSource>,
        now: DateTime<Utc>,
    ) -> (Vec<(&'a PriceSource, ())>, usize) {
        let mut candidates: Vec<&'a PriceSource> = Vec::new();
        let mut stale_count = 0usize;

        for source in source_map.values() {
            let stale = source.is_stale(now);
            let has_price = source.last_price.is_some();

            if source.is_healthy && !stale && has_price {
                candidates.push(source);
            } else {
                if stale || !has_price {
                    stale_count += 1;
                }
            }
        }

        // Outlier exclusion: if we have >=3 candidates, compute median and
        // exclude any source deviating >5% from the median.
        if candidates.len() >= 3 {
            let mut prices: Vec<Decimal> = candidates
                .iter()
                .filter_map(|s| s.last_price)
                .collect();
            prices.sort();

            let median = prices[prices.len() / 2];

            if !median.is_zero() {
                let mut healthy = Vec::new();
                for src in &candidates {
                    if let Some(p) = src.last_price {
                        let deviation = ((p - median) / median).abs();
                        if deviation > Self::MAX_DEVIATION_FRACTION {
                            warn!(
                                source = %src.name,
                                price = %p,
                                median = %median,
                                deviation_pct = %(&deviation * Decimal::ONE_HUNDRED),
                                "Index source excluded: deviation exceeds 5% threshold"
                            );
                            stale_count += 1; // Count as excluded
                        } else {
                            healthy.push((*src, ()));
                        }
                    }
                }
                return (healthy, stale_count);
            }
        }

        // Not enough candidates for outlier detection — return all candidates.
        let healthy = candidates.into_iter().map(|s| (s, ())).collect();
        (healthy, stale_count)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::source::{PriceSourceConfig, defaults};

    fn btc() -> Symbol {
        Symbol::new("BTCUSDT")
    }

    fn eth() -> Symbol {
        Symbol::new("ETHUSDT")
    }

    fn default_aggregator() -> IndexPriceAggregator {
        IndexPriceAggregator::with_defaults(vec![btc()])
    }

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    // ── Update and retrieval ────────────────────────────────────────────────

    #[test]
    fn update_and_calculate_basic() {
        let mut agg = default_aggregator();
        let t = now();
        agg.update_source_price(&btc(), "Binance", dec!(50000), t);
        agg.update_source_price(&btc(), "Coinbase", dec!(50100), t);
        agg.update_source_price(&btc(), "Kraken", dec!(49950), t);

        let index = agg.calculate_index(&btc()).unwrap();
        assert_eq!(index.sources_used, 3);
        // Weighted median of sorted [(49950,0.20),(50000,0.40),(50100,0.30)] with half=0.45:
        // cum at 49950 = 0.20 < 0.45; cum at 50000 = 0.60 ≥ 0.45 → 50000
        assert_eq!(index.price, dec!(50000));
    }

    #[test]
    fn all_four_sources_healthy() {
        let mut agg = default_aggregator();
        let t = now();
        agg.update_source_price(&btc(), "Binance", dec!(100), t);
        agg.update_source_price(&btc(), "Coinbase", dec!(101), t);
        agg.update_source_price(&btc(), "Kraken", dec!(99), t);
        agg.update_source_price(&btc(), "OKX", dec!(102), t);

        let index = agg.calculate_index(&btc()).unwrap();
        assert_eq!(index.sources_used, 4);
    }

    // ── Insufficient sources ────────────────────────────────────────────────

    #[test]
    fn no_sources_returns_error() {
        let agg = default_aggregator();
        let err = agg.calculate_index(&btc()).unwrap_err();
        assert!(matches!(
            err,
            IndexError::InsufficientSources { available: 0, .. }
        ));
    }

    #[test]
    fn one_source_below_min_returns_error() {
        let mut agg = default_aggregator(); // min_sources = 3
        agg.update_source_price(&btc(), "Binance", dec!(50000), now());
        let err = agg.calculate_index(&btc()).unwrap_err();
        assert!(matches!(
            err,
            IndexError::InsufficientSources { available: 1, .. }
        ));
    }

    #[test]
    fn two_sources_below_min_returns_error() {
        let mut agg = default_aggregator(); // min_sources = 3
        let t = now();
        agg.update_source_price(&btc(), "Binance", dec!(50000), t);
        agg.update_source_price(&btc(), "Coinbase", dec!(50100), t);
        let err = agg.calculate_index(&btc()).unwrap_err();
        assert!(matches!(
            err,
            IndexError::InsufficientSources { available: 2, needed: 3, .. }
        ));
    }

    #[test]
    fn exactly_min_sources_succeeds() {
        let mut agg = default_aggregator(); // min_sources = 3
        let t = now();
        agg.update_source_price(&btc(), "Binance", dec!(50000), t);
        agg.update_source_price(&btc(), "Coinbase", dec!(50100), t);
        agg.update_source_price(&btc(), "Kraken", dec!(49950), t);
        assert!(agg.calculate_index(&btc()).is_ok());
    }

    // ── Stale detection ─────────────────────────────────────────────────────

    #[test]
    fn stale_source_excluded_from_calculation() {
        // stale_timeout_secs = 5 for all test configs
        let configs = vec![
            PriceSourceConfig::new("Fresh1", dec!(0.5), 5),
            PriceSourceConfig::new("Fresh2", dec!(0.5), 5),
            PriceSourceConfig::new("Stale", dec!(0.5), 5),
        ];
        let mut agg = IndexPriceAggregator::new(vec![btc()], configs, 2);
        let now_t = now();

        // Stale source gets a price from 10s ago.
        agg.update_source_price(&btc(), "Stale", dec!(99999), now_t - Duration::seconds(10));
        agg.update_source_price(&btc(), "Fresh1", dec!(100), now_t);
        agg.update_source_price(&btc(), "Fresh2", dec!(102), now_t);

        let index = agg.calculate_index(&btc()).unwrap();
        // Only 2 fresh sources used; stale extreme outlier excluded.
        assert_eq!(index.sources_used, 2);
        // Price should be in the 100–102 range, not 99999.
        assert!(index.price < dec!(200));
    }

    #[test]
    fn all_sources_stale_returns_error() {
        let configs = vec![
            PriceSourceConfig::new("A", dec!(0.5), 5),
            PriceSourceConfig::new("B", dec!(0.5), 5),
        ];
        let mut agg = IndexPriceAggregator::new(vec![btc()], configs, 2);
        let old_time = now() - Duration::seconds(60);
        agg.update_source_price(&btc(), "A", dec!(100), old_time);
        agg.update_source_price(&btc(), "B", dec!(101), old_time);

        assert!(matches!(
            agg.calculate_index(&btc()).unwrap_err(),
            IndexError::InsufficientSources { available: 0, .. }
        ));
    }

    // ── Multi-symbol ────────────────────────────────────────────────────────

    #[test]
    fn multi_symbol_independent_tracking() {
        let mut agg = IndexPriceAggregator::with_defaults(vec![btc(), eth()]);
        let t = now();
        agg.update_source_price(&btc(), "Binance", dec!(50000), t);
        agg.update_source_price(&btc(), "Coinbase", dec!(50100), t);
        agg.update_source_price(&btc(), "Kraken", dec!(49950), t);
        agg.update_source_price(&eth(), "Binance", dec!(3000), t);
        agg.update_source_price(&eth(), "Coinbase", dec!(3010), t);
        agg.update_source_price(&eth(), "Kraken", dec!(2995), t);

        let btc_idx = agg.calculate_index(&btc()).unwrap();
        let eth_idx = agg.calculate_index(&eth()).unwrap();

        assert!(btc_idx.price > dec!(49000));
        assert!(eth_idx.price > dec!(2900));
        assert!(btc_idx.price > eth_idx.price);
    }

    #[test]
    fn get_all_indices_skips_insufficient_symbols() {
        let mut agg = IndexPriceAggregator::with_defaults(vec![btc(), eth()]);
        let t = now();
        // Only feed BTC with 3 sources — ETH has no data.
        agg.update_source_price(&btc(), "Binance", dec!(50000), t);
        agg.update_source_price(&btc(), "Coinbase", dec!(50100), t);
        agg.update_source_price(&btc(), "Kraken", dec!(49950), t);

        let all = agg.get_all_indices();
        assert!(all.contains_key(&btc()));
        assert!(!all.contains_key(&eth()));
    }

    // ── Unknown symbol ──────────────────────────────────────────────────────

    #[test]
    fn unknown_symbol_returns_error() {
        let agg = default_aggregator();
        let err = agg.calculate_index(&Symbol::new("UNKNOWN")).unwrap_err();
        assert!(matches!(err, IndexError::UnknownSymbol(_)));
    }

    #[test]
    fn update_unknown_symbol_is_silently_ignored() {
        let mut agg = default_aggregator();
        // Should not panic.
        agg.update_source_price(&Symbol::new("UNKNOWN"), "Binance", dec!(100), now());
    }

    #[test]
    fn update_unknown_source_is_silently_ignored() {
        let mut agg = default_aggregator();
        // Should not panic.
        agg.update_source_price(&btc(), "NonExistentExchange", dec!(100), now());
    }

    // ── Source status ───────────────────────────────────────────────────────

    #[test]
    fn source_status_reflects_health() {
        let mut agg = default_aggregator();
        agg.update_source_price(&btc(), "Binance", dec!(50000), now());

        let statuses = agg.source_status(&btc());
        let binance = statuses.iter().find(|s| s.name == "Binance").unwrap();
        assert!(binance.is_healthy);
        assert!(!binance.is_stale);

        // Other sources have no price yet.
        let coinbase = statuses.iter().find(|s| s.name == "Coinbase").unwrap();
        assert!(!coinbase.is_healthy);
    }

    #[test]
    fn source_status_empty_for_unknown_symbol() {
        let agg = default_aggregator();
        let statuses = agg.source_status(&Symbol::new("UNKNOWN"));
        assert!(statuses.is_empty());
    }

    // ── Outlier resistance ──────────────────────────────────────────────────

    #[test]
    fn extreme_outlier_source_does_not_skew_index() {
        let mut agg = default_aggregator();
        let t = now();
        agg.update_source_price(&btc(), "Binance", dec!(50000), t);  // 0.40
        agg.update_source_price(&btc(), "Coinbase", dec!(50100), t); // 0.30
        agg.update_source_price(&btc(), "Kraken", dec!(49900), t);   // 0.20
        agg.update_source_price(&btc(), "OKX", dec!(200000), t);     // 0.10 — extreme

        let index = agg.calculate_index(&btc()).unwrap();
        // Weighted median must stay in the normal range, not shifted by OKX outlier.
        assert!(index.price < dec!(55000));
    }

    // ── Custom min_sources ──────────────────────────────────────────────────

    #[test]
    fn custom_min_sources_three_required() {
        let configs = defaults::all();
        let mut agg = IndexPriceAggregator::new(vec![btc()], configs, 3);
        let t = now();
        agg.update_source_price(&btc(), "Binance", dec!(50000), t);
        agg.update_source_price(&btc(), "Coinbase", dec!(50100), t);
        // Only 2 healthy — below min of 3.
        assert!(matches!(
            agg.calculate_index(&btc()).unwrap_err(),
            IndexError::InsufficientSources { needed: 3, available: 2, .. }
        ));
        // Add third source — now passes.
        agg.update_source_price(&btc(), "Kraken", dec!(49900), t);
        assert!(agg.calculate_index(&btc()).is_ok());
    }

    // ── Runtime symbol registration ─────────────────────────────────────────

    #[test]
    fn test_add_symbol_runtime() {
        let mut agg = IndexPriceAggregator::with_defaults(vec![Symbol::new("BTCUSDT")]);
        assert_eq!(agg.symbols().len(), 1);

        // Add new symbol at runtime
        agg.add_symbol(Symbol::new("AVAXUSDT"));
        assert_eq!(agg.symbols().len(), 2);

        // Feed 3 prices to meet min_sources = 3
        let now = chrono::Utc::now();
        agg.update_source_price(&Symbol::new("AVAXUSDT"), "Binance", dec!(35), now);
        agg.update_source_price(&Symbol::new("AVAXUSDT"), "Coinbase", dec!(35), now);
        agg.update_source_price(&Symbol::new("AVAXUSDT"), "Kraken", dec!(35), now);

        let result = agg.calculate_index(&Symbol::new("AVAXUSDT"));
        assert!(result.is_ok(), "dynamically added symbol must compute index");
    }

    // ── Outlier exclusion ─────────────────────────────────────────────────

    #[test]
    fn deviant_source_excluded_by_5pct_rule() {
        // Source at 2x price (100% deviation) must be excluded.
        let mut agg = default_aggregator();
        let t = now();
        agg.update_source_price(&btc(), "Binance", dec!(50000), t);  // normal
        agg.update_source_price(&btc(), "Coinbase", dec!(50100), t); // normal
        agg.update_source_price(&btc(), "Kraken", dec!(49900), t);   // normal
        agg.update_source_price(&btc(), "OKX", dec!(100000), t);     // 2x price — deviant

        let index = agg.calculate_index(&btc()).unwrap();
        // OKX should be excluded, leaving 3 normal sources.
        assert_eq!(index.sources_used, 3, "deviant source must be excluded");
        assert!(index.price < dec!(51000), "index must not be skewed by deviant: {}", index.price);
    }

    #[test]
    fn source_at_4pct_deviation_not_excluded() {
        // 4% deviation is under the 5% threshold — should NOT be excluded.
        let mut agg = default_aggregator();
        let t = now();
        agg.update_source_price(&btc(), "Binance", dec!(50000), t);
        agg.update_source_price(&btc(), "Coinbase", dec!(50100), t);
        agg.update_source_price(&btc(), "Kraken", dec!(49900), t);
        // OKX at ~4% above median ≈ 52000
        agg.update_source_price(&btc(), "OKX", dec!(52000), t);

        let index = agg.calculate_index(&btc()).unwrap();
        // 4% deviation is within threshold — all 4 sources used.
        assert_eq!(index.sources_used, 4, "4% deviation should not trigger exclusion");
    }

    #[test]
    fn all_deviant_except_two_returns_insufficient_error() {
        // If outlier exclusion drops sources below min_sources, should fail.
        let configs = vec![
            PriceSourceConfig::new("A", dec!(0.25), 30),
            PriceSourceConfig::new("B", dec!(0.25), 30),
            PriceSourceConfig::new("C", dec!(0.25), 30),
            PriceSourceConfig::new("D", dec!(0.25), 30),
        ];
        let mut agg = IndexPriceAggregator::new(vec![btc()], configs, 3);
        let t = now();
        agg.update_source_price(&btc(), "A", dec!(50000), t);
        agg.update_source_price(&btc(), "B", dec!(50100), t);
        // C and D are wild outliers
        agg.update_source_price(&btc(), "C", dec!(100000), t);
        agg.update_source_price(&btc(), "D", dec!(200000), t);

        let err = agg.calculate_index(&btc()).unwrap_err();
        assert!(matches!(err, IndexError::InsufficientSources { .. }));
    }

    #[test]
    fn test_add_symbol_idempotent() {
        let mut agg = IndexPriceAggregator::with_defaults(vec![Symbol::new("BTCUSDT")]);
        agg.add_symbol(Symbol::new("BTCUSDT")); // already exists
        assert_eq!(agg.symbols().len(), 1, "duplicate add_symbol must be no-op");
    }
}
