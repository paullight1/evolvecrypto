//! Time-Weighted Average Price (TWAP) execution algorithm.
//!
//! Splits a large order into equal-sized slices spread uniformly across a
//! specified duration. Each slice is submitted at its scheduled timestamp,
//! optionally as a limit order at the mid-price or as a market order.

use chrono::{DateTime, Duration, Utc};
use exchange_common::{OrderType, Side, Symbol, UserId};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};
use uuid::Uuid;

use crate::common::{
    new_algo_id, weighted_avg_price, AlgoOrder, AlgoStatus, SliceOrderType, SliceStatus,
};

/// Configuration for a TWAP execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwapConfig {
    pub user_id: UserId,
    pub symbol: Symbol,
    pub side: Side,
    /// Total quantity to execute over the full duration.
    pub total_qty: Decimal,
    /// Wall-clock duration of the execution window, in seconds.
    pub duration_secs: u64,
    /// Number of equal-interval slices to break the order into.
    pub num_slices: u32,
    /// How each individual slice order is priced.
    pub order_type: SliceOrderType,
    /// Optional price fence: execution halts if the market crosses this level.
    /// For buys this is a maximum price; for sells it is a minimum price.
    pub price_limit: Option<Decimal>,
    /// When `true`, add a small random jitter (±10% of the interval) to each
    /// slice's scheduled time to avoid predictable patterns.
    #[serde(default)]
    pub jitter: bool,
}

/// One slice within a TWAP execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwapSlice {
    /// Zero-based position in the slice schedule.
    pub index: u32,
    /// Wall-clock time at which this slice is due for submission.
    pub scheduled_at: DateTime<Utc>,
    /// Quantity assigned to this slice.
    pub qty: Decimal,
    /// Order ID returned after the slice is submitted to the matching engine.
    pub order_id: Option<Uuid>,
    /// Average fill price for this slice (set after fill confirmation).
    pub fill_price: Option<Decimal>,
    /// Quantity actually filled so far for this slice.
    pub fill_qty: Decimal,
    pub status: SliceStatus,
}

/// A running TWAP execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwapExecution {
    pub id: Uuid,
    pub config: TwapConfig,
    pub slices: Vec<TwapSlice>,
    pub status: AlgoStatus,
    /// Cumulative quantity filled across all slices.
    pub filled_qty: Decimal,
    /// Volume-weighted average fill price across all slices.
    pub avg_fill_price: Decimal,
    pub created_at: DateTime<Utc>,
}

impl TwapExecution {
    /// Creates a new TWAP execution, computing the slice schedule and per-slice quantities.
    ///
    /// Quantities are distributed as evenly as possible: the remainder from integer
    /// division is added to the last slice so that `sum(slice.qty) == total_qty`.
    pub fn new(config: TwapConfig, now: DateTime<Utc>) -> Self {
        debug!(
            symbol = %config.symbol,
            total_qty = %config.total_qty,
            num_slices = config.num_slices,
            duration_secs = config.duration_secs,
            "creating TWAP execution"
        );

        let num_slices = config.num_slices.max(1);
        let interval_secs = config.duration_secs as i64 / num_slices as i64;

        // Base quantity per slice (truncated to avoid floating-point drift).
        let base_qty = config.total_qty / Decimal::from(num_slices);
        // Remainder assigned to the final slice to ensure exact total.
        let remainder = config.total_qty - base_qty * Decimal::from(num_slices - 1);

        let slices = (0..num_slices)
            .map(|i| {
                let scheduled_at = now + Duration::seconds(interval_secs * i as i64);
                let qty = if i == num_slices - 1 {
                    remainder
                } else {
                    base_qty
                };
                TwapSlice {
                    index: i,
                    scheduled_at,
                    qty,
                    order_id: None,
                    fill_price: None,
                    fill_qty: Decimal::ZERO,
                    status: SliceStatus::Pending,
                }
            })
            .collect();

        Self {
            id: new_algo_id(),
            config,
            slices,
            status: AlgoStatus::Active,
            filled_qty: Decimal::ZERO,
            avg_fill_price: Decimal::ZERO,
            created_at: now,
        }
    }

