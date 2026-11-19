//! Momentum ignition detection.
//!
//! Momentum ignition is a manipulation strategy where a trader places a burst of
//! aggressive market orders to move the price significantly, trigger stop-loss
//! orders and liquidations, and then reverses their position to profit from the
//! artificially created price movement.
//!
//! # Detection algorithm
//!
//! 1. Track aggressive fill events per user, per symbol, with timestamps.
//! 2. Detect a "burst" when a single user generates fills totaling ≥ `min_notional`
//!    that move the price ≥ `price_move_threshold` within `burst_window_secs`.
//! 3. After detecting a burst, watch for a reversal (opposite side fills from the
//!    same user) within `reversal_window_secs`.
//! 4. If reversal detected → emit `AlertType::MomentumIgnition`.
//!
//! # Reference
//! IOSCO "Investigating and Prosecuting Market Manipulation" (2013), Section 3.2:
//! "Momentum ignition — entering orders or a series of orders designed to start
//! or exacerbate a trend, and to encourage other participants to accelerate or
//! extend the trend in order to create an opportunity to liquidate/close a
//! position at a favorable price."

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tracing::info;
use uuid::Uuid;

use exchange_common::types::{Side, Symbol, UserId};

use crate::alert::{AlertSeverity, AlertType, AutoAction, SurveillanceAlert};

// ──────────────────────────────────────────────────────────────────────────────
// Public alert payload
// ──────────────────────────────────────────────────────────────────────────────

/// Detailed data attached to a momentum ignition alert.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MomentumIgnitionAlert {
    pub user_id: UserId,
    pub symbol: Symbol,
    /// Side of the initial burst (the aggressive direction).
    pub burst_side: Side,
    /// Total notional volume of the burst.
    pub burst_notional: Decimal,
    /// Price at the start of the burst.
    pub price_at_start: Decimal,
    /// Price at the peak of the burst (before reversal).
    pub price_at_peak: Decimal,
    /// Percentage price move during the burst.
    pub price_move_pct: Decimal,
    /// How quickly the reversal occurred (seconds after burst).
    pub reversal_delay_secs: i64,
    pub detected_at: DateTime<Utc>,
}

// ──────────────────────────────────────────────────────────────────────────────
// Internal tracking
// ──────────────────────────────────────────────────────────────────────────────

/// A single aggressive fill event.
#[derive(Debug, Clone)]
struct AggressiveFill {
    side: Side,
    price: Decimal,
    notional: Decimal, // price * qty
    timestamp: DateTime<Utc>,
}

/// Per-user, per-symbol tracking state.
#[derive(Debug, Clone)]
struct UserSymbolState {
    /// Recent aggressive fills.
    fills: Vec<AggressiveFill>,
    /// Detected burst awaiting reversal.
    pending_burst: Option<PendingBurst>,
}

#[derive(Debug, Clone)]
struct PendingBurst {
    side: Side,
    notional: Decimal,
    price_at_start: Decimal,
    price_at_peak: Decimal,
    price_move_pct: Decimal,
    detected_at: DateTime<Utc>,
}

// ──────────────────────────────────────────────────────────────────────────────
// Detector
// ──────────────────────────────────────────────────────────────────────────────

/// Detects momentum ignition manipulation patterns.
pub struct MomentumIgnitionDetector {
    /// Minimum price move (as fraction, e.g. 0.01 = 1%) to qualify as a burst.
    price_move_threshold: Decimal,
    /// Window in seconds to detect a price-moving burst.
    burst_window_secs: i64,
    /// Window in seconds after burst to detect a reversal.
    reversal_window_secs: i64,
    /// Minimum notional value of fills to qualify as a burst.
    min_burst_notional: Decimal,
    /// Per-(user, symbol) tracking.
    states: HashMap<(UserId, Symbol), UserSymbolState>,
}

impl MomentumIgnitionDetector {
    pub fn new(
        price_move_threshold: Decimal,
        burst_window_secs: i64,
        reversal_window_secs: i64,
        min_burst_notional: Decimal,
    ) -> Self {
        Self {
            price_move_threshold,
            burst_window_secs,
            reversal_window_secs,
            min_burst_notional,
            states: HashMap::new(),
        }
    }

    /// Create a detector with sensible defaults.
    /// - 1% price move threshold
    /// - 60s burst window
    /// - 120s reversal window
    /// - $10,000 minimum burst notional
    pub fn with_defaults() -> Self {
        Self::new(dec!(0.01), 60, 120, dec!(10000))
    }

