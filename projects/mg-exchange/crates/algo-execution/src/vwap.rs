//! Volume-Weighted Average Price (VWAP) execution algorithm.
//!
//! Distributes a large order across time slices according to a historical volume
//! profile. Slices with heavier historical volume receive proportionally more
//! quantity, mimicking market participation without moving the price.

use chrono::{DateTime, Duration, Utc};
use exchange_common::{OrderType, Side, Symbol, UserId};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};
use uuid::Uuid;

use crate::common::{
    new_algo_id, weighted_avg_price, AlgoOrder, AlgoStatus, SliceOrderType, SliceStatus,
};

/// Configuration for a VWAP execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VwapConfig {
    pub user_id: UserId,
    pub symbol: Symbol,
    pub side: Side,
    /// Total quantity to execute over the full duration.
    pub total_qty: Decimal,
    /// Wall-clock duration of the execution window, in seconds.
    pub duration_secs: u64,
    /// Number of time-slices the window is divided into.
    pub num_slices: u32,
    /// Historical volume profile: each element is the fraction of daily volume
    /// that occurs in the corresponding time bucket.
    ///
    /// The length of this vector must equal `num_slices`.  Values do not need to
    /// be pre-normalised — the implementation normalises them at construction time.
    /// If the vector is empty or contains only zeros, quantities are distributed
    /// uniformly (fallback to TWAP behaviour).
    pub volume_profile: Vec<Decimal>,
    /// Optional price fence. Execution halts if the market crosses this level.
    /// For buys this is a maximum acceptable price; for sells a minimum.
    pub price_limit: Option<Decimal>,
    /// How each individual slice order is priced.
    pub order_type: SliceOrderType,
    /// When `true`, add a small random jitter (±10% of the interval) to each
    /// slice's scheduled time to avoid predictable patterns.
    #[serde(default)]
    pub jitter: bool,
}

/// One slice within a VWAP execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VwapSlice {
    /// Zero-based position in the slice schedule.
    pub index: u32,
    /// Wall-clock time at which this slice is due for submission.
    pub scheduled_at: DateTime<Utc>,
    /// Quantity assigned to this slice, weighted by the volume profile.
    pub target_qty: Decimal,
    /// Order ID returned after submission to the matching engine.
    pub order_id: Option<Uuid>,
    /// Average fill price for this slice (set after fill confirmation).
    pub fill_price: Option<Decimal>,
    /// Quantity actually filled so far for this slice.
    pub fill_qty: Decimal,
    pub status: SliceStatus,
}

/// A running VWAP execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VwapExecution {
    pub id: Uuid,
    pub config: VwapConfig,
    pub slices: Vec<VwapSlice>,
    pub status: AlgoStatus,
    /// Cumulative quantity filled across all slices.
    pub filled_qty: Decimal,
    /// Volume-weighted average fill price across all slices.
    pub avg_fill_price: Decimal,
    /// Current market VWAP, updated via [`Self::update_benchmark`].
    /// Used to measure execution quality (slippage).
    pub vwap_benchmark: Option<Decimal>,
    pub created_at: DateTime<Utc>,
}

impl VwapExecution {
    /// Creates a new VWAP execution, distributing `total_qty` proportionally
    /// according to `config.volume_profile`.
    ///
    /// If the profile length differs from `num_slices`, or if all weights are
    /// zero, quantities are distributed uniformly across slices. Any rounding
    /// remainder is added to the last slice so that `sum(slice.qty) == total_qty`.
    pub fn new(config: VwapConfig, now: DateTime<Utc>) -> Self {
        debug!(
            symbol = %config.symbol,
            total_qty = %config.total_qty,
            num_slices = config.num_slices,
            duration_secs = config.duration_secs,
            "creating VWAP execution"
        );

        let num_slices = config.num_slices.max(1) as usize;
        let interval_secs = config.duration_secs as i64 / num_slices as i64;

        // Distribute total_qty according to volume profile weights.
        let quantities = distribute_qty(config.total_qty, &config.volume_profile, num_slices);

        let slices = quantities
            .into_iter()
            .enumerate()
            .map(|(i, qty)| VwapSlice {
                index: i as u32,
                scheduled_at: now + Duration::seconds(interval_secs * i as i64),
                target_qty: qty,
                order_id: None,
                fill_price: None,
                fill_qty: Decimal::ZERO,
                status: SliceStatus::Pending,
            })
            .collect();

        Self {
            id: new_algo_id(),
            config,
            slices,
            status: AlgoStatus::Active,
            filled_qty: Decimal::ZERO,
            avg_fill_price: Decimal::ZERO,
            vwap_benchmark: None,
            created_at: now,
        }
    }

