# Matching Engine — Production Hardening Roadmap

**Created:** 2026-03-24
**Status:** ALL SPRINTS DONE ✓
**Crate:** `crates/matching-engine/`

---

## Current State

| Aspect | Rating | Notes |
|--------|--------|-------|
| Matching algorithm | A | Price-time FIFO, fill at maker's price, correct |
| Order types | A | Limit/Market + GTC/IOC/FOK/PostOnly/GTD |
| Threading | A | Per-symbol OS threads, zero locks in hot path |
| Iceberg orders | B+ | Working but replenishment failure handling weak |
| Circuit breakers | A | Per-symbol price bands + halt mechanism |
| Self-trade prevention | A | Pre-match check + in-sweep STP (cancel self-matching maker) |
| Input validation | A | qty>0, price>=0, tick/lot, max depth, max qty, per-user limit |
| Rate limiting | A | 50/sec, 1200/min per user, sliding window |
| Testing | A | 109 tests: 102 unit + 6 proptest + 1 stress (100K) |
| Documentation | A- | REFERENCES.md with academic citations |

---

## Sprint 1: Input Validation Hardening (P0)

| Task | Description | Status |
|------|-------------|--------|
| 1.1 | Engine-level defensive validation (qty>0, price>=0) | DONE |
| 1.2 | Max book depth limit (100K orders/symbol) | DONE |
| 1.3 | Rate limiting (50 orders/sec, 1200/min per user) | DONE |
| 1.4 | Max qty / max notional per order | DONE |
| 1.5 | Atomic reduce-only check at fill time | DONE |

**Acceptance:** All P0 issues blocked with tests proving rejection.

## Sprint 2: Matching Quality (P1)

| Task | Description | Status |
|------|-------------|--------|
| 2.1 | O(1) cancel via HashMap index + Vec | DONE |
| 2.2 | STP during multi-fill sweep (cancel self-matching maker) | DONE |
| 2.3 | Enforce tick/lot size in engine (defense-in-depth) | DONE |
| 2.4 | Max order qty validation in engine | DONE |
| 2.5 | Iceberg failure recovery (ERROR log + cleanup) | DONE |

**Acceptance:** All P1 exploits blocked, cancel latency O(1).

## Sprint 3: Testing Depth (P2)

| Task | Description | Status |
|------|-------------|--------|
| 3.1 | Property-based tests (proptest, 6 properties × 256 cases) | DONE |
| 3.2 | 100K stress test (114K ops/sec, 75K fills, 0 errors) | DONE |
| 3.3 | Edge case test battery (12 scenarios) | DONE |
| 3.4 | Per-user order limit (200/user, tracked + decremented) | DONE |
| 3.5 | CancelRejected event for non-existent cancel | DONE |

**Acceptance:** 100+ tests, stress test clean, property tests prove invariants.

## Sprint 4: Performance & Documentation (P3)

| Task | Description | Status |
|------|-------------|--------|
| 4.1 | Benchmark baseline (BENCHMARKS.md) — 450K orders/sec | DONE |
| 4.2 | Performance regression CI | DEFERRED (needs CI pipeline) |
| 4.3 | REFERENCES.md (4 new sections: validation, rate limit, STP, cancel reject) | DONE |
| 4.4 | Call auction integration | DEFERRED (low priority) |

**Acceptance:** Benchmarks documented, REFERENCES.md complete.

---

## Definition of "Done"

- [x] All P0 items resolved with tests (Sprint 1: 5/5)
- [x] All P1 items resolved with tests (Sprint 2: 5/5)
- [x] Property-based tests cover matching invariants (6 properties × 256 cases)
- [x] 100K order stress test passes clean (114K ops/sec, 0 errors)
- [x] Benchmark baseline documented (BENCHMARKS.md: 450K orders/sec)
- [x] REFERENCES.md covers all mechanics (7 sections, academic citations)
- [x] 109 tests in `cargo test -p exchange-matching-engine`
- [x] Zero unsafe code in hot path
