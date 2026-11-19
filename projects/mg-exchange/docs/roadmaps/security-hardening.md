# Security Hardening Roadmap — M17 through M26

**Created:** 2026-03-26
**Updated:** 2026-03-27 (v2 — expanded from 8 to 10 marathons after full threat catalogue)
**Status:** IN PROGRESS — M17.5 COMPLETE
**Scope:** Ten focused marathons covering every critical gap identified in comprehensive security audit of 55+ real-world precedents and 20 vulnerability findings.

---

## Threat Model Summary

Before prioritizing marathons, identify what a sophisticated attacker does first:

1. **Account takeover** — steal accounts with real balances (highest immediate ROI).
2. **Withdrawal theft** — exfiltrate funds from compromised or sockpuppet accounts.
3. **Market manipulation** — move price on illiquid pairs, profit on perpetuals.
4. **Compliance evasion** — onboard under false identity, launder through the exchange.
5. **Infrastructure abuse** — DDOS, API scraping, crash the matching engine.
6. **Insider / copy-trading abuse** — front-run followers, self-report inflated stats.

That ordering drives marathon sequencing.

---

## Marathon Overview

| Marathon | Name | Theme | Priority |
|---|---|---|---|
| M17 | Account & Session Hardening | Stop account takeover and withdrawal theft | P0 |
| M17.5 | **Critical Hotfixes** | **FIX auth bypass, social auth guard, mark price audit** | **P0 IMMEDIATE** |
| M18 | Order Pipeline Enforcement | Wire all existing checks into one enforced path | P0 |
| M19 | Surveillance Auto-Actions | Make alerts teeth, not teeth-marks | P0 |
| M20 | Advanced Market Manipulation Detection | Layering, momentum ignition, cross-market, Eisenberg-style | P1 |
| M21 | AML & Sanctions Hardening | Fix the stub, add real blockchain screening | P1 |
| M22 | Compliance Pipeline Integration | KYC gates, withdrawal pipeline, Travel Rule | P1 |
| M23 | Copy-Trading Integrity | Front-running, performance verification | P2 |
| M24 | Infrastructure Persistence & Observability | Durable stores, distributed rate-limit, audit coverage | P2 |
| M25 | **Protocol & Feed Security** | **FIX TLS, WebSocket hardening, index feed protection, algo execution** | **P1** |
| M26 | **Economic Attack Resistance** | **Collateral tiers, PoR integrity, earn timing attacks, fee exploits** | **P1** |

---

## M17 — Account and Session Hardening

**Goal:** Eliminate the most direct paths to financial loss from account compromise and fraudulent withdrawals. An attacker who owns an account should not be able to drain it silently.

**Crates affected:** `api-gateway`, `account`, `custody`, `compliance`

### Tasks

| # | Task | Size | Depends | Acceptance Criteria |
|---|------|------|---------|---------------------|
| 17-1 | `AccountStatus` enforcement at order gateway | S | — | `order-gateway/src/validator.rs` gains a `status: AccountStatus` parameter; orders from `Frozen`, `Suspended`, or `PendingKyc` accounts return `Err(ValidationError::AccountNotTrading)` before any balance lock. Test: frozen user cannot place order. |
| 17-2 | KYC level gate on order placement | S | 17-1 | `PlaceOrderCommand` carries `kyc_level: KycLevel`; validator rejects if `< KycLevel::Basic`. Test: unverified user blocked; KYC-verified user passes. |
| 17-3 | IP allowlist enforcement on API keys | M | — | `ApiKey` struct has an `ip_whitelist: Vec<IpNet>` field that already exists but is not checked. Add `check_ip_allowlist(remote_ip)` in the auth middleware. Empty list = allow all (backward-compatible). Test: key with allowlist rejects foreign IP; key with empty list always passes. |
| 17-4 | Trusted-proxy header validation | M | — | New `TrustedProxyCidr` config field in server startup. `extract_real_ip()` only trusts `X-Forwarded-For` if the connecting IP is in the trusted CIDR; otherwise uses the socket IP directly. Test: spoofed `X-Forwarded-For` from untrusted IP does not bypass IP rate limiting. |
| 17-5 | HMAC/RSA/Ed25519 request signature middleware | M | — | Axum middleware layer `verify_request_signature` checks `X-Signature` header on all `/api/v1/` private endpoints. Keys already support all three algorithms — add the verification. Unsigned requests to private endpoints return 401. Test: valid HMAC sig passes; tampered body fails; missing sig fails; public endpoints unaffected. |
| 17-6 | Withdrawal 2FA confirmation | M | 17-1 | New `WithdrawalConfirmationRequired` flag on `WithdrawalRecord`. Before `PendingApproval -> Processing`, require a fresh TOTP code or email OTP from the withdrawing user. `custody/src/withdrawal.rs` adds `confirm(token, now) -> Result<()>`. Test: withdrawal without confirmation stays in `PendingApproval`; correct TOTP advances it. |
| 17-7 | Withdrawal limits by KYC level enforcement | S | 17-6 | `WithdrawalManager::request()` reads `UserTier` (already defined as `Default/$1K`, `KycBasic/$10K`, `KycAdvanced/$100K`) and rejects if daily sum exceeds tier limit. Test: `Default` user attempting $1,001 in 24h is rejected; `KycBasic` user at $9,999 succeeds. |
| 17-8 | Sub-account STP extension | S | — | `SelfTradeGuard` in `order-gateway/src/self_trade.rs` gains `sub_account_parent_map: HashMap<UserId, UserId>`. Before matching, if buyer and seller share the same master account ID, apply STP. Test: sub-accounts of same master blocked when policy is `Cancel`; accounts with different masters allowed. |
| 17-9 | Multi-account detection heuristics | M | — | New `MultiAccountDetector` in `compliance/src/` that tracks email domain, deposit address, and withdrawal address reuse across user IDs. Emits `ComplianceAlert::MultiAccount` when >= 2 accounts share a deposit address or withdrawal destination. Test: two users depositing to the same on-chain address triggers alert. |
| 17-10 | Adversarial test suite for M17 | M | 17-1 through 17-9 | `crates/account/tests/security_m17.rs` covers: frozen-account order attempt, KYC gate bypass attempt, IP spoof attempt, unsigned API call, withdrawal without 2FA, over-limit withdrawal, cross-sub-account STP, multi-account detection. All 8 scenarios tested and documented. |

### Risks

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| Signature middleware breaks existing API clients | High | Medium | Feature-flag the enforcement; deploy in warn-only mode first, enforce after 2-week deprecation |
| TOTP window drift rejects legitimate withdrawals | Medium | Low | Accept codes from ±1 window (30s grace), same as login flow |
| IP allowlist misconfiguration locks out legitimate bots | High | Medium | Empty list = allow all; document this clearly |

---

## M18 — Order Pipeline Enforcement

**Goal:** Every existing security check must actually fire on every order. The circuit breaker, account status, rate limiting, and IP controls exist in isolation — this marathon wires them together into one sequential pipeline that cannot be bypassed.

**Crates affected:** `matching-engine`, `order-gateway`, `api-gateway`, `risk-engine`

### Tasks

| # | Task | Size | Depends | Acceptance Criteria |
|---|------|------|---------|---------------------|
| 18-1 | Wire `CircuitBreaker` into `EnginePool` dispatch | M | — | `EnginePool::place_order()` calls `circuit_breaker.check_order()` before dispatching to the symbol thread. Currently `CircuitBreaker` exists in `matching-engine/src/circuit_breaker.rs` but `engine_pool.rs` dispatch path does not invoke it. Test: order outside 5% price band is rejected; first order on empty book is allowed (no reference price). |
| 18-2 | Single-entry order pipeline function | M | 18-1 | Create `exchange.rs::validate_and_place_order()` as the sole code path for all order placement. It runs, in sequence: (1) account status check, (2) KYC level check, (3) IP allowlist check, (4) per-user rate limiter, (5) `OrderValidator`, (6) STP check, (7) margin/balance lock, (8) circuit breaker, (9) engine dispatch. No handler in `rest.rs` bypasses this function. Test: integration test simulates each of the 8 checks firing and verifies the correct error is returned. |
| 18-3 | IP-level rate limiting separate from user-level | M | 17-4 | `security.rs` adds `IpRateLimiter` (token bucket, 1000 req/min per IP regardless of user identity). Applied to all `/api/v1/` endpoints before auth. Test: single IP sending 1001 requests/min from different user credentials is still throttled. |
| 18-4 | Position concentration limit | M | — | New `ConcentrationLimitConfig { max_pct_of_oi: Decimal }` in `risk-engine`. Before balance lock, compute `user_position_size / total_open_interest` for the symbol; reject if it would exceed `max_pct_of_oi` (default 20%). Test: user holding 19% of OI can increase; user at 20% blocked; admin can override via config. |
| 18-5 | Max leverage enforcement at gateway | S | — | `OrderValidator::validate()` checks `cmd.leverage <= product.max_leverage` where `max_leverage` is a new field on `Product`. Returns `ValidationError::ExceedsMaxLeverage`. Test: order with leverage 101x on a 100x product is rejected. |
| 18-6 | Portfolio-level margin stop | M | — | New `portfolio_margin_stop` check in `risk-engine/src/lib.rs`: after computing initial margin for new order, verify that total account initial margin across all symbols does not exceed `available_balance`. Return `Err(RiskError::PortfolioMarginInsufficient)`. Test: user with 5 near-full positions cannot open a 6th that would exceed total balance. |
| 18-7 | Funding rate manipulation safeguard | M | — | In `settlement/src/funding.rs`, add a `FundingRateGuard` that clamps the computed funding rate to ±0.05% per epoch (Bybit spec) and additionally flags when the premium index deviates more than 3x from its 24h EWMA, emitting a `SurveillanceAlert::FundingAnomaly`. Test: injected extreme premium index is clamped; anomaly alert fires when premium deviates 4x. |
| 18-8 | Adversarial test suite for M18 | M | 18-1 through 18-7 | `crates/order-gateway/tests/security_m18.rs`: circuit breaker fires on price deviation, pipeline short-circuits on frozen account, IP rate limit fires independently of user limit, concentration limit blocks whale, leverage gate fires, portfolio stop fires, funding manipulation clamped. |

