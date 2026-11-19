# Failure Modes and Effects Analysis (FMEA)

**Date:** 2026-04-16  
**Review cadence:** quarterly or after any P0 incident

This document catalogs the top 20 failure modes for the exchange, ordered by severity. Each entry: what can fail, how we detect it, what the blast radius is, how to mitigate, and how to recover.

---

## Severity Scale

| Level | Meaning | Response |
|-------|---------|----------|
| **S1** | Customer funds at risk | Page CEO + security lead immediately |
| **S2** | Trading halted or degraded | Page on-call, CEO if >30 min |
| **S3** | Partial degradation, no fund risk | On-call ticket, resolve within 4h |
| **S4** | Cosmetic / analytics | Next business day |

---

## Failure Catalog

### F01 — Matching engine thread panic

| Attribute | Value |
|-----------|-------|
| **Severity** | S2 |
| **Detection** | `engine_pool.process()` returns `EngineError::ThreadPanicked`; metrics show zero throughput for affected symbol |
| **Blast radius** | Single symbol halted. Other symbols unaffected (per-symbol threads are bulkheads). |
| **Mitigation** | `EnginePool::restart_symbol()` spawns a fresh thread. Orders for this symbol queue until restart completes. |
| **Recovery** | WAL replay reconstructs the book for the restarted symbol. All in-flight orders on the old thread are lost. |
| **Runbook** | `docs/runbooks/matching-engine-halt.md` |
| **Code** | `crates/matching-engine/src/engine_pool.rs:155` (`restart_symbol`) |

### F02 — WAL fsync failure (EIO / disk full)

| Attribute | Value |
|-----------|-------|
| **Severity** | S1 |
| **Detection** | `group_commit.rs` calls `std::process::abort()` on fatal I/O. Process exits immediately. |
| **Blast radius** | Full exchange down. All in-memory state lost (but WAL entries up to the last successful fsync are durable). |
| **Mitigation** | `panic=abort` + P0-2 fsync abort ensures we never silently lose writes. |
| **Recovery** | Fix the disk (clear space, replace hardware). Restart binary — WAL replay recovers to last fsync point. |
| **Runbook** | `docs/runbooks/wal-corruption.md` |
| **Code** | `crates/journal/src/group_commit.rs` (`is_fatal_io`) |

### F03 — PostgreSQL primary down

| Attribute | Value |
|-----------|-------|
| **Severity** | S3 (trading continues; persistence degrades) |
| **Detection** | Background tasks log `sqlx::Error`. Audit log flusher, reconciliation, KYC write-through all fail. |
| **Blast radius** | In-memory state remains authoritative. Orders, fills, positions continue in memory. DB writes accumulate and retry on reconnect. KYC and audit entries queue in memory. |
| **Mitigation** | WAL is the source of truth, not the DB. DB is write-behind. |
| **Recovery** | Restart Postgres. Background flushers auto-retry. If Postgres was down >5 min, run `admin/reconciliation` to verify consistency. |
| **Runbook** | `docs/runbooks/db-down.md` |

### F04 — PostgreSQL replication lag (>30s)

| Attribute | Value |
|-----------|-------|
| **Severity** | S4 |
| **Detection** | `pg_stat_replication.replay_lag` metric (if replicas configured). |
| **Blast radius** | Read replicas serve stale data. No impact on primary or trading. |
| **Mitigation** | Ensure all reads critical to trading go to primary. |
| **Recovery** | Lag resolves when the replica catches up. If persistent, check I/O on replica host. |

### F05 — NTP drift / clock skew (>1s)

| Attribute | Value |
|-----------|-------|
| **Severity** | S3 |
| **Detection** | `chrono::Utc::now()` values diverge from external NTP reference. Funding rate timestamps may be off. HMAC recv_window checks reject legitimate clients. |
| **Blast radius** | API auth failures for clients with correct clocks. Funding payments applied at wrong time. |
| **Mitigation** | Use `chrono` consistently (no `SystemTime::now()`). `recv_window` default is 5000ms — tolerates small skew. |
| **Recovery** | Sync NTP: `ntpdate` or `chronomagic`. Restart is NOT required — clock correction takes effect immediately for new operations. |

### F06 — OS OOM kill

| Attribute | Value |
|-----------|-------|
| **Severity** | S2 |
| **Detection** | Process disappears. `dmesg` shows OOM killer. |
| **Blast radius** | Full exchange down. All in-memory state lost. |
| **Mitigation** | Set `vm.overcommit_memory=2` and memory limits. Monitor RSS via Grafana. Alert at 80% RAM. |
| **Recovery** | Restart binary. WAL replay reconstructs matching state. Investigate memory growth — likely orderbook depth explosion or leaked session state. |

### F07 — Disk full

| Attribute | Value |
|-----------|-------|
| **Severity** | S2 |
| **Detection** | WAL append fails with ENOSPC. `group_commit.rs` aborts. |
| **Blast radius** | Full exchange down (same as F02). |
| **Mitigation** | WAL segment rotation + old segment cleanup (SegmentedWal exists but not yet wired). Monitor disk usage, alert at 80%. |
| **Recovery** | Free space (delete old WAL segments, clear temp files). Restart. |

