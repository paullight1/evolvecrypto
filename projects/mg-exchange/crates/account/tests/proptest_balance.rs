//! Property-based tests for balance invariants.
//!
//! These tests verify that no sequence of valid operations can violate
//! the fundamental balance invariants:
//! - total >= 0
//! - locked >= 0
//! - available = total - locked >= 0

use exchange_account::balance::Balance;
use proptest::prelude::*;
use rust_decimal::Decimal;

fn valid_amount() -> impl Strategy<Value = Decimal> {
    (1u64..1_000_000u64).prop_map(|cents| Decimal::new(cents as i64, 2))
}

// ── Property 1: deposit never violates invariants ────────────────────────────

proptest! {
    #[test]
    fn prop_deposit_maintains_invariants(
        initial in valid_amount(),
        deposit in valid_amount(),
    ) {
        let mut bal = Balance::zero();
        bal.deposit(initial).unwrap();
        bal.deposit(deposit).unwrap();

        prop_assert!(bal.total() >= Decimal::ZERO);
        prop_assert!(bal.locked() >= Decimal::ZERO);
        prop_assert!(bal.available() >= Decimal::ZERO);
        prop_assert_eq!(bal.available(), bal.total() - bal.locked());
    }
}

// ── Property 2: withdraw cannot exceed available ─────────────────────────────

proptest! {
    #[test]
    fn prop_withdraw_bounded_by_available(
        deposit in valid_amount(),
        withdraw_pct in 0u32..=100u32,
    ) {
        let mut bal = Balance::zero();
        bal.deposit(deposit).unwrap();

        let withdraw_amt = deposit * Decimal::from(withdraw_pct) / Decimal::from(100u32);
        let result = bal.withdraw(withdraw_amt);
        prop_assert!(result.is_ok());
        prop_assert!(bal.available() >= Decimal::ZERO);
    }
}

// ── Property 3: lock + unlock is identity ────────────────────────────────────

proptest! {
    #[test]
    fn prop_lock_unlock_identity(
        deposit in valid_amount(),
        lock_pct in 1u32..=100u32,
    ) {
        let mut bal = Balance::zero();
        bal.deposit(deposit).unwrap();
        let available_before = bal.available();

        let lock_amt = deposit * Decimal::from(lock_pct) / Decimal::from(100u32);
        bal.try_lock(lock_amt).unwrap();
        bal.unlock(lock_amt).unwrap();

        prop_assert_eq!(bal.available(), available_before, "lock+unlock should be identity");
        prop_assert_eq!(bal.total(), deposit);
    }
}

// ── Property 4: random operation sequence preserves invariants ───────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]
    #[test]
    fn prop_random_ops_preserve_invariants(
        ops in prop::collection::vec(
            (0u8..4u8, valid_amount()),
            10..50
        )
    ) {
        let mut bal = Balance::zero();
        // Start with a large deposit so operations have room.
        bal.deposit(Decimal::new(100_000_000, 2)).unwrap(); // $1M

        for (op_type, amount) in ops {
            let small_amount = amount.min(bal.available().max(Decimal::ONE));
            match op_type {
                0 => { let _ = bal.deposit(small_amount); }
                1 => { let _ = bal.withdraw(small_amount.min(bal.available())); }
                2 => { let _ = bal.try_lock(small_amount.min(bal.available())); }
                3 => { let _ = bal.unlock(small_amount.min(bal.locked())); }
                _ => {}
            }

            // Invariants must hold after every operation.
            prop_assert!(bal.total() >= Decimal::ZERO, "total < 0 after op {}", op_type);
            prop_assert!(bal.locked() >= Decimal::ZERO, "locked < 0 after op {}", op_type);
            prop_assert!(bal.available() >= Decimal::ZERO, "available < 0 after op {}", op_type);
            prop_assert_eq!(bal.available(), bal.total() - bal.locked());
        }
    }
}

// ── Property 5: total money is conserved in settle ───────────────────────────

proptest! {
    #[test]
    fn prop_deposit_withdraw_conserves_total(
        amount in valid_amount(),
    ) {
        let mut bal = Balance::zero();
        bal.deposit(amount).unwrap();
        bal.withdraw(amount).unwrap();
        prop_assert_eq!(bal.total(), Decimal::ZERO, "deposit+withdraw same amount → zero");
    }
}
