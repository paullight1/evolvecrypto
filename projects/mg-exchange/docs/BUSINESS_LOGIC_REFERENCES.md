# Business Logic Reference Document

## Purpose

This document traces every material business logic mechanism in the exchange to an
industry standard, academic paper, or documented exchange practice. It is intended
for regulators, auditors, and investors who need assurance that the platform's
behaviour is not ad hoc but grounded in established financial engineering.

Every formula is presented with its full mathematical derivation, a worked numerical
example verifiable by hand, a comparison with Binance and Bybit implementations,
edge case analysis, and a reference to the test file that exercises it.

**Last updated:** 2026-03-22

---

## Table of Contents

1. [Order Matching -- Price-Time Priority (FIFO)](#1-order-matching--price-time-priority-fifo)
2. [Mark Price / Fair Price Marking](#2-mark-price--fair-price-marking)
3. [Funding Rate Mechanism](#3-funding-rate-mechanism)
4. [Liquidation Engine](#4-liquidation-engine)
5. [Auto-Deleveraging (ADL)](#5-auto-deleveraging-adl)
6. [Insurance Fund & Default Waterfall](#6-insurance-fund--default-waterfall)
7. [Index Price -- Weighted Median](#7-index-price--weighted-median)
8. [Margin Calculation (Linear & Inverse)](#8-margin-calculation-linear--inverse)
9. [Portfolio Margin](#9-portfolio-margin)
10. [Perpetual Swap Mechanics](#10-perpetual-swap-mechanics)
11. [TWAP Execution](#11-twap-time-weighted-average-price-execution)
12. [VWAP Execution](#12-vwap-volume-weighted-average-price-execution)
13. [Grid Trading](#13-grid-trading)
14. [DCA (Dollar-Cost Averaging)](#14-dca-dollar-cost-averaging)
15. [Black-Scholes Options Pricing](#15-black-scholes-options-pricing)
16. [Self-Trade Prevention (STP)](#16-self-trade-prevention-stp)
17. [Proof of Reserves -- Merkle Tree](#17-proof-of-reserves--merkle-tree)
18. [KYC/AML Risk Scoring](#18-kycaml-risk-scoring)
19. [Travel Rule](#19-travel-rule)
20. [Collateral Haircuts](#20-collateral-haircuts)
21. [Circuit Breaker](#21-circuit-breaker)
22. [Socialized Loss Distribution](#22-socialized-loss-distribution)
23. [PnL Calculation](#23-pnl-calculation)
24. [Fee Engine & VIP Tiers](#24-fee-engine--vip-tiers)
25. [Order Types -- Complete Specification](#25-order-types--complete-specification)
26. [Copy Trading Economics](#26-copy-trading-economics)
27. [Earn Products -- Yield Mechanics](#27-earn-products--yield-mechanics)
28. [Surveillance -- Detection Algorithms](#28-surveillance--detection-algorithms)
29. [Custody -- Wallet Architecture](#29-custody--wallet-architecture)
30. [WAL (Write-Ahead Log) -- Crash Recovery](#30-wal-write-ahead-log--crash-recovery)

---

## 1. Order Matching -- Price-Time Priority (FIFO)

### Mechanism

Price-time priority matching: orders are first ranked by price (best price
first), then by arrival time within the same price level (earliest first).
The taker always fills at the maker's resting price, providing price
improvement to the aggressor.

### A. Mathematical Derivation

The matching algorithm is a deterministic selection function. Given an incoming order
`O_taker` with side `S`, price `P_t`, and quantity `Q_t`:

1. **Opposite-side selection:** Select the opposite book. If `S = Buy`, select asks;
   if `S = Sell`, select bids.

2. **Price eligibility:** For a buy taker, a resting ask at price `P_m` is eligible
   if `P_m <= P_t` (for limit orders) or unconditionally (for market orders). For a
   sell taker, a resting bid at price `P_m` is eligible if `P_m >= P_t`.

3. **Price priority:** Among eligible levels, the best price is selected first:
   - Asks: `min(P_m)` (lowest ask)
   - Bids: `max(P_m)` (highest bid)

4. **Time priority:** Within a price level, orders are matched in FIFO order. The
   data structure is a queue (VecDeque) per price level.

5. **Fill price:** Always the maker's resting price `P_m`, regardless of the taker's
   limit price. If taker bids $65,100 and maker asks $65,000, fill is at $65,000.
   The taker receives $100 price improvement.

6. **Quantity resolution:**
   - If `Q_maker >= Q_remaining`: partial fill of maker, `Q_remaining` goes to zero.
   - If `Q_maker < Q_remaining`: full fill of maker, `Q_remaining -= Q_maker`, advance to next maker.

7. **Remainder handling:**
   - `GTC`: rest unfilled quantity on the book.
   - `IOC`: cancel unfilled remainder immediately.
   - `FOK`: if full quantity cannot fill in one pass, reject the entire order (no partial).
   - `PostOnly`: reject if any immediate match would occur.
   - `GTD`: rest on book with expiry timestamp; background task removes after deadline.
   - `Market`: cancel unfilled remainder (never rests on book).

### B. Worked Numerical Example

Order book for BTCUSDT before matching:

```
ASKS:                          BIDS:
$65,200  x 0.5 BTC (Alice)    $64,800  x 1.0 BTC (Dave)
$65,100  x 0.3 BTC (Bob)      $64,700  x 0.5 BTC (Eve)
$65,000  x 0.2 BTC (Carol)    $64,600  x 2.0 BTC (Frank)
```

Incoming: Taker buys 0.4 BTC at market.

**Step 1:** Best ask = $65,000 (Carol, 0.2 BTC). Fill 0.2 BTC at $65,000. Remaining = 0.2 BTC.
**Step 2:** Next best ask = $65,100 (Bob, 0.3 BTC). Fill 0.2 BTC at $65,100. Remaining = 0.0 BTC.

Result: Taker gets 0.2 BTC at $65,000 + 0.2 BTC at $65,100 = average price $65,050.
Carol is fully filled. Bob has 0.1 BTC remaining on the book at $65,100.

If the taker had placed a limit buy at $65,050, Carol would fill (65,000 <= 65,050)
but Bob would NOT fill (65,100 > 65,050). Remaining 0.2 BTC rests as a new bid at $65,050.

### C. Reference Implementation Comparison

| Exchange | Implementation | Difference from ours |
|----------|---------------|---------------------|
| **Binance** (https://www.binance.com/en/support/faq/what-is-a-limit-order-and-how-does-it-work-23c3d1c0daff42318ca01fac981a2b05) | Price-time FIFO. Market order unfilled remainder is cancelled. Supports GTC, IOC, FOK, PostOnly, GTD. | **No difference.** Our implementation matches exactly. |
| **Bybit** (https://www.bybit.com/en/help-center/article/Order-Types-Bybit) | Price-time FIFO. Market orders fill at best available, remainder cancelled. Same TIF modes. | **No difference.** |
| **CME Group** (Rulebook Chapter 5) | Price-time FIFO for most products. Some products use pro-rata allocation. | We implement FIFO only. Pro-rata is not applicable for crypto spot/perps. |

### D. Edge Cases & Boundary Behavior

- **Empty book:** Market order against an empty book produces zero fills and is cancelled (no panic, no hang).
- **Self-trade:** Handled upstream by STP module before reaching the matching engine.
- **Zero-quantity order:** Rejected at the order gateway validator level.
- **Tick size:** Price levels are `Decimal` (arbitrary precision). Tick size enforcement is in the order gateway, not the engine.
- **Maximum book depth:** No hard limit. The BTreeMap grows as needed. In practice, circuit breakers and risk checks prevent pathological growth.
- **Iceberg orders:** Displayed quantity matched first; hidden quantity replenishes from the iceberg reserve after the displayed portion fills. Replenished quantity goes to the back of the time queue at that price level.

### E. Test Coverage Reference

- **File:** `crates/matching-engine/src/orderbook.rs` (inline `#[cfg(test)]` module)
- **File:** `crates/api-gateway/tests/trading_scenarios.rs`
- **Key tests:** `test_basic_limit_buy`, `test_market_order_partial_fill`, `test_post_only_rejection`, `test_fok_reject_insufficient_liquidity`, `test_ioc_cancel_remainder`
- **Test values:** BTC prices around $50,000, quantities 0.001--10 BTC.

### Industry Standard / Reference

| Source | Description |
|--------|-------------|
| Harris, L. (2003). *Trading and Exchanges: Market Microstructure for Practitioners*. Oxford University Press. | Definitive textbook on price-time priority. Chapter 9: "Order-Driven Markets." |
| NYSE Rule 72 (Display, Ranking, and Allocation of Orders) | NYSE's formal rule codifying price-time priority for displayed orders. |
| CME Group Rulebook, Chapter 5 (Order Types and Execution Procedures) | CME's specification of price-time FIFO matching for futures. |
| NASDAQ Rule 4757 (Book Processing) | NASDAQ's price-time priority allocation algorithm. |
| London Stock Exchange, "Millennium Exchange Trading," Section 5 | LSE price-time priority specification. |

### Compliance

**Match.** Our implementation follows the standard price-time FIFO algorithm
used by NYSE, NASDAQ, CME, ICE, and LSE. Taker fills at maker's price is the
universal convention. Market order cancellation of unfilled remainder is
consistent with CME/ICE behaviour for futures markets.

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

## 3. Funding Rate Mechanism

### Mechanism

Funding is a periodic payment between long and short perpetual position holders
that anchors the perpetual price to the spot index. Positive rate means longs
pay shorts; negative means shorts pay longs.

### A. Mathematical Derivation

**Rate calculation:**

```
raw_rate = (perp_price - index_price) / index_price
rate = clamp(raw_rate, -max_rate, +max_rate)
```

This is the premium/discount of the perpetual relative to spot, expressed as a fraction.
The clamp prevents extreme rates during flash crashes or squeezes.

**Settlement (linear contracts, USDT-margined):**

```
payment = position_size * mark_price * funding_rate
```

For a long position: the user PAYS when rate > 0 (amount is negative), RECEIVES when rate < 0.
For a short position: the user RECEIVES when rate > 0, PAYS when rate < 0.

The net transfer across all positions sums to zero:

```
sum(payments_all_longs) + sum(payments_all_shorts) = 0
```

Proof: For each symbol, `sum_longs * mark * rate + sum_shorts * mark * (-rate)` where long
amounts are negated. Since longs pay and shorts receive (or vice versa), the algebraic sum is:
`-total_long_size * mark * rate + total_short_size * mark * rate = mark * rate * (short_size - long_size)`.
In practice, the net is zero because we compute per-position and the signs cancel.

**Settlement (inverse contracts, coin-margined):**

```
payment = position_size * contract_size / mark_price * funding_rate
```

The payment is in the base asset (e.g., BTC for BTCUSD).

### B. Worked Numerical Example

**Linear BTCUSDT funding:**
- Perp price: $65,300
- Index price: $65,000
- Max rate: 0.75% (0.0075)
- Position: 2 BTC long at mark price $65,100

```
raw_rate = (65,300 - 65,000) / 65,000 = 300 / 65,000 = 0.004615 (0.4615%)
rate = clamp(0.004615, -0.0075, +0.0075) = 0.004615 (within bounds)
```

Payment for the 2 BTC long:
```
payment = 2 * 65,100 * 0.004615 = 130,200 * 0.004615 = $600.87
```

The long pays $600.87 to the shorts.

**Inverse BTCUSD funding:**
- Same rate: 0.004615
- Position: 100 contracts, contract_size = $1, mark price = $65,100

```
payment = 100 * 1 / 65,100 * 0.004615 = 100/65,100 * 0.004615 = 0.001536 * 0.004615 = 0.00000709 BTC
```

The long pays ~0.00000709 BTC.

**Negative rate scenario:**
- Perp: $64,700, Index: $65,000
```
raw_rate = (64,700 - 65,000) / 65,000 = -300/65,000 = -0.004615
```
Now shorts pay longs: the 2 BTC long above would receive $600.87.

### C. Reference Implementation Comparison

| Exchange | Formula | Difference |
|----------|---------|------------|
| **Binance** (https://www.binance.com/en/support/faq/introduction-to-binance-futures-funding-rates-360033525031) | `rate = average_premium_index + clamp(interest_rate - average_premium_index, -0.05%, +0.05%)`. Uses 8h TWAP of premium. Default interest rate: 0.01% per 8h (3bps/day). Settlement: `payment = position_value * funding_rate`. | We use instantaneous premium, not TWAP. We drop the interest rate component (negligible in crypto). |
| **Bybit** (https://www.bybit.com/en/help-center/article/Funding-Rate-Bybit) | Similar to BitMEX: premium index + clamped interest rate. 8h TWAP averaging window. | Same as Binance difference. |
| **dYdX v3** | Premium-only: `rate = (perp - index) / index / 8`. No interest component. | Our formula is closest to dYdX's premium-only model. The difference: dYdX divides by 8 (hours in period); we do not, because our settlement already accounts for interval length in the mark price. |

### D. Edge Cases & Boundary Behavior

- **Zero index price:** Returns `Err(SettlementError::ZeroIndexPrice)`. Division by zero is guarded.
- **Zero mark price (inverse):** Settlement returns `Decimal::ZERO` for inverse payments when mark is zero.
- **Rate at clamp boundary:** When `raw_rate > max_rate`, rate is clamped to `max_rate`. With default max of 0.75%, a 5% premium only charges 0.75%.
- **Zero rate:** When perp == index, rate = 0. All payments are zero but still recorded for audit trail.
- **Single position in market:** If only longs exist and no shorts, the funding is still computed per-position. Operationally, funding creates an incentive for shorts to enter.
- **Very large positions:** No overflow risk due to `rust_decimal` 96-bit mantissa (max ~7.9 * 10^28).

### E. Test Coverage Reference

- **File:** `crates/settlement/src/funding.rs` (inline tests)
- **File:** `crates/api-gateway/tests/trading_scenarios.rs` (`scenario_funding_settlement`)
- **Key tests:** `test_positive_rate_long_pays`, `test_negative_rate_short_pays`, `test_zero_rate`, `test_zero_index_error`, `test_inverse_funding`
- **Test values:** Prices ~$50,000, rates 0.01%, positions 1--10 BTC.

### Our Implementation

- **Code:** `crates/settlement/src/funding.rs`
- **Rate formula:** `rate = clamp((perp_price - index_price) / index_price, -max_rate, +max_rate)`
- **Settlement formula (linear):** `payment = position_size * mark_price * funding_rate`
- **Settlement formula (inverse):** `payment = position_size * contract_size / mark_price * funding_rate`
- Direction: long pays when rate > 0, short pays when rate < 0.
- Net transfer across all positions sums to zero (zero-sum between longs and shorts).
- Settlement interval: configurable, default 8 hours (3x daily).

### Industry Standard / Reference

| Source | Description |
|--------|-------------|
| Hayes, A. / BitMEX (2016). "Perpetual Contracts." | Original invention of the perpetual swap funding mechanism. Full formula: `rate = premium_index + clamp(interest_rate - premium_index, -0.05%, +0.05%)`. |
| Binance Futures. "Funding Rate." | `rate = average_premium_index + clamp(interest_rate - average_premium_index, ..., ...)` with 8h TWAP premium. |
| dYdX v3. "Funding Rate." | Premium-only model: `rate = (perp_price - index_price) / index_price / 8`. |
| Bybit. "Funding Rate." | Similar to BitMEX with minor formula differences. |

### Compliance

**Match (simplified variant).** Our rate formula matches the premium-only model
used by dYdX and many newer exchanges that drop the interest rate component
(which is negligible in crypto). The clamping mechanism matches all major
exchanges. The settlement formulas for both linear and inverse contracts are
standard.

**Noted simplification:** We use instantaneous premium rather than 8h TWAP of
the premium index. Production exchanges typically use a time-weighted average to
reduce manipulation risk.

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

## 6. Insurance Fund & Default Waterfall

### Mechanism

The insurance fund absorbs the difference between the bankruptcy price and the
actual liquidation fill price. When the fund is depleted, losses are socialised.

### A. Mathematical Derivation

**Surplus (fill better than bankruptcy):**

For a long position liquidated:
```
surplus = (fill_price - bankruptcy_price) * qty   if fill_price > bankruptcy_price
```

For a short position liquidated:
```
surplus = (bankruptcy_price - fill_price) * qty   if fill_price < bankruptcy_price
```

The surplus is positive leftover margin that the user doesn't need. It goes to the fund.

**Deficit (fill worse than bankruptcy):**

```
deficit = abs(fill_price - bankruptcy_price) * qty   (opposite direction)
```

The fund covers this. If `deficit > fund.balance`, the remainder is socialized.

**Waterfall:**
```
1. Trader's margin (consumed at liquidation)
2. Insurance fund surplus/deficit netting
3. Socialized loss (pro-rata across profitable positions)
4. ADL (force-close opposing profitable positions)
```

### B. Worked Numerical Example

**Scenario: Surplus**
- Long liquidated at bankruptcy_price = $58,500, fill_price = $58,700
- Quantity: 1 BTC
```
surplus = (58,700 - 58,500) * 1 = $200
fund_balance_after = fund_balance_before + $200
```

**Scenario: Deficit, fund covers**
- Long liquidated at bankruptcy_price = $58,500, fill_price = $58,300
- Quantity: 1 BTC
- Fund balance: $10,000
```
deficit = (58,500 - 58,300) * 1 = $200
fund_balance_after = 10,000 - 200 = $9,800
```

**Scenario: Deficit, fund insufficient**
- Deficit: $15,000
- Fund balance: $10,000
- Fund pays $10,000, shortfall = $5,000

Socialization: Profitable positions in this market:
| User | Profit |
|------|--------|
| Alice | $3,000 |
| Bob | $7,000 |
| Carol | -$1,000 (excluded) |

Total profit pool = $10,000. Distribution:
```
Alice clawback = 5,000 * (3,000/10,000) = $1,500
Bob clawback = 5,000 * (7,000/10,000) = $3,500
Total = $5,000 (matches shortfall)
```

### C. Reference Implementation Comparison

| Exchange | Waterfall | Difference |
|----------|----------|------------|
| **Binance** (SAFU fund) | Separate insurance fund + SAFU emergency fund. Two layers before socialization. | We have one fund. Conceptually the same; Binance just has an additional emergency reserve. |
| **BitMEX** | Insurance fund only. Surplus from profitable liquidations seeds the fund. ADL before socialization. | Our waterfall is identical: surplus → fund, deficit → fund → socialized loss / ADL. |

### D. Edge Cases & Boundary Behavior

- **Negative surplus amount:** Returns `Err(SettlementError::NegativeAmount)`. Guard prevents fund corruption.
- **Negative deficit amount:** Same error guard.
- **Fund balance exactly zero:** Any deficit triggers socialization immediately.
- **No profitable positions for socialization:** Returns `None`. The shortfall is tracked but cannot be recovered.
- **Rounding:** Clawback amounts are rounded to 8 decimal places to prevent dust accumulation.

### E. Test Coverage Reference

- **File:** `crates/settlement/src/insurance_fund.rs` (inline tests)
- **Key tests:** `test_add_surplus`, `test_cover_deficit_success`, `test_cover_deficit_insufficient`, `test_socialize_distribution`
- **Test values:** Fund balances $10,000--$100,000, deficits $100--$50,000.

### Our Implementation

- **Code:** `crates/settlement/src/insurance_fund.rs` and `crates/risk-engine/src/liquidation.rs`
- **Waterfall:** Trader margin -> insurance fund -> socialized loss / ADL
- **Surplus:** Fill better than bankruptcy → excess goes to fund.
- **Deficit:** Fund covers shortfall; if insufficient, tracked as `socialised_loss`.

### Industry Standard / Reference

| Source | Description |
|--------|-------------|
| CME Rulebook Chapter 8 (Clearing House) | Default waterfall: defaulter's margin, guaranty fund, clearing house surplus. |
| BitMEX. "Insurance Fund." | Perpetual-specific insurance fund: absorbs liquidation shortfalls. |
| Binance SAFU (Secure Asset Fund for Users) | Emergency insurance fund for user protection. |
| Hull, J. (2018). *Options, Futures, and Other Derivatives*, 10th ed. | Chapter 2: clearing house default management. |

### Compliance

**Match.** Standard waterfall: trader margin -> insurance fund -> socialised loss / ADL.

---

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

## 10. Perpetual Swap Mechanics

### Mechanism

A perpetual swap is a derivatives contract with no expiry date. The funding
rate mechanism anchors the contract price to the underlying spot index.

### A. Mathematical Derivation

A perpetual swap combines three mechanisms:

1. **No expiry:** The position persists indefinitely. No settlement date, no roll cost.
2. **Funding rate anchoring:** Periodic payments create an economic incentive for the
   perpetual price to track the spot index. If perp > spot, longs pay shorts, making it
   expensive to maintain long positions, pushing the price down toward spot.
3. **Cash settlement:** PnL is settled in USDT (linear) or BTC (inverse). No physical delivery.

The equilibrium condition: In expectation, the perpetual price equals the spot index.
If `P_perp > P_index`, the positive funding rate creates a cost of carry for longs,
equivalent to a negative yield, driving the price toward convergence.

### B. Worked Numerical Example

**Day 1:** BTC spot = $65,000. Perp trades at $65,150 (premium).
- Funding rate = (65,150 - 65,000) / 65,000 = 0.23%
- 10 BTC long pays: 10 * 65,100 * 0.0023 = $1,497.30 to shorts (every 8h)
- Annualized cost of carry: 0.23% * 3 * 365 = 251.85% (extreme!)
- This extreme cost incentivizes longs to close or sell, pushing perp price down.

**Day 2:** Arbitrageurs short the perp and buy spot, earning the funding.
- Perp price converges to $65,010 (nearly at spot).
- Funding rate = 0.015%, much more sustainable.

This is the funding rate mechanism working as designed: it creates a self-correcting
feedback loop between the perpetual and spot markets.

### C. Reference Implementation Comparison

Identical to BitMEX's original 2016 design, adopted universally by Binance, Bybit, OKX, dYdX, and all major perpetual futures platforms.

### D. Edge Cases & Boundary Behavior

- **Both linear and inverse:** We support BTCUSDT (linear, USDT-settled) and BTCUSD (inverse, BTC-settled).
- **Extreme premium/discount:** Clamped by `max_rate` in funding calculation.
- **Zero funding:** When perp == index exactly, no payments flow. Positions are still valid.

### E. Test Coverage Reference

- **File:** `crates/api-gateway/tests/trading_scenarios.rs` (multiple perp trading scenarios)
- **File:** `crates/settlement/src/funding.rs` (funding tests)

### Industry Standard / Reference

| Source | Description |
|--------|-------------|
| Hayes, A. / BitMEX (2016). "A Perpetual Swap." | The original perpetual swap paper. |
| Shiller, R. (1993). *Macro Markets*. | Theoretical foundation for perpetual futures. |

### Compliance

**Match.** All three pillars present: no expiry, funding-rate anchoring, cash settlement.

---

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

## 16. Self-Trade Prevention (STP)

### Mechanism

STP detects and blocks orders from the same user that would trade against each other.

### A. Mathematical Derivation

No formula. Pure rule-based logic:

```
if incoming_order.user_id == resting_order.user_id:
    match mode:
        CancelTaker: reject incoming order
        CancelMaker: cancel resting order(s), incoming proceeds
        Allow: no action (testing only)
```

### B. Worked Numerical Example

User Alice has a resting sell at $65,000. Alice submits a buy at $65,100.

- **CancelTaker mode:** Buy is rejected. Sell remains. No trade.
- **CancelMaker mode:** Sell is cancelled. Buy proceeds to match other resting orders.
- **Allow mode:** Trade executes (Alice trades with herself -- wash trade).

### C. Reference Implementation Comparison

| Exchange | Modes | Difference |
|----------|-------|------------|
| **Binance** | EXPIRE_TAKER, EXPIRE_MAKER, EXPIRE_BOTH | We lack CancelBoth (minor gap). |
| **CME Globex** | Cancel Newest (=CancelTaker), Cancel Oldest (=CancelMaker), Cancel Both | Same gap. |

### D. Edge Cases & Boundary Behavior

- **Multiple resting orders:** In CancelMaker mode, `conflicting_orders()` returns ALL resting orders on the opposite side for the same user. All are cancelled.
- **Allow mode:** Should only be used in development/testing.

### E. Test Coverage Reference

- **File:** `crates/order-gateway/src/self_trade.rs` (inline tests)

### Compliance

**Match** (minor gap: missing CancelBoth mode).

---

## 17. Proof of Reserves -- Merkle Tree

### Mechanism

Cryptographic proof that the exchange holds sufficient assets to cover all user liabilities.

### A. Mathematical Derivation

**Leaf hash:**
```
leaf = SHA256(user_id || ":" || asset || ":" || balance)
```

**Internal node hash (canonical ordering):**
```
node = SHA256(min(left, right) || max(left, right))
```

Canonical ordering makes the tree deterministic regardless of insertion order.

**Proof verification:** Given a leaf hash `h`, proof path `[(sibling_hash, position)]`:

```
current = h
for (sibling, position) in proof:
    if position == Left:
        current = SHA256(min(sibling, current) || max(sibling, current))
    else:
        current = SHA256(min(current, sibling) || max(current, sibling))
assert(current == root)
```

### B. Worked Numerical Example

Three users:
```
leaf_A = SHA256("alice:BTC:1.5") = 0xab12...
leaf_B = SHA256("bob:BTC:0.3")   = 0xcd34...
leaf_C = SHA256("carol:BTC:2.0") = 0xef56...
```

Tree (padding leaf_C with itself since 3 is odd):
```
           root = SHA256(min(h_AB, h_CC) || max(h_AB, h_CC))
          /                                           \
h_AB = SHA256(min(leaf_A, leaf_B) || max(...))     h_CC = SHA256(min(leaf_C, leaf_C) || ...)
    /       \                                        |
 leaf_A   leaf_B                                   leaf_C
```

Alice's inclusion proof: `[(leaf_B, Right), (h_CC, Right)]`.
Verification: `hash(leaf_A, leaf_B) -> h_AB`, then `hash(h_AB, h_CC) -> root`. Match.

### C. Reference Implementation Comparison

Identical to the Maxwell/Dagher approach used by Kraken and OKX. Our canonical ordering matches Chainlink's implementation.

### D. Edge Cases & Boundary Behavior

- **Single user:** Tree is just the leaf. Proof is empty.
- **Odd number of leaves:** Last leaf is paired with itself.
- **Zero balance:** Valid leaf; included in the tree.

### E. Test Coverage Reference

- **File:** `crates/proof-of-reserves/src/merkle.rs` (inline tests)

### Compliance

**Match.** Standard Merkle tree proof of reserves.

---

## 18. KYC/AML Risk Scoring

### Mechanism

Customer risk scoring evaluates each user against multiple risk factors.

### A. Mathematical Derivation

**Additive risk score:**
```
score = sum(factor_i * indicator_i)
```

where `indicator_i` is 1 if the factor is present, 0 otherwise, and `factor_i` is the weight.

| Factor | Weight |
|--------|--------|
| PEP match | 40 |
| Sanctions near-match | 50 |
| High-risk jurisdiction | 30 |
| Unusual transaction pattern | 25 |
| Frequent cash-outs | 20 |
| Dormant account reactivation | 20 |
| Large transactions | 15 |
| New account | 10 |

**Risk classification:**
```
Low:      0 -- 20
Medium:  21 -- 50
High:    51 -- 80
Critical: 81 -- 100
```

**Maximum possible score:** 40+50+30+25+20+20+15+10 = 210, but capped at 100.

### B. Worked Numerical Example

**User profile:** New account (10), high-risk jurisdiction (30), large transactions (15).
```
score = 10 + 30 + 15 = 55 → High risk
```

Review frequency: quarterly (High) vs. annually (Low).

**Another user:** PEP match (40), sanctions near-match (50).
```
score = 40 + 50 = 90 → Critical risk
```

Action: Enhanced Due Diligence (EDD), potential SAR filing, senior management review.

### C. Reference Implementation Comparison

Follows FATF Recommendation 1 risk-based approach. Standard across compliant exchanges (Binance, Coinbase, Kraken all use weighted factor models; exact weights are proprietary).

### D. Edge Cases & Boundary Behavior

- **No factors present:** Score = 0 → Low risk.
- **All factors present:** Score capped at 100.
- **Boundary values:** Score = 20 → Low, Score = 21 → Medium.

### E. Test Coverage Reference

- **File:** `crates/compliance/src/risk_scoring.rs` (inline tests)

### Compliance

**Match.** FATF-compliant risk-based approach with standard factors.

---

## 19. Travel Rule

### Mechanism

VASPs must exchange originator/beneficiary information for crypto transfers above threshold.

### A. Mathematical Derivation

No formula. Rule-based: if `transfer_amount >= threshold` (default $1,000 FATF / EUR 0 EU TFR), the data package must be exchanged.

### B. Worked Numerical Example

User withdraws 0.5 BTC ($32,500) to external address.
- Above FATF threshold ($1,000) → Travel Rule applies.
- Data package: originator name, account, VASP; beneficiary name, account, VASP.
- Status lifecycle: Pending → Sent → Verified → Funds released.

### C. Reference Implementation Comparison

| Standard | Threshold | Our Implementation |
|----------|----------|-------------------|
| FATF R.16 | $1,000 (or equivalent) | Configurable threshold |
| EU TFR 2023/1113 | EUR 0 (all transfers) | Can be configured to zero |
| FinCEN (US) | $3,000 | Configurable |

### D. Edge Cases & Boundary Behavior

- **Self-hosted wallet:** No counterparty VASP. Data requirements differ by jurisdiction.
- **Unknown VASP:** Transfer held pending manual review.

### E. Test Coverage Reference

- **File:** `crates/compliance/src/travel_rule.rs` (inline tests)

### Compliance

**Match (Phase 1).** Data model captures all FATF R.16 fields. Phase 2 will add TRISA/OpenVASP protocol integration.

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

---

## 23. PnL Calculation

### Mechanism

Realized PnL calculation for closed or partially closed positions.

### A. Mathematical Derivation

**Realized PnL:**
```
Long:  pnl = (exit_price - entry_price) * qty_closed
Short: pnl = (entry_price - exit_price) * qty_closed
```

Generalized with `side.sign()`:
```
pnl = side_sign * qty_closed * (exit_price - entry_price)
```

where `side_sign = +1` for Long, `-1` for Short.

**ROE (Return on Equity):**
```
roe = pnl / margin
```

**Effective leverage:**
```
effective_leverage = notional / equity = (size * mark_price) / (margin + unrealized_pnl)
```

### B. Worked Numerical Example

**Long position closed:**
- Entry: $65,000, Exit: $67,000, Qty: 0.5 BTC, Margin: $3,250 (10x)
```
pnl = (67,000 - 65,000) * 0.5 = $1,000
roe = 1,000 / 3,250 = 0.3077 = 30.77%
```

**Short position closed at a loss:**
- Entry: $65,000, Exit: $66,500, Qty: 1.0 BTC, Margin: $6,500
```
pnl = (65,000 - 66,500) * 1.0 = -$1,500
roe = -1,500 / 6,500 = -0.2308 = -23.08%
```

**Effective leverage at mark:**
- Long 2 BTC, entry $65,000, margin $13,000, mark $66,000
```
unrealized_pnl = (66,000 - 65,000) * 2 = $2,000
equity = 13,000 + 2,000 = $15,000
notional = 2 * 66,000 = $132,000
effective_leverage = 132,000 / 15,000 = 8.8x (down from initial 10x due to profit)
```

### C. Reference Implementation Comparison

Universal formulas. No exchange-specific variation.

### D. Edge Cases & Boundary Behavior

- **Negative quantity:** Returns `Err(SettlementError::NegativeQuantity)`.
- **Non-positive price:** Returns `Err(SettlementError::NonPositivePrice)`.
- **Zero margin for ROE:** Returns `Err(SettlementError::ZeroMargin)`.
- **Zero/negative equity for leverage:** Returns `Err(SettlementError::NonPositiveEquity)`.

### E. Test Coverage Reference

- **File:** `crates/settlement/src/pnl.rs` (inline tests)
- **Key tests:** `test_long_profit`, `test_short_profit`, `test_long_loss`, `test_roe`, `test_negative_qty_error`

### Compliance

**Match.** Universal PnL formulas.

---

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

## 25. Order Types -- Complete Specification

### Mechanism

Complete specification of all order types supported by the exchange, including
their behavior under different time-in-force modes and trigger conditions.

### A. Mathematical Derivation

**Time-in-force behavior matrix:**

| TIF | Unfilled Remainder | Rests on Book? | Expiry? |
|-----|-------------------|----------------|---------|
| GTC (Good-Till-Cancel) | Rests | Yes | No (manual cancel only) |
| IOC (Immediate-or-Cancel) | Cancelled | No | Immediate |
| FOK (Fill-or-Kill) | Entire order rejected | No | Immediate |
| PostOnly | N/A (no match allowed) | Yes | No |
| GTD (Good-Till-Date) | Rests | Yes | At specified time |
| Market | Cancelled | No | Immediate |

**PostOnly rejection condition:**
```
if incoming.side == Buy AND incoming.price >= best_ask:
    REJECT (would immediately match)
if incoming.side == Sell AND incoming.price <= best_bid:
    REJECT
```

**FOK fill condition:**
```
available_qty = sum(qty at eligible price levels)
if available_qty < order.qty:
    REJECT entire order
```

**Stop-Limit trigger condition:**
```
For stop-buy (typically SL for shorts):
    trigger when reference_price >= trigger_price

For stop-sell (typically SL for longs):
    trigger when reference_price <= trigger_price
```

where `reference_price` is one of: last_price, mark_price, or index_price (configurable via `TriggerBy`).

**Trailing stop trigger:**

Fixed mode:
```
For sell: trigger when peak_price - current_price >= trailing_delta
For buy:  trigger when current_price - trough_price >= trailing_delta
```

Percentage mode:
```
For sell: trigger when peak_price - current_price >= peak_price * trailing_delta / 100
For buy:  trigger when current_price - trough_price >= trough_price * trailing_delta / 100
```

**Bracket order lifecycle:**
```
1. Entry order placed in matching engine (limit or market)
2. Entry fills → TP and SL triggers activated in trigger engine
3. TP triggers → SL cancelled (via OCO link)
   OR
   SL triggers → TP cancelled (via OCO link)
```

**OCO linking:**
```
on_fill(order_A):
    cancel(order_B)
on_fill(order_B):
    cancel(order_A)
```

### B. Worked Numerical Example

**Bracket order example:**

1. Entry: Buy 1 BTC at market ($65,000)
2. TP: Sell trigger at $67,000 (take profit)
3. SL: Sell trigger at $63,000 (stop loss)

Price rises to $67,000:
- TP triggers → market sell 1 BTC at ~$67,000
- SL cancelled immediately (OCO)
- Profit: ($67,000 - $65,000) * 1 = $2,000

Alternative: price drops to $63,000:
- SL triggers → market sell 1 BTC at ~$63,000
- TP cancelled immediately (OCO)
- Loss: ($63,000 - $65,000) * 1 = -$2,000

**Trailing stop example:**
- Sell trailing stop, delta = $2,000 (fixed)
- Entry at $65,000

Price path: $65,000 → $67,000 → $68,000 (peak) → $66,500 → $66,000

At $68,000: peak = $68,000, threshold = $68,000 - $2,000 = $66,000.
At $66,500: 68,000 - 66,500 = $1,500 < $2,000. No trigger.
At $66,000: 68,000 - 66,000 = $2,000 >= $2,000. **TRIGGERED.**

Effective exit: $66,000. Protected $1,000 of the $3,000 gain from peak.

**Percentage trailing stop:**
- Delta = 3%, peak = $68,000
- Threshold = $68,000 * 0.03 = $2,040
- Trigger at $68,000 - $2,040 = $65,960

### C. Reference Implementation Comparison

| Order Type | Binance | Bybit | Our Implementation |
|-----------|---------|-------|-------------------|
| Limit (GTC/IOC/FOK) | Full support | Full support | Full support |
| PostOnly | Yes | Yes | Yes |
| GTD | Yes (GTE) | Yes | Yes |
| Stop-Limit/Market | By last/mark/index | By last/mark | By last/mark/index |
| Trailing Stop | Fixed + % modes | Fixed + % modes | Fixed + % modes |
| Bracket (TP+SL) | Yes | Yes | Yes |
| OCO | Yes | Yes | Yes |

**FIX Protocol mapping:**
| FIX OrdType (tag 40) | Our OrderType |
|----------------------|---------------|
| 1 = Market | Market |
| 2 = Limit | Limit |
| 3 = Stop | StopMarket |
| 4 = Stop Limit | StopLimit |
| P = Trailing Stop | TrailingStop |

### D. Edge Cases & Boundary Behavior

- **PostOnly at the spread:** Buy at $65,000 when best ask is $65,000 → REJECTED (would match).
- **FOK with exact fill:** Available qty == order qty → fills completely.
- **GTD already expired:** Rejected at submission if `expires_at < now`.
- **Trailing stop in a monotonic trend:** Peak/trough tracks continuously; never triggers until reversal.
- **Bracket with only TP (no SL):** Valid. Only TP leg activates on entry fill.
- **OCO: both orders fill simultaneously:** Impossible in single-threaded matching. One always processes first.

### E. Test Coverage Reference

- **File:** `crates/matching-engine/src/orderbook.rs` (limit, market, IOC, FOK, PostOnly tests)
- **File:** `crates/trigger-service/src/trigger_engine.rs` (stop, trailing tests)
- **File:** `crates/trigger-service/src/bracket.rs` (bracket lifecycle tests)
- **File:** `crates/trigger-service/src/oco.rs` (OCO linking tests)
- **File:** `crates/trigger-service/src/trailing.rs` (trailing stop tracker tests)

### Our Implementation

- **Code:** `crates/matching-engine/src/orderbook.rs` (core matching for limit/market)
- **Code:** `crates/trigger-service/src/trigger_order.rs` (trigger order types)
- **Code:** `crates/trigger-service/src/bracket.rs` (bracket orders)
- **Code:** `crates/trigger-service/src/oco.rs` (OCO groups)
- **Code:** `crates/trigger-service/src/trailing.rs` (trailing stop tracker)

### Industry Standard / Reference

| Source | Description |
|--------|-------------|
| FIX Protocol 4.4. "OrdType (tag 40)." | Standard order type definitions. |
| CME Group. "Order Types and Execution Procedures." | Comprehensive order type specification. |
| Binance. "Order Types." | Exchange order type documentation. |
| Bybit. "Order Types." | Exchange order type documentation. |

### Compliance

**Match.** All standard order types implemented. FIX 4.4 OrdType mappings are consistent.

---

## 26. Copy Trading Economics

### Mechanism

Copy trading allows follower accounts to automatically replicate a master trader's
positions, with proportional sizing and profit-sharing settlement.

### A. Mathematical Derivation

**Proportional sizing:**
```
follower_qty = master_qty * (follower_allocation / master_equity)
```

This scales the master's trade proportionally to the follower's committed capital.
If the master has $100,000 equity and trades 1 BTC, a follower with $10,000
allocation copies 0.1 BTC.

**Profit sharing:**
```
if gross_profit > 0:
    master_share = gross_profit * profit_share_rate
    follower_net = gross_profit - master_share
else:
    master_share = 0
    follower_net = gross_profit  (follower absorbs full loss)
```

The asymmetry is critical: losses are never shared. The master only earns on profitable trades.

**Stop-loss enforcement:**
```
if total_pnl <= -(stop_loss_pct * allocation):
    deactivate relation
    close all copy positions
```

### B. Worked Numerical Example

**Setup:**
- Master equity: $100,000
- Follower allocation: $5,000
- Profit share rate: 10%

**Master trades 2 BTC long at $65,000:**
```
follower_qty = 2 * (5,000 / 100,000) = 0.1 BTC
follower_margin = 0.1 * 65,000 / 10 = $650 (at 10x leverage)
```

**Position closes at $67,000 (profit):**
```
follower_pnl = (67,000 - 65,000) * 0.1 = $200
master_share = 200 * 0.10 = $20
follower_net = 200 - 20 = $180
```

**Position closes at $63,000 (loss):**
```
follower_pnl = (63,000 - 65,000) * 0.1 = -$200
master_share = $0 (losses not shared)
follower_net = -$200
```

**Stop-loss trigger (stop_loss_pct = 20%):**
```
threshold = -(0.20 * 5,000) = -$1,000
After several losing trades, total_pnl = -$1,050
-1,050 <= -1,000 → DEACTIVATE relation, close all positions
```

### C. Reference Implementation Comparison

| Platform | Sizing | Profit Share | Stop-Loss | Difference |
|----------|--------|-------------|-----------|------------|
| **eToro CopyTrader** (https://www.etoro.com/copytrader/) | Proportional by copy amount / trader's equity. | Variable per trader (typically 5-30%). | Copy stop-loss at relation level. | Identical sizing model. eToro uses CSL (Copy Stop Loss) threshold, same as our stop_loss_pct. |
| **Bybit Copy Trading** (https://www.bybit.com/en/help-center/article/Copy-Trading-Introduction) | Proportional or fixed amount. | 10% default, configurable. | Per-relation and per-trade SL. | We support proportional mode. Fixed-mode is external (relation.proportional = false). |
| **Binance Copy Trading** | Similar proportional model. | 10% profit share. | Configurable stop-loss. | Identical mechanics. |

### D. Edge Cases & Boundary Behavior

- **Zero master equity:** `generate_copy_orders` returns empty Vec. No division by zero.
- **Non-positive follower qty:** Silently dropped (logged at debug level).
- **Inactive relation:** Filtered out before copy order generation.
- **Loss greater than allocation:** Protected by stop-loss. If no stop-loss, follower can lose more than allocation (margin call on copied position).
- **Max masters per follower:** Configurable limit prevents over-diversification.
- **Profit share rate bounds:** Must be in [0, 1]. Returns `Err(CopyTradingError::InvalidShareRate)` otherwise.

### E. Test Coverage Reference

- **File:** `crates/copy-trading/src/copier.rs` (inline tests)
- **File:** `crates/copy-trading/src/settlement.rs` (inline tests)
- **File:** `crates/copy-trading/src/follower.rs` (inline tests)
- **Key tests:** `test_proportional_copy`, `test_zero_equity`, `test_profit_share_positive`, `test_loss_not_shared`, `test_stop_loss_trigger`
- **Test values:** Equities $10,000--$100,000, allocations $1,000--$10,000.

### Our Implementation

- **Code:** `crates/copy-trading/src/copier.rs` (proportional sizing)
- **Code:** `crates/copy-trading/src/settlement.rs` (profit sharing)
- **Code:** `crates/copy-trading/src/follower.rs` (relation management, stop-loss)

### Industry Standard / Reference

| Source | Description |
|--------|-------------|
| eToro. "CopyTrader." | Proportional copy trading with Copy Stop Loss. |
| Bybit. "Copy Trading." | Proportional sizing, profit sharing. |
| ZuluTrade (https://www.zulutrade.com/) | Signal-based copy trading with lot-size scaling. |
| ESMA (2018). "CFDs, Binary Options, and Copy Trading." | Regulatory guidance on copy trading risks. |

### Compliance

**Match.** Proportional sizing, asymmetric profit sharing (no loss sharing), and
per-relation stop-loss are industry standard.

---

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

## 28. Surveillance -- Detection Algorithms

### Mechanism

Market surveillance detects three primary manipulation patterns: spoofing,
wash trading, and quote stuffing.

### A. Mathematical Derivation

**Spoofing detection -- Cancel Ratio:**

```
cancel_ratio = cancelled_orders / total_placed_orders    (within rolling window)
```

Alert condition:
```
cancel_ratio >= cancel_ratio_threshold  AND  total_placed_orders >= min_orders
```

The `min_orders` floor prevents false positives from users who place 1 order and cancel it (100% ratio but not meaningful).

**Wash trading -- Volume Balance:**

For a single user on a single symbol within the rolling window:
```
buy_volume = sum(qty where taker_side == Buy)
sell_volume = sum(qty where taker_side == Sell)
balance_ratio = min(buy_vol, sell_vol) / max(buy_vol, sell_vol)
```

Alert condition:
```
balance_ratio >= volume_balance_threshold   (default 0.80 = 80%)
```

A ratio of 80%+ means the user's buy and sell volume are suspiciously balanced,
suggesting they are both sides of the market.

**Wash trading -- Circular Trading (cycle detection):**

Build a directed graph: edge (A → B) exists if user A is maker and user B is taker.
Search for cycles of length >= 3 (A → B → C → A).

The cycle detection uses BFS from each node, looking for paths that return to the
origin within the analysis window.

**Quote Stuffing -- Message Rate:**

```
order_count_1s = count(orders placed by user in [now - 1s, now])
```

Alert condition:
```
order_count_1s > max_orders_per_second
```

### B. Worked Numerical Example

**Spoofing example:**
- Window: 60 seconds
- User places 100 orders, cancels 96
```
cancel_ratio = 96/100 = 0.96
threshold = 0.95
min_orders = 10
96/100 = 0.96 >= 0.95 AND 100 >= 10 → ALERT (severity: High)
```

**Wash trading -- Volume Balance:**
- User buys 50 BTC and sells 48 BTC of BTCUSDT in 1 hour
```
balance_ratio = min(50, 48) / max(50, 48) = 48/50 = 0.96
threshold = 0.80
0.96 >= 0.80 → ALERT (severity: High)
```

**Wash trading -- Circular:**
- Alice → Bob (Alice is maker, Bob is taker): 5 BTC
- Bob → Carol: 4 BTC
- Carol → Alice: 3 BTC

Cycle detected: Alice → Bob → Carol → Alice. Alert with min volume in cycle = 3 BTC.

**Quote stuffing:**
- User sends 150 orders in 1 second
- Threshold: 100 orders/second
```
150 > 100 → ALERT (severity: Medium, auto_action: RateLimit)
```

### C. Reference Implementation Comparison

| Mechanism | Regulatory Basis | Industry Practice | Our Implementation |
|-----------|-----------------|-------------------|-------------------|
| **Spoofing** | SEC Rule 10b-5 (anti-manipulation); Dodd-Frank Act Section 747; EU MAR Art. 12(1)(a) | All exchanges required to have surveillance. NASDAQ, CME, ICE use proprietary systems. | Cancel-ratio heuristic with configurable threshold. Standard approach. |
| **Wash Trading** | CME Rule 534; FINRA Rule 5210; EU MAR Art. 12(1)(a)(ii) | Volume-balance analysis + circular trade detection. Standard approach. | Both volume balance and cycle detection implemented. |
| **Quote Stuffing** | SEC Regulation SCI; EU MiFID II Art. 17 (Algorithmic Trading) | Message rate monitoring per firm. | Per-user rate limiting with configurable threshold. |

### D. Edge Cases & Boundary Behavior

- **New user (few orders):** `min_orders` floor (default 10) prevents false spoofing alerts.
- **Legitimate HFT:** High cancel ratios are normal for market makers. The threshold should be calibrated per user type.
- **Self-trade via STP:** STP prevents self-trades before they reach the surveillance layer. Wash trading detection operates on actual fills.
- **Cycle of length 2:** Not flagged (could be normal market making). Only cycles >= 3 are suspicious.
- **Rolling window cleanup:** `cleanup_old()` removes stale events to prevent memory growth.

### E. Test Coverage Reference

- **File:** `crates/surveillance/src/spoofing.rs` (inline tests)
- **File:** `crates/surveillance/src/wash_trading.rs` (inline tests)
- **File:** `crates/surveillance/src/quote_stuffing.rs` (inline tests)
- **Key tests:** `test_spoofing_below_threshold`, `test_spoofing_alert`, `test_volume_balance_alert`, `test_cycle_detection`, `test_rate_limit_alert`

### Our Implementation

- **Code:** `crates/surveillance/src/spoofing.rs` (SpoofingDetector)
- **Code:** `crates/surveillance/src/wash_trading.rs` (WashTradingDetector)
- **Code:** `crates/surveillance/src/quote_stuffing.rs` (QuoteStuffingDetector)
- Default thresholds: spoofing 95%, wash 80%, quote stuffing 100/s.
- Configurable rolling windows (default 60s for spoofing, configurable for wash/quote).

### Industry Standard / Reference

| Source | Description |
|--------|-------------|
| IOSCO (2011). "Regulatory Issues Raised by the Impact of Technological Changes on Market Integrity and Efficiency." | International standards for market surveillance. |
| SEC Rule 10b-5 | Anti-manipulation, anti-fraud rule under US Securities Exchange Act of 1934. |
| EU MAR (Market Abuse Regulation 596/2014), Articles 12 and 15 | Market manipulation definition and prohibition in EU. |
| EU MiCA (Regulation 2023/1114), Article 91 | Market abuse provisions specific to crypto-assets. |
| Dodd-Frank Act (2010), Section 747 | Anti-spoofing provision for commodity markets. |
| CME Rule 534 (Wash Trades Prohibited) | Explicit prohibition of wash trading on CME markets. |
| FINRA Rule 5210 | Publication of transactions and quotations; prohibits fictitious trades. |

### Compliance

**Match.** Our surveillance covers the three primary manipulation types required by
regulators (spoofing, wash trading, quote stuffing). The detection algorithms use
standard heuristics consistent with industry practice.

---

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

---

## 30. WAL (Write-Ahead Log) -- Crash Recovery

### Mechanism

Every command is durably persisted to an append-only log BEFORE entering the matching
engine. On crash recovery, the WAL is replayed to reconstruct the matching engine state.

### A. Mathematical Derivation

**WAL guarantees:**

1. **Durability:** Each entry is fsync'd before the command is processed.
   `P(data_loss | crash_after_fsync) = 0` (assuming non-corrupt storage).

2. **Ordering:** Sequence numbers are monotonically increasing and gap-free.
   `seq_{n+1} = seq_n + 1` for all entries.

3. **Idempotent replay:** Replaying the same command twice produces the same state.
   For `PlaceOrder`: if the order already exists, it's a no-op.
   For `CancelOrder`: if the order is already cancelled, it's a no-op.

**Segment rotation:**
```
if current_segment_entries >= max_entries_per_segment:
    create new segment file
    current_segment = commands_{next_id:06d}.wal
    current_entries = 0
```

**Retention:**
```
if count(segment_files) > max_segments:
    delete oldest segment files until count <= max_segments
```

Default: 1,000,000 entries per segment, retain 3 segments.

**Recovery flow:**
```
1. Read all WAL entries from all segments (ordered by sequence)
2. For each entry (seq, command):
   a. If command references a terminal order (filled/cancelled in DB): SKIP
   b. Otherwise: replay into matching engine
3. Matching engine state is now identical to pre-crash state
```

### B. Worked Numerical Example

**WAL file format:**
```
1\t{"PlaceOrder":{"order_id":"abc-123","user_id":"00000000-0000-0000-0000-000000000001","symbol":"BTCUSDT","side":"Buy","price":"65000","qty":"1","order_type":"Limit","time_in_force":"GTC"}}
2\t{"PlaceOrder":{"order_id":"def-456","user_id":"00000000-0000-0000-0000-000000000002","symbol":"BTCUSDT","side":"Sell","price":"65000","qty":"0.5","order_type":"Limit","time_in_force":"GTC"}}
3\t{"CancelOrder":{"order_id":"abc-123"}}
```

**Crash at sequence 2 (after write, before matching engine processes):**

On restart:
1. WAL reader reads entries: [(1, PlaceOrder abc-123), (2, PlaceOrder def-456)]
2. Entry 3 was never written (crash happened before cancel was committed).
3. Replay entry 1: Place buy order abc-123 at $65,000, 1 BTC.
4. Replay entry 2: Place sell order def-456 at $65,000, 0.5 BTC.
5. Matching engine matches: 0.5 BTC fill at $65,000. abc-123 has 0.5 BTC remaining.

State is now identical to what it would have been if no crash occurred.

**Segment rotation example:**
- Segment 1: `commands_000001.wal` (entries 1--1,000,000)
- Segment 2: `commands_000002.wal` (entries 1,000,001--2,000,000)
- Segment 3: `commands_000003.wal` (entries 2,000,001--3,000,000)
- Segment 4 starts: `commands_000004.wal` (entries 3,000,001+)

With max_segments=3, segment 1 is now deleted. Entries 1--1,000,000 are lost.
This is safe only if a matching engine snapshot covers up to entry 1,000,000.

### C. Reference Implementation Comparison

| System | WAL Design | Difference |
|--------|-----------|------------|
| **ARIES** (Mohan et al., 1992. "ARIES: A Transaction Recovery Method." *ACM TODS*) | Write-ahead logging with redo/undo recovery. Page-level logging. Fuzzy checkpointing. | Our WAL is command-level (not page-level). We only need redo (no undo, because matching is deterministic). Simpler than ARIES. |
| **PostgreSQL WAL** (https://www.postgresql.org/docs/current/wal-intro.html) | Write-ahead logging for crash recovery. Segment files (16MB default). Continuous archiving. | Similar segment rotation concept. PostgreSQL has LSN (Log Sequence Number) analogous to our sequence number. |
| **Apache Kafka** | Append-only commit log with segment rotation and retention. | Kafka's partition log is conceptually identical to our WAL segments. We are single-node; Kafka is distributed. |
| **LMAX Disruptor** | Event journal for deterministic replay. Single-writer pattern. | Same philosophy: log events before processing. LMAX uses memory-mapped files for lower latency. |

### D. Edge Cases & Boundary Behavior

- **Empty WAL on first start:** Sequence starts at 0. First entry gets sequence 1.
- **Corrupt entry:** WAL reader skips malformed lines. Logged as error in `ReplayResult::errors`.
- **Gap in sequence numbers:** The replay is gap-tolerant. Commands are processed regardless of gaps.
- **Balance events during replay:** `BalanceDeposit`, `BalanceWithdraw`, `FundingPayment` are skipped by the matching engine replay (they are for the account layer, not the order book).
- **Terminal orders during replay:** If an order is already filled/cancelled (known from DB), the WAL entry is skipped. This prevents re-matching already-settled trades.
- **Durable = false:** For tests and benchmarks, fsync is skipped. Not safe for production.
- **Concurrent writes:** Not supported. Single-writer design consistent with single-threaded matching.

### E. Test Coverage Reference

- **File:** `crates/journal/src/wal.rs` (inline tests)
- **File:** `crates/journal/src/segments.rs` (inline tests)
- **File:** `crates/journal/src/replay.rs` (inline tests)
- **Key tests:** `test_append_and_read`, `test_sequence_continuity`, `test_segment_rotation`, `test_replay_skip_terminal`, `test_replay_skip_balance_events`, `test_truncate`
- **Test values:** 100--10,000 entries, segment rotation at 100 entries.

### Our Implementation

- **Code:** `crates/journal/src/wal.rs` (WriteAheadLog: append, fsync, truncate)
- **Code:** `crates/journal/src/segments.rs` (SegmentedWal: rotation, retention)
- **Code:** `crates/journal/src/replay.rs` (replay_into_engine: idempotent recovery)
- **Code:** `crates/journal/src/reader.rs` (WalReader: sequential read)
- Format: `{seq}\t{json}\n` per entry.
- fsync after each write (configurable).
- Segment rotation: 1M entries/segment, retain 3 segments.
- Replay: idempotent, gap-tolerant, skips terminal orders and balance events.

### Industry Standard / Reference

| Source | Description |
|--------|-------------|
| Mohan, C. et al. (1992). "ARIES: A Transaction Recovery Method Supporting Fine-Granularity Locking and Partial Rollbacks Using Write-Ahead Logging." *ACM Transactions on Database Systems*, 17(1), 94-162. | The foundational WAL recovery protocol. Our command-level WAL is a simplification of ARIES (redo-only, no undo). |
| PostgreSQL. "Write-Ahead Logging (WAL)." (https://www.postgresql.org/docs/current/wal-intro.html) | Production WAL implementation with segment rotation. |
| LMAX Exchange (2011). "The LMAX Architecture." (Martin Fowler) | Event sourcing with deterministic replay for exchange matching engines. |
| Kleppmann, M. (2017). *Designing Data-Intensive Applications*. O'Reilly. Chapter 11: Stream Processing. | Comprehensive treatment of event logs, compaction, and replay semantics. |

### Compliance

**Match.** Our WAL follows the ARIES principle (write before state mutation) adapted for
a deterministic matching engine. Segment rotation with retention matches PostgreSQL and
Kafka patterns. Idempotent replay ensures crash recovery produces identical state.

---

## Summary of Compliance Status

| # | Mechanism | Status | Notes |
|---|-----------|--------|-------|
| 1 | Order Matching (FIFO) | Full match | Standard price-time priority |
| 2 | Mark Price | Partial match | Missing EMA dampening; heavier index weight is conservative |
| 3 | Funding Rate | Match (simplified) | Premium-only model; missing TWAP of premium index |
| 4 | Liquidation Engine | Full match | Tiered MMR, partial liquidation, cross-margin |
| 5 | ADL | Full match | Identical to BitMEX formula |
| 6 | Insurance Fund | Full match | Standard waterfall |
| 7 | Index Price (Weighted Median) | Full match | More robust than some production exchanges |
| 8 | Margin Calculation | Full match | Linear and inverse, tiered |
| 9 | Portfolio Margin | Match (simplified) | Missing vol shocks and correlations |
| 10 | Perpetual Swap | Full match | BitMEX model |
| 11 | TWAP | Full match | Standard academic algorithm |
| 12 | VWAP | Full match | Standard with TWAP fallback |
| 13 | Grid Trading | Full match | Arithmetic grid, 3 modes |
| 14 | DCA | Full match | Standard with deviation filter |
| 15 | Black-Scholes | Full match | Standard BS with A&S CDF |
| 16 | STP | Match (minor gap) | Missing CancelBoth mode |
| 17 | Proof of Reserves | Full match | Maxwell/Dagher Merkle tree |
| 18 | KYC/AML Risk Scoring | Full match | FATF risk-based approach |
| 19 | Travel Rule | Match (Phase 1) | Manual VASP; needs TRISA for production |
| 20 | Collateral Haircuts | Full match | Anti-FTT policy enforced |
| 21 | Circuit Breaker | Full match | LULD-style with halt escalation |
| 22 | Socialized Loss | Full match | Pro-rata by profit |
| 23 | PnL Calculation | Full match | Universal formulas |
| 24 | Fee Engine & VIP Tiers | Full match | 6-tier system, maker rebate at top tier |
| 25 | Order Types | Full match | All standard types + bracket + OCO + trailing |
| 26 | Copy Trading | Full match | Proportional sizing, asymmetric profit sharing |
| 27 | Earn Products | Full match | Flexible/fixed/staking with auto-compound |
| 28 | Surveillance | Full match | Spoofing + wash trading + quote stuffing |
| 29 | Custody | Match (Phase 1) | Hot/warm/cold with circuit breaker; needs MPC for production |
| 30 | WAL (Crash Recovery) | Full match | ARIES-inspired, segment rotation, idempotent replay |

---

## Appendix: Key References (Bibliography)

1. Almgren, R. & Chriss, N. (2000). "Optimal Execution of Portfolio Transactions." *Journal of Risk*, 3(2), 5-39.
2. Abramowitz, M. & Stegun, I. (1964). *Handbook of Mathematical Functions*. National Bureau of Standards.
3. Basel Committee on Banking Supervision (2010, 2014). *Basel III: A Global Regulatory Framework*.
4. Basel Committee (2011). *Sound Practices for the Management and Supervision of Operational Risk*.
5. Berkowitz, S. et al. (1988). "The Total Cost of Transactions on the NYSE." *Journal of Finance*, 43(1), 97-112.
6. Bertsimas, D. & Lo, A. (1998). "Optimal Control of Execution Costs." *Journal of Financial Markets*, 1(1), 1-50.
7. Black, F. & Scholes, M. (1973). "The Pricing of Options and Corporate Liabilities." *Journal of Political Economy*, 81(3), 637-654.
8. Chainlink Labs (2017). "A Decentralized Oracle Network." Whitepaper.
9. Constantinides, G. (1979). "A Note on the Suboptimality of Dollar-Cost Averaging." *JFQA*, 14(2), 443-450.
10. Dagher, G. et al. (2015). "Provisions: Privacy-preserving proofs of solvency for Bitcoin exchanges." *ACM CCS*.
11. FATF (2012, updated 2023). "International Standards on Combating Money Laundering and the Financing of Terrorism & Proliferation."
12. Harris, L. (2003). *Trading and Exchanges: Market Microstructure for Practitioners*. Oxford University Press.
13. Haug, E.G. (2007). *The Complete Guide to Option Pricing Formulas*. 2nd ed. McGraw-Hill.
14. Hayes, A. / BitMEX (2016). "A Perpetual Swap." BitMEX Research.
15. Huber, P. (2004). *Robust Statistics*. Wiley.
16. Hull, J. (2018). *Options, Futures, and Other Derivatives*. 10th ed. Pearson.
17. IOSCO (2011). "Regulatory Issues Raised by the Impact of Technological Changes on Market Integrity and Efficiency."
18. Kissell, R. (2014). *The Science of Algorithmic Trading and Portfolio Management*. Academic Press.
19. Kleppmann, M. (2017). *Designing Data-Intensive Applications*. O'Reilly Media.
20. Maxwell, G. (2013). "Merkle approach to solvency proof." Bitcoin Talk Forum.
21. Merton, R. (1973). "Theory of Rational Option Pricing." *Bell Journal of Economics*, 4(1), 141-183.
22. Mohan, C. et al. (1992). "ARIES: A Transaction Recovery Method." *ACM Transactions on Database Systems*, 17(1), 94-162.
23. Shiller, R. (1993). *Macro Markets*. Oxford University Press.
24. Vanguard (2012). "Dollar-cost averaging just means taking risk later." Vanguard Research.

## Appendix: API Documentation URLs

| Exchange | Documentation |
|----------|--------------|
| Binance Futures | https://www.binance.com/en/support/faq/ |
| Bybit | https://www.bybit.com/en/help-center/ |
| OKX | https://www.okx.com/help-center |
| Deribit | https://www.deribit.com/knowledge-base/ |
| BitMEX | https://www.bitmex.com/app/perpetualContractsGuide |
| CME Group | https://www.cmegroup.com/education.html |
| eToro | https://www.etoro.com/copytrader/ |
| Fireblocks | https://www.fireblocks.com/platforms/ |
| BitGo | https://www.bitgo.com/ |

## Appendix: Regulatory Framework References

| Regulation | Jurisdiction | Relevance |
|-----------|-------------|-----------|
| FATF Recommendations (esp. 1, 10, 11, 16, 20) | International | KYC/AML, Travel Rule, risk-based approach |
| EU AMLD5 (Directive 2018/843) | EU | Customer due diligence requirements |
| EU MAR (Regulation 596/2014) | EU | Market abuse, spoofing, wash trading |
| EU MiCA (Regulation 2023/1114) | EU | Crypto-asset regulation, market abuse Art. 91 |
| EU TFR (Regulation 2023/1113) | EU | Transfer of funds, travel rule for crypto |
| SEC Rule 10b-5 | US | Anti-fraud, anti-manipulation |
| Dodd-Frank Act Section 747 | US | Anti-spoofing for commodity markets |
| CFTC Regulation 17 CFR 39.13 | US | Margin requirements for derivatives clearing |
| FinCEN 31 CFR 1010.410 | US | US wire transfer (travel) rule |
| Basel III (2010, 2014) | International | Capital requirements, collateral haircuts, leverage ratio |
| CME Rulebook | US | Exchange rules for matching, clearing, default waterfall |
| CCSS (CryptoCurrency Security Standard) | International | Custody security framework |

---

## 31. Unpaid Funding Debt

**What:** When a funding payment fails (insufficient balance to pay the funding fee), the debt is tracked on the Position as `unpaid_funding_debt`. This reduces effective margin, potentially triggering earlier liquidation.

**Reference:** Industry standard practice. Bybit, Binance, and OKX all reduce effective margin by unpaid funding obligations. A position with accumulated funding debt is under-margined relative to its notional.

**Formula:**
```
effective_margin = initial_margin + unrealized_pnl - unpaid_funding_debt
```

**Implementation:** `crates/risk-engine/src/liquidation.rs` — `unpaid_funding_debt` field on Position struct, deducted from margin in liquidation checks.

---

## 32. Yield Pool (Earn/Staking)

**What:** All earn and staking payouts are backed by the `YIELD_POOL_ACCOUNT_ID` reserve account. This pool is funded by margin interest income collected during the margin interest accrual task. Payouts are capped to the pool balance — the exchange never creates yield from nothing.

**Reference:** First-principles design. Celsius and BlockFi collapsed because yield obligations exceeded revenue. Our pool-backed model ensures mathematical sustainability: total payouts ≤ pool balance at all times.

**Formula:**
```
yield_per_user = TWAB(user_balance, period) × APY × (period_seconds / seconds_per_year)
payout = min(yield_per_user, pool_available_balance)
```

**Implementation:** `crates/api-gateway/src/tasks.rs` — `run_earn_yield_accrual()` reads pool balance before crediting users. `YIELD_POOL_ACCOUNT_ID` defined in `exchange.rs`.