### F08 — DNS failure

| Attribute | Value |
|-----------|-------|
| **Severity** | S3 |
| **Detection** | Custody provider, Sumsub, Chainalysis API calls fail with "DNS resolution failed". |
| **Blast radius** | Withdrawals, KYC verification, AML screening stop. Trading unaffected. |
| **Mitigation** | Cache DNS locally (`systemd-resolved` or dnsmasq). Use IP addresses for critical internal services. |
| **Recovery** | Fix DNS upstream. Withdrawal poller retries automatically. |

### F09 — TLS certificate expiry

| Attribute | Value |
|-----------|-------|
| **Severity** | S2 |
| **Detection** | Clients get TLS errors. WebSocket disconnects. Monitoring external probe (Pingdom/UptimeRobot) alerts. |
| **Blast radius** | All HTTPS/WSS clients unable to connect. Trading continues internally but no new orders arrive. |
| **Mitigation** | Auto-renewal via Let's Encrypt / Certbot / ACME. Alert 30 days before expiry. |
| **Recovery** | Renew cert, reload reverse proxy. No exchange restart needed. |

### F10 — Hung background task (no heartbeat for >60s)

| Attribute | Value |
|-----------|-------|
| **Severity** | S3 |
| **Detection** | `metrics.touch_task_heartbeat("task_name")` stops updating. Grafana alert on stale heartbeat. |
| **Blast radius** | Depends on task. Liquidation monitor hung = S2 (positions not liquidated). Audit flusher hung = S4 (entries queue in memory). |
| **Mitigation** | `supervised_spawn` restarts panicked tasks. A deadlocked task doesn't panic — it just hangs. No watchdog yet (tracked as P2). |
| **Recovery** | Restart the binary if the hung task is critical. Investigate deadlock — check lock ordering in `exchange.rs`. |

### F11 — Fireblocks API outage

| Attribute | Value |
|-----------|-------|
| **Severity** | S3 |
| **Detection** | Withdrawal poller logs errors. Deposit webhook returns 502/timeout. |
| **Blast radius** | Withdrawals and deposits stop processing. Trading continues. |
| **Mitigation** | `ENABLE_WITHDRAWALS` feature flag can gate new withdrawal requests so the queue doesn't grow unbounded. |
| **Recovery** | Fireblocks recovers. Poller retries pending withdrawals automatically. |

### F12 — Sumsub webhook backlog

| Attribute | Value |
|-----------|-------|
| **Severity** | S4 |
| **Detection** | KYC approval queue grows. Users report "stuck in pending". |
| **Blast radius** | New user onboarding delays. No trading impact for existing users. |
| **Mitigation** | Manual KYC approval via `POST /admin/kyc/{user_id}/approve`. |
| **Recovery** | Sumsub clears backlog. Webhooks arrive and auto-approve. |

### F13 — Chainalysis timeout / rate limit

| Attribute | Value |
|-----------|-------|
| **Severity** | S3 |
| **Detection** | AML screening task logs timeouts. Deposit credits delayed. |
| **Blast radius** | Deposits held in screening state. Withdrawals may be delayed if address screening fails. |
| **Mitigation** | Retry with exponential backoff. Cache recent screening results. |
| **Recovery** | Chainalysis service recovers. Pending screenings process on next poller tick. |

### F14 — WebSocket fanout backlog

| Attribute | Value |
|-----------|-------|
| **Severity** | S3 |
| **Detection** | `tokio::sync::broadcast` channel lag. Clients report stale data. |
| **Blast radius** | Market data delayed for subscribers. No trading impact (order entry is REST). |
| **Mitigation** | `MessageCoalescer` merges book updates within 50ms windows. Lagging subscribers are dropped by broadcast channel. |
| **Recovery** | Client reconnects and gets a fresh snapshot. |
| **Code** | `crates/api-gateway/src/ws_coalesce.rs` |

### F15 — Rate limit exhaustion (legitimate traffic spike)

| Attribute | Value |
|-----------|-------|
| **Severity** | S3 |
| **Detection** | Spike in 429 responses. Market maker complaints. |
| **Blast radius** | Throttled users can't place orders. Others unaffected (per-key buckets). |
| **Mitigation** | `TieredRateLimiter` gives market makers 300 rps vs default 30 rps. Adjust tier assignment if legitimate. |
| **Recovery** | Buckets refill automatically. No restart needed. |

### F16 — Lock contention under load

| Attribute | Value |
|-----------|-------|
| **Severity** | S3 |
| **Detection** | `lock_wait_*` histograms on `/metrics` show p99 > 1ms. |
| **Blast radius** | Order latency degrades. |
| **Mitigation** | 64-way account sharding (S18B). `parking_lot::Mutex` has no poisoning. ArcSwap for read-heavy config. DashSet for frozen accounts. |
| **Recovery** | Identify the contended lock via metrics. If account lock: increase shard count. If engine queue: check if one symbol is overloaded. |
| **Runbook** | `docs/runbooks/high-latency.md` |

### F17 — Panic in axum handler (sync context)

