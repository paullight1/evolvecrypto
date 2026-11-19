#!/usr/bin/env bash
set -euo pipefail

# Exchange PostgreSQL backup script
#
# Backs up the PostgreSQL database, WAL files, and configuration.
# Verifies the dump is readable before reporting success.
#
# Run daily at 03:00 UTC via cron:
#   0 3 * * * /opt/exchange/scripts/backup.sh >> /var/log/exchange-backup.log 2>&1
#
# Environment variables (override defaults as needed):
#   BACKUP_DIR       — destination directory   (default: /opt/exchange/backups)
#   DATABASE_URL     — PostgreSQL connection   (default: postgres://exchange:exchange@localhost:5432/exchange)
#   RETENTION_DAYS   — days to keep old dumps  (default: 30)
#   EXCHANGE_DATA_DIR — WAL directory          (default: /opt/exchange/data)

BACKUP_DIR="${BACKUP_DIR:-/opt/exchange/backups}"
DB_URL="${DATABASE_URL:-postgres://exchange:exchange@localhost:5432/exchange}"
RETENTION_DAYS="${RETENTION_DAYS:-30}"
WAL_DIR="${EXCHANGE_DATA_DIR:-/opt/exchange/data}"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
DUMP_FILE="$BACKUP_DIR/db/exchange_$TIMESTAMP.dump"

echo "=== Exchange Backup Started: $TIMESTAMP ==="
echo "DB URL:       $DB_URL"
echo "Backup dir:   $BACKUP_DIR"
echo "Retention:    $RETENTION_DAYS days"
echo ""

# 1. PostgreSQL dump (custom format, compression level 6)
mkdir -p "$BACKUP_DIR/db"
echo "[1/5] Dumping PostgreSQL database..."
pg_dump "$DB_URL" \
    --format=custom \
    --compress=6 \
    --file="$DUMP_FILE"
echo "      Saved: $DUMP_FILE ($(du -sh "$DUMP_FILE" | cut -f1))"

# 2. WAL files backup (best-effort — may be empty on fresh installs)
mkdir -p "$BACKUP_DIR/wal"
echo "[2/5] Backing up WAL files..."
WAL_COUNT=$(find "$WAL_DIR" -maxdepth 1 -name "*.wal" 2>/dev/null | wc -l | tr -d ' ')
if [ "$WAL_COUNT" -gt 0 ]; then
    cp "$WAL_DIR"/*.wal "$BACKUP_DIR/wal/"
    echo "      Copied $WAL_COUNT WAL file(s) to $BACKUP_DIR/wal/"
else
    echo "      No WAL files found in $WAL_DIR — skipping"
fi

# 3. Configuration backup
mkdir -p "$BACKUP_DIR/config"
echo "[3/5] Backing up configuration..."
if [ -f /opt/exchange/.env ]; then
    cp /opt/exchange/.env "$BACKUP_DIR/config/env_$TIMESTAMP"
    echo "      Saved: $BACKUP_DIR/config/env_$TIMESTAMP"
else
    echo "      /opt/exchange/.env not found — skipping"
fi

# 4. Cleanup old backups (DB dumps only — WAL and config are small)
echo "[4/5] Removing backups older than $RETENTION_DAYS days..."
DELETED=$(find "$BACKUP_DIR/db" -name "*.dump" -mtime +"$RETENTION_DAYS" -print -delete | wc -l | tr -d ' ')
echo "      Deleted $DELETED old dump(s)"

# 5. Verify the dump is readable (pg_restore --list reads the TOC without touching the DB)
echo "[5/5] Verifying dump integrity..."
if pg_restore --list "$DUMP_FILE" > /dev/null; then
    echo "      Dump verified: TOC readable"
else
    echo "ERROR: Dump verification failed — $DUMP_FILE may be corrupt!"
    exit 1
fi

echo ""
echo "=== Backup Complete: $(date) ==="
echo "    Dump: $DUMP_FILE"
