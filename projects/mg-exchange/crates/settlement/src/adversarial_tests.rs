/// # Adversarial Tests — Settlement / Insurance Fund
///
/// These tests verify that the insurance fund:
///   - Tracks ACTUAL balance changes, not just display numbers
///   - Can never go negative (FTX displayed a fake fund balance)
///   - Distributes socialised losses fairly and proportionally
///   - Rejects all negative-amount operations that could corrupt state
#[cfg(test)]
mod adversarial {
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    use exchange_common::types::Symbol;

    use crate::insurance_fund::{InsuranceFund, SocializedLoss};
    use crate::error::SettlementError;

    fn sym() -> Symbol {
        Symbol::new("BTCUSDT")
    }

    fn uid() -> Uuid {
        Uuid::new_v4()
    }

    // ─────────────────────────────────────────────────────────────────────────
    // CATEGORY 1 — Insurance Fund Balance Is Real, Not Display
    //
    // FTX showed a random number as "insurance fund" without it reflecting
    // actual balance changes. Every mutation here must be exact.
    // ─────────────────────────────────────────────────────────────────────────

    /// After adding surplus, balance increases by EXACTLY the surplus amount.
    /// After covering a deficit, balance decreases by EXACTLY the deficit amount.
    /// The fund balance is fully reconstructible from its operation history.
    #[test]
    fn test_insurance_fund_balance_tracks_actual_changes_exactly() {
        let mut fund = InsuranceFund::new(sym(), dec!(10000));
        let initial = fund.balance();
        assert_eq!(initial, dec!(10000));

        // Surplus from a profitable liquidation.
        fund.add_surplus(dec!(1337)).unwrap();
        assert_eq!(fund.balance(), dec!(11337), "surplus must increase balance by exactly 1337");

        fund.add_surplus(dec!(663)).unwrap();
        assert_eq!(fund.balance(), dec!(12000));

        // Deficit covered from fund.
        fund.cover_deficit(dec!(4500)).unwrap();
        assert_eq!(fund.balance(), dec!(7500), "deficit must decrease balance by exactly 4500");

        // Reconstruct balance from history: 10000 + 1337 + 663 - 4500 = 7500.
        let reconstructed = dec!(10000) + dec!(1337) + dec!(663) - dec!(4500);
        assert_eq!(
            fund.balance(),
            reconstructed,
            "balance must equal independently reconstructed value"
        );
    }

    /// The insurance fund can never go negative.
    /// Attempting to cover a deficit larger than the balance must fail
    /// and leave the balance unchanged.
    #[test]
    fn test_insurance_fund_balance_never_goes_negative() {
        let mut fund = InsuranceFund::new(sym(), dec!(1000));

        // Try to drain more than available.
        let err = fund.cover_deficit(dec!(1000.00000001)).unwrap_err();
        assert!(
            matches!(err, SettlementError::InsufficientInsuranceFund { requested, available }
                if requested == dec!(1000.00000001) && available == dec!(1000)),
            "must reject deficit larger than balance: {err:?}"
        );

        // Balance must be unchanged.
        assert_eq!(fund.balance(), dec!(1000), "balance must be unchanged after failed cover");

        // Exact drain must succeed (boundary test).
        fund.cover_deficit(dec!(1000)).unwrap();
        assert_eq!(fund.balance(), Decimal::ZERO);

        // Zero-balance fund cannot cover any deficit.
        let err = fund.cover_deficit(dec!(0.00000001)).unwrap_err();
        assert!(
            matches!(err, SettlementError::InsufficientInsuranceFund { .. }),
            "zero-balance fund must reject any deficit: {err:?}"
        );
        assert_eq!(fund.balance(), Decimal::ZERO);
    }

    /// Negative-amount operations are rejected without state mutation.
    #[test]
    fn test_insurance_fund_rejects_negative_operations() {
        let mut fund = InsuranceFund::new(sym(), dec!(5000));

        let err = fund.add_surplus(dec!(-1)).unwrap_err();
        assert!(
            matches!(err, SettlementError::NegativeAmount(_)),
            "negative surplus must be rejected: {err:?}"
        );
        assert_eq!(fund.balance(), dec!(5000));

        let err = fund.cover_deficit(dec!(-1)).unwrap_err();
        assert!(
            matches!(err, SettlementError::NegativeAmount(_)),
            "negative deficit must be rejected: {err:?}"
        );
        assert_eq!(fund.balance(), dec!(5000));
    }

