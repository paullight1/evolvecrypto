# Production Roadmap

**Document type:** Cross-team synthesis. Findings from four specialist audits (Architecture, Security, Operations, Compliance) consolidated into a single prioritized delivery plan.

**Date:** 2026-03-27
**Current scores:** Architecture 5.8/10 · Security C · Operations 2.4/5 · Compliance 2.7/5
**Verdict:** Not production-ready. Strong core logic, critical gaps in every operational layer.

---

## Executive Summary

The exchange has a solid trading core: 24 crates, 2345 Rust tests, correct matching semantics, proper double-entry ledger, and a working web terminal. The financial logic for perpetuals, options, liquidations, and funding rates is documented with references and largely correct.

What is missing is the operational envelope that turns a correct program into a safe, auditable, regulatorily-compliant service. The four audits surfaced gaps in four layers that compound each other:

- **Architecture:** `place_order()` holds 20+ sequential locks — throughput ceiling is ~1 000 orders/sec. WAL fsync blocks Tokio threads. Unbounded in-memory state will OOM under load.
- **Security:** Admin and private routes have hard-coded dev bypasses that are not gated by build flags. API key secrets are in plaintext. JWT uses a symmetric secret. Withdrawal does not enforce address whitelist.
- **Operations:** No alerting rules. Plain-text logs. Background tasks can die silently. `deploy.sh` overwrites production `.env` with the example file. Custody is `MockProvider`.
- **Compliance:** Risk warnings do not gate futures order entry (MiCA Art. 71). Travel Rule originator is a UUID, not a legal name (FATF R.16). SAR tipping-off prevention is absent (AMLD5 Art. 39 — criminal liability). OFAC sanctions list is not auto-refreshed (strict liability, up to $20M/violation).

The roadmap below sequences fixes in four phases. Phase 0 must complete before any external user touches the system. Phase 1 must complete before accepting real money. Phases 2 and 3 address scale and enterprise requirements.

---

## Prioritization Framework

- **P0 — Blocker:** Financial loss, criminal liability, data corruption, or a bypass that lets unauthenticated actors reach sensitive operations.
- **P1 — Pre-money:** Required before the first real deposit lands. Regulatory violation or fund loss if absent.
- **P2 — Scale gate:** Safe at low volume, breaks or becomes a liability at 10K+ users.
- **P3 — Enterprise:** Multi-node, full institutional compliance, SLA-grade operations.

Effort sizes: **S** < 1 day · **M** 1–3 days · **L** 4–7 days · **XL** 2+ weeks

Owner categories: **BE** = Backend Rust · **DO** = DevOps/Infrastructure · **CO** = Compliance/Legal · **FE** = Frontend

---

## Phase 0 — Blockers (target: 2 weeks)

These items represent conditions under which the system must not be exposed to real users. They are either direct paths to fund loss, criminal liability, or active bypasses of security controls.

### P0-1 — Remove hard-coded auth bypasses

**Risk:** P0 — any attacker who discovers the nil-user bypass gets full admin and trade access.

The admin router and private API both allow requests through when `DEV_MODE` is active or when the key store is absent. This logic is present in production-compiled code with no build-flag guard. A misconfigured deployment (missing environment variable) silently disables authentication.

**Files:**
- `crates/api-gateway/src/auth.rs` — nil-user fallback and absent-keystore bypass
- `crates/api-gateway/src/rest_v2.rs` — admin route guards
- `crates/api-gateway/src/security.rs` — key store initialization path

**Work:**
1. Replace nil-user fallback with an explicit `401 Unauthorized`. Remove the `if user.is_nil()` passthrough entirely.
2. Gate `DEV_MODE` bypasses behind `#[cfg(debug_assertions)]`. If `PRODUCTION=true`, any bypass-enabling path must `panic!` at startup, not at request time.
3. Add an integration test that verifies `401` is returned with no token in both release and debug builds (release: always; debug: still returns 401, just with a dev secret).

**Effort:** M · **Owner:** BE

---

### P0-2 — Enforce withdrawal address whitelist

**Risk:** P0 — a compromised account can withdraw to any address. Funds are irreversibly lost.

`process_withdrawal()` has the whitelist data structure and the per-user address store, but the enforcement check is not called in the withdrawal hot path.

**Files:**
- `crates/custody/src/lib.rs` — `process_withdrawal()` entry point
- `crates/account/src/account_manager.rs` — whitelist storage
- `crates/api-gateway/src/rest_v2.rs` — withdrawal REST handler

**Work:**
1. Before submitting to `CustodyProvider`, look up the destination address in the user's whitelist. Return `403 Forbidden` with a specific error code if absent.
2. New address added to whitelist starts with `status = pending`, becomes `active` after 24h. Reject withdrawals to `pending` addresses.
3. Add test: withdrawal to non-whitelisted address returns 403; withdrawal to whitelisted address proceeds.

**Effort:** M · **Owner:** BE

---

### P0-3 — Fix `deploy.sh` overwriting production `.env`

**Risk:** P0 — re-deploying wipes the production configuration, replacing secrets with example values. All `JWT_SECRET`, `WEBHOOK_SECRET`, and database credentials are lost. The exchange either panics (if `PRODUCTION=true` detects missing vars) or runs with example values (far worse).

**Files:**
- `deploy/deploy.sh`

**Work:**
1. Replace `cp .env.example .env` (or equivalent) with a check: if `.env` exists, skip. Print a warning reminding the operator to review `.env.example` for new required variables.
2. Add a `--force-env-reset` flag for intentional resets that requires explicit confirmation.
3. Add a CI step that greps `deploy.sh` for any `cp .env.example` pattern and fails the build.

