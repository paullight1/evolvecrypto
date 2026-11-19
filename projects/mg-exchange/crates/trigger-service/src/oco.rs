use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use exchange_common::{Symbol, UserId};

/// Lifecycle state of an OCO group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OcoStatus {
    /// Both orders are live. Either may fill.
    Active,
    /// Order A filled; order B has been cancelled.
    TriggeredA,
    /// Order B filled; order A has been cancelled.
    TriggeredB,
    /// Both orders were cancelled (by user request or system action).
    Cancelled,
}

/// A One-Cancels-Other group linking two orders.
///
/// The canonical use-case is a TP + SL pair protecting an open position.
/// When one order fills, the sibling must be cancelled immediately to prevent
/// the position from being doubled.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcoGroup {
    pub id: Uuid,
    pub user_id: UserId,
    pub symbol: Symbol,
    /// The first leg (typically the take-profit).
    pub order_a_id: Uuid,
    /// The second leg (typically the stop-loss).
    pub order_b_id: Uuid,
    pub status: OcoStatus,
    pub created_at: DateTime<Utc>,
}

/// Manages all active OCO groups.
///
/// All operations are O(1) using two hash-maps:
///   - `groups`        — group_id → OcoGroup
///   - `order_to_group` — order_id → group_id
///
/// This struct is single-threaded by design. The caller is responsible for any
/// required synchronisation (identical pattern to `TriggerEngine`).
#[derive(Debug, Default)]
pub struct OcoManager {
    groups: HashMap<Uuid, OcoGroup>,
    order_to_group: HashMap<Uuid, Uuid>,
}

impl OcoManager {
    pub fn new() -> Self {
        Self::default()
    }

    // ------------------------------------------------------------------
    // Group management
    // ------------------------------------------------------------------

    /// Create and register a new OCO group.
    ///
    /// Both order IDs must be unique across all live groups.  If either is
    /// already registered this call still succeeds — the previous mapping is
    /// silently overwritten.  In practice the matching engine guarantees order
    /// IDs are globally unique, so collisions should never occur.
    pub fn create_oco(
        &mut self,
        user_id: UserId,
        symbol: Symbol,
        order_a_id: Uuid,
        order_b_id: Uuid,
    ) -> OcoGroup {
        let group = OcoGroup {
            id: Uuid::new_v4(),
            user_id,
            symbol,
            order_a_id,
            order_b_id,
            status: OcoStatus::Active,
            created_at: Utc::now(),
        };

        self.order_to_group.insert(order_a_id, group.id);
        self.order_to_group.insert(order_b_id, group.id);
        self.groups.insert(group.id, group.clone());

        group
    }

    /// Notify the manager that `order_id` has been filled.
    ///
    /// Returns the **sibling** order id that must now be cancelled, or `None`
    /// if the order belongs to no active group (or the group is already
    /// terminal).
    pub fn on_order_filled(&mut self, order_id: Uuid) -> Option<Uuid> {
        let group_id = *self.order_to_group.get(&order_id)?;
        let group = self.groups.get_mut(&group_id)?;

        if group.status != OcoStatus::Active {
            // Already terminal — idempotent, ignore double-fill.
            return None;
        }

        let sibling_id = if order_id == group.order_a_id {
            group.status = OcoStatus::TriggeredA;
            group.order_b_id
        } else {
            group.status = OcoStatus::TriggeredB;
            group.order_a_id
        };

        // Remove both legs from the reverse index — the group is now terminal.
        self.order_to_group.remove(&group.order_a_id);
        self.order_to_group.remove(&group.order_b_id);

        Some(sibling_id)
    }

    /// Notify the manager that `order_id` was manually cancelled by the user.
    ///
    /// The sibling order should also be cancelled.  Returns the sibling's id,
    /// or `None` if the order belongs to no active group.
    pub fn on_order_cancelled(&mut self, order_id: Uuid) -> Option<Uuid> {
        let group_id = *self.order_to_group.get(&order_id)?;
        let group = self.groups.get_mut(&group_id)?;

        if group.status != OcoStatus::Active {
            return None;
        }

        let sibling_id = if order_id == group.order_a_id {
            group.order_b_id
        } else {
            group.order_a_id
        };

        group.status = OcoStatus::Cancelled;
        self.order_to_group.remove(&group.order_a_id);
        self.order_to_group.remove(&group.order_b_id);

        Some(sibling_id)
    }

