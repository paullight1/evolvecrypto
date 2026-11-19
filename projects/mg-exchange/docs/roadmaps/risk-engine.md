# Risk Engine — Production Hardening Roadmap

**Created:** 2026-03-24
**Status:** ALL SPRINTS DONE ✓
**Crate:** `crates/risk-engine/`

---

## Current State

| Aspect | Rating | Notes |
|--------|--------|-------|
| Margin formulas | A | Linear + inverse, tiered MMR, match Bybit |
| Liquidation | A | Partial + full, insurance fund, cascade prevention |
| ADL | A | Profit × leverage ranking, quantile 1-5 |
| Position lifecycle | A | Open/increase/reduce/close/flip |
| Mark price | A | EMA dampening added (30s span) |
| Portfolio margin | A- | SPAN-like, needs cross-hedge tests |
| Options | A | Black-Scholes + Bybit-style margin |
| Tests | A | 180 tests: unit + proptest + stress + inverse |
| Documentation | A | REFERENCES.md comprehensive |

---

## Sprint 1: Test Gaps + Mark Price (P1)

| Task | Description | Status |
|------|-------------|--------|
| 1.1 | Inverse contract liquidation tests (5 tests) | DONE |
| 1.2 | Cross-hedge portfolio margin tests (3 tests) | DONE |
| 1.3 | Mark price EMA dampening (30s EMA of basis) | DONE |

## Sprint 2: Robustness (P2)

| Task | Description | Status |
|------|-------------|--------|
| 2.1 | Property-based tests (5 properties × 256 cases) | DONE |
| 2.2 | 1000-position liquidation stress test (+ batch limits) | DONE |
| 2.3 | Roadmap updated | DONE |

---

## Definition of "Done"

- [x] Inverse liquidation proven correct (5 tests)
- [x] Cross-hedge portfolio margin tested (3 tests)
- [x] Mark price EMA implemented (calculate_mark_price_ema, 4 tests)
- [x] Property-based tests for margin invariants (5 properties)
- [x] 1000-position stress test passes
- [x] **180 risk engine tests total** (was 130+)
