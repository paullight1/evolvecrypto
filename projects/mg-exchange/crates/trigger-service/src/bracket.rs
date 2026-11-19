use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use exchange_common::{Price, Qty, Symbol, UserId};

/// Specification for a trigger leg (TP or SL) inside a bracket order.
///
/// These parameters are used by `TriggerEngine::add_order` once the entry has
/// filled.  The caller is responsible for mapping them into a full
/// `TriggerOrder`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerOrderSpec {
    /// The price at which the leg activates.
    pub trigger_price: Decimal,
    /// Limit price after activation.  `None` means market order.
    pub order_price: Option<Price>,
    /// Quantity to execute (should match the filled entry quantity).
    pub qty: Qty,
}

/// Lifecycle state of a bracket order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BracketStatus {
    /// Entry order not yet filled; TP/SL legs are dormant.
    Pending,
    /// Entry filled; both TP and SL are live in the trigger engine.
    Active,
    /// TP leg triggered; SL has been cancelled.
    TpFilled,
    /// SL leg triggered; TP has been cancelled.
    SlFilled,
    /// All legs cancelled before the entry filled, or cancelled by user after
    /// entry filled.
    Cancelled,
}

/// A bracket order: entry + optional TP + optional SL as one atomic unit.
///
/// TP and SL are only activated in the trigger engine after the entry fills.
/// Before that they exist only as specifications stored here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BracketOrder {
    pub id: Uuid,
    pub user_id: UserId,
    pub symbol: Symbol,
    /// The entry order placed in the matching engine.
    pub entry_order_id: Uuid,
    /// Take-profit specification (activated on entry fill).
    pub tp_trigger: Option<TriggerOrderSpec>,
    /// Stop-loss specification (activated on entry fill).
    pub sl_trigger: Option<TriggerOrderSpec>,
    pub status: BracketStatus,
    pub created_at: DateTime<Utc>,
    /// Trigger-engine order id of the active TP leg (set after activation).
    pub tp_trigger_order_id: Option<Uuid>,
    /// Trigger-engine order id of the active SL leg (set after activation).
    pub sl_trigger_order_id: Option<Uuid>,
}

/// Manages all bracket orders.
///
/// Index layout:
///   - `brackets`         — bracket_id → BracketOrder
///   - `entry_to_bracket` — entry_order_id → bracket_id  (lookup on entry fill)
///   - `trigger_to_bracket` — trigger_order_id → bracket_id (lookup on TP/SL fill)
///
/// Single-threaded by design, consistent with `TriggerEngine`.
#[derive(Debug, Default)]
pub struct BracketManager {
    brackets: HashMap<Uuid, BracketOrder>,
    /// Reverse index: matching-engine entry order → bracket.
    entry_to_bracket: HashMap<Uuid, Uuid>,
    /// Reverse index: trigger-engine tp/sl order → bracket.
    trigger_to_bracket: HashMap<Uuid, Uuid>,
}

impl BracketManager {
    pub fn new() -> Self {
        Self::default()
    }

    // ------------------------------------------------------------------
    // Order management
    // ------------------------------------------------------------------

    /// Register a new bracket order.
    ///
    /// The entry order must already have been submitted to the matching engine.
    /// TP and SL legs are only submitted to the trigger engine once
    /// `on_entry_filled` is called.
    pub fn create_bracket(
        &mut self,
        user_id: UserId,
        symbol: Symbol,
        entry_order_id: Uuid,
        tp: Option<TriggerOrderSpec>,
        sl: Option<TriggerOrderSpec>,
    ) -> BracketOrder {
        let bracket = BracketOrder {
            id: Uuid::new_v4(),
            user_id,
            symbol,
            entry_order_id,
            tp_trigger: tp,
            sl_trigger: sl,
            status: BracketStatus::Pending,
            created_at: Utc::now(),
            tp_trigger_order_id: None,
            sl_trigger_order_id: None,
        };

        self.entry_to_bracket.insert(entry_order_id, bracket.id);
        self.brackets.insert(bracket.id, bracket.clone());

        bracket
    }

