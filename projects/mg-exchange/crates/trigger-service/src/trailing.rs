use std::collections::HashMap;

use rust_decimal::Decimal;
use uuid::Uuid;

use exchange_common::Side;

use crate::trigger_order::TrailType;

/// Tracks the running price peak (for sell trailing stops) or trough (for buy
/// trailing stops) per active trailing-stop order.
///
/// This is intentionally a plain data structure with no async state so it can
/// be embedded directly inside `TriggerEngine` without locking.
#[derive(Debug, Default)]
pub struct TrailingStopTracker {
    /// Maps order_id → (side, extreme_price, trail_type).
    ///
    /// For `Side::Sell` the extreme is the highest price seen since the order
    /// was registered (the "peak").  For `Side::Buy` it is the lowest price
    /// seen (the "trough").
    extremes: HashMap<Uuid, (Side, Decimal, TrailType)>,
}

impl TrailingStopTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new trailing-stop order and seed its extreme with the
    /// current price.
    pub fn register(&mut self, order_id: Uuid, side: Side, current_price: Decimal) {
        self.register_typed(order_id, side, current_price, TrailType::Fixed);
    }

    /// Register a new trailing-stop order with explicit `TrailType`.
    pub fn register_typed(
        &mut self,
        order_id: Uuid,
        side: Side,
        current_price: Decimal,
        trail_type: TrailType,
    ) {
        self.extremes.insert(order_id, (side, current_price, trail_type));
    }

    /// Update the tracked extreme for an order with the latest price.
    ///
    /// For sells the extreme only moves up (we track the peak).
    /// For buys the extreme only moves down (we track the trough).
    pub fn update_price(&mut self, order_id: Uuid, current_price: Decimal) {
        if let Some((side, extreme, _)) = self.extremes.get_mut(&order_id) {
            match side {
                Side::Sell => {
                    if current_price > *extreme {
                        *extreme = current_price;
                    }
                }
                Side::Buy => {
                    if current_price < *extreme {
                        *extreme = current_price;
                    }
                }
            }
        }
    }

    /// Returns `true` when `current_price` has retraced at least
    /// `trailing_delta` from the tracked extreme.
    ///
    /// The threshold is computed based on `TrailType`:
    /// - `Fixed`:      retrace >= trailing_delta (absolute price distance).
    /// - `Percentage`: retrace >= extreme × trailing_delta / 100.
    ///
    /// Sell: peak − current >= threshold → trigger.
    /// Buy:  current − trough >= threshold → trigger.
    pub fn should_trigger(
        &self,
        order_id: Uuid,
        current_price: Decimal,
        trailing_delta: Decimal,
    ) -> bool {
        match self.extremes.get(&order_id) {
            None => false,
            Some((Side::Sell, peak, trail_type)) => {
                let threshold = compute_threshold(*peak, trailing_delta, *trail_type);
                peak - current_price >= threshold
            }
            Some((Side::Buy, trough, trail_type)) => {
                let threshold = compute_threshold(*trough, trailing_delta, *trail_type);
                current_price - trough >= threshold
            }
        }
    }

    /// Remove tracking state for an order (on cancel, trigger, or expiry).
    pub fn remove(&mut self, order_id: Uuid) {
        self.extremes.remove(&order_id);
    }

    /// Return the current tracked extreme for an order, if any.
    pub fn extreme(&self, order_id: Uuid) -> Option<Decimal> {
        self.extremes.get(&order_id).map(|(_, e, _)| *e)
    }

    /// Return the `TrailType` for an order, if registered.
    pub fn trail_type(&self, order_id: Uuid) -> Option<TrailType> {
        self.extremes.get(&order_id).map(|(_, _, t)| *t)
    }
}