    /// Record a trade (fill) for a user acting as the taker (aggressor).
    pub fn on_taker_fill(
        &mut self,
        user_id: UserId,
        symbol: &Symbol,
        side: Side,
        price: Decimal,
        qty: Decimal,
        timestamp: DateTime<Utc>,
    ) {
        let key = (user_id, symbol.clone());
        let state = self.states.entry(key).or_insert_with(|| UserSymbolState {
            fills: Vec::new(),
            pending_burst: None,
        });

        let notional = price * qty;
        state.fills.push(AggressiveFill {
            side,
            price,
            notional,
            timestamp,
        });
    }

    /// Check a specific user for momentum ignition patterns.
    /// Returns an alert if the full pattern (burst + reversal) is detected.
    pub fn check_user(
        &mut self,
        user_id: UserId,
        symbol: &Symbol,
        now: DateTime<Utc>,
    ) -> Option<SurveillanceAlert> {
        let key = (user_id, symbol.clone());
        let state = self.states.get_mut(&key)?;

        // Prune old fills.
        let max_window = Duration::seconds(self.burst_window_secs + self.reversal_window_secs);
        state.fills.retain(|f| now - f.timestamp < max_window);

        // Check for reversal of a pending burst.
        if let Some(burst) = state.pending_burst.clone() {
            let reversal_deadline = burst.detected_at + Duration::seconds(self.reversal_window_secs);
            if now > reversal_deadline {
                // Expired — no reversal detected.
                state.pending_burst = None;
            } else {
                // Look for reversal fills (opposite side) after the burst.
                let opposite_side = match burst.side {
                    Side::Buy => Side::Sell,
                    Side::Sell => Side::Buy,
                };
                let reversal_notional: Decimal = state
                    .fills
                    .iter()
                    .filter(|f| f.side == opposite_side && f.timestamp > burst.detected_at)
                    .map(|f| f.notional)
                    .sum();

                // Reversal = at least 50% of burst notional on opposite side.
                if reversal_notional >= burst.notional * dec!(0.5) {
                    let reversal_delay = (now - burst.detected_at).num_seconds();
                    let burst_side = burst.side;
                    let alert_data = MomentumIgnitionAlert {
                        user_id,
                        symbol: symbol.clone(),
                        burst_side,
                        burst_notional: burst.notional,
                        price_at_start: burst.price_at_start,
                        price_at_peak: burst.price_at_peak,
                        price_move_pct: burst.price_move_pct,
                        reversal_delay_secs: reversal_delay,
                        detected_at: now,
                    };

                    info!(
                        %user_id,
                        %symbol,
                        burst_side = ?burst_side,
                        price_move_pct = %burst.price_move_pct,
                        reversal_delay = reversal_delay,
                        "momentum ignition detected"
                    );

                    state.pending_burst = None;

                    let side_str = match burst_side {
                        Side::Buy => "buy",
                        Side::Sell => "sell",
                    };

                    return Some(SurveillanceAlert {
                        id: Uuid::new_v4(),
                        alert_type: AlertType::MomentumIgnition(alert_data),
                        severity: AlertSeverity::High,
                        user_ids: vec![user_id],
                        symbol: Some(symbol.clone()),
                        description: format!(
                            "Momentum ignition: {} burst moved price {:.2}%, reversed within {}s",
                            side_str,
                            burst.price_move_pct * dec!(100),
                            reversal_delay,
                        ),
                        auto_action: AutoAction::CancelOrders,
                        created_at: now,
                        reviewed: false,
                    });
                }
            }
        }

        // No pending burst — try to detect a new burst.
        if state.pending_burst.is_none() {
            let burst_start = now - Duration::seconds(self.burst_window_secs);

            for side in [Side::Buy, Side::Sell] {
                let recent_fills: Vec<&AggressiveFill> = state
                    .fills
                    .iter()
                    .filter(|f| f.side == side && f.timestamp >= burst_start)
                    .collect();

                if recent_fills.is_empty() {
                    continue;
                }

                let total_notional: Decimal = recent_fills.iter().map(|f| f.notional).sum();
                if total_notional < self.min_burst_notional {
                    continue;
                }

                // Check price move from first to last fill.
                let first_price = recent_fills.first().unwrap().price;
                let last_price = recent_fills.last().unwrap().price;

                if first_price.is_zero() {
                    continue;
                }

                let price_change = match side {
                    Side::Buy => (last_price - first_price) / first_price,
                    Side::Sell => (first_price - last_price) / first_price,
                };

                if price_change >= self.price_move_threshold {
                    state.pending_burst = Some(PendingBurst {
                        side,
                        notional: total_notional,
                        price_at_start: first_price,
                        price_at_peak: last_price,
                        price_move_pct: price_change,
                        detected_at: now,
                    });
                    break;
                }
            }
        }

        None
    }