    /// Notify that the entry order has been filled.
    ///
    /// Returns `(Option<TriggerOrderSpec>, Option<TriggerOrderSpec>)` — the TP
    /// and SL specs that must be submitted to `TriggerEngine`.  The caller is
    /// responsible for constructing the full `TriggerOrder` and then recording
    /// the resulting trigger-order ids via `record_trigger_ids`.
    ///
    /// Returns `None` for both if the bracket does not exist, is not `Pending`,
    /// or has neither TP nor SL.
    pub fn on_entry_filled(
        &mut self,
        entry_order_id: Uuid,
    ) -> Option<(Option<TriggerOrderSpec>, Option<TriggerOrderSpec>)> {
        let bracket_id = *self.entry_to_bracket.get(&entry_order_id)?;
        let bracket = self.brackets.get_mut(&bracket_id)?;

        if bracket.status != BracketStatus::Pending {
            return None;
        }

        bracket.status = BracketStatus::Active;
        self.entry_to_bracket.remove(&entry_order_id);

        Some((bracket.tp_trigger.clone(), bracket.sl_trigger.clone()))
    }

    /// Record the trigger-engine order ids assigned to TP and SL after they
    /// have been submitted to `TriggerEngine`.  Call this immediately after
    /// `on_entry_filled` succeeds.
    ///
    /// These ids are needed so that when TP fires we know the SL trigger id to
    /// cancel (and vice versa).
    pub fn record_trigger_ids(
        &mut self,
        bracket_id: Uuid,
        tp_id: Option<Uuid>,
        sl_id: Option<Uuid>,
    ) {
        if let Some(bracket) = self.brackets.get_mut(&bracket_id) {
            if let Some(id) = tp_id {
                bracket.tp_trigger_order_id = Some(id);
                self.trigger_to_bracket.insert(id, bracket_id);
            }
            if let Some(id) = sl_id {
                bracket.sl_trigger_order_id = Some(id);
                self.trigger_to_bracket.insert(id, bracket_id);
            }
        }
    }

    /// Notify that the TP trigger order has been filled.
    ///
    /// Transitions the bracket to `TpFilled` and returns the SL trigger-order
    /// id that must be cancelled in the trigger engine, if any.
    pub fn on_tp_filled(&mut self, bracket_id: Uuid) -> Option<Uuid> {
        let bracket = self.brackets.get_mut(&bracket_id)?;

        if bracket.status != BracketStatus::Active {
            return None;
        }

        bracket.status = BracketStatus::TpFilled;

        // Clean up reverse indices.
        if let Some(id) = bracket.tp_trigger_order_id {
            self.trigger_to_bracket.remove(&id);
        }
        if let Some(id) = bracket.sl_trigger_order_id {
            self.trigger_to_bracket.remove(&id);
        }

        bracket.sl_trigger_order_id
    }

    /// Notify that the SL trigger order has been filled.
    ///
    /// Transitions the bracket to `SlFilled` and returns the TP trigger-order
    /// id that must be cancelled in the trigger engine, if any.
    pub fn on_sl_filled(&mut self, bracket_id: Uuid) -> Option<Uuid> {
        let bracket = self.brackets.get_mut(&bracket_id)?;

        if bracket.status != BracketStatus::Active {
            return None;
        }

        bracket.status = BracketStatus::SlFilled;

        // Clean up reverse indices.
        if let Some(id) = bracket.tp_trigger_order_id {
            self.trigger_to_bracket.remove(&id);
        }
        if let Some(id) = bracket.sl_trigger_order_id {
            self.trigger_to_bracket.remove(&id);
        }

        bracket.tp_trigger_order_id
    }

