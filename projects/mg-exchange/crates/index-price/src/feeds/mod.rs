pub mod binance;
pub mod coinbase;
pub mod common;
pub mod kraken;
pub mod spawner;

use chrono::{DateTime, Utc};
use exchange_common::Symbol;
use rust_decimal::Decimal;

pub use common::{FeedError, normalize_symbol};

/// A price update received from an external feed.
#[derive(Debug, Clone)]
pub struct PriceUpdate {
    /// Exchange name matching `PriceSourceConfig::name` (e.g., `"Binance"`).
    pub source: String,
    /// Normalised symbol (e.g., `Symbol::new("BTCUSDT")`).
    pub symbol: Symbol,
    /// Latest trade / best-bid-ask mid price reported by the exchange.
    pub price: Decimal,
    /// Wall-clock time when the update was received locally.
    pub timestamp: DateTime<Utc>,
}

/// Trait implemented by every exchange-specific WebSocket feed client.
///
/// Implementors are responsible for connecting, subscribing, parsing,
/// and reconnecting. They communicate with the rest of the system
/// exclusively through a `tokio::sync::mpsc::Sender<PriceUpdate>`.
pub trait PriceFeed: Send + 'static {
    /// Human-readable exchange name, matching the `PriceSourceConfig` name.
    fn name(&self) -> &str;

    /// Symbols this feed is configured to stream.
    fn symbols(&self) -> Vec<Symbol>;
}