### Risks

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| Pipeline function is a 600-line god function | Medium | Medium | Each stage is a separate `fn` called in sequence; pipeline just calls them |
| Concentration limit set too low disrupts market makers | Medium | Medium | Default 20% is generous for launch; configurable per symbol |
| Portfolio margin stop has edge cases with cross-margin | High | Medium | Test hedged positions explicitly; net delta, not gross |

---

## M19 — Surveillance Auto-Actions

**Goal:** The surveillance system already detects spoofing, wash trading, and quote stuffing. `AutoAction::FreezeAccount` and `AutoAction::CancelOrders` are enum variants but no code ever acts on them. This marathon makes alerts have real consequences.

**Crates affected:** `surveillance`, `api-gateway`, `account`, `order-gateway`

### Tasks

| # | Task | Size | Depends | Acceptance Criteria |
|---|------|------|---------|---------------------|
| 19-1 | Enforce `AutoAction::CancelOrders` | M | 17-1 | When `SurveillanceEngine::check_all()` returns an alert with `auto_action: CancelOrders`, the background surveillance task in `tasks.rs` calls `exchange.cancel_all_orders_for_user(user_id)`. Test: spoofing alert with `CancelOrders` action results in empty open-orders for that user. |
| 19-2 | Enforce `AutoAction::FreezeAccount` | M | 17-1, 19-1 | Same mechanism, calls `exchange.freeze_account(user_id)` which sets `AccountStatus::Frozen`. Subsequent order attempts return `AccountNotTrading`. Test: wash-trading alert with `FreezeAccount` blocks future orders from that user. |
| 19-3 | Enforce `AutoAction::ReduceRateLimit` | S | 18-3 | Sets a per-user rate-limit override of 10 req/s (vs. normal 100) stored in `security.rs::UserRateLimitOverrides`. Rate limiter checks overrides before applying defaults. Test: after alert fires, user is throttled to 10/s. |
| 19-4 | Alert deduplication with cooldown | M | — | `SurveillanceEngine` tracks `last_alert_at: HashMap<(UserId, AlertKind), DateTime<Utc>>`. Identical alert types for the same user within a configurable cooldown (default 5 minutes for Medium, 1 minute for Critical) are suppressed. Test: 100 spoofing events in 1 minute produce 1 alert, not 100. |
| 19-5 | Alert persistence to database | M | — | `SurveillanceAlert` serialized and written to `db` crate table `surveillance_alerts`. On startup, `SurveillanceEngine` re-loads unreviewed alerts. In-memory `alerts: Vec<SurveillanceAlert>` becomes a write-through cache. Test: restart the surveillance engine; pre-existing unreviewed alerts are present; `check_all()` does not duplicate them. |
| 19-6 | Layering / book-depth manipulation detector | L | — | New `LayeringDetector` in `surveillance/src/layering.rs`. Tracks large limit orders (> 5x average order size for the symbol) placed on one side and cancelled within 500ms of the opposite side moving in their favor. Emits `AlertType::Layering` with `severity: High`. Reference: ESMA MAR Article 12 layering definition. Test: simulated layering sequence (place large bid, ask price moves, cancel bid) triggers alert; genuine large orders that stay on book do not. |
| 19-7 | Momentum ignition detector | L | — | New `MomentumIgnitionDetector` in `surveillance/src/momentum_ignition.rs`. Identifies user who places a burst of aggressive market orders sufficient to move price >= 1% within 60s, then reverses position within 120s. Emits `AlertType::MomentumIgnition` with `severity: High`. Reference: IOSCO Market Manipulation Guidance (2013) Section 3.2. Test: simulated buy burst + reverse triggers alert; organic buy wave without reversal does not. |
| 19-8 | Admin alert dashboard endpoints | S | 19-1 through 19-5 | `GET /admin/surveillance/alerts?status=unreviewed&page=N`, `POST /admin/surveillance/alerts/{id}/review`, `GET /admin/surveillance/alerts/{id}`. Returns paginated alerts. Test: alerts created in 19-5 are retrievable; review endpoint sets `reviewed: true`. |
| 19-9 | Adversarial test suite for M19 | M | 19-1 through 19-7 | `crates/surveillance/tests/security_m19.rs`: auto-cancel fires, auto-freeze fires and blocks orders, rate-limit reduction fires, 100 events deduplicate to 1, restart preserves alerts, layering sequence triggers alert, momentum ignition triggers alert. |

### Risks

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| False positives freeze legitimate market makers | High | Medium | `ReduceRateLimit` as first action; `FreezeAccount` only at Critical severity with manual review option |
| Layering detector triggers on normal iceberg order unwinding | Medium | Medium | Exclude known institutional API keys from automatic action (still alert) |
| Alert persistence adds DB latency to hot path | Low | Low | Write asynchronously via background task; detection path stays synchronous |

---

## M20 — Advanced Market Manipulation Detection

**Goal:** Add the three manipulation vectors not currently covered: front-running detection (cross-user order timing), cross-market manipulation (spot price affects perpetual funding), and per-counterparty toxicity scoring.

**Crates affected:** `surveillance`, `market-maker`, `settlement`

### Tasks

| # | Task | Size | Depends | Acceptance Criteria |
|---|------|------|---------|---------------------|
| 20-1 | Front-running detector | L | 19-5 | New `FrontRunningDetector` in `surveillance/src/front_running.rs`. Correlates order timestamps across users: if user A places an order within 50ms of user B's order on the same side, and user A fills at a better price, score the pair. After 5 such occurrences in a 1-hour window, emit `AlertType::FrontRunning`. Requires order timestamp precision to microseconds — verify `PlaceOrderCommand` carries microsecond `created_at`. Test: simulated front-run sequence (A places, B places 40ms later on same side, A fills first) accumulates score. |
| 20-2 | Cross-market manipulation detector | M | 19-5 | New `CrossMarketDetector` in `surveillance/src/cross_market.rs`. Monitors if a user's spot sell orders on BTC/USDT precede a short position on BTC-PERP within 5 minutes. Emits `AlertType::CrossMarketManipulation`. Test: sequence (sell spot, open perp short within 300s) on same user ID triggers alert; random order does not. |
| 20-3 | Per-counterparty toxicity score | M | — | Extend `market-maker/src/` with `ToxicityScorer` that maintains a rolling `toxicity_score: Decimal` per user ID, updated after each trade using an EWMA of `adverse_fill_rate` (how often trades move against the MM immediately after filling). Expose `GET /admin/market-maker/toxicity?user_id=X`. Reference: Kyle's Lambda already implemented in `market_maker.rs` — build on top. Test: user who always trades against the MM builds toxicity > 0.8 within 20 trades; random trader stays near 0.5. |
| 20-4 | Toxicity-based rate limiting | S | 20-3 | If `toxicity_score > 0.85`, automatically apply `ReduceRateLimit` action for that user. Config: `toxicity_rate_limit_threshold: Decimal`. Test: high-toxicity user gets 10/s limit; medium-toxicity user is unaffected. |
| 20-5 | Funding rate anomaly alert integration | S | 18-7 | `FundingAnomaly` alert from M18-7 is routed through `SurveillanceEngine`, stored to DB, and surfaced in admin dashboard. Test: injected anomalous premium appears in admin alerts list within one funding epoch. |
| 20-6 | Alert severity escalation rules | M | 19-4 | New `EscalationPolicy` in `surveillance/src/engine.rs`: if the same user accumulates >= 3 Medium alerts within 24 hours, auto-escalate all their alerts to High and trigger `CancelOrders`. If >= 2 High alerts within 24 hours, escalate to Critical and trigger `FreezeAccount`. Test: 3 Medium alerts in a row produce a High escalation and order cancellation. |
| 20-7 | Adversarial test suite for M20 | M | 20-1 through 20-6 | `crates/surveillance/tests/security_m20.rs`: front-run sequence triggers after 5 occurrences, cross-market sequence triggers, toxic user rate-limited, funding anomaly surfaced, 3 mediums escalate to high with cancel action. |

