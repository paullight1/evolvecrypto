use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use tracing::{debug, info, warn};
use uuid::Uuid;

use exchange_common::{
    Command, OrderId, OrderType, PlaceOrderCommand, Price, Side, Symbol, TimeInForce, UserId,
};

use crate::error::TriggerError;
use crate::trailing::TrailingStopTracker;
use crate::trigger_order::{
    TpSlOrderType, TriggerBy, TriggerOrder, TriggerOrderType, TriggerStatus,
};

/// The outcome of a single order that fired during `check_triggers`.
#[derive(Debug)]
pub struct TriggeredResult {
    /// The trigger order with `status` updated to `Triggered`.
    pub trigger_order: TriggerOrder,
    /// The command to forward to the matching engine.
    pub command: Command,
}

/// Configuration for the trigger engine.
#[derive(Debug, Clone)]
pub struct TriggerEngineConfig {
    /// Maximum number of orders converted to matching-engine commands in a
    /// single `check_triggers` call per symbol.  Prevents cascade storms
    /// during mass-liquidation events from flooding the command queue.
    pub max_triggers_per_tick: usize,
}

impl Default for TriggerEngineConfig {
    fn default() -> Self {
        Self {
            max_triggers_per_tick: 50,
        }
    }
}

/// Holds all active trigger orders and evaluates them against incoming price
/// ticks.
///
/// Like the matching engine this struct is single-threaded by design.
/// Concurrency is achieved by sharding across instances (one per symbol shard)
/// rather than internal locking.
pub struct TriggerEngine {
    /// Active orders indexed by symbol for O(symbols) scan per tick.
    orders: HashMap<Symbol, Vec<TriggerOrder>>,
    /// All orders by id for O(1) cancel lookup.
    index: HashMap<OrderId, Symbol>,
    /// Trailing-stop peak/trough tracker.
    trailing: TrailingStopTracker,
    config: TriggerEngineConfig,
}

