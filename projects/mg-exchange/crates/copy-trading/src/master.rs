//! Master trader registry.
//!
//! A master trader is a user whose trades are broadcast to followers.
//! The registry tracks performance statistics and controls registration,
//! deactivation, and leaderboard queries.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::Zero;
use tracing::{debug, info};

use exchange_common::types::UserId;

use crate::error::CopyTradingError;

// ──────────────────────────────────────────────────────────────────────────────
// Types
// ──────────────────────────────────────────────────────────────────────────────

/// A user registered as a signal provider (master trader).
#[derive(Debug, Clone)]
pub struct MasterTrader {
    pub user_id: UserId,
    pub display_name: String,
    /// Cumulative realised PnL across all trades while acting as master.
    pub total_pnl: Decimal,
    /// Total number of closed trades.
    pub total_trades: u64,
    /// Win rate in [0, 1].
    pub win_rate: Decimal,
    /// Current number of active followers.
    pub followers_count: u32,
    /// Hard cap on followers.
    pub max_followers: u32,
    /// Fraction of follower profits taken as performance fee, e.g. `0.10` = 10 %.
    pub profit_share_rate: Decimal,
    pub is_active: bool,
    pub registered_at: DateTime<Utc>,
    // Internal counters used to recompute win_rate incrementally.
    wins: u64,
}

