use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::{error, info};

use crate::aggregator::IndexPriceAggregator;

use super::{
    PriceUpdate,
    binance::BinanceFeed,
    coinbase::CoinbaseFeed,
    kraken::KrakenFeed,
};

/// Spawn WebSocket feed tasks for Binance, Coinbase, and Kraken, wiring each
/// into the shared `aggregator`.
///
/// Each feed runs in its own `tokio::task`. Price updates are received from a
/// channel and forwarded to [`IndexPriceAggregator::update_source_price`].
///
/// Returns the join handles so the caller can await or abort them.
///
/// # Feature gate
/// Feed connections require the `live-feeds` cargo feature. Without it, this
/// function still compiles and returns handles, but each spawned task will
/// immediately exit after logging an error.
pub async fn spawn_all_feeds(
    aggregator: Arc<Mutex<IndexPriceAggregator>>,
) -> Vec<tokio::task::JoinHandle<()>> {
    // Channel capacity: plenty of headroom for burst updates.
    const CHANNEL_CAP: usize = 256;

    // Default symbol sets. In production these would come from config.
    let binance = BinanceFeed::new(vec![
        "btcusdt".into(),
        "ethusdt".into(),
        "solusdt".into(),
    ]);
    let coinbase = CoinbaseFeed::new(vec![
        "BTC-USD".into(),
        "ETH-USD".into(),
        "SOL-USD".into(),
    ]);
    let kraken = KrakenFeed::new(vec![
        "XBT/USD".into(),
        "ETH/USD".into(),
        "SOL/USD".into(),
    ]);

    let mut handles = Vec::with_capacity(3);

    // Spawn one aggregator-writer task per feed.
    for (source_name, feed_handle) in [
        ("Binance", spawn_feed_reader(binance, CHANNEL_CAP)),
        ("Coinbase", spawn_feed_reader(coinbase, CHANNEL_CAP)),
        ("Kraken", spawn_feed_reader(kraken, CHANNEL_CAP)),
    ] {
        let (feed_task, mut rx) = feed_handle;
        let agg = Arc::clone(&aggregator);

        // Writer task: reads from channel → locks aggregator → calls update_source_price.
        let writer = tokio::spawn(async move {
            while let Some(update) = rx.recv().await {
                let mut agg = agg.lock().await;
                agg.update_source_price(
                    &update.symbol,
                    &update.source,
                    update.price,
                    update.timestamp,
                );
            }
            info!(source = source_name, "Price feed writer exiting");
        });

        handles.push(feed_task);
        handles.push(writer);
    }

    handles
}

/// Internal helper: spawns the WebSocket connect loop for `feed` and returns
/// the task handle plus the receiver for price updates.
fn spawn_feed_reader<F>(
    feed: F,
    channel_cap: usize,
) -> (
    tokio::task::JoinHandle<()>,
    tokio::sync::mpsc::Receiver<PriceUpdate>,
)
where
    F: super::PriceFeed
        + HasConnect
        + Send
        + 'static,
{
    let (tx, rx) = tokio::sync::mpsc::channel(channel_cap);
    let handle = tokio::spawn(async move {
        if let Err(e) = feed.connect(tx).await {
            error!(error = %e, "Price feed connection error");
        }
    });
    (handle, rx)
}

/// Sealed helper trait so `spawn_feed_reader` can call `.connect()` generically
/// without making the `connect` method part of the public `PriceFeed` trait
/// (which would complicate the trait object story and feature-gating).
///
/// Takes `self` by value so the feed can be moved into the spawned task.
/// The returned future must be `Send` so it can be handed to `tokio::spawn`.
pub trait HasConnect: super::PriceFeed + Sized {
    fn connect(
        self,
        tx: tokio::sync::mpsc::Sender<PriceUpdate>,
    ) -> impl std::future::Future<Output = Result<(), super::common::FeedError>> + Send;
}

impl HasConnect for BinanceFeed {
    fn connect(
        self,
        tx: tokio::sync::mpsc::Sender<PriceUpdate>,
    ) -> impl std::future::Future<Output = Result<(), super::common::FeedError>> + Send {
        async move { BinanceFeed::connect(&self, tx).await }
    }
}

impl HasConnect for CoinbaseFeed {
    fn connect(
        self,
        tx: tokio::sync::mpsc::Sender<PriceUpdate>,
    ) -> impl std::future::Future<Output = Result<(), super::common::FeedError>> + Send {
        async move { CoinbaseFeed::connect(&self, tx).await }
    }
}

