# Game Day Playbook

**Cadence:** quarterly  
**Environment:** staging (production-like, no real funds)  
**Duration:** 2-4 hours per drill

A game day is a controlled chaos exercise: simulate a real failure, execute the runbook, measure recovery time, and update documentation based on what went wrong.

---

## Before the Drill

1. **Pick a scenario** from `docs/fmea.md`. Rotate through the 20 failure modes over ~5 quarters so every scenario gets exercised.

2. **Announce** to all participants 48 hours in advance: date, time, which scenario, who is on-call for the drill.

3. **Pre-check staging** environment:
   - Exchange binary is running with recent production config
   - At least 1 funded account with open orders
   - Monitoring dashboards visible (`ops/grafana/`)
   - Runbook for the chosen scenario printed / accessible

4. **Prepare the injection** method:
   - F01 (engine panic): `kill -9` the matching thread? Not possible — inject a panic via admin endpoint or test order.
   - F02 (WAL fsync fail): `chmod 000` the WAL directory.
   - F03 (DB down): `docker stop postgres`.
   - F06 (OOM): `stress --vm 1 --vm-bytes 90%` on the host.
   - F07 (disk full): `fallocate -l 100G /data/filler`.
   - F09 (cert expiry): Replace cert with an expired one, reload nginx.
   - F11 (Fireblocks down): Block outgoing HTTPS to Fireblocks IP.
   - F19 (liquidation cascade): Rapidly move mark price via admin endpoint.
   - F20 (security breach): Simulate anomalous withdrawals from a test account.

---

## During the Drill

| Step | Action | Record |
|------|--------|--------|
| 1 | **Inject the failure** | Timestamp of injection |
| 2 | **Wait for detection** | How long until monitoring/watchdog alerts? |
| 3 | **Open the runbook** for the scenario | Which runbook? Is it accurate? |
| 4 | **Execute the runbook** step by step | Note any step that fails or is unclear |
| 5 | **Verify recovery** | Check health endpoint, place a test order, verify balances |
| 6 | **Record RTO** | Time from injection to "exchange fully operational" |

### Rules

- **Do not skip runbook steps** — the point is to test the runbook, not to show you can fix it faster without one.
- **Do not fix the failure off-script** — if the runbook is wrong, follow it anyway and note the failure. Fix the runbook after the drill.
- **Record everything** — timestamps, commands run, errors seen, decisions made.

---

## After the Drill

### Debrief (same day, 30 min)

| Question | Answer |
|----------|--------|
| Was the failure detected by monitoring? How quickly? | |
| Was the runbook accurate and complete? | |
| What steps were unclear, missing, or wrong? | |
| What was the actual RTO? Does it meet the target in `docs/slo.md`? | |
| Were there any surprises? | |

### Action Items

For each finding, create a task:
- Runbook inaccuracy → update the runbook
- Missing monitoring → add alert / dashboard panel
- RTO exceeded target → investigate and fix the bottleneck
- New failure mode discovered → add to `docs/fmea.md`

### Archive

Save the drill report to `docs/game-day-reports/YYYY-MM-DD-scenario.md`:
```
# Game Day Report: [Scenario Name]
Date: YYYY-MM-DD
Participants: [names]
Scenario: F[NN] from FMEA
Injection method: [what you did]
Detection time: [N seconds/minutes]
RTO: [N minutes]
SLO target: [from slo.md]
Findings: [list]
Action items: [list with owners]
```

---

## Recommended Rotation

| Quarter | Scenario | Risk Level | Why First |
|---------|----------|------------|-----------|
| Q1 | F01 — Engine thread panic | S2 | Lowest blast radius, safest to simulate |
| Q2 | F03 — Postgres down | S3 | Common in production, tests write-behind resilience |
| Q3 | F02 — WAL fsync failure | S1 | Tests the P0-2 abort behavior |
| Q4 | F19 — Liquidation cascade | S1 | Tests the full incident response chain |
| Q5 | F20 — Security breach | S1 | Tests the suspected-hack runbook end-to-end |

---

## Anti-Patterns

- **"Let's just do it in prod"** — No. Staging first, always. Prod game days are maturity level 4+ and require blast-radius limits, kill switches, and customer communication.
- **Skipping the debrief** — the debrief is where the value is. Without it you just broke something and fixed it.
- **Testing the same scenario every time** — rotate. The point is to exercise *all* failure modes over time.
- **Hero culture** — if one person can fix it without the runbook, the runbook is useless. Test with someone who *hasn't* seen this failure before.
