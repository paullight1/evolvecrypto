# Index Price — Business Logic References

Traces index price aggregation to industry standards.

## 7. Index Price -- Weighted Median

### Mechanism

The index price is a manipulation-resistant aggregation of spot prices from
multiple external exchanges.

### A. Mathematical Derivation

Given `n` sources with prices `p_1, p_2, ..., p_n` and weights `w_1, w_2, ..., w_n`:

1. Sort pairs `(p_i, w_i)` by price ascending.
2. Compute `W = sum(w_i)` (total weight).
3. Accumulate weights from the lowest price upward: `cum_w = 0`.
4. For each pair in sorted order: `cum_w += w_i`. If `cum_w >= W/2`, the current `p_i` is the weighted median.

**Why weighted median is more robust than weighted average:**

A single manipulated source with weight `w_k` can shift the weighted average by up to
`w_k / W * delta`, where `delta` is the manipulation magnitude. For the weighted median,
a single source can only shift the result if it has >= 50% of total weight. With 4+ sources
of similar weight, no single source can move the median at all.

### B. Worked Numerical Example

Sources for BTCUSDT:
| Exchange | Price | Weight |
|----------|-------|--------|
| Binance | $65,010 | 3 |
| Coinbase | $65,020 | 2 |
| Kraken | $65,000 | 2 |
| OKX | $67,000 | 1 | ← manipulated/stale

Sorted by price:
1. Kraken: $65,000, w=2, cum=2
2. Binance: $65,010, w=3, cum=5
3. Coinbase: $65,020, w=2, cum=7
4. OKX: $67,000, w=1, cum=8

Total weight W = 8. Half = 4.

After Kraken: cum=2 < 4. Continue.
After Binance: cum=5 >= 4. **Weighted median = $65,010.**

The $67,000 outlier from OKX has zero impact on the index.

If we used weighted average: `(65,000*2 + 65,010*3 + 65,020*2 + 67,000*1) / 8 = $65,258.75`. The outlier shifts the average by $248.

### C. Reference Implementation Comparison

| Exchange | Aggregation | Difference |
|----------|------------|------------|
| **Binance** (https://www.binance.com/en/support/faq/how-binance-futures-calculates-the-index-price-360033524991) | Volume-weighted average from 7+ exchanges with deviation-based outlier exclusion. Drops sources deviating >5% from median. | We use weighted median directly (no exclusion step needed). Strictly more robust — even a 49%-weight outlier cannot affect the result. |
| **Bybit** (https://www.bybit.com/en/help-center/article/Index-Price-Bybit) | Equal-weighted from multiple sources with deviation filtering. | We use configurable per-source weights; more flexible than equal weighting. |
| **Chainlink** (Oracle Network) | Weighted median across oracle nodes. | Our algorithm is identical to Chainlink's aggregation. |

### D. Edge Cases & Boundary Behavior

- **Single source:** Returns that source's price directly. No aggregation needed.
- **All weights zero:** Returns `Decimal::ZERO` (no valid sources).
- **Empty input:** Returns `Decimal::ZERO`.
- **Two sources, equal weight:** Returns the lower of the two (first to reach 50%).
- **Stale source:** Configurable timeout excludes sources that haven't updated. Weight drops to zero.

### E. Test Coverage Reference

- **File:** `crates/index-price/src/weighted_median.rs` (inline tests)
- **Key tests:** `test_single_source`, `test_outlier_resistance`, `test_equal_weights`, `test_empty_input`, `test_zero_weights`
- **Test values:** Prices $49,000--$51,000 with one outlier at $55,000.

### Our Implementation

- **Code:** `crates/index-price/src/weighted_median.rs` and `crates/index-price/src/aggregator.rs`
- Sort-and-accumulate algorithm. Handles non-normalized weights, zero weights, single sources.
- Sources include Binance, Coinbase, Kraken, OKX with configurable weights.

### Industry Standard / Reference

| Source | Description |
|--------|-------------|
| Huber, P. (2004). *Robust Statistics*. Wiley. | Foundational text on robust estimators; weighted median is a well-known robust measure. |
| Chainlink Labs (2017). "A Decentralized Oracle Network." | Weighted median for oracle price aggregation. |
| BitMEX. "Index Price." | Composite index with deviation-based exclusion. |
| Binance. "Index Price." | Volume-weighted composite with outlier removal. |

### Compliance

**Match.** The weighted median is strictly more robust than trimmed means used by some competitors.

---
