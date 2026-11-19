# Phase 4 — Production Readiness

All items required before handling real money.

## Sprint 1: Risk & Safety (Critical)

### 1.1 Dynamic Leverage Tiers
- [ ] `LeverageTier` struct: { min_size, max_size, max_leverage, mmr }
- [ ] `PerpetualConfig.tiers: Vec<LeverageTier>` replaces flat rates
- [ ] `effective_mmr(position_size)` → looks up correct tier
- [ ] `max_leverage_for_size(size)` → decreasing leverage with size
- [ ] Update margin_calculator, liquidation engine, risk engine
- [ ] Defaults: BTC (5 tiers), ETH (4 tiers), SOL (4 tiers)

### 1.2 Reduce-Only & Close-On-Trigger Orders
- [ ] `reduce_only: bool` on PlaceOrderCommand
- [ ] Validation: if reduce_only, qty <= current position size
- [ ] Prevent reduce-only from opening opposite position
- [ ] `close_on_trigger: bool` on TriggerOrder (already exists, wire it)
- [ ] Tests: reduce-only blocks over-close, works with partial fills

### 1.3 Circuit Breakers (Price Bands)
- [ ] `CircuitBreaker` struct per symbol: max_move_pct, window_secs
- [ ] Reject orders that would move price > X% from last trade
- [ ] Configurable bands: BTC ±5%, alts ±10%
- [ ] Temporary halt: if triggered 3x in 60s → pause matching 30s
- [ ] Log + alert on every trigger

### 1.4 Conditional TP/SL on Position
- [ ] When opening position → attach TP and SL trigger orders
- [ ] Auto-cancel attached orders when position closed
- [ ] OCO behavior: filling TP cancels SL and vice versa

## Sprint 2: Account Security

### 2.1 TOTP 2FA
- [ ] `TotpSecret` per user (encrypted at rest)
- [ ] Setup flow: generate secret → QR code → verify code → enable
- [ ] Required for: login, withdrawal, API key creation, 2FA disable
- [ ] Recovery codes (10 one-time codes)
- [ ] Backup: email verification as fallback

### 2.2 API Key Permissions & IP Whitelist
- [ ] `ApiKeyPermissions` { can_read, can_trade, can_withdraw, can_transfer }
- [ ] `allowed_ips: Vec<IpAddr>` per API key
- [ ] Enforce in auth middleware
- [ ] API key management endpoints: create, list, revoke

### 2.3 Withdrawal Security
- [ ] Address whitelist per user
- [ ] New address → 24h cooldown before active
- [ ] Withdrawal requires 2FA confirmation
- [ ] Large withdrawal (>$10K) → manual review queue
- [ ] Rate limit: max 10 withdrawals per 24h

### 2.4 Login Security
- [ ] Email notification on new IP/device login
- [ ] Device fingerprinting + management
- [ ] Session management: view active, revoke
- [ ] Anti-phishing code in all emails

## Sprint 3: Fee Engine & Economics

### 3.1 VIP Fee Tiers
- [ ] 30-day rolling volume per user (spot + derivatives)
- [ ] VIP levels: 0-5 + Supreme
- [ ] Tier thresholds: volume OR asset balance
- [ ] Dynamic fee lookup in fill handler
- [ ] Maker rebates at VIP3+ (negative fees)
- [ ] Daily recalculation job

### 3.2 Referral System
- [ ] Referral code generation per user
- [ ] Commission: X% of referred user's trading fees
- [ ] Multi-level: tier 1 (direct) + tier 2 (indirect)
- [ ] Anti-fraud: IP dedup, device fingerprint, self-referral block
- [ ] Dashboard: earnings, referred users, withdrawable balance

## Sprint 4: Compliance & Monitoring

### 4.1 Market Surveillance (using bot-trader core)
- [ ] Wire `crates/surveillance/` to bot-trader `crates/core/` detectors
- [ ] Spoofing detection: order-to-cancel ratio per user
- [ ] Wash trading: same-entity cross-account detection
- [ ] Layering: stacked orders that vanish on approach
- [ ] Auto-actions: freeze account, cancel orders, alert compliance
- [ ] Case management dashboard

### 4.2 Audit Trail
- [ ] Every action logged: login, order, cancel, withdrawal, admin action
- [ ] Immutable append-only (PostgreSQL + write-only role)
- [ ] Structured: { user_id, action, detail (JSONB), ip, timestamp }
- [ ] 5-year retention policy
- [ ] Export for regulator on request

### 4.3 Balance Reconciliation
- [ ] Scheduled job: sum all user balances per asset
- [ ] Compare with: hot wallet + warm wallet + cold wallet (Fireblocks)
- [ ] Alert if discrepancy > 0.01%
- [ ] Daily report stored in audit log
- [ ] Endpoint: GET /admin/reconciliation

### 4.4 Notification System
- [ ] Event types: fill, liquidation warning (80% margin used), margin call, login, withdrawal
- [ ] Channels: email (critical), WebSocket (all), push (future)
- [ ] User preferences: toggle per event type
- [ ] Liquidation warning: at 70%, 80%, 90% margin usage
- [ ] Template system for email content

## Sprint 5: Advanced Orders

### 5.1 OCO (One-Cancels-Other)
- [ ] `OcoGroup` linking two orders
- [ ] When one fills → cancel the other
- [ ] Works for TP+SL pair on a position

### 5.2 Bracket Orders
- [ ] Entry + TP + SL as atomic unit
- [ ] If entry fills → activate TP and SL as trigger orders
- [ ] If entry cancelled → cancel all

### 5.3 Good-Till-Date
- [ ] `expires_at: Option<DateTime>` on orders
- [ ] Background task: expire_old_orders every 10s

### 5.4 Iceberg Orders
- [ ] `visible_qty` + `hidden_qty`
- [ ] Only `visible_qty` shown in order book
- [ ] When visible fills → refill from hidden
- [ ] Repeat until hidden exhausted

## Sprint 6: Institutional

### 6.1 Sub-Accounts
- [ ] Master account → up to 20 sub-accounts
- [ ] Isolated balances per sub
- [ ] Internal transfer between subs (instant, zero-fee)
- [ ] Per-sub API keys
- [ ] Master sees unified P&L

### 6.2 FIX 4.4 Protocol
- [ ] FIX session management (logon, heartbeat, sequence)
- [ ] NewOrderSingle, OrderCancelRequest, ExecutionReport
- [ ] Dedicated TCP connections for MMs
- [ ] Lower latency than REST/WS path

## Estimated Timeline

| Sprint | Duration | Focus |
|--------|----------|-------|
| Sprint 1 | 2-3 weeks | Risk & Safety |
| Sprint 2 | 2-3 weeks | Account Security |
| Sprint 3 | 1-2 weeks | Fee Engine |
| Sprint 4 | 3-4 weeks | Compliance & Monitoring |
| Sprint 5 | 2-3 weeks | Advanced Orders |
| Sprint 6 | 3-4 weeks | Institutional |
| **Total** | **~14-19 weeks** | |
