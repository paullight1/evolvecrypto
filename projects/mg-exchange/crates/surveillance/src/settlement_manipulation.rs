//! Settlement window / "banging the close" manipulation detection.
//!
//! Settlement-window manipulation involves placing aggressive orders in the
//! period immediately before a settlement (e.g. perpetual funding rate
//! calculation) with the intent of moving the settlement price in the
//! direction that benefits open positions.
//!
//! # Reference
//! Tower Research Capital LLC v. SEC (2018) — the "banging the close" case.
//! Traders with net-long positions placed large aggressive buy orders in the
//! final seconds before settlement to push the settlement price higher,
//! increasing the value of their positions at the funding snapshot.
//! See also: CFTC v. Kraft Foods (2015), "Banging the close" definition in
//! CFTC Regulation 180.2.
//!
//! # Detection algorithm
//!
//! 1. The caller notifies the detector when a funding settlement window opens
//!    via [`SettlementManipulationDetector::on_settlement_window_open`].
//! 2. Any order placed during the settlement window (last `window_secs` before
//!    funding) is recorded alongside the user's open position direction.
//! 3. If a user places aggressive orders (market orders or limit orders that
//!    cross the spread) in the settlement window on the *same side* as their
//!    open position, and the cumulative notional of those orders exceeds the
//!    threshold, an alert is emitted.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tracing::{debug, trace};
use uuid::Uuid;

use exchange_common::types::{OrderId, Side, Symbol, UserId};

use crate::alert::{AlertSeverity, AlertType, AutoAction, SurveillanceAlert};

// ──────────────────────────────────────────────────────────────────────────────
// Public alert payload
// ──────────────────────────────────────────────────────────────────────────────

/// Detailed data attached to a settlement manipulation alert.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SettlementManipulationAlert {
    pub user_id: UserId,
    pub symbol: Symbol,
    /// Direction of the user's open position.
    pub position_side: Side,
    /// Side of the orders placed during the settlement window (matches
    /// `position_side` in a manipulation scenario).
    pub order_side: Side,
    /// Cumulative notional value of the suspicious orders in the window.
    pub cumulative_notional: Decimal,
    /// Number of suspicious orders placed during the window.
    pub order_count: usize,
    pub settlement_window_start: DateTime<Utc>,
    pub detected_at: DateTime<Utc>,
}

// ──────────────────────────────────────────────────────────────────────────────
// Internal types
// ──────────────────────────────────────────────────────────────────────────────

/// A single order placed during the settlement window.
#[derive(Debug, Clone)]
struct WindowOrder {
    _order_id: OrderId,
    side: Side,
    /// `price × qty` — notional exposure.
    notional: Decimal,
    timestamp: DateTime<Utc>,
}

/// State tracked per (user, symbol) during a settlement window.
#[derive(Debug, Default, Clone)]
struct UserWindowState {
    orders: Vec<WindowOrder>,
}

impl UserWindowState {
    /// Cumulative notional for orders matching `side`.
    fn notional_for_side(&self, side: Side, since: DateTime<Utc>) -> Decimal {
        self.orders
            .iter()
            .filter(|o| o.side == side && o.timestamp >= since)
            .map(|o| o.notional)
            .fold(Decimal::ZERO, |acc, n| acc + n)
    }

