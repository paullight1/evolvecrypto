//! Spoofing and layering detection.
//!
//! Spoofing: a trader places large orders to create the appearance of demand /
//! supply, then cancels them before execution to move the price and profit on
//! a position in the opposite direction.
//!
//! Detection heuristic: within a rolling time window, if the user's
//! cancel-to-placed ratio exceeds `cancel_ratio_threshold` AND the total number
//! of orders placed meets the `min_orders` floor, we emit a [`SpoofingAlert`].

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tracing::{debug, trace};

use exchange_common::types::{OrderId, Side, UserId};

use crate::alert::{AlertSeverity, AutoAction, SpoofingAlert, SurveillanceAlert};

// ──────────────────────────────────────────────────────────────────────────────
// Internal types
// ──────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct OrderEvent {
    order_id: OrderId,
    #[allow(dead_code)]
    side: Side,
    #[allow(dead_code)]
    qty: Decimal,
    placed_at: DateTime<Utc>,
    cancelled_at: Option<DateTime<Utc>>,
    filled: bool,
}

// ──────────────────────────────────────────────────────────────────────────────
// Public detector
// ──────────────────────────────────────────────────────────────────────────────

/// Tracks order events per user and emits alerts when the cancel ratio inside
/// the rolling window breaches the configured threshold.
pub struct SpoofingDetector {
    /// Order events keyed by user ID.
    order_events: HashMap<UserId, Vec<OrderEvent>>,
    /// Fraction of placed orders that must be cancelled to trigger ([0, 1]).
    cancel_ratio_threshold: Decimal,
    /// Width of the rolling window in seconds.
    window_secs: u64,
    /// Minimum number of placed orders required before the ratio is evaluated.
    min_orders: usize,
}

impl SpoofingDetector {
    /// Create a new detector.
    ///
    /// * `cancel_ratio_threshold` — e.g. `dec!(0.95)` means ≥95 % cancellations.
    /// * `window_secs` — rolling window length in seconds.
    /// * `min_orders` — ignore users with fewer than this many orders in the window.
    pub fn new(cancel_ratio_threshold: Decimal, window_secs: u64, min_orders: usize) -> Self {
        Self {
            order_events: HashMap::new(),
            cancel_ratio_threshold,
            window_secs,
            min_orders,
        }
    }

    /// Record that a new order was placed.
    pub fn on_order_placed(
        &mut self,
        user_id: UserId,
        order_id: OrderId,
        side: Side,
        qty: Decimal,
        timestamp: DateTime<Utc>,
    ) {
        trace!(%user_id, %order_id, ?side, %qty, "spoofing: order placed");
        let event = OrderEvent {
            order_id,
            side,
            qty,
            placed_at: timestamp,
            cancelled_at: None,
            filled: false,
        };
        self.order_events.entry(user_id).or_default().push(event);
    }

    /// Record that an order was cancelled.
    pub fn on_order_cancelled(
        &mut self,
        user_id: UserId,
        order_id: OrderId,
        timestamp: DateTime<Utc>,
    ) {
        trace!(%user_id, %order_id, "spoofing: order cancelled");
        if let Some(events) = self.order_events.get_mut(&user_id) {
            if let Some(ev) = events.iter_mut().find(|e| e.order_id == order_id) {
                ev.cancelled_at = Some(timestamp);
            }
        }
    }

    /// Record that an order was (at least partially) filled.
    pub fn on_order_filled(&mut self, user_id: UserId, order_id: OrderId) {
        trace!(%user_id, %order_id, "spoofing: order filled");
        if let Some(events) = self.order_events.get_mut(&user_id) {
            if let Some(ev) = events.iter_mut().find(|e| e.order_id == order_id) {
                ev.filled = true;
            }
        }
    }

