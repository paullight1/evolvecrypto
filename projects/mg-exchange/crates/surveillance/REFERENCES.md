# Surveillance — Business Logic References

Traces market surveillance algorithms to regulatory standards.

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
