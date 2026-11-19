use exchange_common::Symbol;

use super::{
    PriceFeed, PriceUpdate,
    common::{FeedError, normalize_symbol},
};
#[cfg(feature = "live-feeds")]
use super::common::backoff_sleep;

/// WebSocket feed for Coinbase Advanced Trade.
///
/// Endpoint: `wss://advanced-trade-ws.coinbase.com`
///
/// Subscribes via a JSON `subscribe` message and parses `ticker` channel
/// updates. The `price` field is used as the current price.
pub struct CoinbaseFeed {
    /// Coinbase product IDs (e.g., `["BTC-USD", "ETH-USD"]`).
    symbols: Vec<String>,
}

impl CoinbaseFeed {
    /// Create a new feed. `symbols` should be Coinbase product IDs such as
    /// `"BTC-USD"`, `"ETH-USD"`, `"SOL-USD"`.
    pub fn new(symbols: Vec<String>) -> Self {
        Self { symbols }
    }

    /// Connect to Coinbase WebSocket and forward price updates to `tx`.
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

        let url_str = "wss://advanced-trade-ws.coinbase.com";
        let subscribe_msg = serde_json::json!({
            "type": "subscribe",
            "product_ids": self.symbols,
            "channel": "ticker"
        })
        .to_string();

        let mut attempt = 0u32;
        loop {
            match connect_async(&*url_str).await {
                Err(e) => {
                    warn!(
                        exchange = "Coinbase",
                        attempt,
                        error = %e,
                        "WebSocket connection failed, retrying"
                    );
                }
                Ok((mut ws_stream, _response)) => {
                    attempt = 0;
                    debug!(exchange = "Coinbase", "Connected");

                    // Send the subscription request.
                    if let Err(e) = ws_stream.send(Message::Text(subscribe_msg.clone().into())).await {
                        warn!(exchange = "Coinbase", error = %e, "Failed to send subscribe");
                    } else {
                        while let Some(msg) = ws_stream.next().await {
                            match msg {
                                Err(e) => {
                                    warn!(exchange = "Coinbase", error = %e, "WebSocket error");
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

                    warn!(exchange = "Coinbase", "Stream disconnected, reconnecting");
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

impl PriceFeed for CoinbaseFeed {
    fn name(&self) -> &str {
        "Coinbase"
    }

    fn symbols(&self) -> Vec<Symbol> {
        self.symbols
            .iter()
            .filter_map(|s| normalize_symbol("Coinbase", s))
            .collect()
    }
}

/// Parse a Coinbase Advanced Trade ticker message.
///
/// Expected format (channel event wrapper):
/// ```json
/// {"channel":"ticker","events":[{"type":"update","tickers":[{"type":"ticker","product_id":"BTC-USD","price":"50123.45",...}]}]}
/// ```
///
/// Also handles the flat legacy format:
/// ```json
/// {"type":"ticker","product_id":"BTC-USD","price":"50123.45"}
/// ```
#[cfg(feature = "live-feeds")]
fn parse_ticker_message(text: &str) -> Option<PriceUpdate> {
    use chrono::Utc;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    let root: serde_json::Value = serde_json::from_str(text).ok()?;

    // Try the new channel/events wrapper first.
    if root.get("channel").and_then(|v| v.as_str()) == Some("ticker") {
        if let Some(events) = root.get("events").and_then(|v| v.as_array()) {
            for event in events {
                if let Some(tickers) = event.get("tickers").and_then(|v| v.as_array()) {
                    for ticker in tickers {
                        if let Some(update) = extract_coinbase_ticker(ticker) {
                            return Some(update);
                        }
                    }
                }
            }
        }
        return None;
    }

    // Fallback: flat ticker message.
    if root.get("type").and_then(|v| v.as_str()) == Some("ticker") {
        return extract_coinbase_ticker(&root);
    }

    // subscriptions / heartbeat / other — ignore
    None
}

#[cfg(feature = "live-feeds")]
fn extract_coinbase_ticker(v: &serde_json::Value) -> Option<PriceUpdate> {
    use chrono::Utc;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    let product_id = v.get("product_id")?.as_str()?;
    let price_str = v.get("price")?.as_str()?;
    let price = Decimal::from_str(price_str).ok()?;
    let symbol = normalize_symbol("Coinbase", product_id)?;

    Some(PriceUpdate {
        source: "Coinbase".into(),
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
        let feed = CoinbaseFeed::new(vec!["BTC-USD".into(), "ETH-USD".into()]);
        assert_eq!(feed.symbols, vec!["BTC-USD", "ETH-USD"]);
    }

    #[test]
    fn name_is_coinbase() {
        let feed = CoinbaseFeed::new(vec!["BTC-USD".into()]);
        assert_eq!(feed.name(), "Coinbase");
    }

    #[test]
    fn symbols_normalises_coinbase_format() {
        let feed = CoinbaseFeed::new(vec!["BTC-USD".into(), "ETH-USD".into()]);
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
            let feed = CoinbaseFeed::new(vec!["BTC-USD".into()]);
            let (tx, _rx) = tokio::sync::mpsc::channel(1);
            let err = rt.block_on(feed.connect(tx)).unwrap_err();
            assert!(matches!(err, FeedError::FeatureDisabled(_)));
        }
        #[cfg(feature = "live-feeds")]
        {}
    }

    #[cfg(feature = "live-feeds")]
    #[test]
    fn parse_flat_ticker_message() {
        let json = r#"{"type":"ticker","product_id":"BTC-USD","price":"50123.45","best_bid":"50100","best_ask":"50150"}"#;
        let update = parse_ticker_message(json).unwrap();
        assert_eq!(update.source, "Coinbase");
        assert_eq!(update.symbol.as_str(), "BTCUSD");
        use rust_decimal_macros::dec;
        assert_eq!(update.price, dec!(50123.45));
    }

    #[cfg(feature = "live-feeds")]
    #[test]
    fn parse_channel_events_ticker_message() {
        let json = r#"{
            "channel": "ticker",
            "events": [{"type":"update","tickers":[{"type":"ticker","product_id":"ETH-USD","price":"3001.50"}]}]
        }"#;
        let update = parse_ticker_message(json).unwrap();
        assert_eq!(update.source, "Coinbase");
        assert_eq!(update.symbol.as_str(), "ETHUSD");
        use rust_decimal_macros::dec;
        assert_eq!(update.price, dec!(3001.50));
    }

    #[cfg(feature = "live-feeds")]
    #[test]
    fn parse_subscription_ack_returns_none() {
        let json = r#"{"type":"subscriptions","channels":[]}"#;
        assert!(parse_ticker_message(json).is_none());
    }
}
