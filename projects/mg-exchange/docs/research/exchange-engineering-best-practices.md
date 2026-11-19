# Exchange Engineering Best Practices: Audit & Roadmap

**Date:** 2026-04-16  
**Scope:** Audit the current Rust exchange codebase against industry best practice across 8 engineering concerns, produce a prioritized gap list.  
**Method:** 5 parallel research tracks — 1 internal code audit (Explore agent over the monolith), 4 external research tracks (general-purpose agents with web access) covering security/regulatory, latency/hot-path, durability/testing, observability/resilience. Internal audit claims were cross-checked against the code; several were wrong and are corrected below.

---

## 1. Our Baseline

- **Scale:** 24 crates, 2163+ tests, ~42K ops/sec peak in `concurrent_10_users_100_orders` bench, p50 place_order ≈ 56µs (release mode, S13-S22 final numbers).
- **Architecture:** monolith, per-symbol matching threads (OS threads + `crossbeam` channel), WAL with group commit, 64-way sharded `AccountManager`, `ArcSwap` for read-heavy config, `DashSet` for lock-free frozen-account checks, HMAC auth with recv_window.
- **Deployment:** single binary `crates/server/src/main.rs`, axum REST + WS on port 8080, Postgres optional (in-memory dev mode), Fireblocks for custody, Sumsub for KYC, Chainalysis for address screening.
- **Team:** single operator. No named institutional pilot client. Retail + bot traders over public internet.
- **Regulatory target:** MiCA CASP registration in the EU, FATF-compliant Travel Rule, standard AML/KYC.

The scale and team constraint matter — they determine which "best practices" are load-bearing vs premature optimization.

---

## 2. Eight Concerns

For each concern: **Industry standard → Our state → Gap → Recommendation**. Sources cited by name, not URL. Claims from research agents that could not be verified against a primary source are flagged `[UNVERIFIED]`.

### 2.1 Security

**Industry standard.** Three layers. (a) *Secrets at rest*: HSM-backed root key (FIPS 140-2 Level 3 — YubiHSM, CloudHSM, Thales Luna) wraps a KMS transit key that wraps application secrets. Vault and cloud KMS (AWS/GCP Secrets Manager) provide dynamic DB credentials and automatic rotation. JWT signing keys typically live in KMS; only the sign-operation is exposed. (b) *API key storage*: show once, store as HMAC-SHA256 or bcrypt/argon2 hash. Constant-time comparison on verify. (c) *Withdraw signing pipeline*: 95–98% cold (multi-sig or threshold MPC, physically distributed signers), 2–3% warm (multisig online), 1–2% hot (MPC or hardware-backed single-sig). Admin access requires FIDO2 (YubiKey/Titan), IP whitelist, four-eyes for sensitive ops, immutable audit log shipped to WORM storage.

*Sources:* Coinbase S-1 filing (cold ratio), Coinbase Security blog on FIDO2 adoption, Kraken security blog (cold storage disclosure), Fireblocks and BitGo architecture docs (MPC vs multisig), NIST SP 800-57 (key management), FATF/FinCEN guidance. Postmortems: Mt.Gox (no segregation), QuadrigaCX (single-person key control), Coincheck (hot wallet without multisig), FTX Chapter 11 declaration by John Ray III (commingled funds, no audit controls).

**Our state.**
- ✅ API secrets encrypted at rest with AES-256-GCM (`crates/api-gateway/src/auth.rs:601`). Encryption master key from `API_SECRET_ENCRYPTION_KEY` env var.
- ✅ Hash-based verification via `verify_signature_typed` (constant-time `verify_slice` from the `hmac` crate) — `crates/api-gateway/src/auth.rs:293`.
- ✅ Role-based admin permissions with `AdminRole::has_permission` check — `crates/api-gateway/src/auth.rs:247`.
- ✅ Audit log with SHA-256 hash chain + `verify_integrity()` walk — `crates/admin/src/audit.rs:69,208`.
- ✅ Fireblocks custody integration — `crates/custody/src/fireblocks.rs`.
- ✅ Production startup panics if `JWT_SECRET`, `DATABASE_URL`, `WEBHOOK_SECRET`, etc. are missing when `PRODUCTION=true` — `crates/server/src/main.rs:65`.

**Gap.**
1. **Audit log is in-memory only** — the module itself says "Phase 1: in-memory. Phase 2: persist to an append-only PostgreSQL table" (`crates/admin/src/audit.rs:147`). A restart loses the chain. WORM archival not wired. 🔴 HIGH.
2. **Fireblocks RSA private key loaded as plaintext env var** — `crates/custody/src/fireblocks.rs` uses env-string-to-key. No HSM/KMS abstraction. If the process dumps core the key is in the core file. 🔴 HIGH.
3. **No FIDO2 / hardware key requirement on admin endpoints** — admin auth checks an API key + role, not a device binding. 🟡 MED.
4. **No dual-control ("four-eyes") on sensitive admin ops** — single admin key revokes users, toggles flags, credits deposits. 🟡 MED.
5. **No secret rotation** — encryption master key is static env var; no runtime rotation path. 🟡 MED.

**Recommendation.**
- **P0:** Durable audit log. Write a Postgres-backed `AuditLogStore` with append-only schema + SHA-256 chain column, a verifier that re-walks on startup, and a daily job that anchors the last hash to an external immutable store (simplest: S3 Object Lock in COMPLIANCE mode). Effort: M.
- **P1:** `SecretProvider` trait with `EnvSecretProvider` (current) and `KmsSecretProvider` (AWS KMS / GCP Secret Manager / Vault transit) implementations. Fireblocks RSA key goes through it. Effort: M.
- **P2:** Four-eyes middleware for {credit_deposit, revoke_user_api_keys, feature_flag_set, block_ip}. Requires a second admin approval within 5 min before the action executes. Effort: M.
- **P3:** WebAuthn/FIDO2 layer in front of admin endpoints (separate from the HMAC layer we added in S19). Effort: L.

