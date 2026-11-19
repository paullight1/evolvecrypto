use std::sync::Arc;
use std::time::Instant;

use rust_decimal_macros::dec;
use serde_json::json;
use uuid::Uuid;

use crate::stats::LoadTestStats;

/// Market-maker scenario: continuously post a limit bid and offer, wait briefly,
/// then cancel both.
///
/// Measures:
/// - `place` latency: time to receive the accepted response for each limit order.
/// - `cancel` latency: time to cancel the resting order.
pub struct MarketMakerScenario {
    pub symbol: String,
    /// Spread half-width in ticks (bid = mid - offset, ask = mid + offset).
    pub spread_offset: rust_decimal::Decimal,
    /// Notional size per order in quote currency.
    pub qty: rust_decimal::Decimal,
    /// Mid-price to quote around.
    pub mid_price: rust_decimal::Decimal,
}

impl MarketMakerScenario {
    pub fn new(symbol: &str) -> Self {
        Self {
            symbol: symbol.to_string(),
            spread_offset: dec!(5),
            qty: dec!(0.001),
            mid_price: dec!(50000),
        }
    }
}

impl super::Scenario for MarketMakerScenario {
    async fn run(
        &self,
        client: Arc<reqwest::Client>,
        base_url: Arc<String>,
        stats: Arc<LoadTestStats>,
    ) {
        let bid_price = self.mid_price - self.spread_offset;
        let ask_price = self.mid_price + self.spread_offset;
        let url = format!("{}/api/v1/orders", base_url);

        // Place limit buy
        let bid_id = Uuid::new_v4().to_string();
        let t = Instant::now();
        let bid_result = client
            .post(&url)
            .json(&json!({
                "symbol": self.symbol,
                "side": "Buy",
                "order_type": "Limit",
                "price": bid_price,
                "qty": self.qty,
                "client_order_id": bid_id,
            }))
            .send()
            .await;
        let elapsed = t.elapsed().as_micros() as u64;
        let (bid_success, bid_order_id) = match bid_result {
            Ok(r) if r.status().is_success() => {
                stats.record(elapsed, true);
                let body: serde_json::Value = r.json().await.unwrap_or_default();
                let id = body["order_id"].as_str().unwrap_or("").to_string();
                (true, id)
            }
            Ok(r) => {
                tracing::debug!(status = %r.status(), "bid placement failed");
                stats.record(elapsed, false);
                (false, String::new())
            }
            Err(e) => {
                tracing::debug!(error = %e, "bid placement error");
                stats.record(elapsed, false);
                (false, String::new())
            }
        };

        // Place limit sell
        let ask_id = Uuid::new_v4().to_string();
        let t = Instant::now();
        let ask_result = client
            .post(&url)
            .json(&json!({
                "symbol": self.symbol,
                "side": "Sell",
                "order_type": "Limit",
                "price": ask_price,
                "qty": self.qty,
                "client_order_id": ask_id,
            }))
            .send()
            .await;
        let elapsed = t.elapsed().as_micros() as u64;
        let (ask_success, ask_order_id) = match ask_result {
            Ok(r) if r.status().is_success() => {
                stats.record(elapsed, true);
                let body: serde_json::Value = r.json().await.unwrap_or_default();
                let id = body["order_id"].as_str().unwrap_or("").to_string();
                (true, id)
            }
            Ok(r) => {
                tracing::debug!(status = %r.status(), "ask placement failed");
                stats.record(elapsed, false);
                (false, String::new())
            }
            Err(e) => {
                tracing::debug!(error = %e, "ask placement error");
                stats.record(elapsed, false);
                (false, String::new())
            }
        };

        // Cancel bid if placed
        if bid_success && !bid_order_id.is_empty() {
            let cancel_url = format!("{}/api/v1/orders/{}", base_url, bid_order_id);
            let t = Instant::now();
            let result = client.delete(&cancel_url).send().await;
            let elapsed = t.elapsed().as_micros() as u64;
            match result {
                Ok(r) => stats.record(elapsed, r.status().is_success()),
                Err(_) => stats.record(elapsed, false),
            }
        }

        // Cancel ask if placed
        if ask_success && !ask_order_id.is_empty() {
            let cancel_url = format!("{}/api/v1/orders/{}", base_url, ask_order_id);
            let t = Instant::now();
            let result = client.delete(&cancel_url).send().await;
            let elapsed = t.elapsed().as_micros() as u64;
            match result {
                Ok(r) => stats.record(elapsed, r.status().is_success()),
                Err(_) => stats.record(elapsed, false),
            }
        }
    }
}
