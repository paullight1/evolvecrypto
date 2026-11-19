# Task Tracker

Current work-in-progress and completed items. Updated each session.

**Last updated:** 2026-03-24 (Phase A+D+E complete)

---

## Active: M16 — Web Terminal + Backend Hardening

### Context
Agents performed a major restructuring: deleted old monolithic web client (~40K lines), created new feature-sliced architecture (~33 features), and made Rust backend improvements. Need to validate everything, fix broken tests, and bring the web client to exchange-grade quality.

### Rust Backend

| # | Task | Status | Notes |
|---|------|--------|-------|
| R1 | `unpaid_funding_debt` on Position | DONE | Reduces effective margin, triggers earlier liquidation. Industry standard. |
| R2 | `YIELD_POOL_ACCOUNT_ID` for earn/staking | DONE | Payouts capped to pool balance. Fed by margin interest. |
| R3 | Production safety checks in main.rs | DONE | Panics if PRODUCTION=true without DATABASE_URL/JWT_SECRET/WEBHOOK_SECRET. |
| R4 | Fix `bug_fixes.rs` compilation (7 errors) | DONE | Added 3 methods: `FeeEngine::user_tiers()`, `FeeEngine::with_custom_tiers()`, `AdvancedBot::live_order_ids()`. |
| R5 | Verify all existing tests still pass | DONE | 1690 tests pass, 0 failures. |
| R6 | Fix Bug 2: referral commissions not distributed on fill | DONE | `distribute_affiliate_commissions` was defined but never called. Added inline referral commission in fill path. |
| R7 | Fix doctest in ws.rs | DONE | URL example was compiled as Rust code. Changed to `text` block. |

### Web Client

| # | Task | Status | Notes |
|---|------|--------|-------|
| W1 | Feature-sliced architecture setup | DONE | app/, features/, shared/ structure. 33 feature modules. |
| W2 | Shared UI library (14 components) | DONE | Button, Modal, Table, Toast, Spinner, Badge, etc. All tested. |
| W3 | API client with auto token refresh | DONE | client.ts with 401 → refresh → retry flow. |
| W4 | WebSocket with auto-reconnect | DONE | Exponential backoff, topic subscription tracking, ping keepalive. |
| W5 | Trading terminal layout | DONE | TradingTerminal.tsx with OrderBook, OrderEntry, Chart, BottomPanel. |
| W6 | TypeScript compilation | DONE | `tsc --noEmit` passes clean. |
| W7 | Unit tests | DONE | 161 files, 1396 tests pass. |
| W8 | Audit page quality vs Bybit/Binance | DONE | 28/33 REAL, 5 PARTIAL, 2 STUB (FeesPage, HelpPage). Much better than expected. |
| W9 | Trading terminal UX review | DONE | Pro-trader audit complete: 5/10 score. 3 IMMEDIATE dealbreakers, 10+ HIGH gaps. See below. |
| W10 | Missing order types in OrderEntry | DONE | All 5 types (limit, market, stop-limit, stop-market, trailing) + IOC/FOK/PostOnly TIF + TP/SL bracket. OCO via bracket TP+SL legs. |
| W11 | E2E tests with Playwright | DONE | 6 spec files, 78 tests (73 pass without backend, 2 need backend: Admin/API Docs, 4 visual need live server). History API routing, all pages, auth, trading, responsive. |

### IMMEDIATE Fixes (Financial Risk)

| # | Task | Status | Notes |
|---|------|--------|-------|
| F1 | Fix TP/SL orphan bug | DONE | Now passes `take_profit`/`stop_loss` on the main order. Backend creates linked triggers that auto-cancel on position close. |
| F2 | Fix market order % qty buttons | DONE | Uses bestAsk (buy) / bestBid (sell) from orderbook store for market orders instead of empty price field. Also wired bestBid/bestAsk from store so they work even without props. |
| F3 | Fetch pair list from backend API | DONE | Added `GET /api/v1/markets` endpoint. PairList fetches from API, sorts by volume/price/change. No more hardcoded arrays. |

### HIGH Priority (Week 1 — professionals leave without these)

| # | Task | Status | Notes |
|---|------|--------|-------|
| H1 | Add trailing stop to OrderEntry | DONE | Trailing Stop tab with delta input ($ or %), wired to API. |
| H2 | Add IOC/FOK/PostOnly time-in-force selector | DONE | Added TIF row (GTC/IOC/FOK/Post Only) for limit orders. Wired to API. |
| H3 | Add partial close to positions table | DONE | 25/50/75/100% buttons per position row. Computes partial qty. |
| H4 | Add order amendment (modify price/qty) | DONE | `PUT /orders/{id}` wired. Price and qty cells are click-to-edit in Open Orders. |
| H5 | Chart indicators (EMA, RSI, MACD) | DONE | @mg-exchange/charts: 21 built-in indicators (SMA, EMA, RSI, MACD, Bollinger, VWAP, Ichimoku, SMC, etc.) |
| H6 | Chart drawing tools (trendline, fib, horizontal) | DONE | @mg-exchange/charts: 47 drawing tools (Fibonacci suite, Gann, Elliott, patterns, shapes, trading positions) |
| H7 | Add TP/SL + ADL columns to positions table | DONE | TP/SL columns + ADL 5-bar quantile indicator. tp_price/sl_price added to Position type. |
| H8 | Add stop_price column to Open Orders | DONE | Trigger price column between Price and Amount. Shows "—" for non-stop orders. |
| H9 | Add conditional orders tab in BottomPanel | DONE | "Conditional" tab with trigger type, trigger price, cancel button. API: GET/DELETE /trigger-orders. |
| H10 | Order history pagination + symbol filter | DONE | Load More button (50 per page), symbol filter input, filled qty column. |

### Documentation & Process

| # | Task | Status | Notes |
|---|------|--------|-------|
| D1 | CLAUDE.md — financial engineering rules | DONE | Provenance requirement, money flow integrity, precision rules. |
| D2 | CLAUDE.md — development principles | DONE | Plan first, verify after, reference implementations. |
| D3 | CLAUDE.md — web client section | DONE | Architecture, tech stack, file structure. |
| D4 | Task tracker (this file) | DONE | Persistent across sessions. |
| D5 | Verify BUSINESS_LOGIC_REFERENCES.md covers new mechanics | TODO | Check unpaid_funding_debt and yield pool are documented. |
| D6 | Split BUSINESS_LOGIC_REFERENCES into per-crate files | DONE | 15 crates × REFERENCES.md. Index at docs/references/README.md. Legacy monolith preserved. |
| D7 | CLAUDE.md — task tracking section | DONE | Points to docs/TASK_TRACKER.md. |
| D8 | Memory — financial provenance feedback | DONE | + verify API before tests feedback. |

---

## Completed Milestones

- **M3-M14**: ~130 tasks, ~230+ endpoints, 350+ api-gateway tests
- **M15**: Web client redesign (HyperLiquid/KuCoin inspired visual overhaul)
- **M16 partial**: Backend hardening (yield pool, funding debt, production checks), web architecture rewrite

---

## Next: Production Readiness (see docs/PRODUCTION_READINESS.md)

Full audit completed 2026-03-24. Result: 5 COMPLETE, 17 FUNCTIONAL, 5 PARTIAL, 1 STUB, 2 MISSING.

### Phase A — Safety Foundation (P0 blockers) ✓ COMPLETE
- [x] Index price health gate — `is_index_healthy()` blocks perpetual orders when index unavailable (production only), startup warning, admin GET /admin/index/health endpoint
- [x] Sanctions hardening — expanded OFAC/EU/UN default lists (22 names, 25 addresses), deposit address screening, production startup warning
- [x] Options short margin — new `options_margin.rs` with Bybit-style short call/put formulas (15 tests), `check_short_margin()` API
- [x] Cross-asset collateral — ALREADY DONE (CollateralConfig with per-asset haircuts: USDT=1.0, BTC=0.95, ETH=0.90, etc., wired into liquidation monitor)
- [x] Grid bot fill feedback — ALREADY DONE (exchange.rs:6488-6560 calls on_fill() on every fill)
- [x] Copy trading execution — ALREADY DONE (exchange.rs:5099-5184 propagate_copy_trade() submits copy orders)
- [x] Block perpetuals until healthy — included in index health gate above

### Phase B — Custody Integration
- [ ] Fireblocks or self-hosted custody
- [ ] Real deposit address generation
- [ ] Real withdrawal processing with blockchain confirmation
- [ ] On-chain proof of reserves attestation

### Phase C — Compliance Hardening
- [ ] OFAC/UN/EU sanctions list auto-loading
- [ ] Real Sumsub/Diia KYC webhook integration
- [ ] Travel Rule VASP integration

### Phase D — Market Quality ✓ COMPLETE
- [x] TWAP-smoothed funding rate premium index — PremiumBuffer ring buffer (480 samples, 8h window), spawn_premium_sampler background task (60s), funding settler uses TWAP when available
- [x] Volatility surface for options mark pricing — vol_surface field on Exchange, get_option_mark_price() with Black-Scholes, admin POST/GET /admin/options/vol-surface
- [x] FIX ExecutionReport broadcast (drop copy) — fix_execution_tx broadcast channel, ExchangeAdapter::subscribe_executions(), drop-copy writer task per connection
- [x] WAL futures state reconstruction — Command::PositionSnapshot variant, spawn_position_snapshotter (60s WAL writes), extract_latest_position_snapshot on replay, PositionManager::restore_position()

### Phase E — Missing Products ✓ COMPLETE (code portions)
- [x] Liquidity Mining — spawn_lp_reward_accrual() distributes fees pro-rata to LPs, fee routing from spot fills (20% → LP pools), withdraw includes pending_rewards
- [ ] Real staking protocol integration (Lido, etc.) — external dependency
- [x] Delivery futures settlement — ALREADY DONE (spawn_delivery_settlement at tasks.rs:4185)
- [x] TWAP/VWAP adversarial tests — 15 new tests: rounding, price limits, cancel mid-exec, skewed profiles, zero qty edge cases
- [x] Spot ledger reconciliation — fills_processed/fills_ledger_written counters, spawn_ledger_reconciliation() detects persistent deltas

### Other
- [x] Chart library integration — @mg-exchange/charts v0.1.1 (21 indicators, 47 drawing tools, WebGL, trading overlay)
- [ ] E2E tests (Playwright)
- [ ] Deployment pipeline (Docker, CI/CD)
- [ ] Developer documentation (API reference, architecture guide)
- [ ] User documentation (trading guides, FAQ per product)

---

## Production Hardening Roadmaps

Deep per-subsystem hardening — audit, fix, test, document, close.

### Matching Engine (docs/roadmaps/matching-engine.md) — DONE ✓
- Sprint 1 ✓: Input validation (qty, price, max depth, rate limit, reduce-only)
- Sprint 2 ✓: O(1) cancel, STP sweep, tick/lot/max qty in engine, iceberg recovery
- Sprint 3 ✓: proptest (6 properties), 100K stress (114K ops/sec), 12 edge cases, per-user limit, CancelRejected
- Sprint 4 ✓: BENCHMARKS.md (450K orders/sec), REFERENCES.md (7 sections)
- **Result:** 109 ME tests (was 66), scorecard all A's

### Account & Balance (docs/roadmaps/account-balance.md) — DONE ✓
- Sprint 1 ✓: Mutation audit log (ring buffer 100K), history caps (50K), treasury credit helper
- Sprint 2 ✓: Safe transfer rollback (no panic)
- Sprint 3 ✓: proptest (5 properties), roadmap documented
- **Result:** 107 tests, scorecard mostly A's, was already strong
### Risk Engine (docs/roadmaps/risk-engine.md) — DONE ✓
- Sprint 1 ✓: 5 inverse liquidation tests, 3 cross-hedge portfolio tests, mark price EMA (30s dampening)
- Sprint 2 ✓: proptest (5 properties), 1000-position stress test, batch limits
- **Result:** 180 tests (was 130+), all formulas match Bybit, scorecard all A's
### Settlement (docs/roadmaps/settlement.md) — DONE ✓
- Zero-sum funding verification, inverse funding test, full insurance waterfall
- Fixed: adversarial_tests + insurance_staking modules now compiled and running
- **Result:** 96 tests (was 69 — 27 were orphaned!), REFERENCES.md 410 lines, all A's
### Order Gateway (docs/roadmaps/order-gateway.md) — DONE ✓
- REFERENCES.md created (validation, STP, rate limiting with Binance/Bybit/CME citations)
- 4 proptest properties for validator invariants
- **Result:** 41 tests, all A's
### Compliance (docs/roadmaps/compliance.md) — DONE ✓
- 17 new tests for 5 previously untested modules (EDD, risk scoring, complaints, SAR, risk warnings)
- Jurisdiction enforcement deferred (requires REST handler wiring)
- External integrations (OFAC feed, Chainalysis, TRISA) → Phase 2
- **Result:** 156 tests total, core mechanisms solid, stubs documented
### Custody (docs/roadmaps/custody.md) — AUDIT DONE ✓
- 68 tests already, circuit breaker A, whitelist A, wallet management A
- Fireblocks client ready (feature-gated), REFERENCES.md exists
- **Phase 2 needed:** Real blockchain integration, Fireblocks webhook, auto-approve
### Options (docs/roadmaps/options.md) — AUDIT DONE ✓
- Pricing/Greeks/margin/instruments: A (70+ tests)
- options_settlement.rs wired into settlement (was orphaned, +14 tests)
- **Prototype 70%:** needs order book, margin integration, expiry task for product launch

---

## Matching Engine Performance Sprint Plan

**Goal:** Bring `crates/matching-engine/` to best-in-class latency (Binance/Bybit tier) and prove it with benchmarks.
**Baseline (2026-04-13):** place_limit ~2.07 µs, match_1_fill ~2.12 µs, cancel ~324 ns, sweep_500 ~117 µs.
**Architecture constraint:** single-threaded per symbol — do NOT change threading model.
**Dependency note:** `rustc-hash` and `smallvec` already in Cargo.lock (pulled by other crates). `hdrhistogram` is NOT — Sprint 4 must add it to workspace Cargo.toml with justification comment.

