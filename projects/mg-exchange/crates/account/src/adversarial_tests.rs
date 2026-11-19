/// # Adversarial / Penetration-Level Tests — Account Module
///
/// These tests simulate what a malicious insider or attacker would attempt.
/// They are modelled after the exact failure modes that destroyed FTX,
/// caused the Bybit hack, and similar exchange collapses.
///
/// Every test must PASS for the system to be considered safe. A failure
/// indicates a real security or integrity bug.
#[cfg(test)]
mod adversarial {
    use std::collections::HashMap;

    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    use crate::account_manager::AccountManager;
    use crate::balance::Balance;
    use crate::error::AccountError;
    use crate::unified_account::{collateral_haircut, AccountMode, UnifiedAccount};

    // ─────────────────────────────────────────────────────────────────────────
    // Helpers
    // ─────────────────────────────────────────────────────────────────────────

    fn new_user_id() -> Uuid {
        Uuid::new_v4()
    }

    fn btc_prices() -> HashMap<String, Decimal> {
        let mut p = HashMap::new();
        p.insert("USDT".to_string(), dec!(1));
        p.insert("BTC".to_string(), dec!(50000));
        p.insert("ETH".to_string(), dec!(3000));
        p.insert("EXC".to_string(), dec!(5));
        p.insert("SHIB".to_string(), dec!(0.00001));
        p
    }

    // ─────────────────────────────────────────────────────────────────────────
    // CATEGORY 1 — Anti-FTX: Insider Abuse Prevention
    // ─────────────────────────────────────────────────────────────────────────

    /// FTX FAILURE: no account should be able to withdraw more than its
    /// available balance, regardless of any "special" account status.
    ///
    /// We test every code path that can reduce a balance:
    ///   1. withdraw() above available
    ///   2. deduct_locked() above locked
    ///   3. unlock() above locked
    ///   4. Negative deposit (create funds from air)
    ///   5. Boundary: exactly 1 satoshi over the limit
    #[test]
    fn test_no_account_can_have_negative_balance() {
        let mut b = Balance::zero();
        b.deposit(dec!(10000)).unwrap();
        b.try_lock(dec!(6000)).unwrap(); // locked=6000, available=4000

        // --- Path 1: withdraw more than available ---
        let err = b.withdraw(dec!(4000.00000001)).unwrap_err();
        assert!(
            matches!(err, AccountError::InsufficientBalance { .. }),
            "withdraw over available must fail: {:?}",
            err
        );
        // State unchanged — no partial mutation.
        assert_eq!(b.total(), dec!(10000));
        assert_eq!(b.available(), dec!(4000));

        // --- Path 2: deduct_locked more than locked ---
        let err = b.deduct_locked(dec!(6000.00000001)).unwrap_err();
        assert!(
            matches!(err, AccountError::InsufficientLocked { .. }),
            "deduct_locked over locked must fail: {:?}",
            err
        );
        assert_eq!(b.locked(), dec!(6000));

        // --- Path 3: unlock more than locked ---
        let err = b.unlock(dec!(6000.00000001)).unwrap_err();
        assert!(
            matches!(err, AccountError::InsufficientLocked { .. }),
            "unlock over locked must fail: {:?}",
            err
        );
        assert_eq!(b.locked(), dec!(6000));

        // --- Path 4: negative deposit (money creation) ---
        let err = b.deposit(dec!(-1)).unwrap_err();
        assert!(
            matches!(err, AccountError::NegativeAmount(_)),
            "negative deposit must fail: {:?}",
            err
        );
        assert_eq!(b.total(), dec!(10000));

        // --- Path 5: negative lock ---
        let err = b.try_lock(dec!(-0.00000001)).unwrap_err();
        assert!(
            matches!(err, AccountError::NegativeAmount(_)),
            "negative lock must fail: {:?}",
            err
        );

        // --- Final invariant check ---
        assert!(b.total() >= Decimal::ZERO, "total went negative");
        assert!(b.locked() >= Decimal::ZERO, "locked went negative");
        assert!(b.available() >= Decimal::ZERO, "available went negative");
        assert_eq!(b.total(), b.available() + b.locked());
    }