    /// Check whether `user_id` shows a suspicious cancel ratio in the current
    /// window. Returns `Some(SurveillanceAlert)` if the threshold is breached.
    pub fn check_user(
        &self,
        user_id: UserId,
        now: DateTime<Utc>,
    ) -> Option<SurveillanceAlert> {
        let window_start = now - Duration::seconds(self.window_secs as i64);
        let events = self.order_events.get(&user_id)?;

        // Only consider orders placed inside the rolling window.
        let in_window: Vec<&OrderEvent> = events
            .iter()
            .filter(|e| e.placed_at >= window_start)
            .collect();

        let placed = in_window.len();
        if placed < self.min_orders {
            return None;
        }

        // An order counts as cancelled if it was explicitly cancelled and never
        // reached a fill.
        let cancelled = in_window
            .iter()
            .filter(|e| e.cancelled_at.is_some() && !e.filled)
            .count();

        let cancel_ratio = Decimal::from(cancelled) / Decimal::from(placed);

        if cancel_ratio < self.cancel_ratio_threshold {
            return None;
        }

        debug!(
            %user_id,
            placed,
            cancelled,
            %cancel_ratio,
            threshold = %self.cancel_ratio_threshold,
            "spoofing alert triggered"
        );

        let raw = SpoofingAlert {
            user_id,
            cancel_ratio,
            orders_placed: placed,
            orders_cancelled: cancelled,
            window_start,
            window_end: now,
        };

        let severity = severity_from_ratio(cancel_ratio);
        let description = format!(
            "User {user_id} cancelled {cancel_ratio:.0}% of {placed} orders in the last \
             {} seconds (threshold {:.0}%)",
            self.window_secs,
            self.cancel_ratio_threshold * dec!(100),
        );

        Some(SurveillanceAlert {
            id: uuid::Uuid::new_v4(),
            alert_type: crate::alert::AlertType::Spoofing(raw),
            severity,
            user_ids: vec![user_id],
            symbol: None,
            description,
            auto_action: auto_action_for_severity(severity),
            created_at: now,
            reviewed: false,
        })
    }

