# Risk Engine — Business Logic References

Traces every risk engine mechanism to industry standards.
Covers: mark price, liquidation, ADL, margin calculation, portfolio margin,
options pricing, collateral haircuts, circuit breakers, socialized loss.

---

## 2. Mark Price / Fair Price Marking

### Mechanism

Mark price is the fair value used for unrealised PnL and liquidation checks.
It is anchored to the external spot index to prevent manipulation via the
exchange's own last traded price.

### A. Mathematical Derivation

Our formula:

```
mark_price = (2 * index_price + last_price) / 3
```

This is a weighted average where `w_index = 2/3` and `w_last = 1/3`. The derivation
of the manipulation resistance property:

Let `index = I` (assumed fair) and `last = I + d` (where `d` is the deviation due to
potential manipulation on this exchange). Then:

```
mark = (2I + (I + d)) / 3 = (3I + d) / 3 = I + d/3
```

The manipulator can shift mark price by at most `d/3` -- i.e., only 33% of the
deviation they create on the local exchange propagates to mark price.

**BitMEX model** (for comparison):

```
mark = index + EMA_30s(perp_mid - index)
```

where `EMA_30s` is a 30-second exponential moving average of the basis. This
dampens short-term spikes more aggressively. Our formula responds instantly to
basis changes but limits their magnitude.

### B. Worked Numerical Example

**Scenario 1: Normal conditions**
- Index price: $65,000 (from weighted median of Binance, Coinbase, Kraken, OKX)
- Last traded price on our exchange: $65,150

```
mark = (2 * 65,000 + 65,150) / 3 = (130,000 + 65,150) / 3 = 195,150 / 3 = $65,050
```

The mark price is $65,050, halfway between the index and a simple average.

**Scenario 2: Attempted manipulation**
- Index: $65,000
- Last traded: $66,000 (attacker pushed price up $1,000)

```
mark = (2 * 65,000 + 66,000) / 3 = 196,000 / 3 = $65,333.33
```

Despite a $1,000 manipulation, mark only moved $333.33 from fair value.

**Scenario 3: Index equals last**
- Index: $65,000, Last: $65,000

```
mark = (130,000 + 65,000) / 3 = $65,000
```

When prices agree, mark equals both.

### C. Reference Implementation Comparison

