use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Collected statistics for a load test run.
pub struct LoadTestStats {
    pub total_requests: AtomicU64,
    pub successful: AtomicU64,
    pub failed: AtomicU64,
    /// Latencies in microseconds for percentile calculation.
    pub latencies_us: Mutex<Vec<u64>>,
}

/// Summary report produced at the end of a load test.
#[derive(Debug)]
pub struct LoadTestReport {
    pub total_requests: u64,
    pub successful: u64,
    pub failed: u64,
    pub error_rate_pct: f64,
    /// Requests per second (total / duration).
    pub rps: f64,
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
    pub duration_secs: f64,
}

impl LoadTestStats {
    pub fn new() -> Self {
        Self {
            total_requests: AtomicU64::new(0),
            successful: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            latencies_us: Mutex::new(Vec::new()),
        }
    }

    /// Record one request. `latency_us` is the round-trip time; `success` indicates HTTP 2xx.
    pub fn record(&self, latency_us: u64, success: bool) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        if success {
            self.successful.fetch_add(1, Ordering::Relaxed);
        } else {
            self.failed.fetch_add(1, Ordering::Relaxed);
        }
        if let Ok(mut lats) = self.latencies_us.lock() {
            lats.push(latency_us);
        }
    }

    /// Generate a final report. `duration_secs` is the total wall-clock time of the test.
    pub fn report(&self, duration_secs: f64) -> LoadTestReport {
        let total = self.total_requests.load(Ordering::Relaxed);
        let successful = self.successful.load(Ordering::Relaxed);
        let failed = self.failed.load(Ordering::Relaxed);

        let error_rate_pct = if total == 0 {
            0.0
        } else {
            (failed as f64 / total as f64) * 100.0
        };

        let rps = if duration_secs > 0.0 {
            total as f64 / duration_secs
        } else {
            0.0
        };

        let (p50_us, p95_us, p99_us) = {
            let mut lats = self.latencies_us.lock().unwrap_or_else(|e| e.into_inner());
            lats.sort_unstable();
            let n = lats.len();
            if n == 0 {
                (0, 0, 0)
            } else {
                let p50 = lats[percentile_index(n, 50)];
                let p95 = lats[percentile_index(n, 95)];
                let p99 = lats[percentile_index(n, 99)];
                (p50, p95, p99)
            }
        };

        LoadTestReport {
            total_requests: total,
            successful,
            failed,
            error_rate_pct,
            rps,
            p50_us,
            p95_us,
            p99_us,
            duration_secs,
        }
    }
}

/// Returns the array index for the given percentile (0-based, nearest-rank method).
///
/// For a sorted slice of length `n`, returns the index of the value at the
/// given percentile. Uses the nearest-rank formula: `ceil(p/100 * n) - 1`.
pub fn percentile_index(n: usize, p: u8) -> usize {
    // ceil(p / 100 * n) clamped to [0, n-1]
    let rank = ((p as f64 / 100.0) * n as f64).ceil() as usize;
    rank.saturating_sub(1).min(n - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_index_basic() {
        // 10 elements: p50 → ceil(5.0) - 1 = 4
        assert_eq!(percentile_index(10, 50), 4);
        // p95 → ceil(9.5) - 1 = 9
        assert_eq!(percentile_index(10, 95), 9);
        // p99 → ceil(9.9) - 1 = 9
        assert_eq!(percentile_index(10, 99), 9);
    }

    #[test]
    fn stats_percentiles_correct() {
        let stats = LoadTestStats::new();
        // Push latencies 1..=100 µs (sorted order to make assertions clear)
        for i in 1u64..=100 {
            stats.record(i, true);
        }
        let report = stats.report(10.0);
        // p50 of 1..=100 → 50th value (index 49) → 50µs
        assert_eq!(report.p50_us, 50);
        // p95 → ceil(95.0) - 1 = 94 → value 95
        assert_eq!(report.p95_us, 95);
        // p99 → ceil(99.0) - 1 = 98 → value 99
        assert_eq!(report.p99_us, 99);
    }

    #[test]
    fn stats_error_rate() {
        let stats = LoadTestStats::new();
        stats.record(100, true);
        stats.record(200, false);
        stats.record(150, false);
        stats.record(120, true);
        let report = stats.report(1.0);
        assert_eq!(report.total_requests, 4);
        assert_eq!(report.failed, 2);
        assert_eq!(report.successful, 2);
        // 2 failed out of 4 → 50%
        assert!((report.error_rate_pct - 50.0).abs() < 0.001);
    }

    #[test]
    fn stats_zero_requests() {
        let stats = LoadTestStats::new();
        let report = stats.report(5.0);
        assert_eq!(report.total_requests, 0);
        assert_eq!(report.p50_us, 0);
        assert_eq!(report.p99_us, 0);
        assert!((report.error_rate_pct).abs() < 0.001);
        assert!((report.rps).abs() < 0.001);
    }
}
