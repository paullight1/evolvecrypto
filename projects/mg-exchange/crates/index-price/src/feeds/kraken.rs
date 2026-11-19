use exchange_common::Symbol;

use super::{
    PriceFeed, PriceUpdate,
    common::{FeedError, normalize_symbol},
};
#[cfg(feature = "live-feeds")]
use super::common::backoff_sleep;

/// WebSocket feed for Kraken v2 API.
///
/// Endpoint: `wss://ws.kraken.com/v2`
///
/// Subscribes to the `ticker` channel and extracts the `last` price
/// from each update.
pub struct KrakenFeed {
    /// Kraken symbol names (e.g., `["XBT/USD", "ETH/USD"]`).
    symbols: Vec<String>,
}

impl KrakenFeed {
    /// Create a new feed. `symbols` should be Kraken pair names such as
    /// `"XBT/USD"`, `"ETH/USD"`, `"SOL/USD"`.
    pub fn new(symbols: Vec<String>) -> Self {
        Self { symbols }
    }

    /// Connect to Kraken WebSocket and forward price updates to `tx`.
    ///
    /// Sends a subscribe message for the `ticker` channel, then reads
    /// updates in a loop. On disconnect it reconnects with exponential backoff.
    /// The loop exits only when the receiver end of `tx` is dropped.
    ///
    /// Requires the `live-feeds` cargo feature.
    #[cfg(feature = "live-feeds")]
    pub async fn connect(
        &self,
        tx: tokio::sync::mpsc::Sender<PriceUpdate>,
    ) -> Result<(), FeedError> {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::{connect_async, tungstenite::Message};
        use tracing::{debug, warn};

        let url_str = "wss://ws.kraken.com/v2";
        let subscribe_msg = serde_json::json!({
            "method": "subscribe",
            "params": {
                "channel": "ticker",
                "symbol": self.symbols
            }
        })
        .to_string();

        let mut attempt = 0u32;
        loop {
            match connect_async(&*url_str).await {
                Err(e) => {
                    warn!(
                        exchange = "Kraken",
                        attempt,
                        error = %e,
                        "WebSocket connection failed, retrying"
                    );
                }
                Ok((mut ws_stream, _response)) => {
                    attempt = 0;
                    debug!(exchange = "Kraken", "Connected");

                    if let Err(e) = ws_stream.send(Message::Text(subscribe_msg.clone().into())).await {
                        warn!(exchange = "Kraken", error = %e, "Failed to send subscribe");
                    } else {
                        while let Some(msg) = ws_stream.next().await {
                            match msg {
                                Err(e) => {
                                    warn!(exchange = "Kraken", error = %e, "WebSocket error");
                                    break;
                                }
                                Ok(Message::Text(text)) => {
                                    if let Some(update) = parse_ticker_message(&text) {
                                        if tx.send(update).await.is_err() {
                                            return Ok(());
                                        }
                                    }
                                }
                                Ok(_) => {}
                            }
                        }
                    }

                    warn!(exchange = "Kraken", "Stream disconnected, reconnecting");
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

impl PriceFeed for KrakenFeed {
    fn name(&self) -> &str {
        "Kraken"
    }

    fn symbols(&self) -> Vec<Symbol> {
        self.symbols
            .iter()
            .filter_map(|s| normalize_symbol("Kraken", s))
            .collect()
    }
}

/// Parse a Kraken v2 ticker channel message.
///
/// Expected format:
/// ```json
/// {"channel":"ticker","type":"update","data":[{"symbol":"XBT/USD","last":50123.45,...}]}
/// ```
///
/// The `last` field contains the last trade price as a float.
#[cfg(feature = "live-feeds")]
fn parse_ticker_message(text: &str) -> Option<PriceUpdate> {
    use chrono::Utc;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    let root: serde_json::Value = serde_json::from_str(text).ok()?;

    if root.get("channel").and_then(|v| v.as_str()) != Some("ticker") {
        return None; // heartbeat, status, subscribe ack, etc.
    }

    let data = root.get("data")?.as_array()?;
    let ticker = data.first()?;

    let symbol_str = ticker.get("symbol")?.as_str()?;

    // Kraken sends `last` as a float number in v2 API.
    let last = ticker.get("last")?;
    let price = if let Some(f) = last.as_f64() {
        Decimal::from_str(&f.to_string()).ok()?
    } else if let Some(s) = last.as_str() {
        Decimal::from_str(s).ok()?
    } else {
        return None;
    };

    let symbol = normalize_symbol("Kraken", symbol_str)?;

    Some(PriceUpdate {
        source: "Kraken".into(),
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
        let feed = KrakenFeed::new(vec!["XBT/USD".into(), "ETH/USD".into()]);
        assert_eq!(feed.symbols, vec!["XBT/USD", "ETH/USD"]);
    }

    #[test]
    fn name_is_kraken() {
        let feed = KrakenFeed::new(vec!["XBT/USD".into()]);
        assert_eq!(feed.name(), "Kraken");
    }

    #[test]
    fn symbols_normalises_kraken_format() {
        let feed = KrakenFeed::new(vec!["XBT/USD".into(), "ETH/USD".into()]);
        let syms = feed.symbols();
        assert_eq!(syms[0].as_str(), "BTCUSD");
        assert_eq!(syms[1].as_str(), "ETHUSD");
    }

    #[test]
    fn connect_without_live_feeds_returns_feature_disabled() {
        #[cfg(not(feature = "live-feeds"))]
        {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let feed = KrakenFeed::new(vec!["XBT/USD".into()]);
            let (tx, _rx) = tokio::sync::mpsc::channel(1);
            let err = rt.block_on(feed.connect(tx)).unwrap_err();
            assert!(matches!(err, FeedError::FeatureDisabled(_)));
        }
        #[cfg(feature = "live-feeds")]
        {}
    }

    #[cfg(feature = "live-feeds")]
    #[test]
    fn parse_ticker_message_extracts_price() {
        let json = r#"{"channel":"ticker","type":"update","data":[{"symbol":"XBT/USD","last":50123.45,"bid":50100.0,"ask":50150.0}]}"#;
        let update = parse_ticker_message(json).unwrap();
        assert_eq!(update.source, "Kraken");
        assert_eq!(update.symbol.as_str(), "BTCUSD");
        // Float precision — check approximate equality.
        let expected = rust_decimal_macros::dec!(50123.45);
        let diff = (update.price - expected).abs();
        assert!(diff < rust_decimal_macros::dec!(0.01), "price too far: {}", update.price);
    }

    #[cfg(feature = "live-feeds")]
    #[test]
    fn parse_non_ticker_channel_returns_none() {
        let json = r#"{"channel":"heartbeat","data":[]}"#;
        assert!(parse_ticker_message(json).is_none());
    }

    #[cfg(feature = "live-feeds")]
    #[test]
    fn parse_bad_json_returns_none() {
        assert!(parse_ticker_message("not json").is_none());
    }
}
