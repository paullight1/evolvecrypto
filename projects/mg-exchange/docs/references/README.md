# Business Logic References — Index

Every financial mechanism in the exchange traces to an industry standard, academic
paper, or documented exchange practice. References live alongside the code they
describe — one `REFERENCES.md` per crate.

**Format:** Each reference includes: mechanism description, mathematical derivation,
worked numerical example, competitor comparison (Bybit/Binance), edge cases, test
coverage pointers, and compliance status.

---

## Reference Files by Crate

### Core Trading

| Crate | File | Mechanisms |
|-------|------|------------|
| `matching-engine` | [`REFERENCES.md`](../../crates/matching-engine/REFERENCES.md) | Order matching (price-time FIFO), Self-Trade Prevention, Order types (limit/market/IOC/FOK/PostOnly/GTD) |
| `trigger-service` | [`REFERENCES.md`](../../crates/trigger-service/REFERENCES.md) | Stop-limit/market, Trailing stop, Bracket orders (TP+SL), OCO |
| `order-gateway` | — | Pre-trade risk checks (uses risk-engine formulas) |

### Risk & Margin

| Crate | File | Mechanisms |
|-------|------|------------|
| `risk-engine` | [`REFERENCES.md`](../../crates/risk-engine/REFERENCES.md) | Mark price, Liquidation engine, ADL, Margin calculation (linear/inverse), Portfolio margin, Black-Scholes options pricing, Collateral haircuts, Circuit breaker, Socialized loss |

### Settlement & Pricing

| Crate | File | Mechanisms |
|-------|------|------------|
| `settlement` | [`REFERENCES.md`](../../crates/settlement/REFERENCES.md) | Funding rate, Insurance fund & default waterfall, Perpetual swap mechanics, PnL calculation |
| `index-price` | [`REFERENCES.md`](../../crates/index-price/REFERENCES.md) | Index price (weighted median aggregation) |

### Account & Fees

| Crate | File | Mechanisms |
|-------|------|------------|
| `account` | [`REFERENCES.md`](../../crates/account/REFERENCES.md) | Fee engine & VIP tiers, Maker rebates, Referral commissions |

### Products

| Crate | File | Mechanisms |
|-------|------|------------|
| `api-gateway` | [`REFERENCES.md`](../../crates/api-gateway/REFERENCES.md) | Earn products (flexible/fixed/staking), Yield pool mechanics |
| `trading-bots` | [`REFERENCES.md`](../../crates/trading-bots/REFERENCES.md) | Grid trading, DCA (Dollar-Cost Averaging) |
| `algo-execution` | [`REFERENCES.md`](../../crates/algo-execution/REFERENCES.md) | TWAP, VWAP execution algorithms |
| `copy-trading` | [`REFERENCES.md`](../../crates/copy-trading/REFERENCES.md) | Copy trading economics (proportional sizing, profit sharing) |

### Compliance & Surveillance

| Crate | File | Mechanisms |
|-------|------|------------|
| `compliance` | [`REFERENCES.md`](../../crates/compliance/REFERENCES.md) | KYC/AML risk scoring, Travel Rule |
| `surveillance` | [`REFERENCES.md`](../../crates/surveillance/REFERENCES.md) | Spoofing detection, Wash trading detection, Quote stuffing detection |

### Infrastructure

| Crate | File | Mechanisms |
|-------|------|------------|
| `custody` | [`REFERENCES.md`](../../crates/custody/REFERENCES.md) | Hot/warm/cold wallet architecture, Outflow circuit breaker |
| `proof-of-reserves` | [`REFERENCES.md`](../../crates/proof-of-reserves/REFERENCES.md) | Merkle tree proof of reserves |
| `journal` | [`REFERENCES.md`](../../crates/journal/REFERENCES.md) | Write-Ahead Log, Crash recovery, Segment rotation |

---

## Compliance Summary

| # | Mechanism | Status | Crate |
|---|-----------|--------|-------|
| 1 | Order Matching (FIFO) | Full match | matching-engine |
| 2 | Mark Price | Partial (missing EMA dampening) | risk-engine |
| 3 | Funding Rate | Match (simplified, no TWAP) | settlement |
| 4 | Liquidation Engine | Full match | risk-engine |
| 5 | ADL | Full match | risk-engine |
| 6 | Insurance Fund | Full match | settlement |
| 7 | Index Price | Full match | index-price |
| 8 | Margin Calculation | Full match | risk-engine |
| 9 | Portfolio Margin | Match (simplified) | risk-engine |
| 10 | Perpetual Swap | Full match | settlement |
| 11 | TWAP | Full match | algo-execution |
| 12 | VWAP | Full match | algo-execution |
| 13 | Grid Trading | Full match | trading-bots |
| 14 | DCA | Full match | trading-bots |
| 15 | Black-Scholes | Full match | risk-engine |
| 16 | STP | Match (missing CancelBoth) | matching-engine |
| 17 | Proof of Reserves | Full match | proof-of-reserves |
| 18 | KYC/AML Risk Scoring | Full match | compliance |
| 19 | Travel Rule | Match (Phase 1) | compliance |
| 20 | Collateral Haircuts | Full match | risk-engine |
| 21 | Circuit Breaker | Full match | risk-engine |
| 22 | Socialized Loss | Full match | risk-engine |
| 23 | PnL Calculation | Full match | settlement |
| 24 | Fee Engine & VIP Tiers | Full match | account |
| 25 | Order Types | Full match | matching-engine + trigger-service |
| 26 | Copy Trading | Full match | copy-trading |
| 27 | Earn Products | Full match | api-gateway |
| 28 | Surveillance | Full match | surveillance |
| 29 | Custody | Match (Phase 1) | custody |
| 30 | WAL | Full match | journal |

---

## Key Bibliography

1. Almgren, R. & Chriss, N. (2000). "Optimal Execution." *J. Risk*, 3(2).
2. Black, F. & Scholes, M. (1973). "The Pricing of Options." *JPE*, 81(3).
3. Harris, L. (2003). *Trading and Exchanges*. Oxford.
4. Hayes, A. / BitMEX (2016). "A Perpetual Swap."
5. Hull, J. (2018). *Options, Futures, and Other Derivatives*. 10th ed.
6. Kleppmann, M. (2017). *Designing Data-Intensive Applications*. O'Reilly.
7. Mohan, C. et al. (1992). "ARIES." *ACM TODS*, 17(1).

Full bibliography in the legacy file: `docs/BUSINESS_LOGIC_REFERENCES.md` (appendix).
