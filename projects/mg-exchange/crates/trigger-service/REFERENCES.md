# Trigger Service — Business Logic References

Traces stop, trailing, bracket, and OCO order mechanics to industry standards.
See also: `crates/matching-engine/REFERENCES.md` for core order matching (limit/market/IOC/FOK/PostOnly).

---

## Stop-Limit / Stop-Market Trigger

### Mechanism

Stop orders rest in the trigger engine (not the matching engine) until a reference
price crosses the trigger threshold, at which point a limit or market order is
injected into the matching engine.

### Trigger Condition

```
For stop-buy (typically SL for shorts):
    trigger when reference_price >= trigger_price

For stop-sell (typically SL for longs):
    trigger when reference_price <= trigger_price
```

where `reference_price` is one of: last_price, mark_price, or index_price (configurable via `TriggerBy`).

### Reference Implementation Comparison

| Order Type | Binance | Bybit | Our Implementation |
|-----------|---------|-------|-------------------|
| Stop-Limit/Market | By last/mark/index | By last/mark | By last/mark/index |

### Test Coverage

- **File:** `crates/trigger-service/src/trigger_engine.rs` (stop, trailing tests)

### Code

- **Code:** `crates/trigger-service/src/trigger_order.rs` (trigger order types)

---

## Trailing Stop

### Mechanism

A trailing stop adjusts its trigger level as the market moves in the user's
favor, locking in profit while allowing the position to run.

### Formulas

**Fixed mode:**
```
For sell: trigger when peak_price - current_price >= trailing_delta
For buy:  trigger when current_price - trough_price >= trailing_delta
```

**Percentage mode:**
```
For sell: trigger when peak_price - current_price >= peak_price * trailing_delta / 100
For buy:  trigger when current_price - trough_price >= trough_price * trailing_delta / 100
```

### Worked Numerical Example

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

### Reference Implementation Comparison

| Feature | Binance | Bybit | Our Implementation |
|---------|---------|-------|-------------------|
| Trailing Stop | Fixed + % modes | Fixed + % modes | Fixed + % modes |

### Edge Cases

- **Trailing stop in a monotonic trend:** Peak/trough tracks continuously; never triggers until reversal.

### Test Coverage

- **File:** `crates/trigger-service/src/trailing.rs` (trailing stop tracker tests)

### Code

- **Code:** `crates/trigger-service/src/trailing.rs` (trailing stop tracker)

---

## Bracket Orders (TP + SL)

### Mechanism

A bracket order combines an entry order with a take-profit (TP) and stop-loss (SL).
When the entry fills, both TP and SL are activated as trigger orders linked via OCO.

### Lifecycle

```
1. Entry order placed in matching engine (limit or market)
2. Entry fills → TP and SL triggers activated in trigger engine
3. TP triggers → SL cancelled (via OCO link)
   OR
   SL triggers → TP cancelled (via OCO link)
```

### Worked Numerical Example

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

### Edge Cases

- **Bracket with only TP (no SL):** Valid. Only TP leg activates on entry fill.

### Test Coverage

- **File:** `crates/trigger-service/src/bracket.rs` (bracket lifecycle tests)

### Code

- **Code:** `crates/trigger-service/src/bracket.rs` (bracket orders)

---

## OCO (One-Cancels-Other)

### Mechanism

Two orders are linked so that when one fills, the other is automatically cancelled.

### Formula

```
on_fill(order_A):
    cancel(order_B)
on_fill(order_B):
    cancel(order_A)
```

### Edge Cases

- **OCO: both orders fill simultaneously:** Impossible in single-threaded matching. One always processes first.

### Test Coverage

- **File:** `crates/trigger-service/src/oco.rs` (OCO linking tests)

### Code

- **Code:** `crates/trigger-service/src/oco.rs` (OCO groups)

---

## FIX Protocol Mapping

| FIX OrdType (tag 40) | Our OrderType |
|----------------------|---------------|
| 1 = Market | Market |
| 2 = Limit | Limit |
| 3 = Stop | StopMarket |
| 4 = Stop Limit | StopLimit |
| P = Trailing Stop | TrailingStop |

---

## Industry Standard / Reference

| Source | Description |
|--------|-------------|
| FIX Protocol 4.4. "OrdType (tag 40)." | Standard order type definitions. |
| CME Group. "Order Types and Execution Procedures." | Comprehensive order type specification. |
| Binance. "Order Types." | Exchange order type documentation. |
| Bybit. "Order Types." | Exchange order type documentation. |

## Compliance

**Match.** All standard trigger order types implemented. FIX 4.4 OrdType mappings are consistent.
