# Runbook: WAL Replay Failure

## Severity: P0 — Immediate Response

## Symptoms

- Exchange process fails to start after a crash or intentional restart
- Startup logs contain any of:
  - `WAL replay failed: checksum mismatch at offset ...`
  - `WAL replay failed: unknown command type ...`
  - `WAL replay failed: sequence gap detected (expected N, got M)`
  - `WAL replay error: unexpected EOF`
- Exchange exits with a non-zero code immediately after launch
- `systemctl status exchange` shows `Active: failed`

## Impact

- **Exchange is completely offline** — no orders, no matching, no API
- **In-memory orderbook is gone** — all open orders lost from memory
- **Filled orders and balances exist in PostgreSQL** — DB state is the recovery baseline
- Open positions on futures are still in DB but cannot be managed until exchange restarts

## Background: How WAL Works Here

The WAL (`crates/journal/`) appends a `Command` record (place order, cancel order, match, etc.) for every state-changing operation **before** it executes. On startup, the exchange reads all WAL entries from disk and replays them in order to rebuild the in-memory orderbook. If any entry is unreadable, replay stops and the process exits rather than serving stale state.

The WAL is NOT a replacement for PostgreSQL — it only contains the delta needed to reconstruct the orderbook since the last checkpoint. Balances, user records, and trade history live in PostgreSQL.

## Response Steps

### Step 1 — Read the Exact Error (2 min)

```bash
journalctl -u exchange -n 100 --no-pager | grep -A 5 "WAL"
# or for Docker:
docker logs exchange 2>&1 | tail -100 | grep -A 5 "WAL"
```

Note the exact error type and the WAL offset/sequence number. This determines whether truncation or full restore is needed.

### Step 2 — Identify the WAL File

```bash
ls -lh "${EXCHANGE_DATA_DIR:-/opt/exchange/data}/"
# Look for files with .wal extension and their sizes and timestamps
```

Note the file modified time — this tells you when the last successful write was.

### Step 3 — Try Safe Truncation (if checksum error or unexpected EOF)

A corrupt tail (the last few bytes) is the most common cause — it happens when the process is killed mid-write. The data up to the corrupt byte is valid; truncating the tail is safe.

```bash
# Stop exchange if somehow still running
systemctl stop exchange

# Backup the current WAL before touching it
WAL_DIR="${EXCHANGE_DATA_DIR:-/opt/exchange/data}"
cp "$WAL_DIR/exchange.wal" "$WAL_DIR/exchange.wal.bak.$(date +%Y%m%d_%H%M%S)"

# Find the last valid offset from the error log, then truncate
# Replace LAST_VALID_OFFSET with the offset from Step 1 minus the corrupt record
truncate --size=LAST_VALID_OFFSET "$WAL_DIR/exchange.wal"

# Attempt restart
systemctl start exchange
journalctl -u exchange -f
```

If replay succeeds: verify via reconciliation (Step 6), then investigate what caused the corruption.

### Step 4 — Try WAL Reset (if sequence gap or unknown command type)

A sequence gap or unknown command type means the WAL file itself is structurally invalid — truncation will not help. The safest recovery path is to clear the WAL and let PostgreSQL be the source of truth. The orderbook state can be reconstructed from open orders in the DB.

```bash
systemctl stop exchange

WAL_DIR="${EXCHANGE_DATA_DIR:-/opt/exchange/data}"
# Archive the corrupt WAL for post-mortem analysis
mv "$WAL_DIR/exchange.wal" "$WAL_DIR/exchange.wal.corrupt.$(date +%Y%m%d_%H%M%S)"

# Create an empty WAL — exchange will start from scratch and hydrate from DB
touch "$WAL_DIR/exchange.wal"

systemctl start exchange
journalctl -u exchange -f
```

Watch logs for `WAL replay: 0 commands replayed` and then `Hydrating orderbook from database` — this is expected and correct.

### Step 5 — Full Restore (if DB is also inconsistent)

If the exchange starts but reconciliation shows discrepancies, or if PostgreSQL is also in a bad state, a full restore is required.

```bash
systemctl stop exchange

# Find the latest good backup
ls -lt /opt/exchange/backups/db/

# Restore DB
./scripts/restore.sh /opt/exchange/backups/db/exchange_<timestamp>.dump

# Clear the corrupt WAL
WAL_DIR="${EXCHANGE_DATA_DIR:-/opt/exchange/data}"
mv "$WAL_DIR/exchange.wal" "$WAL_DIR/exchange.wal.pre-restore.$(date +%Y%m%d_%H%M%S)"
touch "$WAL_DIR/exchange.wal"

systemctl start exchange
```

### Step 6 — Verify Recovery

```bash
curl http://localhost:8080/health
# Expect: {"status":"ok","db":"connected","wal":"healthy"}

curl -s -H "Authorization: Bearer $ADMIN_TOKEN" http://localhost:8080/admin/reconciliation | jq .
# Expect: {"discrepancies": 0}
```

If discrepancies are not zero: do not resume trading. Escalate.

### Step 7 — Post-Mortem

Save all artifacts for analysis:
- The corrupt WAL file (`.bak` or `.corrupt` copy)
- Kernel logs: `journalctl -k --since "1 hour ago" > kernel.log`
- Disk I/O stats from Grafana at the time of crash

Common root causes:
- **OOM kill** — kernel kills the process mid-write; add memory or tune `vm.overcommit`
- **Disk full** — partial write at EOF; expand disk or increase backup retention cleanup
- **Power loss** — ensure `fsync` is not disabled in exchange config
- **Bug in WAL writer** — file a bug with the exact offset and command type

## Prevention

- **fsync must be enabled** — never set `WAL_FSYNC=false` in production
- **Checksums on every WAL record** — the journal crate already does this; do not disable
- **Disk space alerts at 80%** — WAL growth goes unnoticed until it fills the disk
- **Periodic WAL checkpoints** — compact WAL after confirmed DB flush to keep replay fast
- **Test restore monthly** — run the full restore drill on a staging machine

## Contact

Page the on-call engineer immediately. If WAL reset (Step 4) does not resolve within 15 minutes, page the DB admin.
