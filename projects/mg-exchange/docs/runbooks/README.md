# Exchange Runbooks

Operational playbooks for on-call engineers. Each runbook covers one failure mode:
symptoms, impact, step-by-step response, and prevention.

## Severity Levels

| Level | Response SLA | Definition |
|-------|-------------|------------|
| **P0** | Immediate (< 5 min) | Exchange is down or actively losing money / data |
| **P1** | 15 minutes | Core feature broken; trading impaired |
| **P2** | 1 hour | Degraded performance; workaround exists |
| **P3** | Next business day | Non-critical issue |

## Runbooks Index

| File | Title | Severity | Triggers |
|------|-------|----------|---------|
| [db-down.md](db-down.md) | Database Unavailable | P0 | `/health` shows `db=disconnected` |
| [wal-corruption.md](wal-corruption.md) | WAL Replay Failure | P0 | Exchange fails to start after crash |
| [matching-engine-halt.md](matching-engine-halt.md) | Matching Engine Halt | P0 | Orders accepted but not matched |
| [liquidation-cascade.md](liquidation-cascade.md) | Liquidation Cascade | P0 | Mass liquidations, circuit breaker trips |
| [suspected-hack.md](suspected-hack.md) | Suspected Security Breach | P0 | Unusual withdrawal patterns, audit anomalies |
| [high-latency.md](high-latency.md) | High Matching Latency | P1 | p99 latency > 5 ms |

## Key Contacts

Fill in before going live:

| Role | Name | Pager / Telegram |
|------|------|-----------------|
| On-call engineer | — | — |
| DB admin | — | — |
| Security lead | — | — |
| Exchange CEO | — | — |

## Common Commands

```bash
# Health check
curl http://localhost:8080/health

# Tail exchange logs (systemd)
journalctl -u exchange -f

# Tail exchange logs (Docker)
docker logs -f exchange

# PostgreSQL status
systemctl status postgresql

# Active DB connections
psql "$DATABASE_URL" -c "SELECT pid, usename, application_name, state, query_start FROM pg_stat_activity WHERE datname = 'exchange';"

# Run reconciliation
curl -s -H "Authorization: Bearer $ADMIN_TOKEN" http://localhost:8080/admin/reconciliation | jq .

# Trigger circuit breaker (manual halt of a single symbol's matching engine)
curl -s -X POST -H "Authorization: Bearer $ADMIN_TOKEN" "http://localhost:8080/admin/engine/halt?symbol=BTCUSDT"
```