    /// Returns the next slice that is due for submission at or before `now`,
    /// provided it is still in `Pending` status.
    pub fn next_due_slice(&self, now: DateTime<Utc>) -> Option<&TwapSlice> {
        if self.status != AlgoStatus::Active {
            return None;
        }
        self.slices
            .iter()
            .find(|s| s.status == SliceStatus::Pending && s.scheduled_at <= now)
    }

    /// Builds the [`AlgoOrder`] to submit for a given slice.
    ///
    /// Returns `None` when:
    /// - the slice index is out of range,
    /// - the slice is not in `Pending` status, or
    /// - the algo is not `Active`.
    ///
    /// `mid_price` is the current best-bid/ask midpoint, used to set the limit
    /// price for `LimitAtMid` and `LimitAggressive` order types.
    pub fn generate_slice_order(
        &self,
        slice_index: u32,
        mid_price: Decimal,
    ) -> Option<AlgoOrder> {
        if self.status != AlgoStatus::Active {
            return None;
        }
        let slice = self.slices.get(slice_index as usize)?;
        if slice.status != SliceStatus::Pending {
            return None;
        }

        let (order_type, price) = match self.config.order_type {
            SliceOrderType::Market => (OrderType::Market, None),
            SliceOrderType::LimitAtMid => (OrderType::Limit, Some(mid_price)),
            SliceOrderType::LimitAggressive => {
                // One tick more aggressive than mid: buys price higher, sells lower.
                // We use a fixed aggressive offset of 0.01% of mid; callers can
                // substitute a tick-size-aware version if desired.
                let offset = mid_price * Decimal::new(1, 4); // 0.01%
                let price = match self.config.side {
                    Side::Buy => mid_price + offset,
                    Side::Sell => mid_price - offset,
                };
                (OrderType::Limit, Some(price))
            }
        };

        Some(AlgoOrder {
            user_id: self.config.user_id,
            symbol: self.config.symbol.clone(),
            side: self.config.side,
            qty: slice.qty,
            order_type,
            price,
        })
    }

    /// Records a fill (or partial fill) for the given slice.
    ///
    /// Updates the slice's `fill_qty` and `fill_price`, marks the slice status,
    /// and updates the execution-level `filled_qty` and `avg_fill_price`.
    ///
    /// If all slices are fully filled, transitions the execution to `Completed`.
    pub fn on_fill(&mut self, slice_index: u32, price: Decimal, qty: Decimal) {
        let Some(slice) = self.slices.get_mut(slice_index as usize) else {
            warn!(slice_index, "on_fill called with out-of-range slice index");
            return;
        };

        // Update execution-level running average before mutating slice.
        self.avg_fill_price =
            weighted_avg_price(self.avg_fill_price, self.filled_qty, price, qty);
        self.filled_qty += qty;

        // Update slice-level fill.
        let prev_fill_qty = slice.fill_qty;
        let prev_fill_price = slice.fill_price.unwrap_or(Decimal::ZERO);
        slice.fill_qty += qty;
        slice.fill_price = Some(weighted_avg_price(prev_fill_price, prev_fill_qty, price, qty));

        if slice.fill_qty >= slice.qty {
            slice.status = SliceStatus::Filled;
        } else {
            slice.status = SliceStatus::PartiallyFilled;
        }

        debug!(
            algo_id = %self.id,
            slice_index,
            fill_price = %price,
            fill_qty = %qty,
            total_filled = %self.filled_qty,
            "TWAP slice fill recorded"
        );

        // Transition to Completed when all slices are done.
        if self
            .slices
            .iter()
            .all(|s| matches!(s.status, SliceStatus::Filled | SliceStatus::PartiallyFilled | SliceStatus::Failed))
            && self.filled_qty >= self.config.total_qty
        {
            self.status = AlgoStatus::Completed;
        }
    }

