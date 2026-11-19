//! Property-based tests for the matching engine using proptest.
//!
//! These tests verify invariants that must hold regardless of the input:
//! - Total filled quantity across all fills for an order ≤ order.qty
//! - Fill prices are within the expected range
//! - Book invariant: best ask > best bid (no crossed book after operations)
//! - Cancel reduces order count by exactly 1
//! - WAL replay determinism: two independent engines fed the same command
//!   sequence produce byte-identical MatchEvent outputs (T3.2).

use exchange_common::*;
use exchange_matching_engine::{MatchingEngine, OrderBook};
use proptest::prelude::*;
use rust_decimal::Decimal;
use uuid::Uuid;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn make_book() -> OrderBook {
    OrderBook::new(Symbol::new("BTCUSDT"))
}

fn limit_order(user: u128, side: Side, price: Decimal, qty: Decimal) -> PlaceOrderCommand {
    PlaceOrderCommand {
        order_id: Uuid::new_v4(),
        user_id: Uuid::from_u128(user),
        symbol: Symbol::new("BTCUSDT"),
        side,
        order_type: OrderType::Limit,
        time_in_force: TimeInForce::Gtc,
        price,
        qty,
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

fn market_order(user: u128, side: Side, qty: Decimal) -> PlaceOrderCommand {
    let mut cmd = limit_order(user, side, Decimal::ZERO, qty);
    cmd.order_type = OrderType::Market;
    cmd
}

fn cancel_cmd(order_id: Uuid) -> CancelOrderCommand {
    CancelOrderCommand {
        order_id,
        user_id: Uuid::nil(),
        symbol: Symbol::new("BTCUSDT"),
        timestamp: chrono::Utc::now(),
    }
}

// ── Strategy: generate valid prices and quantities ───────────────────────────

fn valid_price() -> impl Strategy<Value = Decimal> {
    // Prices between 100 and 100,000 in steps of 0.01
    (10_000u64..10_000_000u64).prop_map(|cents| Decimal::new(cents as i64, 2))
}

fn valid_qty() -> impl Strategy<Value = Decimal> {
    // Quantities between 0.001 and 10.0 in steps of 0.001
    (1u64..10_000u64).prop_map(|millis| Decimal::new(millis as i64, 3))
}

fn valid_side() -> impl Strategy<Value = Side> {
    prop_oneof![Just(Side::Buy), Just(Side::Sell)]
}

// ── Property 1: Fill quantity never exceeds order quantity ────────────────────

proptest! {
    #[test]
    fn prop_fill_qty_le_order_qty(
        price in valid_price(),
        qty in valid_qty(),
    ) {
        let mut book = make_book();
        // Place a resting sell.
        book.place_order(limit_order(1, Side::Sell, price, qty));

        // Place a market buy for the same qty.
        let events = book.place_order(market_order(2, Side::Buy, qty));

        let total_filled: Decimal = events.iter()
            .filter_map(|e| match e {
                MatchEvent::Fill(f) => Some(f.qty),
                _ => None,
            })
            .sum();

        prop_assert!(total_filled <= qty, "filled {} > order qty {}", total_filled, qty);
    }
}

// ── Property 2: Fill price within bounds ─────────────────────────────────────

proptest! {
    #[test]
    fn prop_fill_price_is_maker_price(
        ask_price in valid_price(),
        bid_offset in 0u64..1000u64,
        qty in valid_qty(),
    ) {
        let mut book = make_book();
        let ask = ask_price;
        let bid = ask_price + Decimal::new(bid_offset as i64, 2); // bid >= ask → will cross

        book.place_order(limit_order(1, Side::Sell, ask, qty));
        let events = book.place_order(limit_order(2, Side::Buy, bid, qty));

        for event in &events {
            if let MatchEvent::Fill(fill) = event {
                // Fill must be at maker's price (the ask).
                prop_assert_eq!(fill.price, ask, "fill at {} but maker ask was {}", fill.price, ask);
            }
        }
    }
}

// ── Property 3: Book never crossed after operations ──────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]
    #[test]
    fn prop_book_not_crossed_after_random_orders(
        prices in prop::collection::vec(valid_price(), 5..20),
        qtys in prop::collection::vec(valid_qty(), 5..20),
        sides in prop::collection::vec(valid_side(), 5..20),
    ) {
        let mut book = make_book();
        let n = prices.len().min(qtys.len()).min(sides.len());

        for i in 0..n {
            // Use different user per order to avoid STP
            let cmd = limit_order((i + 1) as u128, sides[i], prices[i], qtys[i]);
            book.place_order(cmd);
        }

        // After all operations, book must not be crossed:
        // best bid < best ask (or one side is empty).
        let snap = book.snapshot(1);
        if let (Some(&(bid, _)), Some(&(ask, _))) = (snap.bids.first(), snap.asks.first()) {
            prop_assert!(bid < ask, "crossed book: bid {} >= ask {}", bid, ask);
        }
    }
}

// ── Property 4: Cancel decrements order count by 1 ──────────────────────────

proptest! {
    #[test]
    fn prop_cancel_decrements_count(
        price in valid_price(),
        qty in valid_qty(),
    ) {
        let mut book = make_book();
        let cmd = limit_order(1, Side::Buy, price, qty);
        let oid = cmd.order_id;
        book.place_order(cmd);

        let before = book.order_count();
        book.cancel_order(cancel_cmd(oid));
        let after = book.order_count();

        prop_assert_eq!(after, before - 1, "cancel didn't reduce count");
    }
}

// ── Property 5: Sum of fills across both parties is balanced ─────────────────

proptest! {
    #[test]
    fn prop_fills_are_balanced(
        price in valid_price(),
        sell_qty in valid_qty(),
        buy_qty in valid_qty(),
    ) {
        let mut book = make_book();
        book.place_order(limit_order(1, Side::Sell, price, sell_qty));
        let events = book.place_order(limit_order(2, Side::Buy, price, buy_qty));

        // Every fill has a maker and a taker. The qty is the same for both.
        // Total maker-side fill == total taker-side fill.
        for event in &events {
            if let MatchEvent::Fill(fill) = event {
                // Each fill is a single record: taker gets qty, maker gives qty.
                prop_assert!(fill.qty > Decimal::ZERO, "fill qty must be positive");
                prop_assert!(fill.qty <= sell_qty, "fill qty exceeds sell order");
                prop_assert!(fill.qty <= buy_qty, "fill qty exceeds buy order");
            }
        }
    }
}

// ── Property 6: Market order on empty book produces no fills ─────────────────

proptest! {
    #[test]
    fn prop_market_on_empty_no_fills(
        qty in valid_qty(),
        side in valid_side(),
    ) {
        let mut book = make_book();
        let events = book.place_order(market_order(1, side, qty));

        let fills: Vec<_> = events.iter()
            .filter(|e| matches!(e, MatchEvent::Fill(_)))
            .collect();
        prop_assert!(fills.is_empty(), "market on empty book should not fill");

        // Should have a cancel event (remainder cancelled).
        let cancels: Vec<_> = events.iter()
            .filter(|e| matches!(e, MatchEvent::OrderCancelled { .. }))
            .collect();
        prop_assert!(!cancels.is_empty(), "market on empty book should cancel remainder");
    }
}

// ── T3.2: WAL replay determinism ─────────────────────────────────────────────
//
// Two independent MatchingEngine instances fed the same command sequence must
// produce byte-identical MatchEvent outputs. This validates that the engine is
// fully deterministic given deterministic inputs — a prerequisite for WAL crash
// recovery (replay must reconstitute the same book state).
//
// If proptest shrinks to a divergence, that is a pre-existing determinism bug
// that must be fixed in Sprint 2 before WAL recovery can be trusted.

/// A simplified command type for proptest generation. We convert to the real
/// Command type in `build_command`, using deterministic IDs and timestamps so
/// both engine instances receive byte-identical inputs.
#[derive(Debug, Clone)]
enum TestCmd {
    PlaceLimit {
        /// Index into a logical user pool [0..9].
        user_idx: u8,
        side: Side,
        /// Price as centile offset from 50_000: price = 50_000.00 + offset * 0.01
        price_offset: i16,
        /// Qty as millis [1..9999]: qty = millis * 0.001
        qty_millis: u16,
        tif: TimeInForce,
    },
    PlaceMarket {
        user_idx: u8,
        side: Side,
        qty_millis: u16,
    },
    /// Cancel by position in the running list of accepted order IDs.
    CancelByIndex(usize),
    /// One of the four balance/position variants that the engine silently
    /// ignores. Present in production WAL streams alongside order commands.
    /// Tests that the engine returns an empty Vec and the determinism assertion
    /// still holds with these interleaved.
    BalanceEvent {
        /// Selects which variant: 0=BalanceDeposit, 1=BalanceWithdraw,
        /// 2=FundingPayment, 3=PositionSnapshot.
        variant: u8,
        user_idx: u8,
    },
}

fn any_side() -> impl Strategy<Value = Side> {
    prop_oneof![Just(Side::Buy), Just(Side::Sell)]
}

fn any_tif() -> impl Strategy<Value = TimeInForce> {
    prop_oneof![
        Just(TimeInForce::Gtc),
        Just(TimeInForce::Ioc),
        Just(TimeInForce::PostOnly),
        // FOK is valid but often rejects — include it at lower weight
        Just(TimeInForce::Fok),
    ]
}

fn any_test_cmd() -> impl Strategy<Value = TestCmd> {
    prop_oneof![
        5 => // 50% — place limit orders
        (0u8..10, any_side(), (-500i16..500i16), 1u16..10_000u16, any_tif()).prop_map(
            |(user_idx, side, price_offset, qty_millis, tif)| TestCmd::PlaceLimit {
                user_idx,
                side,
                price_offset,
                qty_millis,
                tif,
            }
        ),
        2 => // 20% — place market orders
        (0u8..10, any_side(), 1u16..5_000u16).prop_map(|(user_idx, side, qty_millis)| {
            TestCmd::PlaceMarket {
                user_idx,
                side,
                qty_millis,
            }
        }),
        2 => // 20% — cancel a previously accepted order (index chosen from 0..1024;
        // the test code will mod by actual live order count so it never panics)
        (0usize..1024).prop_map(TestCmd::CancelByIndex),
        1 => // 10% — balance/position events that the engine ignores (WAL interleaving)
        (0u8..4, 0u8..10).prop_map(|(variant, user_idx)| TestCmd::BalanceEvent {
            variant,
            user_idx,
        }),
    ]
}

/// Convert a `TestCmd` into a real `Command` with deterministic IDs and timestamp.
///
/// `cmd_idx`: the sequential index of this command in the stream (0-based).
/// `accepted_ids`: the live list of (order_id, user_id) for orders currently on
/// the book — used to resolve `CancelByIndex`.
///
/// Returns `None` for `CancelByIndex` when there are no live orders (skip).
fn build_command(
    cmd: &TestCmd,
    cmd_idx: usize,
    accepted_ids: &[(Uuid, Uuid)],
) -> Option<Command> {
    // Deterministic base timestamp: Unix epoch + cmd_idx seconds.
    let ts = chrono::DateTime::from_timestamp(1_700_000_000 + cmd_idx as i64, 0)
        .expect("valid timestamp");

    match cmd {
        TestCmd::PlaceLimit {
            user_idx,
            side,
            price_offset,
            qty_millis,
            tif,
        } => {
            // order_id: embed cmd_idx in upper bits so every command has a unique UUID.
            let order_id = Uuid::from_u128((cmd_idx as u128) << 16 | 0xAAAA_u128);
            let user_id = Uuid::from_u128(*user_idx as u128 + 1);
            let price = Decimal::new(5_000_000_i64 + *price_offset as i64, 2);
            let qty = Decimal::new(*qty_millis as i64, 3);
            Some(Command::PlaceOrder(PlaceOrderCommand {
                order_id,
                user_id,
                symbol: Symbol::new("BTCUSDT"),
                side: *side,
                order_type: OrderType::Limit,
                time_in_force: *tif,
                price,
                qty,
                reduce_only: false,
                timestamp: ts,
                expire_at: None,
                position_idx: 0,
                iceberg_qty: None,
                order_link_id: None,
                slippage_tolerance: None,
                close_position: false,
                stp_mode: StpMode::None,
                request_expire_time: None,
            }))
        }
        TestCmd::PlaceMarket {
            user_idx,
            side,
            qty_millis,
        } => {
            let order_id = Uuid::from_u128((cmd_idx as u128) << 16 | 0xBBBB_u128);
            let user_id = Uuid::from_u128(*user_idx as u128 + 1);
            let qty = Decimal::new(*qty_millis as i64, 3);
            Some(Command::PlaceOrder(PlaceOrderCommand {
                order_id,
                user_id,
                symbol: Symbol::new("BTCUSDT"),
                side: *side,
                order_type: OrderType::Market,
                time_in_force: TimeInForce::Gtc,
                price: Decimal::ZERO,
                qty,
                reduce_only: false,
                timestamp: ts,
                expire_at: None,
                position_idx: 0,
                iceberg_qty: None,
                order_link_id: None,
                slippage_tolerance: None,
                close_position: false,
                stp_mode: StpMode::None,
                request_expire_time: None,
            }))
        }
        TestCmd::CancelByIndex(idx) => {
            if accepted_ids.is_empty() {
                return None; // No live orders to cancel — skip.
            }
            let (order_id, user_id) = accepted_ids[*idx % accepted_ids.len()];
            Some(Command::CancelOrder(CancelOrderCommand {
                order_id,
                user_id,
                symbol: Symbol::new("BTCUSDT"),
                timestamp: ts,
            }))
        }
        TestCmd::BalanceEvent { variant, user_idx } => {
            // All four variants are WAL entries that the matching engine ignores
            // (returns vec![]). We include them to test that interleaved balance
            // events do not disturb determinism.
            let user_id = Uuid::from_u128(*user_idx as u128 + 1);
            let amount = rust_decimal::Decimal::new(cmd_idx as i64 + 1, 2);
            Some(match variant % 4 {
                0 => Command::BalanceDeposit {
                    user_id,
                    asset: "USDT".to_string(),
                    amount,
                    reference: format!("ref-{cmd_idx}"),
                },
                1 => Command::BalanceWithdraw {
                    user_id,
                    asset: "USDT".to_string(),
                    amount,
                    reference: format!("ref-{cmd_idx}"),
                },
                2 => Command::FundingPayment {
                    user_id,
                    asset: "USDT".to_string(),
                    amount,
                    rate: rust_decimal::Decimal::new(1, 4), // 0.0001
                    symbol: "BTCUSDT".to_string(),
                },
                _ => Command::PositionSnapshot { positions: vec![] },
            })
        }
    }
}

/// Run a command sequence through an engine. Track accepted order IDs so that
/// `CancelByIndex` can be resolved consistently.
///
/// Returns the flat list of all events across all commands, formatted as Debug
/// strings for byte-level comparison.
///
/// After processing all commands, calls `check_invariants_public()` on the
/// final book state to verify invariants 2 (per-level qty consistency) and 3
/// (order_locations bidirectional consistency). A violation panics so proptest
/// can shrink to a minimal reproducer.
fn run_sequence(cmds: &[TestCmd]) -> Vec<String> {
    let mut engine = MatchingEngine::new();
    engine.add_symbol(Symbol::new("BTCUSDT"));

    let mut accepted_ids: Vec<(Uuid, Uuid)> = Vec::new();
    let mut all_events: Vec<String> = Vec::new();

    for (i, cmd) in cmds.iter().enumerate() {
        let Some(command) = build_command(cmd, i, &accepted_ids) else {
            continue;
        };

        // Before processing a cancel, remove that order from accepted_ids so
        // subsequent cancels don't try to cancel it again (matches engine behaviour:
        // first cancel succeeds, second is a silent no-op — same in both engines).
        if let Command::CancelOrder(ref cc) = command {
            accepted_ids.retain(|(oid, _)| *oid != cc.order_id);
        }

        let events = engine.process(command.clone());

        // Track newly accepted orders for future cancels.
        for event in &events {
            if let MatchEvent::OrderAccepted { order_id, .. } = event {
                // Recover user_id from the command we just sent.
                if let Command::PlaceOrder(ref po) = command {
                    if po.order_id == *order_id {
                        accepted_ids.push((*order_id, po.user_id));
                    }
                }
            }
        }

        // Serialise events as Debug strings for comparison.
        for event in events {
            all_events.push(format!("{event:?}"));
        }
    }

    // Full invariant check (invariants 2 + 3) on the final book state.
    // Unwrap panics on violation — proptest will shrink to a minimal case.
    engine
        .book(&Symbol::new("BTCUSDT"))
        .expect("BTCUSDT book must exist")
        .check_invariants_public()
        .unwrap_or_else(|msg| panic!("OrderBook invariant violated after sequence: {msg}"));

    all_events
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]
    #[test]
    fn prop_wal_replay_determinism(
        cmds in prop::collection::vec(any_test_cmd(), 10..200),
    ) {
        // Run the same command sequence through two independent engine instances.
        let events_a = run_sequence(&cmds);
        let events_b = run_sequence(&cmds);

        prop_assert_eq!(
            events_a.len(),
            events_b.len(),
            "event count diverged: engine A produced {} events, engine B produced {}",
            events_a.len(),
            events_b.len(),
        );

        for (idx, (ea, eb)) in events_a.iter().zip(events_b.iter()).enumerate() {
            prop_assert_eq!(
                ea, eb,
                "event[{}] diverged:\n  A: {}\n  B: {}",
                idx,
                ea,
                eb,
            );
        }

        // Both books should satisfy all invariants after the full sequence.
        // (We check via a fresh engine replay — re-running is acceptable in proptest.)
        // For simplicity, the per-event invariant check (debug_assert!) already fired
        // during the above runs; reaching here means invariant 1 held throughout.
    }
}

