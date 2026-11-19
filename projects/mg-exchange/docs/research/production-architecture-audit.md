# Production Architecture Audit

**Date:** 2026-04-16  
**Method:** Deep code audit (Explore agent scanning all 24 crates) + external production-readiness research  
**Verdict:** NOT production-ready for real money. 6 CRITICAL issues, 8 HIGH issues, fixable in ~6-8 weeks.

---

## CRITICAL Issues (must fix before any real money)

### C1. In-Memory State Loss on Crash

**The exchange loses ~30 data structures on process restart.** Despite WAL replay recovering the matching engine order books and engine snapshots recovering book state, the following are in-memory only with no persistence:

| Field | Impact on restart |
|-------|-------------------|
| `pending_orders` | All in-flight order metadata lost — users can't cancel resting orders |
| `order_locks` / `futures_margin_locks` | Locked margin released — users can double-spend |
| `order_history` (100K cap) | User's closed-order history empty |
| `user_fills` (500/user cap) | Fill notifications lost |
| `trade_history` (10K/symbol) | Public trade tape empty |
| `closed_positions` | PnL history gone |
| `sessions`, `refresh_tokens` | All users logged out |
| `user_leverage` preferences | Reset to default |
| `grid_bots`, `dca_bots`, `copy_trading` state | All strategies paused |
| `surveillance` alerts | Fresh slate — same manipulator not flagged |

**Why it's critical:** The margin-lock loss (C1) means a user can withdraw funds that should be locked for an open order. On restart, `available = total` (all margin unlocked), the user withdraws, then the resting order fills → exchange owes more than it holds.

**Fix:** Persist `pending_orders`, `order_locks`, `futures_margin_locks`, and `positions` to Postgres. On startup, load from DB and reconcile against WAL replay. Estimated effort: L (2 weeks).

---

### C2. Balance Updates Before Ledger Confirmation

`handle_fill_sync` updates in-memory balances (`accounts.settle_fill`) **before** the async DB ledger write completes. If the ledger rejects the entry (imbalanced, constraint violation), the in-memory state is already mutated.

**Location:** `crates/api-gateway/src/exchange.rs` ~line 7285  
**Impact:** In-memory and DB balances diverge silently.  
**Fix:** Either make the ledger write synchronous (latency cost) or implement a compensation/rollback if the async write fails.

---

### C3. Reconciliation Uses Mock Reserves

`admin_reconciliation` computes wallet reserves by **deriving them from user balances** (hot=10%, warm=20%, cold=70% of total liability). It never queries the actual Fireblocks/custody wallet balances.

**Impact:** Reconciliation always passes. If the hot wallet is drained by a hack, the operator sees "all clear."  
**Fix:** Wire `ChainBalanceProvider` (BTC/ETH, already implemented) into the reconciliation handler. Replace mock reserves with real on-chain queries.

---

### C4. No JWT Token Revocation

JWT access tokens cannot be revoked after issuance. If a user reports a leaked token, there is no mechanism to blacklist it before expiry.

**Impact:** Attacker trades with user's account for the full token TTL (potentially 1 hour).  
**Fix:** Add `token_blacklist` table in Postgres. On logout or admin freeze, insert `(token_hash, expires_at)`. Check blacklist on every auth (fast indexed lookup + TTL cleanup).

---

### C5. WebSocket Connection Limit Not Enforced

`ws_ip_connections: Mutex<HashMap<String, u32>>` is declared but **never incremented/decremented** in the WebSocket handler. An attacker can open unlimited connections from one IP until file descriptors are exhausted.

**Impact:** 100K connections → OOM or fd exhaustion → full exchange crash.  
**Fix:** Increment on upgrade, decrement on close, reject if count > 10 per IP.

---

### C6. No Orderbook Depth Limit

`OrderBook.bids/asks` are unbounded `BTreeMap`. An attacker placing 1M orders at distinct prices creates ~500MB of orderbook state per symbol. 100 symbols = 50GB.

**Impact:** OOM → crash.  
**Fix:** Add `MAX_PRICE_LEVELS = 10,000` per side. Reject orders that would create a new price level beyond the limit.

---

## HIGH Issues

### H1. God Object: Exchange Struct (~100 Mutex Fields)

The `Exchange` struct is a god object with ~100 lock-protected fields spanning all domains (matching, accounts, compliance, custody, surveillance, market data). Any lock contention on a single field affects all threads.