    /// Cancel an entire OCO group by its group id.
    ///
    /// Returns `(order_a_id, order_b_id)` — both must be forwarded to the
    /// matching engine for cancellation.  Returns `None` if the group does not
    /// exist or is already terminal.
    pub fn cancel_group(&mut self, group_id: Uuid) -> Option<(Uuid, Uuid)> {
        let group = self.groups.get_mut(&group_id)?;

        if group.status != OcoStatus::Active {
            return None;
        }

        let ids = (group.order_a_id, group.order_b_id);
        group.status = OcoStatus::Cancelled;

        self.order_to_group.remove(&group.order_a_id);
        self.order_to_group.remove(&group.order_b_id);

        Some(ids)
    }

    // ------------------------------------------------------------------
    // Queries
    // ------------------------------------------------------------------

    /// All OCO groups (active and terminal) belonging to `user_id`.
    pub fn user_ocos(&self, user_id: UserId) -> Vec<&OcoGroup> {
        self.groups
            .values()
            .filter(|g| g.user_id == user_id)
            .collect()
    }

    /// Look up a group by its id.
    pub fn group(&self, group_id: Uuid) -> Option<&OcoGroup> {
        self.groups.get(&group_id)
    }

    /// Total number of tracked groups (active + terminal).
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    /// Number of order-id → group-id mappings currently live.
    ///
    /// Each active group contributes 2.  Once terminal both legs are removed.
    pub fn active_order_count(&self) -> usize {
        self.order_to_group.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn btc() -> Symbol {
        Symbol::new("BTCUSDT")
    }

    fn manager() -> OcoManager {
        OcoManager::new()
    }

    // ------------------------------------------------------------------
    // Creation
    // ------------------------------------------------------------------

    #[test]
    fn create_oco_returns_active_group() {
        let mut mgr = manager();
        let user = Uuid::new_v4();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();

        let group = mgr.create_oco(user, btc(), a, b);

        assert_eq!(group.status, OcoStatus::Active);
        assert_eq!(group.user_id, user);
        assert_eq!(group.order_a_id, a);
        assert_eq!(group.order_b_id, b);
        // Both legs registered in reverse index.
        assert_eq!(mgr.active_order_count(), 2);
    }

    #[test]
    fn created_group_is_queryable() {
        let mut mgr = manager();
        let user = Uuid::new_v4();
        let group = mgr.create_oco(user, btc(), Uuid::new_v4(), Uuid::new_v4());

        let fetched = mgr.group(group.id).expect("group must exist");
        assert_eq!(fetched.id, group.id);
    }

    // ------------------------------------------------------------------
    // Fill A cancels B
    // ------------------------------------------------------------------

    #[test]
    fn fill_a_returns_b_and_marks_triggered_a() {
        let mut mgr = manager();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let group = mgr.create_oco(Uuid::new_v4(), btc(), a, b);

        let sibling = mgr.on_order_filled(a);

        assert_eq!(sibling, Some(b));
        assert_eq!(mgr.group(group.id).unwrap().status, OcoStatus::TriggeredA);
        // Both legs removed from reverse index after terminal state.
        assert_eq!(mgr.active_order_count(), 0);
    }

    // ------------------------------------------------------------------
    // Fill B cancels A
    // ------------------------------------------------------------------

    #[test]
    fn fill_b_returns_a_and_marks_triggered_b() {
        let mut mgr = manager();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let group = mgr.create_oco(Uuid::new_v4(), btc(), a, b);

        let sibling = mgr.on_order_filled(b);

        assert_eq!(sibling, Some(a));
        assert_eq!(mgr.group(group.id).unwrap().status, OcoStatus::TriggeredB);
        assert_eq!(mgr.active_order_count(), 0);
    }

    // ------------------------------------------------------------------
    // Double fill is ignored (idempotent)
    // ------------------------------------------------------------------

    #[test]
    fn double_fill_on_same_order_is_ignored() {
        let mut mgr = manager();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        mgr.create_oco(Uuid::new_v4(), btc(), a, b);

        let first = mgr.on_order_filled(a);
        let second = mgr.on_order_filled(a); // group is already terminal

        assert!(first.is_some());
        assert!(second.is_none());
    }

    #[test]
    fn fill_sibling_after_first_fill_is_ignored() {
        let mut mgr = manager();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        mgr.create_oco(Uuid::new_v4(), btc(), a, b);

        mgr.on_order_filled(a);
        // Now try to fill the sibling — group is TriggeredA, not Active.
        let result = mgr.on_order_filled(b);
        assert!(result.is_none());
    }

    // ------------------------------------------------------------------
    // Manual cancel propagates to sibling
    // ------------------------------------------------------------------

    #[test]
    fn cancel_a_returns_b_and_marks_cancelled() {
        let mut mgr = manager();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let group = mgr.create_oco(Uuid::new_v4(), btc(), a, b);

        let sibling = mgr.on_order_cancelled(a);

        assert_eq!(sibling, Some(b));
        assert_eq!(mgr.group(group.id).unwrap().status, OcoStatus::Cancelled);
        assert_eq!(mgr.active_order_count(), 0);
    }

    #[test]
    fn cancel_b_returns_a_and_marks_cancelled() {
        let mut mgr = manager();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let group = mgr.create_oco(Uuid::new_v4(), btc(), a, b);

        let sibling = mgr.on_order_cancelled(b);

        assert_eq!(sibling, Some(a));
        assert_eq!(mgr.group(group.id).unwrap().status, OcoStatus::Cancelled);
    }

    // ------------------------------------------------------------------
    // cancel_group cancels both
    // ------------------------------------------------------------------

    #[test]
    fn cancel_group_returns_both_order_ids() {
        let mut mgr = manager();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let group = mgr.create_oco(Uuid::new_v4(), btc(), a, b);

        let ids = mgr.cancel_group(group.id);

        assert_eq!(ids, Some((a, b)));
        assert_eq!(mgr.group(group.id).unwrap().status, OcoStatus::Cancelled);
        assert_eq!(mgr.active_order_count(), 0);
    }

    #[test]
    fn cancel_group_on_unknown_id_returns_none() {
        let mut mgr = manager();
        assert!(mgr.cancel_group(Uuid::new_v4()).is_none());
    }

    #[test]
    fn cancel_group_on_already_terminal_returns_none() {
        let mut mgr = manager();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let group = mgr.create_oco(Uuid::new_v4(), btc(), a, b);

        mgr.cancel_group(group.id);
        // Second cancel on same group.
        assert!(mgr.cancel_group(group.id).is_none());
    }

    // ------------------------------------------------------------------
    // user_ocos query
    // ------------------------------------------------------------------

    #[test]
    fn user_ocos_returns_only_that_users_groups() {
        let mut mgr = manager();
        let user_a = Uuid::new_v4();
        let user_b = Uuid::new_v4();

        mgr.create_oco(user_a, btc(), Uuid::new_v4(), Uuid::new_v4());
        mgr.create_oco(user_a, btc(), Uuid::new_v4(), Uuid::new_v4());
        mgr.create_oco(user_b, btc(), Uuid::new_v4(), Uuid::new_v4());

        assert_eq!(mgr.user_ocos(user_a).len(), 2);
        assert_eq!(mgr.user_ocos(user_b).len(), 1);
        assert_eq!(mgr.user_ocos(Uuid::new_v4()).len(), 0);
    }

    // ------------------------------------------------------------------
    // Unknown order ids
    // ------------------------------------------------------------------

    #[test]
    fn fill_unknown_order_returns_none() {
        let mut mgr = manager();
        assert!(mgr.on_order_filled(Uuid::new_v4()).is_none());
    }

    #[test]
    fn cancel_unknown_order_returns_none() {
        let mut mgr = manager();
        assert!(mgr.on_order_cancelled(Uuid::new_v4()).is_none());
    }
}
