//! REST-based forex price feed (Phase-1: simulated).
//!
//! In Phase 2 this will call a real REST API (e.g. Open Exchange Rates or
//! Twelve Data). For now it returns base prices with a small fixed offset
//! (+0.03%) relative to the PythSim feed to give the aggregator realistic
//! multi-source spread.

use anyhow::{Result, bail};
use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use super::{TradFiFeed, TradFiRawPrice};

fn base_price(symbol: &str) -> Option<Decimal> {
    // Offset from PythSim base by +0.03% to simulate natural bid/ask spread
    // differences between data providers.
    match symbol {
        "EURUSD" => Some(dec!(1.08533)),
        "GBPUSD" => Some(dec!(1.27538)),
        "JPYUSD" => Some(dec!(0.00667)),
        "AUDUSD" => Some(dec!(0.65020)),
        "CHFUSD" => Some(dec!(1.12534)),
        _ => None,
    }
}

/// Simulated REST forex feed (ForexFeed adapter).
///
/// Covers major forex pairs only. Metals and indices are not supported —
/// those are handled by [`super::commodity_feed::CommodityFeed`].
pub struct ForexFeed;

impl TradFiFeed for ForexFeed {
    async fn get_price(&self, symbol: &str) -> Result<TradFiRawPrice> {
        let price = base_price(symbol)
            .ok_or_else(|| anyhow::anyhow!("ForexFeed: unsupported symbol '{}'", symbol))?;

        if price <= Decimal::ZERO {
            bail!("ForexFeed: non-positive price for '{}'", symbol);
        }

        Ok(TradFiRawPrice {
            symbol: symbol.to_string(),
            price,
            timestamp: Utc::now(),
            source: "ForexFeed".to_string(),
            confidence: Decimal::ZERO, // REST feed does not publish confidence
        })
    }

    fn name(&self) -> &str {
        "ForexFeed"
    }

    fn supported_symbols(&self) -> Vec<String> {
        vec![
            "EURUSD".into(),
            "GBPUSD".into(),
            "JPYUSD".into(),
            "AUDUSD".into(),
            "CHFUSD".into(),
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
    fn name_is_forex_feed() {
        assert_eq!(ForexFeed.name(), "ForexFeed");
    }

    #[test]
    fn supported_symbols_are_forex_pairs() {
        let syms = ForexFeed.supported_symbols();
        assert!(syms.contains(&"EURUSD".to_string()));
        assert!(syms.contains(&"GBPUSD".to_string()));
        // Metals and indices are NOT forex.
        assert!(!syms.contains(&"XAUUSD".to_string()));
        assert!(!syms.contains(&"SPXUSD".to_string()));
    }

    #[test]
    fn eurusd_price_is_realistic() {
        let p = rt().block_on(ForexFeed.get_price("EURUSD")).unwrap();
        assert_eq!(p.source, "ForexFeed");
        assert!(p.price > dec!(1.07) && p.price < dec!(1.10));
    }

    #[test]
    fn gbpusd_price_is_realistic() {
        let p = rt().block_on(ForexFeed.get_price("GBPUSD")).unwrap();
        assert!(p.price > dec!(1.25) && p.price < dec!(1.30));
    }

    #[test]
    fn unsupported_symbol_returns_error() {
        let err = rt().block_on(ForexFeed.get_price("XAUUSD")).unwrap_err();
        assert!(err.to_string().contains("unsupported symbol"));
    }
}
