# Runbook: Database Unavailable

## Severity: P0 — Immediate Response

## Symptoms

- `GET /health` returns `{"status":"degraded","db":"disconnected"}`
- Grafana: `exchange_db_connected` metric = 0
- Error logs contain: `sqlx::Error: connection refused` or `sqlx::Error: pool timed out`
- New user registrations and logins fail with 503
- Withdrawal and deposit endpoints return errors

## Impact

- **Matching engine continues running** — in-memory orderbook still matches orders
- **Balance changes are NOT persisted** — any fills during DB outage risk data loss
- **Withdrawals and deposits halt** — DB records are required for both
- **WAL journal keeps appending** — this is the recovery safety net; do not stop the process
- Duration tolerance: ~2 minutes before the WAL grows large enough to make replay slow

## Response Steps

### Step 1 — Diagnose PostgreSQL (2 min)

```bash
systemctl status postgresql
journalctl -u postgresql -n 50 --no-pager
```

Common causes and their log signatures:
- **Crash / OOM kill**: `FATAL: terminating connection due to administrator command`
- **Disk full**: `FATAL: could not write to file "pg_wal/..."` — check `df -h /var/lib/postgresql`
- **Max connections**: `FATAL: sorry, too many clients already` — check connection count

### Step 2 — Check Disk Space

```bash
df -h /var/lib/postgresql
df -h /opt/exchange/data
```

If disk is over 90%: clear old logs first, then address the underlying cause.

```bash
# Remove old PostgreSQL log files (keep last 7 days)
find /var/log/postgresql -name "*.log" -mtime +7 -delete
```

### Step 3 — Restart PostgreSQL (if crashed)

```bash
systemctl restart postgresql
# Wait 10 seconds for startup
sleep 10
systemctl status postgresql
```

### Step 4 — Check Connection Pool Exhaustion

If PostgreSQL is running but the exchange still cannot connect:

```bash
psql "$DATABASE_URL" -c "SELECT count(*) FROM pg_stat_activity WHERE datname = 'exchange';"
```

If count equals `max_connections` in `postgresql.conf`:

```bash
# Restart the exchange to reset the connection pool
systemctl restart exchange
```

For a permanent fix, deploy PgBouncer in front of PostgreSQL (see Prevention).

### Step 5 — Verify Recovery

```bash
curl http://localhost:8080/health
# Expect: {"status":"ok","db":"connected","wal":"healthy"}
```

### Step 6 — Run Reconciliation

After DB reconnects, compare in-flight state against persisted records:

```bash
curl -s -H "Authorization: Bearer $ADMIN_TOKEN" http://localhost:8080/admin/reconciliation | jq .
```

- If `discrepancies: 0`: no data loss, normal operations can resume.
- If discrepancies exist: **do not resume trading**. Escalate to DB admin.

### Step 7 — If Data Loss is Confirmed

1. Stop the exchange: `systemctl stop exchange`
2. Identify the latest backup: `ls -lt /opt/exchange/backups/db/`
3. Restore: `./scripts/restore.sh /opt/exchange/backups/db/exchange_<timestamp>.dump`
4. Replay WAL from the backup cutoff point (WAL files in `/opt/exchange/backups/wal/`)
5. Bring exchange back up and re-run reconciliation

## Prevention

- **Disk alerts at 80%** — configure Grafana/Prometheus alert on `node_filesystem_avail_bytes`
- **PgBouncer** — pool connections so the exchange never exhausts PostgreSQL's `max_connections`
- **Read replica** — offload admin queries and reconciliation to the replica, protecting the primary
- **Daily backups** — `scripts/backup.sh` via cron at 03:00 UTC; verify restore works monthly

## Contact

Page the DB admin if: restart fails, disk is full, or data loss is confirmed.
Page the on-call engineer for all other cases.
