use exchange_common::{futures::Position, Command, OrderId};
use exchange_matching_engine::{MatchingEngine, EnginePool};

/// Summary of a WAL replay operation.
#[derive(Debug, Default)]
pub struct ReplayResult {
    /// Total commands successfully processed by the matching engine.
    pub commands_replayed: u64,
    /// Commands skipped because the order was already in a terminal state.
    pub commands_skipped: u64,
    /// Errors encountered during replay: `(seq, description)`.
    /// These are non-fatal — the command was skipped and replay continued.
    pub errors: Vec<(u64, String)>,
    /// Sequence number of the last entry seen (0 if none).
    pub last_seq: u64,
}

/// Replay a sequence of WAL commands into a matching engine.
///
/// For each command:
/// - If it references an order that `is_terminal` reports as already
///   filled or cancelled (e.g. persisted in a database), the command is
///   skipped and `commands_skipped` is incremented.
/// - Otherwise the command is forwarded to `engine.process()`.
///
/// The `is_terminal` callback receives the `OrderId` of the affected order.
/// For `PlaceOrder` commands that is the new order's ID; for `CancelOrder`
/// commands it is the ID of the order being cancelled.
///
/// Replay never panics: any error from the engine is recorded in
/// `ReplayResult::errors` and replay continues with the next command.
///
/// Progress is logged at `INFO` level every 10 000 commands.
pub fn replay_into_engine(
    commands: Vec<(u64, Command)>,
    engine: &mut MatchingEngine,
    is_terminal: impl Fn(&OrderId) -> bool,
) -> ReplayResult {
    let total = commands.len();
    let mut result = ReplayResult::default();

    for (seq, cmd) in commands {
        result.last_seq = result.last_seq.max(seq);

        // Balance events are not for the matching engine — skip them during
        // engine replay.  They are applied separately by the account layer.
        let order_id: &OrderId = match &cmd {
            Command::PlaceOrder(c) => &c.order_id,
            Command::CancelOrder(c) => &c.order_id,
            Command::BalanceDeposit { .. }
            | Command::BalanceWithdraw { .. }
            | Command::FundingPayment { .. }
            | Command::PositionSnapshot { .. } => {
                tracing::debug!(seq, "WAL replay: skipping balance event (not for matching engine)");
                result.commands_skipped += 1;
                continue;
            }
        };

        if is_terminal(order_id) {
            tracing::debug!(seq, %order_id, "WAL replay: skipping terminal order");
            result.commands_skipped += 1;
            continue;
        }

        // engine.process() is infallible — it returns events, never errors.
        let _events = engine.process(cmd);
        result.commands_replayed += 1;

        // Periodic progress logging.
        if result.commands_replayed % 10_000 == 0 {
            tracing::info!(
                replayed = result.commands_replayed,
                skipped = result.commands_skipped,
                total,
                "WAL replay in progress"
            );
        }
    }

    tracing::info!(
        commands_replayed = result.commands_replayed,
        commands_skipped = result.commands_skipped,
        errors = result.errors.len(),
        last_seq = result.last_seq,
        "WAL replay complete"
    );

    result
}

/// Replay WAL commands into an [`EnginePool`] (per-symbol threads).
///
/// Same semantics as [`replay_into_engine`], but dispatches each command to the
/// correct symbol's engine thread via `pool.process()`.
pub fn replay_into_pool(
    commands: Vec<(u64, Command)>,
    pool: &EnginePool,
    is_terminal: impl Fn(&OrderId) -> bool,
) -> ReplayResult {
    let total = commands.len();
    let mut result = ReplayResult::default();

    for (seq, cmd) in commands {
        result.last_seq = result.last_seq.max(seq);

        // Balance events are not for the matching engine — skip them during
        // engine replay.  They are applied separately by the account layer.
        let (order_id, symbol) = match &cmd {
            Command::PlaceOrder(c) => (c.order_id, c.symbol.clone()),
            Command::CancelOrder(c) => (c.order_id, c.symbol.clone()),
            Command::BalanceDeposit { .. }
            | Command::BalanceWithdraw { .. }
            | Command::FundingPayment { .. }
            | Command::PositionSnapshot { .. } => {
                tracing::debug!(seq, "WAL replay: skipping balance event (not for matching engine)");
                result.commands_skipped += 1;
                continue;
            }
        };

        if is_terminal(&order_id) {
            tracing::debug!(seq, %order_id, "WAL replay: skipping terminal order");
            result.commands_skipped += 1;
            continue;
        }

        match pool.process(&symbol, cmd) {
            Ok(_events) => {
                result.commands_replayed += 1;
            }
            Err(e) => {
                tracing::warn!(seq, %order_id, error = %e, "WAL replay: engine error");
                result.errors.push((seq, e.to_string()));
            }
        }

        if result.commands_replayed % 10_000 == 0 && result.commands_replayed > 0 {
            tracing::info!(
                replayed = result.commands_replayed,
                skipped = result.commands_skipped,
                total,
                "WAL replay in progress"
            );
        }
    }

    tracing::info!(
        commands_replayed = result.commands_replayed,
        commands_skipped = result.commands_skipped,
        errors = result.errors.len(),
        last_seq = result.last_seq,
        "WAL replay into pool complete"
    );

    result
}