    /// Boundary test: one satoshi over the limit in every direction.
    ///
    /// Confirms the check is strict (`>`) not lax (`>=`), so no off-by-one
    /// exists at the boundary that would let an attacker extract dust.
    #[test]
    fn test_boundary_one_satoshi_over_limit_always_rejected() {
        let mut b = Balance::zero();
        b.deposit(dec!(100)).unwrap();

        // Withdraw exactly 100 — must succeed.
        let mut b2 = b.clone();
        b2.withdraw(dec!(100)).unwrap();
        assert_eq!(b2.total(), Decimal::ZERO);

        // Withdraw 100.00000001 — must fail.
        let err = b.withdraw(dec!(100.00000001)).unwrap_err();
        assert!(
            matches!(err, AccountError::InsufficientBalance { .. }),
            "one satoshi over must be rejected: {:?}",
            err
        );
        assert_eq!(b.total(), dec!(100), "state must be unchanged after rejection");

        // Lock 100 (all of it), then try to lock 0.00000001 more.
        let mut b3 = Balance::zero();
        b3.deposit(dec!(100)).unwrap();
        b3.try_lock(dec!(100)).unwrap();
        let err = b3.try_lock(dec!(0.00000001)).unwrap_err();
        assert!(
            matches!(err, AccountError::InsufficientBalance { .. }),
            "one satoshi over available must fail: {:?}",
            err
        );
        assert_eq!(b3.locked(), dec!(100));
    }

    /// Verify there is no way to remove funds from the system other than the
    /// sanctioned paths (withdraw / deduct_locked after fill).
    ///
    /// We exhaustively check that:
    ///   - Total assets are conserved across deposit + lock + deduct_locked + credit
    ///   - credit() can only add, never subtract
    ///   - No operation silently reduces total without a corresponding increase elsewhere
    #[test]
    fn test_no_hidden_withdrawal_backdoor_in_balance_operations() {
        let mut b = Balance::zero();
        b.deposit(dec!(50000)).unwrap();
        let initial_total = b.total();

        // Simulate full trade lifecycle:
        //   lock → deduct_locked (funds leave) → credit (funds enter buyer's account)
        // In a real trade the deducted amount flows to the counterparty via credit.
        // Here we just verify that EVERY reduction is explicitly tracked.

        b.try_lock(dec!(10000)).unwrap();
        // Total still 50000 — lock just moves available→locked.
        assert_eq!(b.total(), initial_total);

        // Simulate seller's fill: deduct_locked 10000.
        b.deduct_locked(dec!(10000)).unwrap();
        // Total reduced by exactly the fill cost — no hidden leakage.
        assert_eq!(b.total(), dec!(40000));
        assert_eq!(initial_total - b.total(), dec!(10000), "exactly 10000 was removed");

        // credit() only increases total — cannot be negative (already tested above).
        b.credit(dec!(10000)).unwrap();
        assert_eq!(b.total(), dec!(50000));

        // Negative credit is rejected — cannot silently drain via this path.
        let err = b.credit(dec!(-1)).unwrap_err();
        assert!(matches!(err, AccountError::NegativeAmount(_)));
        assert_eq!(b.total(), dec!(50000));
    }

    /// FTX FAILURE: EXC (exchange token) must NEVER contribute collateral value.
    ///
    /// FTX accepted FTT (its own token) as margin collateral. When FTT price
    /// collapsed, every account backed by FTT was simultaneously insolvent.
    /// This test verifies the structural fix: EXC haircut == 0.
    #[test]
    fn test_exchange_token_excluded_from_margin_collateral() {
        // 1. Direct haircut check.
        assert_eq!(
            collateral_haircut("EXC"),
            Decimal::ZERO,
            "EXC must have a zero haircut — anti-FTT policy"
        );

        // 2. Unified account with 1,000,000 EXC and zero USDT has zero collateral.
        let user_id = new_user_id();
        let mut account = UnifiedAccount::new(user_id, AccountMode::Unified);
        account.set_wallet_balance("EXC", dec!(1_000_000), Decimal::ZERO, Decimal::ZERO);
        account.calculate_equity(&btc_prices(), Decimal::ZERO, Decimal::ZERO);

        assert_eq!(
            account.total_collateral_usd,
            Decimal::ZERO,
            "1M EXC at $5 each should produce ZERO collateral value"
        );

        // 3. Cannot open a leveraged position using only EXC collateral.
        // Even with 1M EXC worth $5M at market, collateral is $0.
        assert!(
            !account.can_open_position(dec!(1)),
            "should not be able to open any position using only EXC"
        );
    }