// ── T1.6c: Top-of-book cache consistency ─────────────────────────────────────
//
// After EVERY command, the best_bid_cache and best_ask_cache must exactly
// agree with the authoritative BTreeMap values. Any disagreement is a cache
// invalidation bug and would cause silent financial loss (missed fills,
// crossed books, stale PostOnly checks).
//
// Uses ≥10K iterations as required by the Sprint 3 acceptance criteria.

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 10_000,
        ..ProptestConfig::default()
    })]

    #[test]
    fn prop_top_of_book_cache_consistent(
        cmds in prop::collection::vec(any_test_cmd(), 1..100),
    ) {
        let mut engine = MatchingEngine::new();
        engine.add_symbol(Symbol::new("BTCUSDT"));

        let mut accepted_ids: Vec<(Uuid, Uuid)> = Vec::new();

        for (i, cmd) in cmds.iter().enumerate() {
            let Some(command) = build_command(cmd, i, &accepted_ids) else {
                continue;
            };

            // Remove cancelled order from tracking before processing.
            if let Command::CancelOrder(ref cc) = command {
                accepted_ids.retain(|(oid, _)| *oid != cc.order_id);
            }

            let events = engine.process(command.clone());

            // Track newly accepted orders.
            for event in &events {
                if let MatchEvent::OrderAccepted { order_id, .. } = event {
                    if let Command::PlaceOrder(ref po) = command {
                        if po.order_id == *order_id {
                            accepted_ids.push((*order_id, po.user_id));
                        }
                    }
                }
            }

            // --- Cache consistency assertion after every command ---
            let book = engine
                .book(&Symbol::new("BTCUSDT"))
                .expect("BTCUSDT book must exist");

            // Authoritative values direct from BTreeMap.
            // These are the ground truth — cache must match exactly.
            let true_best_bid: Option<Price> = book
                .snapshot(1)
                .bids
                .first()
                .map(|&(p, _)| p);
            let true_best_ask: Option<Price> = book
                .snapshot(1)
                .asks
                .first()
                .map(|&(p, _)| p);

            let cached_bid = book.best_bid_cache_raw();
            let cached_ask = book.best_ask_cache_raw();

            prop_assert_eq!(
                cached_bid,
                true_best_bid,
                "best_bid_cache stale after cmd[{}] {:?}: cache={:?} btree={:?}",
                i,
                cmd,
                cached_bid,
                true_best_bid,
            );

            prop_assert_eq!(
                cached_ask,
                true_best_ask,
                "best_ask_cache stale after cmd[{}] {:?}: cache={:?} btree={:?}",
                i,
                cmd,
                cached_ask,
                true_best_ask,
            );

            // Full structural invariants (levels consistent, index bidirectional).
            book.check_invariants_public()
                .unwrap_or_else(|msg| panic!("invariant violated after cmd[{i}]: {msg}"));
        }
    }
}

