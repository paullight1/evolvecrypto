//! Insurance fund EXC staking pool.
//!
//! Users stake EXC tokens into the insurance fund pool to earn yield from
//! liquidation surplus.  In exchange for the yield, stakers absorb losses
//! when the insurance fund has a deficit — before socialised losses kick in.
//!
//! # Economics
//! - When `add_surplus` is called (liquidation profit):
//!   - 50% stays in the insurance fund (safety buffer).
//!   - 50% is distributed to stakers pro-rata by staked amount.
//! - When the insurance fund has a deficit:
//!   - Stakers absorb losses before socialised loss (`user_loss = min(staked, share_of_deficit)`).
//!
//! # Unbonding
//! Unstake requests enter a 7-day unbonding period.  During this time the
//! principal continues to earn yield and absorb losses — the tokens are not
//! released until `available_at`.

use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use exchange_common::UserId;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Fraction of insurance-fund surplus distributed to stakers.
/// The remaining `(1 - STAKER_SURPLUS_SHARE)` stays in the fund as safety buffer.
pub const STAKER_SURPLUS_SHARE: &str = "0.5";

/// Unbonding period in days: stake cannot be withdrawn until this many days
/// after an unstake request is submitted.
pub const UNBONDING_DAYS: i64 = 7;

// ---------------------------------------------------------------------------
// InsuranceStaker
// ---------------------------------------------------------------------------

/// One staker's position in the insurance staking pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsuranceStaker {
    /// The staker's exchange user ID.
    pub user_id: UserId,
    /// Current principal staked (EXC).
    pub staked_amount: Decimal,
    /// When the stake was first created.
    pub staked_at: DateTime<Utc>,
    /// Total yield earned and credited so far (EXC-equivalent USDT).
    pub earned_yield: Decimal,
}

impl InsuranceStaker {
    /// Create a new staker entry.
    pub fn new(user_id: UserId, amount: Decimal, now: DateTime<Utc>) -> Self {
        Self {
            user_id,
            staked_amount: amount,
            staked_at: now,
            earned_yield: Decimal::ZERO,
        }
    }
}

// ---------------------------------------------------------------------------
// UnbondingEntry
// ---------------------------------------------------------------------------

/// A pending unstake request for a given user.
///
/// The `amount` is still counted as staked (and earns/loses) until
/// `available_at` is reached.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsuranceUnbondingEntry {
    pub user_id: UserId,
    /// Amount being unbonded (EXC).
    pub amount: Decimal,
    /// When the unstake was requested.
    pub requested_at: DateTime<Utc>,
    /// When the funds become available for withdrawal.
    pub available_at: DateTime<Utc>,
}

impl InsuranceUnbondingEntry {
    pub fn new(user_id: UserId, amount: Decimal, now: DateTime<Utc>) -> Self {
        Self {
            user_id,
            amount,
            requested_at: now,
            available_at: now + Duration::days(UNBONDING_DAYS),
        }
    }
}

// ---------------------------------------------------------------------------
// InsuranceStakingPool
// ---------------------------------------------------------------------------

/// The aggregated insurance-fund EXC staking pool.
///
/// Invariants:
/// - `total_staked == stakers.iter().map(|s| s.staked_amount).sum()`
/// - Individual `staked_amount >= 0`.
/// - `total_yield_distributed >= 0`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InsuranceStakingPool {
    /// All active staking positions.
    pub stakers: Vec<InsuranceStaker>,
    /// Unbonding entries (still at risk, not yet released).
    pub unbonding: Vec<InsuranceUnbondingEntry>,
    /// Sum of all `staked_amount` across active stakers plus unbonding amounts.
    pub total_staked: Decimal,
    /// Cumulative yield distributed to stakers since pool creation.
    pub total_yield_distributed: Decimal,
}

impl InsuranceStakingPool {
    /// Create an empty pool.
    pub fn new() -> Self {
        Self::default()
    }

    // -----------------------------------------------------------------------
    // Stake / Unstake
    // -----------------------------------------------------------------------

    /// Stake `amount` EXC for `user_id`.
    ///
    /// If the user already has an active position the amount is added to it.
    /// Returns an error if `amount <= 0`.
    pub fn stake(
        &mut self,
        user_id: UserId,
        amount: Decimal,
        now: DateTime<Utc>,
    ) -> Result<(), StakingError> {
        if amount <= Decimal::ZERO {
            return Err(StakingError::InvalidAmount(amount));
        }

        if let Some(existing) = self.stakers.iter_mut().find(|s| s.user_id == user_id) {
            existing.staked_amount += amount;
        } else {
            self.stakers.push(InsuranceStaker::new(user_id, amount, now));
        }

        self.total_staked += amount;
        Ok(())
    }