impl MasterTrader {
    fn new(
        user_id: UserId,
        display_name: String,
        profit_share_rate: Decimal,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            user_id,
            display_name,
            total_pnl: Decimal::zero(),
            total_trades: 0,
            win_rate: Decimal::zero(),
            followers_count: 0,
            max_followers: 500,
            profit_share_rate,
            is_active: true,
            registered_at: now,
            wins: 0,
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Registry
// ──────────────────────────────────────────────────────────────────────────────

/// In-memory store for all registered master traders.
pub struct MasterRegistry {
    masters: HashMap<UserId, MasterTrader>,
    /// M23-3: Signal latency samples per master (milliseconds).
    /// Used to compute P99 latency from master order → follower order submission.
    signal_latencies: HashMap<UserId, Vec<u64>>,
}

impl MasterRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            masters: HashMap::new(),
            signal_latencies: HashMap::new(),
        }
    }

    /// M23-3: Record a signal propagation latency (in milliseconds).
    pub fn record_signal_latency(&mut self, master_id: UserId, latency_ms: u64) {
        let samples = self.signal_latencies.entry(master_id).or_default();
        samples.push(latency_ms);
        // Keep last 1000 samples per master.
        if samples.len() > 1000 {
            samples.drain(..samples.len() - 1000);
        }
    }

    /// M23-3: Get P99 signal latency for a master (in milliseconds).
    /// Returns None if no samples recorded.
    pub fn signal_latency_p99(&self, master_id: UserId) -> Option<u64> {
        let samples = self.signal_latencies.get(&master_id)?;
        if samples.is_empty() {
            return None;
        }
        let mut sorted = samples.clone();
        sorted.sort();
        let p99_idx = (sorted.len() as f64 * 0.99).ceil() as usize;
        let idx = p99_idx.min(sorted.len()) - 1;
        Some(sorted[idx])
    }

    /// M23-3: Check if any master has P99 latency above threshold.
    /// Returns list of (master_id, p99_ms) for those above threshold.
    pub fn latency_alerts(&self, threshold_ms: u64) -> Vec<(UserId, u64)> {
        self.signal_latencies
            .keys()
            .filter_map(|&mid| {
                let p99 = self.signal_latency_p99(mid)?;
                if p99 > threshold_ms {
                    Some((mid, p99))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Register `user_id` as a master trader.
    ///
    /// * `profit_share_rate` — must be in \[0, 1\].
    pub fn register(
        &mut self,
        user_id: UserId,
        display_name: impl Into<String>,
        profit_share_rate: Decimal,
    ) -> Result<(), CopyTradingError> {
        if self.masters.contains_key(&user_id) {
            return Err(CopyTradingError::AlreadyMaster(user_id));
        }

        if profit_share_rate < Decimal::zero() || profit_share_rate > Decimal::ONE {
            return Err(CopyTradingError::InvalidProfitShareRate(
                profit_share_rate.to_string(),
            ));
        }

        let master = MasterTrader::new(user_id, display_name.into(), profit_share_rate, Utc::now());
        info!(%user_id, profit_share_rate = %profit_share_rate, "master trader registered");
        self.masters.insert(user_id, master);
        Ok(())
    }

    /// Deactivate a master trader.  Their followers will receive no more copies.
    /// No-ops if the user is not a master.
    pub fn deactivate(&mut self, user_id: UserId) {
        if let Some(master) = self.masters.get_mut(&user_id) {
            master.is_active = false;
            info!(%user_id, "master trader deactivated");
        }
    }

    /// Update the profit share rate for an existing master.
    /// Returns error if not registered or rate is invalid.
    pub fn update_profit_share(
        &mut self,
        user_id: UserId,
        profit_share_rate: Decimal,
    ) -> Result<(), CopyTradingError> {
        if profit_share_rate < Decimal::ZERO || profit_share_rate > Decimal::ONE {
            return Err(CopyTradingError::InvalidProfitShareRate(profit_share_rate.to_string()));
        }
        match self.masters.get_mut(&user_id) {
            Some(master) => {
                master.profit_share_rate = profit_share_rate;
                info!(%user_id, rate = %profit_share_rate, "master profit share updated");
                Ok(())
            }
            None => Err(CopyTradingError::MasterNotFound(user_id)),
        }
    }

    /// Look up a master by user ID.
    pub fn get(&self, user_id: UserId) -> Option<&MasterTrader> {
        self.masters.get(&user_id)
    }

    /// Mutable access to a master (for seeding/testing).
    pub fn get_mut(&mut self, user_id: UserId) -> Option<&mut MasterTrader> {
        self.masters.get_mut(&user_id)
    }

    /// Record a completed trade for a master and recompute their statistics.
    ///
    /// * `pnl_change` — signed PnL for this trade (positive = profit, negative = loss).
    /// * `is_win` — whether the trade is counted as a winning trade.
    pub fn update_stats(&mut self, user_id: UserId, pnl_change: Decimal, is_win: bool) {
        let Some(master) = self.masters.get_mut(&user_id) else {
            debug!(%user_id, "update_stats: master not found, skipping");
            return;
        };

        master.total_pnl += pnl_change;
        master.total_trades += 1;
        if is_win {
            master.wins += 1;
        }
        master.win_rate = Decimal::from(master.wins) / Decimal::from(master.total_trades);
    }

    /// Return up to `limit` masters sorted by `total_pnl` descending.
    pub fn leaderboard(&self, limit: usize) -> Vec<&MasterTrader> {
        let mut all: Vec<&MasterTrader> = self.masters.values().collect();
        all.sort_by(|a, b| b.total_pnl.cmp(&a.total_pnl));
        all.truncate(limit);
        all
    }

    /// Returns `true` if the user is registered as a master (active or not).
    pub fn is_master(&self, user_id: UserId) -> bool {
        self.masters.contains_key(&user_id)
    }

    // ── M23-1: Front-running protection ──────────────────────────────────────

    /// Check if a master's recent fill price is consistently better than
    /// followers' average fill price. Returns the advantage ratio if suspicious.
    ///
    /// `master_fill_price` — the price at which the master filled.
    /// `follower_avg_price` — the average price followers got on the same signal.
    /// `side` — trade side (determines what "better" means).
    ///
    /// Returns `Some(advantage_pct)` if the master got a >= 1% better price.
    pub fn check_front_run_advantage(
        &self,
        master_fill_price: Decimal,
        follower_avg_price: Decimal,
        side: exchange_common::Side,
    ) -> Option<Decimal> {
        if follower_avg_price.is_zero() {
            return None;
        }
        let advantage = match side {
            exchange_common::Side::Buy => {
                // Master buying cheaper = advantage.
                (follower_avg_price - master_fill_price) / follower_avg_price
            }
            exchange_common::Side::Sell => {
                // Master selling higher = advantage.
                (master_fill_price - follower_avg_price) / follower_avg_price
            }
        };

        if advantage >= Decimal::new(1, 2) {
            // ≥ 1% advantage
            Some(advantage)
        } else {
            None
        }
    }

    // ── M23-2: Performance statistics verification ──────────────────────────

    /// Independently recompute a master's performance stats from a trade log.
    ///
    /// Returns `(computed_pnl, computed_win_rate)`. Caller compares against
    /// the master's reported stats and alerts if deviation > threshold.
    pub fn audit_performance(
        &self,
        master_id: UserId,
        trade_pnls: &[Decimal],
    ) -> Option<(Decimal, Decimal)> {
        if !self.masters.contains_key(&master_id) {
            return None;
        }
        if trade_pnls.is_empty() {
            return Some((Decimal::ZERO, Decimal::ZERO));
        }

        let total_pnl: Decimal = trade_pnls.iter().sum();
        let wins = trade_pnls.iter().filter(|&&pnl| pnl > Decimal::ZERO).count();
        let win_rate = Decimal::from(wins as u64) / Decimal::from(trade_pnls.len() as u64);

        Some((total_pnl, win_rate))
    }

    /// Check if reported vs computed stats deviate by more than `threshold_pct`.
    /// Returns `Some((reported, computed))` if there's a discrepancy.
    pub fn check_performance_discrepancy(
        &self,
        master_id: UserId,
        trade_pnls: &[Decimal],
        threshold_pct: Decimal,
    ) -> Option<(Decimal, Decimal)> {
        let master = self.masters.get(&master_id)?;
        let (computed_pnl, _computed_wr) = self.audit_performance(master_id, trade_pnls)?;

        if master.total_pnl.is_zero() && computed_pnl.is_zero() {
            return None;
        }

        let reported = master.total_pnl;
        let deviation = if reported.is_zero() {
            if computed_pnl.is_zero() { Decimal::ZERO } else { Decimal::ONE }
        } else {
            ((reported - computed_pnl) / reported).abs()
        };

        if deviation > threshold_pct {
            Some((reported, computed_pnl))
        } else {
            None
        }
    }
}

impl Default for MasterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    use super::*;

    fn uid() -> UserId {
        Uuid::new_v4()
    }

    fn registry() -> MasterRegistry {
        MasterRegistry::new()
    }

    // ── register ─────────────────────────────────────────────────────────────

    #[test]
    fn register_succeeds() {
        let mut reg = registry();
        let id = uid();
        assert!(reg.register(id, "Alice", dec!(0.10)).is_ok());
        assert!(reg.is_master(id));
    }

    #[test]
    fn register_duplicate_returns_error() {
        let mut reg = registry();
        let id = uid();
        reg.register(id, "Alice", dec!(0.10)).unwrap();
        let err = reg.register(id, "Alice2", dec!(0.10)).unwrap_err();
        assert_eq!(err, CopyTradingError::AlreadyMaster(id));
    }

    #[test]
    fn register_invalid_rate_above_one_returns_error() {
        let mut reg = registry();
        let err = reg.register(uid(), "Bob", dec!(1.01)).unwrap_err();
        assert!(matches!(err, CopyTradingError::InvalidProfitShareRate(_)));
    }

    #[test]
    fn register_invalid_rate_negative_returns_error() {
        let mut reg = registry();
        let err = reg.register(uid(), "Bob", dec!(-0.01)).unwrap_err();
        assert!(matches!(err, CopyTradingError::InvalidProfitShareRate(_)));
    }

    #[test]
    fn register_zero_rate_is_valid() {
        let mut reg = registry();
        assert!(reg.register(uid(), "Charlie", dec!(0)).is_ok());
    }

    #[test]
    fn register_full_rate_is_valid() {
        let mut reg = registry();
        assert!(reg.register(uid(), "Dave", dec!(1)).is_ok());
    }

    // ── deactivate ───────────────────────────────────────────────────────────

    #[test]
    fn deactivate_sets_inactive() {
        let mut reg = registry();
        let id = uid();
        reg.register(id, "Alice", dec!(0.10)).unwrap();
        assert!(reg.get(id).unwrap().is_active);
        reg.deactivate(id);
        assert!(!reg.get(id).unwrap().is_active);
    }

    #[test]
    fn deactivate_nonexistent_is_noop() {
        let mut reg = registry();
        // Must not panic.
        reg.deactivate(uid());
    }

    // ── stats update ─────────────────────────────────────────────────────────

    #[test]
    fn update_stats_accumulates_pnl_and_win_rate() {
        let mut reg = registry();
        let id = uid();
        reg.register(id, "Alice", dec!(0.10)).unwrap();

        reg.update_stats(id, dec!(100), true);
        reg.update_stats(id, dec!(-50), false);
        reg.update_stats(id, dec!(200), true);

        let master = reg.get(id).unwrap();
        assert_eq!(master.total_pnl, dec!(250));
        assert_eq!(master.total_trades, 3);
        // 2 wins / 3 trades
        let expected_wr = dec!(2) / dec!(3);
        assert_eq!(master.win_rate, expected_wr);
    }

    #[test]
    fn update_stats_unknown_user_is_noop() {
        let mut reg = registry();
        // Must not panic.
        reg.update_stats(uid(), dec!(100), true);
    }

    // ── leaderboard ──────────────────────────────────────────────────────────

    #[test]
    fn leaderboard_sorted_by_pnl_desc() {
        let mut reg = registry();
        let a = uid();
        let b = uid();
        let c = uid();
        reg.register(a, "A", dec!(0.10)).unwrap();
        reg.register(b, "B", dec!(0.10)).unwrap();
        reg.register(c, "C", dec!(0.10)).unwrap();

        reg.update_stats(a, dec!(50), true);
        reg.update_stats(b, dec!(200), true);
        reg.update_stats(c, dec!(100), true);

        let board = reg.leaderboard(10);
        assert_eq!(board[0].user_id, b);
        assert_eq!(board[1].user_id, c);
        assert_eq!(board[2].user_id, a);
    }

    #[test]
    fn leaderboard_respects_limit() {
        let mut reg = registry();
        for _ in 0..5 {
            reg.register(uid(), "X", dec!(0.05)).unwrap();
        }
        assert_eq!(reg.leaderboard(3).len(), 3);
    }

    #[test]
    fn leaderboard_limit_larger_than_count_returns_all() {
        let mut reg = registry();
        for _ in 0..3 {
            reg.register(uid(), "X", dec!(0.05)).unwrap();
        }
        assert_eq!(reg.leaderboard(100).len(), 3);
    }

    // ── is_master ────────────────────────────────────────────────────────────

    #[test]
    fn is_master_returns_false_for_unknown() {
        let reg = registry();
        assert!(!reg.is_master(uid()));
    }

    #[test]
    fn is_master_returns_true_after_register() {
        let mut reg = registry();
        let id = uid();
        reg.register(id, "Alice", dec!(0.10)).unwrap();
        assert!(reg.is_master(id));
    }

    #[test]
    fn is_master_still_true_after_deactivate() {
        let mut reg = registry();
        let id = uid();
        reg.register(id, "Alice", dec!(0.10)).unwrap();
        reg.deactivate(id);
        // Still *registered*, just inactive.
        assert!(reg.is_master(id));
    }
}