    /// Drop all events older than the rolling window to keep memory bounded.
    pub fn cleanup_old(&mut self, now: DateTime<Utc>) {
        let cutoff = now - Duration::seconds(self.window_secs as i64);
        for events in self.order_events.values_mut() {
            events.retain(|e| e.placed_at >= cutoff);
        }
        // Remove users with no remaining events.
        self.order_events.retain(|_, v| !v.is_empty());
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────────────

fn severity_from_ratio(ratio: Decimal) -> AlertSeverity {
    if ratio >= dec!(0.99) {
        AlertSeverity::Critical
    } else if ratio >= dec!(0.97) {
        AlertSeverity::High
    } else if ratio >= dec!(0.95) {
        AlertSeverity::Medium
    } else {
        AlertSeverity::Low
    }
}

fn auto_action_for_severity(severity: AlertSeverity) -> AutoAction {
    match severity {
        AlertSeverity::Critical => AutoAction::FreezeAccount,
        AlertSeverity::High => AutoAction::CancelOrders,
        AlertSeverity::Medium | AlertSeverity::Low => AutoAction::ReduceRateLimit,
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    use exchange_common::types::Side;

    use super::*;

    fn detector() -> SpoofingDetector {
        SpoofingDetector::new(dec!(0.90), 60, 5)
    }

    fn user() -> UserId {
        Uuid::new_v4()
    }

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    #[test]
    fn no_alert_below_min_orders() {
        let mut det = detector();
        let u = user();
        let t = now();

        for i in 0..4u64 {
            let oid = Uuid::new_v4();
            det.on_order_placed(u, oid, Side::Buy, dec!(1), t);
            det.on_order_cancelled(u, oid, t + Duration::seconds(i as i64 + 1));
        }

        assert!(det.check_user(u, t + Duration::seconds(10)).is_none());
    }

    #[test]
    fn no_alert_below_cancel_threshold() {
        let mut det = detector();
        let u = user();
        let t = now();

        // Place 10 orders, cancel only 5 (50% < 90% threshold).
        for _ in 0..5 {
            let oid = Uuid::new_v4();
            det.on_order_placed(u, oid, Side::Buy, dec!(1), t);
            det.on_order_cancelled(u, oid, t + Duration::seconds(1));
        }
        for _ in 0..5 {
            let oid = Uuid::new_v4();
            det.on_order_placed(u, oid, Side::Buy, dec!(1), t);
            det.on_order_filled(u, oid);
        }

        assert!(det.check_user(u, t + Duration::seconds(10)).is_none());
    }

    #[test]
    fn alert_fires_above_threshold() {
        let mut det = detector();
        let u = user();
        let t = now();

        // Place 10 orders and cancel all → 100% cancel ratio.
        for _ in 0..10 {
            let oid = Uuid::new_v4();
            det.on_order_placed(u, oid, Side::Sell, dec!(2), t);
            det.on_order_cancelled(u, oid, t + Duration::seconds(1));
        }

        let alert = det.check_user(u, t + Duration::seconds(10));
        assert!(alert.is_some());
        let alert = alert.unwrap();
        assert_eq!(alert.user_ids, vec![u]);
    }

    #[test]
    fn filled_orders_are_not_counted_as_cancelled() {
        let u = user();
        let t = now();

        // 9 cancelled + 1 filled. 9/10 = 90% — below the 95% threshold.
        let mut det2 = SpoofingDetector::new(dec!(0.95), 60, 5);

        for _ in 0..9 {
            let oid = Uuid::new_v4();
            det2.on_order_placed(u, oid, Side::Buy, dec!(1), t);
            det2.on_order_cancelled(u, oid, t + Duration::seconds(1));
        }
        let filled_oid = Uuid::new_v4();
        det2.on_order_placed(u, filled_oid, Side::Buy, dec!(1), t);
        det2.on_order_filled(u, filled_oid);

        // 90% cancels < 95% threshold → no alert
        assert!(det2.check_user(u, t + Duration::seconds(5)).is_none());
    }

    #[test]
    fn events_outside_window_are_excluded() {
        let mut det = detector();
        let u = user();
        let t = now();

        // Place 10 orders 2 minutes ago — outside the 60-second window.
        for _ in 0..10 {
            let oid = Uuid::new_v4();
            det.on_order_placed(u, oid, Side::Buy, dec!(1), t - Duration::seconds(120));
            det.on_order_cancelled(u, oid, t - Duration::seconds(119));
        }

        // No alert because the window only covers [now-60s, now].
        assert!(det.check_user(u, t).is_none());
    }

    #[test]
    fn cleanup_removes_stale_events() {
        let mut det = detector();
        let u = user();
        let t = now();

        for _ in 0..6 {
            let oid = Uuid::new_v4();
            det.on_order_placed(u, oid, Side::Buy, dec!(1), t - Duration::seconds(120));
        }

        assert!(det.order_events.contains_key(&u));
        det.cleanup_old(t);
        // All events are older than the 60 s window → user entry removed.
        assert!(!det.order_events.contains_key(&u));
    }

    #[test]
    fn severity_escalates_with_ratio() {
        assert_eq!(severity_from_ratio(dec!(0.91)), AlertSeverity::Low);
        assert_eq!(severity_from_ratio(dec!(0.95)), AlertSeverity::Medium);
        assert_eq!(severity_from_ratio(dec!(0.97)), AlertSeverity::High);
        assert_eq!(severity_from_ratio(dec!(0.99)), AlertSeverity::Critical);
    }

    #[test]
    fn alert_severity_critical_at_100_percent() {
        let mut det = SpoofingDetector::new(dec!(0.90), 60, 5);
        let u = user();
        let t = now();

        for _ in 0..10 {
            let oid = Uuid::new_v4();
            det.on_order_placed(u, oid, Side::Buy, dec!(1), t);
            det.on_order_cancelled(u, oid, t + Duration::seconds(1));
        }

        let alert = det.check_user(u, t + Duration::seconds(2)).unwrap();
        assert_eq!(alert.severity, AlertSeverity::Critical);
        assert_eq!(alert.auto_action, AutoAction::FreezeAccount);
    }
}
