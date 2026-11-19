//! Front-running detection.
//!
//! Detects when user A consistently places orders within a narrow time window
//! before user B on the same side and symbol, filling at a better price.
//!
//! # Algorithm
//! 1. Track all fills with microsecond timestamps.
//! 2. For each fill, check if another user filled on the same side + symbol
//!    within `correlation_window_ms` (default 50ms) and at a better price.
//! 3. Maintain a per-(A, B) pair score. After `min_occurrences` within
//!    `scoring_window_secs`, emit an alert.
//!
//! # Reference
//! - SEC Rule 10b-5: front-running is a form of insider trading.
//! - IOSCO "Market Manipulation" (2013), Section 3.1.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use exchange_common::types::{Side, Symbol, UserId};

use crate::alert::{AlertSeverity, AlertType, AutoAction, SurveillanceAlert};

/// Alert payload for detected front-running.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FrontRunningAlert {
    /// The suspected front-runner.
    pub front_runner_id: UserId,
    /// The victim user.
    pub victim_id: UserId,
    /// Number of correlated fills detected.
    pub occurrences: u32,
    /// Average time difference (ms) between front-runner and victim fills.
    pub avg_latency_ms: i64,
    pub detected_at: DateTime<Utc>,
}

/// A recorded fill for correlation analysis.
#[derive(Debug, Clone)]
struct FillRecord {
    user_id: UserId,
    symbol: Symbol,
    side: Side,
    price: Decimal,
    timestamp: DateTime<Utc>,
}

/// Correlation score between two users.
#[derive(Debug, Clone, Default)]
struct PairScore {
    /// Number of correlated front-run events.
    occurrences: u32,
    /// Sum of latency deltas for averaging.
    total_latency_ms: i64,
    /// Timestamp of the first occurrence in the current window.
    window_start: Option<DateTime<Utc>>,
}

/// Detects front-running patterns between users.
pub struct FrontRunningDetector {
    /// Max time between fills to consider them correlated (ms).
    correlation_window_ms: i64,
    /// Minimum correlated events to trigger an alert.
    min_occurrences: u32,
    /// Window for accumulating occurrences (seconds).
    scoring_window_secs: i64,
    /// Recent fills buffer.
    recent_fills: Vec<FillRecord>,
    /// Per-(front_runner, victim) pair scores.
    pair_scores: HashMap<(UserId, UserId), PairScore>,
}

impl FrontRunningDetector {
    pub fn new(
        correlation_window_ms: i64,
        min_occurrences: u32,
        scoring_window_secs: i64,
    ) -> Self {
        Self {
            correlation_window_ms,
            min_occurrences,
            scoring_window_secs,
            recent_fills: Vec::new(),
            pair_scores: HashMap::new(),
        }
    }

    /// Default: 50ms correlation, 5 occurrences in 1 hour.
    pub fn with_defaults() -> Self {
        Self::new(50, 5, 3600)
    }

    /// Record a fill event.
    pub fn on_fill(
        &mut self,
        user_id: UserId,
        symbol: &Symbol,
        side: Side,
        price: Decimal,
        timestamp: DateTime<Utc>,
    ) {
        // Check if any recent fill from another user on the same side+symbol
        // occurred just before this one (within correlation window).
        let window = Duration::milliseconds(self.correlation_window_ms);

        for prev in self.recent_fills.iter().rev() {
            // Only look back within the correlation window.
            if timestamp - prev.timestamp > window {
                break;
            }
            // Same user = not front-running.
            if prev.user_id == user_id {
                continue;
            }
            // Must be same side and symbol.
            if prev.side != side || prev.symbol != *symbol {
                continue;
            }
            // The earlier fill (prev) got a better price → prev is the front-runner.
            let is_better_price = match side {
                Side::Buy => prev.price < price,  // front-runner bought cheaper
                Side::Sell => prev.price > price,  // front-runner sold higher
            };
            if is_better_price {
                let latency_ms = (timestamp - prev.timestamp).num_milliseconds();
                let key = (prev.user_id, user_id); // (front_runner, victim)
                let score = self.pair_scores.entry(key).or_default();

                if score.window_start.is_none() {
                    score.window_start = Some(timestamp);
                }

                // Reset if window expired.
                if let Some(start) = score.window_start {
                    if timestamp - start > Duration::seconds(self.scoring_window_secs) {
                        *score = PairScore::default();
                        score.window_start = Some(timestamp);
                    }
                }

                score.occurrences += 1;
                score.total_latency_ms += latency_ms;
            }
        }

        self.recent_fills.push(FillRecord {
            user_id,
            symbol: symbol.clone(),
            side,
            price,
            timestamp,
        });
    }

