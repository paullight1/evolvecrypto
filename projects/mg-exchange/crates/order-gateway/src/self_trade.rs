use std::collections::{HashMap, HashSet};

use exchange_common::*;

/// Policy for handling self-trades (same user on both sides).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfTradePolicy {
    /// Cancel the incoming (taker) order. Maker stays on book.
    CancelTaker,
    /// Cancel the resting (maker) order. Taker proceeds.
    CancelMaker,
    /// Allow self-trade (no prevention). Used in Phase 1.
    Allow,
}

/// Tracks which users have resting orders to detect potential self-trades.
///
/// In gitbitex, there is NO self-trade prevention. Production exchanges
/// typically implement cancel-newest (CancelTaker) by default.
pub struct SelfTradeGuard {
    policy: SelfTradePolicy,
    /// user_id → set of order_ids currently on the book
    user_orders: HashMap<UserId, HashSet<OrderId>>,
    /// order_id → (user_id, symbol, side)
    order_info: HashMap<OrderId, (UserId, Symbol, Side)>,
}

impl SelfTradeGuard {
    pub fn new(policy: SelfTradePolicy) -> Self {
        Self {
            policy,
            user_orders: HashMap::new(),
            order_info: HashMap::new(),
        }
    }

    /// Register an order that was accepted onto the book.
    pub fn on_order_accepted(&mut self, order_id: OrderId, user_id: UserId, symbol: Symbol, side: Side) {
        self.user_orders
            .entry(user_id)
            .or_default()
            .insert(order_id);
        self.order_info.insert(order_id, (user_id, symbol, side));
    }

    /// Remove an order (filled or cancelled).
    pub fn on_order_removed(&mut self, order_id: &OrderId) {
        if let Some((user_id, _, _)) = self.order_info.remove(order_id) {
            if let Some(orders) = self.user_orders.get_mut(&user_id) {
                orders.remove(order_id);
                if orders.is_empty() {
                    self.user_orders.remove(&user_id);
                }
            }
        }
    }

