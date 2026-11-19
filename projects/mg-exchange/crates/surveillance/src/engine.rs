//! Surveillance engine — orchestrates all detectors and manages the alert log.
//!
//! All matching-engine events flow into the engine via `on_order_placed`,
//! `on_order_cancelled`, and `on_trade`.  Calling `check_all` fans out to
//! every detector, collects newly triggered alerts, appends them to the
//! internal log, and returns the new batch.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use tracing::info;
use uuid::Uuid;

use exchange_common::types::{OrderId, Side, Symbol, UserId};

use crate::{
    alert::SurveillanceAlert,
    quote_stuffing::QuoteStuffingDetector,
    spoofing::SpoofingDetector,
    wash_trading::WashTradingDetector,
};

// ──────────────────────────────────────────────────────────────────────────────
// Engine
// ──────────────────────────────────────────────────────────────────────────────

/// Top-level surveillance coordinator.
pub struct SurveillanceEngine {
    spoofing: SpoofingDetector,
    wash_trading: WashTradingDetector,
    quote_stuffing: QuoteStuffingDetector,
    /// Cumulative alert log (both reviewed and unreviewed).
    alerts: Vec<SurveillanceAlert>,
}

impl SurveillanceEngine {
    /// Create a new engine with the given detector configurations.
    pub fn new(
        spoofing: SpoofingDetector,
        wash_trading: WashTradingDetector,
        quote_stuffing: QuoteStuffingDetector,
    ) -> Self {
        Self {
            spoofing,
            wash_trading,
            quote_stuffing,
            alerts: Vec::new(),
        }
    }

    /// Create an engine with sensible production defaults.
    ///
    /// * Spoofing: 95 % cancel ratio, 60-second window, minimum 10 orders.
    /// * Wash trading: 0.80 volume-balance ratio, 3600-second window.
    /// * Quote stuffing: 50 orders per second maximum.
    pub fn with_defaults() -> Self {
        use rust_decimal_macros::dec;
        Self::new(
            SpoofingDetector::new(dec!(0.95), 60, 10),
            WashTradingDetector::new(dec!(0.80), 3600),
            QuoteStuffingDetector::new(50),
        )
    }

    // ── Ingestion ─────────────────────────────────────────────────────────────

    /// Fan out an order-placed event to all relevant detectors.
    pub fn on_order_placed(
        &mut self,
        user_id: UserId,
        order_id: OrderId,
        side: Side,
        qty: Decimal,
        timestamp: DateTime<Utc>,
    ) {
        self.spoofing
            .on_order_placed(user_id, order_id, side, qty, timestamp);
        self.quote_stuffing.on_order(user_id, timestamp);
    }

    /// Fan out an order-placed event with full order details to all detectors.
    ///
    /// This overload accepts `symbol` and `price` in addition to the base
    /// parameters so that detectors which require the full order context
    /// (e.g. layering, momentum-ignition) can use them.
    pub fn on_order_placed_full(
        &mut self,
        user_id: UserId,
        order_id: OrderId,
        _symbol: &Symbol,
        side: Side,
        _price: Decimal,
        qty: Decimal,
        timestamp: DateTime<Utc>,
    ) {
        self.on_order_placed(user_id, order_id, side, qty, timestamp);
    }

    /// Fan out an order-cancelled event.
    pub fn on_order_cancelled(
        &mut self,
        user_id: UserId,
        order_id: OrderId,
        timestamp: DateTime<Utc>,
    ) {
        self.spoofing.on_order_cancelled(user_id, order_id, timestamp);
    }

    /// Fan out an order-filled event.
    pub fn on_order_filled(&mut self, user_id: UserId, order_id: OrderId) {
        self.spoofing.on_order_filled(user_id, order_id);
    }

