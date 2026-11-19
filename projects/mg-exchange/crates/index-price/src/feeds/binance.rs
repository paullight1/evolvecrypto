use exchange_common::Symbol;

use super::{
    PriceFeed, PriceUpdate,
    common::{FeedError, normalize_symbol},
};
#[cfg(feature = "live-feeds")]
use super::common::backoff_sleep;

/// WebSocket feed for Binance spot mini-ticker streams.
///
/// Subscribes to `<symbol>@miniTicker` for each configured symbol using
/// Binance's combined stream endpoint:
/// `wss://stream.binance.com:9443/stream?streams=btcusdt@miniTicker/...`
///
/// The `c` (close/last price) field is used as the current price.
pub struct BinanceFeed {
    /// Exchange-specific symbol names in lowercase (e.g., `["btcusdt", "ethusdt"]`).
    symbols: Vec<String>,
}

impl BinanceFeed {
    /// Create a new feed. `symbols` should be lowercase Binance pair names
    /// such as `"btcusdt"`, `"ethusdt"`, `"solusdt"`.
    pub fn new(symbols: Vec<String>) -> Self {
        Self { symbols }
    }

    /// Connect to Binance WebSocket and forward price updates to `tx`.
    ///
    /// Runs an infinite loop: on disconnect it logs a warning and re-connects
    /// with exponential backoff (starting at 2 s, capped at 64 s).
    /// The loop exits only when the receiver end of `tx` is dropped.
    ///
    /// Requires the `live-feeds` cargo feature.
    #[cfg(feature = "live-feeds")]
    pub async fn connect(
        &self,
        tx: tokio::sync::mpsc::Sender<PriceUpdate>,
    ) -> Result<(), FeedError> {
        use futures_util::StreamExt;
        use tokio_tungstenite::connect_async;
        use tracing::{debug, warn};

        let streams: Vec<String> = self
            .symbols
            .iter()
            .map(|s| format!("{}@miniTicker", s.to_lowercase()))
            .collect();
        let path = streams.join("/");
        let url_str = format!("wss://stream.binance.com:9443/stream?streams={}", path);

        let mut attempt = 0u32;
        loop {
            match connect_async(&*url_str).await {
                Err(e) => {
                    warn!(
                        exchange = "Binance",
                        attempt,
                        error = %e,
                        "WebSocket connection failed, retrying"
                    );
                }
                Ok((mut ws_stream, _response)) => {
                    attempt = 0; // reset backoff on successful connect
                    debug!(exchange = "Binance", "Connected to combined stream");

                    while let Some(msg) = ws_stream.next().await {
                        match msg {
                            Err(e) => {
                                warn!(exchange = "Binance", error = %e, "WebSocket error");
                                break; // reconnect
                            }
                            Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                                if let Some(update) = parse_combined_message(&text) {
                                    if tx.send(update).await.is_err() {
                                        // Receiver dropped — stop the loop.
                                        return Ok(());
                                    }
                                }
                            }
                            Ok(_) => {} // ping/pong/binary — ignore
                        }
                    }

                    warn!(exchange = "Binance", "Stream disconnected, reconnecting");
                }
            }

            attempt = attempt.saturating_add(1);
            backoff_sleep(attempt).await;
        }
    }

    /// Stub when the `live-feeds` feature is disabled.
    #[cfg(not(feature = "live-feeds"))]
    pub async fn connect(
        &self,
        _tx: tokio::sync::mpsc::Sender<PriceUpdate>,
    ) -> Result<(), FeedError> {
        Err(FeedError::FeatureDisabled("live-feeds".into()))
    }
}

impl PriceFeed for BinanceFeed {
    fn name(&self) -> &str {
        "Binance"
    }

    fn symbols(&self) -> Vec<Symbol> {
        self.symbols
            .iter()
            .filter_map(|s| normalize_symbol("Binance", s))
            .collect()
    }
}

/// Parse a Binance combined-stream envelope.
///
/// Expected format:
/// ```json
/// {"stream":"btcusdt@miniTicker","data":{"e":"24hrMiniTicker","s":"BTCUSDT","c":"50123.45",...}}
/// ```
#[cfg(feature = "live-feeds")]
fn parse_combined_message(text: &str) -> Option<PriceUpdate> {
    use chrono::Utc;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    let root: serde_json::Value = serde_json::from_str(text).ok()?;
    let data = root.get("data")?;

    // "s" = symbol (uppercase), "c" = close / last price
    let symbol_str = data.get("s")?.as_str()?;
    let price_str = data.get("c")?.as_str()?;

    let price = Decimal::from_str(price_str).ok()?;
    let symbol = normalize_symbol("Binance", symbol_str)?;

    Some(PriceUpdate {
        source: "Binance".into(),
        symbol,
        price,
        timestamp: Utc::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_stores_symbols() {
        let feed = BinanceFeed::new(vec!["btcusdt".into(), "ethusdt".into()]);
        assert_eq!(feed.symbols, vec!["btcusdt", "ethusdt"]);
    }

    #[test]
    fn name_is_binance() {
        let feed = BinanceFeed::new(vec!["btcusdt".into()]);
        assert_eq!(feed.name(), "Binance");
    }

    #[test]
    fn symbols_normalises_to_uppercase() {
        let feed = BinanceFeed::new(vec!["btcusdt".into(), "solusdt".into()]);
        let syms = feed.symbols();
        assert_eq!(syms[0].as_str(), "BTCUSDT");
        assert_eq!(syms[1].as_str(), "SOLUSDT");
    }

    #[test]
    fn connect_without_live_feeds_returns_feature_disabled() {
        // This test only runs when the feature is absent (default in `cargo test`).
        #[cfg(not(feature = "live-feeds"))]
        {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let feed = BinanceFeed::new(vec!["btcusdt".into()]);
            let (tx, _rx) = tokio::sync::mpsc::channel(1);
            let err = rt.block_on(feed.connect(tx)).unwrap_err();
            assert!(matches!(err, FeedError::FeatureDisabled(_)));
        }
        #[cfg(feature = "live-feeds")]
        {
            // When the feature IS enabled, the stub is not compiled — nothing to assert.
        }
    }

    #[cfg(feature = "live-feeds")]
    #[test]
    fn parse_combined_message_extracts_price() {
        let json = r#"{"stream":"btcusdt@miniTicker","data":{"e":"24hrMiniTicker","s":"BTCUSDT","c":"50123.45","o":"49000","h":"51000","l":"48000","v":"1000","q":"50000000","O":0,"C":0,"F":0,"L":0,"n":0}}"#;
        let update = parse_combined_message(json).unwrap();
        assert_eq!(update.source, "Binance");
        assert_eq!(update.symbol.as_str(), "BTCUSDT");
        use rust_decimal_macros::dec;
        assert_eq!(update.price, dec!(50123.45));
    }

    #[cfg(feature = "live-feeds")]
    #[test]
    fn parse_combined_message_bad_json_returns_none() {
        assert!(parse_combined_message("not json").is_none());
        assert!(parse_combined_message(r#"{"stream":"x"}"#).is_none()); // no data field
    }
}