**Effort:** S · **Owner:** DO

---

### P0-4 — SAR tipping-off prevention

**Risk:** P0 — criminal liability under AMLD5 Art. 39. Informing a subject that a SAR has been filed is a criminal offence in all EU jurisdictions.

There is currently no mechanism to suppress account closure notifications, withdrawal rejection reasons, or any user-facing message that could reveal a SAR has been filed. Standard practice: generic "regulatory hold" or "account under review" messages that reveal nothing.

**Files:**
- `crates/compliance/src/aml.rs` — SAR filing logic
- `crates/compliance/src/security_hold.rs` — account freeze and hold messages
- `crates/admin/src/notifications.rs` — user-facing notification text

**Work:**
1. Introduce a `SarSubject` flag on accounts. When set, all user-facing messages related to the account action use only generic "account under review" copy.
2. Audit every code path that sends a notification on freeze/hold/rejection and route through a `compliance_safe_message()` function that checks the flag.
3. Suppress any automated emails that would reveal the nature of the restriction.
4. Add a test: filing a SAR sets the flag; subsequent account-action notifications return generic text.

**Effort:** M · **Owner:** BE + CO

---

### P0-5 — Wire real CustodyProvider (or formally gate all money movement)

**Risk:** P0 — `MockCustodyProvider` silently succeeds on all deposit and withdrawal calls. No real funds move. Users who attempt to deposit will see a credited balance with no corresponding on-chain transaction. Withdrawals are silently dropped.

**Files:**
- `crates/custody/src/lib.rs` — `MockCustodyProvider`
- `crates/server/src/main.rs` — provider initialization

**Work (two acceptable paths):**
- **Path A (preferred):** Integrate `mg-custody` service. `CustodyProvider` implementation calls the custody API over HTTP. Wire deposit webhook verification. Wire withdrawal submission.
- **Path B (temporary gate):** Keep `MockCustodyProvider` but make all deposit/withdrawal endpoints return `503 Service Unavailable` with body `{"error": "custody_not_configured"}` when the provider is the mock. Add a startup log line: `WARN custody is MockProvider — deposits and withdrawals are disabled`. This prevents silent fund loss while Path A is in progress.

Path B can ship immediately (S effort) as a safety gate. Path A is the real fix (XL effort, external dependency).

**Effort:** S (gate) + XL (real integration) · **Owner:** BE + DO

---

### P0-6 — Background task supervision with panic restart

**Risk:** P0 — `funding_settler`, `liquidation_monitor`, and other background tasks can panic and exit silently. If `funding_settler` dies, funding payments stop accruing — this is a financial correctness bug. If `liquidation_monitor` dies, underwater positions are not liquidated — the insurance fund absorbs losses that should have been caught.

**Files:**
- `crates/api-gateway/src/tasks.rs` — task spawn sites

**Work:**
1. Wrap each `tokio::spawn(async move { ... })` call for critical financial tasks in a supervisor loop: if the task returns (panic or normal exit), log `ERROR task {name} exited — restarting`, sleep 1 second, and re-spawn.
2. Expose a `background_task_alive_{name}` gauge metric per task. Set to 1 on spawn, 0 on exit.
3. Add an alert rule (see P0-7) that fires when any critical task gauge drops to 0.

**Effort:** M · **Owner:** BE + DO

---

### P0-7 — Minimum viable alerting rules

**Risk:** P0 — metrics are emitted but no alert rules exist. An incident (task death, OOM, high error rate, WAL failure) produces no page, no email, no Slack message. Incidents are discovered by users.

**Files:**
- `deploy/` — add `alerts.yml` for Prometheus AlertManager
- `docker-compose.yml` — wire AlertManager

**Minimum rule set (must go live with Phase 0):**

| Alert | Condition | Severity |
|-------|-----------|----------|
| `BackgroundTaskDead` | `background_task_alive_* == 0` for 30s | critical |
| `HighOrderErrorRate` | `order_errors / order_attempts > 0.05` for 1m | critical |
| `WalWriteFailure` | `wal_write_failures_total > 0` | critical |
| `HighMemoryUsage` | `process_resident_memory_bytes > 6GB` | warning |
| `DBConnectionPoolExhausted` | `db_pool_available == 0` for 30s | critical |
| `LiquidationTaskLag` | liquidation check interval > 10s | warning |

**Effort:** M · **Owner:** DO

---

### P0-8 — Fix stale position snapshot in liquidation monitor

**Risk:** P0 — financial bug. The liquidation monitor takes a snapshot of all positions and then iterates. During iteration, positions may change (new fills, funding payments). A position that was margined-in when snapshotted but becomes underwater during the loop iteration can be double-processed on the next tick, leading to double-ADL of uninvolved counterparties.

**Files:**
- `crates/risk-engine/src/liquidation.rs` — monitor loop and snapshot logic
- `crates/risk-engine/src/position_manager.rs` — position read path

**Work:**
1. Each position evaluated for liquidation must be re-read under lock immediately before the liquidation decision, not from the snapshot.
2. The snapshot is acceptable for the initial scan (identify candidates). The final decision must use a fresh read with a compare-and-swap: liquidate only if `position.margin_ratio` is still below threshold at decision time.
3. Add a test: position becomes healthy between scan and decision — liquidation is skipped.

**Effort:** M · **Owner:** BE

---

## Phase 1 — Foundation (target: 4 weeks after Phase 0)

Items that must be complete before accepting real money. A missing item here represents regulatory non-compliance, significant security risk, or operational inability to respond to incidents.