    /// Fan out a trade event.
    pub fn on_trade(
        &mut self,
        maker_user_id: UserId,
        taker_user_id: UserId,
        symbol: Symbol,
        price: Decimal,
        qty: Decimal,
        taker_side: Side,
        timestamp: DateTime<Utc>,
    ) {
        self.wash_trading.on_trade(
            maker_user_id,
            taker_user_id,
            symbol,
            price,
            qty,
            taker_side,
            timestamp,
        );
    }

    // ── Detection ─────────────────────────────────────────────────────────────

    /// Run all detectors, collect new alerts, append them to the log, and
    /// return the new batch.
    ///
    /// `active_users` — the set of users that have been active since the last
    /// call.  Passing an explicit list avoids iterating all known users on
    /// every tick.  Pass `&[]` to skip per-user checks and only run
    /// aggregate detectors (e.g. circular-trading).
    pub fn check_all(
        &mut self,
        active_users: &[(UserId, Option<Symbol>)],
        now: DateTime<Utc>,
    ) -> Vec<SurveillanceAlert> {
        let mut new_alerts: Vec<SurveillanceAlert> = Vec::new();

        for (user_id, symbol) in active_users {
            // Spoofing check.
            if let Some(alert) = self.spoofing.check_user(*user_id, now) {
                info!(
                    alert_id = %alert.id,
                    %user_id,
                    severity = %alert.severity,
                    "spoofing alert"
                );
                new_alerts.push(alert);
            }

            // Quote-stuffing check.
            if let Some(alert) = self.quote_stuffing.check_rate(*user_id, now) {
                info!(
                    alert_id = %alert.id,
                    %user_id,
                    severity = %alert.severity,
                    "quote stuffing alert"
                );
                new_alerts.push(alert);
            }

            // Wash-trading volume-balance check (only when we know the symbol).
            if let Some(sym) = symbol {
                if let Some(alert) = self.wash_trading.check_volume_balance(*user_id, sym, now) {
                    info!(
                        alert_id = %alert.id,
                        %user_id,
                        %sym,
                        severity = %alert.severity,
                        "wash trading (volume balance) alert"
                    );
                    new_alerts.push(alert);
                }
            }
        }

        // Circular-trading is aggregate — always run it.
        let circular = self.wash_trading.check_circular_trading(now);
        for alert in circular {
            info!(
                alert_id = %alert.id,
                users = ?alert.user_ids,
                "wash trading (circular) alert"
            );
            new_alerts.push(alert);
        }

        self.alerts.extend(new_alerts.clone());

        // Periodic cleanup to keep memory bounded.
        self.spoofing.cleanup_old(now);
        self.wash_trading.cleanup_old(now);
        self.quote_stuffing.cleanup_old(now);

        new_alerts
    }

    // ── Alert management ──────────────────────────────────────────────────────

    /// All alerts that have not yet been reviewed.
    pub fn unreviewed_alerts(&self) -> Vec<&SurveillanceAlert> {
        self.alerts.iter().filter(|a| !a.reviewed).collect()
    }

    /// All alerts (reviewed and unreviewed).
    pub fn all_alerts(&self) -> &[SurveillanceAlert] {
        &self.alerts
    }

