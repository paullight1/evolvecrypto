# Runbook: Matching Engine Halt

## Severity: P0 — Immediate Response

## Symptoms

- Orders submitted via REST return 200 / accepted, but never fill
- `GET /orderbook/{symbol}` returns stale data (timestamp not updating)
- Grafana: `exchange_orders_matched_total` counter has stopped increasing
- Grafana: `exchange_matching_queue_depth` is growing without bound
- WebSocket trade stream has gone silent for the affected symbol(s)
- Logs may contain: `matching engine thread panicked` or `channel closed`

## Impact

- **All new orders for the affected symbol(s) queue up** but are not executed
- **Existing resting orders are safe** — they remain in the in-memory orderbook
- **Users can still submit and cancel orders** — they just don't fill
- **Futures positions are not marked to market** — liquidation checks are also paused for the symbol

## Background: Engine Architecture

Each trading symbol runs on a **dedicated single-threaded matching engine** (no async, no locks). The `order-gateway` crate sequences orders and sends them via channel to the engine thread. If the engine thread panics, the channel sender in `order-gateway` will return an error on the next send, causing a cascade of 500s on the order submission endpoint.

The engine thread panicking does not bring down the entire process — only that symbol is affected.

## Response Steps

### Step 1 — Identify Affected Symbol(s) (2 min)

```bash
# Check a specific symbol's engine status (registered + circuit breaker state)
curl -s -H "Authorization: Bearer $ADMIN_TOKEN" \
    "http://localhost:8080/admin/engine/status?symbol=BTCUSDT" | jq .

# Or from logs:
journalctl -u exchange -n 200 --no-pager | grep -i "panic\|matching\|engine"
```

### Step 2 — Attempt Soft Restart of the Symbol Engine

The exchange supports per-symbol engine restart without full process restart. This is the preferred first action — it has zero impact on other symbols.

```bash
curl -s -X POST \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    "http://localhost:8080/admin/engine/restart?symbol=BTCUSDT"
```

Watch the logs for confirmation:

```bash
journalctl -u exchange -f | grep "BTCUSDT"
# Expect: "Matching engine for BTCUSDT restarted, replaying N queued orders"
```

If the engine restarts successfully: verify fills resume, then proceed to Step 5 for root cause.

### Step 3 — Full Process Restart (if per-symbol restart fails)

```bash
systemctl restart exchange
```

The exchange will replay the WAL on startup and rebuild all orderbooks. This takes 5–30 seconds depending on WAL size. Monitor:

```bash
journalctl -u exchange -f
# Expect: "WAL replay complete: N commands in Xms"
# Then: "HTTP server listening on :8080"
```

Verify all symbols are matching again:

```bash
curl -s http://localhost:8080/admin/engine/status | jq '.symbols[] | select(.queue_depth > 0)'
```

### Step 4 — Check for Panic Cause

A panic is always a bug. Before moving on, capture the panic message:

```bash
journalctl -u exchange --since "30 min ago" | grep -A 30 "panicked at"
```

Common causes:

| Panic message | Cause | Immediate mitigation |
|--------------|-------|----------------------|
| `arithmetic operation overflowed` | Price or quantity overflow | Check for malformed order input |
| `index out of bounds` | Bug in level iteration | Upgrade to latest exchange binary |
| `called unwrap() on None` | Unexpected missing order state | Upgrade to latest binary |
| `channel closed` | Order-gateway sent to dead engine | Already resolved by restart |

File a P1 bug with the full panic backtrace regardless of cause.

### Step 5 — Verify No Orders Were Lost

After restart, check that queued orders were not silently dropped:

```bash
# Confirm the engine is responding again
curl -s -H "Authorization: Bearer $ADMIN_TOKEN" \
    "http://localhost:8080/admin/engine/status?symbol=BTCUSDT" | jq .
# Expect: {"symbol":"BTCUSDT","engine_registered":true,"circuit_breaker_halted":false}

# Compare DB pending orders against engine's current orderbook
curl -s -H "Authorization: Bearer $ADMIN_TOKEN" http://localhost:8080/admin/reconciliation | jq .

# Also check for any orders stuck in 'pending' state for > 60 seconds
psql "$DATABASE_URL" -c "
    SELECT id, user_id, symbol, side, price, qty, status, created_at
    FROM orders
    WHERE status = 'pending'
      AND created_at < NOW() - INTERVAL '60 seconds'
    ORDER BY created_at;
"
```

If stuck pending orders exist: they need to be either injected into the engine or cancelled and the users notified.

### Step 6 — Communicate to Users

If the outage lasted > 2 minutes, post a status update. Template:

> "We experienced a brief disruption to order matching for [SYMBOL] from HH:MM to HH:MM UTC. All orders placed during this window are being processed. No orders were lost. We apologize for the inconvenience."

## Escalation

- If per-symbol restart does not work within 5 minutes: page on-call senior engineer
- If full restart fails: P0 escalation — wake up the CEO and DB admin
- If orders appear to have been lost: treat as data loss — refer to WAL runbook

## Per-Symbol Admin Endpoints

These endpoints require the `Authorization: Bearer $ADMIN_TOKEN` header.

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/admin/engine/status?symbol=BTCUSDT` | Engine registered + circuit breaker state |
| `POST` | `/admin/engine/restart?symbol=BTCUSDT` | Drop + respawn the engine thread |
| `POST` | `/admin/engine/halt?symbol=BTCUSDT` | Force-halt via circuit breaker (manual, no expiry) |
| `POST` | `/admin/engine/resume?symbol=BTCUSDT` | Clear manual halt |

Note: the exchange-wide halt (`POST /admin/halt` / `POST /admin/resume`) halts **all** symbols and is a separate mechanism.

## Prevention

- **Catch panics per engine thread** — wrap the engine loop in `std::panic::catch_unwind` and restart the thread automatically; page for any caught panic
- **Input validation at the gateway layer** — reject malformed prices/quantities before they reach the engine
- **Matching engine fuzz tests** — run AFL/libfuzzer against the engine on every release
- **Alerting on `exchange_matching_queue_depth`** — alert if any symbol queue depth exceeds 1000 for more than 30 seconds

## Contact

Page the on-call engineer immediately. If not resolved in 10 minutes, escalate to senior engineering.