### Risks

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| Front-running detector has high false-positive rate in fast markets | High | High | Start at observe-only (alert, no auto-action); tune thresholds on live data before enabling auto-actions |
| Cross-market detector requires spot and perp data in same context | Medium | Medium | Surveillance engine already receives all trade events; ensure spot and perp events carry symbol type tag |

---

## M21 — AML and Sanctions Hardening

**Goal:** `aml.rs::screen_address()` always returns `Clean`. Sanctions name matching is exact-only. This marathon replaces stubs with real logic and adds blockchain address scoring.

**Crates affected:** `compliance`

### Tasks

| # | Task | Size | Depends | Acceptance Criteria |
|---|------|------|---------|---------------------|
| 21-1 | Fuzzy name matching for sanctions screening | M | — | Replace exact `HashSet::contains()` in `sanctions.rs` with a two-pass approach: (1) exact match, (2) normalized Jaro-Winkler distance >= 0.92 for names > 5 characters. Add transliteration normalization (Cyrillic → Latin). Reference: FATF Guidance on Risk-Based Approach, para 7.8. Test: "Putln Vladimir" matches "Putin Vladimir" (typo); "Путін Владімір" (Cyrillic) matches after transliteration; "John Smith" does not match "Jane Smith". |
| 21-2 | Real-time OFAC SDN XML feed ingestion | L | — | New background task in `tasks.rs`: every 24 hours, fetch `https://www.treasury.gov/ofac/downloads/sdn.xml`, parse `<sdnEntry>` elements, update `SanctionsScreener` via `bulk_load()`. On parse failure, log error and retain previous list (never silently empty). Reference: OFAC SDN XML schema v2.0. Test: mock XML feed with 3 new entries; after fetch, all 3 entries screen as `Match`; stale entries from before feed are still present. |
| 21-3 | `screen_address()` implementation | M | — | Replace stub in `aml.rs`. Phase 1 (this marathon): cross-reference the address against `SanctionsScreener` (which holds OFAC crypto addresses). Phase 2 placeholder: Chainalysis API call gated behind `CHAINALYSIS_API_KEY` env var. If env var present and address not in local list, call API with 5s timeout. Return `AmlResult::Suspicious` on timeout (fail-closed). Test: address in OFAC crypto list returns `Sanctioned`; unknown address without API key returns `Clean`; unknown address with API key times out returns `Suspicious`. |
| 21-4 | Structuring detection improvement | M | — | Current structuring detection only looks at rapid deposit+withdrawal. Extend to detect smurfing across multiple days: if a user makes >= 5 deposits totaling >= 90% of the reporting threshold within any 7-day window, flag as `Smurfing`. Reference: FinCEN Advisory FIN-2014-A005 (structuring). Test: 5 deposits of $1,800 each over 6 days ($9,000 total, below single threshold) triggers smurfing alert. |
| 21-5 | Withdrawal address ownership verification | M | — | New `ProofOfAddressChallenge` in `compliance/src/`. When a new withdrawal address is added: generate a micro-transaction challenge amount (e.g., $0.01 equivalent of the asset) or, for supported chains, a message-signing challenge. Address is not usable for withdrawal until challenge is completed or a 24h cooldown has passed. Test: new address without challenge cannot be used for withdrawal within cooldown period. |
| 21-6 | AML screening on inter-sub-account transfers | S | — | `account/src/transfers.rs` calls `AmlChecker::screen_transfer()` before executing any transfer between sub-accounts or to external addresses. Test: transfer above threshold triggers AML review; small transfer passes. |
| 21-7 | Adversarial test suite for M21 | M | 21-1 through 21-6 | `crates/compliance/tests/security_m21.rs`: Cyrillic name match, typo match, non-match for similar names, OFAC address blocks withdrawal, structuring 5-deposit scenario, smurfing 7-day pattern, proof-of-address blocks early withdrawal. |

### Risks

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| OFAC SDN XML feed changes schema | Medium | Low | Parse defensively; if schema unrecognized, alert ops and retain previous list |
| Fuzzy matching causes false positives on common names | High | Medium | JW threshold 0.92 is strict; additionally require first-name match; whitelist known-good users |
| Chainalysis API cost and rate limits | Medium | Medium | Cache results for 24h per address; screen only deposits above $1,000 threshold in Phase 1 |

---

## M22 — Compliance Pipeline Integration

**Goal:** KYC status gates trading and withdrawals. The withdrawal pipeline becomes an end-to-end audited flow with all checks enforced sequentially. Travel Rule is enforced on transactions above the threshold.

**Crates affected:** `compliance`, `custody`, `api-gateway`, `account`

### Tasks

| # | Task | Size | Depends | Acceptance Criteria |
|---|------|------|---------|---------------------|
| 22-1 | KYC gate on perpetual trading | S | 17-2 | Users with `KycLevel::None` cannot open perpetual positions (spot trading allowed). Check added in `validate_and_place_order()` from M18-2 for futures products. Test: unverified user blocked on BTCUSDT-PERP; same user succeeds on BTCUSDT spot. |
| 22-2 | KYC gate on withdrawal above $500 | S | 17-7 | `WithdrawalManager::request()` rejects withdrawals above $500 if `kyc_level == KycLevel::None`. Reference: FATF Recommendation 10 (customer due diligence). Test: unverified user withdrawing $501 is rejected; $499 succeeds. |
| 22-3 | Withdrawal pipeline as explicit state machine | M | 17-6, 21-3 | `WithdrawalRecord` state transitions enforced: `PendingApproval -> AmlScreening -> SanctionsCheck -> ConfirmationPending -> ApprovalQueue -> Processing -> OnChain -> Completed`. Each stage gated by the corresponding check. No transition skips a stage. Test: full pipeline integration test: create withdrawal, observe each state transition, confirm each gate can reject. |
| 22-4 | Travel Rule enforcement | M | — | `travel_rule.rs` already has a `TravelRuleEngine`. Enforce it: on withdrawals >= $1,000 (FATF threshold), require originator VASP info if destination is a known VASP address (checked via `VaspDirectory`). Withdrawal blocked until info provided. Test: $1,000 withdrawal to known VASP address requires VASP info; $999 withdrawal bypasses Travel Rule; withdrawal to non-VASP address bypasses Travel Rule. |
| 22-5 | Withdrawal limit by jurisdiction | M | — | `JurisdictionEngine` already exists. Add `WithdrawalLimitByJurisdiction` config: certain jurisdictions (e.g., sanctioned countries, restricted jurisdictions) have $0 withdrawal limit. Test: user with IP from restricted jurisdiction cannot initiate withdrawal; user from unrestricted jurisdiction is subject to KYC-tier limit only. |
| 22-6 | Audit log coverage for all privileged operations | M | — | `AuditLog` already exists. Verify and add logging for: account freeze, account unfreeze, withdrawal approval, withdrawal rejection, KYC override, sanctions list load, admin login. Currently these either log nothing or log inconsistently. Test: perform each privileged action; verify corresponding audit entry exists with actor, timestamp, and affected entity. |
| 22-7 | Real-time sanctions re-screening on deposit | S | 21-2 | After every deposit confirmation, re-screen the source address against the current sanctions list. If a match appears (list was updated since account creation), freeze withdrawals pending review. Test: user deposits, sanctions list updated to include their deposit address, re-screen fires and withdrawal is frozen. |
| 22-8 | Adversarial test suite for M22 | M | 22-1 through 22-7 | `crates/compliance/tests/security_m22.rs`: KYC gate on perp, withdrawal KYC gate, all withdrawal state transitions, travel rule fires at $1K, jurisdiction block, privileged ops in audit log, deposit re-screening. |

### Risks

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| Withdrawal state machine breaks existing in-flight withdrawals on deploy | High | Medium | Migration: all `PendingApproval` records transition to `ConfirmationPending` to restart from clean state |
| Travel Rule integration requires VASP data not available at launch | Medium | High | Gate behind `TRAVEL_RULE_ENFORCEMENT=true` env var; default off for Phase 1 |

---

## M23 — Copy-Trading Integrity

**Goal:** Prevent master traders from front-running their own followers, ensure performance statistics are audited, and add latency controls.

**Crates affected:** `copy-trading`, `surveillance`

### Tasks

