# Algo Execution — Business Logic References

Traces TWAP and VWAP execution algorithms to industry standards.

## 11. TWAP (Time-Weighted Average Price) Execution

### Mechanism

TWAP splits a large order into equal-sized slices spread uniformly across a
time window to minimise market impact.

### A. Mathematical Derivation

Given total quantity `Q`, number of intervals `N`, and total duration `T`:

```
slice_qty = Q / N
interval_duration = T / N
schedule = [t_0, t_0 + dt, t_0 + 2*dt, ..., t_0 + (N-1)*dt]
```

The expected execution price is the arithmetic mean of prices at each slice time:
```
expected_price = (1/N) * sum(P(t_i))   for i = 0..N-1
```

**Market impact model (Almgren-Chriss):**

The instantaneous market impact of each slice is approximately:
```
impact_per_slice ~ sigma * sqrt(slice_qty / ADV)
```

where `sigma` is volatility and `ADV` is average daily volume. Splitting into N slices
reduces total impact by approximately `sqrt(N)` vs a single block trade.

### B. Worked Numerical Example

- Buy 10 BTC over 2 hours, 10 slices
- slice_qty = 10/10 = 1 BTC per slice
- interval = 120 min / 10 = 12 minutes

| Slice | Time | Market Price | Fill Price |
|-------|------|-------------|-----------|
| 1 | 00:00 | $65,000 | $65,000 |
| 2 | 00:12 | $65,050 | $65,050 |
| 3 | 00:24 | $64,980 | $64,980 |
| ... | ... | ... | ... |
| 10 | 01:48 | $65,100 | $65,100 |

Average execution = sum(fills) / 10 = $65,030 (hypothetical).
TWAP benchmark = average of market prices at each interval = $65,030. Match.

### C. Reference Implementation Comparison

Standard academic algorithm (Almgren & Chriss, 2000). No exchange-specific variation.

### D. Edge Cases & Boundary Behavior

- **Price fence:** If market crosses the fence, remaining slices are paused/cancelled.
- **Zero slices:** Rejected at creation.
- **Single slice:** Degenerates to a single order.

### E. Test Coverage Reference

- **File:** `crates/algo-execution/src/twap.rs` (inline tests)

### Our Implementation

- **Code:** `crates/algo-execution/src/twap.rs`
- Equal-sized slices, configurable intervals, optional price fence.

### Industry Standard / Reference

| Source | Description |
|--------|-------------|
| Almgren, R. & Chriss, N. (2000). "Optimal Execution." *Journal of Risk*, 3(2). | TWAP as the baseline execution strategy. |
| Kissell, R. (2014). *The Science of Algorithmic Trading*. | Comprehensive execution algorithm reference. |

### Compliance

**Match.** Standard TWAP with price fence enhancement.

---

## 12. VWAP (Volume-Weighted Average Price) Execution

### Mechanism

VWAP distributes a large order across time slices proportional to a historical
volume profile.

### A. Mathematical Derivation

Given total quantity `Q`, `N` time buckets, and volume profile `v_1, v_2, ..., v_N`:

```
total_profile = sum(v_i)
slice_qty_i = Q * (v_i / total_profile)
```

The expected execution price should track the VWAP benchmark:
```
VWAP = sum(P_i * V_i) / sum(V_i)
```

where `V_i` is actual market volume in bucket `i`. By matching the volume profile,
our participation rate is constant, minimizing information leakage.

### B. Worked Numerical Example

- Buy 10 BTC, 5 buckets, volume profile: [30%, 20%, 10%, 15%, 25%]

| Bucket | Profile | Slice Qty | Market Vol | Our % |
|--------|---------|-----------|-----------|-------|
| 1 | 30% | 3.0 BTC | 150 BTC | 2.0% |
| 2 | 20% | 2.0 BTC | 100 BTC | 2.0% |
| 3 | 10% | 1.0 BTC | 50 BTC | 2.0% |
| 4 | 15% | 1.5 BTC | 75 BTC | 2.0% |
| 5 | 25% | 2.5 BTC | 125 BTC | 2.0% |

Constant 2% participation rate across all buckets.

### C. Reference Implementation Comparison

Standard algorithm. Fallback to TWAP when no volume profile is available matches industry practice.

### D. Edge Cases & Boundary Behavior

- **Empty profile / all zeros:** Falls back to TWAP (uniform distribution).
- **Non-normalized weights:** Auto-normalized by dividing each weight by total.

### E. Test Coverage Reference

- **File:** `crates/algo-execution/src/vwap.rs` (inline tests)

### Our Implementation

- **Code:** `crates/algo-execution/src/vwap.rs`
- Volume profile input, auto-normalization, TWAP fallback, optional price fence.

### Industry Standard / Reference

| Source | Description |
|--------|-------------|
| Berkowitz, S. et al. (1988). "The Total Cost of Transactions on the NYSE." | VWAP as execution benchmark. |
| SEC Rule 10b-18 | VWAP-like conditions in safe harbor for issuer repurchases. |

### Compliance

**Match.** Standard VWAP with TWAP fallback.

---