    /// Returns the next slice that is due for submission at or before `now`,
    /// provided it is still in `Pending` status.
    pub fn next_due_slice(&self, now: DateTime<Utc>) -> Option<&VwapSlice> {
        if self.status != AlgoStatus::Active {
            return None;
        }
        self.slices
            .iter()
            .find(|s| s.status == SliceStatus::Pending && s.scheduled_at <= now)
    }

    /// Builds the [`AlgoOrder`] to submit for a given slice.
    ///
    /// Returns `None` when the slice index is out of range, the slice is not
    /// `Pending`, or the algo is not `Active`.
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
            qty: slice.target_qty,
            order_type,
            price,
        })
    }

    /// Records a fill (or partial fill) for the given slice, updating both the
    /// slice and the execution-level aggregate statistics.
    pub fn on_fill(&mut self, slice_index: u32, price: Decimal, qty: Decimal) {
        let Some(slice) = self.slices.get_mut(slice_index as usize) else {
            warn!(slice_index, "on_fill called with out-of-range slice index");
            return;
        };

        self.avg_fill_price =
            weighted_avg_price(self.avg_fill_price, self.filled_qty, price, qty);
        self.filled_qty += qty;

        let prev_fill_qty = slice.fill_qty;
        let prev_fill_price = slice.fill_price.unwrap_or(Decimal::ZERO);
        slice.fill_qty += qty;
        slice.fill_price = Some(weighted_avg_price(prev_fill_price, prev_fill_qty, price, qty));

        if slice.fill_qty >= slice.target_qty {
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
            "VWAP slice fill recorded"
        );

        // Transition to Completed when all slices are resolved.
        if self
            .slices
            .iter()
            .all(|s| matches!(s.status, SliceStatus::Filled | SliceStatus::PartiallyFilled | SliceStatus::Failed))
            && self.filled_qty >= self.config.total_qty
        {
            self.status = AlgoStatus::Completed;
        }
    }

    /// Updates the market VWAP benchmark for slippage tracking.
    pub fn update_benchmark(&mut self, market_vwap: Decimal) {
        self.vwap_benchmark = Some(market_vwap);
    }

    /// Returns the execution slippage relative to the market VWAP benchmark.
    ///
    /// A positive value means we paid more (or sold for less) than the market
    /// VWAP — i.e., worse execution. Returns `None` when no fills have been
    /// recorded or no benchmark has been set.
    ///
    /// Formula:
    /// - Buy:  `avg_fill_price - benchmark`  (positive = slippage cost)
    /// - Sell: `benchmark - avg_fill_price`  (positive = slippage cost)
    pub fn slippage(&self) -> Option<Decimal> {
        let benchmark = self.vwap_benchmark?;
        if self.filled_qty.is_zero() {
            return None;
        }
        let slippage = match self.config.side {
            Side::Buy => self.avg_fill_price - benchmark,
            Side::Sell => benchmark - self.avg_fill_price,
        };
        Some(slippage)
    }

    /// Checks whether execution should continue given the current market price.
    ///
    /// Returns `false` (and updates status) when the price crosses the limit.
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
                "VWAP price limit breached, halting execution"
            );
            self.status = AlgoStatus::PriceLimitBreached;
        }

        !breached
    }

    /// Cancels the execution and returns the order IDs of all submitted
    /// (outstanding) slices that must be cancelled in the matching engine.
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

