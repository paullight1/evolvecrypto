# Custody — Business Logic References

Traces custody and wallet architecture to industry standards.

## 29. Custody -- Wallet Architecture

### Mechanism

The custody system manages exchange-held assets across a hot/warm/cold tiered wallet
architecture with policy-based limits and circuit breakers.

### A. Mathematical Derivation

**Hot/warm/cold allocation policy:**
```
hot_max = total_aum * hot_max_pct       (default 5%)
warm_max = total_aum * warm_max_pct     (default 15%)
cold_min = total_aum * (1 - hot_max_pct - warm_max_pct)  (default 80%)
```

**Sweep trigger:**
```
if hot_balance > sweep_threshold:
    recommend sweep: hot → cold
    sweep_amount = hot_balance - (total_aum * hot_max_pct)
```

**Outflow circuit breaker thresholds:**
```
max_single_withdrawal = $500,000
max_hourly_outflow = $10,000,000
max_daily_outflow = $50,000,000
```

**Check:**
```
if withdrawal_amount > max_single_withdrawal: REJECT
if hourly_cumulative + withdrawal_amount > max_hourly_outflow: REJECT + TRIP
if daily_cumulative + withdrawal_amount > max_daily_outflow: REJECT + TRIP
```

When tripped, ALL withdrawals are blocked until admin manually resets.

### B. Worked Numerical Example

**Wallet allocation (BTC, total AUM = 1000 BTC):**
```
hot_max = 1000 * 0.05 = 50 BTC
warm_max = 1000 * 0.15 = 150 BTC
cold_min = 1000 * 0.80 = 800 BTC
```

Current state: Hot = 60 BTC. This exceeds hot_max by 10 BTC.
Recommendation: Sweep 10 BTC from hot to cold.

**Circuit breaker scenario:**
- Hourly outflow tracker: $8,500,000 already processed this hour.
- New withdrawal request: $2,000,000

```
8,500,000 + 2,000,000 = $10,500,000 > $10,000,000 (hourly limit)
→ REJECT withdrawal
→ TRIP circuit breaker
→ ALL withdrawals blocked until admin reset
→ Alert: "Hourly outflow limit exceeded"
```

**Per-transaction check:**
- Withdrawal: $600,000
```
600,000 > 500,000 (per-transaction limit)
→ REJECT
→ TRIP breaker: "Single withdrawal $600,000 exceeds per-transaction limit $500,000"
```

### C. Reference Implementation Comparison

| Platform | Hot/Cold Split | Circuit Breaker | Difference |
|----------|---------------|----------------|------------|
| **BitGo** (https://www.bitgo.com/resources/case-studies) | Multi-sig hot/cold with configurable policies. TSS or 2-of-3 multi-sig. | Per-wallet and per-org velocity limits. | We implement policy-based limits without multi-sig (Phase 1 in-memory). BitGo adds multi-party approval. |
| **Fireblocks** (https://www.fireblocks.com/platforms/mpc-wallet/) | MPC-based key management. No single point of compromise. Hot/warm/cold tiers. | Customizable policy engine with approval workflows. | Our custody module includes a Fireblocks integration stub (`fireblocks.rs`). Production would use Fireblocks' MPC for key management. |
| **Coinbase Custody** | 98% cold storage, 2% hot. Multi-approval for large withdrawals. | Tiered approval thresholds. | Our 5% hot, 15% warm, 80% cold is more aggressive than Coinbase's 2% but within industry norms. |

### D. Edge Cases & Boundary Behavior

- **Circuit breaker already tripped:** All withdrawals immediately rejected with `AlreadyTripped` error.
- **Admin reset:** Clears trip state, resets reason. Does NOT reset hourly/daily accumulators.
- **Rolling window expiry:** Hourly outflows older than 1 hour are pruned. Daily outflows older than 24 hours are pruned.
- **Zero withdrawal:** Passes all checks (trivially within limits).
- **Unknown coin policy:** Default policies for BTC, ETH, USDT, USDC, SOL. Other coins use no automatic sweep threshold.
- **New address detection:** `new_address_count_hourly` tracks withdrawals to never-before-seen addresses. Anomalous spike triggers additional review.

### E. Test Coverage Reference

- **File:** `crates/custody/src/circuit_breaker.rs` (inline tests)
- **File:** `crates/custody/src/wallet_management.rs` (inline tests)
- **Key tests:** `test_single_withdrawal_exceeded`, `test_hourly_limit`, `test_daily_limit`, `test_trip_and_reset`, `test_hot_warm_cold_balances`

### Our Implementation

- **Code:** `crates/custody/src/wallet_management.rs` (WalletManager, WalletPolicy, hot/warm/cold tiers)
- **Code:** `crates/custody/src/circuit_breaker.rs` (OutflowCircuitBreaker)
- **Code:** `crates/custody/src/deposit.rs` (deposit address management)
- **Code:** `crates/custody/src/withdrawal.rs` (withdrawal approval workflow)
- **Code:** `crates/custody/src/address_whitelist.rs` (address whitelist)
- **Code:** `crates/custody/src/fireblocks.rs` (Fireblocks MPC integration stub)

### Industry Standard / Reference

| Source | Description |
|--------|-------------|
| BitGo. "Custody Documentation." (https://www.bitgo.com/) | Multi-sig custody with policy engine. |
| Fireblocks. "MPC Architecture." (https://www.fireblocks.com/) | MPC-based key management, no single point of compromise. |
| Basel Committee (2011). "Sound Practices for the Management and Supervision of Operational Risk." | Regulatory framework for operational risk, including custody and asset management. |
| CCSS (CryptoCurrency Security Standard) (https://cryptoconsortium.org/ccss/) | Industry security standard for cryptocurrency custody. |
| SOC 2 Type II | Audit framework for security controls at service organizations. |

### Compliance

**Match (Phase 1).** Hot/warm/cold tiering with policy limits matches industry practice.
Circuit breaker thresholds are conservative ($500K per transaction, $10M/hour, $50M/day).
Production deployment should integrate Fireblocks or BitGo for MPC-based key management.
