use std::time::Duration;

use exchange_common::Symbol;
use thiserror::Error;

/// Errors that can occur in a price feed.
#[derive(Debug, Error)]
pub enum FeedError {
    #[error("WebSocket error: {0}")]
    WebSocket(String),

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Feature disabled: enable the '{0}' cargo feature to use live feeds")]
    FeatureDisabled(String),
}

/// Map an exchange-specific symbol name to our internal `Symbol` format.
///
/// Returns `None` for unknown exchanges.
///
/// Mapping rules:
/// - **Binance**: uppercase as-is (`"btcusdt"` → `"BTCUSDT"`)
/// - **Coinbase**: strip the dash (`"BTC-USD"` → `"BTCUSD"`)
/// - **Kraken**: strip slash, replace `XBT` with `BTC` (`"XBT/USD"` → `"BTCUSD"`)
pub fn normalize_symbol(exchange: &str, symbol: &str) -> Option<Symbol> {
    match exchange {
        "Binance" => Some(Symbol::new(symbol.to_uppercase())),
        "Coinbase" => Some(Symbol::new(symbol.replace('-', ""))),
        "Kraken" => {
            let s = symbol.replace('/', "").replace("XBT", "BTC");
            Some(Symbol::new(s))
        }
        _ => None,
    }
}

/// Exponential backoff with deterministic jitter for reconnection loops.
///
/// Waits `2^attempt` seconds (capped at 64 s) plus ±25% jitter derived from
/// the attempt counter. Does not require a random-number dependency.
pub async fn backoff_sleep(attempt: u32) {
    let base_secs = 2u64.pow(attempt.min(6)); // 1, 2, 4, 8, 16, 32, 64
    // Simple deterministic jitter: +0..25% of base, derived from attempt.
    // No rand crate needed; the variation is just to avoid thundering-herd.
    let jitter_secs = (attempt as u64 * 3 + 1) % (base_secs / 4 + 1);
    let total = Duration::from_secs(base_secs + jitter_secs);
    tokio::time::sleep(total).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── normalize_symbol ────────────────────────────────────────────────────

    #[test]
    fn normalize_binance_lowercase() {
        let sym = normalize_symbol("Binance", "btcusdt").unwrap();
        assert_eq!(sym.as_str(), "BTCUSDT");
    }

    #[test]
    fn normalize_binance_mixed_case() {
        let sym = normalize_symbol("Binance", "ethusdt").unwrap();
        assert_eq!(sym.as_str(), "ETHUSDT");
    }

    #[test]
    fn normalize_coinbase_btc_usd() {
        let sym = normalize_symbol("Coinbase", "BTC-USD").unwrap();
        assert_eq!(sym.as_str(), "BTCUSD");
    }

    #[test]
    fn normalize_coinbase_eth_usd() {
        let sym = normalize_symbol("Coinbase", "ETH-USD").unwrap();
        assert_eq!(sym.as_str(), "ETHUSD");
    }

    #[test]
    fn normalize_kraken_xbt_usd() {
        let sym = normalize_symbol("Kraken", "XBT/USD").unwrap();
        assert_eq!(sym.as_str(), "BTCUSD");
    }

    #[test]
    fn normalize_kraken_eth_usd() {
        let sym = normalize_symbol("Kraken", "ETH/USD").unwrap();
        assert_eq!(sym.as_str(), "ETHUSD");
    }

    #[test]
    fn normalize_unknown_exchange_returns_none() {
        assert!(normalize_symbol("Bitfinex", "BTCUSD").is_none());
        assert!(normalize_symbol("", "BTCUSD").is_none());
    }

    // ── FeedError display ───────────────────────────────────────────────────

    #[test]
    fn feed_error_websocket_display() {
        let e = FeedError::WebSocket("connection reset".into());
        assert_eq!(e.to_string(), "WebSocket error: connection reset");
    }

    #[test]
    fn feed_error_connection_failed_display() {
        let e = FeedError::ConnectionFailed("refused".into());
        assert_eq!(e.to_string(), "Connection failed: refused");
    }

    #[test]
    fn feed_error_parse_error_display() {
        let e = FeedError::ParseError("missing field".into());
        assert_eq!(e.to_string(), "Parse error: missing field");
    }

    #[test]
    fn feed_error_feature_disabled_display() {
        let e = FeedError::FeatureDisabled("live-feeds".into());
        let msg = e.to_string();
        assert!(msg.contains("live-feeds"));
        assert!(msg.contains("feature"));
    }
}