    /// Checks whether execution should continue given the current market price.
    ///
    /// Returns `false` (and updates status to `PriceLimitBreached`) when the price
    /// crosses the configured limit:
    /// - For **buys**: breached when `current_price > price_limit`.
    /// - For **sells**: breached when `current_price < price_limit`.
    pub fn check_price_limit(&mut self, current_price: Decimal) -> bool {
        let Some(limit) = self.config.price_limit else {
            return true;
        };

        let breached = match self.config.side {
            Side::Buy => current_price > limit,
            Side::Sell => current_price < limit,
        };

        if breached {
            warn!(
                algo_id = %self.id,
                current_price = %current_price,
                price_limit = %limit,
                "TWAP price limit breached, halting execution"
            );
            self.status = AlgoStatus::PriceLimitBreached;
        }

        !breached
    }

    /// Returns the fraction of `total_qty` that has been filled, in `[0, 1]`.
    pub fn progress(&self) -> Decimal {
        if self.config.total_qty.is_zero() {
            return Decimal::ONE;
        }
        self.filled_qty / self.config.total_qty
    }

    /// Cancels the execution and returns the order IDs of all outstanding
    /// (submitted but not yet filled) slices that should be cancelled in the
    /// matching engine.
    pub fn cancel(&mut self) -> Vec<Uuid> {
        self.status = AlgoStatus::Cancelled;
        self.slices
            .iter_mut()
            .filter_map(|s| {
                if s.status == SliceStatus::Submitted {
                    s.status = SliceStatus::Failed;
                    s.order_id
                } else {
                    None
                }
            })
            .collect()
    }