    /// Haircut caps prevent disproportionate positions from illiquid tokens.
    ///
    /// An asset unknown to the system gets an 80% haircut (conservative default).
    /// Its collateral contribution is therefore capped below face value.
    #[test]
    fn test_illiquid_token_collateral_capped_by_haircut() {
        let user_id = new_user_id();
        let mut account = UnifiedAccount::new(user_id, AccountMode::Unified);

        // "MEME" token: 1,000,000 tokens at $0.001 each = $1,000 face value.
        // Haircut = 0.80 (unknown token) → collateral = $800.
        let mut prices = btc_prices();
        prices.insert("MEME".to_string(), dec!(0.001));
        account.set_wallet_balance("MEME", dec!(1_000_000), Decimal::ZERO, Decimal::ZERO);
        account.calculate_equity(&prices, Decimal::ZERO, Decimal::ZERO);

        let expected_collateral = dec!(1_000_000) * dec!(0.001) * dec!(0.80); // = 800
        assert_eq!(
            account.total_collateral_usd,
            expected_collateral,
            "illiquid token must be discounted by 20% haircut"
        );

        // Cannot open a position requiring $900 margin (collateral only $800).
        assert!(!account.can_open_position(dec!(900)));
        // Can open a position requiring $800 (exactly at limit).
        assert!(account.can_open_position(dec!(800)));
    }

