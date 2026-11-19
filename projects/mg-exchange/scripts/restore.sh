#!/usr/bin/env bash
set -euo pipefail

# Exchange PostgreSQL restore script
#
# Drops and recreates the exchange database from a pg_dump custom-format file,
# then verifies row counts to confirm the restore succeeded.
#
# Usage:
#   ./scripts/restore.sh /path/to/exchange_20260401_030000.dump
#
# Environment variables (override defaults as needed):
#   DATABASE_URL — PostgreSQL connection string
#                  (default: postgres://exchange:exchange@localhost:5432/exchange)
#
# CAUTION: This script is DESTRUCTIVE. It terminates all active sessions,
# drops the exchange database, and restores from the provided dump file.
# Run only on a maintenance window with the exchange service stopped.

DUMP_FILE="${1:?Usage: ./restore.sh <dump_file>}"
DB_URL="${DATABASE_URL:-postgres://exchange:exchange@localhost:5432/exchange}"

# Derive plain database name from the connection string (last path component)
DB_NAME=$(basename "$(echo "$DB_URL" | sed 's|.*://[^/]*/||')")
# Derive admin connection (connect to postgres maintenance DB instead of exchange)
ADMIN_URL=$(echo "$DB_URL" | sed "s|/$DB_NAME$|/postgres|")

echo "=== Exchange Restore ==="
echo "Dump file:   $DUMP_FILE"
echo "Database:    $DB_NAME"
echo "Admin URL:   $ADMIN_URL"
echo ""

# Pre-flight: confirm dump file exists and looks valid
if [ ! -f "$DUMP_FILE" ]; then
    echo "ERROR: Dump file not found: $DUMP_FILE"
    exit 1
fi
echo "Verifying dump integrity before proceeding..."
if ! pg_restore --list "$DUMP_FILE" > /dev/null; then
    echo "ERROR: Dump verification failed — file may be corrupt. Aborting."
    exit 1
fi
echo "Dump verified."
echo ""

echo "WARNING: This will DROP and recreate the '$DB_NAME' database!"
echo "         All active sessions will be terminated."
echo "         Ensure the exchange service is stopped before continuing."
echo ""
read -r -p "Type 'yes' to continue: " confirm
[ "$confirm" = "yes" ] || { echo "Aborted."; exit 1; }
echo ""

# 1. Terminate all connections to the target database so it can be dropped
echo "[1/4] Terminating active sessions on '$DB_NAME'..."
psql "$ADMIN_URL" -q -c "
    SELECT pg_terminate_backend(pid)
    FROM pg_stat_activity
    WHERE datname = '$DB_NAME'
      AND pid <> pg_backend_pid();
" 2>/dev/null || true

# 2. Drop and recreate database
echo "[2/4] Dropping and recreating database '$DB_NAME'..."
psql "$ADMIN_URL" -q -c "DROP DATABASE IF EXISTS $DB_NAME;"
psql "$ADMIN_URL" -q -c "CREATE DATABASE $DB_NAME;"
echo "      Database recreated."

# 3. Restore from dump
echo "[3/4] Restoring from dump (this may take several minutes)..."
pg_restore \
    --dbname="$DB_URL" \
    --clean \
    --if-exists \
    --no-owner \
    --no-acl \
    "$DUMP_FILE"
echo "      Restore complete."

# 4. Verify row counts as a basic sanity check
echo "[4/4] Verifying restore..."
USERS=$(psql "$DB_URL" -t -c "SELECT COUNT(*) FROM users;" | tr -d ' ')
ORDERS=$(psql "$DB_URL" -t -c "SELECT COUNT(*) FROM orders;" | tr -d ' ')
BALANCES=$(psql "$DB_URL" -t -c "SELECT COUNT(*) FROM balances;" | tr -d ' ')
echo "      users=$USERS  orders=$ORDERS  balances=$BALANCES"

if [ "$USERS" -eq 0 ]; then
    echo ""
    echo "WARNING: users table is empty — verify the dump contains data."
fi

echo ""
echo "=== Restore Complete: $(date) ==="
echo ""
echo "NEXT STEPS:"
echo "  1. Restart the exchange server: systemctl restart exchange"
echo "  2. The matching engine will rebuild its orderbook from WAL replay."
echo "  3. Verify system health: curl http://localhost:8080/health"
echo "  4. Run balance reconciliation: curl -H 'Authorization: Bearer \$ADMIN_TOKEN' http://localhost:8080/admin/reconciliation"
echo "  5. Monitor Grafana dashboards for 30 minutes before resuming trading."
