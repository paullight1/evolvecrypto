//! Property-based test for WAL replay determinism.
//!
//! The invariant: given the same sequence of commands, direct execution and
//! WAL-write-then-replay must produce identical order book state. This is the
//! foundation of crash recovery — if replay diverges from the original
//! execution, the exchange resumes with a wrong book.

use exchange_common::*;
use exchange_journal::WriteAheadLog;
use exchange_matching_engine::MatchingEngine;
use proptest::prelude::*;
use rust_decimal::Decimal;
use uuid::Uuid;

fn user() -> Uuid {
    Uuid::from_u128(0xAAAA)
}
fn user2() -> Uuid {
    Uuid::from_u128(0xBBBB)
}

fn sym() -> Symbol {
    Symbol::new("BTCUSDT")
}

fn place_cmd(side: Side, price: Decimal, qty: Decimal, uid: Uuid) -> Command {
    Command::PlaceOrder(PlaceOrderCommand {
        order_id: Uuid::new_v4(),
        user_id: uid,
        symbol: sym(),
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
        stp_mode: StpMode::None,
        request_expire_time: None,
    })
}

/// Strategy: generate a random command sequence of limit buy/sell orders.
fn command_sequence() -> impl Strategy<Value = Vec<Command>> {
    let side = prop_oneof![Just(Side::Buy), Just(Side::Sell)];
    let price = (4000u64..6000u64).prop_map(|p| Decimal::new(p as i64, 0));
    let qty = (1u64..100u64).prop_map(|q| Decimal::new(q as i64, 2));
    let uid = prop_oneof![Just(user()), Just(user2())];

    prop::collection::vec((side, price, qty, uid), 5..30)
        .prop_map(|ops| {
            ops.into_iter()
                .map(|(s, p, q, u)| place_cmd(s, p, q, u))
                .collect()
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]
    #[test]
    fn prop_wal_replay_produces_identical_book(commands in command_sequence()) {
        let dir = tempfile::tempdir().unwrap();
        let wal_path = dir.path().join("test.wal");

        // Phase 1: execute commands directly and record the book state.
        let mut engine_direct = MatchingEngine::new();
        engine_direct.add_symbol(sym());
        for cmd in &commands {
            engine_direct.process(cmd.clone());
        }
        let snap_direct = engine_direct.book(&sym()).unwrap().snapshot(50);

        // Phase 2: write commands to WAL.
        {
            let mut wal = WriteAheadLog::open(wal_path.to_str().unwrap(), false).unwrap();
            for cmd in &commands {
                wal.append(cmd).unwrap();
            }
        }

        // Phase 3: replay from WAL into a fresh engine.
        let replayed_commands = WriteAheadLog::replay_from(wal_path.to_str().unwrap(), 0).unwrap();
        let mut engine_replay = MatchingEngine::new();
        engine_replay.add_symbol(sym());
        for (_seq, cmd) in replayed_commands {
            engine_replay.process(cmd);
        }
        let snap_replay = engine_replay.book(&sym()).unwrap().snapshot(50);

        // Assert identical book state.
        prop_assert_eq!(
            snap_direct.bids.len(),
            snap_replay.bids.len(),
            "bid depth mismatch: direct {} vs replay {}",
            snap_direct.bids.len(),
            snap_replay.bids.len(),
        );
        prop_assert_eq!(
            snap_direct.asks.len(),
            snap_replay.asks.len(),
            "ask depth mismatch",
        );

        for (d, r) in snap_direct.bids.iter().zip(snap_replay.bids.iter()) {
            prop_assert_eq!(d.0, r.0, "bid price mismatch");
            prop_assert_eq!(d.1, r.1, "bid qty mismatch at price {}", d.0);
        }
        for (d, r) in snap_direct.asks.iter().zip(snap_replay.asks.iter()) {
            prop_assert_eq!(d.0, r.0, "ask price mismatch");
            prop_assert_eq!(d.1, r.1, "ask qty mismatch at price {}", d.0);
        }

        prop_assert_eq!(
            snap_direct.sequence,
            snap_replay.sequence,
            "sequence number mismatch",
        );
    }
}
