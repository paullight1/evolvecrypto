//! Feed adapters for TradFi price sources.
//!
//! Each adapter implements [`TradFiFeed`] and produces [`TradFiRawPrice`] values
//! that the [`crate::aggregator::TradFiPriceAggregator`] consumes.
//!
//! Phase-1 implementations are fully simulated — they return deterministic
//! prices with a small constant offset to emulate multi-source disagreement.
//! A real Pyth Network WebSocket adapter (and a REST forex aggregator) will
//! replace these in Phase 2 without changing the aggregator interface.

pub mod commodity_feed;
pub mod forex_feed;
pub mod pyth_sim;

use anyhow::Result;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

// ── Configuration ────────────────────────────────────────────────────────────

/// Configuration for a single TradFi price source.
///
/// Mirrors `PriceSourceConfig` in `exchange-index-price` so the aggregator can
/// apply identical weighted-median logic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedSourceConfig {
    /// Human-readable name, e.g. `"PythSim"`, `"ForexFeed"`.
    pub name: String,
    /// Relative weight in the weighted median. All weights should sum to ≈1.0.
    pub weight: Decimal,
    /// Age in seconds after which a price update is considered stale.
    pub stale_timeout_secs: u64,
}

impl FeedSourceConfig {
    pub fn new(name: impl Into<String>, weight: Decimal, stale_timeout_secs: u64) -> Self {
        Self {
            name: name.into(),
            weight,
            stale_timeout_secs,
        }
    }
}

/// Pre-configured default sources for TradFi symbols (Phase 1: all simulated).
pub mod defaults {
    use rust_decimal_macros::dec;

    use super::FeedSourceConfig;

    /// Simulated Pyth Network feed — 50% weight, highest authority in Phase 1.
    pub fn pyth_sim() -> FeedSourceConfig {
        FeedSourceConfig::new("PythSim", dec!(0.50), 30)
    }

    /// Simulated REST forex/commodity feed — 30% weight.
    pub fn forex_feed() -> FeedSourceConfig {
        FeedSourceConfig::new("ForexFeed", dec!(0.30), 30)
    }

    /// Simulated commodity/equity feed — 20% weight.
    pub fn commodity_feed() -> FeedSourceConfig {
        FeedSourceConfig::new("CmdtyFeed", dec!(0.20), 30)
    }

    /// The full default set (weights sum to 1.0).
    pub fn all() -> Vec<FeedSourceConfig> {
        vec![pyth_sim(), forex_feed(), commodity_feed()]
    }
}

// ── Raw price ────────────────────────────────────────────────────────────────

/// A raw price observation from a single feed adapter.
#[derive(Debug, Clone)]
pub struct TradFiRawPrice {
    /// Normalised symbol (e.g. `"XAUUSD"`, `"EURUSD"`).
    pub symbol: String,
    /// Mid / last price in USD.
    pub price: Decimal,
    /// When the price was obtained (UTC).
    pub timestamp: DateTime<Utc>,
    /// Feed adapter name — must match the `FeedSourceConfig::name` in the aggregator.
    pub source: String,
    /// One-sided confidence radius (e.g. Pyth's σ). Zero means no confidence data.
    pub confidence: Decimal,
}

// ── Feed trait ───────────────────────────────────────────────────────────────

/// A TradFi price feed adapter.
///
/// Implementations are responsible for fetching (or simulating) prices and
/// returning them as [`TradFiRawPrice`] values.  The aggregator calls
/// [`get_price`] on each registered feed.
///
/// Async functions in traits are supported natively in Rust edition 2024.
pub trait TradFiFeed: Send + Sync {
    /// Fetch the current price for `symbol`.
    ///
    /// Returns `Err` when the feed is unavailable or does not cover the symbol.
    fn get_price(
        &self,
        symbol: &str,
    ) -> impl std::future::Future<Output = Result<TradFiRawPrice>> + Send;

    /// Human-readable feed adapter name.
    fn name(&self) -> &str;

    /// Symbols this adapter can service.
    fn supported_symbols(&self) -> Vec<String>;
}