### P1-1 — Structured JSON logging

**Risk:** P1 — plain-text logs cannot be aggregated, searched, or alerted on. Incident response in production without structured logs adds 30–60 minutes to every incident timeline.

**Files:**
- `crates/server/src/main.rs` — `tracing_subscriber` initialization
- All crates (no code changes — format is configured at the subscriber level)

**Work:**
1. Replace `tracing_subscriber::fmt()` with `tracing_subscriber::fmt().json()` when `PRODUCTION=true` or `LOG_FORMAT=json`.
2. Add `request_id` to every HTTP request via a `tower` middleware layer. Propagate through all downstream log events using `tracing::Span`.
3. Configure log shipping to a log aggregator (Loki, DataDog, CloudWatch — operator's choice). Add the shipper to `docker-compose.yml` as an optional profile.

**Effort:** M · **Owner:** DO + BE

---

### P1-2 — Risk warnings gate futures order entry (MiCA Art. 71)

**Risk:** P1 — regulatory. MiCA Art. 71 requires that retail clients acknowledge specific risk warnings before placing leveraged orders. Accepting a leveraged order from a user who has not seen the required disclosures is a compliance violation.

**Files:**
- `crates/compliance/src/lib.rs` — appropriateness assessment
- `crates/order-gateway/src/validator.rs` — pre-trade validation
- `crates/api-gateway/src/rest_v2.rs` — order placement handler
- `clients/web/src/features/trading/` — acknowledgement UI

**Work:**
1. Add `futures_risk_acknowledged_at: Option<DateTime>` to the user record.
2. In `order-gateway` validator: if order is for a perpetual or delivery futures instrument and `futures_risk_acknowledged_at` is `None`, reject with `403` and error code `RISK_WARNING_REQUIRED`.
3. Add REST endpoint `POST /compliance/risk-acknowledgement` that sets the timestamp.
4. Web client: intercept `RISK_WARNING_REQUIRED` error, show modal with required MiCA disclosures, submit acknowledgement, then re-submit the order.

**Effort:** M · **Owner:** BE + CO + FE

---

### P1-3 — OFAC / sanctions list auto-refresh

**Risk:** P1 — OFAC strict liability. Every transaction with a sanctioned entity is a violation regardless of knowledge. Fine: up to $1M per transaction, aggregate up to $20M. The list must be current.

**Files:**
- `crates/compliance/src/sanctions.rs` — list storage and check logic
- `crates/compliance/src/ofac_parser.rs` — OFAC XML parser
- `crates/api-gateway/src/tasks.rs` — background task spawn

**Work:**
1. Add a background task that fetches the OFAC SDN list from `https://www.treasury.gov/ofac/downloads/sdn.xml` on a configurable interval (default: every 6 hours).
2. On fetch failure: log `ERROR`, increment a `sanctions_refresh_failures_total` counter, continue using the existing list. Do not crash.
3. On fetch success: parse and atomically swap in the new list (swap an `Arc<SanctionsList>` behind an `ArcSwap`).
4. Add a startup check: if the list was last refreshed more than 48 hours ago, log `WARN` and emit a metric. Alert on this metric (add to P0-7 alert rules).
5. Add a `GET /admin/compliance/sanctions/status` endpoint returning last refresh timestamp, list size, and next scheduled refresh.

**Effort:** M · **Owner:** BE + DO

---

### P1-4 — Travel Rule: originator legal name (FATF R.16)

**Risk:** P1 — FATF R.16 requires travel rule messages to contain the originator's legal name, not a system-internal identifier. Transmitting a UUID as the originator name means every inter-VASP message is non-compliant. Regulators classify this as a systemic deficiency.

**Files:**
- `crates/compliance/src/lib.rs` — Travel Rule message construction
- `crates/account/src/account_manager.rs` — KYC data access

**Work:**
1. When constructing a Travel Rule message, resolve the originator user ID to their verified legal name from the KYC record. If KYC is not complete, block the withdrawal with error `KYC_REQUIRED_FOR_TRANSFER`.
2. Add a test: Travel Rule message for a KYC-verified user contains `first_name + last_name`; for an unverified user, withdrawal is rejected.

**Effort:** S · **Owner:** BE + CO

---

### P1-5 — Fix WAL write failure silent ignore

**Risk:** P1 — the WAL is the source of truth for crash recovery. If a WAL write fails and the in-memory state is updated anyway, a crash at that point produces an irrecoverable inconsistency. Orders are matched without a recovery record.

**Files:**
- `crates/journal/src/lib.rs` — write path and `wal_healthy` flag

**Work:**
1. On WAL write failure: set `wal_healthy = false`, reject the order that triggered the write with `503 Service Unavailable`, increment `wal_write_failures_total` counter.
2. Add a recovery path: when `wal_healthy` is false, drain in-flight orders, then attempt WAL re-open. If re-open succeeds, set `wal_healthy = true` and resume.
3. The `wal_write_failures_total > 0` alert from P0-7 will fire immediately.

**Effort:** M · **Owner:** BE

---

### P1-6 — API key secrets: replace plaintext with HMAC-SHA256 hashed storage

**Risk:** P1 — plaintext secrets in memory means any memory dump (heap profiler, core dump, `/proc/self/mem`) leaks all active API keys. API keys grant trade and withdrawal access.

**Files:**
- `crates/api-gateway/src/auth.rs` — key storage and verification
- `crates/api-gateway/src/security.rs` — key creation

**Work:**
1. On key creation: generate the secret, return it to the user once (never again), store only `HMAC-SHA256(secret, per-key salt)`.
2. On key verification: compute `HMAC-SHA256(provided_secret, stored_salt)` and compare with constant-time equality.
3. Migrate existing keys: on next use, re-hash and update. Keys not used within 90 days are revoked.
4. Do not log API key values at any log level.

**Effort:** M · **Owner:** BE

---

### P1-7 — WebSocket: authentication rate limit and session timeout

**Risk:** P1 — unauthenticated WebSocket upgrade attempts have no rate limit. An attacker can exhaust Tokio tasks with connection floods. Sessions have no timeout — a leaked token remains valid indefinitely for WS connections.

**Files:**
- `crates/api-gateway/src/ws.rs` — upgrade handler and session management

**Work:**
1. Rate limit WS auth attempts per IP: 10 failures per minute → 60-second ban.
2. Add session timeout: if no message (ping or subscribe) is received within 5 minutes, close the connection with `4001 Session Timeout`.
3. Add a `ws_active_sessions` gauge metric.

**Effort:** M · **Owner:** BE

---

### P1-8 — Security event log persistence

**Risk:** P1 — the in-memory ring buffer for security events (failed logins, suspicious patterns, freeze actions) is lost on every restart. Post-incident forensics and regulatory audit logs are unavailable. AMLD5 requires a 5-year retention period for compliance-relevant events.

**Files:**
- `crates/surveillance/src/lib.rs` — event log storage
- `crates/db/` — add a `security_events` table and repository

**Work:**
1. Write every security event to the `security_events` table in addition to the in-memory ring buffer.
2. Index on `(user_id, created_at)` and `(event_type, created_at)`.
3. Add `GET /admin/security/events` with pagination, time range, and event type filters.
4. Retain events for 5 years (add a DB-level policy or a quarterly archival job).

**Effort:** M · **Owner:** BE

---

### P1-9 — DB connection pool sizing

**Risk:** P1 — the default `sqlx` pool of 10 connections serves 12+ background tasks plus HTTP request handlers. Under any real load the pool exhausts, all tasks that need the DB block, and the exchange grinds to a halt. This includes liquidation and funding settlement.

**Files:**
- `crates/db/` — pool initialization
- `.env.example` — document the configuration variable

**Work:**
1. Set pool minimum: 20, maximum: 50 (configurable via `DB_POOL_MIN` / `DB_POOL_MAX`).
2. Add `db_pool_available` and `db_pool_size` gauge metrics.
3. The `DBConnectionPoolExhausted` alert from P0-7 will fire when exhausted.

**Effort:** S · **Owner:** DO + BE

---

### P1-10 — Unhosted wallet ownership verification (MiCA Art. 68(5))

**Risk:** P1 — MiCA Art. 68(5) requires CASPs to verify that a user controls an unhosted wallet before crediting a deposit from or processing a withdrawal to that address. Non-compliance blocks MiCA authorization.

**Files:**
- `crates/custody/src/lib.rs` — deposit and withdrawal handlers
- `crates/compliance/src/address_verification.rs` — existing file

**Work:**
1. For withdrawal to a new address: require the user to sign a challenge with the private key of the destination address (or provide a self-declaration for small amounts under MiCA threshold).
2. For deposits: on first deposit from an address, flag for ownership verification. Credit the deposit but restrict withdrawal of those funds until verification is complete.
3. Add a `wallet_verifications` table tracking address, user, method, and status.
4. Add `GET /compliance/wallet-verification/status` and `POST /compliance/wallet-verification/submit` endpoints.

**Effort:** L · **Owner:** BE + CO

---

### P1-11 — FIX gateway: restrict to authenticated networks

**Risk:** P1 — FIX port 8081 is exposed publicly in the Docker configuration. FIX has weaker authentication than REST. Institutional clients should access FIX over VPN or dedicated private network, not the public internet.

**Files:**
- `docker-compose.yml` — port binding
- `crates/fix-gateway/src/tcp_server.rs` — connection acceptance

**Work:**
1. Change the Docker port binding from `0.0.0.0:8081` to `127.0.0.1:8081` (or an internal network address). FIX is not a public service.
2. Add IP allowlist enforcement at the TCP accept level: connections from IPs not in `FIX_ALLOWED_IPS` are dropped immediately.
3. Document in the runbook how to provision FIX access for institutional clients.

**Effort:** S · **Owner:** DO

---

### P1-12 — Grafana: remove hardcoded default password

**Risk:** P1 — the `docker-compose.yml` has `GF_SECURITY_ADMIN_PASSWORD=exchange` hardcoded. The Grafana instance exposes all operational metrics including trade volumes, user counts, and system health. Default credentials are the first thing automated scanners try.

**Files:**
- `docker-compose.yml`

**Work:**
1. Replace the hardcoded value with `${GRAFANA_ADMIN_PASSWORD:?GRAFANA_ADMIN_PASSWORD must be set}`. The `:?` syntax causes Docker Compose to fail fast if the variable is absent.
2. Add `GRAFANA_ADMIN_PASSWORD=` (empty, requiring the operator to set it) to `.env.example` with a comment.

**Effort:** S · **Owner:** DO

---

### P1-13 — SAR 30-day filing deadline tracking

**Risk:** P1 — regulatory. FATF and EU AMLD5 require SARs to be filed within 30 days of suspicion arising. Without deadline tracking, SARs are filed late or missed. Late filing is a compliance violation subject to regulatory action.

**Files:**
- `crates/compliance/src/aml.rs` — SAR creation
- `crates/api-gateway/src/tasks.rs` — background tasks

**Work:**
1. Add `sar_deadline: DateTime` (30 days from `suspicious_activity_detected_at`) to the SAR record.
2. Add a background task that runs daily: query SARs with `status = pending` and `deadline < now + 3 days`. Log `WARN sar_deadline_approaching sar_id={} deadline={}` and send an internal alert.
3. Add `GET /admin/compliance/sar/pending` with the deadline field visible.

**Effort:** S · **Owner:** BE + CO

---

## Phase 2 — Scale (target: 8 weeks after Phase 1)

Items that are safe at low volume but become operational liabilities or compliance gaps at 10K+ users.

### P2-1 — Break `place_order()` lock chain: account sharding

**Risk:** P2 — the current sequential acquisition of 20+ `Mutex` locks in `place_order()` creates a global serialization point. Throughput ceiling is ~1 000 orders/sec. At 10K users trading actively, this is hit within minutes.

**Files:**
- `crates/api-gateway/src/exchange.rs` — `Exchange` struct and `place_order()`
- `crates/account/src/account_manager.rs` — balance locks

**Work:**
1. Shard account balances by user ID: replace the single `Mutex<HashMap<UserId, Balance>>` with `[Mutex<HashMap<UserId, Balance>>; N_SHARDS]` where `N_SHARDS = 64`.
2. `place_order()` acquires only the shard lock for the user's account, not a global lock.
3. Cross-user operations (transfers, referral commissions) acquire shards in a deterministic order (lower shard ID first) to avoid deadlock.
4. Benchmark: throughput must exceed 5 000 orders/sec on a 4-core machine after this change.

**Effort:** L · **Owner:** BE

---

### P2-2 — WAL: group commit with async fsync

**Risk:** P2 — synchronous `fsync` per order on the WAL blocks the calling Tokio thread. At 200+ orders/sec, this saturates `io_uring` or the kernel's write queue and stalls the runtime. The theoretical max is 2 000 orders/sec on NVMe; real disks are slower.

**Files:**
- `crates/journal/src/lib.rs` — write and sync path

**Work:**
1. Implement group commit: collect all WAL entries queued within a configurable window (default 1ms) and issue a single `fsync` for the batch.
2. Each order waits on a `tokio::sync::oneshot` channel. The group commit task resolves all waiting channels after the `fsync` completes.
3. Use `tokio::task::spawn_blocking` for the `fsync` call so it runs on the blocking thread pool, not the async runtime.
4. Benchmark: target 10 000 committed writes/sec on NVMe; 2 000 on HDD.

**Effort:** L · **Owner:** BE

---

### P2-3 — Bound in-memory state: user_fills, sessions, order books

**Risk:** P2 — `HashMap`s keyed by user ID grow without bound. At 100K users each with 1 000 fill records, `user_fills` alone is ~500MB. The exchange will OOM and crash.

**Files:**
- `crates/api-gateway/src/exchange.rs` — all `HashMap` fields
- `crates/matching-engine/src/orderbook.rs` — completed order retention

**Work:**
1. `user_fills`: keep only the last 1 000 fills per user in memory. Older fills are in the database.
2. `sessions`: add TTL-based expiry. Sessions inactive for 24 hours are evicted from memory. Database is the source of truth for active session validation.
3. `order_history`: keep only open orders in memory. Closed orders are in the database.
4. Add a background task that runs every hour and logs current memory usage per collection.

**Effort:** M · **Owner:** BE

---

### P2-4 — WebSocket market data: fan-out decoupling

**Risk:** P2 — the current fan-out is O(events × connections). At 10K connected clients each subscribed to BTC/USDT trades, a single fill generates 10K task wakeups. This saturates the Tokio scheduler.

**Files:**
- `crates/api-gateway/src/ws.rs` — subscription and fan-out logic

**Work:**
1. Replace per-connection task wakeups with a broadcast channel per topic (e.g., `tokio::sync::broadcast`). Each subscriber reads from the channel; the sender writes once.
2. Slow consumers that lag the broadcast channel by more than 1 000 messages are disconnected with `4002 Consumer Too Slow`.
3. Add `ws_broadcast_lag_p99` metric per topic.

**Effort:** L · **Owner:** BE

---

### P2-5 — JWT: asymmetric signing (RS256) and revocation list

**Risk:** P2 — HS256 with a symmetric secret means any process that can verify tokens can also forge them. A secret leak (log, core dump, env var exposure) means every existing token must be considered forged. RS256 separates signing from verification.

**Files:**
- `crates/api-gateway/src/auth.rs` — JWT signing and verification
- `crates/api-gateway/src/security.rs` — token issuance

**Work:**
1. Replace HS256 with RS256. Generate a 2048-bit RSA key pair on startup (or load from a secrets manager). The private key signs; the public key verifies.
2. Add a JWT revocation list in Redis (or a database table) keyed by `jti`. On logout or account freeze, add the `jti` to the list with TTL equal to the remaining token lifetime.
3. Verification middleware checks the revocation list on every request (use a short in-memory cache with 30-second TTL to avoid DB round-trips on every request).

**Effort:** L · **Owner:** BE

---

### P2-6 — Periodic KYC re-verification and PEP re-screening

**Risk:** P2 — KYC is a snapshot in time. A user who passes KYC today may become a PEP, appear on a sanctions list, or change their risk profile tomorrow. FATF requires ongoing monitoring proportional to risk level.

**Files:**
- `crates/compliance/src/lib.rs` — KYC status management
- `crates/compliance/src/sanctions.rs` — screening integration
- `crates/api-gateway/src/tasks.rs` — background tasks

**Work:**
1. Add `kyc_next_review_at: DateTime` to the user record. Set to 12 months for standard users, 6 months for high-risk, 3 months for PEPs.
2. Add a daily background task that finds users where `kyc_next_review_at < now`. Re-screen against the current sanctions list and PEP database. If a hit is found, freeze the account and file a SAR.
3. Notify the user via email that their KYC requires renewal (without revealing why if a PEP/sanctions hit).

**Effort:** M · **Owner:** BE + CO

---

### P2-7 — X-Forwarded-For trusted proxy validation

**Risk:** P2 — the admin middleware reads `X-Forwarded-For` to get the client IP for rate limiting and audit logging. Without trusted proxy validation, any client can spoof this header and bypass IP-based controls.

**Files:**
- `crates/api-gateway/src/auth.rs` — IP extraction logic

**Work:**
1. Add `TRUSTED_PROXIES` environment variable (CIDR list). Only accept `X-Forwarded-For` from requests arriving from a trusted proxy IP.
2. If the request does not arrive via a trusted proxy, use `peer_addr` directly.
3. Add to `.env.example` with documentation.

**Effort:** S · **Owner:** BE + DO

---

### P2-8 — Appropriateness assessment before leveraged trading (MiCA Art. 71(2))

**Risk:** P2 — MiCA Art. 71(2) requires CASPs to assess whether leveraged products are appropriate for a retail client based on their knowledge and experience. Allowing any user to trade perpetual futures without this assessment is a direct regulatory violation.

**Files:**
- `crates/compliance/src/lib.rs` — appropriateness assessment (currently exists as a struct, not enforced)
- `crates/order-gateway/src/validator.rs` — pre-trade checks

**Work:**
1. Add `appropriateness_assessed_at: Option<DateTime>` and `appropriateness_result: Option<Appropriate|NotAppropriate|NotCompleted>` to the user record.
2. In `order-gateway` validator: reject leveraged orders for users with `appropriateness_result = None` or `NotCompleted`.
3. If `NotAppropriate`, allow the user to proceed with an explicit additional acknowledgement (the "explicit risk override" flow required by MiCA).
4. Web client: add a multi-step appropriateness questionnaire in the onboarding flow and as a gate before the first leveraged order.

**Effort:** L · **Owner:** BE + CO + FE

---

### P2-9 — `/metrics` endpoint access control

**Risk:** P2 — Prometheus metrics at `/metrics` are publicly accessible. Metrics expose internal topology: number of users, order rates, position sizes, engine thread counts. This is sensitive operational intelligence.

**Files:**
- `crates/api-gateway/src/server.rs` — metrics route registration

**Work:**
1. Move `/metrics` to the admin router (already auth-gated) or add a dedicated `METRICS_TOKEN` bearer auth check.
2. Alternatively, bind the metrics server on a separate port (`METRICS_PORT`, default 9090) that is not exposed in the public Docker port mapping.

**Effort:** S · **Owner:** DO + BE

---

### P2-10 — Internal wallet transfer TOCTOU fix

**Risk:** P2 — the transfer path reads both balances, checks that source has sufficient funds, then debits and credits. Between the check and the debit, a concurrent transfer can drain the source account, violating the `available >= 0` invariant.

**Files:**
- `crates/account/src/transfers.rs` — transfer execution

**Work:**
1. The check and debit must be atomic. Hold the source account shard lock across both the check and the debit. Then acquire the destination shard lock for the credit (using the deterministic lock order from P2-1 to avoid deadlock).
2. Add a test with concurrent transfers from the same account that would overdraft if the race were present.

**Effort:** M · **Owner:** BE

---

### P2-11 — `std::env::var` hot path removal

**Risk:** P2 — `std::env::var` acquires a global lock on the process environment. Called on every order, at high throughput this becomes measurable contention. Reading environment variables belongs in startup initialization, not in the hot path.

**Files:**
- `crates/api-gateway/src/exchange.rs` — any `std::env::var` calls in order processing
- `crates/order-gateway/src/validator.rs` — same

**Work:**
1. Move all `std::env::var` reads to the `Exchange::new()` or `Exchange::with_journal()` initialization. Store results as plain fields on `Exchange` or as constants.
2. Search all crates for `std::env::var` calls outside of `main()` or `new()` functions and move them.

**Effort:** S · **Owner:** BE

---

### P2-12 — Travel Rule: replace stub transmission with real implementation

**Risk:** P2 — the Travel Rule transmission is a stub that logs and returns `Ok`. For transfers above the FATF threshold ($1 000 or local equivalent), the originator VASP must actually transmit the required data to the beneficiary VASP. A stub is non-compliant.

**Files:**
- `crates/compliance/src/lib.rs` — Travel Rule transmission

**Work:**
1. Integrate with a Travel Rule protocol provider (Notabene, Sygna, or OpenVASP).
2. For transfers above threshold: call the provider API to initiate the IVMS101 data exchange before releasing the withdrawal.
3. Add retry logic with exponential backoff. If the Travel Rule exchange cannot complete within 4 hours, hold the withdrawal for manual review.

**Effort:** XL · **Owner:** BE + CO

---

## Phase 3 — Enterprise (target: 16 weeks after Phase 2)

Full multi-node deployment, institutional compliance, and SLA-grade operations.

### P3-1 — Service decomposition: Matching, Account, MarketData, Settlement, Risk

**Context:** The current monolith has a hard throughput ceiling because all state lives in one process. The architect's migration path is: Group Commit WAL → Account sharding → Market data fan-out → Service split.

Phases 0–2 deliver the first three steps. Phase 3 completes the split.

**Target architecture:**

| Service | Responsibility | Scaling axis |
|---------|---------------|--------------|
| `matching-service` | Order book, matching, per-symbol threads | Horizontal by symbol |
| `account-service` | Balances, locks, settlements | Horizontal by user shard |
| `market-data-service` | Klines, tickers, WS fan-out | Horizontal, stateless |
| `risk-service` | Margin, liquidation, ADL | Vertical (needs full position view) |
| `settlement-service` | Funding, insurance, daily settlement | Single-leader |
| `api-gateway` | HTTP/WS ingress, auth, routing | Horizontal, stateless |

**Files:** Entire codebase restructure. New `services/` top-level directory.

**Effort:** XL · **Owner:** BE

---

### P3-2 — Complete FIX gateway TLS

**Context:** The TLS check exists in `fix-gateway/src/tcp_server.rs` but TLS is not actually applied to the connection. Institutional clients connecting over the internet have their FIX traffic in plaintext, including authentication credentials.

**Files:**
- `crates/fix-gateway/src/tcp_server.rs` — TLS wrapping
- `crates/fix-gateway/Cargo.toml` — add `rustls` or `native-tls`

**Work:**
1. Load a TLS certificate and key from `FIX_TLS_CERT_PATH` / `FIX_TLS_KEY_PATH`.
2. Wrap accepted TCP streams with `tokio-rustls::TlsAcceptor`.
3. Add startup check: if `FIX_PORT` is set and TLS paths are not, refuse to start.

**Effort:** M · **Owner:** BE

---

### P3-3 — Runbook accuracy: add missing admin endpoints

**Context:** The runbook references `POST /admin/engine/restart` which does not exist. During an incident, operators following the runbook will fail, wasting critical minutes.

**Files:**
- `docs/runbooks/` — all runbook files
- `crates/api-gateway/src/rest_v2.rs` — admin endpoint registration

**Work:**
1. Audit every runbook command against the actual route table. Flag any endpoint that does not exist.
2. For each missing endpoint: either implement it or remove the reference and document the correct procedure.
3. Add a CI test that curls the admin endpoint list and diffs against the runbook's endpoint references.

**Effort:** M · **Owner:** DO + BE

---

### P3-4 — Regulatory reporter: fix zero-producing report generator

**Context:** The regulatory reporter produces zeros for all fields. A regulator receiving a zero report will interpret it as either a system failure or an attempt to conceal activity. Either outcome triggers regulatory action.

**Files:**
- `crates/compliance/src/lib.rs` — regulatory report generation
- `crates/db/` — transaction and position query repositories

**Work:**
1. Implement each report section by querying the appropriate database tables for the reporting period.
2. Add a reconciliation step: total reported trading volume must equal sum of all fills in the period. Fail the report generation with an error if they diverge.
3. Add a test with known data: populate trades, generate the report, assert the values match.

**Effort:** L · **Owner:** BE + CO

---

### P3-5 — Secrets management: move from environment variables to a secrets manager

**Context:** JWT secrets, database credentials, API signing keys, and SMTP passwords are all in environment variables. Environment variables are visible in process listings, Docker inspect output, and Kubernetes pod specs. At enterprise scale, a secrets manager (HashiCorp Vault, AWS Secrets Manager, or GCP Secret Manager) is the standard.

**Files:**
- `crates/server/src/main.rs` — secret loading at startup
- `deploy/deploy.sh` — secret provisioning

**Work:**
1. Abstract secret loading behind a `SecretProvider` trait with `EnvSecretProvider` (current) and `VaultSecretProvider` implementations.
2. At startup: if `VAULT_ADDR` is set, use Vault. Otherwise, fall back to environment variables with a `WARN` log.
3. Rotate JWT signing key on a configurable schedule (default: 30 days). Old tokens issued before rotation remain valid until expiry; new tokens use the new key.

**Effort:** L · **Owner:** DO + BE

---

### P3-6 — Proof of Reserves: on-chain verification

**Context:** The current Proof of Reserves implementation proves internal ledger consistency (Merkle tree of user balances) but does not prove that on-chain holdings equal the liabilities. A user cannot independently verify that the exchange holds the funds it claims.

**Files:**
- `crates/proof-of-reserves/src/report.rs`

**Work:**
1. Add on-chain balance queries for each supported asset (BTC via Bitcoin node RPC, ETH/ERC20 via Ethereum node).
2. The proof report must include: total user liabilities (from Merkle tree), total on-chain holdings (from node queries), and the difference (should be zero or positive — exchange reserves minus user liabilities).
3. Publish the report on a public endpoint: `GET /proof-of-reserves/latest` with a signature from the exchange's attestation key.

**Effort:** XL · **Owner:** BE + DO

---

## Summary Table

| ID | Item | Phase | Effort | Owner | Risk Category |
|----|------|-------|--------|-------|---------------|
| P0-1 | Remove hard-coded auth bypasses | 0 | M | BE | Security |
| P0-2 | Enforce withdrawal address whitelist | 0 | M | BE | Financial |
| P0-3 | Fix deploy.sh .env overwrite | 0 | S | DO | Operational |
| P0-4 | SAR tipping-off prevention | 0 | M | BE+CO | Regulatory/Criminal |
| P0-5 | Wire real CustodyProvider (gate mock) | 0 | S+XL | BE+DO | Financial |
| P0-6 | Background task supervision | 0 | M | BE+DO | Operational/Financial |
| P0-7 | Minimum viable alerting rules | 0 | M | DO | Operational |
| P0-8 | Fix stale snapshot in liquidation monitor | 0 | M | BE | Financial |
| P1-1 | Structured JSON logging | 1 | M | DO+BE | Operational |
| P1-2 | Risk warnings gate futures entry (MiCA Art. 71) | 1 | M | BE+CO+FE | Regulatory |
| P1-3 | OFAC sanctions auto-refresh | 1 | M | BE+DO | Regulatory |
| P1-4 | Travel Rule: originator legal name | 1 | S | BE+CO | Regulatory |
| P1-5 | Fix WAL write failure silent ignore | 1 | M | BE | Financial |
| P1-6 | API key secrets: HMAC-SHA256 hashed storage | 1 | M | BE | Security |
| P1-7 | WebSocket auth rate limit + session timeout | 1 | M | BE | Security |
| P1-8 | Security event log persistence | 1 | M | BE | Regulatory |
| P1-9 | DB connection pool sizing | 1 | S | DO+BE | Operational |
| P1-10 | Unhosted wallet ownership verification (MiCA Art. 68(5)) | 1 | L | BE+CO | Regulatory |
| P1-11 | FIX gateway: restrict to internal network | 1 | S | DO | Security |
| P1-12 | Grafana: remove hardcoded password | 1 | S | DO | Security |
| P1-13 | SAR 30-day deadline tracking | 1 | S | BE+CO | Regulatory |
| P2-1 | Account sharding (break lock chain) | 2 | L | BE | Performance |
| P2-2 | WAL group commit + async fsync | 2 | L | BE | Performance |
| P2-3 | Bound in-memory state (OOM prevention) | 2 | M | BE | Operational |
| P2-4 | WebSocket fan-out: broadcast channel | 2 | L | BE | Performance |
| P2-5 | JWT: RS256 + revocation list | 2 | L | BE | Security |
| P2-6 | Periodic KYC re-verification + PEP re-screening | 2 | M | BE+CO | Regulatory |
| P2-7 | X-Forwarded-For trusted proxy validation | 2 | S | BE+DO | Security |
| P2-8 | Appropriateness assessment (MiCA Art. 71(2)) | 2 | L | BE+CO+FE | Regulatory |
| P2-9 | `/metrics` endpoint access control | 2 | S | DO+BE | Security |
| P2-10 | Internal transfer TOCTOU fix | 2 | M | BE | Financial |
| P2-11 | Remove `std::env::var` from hot path | 2 | S | BE | Performance |
| P2-12 | Travel Rule: real IVMS101 transmission | 2 | XL | BE+CO | Regulatory |
| P3-1 | Service decomposition (5 services) | 3 | XL | BE | Performance |
| P3-2 | FIX gateway TLS | 3 | M | BE | Security |
| P3-3 | Runbook accuracy + missing admin endpoints | 3 | M | DO+BE | Operational |
| P3-4 | Regulatory reporter: fix zero output | 3 | L | BE+CO | Regulatory |
| P3-5 | Secrets management (Vault integration) | 3 | L | DO+BE | Security |
| P3-6 | Proof of Reserves: on-chain verification | 3 | XL | BE+DO | Financial |

---

## Definition of Done per Phase

### Phase 0 complete when:
- All eight P0 items have passing tests demonstrating the fix
- `cargo test` passes with 0 failures
- `deploy.sh` runs on a clean machine without overwriting a pre-existing `.env`
- A mock incident drill confirms that the background task alert fires within 60 seconds of a task death

### Phase 1 complete when:
- `cargo test` passes with 0 failures
- A MiCA-compliant test user cannot place a futures order without a risk acknowledgement
- The OFAC list refreshes automatically every 6 hours (verified by log output)
- Security events survive a process restart (verified by querying the database)
- A withdrawal to a non-whitelisted address returns 403 (verified by integration test)

### Phase 2 complete when:
- `cargo bench` shows > 5 000 place_order/sec on a 4-core machine
- Memory usage is stable over a 24-hour soak test with 10K simulated users
- WebSocket fan-out benchmark: 10K subscribers, single topic, < 10ms p99 delivery latency
- JWT RS256 is verified: a token signed with an old secret fails verification
- MiCA appropriateness assessment blocks a new user from placing their first leveraged order

### Phase 3 complete when:
- Service decomposition allows independent scaling of matching and account services
- Proof of Reserves report is publicly accessible and matches on-chain balances
- Regulatory report for a test period contains correct non-zero values
- FIX TLS: institutional client connection uses TLS 1.3 (verified by Wireshark or `openssl s_client`)

---

## Open Questions

The following questions must be answered before Phase 1 work begins. They affect scope and sequencing.

1. **Custody provider selection:** Which CustodyProvider implementation will be used in production — `mg-custody` service, Fireblocks, or direct node integration? This determines the scope of P0-5.

2. **MiCA jurisdiction applicability:** Is the exchange seeking EU MiCA authorization, or operating under a different jurisdiction's framework? This determines the exact disclosure text and gating logic for P1-2 and P2-8.

3. **Travel Rule protocol:** Which Travel Rule protocol is preferred — OpenVASP (open source), Notabene (SaaS), or Sygna? This determines the integration approach for P2-12.

4. **Secrets manager:** Is HashiCorp Vault already in the infrastructure, or is a cloud-native option (AWS Secrets Manager, GCP Secret Manager) preferred? This determines the implementation of P3-5.

5. **Alerting destination:** Where should P0-7 alert rules send notifications — PagerDuty, OpsGenie, Slack, or email? This determines the AlertManager configuration.
