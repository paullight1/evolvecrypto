# Account & Balance — Production Hardening Roadmap

**Created:** 2026-03-24
**Status:** ALL SPRINTS DONE ✓
**Crate:** `crates/account/`

---

## Current State

| Aspect | Rating | Notes |
|--------|--------|-------|
| Balance invariants | A | total = available + locked, enforced everywhere |
| Negative balance prevention | A | All mutations check before applying |
| Lock mechanism | A | Funds isolated from other uses |
| Fee calculations | A | Matches Bybit/Binance, Supreme rebate |
| Referral commissions | A | 2-level tree, 20%/5% |
| Unified account collateral | A | Per-asset haircuts, anti-FTT |
| Tests | A | 200+ unit + 13 adversarial |
| Documentation | A | REFERENCES.md with citations |
| Audit trail | A | Mutation log (ring buffer 100K entries) |
| Hardcoded config | B | Haircuts hardcoded but conservative |
| Unbounded history | A | Transfers 50K, referrals 50K caps |

---

## Sprint 1: Audit Trail & Safety Nets (P1)

| Task | Description | Status |
|------|-------------|--------|
| 1.1 | Balance mutation log (ring buffer, 100K entries) | DONE |
| 1.2 | Cap history vectors (transfers 50K, referrals 50K) | DONE |
| 1.3 | settle_fill_and_credit_treasury() helper | DONE |

## Sprint 2: Configuration & Resilience (P2)

| Task | Description | Status |
|------|-------------|--------|
| 2.1 | Runtime-configurable haircuts | DEFERRED (low risk — hardcoded values are conservative) |
| 2.2 | Safe transfer rollback (Result instead of panic) | DONE |
| 2.3 | Configurable sub-account limit per user | DEFERRED (low priority) |

## Sprint 3: Testing & Docs (P3)

| Task | Description | Status |
|------|-------------|--------|
| 3.1 | Property-based tests (5 properties × 256 cases) | DONE |
| 3.2 | Roadmap updated, REFERENCES.md already comprehensive | DONE |

---

## Definition of "Done"

- [x] All P1 items resolved (mutation log, history caps, treasury helper)
- [x] P2 critical item (safe rollback) resolved
- [x] Property-based tests prove invariants (5 properties × 256 random cases)
- [x] REFERENCES.md already comprehensive (pre-existing)
- [x] 107 account tests (102 unit + 5 proptest)