| # | Task | Size | Depends | Acceptance Criteria |
|---|------|------|---------|---------------------|
| 23-1 | Front-running protection for copy trading | M | 20-1 | New `CopyFrontRunGuard` in `copy-trading/src/master.rs`. Before broadcasting a signal to followers, check if the master opened the same position within the last 500ms. If yes: broadcast is gated by 500ms delay. If master's fill price is >= 5% better than followers' fill price on average over 10 trades, flag as `CopyFrontRunSuspected`. Test: master who opens position 400ms before broadcast triggers delay; master who broadcasts first is unaffected. |
| 23-2 | Performance statistics verification | M | — | `MasterTrader::total_pnl` and `win_rate` are currently self-reported / incrementally updated without audit. Add `PerformanceAudit::recompute_from_trades(master_id, trade_log) -> PerformanceStats` that independently recomputes stats from the trade journal. Run as daily background task, emit alert if deviation > 5%. Test: master with 3 wins and 7 losses shows `win_rate = 0.30`; manually injected discrepancy of 10% triggers audit alert. |
| 23-3 | Signal latency monitoring | S | — | Track `signal_latency_p99: Duration` in `MasterRegistry` — the 99th percentile of time from master order creation to follower order submission. Expose via `GET /admin/copy-trading/latency`. Alert if P99 > 2 seconds. Test: simulated slow signal propagation (P99 = 3s) triggers latency alert. |
| 23-4 | Follower consent and risk disclosure gate | S | — | `FollowerManager::subscribe()` requires `risk_acknowledged: bool` parameter. If false, subscription is rejected. Risk disclosure text stored in `TosManager`. Test: subscription without acknowledgment fails; with acknowledgment succeeds. |
| 23-5 | Adversarial test suite for M23 | M | 23-1 through 23-4 | `crates/copy-trading/tests/security_m23.rs`: front-run delay applied, front-run suspicion flagged after 10 trades, performance discrepancy alert fires, latency alert fires at P99 > 2s, subscription without consent blocked. |

### Risks

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| 500ms front-run delay is an arbitrary threshold | Medium | Medium | Reference: Bybit Copy Trading FAQ states "signal latency is typically < 200ms"; set 500ms as conservative bound |
| Performance recomputation is expensive for high-frequency masters | Low | Low | Run off the hot path as a daily batch job, not on every trade |

---

## M24 — Infrastructure Persistence and Observability

**Goal:** Every security-critical store that currently lives only in memory must survive restarts. Distributed rate limiting must work across multiple exchange instances. End-to-end integration tests must validate the whole security pipeline.

**Crates affected:** `surveillance`, `compliance`, `api-gateway`, `order-gateway`, `db`

### Tasks

| # | Task | Size | Depends | Acceptance Criteria |
|---|------|------|---------|---------------------|
| 24-1 | Persist AML alerts to database | M | 19-5 | `AmlAlert` in `compliance/src/aml.rs` written to `db` crate `aml_alerts` table on creation. On startup, unreviewed alerts loaded back into `AmlChecker`. Test: restart AmlChecker; unreviewed alert from before restart is present. |
| 24-2 | Persist rate-limit state for multi-instance | M | — | Replace in-memory `HashMap` in `rate_limit.rs` with a Redis-backed token bucket when `REDIS_URL` env var is present. Fallback to in-memory if absent (single-instance mode). Test (with mock Redis): two "instances" sharing a Redis store share a rate-limit counter; 600 requests from instance A + 600 from instance B on same user ID triggers the 1000/min limit. |
| 24-3 | Persist IP reputation state | S | 24-2 | `IpReputationTracker` in `security.rs` uses Redis when available. Failed auth attempts from an IP accumulate across restarts and instances. Test: 4 failed logins across two "instances" count toward the 5-attempt lockout. |
| 24-4 | Persist account freeze state | S | — | `AccountManager` account status (`Frozen`, `Suspended`, etc.) written to `db` crate `account_status` table on every state change. On startup, load all non-`Active` statuses. Test: freeze account, restart AccountManager, verify account is still frozen. |
| 24-5 | End-to-end security pipeline integration test | L | All M17-M24 tasks | New test file `crates/api-gateway/tests/security_e2e.rs`. Scenario: (1) attacker account created, (2) attacker places orders to manipulate price, (3) surveillance detects and freezes account, (4) attacker tries to withdraw, (5) withdrawal blocked due to frozen status and AML screening, (6) admin reviews alert, (7) account remains frozen. All assertions verified against real in-memory state; no mocking of security components. |
| 24-6 | Security audit log completeness check | M | 22-6 | Write an exhaustive list of security-relevant operations and verify each has an audit log entry. Gap analysis: compare operations list against `AuditLog` write sites via code search. Add missing call sites. Test: run `cargo test` audit log coverage test that performs every privileged operation and asserts an audit entry was created. |
| 24-7 | Adversarial test suite for M24 | M | 24-1 through 24-6 | `crates/api-gateway/tests/security_m24.rs`: restart persistence verified for alerts/freeze/AML, multi-instance rate limit coordination, full E2E attack scenario, audit log completeness verified. |

### Risks

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| Redis dependency makes local dev harder | Low | Medium | Fallback to in-memory when `REDIS_URL` absent; CI uses in-memory mode |
| E2E test is slow (multiple subsystem interactions) | Low | Low | Mark with `#[ignore]` for normal CI; run in dedicated security CI job |

---

## M17.5 — Critical Hotfixes (IMMEDIATE — before any marathon)

**Goal:** Fix vulnerabilities that represent complete authentication bypasses or trivially exploitable logic errors. These are not hardening — they are bugs that must be fixed before ANY production traffic.

**Crates affected:** `fix-gateway`, `api-gateway`, `risk-engine`, `index-price`

### Tasks

| # | Task | Precedent | Size | Acceptance Criteria |
|---|------|-----------|------|---------------------|
| 17.5-1 | **FIX gateway authentication** — `tcp_server.rs` accepts connections with NO API key verification on Logon message. Any TCP client can send orders. Add API key auth on Logon, panic at startup if `PRODUCTION=true` and no auth configured. | General: unauthenticated access | S | FIX client without valid API key gets Logout with reason "authentication failed". Production mode refuses to start without auth config. |
| 17.5-2 | **Social auth production guard** — `rest_auth.rs` simulates OAuth when `GOOGLE_CLIENT_ID` not set. Any non-empty token accepted as valid Google auth. Add `PRODUCTION` guard — simulation path must panic at startup in production. | General: auth bypass | XS | `PRODUCTION=true` without `GOOGLE_CLIENT_ID` set → panic with clear message. Dev mode logs warning. |
| 17.5-3 | **Mark price audit** — verify ALL call sites that affect liquidation/margin use `calculate_mark_price_ema()`, not the simple `(2*index + last) / 3` formula. Simple formula gives 33% weight to last trade — one large trade can trigger liquidations. | Mango Markets (2022) | S | Every margin and liquidation code path uses EMA. Simple formula restricted to display-only contexts. Test: single large trade at 2x price does not move mark price by more than configured EMA damping. |
| 17.5-4 | **Index aggregator min_sources → 3** — current `min_sources=2` means attacker who DDoSes 2 of 4 sources controls the median. Raise to 3, require 5+ configured sources for major assets. | Index manipulation attacks | XS | `min_sources=3` in production config. Test: with only 2 sources responding, mark price falls back to stale-price protection. |

### Risks

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| FIX auth breaks existing institutional connections | High | Low (no production FIX clients yet) | Coordinate with any beta testers |
| Social auth guard breaks dev workflow | None | None | Simulation still works without PRODUCTION=true |

---

## M25 — Protocol and Feed Security

**Goal:** Harden all non-REST communication protocols (FIX, WebSocket) and all external data feeds (index sources, market data) against protocol-level attacks and information leakage.

**Crates affected:** `fix-gateway`, `api-gateway` (ws.rs), `index-price`, `algo-execution`, `market-data`

### Tasks

