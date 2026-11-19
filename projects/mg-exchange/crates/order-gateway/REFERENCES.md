# Order Gateway — Business Logic References

Traces every pre-trade check to industry standards.

---

## Table of Contents

1. [Pre-trade Validation](#1-pre-trade-validation)
2. [Self-Trade Prevention (STP)](#2-self-trade-prevention-stp)
3. [Rate Limiting](#3-rate-limiting)

---

## 1. Pre-trade Validation

### Mechanism

Every incoming order passes through `OrderValidator::validate()` before balance lock
or matching engine submission. Checks are ordered to reject cheaply first.

### Checks (in order)

| # | Check | Condition | Reference |
|---|-------|-----------|-----------|
| 1 | Quantity positive | `qty > 0` | Universal — no exchange accepts zero-qty |
| 2 | Lot size alignment | `qty % lot_size == 0` | Binance: LOT_SIZE filter; Bybit: qtyStep |
| 3 | Price positive (limit) | `price > 0` | Universal |
| 4 | Tick size alignment | `price % tick_size == 0` | Binance: PRICE_FILTER; Bybit: tickSize |
| 5 | Min notional | `price * qty >= min_notional` | Binance: MIN_NOTIONAL; Bybit: minOrderQty × price |
| 6 | Market + PostOnly rejected | Contradictory combination | Bybit: rejects with 10001 |
| 7 | Market + FOK rejected | Not supported (market is always IOC) | Binance: rejects |
| 8 | Max order qty | `qty <= max_order_qty` (if configured) | Binance: MAX_QTY; Bybit: maxOrderQty |
| 9 | Max notional | `price * qty <= max_order_notional` (if configured) | Binance: MAX_NUM_ALGO_ORDERS has notional equivalent |

### Reference

- Binance Exchange Info filters: https://binance-docs.github.io/apidocs/spot/en/#exchange-information
- Bybit Instrument Info: https://bybit-exchange.github.io/docs/v5/market/instrument

---

## 2. Self-Trade Prevention (STP)

### Mechanism

Prevents a user from trading with themselves (which could be used for wash trading
or fee manipulation). Three policies:

| Policy | Behavior | When to use |
|--------|----------|-------------|
| CancelTaker | Reject incoming order | Default for production |
| CancelMaker | Cancel resting orders, let taker through | Market makers need this |
| Allow | No prevention | Development/testing only |

### Detection

Pre-trade: `SelfTradeGuard` maintains `user_id → {order_ids}` map. On each new order,
checks if user has resting orders on the opposite side of the same symbol.

In-engine: Matching loop also checks `taker.user_id == maker.user_id` during sweep
(defense-in-depth against multi-level self-trade).

### Reference

- Binance STP modes: https://www.binance.com/en/support/faq/self-trade-prevention
- CME Globex Self-Match Prevention: CME Rule 575
- NASDAQ: Rule 4757 (wash trading prohibition)

---

## 3. Rate Limiting

### Mechanism

Sliding-window counter per (user_id, window). Two independent windows:

| Window | Default | Purpose |
|--------|---------|---------|
| Per-second | 50 orders/sec | Burst protection |
| Per-minute | 1200 orders/min | Sustained load protection |

### Implementation

`OrderRateLimiter` uses a `Vec<Instant>` ring buffer per user. On each order:
1. Count timestamps within the window (1s or 60s ago to now)
2. If count >= limit, reject
3. Otherwise, record timestamp and allow

### Reference

| Exchange | Per-second | Per-minute | Per-day |
|----------|-----------|------------|---------|
| Binance | 10 (spot) | — | 200K |
| Bybit | 10-100 | — | — |
| **Ours** | **50** | **1200** | — |

Source: Binance API Rate Limits, Bybit API Rate Limits.

---

## Compliance

All pre-trade checks match or exceed Binance/Bybit standards. The order gateway
serves as the first line of defense — malformed orders are rejected before touching
balance state or matching engine, minimizing the blast radius of invalid input.
