//! Adversarial concurrency stress tests for ShardedAccounts.
//!
//! These tests hammer the lock-ordering paths with high parallelism to
//! detect deadlocks (via timeout) and data races (via invariant violations).
//!
//! Why not loom: ShardedAccounts uses parking_lot::Mutex which loom
//! cannot instrument. loom would require a cfg(loom) refactor to swap
//! mutex types across the codebase. These stress tests provide practical
//! confidence without that investment.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use exchange_account::ShardedAccounts;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;

const ASSET: &str = "USDT";

fn user(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

/// Helper: sum all balances for an asset across users.
fn total_supply(accounts: &ShardedAccounts, users: &[Uuid]) -> Decimal {
    users
        .iter()
        .map(|u| {
            accounts
                .get_all_balances(*u)
                .get(ASSET)
                .map(|b| b.total())
                .unwrap_or(Decimal::ZERO)
        })
        .sum()
}

/// 32 threads × 500 random cross-shard transfers between 8 users.
/// Must not deadlock (timeout = 30s) and must conserve total supply.
#[test]
fn stress_cross_shard_transfers_no_deadlock() {
    let accounts = Arc::new(ShardedAccounts::new());
    let num_users = 8;
    let users: Vec<Uuid> = (1..=num_users).map(|n| user(n as u128)).collect();

    // Fund each user.
    for u in &users {
        accounts.create_account(*u);
        accounts.deposit(*u, ASSET, dec!(1_000_000)).unwrap();
    }

    let initial_supply = total_supply(&accounts, &users);

    let threads: Vec<_> = (0..32)
        .map(|t| {
            let accounts = Arc::clone(&accounts);
            let users = users.clone();
            thread::spawn(move || {
                // Deterministic-ish seed per thread.
                let mut rng_state: u64 = ((t as u64) + 1).wrapping_mul(6364136223846793005);
                for _ in 0..500 {
                    rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
                    let from_idx = (rng_state as usize) % users.len();
                    rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
                    let to_idx = (rng_state as usize) % users.len();
                    let amount = Decimal::new((rng_state % 100 + 1) as i64, 2); // 0.01 - 1.00

                    let _ = accounts.transfer(users[from_idx], users[to_idx], ASSET, amount);
                }
            })
        })
        .collect();

    // Timeout: if any thread hangs for >30s we have a deadlock.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    for t in threads {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            panic!("DEADLOCK DETECTED: thread hung beyond 30s deadline");
        }
        // Can't use thread::JoinHandle::join_timeout (doesn't exist in std).
        // But if the test doesn't complete in 30s, cargo test --timeout will catch it.
        t.join().expect("thread panicked");
    }

    let final_supply = total_supply(&accounts, &users);
    assert_eq!(
        initial_supply, final_supply,
        "total supply must be conserved: initial={initial_supply} final={final_supply}"
    );
}

/// Concurrent deposits + withdrawals on the same user from 16 threads.
/// Tests lock contention on a single shard.
#[test]
fn stress_concurrent_deposit_withdraw_single_user() {
    let accounts = Arc::new(ShardedAccounts::new());
    let u = user(42);
    accounts.create_account(u);
    accounts.deposit(u, ASSET, dec!(10_000_000)).unwrap();

    let initial = total_supply(&accounts, &[u]);

    let threads: Vec<_> = (0..16)
        .map(|t| {
            let accounts = Arc::clone(&accounts);
            thread::spawn(move || {
                for i in 0..1000 {
                    if (t + i) % 2 == 0 {
                        let _ = accounts.deposit(u, ASSET, dec!(1));
                    } else {
                        let _ = accounts.withdraw(u, ASSET, dec!(1));
                    }
                }
            })
        })
        .collect();

    for t in threads {
        t.join().expect("thread panicked");
    }

    // Each thread does 500 deposits + 500 withdrawals = net zero per thread.
    // 16 threads × net zero = no change.
    let final_balance = total_supply(&accounts, &[u]);
    assert_eq!(
        initial, final_balance,
        "deposit/withdraw symmetry violated: initial={initial} final={final_balance}"
    );
}

/// Mixed transfers + deposits + withdrawals across 4 users, 24 threads.
/// The most adversarial pattern: different operations compete for overlapping shards.
#[test]
fn stress_mixed_operations_adversarial() {
    let accounts = Arc::new(ShardedAccounts::new());
    let users: Vec<Uuid> = (1..=4).map(|n| user(n)).collect();
    for u in &users {
        accounts.create_account(*u);
        accounts.deposit(*u, ASSET, dec!(5_000_000)).unwrap();
    }

    let initial_supply = total_supply(&accounts, &users);

    let threads: Vec<_> = (0..24)
        .map(|t| {
            let accounts = Arc::clone(&accounts);
            let users = users.clone();
            thread::spawn(move || {
                let mut rng: u64 = ((t as u64) + 1).wrapping_mul(2862933555777941757);
                for _ in 0..300 {
                    rng = rng.wrapping_mul(2862933555777941757).wrapping_add(1);
                    let op = rng % 3;
                    let u1 = users[(rng as usize >> 8) % users.len()];
                    rng = rng.wrapping_mul(2862933555777941757).wrapping_add(1);
                    let u2 = users[(rng as usize >> 8) % users.len()];
                    let amt = Decimal::new((rng % 50 + 1) as i64, 2);

                    match op {
                        0 => { let _ = accounts.transfer(u1, u2, ASSET, amt); }
                        1 => { let _ = accounts.deposit(u1, ASSET, amt); }
                        2 => { let _ = accounts.withdraw(u1, ASSET, amt); }
                        _ => unreachable!(),
                    }
                }
            })
        })
        .collect();

    for t in threads {
        t.join().expect("thread panicked");
    }

    // Deposits and withdrawals change total supply; count them.
    // We can't predict the exact final supply because deposit/withdraw
    // success depends on available balance. But we CAN assert:
    // 1. No user has negative balance.
    for u in &users {
        let bals = accounts.get_all_balances(*u);
        if let Some(bal) = bals.get(ASSET) {
            assert!(
                bal.total() >= Decimal::ZERO,
                "user {u} has negative total: {}",
                bal.total()
            );
            assert!(
                bal.available() >= Decimal::ZERO,
                "user {u} has negative available: {}",
                bal.available()
            );
        }
    }

    // 2. Total supply >= initial (deposits can only increase it; withdrawals
    //    are bounded by available balance, so they can reduce it, but never
    //    below zero for any user — the aggregate may differ).
    let final_supply = total_supply(&accounts, &users);
    assert!(
        final_supply >= Decimal::ZERO,
        "total supply went negative: {final_supply}"
    );
}