| Exchange | Formula | Difference |
|----------|---------|------------|
| **Binance** (https://www.binance.com/en/support/faq/how-to-calculate-mark-price-in-usd-s-m-futures-contracts-3a1a957c20834a0db41343ea78990ced) | `mark = median(price_1, price_2, contract_price)` where `price_1 = index * (1 + last_funding_rate)`, `price_2 = index + EMA_basis`. Three-input median for additional robustness. | Our formula uses a simpler weighted average. Binance includes the funding rate in the calculation; we do not. |
| **Bybit** (https://www.bybit.com/en/help-center/article/Mark-Price-Bybit) | `mark = index + EMA(basis)` with 30-second EMA. Identical to BitMEX. | We lack EMA dampening. Our index weight (67%) is more conservative than Bybit's (50%). |
| **Our implementation** | `mark = (2 * index + last) / 3`. Simpler, no EMA state. | Missing EMA means slightly noisier mark during high volatility. The `_funding_rate` parameter placeholder exists for future upgrade. |

### D. Edge Cases & Boundary Behavior

- **Zero index price:** Returns mark = last_price / 3, which would be incorrect. In practice, index price is never zero because the aggregator requires at least one source. The `basis_rate()` function returns `None` when index is zero.
- **Very large basis:** If last = 2 * index, mark = (2I + 2I) / 3 = 4I/3. The maximum mark displacement from index is always bounded by `abs(last - index) / 3`.
- **Negative prices:** Not applicable in crypto (prices are always positive). The Decimal type handles signed arithmetic correctly regardless.
- **Equal prices:** When index == last, mark == both. Verified by test `test_mark_price_equal_index_and_last`.

### E. Test Coverage Reference

- **File:** `crates/risk-engine/src/mark_price.rs` (inline tests)
- **Key tests:**
  - `test_mark_price_equal_index_and_last`: index=50000, last=50000, mark=50000
  - `test_mark_price_last_above_index`: index=50000, last=50300, mark=50100
  - `test_mark_price_last_below_index`: symmetry test
  - `test_basis_rate_zero_index`: returns None
- **Test values:** Prices around $50,000.

### Our Implementation

- **Code:** `crates/risk-engine/src/mark_price.rs`, function `calculate_mark_price`
- **Formula:** `mark = (2 * index_price + last_price) / 3`
- The last traded price can move mark by at most 33% of the spread between
  index and last, limiting single-exchange manipulation impact.
- The `funding_rate` parameter is accepted for future EMA-based mark price
  but is currently unused.

### Industry Standard / Reference

| Source | Description |
|--------|-------------|
| BitMEX (2016). "Fair Price Marking." | Pioneered fair-price marking for perpetual swaps: `mark = index + 30s EMA(perp_mid - index)`. |
| Binance Futures. "Mark Price." | `mark = median(price_1, price_2, contract_price)` where `price_1 = index * (1 + last_funding)`. |
| dYdX v3 Protocol Specification | Mark = index-anchored price with impact mid smoothing. |

### Compliance

**Partial match with noted deviation.** Our formula correctly anchors mark
price to the spot index (the core principle). The deviation is the absence of
EMA dampening on the basis component; BitMEX and Binance both use time-smoothed
basis. Our simplified formula is conservative (heavier index weighting: 67%
vs. typical 50%) but may produce noisier mark prices during high-volatility
periods. The `_funding_rate` parameter placeholder indicates intent to upgrade
to EMA-based marking.

**Recommendation:** Implement 30-second EMA of (perp_mid - index) for production
to match the BitMEX standard and reduce mark price noise.

---

## 4. Liquidation Engine

### Mechanism

The liquidation engine monitors all open positions against current mark prices.
When a position's effective margin falls below the maintenance margin
requirement, the engine triggers liquidation.

### A. Mathematical Derivation

**Trigger condition:**

```
effective_margin < maintenance_margin
```

where:
```
effective_margin = margin + unrealized_pnl
unrealized_pnl_long = (mark_price - entry_price) * size
unrealized_pnl_short = (entry_price - mark_price) * size
maintenance_margin = notional * tiered_mmr
notional = size * mark_price  (linear)
```

**Liquidation price derivation (linear, long):**

We want to find the mark price `P_liq` at which `effective_margin = maintenance_margin`:

```
margin + (P_liq - entry) * size = size * P_liq * mmr
margin + P_liq * size - entry * size = size * P_liq * mmr
margin - entry * size = size * P_liq * mmr - P_liq * size
margin - entry * size = P_liq * size * (mmr - 1)
P_liq = (margin - entry * size) / (size * (mmr - 1))
P_liq = entry - margin/size * (1/(1 - mmr))
```

For small mmr (e.g., 0.5%), `1/(1-mmr) ~ 1`, so:

```
P_liq ~ entry - margin/size  (approximate)
```

**Bankruptcy price (linear):**

The bankruptcy price is where the position's equity is exactly zero:

```
margin + (P_bk - entry) * size = 0
P_bk = entry - margin/size  (long)
P_bk = entry + margin/size  (short)
```

**Bankruptcy price (inverse, long):**

For inverse contracts, margin and PnL are in the base asset:
```
unrealized_pnl = size * contract_size * (1/entry - 1/mark)
```

Setting equity to zero:
```
margin + size * cs * (1/entry - 1/P_bk) = 0
margin = size * cs * (1/P_bk - 1/entry)
margin = size * cs / P_bk - size * cs / entry
margin + size * cs / entry = size * cs / P_bk
P_bk = size * cs / (margin + size * cs / entry)
P_bk = entry * size * cs / (size * cs + margin * entry)
```

Which is our formula: `bk = entry * N*cs / (N*cs + margin*entry)` for longs.

For shorts: `bk = entry * N*cs / (N*cs - margin*entry)`.

**Partial liquidation sizing:**

The engine targets remaining margin ratio above maintenance with a 10% safety buffer:

```
effective_per_unit = margin/size + upnl_per_unit
required_per_unit = mark_price * mmr * 1.1
```

If `effective_per_unit >= required_per_unit` for a lower tier, the engine finds the
maximum remaining size that fits within that tier, minimizing the liquidation quantity.

### B. Worked Numerical Example

**Linear long position liquidation:**

- User opens 1 BTC long at $65,000 with 10x leverage
- Initial margin = 1 * 65,000 / 10 = $6,500
- Tier-0 MMR = 0.5% (maintenance margin rate for < $2M notional)

At what price does liquidation trigger?

```
effective_margin = 6,500 + (P - 65,000) * 1
maintenance_margin = 1 * P * 0.005

Trigger: 6,500 + P - 65,000 < P * 0.005
         6,500 - 65,000 < P * 0.005 - P
         -58,500 < P * (0.005 - 1)
         -58,500 < -0.995 * P
         P < 58,500 / 0.995
         P < $58,793.97
```

If mark price drops to $58,793:
```
effective_margin = 6,500 + (58,793 - 65,000) * 1 = 6,500 - 6,207 = $293
maintenance_margin = 1 * 58,793 * 0.005 = $293.97
293 < 293.97 → LIQUIDATION TRIGGERED
```

Bankruptcy price:
```
P_bk = 65,000 - 6,500/1 = $58,500
```

Liquidation fee:
```
fee = notional * 0.0015 = 58,793 * 0.0015 = $88.19 (to insurance fund)
```

If the position liquidates at $58,700 (fill price):
```
surplus = (58,700 - 58,500) * 1 = $200 → insurance fund
```

If the position liquidates at $58,400 (below bankruptcy):
```
deficit = (58,500 - 58,400) * 1 = $100 → insurance fund covers (or socializes)
```

**Partial liquidation example:**

- User has 5 BTC long at $65,000 with 10x, margin = $32,500
- Mark drops to $63,000
- Unrealized PnL = (63,000 - 65,000) * 5 = -$10,000
- Effective margin = 32,500 - 10,000 = $22,500
- Notional at mark = 5 * 63,000 = $315,000
- Maintenance margin = 315,000 * 0.005 = $1,575
- 22,500 > 1,575 → NOT liquidatable yet

Now mark drops to $59,000:
- Unrealized PnL = (59,000 - 65,000) * 5 = -$30,000
- Effective margin = 32,500 - 30,000 = $2,500
- Notional = 5 * 59,000 = $295,000
- Maintenance = 295,000 * 0.005 = $1,475
- 2,500 > 1,475 → still safe

Mark drops to $58,800:
- Unrealized PnL = -$31,000
- Effective margin = 32,500 - 31,000 = $1,500
- Maintenance = 5 * 58,800 * 0.005 = $1,470
- 1,500 > 1,470 → barely safe

Mark drops to $58,700:
- Unrealized PnL = -$31,500
- Effective margin = $1,000
- Maintenance = 5 * 58,700 * 0.005 = $1,467.50
- 1,000 < 1,467.50 → LIQUIDATION TRIGGERED

Partial: reduce to size where effective_margin > maintenance * 1.1:
The engine walks tiers and finds the largest remaining position that satisfies the
per-unit margin condition.

### C. Reference Implementation Comparison

| Exchange | Liquidation | Difference |
|----------|------------|------------|
| **Binance** (https://www.binance.com/en/support/faq/how-to-calculate-liquidation-price-of-usd-s-m-futures-contracts-b3c689c1f50a44cfa57b88f27aee5a5f) | `Lp = WB / (qty * (MMR - side_factor)) + entry` where WB = wallet balance. Includes accumulated funding in the calculation. Tiered MMR. Partial liquidation with smart reduction. | Our formula uses isolated margin amount only (not wallet balance for cross-margin). Binance includes funding; we settle funding separately. |
| **Bybit** (https://www.bybit.com/en/help-center/article/Liquidation-Price-Calculation) | Standard bankruptcy price formulas for linear and inverse. Partial liquidation before full close. | **Identical** for isolated margin. Our inverse formula matches exactly. |

### D. Edge Cases & Boundary Behavior

- **Zero mark price:** `partial_liquidation_qty` returns full position size immediately (size, false) when mark is zero.
- **Zero position size:** Returns (size=0, false). No-op.
- **Position already at bankruptcy:** Full liquidation with zero surplus. Insurance fund absorbs any deficit.
- **Maximum leverage (125x):** At 125x, initial margin is 0.8% of notional. With tier-0 MMR of 0.5%, the position can only sustain a 0.3% adverse move before liquidation.
- **Cascade prevention:** `max_batch_size` limits the number of liquidations per cycle. Default prevents more than N liquidations per call to `check_all_positions`.
- **Cross-margin:** Checks total equity (sum of all wallet balances + total unrealized PnL) vs. sum of all maintenance margins for cross-mode positions.

### E. Test Coverage Reference

- **File:** `crates/risk-engine/src/liquidation.rs` (inline tests)
- **File:** `crates/risk-engine/src/adversarial_tests.rs`
- **File:** `crates/api-gateway/tests/trading_scenarios.rs` (`scenario_liquidation_partial`, `scenario_liquidation_cross_margin`)
- **Key test values:** Entry $50,000, margin $5,000 (10x), mark dropping to $45,000--$44,000 range.

### Our Implementation

- **Code:** `crates/risk-engine/src/liquidation.rs`
- **Trigger condition:** `effective_margin < maintenance_margin`
- **Partial liquidation:** Reduces position just enough to bring margin ratio above maintenance + 10% safety buffer.
- **Bankruptcy price (linear):** Long: `entry - margin/size`, Short: `entry + margin/size`
- **Bankruptcy price (inverse):** Long: `entry * N*cs / (N*cs + margin*entry)`, Short: `entry * N*cs / (N*cs - margin*entry)`
- **Liquidation fee:** 0.15% of notional, directed to insurance fund.
- **Cascade prevention:** Maximum batch size per cycle limits chain liquidations.

### Industry Standard / Reference

| Source | Description |
|--------|-------------|
| BitMEX. "Liquidation." | Partial liquidation before full close; insurance fund absorption of surplus/deficit; bankruptcy price concept. |
| Binance Futures. "Liquidation." | Tiered maintenance margin with partial liquidation; separate isolated and cross-margin checks. |
| Bybit. "Liquidation Price Calculation." | Standard bankruptcy price formulas for linear and inverse contracts. |
| CFTC Regulation 17 CFR 39.13 | US regulatory standard for margin requirements in derivatives clearing. |
| CME Rulebook Chapter 8 (Clearing House) | Default waterfall: margin, guaranty fund, clearing house capital. |

### Compliance

**Match.** Our liquidation engine implements tiered maintenance margin rates,
partial liquidation with safety buffer, correct bankruptcy price formulas for
both linear and inverse contracts, cascade prevention via batch limiting, and
both isolated-margin and cross-margin liquidation paths.

---

## 5. Auto-Deleveraging (ADL)

### Mechanism

When a liquidation results in a loss exceeding the insurance fund balance, the
most profitable opposing positions are force-closed to cover the deficit.

### A. Mathematical Derivation

**ADL score formula:**

```
score = pnl_ratio * effective_leverage
```

where:

```
pnl_ratio = unrealized_pnl / margin        (ROE -- return on equity)
effective_leverage = notional / effective_margin
effective_margin = margin + unrealized_pnl
notional = size * mark_price
```

**Why this ranking works:** The score combines two dimensions:
1. `pnl_ratio` (ROE): How much profit relative to margin. High ROE = took on risk and won.
2. `effective_leverage`: How leveraged the position currently is. High leverage = less cushion.

The product selects positions that are both highly profitable AND highly leveraged -- the
users most capable of absorbing a forced close without catastrophic loss (they are in profit)
AND most deserving of deleveraging (they are over-exposed).

**Quantile indicator:**

```
quantile = ceil(rank_position / total_candidates * 5)
```

Bucket 5 (top 20%) = highest risk of being ADL'd. Bucket 1 = lowest risk.

### B. Worked Numerical Example

**Setup:** Bankrupt short position at $65,000 with $500 deficit after insurance fund depletion.

Opposing long candidates:

| User | Entry | Size | Margin | Mark=$66,000 | uPnL | pnl_ratio | eff_lev | Score |
|------|-------|------|--------|-------------|------|-----------|---------|-------|
| Alice | $64,000 | 0.5 | $3,200 | $33,000 | $1,000 | 0.3125 | 7.857 | 2.455 |
| Bob | $63,000 | 1.0 | $6,300 | $66,000 | $3,000 | 0.4762 | 7.097 | 3.380 |
| Carol | $65,500 | 2.0 | $13,100 | $132,000 | $1,000 | 0.0763 | 9.362 | 0.714 |

Detailed calculation for Bob:
```
pnl_ratio = 3,000 / 6,300 = 0.4762
effective_margin = 6,300 + 3,000 = 9,300
effective_leverage = 66,000 / 9,300 = 7.097
score = 0.4762 * 7.097 = 3.380
```

**Ranking:** Bob (3.380) > Alice (2.455) > Carol (0.714)

**ADL execution:** Deficit = $500, bankruptcy_price = $64,500 (of the bankrupt short).

Bob is first. Need to close enough of Bob's position at $64,500 to cover $500:
```
close_qty = 500 / 64,500 = 0.00775 BTC
```

Bob is partially closed: 0.00775 BTC at $64,500 (not mark price).
Bob's remaining position: 1.0 - 0.00775 = 0.99225 BTC.

Deficit covered. Alice and Carol are not touched.

### C. Reference Implementation Comparison

| Exchange | ADL Ranking | Difference |
|----------|------------|------------|
| **Binance** (https://www.binance.com/en/support/faq/auto-deleveraging-adl-360033525271) | `ranking = PnL_percentage * effective_leverage`. Quantile indicator 1--5. Only triggered after insurance fund exhaustion. | **Identical formula.** Our 5-bucket quantile matches. |
| **Bybit** (https://www.bybit.com/en/help-center/article/Auto-Deleveraging-ADL) | Same formula: `score = ROE * effective_leverage`. Same quantile system. | **Identical.** |

### D. Edge Cases & Boundary Behavior

- **Zero margin position:** Returns `score = 0`. Cannot divide by zero in pnl_ratio. These positions are not selected.
- **Negative effective margin:** Returns `score = 0`. A position with negative effective margin is underwater, not profitable.
- **No profitable opposing positions:** `calculate_adl` returns an empty Vec. The deficit remains uncovered (tracked as socialized loss).
- **Deficit larger than all candidates:** All candidates are fully closed. Remaining deficit is socialized.
- **Zero bankruptcy price:** `calculate_adl` returns empty Vec immediately.

### E. Test Coverage Reference

- **File:** `crates/risk-engine/src/adl.rs` (inline tests)
- **Key tests:** `test_adl_score_profitable`, `test_adl_ranking_order`, `test_adl_partial_close`, `test_adl_full_deficit_coverage`, `test_adl_zero_margin`
- **Test values:** Positions with entry ~$50,000, varying margins and PnL.

### Our Implementation

- **Code:** `crates/risk-engine/src/adl.rs`
- **Ranking formula:** `score = pnl_ratio * effective_leverage`
- Only profitable positions on the opposite side are ranked.
- Positions are closed at the bankrupt position's bankruptcy price (not mark).
- Partial deleveraging: only enough size is closed to cover the deficit.
- Quantile indicator (1--5): users can monitor their ADL risk bucket.

### Industry Standard / Reference

| Source | Description |
|--------|-------------|
| BitMEX (2016). "Auto-Deleveraging." | Original ADL mechanism. Ranking = PnL percentage * effective leverage. |
| Bybit. "Auto-Deleveraging (ADL)." | Identical ranking formula; quantile indicator with 5 levels. |
| OKX. "Auto-Deleveraging." | Similar ranking; triggered only after insurance fund exhaustion. |
| Binance Futures. "Auto-Deleveraging." | Same concept, triggered as last resort after insurance fund. |

### Compliance

**Match.** Our ADL scoring formula is identical to the BitMEX original. The quantile
indicator matches the Bybit/Binance convention. Execution at bankruptcy price is standard.

---

## 8. Margin Calculation (Linear & Inverse)

### Mechanism

Initial and maintenance margin requirements for perpetual futures positions,
with distinct formulas for USDT-margined (linear) and coin-margined (inverse)
contracts.

### A. Mathematical Derivation

**Linear (USDT-margined):**

Initial margin is the collateral required to open a position:
```
initial_margin = notional / leverage = size * price / leverage
```

This is derived from the leverage definition: `leverage = notional / margin`, thus `margin = notional / leverage`.

Maintenance margin is the minimum collateral to keep a position open:
```
maintenance_margin = size * mark_price * mmr
```

where `mmr` is the tiered maintenance margin rate.

**Inverse (coin-margined):**

For inverse contracts, each contract represents a fixed USD value (`contract_size`, typically $1 or $100).
Margin is denominated in the base asset.

```
initial_margin_base = (contracts * contract_size) / (price * leverage)
```

Derivation: The USD notional is `contracts * contract_size`. To express this in base asset terms,
divide by price: `notional_base = notional_usd / price`. Then apply leverage:
`margin = notional_base / leverage = (contracts * contract_size) / (price * leverage)`.

Maintenance:
```
maintenance_margin_base = (contracts * contract_size) / mark_price * mmr
```

**Tiered MMR schedule (default, Binance-style):**

| Tier | Notional Range | MMR |
|------|---------------|-----|
| 0 | $0 -- $2,000,000 | 0.50% |
| 1 | $2,000,000 -- $10,000,000 | 1.00% |
| 2 | $10,000,000 -- $25,000,000 | 2.50% |
| 3 | > $25,000,000 | 5.00% |

The effective MMR is determined by the position's current notional value.

**Available margin:**
```
available = max(equity - total_initial_margin, 0)
```

**Max position size (linear):**
```
max_size = available_margin * leverage / price
```

### B. Worked Numerical Example

**Linear position:**
- Buy 2 BTC at $65,000 with 20x leverage
```
initial_margin = 2 * 65,000 / 20 = $6,500
notional = 2 * 65,000 = $130,000 (tier 0, < $2M)
maintenance_margin = 2 * 65,000 * 0.005 = $650
```

Available margin check: If equity = $10,000:
```
available = max(10,000 - 6,500, 0) = $3,500
max_additional_size = 3,500 * 20 / 65,000 = 1.077 BTC
```

**Inverse position (BTCUSD):**
- Buy 1000 contracts of BTCUSD at $65,000, contract_size = $1, 25x leverage
```
initial_margin_btc = (1000 * 1) / (65,000 * 25) = 1000 / 1,625,000 = 0.000615 BTC
notional_usd = 1000 * 1 = $1,000 (tier 0)
maintenance_margin_btc = (1000 * 1) / 65,000 * 0.005 = 0.015385 * 0.005 = 0.0000769 BTC
```

**Tiered MMR example:**
- Position: 50 BTC at $65,000 = $3,250,000 notional → tier 1 (MMR = 1.0%)
```
maintenance_margin = 50 * 65,000 * 0.01 = $32,500
```

Compare with tier 0 for a smaller position of 10 BTC:
```
notional = 10 * 65,000 = $650,000 → tier 0 (MMR = 0.5%)
maintenance_margin = 10 * 65,000 * 0.005 = $3,250
```

### C. Reference Implementation Comparison

| Exchange | Formula | Difference |
|----------|---------|------------|
| **Binance** (https://www.binance.com/en/support/faq/how-to-calculate-cost-required-to-open-a-position-in-perpetual-futures-contracts-87fa7ee33b574f7084f02f8edc63b185) | Linear: `initial = notional / leverage`. Tiered maintenance margin tables with 10+ tiers for BTC. | **Identical formula.** We have fewer tiers (4 vs 10+), which is a configuration choice, not a formula difference. |
| **Bybit** (https://www.bybit.com/en/help-center/article/Margin-Calculation-Bybit) | Identical formulas for linear and inverse. `initial_btc = contracts * contract_size / (price * leverage)`. | **Identical.** |
| **BitMEX** | Inverse: `margin_btc = contracts * contract_size / (price * leverage)`. | **Identical.** |

### D. Edge Cases & Boundary Behavior

- **Zero leverage:** `debug_assert!(!leverage.is_zero())`. Panics in debug mode; in release, would produce infinity which is caught by Decimal overflow.
- **Zero price (inverse):** Returns `Decimal::ZERO` for maintenance margin. The `debug_assert` fires in debug.
- **Very high leverage (125x):** Initial margin = 0.8% of notional. At tier-0 MMR of 0.5%, the buffer between initial and maintenance is only 0.3% of notional.
- **Position spanning tiers:** The `effective_mmr` function looks up the single tier matching the total notional. It does NOT blend rates across tiers (Binance does blend in some products).

### E. Test Coverage Reference

- **File:** `crates/risk-engine/src/margin_calculator.rs` (inline tests)
- **File:** `crates/api-gateway/tests/cross_collateral.rs`
- **Key tests:** `test_initial_margin_linear`, `test_initial_margin_inverse`, `test_maintenance_margin`, `test_tiered_mmr`, `test_available_margin`, `test_max_position_size`

### Our Implementation

- **Code:** `crates/risk-engine/src/margin_calculator.rs`
- Linear and inverse formulas as described above.
- Tiered MMR via `PerpetualConfig::effective_mmr(notional)`.
- `available >= 0` invariant enforced throughout.

### Industry Standard / Reference

| Source | Description |
|--------|-------------|
| CME Group (1988). SPAN. | The original scenario-based margining system. |
| Binance Futures. "Margin Calculation." | Linear: `initial = notional / leverage`. Tiered tables. |
| Bybit. "Margin." | Identical formulas for both contract types. |
| BitMEX. "Margin for Position (Inverse)." | `margin_btc = contracts * contract_size / (price * leverage)`. |

### Compliance

**Match.** Our formulas are identical to Binance/Bybit/BitMEX specifications.

---

## 9. Portfolio Margin

### Mechanism

Portfolio margin evaluates the worst-case loss across a grid of price shock
scenarios rather than computing margin per position in isolation.

### A. Mathematical Derivation

**Scenario grid:** 6 symmetric shocks: +/-5%, +/-10%, +/-15% from mark price.

For each shock `s` in {-0.15, -0.10, -0.05, +0.05, +0.10, +0.15}:

```
shocked_price = mark_price * (1 + s)
```

For each position under the shocked price, compute unrealized PnL:

```
pnl_linear = side_sign * size * (shocked_price - entry_price)
pnl_inverse = side_sign * size * contract_size * (1/entry_price - 1/shocked_price)
```

Aggregate across all positions:
```
scenario_pnl(s) = sum(pnl_i(s)) for all positions i
```

**Required margin:**
```
worst_pnl = min(scenario_pnl(s)) for all s  (most negative)
required_margin = abs(worst_pnl) * 1.2       (20% safety buffer)
```

**Delta netting:**

Net delta per base asset measures directional exposure:
```
delta_linear = side_sign * size
delta_inverse = side_sign * size * contract_size / mark_price^2
```

A portfolio with 1 BTC long linear and 1 BTC short inverse has near-zero net delta.

### B. Worked Numerical Example

**Portfolio:**
- Position A: 2 BTC BTCUSDT long, entry $64,000, mark $65,000
- Position B: 1 BTC BTCUSDT short, entry $66,000, mark $65,000

Net delta = +2 - 1 = +1 BTC (net long exposure).

**Scenario: -15% shock**
```
shocked_price = 65,000 * (1 - 0.15) = $55,250
pnl_A = +1 * 2 * (55,250 - 64,000) = 2 * (-8,750) = -$17,500
pnl_B = -1 * 1 * (55,250 - 66,000) = -1 * (-10,750) = +$10,750
portfolio_pnl = -17,500 + 10,750 = -$6,750
```

**Scenario: +15% shock**
```
shocked_price = 65,000 * 1.15 = $74,750
pnl_A = 2 * (74,750 - 64,000) = 2 * 10,750 = +$21,500
pnl_B = -1 * (74,750 - 66,000) = -$8,750
portfolio_pnl = 21,500 - 8,750 = +$12,750
```

**All scenarios:**
| Shock | Portfolio PnL |
|-------|--------------|
| -15% | -$6,750 |
| -10% | -$4,500 |
| -5% | -$2,250 |
| +5% | +$2,250 |
| +10% | +$4,500 |
| +15% | +$12,750 |

Worst PnL = -$6,750 (at -15%).

```
required_margin = abs(-6,750) * 1.2 = $8,100
```

Compare with isolated margin:
```
isolated_A = 2 * 65,000 / 10 = $13,000 (assuming 10x)
isolated_B = 1 * 65,000 / 10 = $6,500
total_isolated = $19,500
```

Portfolio margin saves $19,500 - $8,100 = $11,400 in capital.

### C. Reference Implementation Comparison

| Exchange | Approach | Difference |
|----------|---------|------------|
| **Binance** (https://www.binance.com/en/support/faq/introduction-to-binance-portfolio-margin-360041188052) | SPAN-like with 7 shock levels and volatility dimension. Includes inter-asset correlations for qualifying assets. | We have 6 shocks, no volatility dimension, no cross-asset correlations. Simpler but conservative. |
| **Bybit** (https://www.bybit.com/en/help-center/article/Unified-Trading-Account-Bybit) | Unified Trading Account: cross-asset netting with scenario-based stress testing. | Similar concept. Bybit includes options delta/gamma in the scenario. |
| **CME SPAN** | 16 risk scenarios including volatility shifts (+/- 1 vol). Inter-commodity spreads. | Our 6-scenario grid is a simplified version of SPAN's 16 scenarios. |

### D. Edge Cases & Boundary Behavior

- **No positions:** Returns zero required margin.
- **No mark prices available:** Positions without mark prices are skipped (contribute zero PnL).
- **Shocked price <= 0:** At -15% shock, if mark is very low, shocked price could go negative. The code checks `if shocked_price <= 0 { return acc; }` -- the position is skipped.
- **All scenarios positive:** If worst_pnl >= 0, required_margin = 0 (the portfolio profits in all scenarios).
- **Single position:** Portfolio margin degenerates to isolated-style margin with scenario shocks.

### E. Test Coverage Reference

- **File:** `crates/risk-engine/src/portfolio_margin.rs` (inline tests)
- **Key tests:** `test_single_long_position`, `test_hedged_portfolio_lower_margin`, `test_empty_portfolio`, `test_inverse_delta_netting`

### Our Implementation

- **Code:** `crates/risk-engine/src/portfolio_margin.rs`
- 6 symmetric shocks, 20% safety buffer, delta netting across linear and inverse.

### Industry Standard / Reference

| Source | Description |
|--------|-------------|
| CME Group. SPAN Margining System. | Original 16-scenario portfolio margin system. |
| Binance. "Portfolio Margin." | Scenario-based with 7 shock levels. |
| Bybit. "Unified Trading Account." | Cross-asset portfolio margin. |
| SEC Rule 15c3-1 (Net Capital Rule) | Regulatory framework for portfolio-based risk. |

### Compliance

**Match with simplification.** Our 6-scenario grid is a simplified SPAN. The 20% safety
buffer is conservative. Missing: volatility shocks and inter-asset correlations.

---

## 15. Black-Scholes Options Pricing

### Mechanism

The Black-Scholes model provides theoretical pricing for European-style options.

### A. Mathematical Derivation

**Input parameters:**
- `S` = spot (underlying) price
- `K` = strike price
- `T` = time to expiry (years)
- `r` = risk-free rate (annualized)
- `sigma` = implied volatility (annualized)

**d1 and d2:**
```
d1 = [ln(S/K) + (r + sigma^2/2) * T] / (sigma * sqrt(T))
d2 = d1 - sigma * sqrt(T)
```

**Option prices:**
```
Call = S * N(d1) - K * exp(-rT) * N(d2)
Put  = K * exp(-rT) * N(-d2) - S * N(-d1)
```

where `N(x)` is the standard normal CDF.

**Put-call parity verification:**
```
Call - Put = S * N(d1) - K*e^(-rT)*N(d2) - K*e^(-rT)*N(-d2) + S*N(-d1)
           = S * [N(d1) + N(-d1)] - K*e^(-rT) * [N(d2) + N(-d2)]
           = S * 1 - K*e^(-rT) * 1
           = S - K*e^(-rT)
```

This is the standard put-call parity identity.

**Greeks:**
```
delta_call = N(d1)
delta_put  = N(d1) - 1
gamma      = phi(d1) / (S * sigma * sqrt(T))
theta_call = -S*phi(d1)*sigma/(2*sqrt(T)) - r*K*e^(-rT)*N(d2)   (per year)
theta per day = theta_annual / 365
vega       = S * phi(d1) * sqrt(T) / 100  (per 1% IV move)
rho_call   = K * T * e^(-rT) * N(d2) / 100  (per 1% rate move)
```

where `phi(x) = e^(-x^2/2) / sqrt(2*pi)` is the standard normal PDF.

**Normal CDF approximation (Abramowitz & Stegun 26.2.17):**
```
P = 0.2316419
b1 = 0.319381530, b2 = -0.356563782, b3 = 1.781477937
b4 = -1.821255978, b5 = 1.330274429
t = 1 / (1 + P * |x|)
N(x) = 1 - phi(x) * (b1*t + b2*t^2 + b3*t^3 + b4*t^4 + b5*t^5)  for x >= 0
N(x) = 1 - N(-x)  for x < 0
```

Accuracy: +/- 7.5 * 10^-8.

### B. Worked Numerical Example

**BTC call option:**
- S = $65,000, K = $70,000, T = 30/365 = 0.08219, r = 5% = 0.05, sigma = 60% = 0.60

```
d1 = [ln(65000/70000) + (0.05 + 0.36/2) * 0.08219] / (0.60 * sqrt(0.08219))
   = [ln(0.92857) + 0.23 * 0.08219] / (0.60 * 0.28669)
   = [-0.07411 + 0.01890] / 0.17201
   = -0.05521 / 0.17201
   = -0.32099

d2 = -0.32099 - 0.17201 = -0.49300

N(d1) = N(-0.32099) = 0.3741
N(d2) = N(-0.49300) = 0.3110

Call = 65,000 * 0.3741 - 70,000 * exp(-0.05*0.08219) * 0.3110
     = 24,316.50 - 70,000 * 0.99589 * 0.3110
     = 24,316.50 - 21,680.62
     = $2,635.88

Greeks:
phi(d1) = e^(-0.32099^2/2) / sqrt(2*pi) = e^(-0.05152) / 2.50663 = 0.94977 / 2.50663 = 0.37886
delta = N(-0.32099) = 0.3741
gamma = 0.37886 / (65,000 * 0.60 * 0.28669) = 0.37886 / 11,180.91 = 0.00003389
vega  = 65,000 * 0.37886 * 0.28669 / 100 = $70.61 per 1% IV
theta = [-65,000 * 0.37886 * 0.60 / (2*0.28669) - 0.05*70,000*0.99589*0.3110] / 365
      = [-25,744.50 - 1,084.03] / 365 = -$73.50 per day
```

### C. Reference Implementation Comparison

| Exchange | Pricing Model | Difference |
|----------|--------------|------------|
| **Deribit** (https://www.deribit.com/knowledge-base/options-pricing) | Black-Scholes for European options, with mark price adjustments. Uses the same A&S CDF approximation. | **Identical** core model. Deribit adds IV adjustments for mark price. |
| **Binance Options** | Black-Scholes based. Mark IV from order book mid-IV with dampening. | Our pricing model is standard BS; IV sourcing differs (configurable input vs. order-book-derived). |

### D. Edge Cases & Boundary Behavior

- **T = 0 (at expiry):** Returns `None` from `inputs_f64()` (T must be positive). At expiry, option value = max(S-K, 0) for calls.
- **sigma = 0:** Returns `None`. Zero vol means the option is deterministic.
- **S = 0 or K = 0:** Returns `None`. Prices must be positive.
- **Deep ITM:** d1 and d2 are large positive; N(d1) ~ 1, N(d2) ~ 1. Call ~ S - K*e^(-rT).
- **Deep OTM:** d1 and d2 are large negative; N(d1) ~ 0, N(d2) ~ 0. Call ~ 0.
- **f64 precision:** Transcendental functions (exp, ln, sqrt) use f64 internally. Results are converted to Decimal at the boundary. NaN/Inf produce `Decimal::ZERO`.

### E. Test Coverage Reference

- **File:** `crates/risk-engine/src/options_pricing.rs` (inline tests)
- **Key tests:** `test_call_price`, `test_put_call_parity`, `test_greeks`, `test_atm_delta`, `test_expired_returns_none`
- **Test values:** S=$50,000, K=$50,000--55,000, T=30d--90d, sigma=50%--80%.

### Our Implementation

- **Code:** `crates/risk-engine/src/options_pricing.rs`
- Standard Black-Scholes with Abramowitz & Stegun CDF.
- Full Greeks: delta, gamma, theta (per day), vega (per 1% IV), rho (per 1% rate).
- All public APIs use `Decimal`; f64 internal only for transcendentals.

### Industry Standard / Reference

| Source | Description |
|--------|-------------|
| Black, F. & Scholes, M. (1973). "The Pricing of Options and Corporate Liabilities." | The original paper. |
| Abramowitz, M. & Stegun, I. (1964). *Handbook of Mathematical Functions*. Section 26.2.17. | CDF approximation. |
| Haug, E.G. (2007). *The Complete Guide to Option Pricing Formulas*. | Greeks conventions. |

### Compliance

**Match.** Standard Black-Scholes with industry-standard CDF approximation.

---

## 20. Collateral Haircuts

### Mechanism

Cross-collateral haircuts discount non-stablecoin assets used as margin.

### A. Mathematical Derivation

```
effective_collateral = balance * market_price_usd * haircut_factor
```

For a portfolio of N assets:
```
total_effective_collateral = sum(balance_i * price_usd_i * haircut_i)
```

### B. Worked Numerical Example

User holds:
| Asset | Balance | Price | Haircut | Effective |
|-------|---------|-------|---------|-----------|
| USDT | 10,000 | $1.00 | 1.00 | $10,000 |
| BTC | 0.5 | $65,000 | 0.95 | $30,875 |
| ETH | 5.0 | $3,500 | 0.90 | $15,750 |
| SOL | 100 | $150 | 0.85 | $12,750 |
| EXC | 1,000,000 | $0.10 | 0.00 | $0 |

Total nominal: $10,000 + $32,500 + $17,500 + $15,000 + $100,000 = $175,000
Total effective: $10,000 + $30,875 + $15,750 + $12,750 + $0 = $69,375

The EXC token (exchange's own token) has zero collateral value. A user with
$100,000 worth of EXC cannot use it for margin. This is the anti-FTT policy.

### C. Reference Implementation Comparison

| Exchange | BTC Haircut | ETH Haircut | Own Token |
|----------|------------|------------|-----------|
| **Binance** (https://www.binance.com/en/support/faq/binance-cross-collateral-introduction-360041505071) | 95% (5% haircut) | 90% (10% haircut) | BNB: 70-80% |
| **Bybit** (https://www.bybit.com/en/help-center/article/Unified-Trading-Account-Collateral) | ~95% BTC | ~90% ETH | -- |
| **Our implementation** | 95% BTC | 90% ETH | EXC: 0% (anti-FTT) |

Our EXC haircut of 100% is more conservative than Binance's BNB treatment (70-80%). This is a deliberate post-FTX design decision.

### D. Edge Cases & Boundary Behavior

- **Unknown asset:** Defaults to `other_alts` category (80% = 20% haircut).
- **EXC token:** Always zero. Tests verify this invariant.
- **Zero balance:** Contributes zero effective collateral.

### E. Test Coverage Reference

- **File:** `crates/account/src/unified_account.rs` (inline tests)
- **File:** `crates/api-gateway/tests/cross_collateral.rs`

### Our Implementation

| Asset Class | Haircut Factor |
|-------------|---------------|
| Stablecoins (USDT, USDC, BUSD, DAI, FDUSD) | 1.00 |
| BTC | 0.95 |
| ETH | 0.90 |
| Major alts (SOL, BNB, XRP, ADA, AVAX, DOT) | 0.85 |
| Other alts | 0.80 |
| EXC (exchange token) | 0.00 |

### Industry Standard / Reference

| Source | Description |
|--------|-------------|
| Basel III (2010). "Minimum haircut floors." | Regulatory framework for collateral haircuts. |
| ISDA (2016). Standard CSA. | OTC derivatives collateral haircut schedules. |
| Binance. "Cross-Collateral." | BTC 95%, ETH 90%. |

### Compliance

**Match.** Anti-FTT policy enforced. Haircut schedule consistent with industry.

---

## 21. Circuit Breaker

### Mechanism

Price bands around a reference price halt trading when breached repeatedly.

### A. Mathematical Derivation

```
upper_band = reference_price * (1 + max_move_pct)
lower_band = reference_price * (1 - max_move_pct)
```

An order is rejected if:
```
order_price > upper_band  OR  order_price < lower_band
```

**Halt trigger:**
```
if trigger_count_in_60s >= max_triggers_before_halt:
    halt for halt_duration_secs
```

### B. Worked Numerical Example

- Reference: $65,000, max_move_pct = 5%
- Upper band: $65,000 * 1.05 = $68,250
- Lower band: $65,000 * 0.95 = $61,750

Order at $69,000: REJECTED (above $68,250). trigger_count++.
Order at $61,000: REJECTED (below $61,750). trigger_count++.

If 3 rejections in 60 seconds (max_triggers_before_halt = 3), trading halts for 30 seconds.

### C. Reference Implementation Comparison

Follows SEC LULD (Limit Up-Limit Down) model with configurable parameters. Similar to CME Velocity Logic.

### D. Edge Cases & Boundary Behavior

- **No reference price:** All orders allowed (bootstrapping).
- **Market orders:** Checked against best bid/ask, not order price.
- **Halt expiry:** Auto-resumes after `halt_duration_secs`.

### E. Test Coverage Reference

- **File:** `crates/matching-engine/src/circuit_breaker.rs` (inline tests)

### Compliance

**Match.** LULD-style with halt escalation.

---

## 22. Socialized Loss Distribution

### Mechanism

When the insurance fund is exhausted, losses are distributed proportionally
across all profitable positions.

### A. Mathematical Derivation

```
clawback_i = shortfall * (profit_i / total_profit)
```

where:
- `shortfall` = deficit not covered by insurance fund
- `profit_i` = unrealized/realized profit of user i (only positive)
- `total_profit` = sum of all positive profits

Verification: `sum(clawback_i) = shortfall * sum(profit_i) / total_profit = shortfall`. Correct.

### B. Worked Numerical Example

Shortfall: $5,000
| User | Profit | Share | Clawback |
|------|--------|-------|----------|
| Alice | $3,000 | 30% | $1,500 |
| Bob | $7,000 | 70% | $3,500 |
| Carol | -$2,000 | 0% | $0 |

Total clawback: $1,500 + $3,500 = $5,000. Matches shortfall.

### C. Reference Implementation Comparison

Identical to BitMEX/OKX socialized loss model. Standard pro-rata by profit share.

### D. Edge Cases & Boundary Behavior

- **No profitable positions:** Returns `None`. Shortfall cannot be recovered.
- **Rounding:** Amounts rounded to 8 decimal places.

### E. Test Coverage Reference

- **File:** `crates/settlement/src/insurance_fund.rs` (inline tests, `SocializedLoss::distribute`)

### Compliance

**Match.** Standard pro-rata by profit.
