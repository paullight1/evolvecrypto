use std::collections::HashMap;

use exchange_common::{OrderId, Qty};
use rust_decimal::Decimal;

/// NOTE: IcebergManager does NOT directly touch OrderBook::bids or OrderBook::asks.
/// Iceberg replenishment flows through OrderBook::place_order → insert_order, which
/// updates the best_bid/best_ask cache. No cache update hooks are needed here.

/// Tracks a single iceberg order's full-size and fill state.
///
/// Only `visible_qty` is submitted to the order book at any time.  When the
/// visible slice is fully filled, `on_visible_filled` returns the next slice
/// size to re-submit.  This continues until all `total_qty` has been consumed.
#[derive(Debug, Clone)]
pub struct IcebergOrder {
    pub order_id: OrderId,
    /// The total (hidden + visible) quantity of the original order.
    pub total_qty: Qty,
    /// The size of each visible slice.
    pub visible_qty: Qty,
    /// Cumulative quantity filled so far.
    pub filled_qty: Qty,
}

impl IcebergOrder {
    /// Quantity still hidden (not yet injected into the order book).
    pub fn remaining_hidden(&self) -> Qty {
        let remaining_total = self.total_qty - self.filled_qty;
        remaining_total.saturating_sub(self.visible_qty).max(Decimal::ZERO)
    }
}

/// Manages all live iceberg orders for a matching engine instance.
///
/// Iceberg orders are invisible to other market participants: the order book
/// only ever sees the current `visible_qty` slice.  This manager tracks the
/// full hidden quantity so the engine can replenish the visible slice after
/// each fill.
///
/// Single-threaded by design — consistent with `OrderBook`.
#[derive(Debug, Default)]
pub struct IcebergManager {
    icebergs: HashMap<OrderId, IcebergOrder>,
}

impl IcebergManager {
    pub fn new() -> Self {
        Self::default()
    }

    // ------------------------------------------------------------------
    // Registration
    // ------------------------------------------------------------------

    /// Register a new iceberg order.
    ///
    /// `total_qty` is the full order size; `visible_qty` is the slice currently
    /// on the book.  Both must be positive.  If `visible_qty > total_qty` the
    /// order is not truly hidden but is registered regardless — it will simply
    /// exhaust in one slice.
    ///
    /// Silently overwrites any previous registration for the same `order_id`.
    pub fn register(&mut self, order_id: OrderId, total_qty: Qty, visible_qty: Qty) {
        let iceberg = IcebergOrder {
            order_id,
            total_qty,
            visible_qty,
            filled_qty: Decimal::ZERO,
        };
        self.icebergs.insert(order_id, iceberg);
    }

    // ------------------------------------------------------------------
    // Fill handling
    // ------------------------------------------------------------------

    /// Notify the manager that the current visible slice of `order_id` has
    /// been fully filled.
    ///
    /// Returns the **size of the next visible slice** that must be placed on
    /// the book, or `None` if the iceberg is fully consumed or the order id is
    /// not known.
    ///
    /// The returned slice may be smaller than `visible_qty` when the remainder
    /// is less than one full slice.
    pub fn on_visible_filled(&mut self, order_id: OrderId) -> Option<Qty> {
        let iceberg = self.icebergs.get_mut(&order_id)?;

        // Advance the fill cursor by one visible slice.
        iceberg.filled_qty += iceberg.visible_qty;

        // Cap at total to guard against floating-point-style rounding if Decimal
        // arithmetic were ever inexact (it is exact, but defensive is good).
        if iceberg.filled_qty > iceberg.total_qty {
            iceberg.filled_qty = iceberg.total_qty;
        }

        let remaining = iceberg.total_qty - iceberg.filled_qty;

        if remaining <= Decimal::ZERO {
            // Fully consumed — clean up.
            self.icebergs.remove(&order_id);
            return None;
        }

        // Next slice: full visible_qty or the remaining tail, whichever is smaller.
        Some(remaining.min(iceberg.visible_qty))
    }

    // ------------------------------------------------------------------
    // Queries
    // ------------------------------------------------------------------

    /// Remaining unfilled quantity (hidden + the current visible slice).
    ///
    /// Returns `None` if the order is not tracked.
    pub fn remaining(&self, order_id: OrderId) -> Option<Qty> {
        self.icebergs
            .get(&order_id)
            .map(|i| i.total_qty - i.filled_qty)
    }

    /// Returns `true` if `order_id` is currently tracked as an iceberg.
    pub fn is_iceberg(&self, order_id: OrderId) -> bool {
        self.icebergs.contains_key(&order_id)
    }

