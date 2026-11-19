use std::borrow::Cow;
use std::collections::BTreeMap;

use exchange_common::*;
use rust_decimal::Decimal;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use crate::price_level::PriceLevel;

/// Inline event buffer capacity.
///
/// The typical hot path emits 1–2 events (OrderAccepted + optional Fill).
/// Using SmallVec<[MatchEvent; 2]> covers the dominant 1-fill case without
/// spilling and halves the stack footprint vs [4]:
///   [4] = 4 × 144 bytes = 576 bytes inline + 24 header = 600 bytes/frame
///   [2] = 2 × 144 bytes = 288 bytes inline + 24 header = 312 bytes/frame
///
/// The sweep path (3+ fills) already traverses the book on the heap; one
/// additional SmallVec spill at event 3 is not the bottleneck.
pub type EventBuffer = SmallVec<[MatchEvent; 2]>;

/// Order book for a single trading pair.
///
/// Architecture: BTreeMap<Price, PriceLevel> for each side.
/// - Bids sorted descending (highest first) — use Reverse key or iterate from back
/// - Asks sorted ascending (lowest first) — natural BTreeMap order
///
/// Design decisions from gitbitex analysis:
/// - Price-time priority (FIFO within price level)
/// - Taker always gets filled at MAKER's price (price improvement)
/// - Market order remainder is CANCELLED, not left on book
/// - No self-trade prevention in base engine (added as configurable check)
/// Maximum number of distinct price levels per side (bid or ask).
/// Prevents OOM via orderbook depth abuse (attacker placing 1M orders at distinct prices).
pub const MAX_PRICE_LEVELS_PER_SIDE: usize = 10_000;

pub struct OrderBook {
    symbol: Symbol,
    asks: BTreeMap<Price, PriceLevel>,
    bids: BTreeMap<Price, PriceLevel>,
    /// Fast O(1) lookup for cancel operations. FxHashMap: faster than std HashMap
    /// for integer/UUID keys because FxHasher is cheaper than SipHash.
    order_locations: FxHashMap<OrderId, (Side, Price)>,
    /// Per-user open order count on this symbol. O(1) lookup for order-limit checks.
    ///
    /// Incremented by `insert_order`, decremented by `cancel_order` (found branch)
    /// and by the `pending_location_removes` flush in `match_order` (maker fully filled).
    /// Source of truth for `OrderLimitConfig::max_open_orders_per_user_per_symbol`
    /// enforcement in `order-gateway/src/validator.rs` (Sprint 8, KG-2).
    user_order_count: FxHashMap<UserId, usize>,
    /// Monotonic trade ID counter.
    next_trade_id: TradeId,
    /// Monotonic sequence number.
    sequence: SeqNo,
    /// Cached best bid price. O(1) lookup via `best_bid()`. Maintained by
    /// `update_best_bid_cache()` on every bid-side mutation.
    best_bid_cache: Option<Price>,
    /// Cached best ask price. Same maintenance contract.
    best_ask_cache: Option<Price>,
}

impl OrderBook {
    pub fn new(symbol: Symbol) -> Self {
        Self {
            symbol,
            asks: BTreeMap::new(),
            bids: BTreeMap::new(),
            order_locations: FxHashMap::default(),
            user_order_count: FxHashMap::default(),
            next_trade_id: 1,
            sequence: 0,
            best_bid_cache: None,
            best_ask_cache: None,
        }
    }

    /// Current next trade ID (for call auction seeding).
    pub fn next_trade_id(&self) -> TradeId {
        self.next_trade_id
    }

    /// Build and emit a PostOnly rejection event.
    ///
    /// Marked `#[cold]` + `#[inline(never)]` so the compiler knows this branch
    /// is unlikely and avoids inlining the rejection code into the hot path of
    /// `place_order`. The common case (order accepted / matched) stays in the
    /// branch predictor's fast lane.
    #[cold]
    #[inline(never)]
    fn reject_postonly(
        symbol: &Symbol,
        order_id: OrderId,
        sequence: &mut SeqNo,
        events: &mut EventBuffer,
    ) {
        *sequence += 1;
        events.push(MatchEvent::OrderRejected {
            order_id,
            symbol: symbol.clone(),
            reason: Cow::Borrowed("Post-only order would take liquidity"),
            seq: *sequence,
        });
    }

    /// Build and emit a FOK rejection event.
    ///
    /// Same `#[cold]` + `#[inline(never)]` rationale as `reject_postonly`.
    #[cold]
    #[inline(never)]
    fn reject_fok(
        symbol: &Symbol,
        order_id: OrderId,
        sequence: &mut SeqNo,
        events: &mut EventBuffer,
    ) {
        *sequence += 1;
        events.push(MatchEvent::OrderRejected {
            order_id,
            symbol: symbol.clone(),
            reason: Cow::Borrowed("FOK order cannot be fully filled"),
            seq: *sequence,
        });
    }

    /// Process an incoming order: match against the book, then rest remainder.
    ///
    /// Returns a list of events (fills, accepted, cancelled, rejected).
    pub fn place_order(&mut self, cmd: PlaceOrderCommand) -> EventBuffer {
        let mut events = EventBuffer::new();

        let mut order = Order {
            id: cmd.order_id,
            user_id: cmd.user_id,
            symbol: cmd.symbol,
            side: cmd.side,
            order_type: cmd.order_type,
            time_in_force: cmd.time_in_force,
            price: cmd.price,
            qty: cmd.qty,
            filled_qty: Decimal::ZERO,
            status: OrderStatus::Pending,
            created_at: cmd.timestamp,
            expire_at: cmd.expire_at,
            order_link_id: cmd.order_link_id,
        };

        // PostOnly check: reject if would immediately match.
        // Rejection path is cold; hot path (non-crossing PostOnly) falls through
        // to the fast path below.
        if order.time_in_force == TimeInForce::PostOnly && self.would_match(&order) {
            Self::reject_postonly(&self.symbol, order.id, &mut self.sequence, &mut events);
            return events;
        }

        // FOK check: verify full fill is possible before matching.
        // Pass user_id and stp_mode so can_fill_fully can exclude self-orders
        // when STP is active (stp_mode is on PlaceOrderCommand, not on Order).
        if order.time_in_force == TimeInForce::Fok
            && !self.can_fill_fully(&order, order.user_id, cmd.stp_mode)
        {
            Self::reject_fok(&self.symbol, order.id, &mut self.sequence, &mut events);
            return events;
        }

        // Fast path: non-crossing GTC / GTD / PostOnly limit order.
        //
        // When `would_match` returns false for a GTC/GTD/PostOnly limit, we
        // know no fills are possible in this call (single-threaded book — no
        // concurrent mutations). Skip `match_order` entirely:
        //   • no `Vec::with_capacity(8)` allocation
        //   • no outer loop + first/last_entry call
        //   • no crossing check (already done via would_match / cache hit above)
        //
        // IOC is explicitly excluded: an IOC with no cross must emit
        // OrderCancelled (not OrderAccepted), so it must go through match_order.
        // Market and FOK are excluded: they must always run the match loop.
        if order.order_type == OrderType::Limit
            && matches!(
                order.time_in_force,
                TimeInForce::Gtc | TimeInForce::Gtd | TimeInForce::PostOnly
            )
            && !self.would_match(&order)
        {
            // Non-crossing resting order: rest directly without matching.
            order.status = OrderStatus::Open;
            self.sequence += 1;
            let accepted_seq = self.sequence;
            let order_id = order.id;
            if self.insert_order(order) {
                events.push(MatchEvent::OrderAccepted {
                    order_id,
                    symbol: self.symbol.clone(),
                    seq: accepted_seq,
                });
            } else {
                self.sequence += 1;
                events.push(MatchEvent::OrderRejected {
                    order_id,
                    symbol: self.symbol.clone(),
                    reason: std::borrow::Cow::Borrowed("Orderbook depth limit exceeded"),
                    seq: self.sequence,
                });
            }
            #[cfg(debug_assertions)]
            self.check_invariants();
            return events;
        }

        // Slow path: run the full match loop (crossing, IOC, Market, FOK fills).
        // Match against the opposite side.
        // Returns true if STP terminated the taker (OrderCancelled already emitted).
        let stp_cancelled = self.match_order(&mut order, &mut events, cmd.timestamp, cmd.stp_mode);

        // Handle remaining quantity — skip if STP already cancelled the taker.
        if !stp_cancelled && !order.is_fully_filled() {
            match order.order_type {
                OrderType::Market => {
                    // Market order remainder is cancelled (gitbitex insight!).
                    // A partial fill leaves status PartiallyFilled; the subsequent
                    // OrderCancelled event carries the remaining_qty > 0. Setting
                    // Filled here would be internally contradictory with that event.
                    if order.filled_qty > Decimal::ZERO {
                        order.status = OrderStatus::PartiallyFilled;
                    } else {
                        order.status = OrderStatus::Cancelled;
                    }
                    self.sequence += 1;
                    events.push(MatchEvent::OrderCancelled {
                        order_id: order.id,
                        symbol: self.symbol.clone(),
                        remaining_qty: order.remaining_qty(),
                        seq: self.sequence,
                    });
                }
                OrderType::Limit => {
                    match order.time_in_force {
                        TimeInForce::Ioc => {
                            // IOC: cancel unfilled remainder
                            order.status = OrderStatus::Cancelled;
                            self.sequence += 1;
                            events.push(MatchEvent::OrderCancelled {
                                order_id: order.id,
                                symbol: self.symbol.clone(),
                                remaining_qty: order.remaining_qty(),
                                seq: self.sequence,
                            });
                        }
                        TimeInForce::Gtc | TimeInForce::PostOnly | TimeInForce::Gtd => {
                            order.status = if order.filled_qty > Decimal::ZERO {
                                OrderStatus::PartiallyFilled
                            } else {
                                OrderStatus::Open
                            };
                            self.sequence += 1;
                            let accepted_seq = self.sequence;
                            let order_id = order.id;
                            let remaining = order.remaining_qty();
                            if self.insert_order(order) {
                                events.push(MatchEvent::OrderAccepted {
                                    order_id,
                                    symbol: self.symbol.clone(),
                                    seq: accepted_seq,
                                });
                            } else {
                                // Depth limit reached — reject the remainder.
                                self.sequence += 1;
                                events.push(MatchEvent::OrderRejected {
                                    order_id,
                                    symbol: self.symbol.clone(),
                                    reason: std::borrow::Cow::Borrowed("Orderbook depth limit exceeded"),
                                    seq: self.sequence,
                                });
                            }
                        }
                        TimeInForce::Fok => {
                            // Should not reach here — FOK is checked above
                            unreachable!("FOK order should have been fully filled or rejected");
                        }
                    }
                }
            }
        }

        #[cfg(debug_assertions)]
        self.check_invariants();

        events
    }

    /// Cancel an existing order on the book.
    pub fn cancel_order(&mut self, cmd: CancelOrderCommand) -> EventBuffer {
        let mut events = EventBuffer::new();

        if let Some((side, price)) = self.order_locations.remove(&cmd.order_id) {
            let book_side = match side {
                Side::Buy => &mut self.bids,
                Side::Sell => &mut self.asks,
            };

            let mut remaining_qty = Decimal::ZERO;
            let mut cancelled_user_id: Option<UserId> = None;

            if let Some(level) = book_side.get_mut(&price) {
                if let Some(removed) = level.remove(&cmd.order_id) {
                    remaining_qty = removed.remaining_qty();
                    cancelled_user_id = Some(removed.user_id);
                }
                if level.is_empty() {
                    book_side.remove(&price);
                }
            }

            // Decrement per-user open order count (order left the book via cancel).
            if let Some(uid) = cancelled_user_id {
                let count = self.user_order_count.entry(uid).or_insert(0);
                *count = count.saturating_sub(1);
            }

            // Update cache: cancelling the sole order on the best level removes
            // that level, which changes best_bid/best_ask. Safe to call
            // unconditionally — refresh is O(log n) and correct in all cases
            // (order was at best level, non-best level, or level still has orders).
            match side {
                Side::Buy => self.update_best_bid_cache(),
                Side::Sell => self.update_best_ask_cache(),
            }

            self.sequence += 1;
            events.push(MatchEvent::OrderCancelled {
                order_id: cmd.order_id,
                symbol: self.symbol.clone(),
                remaining_qty,
                seq: self.sequence,
            });
        }
        // Silent ignore if order not found (gitbitex behavior).
        // NOTE: no cache update needed — no mutation happened.

        #[cfg(debug_assertions)]
        self.check_invariants();

        events
    }

