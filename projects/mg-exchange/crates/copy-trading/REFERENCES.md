# Copy Trading — Business Logic References

Traces copy trading economics to industry standards.

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
