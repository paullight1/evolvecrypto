//! Cross-market manipulation detection.
//!
//! Detects when a user manipulates the spot market to profit on perpetual
//! futures (or vice versa). The canonical pattern is:
//!
//! 1. User sells a large amount on spot (pushing the index price down).
//! 2. Within a short window, user opens a short perpetual position.
//! 3. The depressed index drags down the mark price, profiting the short.
//!
//! The reverse (buy spot → long perp) is also detected.
//!
//! # Reference
//! - IOSCO "Market Manipulation in Commodity Derivatives" (2018), cross-market manipulation.
//! - Precedent B7 in threat catalogue: Cross-Exchange Manipulation.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;

use exchange_common::types::{Side, Symbol, UserId};

use crate::alert::{AlertSeverity, AlertType, AutoAction, SurveillanceAlert};

/// Detailed data attached to a cross-market manipulation alert.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CrossMarketAlert {
    pub user_id: UserId,
    /// The spot symbol (e.g. BTCUSDT).
    pub spot_symbol: Symbol,
    /// The perpetual symbol (e.g. BTCUSDT-PERP).
    pub perp_symbol: Symbol,
    /// Side of the spot trade (Sell = short setup, Buy = long setup).
    pub spot_side: Side,
    /// Notional of the spot trades in the window.
    pub spot_notional: Decimal,
    /// Side of the perp trade (opposite of spot_side for manipulation).
    pub perp_side: Side,
    /// Notional of the perp position opened.
    pub perp_notional: Decimal,
    /// Time between the spot and perp trades (seconds).
    pub delay_secs: i64,
    pub detected_at: DateTime<Utc>,
}

/// A recorded trade event for cross-market tracking.
#[derive(Debug, Clone)]
struct TradeRecord {
    side: Side,
    notional: Decimal,
    timestamp: DateTime<Utc>,
    is_perp: bool,
}

/// Detects cross-market manipulation between spot and perpetual markets.
pub struct CrossMarketDetector {
    /// Window in seconds: spot → perp must occur within this time.
    window_secs: i64,
    /// Minimum notional on each leg to qualify.
    min_notional: Decimal,
    /// Per-user recent trade records.
    trades: HashMap<UserId, Vec<TradeRecord>>,
}

impl CrossMarketDetector {
    pub fn new(window_secs: i64, min_notional: Decimal) -> Self {
        Self {
            window_secs,
            min_notional,
            trades: HashMap::new(),
        }
    }

    /// Default: 5-minute window, $10,000 minimum per leg.
    pub fn with_defaults() -> Self {
        Self::new(300, dec!(10000))
    }

    /// Record a trade for a user.
    #[allow(clippy::too_many_arguments)]
    pub fn on_trade(
        &mut self,
        user_id: UserId,
        _symbol: &Symbol,
        side: Side,
        price: Decimal,
        qty: Decimal,
        timestamp: DateTime<Utc>,
        is_perp: bool,
    ) {
        let notional = price * qty;
        self.trades.entry(user_id).or_default().push(TradeRecord {
            side,
            notional,
            timestamp,
            is_perp,
        });
    }

