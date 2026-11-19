//! Conditional order chains: execute a sequence of orders where each order is
//! placed only after the previous one fully fills.
//!
//! A chain is created via `POST /orders/chain` with an array of order specs.
//! The first order is submitted to the matching engine immediately.  Each
//! subsequent order sits dormant until the exchange signals that the previous
//! order has been fully filled (via `on_order_filled`).
//!
//! The manager is single-threaded by design — callers hold a `Mutex` around it.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use exchange_common::{OrderType, Price, Qty, Side, Symbol, TimeInForce, UserId};

/// Specification for a single step in a chain order.
///
/// Fields mirror `PlaceOrderCommand` but without system-assigned ids or
/// timestamps, which the exchange fills in at submission time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainStep {
    pub symbol: Symbol,
    pub side: Side,
    pub order_type: OrderType,
    pub time_in_force: TimeInForce,
    /// Limit price. Zero / ignored for market orders.
    pub price: Price,
    pub qty: Qty,
    /// Optional reduce-only flag.
    #[serde(default)]
    pub reduce_only: bool,
    /// Optional position index for hedge mode.
    #[serde(default)]
    pub position_idx: u8,
    /// Optional user-provided client order ID.
    #[serde(default)]
    pub order_link_id: Option<String>,
    /// Optional slippage tolerance for market orders.
    #[serde(default)]
    pub slippage_tolerance: Option<Decimal>,
}

/// Lifecycle state of a chain order group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChainStatus {
    /// Some steps are still pending; the active step is running.
    Active,
    /// All steps have been submitted and filled.
    Completed,
    /// The chain was cancelled by the user (or a step was rejected).
    Cancelled,
}

/// A chain order: an ordered list of steps executed sequentially.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainOrder {
    pub id: Uuid,
    pub user_id: UserId,
    /// Ordered list of steps.  Step 0 is submitted immediately on creation.
    pub steps: Vec<ChainStep>,
    /// Index of the step currently running (submitted to the matching engine).
    pub current_step: usize,
    /// Matching-engine order ID assigned to the currently running step.
    pub active_order_id: Option<Uuid>,
    pub status: ChainStatus,
    pub created_at: DateTime<Utc>,
}

impl ChainOrder {
    /// Returns `true` if there is a next step waiting to be submitted.
    pub fn has_next_step(&self) -> bool {
        self.current_step + 1 < self.steps.len()
    }

    /// Returns the spec for the next step, if any.
    pub fn next_step(&self) -> Option<&ChainStep> {
        self.steps.get(self.current_step + 1)
    }
}

/// Manages all active chain orders.
///
/// Index layout:
///   - `chains`             — chain_id → ChainOrder
///   - `order_to_chain`     — matching-engine order_id → chain_id
///
/// Single-threaded by design.
#[derive(Debug, Default)]
pub struct ChainManager {
    chains: HashMap<Uuid, ChainOrder>,
    /// Maps the matching-engine order ID of the *active* step → its chain id.
    order_to_chain: HashMap<Uuid, Uuid>,
}

impl ChainManager {
    pub fn new() -> Self {
        Self::default()
    }

    // ------------------------------------------------------------------
    // Chain management
    // ------------------------------------------------------------------

    /// Register a new chain order.
    ///
    /// `initial_order_id` is the matching-engine order ID already assigned to
    /// the first step by the caller.
    ///
    /// Returns the created `ChainOrder`.  The caller must have already
    /// submitted step 0 to the matching engine before calling this.
    ///
    /// Returns `None` if `steps` is empty.
    pub fn create_chain(
        &mut self,
        user_id: UserId,
        steps: Vec<ChainStep>,
        initial_order_id: Uuid,
    ) -> Option<ChainOrder> {
        if steps.is_empty() {
            return None;
        }

        let chain = ChainOrder {
            id: Uuid::new_v4(),
            user_id,
            steps,
            current_step: 0,
            active_order_id: Some(initial_order_id),
            status: ChainStatus::Active,
            created_at: Utc::now(),
        };

        self.order_to_chain.insert(initial_order_id, chain.id);
        self.chains.insert(chain.id, chain.clone());

        Some(chain)
    }

    /// Notify that the active matching-engine order has been fully filled.
    ///
    /// Returns the next `ChainStep` to submit (along with the chain id), or
    /// `None` if:
    /// - The order belongs to no chain.
    /// - The chain is not `Active`.
    /// - All steps are already done (chain transitions to `Completed`).
    ///
    /// When a next step is returned the caller must:
    /// 1. Submit it to the matching engine to get a new `order_id`.
    /// 2. Call `record_active_order` with the new `order_id`.
    pub fn on_order_filled(&mut self, order_id: Uuid) -> Option<(Uuid, ChainStep)> {
        let chain_id = self.order_to_chain.remove(&order_id)?;
        let chain = self.chains.get_mut(&chain_id)?;

        if chain.status != ChainStatus::Active {
            return None;
        }

        chain.active_order_id = None;

        if chain.has_next_step() {
            // Advance to the next step and return it for submission.
            chain.current_step += 1;
            let step = chain.next_step_spec();
            Some((chain_id, step))
        } else {
            // All steps done.
            chain.status = ChainStatus::Completed;
            None
        }
    }

