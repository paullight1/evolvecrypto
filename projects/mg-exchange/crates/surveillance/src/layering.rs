//! Layering / book-depth manipulation detection.
//!
//! Layering is a form of market manipulation where a trader places large orders
//! on one side of the book to create the illusion of supply or demand, then
//! cancels them after the opposite side has moved in their favor (allowing the
//! trader to profit on a position they hold in that direction).
//!
//! # Detection algorithm
//!
//! 1. Track every limit order placed per symbol.  Maintain a rolling average
//!    order size per symbol.
//! 2. Flag orders that are ≥ `large_order_multiplier` × average as "large".
//! 3. When a large order is cancelled within `cancel_window_ms` milliseconds,
//!    check whether the opposite side's best price moved in the direction that
//!    benefits a position on that side during the order's lifetime.
//! 4. If yes → emit [`AlertType::Layering`].
//!
//! # Reference
//! ESMA MAR Article 12(2)(b) — layering and spoofing definition.
//! "[...] entering large orders on one side of the order book and then
//! cancelling them after the price has moved in a favorable direction."
//! See also: ESMA/2017/1515, "Guidelines on the Market Abuse Regulation",
//! Section 3.3 (Indicators of market manipulation — layering/spoofing).

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

/// Detailed data attached to a layering alert.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LayeringAlert {
    pub user_id: UserId,
    pub symbol: Symbol,
    /// Side the large orders were placed on (the "fake" side).
    pub spoofed_side: Side,
    /// Quantity of the cancelled large order.
    pub order_qty: Decimal,
    /// Average order size for the symbol at detection time.
    pub avg_order_size: Decimal,
    /// How many multiples of average this order was.
    pub size_multiple: Decimal,
    /// Time between placement and cancellation.
    pub cancel_latency_ms: i64,
    /// Price at placement on the opposite side.
    pub price_at_placement: Decimal,
    /// Price at cancellation on the opposite side (shows the favorable move).
    pub price_at_cancel: Decimal,
    pub detected_at: DateTime<Utc>,
}

// ──────────────────────────────────────────────────────────────────────────────
// Internal types
// ──────────────────────────────────────────────────────────────────────────────

/// A limit order being tracked for potential layering.
#[derive(Debug, Clone)]
struct TrackedOrder {
    order_id: OrderId,
    user_id: UserId,
    symbol: Symbol,
    side: Side,
    qty: Decimal,
    placed_at: DateTime<Utc>,
    /// Best price on the *opposite* side at the moment this order was placed.
    /// Used to determine whether the market moved in the layerer's favor.
    opposite_best_at_placement: Option<Decimal>,
    /// `true` once the order has been flagged as "large".
    is_large: bool,
}

/// Per-symbol state maintained by the detector.
#[derive(Debug, Default, Clone)]
struct SymbolState {
    /// Best bid price (highest price a buyer is willing to pay).
    best_bid: Option<Decimal>,
    /// Best ask price (lowest price a seller is willing to accept).
    best_ask: Option<Decimal>,
    /// Running sum of all order quantities placed (for average calculation).
    total_qty: Decimal,
    /// Count of all orders placed (for average calculation).
    order_count: u64,
}

impl SymbolState {
    /// Average order size across all recorded orders, or zero if none.
    fn avg_order_size(&self) -> Decimal {
        if self.order_count == 0 {
            Decimal::ZERO
        } else {
            self.total_qty / Decimal::from(self.order_count)
        }
    }

    /// Record a newly placed order and update the running average.
    fn record_order(&mut self, qty: Decimal) {
        self.total_qty += qty;
        self.order_count += 1;
    }

    /// Best price on the side *opposite* to `side`.
    ///
    /// * For a Buy order, the opposing side is the ask (sellers).
    /// * For a Sell order, the opposing side is the bid (buyers).
    fn opposite_best(&self, side: Side) -> Option<Decimal> {
        match side {
            Side::Buy => self.best_ask,
            Side::Sell => self.best_bid,
        }
    }