    /// Mark the alert with `id` as reviewed.  Returns `true` if found.
    pub fn review_alert(&mut self, id: Uuid) -> bool {
        if let Some(alert) = self.alerts.iter_mut().find(|a| a.id == id) {
            alert.review();
            true
        } else {
            false
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    use exchange_common::types::{Side, Symbol};

    use crate::{
        quote_stuffing::QuoteStuffingDetector,
        spoofing::SpoofingDetector,
        wash_trading::WashTradingDetector,
    };

    use super::*;

    fn engine() -> SurveillanceEngine {
        SurveillanceEngine::new(
            SpoofingDetector::new(dec!(0.90), 60, 5),
            WashTradingDetector::new(dec!(0.80), 3600),
            QuoteStuffingDetector::new(10),
        )
    }

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
    fn spoofing_alert_collected_via_engine() {
        let mut eng = engine();
        let u = user();
        let t = now();

        // Place 10 orders and cancel all.
        for _ in 0..10 {
            let oid = Uuid::new_v4();
            eng.on_order_placed(u, oid, Side::Buy, dec!(1), t);
            eng.on_order_cancelled(u, oid, t + Duration::seconds(1));
        }

        let alerts = eng.check_all(&[(u, None)], t + Duration::seconds(2));
        assert!(!alerts.is_empty());
        assert!(eng.unreviewed_alerts().len() >= 1);
    }

    #[test]
    fn quote_stuffing_alert_collected_via_engine() {
        let mut eng = engine();
        let u = user();
        let t = now();

        for _ in 0..15 {
            eng.on_order_placed(u, Uuid::new_v4(), Side::Sell, dec!(1), t);
        }

        let alerts = eng.check_all(&[(u, None)], t);
        let qs_alerts: Vec<_> = alerts
            .iter()
            .filter(|a| {
                matches!(a.alert_type, crate::alert::AlertType::QuoteStuffing(_))
            })
            .collect();
        assert!(!qs_alerts.is_empty());
    }

    #[test]
    fn review_alert_marks_reviewed() {
        let mut eng = engine();
        let u = user();
        let t = now();

        for _ in 0..15 {
            let oid = Uuid::new_v4();
            eng.on_order_placed(u, oid, Side::Buy, dec!(1), t);
            eng.on_order_cancelled(u, oid, t + Duration::seconds(1));
        }

        let alerts = eng.check_all(&[(u, None)], t + Duration::seconds(2));
        assert!(!alerts.is_empty());

        let id = alerts[0].id;
        assert!(eng.review_alert(id));
        // That specific alert is now reviewed.
        let found = eng.all_alerts().iter().find(|a| a.id == id).unwrap();
        assert!(found.reviewed);
    }

    #[test]
    fn review_unknown_id_returns_false() {
        let mut eng = engine();
        assert!(!eng.review_alert(Uuid::new_v4()));
    }

    #[test]
    fn no_alerts_for_clean_activity() {
        let mut eng = engine();
        let u = user();
        let t = now();

        // Place 3 orders and fill them — well below any threshold.
        for _ in 0..3 {
            let oid = Uuid::new_v4();
            eng.on_order_placed(u, oid, Side::Buy, dec!(1), t);
            eng.on_order_filled(u, oid);
        }

        let alerts = eng.check_all(&[(u, None)], t);
        // No spoofing (below min_orders), no quote stuffing (3 < 10).
        assert!(alerts.is_empty());
    }

    #[test]
    fn wash_trading_detected_via_engine() {
        let mut eng = engine();
        let u = user();
        let cp = Uuid::new_v4();
        let t = now();

        eng.on_trade(cp, u, sym(), dec!(50000), dec!(5), Side::Buy, t);
        eng.on_trade(cp, u, sym(), dec!(50000), dec!(5), Side::Sell, t);

        let alerts = eng.check_all(&[(u, Some(sym()))], t);
        let wt_alerts: Vec<_> = alerts
            .iter()
            .filter(|a| matches!(a.alert_type, crate::alert::AlertType::WashTrading(_)))
            .collect();
        assert!(!wt_alerts.is_empty());
    }

    #[test]
    fn alerts_accumulate_across_multiple_check_calls() {
        let mut eng = engine();
        let u = user();
        let t = now();

        // First burst.
        for _ in 0..15 {
            eng.on_order_placed(u, Uuid::new_v4(), Side::Buy, dec!(1), t);
        }
        eng.check_all(&[(u, None)], t);

        // Second burst 30 seconds later.
        let t2 = t + Duration::seconds(30);
        for _ in 0..15 {
            eng.on_order_placed(u, Uuid::new_v4(), Side::Buy, dec!(1), t2);
        }
        eng.check_all(&[(u, None)], t2);

        // At least two alerts total across both passes.
        assert!(eng.all_alerts().len() >= 2);
    }
}
