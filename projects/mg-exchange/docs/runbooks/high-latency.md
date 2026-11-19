# Runbook: High Matching Latency

## Severity: P1 — 15-Minute Response

## Symptoms

- Grafana: `exchange_order_latency_p99` exceeds 5 ms (alert threshold)
- Grafana: `exchange_order_latency_p99` exceeds 20 ms (severe)
- Users report "orders are slow"
- WebSocket trade events are arriving with noticeable delay after REST submission
- `GET /admin/engine/status` shows queue depths > 100 for any symbol

## Impact

- Market makers see stale quotes and widen spreads — liquidity degrades
- High-frequency users cannot operate effectively
- Latency > 50 ms triggers order timeout on some client libraries
- Prolonged high latency can be mistaken for an outage and cause user panic

## Normal Baselines

| Metric | Normal | Warning | Critical |
|--------|--------|---------|---------|
| p50 order latency | < 0.5 ms | > 2 ms | > 10 ms |
| p99 order latency | < 2 ms | > 5 ms | > 20 ms |
| Engine queue depth | < 10 | > 50 | > 500 |
| CPU usage | < 40% | > 70% | > 90% |

## Response Steps

### Step 1 — Identify the Bottleneck (5 min)

Check the three most common causes in order:

**A. CPU saturation:**

```bash
# Overall CPU
top -b -n 1 | head -20

# Which threads are hot?
ps -eLf | grep exchange | sort -k5 -rn | head -20

# Per-core usage
mpstat -P ALL 1 3
```

If a single CPU core is at 100% while others are idle: the matching engine for one symbol is overloaded (expected — engines are single-threaded).

**B. Memory pressure / GC:**

```bash
# Check process RSS
ps aux | grep exchange

# Check for swap usage
free -m
vmstat 1 5
```

Rust has no GC, but high memory usage can cause page faults. If RSS > 80% of RAM, investigate memory growth.

**C. Order queue depth per symbol:**

```bash
curl -s http://localhost:8080/admin/engine/status | jq '.symbols | to_entries | sort_by(-.value.queue_depth) | .[0:10]'
```

If one symbol has queue depth > 500: that symbol's engine is falling behind.

### Step 2 — High Order Rate on a Single Symbol

If one symbol is receiving an abnormally high order rate (e.g., a bot hammering the exchange):

```bash
# Find top submitters in the last 5 minutes
psql "$DATABASE_URL" -c "
    SELECT user_id, COUNT(*) as orders_5min
    FROM orders
    WHERE created_at > NOW() - INTERVAL '5 minutes'
    GROUP BY user_id
    ORDER BY orders_5min DESC
    LIMIT 10;
"
```

If a single user is submitting > 100 orders/sec, temporarily block the
abusive IP via the admin API. Per-user runtime rate-limit overrides are
still set via config+restart; IP-level blocking is the live-traffic knob.

```bash
# 1. Identify the source IP from the audit log
psql "$DATABASE_URL" -c "
    SELECT ip_address, COUNT(*)
    FROM audit_log
    WHERE user_id = '<user_id>'
      AND created_at > NOW() - INTERVAL '5 minutes'
    GROUP BY ip_address
    ORDER BY 2 DESC
    LIMIT 5;
"

# 2. Push the IP into the block list (default TTL 30 min)
curl -s -X POST \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"ip": "<ip>", "duration_secs": 1800, "reason": "rate_limit_abuse"}' \
    http://localhost:8080/admin/security/blocked-ips

# 3. Verify
curl -s -H "Authorization: Bearer $ADMIN_TOKEN" http://localhost:8080/admin/security/blocked-ips | jq .
```

### Step 3 — CPU Saturation (Multi-Symbol Load)

If overall CPU is the issue and no single user is responsible, the exchange is at capacity for this server:

**Immediate mitigations (in order):**

1. Disable low-volume symbols to reduce engine thread count:

```bash
curl -s -X DELETE \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    http://localhost:8080/admin/pairs/DOGEUSDT
```

2. Gate non-critical features behind feature flags to shed load (trading-bots,
   copy-trading, etc. all call into the matching path):

```bash
# Example: pause copy-trading replication to cut cross-symbol traffic
curl -s -X POST \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"flag": "copy_trading_enabled", "enabled": false}' \
    http://localhost:8080/admin/feature-flags
```

3. If neither helps within 5 minutes: add capacity (vertical scale or a second instance behind a load balancer for non-matching traffic).

### Step 4 — Disk I/O Contention

The WAL journal writes to disk on every command. If the disk is saturated, WAL writes block the engine.

```bash
# Check I/O wait
iostat -x 1 5

# Check WAL write latency
journalctl -u exchange -n 50 | grep "wal.*ms"
```

If `%iowait` > 20%:

```bash
# Is the backup script running? (It does pg_dump which is I/O heavy)
ps aux | grep pg_dump

# Check for other heavy I/O processes
iotop -o -b -n 3
```

If backup is the cause: WAL journal and backup should be on separate disks (production config). As immediate mitigation, stop the current backup:

```bash
pkill -f pg_dump   # only if backup contention is confirmed as the cause
```

### Step 5 — Memory Pressure

```bash
# Detailed memory breakdown
cat /proc/$(pgrep exchange)/status | grep -E "VmRSS|VmPeak|VmSize"

# Check for orderbook memory growth (large symbol with many price levels)
curl -s http://localhost:8080/admin/engine/status | jq '.symbols | to_entries | sort_by(-.value.orderbook_levels) | .[0:5]'
```

If orderbook has grown very large (> 100k price levels), it may indicate a
market maker left behind many resting orders. Prune orders that are BOTH old
AND far from mid-price — near-mid orders are left intact so legitimate
liquidity is not disrupted:

```bash
curl -s -X POST \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"symbol": "BTCUSDT", "older_than_hours": 24, "distance_from_mid_pct": "5"}' \
    http://localhost:8080/admin/orders/prune-stale
# → {"symbol":"BTCUSDT","mid_price":"...","candidates":N,"cancelled":N,"failed":0}
```

### Step 6 — Verify Recovery

```bash
# Watch latency return to normal
watch -n 5 'curl -s http://localhost:8080/admin/engine/status | jq ".latency_p99_ms"'

# Confirm queue depths draining
curl -s http://localhost:8080/admin/engine/status | jq '.symbols | to_entries | map(select(.value.queue_depth > 0))'
```

## Prevention

- **Rate limiting** — enforce at the gateway layer; 30 requests/second/IP is the default; no exceptions for API users without a written agreement
- **Per-user order rate limits** — 10 orders/second default; market makers negotiated separately with higher limits
- **WAL on NVMe SSD** — separate disk from PostgreSQL; never on a shared HDD
- **CPU alerts at 70%** — gives time to scale before latency degrades
- **Orderbook depth monitoring** — alert if any symbol's orderbook exceeds 50k levels (indicates stale order accumulation)

## Contact

Page the on-call engineer. No need to wake the CEO unless latency remains > 50 ms for more than 30 minutes.