| Attribute | Value |
|-----------|-------|
| **Severity** | S3 |
| **Detection** | `panic=abort` kills the process. Supervisor (systemd) restarts it. |
| **Blast radius** | Full exchange restart. All in-memory state reloaded via WAL replay. ~1 minute RTO. |
| **Mitigation** | Comprehensive error handling. Don't `.unwrap()` in handler code. |
| **Recovery** | Automatic restart + WAL replay. Check logs for the panic backtrace. |

### F18 — Shutdown race (SIGTERM during heavy load)

| Attribute | Value |
|-----------|-------|
| **Severity** | S3 |
| **Detection** | "Graceful shutdown timeout" in logs. Some background tasks killed mid-operation. |
| **Blast radius** | Possible partial state: audit entries queued but not flushed, positions not snapshot. |
| **Mitigation** | 5-phase graceful shutdown (P1-CP4): halt mode → drain tasks → flush WAL → stop engines → close DB. |
| **Recovery** | WAL replay on restart covers any unflushed matching state. Audit entries re-read from in-memory log if present. |

### F19 — Liquidation cascade

| Attribute | Value |
|-----------|-------|
| **Severity** | S1 |
| **Detection** | Liquidation rate > 100/min. Insurance fund dropping rapidly. Circuit breaker triggered. |
| **Blast radius** | Mass forced liquidations. Insurance fund depletion. Potential socialized loss via ADL. |
| **Mitigation** | Per-symbol circuit breakers. Conservative initial leverage (10x max). Staggered liquidation orders. |
| **Recovery** | `docs/runbooks/liquidation-cascade.md` — halt symbol, assess insurance fund, resume after price stabilizes. |
| **Runbook** | `docs/runbooks/liquidation-cascade.md` |

### F20 — Security breach (suspected unauthorized access)

| Attribute | Value |
|-----------|-------|
| **Severity** | S1 |
| **Detection** | Anomalous withdrawal volume. Admin access from unexpected IP. User reports unauthorized activity. |
| **Blast radius** | Direct financial loss if withdrawal system compromised. Reputational damage. |
| **Mitigation** | Feature flag gate on withdrawals (`ENABLE_WITHDRAWALS=false`). IP-blocked admin endpoints. Audit log hash chain. |
| **Recovery** | `docs/runbooks/suspected-hack.md` — halt withdrawals, preserve evidence, freeze accounts, revoke API keys, investigate. |
| **Runbook** | `docs/runbooks/suspected-hack.md` |

---

## Summary Matrix

| ID | Failure | Sev | Detection Time | Blast Radius | Auto-Recover? |
|----|---------|-----|---------------|--------------|---------------|
| F01 | Engine thread panic | S2 | <1s (error return) | Single symbol | Yes (restart_symbol) |
| F02 | WAL fsync failure | S1 | Immediate (abort) | Full exchange | No (manual disk fix) |
| F03 | Postgres down | S3 | <30s (task error) | Persistence only | Yes (auto-retry) |
| F04 | Replication lag | S4 | Metric-based | Read replicas | Yes (catches up) |
| F05 | Clock skew | S3 | Auth failures | Auth + funding | Yes (NTP sync) |
| F06 | OOM kill | S2 | Process gone | Full exchange | Yes (WAL replay) |
| F07 | Disk full | S2 | Immediate (abort) | Full exchange | No (manual cleanup) |
| F08 | DNS failure | S3 | API call errors | External integrations | Yes (retries) |
| F09 | TLS cert expiry | S2 | External probe | All clients | No (manual renewal) |
| F10 | Hung task | S3 | Heartbeat stale | Depends on task | No (no watchdog yet) |
| F11 | Fireblocks outage | S3 | Poller errors | Deposits/withdrawals | Yes (retries) |
| F12 | Sumsub backlog | S4 | Queue growth | New user onboarding | Yes (auto-approve) |
| F13 | Chainalysis timeout | S3 | Screening errors | Deposit/withdrawal | Yes (retries) |
| F14 | WS fanout backlog | S3 | Channel lag | Market data delay | Yes (reconnect) |
| F15 | Rate limit exhaustion | S3 | 429 spike | Throttled users | Yes (bucket refill) |
| F16 | Lock contention | S3 | Histogram p99 | Order latency | Yes (sharding helps) |
| F17 | Handler panic | S3 | Process restart | Full exchange | Yes (WAL replay) |
| F18 | Shutdown race | S3 | Log warning | Partial flush | Yes (WAL replay) |
| F19 | Liquidation cascade | S1 | Rate spike | Mass liquidation | No (manual halt) |
| F20 | Security breach | S1 | Anomaly detection | Financial loss | No (manual response) |

---

## Next Actions

1. Implement task heartbeat watchdog (P2) — F10 currently has no auto-recovery.
2. Wire SegmentedWal into production — F07 disk-full mitigation depends on old segment cleanup.
3. Set up external synthetic monitoring — F09 TLS cert expiry currently has no automated detection.
4. Add Alertmanager rules for each detection mechanism listed above.
5. Schedule first game day exercise — pick F01 (engine thread panic) as the least risky to simulate.