### 2.2 Latency / Hot path

**Industry standard.** LMAX single-writer principle: one thread owns all mutations to a given piece of state; cross-thread communication via pre-allocated ring buffer with sequence-based coordination. Zero allocation in the matching loop (object pools, slab allocators, `smallvec`/`arrayvec`). CPU pinning via `isolcpus` / `taskset` / `sched_setaffinity`; disable hyperthreading and C-states on isolated cores; NUMA-local memory. SPSC queues (fastest) where topology allows, MPSC where producers are multiple gateways. Kernel bypass (DPDK, Solarflare OpenOnload) for trad-fi co-lo only. Market data fanout via UDP multicast + TCP snapshot gap-fill (ITCH, CME MDP3) in the DC, WebSocket bridge to retail clients externally.

*Sources:* LMAX Disruptor paper (Thompson, Barker, Farooqui, Gee, Stewart, 2011) — Disruptor hand-off ~50ns vs `ArrayBlockingQueue` ~32µs. Martin Thompson mechanical-sympathy.blogspot.com. Real Logic Aeron and Agrona docs. Carl Cook (Optiver) CppCon 2017 "When a Microsecond Is an Eternity" — ~1µs tick-to-trade target. Peter Lawrey, Chronicle Queue. NASDAQ ITCH 5.0 spec. CME iLink 3 Developer's Guide. Erik Rigtorp's blog (NUMA / CPU pinning microbenchmarks — hundreds of µs jitter → single-digit µs after isolcpus).

**Our state.**
- ✅ Per-symbol OS threads with bounded `crossbeam` channel (cap 1024) — `crates/matching-engine/src/engine_pool.rs:46`, `engine_thread.rs`. Zero shared locks across symbols.
- ✅ `SmallVec<[MatchEvent; 4]>` in hot loop avoids heap alloc for typical 1–3-fill cases — `engine_thread.rs`.
- ✅ HDR-style latency histograms with 11 fixed buckets, reset every 5 min — `crates/admin/src/metrics.rs`.
- ✅ 15+ staged timing across `place_order` — `crates/api-gateway/src/exchange.rs` (each stage timed and recorded to per-stage histogram).
- ✅ `panic = "abort"` in release profile — `Cargo.toml:157`. This is *important* because we use `parking_lot::Mutex` which cannot poison, so abort-on-panic eliminates poisoned-lock recovery entirely.
- ✅ 64-way sharded `AccountManager` with deadlock-safe lock ordering — `crates/account/src/sharded.rs`.
- ✅ `ArcSwap` for read-heavy config (products, perp configs) — removes RwLock from hot path.
- ✅ `DashSet` for frozen-account check — lock-free O(1) pre-check before expensive stages.

**Gap.**
1. **No CPU pinning for engine threads.** When we deploy to a multi-socket host, cache-line bouncing + NUMA remote accesses will show up as tail-latency jitter. 🟢 LOW at current scale, 🟡 MED once we're past ~100K ops/sec.
2. **No preallocated order slab.** Each `PlaceOrderCommand` allocates. Per Carl Cook the dominant cost at <1µs tick-to-trade is allocation. At our 56µs p50, allocation is ~5% — not the bottleneck yet. 🟢 LOW.
3. **`parking_lot::Mutex` usage means no poisoning by design** — the internal audit's "982 places use `unwrap_or_else(|e| e.into_inner())`" count is an artifact of legacy `std::sync::Mutex` sites that should be migrated; those sites are no-ops when `panic=abort` but still clutter the code. 🟢 LOW cosmetic, not a correctness issue.
4. **Matching channel is MPSC `crossbeam::channel`** not SPSC `rtrb`. `crossbeam` bounded send/recv is ~80–150ns per pair, `rtrb` is tens of ns. Delta is negligible for us today (56µs end-to-end) but swap becomes cheap win if we ever approach Disruptor territory. 🟢 LOW.

**Recommendation.**
- **P3 / deferred:** CPU pinning via `core_affinity` crate, configurable via env var. Only implement when we have measured NUMA jitter on the target host. Effort: S.
- **P3 / deferred:** rtrb SPSC swap for the matching engine command channel — requires pinning *one* gateway submission point to each engine, which is a bigger architectural refactor. Not worth it at our scale. Effort: M.
- **P4:** Migrate remaining `std::sync::Mutex` sites to `parking_lot::Mutex` to eliminate the `unwrap_or_else(|e| e.into_inner())` cosmetic debt. Effort: S, but touches ~100 files.

The honest read: **the hot path is already good.** LMAX cites mean ~50µs end-to-end on 2011 Nehalem hardware; we're at 56µs p50 on modern hardware for a much heavier path (auth, HMAC, risk checks, WAL append, settlement). The gap to further improvement is in the surrounding stages, not the matching thread.

### 2.3 Data durability & recovery

**Industry standard.** Append-only segmented WAL (16MB–1GB segments), per-record CRC32C, fsync per batch or per segment boundary. Group commit batches N commits or T microseconds into a single fsync (Postgres `commit_delay`, MySQL binlog group commit, Kafka `linger.ms`, Aeron Archive position-based). `fdatasync` vs `fsync` vs `O_DIRECT` gotchas — post-2018 Postgres panics on fsync EIO because Linux silently discards dirty pages after reporting the error once (fsyncgate — LWN April 2018, Craig Ringer, Andres Freund). Snapshot + replay recovery: periodic full-state snapshot, WAL retained from the snapshot's LSN. Double-entry ledger: every transaction ≥2 ledger lines summing to zero per currency; enforced at code level, not policy. Continuous reconciliation (matching state ↔ WAL ↔ DB ↔ custody). RPO ≈ 0 for the trade path (synchronous replication); RTO seconds-to-minutes for serious venues. Monthly restore drills — "a backup you have not restored is not a backup" (Google SRE book, ch. 26).