| # | Task | Precedent | Size | Acceptance Criteria |
|---|------|-----------|------|---------------------|
| 25-1 | **FIX TLS enforcement** — add `rustls` TLS termination to FIX TCP server. Require TLS in production, optional in dev. Enforce certificate validation for client connections. | Bitfinex (2016), general: credential interception | L | FIX connections over plaintext rejected in production. `FIX_TLS_CERT`/`FIX_TLS_KEY` env vars required. Test: plaintext connection refused; TLS connection accepted. |
| 25-2 | **FIX session hijack prevention** — enforce strict sequence number tracking per session. Reject messages with out-of-order sequence numbers. Add `MsgSeqNum` gap detection and recovery protocol (ResendRequest). | FIX protocol attacks | M | Out-of-order message rejected with SessionLevelReject. Gap detected → ResendRequest sent. Test: injected out-of-order message causes reject, not execution. |
| 25-3 | **FIX connection limit per IP** — max 5 FIX sessions per source IP. Prevent connection exhaustion attacks. | General: DoS | S | 6th connection from same IP rejected. Test: 5 connections succeed, 6th gets Logout. |
| 25-4 | **WebSocket subscription limit** — max 50 subscriptions per connection. Prevents subscription bombing that exhausts server memory. | General: DoS | S | 51st subscription returns error. Test: 50 subscriptions succeed, 51st rejected. Connection not dropped. |
| 25-5 | **WebSocket connection limit per IP** — max 10 WS connections per IP (or per user if authenticated). | General: connection exhaustion | S | 11th WS connection from same IP rejected with HTTP 429. |
| 25-6 | **WebSocket token expiry enforcement** — background task checks JWT expiry on active WS connections. Expired tokens → disconnect with code 4001. | Session hijacking | M | WS connection disconnected when JWT expires. Test: set short-lived token, wait for expiry, verify disconnection. |
| 25-7 | **Index source deviation circuit breaker** — if any single index source deviates >5% from median of other sources, exclude it and alert. Prevents compromised upstream exchange from affecting our index. | Cross-exchange manipulation, MyEtherWallet DNS (2018) | M | Deviant source excluded from aggregation. Alert fires. Test: inject one source at 2x price, verify excluded and alert generated. Remaining sources still produce valid index. |
| 25-8 | **Index source staleness detection** — if any source hasn't updated in >30s, mark stale and exclude. If all sources stale >60s, halt trading for that symbol. | Stale price exploitation | M | Stale source excluded. All-stale → trading halt via circuit breaker. Test: freeze all mock sources, verify halt fires after 60s. |
| 25-9 | **TWAP/VWAP execution randomization** — add ±15% random jitter to slice timing and ±10% to slice sizing. Never expose future `scheduled_at` values via API. | TWAP front-running (algo execution gaming) | S | Slice times are non-deterministic. API returns only `next_slice_after` (lower bound), not exact time. Test: 100 slices show coefficient of variation > 0.1 in timing. |
| 25-10 | **API information leakage audit** — review all error responses for: balance amounts in errors, position sizes in rejection messages, order IDs in sequential patterns (replace with UUID if numeric). | API front-running, information leakage | M | No error response contains balance, position size, or margin info. Order IDs are UUIDs. Test: all error response bodies reviewed against allowlist of safe fields. |
| 25-11 | **HSTS preload + CSP headers** — add `Strict-Transport-Security: max-age=63072000; includeSubDomains; preload` and `Content-Security-Policy` headers to all HTTP responses. | MyEtherWallet DNS hijack (2018), SSL stripping | S | Headers present on all responses. CSP prevents inline scripts. Test: response header check on 10 representative endpoints. |
| 25-12 | **Adversarial test suite for M25** | All above | M | FIX auth required, TLS enforced, WS limits enforced, stale sources excluded, TWAP randomized, no info leaked in errors. |

---

## M26 — Economic Attack Resistance

**Goal:** Prevent economic attacks that are "within the rules" but drain the exchange — Mango Markets style collateral manipulation, proof-of-reserves gaming, earn product timing attacks, and fee structure exploits.

**Crates affected:** `risk-engine`, `proof-of-reserves`, `settlement`, `order-gateway`, `account`

### Tasks

| # | Task | Precedent | Size | Acceptance Criteria |
|---|------|-----------|------|---------------------|
| 26-1 | **Collateral eligibility tiers** — add `collateral_tier: CollateralTier` and `collateral_weight: Decimal` to asset registry. Tiers: Tier1 (BTC/ETH/USDT/USDC, 95-100%), Tier2 (major alts, 80-90%), Tier3 (small caps, 50-70%), Tier4 (memes/exchange token, 0%). Apply haircuts in ALL margin calculations. | Mango Markets / Eisenberg ($114M, 2022), FTX/FTT circular collateral ($8B, 2022) | L | `available_margin()` and `max_position_size()` apply collateral weights. EXC token = Tier4 = 0% weight (anti-FTX test already exists, extend to all margin calcs). Test: $1000 of Tier3 collateral provides only $600 of margin. |
| 26-2 | **Collateral concentration limit** — single asset cannot constitute >40% of a user's cross-margin collateral (except USDT/USDC). Prevents over-reliance on manipulable asset. | Mango Markets — attacker's collateral was 100% MNGO | M | Order rejected if it would cause single non-stable collateral asset to exceed 40% of total. Test: user with $100K BTC collateral cannot add $200K SOL collateral without adding stables. |
| 26-3 | **PoR completeness enforcement** — `ReportGenerator::generate()` must query database directly for ALL accounts. No caller-provided account slice. Add account count assertion: Merkle tree leaf count == total account count in DB. | PoR gaming — excluding accounts from Merkle tree | M | PoR generation has no `liabilities` parameter — it fetches from DB. Test: create 100 accounts, generate PoR, verify tree has exactly 100 leaves. Manually excluded account causes panic/error. |
| 26-4 | **PoR snapshot timing defense** — PoR snapshots must use a random timestamp within a 24h window, not a predictable schedule. Log snapshot time immutably. Compare 3 consecutive snapshots for >10% asset swings (indicates snapshot gaming). | PoR snapshot gaming — moving funds in temporarily | M | Snapshot timestamp randomized. Alert if asset balance swing >10% between snapshots for any account. Test: account with suspicious deposit-before-snapshot and withdrawal-after triggers alert. |
| 26-5 | **Earn product TWAB** — replace snapshot-based yield calculation with time-weighted average balance (TWAB) over the entire yield period. User who deposits 1 minute before snapshot gets proportional yield, not full-period yield. | Yield timing attacks (deposit-before-snapshot) | M | `calculate_yield(user, period)` uses integral of balance over time, not balance at snapshot. Test: user depositing at 95% through the period receives ~5% of the yield vs. user depositing at start. |
| 26-6 | **Earn pool maximum capacity** — hard cap on total deposits per earn pool. Prevent single user from depositing more than the pool can sustainably pay out. Add per-user maximum as % of pool. | Celsius/BlockFi insolvency from unsustainable yields | S | Pool rejects deposits beyond capacity. Per-user cap = 5% of pool max. Test: pool at 98% capacity accepts $1K deposit; pool at 100% rejects. |
| 26-7 | **Fee minimum notional enforcement** — orders below `min_notional` in `Product` config are rejected even if they pass lot-size checks. Prevents zero-fee micro-orders used for order book manipulation. | Fee exploitation — sub-minimum orders with zero fee | S | Order with notional below `min_notional` rejected. Test: $0.01 notional order rejected even if qty and price are valid tick/lot multiples. |
| 26-8 | **Volume tier gaming detection** — track if user's volume spike pattern correlates with tier boundaries. Flag accounts whose 30-day volume is within 5% of a tier boundary and showing wash-like patterns (same-size orders, rapid cancel-replace). | Fee tier gaming — wash to reach rebate tier | M | Alert fires when user approaches tier boundary with suspicious volume patterns. Test: user at 95% of VIP tier boundary with circular trading pattern triggers alert. |
| 26-9 | **Liquidation cascade circuit breaker** — if >N liquidations fire within 60 seconds on the same symbol, halt new liquidations for 30 seconds (allow only position reductions). Prevents cascade amplification. | BitMEX cascading liquidations (2018-2020) | M | After 10 liquidations in 60s, new liquidations paused. Users can still close positions voluntarily. Test: simulate 11 rapid liquidations, verify 11th is delayed. |
| 26-10 | **Cross-collateral unrealized PnL haircut** — unrealized PnL used as collateral for other positions gets a 50% haircut (not 100% face value). Prevents correlated collapse where position A's paper profit margins position B. | Cross-margin correlated collapse | M | `cross_margin_available()` applies 50% weight to unrealized PnL component. Test: user with $10K unrealized profit gets only $5K additional margin. |
| 26-11 | **Anti-phishing code in emails** — all exchange emails display a user-set anti-phishing code that only the real exchange knows. Users can verify email authenticity. | Phishing / social engineering on users | S | User sets code in settings. All emails from exchange include it. Test: email template renders code. User without code set gets reminder to set one. |
| 26-12 | **Adversarial test suite for M26** | All above | L | Collateral tiers enforced, PoR complete, TWAB correct, fee minimums enforced, cascade halted, cross-collateral haircut applied. |

---

## Cross-Marathon Dependencies

```
M17.5 (Critical hotfixes) ─── FIRST, no dependencies

M17 (Account hardening) ──────┐
  └─> M18 (Pipeline)          │
        └─> M19 (Auto-actions) │
              └─> M20 (Advanced detection)
                                │
M21 (AML/Sanctions) ──────────┤
  └─> M22 (Compliance pipeline)│
                                │
M25 (Protocol/Feed security) ──┤  ← can start in parallel with M17
                                │
M26 (Economic attacks) ────────┤  ← needs M18 for pipeline, otherwise independent
                                │
M19 + M22 ─────────────────────┤
  └─> M23 (Copy trading)      │
                                │
All above ─────────────────────┘
  └─> M24 (Persistence — persists everything built above)
```

