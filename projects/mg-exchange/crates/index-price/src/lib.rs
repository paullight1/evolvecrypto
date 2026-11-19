//! Index price computation for perpetual futures.
//!
//! The index price is a manipulation-resistant weighted median of spot prices
//! from multiple external exchanges. It is used as the fair-value basis for
//! mark price, PnL calculation, and liquidation thresholds.
//!
//! # Design
//! - Pure computation — no networking, no I/O.
//! - The server binary feeds price updates via [`aggregator::IndexPriceAggregator::update_source_price`].
//! - The weighted median algorithm (see [`weighted_median`]) is more outlier-resistant
//!   than a simple weighted average: a single manipulated source cannot move the index
//!   by more than its weight allows.
//!
//! # Example
//! ```no_run
//! use exchange_index_price::{IndexPriceAggregator, source::defaults};
//! use exchange_common::Symbol;
//! use rust_decimal_macros::dec;
//! use chrono::Utc;
//!
//! let symbols = vec![Symbol::new("BTCUSDT")];
//! let mut agg = IndexPriceAggregator::with_defaults(symbols);
//!
//! // Feed prices (called by the WS ingestion layer in the server binary).
//! let btc = Symbol::new("BTCUSDT");
//! agg.update_source_price(&btc, "Binance",  dec!(50000), Utc::now());
//! agg.update_source_price(&btc, "Coinbase", dec!(50100), Utc::now());
//! agg.update_source_price(&btc, "Kraken",   dec!(49950), Utc::now());
//!
//! let index = agg.calculate_index(&btc).unwrap();
//! println!("BTC index price: {}", index.price);
//! ```

pub mod aggregator;
pub mod feeds;
pub mod mock_feed;
pub mod source;
pub mod weighted_median;

// Re-export the most commonly used types at crate root.
pub use aggregator::{IndexError, IndexPrice, IndexPriceAggregator};
pub use source::{PriceSource, PriceSourceConfig, SourceStatus};
pub use weighted_median::weighted_median;
