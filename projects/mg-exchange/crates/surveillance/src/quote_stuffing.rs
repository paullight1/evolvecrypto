//! Quote-stuffing detection.
//!
//! Quote stuffing is the practice of flooding an exchange with a large number
//! of orders (and cancellations) in a very short time, overwhelming the order
//! book and market-data feed to create artificial latency for competing traders.
//!
//! Detection: we count the number of `on_order` events per user inside a
//! one-second sliding window.  If the count exceeds `max_orders_per_second` an
//! alert is emitted.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use tracing::debug;

use exchange_common::types::UserId;

use crate::alert::{AlertSeverity, AutoAction, QuoteStuffingAlert, SurveillanceAlert};

// ──────────────────────────────────────────────────────────────────────────────
// Public detector
// ──────────────────────────────────────────────────────────────────────────────

/// Tracks per-user order submission rates and emits alerts when a user
/// exceeds `max_orders_per_second`.
pub struct QuoteStuffingDetector {
    /// Timestamps of recent order events keyed by user ID.
    order_rates: HashMap<UserId, Vec<DateTime<Utc>>>,
    /// Maximum number of orders per second before an alert fires.
    max_orders_per_second: u32,
}

impl QuoteStuffingDetector {
    pub fn new(max_orders_per_second: u32) -> Self {
        Self {
            order_rates: HashMap::new(),
            max_orders_per_second,
        }
    }

    /// Record that user `user_id` submitted an order at `timestamp`.
    pub fn on_order(&mut self, user_id: UserId, timestamp: DateTime<Utc>) {
        self.order_rates.entry(user_id).or_default().push(timestamp);
    }

    /// Check whether `user_id` is exceeding the rate limit.
    ///
    /// The check counts all recorded timestamps that fall within the 1-second
    /// window `[now - 1s, now]`.  If the count exceeds the threshold an alert
    /// is returned.
    pub fn check_rate(
        &self,
        user_id: UserId,
        now: DateTime<Utc>,
    ) -> Option<SurveillanceAlert> {
        let window_start = now - Duration::seconds(1);
        let timestamps = self.order_rates.get(&user_id)?;

        let count = timestamps
            .iter()
            .filter(|&&ts| ts >= window_start && ts <= now)
            .count() as u32;

        if count <= self.max_orders_per_second {
            return None;
        }

        debug!(
            %user_id,
            count,
            threshold = self.max_orders_per_second,
            "quote stuffing alert triggered"
        );

        let raw = QuoteStuffingAlert {
            user_id,
            orders_per_second: count,
            threshold: self.max_orders_per_second,
            detected_at: now,
        };

        let severity = severity_from_rate(count, self.max_orders_per_second);
        let description = format!(
            "User {user_id} submitted {count} orders/s (threshold {})",
            self.max_orders_per_second,
        );

        Some(SurveillanceAlert {
            id: uuid::Uuid::new_v4(),
            alert_type: crate::alert::AlertType::QuoteStuffing(raw),
            severity,
            user_ids: vec![user_id],
            symbol: None,
            description,
            auto_action: AutoAction::ReduceRateLimit,
            created_at: now,
            reviewed: false,
        })
    }

    /// Drop all timestamps older than 1 second to keep memory bounded.
    pub fn cleanup_old(&mut self, now: DateTime<Utc>) {
        let cutoff = now - Duration::seconds(1);
        for ts_vec in self.order_rates.values_mut() {
            ts_vec.retain(|&ts| ts >= cutoff);
        }
        self.order_rates.retain(|_, v| !v.is_empty());
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────────────

fn severity_from_rate(count: u32, threshold: u32) -> AlertSeverity {
    let ratio = count as f64 / threshold as f64;
    if ratio >= 10.0 {
        AlertSeverity::Critical
    } else if ratio >= 5.0 {
        AlertSeverity::High
    } else if ratio >= 2.0 {
        AlertSeverity::Medium
    } else {
        AlertSeverity::Low
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn detector() -> QuoteStuffingDetector {
        QuoteStuffingDetector::new(10)
    }

    fn user() -> UserId {
        Uuid::new_v4()
    }

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    #[test]
    fn no_alert_below_threshold() {
        let mut det = detector();
        let u = user();
        let t = now();

        for _ in 0..10 {
            det.on_order(u, t);
        }

        assert!(det.check_rate(u, t).is_none());
    }

    #[test]
    fn alert_fires_above_threshold() {
        let mut det = detector();
        let u = user();
        let t = now();

        for _ in 0..15 {
            det.on_order(u, t);
        }

        let alert = det.check_rate(u, t);
        assert!(alert.is_some());
        let alert = alert.unwrap();
        assert_eq!(alert.user_ids, vec![u]);
    }

    #[test]
    fn orders_outside_one_second_window_are_excluded() {
        let mut det = detector();
        let u = user();
        let t = now();

        // 15 old orders — placed 2 seconds ago, outside the 1-second window.
        for _ in 0..15 {
            det.on_order(u, t - Duration::seconds(2));
        }
        // 5 recent orders — inside the window, below threshold.
        for _ in 0..5 {
            det.on_order(u, t);
        }

        assert!(det.check_rate(u, t).is_none());
    }

    #[test]
    fn cleanup_removes_old_timestamps() {
        let mut det = detector();
        let u = user();
        let t = now();

        for _ in 0..5 {
            det.on_order(u, t - Duration::seconds(5));
        }

        assert!(det.order_rates.contains_key(&u));
        det.cleanup_old(t);
        assert!(!det.order_rates.contains_key(&u));
    }

    #[test]
    fn severity_low_just_above_threshold() {
        let mut det = QuoteStuffingDetector::new(10);
        let u = user();
        let t = now();

        // 11 orders — 1.1× the threshold → Low severity.
        for _ in 0..11 {
            det.on_order(u, t);
        }

        let alert = det.check_rate(u, t).unwrap();
        assert_eq!(alert.severity, AlertSeverity::Low);
    }

    #[test]
    fn severity_critical_at_10x_threshold() {
        let mut det = QuoteStuffingDetector::new(10);
        let u = user();
        let t = now();

        // 100 orders — 10× the threshold → Critical severity.
        for _ in 0..100 {
            det.on_order(u, t);
        }

        let alert = det.check_rate(u, t).unwrap();
        assert_eq!(alert.severity, AlertSeverity::Critical);
    }

    #[test]
    fn unknown_user_returns_none() {
        let det = detector();
        let u = user();
        assert!(det.check_rate(u, now()).is_none());
    }
}