impl TriggerEngine {
    pub fn new(config: TriggerEngineConfig) -> Self {
        Self {
            orders: HashMap::new(),
            index: HashMap::new(),
            trailing: TrailingStopTracker::new(),
            config,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(TriggerEngineConfig::default())
    }

    // ------------------------------------------------------------------
    // Order management
    // ------------------------------------------------------------------

    /// Add a trigger order to the book.
    ///
    /// Returns `Err` if the order is already known (duplicate id).
    pub fn add_order(&mut self, order: TriggerOrder) -> Result<(), TriggerError> {
        if self.index.contains_key(&order.id) {
            return Err(TriggerError::DuplicateOrderId(order.id));
        }
        if !order.is_active() {
            return Err(TriggerError::OrderNotActive(order.id));
        }
        if order.trigger_type == TriggerOrderType::TrailingStop {
            validate_trailing(&order)?;
        }

        debug!(
            order_id = %order.id,
            symbol = %order.symbol,
            trigger_type = ?order.trigger_type,
            trigger_price = %order.trigger_price,
            "trigger order added"
        );

        self.index.insert(order.id, order.symbol.clone());

        // Register trailing tracker with an initial extreme equal to trigger_price
        // so that updates from the very first tick move the extreme correctly.
        if order.trigger_type == TriggerOrderType::TrailingStop {
            self.trailing
                .register_typed(order.id, order.side, order.trigger_price, order.trail_type);
        }

        self.orders
            .entry(order.symbol.clone())
            .or_default()
            .push(order);

        Ok(())
    }

    /// Cancel a trigger order by id.
    ///
    /// Returns the cancelled order or `Err` if not found / already terminal.
    pub fn cancel_order(&mut self, order_id: OrderId) -> Result<TriggerOrder, TriggerError> {
        let symbol = self
            .index
            .get(&order_id)
            .cloned()
            .ok_or(TriggerError::OrderNotFound(order_id))?;

        let orders = self.orders.entry(symbol).or_default();
        let pos = orders
            .iter()
            .position(|o| o.id == order_id)
            .ok_or(TriggerError::OrderNotFound(order_id))?;

        let order = &orders[pos];
        if !order.is_active() {
            return Err(TriggerError::OrderNotActive(order_id));
        }

        let mut cancelled = orders.remove(pos);
        cancelled.status = TriggerStatus::Cancelled;
        self.index.remove(&order_id);
        self.trailing.remove(order_id);

        info!(order_id = %order_id, "trigger order cancelled");
        Ok(cancelled)
    }

    // ------------------------------------------------------------------
    // Price evaluation
    // ------------------------------------------------------------------

    /// Evaluate all active orders for `symbol` against the given prices.
    ///
    /// Returns at most `config.max_triggers_per_tick` results.
    /// Triggered orders are removed from the book and their status is set to
    /// `Triggered`.
    pub fn check_triggers(
        &mut self,
        symbol: &Symbol,
        last_price: Price,
        mark_price: Price,
        index_price: Price,
        now: DateTime<Utc>,
    ) -> Vec<TriggeredResult> {
        let orders = match self.orders.get_mut(symbol) {
            None => return vec![],
            Some(v) => v,
        };

        // First update all trailing extremes for active trailing orders.
        for order in orders.iter() {
            if order.is_active() && order.trigger_type == TriggerOrderType::TrailingStop {
                let ref_price = select_price(order.trigger_by, last_price, mark_price, index_price);
                self.trailing.update_price(order.id, ref_price);
            }
        }

        let max = self.config.max_triggers_per_tick;

        // Collect ids of orders that should fire this tick.
        // We avoid holding a borrow on `self.orders` while also needing to
        // call `self.trailing.should_trigger`, so we snapshot the ids first.
        let mut fired_ids = Vec::new();
        {
            let mut pacing_warned = false;
            for order in orders.iter() {
                if fired_ids.len() >= max {
                    if !pacing_warned {
                        warn!(
                            symbol = %symbol,
                            "max_triggers_per_tick reached — remaining orders deferred to next tick"
                        );
                        pacing_warned = true;
                    }
                    break;
                }
                if !order.is_active() {
                    continue;
                }
                let ref_price =
                    select_price(order.trigger_by, last_price, mark_price, index_price);
                if condition_met(order, ref_price, &self.trailing) {
                    fired_ids.push(order.id);
                }
            }
        }

        // Remove fired orders and build results.  Re-borrow `orders` mutably.
        let orders = self.orders.entry(symbol.clone()).or_default();
        let mut results = Vec::new();
        for order_id in fired_ids {
            if let Some(pos) = orders.iter().position(|o| o.id == order_id) {
                let mut order = orders.remove(pos);
                order.status = TriggerStatus::Triggered;
                order.triggered_at = Some(now);
                self.index.remove(&order.id);
                self.trailing.remove(order.id);

                let command = build_command(&order, now);
                info!(
                    order_id = %order.id,
                    symbol = %order.symbol,
                    trigger_type = ?order.trigger_type,
                    "trigger order fired"
                );
                results.push(TriggeredResult {
                    trigger_order: order,
                    command,
                });
            }
        }

        results
    }

    /// Remove orders that have passed their `expires_at` timestamp.
    ///
    /// Returns the expired orders with their status set to `Expired`.
    pub fn expire_orders(&mut self, now: DateTime<Utc>) -> Vec<TriggerOrder> {
        let mut expired = Vec::new();

        for orders in self.orders.values_mut() {
            let mut i = 0;
            while i < orders.len() {
                let should_expire = orders[i]
                    .expires_at
                    .map(|exp| now >= exp && orders[i].is_active())
                    .unwrap_or(false);
                if should_expire {
                    let mut order = orders.remove(i);
                    order.status = TriggerStatus::Expired;
                    expired.push(order);
                    // don't advance i — next element shifted into position i
                } else {
                    i += 1;
                }
            }
        }

        for order in &expired {
            self.index.remove(&order.id);
            self.trailing.remove(order.id);
        }

        if !expired.is_empty() {
            info!(count = expired.len(), "trigger orders expired");
        }
        expired
    }

    // ------------------------------------------------------------------
    // Queries
    // ------------------------------------------------------------------

    /// All active orders belonging to `user_id`.
    pub fn active_orders(&self, user_id: UserId) -> Vec<&TriggerOrder> {
        self.orders
            .values()
            .flat_map(|v| v.iter())
            .filter(|o| o.user_id == user_id && o.is_active())
            .collect()
    }

    /// Count of active orders for `symbol`.
    pub fn active_orders_for_symbol(&self, symbol: &Symbol) -> usize {
        self.orders
            .get(symbol)
            .map(|v| v.iter().filter(|o| o.is_active()).count())
            .unwrap_or(0)
    }

    /// Total active orders across all symbols.
    pub fn total_active_orders(&self) -> usize {
        self.orders
            .values()
            .flat_map(|v| v.iter())
            .filter(|o| o.is_active())
            .count()
    }

    /// Cancel all active orders belonging to `user_id`, optionally filtered by
    /// `symbol` and/or `trigger_type`.
    ///
    /// Returns the list of cancelled orders with their status set to `Cancelled`.
    pub fn cancel_orders_for_user(
        &mut self,
        user_id: UserId,
        symbol: Option<&Symbol>,
        trigger_type: Option<TriggerOrderType>,
    ) -> Vec<TriggerOrder> {
        let mut cancelled = Vec::new();

        for orders in self.orders.values_mut() {
            let mut i = 0;
            while i < orders.len() {
                let matches = orders[i].is_active()
                    && orders[i].user_id == user_id
                    && symbol.map_or(true, |s| &orders[i].symbol == s)
                    && trigger_type.map_or(true, |t| orders[i].trigger_type == t);

                if matches {
                    let mut order = orders.remove(i);
                    order.status = TriggerStatus::Cancelled;
                    self.index.remove(&order.id);
                    self.trailing.remove(order.id);
                    cancelled.push(order);
                    // don't advance i — next element shifted into position i
                } else {
                    i += 1;
                }
            }
        }

        if !cancelled.is_empty() {
            info!(
                user_id = %user_id,
                count = cancelled.len(),
                "cancel_all trigger orders for user"
            );
        }
        cancelled
    }

}

// ------------------------------------------------------------------
// Free helpers
// ------------------------------------------------------------------

fn condition_met(
    order: &TriggerOrder,
    ref_price: Price,
    trailing: &TrailingStopTracker,
) -> bool {
    match order.trigger_type {
        TriggerOrderType::StopLimit | TriggerOrderType::StopMarket => match order.side {
            Side::Buy => ref_price >= order.trigger_price,
            Side::Sell => ref_price <= order.trigger_price,
        },
        TriggerOrderType::TakeProfit | TriggerOrderType::TakeProfitMarket => match order.side {
            Side::Buy => ref_price <= order.trigger_price,
            Side::Sell => ref_price >= order.trigger_price,
        },
        TriggerOrderType::TrailingStop => {
            let delta = match order.trailing_delta {
                Some(d) => d,
                None => return false,
            };
            trailing.should_trigger(order.id, ref_price, delta)
        }
    }
}

fn select_price(trigger_by: TriggerBy, last: Price, mark: Price, index: Price) -> Price {
    match trigger_by {
        TriggerBy::LastPrice => last,
        TriggerBy::MarkPrice => mark,
        TriggerBy::IndexPrice => index,
    }
}

fn build_command(order: &TriggerOrder, now: DateTime<Utc>) -> Command {
    // Determine whether the child order should be a limit or market order.
    //
    // Priority (highest to lowest):
    //  1. `tp_order_type` / `sl_order_type` explicitly set to `Limit` with a
    //     corresponding limit price.
    //  2. Legacy `order_price` field (stop-limit / take-profit with a price set
    //     at order creation time).
    //  3. Default: market order.
    let resolved = resolve_child_order_type(order);

    match resolved {
        (OrderType::Limit, Some(limit_price)) => Command::PlaceOrder(PlaceOrderCommand {
            order_id: Uuid::new_v4(), // new id for the child order
            user_id: order.user_id,
            symbol: order.symbol.clone(),
            side: order.side,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
            price: limit_price,
            qty: order.qty,
            reduce_only: order.reduce_only,
            expire_at: None,
            timestamp: now,
            position_idx: 0,
            iceberg_qty: None,
            order_link_id: None,
            slippage_tolerance: None,
            close_position: order.close_on_trigger,
            stp_mode: Default::default(),
            request_expire_time: None,
        }),
        _ => Command::PlaceOrder(PlaceOrderCommand {
            order_id: Uuid::new_v4(),
            user_id: order.user_id,
            symbol: order.symbol.clone(),
            side: order.side,
            order_type: OrderType::Market,
            time_in_force: TimeInForce::Ioc,
            price: Decimal::ZERO, // ignored for market orders
            qty: order.qty,
            reduce_only: order.reduce_only,
            expire_at: None,
            timestamp: now,
            position_idx: 0,
            iceberg_qty: None,
            order_link_id: None,
            slippage_tolerance: None,
            close_position: order.close_on_trigger,
            stp_mode: Default::default(),
            request_expire_time: None,
        }),
    }
}

/// Resolve the child order type and price for a trigger order.
///
/// Returns `(OrderType, Option<Price>)`:
/// - `(Limit, Some(price))` when a limit execution is configured.
/// - `(Market, None)` for market execution.
fn resolve_child_order_type(order: &TriggerOrder) -> (OrderType, Option<Price>) {
    // Check explicit TP/SL limit price fields first.
    match order.trigger_type {
        TriggerOrderType::TakeProfit | TriggerOrderType::TakeProfitMarket => {
            if order.tp_order_type == TpSlOrderType::Limit {
                if let Some(lp) = order.tp_limit_price {
                    return (OrderType::Limit, Some(lp));
                }
            }
        }
        TriggerOrderType::StopMarket | TriggerOrderType::StopLimit => {
            if order.sl_order_type == TpSlOrderType::Limit {
                if let Some(lp) = order.sl_limit_price {
                    return (OrderType::Limit, Some(lp));
                }
            }
        }
        TriggerOrderType::TrailingStop => {}
    }

    // Fall back to legacy order_price field.
    if let Some(price) = order.order_price {
        return (OrderType::Limit, Some(price));
    }

    (OrderType::Market, None)
}

fn validate_trailing(order: &TriggerOrder) -> Result<(), TriggerError> {
    match order.trailing_delta {
        None => Err(TriggerError::MissingTrailingDelta(order.id)),
        Some(d) if d <= Decimal::ZERO => Err(TriggerError::InvalidTrailingDelta(order.id, d)),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trigger_order::{TrailType, TriggerBy, TriggerOrder, TriggerOrderType, TriggerStatus};
    use rust_decimal_macros::dec;

    fn btc() -> Symbol {
        Symbol::new("BTCUSDT")
    }
    fn eth() -> Symbol {
        Symbol::new("ETHUSDT")
    }

    fn active_order(
        symbol: Symbol,
        trigger_type: TriggerOrderType,
        side: Side,
        trigger_price: Price,
        order_price: Option<Price>,
        trigger_by: TriggerBy,
    ) -> TriggerOrder {
        TriggerOrder {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            symbol,
            trigger_type,
            side,
            trigger_price,
            order_price,
            qty: dec!(1),
            trigger_by,
            reduce_only: false,
            close_on_trigger: false,
            trailing_delta: None,
            trail_type: TrailType::Fixed,
            tp_limit_price: None,
            sl_limit_price: None,
            tp_order_type: Default::default(),
            sl_order_type: Default::default(),
            status: TriggerStatus::Active,
            created_at: Utc::now(),
            triggered_at: None,
            expires_at: None,
        }
    }

    fn engine() -> TriggerEngine {
        TriggerEngine::with_defaults()
    }

    fn tick(
        eng: &mut TriggerEngine,
        sym: &Symbol,
        price: Price,
    ) -> Vec<TriggeredResult> {
        eng.check_triggers(sym, price, price, price, Utc::now())
    }

    // ------------------------------------------------------------------
    // StopLimit
    // ------------------------------------------------------------------

    #[test]
    fn stop_limit_buy_triggers_when_price_reaches_trigger() {
        let mut eng = engine();
        let mut order = active_order(
            btc(),
            TriggerOrderType::StopLimit,
            Side::Buy,
            dec!(51000),
            Some(dec!(51050)),
            TriggerBy::LastPrice,
        );
        order.qty = dec!(0.5);
        eng.add_order(order).unwrap();

        // Below trigger — no fire
        let r = tick(&mut eng, &btc(), dec!(50000));
        assert!(r.is_empty());

        // Exactly at trigger — fires
        let r = tick(&mut eng, &btc(), dec!(51000));
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].trigger_order.status, TriggerStatus::Triggered);
        // Should produce a limit order
        assert!(matches!(
            &r[0].command,
            Command::PlaceOrder(c) if c.order_type == OrderType::Limit
        ));
    }