*Sources:* PostgreSQL WAL chapter, Kafka design doc (Jay Kreps), RocksDB WAL wiki, Aeron Archive docs, Chronicle Queue (Peter Lawrey), LMAX Disruptor + LMAX Exchange architecture talks (Martin Fowler 2011), Raft paper §7 (Ongaro, Ousterhout), Stripe engineering blog (Brandur Leach on idempotency), Jane Street blog (accounting invariants), TigerBeetle design docs (Joran Dirk Greef — double-entry as primitive), Martin Kleppmann *Designing Data-Intensive Applications*, Google SRE ch. 26. fsyncgate: LWN April 2018.

**Our state.**
- ✅ Command WAL with sequence numbers, JSON format — `crates/journal/src/wal.rs`.
- ✅ Group commit with batch-of-100 / 10ms linger — `crates/journal/src/group_commit.rs:49`. This alone drops fsync rate from 1000/sec to ~10/sec in bench.
- ✅ WAL replay on startup from `crates/server/src/main.rs` reproduces into matching engine pool.
- ✅ Double-entry ledger in Postgres with sum-to-zero check before commit — `crates/db/src/ledger.rs`.
- ✅ Balance reconciliation between user ledger and custody wallets with 1bp tolerance — `crates/admin/src/reconciliation.rs`.
- ✅ Ledger atomic batch write in single Postgres transaction.

**Gap.**
1. **WAL is a single file — no segment rotation.** Grows unbounded. Recovery cost grows linearly. 🔴 HIGH.
2. **No snapshot strategy.** Recovery replays the full WAL from epoch. At production cadence (~42K ops/sec) the WAL reaches GB within hours; replay cost blows past any RTO target. 🔴 HIGH.
3. **WAL replay silently continues on error.** `main.rs:524` logs a warning and proceeds with an empty book. This is the **worst** possible recovery behavior — we'd start trading against a book that doesn't reflect reality. 🔴 P0.
4. **No documented RPO/RTO target.** We don't know what we're aiming for. 🟡 MED.
5. **No restore drill.** Backups may or may not work. 🟡 MED.
6. **fsync error handling not audited.** We call `fsync` inside group_commit but don't specify what happens on EIO. Post-fsyncgate the only correct answer is PANIC and force WAL replay from the last known-good position on restart. 🟡 MED.

**Recommendation.**
- **P0:** Make WAL replay failure fatal. Replace the log-and-continue pattern at `main.rs:524` with a hard `bail!` that prevents the server from starting. An operator then has to decide: rotate in a backup, or replay skipping the broken segment with an explicit flag. Effort: S. Risk: medium — we want to test startup robustness after this change.
- **P0:** Audit `group_commit.rs` fsync error path. Panic on EIO. Effort: S.
- **P1:** WAL segment rotation. Segment size configurable (default 64MB). Rotate on size boundary. Old segments compacted after a snapshot covers them. Effort: M.
- **P1:** Periodic engine snapshot. Snapshot the `OrderBook` + `AccountManager` shard states, write to a snapshot file, record the snapshot-start LSN in the WAL. Recovery: load latest snapshot → replay WAL from snapshot LSN. Effort: L — ~2 weeks with tests.
- **P1:** Document RPO target. Our target should be **RPO = 0 for the trade path** (a confirmed order must never be lost), RPO ≤ 5 min for analytics/klines (acceptable to lose a few minutes of tickers). RTO target: ≤ 5 min for warm restart, ≤ 30 min for cold restart from backup. Effort: S — doc only.
- **P2:** Scheduled restore drill — quarterly, on a staging cluster from a real production backup. Effort: M operationally.

### 2.4 Observability

**Industry standard.** SLO-driven monitoring (Google SRE book ch. 4, 6). Pick ~5 user-facing SLIs, set SLO targets slightly looser than internal aspirations, derive an error budget, gate releases on it. RED method (Tom Wilkie) for request services: Rate/Errors/Duration. USE method (Brendan Gregg) for resources: Utilization/Saturation/Errors. Four Golden Signals (Beyer et al., Google SRE ch. 6) as umbrella. Structured JSON logs with PII redaction at emission. Request_id propagated across async boundaries via `tracing::Span`. Distributed tracing via OpenTelemetry — head-based 1–10% sampling for general traffic, tail-based 100% for errors and high-latency outliers. Multi-window multi-burn-rate alerting (SRE Workbook ch. "Alerting on SLOs") instead of static thresholds. Alert payload includes runbook URL. Audit log shipped to immutable storage for regulatory retention (MiFID II RTS 6 — 5 years; MiCA Title V mirrors this).

*Sources:* Google SRE Book & SRE Workbook (free at sre.google/books), Brendan Gregg *Systems Performance* and USE method page, Tom Wilkie / Grafana blog "The RED Method", Charity Majors (Honeycomb) on "wide events", OpenTelemetry spec, Ben Sigelman (LightStep) on tail-based sampling, Google Dapper paper, Rob Ewaschuk "My Philosophy on Alerting", Alex Hidalgo *Implementing Service Level Objectives*.

**Our state.**
- ✅ Prometheus `/metrics` endpoint gated by constant-time `X-Metrics-Key` (S20) — `crates/api-gateway/src/server.rs:1147`.
- ✅ Lock-wait histograms for stp_guard, surveillance, pending_orders, order_locks — `crates/admin/src/metrics.rs`.
- ✅ Per-endpoint histograms (place_order, cancel, amend, positions, open_orders, history) — `crates/admin/src/metrics.rs`.
- ✅ Request_id propagated through tracing spans, including across `spawn_blocking_traced` — `crates/api-gateway/src/request_id.rs`.
- ✅ Structured JSON logging (Phase 1 of PRODUCTION_ROADMAP P1-1).
- ✅ `/health` endpoint — `crates/api-gateway/src/server.rs:1146`. (Internal audit incorrectly claimed it was missing.)
- ✅ Audit log with hash chain (in-memory — see 2.1).
- ✅ 4 runbooks audited against real routes, CI guard `tests/runbook_accuracy.rs` (done this session).