    /// Record the matching-engine order ID assigned to the newly submitted step.
    ///
    /// Call this immediately after `on_order_filled` returns a next step and
    /// you have submitted it to the matching engine.
    pub fn record_active_order(&mut self, chain_id: Uuid, order_id: Uuid) {
        if let Some(chain) = self.chains.get_mut(&chain_id) {
            chain.active_order_id = Some(order_id);
            self.order_to_chain.insert(order_id, chain_id);
        }
    }

    /// Cancel a chain order.
    ///
    /// Returns the matching-engine order ID of the currently active step (which
    /// the caller must cancel in the matching engine), or `None` if the chain
    /// does not exist, is already terminal, or has no active step.
    pub fn cancel_chain(&mut self, chain_id: Uuid) -> Option<Uuid> {
        let chain = self.chains.get_mut(&chain_id)?;

        if chain.status != ChainStatus::Active {
            return None;
        }

        let active_order_id = chain.active_order_id;
        chain.status = ChainStatus::Cancelled;

        if let Some(oid) = active_order_id {
            self.order_to_chain.remove(&oid);
        }

        active_order_id
    }

    // ------------------------------------------------------------------
    // Queries
    // ------------------------------------------------------------------

    /// Look up a chain by its id.
    pub fn chain(&self, chain_id: Uuid) -> Option<&ChainOrder> {
        self.chains.get(&chain_id)
    }

    /// All chains (all statuses) belonging to `user_id`.
    pub fn user_chains(&self, user_id: UserId) -> Vec<&ChainOrder> {
        self.chains
            .values()
            .filter(|c| c.user_id == user_id)
            .collect()
    }

    /// Total number of tracked chains.
    pub fn chain_count(&self) -> usize {
        self.chains.len()
    }
}