**Execution order:**
1. **M17.5** — immediately (4 tasks, ~1 day). Blocks nothing, fixes critical auth bypasses.
2. **M17 + M21 + M25** — in parallel (3 marathons, independent).
3. **M18 + M22** — after M17/M21 respectively.
4. **M19 + M26** — after M18.
5. **M20 + M23** — after M19/M22.
6. **M24** — last (persistence for all above).

---

## Total Scope Estimate

| Marathon | Tasks | Estimated Hours |
|---|---|---|
| M17 | 10 | ~25h |
| M18 | 8 | ~22h |
| M19 | 9 | ~28h |
| M20 | 7 | ~24h |
| M21 | 7 | ~22h |
| M22 | 8 | ~22h |
| M23 | 5 | ~14h |
| M24 | 7 | ~24h |
| **Total** | **61** | **~181h** |

---

## Appendix A — Threat Catalogue (Real-World Precedents)

Every task in M17-M24 traces to at least one real-world incident. This catalogue maps known attacks to our defenses.

### A. Market Manipulation (Traditional Finance)

| ID | Case | Year | Technique | Impact | Our Defense | Status |
|----|-------|------|-----------|--------|-------------|--------|
| A1 | **Navinder Sarao / Flash Crash** | 2010 | Dynamic layering: 4-5 levels of large sell orders, modified (not cancelled) every 200-400ms to stay 3-4 ticks from best. Created fake "wall" = 20-30% of visible depth. Profited on opposite side. | ~$40M profit, $1T intraday market cap wipe | 19-6 (LayeringDetector), **+19-10** (modify tracking), **+19-11** (time-to-cancel), **+19-12** (book depth impact), **+20-8** (multi-level wall), **+20-9** (spoof-and-trade correlator) | PARTIAL → supplemented |
| A2 | **Tower Research / Banging the Close** | 2018 | Placed aggressive orders in final seconds of settlement window to move the settlement price in favor of existing positions | $67.4M SEC settlement | **+NEW-1** Settlement window manipulation detector | GAP |
| A3 | **Panther Energy / Spoofing** | 2015 | Simple cancel-ratio spoofing in natural gas futures | First CFTC spoofing conviction | Existing `spoofing.rs` cancel-ratio detector | COVERED |
| A4 | **Various / Momentum Ignition** | Ongoing | Burst of aggressive market orders to move price ≥1%, trigger stops, then reverse position | Variable | 19-7 (MomentumIgnitionDetector) | COVERED |
| A5 | **Hunt Brothers / Silver Corner** | 1980 | Accumulated 50%+ of silver supply, squeezed short sellers | $1.7B loss when margin called | 18-4 (ConcentrationLimit, 20% of OI) | COVERED |
| A6 | **Churning** | Ongoing | Managed account generates excessive trades for commission income | Variable | **+NEW-2** API-key managed account activity monitor | GAP |

### B. Crypto-Specific Market Manipulation

| ID | Case | Year | Technique | Impact | Our Defense | Status |
|----|-------|------|-----------|--------|-------------|--------|
| B1 | **Telegram P&D Groups** | 2017-now | Coordinate buy signal on illiquid token, dump on retail. Accounts created days before. | $825M (WSJ 2018 study) | **+NEW-3** PumpDetector: price velocity + new account concentration | GAP |
| B2 | **Bitwise Report / Fake Volume** | 2019 | 95% of reported BTC volume was wash trading for exchange ranking manipulation | Industry-wide trust damage | Existing `wash_trading.rs` (volume balance + circular rings) | COVERED |
| B3 | **Maker Rebate Farming** | Ongoing | Self-trade across accounts to earn maker rebates. Same person, different KYC. | Per-exchange fee revenue loss | 17-8 (sub-account STP), **+NEW-4** beneficial ownership STP | PARTIAL |
| B4 | **Stop/Liquidation Hunting** | Ongoing | Large whale deliberately pushes price through stop-loss/liquidation clusters | Retail losses, cascade liquidations | 18-4 (concentration), partial liquidation, **+NEW-5** stop cluster policy | PARTIAL |
| B5 | **Mark Price Manipulation** | 2020-21 | Manipulate index source exchanges to move mark price, triggering liquidations on target exchange | $200M+ across incidents | mark_price.rs (EMA-dampened), weighted median index with min_sources | COVERED |
| B6 | **Funding Rate Manipulation** | Ongoing | Take large position to skew premium index, earn funding from counterparties | Variable | 18-7 (FundingRateGuard, ±0.05% clamp) | COVERED |
| B7 | **Cross-Exchange Manipulation** | Ongoing | Manipulate spot on low-liquidity exchange that feeds index, profit on perp on target exchange | Variable | 20-2 (CrossMarketDetector) | COVERED |
| B8 | **Flash Loan Pattern (CEX equivalent)** | 2020-now | Deposit → manipulate price → profit → withdraw before confirmation | DeFi: $1B+ total | Required confirmations + 24h withdrawal cooldown | COVERED |

### C. Exchange Infrastructure Attacks

| ID | Case | Year | Technique | Impact | Our Defense | Status |
|----|-------|------|-----------|--------|-------------|--------|
| C1 | **Mt. Gox / Transaction Malleability** | 2014 | Modify txid of withdrawal before confirmation, claim "never received", request re-send | 850,000 BTC ($450M at time) | 22-3 (withdrawal state machine, txid tracking) | COVERED |
| C2 | **Bitfinex / Multisig Compromise** | 2016 | Compromised BitGo cosigning server, drained hot wallet | 119,756 BTC ($72M) | Custody circuit breaker ($500K/$10M/$50M limits) | COVERED |
| C3 | **Binance API Key Phishing** | 2019 | Phished API keys + 2FA codes, stole 7,000 BTC from hot wallet | $40M | 17-3 (IP allowlist), 17-6 (withdrawal 2FA) | COVERED |
| C4 | **Binance SYS Pump** | 2018 | Stolen API keys used to pump illiquid SYS token via market buys | Trading halted, losses unclear | **+NEW-6** Unusual pair activity detector per API key | GAP |
| C5 | **KuCoin Hot Wallet** | 2020 | Private key compromised, $281M drained across multiple chains | $281M (most recovered) | Custody circuit breaker + address whitelist | COVERED |
| C6 | **FTX/Alameda / Insider Abuse** | 2022 | Customer funds lent to sister company, FTT used as collateral (circular), backdoor "allow_negative" flag | $8B+ customer losses | Anti-FTX tests (EXC excluded from collateral), no allow_negative, proof-of-reserves | COVERED |
| C7 | **Knight Capital / Deploy Bug** | 2012 | Old code reactivated by deployment, sent millions of unintended orders in 45 min | $440M loss | **+NEW-7** Loss-rate kill switch (halt on rapid loss accumulation) | GAP |
| C8 | **Ronin / Validator Key** | 2022 | Compromised 5/9 validator keys through social engineering | $625M | **+NEW-8** Privileged access expiry/review process | GAP |

### D. Liquidation & Risk Engine Exploits

| ID | Case | Year | Technique | Impact | Our Defense | Status |
|----|-------|------|-----------|--------|-------------|--------|
| D1 | **BitMEX Cascading Liquidations** | 2018-20 | Liquidation engine market-sells into thin book, cascading further liquidations | Repeated flash crashes, customer losses | `max_batch_size` + partial liquidation + insurance fund | COVERED |
| D2 | **OKEx Clawback** | 2018 | Single whale $416M position liquidated, insurance fund insufficient, losses socialized | $9M clawback from profitable traders | ADL system + 18-4 concentration limit | COVERED |
| D3 | **Insurance Fund Drainage** | Theoretical | Create intentionally-losing positions in thin markets designed to exceed insurance fund | Fund depletion | **+NEW-9** Adversarial large-position-in-thin-market test | PARTIAL |
| D4 | **Cross-Margin Correlated Collapse** | Theoretical | Use unrealized PnL from position A to margin position B, both crash simultaneously | Total account wipeout beyond margin | 18-6 (portfolio margin stop) | COVERED |

### E. Account & Identity Exploits

| ID | Case | Year | Technique | Impact | Our Defense | Status |
|----|-------|------|-----------|--------|-------------|--------|
| E1 | **Sybil / Referral Farming** | Ongoing | Create hundreds of accounts for referral bonuses | Revenue loss | 17-9 (MultiAccountDetector) | COVERED |
| E2 | **KYC Deepfake Fraud** | 2023-now | AI-generated faces and documents pass automated KYC | Account creation for money laundering | **+NEW-10** KYC liveness detection requirement | GAP |
| E3 | **SIM Swap → 2FA Bypass** | Ongoing | Port victim's phone number, receive 2FA codes, drain account | Per-account total balance | 17-6 (withdrawal 2FA), **+NEW-11** 48h post-2FA-change withdrawal hold | PARTIAL |
| E4 | **Credential Stuffing** | Ongoing | Automated login attempts using leaked password databases | Account takeover at scale | security.rs (IP brute-force protection, TOTP limiter) | COVERED |
| E5 | **API Key Leakage** | Ongoing | API keys committed to public GitHub repos, scraped by bots within seconds | Full account drainage | 17-3 (IP allowlist), 17-5 (request signature) | COVERED |