    /// Count of orders matching `side`.
    fn count_for_side(&self, side: Side, since: DateTime<Utc>) -> usize {
        self.orders
            .iter()
            .filter(|o| o.side == side && o.timestamp >= since)
            .count()
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Public detector
// ──────────────────────────────────────────────────────────────────────────────

/// Detects "banging the close" / settlement-window manipulation.
pub struct SettlementManipulationDetector {
    /// How many seconds before the funding event constitute the settlement
    /// window (default: 60).
    window_secs: i64,
    /// Minimum cumulative notional (in quote currency) of orders in the window
    /// required to fire an alert.  Below this threshold the order flow is
    /// noise.
    min_notional_threshold: Decimal,
    /// Active settlement windows: symbol → window open time.
    active_windows: HashMap<Symbol, DateTime<Utc>>,
    /// Orders recorded during active windows, keyed by (user_id, symbol).
    window_orders: HashMap<(UserId, Symbol), UserWindowState>,
    /// Open positions per (user, symbol): `Some(Side::Buy)` = net long,
    /// `Some(Side::Sell)` = net short, `None` = flat.
    positions: HashMap<(UserId, Symbol), Side>,
}

impl SettlementManipulationDetector {
    /// Create a new detector.
    ///
    /// * `window_secs` — seconds before funding that constitute the settlement
    ///   window (CFTC "banging the close" typically considers the last 30–60 s).
    /// * `min_notional_threshold` — minimum cumulative notional of same-side
    ///   orders required to trigger an alert.
    pub fn new(window_secs: i64, min_notional_threshold: Decimal) -> Self {
        Self {
            window_secs,
            min_notional_threshold,
            active_windows: HashMap::new(),
            window_orders: HashMap::new(),
            positions: HashMap::new(),
        }
    }

    /// Create a detector with production defaults (60 s window, 10 000 USDT
    /// minimum notional).
    pub fn with_defaults() -> Self {
        Self::new(60, dec!(10000))
    }

    // ── Position tracking ─────────────────────────────────────────────────

    /// Update the net position direction for `user_id` on `symbol`.
    ///
    /// Pass `None` to indicate the user is flat.
    pub fn on_position_update(
        &mut self,
        user_id: UserId,
        symbol: &Symbol,
        position_side: Option<Side>,
    ) {
        match position_side {
            Some(side) => {
                self.positions.insert((user_id, symbol.clone()), side);
            }
            None => {
                self.positions.remove(&(user_id, symbol.clone()));
            }
        }
    }

    // ── Settlement window lifecycle ────────────────────────────────────────

    /// Notify the detector that the settlement window for `symbol` has opened.
    ///
    /// Call this `window_secs` before the scheduled funding event.
    pub fn on_settlement_window_open(&mut self, symbol: &Symbol, now: DateTime<Utc>) {
        debug!(%symbol, "settlement manipulation: window opened");
        self.active_windows.insert(symbol.clone(), now);
    }

    /// Notify the detector that the settlement for `symbol` has completed.
    ///
    /// Clears the window state; returns any alerts that should be raised for
    /// the just-completed window.
    pub fn on_settlement_complete(
        &mut self,
        symbol: &Symbol,
        now: DateTime<Utc>,
    ) -> Vec<SurveillanceAlert> {
        let window_start = match self.active_windows.remove(symbol) {
            Some(t) => t,
            None => return Vec::new(),
        };

        let mut alerts = Vec::new();

        // Gather all users that had orders during this window for this symbol.
        let keys: Vec<(UserId, Symbol)> = self
            .window_orders
            .keys()
            .filter(|(_, sym)| sym == symbol)
            .cloned()
            .collect();

        for key in &keys {
            let (user_id, sym) = key;
            let state = match self.window_orders.get(key) {
                Some(s) => s,
                None => continue,
            };

            let position_side = match self.positions.get(&(*user_id, sym.clone())) {
                Some(&side) => side,
                None => {
                    // No open position → cannot be banging the close.
                    continue;
                }
            };

            // Orders that match the position direction — the "suspicious" side.
            let notional = state.notional_for_side(position_side, window_start);
            let count = state.count_for_side(position_side, window_start);

            if notional < self.min_notional_threshold || count == 0 {
                continue;
            }

            debug!(
                %user_id,
                %sym,
                ?position_side,
                %notional,
                count,
                "settlement manipulation alert triggered"
            );

            let payload = SettlementManipulationAlert {
                user_id: *user_id,
                symbol: sym.clone(),
                position_side,
                order_side: position_side,
                cumulative_notional: notional,
                order_count: count,
                settlement_window_start: window_start,
                detected_at: now,
            };

            let severity = severity_from_notional(notional, self.min_notional_threshold);
            let description = format!(
                "User {user_id} placed {count} {side} orders totalling {notional:.2} notional \
                 during the {window_secs}s settlement window on {sym}, aligned with their \
                 {side} position (possible 'banging the close')",
                side = side_label(position_side),
                window_secs = self.window_secs,
            );

            alerts.push(SurveillanceAlert {
                id: Uuid::new_v4(),
                alert_type: AlertType::SettlementManipulation(payload),
                severity,
                user_ids: vec![*user_id],
                symbol: Some(sym.clone()),
                description,
                auto_action: AutoAction::CancelOrders,
                created_at: now,
                reviewed: false,
            });
        }

        // Clean up window order state for this symbol.
        self.window_orders.retain(|(_, sym), _| sym != symbol);

        alerts
    }

    // ── Order ingestion ───────────────────────────────────────────────────

    /// Record that an order was placed.
    ///
    /// If a settlement window is active for `symbol`, the order is recorded
    /// for later analysis.
    #[allow(clippy::too_many_arguments)]
    pub fn on_order_placed(
        &mut self,
        user_id: UserId,
        order_id: OrderId,
        symbol: &Symbol,
        side: Side,
        price: Decimal,
        qty: Decimal,
        timestamp: DateTime<Utc>,
    ) {
        if !self.active_windows.contains_key(symbol) {
            return; // No active window → nothing to record.
        }

        let notional = price * qty;
        trace!(
            %user_id,
            %order_id,
            %symbol,
            ?side,
            %notional,
            "settlement manipulation: recording order in window"
        );

        self.window_orders
            .entry((user_id, symbol.clone()))
            .or_default()
            .orders
            .push(WindowOrder {
                _order_id: order_id,
                side,
                notional,
                timestamp,
            });
    }

    /// Drop state for windows that are older than `window_secs * 2` to prevent
    /// memory growth when `on_settlement_complete` is missed.
    pub fn cleanup_old(&mut self, now: DateTime<Utc>) {
        let cutoff = now - Duration::seconds(self.window_secs * 2);
        self.active_windows.retain(|_, opened_at| *opened_at >= cutoff);

        // Also prune old individual orders within window state.
        for state in self.window_orders.values_mut() {
            state.orders.retain(|o| o.timestamp >= cutoff);
        }
        self.window_orders.retain(|_, s| !s.orders.is_empty());
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────────────

fn severity_from_notional(notional: Decimal, threshold: Decimal) -> AlertSeverity {
    if threshold.is_zero() {
        return AlertSeverity::High;
    }
    let multiple = notional / threshold;
    if multiple >= dec!(10) {
        AlertSeverity::Critical
    } else if multiple >= dec!(5) {
        AlertSeverity::High
    } else if multiple >= dec!(2) {
        AlertSeverity::Medium
    } else {
        AlertSeverity::Low
    }
}

fn side_label(side: Side) -> &'static str {
    match side {
        Side::Buy => "buy",
        Side::Sell => "sell",
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

    fn symbol() -> Symbol {
        Symbol::new("BTCUSDT")
    }

    fn user() -> UserId {
        Uuid::new_v4()
    }

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    fn det() -> SettlementManipulationDetector {
        // 60 s window, 1000 USDT minimum notional (low for test convenience).
        SettlementManipulationDetector::new(60, dec!(1000))
    }

    // ── Positive: long user buys aggressively in window ────────────────────

    #[test]
    fn long_user_buying_in_window_triggers_alert() {
        let mut det = det();
        let sym = symbol();
        let u = user();
        let t = now();

        // User has a net-long position.
        det.on_position_update(u, &sym, Some(Side::Buy));

        // Settlement window opens.
        det.on_settlement_window_open(&sym, t);

        // User places aggressive buy orders during the window.
        // notional = 50000 * 0.5 = 25000 > 1000 threshold.
        for i in 0..5 {
            let oid = Uuid::new_v4();
            det.on_order_placed(
                u,
                oid,
                &sym,
                Side::Buy,
                dec!(50000),
                dec!(0.1),
                t + Duration::seconds(i),
            );
        }

        // Settlement completes.
        let alerts = det.on_settlement_complete(&sym, t + Duration::seconds(55));
        assert!(
            !alerts.is_empty(),
            "long user buying in window should trigger settlement manipulation alert"
        );
        let alert = &alerts[0];
        assert_eq!(alert.user_ids, vec![u]);
        assert!(matches!(
            alert.alert_type,
            AlertType::SettlementManipulation(_)
        ));
    }

    // ── Positive: short user selling aggressively in window ────────────────

    #[test]
    fn short_user_selling_in_window_triggers_alert() {
        let mut det = det();
        let sym = symbol();
        let u = user();
        let t = now();

        det.on_position_update(u, &sym, Some(Side::Sell));
        det.on_settlement_window_open(&sym, t);

        for i in 0..5 {
            let oid = Uuid::new_v4();
            det.on_order_placed(
                u,
                oid,
                &sym,
                Side::Sell,
                dec!(50000),
                dec!(0.1),
                t + Duration::seconds(i),
            );
        }

        let alerts = det.on_settlement_complete(&sym, t + Duration::seconds(55));
        assert!(
            !alerts.is_empty(),
            "short user selling in window should trigger alert"
        );
    }

    // ── Negative: flat user's orders are not flagged ───────────────────────

    #[test]
    fn flat_user_orders_not_flagged() {
        let mut det = det();
        let sym = symbol();
        let u = user();
        let t = now();

        // Flat user (no position).
        det.on_settlement_window_open(&sym, t);

        for i in 0..5 {
            let oid = Uuid::new_v4();
            det.on_order_placed(
                u,
                oid,
                &sym,
                Side::Buy,
                dec!(50000),
                dec!(0.1),
                t + Duration::seconds(i),
            );
        }

        let alerts = det.on_settlement_complete(&sym, t + Duration::seconds(55));
        assert!(
            alerts.is_empty(),
            "flat user has no position to benefit from — no alert"
        );
    }

    // ── Negative: long user sells against position (hedging, not banging) ──

    #[test]
    fn long_user_selling_is_not_flagged() {
        let mut det = det();
        let sym = symbol();
        let u = user();
        let t = now();

        det.on_position_update(u, &sym, Some(Side::Buy));
        det.on_settlement_window_open(&sym, t);

        // User is long but places SELL orders — reducing exposure, not gaming.
        for i in 0..5 {
            let oid = Uuid::new_v4();
            det.on_order_placed(
                u,
                oid,
                &sym,
                Side::Sell,
                dec!(50000),
                dec!(0.1),
                t + Duration::seconds(i),
            );
        }

        let alerts = det.on_settlement_complete(&sym, t + Duration::seconds(55));
        assert!(
            alerts.is_empty(),
            "long user placing sell orders is reducing risk, not gaming settlement"
        );
    }

    // ── Negative: below notional threshold ────────────────────────────────

    #[test]
    fn below_notional_threshold_no_alert() {
        let mut det = det();
        let sym = symbol();
        let u = user();
        let t = now();

        det.on_position_update(u, &sym, Some(Side::Buy));
        det.on_settlement_window_open(&sym, t);

        // notional = 100 * 0.001 = 0.1 USDT << 1000 threshold.
        let oid = Uuid::new_v4();
        det.on_order_placed(u, oid, &sym, Side::Buy, dec!(100), dec!(0.001), t);

        let alerts = det.on_settlement_complete(&sym, t + Duration::seconds(55));
        assert!(
            alerts.is_empty(),
            "trivially small order should not trigger alert"
        );
    }

    // ── Negative: orders placed outside window (before it opened) ──────────

    #[test]
    fn orders_before_window_are_not_counted() {
        let mut det = det();
        let sym = symbol();
        let u = user();
        let t = now();

        det.on_position_update(u, &sym, Some(Side::Buy));

        // Orders placed before the window opens — detector ignores them because
        // there is no active window yet.
        for i in 0..5 {
            let oid = Uuid::new_v4();
            det.on_order_placed(
                u,
                oid,
                &sym,
                Side::Buy,
                dec!(50000),
                dec!(0.1),
                t - Duration::seconds(10 - i),
            );
        }

        // Window opens now.
        det.on_settlement_window_open(&sym, t);

        let alerts = det.on_settlement_complete(&sym, t + Duration::seconds(55));
        assert!(
            alerts.is_empty(),
            "orders placed before the window should not count"
        );
    }

    // ── No alerts when no active window ───────────────────────────────────

    #[test]
    fn orders_outside_any_window_not_recorded() {
        let mut det = det();
        let sym = symbol();
        let u = user();
        let t = now();

        det.on_position_update(u, &sym, Some(Side::Buy));
        // No window opened.

        let oid = Uuid::new_v4();
        det.on_order_placed(u, oid, &sym, Side::Buy, dec!(50000), dec!(1), t);

        // No window → on_settlement_complete returns nothing and has no window to close.
        let alerts = det.on_settlement_complete(&sym, t + Duration::seconds(60));
        assert!(alerts.is_empty());
        assert!(det.window_orders.is_empty());
    }

    // ── Severity scales with notional ──────────────────────────────────────

    #[test]
    fn severity_scales_with_notional_multiple() {
        let threshold = dec!(1000);
        assert_eq!(severity_from_notional(dec!(1500), threshold), AlertSeverity::Low);
        assert_eq!(severity_from_notional(dec!(2000), threshold), AlertSeverity::Medium);
        assert_eq!(severity_from_notional(dec!(5000), threshold), AlertSeverity::High);
        assert_eq!(severity_from_notional(dec!(10000), threshold), AlertSeverity::Critical);
    }

    // ── Cleanup removes stale state ────────────────────────────────────────

    #[test]
    fn cleanup_removes_stale_windows() {
        let mut det = SettlementManipulationDetector::new(60, dec!(1000));
        let sym = symbol();
        let t = now();

        det.on_settlement_window_open(&sym, t - Duration::seconds(200));

        assert!(!det.active_windows.is_empty());
        det.cleanup_old(t);
        assert!(det.active_windows.is_empty(), "stale window must be cleaned up");
    }
}
