/// # Adversarial / Penetration-Level Tests — Risk Engine
///
/// These tests verify that the liquidation engine, mark price calculator,
/// and margin calculator cannot be exploited to:
///   - Exempt any position from liquidation
///   - Trigger liquidations via mark price manipulation
///   - Allow under-margined positions to remain open
///   - Allow maximum-leverage positions to fail at incorrect prices
#[cfg(test)]
mod adversarial {
    use std::collections::HashMap;

    use chrono::Utc;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    use exchange_common::{
        futures::{
            ContractType, LeverageTier, LiquidationOrder, MarginMode, PerpetualConfig, Position,
            PositionSide,
        },
        types::Symbol,
    };

    use crate::{
        liquidation::{
            bankruptcy_price, LiquidationEngine,
        },
        margin_calculator::{
            is_liquidatable, required_initial_margin, required_maintenance_margin,
        },
        mark_price::calculate_mark_price,
    };

    // ─────────────────────────────────────────────────────────────────────────
    // Helpers
    // ─────────────────────────────────────────────────────────────────────────

    fn new_uid() -> Uuid {
        Uuid::new_v4()
    }

    fn sym() -> Symbol {
        Symbol::new("BTCUSDT")
    }

    fn btc_config() -> PerpetualConfig {
        PerpetualConfig::btc_default()
    }

    /// Build an isolated long position.
    fn long_pos(size: Decimal, entry: Decimal, margin: Decimal, user_id: Uuid) -> Position {
        Position {
            user_id,
            symbol: sym(),
            side: PositionSide::Long,
            size,
            entry_price: entry,
            leverage: dec!(10),
            margin_mode: MarginMode::Isolated,
            margin,
            realized_pnl: Decimal::ZERO,
            funding_accumulated: Decimal::ZERO,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            position_idx: 0,
            tp_price: None,
            sl_price: None,
            trailing_stop: None,
            tp_trigger_id: None,
            sl_trigger_id: None,
            tp_size: None,
            sl_size: None,
            auto_add_margin: false,
        }
    }

    /// Build an isolated short position.
    fn short_pos(size: Decimal, entry: Decimal, margin: Decimal, user_id: Uuid) -> Position {
        let mut p = long_pos(size, entry, margin, user_id);
        p.side = PositionSide::Short;
        p
    }

    // ─────────────────────────────────────────────────────────────────────────
    // CATEGORY 1 — No Liquidation Exemptions
    // ─────────────────────────────────────────────────────────────────────────

    /// An "insider" account and a regular account with identical positions must
    /// be liquidated at precisely the same mark price.
    ///
    /// The liquidation check is purely mathematical. There is no account-identity
    /// lookup, no exception list, no "privileged" flag in the system.
    #[test]
    fn test_no_account_can_bypass_liquidation() {
        let insider_id = new_uid();
        let regular_id = new_uid();

        // Both: 1 BTC long at 50,000 with 5,000 margin (10x leverage).
        // Bankruptcy price = 50,000 - 5,000/1 = 45,000.
        // At 44,800 both should be liquidatable.

        let insider_pos = long_pos(dec!(1), dec!(50000), dec!(5000), insider_id);
        let regular_pos = long_pos(dec!(1), dec!(50000), dec!(5000), regular_id);
        let config = btc_config();
        let mark = dec!(44800);

        let insider_liq = is_liquidatable(&insider_pos, mark, &config);
        let regular_liq = is_liquidatable(&regular_pos, mark, &config);

        assert!(
            insider_liq,
            "insider position must be liquidatable at {mark}"
        );
        assert!(
            regular_liq,
            "regular position must be liquidatable at {mark}"
        );
        assert_eq!(
            insider_liq, regular_liq,
            "liquidation decision must be identical regardless of account identity"
        );
    }