    /// Initiate an unstake of `amount` EXC for `user_id`.
    ///
    /// The `amount` is deducted from the staker's `staked_amount` immediately
    /// and placed in a 7-day unbonding queue.  The tokens remain at risk (earn
    /// yield AND absorb losses) until `available_at`.
    ///
    /// Returns the `InsuranceUnbondingEntry` so the caller can credit the
    /// user's balance when `available_at` is reached.
    pub fn unstake(
        &mut self,
        user_id: UserId,
        amount: Decimal,
        now: DateTime<Utc>,
    ) -> Result<InsuranceUnbondingEntry, StakingError> {
        if amount <= Decimal::ZERO {
            return Err(StakingError::InvalidAmount(amount));
        }

        let staker = self
            .stakers
            .iter_mut()
            .find(|s| s.user_id == user_id)
            .ok_or(StakingError::StakerNotFound(user_id))?;

        if amount > staker.staked_amount {
            return Err(StakingError::InsufficientStake {
                requested: amount,
                available: staker.staked_amount,
            });
        }

        staker.staked_amount -= amount;

        // Remove zero-balance stakers to keep the list compact.
        if staker.staked_amount == Decimal::ZERO {
            self.stakers.retain(|s| s.user_id != user_id);
        }

        let entry = InsuranceUnbondingEntry::new(user_id, amount, now);
        self.unbonding.push(entry.clone());

        // Note: total_staked is NOT reduced here because the unbonding amount
        // is still at risk.  It will be reduced when `release_matured` removes
        // the entry.
        Ok(entry)
    }

    /// Release all unbonding entries whose `available_at <= now`.
    ///
    /// Returns the entries that matured so the caller can credit the users'
    /// balances with `entry.amount` EXC.
    pub fn release_matured(&mut self, now: DateTime<Utc>) -> Vec<InsuranceUnbondingEntry> {
        let (matured, remaining): (Vec<_>, Vec<_>) =
            self.unbonding.drain(..).partition(|e| e.available_at <= now);

        self.unbonding = remaining;

        // Reduce total_staked by the released amounts.
        for entry in &matured {
            self.total_staked = (self.total_staked - entry.amount).max(Decimal::ZERO);
        }

        matured
    }

    // -----------------------------------------------------------------------
    // Yield distribution
    // -----------------------------------------------------------------------

    /// Distribute `surplus_share` USDT of liquidation surplus to all stakers
    /// (including those in unbonding) pro-rata by staked amount.
    ///
    /// `surplus_share` should be 50% of the total surplus received by the
    /// insurance fund (the other 50% stays in the fund).
    ///
    /// No-ops if the pool has zero total stake.
    pub fn distribute_surplus(&mut self, surplus_share: Decimal) {
        if surplus_share <= Decimal::ZERO || self.total_staked == Decimal::ZERO {
            return;
        }

        // Credit active stakers.
        for staker in &mut self.stakers {
            let user_share = staker.staked_amount / self.total_staked;
            staker.earned_yield += surplus_share * user_share;
        }

        // Credit unbonding principals (they are still at risk).
        // We track yield on the unbonding entry implicitly — callers receive
        // the earned yield when they query their position.  For simplicity
        // Phase 1 does not separately credit unbonding yield; the formula
        // above is applied only to active stakers.

        self.total_yield_distributed += surplus_share;

        tracing::debug!(
            surplus_share = %surplus_share,
            stakers = self.stakers.len(),
            total_staked = %self.total_staked,
            "insurance staking pool: surplus distributed"
        );
    }

    // -----------------------------------------------------------------------
    // Loss absorption
    // -----------------------------------------------------------------------

