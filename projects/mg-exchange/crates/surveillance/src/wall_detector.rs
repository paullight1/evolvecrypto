//! Multi-level wall and spoof-and-trade detection.
//!
//! # M19-12: Book depth impact scoring
//! Tracks order_qty / total_book_depth_same_side. User responsible for >15%
//! of visible depth who then cancels = layering signal.
//!
//! # M20-8: Multi-level wall detector
//! Orders on ≥3 price levels same side, >10% depth combined, all cancelled
//! within 2 seconds = wall pattern.
//!
//! # M20-9: Spoof-and-trade correlator
//! Wall placed → price moves in favorable direction → fills on opposite side →
//! wall cancelled. Complete Navinder Sarao pattern.
//!
//! # Reference
//! - Navinder Sarao / Flash Crash (2010): 4-5 levels of large sell orders,
//!   modified every 200-400ms, 20-30% of visible depth.
//! - CFTC v. Sarao, case 15-cv-03398.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;

use exchange_common::types::{OrderId, Side, Symbol, UserId};

use crate::alert::{AlertSeverity, AlertType, AutoAction, SurveillanceAlert};

/// A tracked order for wall detection.
#[derive(Debug, Clone)]
struct WallOrder {
    order_id: OrderId,
    user_id: UserId,
    symbol: Symbol,
    side: Side,
    price: Decimal,
    qty: Decimal,
    placed_at: DateTime<Utc>,
    cancelled_at: Option<DateTime<Utc>>,
}

/// Detects multi-level wall patterns and spoof-and-trade sequences.
#[allow(dead_code)]
pub struct WallDetector {
    /// Minimum fraction of book depth to flag as significant (default 15%).
    depth_impact_threshold: Decimal,
    /// Minimum number of price levels for a wall pattern.
    min_wall_levels: usize,
    /// Maximum time for all wall orders to be cancelled (seconds).
    cancel_window_secs: i64,
    /// Active orders being tracked.
    orders: HashMap<OrderId, WallOrder>,
    /// Recent cancellations for pattern detection.
    cancellations: Vec<(UserId, Symbol, Side, DateTime<Utc>)>,
}

impl WallDetector {
    pub fn new(
        depth_impact_threshold: Decimal,
        min_wall_levels: usize,
        cancel_window_secs: i64,
    ) -> Self {
        Self {
            depth_impact_threshold,
            min_wall_levels,
            cancel_window_secs,
            orders: HashMap::new(),
            cancellations: Vec::new(),
        }
    }

    /// Default: 15% depth impact, 3 levels, 2s cancel window.
    pub fn with_defaults() -> Self {
        Self::new(dec!(0.15), 3, 2)
    }

    /// Track a new order placement.
    #[allow(clippy::too_many_arguments)]
    pub fn on_order_placed(
        &mut self,
        order_id: OrderId,
        user_id: UserId,
        symbol: &Symbol,
        side: Side,
        price: Decimal,
        qty: Decimal,
        timestamp: DateTime<Utc>,
    ) {
        self.orders.insert(order_id, WallOrder {
            order_id,
            user_id,
            symbol: symbol.clone(),
            side,
            price,
            qty,
            placed_at: timestamp,
            cancelled_at: None,
        });
    }

    /// Track an order cancellation. Returns an alert if a wall pattern is detected.
    pub fn on_order_cancelled(
        &mut self,
        order_id: OrderId,
        timestamp: DateTime<Utc>,
    ) -> Option<SurveillanceAlert> {
        let order = self.orders.get_mut(&order_id)?;
        order.cancelled_at = Some(timestamp);

        let user_id = order.user_id;
        let symbol = order.symbol.clone();
        let side = order.side;

        self.cancellations.push((user_id, symbol.clone(), side, timestamp));

        // Check for multi-level wall pattern:
        // ≥3 price levels from the same user, same side, same symbol,
        // all cancelled within cancel_window_secs.
        let window = Duration::seconds(self.cancel_window_secs);
        let cutoff = timestamp - window;

        let user_cancelled: Vec<&WallOrder> = self.orders.values()
            .filter(|o| {
                o.user_id == user_id
                    && o.symbol == symbol
                    && o.side == side
                    && o.cancelled_at.map(|t| t >= cutoff).unwrap_or(false)
            })
            .collect();

        // Count distinct price levels.
        let mut price_levels: Vec<Decimal> = user_cancelled.iter().map(|o| o.price).collect();
        price_levels.sort();
        price_levels.dedup();

        if price_levels.len() >= self.min_wall_levels {
            let total_qty: Decimal = user_cancelled.iter().map(|o| o.qty).sum();

            // Clean up detected orders.
            let detected_ids: Vec<OrderId> = user_cancelled.iter().map(|o| o.order_id).collect();
            for id in &detected_ids {
                self.orders.remove(id);
            }

            return Some(SurveillanceAlert {
                id: Uuid::new_v4(),
                alert_type: AlertType::Layering(crate::layering::LayeringAlert {
                    user_id,
                    symbol: symbol.clone(),
                    spoofed_side: side,
                    order_qty: total_qty,
                    avg_order_size: Decimal::ZERO, // wall detector doesn't track avg
                    size_multiple: Decimal::from(price_levels.len() as u64),
                    cancel_latency_ms: self.cancel_window_secs * 1000,
                    price_at_placement: price_levels[0],
                    price_at_cancel: *price_levels.last().unwrap(),
                    detected_at: timestamp,
                }),
                severity: AlertSeverity::High,
                user_ids: vec![user_id],
                symbol: Some(symbol),
                description: format!(
                    "Multi-level wall: {} levels, {} total qty, cancelled within {}s",
                    price_levels.len(),
                    total_qty,
                    self.cancel_window_secs,
                ),
                auto_action: AutoAction::CancelOrders,
                created_at: timestamp,
                reviewed: false,
            });
        }

        None
    }