    /// Match incoming order against the opposite side of the book.
    ///
    /// STP enforcement is fully active for all TIF variants (GTC, GTD, IOC,
    /// Market). The `stp_mode` parameter determines behavior on self-trade:
    ///
    /// - `None` — allow self-fill
    /// - `ExpireTaker` — cancel incoming taker, preserve prior fills
    /// - `ExpireMaker` — cancel resting maker, taker continues matching
    /// - `ExpireBoth` — cancel both, taker exits
    ///
    /// Emits `OrderCancelled` events for any cancellation triggered by STP.
    /// Sequence numbers are monotonic across STP cancels and regular fills.
    ///
    /// FOK orders pre-check fillability via `can_fill_fully` (T1.5), which
    /// applies the same STP filter. A FOK that passes the pre-check will not
    /// encounter self-orders during matching because `can_fill_fully` filtered
    /// them out — but the STP check in this function is still present for
    /// defense in depth.
    ///
    /// Reference: Bybit API Spec v5 "Self-Trade Prevention (STP) Mode";
    /// Binance "Self-Trade Prevention Mode" — semantics match industry standard.
    ///
    /// T1.1 (Sprint 2): lazy BTreeMap traversal via `first_entry`/`last_entry`.
    /// Eliminates the `keys().cloned().collect()` Vec allocation that previously
    /// caused O(depth) cost even when the taker fills in O(1) levels.
    ///
    /// T1.2 (Sprint 2): `empty_prices` Vec removed; empty levels are removed
    /// inline via `OccupiedEntry::remove()` when the borrow is released.
    ///
    /// Borrow-safety pattern: `OccupiedEntry` holds `&mut self.asks/bids`.
    /// `self.order_locations.remove()` cannot be called while that borrow is live.
    /// Solution: accumulate completed maker IDs in `pending_location_removes` and
    /// flush them after the entry borrow is released. STP cancels reuse the same
    /// buffer — ExpireMaker pushes cancelled maker ids just like fills do.
    /// Returns `true` if an STP rule cancelled the taker (ExpireTaker or
    /// ExpireBoth), meaning `OrderCancelled` was already emitted for the taker
    /// and the caller must NOT emit a second cancel or rest the order.
    fn match_order(
        &mut self,
        taker: &mut Order,
        events: &mut EventBuffer,
        timestamp: chrono::DateTime<chrono::Utc>,
        stp_mode: StpMode,
    ) -> bool {
        // Capacity 8: covers typical sweep depth without re-allocation.
        // Stores (OrderId, UserId) so we can decrement per-user count when
        // flushing makers that were fully consumed by this taker.
        let mut pending_location_removes: Vec<(OrderId, UserId)> = Vec::with_capacity(8);

        // Set when an STP rule terminates the taker (ExpireTaker or ExpireBoth).
        // We cannot return inside the entry borrow scope, so we use a flag that
        // breaks both inner and outer loops, then return after the flush.
        let mut taker_terminated = false;

        'outer: loop {
            if taker.is_fully_filled() {
                break;
            }

            // Grab the best opposing price level as an OccupiedEntry.
            // For a buy taker, we consume the lowest ask (ascending order).
            // For a sell taker, we consume the highest bid (descending order).
            let mut entry = match taker.side {
                Side::Buy => match self.asks.first_entry() {
                    Some(e) => e,
                    None => break,
                },
                Side::Sell => match self.bids.last_entry() {
                    Some(e) => e,
                    None => break,
                },
            };

            let price = *entry.key();

            // Check whether the taker's price crosses this level.
            let crosses = match taker.order_type {
                OrderType::Market => true,
                OrderType::Limit => match taker.side {
                    Side::Buy => taker.price >= price,
                    Side::Sell => taker.price <= price,
                },
            };
            if !crosses {
                break;
            }

            // entry.get_mut() returns &mut PriceLevel — still inside the entry borrow.
            let level = entry.get_mut();

            // Match against orders at this price level (FIFO).
            while !taker.is_fully_filled() && !level.is_empty() {
                let maker_user_id = level.front().unwrap().user_id;

                // STP check: only when stp_mode != None and same user.
                // Reference: Bybit STP spec — self-trade detected when
                // taker.user_id == maker.user_id.
                if stp_mode != StpMode::None && maker_user_id == taker.user_id {
                    match stp_mode {
                        StpMode::ExpireTaker => {
                            // Cancel the incoming taker. Preserve any fills already
                            // emitted against non-self makers on earlier levels.
                            // Variant A (Bybit/Binance standard): keep partial fills.
                            taker.status = if taker.filled_qty > Decimal::ZERO {
                                OrderStatus::PartiallyFilled
                            } else {
                                OrderStatus::Cancelled
                            };
                            self.sequence += 1;
                            events.push(MatchEvent::OrderCancelled {
                                order_id: taker.id,
                                symbol: self.symbol.clone(),
                                remaining_qty: taker.remaining_qty(),
                                seq: self.sequence,
                            });
                            taker_terminated = true;
                            break;
                        }
                        StpMode::ExpireMaker => {
                            // Cancel the self-maker; taker continues matching
                            // against the next order at this level (or next level).
                            let maker = level.pop_front().unwrap();
                            let maker_id = maker.id;
                            let maker_uid = maker.user_id;
                            self.sequence += 1;
                            events.push(MatchEvent::OrderCancelled {
                                order_id: maker_id,
                                symbol: self.symbol.clone(),
                                remaining_qty: maker.remaining_qty(),
                                seq: self.sequence,
                            });
                            // Buffer for post-entry-drop flush (same pattern as fills).
                            pending_location_removes.push((maker_id, maker_uid));
                            // Continue inner loop: pick up the next front order.
                            continue;
                        }
                        StpMode::ExpireBoth => {
                            // Cancel both the self-maker and the taker.
                            let maker = level.pop_front().unwrap();
                            let maker_id = maker.id;
                            let maker_uid = maker.user_id;
                            self.sequence += 1;
                            events.push(MatchEvent::OrderCancelled {
                                order_id: maker_id,
                                symbol: self.symbol.clone(),
                                remaining_qty: maker.remaining_qty(),
                                seq: self.sequence,
                            });
                            pending_location_removes.push((maker_id, maker_uid));

                            taker.status = if taker.filled_qty > Decimal::ZERO {
                                OrderStatus::PartiallyFilled
                            } else {
                                OrderStatus::Cancelled
                            };
                            self.sequence += 1;
                            events.push(MatchEvent::OrderCancelled {
                                order_id: taker.id,
                                symbol: self.symbol.clone(),
                                remaining_qty: taker.remaining_qty(),
                                seq: self.sequence,
                            });
                            taker_terminated = true;
                            break;
                        }
                        StpMode::None => unreachable!("guarded above"),
                    }
                }

                let (fill_qty, fill_price, maker_order_id) = {
                    let maker = level.front().unwrap();
                    (
                        taker.remaining_qty().min(maker.remaining_qty()),
                        maker.price,
                        maker.id,
                    )
                };

                level.front_mut().unwrap().filled_qty += fill_qty;
                level.reduce_qty(fill_qty);
                taker.filled_qty += fill_qty;

                self.sequence += 1;
                let trade_id = self.next_trade_id;
                self.next_trade_id += 1;

                events.push(MatchEvent::Fill(FillEvent {
                    trade_id,
                    symbol: self.symbol.clone(),
                    price: fill_price,
                    qty: fill_qty,
                    maker_order_id,
                    taker_order_id: taker.id,
                    maker_user_id,
                    taker_user_id: taker.user_id,
                    taker_side: taker.side,
                    timestamp,
                    seq: self.sequence,
                }));

                let maker_done = level.front().map(|m| m.is_fully_filled()).unwrap_or(true);
                if maker_done {
                    let removed = level.pop_front().unwrap();
                    // Cannot call self.order_locations.remove() or update
                    // user_order_count here — entry borrow is still live.
                    // Defer both to the flush below.
                    pending_location_removes.push((removed.id, removed.user_id));
                }
            }

            // Remove the price level from the book if it is now empty.
            // This consumes the entry and drops the borrow on self.asks/bids,
            // after which the pending flush is safe.
            let level_removed = level.is_empty();
            if level_removed {
                entry.remove();
            }
            // entry borrow released here — self.order_locations is accessible.

            // Unconditional drain: even if `level.is_empty()` was false (this level
            // still has resting orders), we still need to flush any maker order IDs
            // that were fully consumed in THIS iteration (partial-level sweep). Do
            // not gate this drain on `level.is_empty()` — that would leak stale
            // entries in order_locations for makers whose parent level is still live.
            for (id, uid) in pending_location_removes.drain(..) {
                self.order_locations.remove(&id);
                // Decrement per-user open order count (maker left the book via fill
                // or STP ExpireMaker/ExpireBoth cancel).
                let count = self.user_order_count.entry(uid).or_insert(0);
                *count = count.saturating_sub(1);
            }

            // Update cache after level drain + location flush.
            // Only needed when the best level was actually removed — if the level
            // still has resting orders, best price on the opposite side is unchanged.
            // We call unconditionally anyway: a single first/last_key_value() per
            // iteration is O(log n) and simpler than conditional cache updates that
            // could be wrong in subtle partial-fill edge cases.
            //
            // Taker Buy → consumes asks → update ask cache.
            // Taker Sell → consumes bids → update bid cache.
            if level_removed {
                match taker.side {
                    Side::Buy => self.update_best_ask_cache(),
                    Side::Sell => self.update_best_bid_cache(),
                }
            }

            // If taker was terminated by STP (ExpireTaker or ExpireBoth), exit
            // the outer loop. The flush above already ran; status was set inside
            // the inner loop before breaking. We use a labeled break here rather
            // than returning inside the entry borrow to respect borrow safety.
            if taker_terminated {
                break 'outer;
            }
        }

        // Final flush: catches completed makers from the last iteration where
        // the taker became fully filled mid-level (level not emptied).
        for (id, uid) in pending_location_removes {
            self.order_locations.remove(&id);
            let count = self.user_order_count.entry(uid).or_insert(0);
            *count = count.saturating_sub(1);
        }

        // Only set status here if STP did not already set it above.
        if !taker_terminated {
            if taker.is_fully_filled() {
                taker.status = OrderStatus::Filled;
            } else if taker.filled_qty > Decimal::ZERO {
                taker.status = OrderStatus::PartiallyFilled;
            }
        }

