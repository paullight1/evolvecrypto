//! Simulated Pyth Network price feed.
//!
//! Phase-1 stand-in for a real Pyth WebSocket connection. Returns realistic
//! base prices for each TradFi symbol plus a small, deterministic offset that
//! cycles over time so the aggregator always sees slight inter-source variation.
//!
//! In Phase 2 this module will be replaced by a real Pyth price-service client
//! that maintains a long-lived WebSocket subscription and caches the latest
//! price + confidence interval.

use anyhow::{Result, bail};
use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use super::{TradFiFeed, TradFiRawPrice};

// ── Base prices (realistic 2024–2025 levels) ─────────────────────────────────

/// Canonical base prices returned by this simulated feed.
fn base_price(symbol: &str) -> Option<Decimal> {
    match symbol {
        "XAUUSD" => Some(dec!(2350.00)),  // Gold ~$2,350/oz
        "XAGUSD" => Some(dec!(28.50)),    // Silver ~$28.50/oz
        "EURUSD" => Some(dec!(1.08500)),  // Euro ~1.085
        "GBPUSD" => Some(dec!(1.27500)),  // GBP ~1.275
        "JPYUSD" => Some(dec!(0.00667)),  // JPY (1/150)
        "AUDUSD" => Some(dec!(0.65000)),  // AUD ~0.65
        "CHFUSD" => Some(dec!(1.12500)),  // CHF ~1.125
        "SPXUSD" => Some(dec!(5400.00)),  // S&P 500 ~5,400
        "NDXUSD" => Some(dec!(19000.00)), // Nasdaq-100 ~19,000
        "WTIUSD" => Some(dec!(78.50)),    // WTI crude ~$78.50/bbl
        _ => None,
    }
}

/// Small confidence radius reported alongside each simulated price (±0.05%).
fn confidence(base: Decimal) -> Decimal {
    base * dec!(0.0005)
}

/// Deterministic intra-second price variation: ±(0.02% × (seconds % 7)).
///
/// This gives the aggregator realistic multi-source disagreement without
/// requiring a PRNG dependency.
fn noise_offset(base: Decimal) -> Decimal {
    use std::time::{SystemTime, UNIX_EPOCH};
    let epoch_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let tick = Decimal::from(epoch_secs % 7);
    // ±0.02% × tick, centred around zero by alternating sign on odd ticks.
    let magnitude = base * dec!(0.0002) * tick;
    if (epoch_secs % 2) == 0 {
        magnitude
    } else {
        -magnitude
    }
}

// ── Feed implementation ───────────────────────────────────────────────────────

/// Simulated Pyth Network feed.
///
/// Returns deterministic prices with sub-second variation for testing and
/// development.  Weights: 50% (configured by the aggregator).
pub struct PythSimFeed;

impl TradFiFeed for PythSimFeed {
    async fn get_price(&self, symbol: &str) -> Result<TradFiRawPrice> {
        let base = base_price(symbol)
            .ok_or_else(|| anyhow::anyhow!("PythSim: unsupported symbol '{}'", symbol))?;

        let price = base + noise_offset(base);
        if price <= Decimal::ZERO {
            bail!("PythSim: computed non-positive price for '{}'", symbol);
        }

        Ok(TradFiRawPrice {
            symbol: symbol.to_string(),
            price,
            timestamp: Utc::now(),
            source: "PythSim".to_string(),
            confidence: confidence(base),
        })
    }

    fn name(&self) -> &str {
        "PythSim"
    }

    fn supported_symbols(&self) -> Vec<String> {
        vec![
            "XAUUSD".into(),
            "XAGUSD".into(),
            "EURUSD".into(),
            "GBPUSD".into(),
            "JPYUSD".into(),
            "AUDUSD".into(),
            "CHFUSD".into(),
            "SPXUSD".into(),
            "NDXUSD".into(),
            "WTIUSD".into(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn name_is_pyth_sim() {
        assert_eq!(PythSimFeed.name(), "PythSim");
    }

    #[test]
    fn supported_symbols_includes_all_tradfi() {
        let syms = PythSimFeed.supported_symbols();
        assert!(syms.contains(&"XAUUSD".to_string()));
        assert!(syms.contains(&"EURUSD".to_string()));
        assert!(syms.contains(&"SPXUSD".to_string()));
        assert!(syms.contains(&"GBPUSD".to_string()));
    }

    #[test]
    fn get_price_gold_is_realistic() {
        let price = rt().block_on(PythSimFeed.get_price("XAUUSD")).unwrap();
        assert_eq!(price.source, "PythSim");
        assert_eq!(price.symbol, "XAUUSD");
        // Should be within ±2% of base $2,350.
        assert!(price.price > dec!(2300) && price.price < dec!(2400));
        assert!(price.confidence > Decimal::ZERO);
    }

    #[test]
    fn get_price_eurusd_is_realistic() {
        let price = rt().block_on(PythSimFeed.get_price("EURUSD")).unwrap();
        assert!(price.price > dec!(1.07) && price.price < dec!(1.10));
    }

    #[test]
    fn get_price_spx_is_realistic() {
        let price = rt().block_on(PythSimFeed.get_price("SPXUSD")).unwrap();
        assert!(price.price > dec!(5000) && price.price < dec!(5800));
    }

    #[test]
    fn get_price_unknown_symbol_returns_error() {
        let err = rt().block_on(PythSimFeed.get_price("UNKNOWNSYM")).unwrap_err();
        assert!(err.to_string().contains("unsupported symbol"));
    }
}