/// Distributes `total_qty` across `num_slices` according to `profile`.
///
/// When the profile length matches `num_slices` and its weight sum is non-zero,
/// each slice receives `total_qty * (weight / total_weight)`.  Otherwise
/// quantities are distributed uniformly using integer division so that no
/// floating-point imprecision accumulates.
///
/// In both cases the remainder from truncation is added to the last slice,
/// guaranteeing `sum(quantities) == total_qty` exactly.
fn distribute_qty(total_qty: Decimal, profile: &[Decimal], num_slices: usize) -> Vec<Decimal> {
    if profile.len() == num_slices {
        let weight_sum: Decimal = profile.iter().copied().sum();
        if !weight_sum.is_zero() {
            let mut qtys: Vec<Decimal> = profile
                .iter()
                .map(|&w| total_qty * w / weight_sum)
                .collect();
            // Correct rounding residual on the last slice.
            let distributed: Decimal = qtys.iter().copied().sum();
            if let Some(last) = qtys.last_mut() {
                *last += total_qty - distributed;
            }
            return qtys;
        }
    }
    // Fallback: uniform distribution via integer division to stay exact.
    let n = Decimal::from(num_slices as u64);
    let base = total_qty / n;
    // Truncate to avoid recurring decimal in base.
    let base = base.trunc();
    let mut qtys = vec![base; num_slices];
    // Assign remainder (total - n*base) to last slice.
    let remainder = total_qty - base * n;
    if let Some(last) = qtys.last_mut() {
        *last += remainder;
    }
    qtys
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use exchange_common::Symbol;
    use rust_decimal_macros::dec;

    fn fixed_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap()
    }

    fn uniform_config(total_qty: Decimal, num_slices: u32) -> VwapConfig {
        VwapConfig {
            user_id: Uuid::new_v4(),
            symbol: Symbol::new("ETHUSDT"),
            side: Side::Buy,
            total_qty,
            duration_secs: num_slices as u64 * 60,
            num_slices,
            volume_profile: vec![],
            price_limit: None,
            order_type: SliceOrderType::Market,
            jitter: false,
        }
    }

    fn profile_config(total_qty: Decimal, profile: Vec<Decimal>) -> VwapConfig {
        let num_slices = profile.len() as u32;
        VwapConfig {
            user_id: Uuid::new_v4(),
            symbol: Symbol::new("ETHUSDT"),
            side: Side::Buy,
            total_qty,
            duration_secs: num_slices as u64 * 60,
            num_slices,
            volume_profile: profile,
            price_limit: None,
            order_type: SliceOrderType::Market,
            jitter: false,
        }
    }

    // ---- volume-weighted distribution ----

    #[test]
    fn uniform_profile_distributes_evenly() {
        let exec = VwapExecution::new(uniform_config(dec!(120), 4), fixed_now());
        assert_eq!(exec.slices.len(), 4);
        let total: Decimal = exec.slices.iter().map(|s| s.target_qty).sum();
        assert_eq!(total, dec!(120));
        // All slices should be equal for uniform profile.
        for s in &exec.slices {
            assert_eq!(s.target_qty, dec!(30));
        }
    }

    #[test]
    fn volume_profile_weights_applied_correctly() {
        // Weights: [0.5, 0.3, 0.2] — total = 1.0 (already normalised).
        let exec = VwapExecution::new(
            profile_config(dec!(100), vec![dec!(0.5), dec!(0.3), dec!(0.2)]),
            fixed_now(),
        );
        assert_eq!(exec.slices[0].target_qty, dec!(50));
        assert_eq!(exec.slices[1].target_qty, dec!(30));
        assert_eq!(exec.slices[2].target_qty, dec!(20));
    }

    #[test]
    fn higher_volume_buckets_get_more_qty() {
        // Profile where bucket 1 is clearly dominant.
        let exec = VwapExecution::new(
            profile_config(dec!(100), vec![dec!(0.1), dec!(0.7), dec!(0.2)]),
            fixed_now(),
        );
        assert!(exec.slices[1].target_qty > exec.slices[0].target_qty);
        assert!(exec.slices[1].target_qty > exec.slices[2].target_qty);
    }

    #[test]
    fn unnormalised_profile_sums_to_total_qty() {
        // Weights don't add to 1.0 — should be normalised internally.
        let exec = VwapExecution::new(
            profile_config(dec!(100), vec![dec!(2), dec!(3), dec!(5)]),
            fixed_now(),
        );
        let total: Decimal = exec.slices.iter().map(|s| s.target_qty).sum();
        assert_eq!(total, dec!(100));
        // Proportions: 2/10=0.2, 3/10=0.3, 5/10=0.5
        assert_eq!(exec.slices[0].target_qty, dec!(20));
        assert_eq!(exec.slices[1].target_qty, dec!(30));
        assert_eq!(exec.slices[2].target_qty, dec!(50));
    }

    #[test]
    fn wrong_profile_length_falls_back_to_uniform() {
        // Profile has 2 elements but config says 4 slices.
        let config = VwapConfig {
            num_slices: 4,
            volume_profile: vec![dec!(0.8), dec!(0.2)], // length mismatch
            ..uniform_config(dec!(100), 4)
        };
        let exec = VwapExecution::new(config, fixed_now());
        let total: Decimal = exec.slices.iter().map(|s| s.target_qty).sum();
        assert_eq!(total, dec!(100));
        // Should fall back to 25 each.
        for s in &exec.slices {
            assert_eq!(s.target_qty, dec!(25));
        }
    }

    #[test]
    fn all_zero_profile_falls_back_to_uniform() {
        let exec = VwapExecution::new(
            profile_config(dec!(90), vec![dec!(0), dec!(0), dec!(0)]),
            fixed_now(),
        );
        for s in &exec.slices {
            assert_eq!(s.target_qty, dec!(30));
        }
    }

    // ---- scheduling ----

    #[test]
    fn slices_scheduled_at_equal_intervals() {
        let now = fixed_now();
        let exec = VwapExecution::new(uniform_config(dec!(100), 4), now);
        // interval = 4*60 / 4 = 60 seconds
        for (i, slice) in exec.slices.iter().enumerate() {
            let expected = now + Duration::seconds(60 * i as i64);
            assert_eq!(slice.scheduled_at, expected, "slice {i} scheduled incorrectly");
        }
    }

    #[test]
    fn first_slice_is_due_immediately() {
        let now = fixed_now();
        let exec = VwapExecution::new(uniform_config(dec!(100), 4), now);
        let due = exec.next_due_slice(now);
        assert!(due.is_some());
        assert_eq!(due.unwrap().index, 0);
    }

    // ---- fill tracking ----

    #[test]
    fn on_fill_updates_slice_and_execution_totals() {
        let now = fixed_now();
        let mut exec = VwapExecution::new(uniform_config(dec!(100), 4), now);
        exec.on_fill(0, dec!(2000), dec!(25));

        assert_eq!(exec.filled_qty, dec!(25));
        assert_eq!(exec.avg_fill_price, dec!(2000));
        assert_eq!(exec.slices[0].fill_qty, dec!(25));
        assert_eq!(exec.slices[0].status, SliceStatus::Filled);
    }

    #[test]
    fn avg_fill_price_is_volume_weighted() {
        let mut exec = VwapExecution::new(uniform_config(dec!(100), 2), fixed_now());
        exec.on_fill(0, dec!(100), dec!(50));
        exec.on_fill(1, dec!(300), dec!(50));
        // VWAP = (50*100 + 50*300) / 100 = 200
        assert_eq!(exec.avg_fill_price, dec!(200));
    }

    #[test]
    fn on_fill_out_of_range_does_not_panic() {
        let mut exec = VwapExecution::new(uniform_config(dec!(100), 2), fixed_now());
        // Should log a warning but not panic.
        exec.on_fill(99, dec!(100), dec!(10));
        assert_eq!(exec.filled_qty, Decimal::ZERO);
    }

    // ---- benchmark and slippage ----

    #[test]
    fn slippage_none_when_no_benchmark_set() {
        let mut exec = VwapExecution::new(uniform_config(dec!(100), 2), fixed_now());
        exec.on_fill(0, dec!(100), dec!(50));
        assert!(exec.slippage().is_none());
    }

    #[test]
    fn slippage_none_when_no_fills() {
        let mut exec = VwapExecution::new(uniform_config(dec!(100), 2), fixed_now());
        exec.update_benchmark(dec!(100));
        assert!(exec.slippage().is_none());
    }

    #[test]
    fn benchmark_comparison_buy_positive_slippage() {
        let mut exec = VwapExecution::new(uniform_config(dec!(100), 2), fixed_now());
        exec.on_fill(0, dec!(105), dec!(50)); // paid above benchmark
        exec.update_benchmark(dec!(100));
        // avg_fill = 105, benchmark = 100 → slippage = +5 (cost)
        assert_eq!(exec.slippage(), Some(dec!(5)));
    }

    #[test]
    fn benchmark_comparison_buy_negative_slippage() {
        let mut exec = VwapExecution::new(uniform_config(dec!(100), 2), fixed_now());
        exec.on_fill(0, dec!(95), dec!(50)); // paid below benchmark — favourable
        exec.update_benchmark(dec!(100));
        // avg_fill = 95, benchmark = 100 → slippage = -5 (improvement)
        assert_eq!(exec.slippage(), Some(dec!(-5)));
    }

    #[test]
    fn benchmark_comparison_sell_positive_slippage() {
        let config = VwapConfig {
            side: Side::Sell,
            ..uniform_config(dec!(100), 2)
        };
        let mut exec = VwapExecution::new(config, fixed_now());
        exec.on_fill(0, dec!(95), dec!(50)); // sold below benchmark — cost
        exec.update_benchmark(dec!(100));
        // sell slippage = benchmark - avg_fill = 100 - 95 = +5
        assert_eq!(exec.slippage(), Some(dec!(5)));
    }

    #[test]
    fn update_benchmark_stores_value() {
        let mut exec = VwapExecution::new(uniform_config(dec!(100), 2), fixed_now());
        exec.update_benchmark(dec!(3500));
        assert_eq!(exec.vwap_benchmark, Some(dec!(3500)));
    }
}