        taker_terminated
    }

    /// Recompute and store the best bid price from the BTreeMap. O(log n).
    ///
    /// Must be called after EVERY mutation that could change the highest bid
    /// price level. A single `last_key_value()` call per mutation is cheaper
    /// than calling it on every `best_bid()` read in the hot path. The design
    /// stores price only (not qty) so the helper is unconditional — we do not
    /// need to decide "did this mutation change the best?"; we just refresh.
    fn update_best_bid_cache(&mut self) {
        self.best_bid_cache = self.bids.last_key_value().map(|(price, _)| *price);
    }

    /// Recompute and store the best ask price from the BTreeMap. O(log n).
    /// Same maintenance contract as `update_best_bid_cache`.
    fn update_best_ask_cache(&mut self) {
        self.best_ask_cache = self.asks.first_key_value().map(|(price, _)| *price);
    }

    /// Insert a resting order into the book.
    /// Returns true if the order was inserted, false if rejected due to depth limit.
    fn insert_order(&mut self, order: Order) -> bool {
        let side = order.side;
        let price = order.price;
        let id = order.id;
        let user_id = order.user_id;

        let book_side = match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        };

        // C6: Reject if this would create a new price level beyond the depth limit.
        if !book_side.contains_key(&price) && book_side.len() >= MAX_PRICE_LEVELS_PER_SIDE {
            tracing::warn!(
                symbol = %self.symbol,
                side = ?side,
                price = %price,
                depth = book_side.len(),
                "Order rejected: orderbook depth limit reached (MAX_PRICE_LEVELS_PER_SIDE={})",
                MAX_PRICE_LEVELS_PER_SIDE,
            );
            return false;
        }

        book_side
            .entry(price)
            .or_insert_with(PriceLevel::new)
            .push(order);

        self.order_locations.insert(id, (side, price));

        // Increment per-user open order count for this symbol.
        *self.user_order_count.entry(user_id).or_insert(0) += 1;

        // Update cache: this insert may have created a new best-price level.
        match side {
            Side::Buy => self.update_best_bid_cache(),
            Side::Sell => self.update_best_ask_cache(),
        }
        true
    }

    /// Check if an incoming order would immediately match (for PostOnly rejection).
    ///
    /// O(1): reads directly from the best_bid_cache / best_ask_cache. No BTreeMap
    /// traversal on this hot path (PostOnly check fires on every non-taker order).
    fn would_match(&self, order: &Order) -> bool {
        match order.side {
            Side::Buy => match self.best_ask_cache {
                Some(best_ask) => order.price >= best_ask,
                None => false,
            },
            Side::Sell => match self.best_bid_cache {
                Some(best_bid) => order.price <= best_bid,
                None => false,
            },
        }
    }

    /// Check if a FOK order can be fully filled.
    ///
    /// T1.3 (Sprint 2): `Box<dyn Iterator>` removed; the loop is duplicated per
    /// side instead. Zero heap allocation on the FOK check path.
    ///
    /// T1.5 (Sprint 2): When STP is active (`stp_mode != StpMode::None`), orders
    /// belonging to `taker_user_id` are excluded from the available quantity at
    /// each level. A FOK order must be fully fillable by *other* users; if self-
    /// orders are the only liquidity, the FOK check correctly returns `false`
    /// and the order is rejected — preventing a partial-fill FOK contract violation
    /// that would occur if the STP skip happened during real matching.
    ///
    /// Note: `stp_mode` is carried on `PlaceOrderCommand`, not on `Order`, so it
    /// is passed separately here.
    fn can_fill_fully(&self, order: &Order, taker_user_id: UserId, stp_mode: StpMode) -> bool {
        let opposite = match order.side {
            Side::Buy => &self.asks,
            Side::Sell => &self.bids,
        };

        let stp_active = stp_mode != StpMode::None;
        let mut remaining = order.qty;

        // Duplicated loop per side (no Box<dyn Iterator> — zero alloc).
        // Buy: iterate asks ascending (best price first).
        // Sell: iterate bids descending (best price first).
        macro_rules! do_walk {
            ($iter:expr) => {
                for (price, level) in $iter {
                    let crosses = match order.order_type {
                        OrderType::Market => true,
                        OrderType::Limit => match order.side {
                            Side::Buy => order.price >= *price,
                            Side::Sell => order.price <= *price,
                        },
                    };

                    if !crosses {
                        break;
                    }

                    // Available qty at this level, excluding self-orders when STP active.
                    // SECURITY: This filter is O(orders_at_level). Per-user cost is bounded
                    // by `OrderLimitConfig::max_orders_per_user_per_price_level` (default 50).
                    // Per-level total is bounded by `(num_users_at_level × 50)` — there is
                    // no per-level total cap across users. Realistic worst case on a popular
                    // symbol: ~100 users × 50 = 5000 iterations per FOK check (≈50 µs at
                    // 10 ns/step). See KG-2 enforcement and KG-7 (cross-user total cap) for
                    // the gap. Comment corrected after Sprint 8 trading-qa + security review.
                    let available = if stp_active {
                        level
                            .orders_iter()
                            .filter(|o| o.user_id != taker_user_id)
                            .map(|o| o.remaining_qty())
                            .fold(Decimal::ZERO, |acc, q| acc + q)
                    } else {
                        level.total_qty()
                    };

                    let filled = available.min(remaining);
                    remaining -= filled;

                    if remaining.is_zero() {
                        return true;
                    }
                }
            };
        }

        match order.side {
            Side::Buy => do_walk!(opposite.iter()),
            Side::Sell => do_walk!(opposite.iter().rev()),
        }

        remaining.is_zero()
    }

    // --- Public accessors ---

    pub fn best_bid(&self) -> Option<(Price, Qty)> {
        // O(1) cache read for the price; O(log n) BTreeMap.get() for the qty.
        let price = self.best_bid_cache?;
        self.bids.get(&price).map(|level| (price, level.total_qty()))
    }

    pub fn best_ask(&self) -> Option<(Price, Qty)> {
        // O(1) cache read for the price; O(log n) BTreeMap.get() for the qty.
        let price = self.best_ask_cache?;
        self.asks.get(&price).map(|level| (price, level.total_qty()))
    }

    pub fn spread(&self) -> Option<Price> {
        match (self.best_ask(), self.best_bid()) {
            (Some((ask, _)), Some((bid, _))) => Some(ask - bid),
            _ => None,
        }
    }

    pub fn mid_price(&self) -> Option<Price> {
        match (self.best_ask(), self.best_bid()) {
            (Some((ask, _)), Some((bid, _))) => Some((ask + bid) / Decimal::TWO),
            _ => None,
        }
    }

    /// Get L2 depth snapshot (aggregated by price level).
    pub fn snapshot(&self, depth: usize) -> OrderBookSnapshot {
        let bids: Vec<(Price, Qty)> = self
            .bids
            .iter()
            .rev()
            .take(depth)
            .map(|(&p, l)| (p, l.total_qty()))
            .collect();

        let asks: Vec<(Price, Qty)> = self
            .asks
            .iter()
            .take(depth)
            .map(|(&p, l)| (p, l.total_qty()))
            .collect();

        OrderBookSnapshot {
            symbol: self.symbol.clone(),
            bids,
            asks,
            sequence: self.sequence,
        }
    }

    /// Full-fidelity snapshot: every individual resting order, preserving
    /// price-time priority. Used for engine state persistence / fast recovery.
    pub fn full_snapshot(&self) -> FullBookSnapshot {
        let mut orders = Vec::new();

        // Bids: highest first (same order as matching priority).
        for (_, level) in self.bids.iter().rev() {
            for order in level.orders_iter() {
                orders.push(order.clone());
            }
        }
        // Asks: lowest first.
        for (_, level) in self.asks.iter() {
            for order in level.orders_iter() {
                orders.push(order.clone());
            }
        }

        FullBookSnapshot {
            symbol: self.symbol.clone(),
            sequence: self.sequence,
            orders,
        }
    }

    /// Rebuild the book from a full-fidelity snapshot.
    ///
    /// Clears any existing state and inserts each order in FIFO order
    /// (the snapshot preserves insertion order within each price level).
    pub fn restore_from_snapshot(&mut self, snap: FullBookSnapshot) {
        self.bids.clear();
        self.asks.clear();
        self.order_locations.clear();
        self.user_order_count.clear();
        self.sequence = snap.sequence;

        for order in snap.orders {
            let price = order.price;
            let side = order.side;
            let order_id = order.id;
            let user_id = order.user_id;

            self.order_locations.insert(order_id, (side, price));
            *self.user_order_count.entry(user_id).or_insert(0) += 1;

            let levels = match side {
                Side::Buy => &mut self.bids,
                Side::Sell => &mut self.asks,
            };

            levels
                .entry(price)
                .or_insert_with(crate::price_level::PriceLevel::new)
                .push(order);
        }
    }

    pub fn symbol(&self) -> &Symbol {
        &self.symbol
    }

    pub fn sequence(&self) -> SeqNo {
        self.sequence
    }

    pub fn order_count(&self) -> usize {
        self.order_locations.len()
    }

    /// Return the number of open orders for `user_id` on this symbol.
    ///
    /// O(1): reads from the `user_order_count` secondary index maintained by
    /// `insert_order`, `cancel_order`, and `match_order` (maker-fill path).
    ///
    /// Used by `OrderLimitConfig::max_open_orders_per_user_per_symbol` enforcement
    /// in `order-gateway/src/validator.rs` (Sprint 8, KG-2).
    pub fn user_open_order_count(&self, user_id: UserId) -> usize {
        self.user_order_count.get(&user_id).copied().unwrap_or(0)
    }

    /// Return the number of open orders for `user_id` at a specific price level.
    ///
    /// O(orders_at_level): walks the level's order queue and counts matching
    /// user_ids.  This is acceptable because `max_orders_per_user_per_price_level`
    /// (default 50) caps the worst-case scan cost — the same cost as the T1.5
    /// STP filter in `can_fill_fully`.
    ///
    /// Returns 0 if the price level does not exist.
    pub fn user_orders_at_price(&self, user_id: UserId, side: Side, price: Price) -> usize {
        let book_side = match side {
            Side::Buy => &self.bids,
            Side::Sell => &self.asks,
        };
        book_side
            .get(&price)
            .map(|level| level.orders_iter().filter(|o| o.user_id == user_id).count())
            .unwrap_or(0)
    }

    /// Test-only accessor for the raw cache fields. Bypasses any reconciliation.
    /// Used by proptest to verify cache consistency with the underlying BTreeMap.
    #[cfg(any(test, debug_assertions))]
    pub fn best_bid_cache_raw(&self) -> Option<Price> {
        self.best_bid_cache
    }

    /// Test-only accessor for the raw ask cache field.
    #[cfg(any(test, debug_assertions))]
    pub fn best_ask_cache_raw(&self) -> Option<Price> {
        self.best_ask_cache
    }

    // -------------------------------------------------------------------------
    // Invariant checking
    // -------------------------------------------------------------------------

    /// Full invariant check — exposed for use by property tests and explicit
    /// consistency assertions.
    ///
    /// Checks:
    ///   1. Book not crossed (best_ask >= best_bid when both sides populated).
    ///   2. Per-level `total_qty` equals sum of `remaining_qty` over all orders.
    ///   3. Every `OrderId` in `order_locations` points to an existing order at
    ///      the referenced side/price (O(n) over all live orders).
    ///
    /// Returns `Ok(())` if all invariants hold, `Err(description)` otherwise.
    pub fn check_invariants_public(&self) -> Result<(), String> {
        Self::check_not_crossed(&self.bids, &self.asks)?;
        Self::check_level_qty_consistency(&self.bids, "bid")?;
        Self::check_level_qty_consistency(&self.asks, "ask")?;
        Self::check_location_index_consistent(
            &self.order_locations,
            &self.bids,
            &self.asks,
        )?;
        self.check_user_order_count_consistent()?;
        Ok(())
    }

    /// Lightweight invariant check — called after every mutating operation in
    /// debug builds. Checks only invariant 1 (book not crossed), which is O(1)
    /// using `first_key_value` / `last_key_value` on the BTreeMaps.
    ///
    /// Invariants 2 and 3 are more expensive (O(levels) and O(orders)
    /// respectively) and are reserved for explicit `check_invariants_public`
    /// calls in property tests, where the cost is amortised over many operations.
    ///
    /// Compiles to nothing in release builds.
    #[cfg(debug_assertions)]
    fn check_invariants(&self) {
        if let Err(msg) = Self::check_not_crossed(&self.bids, &self.asks) {
            panic!("OrderBook invariant violated: {msg}");
        }
    }

    // -- helpers --

    fn check_not_crossed(
        bids: &BTreeMap<Price, PriceLevel>,
        asks: &BTreeMap<Price, PriceLevel>,
    ) -> Result<(), String> {
        let best_bid = bids.last_key_value().map(|(&p, _)| p);
        let best_ask = asks.first_key_value().map(|(&p, _)| p);
        if let (Some(bid), Some(ask)) = (best_bid, best_ask) {
            if ask < bid {
                return Err(format!("crossed book: best_ask={ask} < best_bid={bid}"));
            }
        }
        Ok(())
    }

    fn check_level_qty_consistency(
        side: &BTreeMap<Price, PriceLevel>,
        label: &str,
    ) -> Result<(), String> {
        for (&price, level) in side {
            let computed: Qty = level.orders_iter().map(|o| o.remaining_qty()).sum();
            if computed != level.total_qty() {
                return Err(format!(
                    "{label} level {price}: total_qty={} but sum of remaining_qty={computed}",
                    level.total_qty()
                ));
            }
            // order_count() must equal the iterator count — both derived from
            // the same VecDeque, so this is a consistency self-check.
            let iter_count = level.orders_iter().count();
            if level.order_count() != iter_count {
                return Err(format!(
                    "{label} level {price}: order_count()={} but orders_iter().count()={iter_count}",
                    level.order_count()
                ));
            }
        }
        Ok(())
    }

    fn check_location_index_consistent(
        locations: &FxHashMap<OrderId, (Side, Price)>,
        bids: &BTreeMap<Price, PriceLevel>,
        asks: &BTreeMap<Price, PriceLevel>,
    ) -> Result<(), String> {
        // Forward pass: every entry in order_locations must point to a real order.
        for (order_id, (side, price)) in locations {
            let book_side = match side {
                Side::Buy => bids,
                Side::Sell => asks,
            };
            let level = match book_side.get(price) {
                Some(l) => l,
                None => {
                    return Err(format!(
                        "order_locations contains order {order_id} at {side:?} {price} \
                         but that price level does not exist"
                    ));
                }
            };
            if !level.orders_iter().any(|o| o.id == *order_id) {
                return Err(format!(
                    "order_locations contains order {order_id} at {side:?} {price} \
                     but order is not present in that level"
                ));
            }
        }

        // Reverse pass: every order in every price level must have a matching
        // order_locations entry with the correct (Side, Price).
        // This catches orphaned orders that exist on the book but lack an index entry.
        for (&price, level) in bids {
            for order in level.orders_iter() {
                match locations.get(&order.id) {
                    None => {
                        return Err(format!(
                            "bid order {} at price {price} is on the book \
                             but has no order_locations entry",
                            order.id
                        ));
                    }
                    Some((side, loc_price)) => {
                        if *side != Side::Buy || *loc_price != price {
                            return Err(format!(
                                "bid order {} at price {price}: order_locations says \
                                 ({side:?}, {loc_price}) — mismatch",
                                order.id
                            ));
                        }
                    }
                }
            }
        }
        for (&price, level) in asks {
            for order in level.orders_iter() {
                match locations.get(&order.id) {
                    None => {
                        return Err(format!(
                            "ask order {} at price {price} is on the book \
                             but has no order_locations entry",
                            order.id
                        ));
                    }
                    Some((side, loc_price)) => {
                        if *side != Side::Sell || *loc_price != price {
                            return Err(format!(
                                "ask order {} at price {price}: order_locations says \
                                 ({side:?}, {loc_price}) — mismatch",
                                order.id
                            ));
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Verify that `user_order_count` is consistent with the actual orders on
    /// the book (bid + ask sides combined).
    ///
    /// Recomputes per-user counts from scratch and compares to the cached index.
    /// O(total_orders). Called by `check_invariants_public` in property tests.
    fn check_user_order_count_consistent(&self) -> Result<(), String> {
        let mut expected: FxHashMap<UserId, usize> = FxHashMap::default();
        for level in self.bids.values().chain(self.asks.values()) {
            for order in level.orders_iter() {
                *expected.entry(order.user_id).or_insert(0) += 1;
            }
        }
        // Only non-zero entries need to match; entries with 0 in user_order_count
        // are allowed (they are never cleaned up eagerly to keep the hot path simple).
        for (user_id, &exp_count) in &expected {
            let actual = self.user_order_count.get(user_id).copied().unwrap_or(0);
            if actual != exp_count {
                return Err(format!(
                    "user_order_count[{user_id}] = {actual} but book has {exp_count} open orders"
                ));
            }
        }
        // Also check that no user_order_count entry exceeds what the book holds.
        for (user_id, &actual) in &self.user_order_count {
            let exp_count = expected.get(user_id).copied().unwrap_or(0);
            if actual > exp_count {
                return Err(format!(
                    "user_order_count[{user_id}] = {actual} but book only has {exp_count} open orders"
                ));
            }
        }
        Ok(())
    }
}

/// L2 order book snapshot.
#[derive(Debug, Clone)]
pub struct OrderBookSnapshot {
    pub symbol: Symbol,
    pub bids: Vec<(Price, Qty)>,
    pub asks: Vec<(Price, Qty)>,
    pub sequence: SeqNo,
}

/// Full-fidelity snapshot: every individual resting order, preserving
/// price-time priority (FIFO within each price level).
///
/// This is the persistence format for engine state snapshots. Unlike
/// `OrderBookSnapshot` (which is L2/aggregated), this contains enough
/// information to fully reconstruct the book without WAL replay.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FullBookSnapshot {
    pub symbol: Symbol,
    pub sequence: SeqNo,
    pub orders: Vec<Order>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_limit_order(
        side: Side,
        price: Decimal,
        qty: Decimal,
    ) -> PlaceOrderCommand {
        PlaceOrderCommand {
            order_id: uuid::Uuid::new_v4(),
            user_id: uuid::Uuid::new_v4(),
            symbol: Symbol::new("BTCUSDT"),
            side,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
            price,
            qty,
            reduce_only: false,
            timestamp: chrono::Utc::now(),
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

    fn make_market_order(side: Side, qty: Decimal) -> PlaceOrderCommand {
        PlaceOrderCommand {
            order_id: uuid::Uuid::new_v4(),
            user_id: uuid::Uuid::new_v4(),
            symbol: Symbol::new("BTCUSDT"),
            side,
            order_type: OrderType::Market,
            time_in_force: TimeInForce::Gtc,
            price: Decimal::ZERO,
            qty,
            reduce_only: false,
            timestamp: chrono::Utc::now(),
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
    fn test_limit_order_rests_on_book() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));

        let events = book.place_order(make_limit_order(Side::Buy, dec!(50000), dec!(1)));

        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], MatchEvent::OrderAccepted { .. }));
        assert_eq!(book.best_bid(), Some((dec!(50000), dec!(1))));
        assert_eq!(book.order_count(), 1);
    }

    #[test]
    fn test_limit_orders_match_at_maker_price() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));

        // Maker: sell at 50000
        book.place_order(make_limit_order(Side::Sell, dec!(50000), dec!(1)));

        // Taker: buy at 50100 (willing to pay more)
        let events = book.place_order(make_limit_order(Side::Buy, dec!(50100), dec!(1)));

        // Should fill at MAKER's price (50000), not taker's (50100)
        let fill = events.iter().find_map(|e| match e {
            MatchEvent::Fill(f) => Some(f),
            _ => None,
        });

        assert!(fill.is_some());
        assert_eq!(fill.unwrap().price, dec!(50000)); // Maker's price!
        assert_eq!(book.order_count(), 0); // Both fully filled
    }

    #[test]
    fn test_price_time_priority() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));

        let user_first = uuid::Uuid::new_v4();
        let user_second = uuid::Uuid::new_v4();

        // First order at 50000
        let mut cmd1 = make_limit_order(Side::Sell, dec!(50000), dec!(1));
        cmd1.user_id = user_first;
        book.place_order(cmd1);

        // Second order at same price
        let mut cmd2 = make_limit_order(Side::Sell, dec!(50000), dec!(1));
        cmd2.user_id = user_second;
        book.place_order(cmd2);

        // Buy 1 — should match the FIRST order (FIFO)
        let events = book.place_order(make_market_order(Side::Buy, dec!(1)));

        let fill = events.iter().find_map(|e| match e {
            MatchEvent::Fill(f) => Some(f),
            _ => None,
        });

        assert!(fill.is_some());
        assert_eq!(fill.unwrap().maker_user_id, user_first); // FIFO!
        assert_eq!(book.order_count(), 1); // Second order still on book
    }

    #[test]
    fn test_market_order_remainder_cancelled() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));

        // Only 0.5 BTC available
        book.place_order(make_limit_order(Side::Sell, dec!(50000), dec!(0.5)));

        // Try to buy 1 BTC
        let events = book.place_order(make_market_order(Side::Buy, dec!(1)));

        // Should get partial fill (0.5) + cancel remainder (0.5)
        let fills: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                MatchEvent::Fill(f) => Some(f),
                _ => None,
            })
            .collect();

        let cancels: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                MatchEvent::OrderCancelled { remaining_qty, .. } => Some(remaining_qty),
                _ => None,
            })
            .collect();

        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].qty, dec!(0.5));
        assert_eq!(cancels.len(), 1);
        assert_eq!(*cancels[0], dec!(0.5)); // Remainder cancelled
    }

    #[test]
    fn test_partial_fill_across_levels() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));

        // Sell side: 0.5 @ 50000, 0.3 @ 50100, 0.2 @ 50200
        book.place_order(make_limit_order(Side::Sell, dec!(50000), dec!(0.5)));
        book.place_order(make_limit_order(Side::Sell, dec!(50100), dec!(0.3)));
        book.place_order(make_limit_order(Side::Sell, dec!(50200), dec!(0.2)));

        // Buy 1 BTC market order — should sweep all 3 levels
        let events = book.place_order(make_market_order(Side::Buy, dec!(1)));

        let fills: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                MatchEvent::Fill(f) => Some(f),
                _ => None,
            })
            .collect();

        assert_eq!(fills.len(), 3);
        assert_eq!(fills[0].price, dec!(50000)); // Best price first
        assert_eq!(fills[0].qty, dec!(0.5));
        assert_eq!(fills[1].price, dec!(50100));
        assert_eq!(fills[1].qty, dec!(0.3));
        assert_eq!(fills[2].price, dec!(50200));
        assert_eq!(fills[2].qty, dec!(0.2));
        assert_eq!(book.order_count(), 0);
    }

    #[test]
    fn test_post_only_rejected_if_would_take() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));

        // Resting sell at 50000
        book.place_order(make_limit_order(Side::Sell, dec!(50000), dec!(1)));

        // PostOnly buy at 50000 — would take, should be rejected
        let mut cmd = make_limit_order(Side::Buy, dec!(50000), dec!(1));
        cmd.time_in_force = TimeInForce::PostOnly;
        let events = book.place_order(cmd);

        assert!(matches!(events[0], MatchEvent::OrderRejected { .. }));
        assert_eq!(book.order_count(), 1); // Original sell still there
    }

    #[test]
    fn test_fok_rejected_if_cannot_fill() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));

        // Only 0.5 available
        book.place_order(make_limit_order(Side::Sell, dec!(50000), dec!(0.5)));

        // FOK buy for 1.0 — can't fill fully, reject
        let mut cmd = make_limit_order(Side::Buy, dec!(50000), dec!(1));
        cmd.time_in_force = TimeInForce::Fok;
        let events = book.place_order(cmd);

        assert!(matches!(events[0], MatchEvent::OrderRejected { .. }));
        assert_eq!(book.order_count(), 1); // Nothing changed
    }

    #[test]
    fn test_ioc_cancels_remainder() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));

        book.place_order(make_limit_order(Side::Sell, dec!(50000), dec!(0.5)));

        // IOC buy for 1.0 — fill 0.5, cancel 0.5
        let mut cmd = make_limit_order(Side::Buy, dec!(50000), dec!(1));
        cmd.time_in_force = TimeInForce::Ioc;
        let events = book.place_order(cmd);

        assert!(events.iter().any(|e| matches!(e, MatchEvent::Fill(_))));
        assert!(events
            .iter()
            .any(|e| matches!(e, MatchEvent::OrderCancelled { .. })));
        assert_eq!(book.order_count(), 0); // Sell filled, IOC remainder cancelled
    }

    #[test]
    fn test_cancel_order() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));

        let cmd = make_limit_order(Side::Buy, dec!(50000), dec!(1));
        let order_id = cmd.order_id;
        let user_id = cmd.user_id;
        book.place_order(cmd);

        assert_eq!(book.order_count(), 1);

        let events = book.cancel_order(CancelOrderCommand {
            order_id,
            user_id,
            symbol: Symbol::new("BTCUSDT"),
            timestamp: chrono::Utc::now(),
        });

        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], MatchEvent::OrderCancelled { .. }));
        assert_eq!(book.order_count(), 0);
    }

    #[test]
    fn test_cancel_nonexistent_is_silent() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));

        let events = book.cancel_order(CancelOrderCommand {
            order_id: uuid::Uuid::new_v4(),
            user_id: uuid::Uuid::new_v4(),
            symbol: Symbol::new("BTCUSDT"),
            timestamp: chrono::Utc::now(),
        });

        assert!(events.is_empty()); // Silent ignore
    }

    #[test]
    fn test_spread_and_mid_price() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));

        book.place_order(make_limit_order(Side::Buy, dec!(49900), dec!(1)));
        book.place_order(make_limit_order(Side::Sell, dec!(50100), dec!(1)));

        assert_eq!(book.spread(), Some(dec!(200)));
        assert_eq!(book.mid_price(), Some(dec!(50000)));
    }

    #[test]
    fn test_l2_snapshot() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));

        book.place_order(make_limit_order(Side::Buy, dec!(49900), dec!(1)));
        book.place_order(make_limit_order(Side::Buy, dec!(49800), dec!(2)));
        book.place_order(make_limit_order(Side::Sell, dec!(50100), dec!(0.5)));

        let snap = book.snapshot(10);

        assert_eq!(snap.bids.len(), 2);
        assert_eq!(snap.bids[0], (dec!(49900), dec!(1))); // Best bid first
        assert_eq!(snap.asks.len(), 1);
        assert_eq!(snap.asks[0], (dec!(50100), dec!(0.5)));
    }

    // ── T1.5: FOK + STP correctness ────────────────────────────────────────────
    //
    // Bug scenario: can_fill_fully counted total_qty including self-orders. When
    // STP is active those self-orders are skipped during real matching → the FOK
    // check would pass but the order would only partially fill, violating FOK.
    // Fix: can_fill_fully receives taker_user_id and excludes self-orders when
    // STP is active.

    fn make_fok_order_with_stp(
        user_id: uuid::Uuid,
        side: Side,
        price: Decimal,
        qty: Decimal,
        stp_mode: StpMode,
    ) -> PlaceOrderCommand {
        PlaceOrderCommand {
            order_id: uuid::Uuid::new_v4(),
            user_id,
            symbol: Symbol::new("BTCUSDT"),
            side,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Fok,
            price,
            qty,
            reduce_only: false,
            timestamp: chrono::Utc::now(),
            expire_at: None,
            position_idx: 0,
            iceberg_qty: None,
            order_link_id: None,
            slippage_tolerance: None,
            close_position: false,
            stp_mode,
            request_expire_time: None,
        }
    }

    /// FOK from user A against a book that only has self-orders from user A.
    /// With STP active, no non-self liquidity → FOK must be rejected.
    #[test]
    fn test_fok_stp_rejects_when_only_self_orders() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));
        let user_a = uuid::Uuid::from_u128(0xAAAA);

        // Two sell orders at $50000 from user A, qty 1 each (total 2).
        let mut sell1 = make_limit_order(Side::Sell, dec!(50000), dec!(1));
        sell1.user_id = user_a;
        let mut sell2 = make_limit_order(Side::Sell, dec!(50000), dec!(1));
        sell2.user_id = user_a;
        book.place_order(sell1);
        book.place_order(sell2);
        assert_eq!(book.order_count(), 2);

        // FOK buy from user A for qty 2 with STP active.
        // Total available = 2, but all 2 belong to user A.
        // After STP exclusion: 0 available → FOK must be rejected.
        let fok = make_fok_order_with_stp(
            user_a,
            Side::Buy,
            dec!(50000),
            dec!(2),
            StpMode::ExpireTaker,
        );
        let events = book.place_order(fok);

        // Must be a single OrderRejected event.
        assert_eq!(events.len(), 1, "expected exactly 1 event, got: {events:?}");
        assert!(
            matches!(events[0], MatchEvent::OrderRejected { .. }),
            "expected OrderRejected, got: {:?}",
            events[0]
        );
        if let MatchEvent::OrderRejected { ref reason, .. } = events[0] {
            assert!(
                reason.contains("FOK"),
                "rejection reason should mention FOK, got: {reason}"
            );
        }

        // Book unchanged — 2 self-orders still resting.
        assert_eq!(book.order_count(), 2);
    }

    /// FOK happy path: sufficient non-self liquidity → order fills fully.
    #[test]
    fn test_fok_stp_fills_against_other_user() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));
        let user_a = uuid::Uuid::from_u128(0xAAAA);
        let user_b = uuid::Uuid::from_u128(0xBBBB);

        // User B provides 2 units of sell liquidity at $50000.
        let mut sell = make_limit_order(Side::Sell, dec!(50000), dec!(2));
        sell.user_id = user_b;
        book.place_order(sell);

        // FOK buy from user A for qty 2 with STP active — should fill fully.
        let fok = make_fok_order_with_stp(
            user_a,
            Side::Buy,
            dec!(50000),
            dec!(2),
            StpMode::ExpireTaker,
        );
        let events = book.place_order(fok);

        let fills: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                MatchEvent::Fill(f) => Some(f),
                _ => None,
            })
            .collect();

        assert_eq!(fills.len(), 1, "expected 1 fill, got: {events:?}");
        assert_eq!(fills[0].qty, dec!(2));
        assert_eq!(book.order_count(), 0);
    }

    /// FOK with STP=None: self-orders count as available liquidity.
    /// The order fills against its own resting orders (STP not active).
    #[test]
    fn test_fok_no_stp_fills_against_self() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));
        let user_a = uuid::Uuid::from_u128(0xAAAA);

        // Two self-sell orders at $50000 from user A.
        let mut sell1 = make_limit_order(Side::Sell, dec!(50000), dec!(1));
        sell1.user_id = user_a;
        let mut sell2 = make_limit_order(Side::Sell, dec!(50000), dec!(1));
        sell2.user_id = user_a;
        book.place_order(sell1);
        book.place_order(sell2);

        // FOK buy from user A with StpMode::None — self-fill allowed.
        let fok = make_fok_order_with_stp(
            user_a,
            Side::Buy,
            dec!(50000),
            dec!(2),
            StpMode::None,
        );
        let events = book.place_order(fok);

        let fills: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                MatchEvent::Fill(f) => Some(f),
                _ => None,
            })
            .collect();

        // Both self-orders fill — STP=None means no protection.
        assert_eq!(fills.len(), 2, "expected 2 fills, got: {events:?}");
        assert_eq!(fills[0].qty + fills[1].qty, dec!(2));
        assert_eq!(book.order_count(), 0);
    }

    /// FOK with mixed book (some self, some other): only non-self qty counts.
    /// User A has 1 unit resting + user B has 1 unit. FOK for qty 2 with STP:
    /// only 1 non-self unit available → rejected.
    #[test]
    fn test_fok_stp_mixed_book_insufficient_non_self() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));
        let user_a = uuid::Uuid::from_u128(0xAAAA);
        let user_b = uuid::Uuid::from_u128(0xBBBB);

        let mut self_sell = make_limit_order(Side::Sell, dec!(50000), dec!(1));
        self_sell.user_id = user_a;
        book.place_order(self_sell);

        let mut other_sell = make_limit_order(Side::Sell, dec!(50000), dec!(1));
        other_sell.user_id = user_b;
        book.place_order(other_sell);

        // FOK buy for qty 2 with STP: non-self available = 1, need 2 → reject.
        let fok = make_fok_order_with_stp(
            user_a,
            Side::Buy,
            dec!(50000),
            dec!(2),
            StpMode::ExpireTaker,
        );
        let events = book.place_order(fok);

        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], MatchEvent::OrderRejected { .. }));
        // Both resting orders must still be on the book.
        assert_eq!(book.order_count(), 2);
    }

    /// FOK with mixed book (some self, some other): exactly enough non-self qty → fills.
    /// User A has 1 unit resting + user B has 1 unit. FOK for qty 1 with STP:
    /// 1 non-self unit available → fills; user A's resting order untouched.
    #[test]
    fn test_fok_stp_mixed_book_sufficient_non_self() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));
        let user_a = uuid::Uuid::from_u128(0xAAAA);
        let user_b = uuid::Uuid::from_u128(0xBBBB);

        // User B's resting sell — placed FIRST so it is at the front of the FIFO
        // queue and will be matched first by match_order (which does not skip
        // self-orders; STP enforcement here is only at the FOK pre-check level).
        let mut other_sell = make_limit_order(Side::Sell, dec!(50000), dec!(1));
        other_sell.user_id = user_b;
        let other_sell_id = other_sell.order_id;
        book.place_order(other_sell);

        // User A's resting sell (self-order) — placed SECOND, behind user B in queue.
        let mut self_sell = make_limit_order(Side::Sell, dec!(50000), dec!(1));
        self_sell.user_id = user_a;
        let self_sell_id = self_sell.order_id;
        book.place_order(self_sell);

        assert_eq!(book.order_count(), 2);

        // FOK buy from user A for qty 1 with STP active.
        // Non-self available = 1 (user B), need 1 → can_fill_fully returns true → fills.
        // The actual fill hits user B's order (front of queue).
        let fok = make_fok_order_with_stp(
            user_a,
            Side::Buy,
            dec!(50000),
            dec!(1),
            StpMode::ExpireTaker,
        );
        let events = book.place_order(fok);

        let fills: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                MatchEvent::Fill(f) => Some(f),
                _ => None,
            })
            .collect();

        assert_eq!(fills.len(), 1, "expected 1 fill, got: {events:?}");
        assert_eq!(fills[0].qty, dec!(1));
        assert_eq!(fills[0].maker_order_id, other_sell_id, "fill must be against user B (front of queue)");

        // User A's resting self-order must still be on the book.
        assert_eq!(book.order_count(), 1);
        // Verify the remaining order is user A's self-order (cancel it and check id).
        let cancel_events = book.cancel_order(CancelOrderCommand {
            order_id: self_sell_id,
            user_id: user_a,
            symbol: Symbol::new("BTCUSDT"),
            timestamp: chrono::Utc::now(),
        });
        assert_eq!(cancel_events.len(), 1, "self-order should cancel cleanly");
        assert_eq!(book.order_count(), 0);
    }

    // ── Item 4: ExpireMaker and ExpireBoth coverage ────────────────────────────
    //
    // The T1.5 fix treats all stp_mode != None identically in can_fill_fully.
    // These tests verify that ExpireMaker and ExpireBoth are also correctly
    // rejected when the only resting liquidity belongs to the taker themselves.

    /// FOK from user A against only-self book with StpMode::ExpireMaker → rejected.
    #[test]
    fn test_fok_stp_rejects_when_only_self_orders_expire_maker() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));
        let user_a = uuid::Uuid::from_u128(0xAAAA);

        let mut sell = make_limit_order(Side::Sell, dec!(50000), dec!(1));
        sell.user_id = user_a;
        book.place_order(sell);

        let fok = make_fok_order_with_stp(
            user_a,
            Side::Buy,
            dec!(50000),
            dec!(1),
            StpMode::ExpireMaker,
        );
        let events = book.place_order(fok);

        assert_eq!(events.len(), 1, "expected exactly 1 event, got: {events:?}");
        assert!(
            matches!(events[0], MatchEvent::OrderRejected { .. }),
            "expected OrderRejected, got: {:?}",
            events[0]
        );
        assert_eq!(book.order_count(), 1);
    }

    /// Sprint 3 T1.6b deterministic coverage: multi-level sweep that
    /// fully drains several top levels, then partially fills the final
    /// remaining level. Verifies the `level_removed` gating deviation from
    /// the architect's unconditional-update spec: cache must equal the
    /// partially-drained level's price (not the drained ones above it) at
    /// the end of the sweep. This is the exact transition that proptest is
    /// unlikely to sample with high confidence at 10K iterations.
    #[test]
    fn test_top_of_book_cache_multi_level_sweep_partial_final() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));

        // Build ask book: 4 distinct levels ascending, 1 unit each at the
        // first three levels, 5 units at the fourth.
        book.place_order(make_limit_order(Side::Sell, dec!(50000), dec!(1)));
        book.place_order(make_limit_order(Side::Sell, dec!(50001), dec!(1)));
        book.place_order(make_limit_order(Side::Sell, dec!(50002), dec!(1)));
        book.place_order(make_limit_order(Side::Sell, dec!(50003), dec!(5)));

        // Initial cache state — best ask is the lowest (50000).
        assert_eq!(book.best_ask_cache_raw(), Some(dec!(50000)));
        assert_eq!(book.best_bid_cache_raw(), None);

        // Market buy for qty=4: drains levels 50000, 50001, 50002 entirely
        // (entry.remove fires 3 times → cache update fires 3 times), then
        // partially drains level 50003 (1 of 5 → level NOT removed → NO
        // cache update on the final iteration).
        let events = book.place_order(make_market_order(Side::Buy, dec!(4)));

        // 4 fills total (1+1+1+1 across the 4 levels).
        let fill_count = events
            .iter()
            .filter(|e| matches!(e, MatchEvent::Fill(_)))
            .count();
        assert_eq!(fill_count, 4, "expected 4 fills, got {events:?}");

        // Cache must point to the final level that still has residual qty.
        // If the `level_removed` gating were buggy and cache updates were
        // skipped inappropriately, the cache could still point to 50000,
        // 50001, or 50002 — all of which are now gone from the BTreeMap.
        assert_eq!(
            book.best_ask_cache_raw(),
            Some(dec!(50003)),
            "cache must equal price of the partially-drained final level"
        );

        // And the BTreeMap truth must match the cache.
        assert_eq!(
            book.asks.first_key_value().map(|(p, _)| *p),
            Some(dec!(50003))
        );

        // Final level has 4 units remaining (5 - 1 filled).
        assert_eq!(book.asks.get(&dec!(50003)).unwrap().total_qty(), dec!(4));

        // Full invariant check passes.
        book.check_invariants_public()
            .expect("invariants must hold after partial multi-level sweep");
    }

    /// FOK from user A against only-self book with StpMode::ExpireBoth → rejected.
    #[test]
    fn test_user_order_count_partial_then_full_fill() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));

        let user_a = uuid::Uuid::from_u128(0xAAAA);
        let user_b = uuid::Uuid::from_u128(0xBBBB);

        // User A places a limit sell at $50000 qty=5.
        let mut sell = make_limit_order(Side::Sell, dec!(50000), dec!(5));
        sell.user_id = user_a;
        book.place_order(sell);

        assert_eq!(book.user_open_order_count(user_a), 1);

        // User B buys qty=3 — partially fills A's order.
        let mut buy_partial = make_market_order(Side::Buy, dec!(3));
        buy_partial.user_id = user_b;
        let events = book.place_order(buy_partial);

        // A fill must have occurred.
        assert!(
            events.iter().any(|e| matches!(e, MatchEvent::Fill(_))),
            "expected a fill event, got: {events:?}"
        );

        // A's order is still open (partially filled) so count must remain 1.
        assert_eq!(
            book.user_open_order_count(user_a),
            1,
            "user_order_count should stay 1 after a partial fill"
        );
        // Best ask still exists with reduced qty.
        assert_eq!(
            book.best_ask(),
            Some((dec!(50000), dec!(2))),
            "ask should have qty=2 remaining after partial fill"
        );

        // User B buys the remaining qty=2 — fully drains A's order.
        let mut buy_rest = make_market_order(Side::Buy, dec!(2));
        buy_rest.user_id = user_b;
        let events2 = book.place_order(buy_rest);

        assert!(
            events2.iter().any(|e| matches!(e, MatchEvent::Fill(_))),
            "expected a fill event on second buy, got: {events2:?}"
        );

        // A's order is now gone — count must drop to 0.
        assert_eq!(
            book.user_open_order_count(user_a),
            0,
            "user_order_count should be 0 after order fully filled"
        );
        assert_eq!(book.order_count(), 0, "book should be empty");
    }

    #[test]
    fn test_fok_stp_rejects_when_only_self_orders_expire_both() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));
        let user_a = uuid::Uuid::from_u128(0xAAAA);

        let mut sell = make_limit_order(Side::Sell, dec!(50000), dec!(1));
        sell.user_id = user_a;
        book.place_order(sell);

        let fok = make_fok_order_with_stp(
            user_a,
            Side::Buy,
            dec!(50000),
            dec!(1),
            StpMode::ExpireBoth,
        );
        let events = book.place_order(fok);

        assert_eq!(events.len(), 1, "expected exactly 1 event, got: {events:?}");
        assert!(
            matches!(events[0], MatchEvent::OrderRejected { .. }),
            "expected OrderRejected, got: {:?}",
            events[0]
        );
        assert_eq!(book.order_count(), 1);
    }

    // ── KG-1.5: Sprint 7 — STP enforcement in match_order ─────────────────────
    //
    // These tests verify STP behavior across all modes (None, ExpireTaker,
    // ExpireMaker, ExpireBoth) for GTC, IOC, Market, and GTD orders.
    //
    // Reference: Bybit API Spec v5 "Self-Trade Prevention (STP) Mode";
    // Binance "Self-Trade Prevention Mode".

    fn make_stp_order(
        user_id: uuid::Uuid,
        side: Side,
        order_type: OrderType,
        tif: TimeInForce,
        price: Decimal,
        qty: Decimal,
        stp_mode: StpMode,
    ) -> PlaceOrderCommand {
        PlaceOrderCommand {
            order_id: uuid::Uuid::new_v4(),
            user_id,
            symbol: Symbol::new("BTCUSDT"),
            side,
            order_type,
            time_in_force: tif,
            price,
            qty,
            reduce_only: false,
            timestamp: chrono::Utc::now(),
            expire_at: None,
            position_idx: 0,
            iceberg_qty: None,
            order_link_id: None,
            slippage_tolerance: None,
            close_position: false,
            stp_mode,
            request_expire_time: None,
        }
    }

    /// STP=None allows self-fill: GTC buy from user A against GTC sell from user A
    /// with stp_mode=None → fills normally with no cancel event.
    #[test]
    fn test_stp_none_allows_self_fill() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));
        let user_a = uuid::Uuid::from_u128(0xAAAA);

        let mut sell = make_limit_order(Side::Sell, dec!(50000), dec!(1));
        sell.user_id = user_a;
        book.place_order(sell);

        let mut buy = make_stp_order(
            user_a,
            Side::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            dec!(50000),
            dec!(1),
            StpMode::None,
        );
        buy.user_id = user_a;
        let events = book.place_order(buy);

        let fills: Vec<_> = events.iter().filter(|e| matches!(e, MatchEvent::Fill(_))).collect();
        let cancels: Vec<_> = events.iter().filter(|e| matches!(e, MatchEvent::OrderCancelled { .. })).collect();

        assert_eq!(fills.len(), 1, "self-fill must occur with STP=None: {events:?}");
        assert_eq!(cancels.len(), 0, "no cancel with STP=None: {events:?}");
        assert_eq!(book.order_count(), 0);
        book.check_invariants_public().expect("invariants after STP=None self-fill");
    }

    /// ExpireTaker: incoming taker is cancelled, resting maker is untouched.
    #[test]
    fn test_stp_expire_taker_cancels_incoming() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));
        let user_a = uuid::Uuid::from_u128(0xAAAA);

        let maker_id = uuid::Uuid::new_v4();
        let mut sell = make_limit_order(Side::Sell, dec!(50000), dec!(1));
        sell.user_id = user_a;
        sell.order_id = maker_id;
        book.place_order(sell);

        let taker_id = uuid::Uuid::new_v4();
        let mut buy = make_stp_order(
            user_a,
            Side::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            dec!(50000),
            dec!(1),
            StpMode::ExpireTaker,
        );
        buy.order_id = taker_id;
        let events = book.place_order(buy);

        let fills: Vec<_> = events.iter().filter(|e| matches!(e, MatchEvent::Fill(_))).collect();
        let cancels: Vec<_> = events.iter().filter_map(|e| match e {
            MatchEvent::OrderCancelled { order_id, .. } => Some(order_id),
            _ => None,
        }).collect();

        assert_eq!(fills.len(), 0, "no fill on ExpireTaker: {events:?}");
        assert_eq!(cancels.len(), 1, "exactly one cancel on ExpireTaker: {events:?}");
        assert_eq!(*cancels[0], taker_id, "cancel must be for the taker");
        // Maker is still on the book.
        assert_eq!(book.order_count(), 1);
        book.check_invariants_public().expect("invariants after ExpireTaker");
    }

    /// ExpireMaker: resting maker is cancelled, taker finds no further liquidity
    /// (maker was the only order) so taker goes on the book (GTC).
    #[test]
    fn test_stp_expire_maker_cancels_resting() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));
        let user_a = uuid::Uuid::from_u128(0xAAAA);

        let maker_id = uuid::Uuid::new_v4();
        let mut sell = make_limit_order(Side::Sell, dec!(50000), dec!(1));
        sell.user_id = user_a;
        sell.order_id = maker_id;
        book.place_order(sell);

        let taker_id = uuid::Uuid::new_v4();
        let mut buy = make_stp_order(
            user_a,
            Side::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            dec!(50000),
            dec!(1),
            StpMode::ExpireMaker,
        );
        buy.order_id = taker_id;
        let events = book.place_order(buy);

        let fills: Vec<_> = events.iter().filter(|e| matches!(e, MatchEvent::Fill(_))).collect();
        let cancels: Vec<_> = events.iter().filter_map(|e| match e {
            MatchEvent::OrderCancelled { order_id, .. } => Some(*order_id),
            _ => None,
        }).collect();
        let accepted: Vec<_> = events.iter().filter_map(|e| match e {
            MatchEvent::OrderAccepted { order_id, .. } => Some(*order_id),
            _ => None,
        }).collect();

        assert_eq!(fills.len(), 0, "no fill on ExpireMaker: {events:?}");
        assert_eq!(cancels.len(), 1, "exactly one cancel for the maker: {events:?}");
        assert_eq!(cancels[0], maker_id, "cancel must be for the maker");
        // Taker rests on the book (GTC, no opposite liquidity after maker cancel).
        assert_eq!(accepted.len(), 1, "taker should be accepted onto book: {events:?}");
        assert_eq!(accepted[0], taker_id);
        assert_eq!(book.order_count(), 1, "taker rests on book after maker cancel");
        book.check_invariants_public().expect("invariants after ExpireMaker");
    }

    /// ExpireBoth: both maker and taker are cancelled, nothing on the book.
    #[test]
    fn test_stp_expire_both_cancels_both() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));
        let user_a = uuid::Uuid::from_u128(0xAAAA);

        let maker_id = uuid::Uuid::new_v4();
        let mut sell = make_limit_order(Side::Sell, dec!(50000), dec!(1));
        sell.user_id = user_a;
        sell.order_id = maker_id;
        book.place_order(sell);

        let taker_id = uuid::Uuid::new_v4();
        let mut buy = make_stp_order(
            user_a,
            Side::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            dec!(50000),
            dec!(1),
            StpMode::ExpireBoth,
        );
        buy.order_id = taker_id;
        let events = book.place_order(buy);

        let fills: Vec<_> = events.iter().filter(|e| matches!(e, MatchEvent::Fill(_))).collect();
        let cancelled_ids: Vec<uuid::Uuid> = events.iter().filter_map(|e| match e {
            MatchEvent::OrderCancelled { order_id, .. } => Some(*order_id),
            _ => None,
        }).collect();

        assert_eq!(fills.len(), 0, "no fill on ExpireBoth: {events:?}");
        assert_eq!(cancelled_ids.len(), 2, "both maker and taker cancelled: {events:?}");
        assert!(cancelled_ids.contains(&maker_id), "maker cancel missing: {events:?}");
        assert!(cancelled_ids.contains(&taker_id), "taker cancel missing: {events:?}");
        assert_eq!(book.order_count(), 0, "book empty after ExpireBoth");
        book.check_invariants_public().expect("invariants after ExpireBoth");
    }

    /// ExpireTaker partial fill preservation: taker sweeps user B on level 1,
    /// then hits own order on level 2 → taker cancelled with remaining_qty=3,
    /// status PartiallyFilled.
    #[test]
    fn test_stp_expire_taker_preserves_prior_fills() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));
        let user_a = uuid::Uuid::from_u128(0xAAAA);
        let user_b = uuid::Uuid::from_u128(0xBBBB);

        // Level 1: user B, qty=2 @ 50000
        let mut sell_b = make_limit_order(Side::Sell, dec!(50000), dec!(2));
        sell_b.user_id = user_b;
        book.place_order(sell_b);

        // Level 2: user A (self), qty=3 @ 50001
        let self_maker_id = uuid::Uuid::new_v4();
        let mut sell_a = make_limit_order(Side::Sell, dec!(50001), dec!(3));
        sell_a.user_id = user_a;
        sell_a.order_id = self_maker_id;
        book.place_order(sell_a);

        // Taker: user A buys qty=5 with ExpireTaker
        let taker_id = uuid::Uuid::new_v4();
        let mut buy = make_stp_order(
            user_a,
            Side::Buy,
            OrderType::Market,
            TimeInForce::Gtc,
            dec!(0),
            dec!(5),
            StpMode::ExpireTaker,
        );
        buy.order_id = taker_id;
        let events = book.place_order(buy);

        let fills: Vec<_> = events.iter().filter_map(|e| match e {
            MatchEvent::Fill(f) => Some(f),
            _ => None,
        }).collect();
        let cancels: Vec<_> = events.iter().filter_map(|e| match e {
            MatchEvent::OrderCancelled { order_id, remaining_qty, .. } => Some((*order_id, *remaining_qty)),
            _ => None,
        }).collect();

        // Should fill qty=2 against user B on level 1.
        assert_eq!(fills.len(), 1, "expected 1 fill against user B: {events:?}");
        assert_eq!(fills[0].qty, dec!(2));
        assert_eq!(fills[0].maker_user_id, user_b);

        // Should cancel taker with remaining_qty=3 (5-2).
        assert_eq!(cancels.len(), 1, "expected 1 cancel for taker: {events:?}");
        assert_eq!(cancels[0].0, taker_id, "cancel must be for taker");
        assert_eq!(cancels[0].1, dec!(3), "remaining_qty must be 3");

        // Fill seq must precede cancel seq — monotonicity invariant.
        let fill_seq = events.iter().find_map(|e| match e {
            MatchEvent::Fill(f) => Some(f.seq),
            _ => None,
        }).expect("expected fill event");
        let cancel_seq = events.iter().find_map(|e| match e {
            MatchEvent::OrderCancelled { seq, .. } => Some(*seq),
            _ => None,
        }).expect("expected cancel event");
        assert!(
            fill_seq < cancel_seq,
            "fill seq ({fill_seq}) must precede STP cancel seq ({cancel_seq}) — monotonicity invariant",
        );

        // Self-maker at level 2 must still be on the book.
        assert_eq!(book.order_count(), 1);
        book.check_invariants_public().expect("invariants after ExpireTaker partial fill");
    }

    /// ExpireTaker mid-level: taker fills front of level (user B, qty=2 partial),
    /// then front becomes user A (self). ExpireTaker fires at remaining_qty=3.
    #[test]
    fn test_stp_expire_taker_mid_level() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));
        let user_a = uuid::Uuid::from_u128(0xAAAA);
        let user_b = uuid::Uuid::from_u128(0xBBBB);

        // Single price level, two orders: user B qty=2, then user A qty=3 (FIFO).
        let mut sell_b = make_limit_order(Side::Sell, dec!(50000), dec!(2));
        sell_b.user_id = user_b;
        book.place_order(sell_b);

        let self_maker_id = uuid::Uuid::new_v4();
        let mut sell_a = make_limit_order(Side::Sell, dec!(50000), dec!(3));
        sell_a.user_id = user_a;
        sell_a.order_id = self_maker_id;
        book.place_order(sell_a);

        let taker_id = uuid::Uuid::new_v4();
        let mut buy = make_stp_order(
            user_a,
            Side::Buy,
            OrderType::Market,
            TimeInForce::Gtc,
            dec!(0),
            dec!(5),
            StpMode::ExpireTaker,
        );
        buy.order_id = taker_id;
        let events = book.place_order(buy);

        let fills: Vec<_> = events.iter().filter_map(|e| match e {
            MatchEvent::Fill(f) => Some(f),
            _ => None,
        }).collect();
        let cancels: Vec<_> = events.iter().filter_map(|e| match e {
            MatchEvent::OrderCancelled { order_id, remaining_qty, .. } => Some((*order_id, *remaining_qty)),
            _ => None,
        }).collect();

        assert_eq!(fills.len(), 1, "exactly 1 fill against user B: {events:?}");
        assert_eq!(fills[0].qty, dec!(2));
        assert_eq!(fills[0].maker_user_id, user_b);

        assert_eq!(cancels.len(), 1, "taker cancelled: {events:?}");
        assert_eq!(cancels[0].0, taker_id);
        assert_eq!(cancels[0].1, dec!(3), "remaining_qty must be 3 (5-2)");

        // Self-maker (user A, qty=3) must still be on the book.
        assert_eq!(book.order_count(), 1);
        book.check_invariants_public().expect("invariants after mid-level ExpireTaker");
    }

    /// ExpireMaker continues to next maker: level has [self_maker qty=1, user_B qty=2].
    /// Taker buys qty=2. Self cancelled, then fills 2 against user B.
    #[test]
    fn test_stp_expire_maker_continues_to_next_maker() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));
        let user_a = uuid::Uuid::from_u128(0xAAAA);
        let user_b = uuid::Uuid::from_u128(0xBBBB);

        let self_maker_id = uuid::Uuid::new_v4();
        let mut sell_a = make_limit_order(Side::Sell, dec!(50000), dec!(1));
        sell_a.user_id = user_a;
        sell_a.order_id = self_maker_id;
        book.place_order(sell_a);

        let mut sell_b = make_limit_order(Side::Sell, dec!(50000), dec!(2));
        sell_b.user_id = user_b;
        book.place_order(sell_b);

        let taker_id = uuid::Uuid::new_v4();
        let mut buy = make_stp_order(
            user_a,
            Side::Buy,
            OrderType::Market,
            TimeInForce::Gtc,
            dec!(0),
            dec!(2),
            StpMode::ExpireMaker,
        );
        buy.order_id = taker_id;
        let events = book.place_order(buy);

        let fills: Vec<_> = events.iter().filter_map(|e| match e {
            MatchEvent::Fill(f) => Some(f),
            _ => None,
        }).collect();
        let cancels: Vec<_> = events.iter().filter_map(|e| match e {
            MatchEvent::OrderCancelled { order_id, .. } => Some(*order_id),
            _ => None,
        }).collect();

        // Self-maker cancelled via STP.
        assert_eq!(cancels.len(), 1, "self-maker cancelled: {events:?}");
        assert_eq!(cancels[0], self_maker_id);

        // Taker fills qty=2 against user B.
        assert_eq!(fills.len(), 1, "fills against user B: {events:?}");
        assert_eq!(fills[0].qty, dec!(2));
        assert_eq!(fills[0].maker_user_id, user_b);

        assert_eq!(book.order_count(), 0, "fully filled + maker cancelled");
        book.check_invariants_public().expect("invariants after ExpireMaker continues to next");
    }

    /// ExpireMaker continues to next level: level 1 has only self-maker qty=1.
    /// Level 2 has user B qty=2. Taker buys qty=2.
    #[test]
    fn test_stp_expire_maker_continues_to_next_level() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));
        let user_a = uuid::Uuid::from_u128(0xAAAA);
        let user_b = uuid::Uuid::from_u128(0xBBBB);

        let self_maker_id = uuid::Uuid::new_v4();
        let mut sell_a = make_limit_order(Side::Sell, dec!(50000), dec!(1));
        sell_a.user_id = user_a;
        sell_a.order_id = self_maker_id;
        book.place_order(sell_a);

        let mut sell_b = make_limit_order(Side::Sell, dec!(50001), dec!(2));
        sell_b.user_id = user_b;
        book.place_order(sell_b);

        let taker_id = uuid::Uuid::new_v4();
        let mut buy = make_stp_order(
            user_a,
            Side::Buy,
            OrderType::Market,
            TimeInForce::Gtc,
            dec!(0),
            dec!(2),
            StpMode::ExpireMaker,
        );
        buy.order_id = taker_id;
        let events = book.place_order(buy);

        let fills: Vec<_> = events.iter().filter_map(|e| match e {
            MatchEvent::Fill(f) => Some(f),
            _ => None,
        }).collect();
        let cancels: Vec<_> = events.iter().filter_map(|e| match e {
            MatchEvent::OrderCancelled { order_id, .. } => Some(*order_id),
            _ => None,
        }).collect();

        assert_eq!(cancels.len(), 1, "level-1 self-maker cancelled: {events:?}");
        assert_eq!(cancels[0], self_maker_id);
        assert_eq!(fills.len(), 1, "fills against user B on level 2: {events:?}");
        assert_eq!(fills[0].qty, dec!(2));
        assert_eq!(fills[0].maker_user_id, user_b);

        assert_eq!(book.order_count(), 0);
        book.check_invariants_public().expect("invariants after ExpireMaker cross-level");
    }

    /// ExpireBoth with prior fills: taker partially fills non-self on level 1,
    /// then ExpireBoth on level 2. Prior fills preserved, taker PartiallyFilled.
    #[test]
    fn test_stp_expire_both_with_prior_fills() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));
        let user_a = uuid::Uuid::from_u128(0xAAAA);
        let user_b = uuid::Uuid::from_u128(0xBBBB);

        let mut sell_b = make_limit_order(Side::Sell, dec!(50000), dec!(2));
        sell_b.user_id = user_b;
        book.place_order(sell_b);

        let self_maker_id = uuid::Uuid::new_v4();
        let mut sell_a = make_limit_order(Side::Sell, dec!(50001), dec!(3));
        sell_a.user_id = user_a;
        sell_a.order_id = self_maker_id;
        book.place_order(sell_a);

        let taker_id = uuid::Uuid::new_v4();
        let mut buy = make_stp_order(
            user_a,
            Side::Buy,
            OrderType::Market,
            TimeInForce::Gtc,
            dec!(0),
            dec!(5),
            StpMode::ExpireBoth,
        );
        buy.order_id = taker_id;
        let events = book.place_order(buy);

        let fills: Vec<_> = events.iter().filter_map(|e| match e {
            MatchEvent::Fill(f) => Some(f),
            _ => None,
        }).collect();
        let cancelled_ids: Vec<uuid::Uuid> = events.iter().filter_map(|e| match e {
            MatchEvent::OrderCancelled { order_id, .. } => Some(*order_id),
            _ => None,
        }).collect();

        // Prior fills against user B are preserved.
        assert_eq!(fills.len(), 1, "fill against user B preserved: {events:?}");
        assert_eq!(fills[0].qty, dec!(2));

        // Both self-maker and taker cancelled.
        assert_eq!(cancelled_ids.len(), 2, "both cancelled: {events:?}");
        assert!(cancelled_ids.contains(&self_maker_id));
        assert!(cancelled_ids.contains(&taker_id));

        // Only level-2 maker removed; book empty.
        assert_eq!(book.order_count(), 0);
        book.check_invariants_public().expect("invariants after ExpireBoth with prior fills");
    }

    /// IOC + ExpireMaker: self-maker cancelled, IOC cancels unfilled remainder.
    #[test]
    fn test_ioc_with_expire_maker() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));
        let user_a = uuid::Uuid::from_u128(0xAAAA);

        // Only a self-maker on the book.
        let self_maker_id = uuid::Uuid::new_v4();
        let mut sell_a = make_limit_order(Side::Sell, dec!(50000), dec!(1));
        sell_a.user_id = user_a;
        sell_a.order_id = self_maker_id;
        book.place_order(sell_a);

        let taker_id = uuid::Uuid::new_v4();
        let mut buy = make_stp_order(
            user_a,
            Side::Buy,
            OrderType::Limit,
            TimeInForce::Ioc,
            dec!(50000),
            dec!(1),
            StpMode::ExpireMaker,
        );
        buy.order_id = taker_id;
        let events = book.place_order(buy);

        let fills: Vec<_> = events.iter().filter(|e| matches!(e, MatchEvent::Fill(_))).collect();
        let cancelled_ids: Vec<uuid::Uuid> = events.iter().filter_map(|e| match e {
            MatchEvent::OrderCancelled { order_id, .. } => Some(*order_id),
            _ => None,
        }).collect();

        assert_eq!(fills.len(), 0, "no fill: {events:?}");
        // Self-maker cancelled by STP + IOC remainder cancel.
        assert_eq!(cancelled_ids.len(), 2, "maker cancel + IOC remainder cancel: {events:?}");
        assert!(cancelled_ids.contains(&self_maker_id), "maker must be cancelled");
        assert!(cancelled_ids.contains(&taker_id), "IOC taker must be cancelled");
        assert_eq!(book.order_count(), 0);
        book.check_invariants_public().expect("invariants after IOC+ExpireMaker");
    }

    /// Market + ExpireTaker: market order against self-maker is cancelled.
    #[test]
    fn test_market_with_expire_taker() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));
        let user_a = uuid::Uuid::from_u128(0xAAAA);

        let mut sell_a = make_limit_order(Side::Sell, dec!(50000), dec!(1));
        sell_a.user_id = user_a;
        book.place_order(sell_a);

        let taker_id = uuid::Uuid::new_v4();
        let mut buy = make_stp_order(
            user_a,
            Side::Buy,
            OrderType::Market,
            TimeInForce::Gtc,
            dec!(0),
            dec!(1),
            StpMode::ExpireTaker,
        );
        buy.order_id = taker_id;
        let events = book.place_order(buy);

        let fills: Vec<_> = events.iter().filter(|e| matches!(e, MatchEvent::Fill(_))).collect();
        let cancelled_ids: Vec<uuid::Uuid> = events.iter().filter_map(|e| match e {
            MatchEvent::OrderCancelled { order_id, .. } => Some(*order_id),
            _ => None,
        }).collect();

        assert_eq!(fills.len(), 0, "no fill: {events:?}");
        // Only the taker is cancelled (by STP), not the market order remainder cancel.
        assert_eq!(cancelled_ids.len(), 1, "taker cancelled by STP: {events:?}");
        assert_eq!(cancelled_ids[0], taker_id);
        // Maker still on book.
        assert_eq!(book.order_count(), 1);
        book.check_invariants_public().expect("invariants after market+ExpireTaker");
    }

    /// GTD + ExpireMaker: self-maker cancelled, GTD order rests on book.
    #[test]
    fn test_gtd_with_expire_maker() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));
        let user_a = uuid::Uuid::from_u128(0xAAAA);

        let self_maker_id = uuid::Uuid::new_v4();
        let mut sell_a = make_limit_order(Side::Sell, dec!(50000), dec!(1));
        sell_a.user_id = user_a;
        sell_a.order_id = self_maker_id;
        book.place_order(sell_a);

        let taker_id = uuid::Uuid::new_v4();
        let mut buy = make_stp_order(
            user_a,
            Side::Buy,
            OrderType::Limit,
            TimeInForce::Gtd,
            dec!(50000),
            dec!(1),
            StpMode::ExpireMaker,
        );
        buy.order_id = taker_id;
        buy.expire_at = Some(chrono::Utc::now() + chrono::Duration::hours(1));
        let events = book.place_order(buy);

        let fills: Vec<_> = events.iter().filter(|e| matches!(e, MatchEvent::Fill(_))).collect();
        let cancelled_ids: Vec<uuid::Uuid> = events.iter().filter_map(|e| match e {
            MatchEvent::OrderCancelled { order_id, .. } => Some(*order_id),
            _ => None,
        }).collect();
        let accepted_ids: Vec<uuid::Uuid> = events.iter().filter_map(|e| match e {
            MatchEvent::OrderAccepted { order_id, .. } => Some(*order_id),
            _ => None,
        }).collect();

        assert_eq!(fills.len(), 0, "no fill: {events:?}");
        assert_eq!(cancelled_ids.len(), 1, "self-maker cancelled: {events:?}");
        assert_eq!(cancelled_ids[0], self_maker_id);
        assert_eq!(accepted_ids.len(), 1, "GTD taker rests on book: {events:?}");
        assert_eq!(accepted_ids[0], taker_id);
        assert_eq!(book.order_count(), 1, "GTD taker on book");
        book.check_invariants_public().expect("invariants after GTD+ExpireMaker");
    }

    /// user_order_count decremented correctly when self-maker is cancelled by STP.
    #[test]
    fn test_expire_maker_decrements_user_order_count() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));
        let user_a = uuid::Uuid::from_u128(0xAAAA);

        let mut sell_a = make_limit_order(Side::Sell, dec!(50000), dec!(1));
        sell_a.user_id = user_a;
        book.place_order(sell_a);

        // Before: user A has 1 order on book.
        assert_eq!(book.user_open_order_count(user_a), 1);

        // Place taker from same user with ExpireMaker — self-maker cancelled,
        // no other liquidity, taker rests as GTC.
        let mut buy = make_stp_order(
            user_a,
            Side::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            dec!(50000),
            dec!(1),
            StpMode::ExpireMaker,
        );
        buy.user_id = user_a;
        book.place_order(buy);

        // Taker rests on book as bid, self-maker cancelled → user_order_count = 1 (taker).
        assert_eq!(
            book.user_open_order_count(user_a),
            1,
            "maker cancelled (-1) + taker rested (+1) = 1"
        );
        book.check_invariants_public().expect("invariants after expire_maker count check");
    }

    /// Market + ExpireMaker: self-maker at best level cancelled, market buy
    /// continues to the next level and fills against a non-self maker.
    #[test]
    fn test_market_with_expire_maker() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));
        let user_a = uuid::Uuid::from_u128(0xAAAA);
        let user_b = uuid::Uuid::from_u128(0xBBBB);

        // Level 1: user A self-maker @ 50000 qty=1
        let self_maker_id = uuid::Uuid::new_v4();
        let mut sell_a = make_limit_order(Side::Sell, dec!(50000), dec!(1));
        sell_a.user_id = user_a;
        sell_a.order_id = self_maker_id;
        book.place_order(sell_a);

        // Level 2: user B maker @ 50001 qty=2
        let mut sell_b = make_limit_order(Side::Sell, dec!(50001), dec!(2));
        sell_b.user_id = user_b;
        book.place_order(sell_b);

        // Taker: user A market buy qty=2 with ExpireMaker
        let taker_id = uuid::Uuid::new_v4();
        let mut buy = make_stp_order(
            user_a,
            Side::Buy,
            OrderType::Market,
            TimeInForce::Gtc,
            dec!(0),
            dec!(2),
            StpMode::ExpireMaker,
        );
        buy.order_id = taker_id;
        let events = book.place_order(buy);

        let fills: Vec<_> = events.iter().filter_map(|e| match e {
            MatchEvent::Fill(f) => Some(f),
            _ => None,
        }).collect();
        let cancelled_ids: Vec<uuid::Uuid> = events.iter().filter_map(|e| match e {
            MatchEvent::OrderCancelled { order_id, .. } => Some(*order_id),
            _ => None,
        }).collect();

        // Self-maker at level 1 cancelled by STP.
        assert!(cancelled_ids.contains(&self_maker_id), "self-maker must be cancelled: {events:?}");
        // Market taker must NOT appear in cancelled (it fills fully against user B).
        assert!(!cancelled_ids.contains(&taker_id), "taker must NOT be cancelled: {events:?}");
        // Exactly 1 fill against user B at level 2.
        assert_eq!(fills.len(), 1, "expected 1 fill against user B: {events:?}");
        assert_eq!(fills[0].maker_user_id, user_b);
        assert_eq!(fills[0].qty, dec!(2));
        // Book: only user B's maker remains (partially or fully drained).
        // user B had qty=2, taker filled qty=2 → level fully drained.
        assert_eq!(book.order_count(), 0, "both levels drained: {events:?}");
        book.check_invariants_public().expect("invariants after market+ExpireMaker");
    }

    /// Market + ExpireBoth: self-maker cancelled AND market taker cancelled,
    /// no fill, book remains empty.
    #[test]
    fn test_market_with_expire_both() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));
        let user_a = uuid::Uuid::from_u128(0xAAAA);

        // Only a self-maker on the book.
        let self_maker_id = uuid::Uuid::new_v4();
        let mut sell_a = make_limit_order(Side::Sell, dec!(50000), dec!(1));
        sell_a.user_id = user_a;
        sell_a.order_id = self_maker_id;
        book.place_order(sell_a);

        // Taker: user A market buy qty=1 with ExpireBoth
        let taker_id = uuid::Uuid::new_v4();
        let mut buy = make_stp_order(
            user_a,
            Side::Buy,
            OrderType::Market,
            TimeInForce::Gtc,
            dec!(0),
            dec!(1),
            StpMode::ExpireBoth,
        );
        buy.order_id = taker_id;
        let events = book.place_order(buy);

        let fills: Vec<_> = events.iter().filter(|e| matches!(e, MatchEvent::Fill(_))).collect();
        let cancelled_ids: Vec<uuid::Uuid> = events.iter().filter_map(|e| match e {
            MatchEvent::OrderCancelled { order_id, .. } => Some(*order_id),
            _ => None,
        }).collect();

        // No fill.
        assert_eq!(fills.len(), 0, "no fill expected: {events:?}");
        // Both orders cancelled.
        assert_eq!(cancelled_ids.len(), 2, "both orders cancelled: {events:?}");
        assert!(cancelled_ids.contains(&self_maker_id), "self-maker cancelled: {events:?}");
        assert!(cancelled_ids.contains(&taker_id), "taker cancelled: {events:?}");
        // Book empty.
        assert_eq!(book.order_count(), 0);
        book.check_invariants_public().expect("invariants after market+ExpireBoth");
    }

    /// IOC + ExpireTaker: when STP fires and cancels the taker, the IOC
    /// remainder-cancel path must NOT emit a second OrderCancelled for the
    /// same taker order (no double-cancel).
    #[test]
    fn test_ioc_with_expire_taker_no_double_cancel() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));
        let user_a = uuid::Uuid::from_u128(0xAAAA);

        // Self-maker on the book.
        let self_maker_id = uuid::Uuid::new_v4();
        let mut sell_a = make_limit_order(Side::Sell, dec!(50000), dec!(1));
        sell_a.user_id = user_a;
        sell_a.order_id = self_maker_id;
        book.place_order(sell_a);

        // IOC taker from same user with ExpireTaker.
        let taker_id = uuid::Uuid::new_v4();
        let mut buy = make_stp_order(
            user_a,
            Side::Buy,
            OrderType::Limit,
            TimeInForce::Ioc,
            dec!(50000),
            dec!(1),
            StpMode::ExpireTaker,
        );
        buy.order_id = taker_id;
        let events = book.place_order(buy);

        let fills: Vec<_> = events.iter().filter(|e| matches!(e, MatchEvent::Fill(_))).collect();
        let taker_cancels: Vec<_> = events.iter().filter(|e| matches!(
            e,
            MatchEvent::OrderCancelled { order_id, .. } if *order_id == taker_id
        )).collect();

        // No fill — STP prevented matching.
        assert_eq!(fills.len(), 0, "no fill expected: {events:?}");
        // Exactly ONE OrderCancelled for the taker — not two.
        assert_eq!(
            taker_cancels.len(), 1,
            "taker must be cancelled exactly once (no double-cancel from IOC path): {events:?}",
        );
        // Self-maker still on the book (ExpireTaker preserves maker).
        assert_eq!(book.order_count(), 1, "self-maker must remain: {events:?}");
        book.check_invariants_public().expect("invariants after IOC+ExpireTaker");
    }

    /// FOK defense-in-depth: when can_fill_fully correctly filters self-orders
    /// and the remaining non-self liquidity is insufficient, FOK is rejected at
    /// the pre-check stage and never reaches the STP path in match_order.
    /// This documents the expected behavior and guards against can_fill_fully
    /// regressions that could allow a partial fill via STP.
    #[test]
    fn test_fok_defense_in_depth_no_partial_via_stp() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));
        let user_a = uuid::Uuid::from_u128(0xAAAA);
        let user_b = uuid::Uuid::from_u128(0xBBBB);

        // User A self-maker @ 50000 qty=5.
        let mut sell_a = make_limit_order(Side::Sell, dec!(50000), dec!(5));
        sell_a.user_id = user_a;
        book.place_order(sell_a);

        // User B non-self maker @ 50001 qty=2.
        let mut sell_b = make_limit_order(Side::Sell, dec!(50001), dec!(2));
        sell_b.user_id = user_b;
        book.place_order(sell_b);

        // User A FOK buy qty=3 with ExpireTaker.
        // can_fill_fully: self-qty filtered → non-self available = 2 < 3 → FOK rejected.
        let fok_id = uuid::Uuid::new_v4();
        let mut buy = make_stp_order(
            user_a,
            Side::Buy,
            OrderType::Limit,
            TimeInForce::Fok,
            dec!(50001),
            dec!(3),
            StpMode::ExpireTaker,
        );
        buy.order_id = fok_id;
        let events = book.place_order(buy);

        let fills: Vec<_> = events.iter().filter(|e| matches!(e, MatchEvent::Fill(_))).collect();
        let cancels: Vec<_> = events.iter().filter(|e| matches!(e, MatchEvent::OrderCancelled { .. })).collect();
        let rejects: Vec<_> = events.iter().filter(|e| matches!(e, MatchEvent::OrderRejected { .. })).collect();

        // Rejected at pre-check — never reached STP path.
        assert_eq!(rejects.len(), 1, "expected 1 OrderRejected: {events:?}");
        assert_eq!(fills.len(), 0, "no fill: {events:?}");
        assert_eq!(cancels.len(), 0, "no cancel (STP path not reached): {events:?}");
        // Book unchanged — both makers still present.
        assert_eq!(book.order_count(), 2, "book unchanged: {events:?}");
        book.check_invariants_public().expect("invariants after FOK defense-in-depth");
    }

    // ── Full snapshot / restore roundtrip ────────────────────────────────

    #[test]
    fn test_full_snapshot_restore_roundtrip() {
        let mut book = OrderBook::new(Symbol::new("BTCUSDT"));

        // Place several resting orders at different prices.
        for i in 1..=5 {
            book.place_order(make_limit_order(
                Side::Buy,
                dec!(50000) - Decimal::from(i) * dec!(100),
                dec!(1),
            ));
        }
        for i in 1..=3 {
            book.place_order(make_limit_order(
                Side::Sell,
                dec!(50000) + Decimal::from(i) * dec!(100),
                dec!(2),
            ));
        }

        let original_l2 = book.snapshot(50);
        let original_count = book.order_count();
        let original_seq = book.sequence();

        // Take a full snapshot.
        let full_snap = book.full_snapshot();
        assert_eq!(full_snap.orders.len(), 8, "5 buys + 3 sells = 8 orders");
        assert_eq!(full_snap.sequence, original_seq);

        // Restore into a fresh book.
        let mut restored = OrderBook::new(Symbol::new("BTCUSDT"));
        restored.restore_from_snapshot(full_snap);

        // L2 snapshot should be identical.
        let restored_l2 = restored.snapshot(50);
        assert_eq!(restored_l2.bids, original_l2.bids, "bid levels must match");
        assert_eq!(restored_l2.asks, original_l2.asks, "ask levels must match");
        assert_eq!(restored.order_count(), original_count, "order count must match");
        assert_eq!(restored.sequence(), original_seq, "sequence must match");

        // New orders on the restored book should work normally.
        let events = restored.place_order(make_limit_order(Side::Buy, dec!(49500), dec!(1)));
        assert!(
            events.iter().any(|e| matches!(e, MatchEvent::OrderAccepted { .. })),
            "must be able to place new orders after restore"
        );
    }

    #[test]
    fn test_full_snapshot_serialization_roundtrip() {
        let mut book = OrderBook::new(Symbol::new("ETHUSDT"));
        book.place_order(make_limit_order(Side::Buy, dec!(3000), dec!(10)));
        book.place_order(make_limit_order(Side::Sell, dec!(3100), dec!(5)));

        let snap = book.full_snapshot();
        let json = serde_json::to_string(&snap).expect("serialize");
        let deserialized: FullBookSnapshot = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.symbol, snap.symbol);
        assert_eq!(deserialized.sequence, snap.sequence);
        assert_eq!(deserialized.orders.len(), snap.orders.len());
    }
}
