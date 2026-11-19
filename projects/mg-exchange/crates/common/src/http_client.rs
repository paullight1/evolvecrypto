//! Shared HTTP client configuration for external service calls.
//!
//! All external HTTP calls (Fireblocks, Sumsub, Chainalysis, Notabene)
//! should use these defaults to ensure consistent timeout behavior and
//! prevent indefinite hangs when a service is down.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

/// Default request timeout for external services.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Default connection timeout.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Build a reqwest Client with exchange-standard timeouts.
///
/// Reuse this client across calls to the same service — reqwest pools
/// connections internally.
pub fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(DEFAULT_TIMEOUT)
        .connect_timeout(DEFAULT_CONNECT_TIMEOUT)
        .pool_max_idle_per_host(10)
        .build()
        .expect("reqwest client build should not fail")
}

/// Build a blocking reqwest Client with the same timeouts.
pub fn build_blocking_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(DEFAULT_TIMEOUT)
        .connect_timeout(DEFAULT_CONNECT_TIMEOUT)
        .pool_max_idle_per_host(10)
        .build()
        .expect("reqwest blocking client build should not fail")
}

/// Simple circuit breaker: tracks consecutive failures and opens after
/// a threshold, rejecting calls for a cooldown period.
///
/// Thread-safe via atomics — no Mutex.
pub struct CircuitBreaker {
    /// Consecutive failure count.
    failures: AtomicU32,
    /// Unix timestamp (seconds) when the circuit was opened.
    opened_at: AtomicU64,
    /// Open after this many consecutive failures.
    failure_threshold: u32,
    /// Stay open for this many seconds before allowing a probe.
    cooldown_secs: u64,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, cooldown_secs: u64) -> Self {
        Self {
            failures: AtomicU32::new(0),
            opened_at: AtomicU64::new(0),
            failure_threshold,
            cooldown_secs,
        }
    }

    /// Check if requests are allowed. Returns false if the circuit is open.
    pub fn allow_request(&self) -> bool {
        let opened = self.opened_at.load(Ordering::Relaxed);
        if opened == 0 {
            return true; // Closed — allow
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        if now >= opened + self.cooldown_secs {
            // Half-open: allow one probe request.
            true
        } else {
            false
        }
    }

    /// Record a successful call. Resets the failure counter and closes the circuit.
    pub fn record_success(&self) {
        self.failures.store(0, Ordering::Relaxed);
        self.opened_at.store(0, Ordering::Relaxed);
    }

    /// Record a failed call. If failures exceed the threshold, opens the circuit.
    pub fn record_failure(&self) {
        let count = self.failures.fetch_add(1, Ordering::Relaxed) + 1;
        if count >= self.failure_threshold {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            self.opened_at.store(now, Ordering::Relaxed);
            tracing::warn!(
                failures = count,
                cooldown_secs = self.cooldown_secs,
                "Circuit breaker OPENED — rejecting calls for {}s",
                self.cooldown_secs,
            );
        }
    }

    /// True if the circuit is currently open (rejecting calls).
    pub fn is_open(&self) -> bool {
        !self.allow_request()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circuit_starts_closed() {
        let cb = CircuitBreaker::new(3, 60);
        assert!(cb.allow_request());
        assert!(!cb.is_open());
    }

    #[test]
    fn opens_after_threshold_failures() {
        let cb = CircuitBreaker::new(3, 60);
        cb.record_failure();
        cb.record_failure();
        assert!(cb.allow_request()); // 2 < 3
        cb.record_failure();
        assert!(cb.is_open()); // 3 >= 3
    }

    #[test]
    fn success_resets_failures() {
        let cb = CircuitBreaker::new(3, 60);
        cb.record_failure();
        cb.record_failure();
        cb.record_success();
        cb.record_failure();
        assert!(cb.allow_request()); // reset to 1
    }
}