### F. Withdrawal & Deposit Exploits

| ID | Case | Year | Technique | Impact | Our Defense | Status |
|----|-------|------|-----------|--------|-------------|--------|
| F1 | **51% / Double-Spend** | 2018-20 | ETC, BTG attacked: reorg chain after confirmation, double-spend deposits | $5.6M (ETC), $18M (BTG) | **+NEW-12** 51% attack screening (NiceHash cost analysis per asset) | GAP |
| F2 | **Withdrawal Race Condition** | Theoretical | Submit two withdrawals simultaneously, both pass balance check before either deducts | Double withdrawal | Rust Mutex atomicity on balance operations | COVERED |
| F3 | **Chain Fork Replay** | 2016-17 | ETH/ETC split: transactions valid on both chains, replay deposits/withdrawals | Variable | **+NEW-13** Fork monitoring + chain-specific replay protection | GAP |

### G. Matching Engine Exploits

| ID | Case | Year | Technique | Impact | Our Defense | Status |
|----|-------|------|-----------|--------|-------------|--------|
| G1 | **Quote Stuffing (Citadel)** | 2020 | Flood matching engine with orders to create latency advantage | Market-wide delays | `quote_stuffing.rs` + M19 auto-actions | COVERED |
| G2 | **Fat Finger Exploitation** | Ongoing | Place limit orders at extreme prices to catch accidental large market orders | Per-trade windfall | 18-1 (circuit breaker wired in) | COVERED |

### H. Copy Trading Exploits

| ID | Case | Year | Technique | Impact | Our Defense | Status |
|----|-------|------|-----------|--------|-------------|--------|
| H1 | **Master Front-Running Followers** | Ongoing | Master trades ahead knowing followers will push price | Follower losses = master profit | 23-1 (CopyFrontRunGuard, 500ms delay) | COVERED |
| H2 | **Fake Performance (Two-Account Trick)** | Ongoing | Trade opposite sides on two accounts, show only the winning one as "master" | Follower losses from following false track record | 23-2 (PerformanceAudit), **+NEW-14** min 90-day/200-trade history | PARTIAL |
| H3 | **Follower Herding for Exit Liquidity** | Theoretical | Accumulate position, broadcast buy to followers (pumping price), sell into their buys | Followers become exit liquidity | **+NEW-15** Simultaneous broadcast enforcement | GAP |

### I. Financial Logic Bugs

| ID | Case | Year | Technique | Impact | Our Defense | Status |
|----|-------|------|-----------|--------|-------------|--------|
| I1 | **TOCTOU / Balance Check Race** | General | Check balance → context switch → another thread spends → original thread also spends | Negative balance | Rust Mutex single-process atomicity, **+NEW-16** withdrawal sequencing adversarial test | COVERED |
| I2 | **Integer Overflow** | General | Amount overflows, wraps to small number or zero | Wrong balance | `rust_decimal` 128-bit throughout | COVERED |
| I3 | **Rounding Direction Exploitation** | General | Exploit rounding direction to extract dust across millions of operations | Accumulated dust theft | Truncate in user's favor for debits, against for credits (CLAUDE.md policy) | COVERED |

### J. Insider Threats

| ID | Case | Year | Technique | Impact | Our Defense | Status |
|----|-------|------|-----------|--------|-------------|--------|
| J1 | **Jérôme Kerviel / SocGen** | 2008 | Fictitious hedging trades to hide €50B unauthorized position | €4.9B loss when unwound | **+NEW-17** Position reconciliation (positions vs. balance locks) | GAP |
| J2 | **Kweku Adoboli / UBS** | 2011 | Booked fictitious ETF trades to offset real losses, exploited back-office reconciliation gaps | $2.3B | 22-6 (audit log), **+NEW-18** 4-eyes approval for large positions | GAP |
| J3 | **FTX / Operator Trading Against Users** | 2022 | Exchange operator's trading firm had privileged order flow access | $8B+ | Separation by code design + proof-of-reserves | COVERED |

---

### K. Protocol & Communication Attacks

| ID | Case | Year | Technique | Impact | Our Defense | Status |
|----|-------|------|-----------|--------|-------------|--------|
| K1 | **MyEtherWallet DNS Hijack** | 2018 | Compromised Google DNS to redirect users to phishing clone | $17M stolen | 25-11 (HSTS preload + CSP), **+25-7** (index source deviation) | NEW |
| K2 | **Celer Network BGP Hijack** | 2022 | BGP route manipulation to intercept API traffic | Bridge fund theft | HSTS + certificate pinning (operational) | PARTIAL |
| K3 | **FIX Session Hijack** | General | Sequence number manipulation, unauthorized order injection | Arbitrary order execution | **M17.5-1** (auth), **25-2** (sequence tracking) | NEW |
| K4 | **WebSocket Subscription Bombing** | General | Subscribe to all symbols on one connection, exhaust server memory | DoS | **25-4** (50 sub limit per connection) | NEW |
| K5 | **Stale Price Exploitation** | 2021-now | Index source goes down, stale price persists, profitable to trade against | Arbitrage at exchange's expense | **25-8** (staleness detection, trading halt) | NEW |

### L. Economic / "Within the Rules" Attacks

| ID | Case | Year | Technique | Impact | Our Defense | Status |
|----|-------|------|-----------|--------|-------------|--------|
| L1 | **Mango Markets / Eisenberg** | 2022 | Inflated MNGO token price via spot buys → used inflated collateral to borrow $114M → drained protocol | $114M | **26-1** (collateral tiers), **26-2** (concentration limit) | NEW |
| L2 | **Celsius/BlockFi Insolvency** | 2022 | Offered unsustainable yields, rehypothecated customer deposits | $4.7B+ (Celsius) | **26-6** (pool capacity cap), **26-5** (TWAB) | NEW |
| L3 | **PoR Snapshot Gaming** | Ongoing | Temporarily move funds in before proof-of-reserves snapshot, withdraw after | False solvency proof | **26-3** (completeness), **26-4** (random timing, swing detection) | NEW |
| L4 | **Yield Timing Attack** | DeFi, ongoing | Deposit right before yield snapshot, withdraw right after | Unfair yield extraction | **26-5** (TWAB replaces snapshot) | NEW |
| L5 | **Fee Tier Gaming** | Ongoing | Wash trade near tier boundary to reach maker rebate tier, then trade profitably | Fee revenue drain | **26-8** (volume tier gaming detection) | NEW |
| L6 | **Zero-Fee Micro-Order Manipulation** | Ongoing | Place sub-minimum orders at zero fee to manipulate order book | Market integrity | **26-7** (min notional enforcement) | NEW |

### M. Algo Execution Gaming

| ID | Case | Year | Technique | Impact | Our Defense | Status |
|----|-------|------|-----------|--------|-------------|--------|
| M1 | **TWAP Front-Running** | General | Predict equal-time slices, front-run each execution | Worse fill for TWAP user | **25-9** (±15% timing jitter, ±10% size jitter) | NEW |
| M2 | **Grid Bot Range Exploitation** | General | Detect user's grid range from order pattern, push price outside range | Grid bot stops working, user stuck | Grid bot awareness (operational monitoring) | LOW PRIORITY |

### N. Social Engineering on Operations

| ID | Case | Year | Technique | Impact | Our Defense | Status |
|----|-------|------|-----------|--------|-------------|--------|
| N1 | **Support Phishing for 2FA Reset** | Ongoing | Impersonate user to customer support, request 2FA reset, drain account | Full account balance | **17-11** (48h post-2FA hold), operational SOPs | COVERED when M17 complete |
| N2 | **Insider Recruitment** | General | External attacker recruits exchange employee for access | Fund theft, data leak | **24-9** (access review), **24-10** (4-eyes approval) | COVERED when M24 complete |
| N3 | **Phishing Exchange Emails** | Ongoing | Clone exchange emails to trick users into entering credentials on fake site | Account takeover | **26-11** (anti-phishing code in emails) | NEW |

---

## Appendix B — New Tasks from Precedent Analysis

Tasks identified from the threat catalogue that are NOT in the original M17-M24 plan.

### Added to M17 (Account Hardening)

| # | Task | Precedent | Size |
|---|------|-----------|------|
| 17-11 | **Post-2FA-change withdrawal hold** — 48h freeze on withdrawals after TOTP device change/reset. Standard SIM swap protection (Coinbase/Kraken pattern). | E3: SIM Swap | S |
| 17-12 | **Withdrawal sequencing adversarial test** — verify lock → record → broadcast → (fail → unlock) order in custody, with injected broadcast failure. | I1: TOCTOU | S |

### Added to M18 (Order Pipeline)