**Gap.**
1. **No declared SLO targets.** We don't know what "good" looks like. Can't compute burn rates. 🔴 HIGH (documentation gap, not code).
2. **No burn-rate alerting.** Alerts will be cause-based on metric thresholds, causing fatigue. 🟡 MED.
3. **No OpenTelemetry / distributed tracing exporter.** Currently `tracing` spans stay in the log line; there's no OTLP sink. For a monolith this is tolerable; once we have N services it becomes critical. 🟢 LOW at our current architecture.
4. **Runbook URLs not embedded in alert payloads.** We have runbooks and we have metrics; they're not cross-linked. 🟡 MED.
5. **Grafana dashboards not version-controlled.** No dashboards-as-code. 🟡 MED.

**Recommendation.**
- **P1:** Write `docs/slo.md` declaring 5 SLIs with targets: (a) order_ack_latency p99 ≤ 10ms, (b) fill_publish_latency p99 ≤ 50ms, (c) matching_engine_availability ≥ 99.9% per symbol, (d) ws_fanout_lag p99 ≤ 100ms, (e) withdrawal_success_rate ≥ 99.5%. Effort: S — research + doc.
- **P1:** Alertmanager rules based on multi-burn-rate method (2% budget in 1h = fast burn → page; 10% in 6h = slow burn → ticket). Payload includes runbook URL matching the detected symptom. Effort: M.
- **P2:** Grafana dashboards as code (Jsonnet / Grafonnet or HCL), committed under `ops/grafana/`. Effort: M.
- **P3:** OpenTelemetry OTLP exporter, 10% sampling for general traffic, 100% for errors. Effort: M. Defer until we actually need it (i.e., after service split, if it happens).

### 2.5 Operational control plane

**Industry standard.** Sharp distinction between *release flags* (cohort-based, short-lived — LaunchDarkly/Unleash/Split.io) and *operational kill switches* (long-lived, locally cached, work when vendor is down). Three scopes of halts: venue-wide, symbol-level, user-level. MiFID II Art. 17 + RTS 6 require a "kill functionality" that immediately cancels all unexecuted orders, testable and logged. Graceful degradation modes: normal → read-only → cancel-only → market-data-only → fully halted; each a single config flip, rehearsed. Runtime admin surface separate from deploy-time config; every runtime admin call logged with operator identity and reason; break-glass procedure for emergencies with automatic post-hoc review.

*Sources:* Pete Hodgson "Feature Toggles" on martinfowler.com (canonical taxonomy), ESMA MiFID II RTS 6 (Delegated Regulation 2017/589) on kill functionality, Michael Nygard *Release It!* (graceful degradation, circuit breakers, bulkheads), NIST SP 800-53 AC-3 / AU-2 (admin access controls), Coinbase/Kraken public status pages showing cancel-only mode during incidents, FIA "Recommendations for Risk Controls for Trading Firms".

**Our state.**
- ✅ Feature flags enum with runtime toggle via `POST /admin/feature-flags` — `crates/common/src/feature_flags.rs`, `crates/api-gateway/src/rest.rs:1292`. 6 flags: FUTURES, WITHDRAWALS, COPY_TRADING, TRADING_BOTS, REGISTRATION, DEPOSITS.
- ✅ Venue-wide kill switch via `AtomicBool is_halted` checked at order ingress — `crates/api-gateway/src/exchange.rs:4542`.
- ✅ Per-symbol circuit breakers (price bands + trigger counting, auto-halt for 60s) — `crates/matching-engine/src/circuit_breaker.rs`.
- ✅ User-level freeze via `/admin/surveillance/action` — `crates/api-gateway/src/rest.rs:6925`.
- ✅ 13+ admin endpoints for halt/resume, engine restart, KYC approval, deposit credit, IP block/unblock, prune-stale.
- ✅ Withdrawal circuit breaker with per-tx / per-hour / per-day / new-address limits — `crates/custody/src/circuit_breaker.rs`.
- ✅ Axum graceful shutdown via `with_graceful_shutdown` — `crates/api-gateway/src/server.rs:1197`. Minimal — not a full coordinator.

**Gap.**
1. **Feature-flag changes not audited.** Anyone with admin key can flip `ENABLE_WITHDRAWALS` off without an audit trail of who/when/why. 🔴 HIGH.
2. **No explicit operating-mode enum.** We have `is_halted` on/off; we don't have read-only or cancel-only modes. During an incident the operator has two choices: everything on, everything off. Missing the middle ground. 🟡 MED.
3. **Graceful shutdown is axum-only.** `with_graceful_shutdown` drains HTTP but doesn't own the shutdown of background tasks, WAL flusher, matching engine threads, or DB pool. A SIGTERM during heavy load may drop in-flight work. 🔴 HIGH.
4. **No kill switch test.** MiFID II RTS 6 requires the kill to be testable AND the test to be logged. We haven't either. 🟡 MED.
5. **No break-glass / four-eyes for irreversible ops** (see 2.1).

