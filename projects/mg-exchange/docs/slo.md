# Service Level Objectives (SLO)

**Date:** 2026-04-16  
**Status:** v1 — initial declaration based on engineering best-practices audit  
**Review cadence:** quarterly

---

## SLI / SLO Definitions

We define 5 user-facing SLIs. Each SLO is set slightly looser than internal aspirations to establish a meaningful error budget.

| # | SLI | Measurement | SLO Target | Measurement Point |
|---|-----|-------------|------------|-------------------|
| 1 | **Order acknowledgement latency** | p99 of time from HTTP request received to `OrderAccepted` event | **p99 <= 10 ms** | `crates/admin/src/metrics.rs` place_order histogram |
| 2 | **Fill publish latency** | p99 of time from matching engine fill to WS `trade` event broadcast | **p99 <= 50 ms** | post_fill_event_worker timing |
| 3 | **Matching engine availability** | Per-symbol uptime — fraction of 1-minute windows where the engine responds within 100 ms | **>= 99.9% per symbol** | engine_pool health check + circuit breaker state |
| 4 | **WebSocket fanout lag** | p99 of time from market event generation to delivery at the subscriber's TCP socket | **p99 <= 100 ms** | ws_coalesce flush timing |
| 5 | **Withdrawal success rate** | Fraction of withdrawal requests that reach `Completed` state within 60 minutes (excluding user-cancelled) | **>= 99.5%** | custody withdrawal state transitions |

### Error Budget

Each SLO has a **30-day rolling error budget**:

| SLO | Budget (30 days) | Meaning |
|-----|------------------|---------|
| Order ack p99 <= 10 ms | 0.1% of orders may exceed | ~43 "slow" orders per 43,000 |
| Fill publish p99 <= 50 ms | 0.1% of fills may exceed | same ratio |
| Matching availability >= 99.9% | ~43 minutes downtime per symbol | automatic via circuit breaker halt duration |
| WS fanout p99 <= 100 ms | 0.1% of events may exceed | measured at subscriber |
| Withdrawal success >= 99.5% | 0.5% of withdrawals may fail/stall | operator investigation within 1 hour |

### Alerting

Alerts are based on **multi-window multi-burn-rate** (Google SRE Workbook pattern):
- **Fast burn** (2% of 30-day budget consumed in 1 hour) -> **page on-call immediately**
- **Slow burn** (10% of 30-day budget consumed in 6 hours) -> **ticket for next-business-day triage**

Every alert payload includes:
- Severity (page / ticket)
- Runbook URL from `docs/runbooks/`
- Current SLO burn rate
- Last deploy hash

---

## RPO / RTO Targets

| Data Category | RPO (Recovery Point Objective) | RTO (Recovery Time Objective) | Rationale |
|---------------|-------------------------------|-------------------------------|-----------|
| **Trade path** (orders, fills, balances) | **0** (zero data loss) | **<= 5 min** (warm restart with WAL replay) | A confirmed order must never be lost. WAL group commit + fsync ensures durability. |
| **Positions & funding** | **0** | **<= 5 min** | Position snapshots in WAL; funding payments are ledger entries. |
| **KYC records** | **<= 5 sec** (background flusher interval) | **<= 5 min** | Persisted via write-through; on restart, loaded from Postgres. |
| **Audit log** | **<= 5 sec** (flusher interval) | **<= 5 min** | Hash-chain entries flushed to Postgres every 5 seconds. |
| **Market data** (klines, tickers) | **<= 5 min** | **<= 30 min** (cold rebuild from trades) | Can be reconstructed from trade history. Loss is cosmetic, not financial. |
| **Analytics / derived data** | **<= 1 hour** | **<= 1 hour** | Open interest snapshots, performance stats, leaderboards. Non-critical. |

### Recovery Procedures

| Scenario | Procedure | Expected RTO |
|----------|-----------|-------------|
| Process crash (clean restart) | Restart binary -> WAL replay -> resume | < 1 min (warm, ~42K cmds/sec replay) |
| Process crash (corrupted WAL tail) | Truncate to last valid entry -> replay -> resume with gap alert | < 5 min |
| Database primary down | Failover to replica (if configured) or restart primary | 5-15 min (depends on Postgres recovery) |
| Disk failure | Restore from most recent backup + WAL archive -> replay gap | 30-60 min |
| Full DC outage | Cold restart from backup in alternate DC (not yet configured) | Hours — tracked as a future DR initiative |

### Restore Drill

**Cadence:** quarterly  
**Procedure:** restore production backup to staging cluster, verify WAL replay completes, spot-check 10 random accounts against production state, document any drift.  
**First drill:** TBD after production launch.

---

## What's NOT covered (known gaps)

1. **No geographic redundancy.** Single-DC deployment. Full DC outage = hours of downtime. This is acceptable pre-launch; multi-DC is a Phase 3+ initiative.
2. **No automated failover.** Database failover is manual. Automated promotion is a future Patroni/pg_auto_failover integration.
3. **SLO burn-rate alerting not yet wired.** The targets above are declared but Alertmanager rules are not implemented. Tracked as P1-O2 in the backlog.
4. **No external synthetic monitoring.** SLIs are measured from inside the process. External probes (e.g., Grafana synthetic checks, Pingdom) are future work.

---

## References

- Google SRE Book, Chapter 4: "Service Level Objectives"
- Google SRE Workbook, Chapter: "Alerting on SLOs" (multi-window multi-burn-rate)
- Alex Hidalgo, *Implementing Service Level Objectives*
- CME Group system availability disclosures (regulatory filings)
- `docs/research/exchange-engineering-best-practices.md` Section 2.4
