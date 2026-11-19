# Runbook: Suspected Security Breach

## Severity: P0 — Immediate Response

## Symptoms

Any one of these warrants immediate investigation:

- Withdrawal volume spike: > 3x the rolling 24-hour average in under 1 hour
- A single account withdrawing near or above the daily limit with no prior history
- Audit log shows API calls from unexpected IPs for privileged accounts
- Audit log shows bulk account actions (many password resets, many API key rotations) in a short window
- Admin endpoints called from an IP that is not in the whitelist
- Grafana: `exchange_withdrawal_volume_usd` exceeds anomaly threshold
- AML/surveillance system flags a pattern of structuring (many small withdrawals just under the reporting threshold)
- User reports that their account was accessed without their consent

## Impact

- If withdrawal system is compromised: direct financial loss in progress — every minute matters
- If admin access is compromised: attacker may have elevated privileges over the entire system
- If private key is compromised: all hot-wallet funds are at risk

## Response Steps

### Step 1 — Halt All Withdrawals Immediately (< 2 min)

Do not wait for confirmation. If there is reasonable suspicion, halt first and investigate second.

```bash
# Toggle the ENABLE_WITHDRAWALS feature flag off — withdrawal endpoints check
# this flag at request time and will reject new requests with 503 immediately.
curl -s -X POST \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"flag": "ENABLE_WITHDRAWALS", "enabled": false}' \
    http://localhost:8080/admin/feature-flags

# Verify halt is active
curl -s -H "Authorization: Bearer $ADMIN_TOKEN" http://localhost:8080/admin/feature-flags \
    | jq '.ENABLE_WITHDRAWALS'
# Expect: false
```

Post in the internal ops channel: "Withdrawals halted at HH:MM UTC. Investigating potential security incident. Do not re-enable without explicit sign-off."

### Step 2 — Preserve Evidence (< 5 min)

Before changing anything, export all relevant logs. Evidence can be destroyed by log rotation or by an attacker cleaning up.

```bash
# Export audit log for the last 24 hours
psql "$DATABASE_URL" -c "
    COPY (
        SELECT * FROM audit_log
        WHERE created_at > NOW() - INTERVAL '24 hours'
        ORDER BY created_at
    ) TO '/tmp/audit_incident_$(date +%Y%m%d_%H%M%S).csv' CSV HEADER;
"

# Export withdrawal records for the last 24 hours
psql "$DATABASE_URL" -c "
    COPY (
        SELECT w.*, u.email
        FROM withdrawals w
        JOIN users u ON w.user_id = u.id
        WHERE w.created_at > NOW() - INTERVAL '24 hours'
        ORDER BY w.created_at
    ) TO '/tmp/withdrawals_incident_$(date +%Y%m%d_%H%M%S).csv' CSV HEADER;
"

# Save current application logs
journalctl -u exchange --since "24 hours ago" > /tmp/exchange_logs_incident_$(date +%Y%m%d_%H%M%S).log

# Save nginx/load balancer access logs
cp /var/log/nginx/access.log /tmp/nginx_access_incident_$(date +%Y%m%d_%H%M%S).log
```

Store copies in a location the attacker cannot access (offline storage, separate cloud account).

### Step 3 — Identify the Scope

```bash
# Which accounts had recent withdrawals?
psql "$DATABASE_URL" -c "
    SELECT user_id, email, SUM(amount_usd) as total_usd, COUNT(*) as withdrawal_count
    FROM withdrawals w
    JOIN users u ON w.user_id = u.id
    WHERE w.created_at > NOW() - INTERVAL '6 hours'
      AND w.status IN ('completed', 'pending')
    GROUP BY user_id, email
    ORDER BY total_usd DESC
    LIMIT 50;
"

# Which IPs accessed the admin API in the last 24 hours?
psql "$DATABASE_URL" -c "
    SELECT ip_address, COUNT(*) as requests, MIN(created_at) as first_seen, MAX(created_at) as last_seen
    FROM audit_log
    WHERE endpoint LIKE '/admin/%'
      AND created_at > NOW() - INTERVAL '24 hours'
    GROUP BY ip_address
    ORDER BY requests DESC;
"

# Any accounts with API keys created in the last 24 hours that also traded?
psql "$DATABASE_URL" -c "
    SELECT u.id, u.email, ak.created_at as key_created, COUNT(o.id) as orders_placed
    FROM api_keys ak
    JOIN users u ON ak.user_id = u.id
    LEFT JOIN orders o ON o.user_id = u.id AND o.created_at > ak.created_at
    WHERE ak.created_at > NOW() - INTERVAL '24 hours'
    GROUP BY u.id, u.email, ak.created_at
    HAVING COUNT(o.id) > 10
    ORDER BY orders_placed DESC;
"
```