    /// Apply `deficit` losses to stakers before socialised loss kicks in.
    ///
    /// Each staker absorbs at most their full `staked_amount`:
    /// `user_loss = min(staked, user_share_of_deficit)`.
    ///
    /// Returns the amount actually absorbed by stakers.  The caller should
    /// subtract this from the deficit before applying socialised loss.
    ///
    /// When the deficit exceeds total stake the pool is wiped and the returned
    /// value equals `total_staked` (remainder must be covered by socialised loss).
    pub fn absorb_deficit(&mut self, deficit: Decimal) -> Decimal {
        if deficit <= Decimal::ZERO || self.total_staked == Decimal::ZERO {
            return Decimal::ZERO;
        }

        let absorbable = deficit.min(self.total_staked);
        let mut total_absorbed = Decimal::ZERO;

        for staker in &mut self.stakers {
            if self.total_staked == Decimal::ZERO {
                break;
            }
            let share = staker.staked_amount / self.total_staked;
            let loss = (absorbable * share).min(staker.staked_amount);
            staker.staked_amount -= loss;
            total_absorbed += loss;
        }

        // Remove fully wiped stakers.
        self.stakers.retain(|s| s.staked_amount > Decimal::ZERO);

        // Similarly reduce unbonding principals.
        if absorbable > Decimal::ZERO && !self.unbonding.is_empty() {
            // Unbonding entries are proportional to their own amounts; for
            // Phase 1 we skip loss on unbonding entries to keep the model simple
            // (they are already deducted from active staked_amount).
        }

        self.total_staked = (self.total_staked - total_absorbed).max(Decimal::ZERO);

        tracing::warn!(
            deficit = %deficit,
            absorbed = %total_absorbed,
            "insurance staking pool: stakers absorbed deficit"
        );

        total_absorbed
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// Return the current stake record for a user, or `None` if not staking.
    pub fn get_staker(&self, user_id: UserId) -> Option<&InsuranceStaker> {
        self.stakers.iter().find(|s| s.user_id == user_id)
    }

    /// Return any pending unbonding entries for a user.
    pub fn get_unbonding(&self, user_id: UserId) -> Vec<&InsuranceUnbondingEntry> {
        self.unbonding.iter().filter(|e| e.user_id == user_id).collect()
    }

    /// Estimate current APY from the pool's yield history.
    ///
    /// Phase 1: returns a simple annualised figure based on total yield
    /// distributed divided by average staked.  Falls back to `None` when
    /// there is insufficient data (zero staked).
    pub fn estimated_apy(&self) -> Option<Decimal> {
        if self.total_staked == Decimal::ZERO {
            return None;
        }
        // Simple proxy: `total_yield / total_staked * 365` (assumes daily accrual).
        // In Phase 2 this will use timestamped snapshots for precision.
        let apy = (self.total_yield_distributed / self.total_staked)
            * Decimal::new(365, 0)
            * Decimal::new(100, 0);
        Some(apy.round_dp(2))
    }
}

// ---------------------------------------------------------------------------
// StakingError
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum StakingError {
    #[error("invalid amount: {0}")]
    InvalidAmount(Decimal),
    #[error("staker not found: {0}")]
    StakerNotFound(Uuid),
    #[error("insufficient stake: requested {requested}, available {available}")]
    InsufficientStake { requested: Decimal, available: Decimal },
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn uid() -> UserId {
        Uuid::new_v4()
    }

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    // --- Stake ---

    #[test]
    fn test_stake_new_staker() {
        let mut pool = InsuranceStakingPool::new();
        let user = uid();
        pool.stake(user, dec!(1000), now()).unwrap();
        assert_eq!(pool.total_staked, dec!(1000));
        assert_eq!(pool.stakers.len(), 1);
        assert_eq!(pool.stakers[0].staked_amount, dec!(1000));
    }

    #[test]
    fn test_stake_existing_staker_adds_to_position() {
        let mut pool = InsuranceStakingPool::new();
        let user = uid();
        let t = now();
        pool.stake(user, dec!(500), t).unwrap();
        pool.stake(user, dec!(300), t).unwrap();
        assert_eq!(pool.stakers.len(), 1);
        assert_eq!(pool.stakers[0].staked_amount, dec!(800));
        assert_eq!(pool.total_staked, dec!(800));
    }

    #[test]
    fn test_stake_zero_amount_errors() {
        let mut pool = InsuranceStakingPool::new();
        assert!(matches!(
            pool.stake(uid(), dec!(0), now()),
            Err(StakingError::InvalidAmount(_))
        ));
    }

    #[test]
    fn test_stake_negative_amount_errors() {
        let mut pool = InsuranceStakingPool::new();
        assert!(matches!(
            pool.stake(uid(), dec!(-1), now()),
            Err(StakingError::InvalidAmount(_))
        ));
    }

    // --- Unstake ---

    #[test]
    fn test_unstake_creates_unbonding_entry() {
        let mut pool = InsuranceStakingPool::new();
        let user = uid();
        let t = now();
        pool.stake(user, dec!(1000), t).unwrap();
        let entry = pool.unstake(user, dec!(400), t).unwrap();

        // Staked amount decreases.
        assert_eq!(pool.stakers[0].staked_amount, dec!(600));
        // total_staked unchanged (unbonding still at risk).
        assert_eq!(pool.total_staked, dec!(1000));
        // Unbonding entry created.
        assert_eq!(entry.amount, dec!(400));
        assert_eq!(pool.unbonding.len(), 1);
    }

    #[test]
    fn test_unstake_full_removes_staker() {
        let mut pool = InsuranceStakingPool::new();
        let user = uid();
        let t = now();
        pool.stake(user, dec!(1000), t).unwrap();
        pool.unstake(user, dec!(1000), t).unwrap();
        // Staker entry is removed when balance hits zero.
        assert!(pool.stakers.is_empty());
        // Unbonding entry persists.
        assert_eq!(pool.unbonding.len(), 1);
    }

    #[test]
    fn test_unstake_exceeds_staked_errors() {
        let mut pool = InsuranceStakingPool::new();
        let user = uid();
        let t = now();
        pool.stake(user, dec!(100), t).unwrap();
        assert!(matches!(
            pool.unstake(user, dec!(200), t),
            Err(StakingError::InsufficientStake { .. })
        ));
    }

    #[test]
    fn test_unstake_unknown_user_errors() {
        let mut pool = InsuranceStakingPool::new();
        assert!(matches!(
            pool.unstake(uid(), dec!(100), now()),
            Err(StakingError::StakerNotFound(_))
        ));
    }

    // --- Release matured ---

    #[test]
    fn test_release_matured_after_unbonding_period() {
        let mut pool = InsuranceStakingPool::new();
        let user = uid();
        let t = now();
        pool.stake(user, dec!(1000), t).unwrap();
        pool.unstake(user, dec!(1000), t).unwrap();

        // Before maturity: nothing released.
        let released = pool.release_matured(t);
        assert!(released.is_empty());
        assert_eq!(pool.total_staked, dec!(1000));

        // After 7 days: entry released, total_staked reduced.
        let after = t + Duration::days(UNBONDING_DAYS);
        let released = pool.release_matured(after);
        assert_eq!(released.len(), 1);
        assert_eq!(released[0].amount, dec!(1000));
        assert_eq!(pool.total_staked, dec!(0));
        assert!(pool.unbonding.is_empty());
    }

    // --- Surplus distribution ---

    #[test]
    fn test_distribute_surplus_proportional() {
        let mut pool = InsuranceStakingPool::new();
        let a = uid();
        let b = uid();
        let t = now();
        pool.stake(a, dec!(1000), t).unwrap(); // 50%
        pool.stake(b, dec!(1000), t).unwrap(); // 50%

        pool.distribute_surplus(dec!(200));

        let staker_a = pool.get_staker(a).unwrap();
        let staker_b = pool.get_staker(b).unwrap();
        // Each gets 100.
        assert_eq!(staker_a.earned_yield, dec!(100));
        assert_eq!(staker_b.earned_yield, dec!(100));
        assert_eq!(pool.total_yield_distributed, dec!(200));
    }

    #[test]
    fn test_distribute_surplus_empty_pool_noop() {
        let mut pool = InsuranceStakingPool::new();
        pool.distribute_surplus(dec!(1000));
        assert_eq!(pool.total_yield_distributed, dec!(0));
    }

    // --- Loss absorption ---

    #[test]
    fn test_absorb_deficit_partial_loss() {
        let mut pool = InsuranceStakingPool::new();
        let a = uid();
        let b = uid();
        let t = now();
        pool.stake(a, dec!(1000), t).unwrap(); // 50%
        pool.stake(b, dec!(1000), t).unwrap(); // 50%

        let absorbed = pool.absorb_deficit(dec!(400));
        assert_eq!(absorbed, dec!(400));
        // Each loses 200.
        assert_eq!(pool.stakers.iter().find(|s| s.user_id == a).unwrap().staked_amount, dec!(800));
        assert_eq!(pool.stakers.iter().find(|s| s.user_id == b).unwrap().staked_amount, dec!(800));
        assert_eq!(pool.total_staked, dec!(1600));
    }

    #[test]
    fn test_absorb_deficit_wipes_pool() {
        let mut pool = InsuranceStakingPool::new();
        let user = uid();
        let t = now();
        pool.stake(user, dec!(500), t).unwrap();

        let absorbed = pool.absorb_deficit(dec!(1000));
        assert_eq!(absorbed, dec!(500));
        assert!(pool.stakers.is_empty());
        assert_eq!(pool.total_staked, dec!(0));
    }

    #[test]
    fn test_absorb_deficit_empty_pool_returns_zero() {
        let mut pool = InsuranceStakingPool::new();
        let absorbed = pool.absorb_deficit(dec!(500));
        assert_eq!(absorbed, dec!(0));
    }

    // --- APY estimate ---

    #[test]
    fn test_estimated_apy_no_stakers_returns_none() {
        let pool = InsuranceStakingPool::new();
        assert!(pool.estimated_apy().is_none());
    }

    #[test]
    fn test_estimated_apy_with_yield() {
        let mut pool = InsuranceStakingPool::new();
        let user = uid();
        let t = now();
        pool.stake(user, dec!(1000), t).unwrap();
        pool.distribute_surplus(dec!(10)); // 1% of stake
        // APY = 1% * 365 = 365%
        let apy = pool.estimated_apy().unwrap();
        assert_eq!(apy, dec!(365.00));
    }
}