// Private helper on ChainOrder.
impl ChainOrder {
    fn next_step_spec(&self) -> ChainStep {
        self.steps[self.current_step].clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn btc() -> Symbol {
        Symbol::new("BTCUSDT")
    }

    fn limit_step(price: Decimal, qty: Decimal) -> ChainStep {
        ChainStep {
            symbol: btc(),
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
            price,
            qty,
            reduce_only: false,
            position_idx: 0,
            order_link_id: None,
            slippage_tolerance: None,
        }
    }

    fn manager() -> ChainManager {
        ChainManager::new()
    }

    // ------------------------------------------------------------------
    // Creation
    // ------------------------------------------------------------------

    #[test]
    fn create_chain_returns_active() {
        let mut mgr = manager();
        let initial_oid = Uuid::new_v4();
        let steps = vec![limit_step(dec!(100), dec!(1)), limit_step(dec!(200), dec!(1))];
        let chain = mgr.create_chain(Uuid::new_v4(), steps, initial_oid).unwrap();

        assert_eq!(chain.status, ChainStatus::Active);
        assert_eq!(chain.current_step, 0);
        assert_eq!(chain.active_order_id, Some(initial_oid));
    }

    #[test]
    fn empty_steps_returns_none() {
        let mut mgr = manager();
        assert!(mgr.create_chain(Uuid::new_v4(), vec![], Uuid::new_v4()).is_none());
    }

    #[test]
    fn chain_queryable_after_creation() {
        let mut mgr = manager();
        let initial_oid = Uuid::new_v4();
        let chain = mgr
            .create_chain(Uuid::new_v4(), vec![limit_step(dec!(100), dec!(1))], initial_oid)
            .unwrap();

        assert!(mgr.chain(chain.id).is_some());
    }

    // ------------------------------------------------------------------
    // Single-step chain completes on fill
    // ------------------------------------------------------------------

    #[test]
    fn single_step_chain_completes_on_fill() {
        let mut mgr = manager();
        let oid = Uuid::new_v4();
        let chain = mgr
            .create_chain(Uuid::new_v4(), vec![limit_step(dec!(100), dec!(1))], oid)
            .unwrap();

        let result = mgr.on_order_filled(oid);
        assert!(result.is_none()); // no next step
        assert_eq!(mgr.chain(chain.id).unwrap().status, ChainStatus::Completed);
    }

    // ------------------------------------------------------------------
    // Multi-step: fill advances to next step
    // ------------------------------------------------------------------

    #[test]
    fn fill_returns_next_step_spec() {
        let mut mgr = manager();
        let oid1 = Uuid::new_v4();
        let steps = vec![
            limit_step(dec!(100), dec!(1)),
            limit_step(dec!(200), dec!(2)),
        ];
        let chain = mgr.create_chain(Uuid::new_v4(), steps, oid1).unwrap();

        let result = mgr.on_order_filled(oid1);
        assert!(result.is_some());
        let (chain_id, step) = result.unwrap();
        assert_eq!(chain_id, chain.id);
        assert_eq!(step.price, dec!(200));
        assert_eq!(step.qty, dec!(2));
    }

    #[test]
    fn fill_advances_current_step() {
        let mut mgr = manager();
        let oid1 = Uuid::new_v4();
        let chain = mgr
            .create_chain(
                Uuid::new_v4(),
                vec![limit_step(dec!(100), dec!(1)), limit_step(dec!(200), dec!(1))],
                oid1,
            )
            .unwrap();

        mgr.on_order_filled(oid1);
        assert_eq!(mgr.chain(chain.id).unwrap().current_step, 1);
    }

    #[test]
    fn record_active_order_enables_subsequent_fill() {
        let mut mgr = manager();
        let oid1 = Uuid::new_v4();
        let chain = mgr
            .create_chain(
                Uuid::new_v4(),
                vec![
                    limit_step(dec!(100), dec!(1)),
                    limit_step(dec!(200), dec!(1)),
                    limit_step(dec!(300), dec!(1)),
                ],
                oid1,
            )
            .unwrap();

        let (chain_id, _step) = mgr.on_order_filled(oid1).unwrap();
        let oid2 = Uuid::new_v4();
        mgr.record_active_order(chain_id, oid2);

        // Second fill should return the third step.
        let result = mgr.on_order_filled(oid2);
        assert!(result.is_some());
        let (_, step) = result.unwrap();
        assert_eq!(step.price, dec!(300));
        // After third step submitted, chain should still be Active.
        assert_eq!(mgr.chain(chain.id).unwrap().status, ChainStatus::Active);
    }

    #[test]
    fn three_step_chain_completes_after_last_fill() {
        let mut mgr = manager();
        let oid1 = Uuid::new_v4();
        let chain = mgr
            .create_chain(
                Uuid::new_v4(),
                vec![
                    limit_step(dec!(100), dec!(1)),
                    limit_step(dec!(200), dec!(1)),
                ],
                oid1,
            )
            .unwrap();

        let (chain_id, _) = mgr.on_order_filled(oid1).unwrap();
        let oid2 = Uuid::new_v4();
        mgr.record_active_order(chain_id, oid2);

        // Filling the second (last) step → Completed.
        let result = mgr.on_order_filled(oid2);
        assert!(result.is_none());
        assert_eq!(mgr.chain(chain.id).unwrap().status, ChainStatus::Completed);
    }

    // ------------------------------------------------------------------
    // Cancellation
    // ------------------------------------------------------------------

    #[test]
    fn cancel_returns_active_order_id() {
        let mut mgr = manager();
        let oid = Uuid::new_v4();
        let chain = mgr
            .create_chain(
                Uuid::new_v4(),
                vec![limit_step(dec!(100), dec!(1)), limit_step(dec!(200), dec!(1))],
                oid,
            )
            .unwrap();

        let returned = mgr.cancel_chain(chain.id);
        assert_eq!(returned, Some(oid));
        assert_eq!(mgr.chain(chain.id).unwrap().status, ChainStatus::Cancelled);
    }

    #[test]
    fn cancel_terminal_chain_returns_none() {
        let mut mgr = manager();
        let oid = Uuid::new_v4();
        let chain = mgr
            .create_chain(Uuid::new_v4(), vec![limit_step(dec!(100), dec!(1))], oid)
            .unwrap();

        mgr.cancel_chain(chain.id);
        assert!(mgr.cancel_chain(chain.id).is_none());
    }

    #[test]
    fn cancel_unknown_chain_returns_none() {
        let mut mgr = manager();
        assert!(mgr.cancel_chain(Uuid::new_v4()).is_none());
    }

    // ------------------------------------------------------------------
    // Unknown order fill is ignored
    // ------------------------------------------------------------------

    #[test]
    fn fill_unknown_order_returns_none() {
        let mut mgr = manager();
        assert!(mgr.on_order_filled(Uuid::new_v4()).is_none());
    }

    // ------------------------------------------------------------------
    // User query
    // ------------------------------------------------------------------

    #[test]
    fn user_chains_returns_only_that_users_chains() {
        let mut mgr = manager();
        let user_a = Uuid::new_v4();
        let user_b = Uuid::new_v4();

        mgr.create_chain(user_a, vec![limit_step(dec!(100), dec!(1))], Uuid::new_v4());
        mgr.create_chain(user_a, vec![limit_step(dec!(200), dec!(1))], Uuid::new_v4());
        mgr.create_chain(user_b, vec![limit_step(dec!(300), dec!(1))], Uuid::new_v4());

        assert_eq!(mgr.user_chains(user_a).len(), 2);
        assert_eq!(mgr.user_chains(user_b).len(), 1);
        assert_eq!(mgr.user_chains(Uuid::new_v4()).len(), 0);
    }
}