/// Compute the trigger threshold from an extreme price, delta, and trail type.
///
/// - `Fixed`:      threshold = delta (absolute price units).
/// - `Percentage`: threshold = extreme × delta / 100.
fn compute_threshold(extreme: Decimal, delta: Decimal, trail_type: TrailType) -> Decimal {
    match trail_type {
        TrailType::Fixed => delta,
        TrailType::Percentage => extreme * delta / Decimal::ONE_HUNDRED,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn sell_tracker_at(price: Decimal) -> (TrailingStopTracker, Uuid) {
        let mut t = TrailingStopTracker::new();
        let id = Uuid::new_v4();
        t.register(id, Side::Sell, price);
        (t, id)
    }

    fn buy_tracker_at(price: Decimal) -> (TrailingStopTracker, Uuid) {
        let mut t = TrailingStopTracker::new();
        let id = Uuid::new_v4();
        t.register(id, Side::Buy, price);
        (t, id)
    }

    // --- sell trailing stop ---

    #[test]
    fn sell_trailing_no_trigger_before_retrace() {
        let (t, id) = sell_tracker_at(dec!(100));
        // Price barely dropped but not by delta yet
        assert!(!t.should_trigger(id, dec!(99), dec!(5)));
    }

    #[test]
    fn sell_trailing_triggers_on_exact_delta() {
        let (t, id) = sell_tracker_at(dec!(100));
        assert!(t.should_trigger(id, dec!(95), dec!(5)));
    }

    #[test]
    fn sell_trailing_triggers_when_retrace_exceeds_delta() {
        let (t, id) = sell_tracker_at(dec!(100));
        assert!(t.should_trigger(id, dec!(90), dec!(5)));
    }

    #[test]
    fn sell_trailing_peak_advances_with_price() {
        let (mut t, id) = sell_tracker_at(dec!(100));
        t.update_price(id, dec!(110));
        t.update_price(id, dec!(105)); // would not lower the peak
        assert_eq!(t.extreme(id), Some(dec!(110)));
        // Delta 5 from 110: need price <= 105 to trigger
        assert!(!t.should_trigger(id, dec!(106), dec!(5)));
        assert!(t.should_trigger(id, dec!(105), dec!(5)));
    }

    #[test]
    fn sell_trailing_peak_does_not_decrease() {
        let (mut t, id) = sell_tracker_at(dec!(100));
        t.update_price(id, dec!(80)); // price dropped — peak stays at 100
        assert_eq!(t.extreme(id), Some(dec!(100)));
    }

    // --- buy trailing stop ---

    #[test]
    fn buy_trailing_no_trigger_before_retrace() {
        let (t, id) = buy_tracker_at(dec!(100));
        assert!(!t.should_trigger(id, dec!(101), dec!(5)));
    }

    #[test]
    fn buy_trailing_triggers_on_exact_delta() {
        let (t, id) = buy_tracker_at(dec!(100));
        assert!(t.should_trigger(id, dec!(105), dec!(5)));
    }

    #[test]
    fn buy_trailing_triggers_when_retrace_exceeds_delta() {
        let (t, id) = buy_tracker_at(dec!(100));
        assert!(t.should_trigger(id, dec!(110), dec!(5)));
    }

    #[test]
    fn buy_trailing_trough_advances_downward() {
        let (mut t, id) = buy_tracker_at(dec!(100));
        t.update_price(id, dec!(90));
        t.update_price(id, dec!(95)); // would not raise the trough
        assert_eq!(t.extreme(id), Some(dec!(90)));
        // Delta 5 from 90: need price >= 95 to trigger
        assert!(!t.should_trigger(id, dec!(94), dec!(5)));
        assert!(t.should_trigger(id, dec!(95), dec!(5)));
    }

    #[test]
    fn buy_trailing_trough_does_not_increase() {
        let (mut t, id) = buy_tracker_at(dec!(100));
        t.update_price(id, dec!(120)); // price rose — trough stays at 100
        assert_eq!(t.extreme(id), Some(dec!(100)));
    }

    // --- unregistered order ---

    #[test]
    fn unknown_order_never_triggers() {
        let t = TrailingStopTracker::new();
        assert!(!t.should_trigger(Uuid::new_v4(), dec!(50), dec!(1)));
    }

    #[test]
    fn remove_clears_tracking() {
        let (mut t, id) = sell_tracker_at(dec!(100));
        t.remove(id);
        assert_eq!(t.extreme(id), None);
        assert!(!t.should_trigger(id, dec!(50), dec!(1)));
    }

    // --- percentage trailing ---

    #[test]
    fn percentage_sell_trailing_triggers_on_exact_pct() {
        let mut t = TrailingStopTracker::new();
        let id = Uuid::new_v4();
        // Seed at 100; 1% trail → threshold = 1.0
        t.register_typed(id, Side::Sell, dec!(100), TrailType::Percentage);
        // Dropped exactly 1% from 100 → 99
        assert!(t.should_trigger(id, dec!(99), dec!(1)));
    }

    #[test]
    fn percentage_sell_trailing_no_trigger_above_threshold() {
        let mut t = TrailingStopTracker::new();
        let id = Uuid::new_v4();
        t.register_typed(id, Side::Sell, dec!(100), TrailType::Percentage);
        // Only 0.5% retrace — threshold is 1%
        assert!(!t.should_trigger(id, dec!(99.5), dec!(1)));
    }

    #[test]
    fn percentage_buy_trailing_triggers_on_exact_pct() {
        let mut t = TrailingStopTracker::new();
        let id = Uuid::new_v4();
        // Seed at 100; 2% trail → threshold = 2.0
        t.register_typed(id, Side::Buy, dec!(100), TrailType::Percentage);
        // Rose exactly 2% from 100 → 102
        assert!(t.should_trigger(id, dec!(102), dec!(2)));
    }

    #[test]
    fn percentage_sell_peak_advances_threshold_proportionally() {
        let mut t = TrailingStopTracker::new();
        let id = Uuid::new_v4();
        t.register_typed(id, Side::Sell, dec!(100), TrailType::Percentage);
        t.update_price(id, dec!(200)); // peak moves to 200; 1% → threshold = 2
        assert!(!t.should_trigger(id, dec!(199), dec!(1))); // only 0.5% retrace
        assert!(t.should_trigger(id, dec!(198), dec!(1))); // exactly 1% retrace
    }
}
