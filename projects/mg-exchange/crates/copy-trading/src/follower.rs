//! Follower management — follow relations, PnL tracking, stop-loss enforcement.
//!
//! A [`FollowRelation`] links one follower to one master, carrying allocation
//! limits, sizing preferences, and an optional stop-loss threshold.
//! [`FollowerManager`] owns all relations and maintains secondary indexes so
//! lookups by either party are O(1) (amortised).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::Zero;
use tracing::{debug, info, warn};
use uuid::Uuid;

use exchange_common::types::UserId;

use crate::error::CopyTradingError;

// ──────────────────────────────────────────────────────────────────────────────
// Types
// ──────────────────────────────────────────────────────────────────────────────

/// A directional follow relationship: one follower mirrors one master.
#[derive(Debug, Clone)]
pub struct FollowRelation {
    /// Unique identifier for this relation.
    pub id: Uuid,
    pub follower_id: UserId,
    pub master_id: UserId,
    /// Maximum USDT the follower is willing to allocate to copy trades.
    pub allocation: Decimal,
    /// USDT currently in open copy positions.
    pub used_allocation: Decimal,
    /// When `true`, order sizes are scaled proportionally to `allocation / master_equity`.
    /// When `false`, the follower uses a fixed per-trade quantity (handled externally).
    pub proportional: bool,
    /// If cumulative PnL on this relation drops below `-stop_loss_pct * allocation`,
    /// the relation is automatically deactivated.
    pub stop_loss_pct: Option<Decimal>,
    /// Cumulative realised PnL on this relation (positive = profit, negative = loss).
    pub total_pnl: Decimal,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

// ──────────────────────────────────────────────────────────────────────────────
// Manager
// ──────────────────────────────────────────────────────────────────────────────

/// Owns all follow relations and the secondary indexes required for efficient
/// fan-out queries.
pub struct FollowerManager {
    relations: HashMap<Uuid, FollowRelation>,
    /// follower_id → list of relation IDs
    follower_to_masters: HashMap<UserId, Vec<Uuid>>,
    /// master_id → list of relation IDs
    master_to_followers: HashMap<UserId, Vec<Uuid>>,
    /// Maximum number of masters a single follower may follow simultaneously.
    max_masters_per_follower: usize,
}

impl FollowerManager {
    /// Create a manager with a custom per-follower master limit.
    pub fn new(max_masters_per_follower: usize) -> Self {
        Self {
            relations: HashMap::new(),
            follower_to_masters: HashMap::new(),
            master_to_followers: HashMap::new(),
            max_masters_per_follower,
        }
    }

    /// Create a manager with the default limit of 10 masters per follower.
    pub fn with_defaults() -> Self {
        Self::new(10)
    }

    // ── Mutations ─────────────────────────────────────────────────────────────