    /// Check if an incoming order would self-trade.
    /// Returns true if the order should be REJECTED (when policy is CancelTaker).
    ///
    /// The caller should check this BEFORE sending to matching engine.
    pub fn would_self_trade(&self, cmd: &PlaceOrderCommand) -> bool {
        if self.policy == SelfTradePolicy::Allow {
            return false;
        }

        // Check if this user has any resting orders on the opposite side
        if let Some(user_order_ids) = self.user_orders.get(&cmd.user_id) {
            for oid in user_order_ids {
                if let Some((_, symbol, side)) = self.order_info.get(oid) {
                    if *symbol == cmd.symbol && *side == cmd.side.opposite() {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Get the policy.
    pub fn policy(&self) -> SelfTradePolicy {
        self.policy
    }

    /// Get order IDs that would cause a self-trade (for CancelMaker policy).
    pub fn conflicting_orders(&self, cmd: &PlaceOrderCommand) -> Vec<OrderId> {
        let mut conflicts = Vec::new();

        if let Some(user_order_ids) = self.user_orders.get(&cmd.user_id) {
            for oid in user_order_ids {
                if let Some((_, symbol, side)) = self.order_info.get(oid) {
                    if *symbol == cmd.symbol && *side == cmd.side.opposite() {
                        conflicts.push(*oid);
                    }
                }
            }
        }

        conflicts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn make_cmd(user_id: UserId, side: Side) -> PlaceOrderCommand {
        PlaceOrderCommand {
            order_id: Uuid::new_v4(),
            user_id,
            symbol: Symbol::new("BTCUSDT"),
            side,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
            price: dec!(50000),
            qty: dec!(1),
            timestamp: chrono::Utc::now(),
            reduce_only: false,
            expire_at: None,
            position_idx: 0,
            iceberg_qty: None,
            order_link_id: None,
            slippage_tolerance: None,
            close_position: false,
            stp_mode: StpMode::default(),
            request_expire_time: None,
        }
    }

    #[test]
    fn test_allow_policy_permits_self_trade() {
        let mut guard = SelfTradeGuard::new(SelfTradePolicy::Allow);
        let user = Uuid::new_v4();

        // User has resting sell
        let sell = make_cmd(user, Side::Sell);
        guard.on_order_accepted(sell.order_id, user, sell.symbol.clone(), sell.side);

        // User places buy — should NOT be flagged
        let buy = make_cmd(user, Side::Buy);
        assert!(!guard.would_self_trade(&buy));
    }

    #[test]
    fn test_cancel_taker_detects_self_trade() {
        let mut guard = SelfTradeGuard::new(SelfTradePolicy::CancelTaker);
        let user = Uuid::new_v4();

        // User has resting sell
        let sell = make_cmd(user, Side::Sell);
        guard.on_order_accepted(sell.order_id, user, sell.symbol.clone(), sell.side);

        // User places buy — would self-trade
        let buy = make_cmd(user, Side::Buy);
        assert!(guard.would_self_trade(&buy));
    }

    #[test]
    fn test_different_users_no_self_trade() {
        let mut guard = SelfTradeGuard::new(SelfTradePolicy::CancelTaker);
        let alice = Uuid::new_v4();
        let bob = Uuid::new_v4();

        // Alice has resting sell
        let sell = make_cmd(alice, Side::Sell);
        guard.on_order_accepted(sell.order_id, alice, sell.symbol.clone(), sell.side);

        // Bob places buy — NOT self-trade
        let buy = make_cmd(bob, Side::Buy);
        assert!(!guard.would_self_trade(&buy));
    }

    #[test]
    fn test_same_side_no_self_trade() {
        let mut guard = SelfTradeGuard::new(SelfTradePolicy::CancelTaker);
        let user = Uuid::new_v4();

        // User has resting buy
        let buy1 = make_cmd(user, Side::Buy);
        guard.on_order_accepted(buy1.order_id, user, buy1.symbol.clone(), buy1.side);

        // User places another buy — same side, NOT self-trade
        let buy2 = make_cmd(user, Side::Buy);
        assert!(!guard.would_self_trade(&buy2));
    }

    #[test]
    fn test_different_symbol_no_self_trade() {
        let mut guard = SelfTradeGuard::new(SelfTradePolicy::CancelTaker);
        let user = Uuid::new_v4();

        // User has resting sell on BTCUSDT
        let sell = make_cmd(user, Side::Sell);
        guard.on_order_accepted(sell.order_id, user, sell.symbol.clone(), sell.side);

        // User places buy on ETHUSDT — different symbol, NOT self-trade
        let mut buy = make_cmd(user, Side::Buy);
        buy.symbol = Symbol::new("ETHUSDT");
        assert!(!guard.would_self_trade(&buy));
    }

    #[test]
    fn test_removed_order_no_longer_conflicts() {
        let mut guard = SelfTradeGuard::new(SelfTradePolicy::CancelTaker);
        let user = Uuid::new_v4();

        // User has resting sell
        let sell = make_cmd(user, Side::Sell);
        let sell_id = sell.order_id;
        guard.on_order_accepted(sell.order_id, user, sell.symbol.clone(), sell.side);

        // Remove it (filled/cancelled)
        guard.on_order_removed(&sell_id);

        // Now buy should NOT self-trade
        let buy = make_cmd(user, Side::Buy);
        assert!(!guard.would_self_trade(&buy));
    }

    #[test]
    fn test_conflicting_orders_for_cancel_maker() {
        let mut guard = SelfTradeGuard::new(SelfTradePolicy::CancelMaker);
        let user = Uuid::new_v4();

        // User has 2 resting sells
        let sell1 = make_cmd(user, Side::Sell);
        let sell2 = make_cmd(user, Side::Sell);
        let sell1_id = sell1.order_id;
        let sell2_id = sell2.order_id;
        guard.on_order_accepted(sell1.order_id, user, sell1.symbol.clone(), sell1.side);
        guard.on_order_accepted(sell2.order_id, user, sell2.symbol.clone(), sell2.side);

        // User places buy — should find both conflicts
        let buy = make_cmd(user, Side::Buy);
        let conflicts = guard.conflicting_orders(&buy);
        assert_eq!(conflicts.len(), 2);
        assert!(conflicts.contains(&sell1_id));
        assert!(conflicts.contains(&sell2_id));
    }
}