---

### Sprint 1 — Measurement Infrastructure (Days 1-2)

**Goal:** Establish the measurement and safety net that all subsequent sprints depend on. No optimization ships without this.

| ID | Task | Acceptance Criteria |
|----|------|---------------------|
| T3.4 | Deep-book benchmark: new bench in `throughput.rs` building a 5000-level sparse book and measuring touch-top match cost | Bench exists, runs under `cargo bench`, baseline number recorded in `BENCHMARKS.md`. Expected pre-fix: ~20 µs. |
| T3.3 | `debug_assert!` invariants in `orderbook.rs` after every `match_order` call | Asserts: book not crossed (`best_ask >= best_bid`), per-level `total_qty` equals sum of order qtys, filled_qty sum consistent with match events. All pass in `cargo test`. |
| T3.2 | WAL replay determinism property test | proptest: given N random commands, two independent `MatchingEngine` instances emit byte-identical `Vec<MatchEvent>`. Added to existing proptest suite. `cargo test` passes. |
| T3.5 | Call auction benchmark in `market_maker.rs` | At least one bench covering `call_auction.rs` auction cycle. Baseline recorded. |

**Expected benchmark delta:** none (infrastructure only).
**Risk:** Low. Additive only. Tests may reveal pre-existing non-determinism in T3.2 (treat as blocker for Sprint 2 if found).
**Validation gate:**
- `systems-qa`: review T3.2 property test for completeness (are all command variants covered?), review T3.3 asserts for correctness.
- `developer`: review bench harness setup for measurement noise (warm-up iters, sample count).
**Exit criteria:** `cargo test` passes (75+ tests), `cargo bench` runs all four benches without panicking, `BENCHMARKS.md` updated with baseline numbers.

---

### Sprint 2 — Hot Path Correctness and Allocation Fixes (Days 3-5)

**Goal:** Fix the correctness bug (T1.5) and all in-loop allocations in `orderbook.rs`. These are local to one file and low-risk structurally.

| ID | Task | Acceptance Criteria |
|----|------|---------------------|
| T1.5 | FOK/STP correctness: `can_fill_fully` must exclude same-user orders from `total_qty` when STP is active | New test: FOK order against a book where only self-orders at top — must be rejected, NOT partial-filled. `cargo test` passes. Reviewed by `trading-qa` and `quant-reviewer`. |
| T1.1 | Replace `opposite_side.keys().cloned().collect()` Vec with lazy `BTreeMap::first_entry()`/`last_entry()` loop. **See Architectural Design Decisions section below for the complete borrow-safe Rust code pattern — use it drop-in.** Key constraint: `OccupiedEntry` holds `&mut self.asks/bids`; `self.order_locations.remove()` cannot be called inside the entry scope. Use a local `pending_location_removes: Vec<OrderId>` with `Vec::with_capacity(8)`, flush after `entry.remove()` drops the borrow. If disjoint-field borrow on `self.sequence`/`self.next_trade_id` fails to compile, extract them into a `MatchCounters` sub-struct as documented fallback. | `sweep_depth/500` bench improves. deep_book/5000 bench drops from 17.3 µs (corrected baseline — not 239 µs) to ≤ 500 ns. No regressions in `cargo test`. Integer test coverage: taker fully fills, partial fill + remainder rests, market sweep across N levels, FOK, IOC, PostOnly reject. |
| T1.2 | Remove `empty_prices` Vec; call `entry.remove()` inline in the T1.1 loop | Second allocation gone, confirmed by removing `Vec::new()` line. Covered by T1.1 bench improvement. |
| T1.3 | Replace `Box<dyn Iterator>` in `can_fill_fully` with enum-based iterator or duplicated loop | No heap alloc on FOK check path. Verified by code inspection (no `Box::new` on hot path). |
| T1.4 | Change `MatchEvent::OrderRejected.reason` field from `String` to `&'static str` or `Cow<'static, str>` | All construction sites updated (grep for `OrderRejected`). `cargo build --release` 0 new warnings. **Check WAL deserialization impact** — if `Cow<'static, str>` is chosen, bincode round-trip through WAL must remain safe (or use `&'static str` only for the matching-engine rejection path). |
| T2.4 | `insert_order` takes `order` by move (remove `order.clone()` at `orderbook.rs:137`) | `order.clone()` call removed. `cargo build --release` clean. |
| T2.3 | **REORDERED FROM S5.** Replace `HashMap<OrderId, ...>` with `rustc_hash::FxHashMap` in `orderbook.rs` | `rustc-hash` added to `exchange-matching-engine` Cargo.toml (already in lock). `cancel_order` bench improves from ~324 ns to ≤ 150 ns. `cargo test` clean. **Rationale for reorder:** one-line type swap, low risk, gives S3 baseline a cleaner starting point. |

**Expected benchmark delta:** `sweep_depth/500` from ~117 µs to ≤ 60 µs. `match_order_1_fill` from ~2.12 µs to ≤ 1.5 µs. `deep_book/5000` from **17.3 µs** (corrected after S1 methodology fix) to ≤ 500 ns. `cancel_order` from 324 ns to ≤ 150 ns.
**Risk:** Medium. T1.5 is a correctness fix touching FOK logic — wrong fix could cause all FOK orders to be rejected. T1.1 loop restructuring must preserve fill-at-maker-price and partial-fill semantics exactly.
**Validation gate:**
- `trading-qa`: review T1.5 STP/FOK fix and T1.1 matching loop for correctness (fill price, partial fill semantics, taker remainder cancel).
- `quant-reviewer`: review T1.5 specifically — verify FOK contract is preserved under all STP mode combinations.
- `security-auditor`: review T1.1/T1.3 for DoS vectors (can a malicious deep book now be traversed more cheaply by attacker, enabling new DoS pattern?).
- `developer`: review T1.4 Cow/static str change for lifetime correctness across all call sites.
**Exit criteria:** `cargo test` passes (all 75+ tests including new T1.5 test), benchmarks show improvement as stated above, `cargo clippy -- -D warnings` clean.

---

### Sprint 3 — Top-of-Book Cache (Days 6-7)

**Goal:** Add `best_bid` / `best_ask` O(1) cache to `OrderBook`. This requires Sprint 1 invariants (T3.3) to be live and Sprint 2 restructuring to be stable first.

