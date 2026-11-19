use chrono::{DateTime, Utc};
use exchange_common::Price;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Configuration for a price source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceSourceConfig {
    /// Human-readable name (e.g., "Binance").
    pub name: String,
    /// Relative weight in the weighted median. All weights should sum to 1.0 across sources.
    pub weight: Decimal,
    /// Age in seconds after which a price update is considered stale.
    pub stale_timeout_secs: u64,
}

impl PriceSourceConfig {
    pub fn new(name: impl Into<String>, weight: Decimal, stale_timeout_secs: u64) -> Self {
        Self {
            name: name.into(),
            weight,
            stale_timeout_secs,
        }
    }
}

/// Pre-defined configurations for well-known exchanges.
pub mod defaults {
    use rust_decimal_macros::dec;

    use super::PriceSourceConfig;

    /// Binance: highest liquidity, 40% weight.
    pub fn binance() -> PriceSourceConfig {
        PriceSourceConfig::new("Binance", dec!(0.40), 30)
    }

    /// Coinbase: reputable US exchange, 30% weight.
    pub fn coinbase() -> PriceSourceConfig {
        PriceSourceConfig::new("Coinbase", dec!(0.30), 30)
    }

    /// Kraken: European liquidity, 20% weight.
    pub fn kraken() -> PriceSourceConfig {
        PriceSourceConfig::new("Kraken", dec!(0.20), 30)
    }

    /// OKX: Asian liquidity, 10% weight.
    pub fn okx() -> PriceSourceConfig {
        PriceSourceConfig::new("OKX", dec!(0.10), 30)
    }

    /// Returns the full default set of four sources.
    pub fn all() -> Vec<PriceSourceConfig> {
        vec![binance(), coinbase(), kraken(), okx()]
    }
}

/// A live price source, tracking the latest price update and health state.
#[derive(Debug, Clone)]
pub struct PriceSource {
    pub name: String,
    pub weight: Decimal,
    pub last_price: Option<Price>,
    pub last_update: DateTime<Utc>,
    pub is_healthy: bool,
    pub stale_timeout_secs: u64,
}

impl PriceSource {
    /// Create a new source from config. `is_healthy` starts as false until first price arrives.
    pub fn from_config(config: &PriceSourceConfig) -> Self {
        Self {
            name: config.name.clone(),
            weight: config.weight,
            last_price: None,
            // Use epoch as sentinel — any real timestamp will be newer.
            last_update: DateTime::UNIX_EPOCH,
            is_healthy: false,
            stale_timeout_secs: config.stale_timeout_secs,
        }
    }

    /// Returns true when the last update is older than `stale_timeout_secs` relative to `now`.
    pub fn is_stale(&self, now: DateTime<Utc>) -> bool {
        let age = now.signed_duration_since(self.last_update);
        age.num_seconds() as u64 >= self.stale_timeout_secs
    }

    /// Record a new price and timestamp. Marks the source healthy.
    pub fn update_price(&mut self, price: Price, timestamp: DateTime<Utc>) {
        self.last_price = Some(price);
        self.last_update = timestamp;
        self.is_healthy = true;
    }

    /// Re-evaluate health based on staleness. Call this before consuming price data.
    pub fn refresh_health(&mut self, now: DateTime<Utc>) {
        if self.is_stale(now) {
            self.is_healthy = false;
        }
    }
}

/// A snapshot of a source's health for external inspection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceStatus {
    pub name: String,
    pub weight: Decimal,
    pub last_price: Option<Price>,
    pub last_update: DateTime<Utc>,
    pub is_healthy: bool,
    pub is_stale: bool,
}

impl PriceSource {
    pub fn status(&self, now: DateTime<Utc>) -> SourceStatus {
        SourceStatus {
            name: self.name.clone(),
            weight: self.weight,
            last_price: self.last_price,
            last_update: self.last_update,
            is_healthy: self.is_healthy,
            is_stale: self.is_stale(now),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use rust_decimal_macros::dec;

    use super::*;

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    #[test]
    fn new_source_is_not_healthy() {
        let cfg = defaults::binance();
        let src = PriceSource::from_config(&cfg);
        assert!(!src.is_healthy);
        assert!(src.last_price.is_none());
    }

    #[test]
    fn update_price_sets_healthy() {
        let cfg = defaults::binance();
        let mut src = PriceSource::from_config(&cfg);
        let t = now();
        src.update_price(dec!(50000), t);
        assert!(src.is_healthy);
        assert_eq!(src.last_price, Some(dec!(50000)));
    }

    #[test]
    fn is_stale_returns_true_after_timeout() {
        let cfg = PriceSourceConfig::new("Test", dec!(1.0), 10);
        let mut src = PriceSource::from_config(&cfg);
        let t = now();
        src.update_price(dec!(100), t - Duration::seconds(11));
        assert!(src.is_stale(t));
    }

    #[test]
    fn is_stale_returns_false_within_timeout() {
        let cfg = PriceSourceConfig::new("Test", dec!(1.0), 10);
        let mut src = PriceSource::from_config(&cfg);
        let t = now();
        src.update_price(dec!(100), t - Duration::seconds(5));
        assert!(!src.is_stale(t));
    }

    #[test]
    fn refresh_health_marks_unhealthy_when_stale() {
        let cfg = PriceSourceConfig::new("Test", dec!(1.0), 10);
        let mut src = PriceSource::from_config(&cfg);
        let t = now();
        src.update_price(dec!(100), t - Duration::seconds(11));
        src.refresh_health(t);
        assert!(!src.is_healthy);
    }

    #[test]
    fn defaults_weights_sum_to_one() {
        let total: Decimal = defaults::all().iter().map(|c| c.weight).sum();
        assert_eq!(total, dec!(1.00));
    }
}
