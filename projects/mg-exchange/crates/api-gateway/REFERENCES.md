# API Gateway — Business Logic References

Traces earn/yield product mechanics to industry standards.
Note: Earn products live in api-gateway because the business logic (product catalog,
allocations, yield accrual tasks) is in exchange.rs and tasks.rs.

## 27. Earn Products -- Yield Mechanics

### Mechanism

Earn products allow users to generate yield on idle assets through flexible savings,
fixed-term deposits, staking, and structured products.

### A. Mathematical Derivation

**APY to hourly rate conversion:**

```
hourly_rate = (1 + APY)^(1/8760) - 1
```

where 8760 = 365 * 24 (hours in a year). For small APY, this approximates:
```
hourly_rate ~ APY / 8760
```

**Hourly interest accrual (simple):**
```
interest_this_hour = principal * hourly_rate
```

**Auto-compound (hourly reinvestment):**
```
After N hours: principal_N = principal_0 * (1 + hourly_rate)^N
```

This achieves the full APY through compounding.

**Effective APY from hourly compounding:**
```
effective_APY = (1 + hourly_rate)^8760 - 1
```

When `hourly_rate = APY / 8760` (simple rate), the effective APY slightly exceeds
the stated APY due to compounding.

**Fixed-term:**
```
total_interest = principal * APY * (lock_days / 365)
```

Paid at maturity (auto-redemption at `unlock_at`).

**Staking rewards:**
```
reward_per_hour = staked_amount * staking_APY / 8760
```

Accrued hourly by background task `spawn_staking_rewards_accrual`.

### B. Worked Numerical Example

**Flexible savings (USDT, 5% APY, auto-compound):**
- Principal: $10,000
- Hourly rate: 5% / 8760 = 0.000005708

After 24 hours (simple):
```
interest = 10,000 * 0.000005708 * 24 = $1.3699
```

After 24 hours (auto-compound):
```
principal = 10,000 * (1 + 0.000005708)^24 = 10,000 * 1.000137 = $10,001.37
```

After 365 days (auto-compound):
```
principal = 10,000 * (1 + 0.000005708)^8760 = 10,000 * 1.05127 = $10,512.67
effective_APY = 5.127% (slightly above stated 5% due to hourly compounding)
```

**Fixed-term (30 days, 8% APY, BTC):**
- Principal: 1.0 BTC
```
total_interest = 1.0 * 0.08 * (30/365) = 0.00657534 BTC
```

After 30 days: user receives 1.0 + 0.006575 = 1.006575 BTC.

**Staking (ETH, 4% APY):**
- Staked: 10 ETH
```
reward_per_hour = 10 * 0.04 / 8760 = 0.0000456621 ETH
daily_reward = 0.0000456621 * 24 = 0.001096 ETH
annual_reward = 10 * 0.04 = 0.4 ETH
```

### C. Reference Implementation Comparison

| Platform | Flexible APY | Compounding | Fixed-Term | Difference |
|----------|-------------|-------------|-----------|------------|
| **Binance Earn** (https://www.binance.com/en/earn) | Variable (1-10% depending on asset). Hourly accrual for flexible. Fixed-term at higher rates. | Auto-subscribe (compound) optional. | 30/60/90/120 day locks. | Identical model. Our `supports_auto_compound` flag matches Binance's auto-subscribe toggle. |
| **Bybit Earn** | Similar rates. | Auto-compound available. | Fixed-term with early redemption penalty. | We do not implement early redemption penalty (status goes from "active" to "redeemed" at maturity). |
| **Aave/Compound** (DeFi) | Continuous accrual via interest rate model. Supply rate = utilization * borrow rate * (1 - reserve). | Continuous (per-block). | N/A (always flexible). | Our hourly accrual is a discrete approximation of DeFi's continuous model. |

### D. Edge Cases & Boundary Behavior

- **Zero APY product:** Interest accrued is always zero. Product may exist for custody/lock purposes.
- **Amount below minimum:** Subscription rejected by validation.
- **Amount above maximum:** Subscription rejected.
- **Lock period expiry:** Background task checks `unlock_at` and auto-redeems (returns principal + interest to available balance).
- **Auto-compound off:** Interest accumulates in `earned_interest` field without increasing `amount`.
- **Double accrual prevention:** `last_accrued_at` timestamp prevents the hourly task from double-counting.

### E. Test Coverage Reference

- **File:** `crates/api-gateway/tests/smoke_test.rs` (`smoke_earn_subscribe_auto_compound`)
- **File:** `crates/api-gateway/src/tasks.rs` (`spawn_staking_rewards_accrual`)
- **Key test:** Verifies that after N hours, earned_interest matches `principal * hourly_rate * N`.

### Our Implementation

- **Code:** `crates/api-gateway/src/exchange.rs` (EarnProduct, EarnAllocation structs)
- **Code:** `crates/api-gateway/src/rest.rs` (`earn_subscribe` handler)
- **Code:** `crates/api-gateway/src/tasks.rs` (`spawn_staking_rewards_accrual`)
- Product types: "flexible", "fixed_30d", "fixed_90d", "staking"
- Hourly accrual via background task.
- Auto-compound flag per allocation.

### Industry Standard / Reference

| Source | Description |
|--------|-------------|
| Binance. "Earn Products." (https://www.binance.com/en/earn) | Flexible savings, fixed-term, staking, dual investment. |
| Aave Protocol (https://aave.com/) | DeFi lending with continuous interest accrual. |
| Compound Protocol (https://compound.finance/) | DeFi lending with per-block interest. |
| MakerDAO DSR (Dai Savings Rate) | Protocol-level savings rate mechanism. |

### Compliance

**Match.** Our earn products follow the standard centralized exchange model (Binance, Bybit).
Hourly accrual with auto-compound option is industry standard.

---