    /// Check a user for cross-market manipulation.
    pub fn check_user(
        &mut self,
        user_id: UserId,
        now: DateTime<Utc>,
    ) -> Option<SurveillanceAlert> {
        let records = self.trades.get(&user_id)?;

        let window = Duration::seconds(self.window_secs);
        let cutoff = now - window;

        // Recent spot and perp trades.
        let recent_spot: Vec<&TradeRecord> = records
            .iter()
            .filter(|r| !r.is_perp && r.timestamp >= cutoff)
            .collect();
        let recent_perp: Vec<&TradeRecord> = records
            .iter()
            .filter(|r| r.is_perp && r.timestamp >= cutoff)
            .collect();

        if recent_spot.is_empty() || recent_perp.is_empty() {
            return None;
        }

        // Check for the pattern: spot sell + perp short, or spot buy + perp long.
        for spot_side in [Side::Sell, Side::Buy] {
            let perp_side = spot_side; // same direction on perp = suspicious
            // Actually, the manipulation pattern is:
            // Sell spot (push price down) + Short perp (profit from down)
            // Buy spot (push price up) + Long perp (profit from up)
            // So spot_side == perp_side is the suspicious pattern.

            let spot_notional: Decimal = recent_spot
                .iter()
                .filter(|r| r.side == spot_side)
                .map(|r| r.notional)
                .sum();

            let perp_notional: Decimal = recent_perp
                .iter()
                .filter(|r| r.side == perp_side)
                .map(|r| r.notional)
                .sum();

            if spot_notional >= self.min_notional && perp_notional >= self.min_notional {
                // Find the delay between the last spot trade and first perp trade.
                let last_spot_ts = recent_spot
                    .iter()
                    .filter(|r| r.side == spot_side)
                    .map(|r| r.timestamp)
                    .max()?;
                let first_perp_ts = recent_perp
                    .iter()
                    .filter(|r| r.side == perp_side)
                    .map(|r| r.timestamp)
                    .min()?;

                let delay_secs = (first_perp_ts - last_spot_ts).num_seconds().abs();

                let alert_data = CrossMarketAlert {
                    user_id,
                    spot_symbol: Symbol::new("SPOT"),
                    perp_symbol: Symbol::new("PERP"),
                    spot_side,
                    spot_notional,
                    perp_side,
                    perp_notional,
                    delay_secs,
                    detected_at: now,
                };

                // Clear records to avoid re-firing.
                if let Some(recs) = self.trades.get_mut(&user_id) {
                    recs.retain(|r| r.timestamp > now);
                }

                return Some(SurveillanceAlert {
                    id: Uuid::new_v4(),
                    alert_type: AlertType::CrossMarketManipulation(alert_data),
                    severity: AlertSeverity::High,
                    user_ids: vec![user_id],
                    symbol: None,
                    description: format!(
                        "Cross-market manipulation: {} spot ${:.0} + {} perp ${:.0} within {}s",
                        match spot_side { Side::Buy => "buy", Side::Sell => "sell" },
                        spot_notional,
                        match perp_side { Side::Buy => "long", Side::Sell => "short" },
                        perp_notional,
                        delay_secs,
                    ),
                    auto_action: AutoAction::CancelOrders,
                    created_at: now,
                    reviewed: false,
                });
            }
        }

        None
    }

    /// Remove stale tracking data.
    pub fn cleanup_old(&mut self, now: DateTime<Utc>) {
        let cutoff = now - Duration::seconds(self.window_secs + 60);
        for records in self.trades.values_mut() {
            records.retain(|r| r.timestamp > cutoff);
        }
        self.trades.retain(|_, v| !v.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn user() -> UserId { Uuid::new_v4() }
    fn now() -> DateTime<Utc> { Utc::now() }

    #[test]
    fn spot_sell_then_perp_short_triggers_alert() {
        let mut det = CrossMarketDetector::new(300, dec!(1000));
        let u = user();
        let t = now();

        // Spot sell $5000
        det.on_trade(u, &Symbol::new("BTCUSDT"), Side::Sell, dec!(50000), dec!(0.1), t, false);
        // Perp short $5000 within 60s
        det.on_trade(u, &Symbol::new("BTCUSDT-PERP"), Side::Sell, dec!(50000), dec!(0.1), t + Duration::seconds(60), true);

        let alert = det.check_user(u, t + Duration::seconds(120));
        assert!(alert.is_some());
        let alert = alert.unwrap();
        assert!(matches!(alert.alert_type, AlertType::CrossMarketManipulation(_)));
    }

    #[test]
    fn no_alert_without_perp_leg() {
        let mut det = CrossMarketDetector::new(300, dec!(1000));
        let u = user();
        let t = now();

        // Only spot sell
        det.on_trade(u, &Symbol::new("BTCUSDT"), Side::Sell, dec!(50000), dec!(0.1), t, false);

        let alert = det.check_user(u, t + Duration::seconds(120));
        assert!(alert.is_none());
    }

    #[test]
    fn below_notional_threshold_no_alert() {
        let mut det = CrossMarketDetector::new(300, dec!(100000)); // high threshold
        let u = user();
        let t = now();

        det.on_trade(u, &Symbol::new("BTCUSDT"), Side::Sell, dec!(50000), dec!(0.01), t, false); // $500
        det.on_trade(u, &Symbol::new("BTCUSDT-PERP"), Side::Sell, dec!(50000), dec!(0.01), t + Duration::seconds(60), true);

        let alert = det.check_user(u, t + Duration::seconds(120));
        assert!(alert.is_none());
    }

    #[test]
    fn opposite_sides_no_alert() {
        let mut det = CrossMarketDetector::new(300, dec!(1000));
        let u = user();
        let t = now();

        // Spot sell + perp long (opposite sides = normal hedging, not manipulation)
        det.on_trade(u, &Symbol::new("BTCUSDT"), Side::Sell, dec!(50000), dec!(0.1), t, false);
        det.on_trade(u, &Symbol::new("BTCUSDT-PERP"), Side::Buy, dec!(50000), dec!(0.1), t + Duration::seconds(60), true);

        let alert = det.check_user(u, t + Duration::seconds(120));
        assert!(alert.is_none());
    }
}
