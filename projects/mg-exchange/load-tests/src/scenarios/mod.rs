use std::sync::Arc;

use crate::stats::LoadTestStats;

pub mod market_maker;
pub mod mixed;

/// A load test scenario that a virtual user can execute in a loop.
pub trait Scenario: Send + Sync {
    /// Run one iteration of the scenario.
    ///
    /// Should place/cancel orders via `client`, recording latencies into `stats`.
    /// The method should be cancel-safe: if the future is dropped mid-flight the
    /// exchange state may be inconsistent, but stats must not be corrupted.
    fn run(
        &self,
        client: Arc<reqwest::Client>,
        base_url: Arc<String>,
        stats: Arc<LoadTestStats>,
    ) -> impl std::future::Future<Output = ()> + Send;
}
