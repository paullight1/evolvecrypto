# Trading Bots — Business Logic References

Traces grid trading and DCA mechanics to industry standards.

## 13. Grid Trading

### Mechanism

Grid trading places evenly-spaced buy and sell orders across a defined price
range, profiting from oscillation within that range.

### A. Mathematical Derivation

Given `upper_price`, `lower_price`, and `grid_count` N:

```
grid_spacing = (upper_price - lower_price) / N
grid_levels = [lower_price + i * grid_spacing]  for i = 0..N
per_grid_investment = total_investment / N
qty_per_grid = per_grid_investment / grid_level_price
```

**Profit per grid capture:**
When price oscillates from one grid level to the adjacent level:
```
profit = qty * grid_spacing - 2 * qty * avg_price * fee_rate
```

The break-even condition requires `grid_spacing / avg_price > 2 * fee_rate`.

### B. Worked Numerical Example

- Range: $60,000 -- $70,000, 10 grids, $10,000 total investment
- Spacing: ($70,000 - $60,000) / 10 = $1,000
- Per grid: $1,000

Grid levels: $60k, $61k, $62k, ..., $70k

Initial buy orders placed below current price ($65,000): $60k, $61k, $62k, $63k, $64k.
Initial sell orders placed above: $66k, $67k, $68k, $69k, $70k.

Price drops to $64,000: buy fills at $64,000, qty = $1,000/$64,000 = 0.015625 BTC.
Immediately place sell at $65,000 (one grid up).

Price rises to $65,000: sell fills at $65,000.
Profit = 0.015625 * (65,000 - 64,000) = $15.625 gross.
Fee (assuming 0.1%): 0.015625 * 64,000 * 0.001 + 0.015625 * 65,000 * 0.001 = $1.00 + $1.02 = $2.02
Net profit = $15.625 - $2.02 = $13.61 per grid capture.

### C. Reference Implementation Comparison

Standard arithmetic grid identical to Binance Grid Bot and Bybit Grid Bot. We support neutral/long/short modes.

### D. Edge Cases & Boundary Behavior

- **Price exits range:** Grid stops capturing profit but holds positions. No loss beyond opportunity cost.
- **Zero grid count:** Rejected at creation.
- **Upper == lower:** Rejected (no spacing possible).

### E. Test Coverage Reference

- **File:** `crates/trading-bots/src/grid.rs` (inline tests)

### Compliance

**Match.** Standard arithmetic grid with neutral/long/short modes.

---

## 14. DCA (Dollar-Cost Averaging)

### Mechanism

DCA invests a fixed quote-asset amount at regular intervals regardless of
price.

### A. Mathematical Derivation

Over `N` purchases of fixed amount `A` at prices `P_1, P_2, ..., P_N`:

```
total_invested = N * A
total_received = A/P_1 + A/P_2 + ... + A/P_N = A * sum(1/P_i)
average_cost = total_invested / total_received = N / sum(1/P_i)
```

This is the **harmonic mean** of the purchase prices. The harmonic mean is always
<= the arithmetic mean when prices vary, meaning DCA systematically buys more
units when prices are low and fewer when prices are high.

### B. Worked Numerical Example

Monthly DCA of $1,000 into BTC:

| Month | Price | BTC Bought | Cumulative BTC | Avg Cost |
|-------|-------|-----------|---------------|---------|
| 1 | $65,000 | 0.01538 | 0.01538 | $65,000 |
| 2 | $60,000 | 0.01667 | 0.03205 | $62,402 |
| 3 | $55,000 | 0.01818 | 0.05023 | $59,726 |
| 4 | $70,000 | 0.01429 | 0.06452 | $62,000 |

After 4 months: $4,000 invested, 0.06452 BTC received.
Average cost = 4,000 / 0.06452 = $62,000/BTC.
Arithmetic mean of prices = (65+60+55+70)/4 = $62,500.

DCA average ($62,000) < arithmetic mean ($62,500). DCA saved $500 * 0.06452 = $32.26.

### C. Reference Implementation Comparison

Standard DCA identical to Binance Auto-Invest. Our price deviation filter matches 3Commas "safety orders."

### D. Edge Cases & Boundary Behavior

- **Zero amount:** Rejected.
- **Price = 0:** Division by zero guarded in quantity calculation.
- **Deviation filter:** Only buys when price has dipped below last fill by configured percentage.

### E. Test Coverage Reference

- **File:** `crates/trading-bots/src/dca.rs` (inline tests)

### Compliance

**Match.** Standard DCA with deviation filter.

---