    /// Start following a master.
    ///
    /// Returns the new [`FollowRelation`] on success.
    ///
    /// # Errors
    /// * [`CopyTradingError::AlreadyFollowing`] if this pair already has an active relation.
    /// * [`CopyTradingError::TooManyMasters`] if `max_masters_per_follower` is reached.
    /// * [`CopyTradingError::InvalidAllocation`] if `allocation` is not positive.
    pub fn follow(
        &mut self,
        follower_id: UserId,
        master_id: UserId,
        allocation: Decimal,
        proportional: bool,
        stop_loss_pct: Option<Decimal>,
        // Caller must pass `true` to indicate the user has acknowledged the
        // risk disclosure for copy trading.  Accepted but not yet enforced
        // in Phase 1; will be required in Phase 2 compliance checks.
        _risk_acknowledged: bool,
    ) -> Result<FollowRelation, CopyTradingError> {
        if allocation <= Decimal::zero() {
            return Err(CopyTradingError::InvalidAllocation(allocation.to_string()));
        }

        // Check for an existing active relation between this pair.
        let already_follows = self
            .follower_to_masters
            .get(&follower_id)
            .map(|ids| {
                ids.iter().any(|id| {
                    self.relations
                        .get(id)
                        .map(|r| r.master_id == master_id && r.is_active)
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);

        if already_follows {
            return Err(CopyTradingError::AlreadyFollowing {
                follower: follower_id,
                master: master_id,
            });
        }

        // Enforce per-follower master cap (count only active relations).
        let active_count = self
            .follower_to_masters
            .get(&follower_id)
            .map(|ids| {
                ids.iter()
                    .filter(|id| {
                        self.relations
                            .get(*id)
                            .map(|r| r.is_active)
                            .unwrap_or(false)
                    })
                    .count()
            })
            .unwrap_or(0);

        if active_count >= self.max_masters_per_follower {
            return Err(CopyTradingError::TooManyMasters(follower_id));
        }

        let relation = FollowRelation {
            id: Uuid::new_v4(),
            follower_id,
            master_id,
            allocation,
            used_allocation: Decimal::zero(),
            proportional,
            stop_loss_pct,
            total_pnl: Decimal::zero(),
            is_active: true,
            created_at: Utc::now(),
        };

        let rid = relation.id;
        info!(
            relation_id = %rid,
            %follower_id,
            %master_id,
            %allocation,
            proportional,
            "follow relation created"
        );

        self.follower_to_masters
            .entry(follower_id)
            .or_default()
            .push(rid);
        self.master_to_followers
            .entry(master_id)
            .or_default()
            .push(rid);
        self.relations.insert(rid, relation.clone());

        Ok(relation)
    }

    /// Deactivate a follow relation by ID.  No-ops if the relation does not exist.
    pub fn unfollow(&mut self, relation_id: Uuid) {
        if let Some(rel) = self.relations.get_mut(&relation_id) {
            rel.is_active = false;
            debug!(
                relation_id = %relation_id,
                follower_id = %rel.follower_id,
                master_id = %rel.master_id,
                "follow relation deactivated"
            );
        }
    }

    /// Apply a PnL change to a relation and trigger stop-loss if breached.
    ///
    /// Stop-loss condition: `total_pnl < -(stop_loss_pct * allocation)`.
    pub fn update_pnl(&mut self, relation_id: Uuid, pnl_change: Decimal) {
        let Some(rel) = self.relations.get_mut(&relation_id) else {
            debug!(relation_id = %relation_id, "update_pnl: relation not found, skipping");
            return;
        };

        rel.total_pnl += pnl_change;

        // Check stop-loss.
        if let Some(sl_pct) = rel.stop_loss_pct {
            let threshold = -(sl_pct * rel.allocation);
            if rel.total_pnl < threshold {
                warn!(
                    relation_id = %relation_id,
                    follower_id = %rel.follower_id,
                    master_id = %rel.master_id,
                    total_pnl = %rel.total_pnl,
                    threshold = %threshold,
                    "stop-loss breached — auto-unfollowing"
                );
                rel.is_active = false;
            }
        }
    }

    // ── Queries ───────────────────────────────────────────────────────────────

    /// All active relations where `follower_id` is the follower.
    pub fn follower_masters(&self, follower_id: UserId) -> Vec<&FollowRelation> {
        self.follower_to_masters
            .get(&follower_id)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.relations.get(id))
                    .filter(|r| r.is_active)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// All active relations where `master_id` is the master.
    pub fn master_followers(&self, master_id: UserId) -> Vec<&FollowRelation> {
        self.master_to_followers
            .get(&master_id)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.relations.get(id))
                    .filter(|r| r.is_active)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Scan all active relations and return the IDs of those whose stop-loss
    /// is currently breached.  Useful for batch checks before copying a trade.
    pub fn check_stop_losses(&self) -> Vec<Uuid> {
        self.relations
            .values()
            .filter(|r| r.is_active)
            .filter(|r| {
                r.stop_loss_pct
                    .map(|sl_pct| r.total_pnl < -(sl_pct * r.allocation))
                    .unwrap_or(false)
            })
            .map(|r| r.id)
            .collect()
    }

    /// Look up a relation by ID.
    pub fn get(&self, relation_id: Uuid) -> Option<&FollowRelation> {
        self.relations.get(&relation_id)
    }

    /// Look up a relation by ID (mutable).
    pub fn get_mut(&mut self, relation_id: Uuid) -> Option<&mut FollowRelation> {
        self.relations.get_mut(&relation_id)
    }

    /// Return clones of all currently active follow relations.
    ///
    /// Used by the profit-share settlement task to iterate over all
    /// relations without holding the lock across account mutations.
    pub fn all_active_relations(&self) -> Vec<FollowRelation> {
        self.relations
            .values()
            .filter(|r| r.is_active)
            .cloned()
            .collect()
    }

    /// Reset the `total_pnl` counter on a relation to zero.
    ///
    /// Called after a profit-share settlement so the next settlement
    /// period starts from a clean baseline.
    pub fn reset_pnl(&mut self, relation_id: Uuid) {
        if let Some(rel) = self.relations.get_mut(&relation_id) {
            rel.total_pnl = Decimal::zero();
        }
    }
}

impl Default for FollowerManager {
    fn default() -> Self {
        Self::with_defaults()
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

    fn mgr() -> FollowerManager {
        FollowerManager::with_defaults()
    }

    // ── follow ────────────────────────────────────────────────────────────────

    #[test]
    fn follow_succeeds() {
        let mut mgr = mgr();
        let f = uid();
        let m = uid();
        let rel = mgr.follow(f, m, dec!(1000), true, None, true).unwrap();
        assert_eq!(rel.follower_id, f);
        assert_eq!(rel.master_id, m);
        assert!(rel.is_active);
        assert_eq!(rel.total_pnl, dec!(0));
    }

    #[test]
    fn follow_already_following_returns_error() {
        let mut mgr = mgr();
        let f = uid();
        let m = uid();
        mgr.follow(f, m, dec!(500), true, None, true).unwrap();
        let err = mgr.follow(f, m, dec!(500), true, None, true).unwrap_err();
        assert!(matches!(err, CopyTradingError::AlreadyFollowing { .. }));
    }

    #[test]
    fn follow_zero_allocation_returns_error() {
        let mut mgr = mgr();
        let err = mgr.follow(uid(), uid(), dec!(0), true, None, true).unwrap_err();
        assert!(matches!(err, CopyTradingError::InvalidAllocation(_)));
    }

    #[test]
    fn follow_negative_allocation_returns_error() {
        let mut mgr = mgr();
        let err = mgr
            .follow(uid(), uid(), dec!(-100), true, None, true)
            .unwrap_err();
        assert!(matches!(err, CopyTradingError::InvalidAllocation(_)));
    }

    // ── max masters per follower ───────────────────────────────────────────────

    #[test]
    fn follow_max_masters_limit_enforced() {
        let mut mgr = FollowerManager::new(3);
        let f = uid();
        for _ in 0..3 {
            mgr.follow(f, uid(), dec!(100), true, None, true).unwrap();
        }
        let err = mgr.follow(f, uid(), dec!(100), true, None, true).unwrap_err();
        assert_eq!(err, CopyTradingError::TooManyMasters(f));
    }

    #[test]
    fn unfollowed_slot_does_not_count_toward_cap() {
        let mut mgr = FollowerManager::new(2);
        let f = uid();
        let rel1 = mgr.follow(f, uid(), dec!(100), true, None, true).unwrap();
        mgr.follow(f, uid(), dec!(100), true, None, true).unwrap();
        // At cap; unfollow one.
        mgr.unfollow(rel1.id);
        // Should succeed now.
        assert!(mgr.follow(f, uid(), dec!(100), true, None, true).is_ok());
    }

    // ── unfollow ──────────────────────────────────────────────────────────────

    #[test]
    fn unfollow_deactivates_relation() {
        let mut mgr = mgr();
        let f = uid();
        let m = uid();
        let rel = mgr.follow(f, m, dec!(500), true, None, true).unwrap();
        mgr.unfollow(rel.id);
        assert!(!mgr.get(rel.id).unwrap().is_active);
        // Should no longer appear in follower_masters.
        assert!(mgr.follower_masters(f).is_empty());
    }

    #[test]
    fn unfollow_nonexistent_is_noop() {
        let mut mgr = mgr();
        // Must not panic.
        mgr.unfollow(Uuid::new_v4());
    }

    // ── queries ───────────────────────────────────────────────────────────────

    #[test]
    fn follower_masters_returns_only_active() {
        let mut mgr = mgr();
        let f = uid();
        let m1 = uid();
        let m2 = uid();
        let rel1 = mgr.follow(f, m1, dec!(100), true, None, true).unwrap();
        mgr.follow(f, m2, dec!(100), true, None, true).unwrap();
        mgr.unfollow(rel1.id);

        let active = mgr.follower_masters(f);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].master_id, m2);
    }

    #[test]
    fn master_followers_returns_only_active() {
        let mut mgr = mgr();
        let m = uid();
        let f1 = uid();
        let f2 = uid();
        let rel1 = mgr.follow(f1, m, dec!(100), true, None, true).unwrap();
        mgr.follow(f2, m, dec!(100), true, None, true).unwrap();
        mgr.unfollow(rel1.id);

        let active = mgr.master_followers(m);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].follower_id, f2);
    }