`pending_orders` and `order_locks` are global (not sharded) — they become bottlenecks under high concurrency. `stp_guard` is also global.

**Fix (incremental):** Shard `pending_orders` and `order_locks` by user_id (same 64-shard pattern as AccountManager). Move STP check into the matching engine thread (eliminates the lock entirely from the hot path).

### H2. 15+ Lock Acquisitions per Order

`place_order` acquires locks on: `operating_mode` (atomic), `frozen_accounts` (DashSet), `kyc_approved` (DashSet), `perp_configs` (ArcSwap), accounts shard (×3 for validation, margin, settle), `stp_guard`, `order_locks`, `pending_orders`, `journal` (shared Mutex for group commit), `engine_pool` (RwLock).

Many of these are lock-free (good) but `stp_guard`, `order_locks`, `pending_orders` are contention points.

### H3. Admin Endpoints on Same Port (Bypassable in Dev Mode)

All admin endpoints run on port 8080 alongside public API. In dev mode (no `key_store`), admin routes are unprotected. If a deployment accidentally runs with `PRODUCTION != "true"`, admin endpoints are open.

**Fix:** Panic if `is_production && key_store.is_none()`. Consider moving admin to a separate port.

### H4. No External Service Circuit Breakers

Fireblocks, Sumsub, Chainalysis, Notabene HTTP calls have no circuit breaker or timeout. If an external service is down, the calling thread blocks indefinitely.

**Fix:** Add `reqwest::Client` with `timeout(Duration::from_secs(10))`. Implement circuit breaker: after 5 consecutive failures, stop calling for 60 seconds.

### H5. No Database Connectivity Timeout at Startup

If `DATABASE_URL` is set but Postgres is unreachable, `Database::connect_with()` blocks indefinitely. The server never starts, never logs an error.

**Fix:** Set `acquire_timeout` on the pool (already in `PoolConfig::recommended` = 3s). But the initial `.connect()` call may not respect it — add a `tokio::time::timeout(10s, connect)` wrapper.

### H6. `convert_history`, `dust_conversions` — Unbounded Vecs

Several in-memory Vecs grow without bound. Under sustained operation, these leak memory.

**Fix:** Cap all history Vecs with `VecDeque` + `MAX_CAPACITY`. Run a background cleanup task.

### H7. Post-Fill Event Channel Can Drop Events

`post_fill_event_tx.try_send()` drops events when the channel (8192 capacity) is full. Dropped events = missed surveillance checks, missed WS notifications, missed FIX execution reports.

**Fix:** Log + alert on drops (already logged). Add backpressure: if drops > 100/sec, temporarily slow order acceptance.

### H8. No Config Validation Struct

Environment variables are parsed ad-hoc with `unwrap_or`, `parse().ok()`, no central validation. Negative fee rates, zero tick sizes, invalid URLs are not caught at startup.

**Fix:** Create a `Config` struct with `from_env() -> Result<Config, ConfigError>` that validates all values upfront. Print validated config on startup (secrets redacted).

---

## Production Infrastructure Gaps

### I1. No Redundancy / No Failover

Single binary, single server. Binary crash = full exchange down. No load balancer, no health-check-based restart, no read replicas.

**Minimum for launch:** Managed Postgres HA (RDS Multi-AZ or CloudSQL HA) + systemd auto-restart + health endpoint monitoring (already have `/health`).

### I2. No Redis/Cache Layer