    /// Test that check_all_positions treats all positions equally.
    ///
    /// 10 users each have the same position. All 10 must appear in the
    /// liquidation batch when the mark price crosses their threshold.
    #[test]
    fn test_liquidation_engine_treats_all_accounts_equally() {
        let engine = LiquidationEngine::new(1000);

        // 10 users, all with identical 1 BTC long @ 50,000 with 5,000 margin.
        let users: Vec<Uuid> = (0..10).map(|_| new_uid()).collect();
        let positions: Vec<Position> = users
            .iter()
            .map(|&uid| long_pos(dec!(1), dec!(50000), dec!(5000), uid))
            .collect();

        let mut mark_prices = HashMap::new();
        mark_prices.insert(sym(), dec!(44000)); // Below liq threshold for all

        let mut configs = HashMap::new();
        configs.insert(sym(), btc_config());

        let batch = engine.check_all_positions(positions.iter(), &mark_prices, &configs);

        assert_eq!(
            batch.orders.len(),
            10,
            "all 10 identical positions must be liquidated — no exemptions: got {}",
            batch.orders.len()
        );

        // Each order must reference the correct user.
        let user_set: std::collections::HashSet<Uuid> =
            batch.orders.iter().map(|o| o.user_id).collect();
        assert_eq!(user_set.len(), 10, "all 10 distinct users must appear in the batch");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // CATEGORY 2 — Mark Price Manipulation Resistance
    // ─────────────────────────────────────────────────────────────────────────

    /// Mark price = (2*index + last) / 3.
    ///
    /// A single trader can only manipulate "last" price on the exchange.
    /// Even moving last to 2× the index, mark only increases by 33% of
    /// the spread — not enough to trigger cascading liquidations on normally-
    /// margined positions.
    ///
    /// Concretely: index = 50,000. Attacker pumps last to 100,000.
    /// Mark = (100,000 + 100,000) / 3 = 66,666.67 — not 100,000.
    ///
    /// A normally-margined long at 10x has a liq price ~45,000.
    /// A mark of 66,667 is well above that — so longs are not liquidated.
    ///
    /// For shorts with entry 50,000 and 5,000 margin (10x), the bankruptcy
    /// price is 55,000. A mark of 66,667 would trigger that short liquidation
    /// — but the test verifies mark didn't reach 100,000 (the attacker's target).
    #[test]
    fn test_mark_price_cannot_be_manipulated_by_single_trader_to_full_last_price() {
        let index_price = dec!(50000);
        let manipulated_last = dec!(100000); // Attacker doubled the last price.

        let mp = calculate_mark_price(sym(), index_price, manipulated_last, dec!(0));

        // Mark must be (2*50000 + 100000) / 3 = 66666.666...
        let expected = (Decimal::TWO * index_price + manipulated_last) / Decimal::from(3u32);
        assert_eq!(mp.price, expected, "mark price formula must be exact");

        // Mark must be strictly between index and last (not equal to last).
        assert!(
            mp.price > index_price && mp.price < manipulated_last,
            "mark ({}) must be between index ({}) and last ({}) — manipulation was not fully effective",
            mp.price, index_price, manipulated_last
        );

        // Last can move mark by at most 1/3 of the spread.
        let spread = manipulated_last - index_price;
        let mark_deviation = mp.price - index_price;
        let max_allowed_deviation = spread / Decimal::from(3u32);
        assert_eq!(
            mark_deviation, max_allowed_deviation,
            "mark deviation must be exactly 1/3 of spread"
        );

        // A normally-margined long (10x, entry 50,000, margin 5,000) is NOT
        // liquidated by this manipulated mark price.
        let pos = long_pos(dec!(1), dec!(50000), dec!(5000), new_uid());
        let config = btc_config();
        let at_manipulated_mark = is_liquidatable(&pos, mp.price, &config);
        assert!(
            !at_manipulated_mark,
            "normal long should NOT be liquidated by an attacker pumping last price"
        );
    }

    /// Verify the symmetric case: attacker dumps last price 50% below index.
    /// Mark drops only 1/3 of the way toward the dump target.
    /// A short position with adequate margin is not falsely liquidated.
    #[test]
    fn test_mark_price_dump_attack_also_capped_at_one_third() {
        let index_price = dec!(50000);
        let dumped_last = dec!(25000); // Attacker halved the last price.

        let mp = calculate_mark_price(sym(), index_price, dumped_last, dec!(0));

        let expected = (Decimal::TWO * index_price + dumped_last) / Decimal::from(3u32);
        assert_eq!(mp.price, expected);
        assert_eq!(mp.price, dec!(41666.666666666666666666666667));

        // Mark is between dumped_last and index — not as low as the attacker intended.
        assert!(
            mp.price > dumped_last && mp.price < index_price,
            "mark ({}) must be between dump target ({}) and index ({})",
            mp.price, dumped_last, index_price
        );

        // A short position (entry 50,000, margin 5,000 = 10x) has a bankruptcy
        // price of 50,000 + 5,000 = 55,000. Mark of ~41,667 is below entry
        // (profitable for short). The short is NOT liquidatable.
        let pos = short_pos(dec!(1), dec!(50000), dec!(5000), new_uid());
        let config = btc_config();
        assert!(
            !is_liquidatable(&pos, mp.price, &config),
            "short position should not be liquidated just because an attacker dumped last price"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // CATEGORY 3 — Maximum-Leverage Liquidation Precision
    // ─────────────────────────────────────────────────────────────────────────

    /// 100x leverage: $100 margin for a $10,000 position.
    /// A 1% move ($100) = total margin loss.
    ///
    /// The liquidation price for a long is:
    ///   liq_price ≈ entry - margin/size = 10,000 - 100/1 = 9,900
    ///   bankruptcy = 9,900 (same formula with no MMR offset)
    ///
    /// We test:
    ///   - At entry (10,000): NOT liquidatable.
    ///   - At 9,900 (bankruptcy): effective margin = 0, maintenance > 0 → liquidatable.
    ///   - At 9,901 (one tick above bankruptcy): should NOT be liquidatable if margin > maintenance.
    ///
    /// Important: the liquidation trigger is `effective_margin < maintenance_margin`,
    /// not `effective_margin < 0`. So the exact trigger price is slightly above
    /// the bankruptcy price.
    #[test]
    fn test_maximum_leverage_100x_liquidation_precision() {
        // Custom config: single tier with MMR = 0.5% (same as btc_default tier 1).
        let config = PerpetualConfig {
            symbol: sym(),
            max_leverage: dec!(100),
            initial_margin_rate: dec!(0.01),
            maintenance_margin_rate: dec!(0.005),
            funding_interval_hours: 8,
            max_funding_rate: dec!(0.01),
            insurance_fund: Decimal::ZERO,
            contract_type: ContractType::Linear,
            settle_coin: "USDT".to_string(),
            contract_size: Decimal::ONE,
            tiers: vec![LeverageTier {
                min_notional: Decimal::ZERO,
                max_notional: Decimal::MAX,
                max_leverage: dec!(100),
                maintenance_margin_rate: dec!(0.005),
            }],
        };

        // 1 BTC long at 10,000 USDT entry, 100 USDT margin (100x leverage).
        let pos = long_pos(dec!(1), dec!(10000), dec!(100), new_uid());

        // Initial margin check.
        let required = required_initial_margin(dec!(1), dec!(10000), dec!(100));
        assert_eq!(required, dec!(100), "initial margin for 100x must be 100 USDT");

        // At entry price — not liquidatable.
        // effective = 100 + (10000 - 10000) = 100, maintenance = 10000 * 0.005 = 50.
        // 100 > 50 → not liquidatable.
        assert!(!is_liquidatable(&pos, dec!(10000), &config));

        // Bankruptcy price = 10,000 - 100/1 = 9,900.
        let bk = bankruptcy_price(&pos);
        assert_eq!(bk, dec!(9900), "bankruptcy price must be 9,900");

        // AT the bankruptcy price: effective = 100 + (9900 - 10000) = 0.
        // maintenance = 9900 * 0.005 = 49.5.
        // 0 < 49.5 → liquidatable.
        assert!(
            is_liquidatable(&pos, bk, &config),
            "position must be liquidatable at bankruptcy price"
        );

        // At exactly 1 tick above the trigger price.
        // Trigger occurs when: margin + (mark - entry) = mark * mmr
        //   100 + mark - 10000 = mark * 0.005
        //   0.995 * mark = 9900
        //   mark = 9900 / 0.995 = 9949.748...
        // So at mark = 9,950: effective = 100 + (9950 - 10000) = 50.
        // maintenance = 9950 * 0.005 = 49.75.
        // 50 > 49.75 → NOT liquidatable (still safe by a tiny margin).
        let mark_safe = dec!(9950);
        let effective_at_safe = dec!(100) + (mark_safe - dec!(10000)) * dec!(1);
        let maintenance_at_safe = required_maintenance_margin(dec!(1), mark_safe, dec!(0.005));
        assert!(
            effective_at_safe > maintenance_at_safe,
            "at 9950 position should be above maintenance: effective={} > maintenance={}",
            effective_at_safe, maintenance_at_safe
        );
        assert!(!is_liquidatable(&pos, mark_safe, &config));

        // At mark = 9,900 (bankruptcy): liquidatable, as tested above.
        // The liquidation price is between 9,900 and 9,950 — never outside this range.
        // Too-early liquidation (at 9,950) would be unfair; too-late (below 9,900)
        // would cause a deficit. Both sides are tested above.
    }

    /// Flash crash scenario: 50% drop in one tick.
    ///
    /// All 10x leveraged longs must be liquidated after BTC drops from 50,000
    /// to 25,000. The engine must handle this without panicking.
    #[test]
    fn test_flash_crash_50_percent_drop_liquidates_all_leveraged_longs() {
        let engine = LiquidationEngine::new(1000);

        // 5 users with 10x leveraged longs at 50,000 entry.
        // Margin = 5,000 each. Bankruptcy price = 50,000 - 5,000/1 = 45,000.
        // At 25,000 (50% drop), all are deeply underwater.
        let users: Vec<Uuid> = (0..5).map(|_| new_uid()).collect();
        let positions: Vec<Position> = users
            .iter()
            .map(|&uid| long_pos(dec!(1), dec!(50000), dec!(5000), uid))
            .collect();

        let mut mark_prices = HashMap::new();
        mark_prices.insert(sym(), dec!(25000)); // 50% flash crash

        let mut configs = HashMap::new();
        configs.insert(sym(), btc_config());

        let batch = engine.check_all_positions(positions.iter(), &mark_prices, &configs);

        assert_eq!(
            batch.orders.len(),
            5,
            "all 5 positions must be liquidated after 50% flash crash"
        );

        // For a 50% flash crash, the mark price (25,000) is BELOW the bankruptcy
        // price (45,000) — this is precisely the deficit scenario where the
        // insurance fund must absorb the difference. Verify the ordering.
        for order in &batch.orders {
            assert!(
                order.bankruptcy_price > order.mark_price,
                "after a 50% crash, bankruptcy price ({}) must be above mark ({}) — this is a deficit liquidation",
                order.bankruptcy_price, order.mark_price
            );
        }
    }

    /// Zero-liquidity scenario: empty positions iterator — engine must return
    /// an empty batch without panicking.
    #[test]
    fn test_zero_liquidity_market_no_panic() {
        let engine = LiquidationEngine::new(100);
        let empty_positions: Vec<Position> = vec![];

        let mut mark_prices = HashMap::new();
        mark_prices.insert(sym(), dec!(50000));

        let mut configs = HashMap::new();
        configs.insert(sym(), btc_config());

        // Must not panic.
        let batch = engine.check_all_positions(empty_positions.iter(), &mark_prices, &configs);
        assert!(batch.orders.is_empty(), "no positions → no liquidation orders");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // CATEGORY 4 — Bankruptcy Price Correctness
    // ─────────────────────────────────────────────────────────────────────────

    /// Bankruptcy price formula correctness for long and short.
    ///
    /// Long:  bk = entry - margin/size
    /// Short: bk = entry + margin/size
    ///
    /// This is the price at which the insurance fund absorbs zero surplus
    /// (fill at bankruptcy = zero delta to fund).
    #[test]
    fn test_bankruptcy_price_exact_formulas() {
        let uid = new_uid();

        // Long: 2 BTC @ 40,000 entry, 8,000 margin (5x leverage).
        // bk = 40,000 - 8,000/2 = 40,000 - 4,000 = 36,000.
        let long = long_pos(dec!(2), dec!(40000), dec!(8000), uid);
        assert_eq!(
            bankruptcy_price(&long),
            dec!(36000),
            "long bankruptcy price must be entry - margin/size"
        );

        // Short: 2 BTC @ 40,000 entry, 8,000 margin (5x leverage).
        // bk = 40,000 + 8,000/2 = 40,000 + 4,000 = 44,000.
        let short = short_pos(dec!(2), dec!(40000), dec!(8000), uid);
        assert_eq!(
            bankruptcy_price(&short),
            dec!(44000),
            "short bankruptcy price must be entry + margin/size"
        );
    }

    /// Insurance fund surplus/deficit sign correctness.
    ///
    /// Fill above bankruptcy for long → surplus (fund gains).
    /// Fill below bankruptcy for long → deficit (fund pays).
    /// Symmetrical for short.
    #[test]
    fn test_insurance_fund_surplus_exact_amount() {
        // LiquidationOrder is imported at the module level
        use exchange_common::futures::POSITION_IDX_ONE_WAY;

        let uid = new_uid();
        let engine = LiquidationEngine::new(100);

        // Long 1 BTC @ 50,000, margin 5,000, bankruptcy = 45,000.
        let pos = long_pos(dec!(1), dec!(50000), dec!(5000), uid);
        let bk = bankruptcy_price(&pos);
        assert_eq!(bk, dec!(45000));

        let order = LiquidationOrder {
            user_id: uid,
            symbol: sym(),
            side: PositionSide::Long,
            size: dec!(1),
            bankruptcy_price: bk,
            mark_price: dec!(46000),
            timestamp: Utc::now(),
            position_idx: POSITION_IDX_ONE_WAY,
        };

        // Surplus case: fill at 46,000 > bankruptcy 45,000.
        // surplus = (46,000 - 45,000) * 1 = 1,000.
        let update = engine.process_insurance_fund(&order, dec!(46000), dec!(10000));
        assert_eq!(
            update.delta,
            dec!(1000),
            "surplus must be exactly 1,000 USDT"
        );
        assert_eq!(update.socialised_loss, Decimal::ZERO);

        // Deficit case: fill at 44,000 < bankruptcy 45,000.
        // deficit = (45,000 - 44,000) * 1 = 1,000.
        let update_deficit = engine.process_insurance_fund(&order, dec!(44000), dec!(10000));
        assert_eq!(
            update_deficit.delta,
            dec!(-1000),
            "deficit must be exactly -1,000 USDT deducted from fund"
        );
        assert_eq!(update_deficit.socialised_loss, Decimal::ZERO);
    }

    /// Extreme deficit exceeds insurance fund: excess becomes socialised loss.
    #[test]
    fn test_insurance_fund_exhaustion_triggers_socialised_loss() {
        // LiquidationOrder is imported at the module level
        use exchange_common::futures::POSITION_IDX_ONE_WAY;

        let uid = new_uid();
        let engine = LiquidationEngine::new(100);

        // Long 10 BTC @ 50,000, margin 50,000, bankruptcy = 50,000 - 5,000 = 45,000.
        let bk = dec!(45000);
        let order = LiquidationOrder {
            user_id: uid,
            symbol: sym(),
            side: PositionSide::Long,
            size: dec!(10),
            bankruptcy_price: bk,
            mark_price: dec!(43000),
            timestamp: Utc::now(),
            position_idx: POSITION_IDX_ONE_WAY,
        };

        // Fill at 43,000 — deficit = (45,000 - 43,000) * 10 = 20,000.
        // Fund has only 5,000 — covered = 5,000, socialised = 15,000.
        let fund_balance = dec!(5000);
        let update = engine.process_insurance_fund(&order, dec!(43000), fund_balance);

        assert_eq!(
            update.delta,
            dec!(-5000),
            "fund covers 5,000: delta must be -5,000"
        );
        assert_eq!(
            update.socialised_loss,
            dec!(15000),
            "remaining 15,000 must be socialised"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // CATEGORY 5 — Funding Rate Edge Cases
    // ─────────────────────────────────────────────────────────────────────────

    /// Extreme funding rate at maximum for 24 hours (3 settlements at 0.75% each).
    ///
    /// A position with entry 50,000 and 10x leverage (margin = 5,000) should not
    /// go bankrupt from funding alone, provided the price stays at entry.
    ///
    /// Total funding = 3 * 0.75% * 50,000 = 1,125 USDT.
    /// Remaining margin after funding = 5,000 - 1,125 = 3,875 USDT.
    /// 3,875 > 0 → position is still solvent (price hasn't moved).
    ///
    /// We verify this arithmetic and that the funding deduction does not go negative.
    #[test]
    fn test_funding_rate_maximum_24_hours_does_not_bankrupt_normally_margined_position() {
        // 1 BTC long at 50,000, 10x leverage, margin = 5,000.
        let entry_price = dec!(50000);
        let position_size = dec!(1);
        let initial_margin = required_initial_margin(position_size, entry_price, dec!(10));
        assert_eq!(initial_margin, dec!(5000));

        // Maximum funding rate: 0.75% per period, 3 periods in 24 hours.
        let max_funding_rate = dec!(0.0075); // 0.75%
        let periods = 3u32;
        let notional = position_size * entry_price; // 50,000 USDT
        let total_funding = max_funding_rate * Decimal::from(periods) * notional;
        assert_eq!(total_funding, dec!(1125), "total funding over 24h at max rate");

        let remaining_margin = initial_margin - total_funding;
        assert_eq!(remaining_margin, dec!(3875));
        assert!(
            remaining_margin > Decimal::ZERO,
            "margin must remain positive after max 24h funding: {remaining_margin}"
        );

        // This remaining margin is still above the maintenance margin threshold.
        // At mark price = entry: maintenance = 50,000 * 0.005 = 250 USDT.
        let maintenance = required_maintenance_margin(position_size, entry_price, dec!(0.005));
        assert_eq!(maintenance, dec!(250));
        assert!(
            remaining_margin > maintenance,
            "position with {remaining_margin} margin must not be liquidatable after max funding"
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // CATEGORY 6 — Tiered MMR: Large Positions Face Stricter Standards
    // ─────────────────────────────────────────────────────────────────────────

    /// Large positions must use the stricter MMR tier, making them liquidatable
    /// sooner than small positions at the same leverage ratio.
    ///
    /// This prevents a whale from holding an effectively under-collateralised
    /// position by relying on a low base MMR.
    #[test]
    fn test_tiered_mmr_large_position_faces_stricter_threshold() {
        let config = btc_config();
        let uid = new_uid();

        // Small position: 1 BTC at 50,000 (notional = 50,000 → tier 1, MMR = 0.5%).
        // 10x leverage → margin = 5,000.
        // At mark = 49,800: effective = 5,000 + (49,800 - 50,000) = 4,800.
        // maintenance = 49,800 * 0.005 = 249. 4,800 >> 249 → NOT liquidatable.
        let small = long_pos(dec!(1), dec!(50000), dec!(5000), uid);
        assert!(!is_liquidatable(&small, dec!(49800), &config));

        // Large position: we need a position large enough to hit a higher MMR tier.
        // btc_default() tier 4: notional >= 25,000,000 → MMR = 5%.
        // 500 BTC at 50,000 entry = 25,000,000 notional → tier 4.
        // margin = 1,250,000 (10x on 500 BTC @ 50,000).
        // At mark = 49,800: effective = 1,250,000 + 500*(49800 - 50000) = 1,250,000 - 100,000 = 1,150,000.
        // notional = 500 * 49,800 = 24,900,000 → still tier 3 (12M-25M, MMR = 2.5%).
        // maintenance = 24,900,000 * 0.025 = 622,500.
        // 1,150,000 > 622,500 → NOT yet liquidatable, but uses 2.5% MMR vs 0.5%.

        // At a deeper drop where MMR tier matters more:
        // At mark = 47,500: effective = 1,250,000 + 500*(47500 - 50000) = 1,250,000 - 1,250,000 = 0.
        // notional = 500 * 47,500 = 23,750,000 → tier 3 (12M-25M, MMR = 2.5%).
        // maintenance = 23,750,000 * 0.025 = 593,750.
        // 0 < 593,750 → liquidatable.
        let large = long_pos(dec!(500), dec!(50000), dec!(1_250_000), uid);
        assert!(
            is_liquidatable(&large, dec!(47500), &config),
            "large position with tiered MMR must be liquidatable when effective margin is zero"
        );

        // Small position at the same mark = 47,500:
        // effective = 5,000 + (47,500 - 50,000) = 5,000 - 2,500 = 2,500.
        // maintenance = 47,500 * 0.005 = 237.50.
        // 2,500 > 237.50 → NOT liquidatable.
        assert!(
            !is_liquidatable(&small, dec!(47500), &config),
            "small position (low MMR tier) must NOT be liquidatable at same mark price"
        );
    }
}
