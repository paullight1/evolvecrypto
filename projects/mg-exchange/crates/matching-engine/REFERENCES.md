# Matching Engine — Business Logic References

Traces every matching engine mechanism to industry standards.
See also: `crates/trigger-service/REFERENCES.md` for stop, trailing, bracket, OCO.

---

## Table of Contents

1. [Order Matching -- Price-Time Priority (FIFO)](#1-order-matching--price-time-priority-fifo)
16. [Self-Trade Prevention (STP)](#16-self-trade-prevention-stp)
25. [Order Types -- Matching Engine Subset](#25-order-types--matching-engine-subset)

> **Scope note:** Section numbers are preserved from the master reference document at
> `docs/BUSINESS_LOGIC_REFERENCES.md`. Section 25 in that document covers all order
> types; this file includes only the types handled inside the matching engine
> (Limit, Market, IOC, FOK, PostOnly, GTD). Stop, trailing stop, bracket, and OCO
> are implemented in `crates/trigger-service/` and documented there.

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

## 25. Order Types -- Matching Engine Subset

> **Scope note:** This section covers only the order types and time-in-force modes
> handled directly inside the matching engine. Stop-limit, stop-market, trailing stop,
> bracket, and OCO orders are processed by `crates/trigger-service/` and are documented
> in `crates/trigger-service/REFERENCES.md` (section 25, trigger-service subset).

### Mechanism

Specification of all order types whose lifecycle is managed entirely within the
matching engine, including their behavior under different time-in-force modes.

### A. Mathematical Derivation

**Time-in-force behavior matrix (matching-engine-owned types):**

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

### B. Worked Numerical Example

**PostOnly at the spread:**

Order book: best ask = $65,000.
User submits PostOnly buy at $65,000. Since $65,000 >= best_ask ($65,000), order is REJECTED.
User submits PostOnly buy at $64,999. Since $64,999 < best_ask ($65,000), order RESTS on the bid side.

**FOK with insufficient liquidity:**

Order book asks: 0.3 BTC at $65,000, 0.1 BTC at $65,100. Total available = 0.4 BTC.
FOK buy for 0.5 BTC at $65,200: available_qty (0.4) < order.qty (0.5) → REJECTED. No partial fill, no book change.

**FOK with exact fill:**

Same book, FOK buy for 0.4 BTC at $65,200: available_qty (0.4) == order.qty (0.4) → fills completely.

**GTD already expired at submission:**

Order submitted with `expires_at = 2026-03-22T12:00:00Z`, current time = `2026-03-22T12:00:01Z` → rejected at submission.

**GTD expiry via background task:**

Order rests on book with `expires_at = T`. Background GTD task runs at interval, finds all orders where `expires_at <= now`, cancels them. Order is removed from the BTreeMap and VecDeque at its price level.

### C. Reference Implementation Comparison

| Order Type | Binance | Bybit | Our Implementation |
|-----------|---------|-------|-------------------|
| Limit (GTC/IOC/FOK) | Full support | Full support | Full support |
| PostOnly | Yes | Yes | Yes |
| GTD | Yes (GTE) | Yes | Yes |
| Market | Yes | Yes | Yes |

**FIX Protocol mapping (matching-engine types):**

| FIX OrdType (tag 40) | Our OrderType |
|----------------------|---------------|
| 1 = Market | Market |
| 2 = Limit | Limit (with GTC/IOC/FOK/PostOnly/GTD TIF) |

> Stop (FIX tag 40 = 3), Stop Limit (4), and Trailing Stop (P) are handled by
> `crates/trigger-service/` — see `crates/trigger-service/REFERENCES.md`.

### D. Edge Cases & Boundary Behavior

- **PostOnly at the spread:** Buy at $65,000 when best ask is $65,000 → REJECTED (would match).
- **FOK with exact fill:** Available qty == order qty → fills completely.
- **GTD already expired:** Rejected at submission if `expires_at < now`.
- **Trailing stop in a monotonic trend:** Not applicable here; handled in trigger-service.
- **Market order against empty book:** Zero fills; order cancelled immediately.
- **IOC partial fill:** Whatever fills, fills. Unfilled remainder is cancelled — no error, no rejection.

### E. Test Coverage Reference

- **File:** `crates/matching-engine/src/orderbook.rs` (limit, market, IOC, FOK, PostOnly, GTD tests)
- **Key tests:** `test_post_only_rejection`, `test_fok_reject_insufficient_liquidity`, `test_ioc_cancel_remainder`, `test_gtd_expiry`

> Stop, trailing, bracket, and OCO tests are in `crates/trigger-service/`:
> `trigger_engine.rs`, `bracket.rs`, `oco.rs`, `trailing.rs`.

### Our Implementation

- **Code:** `crates/matching-engine/src/orderbook.rs` (core matching for limit/market/IOC/FOK/PostOnly/GTD)

> Trigger-activated order types (stop, trailing, bracket, OCO) are implemented in
> `crates/trigger-service/src/trigger_order.rs`, `bracket.rs`, `oco.rs`, `trailing.rs`.

### Industry Standard / Reference

| Source | Description |
|--------|-------------|
| FIX Protocol 4.4. "OrdType (tag 40)." | Standard order type definitions. |
| CME Group. "Order Types and Execution Procedures." | Comprehensive order type specification. |
| Binance. "Order Types." | Exchange order type documentation. |
| Bybit. "Order Types." | Exchange order type documentation. |

### Compliance

**Match.** All matching-engine order types (Limit, Market, IOC, FOK, PostOnly, GTD) are fully
implemented. FIX 4.4 OrdType mappings for market and limit orders are consistent.
Trigger-service order types (stop, trailing, bracket, OCO) are separately compliant --
see `crates/trigger-service/REFERENCES.md`.

---