    /// Cancel a bracket order entirely.
    ///
    /// If `Pending` — the entry has not filled yet; only the entry itself needs
    /// to be cancelled in the matching engine (returned as the first tuple
    /// element).
    ///
    /// If `Active` — both trigger legs need to be cancelled in the trigger
    /// engine; their ids are returned as the second tuple element.
    ///
    /// Returns `None` if the bracket does not exist or is already terminal.
    ///
    /// Return type: `(entry_order_id, (Option<tp_trigger_id>, Option<sl_trigger_id>))`
    pub fn cancel_bracket(
        &mut self,
        bracket_id: Uuid,
    ) -> Option<(Uuid, (Option<Uuid>, Option<Uuid>))> {
        let bracket = self.brackets.get_mut(&bracket_id)?;

        match bracket.status {
            BracketStatus::Pending | BracketStatus::Active => {}
            _ => return None, // already terminal
        }

        let entry_id = bracket.entry_order_id;
        let tp_id = bracket.tp_trigger_order_id;
        let sl_id = bracket.sl_trigger_order_id;

        bracket.status = BracketStatus::Cancelled;

        // Clean up reverse indices.
        self.entry_to_bracket.remove(&entry_id);
        if let Some(id) = tp_id {
            self.trigger_to_bracket.remove(&id);
        }
        if let Some(id) = sl_id {
            self.trigger_to_bracket.remove(&id);
        }

        Some((entry_id, (tp_id, sl_id)))
    }

    // ------------------------------------------------------------------
    // Queries
    // ------------------------------------------------------------------

    /// Look up a bracket by its id.
    pub fn bracket(&self, bracket_id: Uuid) -> Option<&BracketOrder> {
        self.brackets.get(&bracket_id)
    }

    /// Look up the bracket that contains `entry_order_id` as its entry leg.
    pub fn bracket_for_entry(&self, entry_order_id: Uuid) -> Option<&BracketOrder> {
        let bracket_id = self.entry_to_bracket.get(&entry_order_id)?;
        self.brackets.get(bracket_id)
    }

    /// Look up the bracket that contains `trigger_order_id` as a TP or SL leg.
    pub fn bracket_for_trigger(&self, trigger_order_id: Uuid) -> Option<&BracketOrder> {
        let bracket_id = self.trigger_to_bracket.get(&trigger_order_id)?;
        self.brackets.get(bracket_id)
    }