    /// Returns `true` if the opposite-side price moved in a direction that
    /// benefits a trader who placed a large order on `side` to create fake
    /// pressure.
    ///
    /// Layering mechanic:
    /// - Large BUY orders on the bid push the ask down (sellers undercut to
    ///   compete) → cancel when ask is lower than at placement → profit on a
    ///   short position.
    /// - Large SELL orders on the ask push the bid up (buyers raise bids to
    ///   compete) → cancel when bid is higher than at placement → profit on a
    ///   long position.
    fn price_moved_in_favor(
        side: Side,
        price_at_placement: Decimal,
        price_at_cancel: Decimal,
    ) -> bool {
        match side {
            // Fake buy pressure → ask fell → favorable for a short.
            Side::Buy => price_at_cancel < price_at_placement,
            // Fake sell pressure → bid rose → favorable for a long.
            Side::Sell => price_at_cancel > price_at_placement,
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Public detector
// ──────────────────────────────────────────────────────────────────────────────

/// Detects layering / book-depth manipulation.
pub struct LayeringDetector {
    /// Multiplier above the average order size that qualifies an order as
    /// "large".  Default: 5 (i.e. >5× the symbol average).
    large_order_multiplier: Decimal,
    /// Maximum time in milliseconds between placement and cancellation for
    /// the pattern to count.  Default: 500 ms.
    cancel_window_ms: i64,
    /// Active orders that have not yet been cancelled or filled.
    open_orders: HashMap<OrderId, TrackedOrder>,
    /// Per-symbol state (best bid/ask, running average).
    symbol_state: HashMap<Symbol, SymbolState>,
}

impl LayeringDetector {
    /// Create a new detector.
    ///
    /// * `large_order_multiplier` — how many times the symbol average an order
    ///   must be to count as "large" (e.g. `dec!(5)` → 5× average).
    /// * `cancel_window_ms` — maximum milliseconds between placement and
    ///   cancellation for the pattern to be considered (default: 500).
    pub fn new(large_order_multiplier: Decimal, cancel_window_ms: i64) -> Self {
        Self {
            large_order_multiplier,
            cancel_window_ms,
            open_orders: HashMap::new(),
            symbol_state: HashMap::new(),
        }
    }

    /// Create a detector with production defaults (5× average, 500 ms window).
    pub fn with_defaults() -> Self {
        Self::new(dec!(5), 500)
    }

    // ── Ingestion ─────────────────────────────────────────────────────────────

    /// Record a new limit order being placed.
    pub fn on_order_placed(
        &mut self,
        user_id: UserId,
        order_id: OrderId,
        symbol: &Symbol,
        side: Side,
        qty: Decimal,
        timestamp: DateTime<Utc>,
    ) {
        trace!(%user_id, %order_id, %symbol, ?side, %qty, "layering: order placed");

        let state = self.symbol_state.entry(symbol.clone()).or_default();
        state.record_order(qty);

        let avg = state.avg_order_size();
        let opposite_best = state.opposite_best(side);

        // An order is "large" if the symbol has enough history and it exceeds
        // the multiplier threshold.  We need at least 5 prior orders before
        // the average is meaningful to avoid false positives at startup.
        let is_large = state.order_count > 5
            && avg > Decimal::ZERO
            && qty >= avg * self.large_order_multiplier;

        if is_large {
            debug!(
                %user_id, %order_id, %symbol, ?side, %qty, %avg,
                "layering: large order placed ({:.1}× avg)", qty / avg
            );
        }

        self.open_orders.insert(
            order_id,
            TrackedOrder {
                order_id,
                user_id,
                symbol: symbol.clone(),
                side,
                qty,
                placed_at: timestamp,
                opposite_best_at_placement: opposite_best,
                is_large,
            },
        );
    }

    /// Update the best bid/ask for a symbol.
    ///
    /// Call this whenever the best price on either side changes (e.g. after
    /// each matching-engine event).  Accurate price tracking is required for
    /// the price-movement correlation check.
    pub fn on_best_price_update(
        &mut self,
        symbol: &Symbol,
        best_bid: Option<Decimal>,
        best_ask: Option<Decimal>,
    ) {
        let state = self.symbol_state.entry(symbol.clone()).or_default();
        state.best_bid = best_bid;
        state.best_ask = best_ask;
    }

    /// Record that an order was filled (removes it from tracking — filled
    /// orders are not candidates for layering).
    pub fn on_order_filled(&mut self, order_id: OrderId) {
        self.open_orders.remove(&order_id);
    }

    /// Record that an order was cancelled.
    ///
    /// Returns `Some(SurveillanceAlert)` if the cancellation completes a
    /// layering pattern.
    pub fn on_order_cancelled(
        &mut self,
        order_id: OrderId,
        timestamp: DateTime<Utc>,
    ) -> Option<SurveillanceAlert> {
        let order = self.open_orders.remove(&order_id)?;

        if !order.is_large {
            trace!(order_id = %order.order_id, "layering: non-large order cancelled — skip");
            return None;
        }

        let cancel_latency_ms = (timestamp - order.placed_at).num_milliseconds();
        if cancel_latency_ms > self.cancel_window_ms {
            trace!(
                order_id = %order.order_id,
                cancel_latency_ms,
                window = self.cancel_window_ms,
                "layering: large order cancelled too late — skip"
            );
            return None;
        }

        // We need a reference price at placement to determine movement.
        let price_at_placement = order.opposite_best_at_placement?;

        // Current opposite-side price at cancellation time.
        let state = self.symbol_state.get(&order.symbol)?;
        let price_at_cancel = state.opposite_best(order.side)?;

        if !SymbolState::price_moved_in_favor(order.side, price_at_placement, price_at_cancel) {
            trace!(
                order_id = %order.order_id,
                ?price_at_placement,
                ?price_at_cancel,
                "layering: price did not move in favor — no alert"
            );
            return None;
        }

        let avg = state.avg_order_size();
        let size_multiple = if avg > Decimal::ZERO {
            order.qty / avg
        } else {
            Decimal::ONE
        };

        debug!(
            user_id = %order.user_id,
            order_id = %order.order_id,
            symbol = %order.symbol,
            side = ?order.side,
            qty = %order.qty,
            %size_multiple,
            cancel_latency_ms,
            %price_at_placement,
            %price_at_cancel,
            "layering alert triggered"
        );

        let payload = LayeringAlert {
            user_id: order.user_id,
            symbol: order.symbol.clone(),
            spoofed_side: order.side,
            order_qty: order.qty,
            avg_order_size: avg,
            size_multiple,
            cancel_latency_ms,
            price_at_placement,
            price_at_cancel,
            detected_at: timestamp,
        };

        let severity = severity_from_multiple(size_multiple);
        let description = format!(
            "User {} placed a {:.1}× avg-size {} order on {} and cancelled it \
             within {}ms after the opposite side moved {:.4} → {:.4}",
            order.user_id,
            size_multiple,
            side_label(order.side),
            order.symbol,
            cancel_latency_ms,
            price_at_placement,
            price_at_cancel,
        );

        Some(SurveillanceAlert {
            id: Uuid::new_v4(),
            alert_type: AlertType::Layering(payload),
            severity,
            user_ids: vec![order.user_id],
            symbol: Some(order.symbol),
            description,
            auto_action: AutoAction::CancelOrders,
            created_at: timestamp,
            reviewed: false,
        })
    }

    /// Remove state for orders placed before `cutoff` to keep memory bounded.
    ///
    /// Orders older than 10 minutes that have not been cancelled or filled are
    /// presumed to be genuine resting orders and are evicted from tracking.
    pub fn cleanup_old(&mut self, now: DateTime<Utc>) {
        let cutoff = now - Duration::minutes(10);
        self.open_orders.retain(|_, o| o.placed_at >= cutoff);
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────────────

fn severity_from_multiple(multiple: Decimal) -> AlertSeverity {
    if multiple >= dec!(20) {
        AlertSeverity::Critical
    } else if multiple >= dec!(10) {
        AlertSeverity::High
    } else if multiple >= dec!(7) {
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

    fn det() -> LayeringDetector {
        LayeringDetector::new(dec!(5), 500)
    }

    /// Seed the detector with `n` normal-sized orders (qty=1) so the running
    /// average is meaningful and large orders are correctly classified.
    fn seed_avg(det: &mut LayeringDetector, sym: &Symbol, n: u32) {
        let t = now() - Duration::seconds(60);
        for _ in 0..n {
            let oid = Uuid::new_v4();
            det.on_order_placed(user(), oid, sym, Side::Buy, dec!(1), t);
            det.on_order_filled(oid);
        }
        // After seeding with qty=1, avg ≈ 1; large threshold = 5.
    }

    // ── Positive case: classic layering sequence ───────────────────────────

    #[test]
    fn layering_sequence_triggers_alert() {
        let mut det = det();
        let sym = symbol();
        let u = user();
        let t = now();

        // Seed 10 normal orders (qty = 1) so avg = 1, large threshold = 5.
        seed_avg(&mut det, &sym, 10);

        // Establish a reference price: best ask = 100.
        det.on_best_price_update(&sym, Some(dec!(99)), Some(dec!(100)));

        // Place a large BUY order (qty = 10 = 10× avg ≥ 5× threshold).
        let large_oid = Uuid::new_v4();
        det.on_order_placed(u, large_oid, &sym, Side::Buy, dec!(10), t);

        // Market reacts: ask drops (moved in favor of the fake buy pressure).
        det.on_best_price_update(&sym, Some(dec!(98)), Some(dec!(97)));

        // Cancel within 500 ms.
        let cancel_t = t + Duration::milliseconds(300);
        let alert = det.on_order_cancelled(large_oid, cancel_t);

        assert!(
            alert.is_some(),
            "should emit layering alert for classic layering sequence"
        );
        let alert = alert.unwrap();
        assert_eq!(alert.user_ids, vec![u]);
        assert!(matches!(alert.alert_type, AlertType::Layering(_)));
    }

    // ── Negative case: large order stays on book ───────────────────────────

    #[test]
    fn genuine_large_order_no_alert_if_not_cancelled_quickly() {
        let mut det = det();
        let sym = symbol();
        let u = user();
        let t = now();

        seed_avg(&mut det, &sym, 10);
        det.on_best_price_update(&sym, Some(dec!(99)), Some(dec!(100)));

        let large_oid = Uuid::new_v4();
        det.on_order_placed(u, large_oid, &sym, Side::Buy, dec!(10), t);

        det.on_best_price_update(&sym, Some(dec!(98)), Some(dec!(97)));

        // Cancel after 1000 ms — outside the 500 ms window.
        let cancel_t = t + Duration::milliseconds(1000);
        let alert = det.on_order_cancelled(large_oid, cancel_t);

        assert!(
            alert.is_none(),
            "large order cancelled after window should not trigger alert"
        );
    }

    // ── Negative case: price didn't move in favor ──────────────────────────

    #[test]
    fn no_alert_when_price_moved_against() {
        let mut det = det();
        let sym = symbol();
        let u = user();
        let t = now();

        seed_avg(&mut det, &sym, 10);
        det.on_best_price_update(&sym, Some(dec!(99)), Some(dec!(100)));

        let large_oid = Uuid::new_v4();
        // Large buy order — but ask RISES (opposite of favorable).
        det.on_order_placed(u, large_oid, &sym, Side::Buy, dec!(10), t);
        det.on_best_price_update(&sym, Some(dec!(100)), Some(dec!(102)));

        let cancel_t = t + Duration::milliseconds(300);
        let alert = det.on_order_cancelled(large_oid, cancel_t);

        assert!(
            alert.is_none(),
            "no alert when price moved against the layering direction"
        );
    }

    // ── Negative case: small order cancelled quickly ───────────────────────

    #[test]
    fn small_order_cancelled_quickly_no_alert() {
        let mut det = det();
        let sym = symbol();
        let u = user();
        let t = now();

        seed_avg(&mut det, &sym, 10);
        det.on_best_price_update(&sym, Some(dec!(99)), Some(dec!(100)));

        // Small order (qty = 1 = 1× avg < 5× threshold).
        let small_oid = Uuid::new_v4();
        det.on_order_placed(u, small_oid, &sym, Side::Buy, dec!(1), t);
        det.on_best_price_update(&sym, Some(dec!(98)), Some(dec!(97)));

        let cancel_t = t + Duration::milliseconds(200);
        let alert = det.on_order_cancelled(small_oid, cancel_t);

        assert!(
            alert.is_none(),
            "small order cancelled quickly should not trigger layering alert"
        );
    }

    // ── Negative case: large order that gets filled ────────────────────────

    #[test]
    fn large_filled_order_no_alert() {
        let mut det = det();
        let sym = symbol();
        let u = user();
        let t = now();

        seed_avg(&mut det, &sym, 10);
        det.on_best_price_update(&sym, Some(dec!(99)), Some(dec!(100)));

        let large_oid = Uuid::new_v4();
        det.on_order_placed(u, large_oid, &sym, Side::Buy, dec!(10), t);
        det.on_best_price_update(&sym, Some(dec!(98)), Some(dec!(97)));

        // Order fills instead of cancelling.
        det.on_order_filled(large_oid);

        // Attempting to cancel a filled order should do nothing.
        let cancel_t = t + Duration::milliseconds(200);
        let alert = det.on_order_cancelled(large_oid, cancel_t);
        assert!(alert.is_none(), "filled orders must not trigger layering alert");
    }

    // ── Sell-side layering ─────────────────────────────────────────────────

    #[test]
    fn sell_side_layering_triggers_alert() {
        let mut det = det();
        let sym = symbol();
        let u = user();
        let t = now();

        seed_avg(&mut det, &sym, 10);
        // Establish reference bid at 99.
        det.on_best_price_update(&sym, Some(dec!(99)), Some(dec!(100)));

        // Large SELL order placed — creates fake sell pressure.
        let large_oid = Uuid::new_v4();
        det.on_order_placed(u, large_oid, &sym, Side::Sell, dec!(10), t);

        // Bid rises (moved in favor of fake sell pressure → layerer is long).
        det.on_best_price_update(&sym, Some(dec!(101)), Some(dec!(102)));

        let cancel_t = t + Duration::milliseconds(400);
        let alert = det.on_order_cancelled(large_oid, cancel_t);

        assert!(
            alert.is_some(),
            "sell-side layering should also trigger an alert"
        );
    }

    // ── Severity escalates with size multiple ──────────────────────────────

    #[test]
    fn severity_scales_with_size_multiple() {
        assert_eq!(severity_from_multiple(dec!(6)), AlertSeverity::Low);
        assert_eq!(severity_from_multiple(dec!(7)), AlertSeverity::Medium);
        assert_eq!(severity_from_multiple(dec!(10)), AlertSeverity::High);
        assert_eq!(severity_from_multiple(dec!(20)), AlertSeverity::Critical);
    }

    // ── Cleanup removes stale orders ───────────────────────────────────────

    #[test]
    fn cleanup_removes_old_open_orders() {
        let mut det = det();
        let sym = symbol();
        let t = now();

        seed_avg(&mut det, &sym, 10);

        // Place an order 15 minutes ago.
        let old_oid = Uuid::new_v4();
        det.on_order_placed(
            user(),
            old_oid,
            &sym,
            Side::Buy,
            dec!(10),
            t - Duration::minutes(15),
        );

        assert!(det.open_orders.contains_key(&old_oid));
        det.cleanup_old(t);
        assert!(
            !det.open_orders.contains_key(&old_oid),
            "stale order must be evicted by cleanup"
        );
    }

    // ── No reference price → no alert ─────────────────────────────────────

    #[test]
    fn no_alert_when_no_reference_price() {
        let mut det = det();
        let sym = symbol();
        let u = user();
        let t = now();

        seed_avg(&mut det, &sym, 10);
        // Do NOT call on_best_price_update — no reference price available.

        let large_oid = Uuid::new_v4();
        det.on_order_placed(u, large_oid, &sym, Side::Buy, dec!(10), t);

        let cancel_t = t + Duration::milliseconds(200);
        let alert = det.on_order_cancelled(large_oid, cancel_t);

        assert!(
            alert.is_none(),
            "without a reference price there is insufficient evidence for layering"
        );
    }
}
