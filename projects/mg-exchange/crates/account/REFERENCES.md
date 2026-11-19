# Account — Business Logic References

Traces fee engine and VIP tier mechanics to industry standards.

## 24. Fee Engine & VIP Tiers

### Mechanism

The fee engine manages maker/taker fee rates based on a tiered VIP system where
users qualify through 30-day trading volume OR account balance.

### A. Mathematical Derivation

**Fee calculation per trade:**

```
fee = trade_notional * fee_rate
trade_notional = fill_qty * fill_price
```

where `fee_rate` is `maker_fee` if the order was resting (maker) or `taker_fee` if
the order was incoming (aggressor).

**Tier qualification (dual-path):**

```
qualified_tier = max tier T where:
    user.volume_30d >= T.min_volume_30d  OR  user.balance_usd >= T.min_balance
```

The OR condition means either sufficient trading volume or sufficient account balance
qualifies. This matches the Binance/Bybit model.

**Negative maker fee (rebate):**

At Supreme tier, maker_fee = -0.01% (negative). The exchange pays the maker:
```
rebate = trade_notional * 0.0001
```

The maker receives this rebate, incentivizing liquidity provision.

**Referral commission (2-level tree):**

```
level1_commission = trade_fee * commission_rate        (default 20%)
level2_commission = trade_fee * level2_rate             (default 5%)
```

Level 1 = direct referrer. Level 2 = the referrer's own referrer. No level 3.

**Referral code generation:**
```
code = SHA256(user_id || ":" || salt)[0..8] (hex, 8 chars)
```

Deterministic per user within a manager instance. Collisions resolved by counter suffix.

### B. Worked Numerical Example

**VIP tier schedule:**

| Level | Name | 30d Volume | Maker Fee | Taker Fee |
|-------|------|-----------|-----------|-----------|
| 0 | Regular | $0 | 0.10% | 0.20% |
| 1 | VIP1 | $1,000,000 | 0.08% | 0.16% |
| 2 | VIP2 | $5,000,000 | 0.06% | 0.12% |
| 3 | VIP3 | $25,000,000 | 0.04% | 0.08% |
| 4 | VIP4 | $100,000,000 | 0.02% | 0.05% |
| 5 | Supreme | $500,000,000 | -0.01% (rebate) | 0.03% |

**Trade fee example (VIP2):**
- Taker buys 1 BTC at $65,000
```
notional = 1 * 65,000 = $65,000
taker_fee = 65,000 * 0.0012 = $78.00
maker_fee = 65,000 * 0.0006 = $39.00
```

**Supreme maker rebate:**
- Maker provides liquidity, 10 BTC fills at $65,000
```
notional = 10 * 65,000 = $650,000
fee = 650,000 * (-0.0001) = -$65.00  (maker receives $65)
```

**Referral commission example:**
- Alice refers Bob. Charlie referred Alice.
- Bob trades $100,000 notional as taker (Regular tier: 0.20%)
```
trade_fee = 100,000 * 0.002 = $200
alice_commission (L1) = 200 * 0.20 = $40
charlie_commission (L2) = 200 * 0.05 = $10
exchange_revenue = 200 - 40 - 10 = $150
```

**Volume decay:**
- User traded $6M in 30-day window → VIP2.
- 30 days pass with no trading.
- `decay_stale_volumes()` fires: volume reset to $0.
- User demoted to Regular tier.

### C. Reference Implementation Comparison

| Exchange | Tiers | Maker (top tier) | Taker (top tier) | Referral | Difference |
|----------|-------|-----------------|-----------------|----------|------------|
| **Binance** (https://www.binance.com/en/fee/schedule) | 10 tiers (Regular -- VIP9) | -0.011% (rebate) | 0.023% | 40/10% default, 3 levels | Our top-tier rebate (-0.01%) is similar. We have 6 tiers vs 10. Our referral is 20/5% (2 levels). |
| **Bybit** (https://www.bybit.com/en/help-center/article/VIP-Fee-Structure) | 7 tiers (Non-VIP -- Pro-3) | -0.015% (Pro-3) | 0.030% (Pro-3) | 20% L1 | Similar structure. Our Supreme tier is close to Bybit Pro-2. |
| **OKX** (https://www.okx.com/fees) | 8 tiers | -0.015% (VIP7) | 0.030% (VIP7) | 30/10% | Slightly more aggressive rebates at top tier. |

### D. Edge Cases & Boundary Behavior

- **New user (no trades):** Defaults to Regular (tier 0): 0.10% maker, 0.20% taker.
- **Volume exactly at tier boundary:** `>=` comparison means the user qualifies for the higher tier.
- **Negative maker fee:** The exchange's net fee is `taker_fee - abs(maker_fee)`. If taker = 0.03% and maker = -0.01%, net = 0.02% (still positive revenue).
- **Volume window expiry:** After 30 days of inactivity, volume resets. Handled by `decay_stale_volumes()`.
- **Self-referral:** Returns `Err(ReferralError::SelfReferral)`.
- **Already referred:** Returns `Err(ReferralError::AlreadyReferred)`.

### E. Test Coverage Reference

- **File:** `crates/account/src/fee_engine.rs` (inline tests)
- **File:** `crates/account/src/referrals.rs` (inline tests)
- **File:** `crates/api-gateway/tests/trading_scenarios.rs` (`scenario_referral_commission`)
- **File:** `crates/api-gateway/tests/bug_fixes.rs` (`bug2_referral_commissions_credited_after_trade`)
- **Key test values:** Volumes $0--$500M, fee calculations verified to 8 decimal places.

### Our Implementation

- **Code:** `crates/account/src/fee_engine.rs` (VIP tiers, fee lookup, volume tracking)
- **Code:** `crates/account/src/referrals.rs` (referral tree, commission distribution)
- 6 VIP tiers (Regular through Supreme). OR qualification: volume OR balance.
- 30-day rolling volume with automatic decay.
- 2-level referral tree: 20% L1, 5% L2 commissions.

### Industry Standard / Reference

| Source | Description |
|--------|-------------|
| Binance Fee Schedule (https://www.binance.com/en/fee/schedule) | Industry-standard tiered fee model. |
| Bybit VIP Program (https://www.bybit.com/en/help-center/article/VIP-Fee-Structure) | Volume-based tiers with maker rebates. |
| OKX Fee Schedule (https://www.okx.com/fees) | Similar tiered structure. |
| ESMA (2018). "Best Execution under MiFID II." | Regulatory guidance on fee transparency. |

### Compliance

**Match.** Our fee model follows the standard tiered maker/taker structure used by all
major exchanges. The maker rebate at top tier is standard practice for incentivizing
liquidity provision.

---