    /// Total number of tracked brackets (all statuses).
    pub fn bracket_count(&self) -> usize {
        self.brackets.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn btc() -> Symbol {
        Symbol::new("BTCUSDT")
    }

    fn tp_spec() -> TriggerOrderSpec {
        TriggerOrderSpec {
            trigger_price: dec!(52000),
            order_price: Some(dec!(51950)),
            qty: dec!(1),
        }
    }

    fn sl_spec() -> TriggerOrderSpec {
        TriggerOrderSpec {
            trigger_price: dec!(48000),
            order_price: None,
            qty: dec!(1),
        }
    }

    fn manager() -> BracketManager {
        BracketManager::new()
    }

    // ------------------------------------------------------------------
    // Creation
    // ------------------------------------------------------------------

    #[test]
    fn create_bracket_returns_pending_order() {
        let mut mgr = manager();
        let entry = Uuid::new_v4();

        let bracket = mgr.create_bracket(
            Uuid::new_v4(),
            btc(),
            entry,
            Some(tp_spec()),
            Some(sl_spec()),
        );

        assert_eq!(bracket.status, BracketStatus::Pending);
        assert_eq!(bracket.entry_order_id, entry);
        assert!(bracket.tp_trigger.is_some());
        assert!(bracket.sl_trigger.is_some());
    }

    #[test]
    fn bracket_for_entry_lookup_works() {
        let mut mgr = manager();
        let entry = Uuid::new_v4();
        let bracket = mgr.create_bracket(Uuid::new_v4(), btc(), entry, None, None);

        let found = mgr.bracket_for_entry(entry).expect("must exist");
        assert_eq!(found.id, bracket.id);
    }

    // ------------------------------------------------------------------
    // Entry filled activates TP + SL
    // ------------------------------------------------------------------

    #[test]
    fn entry_filled_returns_specs_and_marks_active() {
        let mut mgr = manager();
        let entry = Uuid::new_v4();
        let bracket = mgr.create_bracket(
            Uuid::new_v4(),
            btc(),
            entry,
            Some(tp_spec()),
            Some(sl_spec()),
        );

        let result = mgr.on_entry_filled(entry);
        assert!(result.is_some());

        let (tp, sl) = result.unwrap();
        assert!(tp.is_some());
        assert!(sl.is_some());
        assert_eq!(
            mgr.bracket(bracket.id).unwrap().status,
            BracketStatus::Active
        );
    }

    #[test]
    fn entry_filled_for_unknown_order_returns_none() {
        let mut mgr = manager();
        assert!(mgr.on_entry_filled(Uuid::new_v4()).is_none());
    }

    #[test]
    fn entry_filled_twice_second_is_ignored() {
        let mut mgr = manager();
        let entry = Uuid::new_v4();
        mgr.create_bracket(Uuid::new_v4(), btc(), entry, Some(tp_spec()), Some(sl_spec()));

        mgr.on_entry_filled(entry);
        // Second fill — bracket is Active, not Pending.
        let result = mgr.on_entry_filled(entry);
        assert!(result.is_none());
    }

    // ------------------------------------------------------------------
    // TP filled cancels SL
    // ------------------------------------------------------------------

    #[test]
    fn tp_filled_returns_sl_trigger_id() {
        let mut mgr = manager();
        let entry = Uuid::new_v4();
        let bracket = mgr.create_bracket(
            Uuid::new_v4(),
            btc(),
            entry,
            Some(tp_spec()),
            Some(sl_spec()),
        );

        mgr.on_entry_filled(entry);

        let tp_id = Uuid::new_v4();
        let sl_id = Uuid::new_v4();
        mgr.record_trigger_ids(bracket.id, Some(tp_id), Some(sl_id));

        let cancelled = mgr.on_tp_filled(bracket.id);
        assert_eq!(cancelled, Some(sl_id));
        assert_eq!(
            mgr.bracket(bracket.id).unwrap().status,
            BracketStatus::TpFilled
        );
    }

    #[test]
    fn tp_filled_on_non_active_bracket_returns_none() {
        let mut mgr = manager();
        let entry = Uuid::new_v4();
        let bracket = mgr.create_bracket(Uuid::new_v4(), btc(), entry, Some(tp_spec()), None);

        // Bracket is still Pending — not Active.
        let result = mgr.on_tp_filled(bracket.id);
        assert!(result.is_none());
    }

    // ------------------------------------------------------------------
    // SL filled cancels TP
    // ------------------------------------------------------------------

    #[test]
    fn sl_filled_returns_tp_trigger_id() {
        let mut mgr = manager();
        let entry = Uuid::new_v4();
        let bracket = mgr.create_bracket(
            Uuid::new_v4(),
            btc(),
            entry,
            Some(tp_spec()),
            Some(sl_spec()),
        );

        mgr.on_entry_filled(entry);

        let tp_id = Uuid::new_v4();
        let sl_id = Uuid::new_v4();
        mgr.record_trigger_ids(bracket.id, Some(tp_id), Some(sl_id));

        let cancelled = mgr.on_sl_filled(bracket.id);
        assert_eq!(cancelled, Some(tp_id));
        assert_eq!(
            mgr.bracket(bracket.id).unwrap().status,
            BracketStatus::SlFilled
        );
    }

    #[test]
    fn sl_filled_on_non_active_bracket_returns_none() {
        let mut mgr = manager();
        let entry = Uuid::new_v4();
        let bracket = mgr.create_bracket(Uuid::new_v4(), btc(), entry, None, Some(sl_spec()));

        let result = mgr.on_sl_filled(bracket.id);
        assert!(result.is_none());
    }

    // ------------------------------------------------------------------
    // Cancel bracket
    // ------------------------------------------------------------------

    #[test]
    fn cancel_pending_bracket_returns_entry_and_no_trigger_ids() {
        let mut mgr = manager();
        let entry = Uuid::new_v4();
        let bracket =
            mgr.create_bracket(Uuid::new_v4(), btc(), entry, Some(tp_spec()), Some(sl_spec()));

        let result = mgr.cancel_bracket(bracket.id);
        assert!(result.is_some());

        let (returned_entry, (tp_id, sl_id)) = result.unwrap();
        assert_eq!(returned_entry, entry);
        // No trigger ids yet (entry hasn't filled).
        assert!(tp_id.is_none());
        assert!(sl_id.is_none());
        assert_eq!(
            mgr.bracket(bracket.id).unwrap().status,
            BracketStatus::Cancelled
        );
    }

    #[test]
    fn cancel_active_bracket_returns_trigger_ids() {
        let mut mgr = manager();
        let entry = Uuid::new_v4();
        let bracket = mgr.create_bracket(
            Uuid::new_v4(),
            btc(),
            entry,
            Some(tp_spec()),
            Some(sl_spec()),
        );

        mgr.on_entry_filled(entry);
        let tp_id = Uuid::new_v4();
        let sl_id = Uuid::new_v4();
        mgr.record_trigger_ids(bracket.id, Some(tp_id), Some(sl_id));

        let result = mgr.cancel_bracket(bracket.id);
        assert!(result.is_some());

        let (_, (returned_tp, returned_sl)) = result.unwrap();
        assert_eq!(returned_tp, Some(tp_id));
        assert_eq!(returned_sl, Some(sl_id));
    }

    #[test]
    fn cancel_terminal_bracket_returns_none() {
        let mut mgr = manager();
        let entry = Uuid::new_v4();
        let bracket =
            mgr.create_bracket(Uuid::new_v4(), btc(), entry, Some(tp_spec()), Some(sl_spec()));

        mgr.cancel_bracket(bracket.id);
        // Already Cancelled — second cancel returns None.
        assert!(mgr.cancel_bracket(bracket.id).is_none());
    }

    #[test]
    fn cancel_nonexistent_bracket_returns_none() {
        let mut mgr = manager();
        assert!(mgr.cancel_bracket(Uuid::new_v4()).is_none());
    }

    // ------------------------------------------------------------------
    // Reverse index: bracket_for_trigger
    // ------------------------------------------------------------------

    #[test]
    fn bracket_for_trigger_resolves_correctly() {
        let mut mgr = manager();
        let entry = Uuid::new_v4();
        let bracket = mgr.create_bracket(
            Uuid::new_v4(),
            btc(),
            entry,
            Some(tp_spec()),
            Some(sl_spec()),
        );

        mgr.on_entry_filled(entry);
        let tp_id = Uuid::new_v4();
        let sl_id = Uuid::new_v4();
        mgr.record_trigger_ids(bracket.id, Some(tp_id), Some(sl_id));

        let found_by_tp = mgr.bracket_for_trigger(tp_id).expect("must exist");
        assert_eq!(found_by_tp.id, bracket.id);

        let found_by_sl = mgr.bracket_for_trigger(sl_id).expect("must exist");
        assert_eq!(found_by_sl.id, bracket.id);
    }

    // ------------------------------------------------------------------
    // Bracket without TP or SL
    // ------------------------------------------------------------------

    #[test]
    fn bracket_with_no_tp_sl_still_activates() {
        let mut mgr = manager();
        let entry = Uuid::new_v4();
        let bracket = mgr.create_bracket(Uuid::new_v4(), btc(), entry, None, None);

        let result = mgr.on_entry_filled(entry).unwrap();
        assert!(result.0.is_none());
        assert!(result.1.is_none());
        assert_eq!(
            mgr.bracket(bracket.id).unwrap().status,
            BracketStatus::Active
        );
    }
}