/// Extract the most recent `PositionSnapshot` from a WAL command stream.
///
/// Scans `commands` in reverse and returns the `positions` vector from the
/// first (i.e. most recent) `Command::PositionSnapshot` found.  Returns an
/// empty vec if no snapshot exists.
///
/// The caller uses this to restore the in-memory `PositionManager` after a
/// WAL replay, so open futures positions survive process restarts without
/// re-deriving them from fill events.
pub fn extract_latest_position_snapshot(commands: &[(u64, Command)]) -> Vec<Position> {
    for (_seq, cmd) in commands.iter().rev() {
        if let Command::PositionSnapshot { positions } = cmd {
            return positions.clone();
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use exchange_common::*;
    use exchange_matching_engine::MatchingEngine;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn make_engine() -> MatchingEngine {
        let mut e = MatchingEngine::new();
        e.add_symbol(Symbol::new("BTCUSDT"));
        e
    }

    fn place_cmd(order_id: Uuid, price: rust_decimal::Decimal) -> Command {
        Command::PlaceOrder(PlaceOrderCommand {
            order_id,
            user_id: Uuid::nil(),
            symbol: Symbol::new("BTCUSDT"),
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
            price,
            qty: dec!(1),
            timestamp: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap().to_utc(),
            reduce_only: false,
            expire_at: None,
            position_idx: 0,
            iceberg_qty: None,
            order_link_id: None,
            slippage_tolerance: None,
            close_position: false,
            stp_mode: StpMode::default(),
            request_expire_time: None,
        })
    }

    fn cancel_cmd(order_id: Uuid) -> Command {
        Command::CancelOrder(CancelOrderCommand {
            order_id,
            user_id: Uuid::nil(),
            symbol: Symbol::new("BTCUSDT"),
            timestamp: chrono::Utc::now(),
        })
    }

    // Test 1: empty command list → zero replayed.
    #[test]
    fn test_replay_empty() {
        let mut engine = make_engine();
        let result = replay_into_engine(vec![], &mut engine, |_| false);
        assert_eq!(result.commands_replayed, 0);
        assert_eq!(result.commands_skipped, 0);
        assert_eq!(result.last_seq, 0);
        assert_eq!(engine.book(&Symbol::new("BTCUSDT")).unwrap().order_count(), 0);
    }

    // Test 2: 5 place orders → engine has 5 resting orders.
    #[test]
    fn test_replay_five_place_orders() {
        let mut engine = make_engine();
        let commands: Vec<(u64, Command)> = (1u64..=5)
            .map(|seq| (seq, place_cmd(Uuid::new_v4(), dec!(50000) + rust_decimal::Decimal::from(seq))))
            .collect();

        let result = replay_into_engine(commands, &mut engine, |_| false);

        assert_eq!(result.commands_replayed, 5);
        assert_eq!(result.commands_skipped, 0);
        assert_eq!(engine.book(&Symbol::new("BTCUSDT")).unwrap().order_count(), 5);
    }

    // Test 3: place then cancel → engine has 0 resting orders.
    #[test]
    fn test_replay_place_then_cancel() {
        let mut engine = make_engine();
        let oid = Uuid::new_v4();
        let commands = vec![
            (1, place_cmd(oid, dec!(50000))),
            (2, cancel_cmd(oid)),
        ];

        let result = replay_into_engine(commands, &mut engine, |_| false);

        assert_eq!(result.commands_replayed, 2);
        assert_eq!(result.commands_skipped, 0);
        assert_eq!(engine.book(&Symbol::new("BTCUSDT")).unwrap().order_count(), 0);
    }

    // Test 4: is_terminal returns true → command is skipped, engine untouched.
    #[test]
    fn test_replay_skips_terminal_order() {
        let mut engine = make_engine();
        let terminal_id = Uuid::new_v4();
        let other_id = Uuid::new_v4();

        let commands = vec![
            (1, place_cmd(terminal_id, dec!(50000))),
            (2, place_cmd(other_id, dec!(51000))),
        ];

        let result = replay_into_engine(commands, &mut engine, |id| *id == terminal_id);

        assert_eq!(result.commands_replayed, 1);
        assert_eq!(result.commands_skipped, 1);
        // Only the non-terminal order should be in the book.
        assert_eq!(engine.book(&Symbol::new("BTCUSDT")).unwrap().order_count(), 1);
    }

    // Test 9: price-time priority is preserved (first placed has best priority).
    //
    // Uses three distinct `user_id`s for the two makers and the taker. The
    // new matching engine (see `StpMode` footgun warning in
    // `crates/common/src/types.rs`) applies Bybit-style self-trade
    // prevention by default — `StpMode::default() == ExpireTaker`. If all
    // three orders used `Uuid::nil()`, the taker would be cancelled on its
    // first lookup at the resting level and no fill would occur. Giving
    // each order a distinct user_id is the production-realistic setup and
    // matches the scenario the test is meant to describe: two different
    // makers rest at the same price, a third user comes in and takes.
    #[test]
    fn test_replay_preserves_price_time_priority() {
        let mut engine = make_engine();

        // Two sell orders at the same price — first placed should be matched first.
        let first_id = Uuid::new_v4();
        let second_id = Uuid::new_v4();
        let first_user = Uuid::new_v4();
        let second_user = Uuid::new_v4();
        let taker_user = Uuid::new_v4();

        let commands = vec![
            (1, Command::PlaceOrder(PlaceOrderCommand {
                order_id: first_id,
                user_id: first_user,
                symbol: Symbol::new("BTCUSDT"),
                side: Side::Sell,
                order_type: OrderType::Limit,
                time_in_force: TimeInForce::Gtc,
                price: dec!(50000),
                qty: dec!(1),
                timestamp: chrono::DateTime::from_timestamp(1_700_000_001, 0).unwrap().to_utc(),
                reduce_only: false,
                expire_at: None,
                position_idx: 0,
                iceberg_qty: None,
                order_link_id: None,
                slippage_tolerance: None,
                close_position: false,
                stp_mode: StpMode::default(),
                request_expire_time: None,
            })),
            (2, Command::PlaceOrder(PlaceOrderCommand {
                order_id: second_id,
                user_id: second_user,
                symbol: Symbol::new("BTCUSDT"),
                side: Side::Sell,
                order_type: OrderType::Limit,
                time_in_force: TimeInForce::Gtc,
                price: dec!(50000),
                qty: dec!(1),
                timestamp: chrono::DateTime::from_timestamp(1_700_000_002, 0).unwrap().to_utc(),
                reduce_only: false,
                expire_at: None,
                position_idx: 0,
                iceberg_qty: None,
                order_link_id: None,
                slippage_tolerance: None,
                close_position: false,
                stp_mode: StpMode::default(),
                request_expire_time: None,
            })),
        ];

        let result = replay_into_engine(commands, &mut engine, |_| false);
        assert_eq!(result.commands_replayed, 2);

        // Now send a market buy that matches one sell — it must match the first one placed.
        let buy_events = engine.process(Command::PlaceOrder(PlaceOrderCommand {
            order_id: Uuid::new_v4(),
            user_id: taker_user,
            symbol: Symbol::new("BTCUSDT"),
            side: Side::Buy,
            order_type: OrderType::Market,
            time_in_force: TimeInForce::Ioc,
            price: dec!(0),
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
        }));

        // Exactly one fill must have occurred and the maker must be first_id.
        let fill = buy_events.iter().find_map(|e| {
            if let MatchEvent::Fill(f) = e { Some(f) } else { None }
        });
        assert!(fill.is_some(), "expected a fill event");
        assert_eq!(fill.unwrap().maker_order_id, first_id);
    }
}