    // ── PnL and stop-loss ─────────────────────────────────────────────────────

    #[test]
    fn update_pnl_accumulates_correctly() {
        let mut mgr = mgr();
        let rel = mgr.follow(uid(), uid(), dec!(1000), true, None, true).unwrap();
        mgr.update_pnl(rel.id, dec!(50));
        mgr.update_pnl(rel.id, dec!(-20));
        assert_eq!(mgr.get(rel.id).unwrap().total_pnl, dec!(30));
    }

    #[test]
    fn stop_loss_triggers_auto_unfollow() {
        let mut mgr = mgr();
        // 10% stop loss on 1000 USDT allocation → threshold = -100 USDT.
        let rel = mgr
            .follow(uid(), uid(), dec!(1000), true, Some(dec!(0.10)), true)
            .unwrap();
        mgr.update_pnl(rel.id, dec!(-50));
        assert!(mgr.get(rel.id).unwrap().is_active, "should still be active");
        mgr.update_pnl(rel.id, dec!(-60)); // total = -110 < -100
        assert!(
            !mgr.get(rel.id).unwrap().is_active,
            "should be deactivated"
        );
    }

    #[test]
    fn stop_loss_not_triggered_when_exactly_at_threshold() {
        let mut mgr = mgr();
        // 10% of 1000 = 100; loss of exactly 100 should NOT trigger.
        let rel = mgr
            .follow(uid(), uid(), dec!(1000), true, Some(dec!(0.10)), true)
            .unwrap();
        mgr.update_pnl(rel.id, dec!(-100));
        assert!(mgr.get(rel.id).unwrap().is_active);
    }

    #[test]
    fn check_stop_losses_returns_breached_ids() {
        let mut mgr = mgr();
        let rel_with_sl = mgr
            .follow(uid(), uid(), dec!(1000), true, Some(dec!(0.05)), true)
            .unwrap();
        let _rel_no_sl = mgr.follow(uid(), uid(), dec!(1000), true, None, true).unwrap();

        // Set pnl directly so the relation stays active (bypassing update_pnl's
        // auto-deactivation), allowing check_stop_losses to detect the breach.
        // -60 < -(0.05 * 1000) = -50  →  breach condition met.
        mgr.get_mut(rel_with_sl.id).unwrap().total_pnl = dec!(-60);

        let breached = mgr.check_stop_losses();
        assert!(breached.contains(&rel_with_sl.id));
    }

    #[test]
    fn update_pnl_unknown_relation_is_noop() {
        let mut mgr = mgr();
        // Must not panic.
        mgr.update_pnl(Uuid::new_v4(), dec!(-100));
    }
}
