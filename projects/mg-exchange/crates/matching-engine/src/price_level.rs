use exchange_common::{Order, OrderId, Qty};
use std::collections::VecDeque;

/// A price level in the order book — FIFO queue of orders at the same price.
///
/// Uses VecDeque for O(1) push_back / pop_front (FIFO matching).
/// Total quantity is cached and maintained incrementally — never recomputed.
///
/// `remove()` by order_id is O(n) over the level depth. This is a deliberate
/// trade-off: a slab + index-map implementation gives O(1) cancel but pays a
/// constant-factor penalty on every `push` / `pop_front`. Because fills (the
/// hot path) vastly outnumber middle-of-queue cancels in realistic workloads,
/// the simpler VecDeque wins overall. Measured via `benches/market_maker.rs`
/// — see commit history for the negative result from the slab experiment.
#[derive(Debug)]
pub struct PriceLevel {
    orders: VecDeque<Order>,
    total_qty: Qty,
}

impl PriceLevel {
    pub fn new() -> Self {
        Self {
            orders: VecDeque::new(),
            total_qty: Qty::ZERO,
        }
    }

    /// Add an order to the back of the queue (FIFO).
    pub fn push(&mut self, order: Order) {
        self.total_qty += order.remaining_qty();
        self.orders.push_back(order);
    }

    /// Peek at the front order (oldest, highest priority).
    pub fn front(&self) -> Option<&Order> {
        self.orders.front()
    }

    /// Mutable access to the front order.
    pub fn front_mut(&mut self) -> Option<&mut Order> {
        self.orders.front_mut()
    }

    /// Remove the front order (after it's fully filled).
    pub fn pop_front(&mut self) -> Option<Order> {
        if let Some(order) = self.orders.pop_front() {
            self.total_qty -= order.remaining_qty();
            Some(order)
        } else {
            None
        }
    }

    /// Remove a specific order by ID (for cancellation). O(n) — see the
    /// struct-level comment for why this is intentional.
    pub fn remove(&mut self, order_id: &OrderId) -> Option<Order> {
        if let Some(pos) = self.orders.iter().position(|o| o.id == *order_id) {
            let order = self.orders.remove(pos)?;
            self.total_qty -= order.remaining_qty();
            Some(order)
        } else {
            None
        }
    }

    /// Update total_qty when front order gets partially filled.
    pub fn reduce_qty(&mut self, amount: Qty) {
        self.total_qty -= amount;
    }

    pub fn total_qty(&self) -> Qty {
        self.total_qty
    }

    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }

    pub fn order_count(&self) -> usize {
        self.orders.len()
    }

    /// Iterate over all orders in this level (front-to-back, i.e. FIFO order).
    /// Used by invariant checking code.
    pub fn orders_iter(&self) -> impl Iterator<Item = &Order> {
        self.orders.iter()
    }
}
