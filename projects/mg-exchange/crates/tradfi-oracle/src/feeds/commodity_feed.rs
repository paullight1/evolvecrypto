//! Commodity and equity index price feed (Phase-1: simulated).
//!
//! Covers precious metals (XAU, XAG), crude oil (WTI), and equity indices
//! (SPX, NDX). Prices are offset by −0.02% from the PythSim base to emulate
//! natural inter-provider variation.
//!
//! In Phase 2 this will call NYMEX / COMEX settlement APIs or a commercial
//! market-data vendor.

use anyhow::{Result, bail};
use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use super::{TradFiFeed, TradFiRawPrice};

fn base_price(symbol: &str) -> Option<Decimal> {
    // −0.02% offset from PythSim base to provide independent source spread.
    match symbol {
        "XAUUSD" => Some(dec!(2349.53)), // Gold
        "XAGUSD" => Some(dec!(28.494)),  // Silver
        "SPXUSD" => Some(dec!(5398.92)), // S&P 500
        "NDXUSD" => Some(dec!(18996.2)), // Nasdaq-100
        "WTIUSD" => Some(dec!(78.484)),  // WTI crude
        _ => None,
    }
}

/// Simulated commodity and equity index feed (CmdtyFeed adapter).
pub struct CommodityFeed;

impl TradFiFeed for CommodityFeed {
    async fn get_price(&self, symbol: &str) -> Result<TradFiRawPrice> {
        let price = base_price(symbol)
            .ok_or_else(|| anyhow::anyhow!("CmdtyFeed: unsupported symbol '{}'", symbol))?;

        if price <= Decimal::ZERO {
            bail!("CmdtyFeed: non-positive price for '{}'", symbol);
        }

        Ok(TradFiRawPrice {
            symbol: symbol.to_string(),
            price,
            timestamp: Utc::now(),
            source: "CmdtyFeed".to_string(),
            confidence: Decimal::ZERO,
        })
    }

    fn name(&self) -> &str {
        "CmdtyFeed"
    }

    fn supported_symbols(&self) -> Vec<String> {
        vec![
            "XAUUSD".into(),
            "XAGUSD".into(),
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
    fn name_is_cmdty_feed() {
        assert_eq!(CommodityFeed.name(), "CmdtyFeed");
    }

    #[test]
    fn supported_symbols_are_commodities_and_indices() {
        let syms = CommodityFeed.supported_symbols();
        assert!(syms.contains(&"XAUUSD".to_string()));
        assert!(syms.contains(&"SPXUSD".to_string()));
        // Forex not covered.
        assert!(!syms.contains(&"EURUSD".to_string()));
    }

    #[test]
    fn gold_price_is_realistic() {
        let p = rt().block_on(CommodityFeed.get_price("XAUUSD")).unwrap();
        assert_eq!(p.source, "CmdtyFeed");
        assert!(p.price > dec!(2300) && p.price < dec!(2400));
    }

    #[test]
    fn spx_price_is_realistic() {
        let p = rt().block_on(CommodityFeed.get_price("SPXUSD")).unwrap();
        assert!(p.price > dec!(5000) && p.price < dec!(5800));
    }

    #[test]
    fn unsupported_symbol_returns_error() {
        let err = rt().block_on(CommodityFeed.get_price("EURUSD")).unwrap_err();
        assert!(err.to_string().contains("unsupported symbol"));
    }
}