### Step 4 — Assess Hot Wallet

Check the hot wallet balance against what the exchange's ledger says it should be:

```bash
# Per-wallet custody balances (hot / warm / cold) as tracked by the exchange
curl -s -H "Authorization: Bearer $ADMIN_TOKEN" http://localhost:8080/admin/custody/wallets | jq .

# Treasury / fee-revenue accounting (ledger side)
curl -s -H "Authorization: Bearer $ADMIN_TOKEN" http://localhost:8080/admin/treasury | jq .

# Total user liabilities per coin — what the ledger says we owe users
psql "$DATABASE_URL" -c "
    SELECT coin, SUM(available + locked) AS total_liability
    FROM balances
    GROUP BY coin
    ORDER BY coin;
"

# Compare hot-wallet balance from /admin/custody/wallets against actual
# on-chain balance (use a read-only RPC node). This step is specific to your
# blockchain integration — see the hot-wallet service docs.
```

If on-chain hot-wallet balance < `/admin/custody/wallets` hot total: funds have been drained. Escalate immediately.

### Step 5 — Revoke Compromised Access

If a specific account or API key is identified:

```bash
# Revoke every active API key for the user
curl -s -X DELETE \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    "http://localhost:8080/admin/users/<user_id>/api-keys"
# → {"user_id": "...", "revoked": <n>}

# Freeze the user — cancels all open orders and blocks new ones + withdrawals
curl -s -X POST \
    -H "Authorization: Bearer $ADMIN_TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"user_id": "<user_id>", "action": "freeze"}' \
    http://localhost:8080/admin/surveillance/action

# If admin credentials are compromised: rotate the ADMIN_TOKEN secret and
# restart. This invalidates ALL existing admin sessions.
```

### Step 6 — Contact Authorities and Partners

If funds were stolen:

- Contact the blockchain's major exchanges with the destination wallet addresses — they may be able to freeze the funds if the attacker attempts to cash out
- File a police report (required for insurance claims)
- Notify your legal counsel
- Notify affected users within the legally required timeframe (72 hours in most jurisdictions)

### Step 7 — Restore and Harden

Only after the investigation is complete and the vector is understood:

1. Patch the vulnerability
2. Rotate ALL secrets: `DATABASE_URL` password, `ADMIN_TOKEN`, JWT signing key, encryption keys
3. Review and tighten IP whitelist for admin endpoints
4. Re-enable withdrawals only after security lead sign-off
5. Implement additional monitoring for the identified attack pattern

```bash
# Re-enable withdrawals only with explicit approval
curl -s -X POST \
    -H "Authorization: Bearer $NEW_ADMIN_TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"flag": "ENABLE_WITHDRAWALS", "enabled": true}' \
    http://localhost:8080/admin/feature-flags
```

## Do Not

- Do not restart the exchange during the investigation unless absolutely necessary — you may lose in-memory state that contains evidence
- Do not publicly disclose details of the breach until legal counsel has reviewed
- Do not delete logs — even if they are embarrassing, they are needed for investigation
- Do not re-enable withdrawals without a post-mortem and explicit sign-off

## Prevention

- **Withdrawal whitelist per account** — require 24-hour confirmation for new destination addresses
- **Rate limits on withdrawals** — per account and per IP
- **Admin 2FA enforced** — TOTP required for all admin endpoints, no exceptions
- **IP whitelist for admin API** — reject any admin request not from the approved list
- **Anomaly alerts** — Grafana alert if any account withdraws > 10x their 30-day average in 1 hour
- **Multi-sig for large withdrawals** — require 2-of-3 approval for withdrawals over $10,000

## Contact

Page CEO and security lead immediately. This is the one incident where you wake everyone up at 3am without hesitation.