// ── KG-1.5: STP self-fill prevention property test ───────────────────────────
//
// When stp_mode != None, NO Fill event must have maker_user_id == taker_user_id.
// Tested across random command sequences where every order uses a randomly chosen
// STP mode and users can place both sides (potentially matching themselves).
//
// Reference: Bybit API Spec v5 "Self-Trade Prevention (STP) Mode".

fn any_stp_mode() -> impl Strategy<Value = StpMode> {
    prop_oneof![
        Just(StpMode::None),
        Just(StpMode::ExpireTaker),
        Just(StpMode::ExpireMaker),
        Just(StpMode::ExpireBoth),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]
    #[test]
    fn prop_stp_never_self_fills_when_enabled(
        // Use only 3 users so same-user matches happen frequently.
        cmds in prop::collection::vec(
            (0u8..3, any_side(), (-200i16..200i16), 1u16..5_000u16, any_stp_mode()),
            5..80,
        ),
    ) {
        let mut book = make_book();

        for (i, (user_idx, side, price_offset, qty_millis, stp_mode)) in cmds.iter().enumerate() {
            let order_id = Uuid::from_u128((i as u128) << 16 | 0xCCCC_u128);
            let user_id = Uuid::from_u128(*user_idx as u128 + 1);
            let price = Decimal::new(5_000_000_i64 + *price_offset as i64, 2).max(Decimal::ONE);
            let qty = Decimal::new(*qty_millis as i64, 3);

            let cmd = PlaceOrderCommand {
                order_id,
                user_id,
                symbol: Symbol::new("BTCUSDT"),
                side: *side,
                order_type: OrderType::Limit,
                time_in_force: TimeInForce::Gtc,
                price,
                qty,
                reduce_only: false,
                timestamp: chrono::DateTime::from_timestamp(1_700_000_000 + i as i64, 0)
                    .expect("valid ts"),
                expire_at: None,
                position_idx: 0,
                iceberg_qty: None,
                order_link_id: None,
                slippage_tolerance: None,
                close_position: false,
                stp_mode: *stp_mode,
                request_expire_time: None,
            };

            let events = book.place_order(cmd);

            // Core assertion: when STP is active, no fill may be a self-fill.
            if *stp_mode != StpMode::None {
                for event in &events {
                    if let MatchEvent::Fill(fill) = event {
                        prop_assert_ne!(
                            fill.maker_user_id,
                            fill.taker_user_id,
                            "self-fill detected with stp_mode={:?} at cmd[{}]: \
                             maker_user={} taker_user={}",
                            stp_mode,
                            i,
                            fill.maker_user_id,
                            fill.taker_user_id,
                        );
                    }
                }
            }
        }

        // Final book state must satisfy all structural invariants.
        book.check_invariants_public()
            .unwrap_or_else(|msg| panic!("invariant violated: {msg}"));
    }
}