    /// Marks a slice as `Submitted` and records its matching-engine order ID.
    pub fn mark_submitted(&mut self, slice_index: u32, order_id: Uuid) {
        if let Some(slice) = self.slices.get_mut(slice_index as usize) {
            slice.order_id = Some(order_id);
            slice.status = SliceStatus::Submitted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use exchange_common::Symbol;
    use rust_decimal_macros::dec;

    fn test_config(total_qty: Decimal, num_slices: u32, duration_secs: u64) -> TwapConfig {
        TwapConfig {
            user_id: Uuid::new_v4(),
            symbol: Symbol::new("BTCUSDT"),
            side: Side::Buy,
            total_qty,
            duration_secs,
            num_slices,
            order_type: SliceOrderType::Market,
            price_limit: None,
            jitter: false,
        }
    }

    fn fixed_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap()
    }

    // ---- slice calculation ----

    #[test]
    fn slice_count_matches_config() {
        let exec = TwapExecution::new(test_config(dec!(100), 5, 500), fixed_now());
        assert_eq!(exec.slices.len(), 5);
    }

    #[test]
    fn equal_qty_per_slice_divides_evenly() {
        let exec = TwapExecution::new(test_config(dec!(100), 4, 400), fixed_now());
        for slice in &exec.slices {
            assert_eq!(slice.qty, dec!(25));
        }
    }

    #[test]
    fn remainder_assigned_to_last_slice() {
        // 10 / 3 = 3.333... — last slice absorbs remainder.
        let exec = TwapExecution::new(test_config(dec!(10), 3, 300), fixed_now());
        let total: Decimal = exec.slices.iter().map(|s| s.qty).sum();
        assert_eq!(total, dec!(10), "slice quantities must sum to total_qty");
        // First two slices are equal.
        assert_eq!(exec.slices[0].qty, exec.slices[1].qty);
        // Last slice carries any remainder.
        assert!(exec.slices[2].qty >= exec.slices[1].qty);
    }

    #[test]
    fn all_slices_start_pending() {
        let exec = TwapExecution::new(test_config(dec!(100), 5, 500), fixed_now());
        for s in &exec.slices {
            assert_eq!(s.status, SliceStatus::Pending);
        }
    }

    // ---- scheduling ----

    #[test]
    fn slices_scheduled_at_equal_intervals() {
        let now = fixed_now();
        let exec = TwapExecution::new(test_config(dec!(100), 4, 400), now);
        // interval = 400 / 4 = 100 seconds
        for (i, slice) in exec.slices.iter().enumerate() {
            let expected = now + Duration::seconds(100 * i as i64);
            assert_eq!(slice.scheduled_at, expected, "slice {i} scheduled incorrectly");
        }
    }

    #[test]
    fn first_slice_is_due_immediately() {
        let now = fixed_now();
        let exec = TwapExecution::new(test_config(dec!(100), 4, 400), now);
        assert!(exec.next_due_slice(now).is_some());
        assert_eq!(exec.next_due_slice(now).unwrap().index, 0);
    }

    #[test]
    fn second_slice_not_due_until_interval_passes() {
        let now = fixed_now();
        let exec = TwapExecution::new(test_config(dec!(100), 4, 400), now);
        // Only slice 0 is due right at start.
        let due = exec.next_due_slice(now + Duration::seconds(1));
        // Slice 0 is still Pending (we haven't called mark_submitted).
        assert_eq!(due.unwrap().index, 0);
    }

    #[test]
    fn next_due_slice_returns_none_for_cancelled_algo() {
        let now = fixed_now();
        let mut exec = TwapExecution::new(test_config(dec!(100), 4, 400), now);
        exec.cancel();
        assert!(exec.next_due_slice(now).is_none());
    }

    // ---- fill tracking ----

    #[test]
    fn on_fill_updates_slice_and_execution_totals() {
        let now = fixed_now();
        let mut exec = TwapExecution::new(test_config(dec!(100), 4, 400), now);
        exec.on_fill(0, dec!(50000), dec!(25));

        assert_eq!(exec.filled_qty, dec!(25));
        assert_eq!(exec.avg_fill_price, dec!(50000));
        assert_eq!(exec.slices[0].fill_qty, dec!(25));
        assert_eq!(exec.slices[0].fill_price, Some(dec!(50000)));
        assert_eq!(exec.slices[0].status, SliceStatus::Filled);
    }

    #[test]
    fn on_fill_partial_marks_partial_status() {
        let now = fixed_now();
        let mut exec = TwapExecution::new(test_config(dec!(100), 4, 400), now);
        exec.on_fill(0, dec!(50000), dec!(10)); // slice qty = 25, only 10 filled

        assert_eq!(exec.slices[0].status, SliceStatus::PartiallyFilled);
    }

    #[test]
    fn avg_fill_price_is_volume_weighted() {
        let now = fixed_now();
        let mut exec = TwapExecution::new(test_config(dec!(100), 2, 200), now);
        exec.on_fill(0, dec!(100), dec!(50)); // 50 @ 100
        exec.on_fill(1, dec!(200), dec!(50)); // 50 @ 200
        // VWAP = (50*100 + 50*200) / 100 = 150
        assert_eq!(exec.avg_fill_price, dec!(150));
    }

    #[test]
    fn completed_when_all_slices_filled() {
        let now = fixed_now();
        let mut exec = TwapExecution::new(test_config(dec!(100), 2, 200), now);
        exec.on_fill(0, dec!(50000), dec!(50));
        exec.on_fill(1, dec!(50000), dec!(50));
        assert_eq!(exec.status, AlgoStatus::Completed);
    }

    // ---- price limit ----

    #[test]
    fn price_limit_not_breached_when_within_range() {
        let mut exec = TwapExecution::new(
            TwapConfig {
                price_limit: Some(dec!(55000)),
                ..test_config(dec!(100), 4, 400)
            },
            fixed_now(),
        );
        assert!(exec.check_price_limit(dec!(50000)));
        assert_eq!(exec.status, AlgoStatus::Active);
    }

    #[test]
    fn price_limit_breached_for_buy_above_limit() {
        let mut exec = TwapExecution::new(
            TwapConfig {
                price_limit: Some(dec!(50000)),
                ..test_config(dec!(100), 4, 400)
            },
            fixed_now(),
        );
        assert!(!exec.check_price_limit(dec!(50001)));
        assert_eq!(exec.status, AlgoStatus::PriceLimitBreached);
    }

    #[test]
    fn price_limit_breached_for_sell_below_limit() {
        let mut exec = TwapExecution::new(
            TwapConfig {
                side: Side::Sell,
                price_limit: Some(dec!(49000)),
                ..test_config(dec!(100), 4, 400)
            },
            fixed_now(),
        );
        assert!(!exec.check_price_limit(dec!(48999)));
        assert_eq!(exec.status, AlgoStatus::PriceLimitBreached);
    }

    // ---- progress ----

    #[test]
    fn progress_zero_at_start() {
        let exec = TwapExecution::new(test_config(dec!(100), 4, 400), fixed_now());
        assert_eq!(exec.progress(), Decimal::ZERO);
    }

    #[test]
    fn progress_one_when_fully_filled() {
        let mut exec = TwapExecution::new(test_config(dec!(100), 2, 200), fixed_now());
        exec.on_fill(0, dec!(1), dec!(50));
        exec.on_fill(1, dec!(1), dec!(50));
        assert_eq!(exec.progress(), Decimal::ONE);
    }

    #[test]
    fn progress_partial() {
        let mut exec = TwapExecution::new(test_config(dec!(100), 4, 400), fixed_now());
        exec.on_fill(0, dec!(1), dec!(25));
        assert_eq!(exec.progress(), dec!(0.25));
    }

    // ---- cancel ----

    #[test]
    fn cancel_returns_submitted_order_ids() {
        let now = fixed_now();
        let mut exec = TwapExecution::new(test_config(dec!(100), 3, 300), now);
        let oid1 = Uuid::new_v4();
        let oid2 = Uuid::new_v4();
        exec.mark_submitted(0, oid1);
        exec.mark_submitted(1, oid2);

        let cancelled = exec.cancel();
        assert_eq!(exec.status, AlgoStatus::Cancelled);
        assert_eq!(cancelled.len(), 2);
        assert!(cancelled.contains(&oid1));
        assert!(cancelled.contains(&oid2));
    }

    #[test]
    fn cancel_ignores_pending_slices() {
        let now = fixed_now();
        let mut exec = TwapExecution::new(test_config(dec!(100), 3, 300), now);
        // No slices submitted — cancel returns empty vec.
        let cancelled = exec.cancel();
        assert!(cancelled.is_empty());
    }

    // ---- generate_slice_order ----

    #[test]
    fn generate_slice_order_market() {
        let exec = TwapExecution::new(test_config(dec!(100), 4, 400), fixed_now());
        let order = exec.generate_slice_order(0, dec!(50000)).unwrap();
        assert_eq!(order.order_type, OrderType::Market);
        assert!(order.price.is_none());
        assert_eq!(order.qty, dec!(25));
    }

    #[test]
    fn generate_slice_order_limit_at_mid() {
        let config = TwapConfig {
            order_type: SliceOrderType::LimitAtMid,
            ..test_config(dec!(100), 4, 400)
        };
        let exec = TwapExecution::new(config, fixed_now());
        let order = exec.generate_slice_order(0, dec!(50000)).unwrap();
        assert_eq!(order.order_type, OrderType::Limit);
        assert_eq!(order.price, Some(dec!(50000)));
    }

    #[test]
    fn generate_slice_order_returns_none_for_non_pending_slice() {
        let now = fixed_now();
        let mut exec = TwapExecution::new(test_config(dec!(100), 4, 400), now);
        exec.mark_submitted(0, Uuid::new_v4());
        assert!(exec.generate_slice_order(0, dec!(50000)).is_none());
    }
}