    /// Check for any pairs that have reached the alert threshold.
    pub fn check_alerts(&mut self, now: DateTime<Utc>) -> Vec<SurveillanceAlert> {
        let mut alerts = Vec::new();

        let threshold = self.min_occurrences;
        let pairs_to_alert: Vec<_> = self.pair_scores
            .iter()
            .filter(|(_, score)| score.occurrences >= threshold)
            .map(|(&(fr, victim), score)| {
                let avg_latency = if score.occurrences > 0 {
                    score.total_latency_ms / score.occurrences as i64
                } else {
                    0
                };
                (fr, victim, score.occurrences, avg_latency)
            })
            .collect();

        for (front_runner, victim, occurrences, avg_latency_ms) in pairs_to_alert {
            alerts.push(SurveillanceAlert {
                id: Uuid::new_v4(),
                alert_type: AlertType::FrontRunning(FrontRunningAlert {
                    front_runner_id: front_runner,
                    victim_id: victim,
                    occurrences,
                    avg_latency_ms,
                    detected_at: now,
                }),
                severity: AlertSeverity::High,
                user_ids: vec![front_runner],
                symbol: None,
                description: format!(
                    "Front-running: {occurrences} correlated fills ahead of victim within {avg_latency_ms}ms avg",
                ),
                auto_action: AutoAction::CancelOrders,
                created_at: now,
                reviewed: false,
            });

            // Reset the pair score after alerting.
            self.pair_scores.remove(&(front_runner, victim));
        }

        alerts
    }

    /// Remove old fills and expired pair scores.
    pub fn cleanup_old(&mut self, now: DateTime<Utc>) {
        let cutoff = now - Duration::seconds(self.scoring_window_secs + 60);
        self.recent_fills.retain(|f| f.timestamp > cutoff);
        self.pair_scores.retain(|_, s| {
            s.window_start
                .map(|start| now - start < Duration::seconds(self.scoring_window_secs + 60))
                .unwrap_or(false)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn now() -> DateTime<Utc> { Utc::now() }

    #[test]
    fn front_run_sequence_triggers_after_threshold() {
        let mut det = FrontRunningDetector::new(100, 3, 3600); // 100ms window, 3 occurrences
        let fr = Uuid::new_v4(); // front-runner
        let victim = Uuid::new_v4();
        let sym = Symbol::new("BTCUSDT");
        let t = now();

        // 3 correlated fills: fr buys at lower price, victim buys 40ms later at higher price.
        for i in 0..3i64 {
            let base_t = t + Duration::seconds(i * 10);
            det.on_fill(fr, &sym, Side::Buy, dec!(50000), base_t);
            det.on_fill(victim, &sym, Side::Buy, dec!(50010), base_t + Duration::milliseconds(40));
        }

        let alerts = det.check_alerts(t + Duration::seconds(30));
        assert_eq!(alerts.len(), 1);
        assert!(matches!(alerts[0].alert_type, AlertType::FrontRunning(_)));
    }

    #[test]
    fn below_threshold_no_alert() {
        let mut det = FrontRunningDetector::new(100, 5, 3600); // need 5 occurrences
        let fr = Uuid::new_v4();
        let victim = Uuid::new_v4();
        let sym = Symbol::new("BTCUSDT");
        let t = now();

        // Only 2 correlated fills — below threshold.
        for i in 0..2i64 {
            let base_t = t + Duration::seconds(i * 10);
            det.on_fill(fr, &sym, Side::Buy, dec!(50000), base_t);
            det.on_fill(victim, &sym, Side::Buy, dec!(50010), base_t + Duration::milliseconds(40));
        }

        let alerts = det.check_alerts(t + Duration::seconds(30));
        assert!(alerts.is_empty());
    }

    #[test]
    fn same_user_not_front_running() {
        let mut det = FrontRunningDetector::new(100, 1, 3600);
        let user = Uuid::new_v4();
        let sym = Symbol::new("BTCUSDT");
        let t = now();

        det.on_fill(user, &sym, Side::Buy, dec!(50000), t);
        det.on_fill(user, &sym, Side::Buy, dec!(50010), t + Duration::milliseconds(40));

        let alerts = det.check_alerts(t + Duration::seconds(5));
        assert!(alerts.is_empty());
    }

    #[test]
    fn opposite_sides_not_correlated() {
        let mut det = FrontRunningDetector::new(100, 1, 3600);
        let fr = Uuid::new_v4();
        let victim = Uuid::new_v4();
        let sym = Symbol::new("BTCUSDT");
        let t = now();

        // Different sides — not front-running.
        det.on_fill(fr, &sym, Side::Buy, dec!(50000), t);
        det.on_fill(victim, &sym, Side::Sell, dec!(50010), t + Duration::milliseconds(40));

        let alerts = det.check_alerts(t + Duration::seconds(5));
        assert!(alerts.is_empty());
    }
}
