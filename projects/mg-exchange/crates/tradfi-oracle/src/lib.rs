//! TradFi oracle — price feeds and aggregation for traditional financial instruments.
//!
//! Provides manipulation-resistant price feeds for TradFi perpetual contracts:
//! metals (XAU, XAG), forex (EUR, GBP, JPY) and equity indices (SPX, NDX).
//!
//! # Design
//! - Pure computation in the aggregator — no networking, no I/O.
//! - Feed implementations supply price updates; the aggregator applies
//!   weighted-median aggregation identical to the crypto index-price crate.
//! - Market-hours logic gates order acceptance outside exchange sessions.
//!
//! # Example
//! ```rust
//! use exchange_tradfi_oracle::{TradFiPriceAggregator, feeds::FeedSourceConfig};
//! use rust_decimal_macros::dec;
//! use chrono::Utc;
//!
//! let sources = vec![
//!     FeedSourceConfig::new("PythSim",  dec!(0.50), 30),
//!     FeedSourceConfig::new("ForexFeed", dec!(0.30), 30),
//!     FeedSourceConfig::new("CmdtyFeed", dec!(0.20), 30),
//! ];
//! let symbols = vec!["XAUUSD".to_string(), "EURUSD".to_string()];
//! let mut agg = TradFiPriceAggregator::new(symbols, sources, 1);
//!
//! agg.update_source_price("XAUUSD", "PythSim", dec!(2350.00), Utc::now());
//! agg.update_source_price("XAUUSD", "ForexFeed", dec!(2351.50), Utc::now());
//! let price = agg.calculate_price("XAUUSD").unwrap();
//! println!("XAU/USD: {}", price.price);
//! ```

pub mod aggregator;
pub mod feeds;
pub mod market_hours;
pub mod weighted_median;

pub use aggregator::{AggregatorError, TradFiPrice, TradFiPriceAggregator};
pub use market_hours::{AssetClass, MarketSession, TradingHours};