    /// Simulate 100 surplus + deficit cycles and verify the final balance
    /// equals the independently computed sum. No rounding drift.
    #[test]
    fn test_insurance_fund_100_cycle_balance_reconstruction() {
        let mut fund = InsuranceFund::new(sym(), dec!(50000));
        let mut expected = dec!(50000);

        for i in 1u64..=100 {
            let surplus_amount = Decimal::from(i * 13) * dec!(0.01); // e.g. 0.13, 0.26, ...
            fund.add_surplus(surplus_amount).unwrap();
            expected += surplus_amount;

            if expected > dec!(100) {
                let deficit_amount = Decimal::from(i * 7) * dec!(0.01);
                if deficit_amount <= expected {
                    fund.cover_deficit(deficit_amount).unwrap();
                    expected -= deficit_amount;
                }
            }

            assert_eq!(
                fund.balance(),
                expected,
                "balance mismatch at cycle {i}: fund={} expected={}",
                fund.balance(), expected
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // CATEGORY 2 — Socialised Loss Fairness
    // ─────────────────────────────────────────────────────────────────────────

    /// Socialised losses must be distributed proportionally to profit.
    /// A user with 3× the profit pays 3× the clawback.
    #[test]
    fn test_socialised_loss_proportional_distribution_exact() {
        let a = uid();
        let b = uid();
        let c = uid();

        // Profits: A=6000, B=3000, C=1000 → total=10000.
        // Shortfall: 1000.
        // A pays: 1000 * 6000/10000 = 600.
        // B pays: 1000 * 3000/10000 = 300.
        // C pays: 1000 * 1000/10000 = 100.
        let loss = SocializedLoss::distribute(
            sym(),
            dec!(1000),
            &[(a, dec!(6000)), (b, dec!(3000)), (c, dec!(1000))],
        ).unwrap();

        let cb_a = loss.clawbacks.iter().find(|cb| cb.user_id == a).unwrap();
        let cb_b = loss.clawbacks.iter().find(|cb| cb.user_id == b).unwrap();
        let cb_c = loss.clawbacks.iter().find(|cb| cb.user_id == c).unwrap();

        assert_eq!(cb_a.amount, dec!(600), "A must pay 600 (60% of 1000)");
        assert_eq!(cb_b.amount, dec!(300), "B must pay 300 (30% of 1000)");
        assert_eq!(cb_c.amount, dec!(100), "C must pay 100 (10% of 1000)");

        // Total clawed back must equal the shortfall (within rounding epsilon).
        let total = loss.total_clawed_back();
        assert_eq!(total, dec!(1000), "total clawback must exactly equal shortfall");
    }

    /// Users with zero or negative profit are excluded from clawback.
    ///
    /// It would be unjust to clawback from a user already losing money.
    #[test]
    fn test_socialised_loss_skips_unprofitable_users() {
        let winner = uid();
        let breakeven = uid();
        let loser = uid();

        let loss = SocializedLoss::distribute(
            sym(),
            dec!(500),
            &[(winner, dec!(1000)), (breakeven, Decimal::ZERO), (loser, dec!(-200))],
        ).unwrap();

        // Only the winner should be clawed back.
        assert_eq!(loss.clawbacks.len(), 1, "only 1 profitable user");
        assert_eq!(loss.clawbacks[0].user_id, winner);
        assert_eq!(loss.clawbacks[0].amount, dec!(500), "winner absorbs entire shortfall");
    }

    /// Zero shortfall produces no socialised loss event.
    #[test]
    fn test_socialised_loss_zero_shortfall_returns_none() {
        let result = SocializedLoss::distribute(
            sym(),
            Decimal::ZERO,
            &[(uid(), dec!(10000))],
        );
        assert!(result.is_none(), "zero shortfall must produce no loss event");
    }

    /// No profitable positions: loss cannot be distributed, returns None.
    ///
    /// This is the worst-case scenario where the entire market is underwater —
    /// no one to absorb the loss.
    #[test]
    fn test_socialised_loss_no_profitable_positions_returns_none() {
        let result = SocializedLoss::distribute(
            sym(),
            dec!(1000),
            &[(uid(), Decimal::ZERO), (uid(), dec!(-500))],
        );
        assert!(result.is_none(), "cannot socialise loss with no profitable users");
    }

    /// Clawback amounts are all non-negative.
    ///
    /// No user should ever have a negative clawback (which would be a credit).
    #[test]
    fn test_socialised_loss_all_clawbacks_are_non_negative() {
        let users: Vec<Uuid> = (0..20).map(|_| uid()).collect();
        let profits: Vec<(Uuid, Decimal)> = users.iter().enumerate()
            .map(|(i, &u)| (u, Decimal::from(i as u64 + 1) * dec!(100)))
            .collect();

        let loss = SocializedLoss::distribute(sym(), dec!(10000), &profits).unwrap();

        for cb in &loss.clawbacks {
            assert!(
                cb.amount >= Decimal::ZERO,
                "clawback amount must be non-negative: user={} amount={}",
                cb.user_id, cb.amount
            );
        }
    }

    /// Large-scale proportional distribution: 50 users, verify totals.
    #[test]
    fn test_socialised_loss_large_distribution_totals_within_rounding() {
        let users: Vec<Uuid> = (0..50).map(|_| uid()).collect();
        // Profits: 1, 2, 3, ... 50 (varying, all positive).
        let profits: Vec<(Uuid, Decimal)> = users.iter().enumerate()
            .map(|(i, &u)| (u, Decimal::from(i as u64 + 1)))
            .collect();

        let shortfall = dec!(12750); // Arbitrary large shortfall.
        let loss = SocializedLoss::distribute(sym(), shortfall, &profits).unwrap();

        let total_clawed = loss.total_clawed_back();
        let rounding_tolerance = dec!(0.00000001) * Decimal::from(50u64); // 50 users * 1 satoshi

        let diff = (total_clawed - shortfall).abs();
        assert!(
            diff <= rounding_tolerance,
            "total clawed ({total_clawed}) must be within rounding tolerance of shortfall ({shortfall}): diff={diff}"
        );
    }

    // ── Full insurance waterfall: surplus → deficit → fund → socialised loss ──

    #[test]
    fn test_full_insurance_waterfall() {
        let mut fund = InsuranceFund::new(sym(), Decimal::ZERO);

        // Step 1: Liquidation with surplus → fund receives.
        // Liquidated at mark=45000, bankruptcy=44000 → surplus = 1000 USDT.
        fund.add_surplus(dec!(1000)).unwrap();
        assert_eq!(fund.balance(), dec!(1000), "surplus added to fund");

        // Step 2: Second liquidation with deficit of 500 → fund covers.
        fund.cover_deficit(dec!(500)).unwrap();
        assert_eq!(fund.balance(), dec!(500), "fund covers deficit");

        // Step 3: Third liquidation with deficit of 800 → fund has 500, shortfall = 300.
        let result = fund.cover_deficit(dec!(800));
        assert!(result.is_err(), "deficit exceeds fund balance");
        // Fund should NOT be modified by failed call.
        assert_eq!(fund.balance(), dec!(500), "fund unchanged after failed deficit");

        // Step 4: Manually handle the shortfall: drain fund + socialise remainder.
        // Drain whatever fund has.
        fund.cover_deficit(dec!(500)).unwrap();
        assert_eq!(fund.balance(), Decimal::ZERO, "fund drained to zero");

        // Remaining shortfall = 800 - 500 = 300. Socialise across profitable traders.
        let shortfall = dec!(300);
        let profits = vec![
            (Uuid::from_u128(1), dec!(600)),  // 60% of profit
            (Uuid::from_u128(2), dec!(400)),  // 40% of profit
        ];
        let loss = SocializedLoss::distribute(sym(), shortfall, &profits).unwrap();

        // Clawbacks should be proportional: 180 + 120 = 300.
        assert_eq!(loss.clawbacks.len(), 2);
        let total_clawed: Decimal = loss.clawbacks.iter().map(|c| c.amount).sum();
        assert_eq!(total_clawed, dec!(300), "all shortfall distributed");

        // User 1: 300 * 600/1000 = 180.
        let user1 = loss.clawbacks.iter().find(|c| c.user_id == Uuid::from_u128(1)).unwrap();
        assert_eq!(user1.amount, dec!(180));

        // User 2: 300 * 400/1000 = 120.
        let user2 = loss.clawbacks.iter().find(|c| c.user_id == Uuid::from_u128(2)).unwrap();
        assert_eq!(user2.amount, dec!(120));

        // Final fund state: zero (drained).
        assert_eq!(fund.balance(), Decimal::ZERO);
    }
}