| ID | Task | Acceptance Criteria |
|----|------|---------------------|
| T1.6-pre | **NEW (from architect review).** Audit `.symbol.0` direct field accesses in `index-price/feeds/binance.rs:198`, `kraken.rs:214`, `coinbase.rs:231,245`. These access the inner String directly and WILL silently break under S5 T2.1a Arc<str> change. Replace with `.symbol.as_str()` NOW as prerequisite. | grep `\.symbol\.0` returns only `common/src/types.rs` (struct field access inside impl). All 4 index-price sites converted to `.as_str()`. Tests pass. |
| T1.6a | Add `best_bid: Option<Price>` and `best_ask: Option<Price>` fields to `OrderBook`. Use wrapper helper methods `update_best_bid_cache()` / `update_best_ask_cache()` (single O(1) BTreeMap `last_key_value()` / `first_key_value()` call) rather than inline field updates at every site. Rationale from architect review: fewer conditional branches = fewer places cache can go stale. | Fields exist, initialized via `update_*_cache` in `OrderBook::new()`. Helper methods defined. |
| T1.6b | **Exhaustive cache update paths (from architect 9-row table):** call `update_best_bid_cache` / `update_best_ask_cache` at EVERY mutation site:<br>(1) `orderbook.rs:323-328` `insert_order` — after entry push<br>(2) `orderbook.rs:171-177` `cancel_order` — after level remove if cancelled price == best<br>(3) `orderbook.rs:289` `match_order` — after each `entry.remove()` in T1.1 loop<br>(4) any `reduce_qty` on best level if cache stores qty (decision: store price ONLY, not qty)<br>**Confirmed NO action needed:** iceberg.rs (no direct book mutations, replenishment flows through insert_order which covers #1), call_auction.rs (operates on local Vec, never touches OrderBook state), cancel_order silent-ignore branch at orderbook.rs:163 (no mutation). | All 3 active mutation sites call the helper. Iceberg + call_auction explicit NOP confirmed by code inspection. |
| T1.6c | Property test covering all edge cases | proptest: new level becomes best, best level drained by match, cancel of sole order on best, cancel of non-best order, multiple price levels ordered correctly, empty book → first insert → cache match, iceberg replenishment path. All T3.3 debug_asserts pass within same test. ≥10K iterations. |
| T1.6d | Replace `BTreeMap::first_key_value()` / `last_key_value()` call sites for best-bid/ask queries with cache reads. `would_match` and public `best_bid()`/`best_ask()` accessors. | `best_bid()` and `best_ask()` return `O(1)` (Option<Price> copy). No BTreeMap traversal on the hot query path. |

**Expected benchmark delta:** `place_limit` (non-crossing GTC) from ~2.07 µs to ≤ 1.4 µs. `match_order_1_fill` from ≤ 1.5 µs (Sprint 2 result) to ≤ 1.0 µs.
**Risk:** High. Cache invalidation bugs are silent — they cause crossed books or missed fills, which means financial loss. The T3.3 invariants and T1.6c property test are the primary safety net. Any test failure here is a hard blocker.
**Validation gate:**
- `systems-qa`: review T1.6b mutation path coverage — specifically cancel-of-last-order-on-best-level (most common source of stale cache bugs). Review property test for completeness.
- `architect`: review whether top-of-book cache belongs on `OrderBook` or on a wrapper struct. One-time structural decision before implementation.
- `developer`: review for all construction sites of `OrderBook` initializing the new fields.
**Exit criteria:** `cargo test` passes, T1.6c property test runs ≥ 10K iterations without failure, T3.3 asserts never fire in any test run, benchmarks show improvement as stated, `cargo clippy -- -D warnings` clean.

---

### Sprint 4 — Latency Histograms and Fast Paths (Days 8-9)

**Goal:** Add tail latency observability and compiler hints for the non-crossing common case.

| ID | Task | Acceptance Criteria |
|----|------|---------------------|
| T3.1 | Add `hdrhistogram` to workspace `Cargo.toml` (with provenance comment: HdrHistogram by Gil Tene, standard for latency measurement, used by Aeron/Chronicle). Add per-engine p50/p90/p99/p99.9/p99.99 tracking sampled on every order event. Export via existing Prometheus metrics path. | `cargo build --release` clean. Metrics endpoint exposes `matching_engine_latency_ns{quantile="0.99"}` etc. `hdrhistogram` version pinned in workspace. |
| T4.1 | Non-crossing GTC fast path: early check `if !would_match(order)` redirects to insert-only path, skipping `match_order` entirely | New bench `place_limit_nocross` added to `throughput.rs`. Latency ≤ 800 ns (vs ~2.07 µs baseline). Verified that a crossing order still goes through full match path (test). |
| T4.2 | `#[cold]` on rejection helper functions (post-only reject, FOK reject, invalid order reject) | Annotations added. `cargo build --release` clean. No functional change — verified by existing tests. |

**Expected benchmark delta:** `place_limit_nocross` ≤ 800 ns (new bench). p99 latency now measurable and tracked. No regression on other benches.
**Risk:** Low-Medium. T3.1 adds a new workspace dependency (needs justification, see provenance comment requirement). T4.1 fast path must not skip the fast path for IOC, FOK, or Post-Only orders — correctness check required.
**Validation gate:**
- `trading-qa`: verify T4.1 fast path correctly identifies ALL order types that cannot match (Post-Only is non-crossing by definition; IOC/FOK must still go through match path even if book appears empty at time of check due to race window — not applicable here since single-threaded, but the logic must be explicit).
- `developer`: review hdrhistogram integration for correct recorder/histogram lifecycle (reset cadence, overflow handling).
- `security-auditor`: review T3.1 metrics endpoint — latency histograms should not leak order volume or book depth information exploitable by HFT information asymmetry.
**Exit criteria:** `cargo test` passes, `cargo build --release` 0 warnings, metrics endpoint includes latency quantiles, `place_limit_nocross` bench ≤ 800 ns.

---

### Sprint 5 — Memory and Allocation Optimization (Days 10-13)

**Goal:** Eliminate per-call heap allocations in hot path. T2.1 (Symbol interning) is the invasive 24-crate change and must be last within this sprint.

| ID | Task | Acceptance Criteria |
|----|------|---------------------|
| ~~T2.3~~ | **MOVED TO SPRINT 2.** See S2. | — |
| T2.2 | Pass reusable `&mut Vec<MatchEvent>` into `place_order` / `cancel_order` from `engine_thread`, cleared between calls | No `Vec::new()` allocation in `orderbook.rs` on hot path. Alternatively `SmallVec<[MatchEvent; 4]>` — decision at implementation time based on profiling. `cargo build` clean, no API breakage outside matching-engine crate. |
| T2.1a (Stage 1) | **SPIKE — Arc<str> only.** Minimum change: `common/src/types.rs:25` struct Symbol(pub Arc<str>) + `impl Symbol::new` using `impl Into<Arc<str>>`. Fix 4 known silent-break sites: `index-price/feeds/binance.rs:198`, `kraken.rs:214`, `coinbase.rs:231,245` (already done in S3 T1.6-pre as prerequisite). Serde round-trip MUST remain byte-identical to current String representation (verify via WAL replay proptest re-run). | `cargo test --workspace` passes. WAL replay proptest (`prop_wal_replay_determinism`) passes — validates bincode round-trip compatibility. `throughput` bench shows ≥2% improvement on fill-heavy path OR profiler shows >3% CPU in Symbol hashing (justifies Stage 2 go). `cargo clippy -- -D warnings` clean across workspace. |
| T2.1a-gate | **GO/NO-GO decision** after T2.1a benchmarks. | GO to T2.1b only if: (1) throughput improvement <5% (marginal, suggesting more work needed), AND (2) profiler flags Symbol hash as >3% CPU, AND (3) user explicitly approves SymbolId migration effort. Otherwise STOP here — T2.1b moves to backlog. |
| T2.1b (Stage 2) | **BACKLOG / conditional on T2.1a-gate.** SymbolId(u16) + registry in `common` crate. WAL breaking change requires migration tool. Full separate spike with its own go/no-go. **Do not bundle into S5.** Move to PRODUCTION_ROADMAP.md or separate matching-engine-v2 initiative. | Deferred until justified. |

**Expected benchmark delta:** `sweep_depth/500` from ≤ 60 µs (Sprint 2) to ≤ 40 µs (T2.1a removes some String allocs). `place_limit` further improvement from T2.2 event buffer reuse. Note: cancel_order improvement from T2.3 already captured in S2.
**Risk:** Medium (was High, de-risked by staging). T2.1a Arc<str> is minimal-touch; silent-break sites enumerated and fixed in S3 as prerequisite. T2.1b is deferred entirely — removes the "SymbolId through 24 crates in one sprint" risk.
**Validation gate:**
- `architect`: review T2.1a Arc<str> impl in `common/src/types.rs` — specifically verify serde/bincode representation is unchanged. This is the core safety claim.
- `developer`: review T2.2 event buffer reuse for API ergonomics — does passing `&mut Vec` through engine_thread make the call interface awkward? Is SmallVec cleaner?
- `systems-qa`: **re-run WAL replay proptest** after T2.1a merge — this is the regression gate for serde/bincode format stability.
**Exit criteria:** `cargo test --workspace` passes, `cargo build --release` 0 warnings across workspace, benchmarks show improvement as stated, T2.1a-gate decision documented in commit message.

---

### Sprint 6 — Regression Gate and Benchmark Report (Day 14)

**Goal:** Prove the cumulative improvement with a side-by-side comparison and close the sprint plan.

| ID | Task | Acceptance Criteria |
|----|------|---------------------|
| R1 | Run full benchmark suite on same hardware. Record before/after table in `crates/matching-engine/BENCHMARKS.md`. | All 6 benches (place_limit, place_limit_nocross, match_1_fill, cancel, sweep_500, deep_book_5k, call_auction) documented with baseline and final numbers. |
| R2 | Run full test suite: `cargo test --workspace`. | 0 failures, 0 new warnings. Count must be ≥ 75 matching-engine tests (was 68+6 proptest+1 stress = 75). |
| R3 | Comparison against published exchange claims. | `BENCHMARKS.md` includes reference: Binance claims ~100K orders/sec throughput (not latency); Bybit claims sub-millisecond order processing. Our p99 latency from T3.1 histograms documented here. |
| R4 | Update `docs/TASK_TRACKER.md` status for this sprint plan to DONE. | This section marked complete with final benchmark numbers. |

**Expected benchmark delta:** Final targets vs baseline (**corrected after S1 methodology fix**):
- `place_limit`: 2.07 µs → ≤ 1.0 µs (2x improvement)
- `place_limit_nocross` (new): ≤ 800 ns
- `match_order_1_fill`: 2.12 µs → ≤ 1.0 µs (2x improvement)
- `cancel_order`: 324 ns → ≤ 150 ns (2x improvement)
- `sweep_depth/500`: 117 µs → ≤ 30 µs (4x improvement)
- `deep_book/5000` (new): **17.3 µs** → ≤ 500 ns (~35x improvement; NOT 239 µs as originally planned — 239 was teardown-inflated, corrected methodology fix in S1 reduced baseline to 17.3 µs which reflects actual T1.1 bug cost)

**Measurement Scope caveat required in BENCHMARKS.md:** Do NOT present numbers as directly comparable to Binance/Bybit published figures. Those include full pipeline (network, risk, settlement) under production load. Our benches are in-memory microbenchmarks. Use verbatim text from architect-designed Measurement Scope section in BENCHMARKS.md header.

**Risk:** Low. Measurement and reporting only. Risk is benchmark variance on different hardware runs.
**Validation gate:**
- `architect`: final review of `BENCHMARKS.md` — are the claims defensible against Binance/Bybit published numbers? Are caveats (single-threaded, in-memory, no network) documented?
- `systems-qa`: final regression check — any flaky tests introduced across the sprint series?
**Exit criteria:** All benchmarks recorded, all tests pass, `BENCHMARKS.md` updated, this task tracker section marked DONE.

---

### Sprint Plan Summary

| Sprint | Name | Days | Risk | Blocks |
|--------|------|------|------|--------|
| S1 | Measurement Infrastructure | 1-2 | Low | S2, S3 |
| S2 | Hot Path Correctness + Allocation Fixes + FxHashMap | 3-5 | Medium | S3 |
| S3 | Top-of-Book Cache | 6-7 | High | S4 |
| S4 | Latency Histograms + Fast Paths | 8-9 | Low-Med | S5 |
| S5 | Memory + Symbol Interning Stage 1 (Arc<str>) | 10-13 | Medium (de-risked via staging) | S6 |
| S6 | Regression Gate + Report | 14 | Low | — |

**Status:** Sprint 1 DONE + fix-up pass (2026-04-13) — 76 tests pass. Sprint 2 DONE + fix-up pass (2026-04-13) — 83 tests, 0 warnings, T1.1 deep_book/5000 11.9 µs → ~563 ns (−95%), trading-qa/quant-reviewer/security-auditor all approved after fix-up. Awaiting user commit for Sprint 2.

**Status: ALL 6 SPRINTS COMPLETE (2026-04-14).** Matching engine optimization effort closed. See `crates/matching-engine/BENCHMARKS.md` for final numbers. Known gaps (KG-1 through KG-6) carry forward as separate backlog items for future sprints.

---

## Known Gaps (out-of-sprint backlog, tracked but not scheduled)

### KG-1 — STP enforcement missing in `match_order` for GTC/IOC/Market orders

**Severity:** HIGH (wash trading / regulatory exposure)
**Source:** trading-qa review of Sprint 2 (2026-04-13)
**Location:** `crates/matching-engine/src/orderbook.rs::match_order`
**Status:** Documented inline with explicit doc comment at match_order function header (Sprint 2 fix-up item #2). STP is currently enforced ONLY at the FOK pre-check path (`can_fill_fully`, T1.5 fix). For GTC, IOC, and Market orders with `stp_mode != None`, self-fills are currently possible — the matching loop does not compare `user_id` of taker vs maker.
**Fix scope:** Pass `(taker_user_id, stp_mode)` into `match_order`, add self-trade handling in the fill inner loop per the chosen `StpMode` semantics (ExpireTaker cancels the incoming order, ExpireMaker cancels the resting order, ExpireBoth cancels both). Requires new test coverage across all STP modes and TIF variants. Estimated 2-3 days.
**Why not scheduled:** out of scope for matching engine optimization sprints (S1-S6). Needs its own sprint with trading-qa ownership.

### KG-2 — Missing per-user open-orders cap in order-gateway validator

**Severity:** MEDIUM (DoS amplification)
**Source:** security-auditor review of Sprint 2 (2026-04-13)
**Location:** `crates/order-gateway/src/validator.rs` — validator has no `max_open_orders_per_user` / `max_orders_per_price_level` check (confirmed by grep).
**Problem:** The T1.5 FOK/STP filter in `can_fill_fully` runs in O(orders_at_level) when STP is active. A two-account attacker can stack 10K+ orders at one price level, then flood FOK orders from the victim account — each FOK check scans O(N) orders. Without an upstream cap the worst case is unbounded.
**Fix scope:** add `max_open_orders_per_user` (suggest default 500) and `max_orders_per_user_per_level` (suggest default 50) checks in `OrderValidator`. Enforce at REST order entry in `api-gateway/src/rest.rs`. Requires integration test.
**Why not scheduled:** touches order-gateway, not matching-engine. Should be tracked in Production Roadmap or a separate security-hardening sprint.
**Comment left in code:** `orderbook.rs:439-444` has a SECURITY NOTE pointing to this gap.

### KG-3 — Prometheus export path for engine latency histograms not wired ✓ CLOSED Sprint 11 (2026-04-13)

**Severity:** LOW (observability gap, not correctness)
**Source:** Sprint 4 T3.1 implementation (2026-04-13)
**Location:** `crates/matching-engine/src/engine_thread.rs::LatencySnapshot` (histogram data available); `crates/admin/src/metrics.rs` (Prometheus registry lives here).
**Problem:** `EngineHandle::latency_snapshot()` returns per-engine p50/p90/p99/p99.9/p99.99 data on demand, but there is no Prometheus exporter wiring. The metrics endpoint does NOT yet expose `matching_engine_latency_ns{quantile="0.99"}` etc.
**Fix scope:** In `api-gateway` (the layer that owns both the `EnginePool` and the metrics route), add a periodic task or on-demand collector that calls `engine_handle.latency_snapshot()` for each symbol and pushes quantile gauges to the Prometheus registry. The registry type and registration pattern in `admin/src/metrics.rs` must be consulted first to match the existing conventions. **MUST expose aggregate metrics only** (exchange-wide p99.99, max-of-symbols) — see KG-5 below.
**Why not scheduled:** Sprint 4 scope is histogram collection + query API only. Prometheus wiring requires understanding the existing metrics registry pattern (api-gateway dependency), which is outside matching-engine crate scope and belongs in a dedicated observability sprint or Production Roadmap task.

**Closed:** Sprint 11 (2026-04-13) — `spawn_latency_metrics_updater` added to `crates/api-gateway/src/tasks.rs`. Polls engine latency snapshots every 30 s, updates 6 aggregate gauges on `ExchangeMetrics` (KG-5 contract enforced: no per-symbol labels). Wired into `spawn_all_tasks`. See also KG-4 and KG-5.

### KG-4 — Latency histogram reset / rolling window missing ✓ CLOSED Sprint 11 (2026-04-13)

**Severity:** LOW (observability correctness)
**Source:** security-auditor review of Sprint 4 (2026-04-14)
**Location:** `crates/matching-engine/src/engine_thread.rs` — per-engine `hdrhistogram::Histogram<u64>` accumulates over entire engine lifetime. No reset mechanism.
**Problem:** Without reset, p99.99 converges to lifetime worst-case and becomes operationally misleading when debugging current regressions. A 50ms outlier observed weeks ago permanently poisons the p99.99 value for "is the engine slow right now?" queries.
**Fix scope:** add either (a) `reset_latency()` method on `EngineHandle` triggered periodically (e.g. every 5 minutes) OR (b) rolling window via `Histogram::subtract` with double-buffer swap at fixed intervals. Option (b) is the standard Aeron/Chronicle pattern. Must be implemented BEFORE KG-3 Prometheus wiring goes live in production.
**Why not scheduled:** low priority until KG-3 wiring is scheduled.

**Closed:** Sprint 11 (2026-04-13) — `EngineCommand::ResetLatencyHistogram` variant added. `EngineHandle::reset_latency_histogram()` and `EnginePool::reset_all_latency_histograms()` added. `spawn_latency_metrics_updater` resets all histograms every 5 minutes via `reset_interval`. Design: single histogram + clear-on-reset (simpler than double-buffer, acceptable lag). 2 new tests: `test_reset_latency_histogram_clears_samples`, `test_reset_via_engine_pool`.

### KG-5 — Per-symbol latency metrics are an HFT asymmetry leak ✓ CLOSED Sprint 11 (2026-04-13)

**Severity:** MEDIUM (information leak to HFT participants)
**Source:** security-auditor review of Sprint 4 (2026-04-14)
**Problem:** Exposing per-symbol p99.99 latency publicly allows participants to infer engine congestion on specific symbols. In live markets this enables latency arbitrage: an observer seeing `BTCUSDT` engine p99.99 spike to 50 µs can infer a large sweep is in progress and front-run the resulting price impact before the L2 snapshot propagates. Standard HFT asymmetry concern, also relevant under regulations prohibiting unequal access.
**Mitigation contract:** when KG-3 (Prometheus wiring) ships, the public `/metrics` endpoint MUST expose ONLY aggregate metrics — exchange-wide percentiles or max-of-symbols across all engines. Per-symbol breakdowns stay internal, accessible only via authenticated admin endpoint.
**Why not scheduled:** soft constraint on KG-3 dependency — must be enforced when KG-3 is implemented, not a separate ticket.

**Closed:** Sprint 11 (2026-04-13) — `ExchangeMetrics` has 6 new `AtomicU64` fields (p50/p99/p99.9/p99.99/max/count). `to_prometheus()` emits them without any `{symbol=...}` label. `update_aggregate_latency_gauges()` in `tasks.rs` iterates pool symbols internally but only stores max-of-symbols values. Grep confirms: `grep -rn 'symbol=' crates/admin/src/metrics.rs crates/api-gateway/src/tasks.rs` finds zero matches in the new engine latency code.

### KG-6 — Incorrect doc comment on `EngineHandle::latency_snapshot()` ✓ CLOSED Sprint 11 (2026-04-13)

**Severity:** TRIVIAL (documentation accuracy)
**Source:** security-auditor review of Sprint 4 (2026-04-14)
**Location:** `crates/matching-engine/src/engine_thread.rs:~221`
**Problem:** The doc comment says "the clone is cheap (<1 µs) and happens on the engine thread." The implementation does NOT clone the histogram — it extracts five `value_at_quantile()` values directly on the live histogram (O(1) bucket lookups, ~100 ns total). The performance claim is correct but the description is mis-stated.
**Fix:** replace "clone" with "quantile extraction" in the doc comment. One-line fix. Can be bundled with any future touch to `engine_thread.rs`.

**Closed:** Sprint 11 (2026-04-13) — doc comment on `EngineHandle::latency_snapshot()` rewritten to say "extracts 5 quantile values via `value_at_quantile()` calls (O(1) bucket lookups, ~100 ns total — no histogram copy)" and documents the rolling-window reset behavior. File: `crates/matching-engine/src/engine_thread.rs`.

### KG-7 — Missing cross-symbol total open-orders cap ✓ CLOSED Sprint 10 (2026-04-14)

**Severity:** MEDIUM (memory exhaustion vector)
**Source:** security-auditor review of Sprint 8 (2026-04-14)
**Location:** `crates/order-gateway/src/validator.rs` — `OrderLimitConfig` has only per-symbol cap, no cross-symbol total.
**Problem:** With default `max_open_orders_per_user_per_symbol = 500` and N symbols registered (~20+ in current exchange), a single user can hold up to `500 × N = 10,000+` open orders. Memory per order ~200-300 bytes + HashMap index overhead. At 10,000 active users at max: 25-30 GB order state alone. Memory exhaustion vector on coordinated cross-symbol flood.
**Fix scope:** add `max_open_orders_per_user_total: usize` to `OrderLimitConfig` (default ~5000 = 10× per-symbol cap). Requires `OrderCountSource` trait to gain `user_total_open_order_count(user_id) -> usize` method backed by summing across EnginePool OR a separate atomic counter (one counter per user updated on every insert/cancel/fill across all symbols — cleaner but requires cross-crate coordination).
**Why not scheduled:** explicitly out-of-scope per Q2 decision for Sprint 8 (per-symbol only). File as separate security-hardening task.

**Closed:** Sprint 10 (2026-04-14) — `max_open_orders_per_user_total` added to `OrderLimitConfig` (default 5000). `OrderCountSource::user_total_open_order_count` trait method. `EnginePoolCountSource` implementation sums across all registered symbols at query time (O(N_symbols), ~50 µs for 20 symbols — acceptable for validation path). Check runs AFTER per-symbol cap in `validate_with_counts` Step 8a. Returns HTTP 429 `MaxOpenOrdersPerUserTotal`. 5 new tests (4 unit + 1 E2E).

### KG-8 — Single global `OrderLimitConfig` — no per-user VIP tiering ✓ CLOSED Sprint 12 (2026-04-14)

**Severity:** LOW (product feature gap)
**Source:** security-auditor review of Sprint 8 (2026-04-14)
**Closed:** Sprint 12 (2026-04-14) — `order_limit_overrides: HashMap<UserId, OrderLimitConfig>` added to `Exchange` struct (`crates/api-gateway/src/exchange.rs:989`). Public methods `set_user_order_limit_override`, `clear_user_order_limit_override`, `effective_order_limit_config` (lines 4346-4361). `place_order` consults `effective_order_limit_config(cmd.user_id)` at Step 0a1 (line 4489). MVP scope — admin REST endpoint for override management is a separate follow-up task. 2 new tests: `test_user_override_allows_higher_per_symbol_cap` (E2E) + `test_effective_config_returns_override_when_set` (unit).

### KG-9 — `"RATE_LIMIT: "` string prefix is fragile typed-error workaround ✓ CLOSED Sprint 12 (2026-04-14)

**Severity:** LOW (maintenance fragility, not security)
**Source:** security-auditor review of Sprint 8 (2026-04-14)
**Closed:** Sprint 12 (2026-04-14) — `rest.rs:699` now uses `e.downcast_ref::<ValidationError>()` to recover the typed error across the `anyhow::Result` boundary. The `is_rate_limit()` method is called on the typed variant directly to branch to HTTP 429. `exchange.rs` no longer wraps with `"RATE_LIMIT: "` prefix — `ValidationError` propagates as a native `anyhow::Error` via the blanket `From<E: std::error::Error>` impl. Zero fragility: typo-proof, locale-proof, future-proof for new ValidationError variants. Existing 429 tests continue to pass.

---

## All Known Gaps CLOSED (2026-04-14)

KG-1 through KG-9 are all closed. Sprint 1-12 complete:

| KG | Severity | Sprint | Status |
|---|---|---|---|
| KG-1 | HIGH | Sprint 7 | ✓ STP enforcement in match_order for all TIF variants (GTC/GTD/IOC/Market) |
| KG-2 | MEDIUM | Sprint 8 | ✓ max_open_orders_per_user_per_symbol + per_price_level caps in validator |
| KG-3 | LOW | Sprint 11 | ✓ Prometheus wiring for engine latency histograms (aggregate only per KG-5) |
| KG-4 | LOW | Sprint 11 | ✓ Histogram reset + 5-minute rolling window via background task |
| KG-5 | MEDIUM | Sprint 11 | ✓ Aggregate-only exposure contract enforced (no per-symbol labels) |
| KG-6 | TRIVIAL | Sprint 11 | ✓ Latency_snapshot doc comment fixed (quantile extraction, not clone) |
| KG-7 | MEDIUM | Sprint 10 | ✓ max_open_orders_per_user_total cross-symbol cap |
| KG-8 | LOW | Sprint 12 | ✓ Per-user OrderLimitConfig overrides for VIP tiering |
| KG-9 | LOW | Sprint 12 | ✓ Typed error via anyhow::downcast replaces "RATE_LIMIT: " prefix |

---

## Architectural Design Decisions (2026-04-13, from architect review)

### T1.1 match_order borrow-safe pattern (mandatory for Sprint 2)

**Problem:** `BTreeMap::first_entry()` / `last_entry()` return an `OccupiedEntry` that holds exclusive borrow on `self.asks`/`self.bids`. Cannot call `self.order_locations.remove()` while the entry is live. Cannot increment `self.sequence` while entry is live (disjoint-field borrow check may fail).

**Solution:** local `pending_location_removes: Vec<OrderId>` buffer, flush after `entry.remove()` drops the borrow. Fallback if disjoint-field borrow on `self.sequence` fails: extract `sequence + next_trade_id` into `MatchCounters` sub-struct.

**Pattern (drop-in for Sprint 2 T1.1):**
```rust
fn match_order(
    &mut self,
    taker: &mut Order,
    events: &mut Vec<MatchEvent>,
    timestamp: chrono::DateTime<chrono::Utc>,
) {
    let mut pending_location_removes: Vec<OrderId> = Vec::with_capacity(8);

    loop {
        if taker.is_fully_filled() { break; }

        let mut entry = match taker.side {
            Side::Buy => match self.asks.first_entry() {
                Some(e) => e,
                None => break,
            },
            Side::Sell => match self.bids.last_entry() {
                Some(e) => e,
                None => break,
            },
        };

        let price = *entry.key();
        let crosses = match taker.order_type {
            OrderType::Market => true,
            OrderType::Limit => match taker.side {
                Side::Buy  => taker.price >= price,
                Side::Sell => taker.price <= price,
            },
        };
        if !crosses { break; }

        let level = entry.get_mut();
        while !taker.is_fully_filled() && !level.is_empty() {
            let (fill_qty, fill_price, maker_order_id, maker_user_id) = {
                let maker = level.front().unwrap();
                (
                    taker.remaining_qty().min(maker.remaining_qty()),
                    maker.price,
                    maker.id,
                    maker.user_id,
                )
            };

            level.front_mut().unwrap().filled_qty += fill_qty;
            level.reduce_qty(fill_qty);
            taker.filled_qty += fill_qty;

            self.sequence += 1;  // may require MatchCounters extraction
            let trade_id = self.next_trade_id;
            self.next_trade_id += 1;

            events.push(MatchEvent::Fill(FillEvent {
                trade_id, symbol: self.symbol.clone(), price: fill_price,
                qty: fill_qty, maker_order_id, taker_order_id: taker.id,
                maker_user_id, taker_user_id: taker.user_id,
                taker_side: taker.side, timestamp, seq: self.sequence,
            }));

            let maker_done = level.front().map(|m| m.is_fully_filled()).unwrap_or(true);
            if maker_done {
                let removed = level.pop_front().unwrap();
                pending_location_removes.push(removed.id);
            }
        }

        if level.is_empty() { entry.remove(); }
        // entry dropped here — borrow released

        for id in pending_location_removes.drain(..) {
            self.order_locations.remove(&id);
        }
    }

    for id in pending_location_removes {
        self.order_locations.remove(&id);
    }

    if taker.is_fully_filled() { taker.status = OrderStatus::Filled; }
    else if taker.filled_qty > Decimal::ZERO { taker.status = OrderStatus::PartiallyFilled; }
}
```

### T1.6b cache mutation paths — full enumeration

| Site | Mutation | Cache action |
|---|---|---|
| `orderbook.rs:323-328` `insert_order` | new level or push to existing | `update_best_bid_cache` or `update_best_ask_cache` after push |
| `orderbook.rs:171-177` `cancel_order` found branch | level.remove + book_side.remove if empty | Update cache if cancelled price == current best |
| `orderbook.rs:289` `match_order` inline drain (new T1.1 code) | entry.remove after level empty | Update cache after `entry.remove()` + buffer flush |
| `orderbook.rs:163` `cancel_order` silent-ignore | no mutation | NOP |
| `iceberg.rs` replenishment | calls `place_order` → `insert_order` | Covered by row 1 — no extra code |
| `call_auction.rs` `run_auction` | operates on local Vec, never touches OrderBook | NOP |

**Decision:** use wrapper helpers `update_best_bid_cache(&mut self)` / `update_best_ask_cache(&mut self)` that each do a single `last_key_value()` / `first_key_value()` call and write to cache fields. Rationale: fewer conditional branches = fewer places cache can desync. O(log n) per mutation is negligible vs O(depth) bug we're eliminating in T1.1.

### T2.1 Symbol interning blast radius (for S5 planning reference)

**Known silent-break sites** (must fix before or during T2.1a):
- `index-price/feeds/binance.rs:198` — `.symbol.0` direct field access
- `index-price/feeds/kraken.rs:214` — same
- `index-price/feeds/coinbase.rs:231,245` — same

**Serde-critical sites** (verify byte-identical round-trip):
- `journal/src/wal.rs` + `segments.rs` — bincode of `PlaceOrderCommand` containing Symbol
- `common/src/types.rs` — derive(Serialize, Deserialize) — Arc<str> serializes identically to String under bincode and serde_json (verified by architect, gate on WAL replay proptest)

**HashMap key sites** (Hash/Eq must remain stable — Arc<str> preserves this via blanket impl):
- `matching-engine/src/engine.rs:13` — `HashMap<Symbol, OrderBook>`
- `risk-engine`, `surveillance`, `trigger-service`, `settlement` — various `HashMap<Symbol, ...>`

**Total surface:** ~1000+ `Symbol` occurrences across 110 files. Stage 1 (Arc<str>) keeps this manageable; Stage 2 (SymbolId) is deferred.

---

## Sprint 7+8 Plan — STP Enforcement & Order Limits (2026-04-14)

**Total estimated days:** 5–6 (S7: 3 days, S8: 2 days). No ordering dependency — S8 can run before or after S7.
**Recommended start:** S8 first. It is fully self-contained, lower risk, and closes a documented DoS vector without touching the hot match path. S7 carries higher complexity and needs its own trading-qa sign-off.

---

### Sprint 7 — STP Enforcement in `match_order` (KG-1)

**Duration:** 3 days
**Crate:** `crates/matching-engine`
**Risk:** MEDIUM — modifies the hot matching loop; incorrect branching silently produces wrong fills or wrong cancel semantics
**Regulatory exposure if skipped:** HIGH — wash trading is detectable on-chain and violates MiCA Article 89, FATF guidance on market manipulation

**References:**
- Bybit STP: https://www.bybit.com/en/help-center/article/Self-Trade-Prevention — defines ExpireTaker, ExpireMaker, ExpireBoth semantics and states partial-fill behavior under ExpireTaker
- Binance STP filter: https://developers.binance.com/docs/binance-spot-api-docs/filters#self-trade-prevention-stp — `EXPIRE_TAKER`, `EXPIRE_MAKER`, `EXPIRE_BOTH` modes; notes that a partially-filled taker under EXPIRE_TAKER retains its filled portion with status `PARTIALLY_FILLED` (not rolled back)
- Developer must read both pages and document which behavior we adopt where they differ

#### Non-goals (do NOT do these in S7)
- Do not change the `can_fill_fully` / FOK pre-check path — it already handles STP correctly and must not regress
- Do not add STP filtering at the REST layer or order-gateway — enforcement lives exclusively in `match_order`
- Do not implement per-account STP configuration via REST API — STP mode comes from `PlaceOrderCommand`, not account settings
- Do not implement a "cancel all resting self-orders at all deeper levels" sweep under ExpireBoth — only the front-of-queue maker at the first self-encounter is cancelled; taker then stops

#### Tasks

| ID | Task | Size | Depends | Acceptance Criteria |
|----|------|------|---------|---------------------|
| KG-1.1 | Propagate `stp_mode: StpMode` and `taker_user_id: UserId` into `match_order` signature | S | — | Compiles; existing 86 tests pass; all call sites of `match_order` updated |
| KG-1.2 | Implement STP branching in the inner fill loop for all four modes | M | KG-1.1 | No self-fill occurs in any test when stp_mode != None; see test matrix below |
| KG-1.3 | Add `OrderCancelled` event emission with STP reason tag | S | KG-1.2 | Cancel event emitted before any subsequent Fill events at same price level; sequence numbers monotonically increasing across Fill + Cancel events in same response Vec |
| KG-1.4 | Remove "STP SCOPE LIMITATION" doc comment from `match_order` header | S | KG-1.2 | Comment absent in final diff; a new accurate doc comment describing the four modes replaces it |
| KG-1.5 | Unit test matrix: 4 STP modes × 3 TIF (GTC/IOC/Market) × 3 book states (no self / all self / mixed) | M | KG-1.2 | Minimum 15 tests added; all pass; test names follow `stp_{mode}_{tif}_{scenario}` convention |
| KG-1.6 | Edge case tests: partial-fill-then-self for ExpireTaker; ExpireMaker drains level (all makers at price are same user); FOK + ExpireMaker interaction; ExpireBoth with prior partial fills | M | KG-1.3 | 5–6 targeted tests; each documents which Bybit/Binance doc section governs the expected outcome |
| KG-1.7 | Property test: randomised STP + TIF + book state, assert zero self-fills when stp_mode != None | M | KG-1.2 | Proptest runs 1000 cases; 0 self-fill events observed; added to `cargo test` suite |

**Partial-fill semantics decision (developer must document before coding KG-1.2):**
Under ExpireTaker, if the taker has already filled against non-self orders before hitting the self-order, Bybit docs state the taker's filled portion is kept and the taker is cancelled with status `PartiallyFilled`. The developer must confirm this is our intended behavior and add a comment citing the reference before implementing. If we diverge, the reason must be documented.

**FOK + ExpireMaker interaction (developer must resolve before coding KG-1.6):**
`can_fill_fully` (T1.5) performs a pre-check counting available liquidity including self-orders under ExpireMaker. If `can_fill_fully` passes but `match_order` later cancels a maker via ExpireMaker, the remaining fill quantity may be insufficient for FOK to complete. The developer must decide: (a) re-check fillability in `match_order` when ExpireMaker triggers, or (b) exclude self-orders from `can_fill_fully` counting when stp_mode = ExpireMaker. Option (b) is consistent with Binance's documented behavior. Whichever is chosen, add a comment in `can_fill_fully` and `match_order` cross-referencing each other.

#### Validation Gates (S7)
1. **trading-qa** — verify STP correctness matrix: each mode × each TIF produces the correct final order status (Filled / PartiallyFilled / Cancelled), correct event sequence, and correct book state post-match
2. **quant-reviewer** — confirm no financial contract is broken: filled trades are not reversed, maker price preservation holds, no phantom liquidity emitted in events
3. **architect** — confirm semantic alignment with Bybit/Binance reference docs; flag any divergence as explicit design decision
4. **systems-qa** — regression pass: all 86 pre-S7 tests still pass; no new warnings; FOK path unchanged

#### Rollback Plan (S7)
`match_order` signature change is the only public surface change within the crate. The old signature had no `stp_mode` / `taker_user_id` parameters — a revert is a clean `git revert` of the S7 commits. No database schema changes. No WAL format changes. The FOK path (`can_fill_fully`) is untouched, so revert does not regress Sprint 2 fixes.

---

### Sprint 8 — Per-User Open-Orders Cap (KG-2)

**Duration:** 2 days
**Crates:** `crates/order-gateway`, `crates/api-gateway`
**Risk:** LOW — additive validation only; no existing logic removed or altered; worst-case a misconfigured limit rejects valid orders (recoverable by config change, not a code revert)

**References:**
- Binance MAX_NUM_ORDERS filter: https://developers.binance.com/docs/binance-spot-api-docs/filters#max-num-orders — per-symbol cap (default 200 open orders)
- Binance MAX_NUM_ALGO_ORDERS filter: same page — separate cap for conditional orders
- Bybit open order limits: https://bybit-exchange.github.io/docs/v5/order/create-order — "Maximum number of open orders: 500 per symbol (linear/inverse)"
- OKX order limits: https://www.okx.com/docs-v5/en/#order-book-trading-trade-post-place-order — tiered by account level

**Our defaults (must be configurable via `OrderValidatorConfig`, not hardcoded):**
- `max_open_orders_per_user: usize = 500` (total across all symbols, matches Bybit's per-symbol default; developer should evaluate whether total-across-symbols or per-symbol is the correct scope and document the choice)
- `max_open_orders_per_user_per_symbol: usize = 200` (matches Binance MAX_NUM_ORDERS)
- `max_orders_per_user_per_price_level: usize = 50` (DoS protection for T1.5 O(N) STP scan)

#### Non-goals (do NOT do these in S8)
- Do not implement tiered limits based on user KYC level or trading volume — flat caps only in this sprint
- Do not add per-symbol configuration of limits — one global config applies to all symbols
- Do not add a REST endpoint to query a user's current open order count — that is a separate feature
- Do not modify the matching engine or order book — all enforcement is upstream in order-gateway
- Do not add per-algo-order-type caps (conditional, stop, TP) — flat open order count only

#### Design decision (developer must resolve before KG-2.1)
Determine where "current open order count per user" state lives. Three candidates:
- (a) Query the matching engine's `order_locations` map — requires a new read method on `OrderBook`/`EngineHandle`; consistent but adds latency on every placement
- (b) Maintain a counter in `OrderValidator` itself, kept in sync via post-placement callbacks — complex, risk of desync
- (c) Query the `account` crate's open order tracking if it already maintains this — check `crates/account/src/lib.rs` first

Preferred: option (c) if the state exists there; option (a) otherwise. Document the choice and the file:line where the source of truth lives.

#### Tasks

| ID | Task | Size | Depends | Acceptance Criteria |
|----|------|------|---------|---------------------|
| KG-2.1 | Audit `crates/account` and `crates/order-gateway` to locate existing open-order state; document the source of truth | S | — | Written comment in `validator.rs` citing the file:line of the authoritative counter |
| KG-2.2 | Add `max_open_orders_per_user`, `max_open_orders_per_user_per_symbol`, `max_orders_per_user_per_price_level` fields to `OrderValidatorConfig` with the defaults above | S | KG-2.1 | Fields present with documented defaults; `cargo test` passes |
| KG-2.3 | Add three new `ValidationError` variants: `TooManyOpenOrders`, `TooManyOpenOrdersForSymbol`, `TooManyOrdersAtPriceLevel` | S | KG-2.2 | Variants present; REST handler returns HTTP 429 with structured error body for each; error body includes the applicable limit value |
| KG-2.4 | Implement the three limit checks in `OrderValidator::validate()` using the authoritative counter from KG-2.1 | M | KG-2.3 | Checks fire before any balance lock or matching-engine call; existing validation order documented in a comment |
| KG-2.5 | Wire the checks into the REST order placement path in `api-gateway/src/rest.rs` | S | KG-2.4 | `place_order` returns 429 when any cap is exceeded; no order reaches the matching engine |
| KG-2.6 | Integration tests: place 200 orders on same symbol → 201st rejected with `TooManyOpenOrdersForSymbol`; place 50 orders at same price level → 51st rejected with `TooManyOrdersAtPriceLevel`; cancel one → next placement succeeds | M | KG-2.5 | 3 integration tests pass; tests run under `cargo test -p exchange-api-gateway` |

#### Validation Gates (S8)
1. **trading-qa** — confirm limit semantics match Bybit/Binance reference; confirm HTTP 429 (not 400) is the correct status code per exchange convention
2. **security-auditor** — confirm the per-price-level cap (50) is sufficient to bound T1.5 O(N) scan cost; review that counter read is not a TOCTOU race under concurrent placement
3. **systems-qa** — regression pass: all existing tests pass; the 500-order integration test does not time out; limits are enforced atomically (no order slips through under concurrent load)

#### Rollback Plan (S8)
All changes are additive (new config fields, new error variants, new validation calls). Revert is a clean `git revert`. If a config value is set too low and legitimate users are rejected, raise the limit via config change — no code revert needed. No schema migration. No WAL format change.

---

### Open Questions for User Input Before Developer Starts

1. **S7 — Partial-fill retention under ExpireTaker:** Do we keep the taker's already-filled portion when it is cancelled mid-sweep (Bybit/Binance behavior), or do we roll back the entire taker order? Rollback requires reversing balance mutations that have already occurred, which is significantly more complex. Recommendation: keep filled portion, matching Bybit. Confirm?

2. **S8 — Scope of `max_open_orders_per_user`:** Should the 500-order cap be total-across-all-symbols (simpler counter) or per-symbol (matches Bybit's stated "500 per symbol" limit, but harder to count)? The per-symbol interpretation is more permissive (500 × N symbols). Confirm which interpretation to implement.

3. **S8 — HTTP status code for limit rejection:** Should limit-exceeded orders return HTTP `429 Too Many Requests` or `400 Bad Request`? Bybit returns 10010 error code mapped to a 400. Binance returns a 429. Confirm our preference before KG-2.3 is coded.

---

## API Gateway Optimization Sprint Plan (S13-S22, 2026-04-14)

**Goal:** Reduce API gateway end-to-end latency from ~5-50 ms to sub-millisecond for the `place_order` hot path, and raise throughput cap from ~200 orders/sec to ≥ 2000 orders/sec.
**Baseline source:** Explore + architect audit completed 2026-04-14. Key findings: 16-22 Mutex acquisitions per order, WAL fsync on hot path, synchronous process_events, O(1200) rate-limiter scan.
**Architecture constraint:** Exchange is a single Rust binary (monolith Phase 1). No service splits in this plan.
**Ordering constraint (hard — user-approved):** S13 (measurement) → S14 (WAL) → S15 (events) → S16 (parking_lot) → S17 (ArcSwap) → S18 (sharding) → S19 (auth) → S20 (observability) → S21/S22 (optional institutional).

---

### Sprint S13 — Measurement Infrastructure (Days 1-3)

**Goal:** Establish per-stage latency breakdowns and concurrency benchmarks so every subsequent sprint can prove its claimed improvement with hard numbers.

**Duration:** 3 days
**Risk:** Low — additive only; no production logic touched
**Blocks:** All subsequent sprints. Nothing ships without this baseline.

| ID | Task | Size | Depends | Acceptance Criteria |
|----|------|------|---------|---------------------|
| G.1 | Add per-stage `Instant` checkpoints inside `place_order` with labeled Prometheus histograms: `auth`, `rate_limit`, `validation`, `balance_lock`, `wal_write`, `matching`, `process_events` | M | — | 7 histograms visible on `/metrics`; each fires on every order placement; `cargo test -p exchange-api-gateway` passes |
| G.2 | Create `crates/api-gateway/benches/api_bench.rs` REST-level benchmark using `tower::ServiceExt::oneshot` pattern (extend existing `bench_place_limit_order`); single-user baseline | S | — | Bench runs under `cargo bench -p exchange-api-gateway`; p50/p99 latency recorded in `crates/api-gateway/BENCHMARKS.md` (new file) |
| G.3 | Add concurrency stress benchmark: 10 concurrent users × 100 orders each via `tokio::spawn` fan-out, measure throughput (orders/sec) and p99 latency | M | G.2 | Bench exists; baseline throughput number recorded in `BENCHMARKS.md`; expected pre-fix: ~200 orders/sec cap visible under load |
| G.4 | Record baseline Mutex acquisition count (16-22) in `BENCHMARKS.md` by reading `exchange.rs` place_order path and annotating each lock site with a numbered comment `// LOCK-N` | S | — | Comment annotations present at each lock acquisition in `place_order`; total count verified in `BENCHMARKS.md` |
| G.5 | Add `Measurement Scope` section to `BENCHMARKS.md` documenting: in-process benchmarks only (no network), single-node, not directly comparable to Bybit/Binance published throughput figures; caveat that lock wait time histograms require `parking_lot` (available after S16) | S | G.2 | Section present; no overclaiming in any benchmark narrative |

**Expected benchmark delta:** None (infrastructure only). Establishes baselines.
**Validation gate:**
- `systems-qa`: review G.3 concurrency harness for measurement correctness (shared state between tokio tasks, clock drift, warm-up)
- `developer`: review G.1 instrumentation placement — ensure checkpoints do not move logic or change error handling
- `ops-manager`: confirm the 7 histogram label names match the existing metrics naming convention in `crates/admin/src/metrics.rs`

**Exit criteria:** `cargo test -p exchange-api-gateway` passes; `cargo bench -p exchange-api-gateway` runs without panic; `crates/api-gateway/BENCHMARKS.md` created with all baseline numbers; G.4 LOCK-N annotations present in source.

---

### Sprint S14 — WAL Group Commit (Days 4-8)

**Goal:** Wire the existing `GroupCommitWal` implementation to eliminate the per-order fsync that caps throughput at ~200 orders/sec.

**Duration:** 5 days
**Risk:** HIGH — WAL is the crash recovery source of truth; durability contract must be preserved exactly
**Blocks:** S15 (throughput cap removal unlocks meaningful event-path measurement)

**Durability contract (non-negotiable):** Order ack MUST NOT be returned to the caller until the order is durably committed to the WAL. Group commit batches multiple orders into one fsync, but the batch MUST complete the fsync before any caller in the batch is unblocked. Violating this means orders disappear on crash.

**Industry-standard parameters (from PostgreSQL `commit_delay` + LMAX Disruptor journaller):**
- **Batch window: 100-500 µs** (wait for more commits to coalesce before fsync)
- **Batch size: 32-128 commits** (whichever hits first)
- **Canonical shape:** N writers push to a bounded MPSC queue + wait on a oneshot; one writer drains + fsyncs + signals all oneshots in the batch
- **Never** use Kafka-style ack-before-fsync — that model only works for replicated logs; we are single-node ledger
- **Never** use SQLite `synchronous=NORMAL` semantics — periodic checkpoint is not acceptable for an order ledger
- References: PostgreSQL `commit_delay`/`commit_siblings` docs, LMAX Disruptor journaller batching via ring-buffer consumer drain pattern

| ID | Task | Size | Depends | Acceptance Criteria |
|----|------|------|---------|---------------------|
| A.1 | Read `journal/src/group_commit.rs` end-to-end; verify `GroupCommitWal` is functionally complete (handles fsync, batch draining, error propagation); document any gaps as sub-tasks | S | — | Written audit note in PR description or doc comment; if gaps found, promoted to blocking sub-tasks before A.2 |
| A.2 | Add `tokio::sync::oneshot::Sender<Result<()>>` as the caller's acknowledgement channel; `GroupCommitWal` must call `sender.send(Ok(()))` ONLY after the fsync succeeds for the batch containing that entry | M | A.1 | Unit test: kill process immediately after send but before fsync — on replay, the order MUST be present in WAL (use `WalReader`); test passes |
| A.3 | Replace `self.journal.lock().unwrap().append()` at `exchange.rs:4906` with `async` channel send to the group commit worker; `place_order` awaits the oneshot receiver before proceeding past the WAL step | M | A.2 | `place_order` call path becomes `async` at the WAL step; existing REST handler `spawn_blocking` wrapper in `rest.rs:513` must be re-evaluated (see A.5); all existing tests pass |
| A.4 | Propagate WAL errors correctly: if the group commit worker fails (disk full, I/O error), all pending oneshot senders receive `Err`; `place_order` returns an error (not a silent success); HTTP 503 returned to client | S | A.3 | Integration test: simulate WAL write failure (mock or temp-dir removed); endpoint returns 503; no order accepted with status `Open` when WAL failed |
| A.5 | Re-evaluate `spawn_blocking` in `rest.rs:513`: if `place_order` is now async-awaitable at the WAL step, the `spawn_blocking` wrapper may no longer be needed for WAL I/O; remove if confirmed safe | S | A.3 | If removed: `cargo build --release` clean, tests pass; if kept: document why (non-WAL blocking work still present) |
| A.6 | Throughput benchmark: run G.3 concurrency bench post-S14; record new throughput in `BENCHMARKS.md` | S | A.3, G.3 | Throughput increases from ~200 orders/sec baseline to ≥ 1000 orders/sec; p99 WAL write latency histogram drops from 500-5000 µs to ≤ 100 µs; numbers recorded |

**Expected benchmark delta:** WAL write stage from 500-5000 µs to ~5-100 µs per order. Throughput from ~200/sec to ≥ 1000/sec under the G.3 load pattern.
**Validation gate:**
- `architect`: mandatory review of A.2 oneshot ack contract — confirm durability invariant is preserved before any other task proceeds
- `ops-manager`: mandatory review of A.4 error propagation — WAL failures must surface as alerts, not silent data loss; confirm 503 is correct HTTP code
- `systems-qa`: review A.3 async integration with `rest.rs` spawn_blocking wrapper; review crash-recovery test in A.2 for completeness
- `trading-qa`: confirm order acceptance semantics unchanged from client perspective (same HTTP 200/400/429 responses, same order state transitions)

**Exit criteria:** `cargo test -p exchange-api-gateway` passes; `cargo test -p exchange-journal` passes; G.3 bench shows ≥ 1000 orders/sec; A.4 error test passes; `BENCHMARKS.md` updated with post-S14 numbers; architect sign-off on durability contract.

---

### Sprint S15 — `process_events` Decoupling (Days 9-13)

**Goal:** Move all non-financial side effects out of the `place_order` hot path and into a dedicated async worker, eliminating 5-50 µs of synchronous post-fill processing.

**Duration:** 5 days
**Risk:** HIGH — incorrectly moving financial operations (balance settlement, fee capture) to async creates double-spend windows; quant-reviewer must sign off on the boundary

**Sync boundary (hard rule):** Balance settlement and fee calculation MUST remain synchronous and complete before order ack. Everything else moves to the async worker.

| ID | Task | Size | Depends | Acceptance Criteria |
|----|------|------|---------|---------------------|
| B.1 | Audit `process_events` at `exchange.rs:4960` and classify every operation: SYNC-REQUIRED (balance, fees) vs ASYNC-SAFE (surveillance, notifications, WS broadcast, push, kline update, taker volume, copy-trade fanout); document classification with rationale | S | — | Classification table in PR description; reviewed by quant-reviewer before B.2 starts |
| B.2 | Define `MatchEventBatch` struct containing all data needed by the async worker (fill events, symbol, user IDs, fees already computed); add `event_tx: tokio::sync::mpsc::Sender<MatchEventBatch>` field to `Exchange` struct; initialize in both `new()` and `with_journal()` | S | B.1 | `Exchange` compiles with new field; initialized at both construction sites; channel capacity documented (suggest 10_000; rationale: backpressure at sustained ~2000 fills/sec with ~5ms worker lag tolerance) |
| B.3 | Implement `spawn_event_processor` background task: drains the channel, executes all ASYNC-SAFE operations from B.1 classification; replace the async-safe portion of `process_events` | L | B.2 | All ASYNC-SAFE side effects execute in the new task; surveillance, WS broadcast, push notifications, kline updates removed from hot path; unit test: fill event sent on channel, verify WS broadcast received by subscriber |
| B.4 | Strip ASYNC-SAFE operations from inline `process_events` call in `place_order`; hot path now sends `MatchEventBatch` to channel and returns; SYNC-REQUIRED operations remain inline | M | B.3 | `place_order` no longer calls surveillance `on_order_placed_full` inline; `process_events` stage latency histogram (G.1) drops from 5-50 µs to ≤ 1 µs; all existing tests pass |
| B.5 | Handle channel backpressure: if `event_tx.try_send()` returns `Full`, log a warning metric (`event_channel_dropped_total`) and drop the event (non-financial side effects are best-effort); NEVER block `place_order` on channel send | S | B.4 | `try_send` used (not blocking `send`); `event_channel_dropped_total` counter visible on `/metrics`; integration test verifies order succeeds even when worker is deliberately paused (channel saturated) |
| B.6 | Latency benchmark: run G.3 bench post-S15; record `process_events` stage drop in `BENCHMARKS.md` | S | B.4, G.3 | `process_events` histogram p99 ≤ 2 µs; overall p99 order latency improvement ≥ 10 µs vs S14 baseline; recorded |

**Expected benchmark delta:** `process_events` stage: 5-50 µs → ≤ 1 µs. Overall order p99 latency improvement: 10-40 µs.
**Validation gate:**
- `quant-reviewer`: mandatory pre-implementation review of B.1 classification table — confirm no financial operation is moved to async
- `trading-qa`: verify fill acknowledgement semantics unchanged; balance is still settled before HTTP 200; order status in response reflects correct post-fill state
- `architect`: review B.2 channel capacity choice and B.5 backpressure strategy (drop vs block vs bounded-wait)
- `systems-qa`: review B.5 backpressure test; review for event ordering guarantees (does surveillance need to see events in order? does copy-trade?)
- `ops-manager`: review `event_channel_dropped_total` alerting — if this fires in production, what is the operational response?

**Exit criteria:** `cargo test -p exchange-api-gateway` passes; `BENCHMARKS.md` updated; quant-reviewer sign-off on B.1 classification; `process_events` histogram p99 ≤ 2 µs; `event_channel_dropped_total` metric present on `/metrics`.

---

### Sprint S16 — `parking_lot` Migration (Days 14-16)

**Goal:** Replace all `std::sync::Mutex` with `parking_lot::Mutex` in `api-gateway` (77 fields) and dependent crates, achieving 2-5× reduction in uncontended lock time.

**Duration:** 3 days
**Risk:** Medium — mechanical but large surface area; poisoning semantics change must be understood and accepted

**Poisoning semantics change (architect must confirm):** `std::sync::Mutex` poisons on thread panic (other threads get `PoisonError`). `parking_lot::Mutex` does not poison — a panicking thread releases the lock and others continue normally. This changes failure semantics: instead of failing loudly on poisoned state, other threads silently proceed with potentially corrupt data. Mitigation: `parking_lot` promotes using `panic = "abort"` in production (which this codebase should use anyway for a financial system) — a panicking thread kills the process rather than leaving partially-mutated state.

| ID | Task | Size | Depends | Acceptance Criteria |
|----|------|------|---------|---------------------|
| F.1 | Confirm `panic = "abort"` is set in the release profile in workspace `Cargo.toml`; if not, add it with rationale comment (financial system — partial-panic state is worse than process restart) | S | — | `[profile.release] panic = "abort"` present; `cargo build --release` clean |
| F.2 | Add `parking_lot = "0.12"` to workspace `Cargo.toml` [dependencies] with justification comment | S | F.1 | `cargo build` resolves the dependency; justification comment present citing: 2-5× faster uncontended acquire, no poisoning (safe with `panic=abort`), enables lock wait metrics in S20 |
| F.3 | Replace `use std::sync::Mutex` → `use parking_lot::Mutex` in `crates/api-gateway/src/exchange.rs`; remove all `.unwrap()` / `.expect()` on lock results (parking_lot lock is infallible — returns guard directly, not `Result`) | L | F.2 | 77 lock call sites updated; `cargo build --release` clean, 0 new warnings; all `exchange.rs` tests pass |
| F.4 | Replace `std::sync::Mutex` in `crates/api-gateway/src/{rest.rs, tasks.rs, server.rs}` and any other `api-gateway` source files | M | F.3 | All `api-gateway` crate sources use `parking_lot::Mutex`; `cargo clippy -p exchange-api-gateway -- -D warnings` clean |
| F.5 | Replace `std::sync::RwLock` → `parking_lot::RwLock` for the `EnginePool` field; replace `tokio::sync::Mutex` (AsyncMutex on VerificationService) — leave as-is (async mutex stays async) | S | F.4 | `EnginePool` RwLock upgraded; tokio async mutex explicitly left unchanged with comment explaining why |
| F.6 | Benchmark: run G.2 single-user bench and G.3 concurrency bench post-S16; record in `BENCHMARKS.md` | S | F.4, G.2, G.3 | Uncontended lock overhead measurably reduced; p50 order latency improvement ≥ 10% vs S15 baseline recorded |

**Expected benchmark delta:** 2-5× reduction in uncontended lock acquisition overhead. p50 order latency: ~15-25% improvement vs S15 numbers.
**Validation gate:**
- `architect`: mandatory pre-F.3 review confirming `panic=abort` + parking_lot poisoning semantics change is acceptable for this codebase
- `developer`: review F.3 mechanical replacement for correctness — confirm no `.lock().unwrap()` pattern silently becomes incorrect (parking_lot returns guard not Result, so `unwrap()` on a non-Result is a compile error, not a silent bug — but verify)
- `systems-qa`: regression pass after F.4; specifically test any code paths that previously relied on observing `PoisonError` for error handling (grep for `PoisonError` before starting)

**Exit criteria:** `cargo test --workspace` passes; `cargo clippy -- -D warnings` clean across workspace; `BENCHMARKS.md` updated; no `PoisonError` references remain in `api-gateway`; `panic = "abort"` confirmed in release profile.

---

### Sprint S17 — ArcSwap for Read-Heavy Config (Days 17-19)

**Goal:** Eliminate 3-4 Mutex acquisitions per futures order by replacing read-heavy config maps with lock-free `ArcSwap<Arc<HashMap>>`.

**Duration:** 3 days
**Risk:** Medium — read/write visibility semantics change; admin config updates become eventually consistent from the perspective of concurrent orders

**Visibility caveat (trading-qa must confirm acceptable):** An order placed at the exact instant an admin updates `perp_configs` may see the old config. This is a narrow window (~nanoseconds, atomic pointer swap). Bybit and Binance have the same eventual-consistency property on config changes. Confirm this is acceptable before E.1 starts.

| ID | Task | Size | Depends | Acceptance Criteria |
|----|------|------|---------|---------------------|
| E.1 | Add `arc-swap = "1"` to workspace `Cargo.toml` with justification comment; confirm visibility semantics are acceptable with trading-qa pre-sign-off | S | — | Dependency added; sign-off documented in PR |
| E.2 | Replace `perp_configs: Mutex<HashMap<Symbol, PerpetualConfig>>` with `perp_configs: ArcSwap<HashMap<Symbol, PerpetualConfig>>`; update all read sites to use `arcswap.load()` (returns `Guard<Arc<...>>`); update admin write sites to use `arcswap.store(Arc::new(new_map))` | M | E.1 | All read sites use `load()`; all write sites use `store()`; `perp_configs.lock().unwrap().clone()` at `exchange.rs:4822` eliminated; `cargo build --release` clean |
| E.3 | Replace `circuit_breakers: Mutex<HashMap<Symbol, CircuitBreaker>>` with `ArcSwap`; same pattern as E.2 | S | E.2 | Read path lock-free; write path uses atomic store; tests pass |
| E.4 | Replace `product_status: Mutex<HashMap<Symbol, String>>` with `ArcSwap`; same pattern | S | E.3 | Read path lock-free; tests pass |
| E.5 | Count and record remaining Mutex acquisitions in `place_order` after E.2-E.4; update LOCK-N annotations from G.4; update `BENCHMARKS.md` with new count | S | E.4 | LOCK-N annotations updated; Mutex acquisition count drops from 16-22 to ≤ 13; count recorded |
| E.6 | Benchmark: run G.2 and G.3 post-S17 | S | E.4 | Throughput and latency improvement for futures orders specifically; recorded in `BENCHMARKS.md` |

**Expected benchmark delta:** Per-futures-order Mutex acquisitions drop by 3-4. Lock contention on `perp_configs` eliminated. Futures order p99 latency improvement: 5-15 µs.
**Validation gate:**
- `trading-qa`: pre-implementation sign-off on eventual-consistency window for config changes (required before E.1)
- `architect`: review ArcSwap write pattern for admin config updates — confirm no partial-map state is possible (full atomic swap vs incremental mutation)
- `developer`: review that `Guard<Arc<...>>` lifetime does not escape function scope in any read site (ArcSwap Guard must not be stored long-term)

**Exit criteria:** `cargo test --workspace` passes; `cargo clippy -- -D warnings` clean; Mutex acquisition count in `place_order` ≤ 13; `BENCHMARKS.md` updated.

---

### Sprint S18 — Mutex Sharding + Lock-Free Rate Limiter (Days 20-26)

**Goal:** Eliminate single-lock contention on `AccountManager` (all users sharing one lock) and replace the O(1200) rate limiter scan with a lock-free per-user atomic counter.

**Duration:** 7 days
**Risk:** HIGH — AccountManager sharding requires multi-lock ordering discipline to prevent deadlock; incorrect shard key or lock ordering causes intermittent deadlocks under concurrent load

| ID | Task | Size | Depends | Acceptance Criteria |
|----|------|------|---------|---------------------|
| C.1 | Design shard key and cross-user operation protocol: `shard_index = user_id.as_u128() % 64`. **2-party operations (transfer A↔B):** `lock(min(shard_a, shard_b)); lock(max(shard_a, shard_b))` — textbook canonical, fully deadlock-free (SEI CERT LCK07-J). **N-party operations (liquidation cascade):** sort the complete set of shard indices in ascending order, acquire in that order. **Hard cap: ≤4 shards per atomic operation** — beyond 4, use two-phase (optimistic reserve with version + commit). Document in `exchange.rs` at the `accounts` field with a `// DEADLOCK PREVENTION:` comment. | S | — | Comment present before any code changes; reviewed by architect before C.2; cap of 4 enforced via `debug_assert!` in helper function |
| C.2 | Refactor `AccountManager` to `AccountManagerShard` containing a sub-`HashMap<UserId, Account>`; replace single `accounts: Mutex<AccountManager>` with `accounts: [parking_lot::Mutex<AccountManagerShard>; 64]` on `Exchange` | L | C.1 | All `self.accounts.lock()` call sites updated to `self.accounts[shard_index(user_id)].lock()`; `cargo build --release` clean |
| C.3 | Update all cross-user operations to lock shards in ascending order: `transfer_between_users`, `liquidation_cascade`, `affiliate_commission_distribute`; add `assert!(shard_a_idx <= shard_b_idx)` guards | M | C.2 | Integration tests for transfer and liquidation pass; no deadlock under 1000-iteration proptest with randomised user pairs; proptest added |
| C.4 | Replace `Mutex<OrderRateLimiter>` + `Vec<Instant>` O(N) scan with `DashMap<UserId, (AtomicU64, AtomicU64)>` where fields are `(count_in_current_second, slot_start_unix_sec)`; CAS-based increment: if `slot_start == current_sec` CAS `count + 1`; else swap to `(1, current_sec)` | M | — | Rate limit check is lock-free per user; concurrent load test: 100 users × 1000 orders, each user capped correctly, no missed rejections and no false rejections; `cargo test` passes |
| C.5 | Replace `frozen_accounts: Mutex<HashSet<UserId>>` and `kyc_approved: Mutex<HashMap<UserId, bool>>` (if present) with `DashSet<UserId>` / `DashMap<UserId, bool>`; add `dashmap = "5"` to workspace `Cargo.toml` with justification comment | S | — | Read path is lock-free for common case; `cargo build --release` clean |
| C.6 | Count remaining Mutex acquisitions in `place_order`; update LOCK-N annotations; update `BENCHMARKS.md` | S | C.2, C.4 | Mutex acquisition count drops to ≤ 8; concurrency bench shows throughput improvement |
| C.7 | Benchmark: G.3 concurrency bench post-S18 | S | C.6 | Throughput improvement ≥ 50% vs S17 baseline under 10-user concurrent load; recorded |

**Expected benchmark delta:** AccountManager contention eliminated for single-user operations (93% of all orders). Rate limiter: O(1200) scan → O(1) atomic CAS. Concurrency throughput: 50-100% improvement under multi-user load.
**Validation gate:**
- `architect`: mandatory review of C.1 lock ordering protocol before C.2 starts; review C.3 cross-user operation coverage for completeness (are there other multi-user operations not listed?)
- `systems-qa`: mandatory review of C.3 proptest for deadlock detection; review C.4 CAS correctness under clock skew (system clock can go backward — what happens to slot_start comparison?)
- `quant-reviewer`: review C.3 liquidation cascade correctness — shard lock ordering must not change the financial outcome of a cascade liquidation
- `security-auditor`: review C.4 rate limiter CAS for TOCTOU race — is the window between slot check and count increment exploitable?

**Exit criteria:** `cargo test --workspace` passes; deadlock proptest in C.3 runs 1000 iterations without deadlock; Mutex acquisition count ≤ 8; `BENCHMARKS.md` updated; architect sign-off on lock ordering.

---

### Sprint S19 — Real Auth Middleware + Env Cache + recv_window (Days 27-31)

**Goal:** Cache `env::var` syscalls at startup (eliminate per-order syscall), implement real auth middleware replacing the `Uuid::nil()` stub, and enforce Binance-compatible `recv_window` timestamp validation.

**Research correction (2026-04-14):** original plan included D.3 auth session cache (1-sec TTL amortizing HMAC verify). **DROPPED** after researching Binance/Bybit/OKX/Coinbase API docs — **no major crypto exchange caches HMAC verification results**. HMAC-SHA256 is ~100-500 ns per op, 0.05% CPU at 1000 orders/sec — cost is negligible and caching creates revocation races (rotated keys still accepted until TTL expires). The real replay defense is `recv_window` + optional nonce bloom filter. Replaced D.3 with D.4 (recv_window enforcement) + optional D.5 (nonce bloom filter).

**Duration:** 5 days
**Risk:** Medium — D.2 real auth middleware is a correctness change; incorrect implementation silently accepts unauthenticated orders or rejects legitimate ones; security-auditor must review

| ID | Task | Size | Depends | Acceptance Criteria |
|----|------|------|---------|---------------------|
| D.1 | Cache `PRODUCTION` and `DEV_MODE` env vars: add `is_production: bool` and `is_dev_mode: bool` fields to `Exchange`; set in `new()` and `with_journal()` via `std::env::var`; replace all `std::env::var("PRODUCTION")` call sites in hot path with `self.is_production` | S | — | Grep for `env::var("PRODUCTION")` and `env::var("DEV_MODE")` in hot path returns zero matches after D.1; `cargo test` passes |
| D.2 | Implement real auth middleware `from_fn_with_state`: extract `X-API-Key` + `X-Signature` + `X-Timestamp` headers; verify HMAC-SHA256 signature over `timestamp + method + path + body`; look up `UserId` from `api_keys` map; inject via `Extension<AuthenticatedUser>`; return 401 on failure. **No caching** — verify every request per Binance/Bybit standard. | L | — | Auth middleware rejects invalid signature with HTTP 401; accepts valid requests; existing tests using `Uuid::nil()` bypass updated or tagged dev-mode only; `cargo test` passes; HMAC verify runs on every request (confirmed by metric counter) |
| D.4 | Enforce `recv_window` timestamp validation (Binance-compatible): parse `X-Timestamp` header, compute `server_time - client_timestamp`, reject if `> recv_window_ms` (default 5000 ms, max 60000 ms); also reject if `client_timestamp > server_time + 1000 ms` (client clock ahead of server by > 1 sec); `recv_window` can be overridden per-request via `X-Recv-Window` header (capped at 60000) | M | D.2 | Test: request with timestamp 6 sec old with default recv_window → 401 "timestamp outside recv_window"; request with timestamp within 5 sec → accepted; request with `X-Recv-Window: 10000` and timestamp 8 sec old → accepted; request with `X-Recv-Window: 70000` → clamped to 60000; request with client timestamp 2 sec in future → 401 |
| D.5 | (OPTIONAL) Nonce bloom filter for strict replay protection: maintain a 60-second rolling bloom filter of recently-seen `(api_key, timestamp, signature[0..8])` tuples; reject duplicates; only enable if security team requires stricter-than-recv_window protection | M | D.4 | If implemented: bloom filter false positive rate < 0.1% under 1000 req/sec load; memory usage < 10 MB; no legitimate requests rejected in integration test; if deferred: documented as KG-10 in backlog |
| D.6 | Verify no call site expects dynamic env var changes at runtime; grep and audit | S | D.1 | Grep confirms no test modifies `PRODUCTION`/`DEV_MODE` after `Exchange::new()`; if any found, refactor or document |
| D.7 | Benchmark: measure auth stage histogram (G.1) after D.2/D.4; record in `BENCHMARKS.md` | S | D.4, G.1 | Auth stage p99 ≤ 5 µs (dominated by HMAC-SHA256 + hashmap lookup); recorded |

**Expected benchmark delta:** `auth` stage: env syscall eliminated (~1 µs). HMAC verify on every request is cheap (~100-500 ns), no caching needed. Auth stage p99 target: ≤ 5 µs.
**Validation gate:**
- `security-auditor`: mandatory review of D.2 HMAC implementation (algorithm choice, header naming matching Binance conventions, body inclusion in signature, constant-time comparison, timestamp drift tolerance); mandatory review of D.4 `recv_window` bounds (5000/60000 ms match Binance/Bybit defaults)
- `architect`: review D.2 middleware wiring — confirm `from_fn_with_state` pattern is correct for axum version in use; confirm Extension injection is compatible with all downstream handlers
- `developer`: review D.1 field initialization in both `new()` and `with_journal()` (CLAUDE.md rule: initialize ALL new Exchange fields in BOTH)
- `systems-qa`: verify DEV_MODE bypass still works for existing dev/test flows after D.2 is real middleware
- `ops-manager`: confirm clock skew handling (D.4) won't cause legitimate client rejection if server NTP drifts

**Exit criteria:** `cargo test -p exchange-api-gateway` passes; `grep 'env::var("PRODUCTION")' crates/api-gateway/src/` returns zero matches in hot path; auth middleware correctly rejects invalid signatures (test); D.3 cache review documented; `BENCHMARKS.md` updated.

---

### Sprint S20 — Observability + Trace Propagation + Protected Metrics (Days 32-35)

**Goal:** Add request-ID trace propagation through `spawn_blocking`, add lock wait time histograms (using parking_lot from S16), add per-endpoint latency metrics, and protect the `/metrics` endpoint.

**Duration:** 4 days
**Risk:** Low — additive instrumentation; no production logic changed
**Depends on:** S16 (parking_lot must be live for J.3 lock wait metrics)

| ID | Task | Size | Depends | Acceptance Criteria |
|----|------|------|---------|---------------------|
| J.1 | Inject `x-request-id` header into `tracing::Span` via `TraceLayer::make_span_with`; generate UUID if not present in request | S | — | Every request log line includes `request_id` field; verified by inspecting logs in test run |
| J.2 | Propagate current span into `spawn_blocking` and `tokio::spawn` call sites in the order path via `tracing::Span::current().in_scope(...)` or `tracing_futures::Instrument`; ensure span is not lost across await points | M | J.1 | Distributed trace for a single `place_order` request shows continuous span from HTTP accept to WAL write to event processor; no span gaps in test trace output |
| J.3 | Add lock wait time histograms using `parking_lot`'s `lock_api` callback hooks or manual `Instant` before/after `lock()` calls at the top 5 hottest lock sites (identified by G.4 LOCK-N audit: accounts, surveillance, rate_limiter, EnginePool, WAL); expose as `api_lock_wait_seconds{lock="accounts"}` etc. | M | S16 | 5 lock wait histograms visible on `/metrics`; baseline wait times recorded in `BENCHMARKS.md` |
| J.4 | Protect `/metrics` endpoint with admin API key: extract `X-Admin-Key` header; compare to `METRICS_API_KEY` env var (cached at startup per D.1 pattern); return 401 if missing or wrong | S | S19 D.1 | Unauthenticated request to `/metrics` returns 401; authenticated request returns metrics; test covers both cases |
| J.5 | Add per-endpoint latency histograms (not just `place_order`): `cancel_order`, `amend_order`, `get_positions`, `get_open_orders`, `get_order_history`; use same G.1 pattern | M | G.1 | 6 endpoint latency histograms visible on `/metrics`; baseline numbers for each recorded in `BENCHMARKS.md` |

**Expected benchmark delta:** No latency improvement (observability sprint). Unlocks operational insight for production diagnosis.
**Validation gate:**
- `security-auditor`: review J.4 metrics auth — is env var comparison timing-safe? (use constant-time comparison)
- `ops-manager`: review J.3 lock names and J.5 endpoint names for operational naming conventions; confirm metrics are sufficient for an on-call engineer to diagnose a latency spike
- `developer`: review J.2 span propagation for correctness — tracing spans in async Rust have non-obvious semantics across await points

**Exit criteria:** `cargo test -p exchange-api-gateway` passes; all 5 lock wait histograms and 6 endpoint histograms present on `/metrics`; `/metrics` returns 401 without admin key; `BENCHMARKS.md` updated with final cumulative S13-S20 before/after table.

---

### Sprint S21 — Binary Order Protocol (Days 36-40) [OPTIONAL]

**Goal:** Add a `POST /api/v2/orders/binary` endpoint accepting FlatBuffers-encoded orders for institutional clients, targeting 5-10× reduction in per-order serialization overhead.

**Duration:** 5 days
**Risk:** Medium — new dependency, new endpoint surface, new attack vector
**Start condition:** S13-S20 all complete AND user confirms institutional traders are a target segment

| ID | Task | Size | Depends | Acceptance Criteria |
|----|------|------|---------|---------------------|
| H.1 | Architect decision: FlatBuffers vs Protobuf; add chosen dep to workspace `Cargo.toml`; rationale: FlatBuffers preferred for zero-copy access without parse step (Bybit uses FlatBuffers for colocation feeds); document choice | S | — | One of `flatbuffers = "23"` or `prost` (protobuf) added; architect justification comment present |
| H.2 | Define order submission schema covering all `PlaceOrderCommand` fields; generate Rust bindings | M | H.1 | Schema file committed; generated bindings compile; all `PlaceOrderCommand` fields representable |
| H.3 | Implement `POST /api/v2/orders/binary`: parse binary body → `PlaceOrderCommand`; call same `Exchange::place_order`; return binary-encoded response | M | H.2 | Endpoint accepts valid binary payload and returns HTTP 200 with binary-encoded order ID; rejects malformed payload with 400 |
| H.4 | Security: enforce `Content-Type: application/x-flatbuffers` (or protobuf); enforce max body size (same as JSON path); reject oversized payloads before parse | S | H.3 | Payload >1MB returns 413; missing Content-Type returns 415; `cargo test` covers both |
| H.5 | Benchmark comparison: JSON vs binary for single order throughput; record in `BENCHMARKS.md` | S | H.3, G.2 | Binary encoding serialization time < 10% of JSON baseline; total end-to-end latency improvement ≥ 5% vs JSON path; recorded |

**Expected benchmark delta:** Serialization/deserialization: 5-10× faster vs JSON. End-to-end order latency: 5-10% improvement for binary path.
**Validation gate:**
- `architect`: review H.1 schema design for forward-compatibility (adding new fields without breaking existing clients)
- `security-auditor`: review H.3/H.4 for parser DoS vectors (malicious FlatBuffers can cause unbounded traversal in some implementations — verify bounds checking)
- `developer`: review generated bindings for safety (FlatBuffers Rust API requires unsafe for some operations — confirm safe wrapper is used)

**Exit criteria:** `cargo test -p exchange-api-gateway` passes; binary endpoint accepts and processes a valid order; security review complete; `BENCHMARKS.md` updated with JSON vs binary comparison.

---

### Sprint S22 — FIX Gateway Order Submission (Days 41-46) [OPTIONAL]

**Goal:** Wire `crates/fix-gateway` `NewOrderSingle` messages directly to `Exchange::place_order`, completing the institutional order submission path.

**Duration:** 6 days
**Risk:** Medium — FIX protocol parsing is complex; incorrect tag mapping creates incorrect orders; `ExecutionReport` broadcast must correctly reflect order state
**Start condition:** S13-S20 complete AND institutional FIX connectivity is a confirmed product requirement

| ID | Task | Size | Depends | Acceptance Criteria |
|----|------|------|---------|---------------------|
| I.1 | Audit current `crates/fix-gateway` implementation: confirm `NewOrderSingle` parser handles all required tags (11, 21, 38, 40, 44, 54, 55, 59); document any missing tags as sub-tasks | S | — | Audit note in PR; missing tags promoted to I.2 sub-tasks |
| I.2 | Map FIX `NewOrderSingle` tags to `PlaceOrderCommand` fields; implement `fix_order_to_command(msg: &NewOrderSingle) -> Result<PlaceOrderCommand>`; document tag mapping table | M | I.1 | Tag mapping table in code comment; all mandatory FIX 4.4 NOS fields covered; `cargo test` includes roundtrip test |
| I.3 | Wire `fix_order_to_command` into FIX session handler; call `Exchange::place_order` on receive; emit `ExecutionReport` (tag 150=0 New, 150=2 Fill, 150=8 Rejected) back through FIX session | L | I.2 | FIX session receives NOS, order appears in exchange order book, ExecReport emitted; integration test using FIX message strings |
| I.4 | Handle FIX `OrderCancelRequest` (tag 35=F): map to `Exchange::cancel_order`; emit `ExecutionReport` (150=4 Cancelled or 150=8 Rejected) | M | I.3 | Cancel via FIX removes order from book; ExecReport emitted; test passes |
| I.5 | Integration test with simulated FIX session: place 10 orders, fill 5, cancel 5, verify all ExecReports received in correct sequence and order states match REST API state | M | I.4 | Test passes; no order state divergence between FIX and REST views |

**Expected benchmark delta:** No REST latency impact (FIX is a separate TCP path). Unlocks institutional order flow.
**Validation gate:**
- `architect`: review I.3 session-to-exchange wiring for thread safety (FIX session runs on its own thread/task — concurrent access to Exchange must go through the same Mutex path as REST)
- `trading-qa`: review I.2 tag mapping for correctness — wrong tag mapping creates orders with incorrect side, qty, or price
- `security-auditor`: review FIX session authentication (is the FIX SenderCompID/TargetCompID validated against a whitelist? Is the TCP port firewalled from public internet?)

**Exit criteria:** `cargo test -p exchange-fix-gateway` passes; I.5 integration test passes; architect sign-off on concurrency model; security-auditor sign-off on FIX session auth.

---

### Sprint Plan Summary (S13-S22)

| Sprint | Name | Days | Risk | Blocks | Key Metric Target |
|--------|------|------|------|--------|-------------------|
| S13 | Measurement Infrastructure | 3 | Low | All | Baselines established; BENCHMARKS.md created |
| S14 | WAL Group Commit | 5 | High | S15 | Throughput ~200 → ≥ 1000 orders/sec |
| S15 | process_events Decoupling | 5 | High | S16 | process_events stage ≤ 2 µs (was 5-50 µs) |
| S16 | parking_lot Migration | 3 | Medium | S17, S20 | Uncontended lock time 2-5× lower |
| S17 | ArcSwap Config | 3 | Medium | S18 | Mutex acquisitions 16-22 → ≤ 13 |
| S18 | Mutex Sharding + Lock-Free Rate Limiter | 7 | High | S19 | Mutex acquisitions ≤ 8; no per-user contention |
| S19 | Auth Cache + Real Middleware | 5 | Medium | S20 | Auth stage p99 ≤ 5 µs |
| S20 | Observability + Protected Metrics | 4 | Low | — | 5 lock + 6 endpoint histograms on /metrics |
| S21 | Binary Order Protocol [OPTIONAL] | 5 | Medium | S13-S20 done | Serialization 5-10× faster vs JSON |
| S22 | FIX Gateway Order Submission [OPTIONAL] | 6 | Medium | S13-S20 done | Institutional FIX path end-to-end |

**Total (S13-S20, mandatory):** ~35 days (~7 weeks)
**Total (S13-S22, with optional):** ~46 days (~9-10 weeks)

**Cumulative improvement targets (vs S13 baseline):**
- Order throughput: ~200 orders/sec → ≥ 2000 orders/sec
- place_order p99 latency: 5-50 ms → ≤ 1 ms
- Mutex acquisitions in place_order: 16-22 → ≤ 8
- WAL write stage: 500-5000 µs → ≤ 100 µs
- process_events stage: 5-50 µs → ≤ 2 µs

**Status:** S13 not yet started (2026-04-14)

---

## API Gateway Optimization Sprint — Final Status (2026-04-16)

All mandatory sprints S13-S20 plus optional S22 complete. S21 cancelled
after design review — see backlog below.

| Sprint | Status | Commit |
|---|---|---|
| S13 — Measurement infrastructure | ✅ done | 5efd851 |
| S14 — WAL group commit | ✅ done | 5efd851 |
| S15 — process_events decoupling | ✅ done | 5efd851 |
| S16 — parking_lot selective | ✅ done | 5efd851 |
| S17 — ArcSwap config | ✅ done | 5efd851 |
| S18 Phase A — DashSet + lock-free rate limiter | ✅ done | 5efd851 |
| S18 Phase B — AccountManager sharding (64 shards) | ✅ done | 216df65, d97058a |
| S19 — HMAC auth + recv_window + env cache | ✅ done | 31b8637, 5c50441 |
| S20 — Observability + /metrics gate | ✅ done | 3c38319 |
| S21 — Binary order protocol | ❌ cancelled | — |
| S22 — FIX NewOrderSingle wiring | ✅ done | da3f535 |

### Headline numbers (vs S13 baseline)

| Metric | Before | After | Status |
|---|---|---|---|
| `concurrent_10_users_100_orders` throughput | ~19.6K ops/sec | **~42.7K ops/sec** (+153%) | ≥50K target 85% met |
| `contended_100_tasks_true_parallel` throughput | ~17.8K ops/sec | **~41.9K ops/sec** (+170%) | 2.35× |
| `admin_churn_during_orders` throughput | ~7.2K ops/sec | **~15.0K ops/sec** (+106%) | 2.08× |
| `place_limit_order_full_rest` p50 latency | ~62 µs | **~56 µs** | 18× under ≤1 ms target |
| HMAC auth stage p99 (release mode) | n/a (no real auth) | **1.7-3.4 µs** | under ≤5 µs target |
| Hot-path `env::var` syscalls | 3 per order | **0** | target met |
| `/metrics` endpoint protected | open | **constant-time gated by X-Metrics-Key** | done |
| Lock-wait histograms on /metrics | 0 | **4** (stp_guard, surveillance, pending_orders, order_locks) | done |
| Per-endpoint histograms on /metrics | 1 (place_order) | **6** (+ cancel/amend/positions/open_orders/history) | done |
| Request-id in tracing spans | no | **yes** via `http_request` span | done |
| `spawn_blocking` span propagation | broken | **fixed** via `spawn_blocking_traced` | done |
| FIX NewOrderSingle path | parser + translator, no integration test | **full roundtrip test** (10 place + 5 cancel) | done |

### S21 cancellation rationale

Binary order protocol was an optional sprint in the original plan. On
review, no established crypto exchange (Binance, Bybit, OKX, Deribit,
Coinbase, Kraken) ships a binary REST order endpoint. Institutional
binary order entry in traditional finance uses FIX (which we now have
as of S22) plus **SBE** (Simple Binary Encoding) as the FIX Trading
Community binary encoding — used by CME, NASDAQ, LSE, Eurex. MessagePack
and Protobuf are not industry standards in this space.

The plan's cited benefit — "5-10× serialization speedup vs JSON" — is
numerically correct but irrelevant at our scale. JSON parse of a
`PlaceOrderCommand` is 3-5 µs out of a ~56 µs end-to-end order path
(5-9% of total). Any binary replacement saves at most 4 µs per order,
below the S13 bench noise floor and invisible to a client.

Doing S21 properly needs:
- A real institutional pilot client confirming they can't/won't use FIX
- A proper SBE XML schema with forward-compat versioning rules
- Interop test against Real Logic SBE Tool (the canonical implementation)
- A separate codec-only latency micro-bench

None of those preconditions hold, so we're deferring S21 to the
backlog rather than shipping a non-standard "binary REST order"
endpoint that no client will use.

### Backlog — deferred SBE binary protocol research

Opened as a standalone track, not part of the optimization sprint.
Prerequisites for picking it up:

- [ ] A named institutional pilot client who asks for binary order
      entry and cannot use FIX (which we already ship).
- [ ] Design doc: SBE XML schema, version rollout plan, forward-compat
      strategy, security (HMAC tag placement, replay protection parity
      with S19 recv_window).
- [ ] Interop: decode our encoded messages with a Java client using
      Real Logic SBE Tool generated code. If that doesn't round-trip,
      the schema is wrong.
- [ ] Latency bench harness that measures ONLY the codec cost, not
      end-to-end order path.
- [ ] Decision on schema hosting: committed XML in this repo vs
      published artefact with its own versioning.
- [ ] SBE codec implementation choice: hand-rolled fixed-offset
      reader/writer vs `sbe-rs` crate vs Real Logic Java codegen.

Estimated size once prerequisites clear: 3-5 days for a pilot, 2-3
weeks for production-grade with client onboarding docs.

## Engineering Best Practices Sprint — Status (2026-04-16)

Research + implementation session completing the full P0-P3 backlog
identified by the engineering best-practices audit (`docs/research/exchange-engineering-best-practices.md`).

### Documents produced
- `docs/research/exchange-engineering-best-practices.md` — 8 concerns, 32-item backlog
- `docs/slo.md` — 5 SLIs + RPO/RTO targets
- `docs/fmea.md` — 20 failure modes with detection/mitigation/recovery
- `docs/game-day.md` — quarterly failure simulation procedure
- `ops/loadtest/` — 4 k6 profiles (baseline, burst, ramp, soak)
- `ops/grafana/` — 3 dashboards (overview, engine metrics, task health)

### P0 (5/5 DONE)
| Item | Commit |
|------|--------|
| WAL replay fail-fast | a4955b1 |
| fsync EIO → process abort | a4955b1 |
| Durable audit log (Postgres + hash chain) | 9f7ab65 |
| KYC persistence (Postgres write-through) | e8fbb46 |
| Feature-flag audit trail | 4542180 |

### P1 (16/16 DONE)
| Item | Commit |
|------|--------|
| Balance conservation proptests (800 cases) | de69cec |
| WAL replay determinism proptests (100 cases) | d872061 |
| SLO/RPO/RTO doc | 4af20f0 |
| FMEA doc (20 failure modes) | 62b0130 |
| OperatingMode enum (5 states) | 0a358be |
| Kill-switch integration tests | 2e5408c |
| 5-phase graceful shutdown | 291d323 |
| SecretProvider trait + KMS abstraction | c23d4da |
| SegmentedWal wired into production | d7e4b5b |
| Task dependency ordering (5 tiers) | 1c6e0d3 |
| Four-eyes dual-control approval | 72255df |
| Engine snapshot (full-fidelity + background task) | 1aace08, c8fa723 |
| MiCA appropriateness assessment | 7b10c3e |
| Travel Rule provider + transmission | 5c42d29 |
| On-chain PoR (BTC + ETH providers) | a1f734e, 096ed78 |
| Runbook audit + 3 admin endpoints | 2e3af2e, af9a3bf, 56c59e1, fbd498a |

### P2/P3 (11 DONE)
| Item | Commit |
|------|--------|
| Task heartbeat watchdog | e27361d |
| k6 load testing harness | eb7eb1c |
| PoR real HTTP clients (BTC/ETH) | 096ed78 |
| Secret reads migration | 5f54ef7 |
| CPU pinning for engine threads | e1b01c8 |
| Grafana dashboards as code | 4e5a67d |
| OTLP exporter (OpenTelemetry) | 0831661 |
| Adversarial concurrency stress tests | 4bd30c4 |
| Game day playbook | 5cc8c34 |
| WAL split-brain fix (crash recovery) | bb76b5d |
| Snapshot WAL sequence fix | bb76b5d |

### Remaining (3 items — external dependencies)
| Item | Why deferred |
|------|-------------|
| DORA ICT register | Legal/compliance document |
| FIDO2 WebAuthn on admin | Needs frontend JS |
| rtrb SPSC swap | Architectural refactor |

### Headline numbers
- **2188 tests**, 0 failures, 81 test suites
- **36 commits** in this session
- **P0-P3 backlog: 32/35 items done** (3 deferred to external deps)