    /// Remove stale tracking state for users who haven't traded recently.
    pub fn cleanup_old(&mut self, now: DateTime<Utc>) {
        let max_window = Duration::seconds(self.burst_window_secs + self.reversal_window_secs + 60);
        self.states.retain(|_, state| {
            state.fills.iter().any(|f| now - f.timestamp < max_window)
                || state.pending_burst.is_some()
        });
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn user() -> UserId {
        Uuid::new_v4()
    }

    fn sym() -> Symbol {
        Symbol::new("BTCUSDT")
    }

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    #[test]
    fn burst_plus_reversal_triggers_alert() {
        let mut det = MomentumIgnitionDetector::new(
            dec!(0.01),  // 1% threshold
            60,          // 60s burst window
            120,         // 120s reversal window
            dec!(1000),  // $1000 min notional
        );

        let u = user();
        let s = sym();
        let t = now();

        // Burst: aggressive buys moving price up 2% ($50,000 → $51,000).
        det.on_taker_fill(u, &s, Side::Buy, dec!(50000), dec!(0.1), t);
        det.on_taker_fill(u, &s, Side::Buy, dec!(50500), dec!(0.1), t + Duration::seconds(10));
        det.on_taker_fill(u, &s, Side::Buy, dec!(51000), dec!(0.1), t + Duration::seconds(20));

        // First check — should detect burst but no reversal yet.
        let alert = det.check_user(u, &s, t + Duration::seconds(30));
        assert!(alert.is_none(), "no reversal yet");

        // Reversal: aggressive sells.
        det.on_taker_fill(u, &s, Side::Sell, dec!(51000), dec!(0.1), t + Duration::seconds(40));
        det.on_taker_fill(u, &s, Side::Sell, dec!(50800), dec!(0.1), t + Duration::seconds(50));

        // Check again — should now fire.
        let alert = det.check_user(u, &s, t + Duration::seconds(55));
        assert!(alert.is_some(), "reversal detected → alert should fire");

        let alert = alert.unwrap();
        assert!(matches!(alert.alert_type, AlertType::MomentumIgnition(_)));
        assert_eq!(alert.severity, AlertSeverity::High);
        assert_eq!(alert.user_ids, vec![u]);
    }

    #[test]
    fn burst_without_reversal_no_alert() {
        let mut det = MomentumIgnitionDetector::new(
            dec!(0.01), 60, 120, dec!(1000),
        );

        let u = user();
        let s = sym();
        let t = now();

        // Burst: aggressive buys.
        det.on_taker_fill(u, &s, Side::Buy, dec!(50000), dec!(0.1), t);
        det.on_taker_fill(u, &s, Side::Buy, dec!(51000), dec!(0.1), t + Duration::seconds(10));

        // Check — detects burst.
        det.check_user(u, &s, t + Duration::seconds(30));

        // No reversal — wait past reversal window.
        let alert = det.check_user(u, &s, t + Duration::seconds(200));
        assert!(alert.is_none(), "no reversal → no alert");
    }

    #[test]
    fn small_notional_no_burst() {
        let mut det = MomentumIgnitionDetector::new(
            dec!(0.01), 60, 120, dec!(100000), // high min notional
        );

        let u = user();
        let s = sym();
        let t = now();

        // Small fills below threshold.
        det.on_taker_fill(u, &s, Side::Buy, dec!(50000), dec!(0.001), t);
        det.on_taker_fill(u, &s, Side::Buy, dec!(51000), dec!(0.001), t + Duration::seconds(10));

        let alert = det.check_user(u, &s, t + Duration::seconds(30));
        assert!(alert.is_none());
    }

    #[test]
    fn small_price_move_no_burst() {
        let mut det = MomentumIgnitionDetector::new(
            dec!(0.05), // 5% threshold — high
            60, 120, dec!(1000),
        );

        let u = user();
        let s = sym();
        let t = now();

        // 1% price move — below 5% threshold.
        det.on_taker_fill(u, &s, Side::Buy, dec!(50000), dec!(0.1), t);
        det.on_taker_fill(u, &s, Side::Buy, dec!(50500), dec!(0.1), t + Duration::seconds(10));

        let alert = det.check_user(u, &s, t + Duration::seconds(30));
        assert!(alert.is_none());
    }

    #[test]
    fn organic_buy_wave_without_reversal_no_alert() {
        let mut det = MomentumIgnitionDetector::with_defaults();
        let u = user();
        let s = sym();
        let t = now();

        // Large organic buy wave — price moves 2%, but user doesn't reverse.
        for i in 0..10 {
            let price = dec!(50000) + Decimal::from(i) * dec!(100);
            det.on_taker_fill(u, &s, Side::Buy, price, dec!(0.5), t + Duration::seconds(i * 5));
        }

        // Multiple checks — no reversal, no alert.
        for i in 0..5 {
            let alert = det.check_user(u, &s, t + Duration::seconds(60 + i * 30));
            assert!(alert.is_none());
        }
    }
}