    #[test]
    fn stop_limit_buy_no_trigger_below_price() {
        let mut eng = engine();
        eng.add_order(active_order(
            btc(),
            TriggerOrderType::StopLimit,
            Side::Buy,
            dec!(51000),
            Some(dec!(51050)),
            TriggerBy::LastPrice,
        ))
        .unwrap();

        let r = tick(&mut eng, &btc(), dec!(50999));
        assert!(r.is_empty());
    }

    #[test]
    fn stop_limit_sell_triggers_when_price_drops_to_trigger() {
        let mut eng = engine();
        eng.add_order(active_order(
            btc(),
            TriggerOrderType::StopLimit,
            Side::Sell,
            dec!(49000),
            Some(dec!(48950)),
            TriggerBy::LastPrice,
        ))
        .unwrap();

        // Above trigger — no fire
        let r = tick(&mut eng, &btc(), dec!(50000));
        assert!(r.is_empty());

        // At trigger — fires
        let r = tick(&mut eng, &btc(), dec!(49000));
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn stop_limit_sell_no_trigger_above_price() {
        let mut eng = engine();
        eng.add_order(active_order(
            btc(),
            TriggerOrderType::StopLimit,
            Side::Sell,
            dec!(49000),
            Some(dec!(48950)),
            TriggerBy::LastPrice,
        ))
        .unwrap();

        let r = tick(&mut eng, &btc(), dec!(49001));
        assert!(r.is_empty());
    }

    // ------------------------------------------------------------------
    // StopMarket
    // ------------------------------------------------------------------

    #[test]
    fn stop_market_buy_produces_market_command() {
        let mut eng = engine();
        eng.add_order(active_order(
            btc(),
            TriggerOrderType::StopMarket,
            Side::Buy,
            dec!(51000),
            None, // no limit price → market
            TriggerBy::MarkPrice,
        ))
        .unwrap();

        let r = eng.check_triggers(&btc(), dec!(40000), dec!(51000), dec!(40000), Utc::now());
        assert_eq!(r.len(), 1);
        assert!(matches!(
            &r[0].command,
            Command::PlaceOrder(c) if c.order_type == OrderType::Market
        ));
    }

    #[test]
    fn stop_market_sell_triggers_at_or_below() {
        let mut eng = engine();
        eng.add_order(active_order(
            btc(),
            TriggerOrderType::StopMarket,
            Side::Sell,
            dec!(49000),
            None,
            TriggerBy::LastPrice,
        ))
        .unwrap();

        let r = tick(&mut eng, &btc(), dec!(48999));
        assert_eq!(r.len(), 1);
    }

    // ------------------------------------------------------------------
    // TakeProfit
    // ------------------------------------------------------------------

    #[test]
    fn take_profit_sell_triggers_when_price_rises_to_target() {
        let mut eng = engine();
        // Long position: take profit SELL when price rises to 52000.
        // Entry was at 50000; target is above — triggers when price >= 52000.
        eng.add_order(active_order(
            btc(),
            TriggerOrderType::TakeProfit,
            Side::Sell,
            dec!(52000),
            Some(dec!(51900)),
            TriggerBy::LastPrice,
        ))
        .unwrap();

        // Below target — no fire
        let r = tick(&mut eng, &btc(), dec!(50000));
        assert!(r.is_empty());

        // At target — fires
        let r = tick(&mut eng, &btc(), dec!(52000));
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn take_profit_buy_triggers_when_price_falls_to_target() {
        let mut eng = engine();
        // Long position closed: take profit BUY (covering short) when price falls
        eng.add_order(active_order(
            btc(),
            TriggerOrderType::TakeProfit,
            Side::Buy,
            dec!(45000),
            Some(dec!(45100)),
            TriggerBy::LastPrice,
        ))
        .unwrap();

        // Above target — no fire
        let r = tick(&mut eng, &btc(), dec!(50000));
        assert!(r.is_empty());

        // At target — fires
        let r = tick(&mut eng, &btc(), dec!(45000));
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn take_profit_market_sell_produces_market_command() {
        let mut eng = engine();
        eng.add_order(active_order(
            btc(),
            TriggerOrderType::TakeProfitMarket,
            Side::Sell,
            dec!(48000),
            None,
            TriggerBy::LastPrice,
        ))
        .unwrap();

        let r = tick(&mut eng, &btc(), dec!(48000));
        assert_eq!(r.len(), 1);
        assert!(matches!(
            &r[0].command,
            Command::PlaceOrder(c) if c.order_type == OrderType::Market
        ));
    }

    // ------------------------------------------------------------------
    // TriggerBy variants
    // ------------------------------------------------------------------

    #[test]
    fn trigger_by_mark_price_uses_mark() {
        let mut eng = engine();
        eng.add_order(active_order(
            btc(),
            TriggerOrderType::StopMarket,
            Side::Buy,
            dec!(51000),
            None,
            TriggerBy::MarkPrice,
        ))
        .unwrap();

        // last=50000, mark=51000, index=50000 → should trigger on mark
        let r = eng.check_triggers(&btc(), dec!(50000), dec!(51000), dec!(50000), Utc::now());
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn trigger_by_index_price_uses_index() {
        let mut eng = engine();
        eng.add_order(active_order(
            btc(),
            TriggerOrderType::StopMarket,
            Side::Sell,
            dec!(49000),
            None,
            TriggerBy::IndexPrice,
        ))
        .unwrap();

        // last=50000, mark=50000, index=49000 → should trigger on index
        let r = eng.check_triggers(&btc(), dec!(50000), dec!(50000), dec!(49000), Utc::now());
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn trigger_by_last_price_uses_last() {
        let mut eng = engine();
        eng.add_order(active_order(
            btc(),
            TriggerOrderType::StopMarket,
            Side::Buy,
            dec!(51000),
            None,
            TriggerBy::LastPrice,
        ))
        .unwrap();

        // last=51000, mark=50000, index=50000 → only last triggers
        let r = eng.check_triggers(&btc(), dec!(51000), dec!(50000), dec!(50000), Utc::now());
        assert_eq!(r.len(), 1);
    }

    // ------------------------------------------------------------------
    // Cancellation
    // ------------------------------------------------------------------

    #[test]
    fn cancel_removes_order() {
        let mut eng = engine();
        let order = active_order(
            btc(),
            TriggerOrderType::StopMarket,
            Side::Buy,
            dec!(51000),
            None,
            TriggerBy::LastPrice,
        );
        let id = order.id;
        eng.add_order(order).unwrap();

        let cancelled = eng.cancel_order(id).unwrap();
        assert_eq!(cancelled.status, TriggerStatus::Cancelled);

        // No trigger after cancel
        let r = tick(&mut eng, &btc(), dec!(52000));
        assert!(r.is_empty());
    }

    #[test]
    fn cancel_unknown_order_returns_error() {
        let mut eng = engine();
        let result = eng.cancel_order(Uuid::new_v4());
        assert!(matches!(result, Err(TriggerError::OrderNotFound(_))));
    }

    #[test]
    fn cancel_already_triggered_returns_error() {
        let mut eng = engine();
        let order = active_order(
            btc(),
            TriggerOrderType::StopMarket,
            Side::Buy,
            dec!(51000),
            None,
            TriggerBy::LastPrice,
        );
        let id = order.id;
        eng.add_order(order).unwrap();
        tick(&mut eng, &btc(), dec!(51000)); // triggers it

        // Order is gone from the book — cancel should fail
        let result = eng.cancel_order(id);
        assert!(matches!(result, Err(TriggerError::OrderNotFound(_))));
    }

    // ------------------------------------------------------------------
    // Expiration
    // ------------------------------------------------------------------

    #[test]
    fn expired_orders_are_removed() {
        let mut eng = engine();
        let exp = Utc::now() - chrono::Duration::seconds(1);
        let mut order = active_order(
            btc(),
            TriggerOrderType::StopMarket,
            Side::Buy,
            dec!(51000),
            None,
            TriggerBy::LastPrice,
        );
        order.expires_at = Some(exp);
        eng.add_order(order).unwrap();

        let expired = eng.expire_orders(Utc::now());
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].status, TriggerStatus::Expired);

        // Should not trigger after expiry
        let r = tick(&mut eng, &btc(), dec!(52000));
        assert!(r.is_empty());
    }

    #[test]
    fn non_expired_orders_are_kept() {
        let mut eng = engine();
        let exp = Utc::now() + chrono::Duration::hours(1);
        let mut order = active_order(
            btc(),
            TriggerOrderType::StopMarket,
            Side::Buy,
            dec!(51000),
            None,
            TriggerBy::LastPrice,
        );
        order.expires_at = Some(exp);
        eng.add_order(order).unwrap();

        let expired = eng.expire_orders(Utc::now());
        assert!(expired.is_empty());
        assert_eq!(eng.active_orders_for_symbol(&btc()), 1);
    }

    // ------------------------------------------------------------------
    // Trailing stop
    // ------------------------------------------------------------------

    #[test]
    fn trailing_stop_sell_triggers_after_retrace() {
        let mut eng = engine();
        let id = Uuid::new_v4();
        let order = TriggerOrder {
            id,
            user_id: Uuid::new_v4(),
            symbol: btc(),
            trigger_type: TriggerOrderType::TrailingStop,
            side: Side::Sell,
            trigger_price: dec!(50000), // initial extreme seed
            order_price: None,
            qty: dec!(1),
            trigger_by: TriggerBy::LastPrice,
            reduce_only: false,
            close_on_trigger: false,
            trailing_delta: Some(dec!(500)),
            trail_type: TrailType::Fixed,
            tp_limit_price: None,
            sl_limit_price: None,
            tp_order_type: Default::default(),
            sl_order_type: Default::default(),
            status: TriggerStatus::Active,
            created_at: Utc::now(),
            triggered_at: None,
            expires_at: None,
        };
        eng.add_order(order).unwrap();

        // Price rises — peak updates, no trigger yet
        let r = tick(&mut eng, &btc(), dec!(51000));
        assert!(r.is_empty());

        // Price drops by exactly the delta from 51000 → trigger
        let r = tick(&mut eng, &btc(), dec!(50500));
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].trigger_order.status, TriggerStatus::Triggered);
    }

    #[test]
    fn trailing_stop_buy_triggers_after_retrace() {
        let mut eng = engine();
        let order = TriggerOrder {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            symbol: btc(),
            trigger_type: TriggerOrderType::TrailingStop,
            side: Side::Buy,
            trigger_price: dec!(50000),
            order_price: None,
            qty: dec!(1),
            trigger_by: TriggerBy::LastPrice,
            reduce_only: false,
            close_on_trigger: false,
            trailing_delta: Some(dec!(500)),
            trail_type: TrailType::Fixed,
            tp_limit_price: None,
            sl_limit_price: None,
            tp_order_type: Default::default(),
            sl_order_type: Default::default(),
            status: TriggerStatus::Active,
            created_at: Utc::now(),
            triggered_at: None,
            expires_at: None,
        };
        eng.add_order(order).unwrap();

        // Price drops — trough updates, no trigger yet
        let r = tick(&mut eng, &btc(), dec!(49000));
        assert!(r.is_empty());

        // Price rises by delta from 49000 → trigger
        let r = tick(&mut eng, &btc(), dec!(49500));
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn trailing_stop_no_trigger_without_retrace() {
        let mut eng = engine();
        let order = TriggerOrder {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            symbol: btc(),
            trigger_type: TriggerOrderType::TrailingStop,
            side: Side::Sell,
            trigger_price: dec!(50000),
            order_price: None,
            qty: dec!(1),
            trigger_by: TriggerBy::LastPrice,
            reduce_only: false,
            close_on_trigger: false,
            trailing_delta: Some(dec!(1000)),
            trail_type: TrailType::Fixed,
            tp_limit_price: None,
            sl_limit_price: None,
            tp_order_type: Default::default(),
            sl_order_type: Default::default(),
            status: TriggerStatus::Active,
            created_at: Utc::now(),
            triggered_at: None,
            expires_at: None,
        };
        eng.add_order(order).unwrap();

        // Only 400 retrace from 50000 — delta is 1000
        let r = tick(&mut eng, &btc(), dec!(49600));
        assert!(r.is_empty());
    }

    #[test]
    fn trailing_stop_missing_delta_rejected() {
        let mut eng = engine();
        let order = TriggerOrder {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            symbol: btc(),
            trigger_type: TriggerOrderType::TrailingStop,
            side: Side::Sell,
            trigger_price: dec!(50000),
            order_price: None,
            qty: dec!(1),
            trigger_by: TriggerBy::LastPrice,
            reduce_only: false,
            close_on_trigger: false,
            trailing_delta: None, // missing
            trail_type: TrailType::Fixed,
            tp_limit_price: None,
            sl_limit_price: None,
            tp_order_type: Default::default(),
            sl_order_type: Default::default(),
            status: TriggerStatus::Active,
            created_at: Utc::now(),
            triggered_at: None,
            expires_at: None,
        };
        assert!(matches!(
            eng.add_order(order),
            Err(TriggerError::MissingTrailingDelta(_))
        ));
    }

    #[test]
    fn trailing_stop_zero_delta_rejected() {
        let mut eng = engine();
        let order = TriggerOrder {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            symbol: btc(),
            trigger_type: TriggerOrderType::TrailingStop,
            side: Side::Sell,
            trigger_price: dec!(50000),
            order_price: None,
            qty: dec!(1),
            trigger_by: TriggerBy::LastPrice,
            reduce_only: false,
            close_on_trigger: false,
            trailing_delta: Some(dec!(0)),
            trail_type: TrailType::Fixed,
            tp_limit_price: None,
            sl_limit_price: None,
            tp_order_type: Default::default(),
            sl_order_type: Default::default(),
            status: TriggerStatus::Active,
            created_at: Utc::now(),
            triggered_at: None,
            expires_at: None,
        };
        assert!(matches!(
            eng.add_order(order),
            Err(TriggerError::InvalidTrailingDelta(_, _))
        ));
    }

    // ------------------------------------------------------------------
    // Trigger pacing (max per tick)
    // ------------------------------------------------------------------

    #[test]
    fn max_triggers_per_tick_limits_output() {
        let mut eng = TriggerEngine::new(TriggerEngineConfig {
            max_triggers_per_tick: 3,
        });

        for _ in 0..5 {
            eng.add_order(active_order(
                btc(),
                TriggerOrderType::StopMarket,
                Side::Buy,
                dec!(51000),
                None,
                TriggerBy::LastPrice,
            ))
            .unwrap();
        }

        let r = tick(&mut eng, &btc(), dec!(52000));
        assert_eq!(r.len(), 3);
        // 2 orders remain for the next tick
        assert_eq!(eng.active_orders_for_symbol(&btc()), 2);
    }

    #[test]
    fn deferred_orders_fire_on_next_tick() {
        let mut eng = TriggerEngine::new(TriggerEngineConfig {
            max_triggers_per_tick: 2,
        });

        for _ in 0..4 {
            eng.add_order(active_order(
                btc(),
                TriggerOrderType::StopMarket,
                Side::Buy,
                dec!(51000),
                None,
                TriggerBy::LastPrice,
            ))
            .unwrap();
        }

        let r1 = tick(&mut eng, &btc(), dec!(52000));
        assert_eq!(r1.len(), 2);

        let r2 = tick(&mut eng, &btc(), dec!(52000));
        assert_eq!(r2.len(), 2);

        assert_eq!(eng.active_orders_for_symbol(&btc()), 0);
    }

    // ------------------------------------------------------------------
    // Reduce-only flag preserved
    // ------------------------------------------------------------------

    #[test]
    fn reduce_only_flag_is_preserved_in_triggered_order() {
        let mut eng = engine();
        let mut order = active_order(
            btc(),
            TriggerOrderType::StopMarket,
            Side::Sell,
            dec!(49000),
            None,
            TriggerBy::LastPrice,
        );
        order.reduce_only = true;
        eng.add_order(order).unwrap();

        let r = tick(&mut eng, &btc(), dec!(48000));
        assert_eq!(r.len(), 1);
        assert!(r[0].trigger_order.reduce_only);
    }

    // ------------------------------------------------------------------
    // Queries
    // ------------------------------------------------------------------

    #[test]
    fn active_orders_for_user_returns_correct_orders() {
        let mut eng = engine();
        let user_a = Uuid::new_v4();
        let user_b = Uuid::new_v4();

        let mut order_a = active_order(
            btc(),
            TriggerOrderType::StopMarket,
            Side::Buy,
            dec!(51000),
            None,
            TriggerBy::LastPrice,
        );
        order_a.user_id = user_a;

        let mut order_b = active_order(
            btc(),
            TriggerOrderType::StopMarket,
            Side::Sell,
            dec!(49000),
            None,
            TriggerBy::LastPrice,
        );
        order_b.user_id = user_b;

        eng.add_order(order_a).unwrap();
        eng.add_order(order_b).unwrap();

        assert_eq!(eng.active_orders(user_a).len(), 1);
        assert_eq!(eng.active_orders(user_b).len(), 1);
    }

    #[test]
    fn active_orders_for_symbol_counts_correctly() {
        let mut eng = engine();
        for _ in 0..3 {
            eng.add_order(active_order(
                btc(),
                TriggerOrderType::StopMarket,
                Side::Buy,
                dec!(51000),
                None,
                TriggerBy::LastPrice,
            ))
            .unwrap();
        }
        eng.add_order(active_order(
            eth(),
            TriggerOrderType::StopMarket,
            Side::Buy,
            dec!(3000),
            None,
            TriggerBy::LastPrice,
        ))
        .unwrap();

        assert_eq!(eng.active_orders_for_symbol(&btc()), 3);
        assert_eq!(eng.active_orders_for_symbol(&eth()), 1);
    }

    #[test]
    fn duplicate_order_id_rejected() {
        let mut eng = engine();
        let order = active_order(
            btc(),
            TriggerOrderType::StopMarket,
            Side::Buy,
            dec!(51000),
            None,
            TriggerBy::LastPrice,
        );
        let order2 = TriggerOrder { id: order.id, ..order.clone() };
        eng.add_order(order).unwrap();
        assert!(matches!(
            eng.add_order(order2),
            Err(TriggerError::DuplicateOrderId(_))
        ));
    }

    #[test]
    fn total_active_orders_across_symbols() {
        let mut eng = engine();
        eng.add_order(active_order(btc(), TriggerOrderType::StopMarket, Side::Buy, dec!(51000), None, TriggerBy::LastPrice)).unwrap();
        eng.add_order(active_order(eth(), TriggerOrderType::StopMarket, Side::Buy, dec!(3000), None, TriggerBy::LastPrice)).unwrap();
        assert_eq!(eng.total_active_orders(), 2);
    }

    #[test]
    fn triggered_order_no_longer_counted_active() {
        let mut eng = engine();
        eng.add_order(active_order(
            btc(),
            TriggerOrderType::StopMarket,
            Side::Buy,
            dec!(51000),
            None,
            TriggerBy::LastPrice,
        ))
        .unwrap();
        assert_eq!(eng.active_orders_for_symbol(&btc()), 1);
        tick(&mut eng, &btc(), dec!(51000));
        assert_eq!(eng.active_orders_for_symbol(&btc()), 0);
    }

    // ------------------------------------------------------------------
    // Close-on-trigger flag
    // ------------------------------------------------------------------

    #[test]
    fn close_on_trigger_flag_preserved() {
        let mut eng = engine();
        let mut order = active_order(
            btc(),
            TriggerOrderType::StopMarket,
            Side::Sell,
            dec!(49000),
            None,
            TriggerBy::LastPrice,
        );
        order.close_on_trigger = true;
        eng.add_order(order).unwrap();

        let r = tick(&mut eng, &btc(), dec!(48000));
        assert_eq!(r.len(), 1);
        assert!(r[0].trigger_order.close_on_trigger);
    }

    // ------------------------------------------------------------------
    // Exact price boundary
    // ------------------------------------------------------------------

    #[test]
    fn stop_buy_exact_trigger_price_fires() {
        let mut eng = engine();
        eng.add_order(active_order(
            btc(),
            TriggerOrderType::StopMarket,
            Side::Buy,
            dec!(50000),
            None,
            TriggerBy::LastPrice,
        ))
        .unwrap();

        let r = tick(&mut eng, &btc(), dec!(50000));
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn stop_sell_exact_trigger_price_fires() {
        let mut eng = engine();
        eng.add_order(active_order(
            btc(),
            TriggerOrderType::StopMarket,
            Side::Sell,
            dec!(50000),
            None,
            TriggerBy::LastPrice,
        ))
        .unwrap();

        let r = tick(&mut eng, &btc(), dec!(50000));
        assert_eq!(r.len(), 1);
    }

    // ------------------------------------------------------------------
    // No cross-symbol triggering
    // ------------------------------------------------------------------

    #[test]
    fn tick_on_btc_does_not_trigger_eth_orders() {
        let mut eng = engine();
        eng.add_order(active_order(
            eth(),
            TriggerOrderType::StopMarket,
            Side::Buy,
            dec!(3000),
            None,
            TriggerBy::LastPrice,
        ))
        .unwrap();

        // BTC price crosses ETH trigger price — should not fire
        let r = tick(&mut eng, &btc(), dec!(3000));
        assert!(r.is_empty());
        assert_eq!(eng.active_orders_for_symbol(&eth()), 1);
    }

    // ------------------------------------------------------------------
    // Percentage trailing stop (TrailType::Percentage)
    // ------------------------------------------------------------------

    #[test]
    fn percentage_trailing_sell_triggers_on_pct_retrace() {
        let mut eng = engine();
        let order = TriggerOrder {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            symbol: btc(),
            trigger_type: TriggerOrderType::TrailingStop,
            side: Side::Sell,
            trigger_price: dec!(50000),
            order_price: None,
            qty: dec!(1),
            trigger_by: TriggerBy::LastPrice,
            reduce_only: false,
            close_on_trigger: false,
            trailing_delta: Some(dec!(1)), // 1%
            trail_type: TrailType::Percentage,
            tp_limit_price: None,
            sl_limit_price: None,
            tp_order_type: Default::default(),
            sl_order_type: Default::default(),
            status: TriggerStatus::Active,
            created_at: Utc::now(),
            triggered_at: None,
            expires_at: None,
        };
        eng.add_order(order).unwrap();

        // Price rises to 52000 (peak), then drops by 1% = 520 → trigger at 51480
        let r = tick(&mut eng, &btc(), dec!(52000));
        assert!(r.is_empty());

        // Price drops exactly 1% from 52000 = 51480
        let r = tick(&mut eng, &btc(), dec!(51480));
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].trigger_order.status, TriggerStatus::Triggered);
    }

    #[test]
    fn percentage_trailing_sell_no_trigger_below_threshold() {
        let mut eng = engine();
        let order = TriggerOrder {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            symbol: btc(),
            trigger_type: TriggerOrderType::TrailingStop,
            side: Side::Sell,
            trigger_price: dec!(50000),
            order_price: None,
            qty: dec!(1),
            trigger_by: TriggerBy::LastPrice,
            reduce_only: false,
            close_on_trigger: false,
            trailing_delta: Some(dec!(2)), // 2%
            trail_type: TrailType::Percentage,
            tp_limit_price: None,
            sl_limit_price: None,
            tp_order_type: Default::default(),
            sl_order_type: Default::default(),
            status: TriggerStatus::Active,
            created_at: Utc::now(),
            triggered_at: None,
            expires_at: None,
        };
        eng.add_order(order).unwrap();

        // 1% retrace only — not enough for 2% trail
        let r = tick(&mut eng, &btc(), dec!(49000)); // 2% drop from 50000 exactly
        // 50000 * 2 / 100 = 1000; 50000 - 49000 = 1000 → exactly triggers
        assert_eq!(r.len(), 1);
    }
}