| # | Task | Precedent | Size |
|---|------|-----------|------|
| 18-9 | **Loss-rate kill switch** — halt account if realized PnL drops >$X in 60s OR position accumulates >$Y notional in 60s. Knight Capital would have been stopped in 2 minutes instead of 45. | C7: Knight Capital | M |
| 18-10 | **Stop cluster exposure policy** — if a user's position size > 5% of volume at known stop-loss price clusters (derived from open stop orders), flag for review. Prevents deliberate liquidation hunting. | B4: Stop Hunting | M |

### Added to M19 (Surveillance)

| # | Task | Precedent | Size |
|---|------|-----------|------|
| 19-10 | **Order modification tracking** — `on_order_modified()` event; treat modify as cancel+replace for spoofing scoring. High modify rate = same signal as high cancel rate. | A1: Sarao | M |
| 19-11 | **Time-to-cancel analysis** — track order lifetime. Avg lifetime < 500ms + cancel ratio > 80% = Sarao signature. | A1: Sarao | S |
| 19-12 | **Book depth impact scoring** — `order_qty / total_book_depth_same_side`. User responsible for > 15% depth who then cancels = layering. | A1: Sarao | M |
| 19-13 | **Settlement window manipulation detector** — flag if any account's orders during pre-settlement window (last 60s) move settlement price in direction of their open positions. | A2: Tower Research | M |

### Added to M20 (Advanced Detection)

| # | Task | Precedent | Size |
|---|------|-----------|------|
| 20-8 | **Multi-level wall detector** — user with orders on ≥3 price levels same side, >10% depth combined, all cancelled/modified within 2s. | A1: Sarao | L |
| 20-9 | **Spoof-and-trade correlator** — wall on side A → price moves → fills on side B → wall cancelled. Exact Sarao pattern. | A1: Sarao | L |
| 20-10 | **Pump detector** — symbol moves >5% in <60s AND volume concentrated in accounts created <7 days ago AND <5 accounts = >50% volume. | B1: Telegram P&D | M |
| 20-11 | **Compromised API key unusual pair detector** — per-key "normal symbol set" from history. First-time symbol use triggers flag. | C4: Binance SYS | M |

### Added to M21 (AML/Sanctions)

| # | Task | Precedent | Size |
|---|------|-----------|------|
| 21-8 | **KYC liveness detection requirement** — mandate challenge-response liveness from KYC provider, document deepfake resistance. | E2: KYC Deepfake | S |
| 21-9 | **Beneficial ownership for STP** — if two accounts share government ID from KYC, treat as same beneficial owner for STP + wash trading. | B3: Rebate Farming | M |
| 21-10 | **51% attack asset screening** — before listing PoW token, compute NiceHash 1h attack cost. If <$1M → extra confirmations or decline. | F1: ETC/BTG 51% | M |

### Added to M22 (Compliance Pipeline)

| # | Task | Precedent | Size |
|---|------|-----------|------|
| 22-9 | **Chain fork replay protection** — registry of chain IDs + replay protection status. Auto-suspend deposits/withdrawals on detected fork. | F3: ETH/ETC Replay | M |

### Added to M23 (Copy Trading)

| # | Task | Precedent | Size |
|---|------|-----------|------|
| 23-6 | **Min 90-day/200-trade history** for master registration. Prevents two-account performance fraud. | H2: Fake Performance | S |
| 23-7 | **Simultaneous broadcast enforcement** — master's personal order and follower broadcast submit atomically. Master cannot fill before followers see the signal. | H3: Follower Herding | M |

### Added to M24 (Infrastructure)

| # | Task | Precedent | Size |
|---|------|-----------|------|
| 24-8 | **Daily position reconciliation** — sum of all positions must match sum of all balance locks ±5%. Flag phantom positions. | J1: Kerviel | M |
| 24-9 | **Privileged access quarterly review** — admin accounts + withdrawal API keys must be re-confirmed quarterly. Unconfirmed → auto-downgrade. | C8: Ronin | S |
| 24-10 | **4-eyes approval for positions >$100K notional** — second admin must approve. | J2: Adoboli | M |

---

## Final Scope Summary (v2)

| Marathon | Tasks | Estimated Hours | Theme |
|---|---|---|---|
| **M17.5** | 4 | ~4h | Critical hotfixes (auth bypasses) |
| M17 | 12 | ~30h | Account & session hardening |
| M18 | 10 | ~28h | Order pipeline enforcement |
| M19 | 13 | ~38h | Surveillance auto-actions + Sarao patterns |
| M20 | 11 | ~34h | Advanced manipulation detection |
| M21 | 10 | ~28h | AML & sanctions hardening |
| M22 | 9 | ~24h | Compliance pipeline integration |
| M23 | 7 | ~18h | Copy-trading integrity |
| M24 | 10 | ~28h | Infrastructure persistence |
| **M25** | 12 | ~32h | Protocol & feed security |
| **M26** | 12 | ~34h | Economic attack resistance |
| **Total** | **110** | **~298h** | **10 marathons + 1 hotfix sprint** |

### Coverage: 55+ real-world precedents mapped

| Category | Precedents Covered | Key Cases |
|---|---|---|
| Market manipulation (traditional) | 6 | Sarao, Tower Research, Panther, Hunt Brothers |
| Crypto market manipulation | 8 | Telegram P&D, wash trading, liquidation hunting, funding manipulation |
| Infrastructure attacks | 8 | Mt. Gox, Bitfinex, Binance, KuCoin, FTX, Knight Capital, Ronin |
| Liquidation/risk exploits | 4 | BitMEX cascade, OKEx clawback, insurance drainage, correlated collapse |
| Account/identity exploits | 5 | Sybil, deepfake KYC, SIM swap, credential stuffing, API key leak |
| Withdrawal/deposit exploits | 3 | 51% double-spend, race condition, fork replay |
| Matching engine exploits | 2 | Quote stuffing, fat finger |
| Copy trading exploits | 3 | Front-running, fake performance, follower herding |
| Financial logic bugs | 4 | TOCTOU, overflow, rounding, fee precision |
| Insider threats | 3 | Kerviel, Adoboli, FTX operator |
| Protocol attacks | 4 | FIX hijack, WS bombing, DNS/MITM, stale feeds |
| Economic attacks | 6 | Mango Markets, PoR gaming, yield timing, fee gaming, cascade, collateral |

---

## Definition of Done (All Marathons)

- [ ] `cargo test` passes with 0 failures before and after every marathon
- [ ] `cargo build --release` with 0 new warnings
- [ ] Every new financial formula cites its reference in the relevant `REFERENCES.md`
- [ ] Every new `AutoAction` execution is covered by an adversarial test
- [ ] Every compliance state transition has an audit log entry
- [ ] `TASK_TRACKER.md` updated at the end of each marathon with evidence of completion
- [ ] No security check is bypassable by calling an alternative handler in `rest.rs` — only `validate_and_place_order()` (M18-2) is the entry point

---

## References

### Regulatory & Standards
- FATF Recommendation 10 — Customer Due Diligence
- FATF Recommendation 16 — Wire Transfer (Travel Rule)
- FATF Guidance on Virtual Assets (2021) — AML/CFT for VASPs
- ESMA MAR Article 12 — Definition of Market Manipulation (layering, spoofing, momentum ignition)
- IOSCO Principles for Financial Market Infrastructures — circuit breakers
- IOSCO Market Manipulation Guidance (2013) Section 3.2 — momentum ignition
- OFAC SDN XML Schema v2.0 — sanctions list format
- FinCEN Advisory FIN-2014-A005 — structuring and smurfing patterns
- MiCA (EU Markets in Crypto-Assets) — comprehensive crypto regulation

### Academic
- Kyle (1985) "Continuous Auctions and Insider Trading" — Kyle's Lambda
- Easley, López de Prado, O'Hara (2011) "The Microstructure of the Flash Crash" — VPIN

### Industry
- Bybit API Documentation — funding rate, liquidation, copy trading SLA
- Binance API Documentation — rate limiting patterns, order types
- Coinbase Security Policies — 48h post-2FA-change hold pattern
- NiceHash Marketplace — 51% attack cost estimation methodology

### Precedent Cases (by financial impact)
- FTX/Alameda (2022) — $8B+ customer fund commingling, FTT circular collateral
- Ronin/Axie (2022) — $625M validator key social engineering
- Mt. Gox (2014) — 850K BTC ($450M) transaction malleability
- Knight Capital (2012) — $440M software deployment error
- KuCoin (2020) — $281M hot wallet key compromise
- Bitfinex (2016) — $72M multisig compromise
- Binance (2019) — $40M API key phishing + hot wallet theft
- Navinder Sarao (2010-2015) — $40M dynamic layering/spoofing, contributed to Flash Crash
- OKEx (2018) — $9M clawback from $416M whale liquidation
- ETC 51% (2019) — $5.6M double-spend
- Tower Research (2018) — $67.4M SEC fine for banging-the-close
- Jérôme Kerviel / SocGen (2008) — €4.9B fictitious hedging
- Kweku Adoboli / UBS (2011) — $2.3B phantom ETF trades
