# Runbook: Liquidation Cascade

## Severity: P0 — Immediate Response

## Symptoms

- Grafana: `exchange_liquidations_per_minute` spikes sharply (normal is < 10/min; cascade is > 100/min)
- Grafana: `exchange_circuit_breaker_triggered` counter increases
- Price on the exchange deviates > 3% from the index price
- Multiple users simultaneously reporting liquidations via support
- Logs: many `LIQUIDATION` records in rapid succession for the same symbol
- Matching engine queue depth grows rapidly as liquidation orders flood in

## Impact

- **Forced sell orders from liquidated positions add to downward pressure** — a feedback loop
- **Insurance fund is being drawn down** — monitor `exchange_insurance_fund_balance`
- **Remaining users with margin positions are at risk** — cascade can liquidate solvent users
- **Market quality degrades** — spread widens, depth thins, further accelerating the move

## Background: Liquidation Flow

The risk engine monitors mark price vs. maintenance margin continuously. When a position's margin ratio drops below threshold, the risk engine sends a liquidation order to the matching engine. In a cascade, many positions liquidate simultaneously, their market orders move price further, which triggers more liquidations.

Circuit breakers exist at ±5% (BTC), ±7% (ETH), ±10% (SOL) from the daily open price. When triggered, new order submissions are paused and the exchange enters **price band mode** — only orders within the band are accepted.

## Response Steps

### Step 1 — Confirm Cascade vs. Normal Liquidations (2 min)

```bash
# Check liquidation rate (list of recent liquidations)
curl -s -H "Authorization: Bearer $ADMIN_TOKEN" http://localhost:8080/admin/liquidations | jq '.liquidations | length'

# Check insurance fund balances
curl -s -H "Authorization: Bearer $ADMIN_TOKEN" http://localhost:8080/admin/risk/insurance-funds | jq .

# Check engine / circuit-breaker state for the symbol under pressure
curl -s -H "Authorization: Bearer $ADMIN_TOKEN" "http://localhost:8080/admin/engine/status?symbol=BTCUSDT" | jq .
```

Normal liquidation: rate < 20/min, insurance fund sufficient, circuit breaker inactive.
Cascade: rate > 50/min and/or insurance fund dropping rapidly.

### Step 2 — Verify Circuit Breakers Are Active

Circuit breakers should have triggered automatically. If they have not:

```bash
# Check why circuit breaker did not fire
journalctl -u exchange -n 100 | grep "circuit.breaker"

# Manually halt matching for the symbol (engine halt = circuit breaker manual trip)
curl -s -X POST \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    "http://localhost:8080/admin/engine/halt?symbol=BTCUSDT"
```

### Step 3 — Assess Insurance Fund

```bash
curl -s -H "Authorization: Bearer $ADMIN_TOKEN" http://localhost:8080/admin/risk/insurance-funds | jq .
```

- If insurance fund > 50% capacity: continue — fund can absorb
- If insurance fund < 20% capacity: **consider suspending new futures positions**; alert CEO

```bash
# Suspend new futures position opens (existing positions still managed)
curl -s -X POST \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -d '{"feature": "futures_open", "enabled": false}' \
    http://localhost:8080/admin/feature-flags
```

### Step 4 — Evaluate Mark Price vs. Index Price

```bash
curl -s http://localhost:8080/api/v1/mark-price/BTCUSDT | jq .

# Compare against the aggregated index from external sources
curl -s -H "Authorization: Bearer $ADMIN_TOKEN" http://localhost:8080/admin/index/health | jq .
```

If the exchange mark price diverges > 5% from the external index:
- The cascade is amplifying an internal pricing problem, not just tracking external market
- Halt matching on the affected symbol so mark-to-market stops re-quoting against a broken book:

```bash
curl -s -X POST \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    "http://localhost:8080/admin/engine/halt?symbol=BTCUSDT"
```

This gives the orderbook time to find natural equilibrium without further forced liquidations.

### Step 5 — Monitor and Wait

Once circuit breakers are active and insurance fund is sufficient:
- Watch `exchange_liquidations_per_minute` — it should slow within 2–3 minutes
- Watch `exchange_mark_price_vs_index` — should converge toward 0%
- Do NOT restart the exchange during a cascade — you will lose the in-memory state

### Step 6 — Resume Trading

Only resume when:
1. Liquidation rate has returned to < 10/min for at least 5 consecutive minutes
2. Mark price is within 1% of index price
3. Insurance fund is stable (not still dropping)

```bash
# Resume matching on the halted symbol
curl -s -X POST \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    "http://localhost:8080/admin/engine/resume?symbol=BTCUSDT"

# Re-enable futures open if it was paused
curl -s -X POST \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -d '{"feature": "futures_open", "enabled": true}' \
    http://localhost:8080/admin/feature-flags
```

### Step 7 — Post-Event Analysis

Within 24 hours:
1. Export all liquidation events from this window
2. Identify the trigger: which position(s) started the cascade?
3. Review if maintenance margin ratios are appropriate
4. Review if circuit breaker thresholds fired at the right time
5. Update thresholds in the next deploy if needed

## Prevention

- **Conservative initial leverage limits** — soft-launch: 10x max; increase gradually
- **Maintenance margin buffer** — set maintenance margin 20% above the minimum required by math
- **Staggered liquidation orders** — do not send all liquidation market orders at once; space them 100ms apart to reduce price impact
- **Insurance fund growth target** — 0.05% of daily volume should flow to insurance fund
- **Auto-deleveraging (ADL)** as last resort — before insurance fund is exhausted, reduce winning positions rather than let losers cascade
- **Alert at 30% insurance fund** — page CEO, not just on-call engineer

## Contact

Page on-call engineer immediately.
If insurance fund drops below 30%: page CEO.
If insurance fund drops below 10%: page CEO + consider full trading halt.
