use std::sync::Arc;
use std::time::Instant;

use rust_decimal_macros::dec;
use serde_json::json;

use crate::stats::LoadTestStats;

const SYMBOLS: &[&str] = &["BTCUSDT", "ETHUSDT", "SOLUSDT"];

/// Mixed scenario: 60% market orders (immediate fill), 40% limit orders.
///
/// Symbols and side are chosen pseudo-randomly based on the per-call counter
/// so there is no need for a thread-local RNG or external dependency.
pub struct MixedScenario {
    /// Rolling counter incremented each iteration to vary choices.
    counter: std::sync::atomic::AtomicU64,
}

impl MixedScenario {
    pub fn new() -> Self {
        Self {
            counter: std::sync::atomic::AtomicU64::new(0),
        }
    }

    fn next_counter(&self) -> u64 {
        self.counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }
}

impl super::Scenario for MixedScenario {
    async fn run(
        &self,
        client: Arc<reqwest::Client>,
        base_url: Arc<String>,
        stats: Arc<LoadTestStats>,
    ) {
        let c = self.next_counter();
        let symbol = SYMBOLS[(c % SYMBOLS.len() as u64) as usize];
        let side = if c % 2 == 0 { "Buy" } else { "Sell" };
        // 60% market, 40% limit (determined by counter mod 10)
        let is_market = (c % 10) < 6;

        let url = format!("{}/api/v1/orders", base_url);

        let body = if is_market {
            json!({
                "symbol": symbol,
                "side": side,
                "order_type": "Market",
                "qty": dec!(0.001),
            })
        } else {
            // Limit order: price a little away from a round number so it rests.
            let price = if side == "Buy" { dec!(49000) } else { dec!(52000) };
            json!({
                "symbol": symbol,
                "side": side,
                "order_type": "Limit",
                "price": price,
                "qty": dec!(0.001),
            })
        };

        let t = Instant::now();
        let result = client.post(&url).json(&body).send().await;
        let elapsed = t.elapsed().as_micros() as u64;

        match result {
            Ok(r) => stats.record(elapsed, r.status().is_success()),
            Err(e) => {
                tracing::debug!(error = %e, "mixed scenario request error");
                stats.record(elapsed, false);
            }
        }
    }
}
