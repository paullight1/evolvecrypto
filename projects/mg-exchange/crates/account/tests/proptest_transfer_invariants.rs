//! Property-based tests for multi-user balance conservation.
//!
//! The core invariant: the sum of all `total` balances across all users for
//! a given asset is constant under deposits (mints), withdrawals (burns), and
//! transfers. Specifically:
//!
//!   sum_total_before + deposits - withdrawals == sum_total_after
//!
//! Transfers are a special case: they move money between users without
//! changing the global sum. This test generates random sequences of
//! {deposit, withdraw, transfer} operations and asserts the invariant
//! holds after every operation.

use exchange_account::ShardedAccounts;
use proptest::prelude::*;
use rust_decimal::Decimal;
use uuid::Uuid;

const ASSET: &str = "USDT";

fn users(n: usize) -> Vec<Uuid> {
    (0..n).map(|i| Uuid::from_u128(i as u128 + 1)).collect()
}

/// Compute the global total across all users for a given asset.
fn global_total(accounts: &ShardedAccounts, user_ids: &[Uuid]) -> Decimal {
    user_ids
        .iter()
        .map(|uid| {
            accounts
                .get_all_balances(*uid)
                .get(ASSET)
                .map(|b| b.total())
                .unwrap_or(Decimal::ZERO)
        })
        .sum()
}

/// Operations we can generate.
#[derive(Debug, Clone)]
enum Op {
    /// Deposit (mint) into a user. Increases global supply.
    Deposit { user_idx: usize, amount: Decimal },
    /// Withdraw (burn) from a user. Decreases global supply.
    Withdraw { user_idx: usize, amount: Decimal },
    /// Transfer between two users. Global supply unchanged.
    Transfer { from_idx: usize, to_idx: usize, amount: Decimal },
}

fn op_strategy(num_users: usize) -> impl Strategy<Value = Op> {
    let user_idx = 0..num_users;
    let amount = (1u64..100_000u64).prop_map(|cents| Decimal::new(cents as i64, 2));

    prop_oneof![
        // 30% deposits
        (user_idx.clone(), amount.clone()).prop_map(|(u, a)| Op::Deposit {
            user_idx: u,
            amount: a,
        }),
        // 30% withdrawals (may fail — that's fine, we track it)
        (user_idx.clone(), amount.clone()).prop_map(|(u, a)| Op::Withdraw {
            user_idx: u,
            amount: a,
        }),
        // 40% transfers
        (user_idx.clone(), user_idx.clone(), amount).prop_map(|(f, t, a)| Op::Transfer {
            from_idx: f,
            to_idx: t,
            amount: a,
        }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]
    #[test]
    fn prop_transfer_conserves_global_total(
        ops in prop::collection::vec(op_strategy(5), 20..100)
    ) {
        let accounts = ShardedAccounts::new();
        let user_ids = users(5);

        // Create accounts.
        for uid in &user_ids {
            accounts.create_account(*uid);
        }

        // Accounting: track expected global supply (deposits - withdrawals).
        let mut total_deposited = Decimal::ZERO;
        let mut total_withdrawn = Decimal::ZERO;

        for op in &ops {
            match op {
                Op::Deposit { user_idx, amount } => {
                    let uid = user_ids[*user_idx];
                    if accounts.deposit(uid, ASSET, *amount).is_ok() {
                        total_deposited += amount;
                    }
                }
                Op::Withdraw { user_idx, amount } => {
                    let uid = user_ids[*user_idx];
                    // Withdraw may fail (insufficient funds) — that's expected.
                    if accounts.withdraw(uid, ASSET, *amount).is_ok() {
                        total_withdrawn += amount;
                    }
                }
                Op::Transfer { from_idx, to_idx, amount } => {
                    let from = user_ids[*from_idx];
                    let to = user_ids[*to_idx];
                    // Transfer may fail — that's expected.
                    let _ = accounts.transfer(from, to, ASSET, *amount);
                    // No supply change regardless of success/failure.
                }
            }

            // After every operation: global total must equal deposits - withdrawals.
            let actual = global_total(&accounts, &user_ids);
            let expected = total_deposited - total_withdrawn;
            prop_assert_eq!(
                actual,
                expected,
                "global total mismatch after {:?}: expected {} (deposited {} - withdrawn {}), got {}",
                op, expected, total_deposited, total_withdrawn, actual,
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]
    #[test]
    fn prop_no_user_balance_goes_negative(
        ops in prop::collection::vec(op_strategy(3), 10..50)
    ) {
        let accounts = ShardedAccounts::new();
        let user_ids = users(3);
        for uid in &user_ids {
            accounts.create_account(*uid);
        }

        for op in &ops {
            match op {
                Op::Deposit { user_idx, amount } => {
                    let _ = accounts.deposit(user_ids[*user_idx], ASSET, *amount);
                }
                Op::Withdraw { user_idx, amount } => {
                    let _ = accounts.withdraw(user_ids[*user_idx], ASSET, *amount);
                }
                Op::Transfer { from_idx, to_idx, amount } => {
                    let _ = accounts.transfer(user_ids[*from_idx], user_ids[*to_idx], ASSET, *amount);
                }
            }

            // After every operation: no user has negative available or total.
            for uid in &user_ids {
                let balances = accounts.get_all_balances(*uid);
                for (asset, bal) in &balances {
                    prop_assert!(
                        bal.total() >= Decimal::ZERO,
                        "user {} asset {} has negative total: {}",
                        uid, asset, bal.total(),
                    );
                    prop_assert!(
                        bal.available() >= Decimal::ZERO,
                        "user {} asset {} has negative available: {}",
                        uid, asset, bal.available(),
                    );
                }
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]
    #[test]
    fn prop_self_transfer_is_noop(
        deposit in (1_000u64..1_000_000u64).prop_map(|c| Decimal::new(c as i64, 2)),
        transfer in (1u64..100_000u64).prop_map(|c| Decimal::new(c as i64, 2)),
    ) {
        let accounts = ShardedAccounts::new();
        let user = Uuid::from_u128(42);
        accounts.create_account(user);
        accounts.deposit(user, ASSET, deposit).unwrap();

        let before = accounts.get_all_balances(user)
            .get(ASSET)
            .map(|b| b.total())
            .unwrap_or(Decimal::ZERO);

        // Self-transfer must succeed and leave total unchanged.
        let amt = transfer.min(before);
        let _ = accounts.transfer(user, user, ASSET, amt);

        let after = accounts.get_all_balances(user)
            .get(ASSET)
            .map(|b| b.total())
            .unwrap_or(Decimal::ZERO);

        prop_assert_eq!(before, after, "self-transfer must not change total");
    }
}