**Recommendation.**
- **P0:** Audit log entry on every `admin_set_feature_flag` call — actor, old value, new value, timestamp, reason (required field in the request). Effort: S.
- **P1:** `enum OperatingMode { Normal, ReadOnly, CancelOnly, MarketDataOnly, Halted }`. Checked at order ingress. Each transition logged. Effort: M.
- **P1:** Graceful shutdown coordinator. Adopt `tokio-graceful-shutdown` crate OR write a small subsystem tree based on `CancellationToken`. Shutdown order: stop accepting connections → drain in-flight → flush WAL synchronously → checkpoint engine state → close DB pool → exit. SIGKILL after hard timeout. Effort: M.
- **P1:** Automated kill-switch test. Integration test that (a) calls `/admin/halt`, (b) places an order and asserts 503, (c) calls `/admin/resume`, (d) places an order and asserts 200. Log the test run in audit log. Effort: S.

### 2.6 Resilience

**Industry standard.** Supervisor trees (Erlang/OTP — Joe Armstrong's PhD thesis; Akka; Elixir). Background tasks must not silently die; a panic or task drop must be detected and recovered with state replayed from WAL. FMEA (Failure Modes and Effects Analysis) document covering matching engine panic, WAL fsync fail, DB primary down, network partition, clock skew, OS OOM, disk full, DNS failure, cert expiry, queue saturation, deadlock. Each entry: detection, blast radius, mitigation, recovery. Cell architecture (AWS, Slack, Airbnb) for blast-radius bounding — partition by user/symbol/region so one cell's failure is isolated. Bulkhead pattern: separate thread pools per dependency. `panic = "abort"` in release (avoid corrupted state from unwinding through locks). Game days before continuous chaos; chaos in prod only at maturity level 4+.

*Sources:* Joe Armstrong "Making Reliable Distributed Systems in the Presence of Software Errors" (PhD thesis, 2003). learnyousomeerlang.com supervisor chapters. *Release It!* (Nygard) on bulkheads. AWS cell architecture papers (Colm MacCárthaigh re:Invent talks). Netflix Simian Army (Chaos Monkey — Bruce Wong, Cory Bennett, 2011). Casey Rosenthal & Nora Jones *Chaos Engineering* book. principlesofchaos.org. Google SRE Workbook postmortem chapters. Cloudflare incident reports.

**Our state.**
- ✅ `supervised_spawn` + `supervised_spawn_with_shutdown` with 1-second backoff restart — `crates/api-gateway/src/tasks.rs:32`.
- ✅ Per-symbol matching threads are natural bulkheads — one symbol's engine crash doesn't affect others.
- ✅ `panic = "abort"` in release — `Cargo.toml:157`. Matches industry best practice.
- ✅ WAL replay on startup (see 2.3 gap — needs to be fatal on error).
- ✅ Background tasks: liquidation monitor, funding settlement, insurance fund, GTD scanner, deposit poller, AML screener, DMS — all supervised.

**Gap.**
1. **No FMEA document.** We've fixed individual failure modes as they've come up but there's no systematic catalog. Can't have a game day without one. 🟡 MED.
2. **No task dependency graph.** `supervised_spawn` restarts individual tasks but doesn't know that (e.g.) the liquidation monitor depends on risk-engine startup. Cold-start ordering is implicit. 🟡 MED.
3. **No deadlock / hung-task watchdog.** A task can be "running" but stuck in a lock; nothing notices. 🟡 MED.
4. **No game day playbook.** No rehearsed failure scenarios. 🟡 MED.
5. **No `FoundationDB`-style deterministic simulation.** For a monolith with non-trivial concurrency this is the gold standard of testing; `loom` exists in Rust and we don't use it. 🟢 LOW — aspirational.

**Recommendation.**
- **P1:** Write `docs/fmea.md` covering top 20 failure modes (matching panic, WAL EIO, DB down, Postgres replication lag, NTP drift, OS OOM, disk full, DNS fail, cert expiry, hung background task, market-data provider timeout, Fireblocks API outage, Sumsub webhook backlog, Chainalysis timeout, WebSocket fanout backlog, rate-limit exhaustion, lock contention, panic in spawn_blocking, panic in axum handler, shutdown race). For each: detection mechanism, blast radius, mitigation, recovery procedure, runbook link. Effort: L — ~1 week.
- **P1:** Task dependency order documented in `main.rs` and asserted (each background task has a list of prerequisites, supervisor checks they're ready before spawning). Effort: M.
- **P2:** Task heartbeat + watchdog. Each background task sends a heartbeat every 30s; supervisor restarts if missing for >60s. Effort: M.
- **P2:** Quarterly game day — pick one FMEA entry, simulate the failure in staging, execute the runbook, time the recovery, update the runbook. Effort: S per drill.
- **P3:** `loom` coverage for account sharding concurrency tests. Effort: M.

### 2.7 Regulatory correctness

**Industry standard.** *MiCA* (EU Reg. 2023/1114) Title V: risk warnings per asset (Art. 66), appropriateness assessment for non-experienced clients before complex services (Art. 71(2)), 5-year record retention (Art. 68), complaints handling with SLA (ESMA draft: 15 business days acknowledgement, 35 days resolution), conflict-of-interest disclosure. *DORA* (EU Reg. 2022/2554) applies to CASPs from Jan 2025: ICT risk management, third-party register, penetration testing (TLPT for systemic). *FATF Recommendation 16 (Travel Rule)*: originator/beneficiary data above EUR 1,000 (EU TFR) / USD 3,000 (US BSA) in IVMS 101 format, transmitted via Notabene / TRUST / Sygna / VerifyVASP / TRP. *AML*: real-time screening against OFAC SDN, EU consolidated, UN, UK HMT; PEP via Refinitiv/ComplyAdvantage/Chainalysis; SAR filing within 30 days of detection (FinCEN). *Proof of Reserves*: Merkle tree of liabilities + on-chain attestation of asset addresses. Liability-only PoR is industry-rejected after FTX.

*Sources:* Regulation (EU) 2023/1114 (MiCA), (EU) 2023/1113 (TFR), (EU) 2022/2554 (DORA), ESMA MiCA Q&A and draft RTS. FATF Updated Guidance on Virtual Assets (October 2021). IVMS 101 spec at intervasp.org. Kraken PoR methodology blog. FTX Chapter 11 first-day declaration (John Ray III).

**Our state.**
- ✅ KYC via Sumsub provider integration — `crates/compliance/src/sumsub.rs`, and manual approval flow — `crates/compliance/src/kyc.rs`.
- ✅ Risk warning gate on futures orders (MiCA Art. 71) — `crates/api-gateway/src/exchange.rs` check before futures order placement.
- ✅ OFAC/sanctions screening via Chainalysis — `crates/compliance/src/chainalysis.rs`.
- ✅ SAR workflow — `crates/compliance/src/sar.rs`.
- ✅ Travel Rule VASP directory Phase 1 — `crates/compliance/src/travel_rule.rs`.
- ✅ Proof of Reserves Merkle tree + per-user inclusion proof — `crates/proof-of-reserves/src/report.rs`.

**Gap.**
1. **KYC records in-memory HashMap.** `crates/compliance/src/kyc.rs:63` says "Phase 1: in-memory store". Restart loses state. 🔴 HIGH.
2. **Travel Rule is a stub.** `travel_rule.rs:49` `find_by_address()` returns `None`. No TRISA/Notabene/TRUST integration. 🔴 HIGH for EU launch.
3. **PoR is liability-only.** No on-chain attestation of asset addresses. Post-FTX this is not acceptable for a credible PoR. 🟡 MED for retail launch, 🔴 HIGH for institutional credibility.
4. **No DORA operational resilience procedures.** TLPT testing, ICT third-party register, etc. 🟡 MED (DORA is already in force).
5. **No appropriateness assessment questionnaire** (MiCA Art. 71(2)). Risk warning exists but not the pre-trade experience assessment. 🟡 MED.

**Recommendation.**
- **P0:** Postgres-backed KYC record store. Schema with `user_id`, `status`, `tier`, `documents`, `verified_at`, `provider_ref`, `last_refresh`. Migrate `KycRegistry` behind a trait. Effort: M.
- **P1:** Travel Rule pilot integration. Notabene has the broadest VASP network; they offer a REST API. Integrate at the withdrawal handler — if destination is a VASP, push IVMS 101 payload to Notabene before broadcasting. Effort: L — ~2 weeks.
- **P1:** On-chain PoR. Add `OnChainAttester` trait with `BtcRpcAttester` (query hot/warm/cold BTC addresses via a public Bitcoin Core RPC), `EthRpcAttester` (same for ETH via Infura/Alchemy/self-hosted). Report format: liability Merkle root + assets per address + signed attestation. Effort: L — ~3 weeks for BTC/ETH, longer for full asset coverage.
- **P1:** Appropriateness assessment questionnaire. MiCA Art. 71(2) is prescriptive; we can translate it directly into a form. Store responses, derive an experience score, gate access to leverage/derivatives on score. Effort: M.
- **P2:** DORA ICT register and TLPT procurement. Effort: L operationally.

### 2.8 Testing strategy

**Industry standard.** Property-based testing (proptest/quickcheck/Hypothesis) for invariants — sum of balances constant, no negative balance, place-then-cancel idempotent. Jepsen-style (Kyle Kingsbury) for consistency — linearizability checking via Knossos, weaker models via Elle. Fault injection / chaos engineering — Chaos Mesh, Toxiproxy, game days. Load testing with coordinated-omission-correct tooling (wrk2 / HdrHistogram — Gil Tene "How NOT to Measure Latency"). Formal verification (TLA+) for protocols, typically limited to consensus and settlement-finality primitives (AWS uses it on S3/DynamoDB/EBS per Newcombe et al. CACM 2015).

*Sources:* `proptest`/`quickcheck` Rust crates docs, Hypothesis (David MacIver), Scott Wlaschin "Choosing properties for property-based testing", John Hughes QuickCheck papers. Jepsen reports at jepsen.io. "Elle: Inferring Isolation Anomalies" (Kingsbury & Alvaro, VLDB 2020). Netflix Chaos Monkey blog. Gil Tene talk on coordinated omission. Leslie Lamport *Specifying Systems*. Newcombe et al. "How Amazon Web Services Uses Formal Methods" CACM 2015. TigerBeetle design docs.

**Our state.**
- ✅ 2163+ tests across workspace.
- ✅ Property-based tests for matching: `crates/matching-engine/tests/proptest_matching.rs` — fill qty ≤ order qty, price range validity, book invariant, WAL replay determinism.
- ✅ Stress test: 100K mixed ops with invariant checks — `crates/matching-engine/tests/stress.rs`.
- ✅ Criterion benches: throughput, deep_book, call_auction, market_maker.
- ✅ Integration tests for 4 of the admin endpoints added this session.
- ✅ Runbook accuracy CI guard — `crates/api-gateway/tests/runbook_accuracy.rs`.

**Gap.**
1. **No property tests on account / settlement crates.** The most load-bearing invariant — sum-of-balances conservation across arbitrary transfer sequences — is NOT proptest-covered. 🔴 HIGH.
2. **No load test harness.** Criterion benches are microbenchmarks, not sustained-load profiles. No burst, no soak, no adversarial. 🟡 MED.
3. **No coordinated-omission-correct tooling.** Any ad-hoc wrk/curl loop under-reports tail latency. 🟡 MED.
4. **No chaos / fault injection.** No toxiproxy, no process-kill tests. 🟡 MED.
5. **No mutation testing.** High test count ≠ high effective coverage. 🟢 LOW — aspirational.
6. **No Jepsen.** Not applicable to a monolith; becomes relevant only after service decomp. 🟢 LOW.

**Recommendation.**
- **P1:** `crates/account/tests/proptest_invariants.rs`. Generate random `Vec<Transfer>` sequences, apply them, assert sum-of-balances is constant. Generate random deposit/withdraw/lock/unlock sequences, assert no negative balance. Effort: S — ~2 days for the core properties. This is the single most valuable test we don't have.
- **P1:** `crates/journal/tests/proptest_wal_replay.rs`. Generate random command sequences, append to WAL, replay, assert engine state is identical. Effort: S.
- **P2:** Load test harness using `wrk2` (or `k6` with coordinated-omission disabled) covering 4 profiles: sustained-baseline, burst-10x-30s, linear-ramp, 24h-soak. Outputs HdrHistogram. Effort: M.
- **P2:** Game day #1 — kill matching engine for BTCUSDT during sustained load, observe recovery, update runbook. Effort: S per drill.
- **P3:** `loom` tests for account sharding concurrency paths. Effort: M.
- **P3:** `cargo-mutants` mutation testing — point-sample 5 critical modules to calibrate effective coverage. Effort: S.

---

## 3. Prioritized Backlog

Items in priority order. P0 = must-fix (weeks), P1 = should-fix (1–2 months), P2 = nice-to-have (3–6 months), P3 = research / deferred.

| # | Concern | Item | P | Effort |
|---|---------|------|---|--------|
| 1 | Durability | Fail-fast on WAL replay error (`main.rs:524`) | **P0** | S |
| 2 | Durability | Audit `group_commit.rs` fsync error path — panic on EIO | **P0** | S |
| 3 | Security | Durable audit log (Postgres + hash chain + S3 archive) | **P0** | M |
| 4 | Regulatory | Postgres-backed KYC record store | **P0** | M |
| 5 | Control plane | Audit log entry on every feature-flag change | **P0** | S |
| 6 | Testing | Property tests on `crates/account` — sum invariant | **P1** | S |
| 7 | Testing | Property tests on WAL replay determinism | **P1** | S |
| 8 | Durability | WAL segment rotation | **P1** | M |
| 9 | Durability | Periodic engine snapshot + replay from snapshot | **P1** | L |
| 10 | Durability | Document RPO/RTO targets | **P1** | S |
| 11 | Observability | `docs/slo.md` + 5 declared SLIs | **P1** | S |
| 12 | Observability | Burn-rate Alertmanager rules + runbook URL embed | **P1** | M |
| 13 | Control plane | `OperatingMode` enum (normal/read-only/cancel-only/halted) | **P1** | M |
| 14 | Control plane | Full graceful shutdown coordinator | **P1** | M |
| 15 | Control plane | Automated kill-switch test | **P1** | S |
| 16 | Security | `SecretProvider` trait + Fireblocks key via KMS | **P1** | M |
| 17 | Resilience | `docs/fmea.md` top-20 failure modes | **P1** | L |
| 18 | Resilience | Task dependency order assertion in startup | **P1** | M |
| 19 | Regulatory | Travel Rule pilot (Notabene) | **P1** | L |
| 20 | Regulatory | On-chain PoR for BTC + ETH | **P1** | L |
| 21 | Regulatory | Appropriateness assessment questionnaire | **P1** | M |
| 22 | Security | Four-eyes on irreversible admin ops | **P2** | M |
| 23 | Observability | Grafana dashboards as code | **P2** | M |
| 24 | Testing | Load test harness with wrk2 | **P2** | M |
| 25 | Testing | Quarterly game day rotation | **P2** | S/drill |
| 26 | Resilience | Task heartbeat + watchdog | **P2** | M |
| 27 | Regulatory | DORA ICT register + TLPT procurement | **P2** | L |
| 28 | Security | FIDO2 WebAuthn wrapper on admin | **P3** | L |
| 29 | Observability | OpenTelemetry OTLP exporter | **P3** | M |
| 30 | Testing | `loom` concurrency state-space for sharding | **P3** | M |
| 31 | Latency | CPU pinning for engine threads | **P3** | S |
| 32 | Latency | rtrb SPSC swap for matching channel | **P3** | M |
| — | Architecture | Service decomposition into microservices | **Defer** | XL |

---

## 4. What we are deliberately NOT doing and why

These are optimizations or practices that appear in the research but are **not justified at our current scale / constraints**:

1. **Kernel bypass (DPDK, Solarflare OpenOnload, AF_XDP).** Solarflare claims ~1–3µs round-trip TCP vs ~15–30µs stock Linux; DPDK pushes sub-µs for UDP. Our bench shows p50 ≈ 56µs end-to-end with public-internet RTTs dominating real user experience. Kernel bypass saves µs; our users lose ms to transit. Not justified until we have co-located institutional clients.

2. **Full SBE binary order entry protocol (S21 on the optimization backlog).** No named pilot client. FIX 4.4 is already shipping. Adding SBE without an institutional asking for it is engineering for an imaginary customer. Deferred with explicit prerequisites in `docs/TASK_TRACKER.md`.

3. **Thread-per-core framework (Glommio/Seastar/monoio).** Redpanda cites ~5ms p99.99 vs Kafka's tens of ms. Valuable above ~500K ops/sec sustained. We're at 42K. Over-engineering.

4. **Service decomposition into microservices.** Conway's law drives real decomposition and we have a 1-person team. No scaling pain. gRPC is not exchange-grade for hot paths (HTTP/2 + protobuf codec overhead). Proper exchange-grade inter-service transports (Aeron, SBE-over-TCP, UDP multicast + TCP snapshot, Redpanda for durable events) are a 3-month research track in themselves. A well-structured monolith with clean crate boundaries — which we already have — is the correct architecture until we have measured pain.

5. **Physical co-location (LD4, FR2, NY4, TY3, SG1).** Crypto retail + bot traders connect over public internet. Co-location is justified for trad-fi equity market makers, not for us.

6. **Full thread-per-socket / NUMA-aware allocation.** We don't know our deployment topology; we're not running on a 2-socket box with multiple NICs. Premature.

7. **Formal verification with TLA+ / Coq / Lean.** AWS uses TLA+ on S3/DynamoDB/EBS replication protocols. Valuable for consensus and distributed-transaction protocols. We don't have distributed transactions — our WAL + double-entry are simpler models that property-based testing covers adequately. Revisit if we decompose.

8. **Jepsen testing.** Jepsen finds bugs in distributed systems; we're a monolith. Revisit after service split, if it happens.

9. **On-chain PoR for every asset on day one.** Start with BTC (Bitcoin Core RPC) and ETH (Alchemy/Infura) — they cover ~80% of user value. Other chains follow as volume justifies.

---

## 5. Sources

Cited by name. No fabricated URLs. Primary sources are linked from the noted author or organization.

**Books & standards**
- Google SRE Book and SRE Workbook (Beyer, Jones, Petoff, Murphy et al.; O'Reilly / free at sre.google/books)
- Brendan Gregg, *Systems Performance* (USE method)
- Michael Nygard, *Release It!* 2nd ed. (circuit breakers, bulkheads, steady state)
- Martin Kleppmann, *Designing Data-Intensive Applications*
- Casey Rosenthal & Nora Jones, *Chaos Engineering*
- Leslie Lamport, *Specifying Systems* (TLA+)
- Joe Armstrong, "Making Reliable Distributed Systems in the Presence of Software Errors" (PhD thesis, 2003)

**Papers & talks**
- Thompson, Barker, Farooqui, Gee, Stewart, "Disruptor: High Performance Alternative to Bounded Queues" (LMAX, 2011)
- Carl Cook (Optiver), "When a Microsecond Is an Eternity" (CppCon 2017)
- Martin Fowler, "The LMAX Architecture" (2011)
- Newcombe et al., "How Amazon Web Services Uses Formal Methods" (CACM, 2015)
- Kingsbury & Alvaro, "Elle: Inferring Isolation Anomalies from Experimental Observations" (VLDB 2020)
- Google Dapper paper (distributed tracing)
- Gil Tene, "How NOT to Measure Latency"

**Regulations**
- Regulation (EU) 2023/1114 (MiCA); 2023/1113 (TFR / Travel Rule); 2022/2554 (DORA)
- ESMA RTS 6 / Commission Delegated Regulation 2017/589 (MiFID II algo trading)
- FATF Updated Guidance on Virtual Assets (October 2021)
- IVMS 101 (intervasp.org)
- FinCEN BSA / OFAC SDN list
- NIST SP 800-53 (access controls), SP 800-57 (key management), SP 800-92 (log management)

**Protocol specs**
- NASDAQ TotalView-ITCH 5.0, OUCH 5.0
- CME iLink 3 / MDP3
- FIX 4.4 / 5.0 SP2 (FIX Trading Community)
- SBE (Simple Binary Encoding, Real Logic)

**Blogs & engineering pages**
- Martin Thompson, mechanical-sympathy.blogspot.com
- Peter Lawrey, OpenHFT / Chronicle Queue
- Jane Street blog (accounting invariants — Yaron Minsky, Ron Minsky)
- Stripe Engineering blog (idempotency — Brandur Leach)
- Netflix Tech Blog (Chaos Monkey origins)
- Coinbase Engineering, Kraken Security, Cloudflare blog (incident postmortems)
- Charity Majors / Honeycomb (wide events, observability)
- Erik Rigtorp (low-latency C++ / NUMA)
- Aphyr / Kyle Kingsbury (jepsen.io)
- Google SRE book chapters on SLO, alerting, postmortems (Rob Ewaschuk's "My Philosophy on Alerting")

**Postmortems / incident reports** (for FMEA research)
- Mt. Gox, QuadrigaCX, Coincheck, FTX Chapter 11 (John Ray III declaration)
- PostgreSQL fsyncgate (LWN April 2018 — Jonathan Corbet; Craig Ringer, Andres Freund follow-ups)
- Public Coinbase, Kraken, Binance, Gemini status-page postmortems

**Rust ecosystem**
- `parking_lot`, `crossbeam`, `rtrb`, `flume`, `glommio`, `tokio`, `tracing`, `opentelemetry-rust`
- `proptest`, `quickcheck`, `loom`, `madsim`, `turmoil`
- `metrics` / `metrics-exporter-prometheus`, `prometheus-client`
- `core_affinity`, `nix::sched`
- `tokio-graceful-shutdown`

---

## 6. Caveats on the research itself

- **External research agents used web search and prior knowledge**, and flagged claims they could not verify with `[UNVERIFIED]`. Before quoting a specific latency figure or feature in a public document, re-verify against the primary source.
- **The internal audit agent had several errors** cross-checked against the code: it claimed `/health` endpoint and runbooks were missing (both exist — `server.rs:1146` and `docs/runbooks/` respectively) and claimed 982 `unwrap_or_else(|e| e.into_inner())` sites were a correctness issue (they are cosmetic debt given `panic = "abort"` in release). Treat the audit as a starting point, not a final word.
- **Numeric latency figures from third-party venues** (LMAX's 50µs, Carl Cook's 1µs, NASDAQ's single-digit µs) are from talks and marketing, not peer-reviewed papers. Useful as order-of-magnitude references only.
- **This document is an audit, not a plan.** Implementation of any P0/P1 item requires its own design review and test plan before code goes in.

---

## 7. Next action

Review this document, select which P0 items to execute first (recommended order: #1–#2 WAL correctness fixes, then #3 durable audit log, then #4 KYC persistence, then #5 flag-change audit). Each P0 is 1–3 days of focused work with tests. After P0 clears, the P1 list has ~16 items spanning 6–8 weeks of focused work — picking 3–4 per sprint is realistic.