    /// Cross-symbol double-spend prevention: a user with exactly 10,000 USDT
    /// must not be able to lock funds for two simultaneous orders that together
    /// require 16,000 USDT.
    ///
    /// The AccountManager's try_lock is the atomicity gate; both orders are
    /// attempted sequentially (simulating two concurrent requests serialised by
    /// the account mutex). The second must fail.
    #[test]
    fn test_concurrent_orders_cannot_double_spend_available_balance() {
        let mut mgr = AccountManager::new();
        let user_id = new_user_id();
        mgr.create_account(user_id);
        mgr.deposit(user_id, "USDT", dec!(10000)).unwrap();

        // First order: needs 8,000 USDT — succeeds.
        mgr.try_lock_margin(user_id, "USDT", dec!(8000)).unwrap();

        // Second order: needs 8,000 more USDT — only 2,000 available now, must fail.
        let err = mgr.try_lock_margin(user_id, "USDT", dec!(8000)).unwrap_err();
        assert!(
            matches!(err, AccountError::InsufficientBalance { needed, available }
                if needed == dec!(8000) && available == dec!(2000)),
            "second order must be rejected: {:?}",
            err
        );

        // Total locked must not exceed total balance.
        let bal = mgr.get_balance(&user_id, "USDT").unwrap();
        assert!(
            bal.locked() <= bal.total(),
            "locked ({}) must never exceed total ({})",
            bal.locked(),
            bal.total()
        );
        assert_eq!(bal.locked(), dec!(8000));
        assert_eq!(bal.available(), dec!(2000));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // CATEGORY 2 — System Integrity: Balance Conservation
    // ─────────────────────────────────────────────────────────────────────────

    /// After 1,000 deposit/lock/unlock/deduct/credit operations the identity
    /// `total == available + locked` must hold exactly after every step.
    ///
    /// Any deviation represents a balance-sheet discrepancy — money created
    /// or destroyed.
    #[test]
    fn test_total_system_identity_holds_through_1000_operations() {
        let mut b = Balance::zero();
        b.deposit(dec!(1_000_000)).unwrap();

        // Deterministic pseudo-random sequence using arithmetic cycling.
        // Each step uses a different modular amount to hit diverse code paths.
        for i in 0u64..1000 {
            let amount = Decimal::from((i % 97) + 1) * dec!(100); // 100 to 9700

            let result = match i % 5 {
                0 => b.deposit(amount),
                1 => b.try_lock(amount),
                2 => {
                    let locked = b.locked();
                    if locked >= amount { b.deduct_locked(amount) } else { b.unlock(locked) }
                }
                3 => {
                    let locked = b.locked();
                    if locked >= amount { b.unlock(amount) } else { b.try_lock(Decimal::ZERO.max(b.available() - amount)) }
                }
                4 => {
                    let avail = b.available();
                    if avail >= amount { b.withdraw(amount) } else { b.credit(amount) }
                }
                _ => unreachable!(),
            };
            // Ignore expected errors (insufficient funds) — only check structural invariant.
            let _ = result;

            assert_eq!(
                b.total(),
                b.available() + b.locked(),
                "total != available + locked at step {i}: total={} avail={} locked={}",
                b.total(), b.available(), b.locked()
            );
            assert!(b.total() >= Decimal::ZERO, "total went negative at step {i}");
            assert!(b.locked() >= Decimal::ZERO, "locked went negative at step {i}");
            assert!(b.available() >= Decimal::ZERO, "available went negative at step {i}");
        }
    }

    /// Dust amount: a 0.00000001 unit balance is preserved exactly.
    /// Ensures we do not silently round or lose dust balances.
    #[test]
    fn test_dust_amount_handling_exact() {
        let mut b = Balance::zero();
        let dust = dec!(0.00000001); // 1 satoshi

        b.deposit(dust).unwrap();
        assert_eq!(b.total(), dust, "deposit of 1 satoshi must be preserved exactly");
        assert_eq!(b.available(), dust);

        b.try_lock(dust).unwrap();
        assert_eq!(b.locked(), dust);
        assert_eq!(b.available(), Decimal::ZERO);

        b.unlock(dust).unwrap();
        assert_eq!(b.locked(), Decimal::ZERO);
        assert_eq!(b.available(), dust);

        b.deduct_locked(Decimal::ZERO).unwrap_or(()); // no-op
        b.try_lock(dust).unwrap();
        b.deduct_locked(dust).unwrap();
        assert_eq!(b.total(), Decimal::ZERO, "after full deduct, balance is zero");
    }

    /// Verify all_asset_totals() correctly reconciles across multiple users.
    ///
    /// This is the anti-FTX solvency check: the sum of all user balances must
    /// equal the exchange's total reported liabilities.
    #[test]
    fn test_all_asset_totals_reconcile_with_individual_balances() {
        let mut mgr = AccountManager::new();

        let users: Vec<Uuid> = (0..10).map(|_| {
            let id = new_user_id();
            mgr.create_account(id);
            id
        }).collect();

        // Deposit varying amounts of USDT and BTC.
        let usdt_amounts = [
            dec!(1000), dec!(2500), dec!(500), dec!(10000), dec!(750),
            dec!(300), dec!(5000), dec!(1200), dec!(8000), dec!(420),
        ];
        let btc_amounts = [
            dec!(0.1), dec!(0.5), dec!(2.0), dec!(0.01), dec!(1.0),
            dec!(0.25), dec!(0.75), dec!(3.0), dec!(0.05), dec!(0.333),
        ];

        let mut expected_usdt = Decimal::ZERO;
        let mut expected_btc = Decimal::ZERO;

        for (i, &user_id) in users.iter().enumerate() {
            mgr.deposit(user_id, "USDT", usdt_amounts[i]).unwrap();
            mgr.deposit(user_id, "BTC", btc_amounts[i]).unwrap();
            expected_usdt += usdt_amounts[i];
            expected_btc += btc_amounts[i];
        }

        let totals = mgr.all_asset_totals();
        assert_eq!(
            *totals.get("USDT").unwrap(),
            expected_usdt,
            "USDT totals mismatch — money created or destroyed"
        );
        assert_eq!(
            *totals.get("BTC").unwrap(),
            expected_btc,
            "BTC totals mismatch — money created or destroyed"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // CATEGORY 3 — Anti-FTX: EXC Exclusion Deep Tests
    // ─────────────────────────────────────────────────────────────────────────

    /// Even mixing EXC with real collateral, EXC contributes zero.
    ///
    /// An attacker with $1M of EXC + $1 of USDT should have exactly $1 of
    /// collateral, not $1,000,001.
    #[test]
    fn test_exc_does_not_pollute_mixed_collateral_calculation() {
        let user_id = new_user_id();
        let mut account = UnifiedAccount::new(user_id, AccountMode::Unified);

        // $1 USDT + 1,000,000 EXC at $5 each (face value $5M).
        account.set_wallet_balance("USDT", dec!(1), Decimal::ZERO, Decimal::ZERO);
        account.set_wallet_balance("EXC", dec!(1_000_000), Decimal::ZERO, Decimal::ZERO);
        account.calculate_equity(&btc_prices(), Decimal::ZERO, Decimal::ZERO);

        assert_eq!(
            account.total_collateral_usd,
            dec!(1), // Only the 1 USDT
            "EXC must not inflate collateral even when mixed with real assets"
        );
    }

    /// Haircut ordering sanity: EXC (0) < SHIB/unknown (0.80) < altcoins (0.85)
    /// < ETH (0.90) < BTC (0.95) < stablecoins (1.00).
    #[test]
    fn test_collateral_haircut_ordering_is_correct() {
        assert!(collateral_haircut("EXC") < collateral_haircut("SHIB"));
        assert!(collateral_haircut("SHIB") < collateral_haircut("SOL"));
        assert!(collateral_haircut("SOL") < collateral_haircut("ETH"));
        assert!(collateral_haircut("ETH") < collateral_haircut("BTC"));
        assert!(collateral_haircut("BTC") < collateral_haircut("USDT"));
        assert_eq!(collateral_haircut("USDT"), Decimal::ONE);
        assert_eq!(collateral_haircut("USDC"), Decimal::ONE);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // CATEGORY 4 — Liquidation Equality
    // ─────────────────────────────────────────────────────────────────────────

    /// No account has special liquidation privileges.
    ///
    /// An "insider" account (simulated here as a specific UUID) and a regular
    /// user account with identical positions must reach the same liquidation
    /// status at the same equity level. The liquidation check is purely
    /// mathematical — it does not consult account identity.
    #[test]
    fn test_no_account_can_bypass_liquidation_check() {
        let insider_id = new_user_id();
        let regular_id = new_user_id();

        // Both accounts: 500 USDT equity, maintenance margin $500 → liquidatable.
        for user_id in [insider_id, regular_id] {
            let mut account = UnifiedAccount::new(user_id, AccountMode::Unified);
            account.set_wallet_balance("USDT", dec!(500), Decimal::ZERO, Decimal::ZERO);
            account.calculate_equity(&btc_prices(), dec!(1000), dec!(500));

            assert!(
                account.is_liquidatable(),
                "user {} must be liquidatable — no exemption possible",
                user_id
            );
        }

        // Both accounts: 5000 USDT equity, maintenance margin $500 → NOT liquidatable.
        for user_id in [insider_id, regular_id] {
            let mut account = UnifiedAccount::new(user_id, AccountMode::Unified);
            account.set_wallet_balance("USDT", dec!(5000), Decimal::ZERO, Decimal::ZERO);
            account.calculate_equity(&btc_prices(), dec!(1000), dec!(500));

            assert!(
                !account.is_liquidatable(),
                "user {} must NOT be liquidatable at adequate equity",
                user_id
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // CATEGORY 5 — Edge Cases
    // ─────────────────────────────────────────────────────────────────────────

    /// Funding payment larger than available balance: the balance must not go
    /// negative. The operation must return an error.
    ///
    /// This models extreme funding rate scenarios (0.75% per 8h for 3 periods
    /// = 2.25% of position notional drained).
    #[test]
    fn test_funding_payment_cannot_make_balance_negative() {
        let mut b = Balance::zero();
        b.deposit(dec!(100)).unwrap(); // Small margin account

        // Funding payment of 200 (larger than entire balance) must fail.
        let err = b.withdraw(dec!(200)).unwrap_err();
        assert!(
            matches!(err, AccountError::InsufficientBalance { needed, available }
                if needed == dec!(200) && available == dec!(100)),
            "overage funding must fail: {:?}",
            err
        );

        // Balance unchanged.
        assert_eq!(b.total(), dec!(100));
        assert!(b.available() >= Decimal::ZERO);
    }

    /// A zero-balance account rejects all operations that assume positive funds.
    #[test]
    fn test_zero_balance_account_rejects_all_extractions() {
        let mut b = Balance::zero();

        // Withdraw from zero.
        assert!(b.withdraw(dec!(0.00000001)).is_err());
        // Lock from zero.
        assert!(b.try_lock(dec!(0.00000001)).is_err());
        // Deduct from zero locked (no locked funds to deduct).
        assert!(b.deduct_locked(dec!(0.00000001)).is_err());
        // Unlock from zero locked.
        assert!(b.unlock(dec!(0.00000001)).is_err());

        // State stays at zero throughout.
        assert_eq!(b.total(), Decimal::ZERO);
        assert_eq!(b.locked(), Decimal::ZERO);
    }

    /// The total system balance is conserved after a complete trade round-trip:
    /// deposit → lock → fill → credit. No money is created or destroyed.
    #[test]
    fn test_total_assets_conserved_across_complete_trade() {
        // Alice has 50,000 USDT (buyer), Bob has 1 BTC (seller).
        // They trade 1 BTC at 50,000 USDT. No fees for simplicity.
        let mut alice_usdt = Balance::zero();
        let mut alice_btc = Balance::zero();
        let mut bob_usdt = Balance::zero();
        let mut bob_btc = Balance::zero();

        alice_usdt.deposit(dec!(50000)).unwrap();
        bob_btc.deposit(dec!(1)).unwrap();

        // Total system assets before trade.
        let total_usdt_before = alice_usdt.total() + bob_usdt.total();
        let total_btc_before = alice_btc.total() + bob_btc.total();

        // Lock phase.
        alice_usdt.try_lock(dec!(50000)).unwrap();
        bob_btc.try_lock(dec!(1)).unwrap();

        // Fill phase.
        alice_usdt.deduct_locked(dec!(50000)).unwrap();
        alice_btc.credit(dec!(1)).unwrap();
        bob_btc.deduct_locked(dec!(1)).unwrap();
        bob_usdt.credit(dec!(50000)).unwrap();

        // Total system assets after trade.
        let total_usdt_after = alice_usdt.total() + bob_usdt.total();
        let total_btc_after = alice_btc.total() + bob_btc.total();

        assert_eq!(
            total_usdt_before, total_usdt_after,
            "USDT was created or destroyed during trade"
        );
        assert_eq!(
            total_btc_before, total_btc_after,
            "BTC was created or destroyed during trade"
        );

        // Verify specific final states.
        assert_eq!(alice_usdt.total(), Decimal::ZERO);
        assert_eq!(alice_btc.total(), dec!(1));
        assert_eq!(bob_usdt.total(), dec!(50000));
        assert_eq!(bob_btc.total(), Decimal::ZERO);
    }

    /// Deposit address uniqueness proxy test: AccountManager.create_account
    /// is idempotent (calling it twice for the same user does not create two
    /// separate balance buckets). Each user_id has exactly one balance entry.
    #[test]
    fn test_deposit_credit_idempotency_single_bucket_per_user() {
        let mut mgr = AccountManager::new();
        let user = new_user_id();

        mgr.create_account(user);
        mgr.create_account(user); // Second call — must be a no-op.

        mgr.deposit(user, "USDT", dec!(1000)).unwrap();
        mgr.deposit(user, "USDT", dec!(500)).unwrap();

        // Balance should be 1500, not 1000 or 500 (no duplicate account bucket).
        let bal = mgr.get_balance(&user, "USDT").unwrap();
        assert_eq!(bal.total(), dec!(1500));
    }
}
