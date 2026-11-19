# Soft Launch Checklist

Milestone gate for moving from internal testing to beta users and then to public launch.
Each section must be fully checked before proceeding to the next phase.

---

## Phase 0: Pre-Launch (Complete Before Inviting Anyone)

### Infrastructure
- [ ] PostgreSQL running on dedicated server (not shared with other services)
- [ ] WAL directory on a separate disk from PostgreSQL data directory
- [ ] SSL/TLS certificate installed and tested (`curl -v https://yourexchange.com/health`)
- [ ] DNS configured and propagated (A record, CNAME for www)
- [ ] Firewall: only ports 80, 443, and 22 (from admin IPs only) open to public internet
- [ ] Swap space configured (minimum 2x RAM) as OOM kill protection

### Monitoring
- [ ] Grafana dashboards deployed and all panels showing data (not "No Data")
- [ ] PagerDuty / Telegram alerting configured and tested (send a test alert)
- [ ] Alert: `exchange_db_connected = 0` → page on-call immediately
- [ ] Alert: `exchange_order_latency_p99 > 5ms` → page on-call
- [ ] Alert: `exchange_liquidations_per_minute > 100` → page on-call + CEO
- [ ] Alert: `exchange_withdrawal_volume_usd > 3x 24h average in 1h` → page security lead
- [ ] Alert: `node_filesystem_avail_bytes < 20%` → page on-call
- [ ] Alert: `node_cpu_seconds_total > 70% for 5 minutes` → page on-call

### Backups
- [ ] `scripts/backup.sh` is executable and passes a dry-run on the production machine
- [ ] Cron job configured: `0 3 * * * /opt/exchange/scripts/backup.sh >> /var/log/exchange-backup.log 2>&1`
- [ ] Restore drill completed: `scripts/restore.sh` run on a staging machine from a real production dump
- [ ] Backup verification passed: row counts match before and after restore
- [ ] Off-site backup destination configured (S3, Backblaze, or equivalent)

### Security
- [ ] Feature flags set conservatively:
  - `FUTURES_ENABLED=false`
  - `COPY_TRADING_ENABLED=false`
  - `TRADING_BOTS_ENABLED=false`
- [ ] Withdrawal limits configured: $500 USD/day per unverified user
- [ ] Circuit breakers configured and tested:
  - BTC: ±5% from daily open
  - ETH: ±7% from daily open
  - SOL: ±10% from daily open
- [ ] Rate limiting active: 30 requests/second per IP (verify with `wrk` or `hey`)
- [ ] Admin endpoints accessible only from whitelisted IPs
- [ ] Admin 2FA enforced (TOTP)
- [ ] API key permissions checked in every handler (`can_read`, `can_trade`, `can_withdraw`)
- [ ] CORS configured to reject requests from non-allowlisted origins
- [ ] All secrets in environment variables, not in code or config files
- [ ] No hardcoded credentials in the Docker image (`docker history <image>` to confirm)

### Data Integrity
- [ ] Health endpoint returns `{"status":"ok","db":"connected","wal":"healthy"}`
- [ ] WAL replay tested: start exchange, place orders, restart exchange, confirm orders survived
- [ ] Balance reconciliation passes: `GET /admin/reconciliation` returns `{"discrepancies": 0}`
- [ ] All database migrations applied: `sqlx migrate info` shows no pending migrations

### Runbooks
- [ ] All runbooks reviewed by at least one person who did not write them
- [ ] On-call engineer has read every runbook in `docs/runbooks/`
- [ ] Contact table in `docs/runbooks/README.md` filled in with real names and pager IDs
- [ ] At least one runbook has been practiced end-to-end on staging

---

## Phase 1: Beta Launch (First 10–50 Users)

### Launch Day Sequence

- [ ] Deploy latest Docker image to production
- [ ] Run database migrations: `sqlx migrate run`
- [ ] Restart exchange: `systemctl restart exchange`
- [ ] Verify health: `curl https://yourexchange.com/health`
- [ ] Verify WAL replay log: `journalctl -u exchange | grep "WAL replay"`
- [ ] Place a test order manually (limit buy at price below market — should sit on book)
- [ ] Place a matching test order (limit sell at same price — should fill)
- [ ] Verify both balances updated correctly
- [ ] Verify WebSocket trade event received for the fill
- [ ] Cancel the test orders if they did not fill
- [ ] Restore test user balances to starting state if needed
- [ ] Invite first 10 beta users (internal team or trusted contacts only)

### First 4 Hours — Active Monitoring

- [ ] On-call engineer watching Grafana continuously
- [ ] Telegram ops channel monitored by 2+ people
- [ ] Respond to any user issue within 15 minutes
- [ ] Run reconciliation every 30 minutes: `curl .../admin/reconciliation`

### Beta Success Criteria (before expanding to 50 users)

- [ ] Zero data integrity issues in first 48 hours
- [ ] Zero unplanned outages > 2 minutes in first week
- [ ] p99 order latency consistently below 5 ms
- [ ] At least 10 round-trip trades completed (place + fill + balance settled)

---

## Phase 2: First Week

- [ ] Daily reconciliation: run at 09:00 UTC, results logged, discrepancies = 0
- [ ] Daily audit log review: look for unusual patterns, flag anything unexpected
- [ ] Daily backup verification: confirm previous night's backup completed (`/var/log/exchange-backup.log`)
- [ ] Respond to beta user feedback within 24 hours
- [ ] Fix any P0 or P1 issue the same day it is discovered
- [ ] Security review of audit logs (focus on admin endpoint access, API key creation)
- [ ] End-of-week: review all Grafana dashboards for trends (latency creep, queue depth growth, memory growth)

---

## Phase 3: Graduation to Public Launch

All of the following must be true before opening to the public or running any marketing:

### 30-Day Track Record
- [ ] 30 consecutive days with zero data integrity discrepancies
- [ ] 30-day uptime > 99.5% (< 3.6 hours total downtime)
- [ ] 100+ beta trades completed successfully (real money, real users)
- [ ] All P0 and P1 bugs from beta fixed and deployed

### Runbook Validation
- [ ] Every runbook tested at least once on staging or during a real incident
- [ ] Disaster recovery drill completed: full restore from backup on staging, timed and documented
- [ ] RTO (Recovery Time Objective) measured and meets target (< 30 min for P0)
- [ ] RPO (Recovery Point Objective) measured and meets target (< 5 min data loss for P0)

### Security Sign-Off
- [ ] External security audit completed (or scheduled)
- [ ] Penetration test findings resolved (Critical and High severity only required for launch)
- [ ] Proof of reserves mechanism in place and documented
- [ ] Terms of Service and Privacy Policy published and linked from the UI
- [ ] KYC/AML compliance reviewed by legal counsel

### Capacity
- [ ] Load test run at 10x current peak (confirm no latency degradation)
- [ ] Database can handle 100x current row count without query plans degrading
- [ ] Horizontal scaling plan documented if load increases 10x unexpectedly

### Progressive Rollout
- [ ] Withdrawal limits increased per KYC tier (basic: $2,000/day; verified: $50,000/day)
- [ ] Rate limits reviewed with market makers and API users
- [ ] Public marketing plan reviewed and approved by CEO
- [ ] Support channel (Telegram or Discord) set up and staffed during market hours
- [ ] Incident communication template ready (for posting public status updates)
- [ ] Futures trading: enable only after 60 days of stable spot operation