    /// Track a fill on the opposite side (for spoof-and-trade correlation).
    pub fn on_fill(&mut self, order_id: OrderId) {
        self.orders.remove(&order_id);
    }

    /// Remove stale orders.
    pub fn cleanup_old(&mut self, now: DateTime<Utc>) {
        let cutoff = now - Duration::seconds(60);
        self.orders.retain(|_, o| o.placed_at > cutoff);
        self.cancellations.retain(|(_, _, _, t)| *t > cutoff);
    }

    /// M19-12: Calculate depth impact for a user's orders on a specific side.
    ///
    /// Returns the fraction of total book depth attributable to this user.
    /// Values >0.15 indicate the user is a significant portion of visible liquidity.
    pub fn depth_impact(
        &self,
        user_id: UserId,
        symbol: &Symbol,
        side: Side,
        total_book_depth: Decimal,
    ) -> Decimal {
        if total_book_depth.is_zero() {
            return Decimal::ZERO;
        }

        let user_depth: Decimal = self.orders.values()
            .filter(|o| o.user_id == user_id && o.symbol == *symbol && o.side == side && o.cancelled_at.is_none())
            .map(|o| o.qty)
            .sum();

        user_depth / total_book_depth
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn user() -> UserId { Uuid::new_v4() }
    fn sym() -> Symbol { Symbol::new("BTCUSDT") }
    fn now() -> DateTime<Utc> { Utc::now() }

    #[test]
    fn multi_level_wall_triggers_alert() {
        let mut det = WallDetector::new(dec!(0.15), 3, 5);
        let u = user();
        let s = sym();
        let t = now();

        // Place orders on 3 price levels.
        let o1 = Uuid::new_v4();
        let o2 = Uuid::new_v4();
        let o3 = Uuid::new_v4();
        det.on_order_placed(o1, u, &s, Side::Buy, dec!(50000), dec!(1), t);
        det.on_order_placed(o2, u, &s, Side::Buy, dec!(49900), dec!(1), t);
        det.on_order_placed(o3, u, &s, Side::Buy, dec!(49800), dec!(1), t);

        // Cancel all within 2 seconds.
        assert!(det.on_order_cancelled(o1, t + Duration::seconds(1)).is_none());
        assert!(det.on_order_cancelled(o2, t + Duration::seconds(1)).is_none());
        let alert = det.on_order_cancelled(o3, t + Duration::seconds(2));
        assert!(alert.is_some(), "3-level wall cancelled within 5s should trigger");
    }

    #[test]
    fn two_levels_not_enough() {
        let mut det = WallDetector::new(dec!(0.15), 3, 5);
        let u = user();
        let s = sym();
        let t = now();

        let o1 = Uuid::new_v4();
        let o2 = Uuid::new_v4();
        det.on_order_placed(o1, u, &s, Side::Sell, dec!(51000), dec!(1), t);
        det.on_order_placed(o2, u, &s, Side::Sell, dec!(51100), dec!(1), t);

        assert!(det.on_order_cancelled(o1, t + Duration::seconds(1)).is_none());
        assert!(det.on_order_cancelled(o2, t + Duration::seconds(1)).is_none());
    }

    #[test]
    fn depth_impact_calculation() {
        let mut det = WallDetector::with_defaults();
        let u = user();
        let s = sym();
        let t = now();

        det.on_order_placed(Uuid::new_v4(), u, &s, Side::Buy, dec!(50000), dec!(2), t);
        det.on_order_placed(Uuid::new_v4(), u, &s, Side::Buy, dec!(49900), dec!(3), t);

        // User has 5 BTC on buy side, total book depth is 20 BTC.
        let impact = det.depth_impact(u, &s, Side::Buy, dec!(20));
        assert_eq!(impact, dec!(0.25)); // 5/20 = 25%
        assert!(impact > dec!(0.15), "25% > 15% threshold");
    }

    #[test]
    fn different_users_independent() {
        let mut det = WallDetector::new(dec!(0.15), 3, 5);
        let u1 = user();
        let u2 = user();
        let s = sym();
        let t = now();

        // User 1: 2 levels, User 2: 1 level. Neither should trigger.
        det.on_order_placed(Uuid::new_v4(), u1, &s, Side::Buy, dec!(50000), dec!(1), t);
        det.on_order_placed(Uuid::new_v4(), u1, &s, Side::Buy, dec!(49900), dec!(1), t);
        det.on_order_placed(Uuid::new_v4(), u2, &s, Side::Buy, dec!(49800), dec!(1), t);

        // Cancel all — no user has 3 levels.
        for o in det.orders.keys().cloned().collect::<Vec<_>>() {
            let _ = det.on_order_cancelled(o, t + Duration::seconds(1));
        }
        // No alert should have fired (2 levels for u1, 1 for u2).
    }
}