    /// Cancel and remove tracking state for an iceberg order.
    ///
    /// This should be called whenever the visible slice is cancelled so that
    /// the manager does not keep stale state.  Silently does nothing if the
    /// order is not tracked.
    pub fn cancel(&mut self, order_id: OrderId) {
        self.icebergs.remove(&order_id);
    }

    /// Number of live iceberg orders tracked.
    pub fn count(&self) -> usize {
        self.icebergs.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn mgr() -> IcebergManager {
        IcebergManager::new()
    }

    fn order_id() -> OrderId {
        Uuid::new_v4()
    }

    // ------------------------------------------------------------------
    // Registration
    // ------------------------------------------------------------------

    #[test]
    fn register_tracks_iceberg() {
        let mut m = mgr();
        let id = order_id();
        m.register(id, dec!(10), dec!(2));
        assert!(m.is_iceberg(id));
        assert_eq!(m.remaining(id), Some(dec!(10)));
    }

    #[test]
    fn unregistered_order_is_not_iceberg() {
        let m = mgr();
        assert!(!m.is_iceberg(order_id()));
    }

    #[test]
    fn unregistered_order_remaining_returns_none() {
        let m = mgr();
        assert!(m.remaining(order_id()).is_none());
    }

    // ------------------------------------------------------------------
    // Refill on visible fill
    // ------------------------------------------------------------------

    #[test]
    fn first_visible_fill_returns_next_slice() {
        let mut m = mgr();
        let id = order_id();
        // total=10, visible=2 → after one fill 8 remain, next slice = 2
        m.register(id, dec!(10), dec!(2));

        let next = m.on_visible_filled(id);
        assert_eq!(next, Some(dec!(2)));
        assert_eq!(m.remaining(id), Some(dec!(8)));
    }

    #[test]
    fn multiple_refills_until_tail() {
        let mut m = mgr();
        let id = order_id();
        // total=5, visible=2 → fills: 2, 2, then last slice = 1
        m.register(id, dec!(5), dec!(2));

        assert_eq!(m.on_visible_filled(id), Some(dec!(2))); // 3 remain
        assert_eq!(m.on_visible_filled(id), Some(dec!(1))); // 1 remain (tail < visible)
    }

    #[test]
    fn last_slice_fill_exhausts_iceberg() {
        let mut m = mgr();
        let id = order_id();
        // total=4, visible=4 → single fill exhausts
        m.register(id, dec!(4), dec!(4));

        let next = m.on_visible_filled(id);
        assert!(next.is_none());
        assert!(!m.is_iceberg(id));
    }

    #[test]
    fn on_visible_filled_unknown_order_returns_none() {
        let mut m = mgr();
        assert!(m.on_visible_filled(order_id()).is_none());
    }

    #[test]
    fn after_exhaustion_order_removed() {
        let mut m = mgr();
        let id = order_id();
        m.register(id, dec!(2), dec!(2));
        m.on_visible_filled(id); // exhausts

        assert!(!m.is_iceberg(id));
        assert!(m.remaining(id).is_none());
        assert_eq!(m.count(), 0);
    }

    // ------------------------------------------------------------------
    // Cancel
    // ------------------------------------------------------------------

    #[test]
    fn cancel_removes_tracking() {
        let mut m = mgr();
        let id = order_id();
        m.register(id, dec!(10), dec!(2));
        m.cancel(id);

        assert!(!m.is_iceberg(id));
        assert!(m.remaining(id).is_none());
    }

    #[test]
    fn cancel_unknown_order_is_silent() {
        let mut m = mgr();
        // Should not panic.
        m.cancel(order_id());
    }

    // ------------------------------------------------------------------
    // Count
    // ------------------------------------------------------------------

    #[test]
    fn count_reflects_live_icebergs() {
        let mut m = mgr();
        assert_eq!(m.count(), 0);

        let a = order_id();
        let b = order_id();
        m.register(a, dec!(10), dec!(2));
        m.register(b, dec!(6), dec!(3));
        assert_eq!(m.count(), 2);

        m.cancel(a);
        assert_eq!(m.count(), 1);
    }

    // ------------------------------------------------------------------
    // Tail slice (non-divisible total)
    // ------------------------------------------------------------------

    #[test]
    fn tail_slice_smaller_than_visible() {
        let mut m = mgr();
        let id = order_id();
        // total=7, visible=3 → fills: 3, 3, then tail=1
        m.register(id, dec!(7), dec!(3));

        assert_eq!(m.on_visible_filled(id), Some(dec!(3))); // 4 remain
        assert_eq!(m.on_visible_filled(id), Some(dec!(1))); // 1 remain
        assert!(m.on_visible_filled(id).is_none()); // exhausted
    }
}