Rate limit counters, sessions, and market data snapshots are all in-process memory. This means:
- Rate limits don't survive restart
- No shared state across multiple instances (can't scale horizontally)

**Minimum for launch:** Redis for rate-limit counters and sessions. In-memory cache is fine for single-instance, but Redis enables future horizontal scaling.

### I3. No CDN / Edge Protection

Static web client served from origin. No Cloudflare, no DDoS mitigation at the edge.

**Minimum for launch:** Cloudflare in front of everything. Free tier covers most DDoS. $20/mo Pro tier adds WAF rules.

### I4. No Deployment Pipeline

No blue-green deploy, no canary, no automated rollback. Matching engine deploy requires WAL replay which is a brief trading halt.

**Minimum for launch:** systemd service with `Restart=on-failure` + WAL replay on startup. Maintenance window for deploys is acceptable for Phase 1.

---

## DDoS Resilience Assessment

| Attack Vector | Current Defense | Gap |
|---------------|-----------------|-----|
| L3/L4 volumetric flood | None | Need Cloudflare/AWS Shield |
| L7 HTTP flood | Token-bucket rate limiter (30-300 rps) | Bypassable via IP spoofing, no WAF |
| WebSocket flood | Connection limit declared, NOT enforced | C5 above |
| Order flooding | Per-user rate limit on orders | No per-symbol limit; no cancel-storm detection |
| Orderbook depth abuse | None | C6 above |
| Slowloris | Axum default connection timeout | Should add explicit 5s upgrade timeout |
| IP spoofing (X-Forwarded-For) | Trusted proxy check exists | Ensure only one proxy hop is trusted |

---

## Multi-Region Readiness

| Component | Can Distribute? | Notes |
|-----------|----------------|-------|
| Matching engine | NO — must be single-writer | Determinism requires total event ordering |
| Market data fanout | YES | WebSocket servers can subscribe to central NATS/Kafka |
| API gateway (reads) | YES | Account info, history from Postgres read replicas |
| API gateway (writes) | NO — must route to primary | Orders, withdrawals |
| Database | Async replicas in other regions | Sync replica in primary region for trade data |
| Web client | YES — CDN | Static assets |

**Verdict:** Single region for launch. Multi-region market data fanout in Phase 2.

---

## Scalability Limits (Current Architecture)

| Metric | Current Limit | Bottleneck |
|--------|--------------|------------|
| Orders/sec throughput | ~42K ops/sec (bench) | Global `pending_orders` Mutex |
| Concurrent symbols | ~100 before thread overhead matters | 1 OS thread per symbol × 2MB stack |
| WebSocket connections | ~10K (fd limit) | No connection management |
| Order book depth | Unbounded (crash at ~50GB) | No depth limit |
| Memory growth | Unbounded (multiple Vec/HashMap) | No eviction on many fields |
| External service calls | Default reqwest pool (32 connections) | No explicit pooling/circuit breaker |

---

## Fix Priority (recommended order)

### Phase A: Must-fix before real money (2-3 weeks)

| # | Issue | Effort |
|---|-------|--------|
| C4 | JWT token revocation | M |
| C5 | WebSocket connection limit enforcement | S |
| C6 | Orderbook depth limit | S |
| C3 | Wire real reserves into reconciliation | M |
| H3 | Panic if admin unprotected in production | S |
| H4 | External service timeouts + circuit breaker | M |
| H5 | DB startup connectivity timeout | S |

### Phase B: Critical data integrity (2-3 weeks)

| # | Issue | Effort |
|---|-------|--------|
| C1 | Persist pending_orders + margin locks to DB | L |
| C2 | Ledger-before-balance or compensating rollback | M |
| H1 | Shard pending_orders + order_locks | M |
| H6 | Cap unbounded Vecs | S |
| H8 | Config validation struct | M |

### Phase C: Infrastructure (2 weeks)

| # | Issue | Effort |
|---|-------|--------|
| I1 | Managed Postgres HA + systemd auto-restart | M |
| I3 | Cloudflare in front of everything | S |
| I2 | Redis for rate limits + sessions (optional for single-instance) | M |

---

## What's Actually Good

Despite the critical issues, the system has strong foundations:

- **Matching engine architecture** is solid (per-symbol threads, lock-free BBO, SmallVec events, CPU pinning)
- **WAL + SegmentedWal + group commit** is correctly implemented (shared WAL, no split-brain)
- **64-shard AccountManager** with deadlock-safe lock ordering
- **Property-based tests** covering balance conservation and WAL replay determinism
- **Audit log with SHA-256 hash chain** persisted to Postgres
- **MiCA compliance scaffolding** (risk warnings, appropriateness assessment, Travel Rule, SAR)
- **Operational tooling** (FMEA, SLOs, game day playbook, Grafana dashboards, Alertmanager rules, k6 load tests)
- **5-phase graceful shutdown** with operating mode transitions
- **2189 tests** across the full workspace with 0 failures

The engineering quality is high — the gaps are mostly in the "production operations" and "state persistence" layers, not in the core trading logic.

---

## Sources

- Internal code audit: 24 crates, ~200K lines Rust
- Google SRE Book: SLOs, incident response, deployment
- Coinbase/Kraken/Binance public postmortems and engineering blogs
- MiCA Regulation (EU) 2023/1114
- FATF Guidance on Virtual Assets (2021)
- FTX Chapter 11 (what went wrong: no segregation, no reconciliation, no reserves)