impl HasConnect for KrakenFeed {
    fn connect(
        self,
        tx: tokio::sync::mpsc::Sender<PriceUpdate>,
    ) -> impl std::future::Future<Output = Result<(), super::common::FeedError>> + Send {
        async move { KrakenFeed::connect(&self, tx).await }
    }
}

/// Spawn all feeds and return a single merged receiver for all price updates.
///
/// This is the preferred entry point for callers that hold the aggregator
/// behind a `std::sync::Mutex` (e.g., the Exchange god struct). The caller
/// reads from the returned receiver and writes to the aggregator under the
/// std::sync::Mutex — avoiding the async/sync Mutex mismatch.
///
/// Returns `(handles, receiver)`.
/// Spawn all feeds with dynamically provided symbols.
///
/// `symbols` — list of normalised symbols (e.g., `["BTCUSDT", "ETHUSDT"]`).
/// Each feed maps these to its exchange-specific format internally.
///
/// If `symbols` is empty, defaults to BTC + ETH + SOL.
pub fn spawn_all_feeds_channel(symbols: Vec<String>) -> (
    Vec<tokio::task::JoinHandle<()>>,
    tokio::sync::mpsc::Receiver<PriceUpdate>,
) {
    const CHANNEL_CAP: usize = 512;
    let (merged_tx, merged_rx) = tokio::sync::mpsc::channel::<PriceUpdate>(CHANNEL_CAP);

    // Map normalised symbols to exchange-specific formats.
    let syms = if symbols.is_empty() {
        vec!["BTCUSDT".to_string(), "ETHUSDT".to_string(), "SOLUSDT".to_string()]
    } else {
        symbols
    };

    let binance_syms: Vec<String> = syms.iter().map(|s| s.to_lowercase()).collect();
    let coinbase_syms: Vec<String> = syms.iter().map(|s| {
        // BTCUSDT → BTC-USD (strip trailing T, insert dash)
        let base = if s.ends_with("USDT") { &s[..s.len()-4] } else { &s[..s.len()-3] };
        let quote = if s.ends_with("USDT") { "USD" } else { "USD" };
        format!("{}-{}", base, quote)
    }).collect();
    let kraken_syms: Vec<String> = syms.iter().map(|s| {
        let base = if s.ends_with("USDT") { &s[..s.len()-4] } else { &s[..s.len()-3] };
        let kraken_base = if base == "BTC" { "XBT" } else { base };
        format!("{}/USD", kraken_base)
    }).collect();

    let binance = BinanceFeed::new(binance_syms);
    let coinbase = CoinbaseFeed::new(coinbase_syms);
    let kraken = KrakenFeed::new(kraken_syms);

    let mut handles = Vec::with_capacity(6);

    for feed_handle in [
        spawn_feed_reader(binance, 128),
        spawn_feed_reader(coinbase, 128),
        spawn_feed_reader(kraken, 128),
    ] {
        let (feed_task, mut rx) = feed_handle;
        let tx = merged_tx.clone();

        // Forward from per-feed channel to the merged channel.
        let forwarder = tokio::spawn(async move {
            while let Some(update) = rx.recv().await {
                if tx.send(update).await.is_err() {
                    break; // Merged receiver dropped.
                }
            }
        });

        handles.push(feed_task);
        handles.push(forwarder);
    }

    // Drop our sender so the receiver closes when all forwarders finish.
    drop(merged_tx);

    (handles, merged_rx)
}

#[cfg(test)]
mod tests {
    use exchange_common::Symbol;

    use super::*;
    use crate::IndexPriceAggregator;

    fn make_aggregator(symbols: Vec<Symbol>) -> Arc<Mutex<IndexPriceAggregator>> {
        Arc::new(Mutex::new(IndexPriceAggregator::with_defaults(symbols)))
    }

    /// Verify that `spawn_all_feeds` returns handles and the aggregator can be
    /// locked immediately after (no deadlock at startup).
    #[tokio::test]
    async fn spawn_all_feeds_returns_handles() {
        let symbols = vec![Symbol::new("BTCUSDT"), Symbol::new("ETHUSDT")];
        let agg = make_aggregator(symbols);
        let handles = spawn_all_feeds(Arc::clone(&agg)).await;

        // 3 feeds × 2 tasks each (connector + writer) = 6 handles.
        assert_eq!(handles.len(), 6);

        // Aggregator should still be lockable — no deadlock.
        let locked = agg.try_lock();
        assert!(locked.is_ok(), "aggregator was unexpectedly locked");

        // Abort all tasks — we don't want real connections in tests.
        for h in handles {
            h.abort();
        }
    }
}
