# Validation Marathon — Make Everything Actually Work

## V1: Backend API Validation (find broken endpoints)

### V1.1 Compile-Time Audit
- [ ] `cargo clippy -- -D warnings` — fix ALL warnings
- [ ] `cargo fmt --check` — format entire workspace
- [ ] Audit every `.unwrap_or_else(|e| e.into_inner())` — log when mutex was poisoned
- [ ] Check all `#[ignore]` tests — document which need DB to run

### V1.2 Endpoint Smoke Tests (automated)
Create `tests/api_smoke.rs` in api-gateway — integration tests hitting every endpoint:
- [ ] Health: GET /health → 200
- [ ] Auth: POST /auth/register + /auth/login → tokens
- [ ] Orders: POST /orders → place, GET /orders → list, DELETE → cancel
- [ ] Positions: GET /positions → empty or list
- [ ] Balances: GET /account/balances → list
- [ ] Wallet: GET /account/wallet → overview
- [ ] Trigger orders: POST/GET/DELETE
- [ ] Deposit address: POST /account/deposit-address
- [ ] Withdrawal: POST /account/withdraw
- [ ] Funding: GET /funding/rate, /funding/history
- [ ] Mark price: GET /markets/{sym}/mark-price
- [ ] Orderbook: GET /markets/{sym}/orderbook
- [ ] All admin endpoints
- [ ] Copy trading, bots, referral, settings
- [ ] Fees, status, por

### V1.3 Response Format Validation
- [ ] Every endpoint returns consistent JSON shape
- [ ] Error responses: always `{"error": "message"}`
- [ ] Success responses: consistent field naming (snake_case)
- [ ] Decimal fields: always strings (not numbers) for precision
- [ ] Timestamps: always ISO 8601 or unix millis (pick one, be consistent)

## V2: E2E Flow Tests

### V2.1 Registration → Trading Flow
- [ ] Register new user (email + password)
- [ ] Login → get JWT tokens
- [ ] Get balances (should be empty)
- [ ] Get deposit address (BTC)
- [ ] Simulate deposit (mock webhook)
- [ ] Verify balance increased
- [ ] Place limit buy order
- [ ] Place limit sell order (different user) → match
- [ ] Verify fill event on WebSocket
- [ ] Verify balance changed
- [ ] Verify trade in history

### V2.2 Futures Trading Flow
- [ ] Open long position (10x leverage)
- [ ] Verify margin deducted
- [ ] Verify position in GET /positions
- [ ] Set TP/SL trigger orders
- [ ] Close position (market sell)
- [ ] Verify realized PnL credited
- [ ] Verify position removed

### V2.3 Withdrawal Flow
- [ ] Add address to whitelist
- [ ] Wait 24h cooldown (or mock time)
- [ ] Submit withdrawal
- [ ] Verify 2FA check (if enabled)
- [ ] Verify daily limit check
- [ ] Verify travel rule (if > $1000)
- [ ] Admin approve withdrawal
- [ ] Verify balance deducted

### V2.4 KYC Flow
- [ ] Start KYC (mock provider)
- [ ] Submit webhook (approved)
- [ ] Verify status → Verified
- [ ] Verify trading limits upgraded

## V3: Frontend Remaining Features

### V3.1 Announcement Banner
- [ ] Top banner for system announcements
- [ ] Dismissible (localStorage remember)
- [ ] Types: info (blue), warning (yellow), critical (red)
- [ ] Fetch from API or hardcode for Phase 1

### V3.2 Maintenance Mode Page
- [ ] Full-screen overlay when exchange is in maintenance
- [ ] Check feature flag: if all trading disabled → show maintenance page
- [ ] Countdown timer to estimated restore
- [ ] Status updates

### V3.3 Mobile Polish
- [ ] Test every page at 375px width
- [ ] Fix overflow issues
- [ ] Touch-friendly: all buttons min 44px
- [ ] Swipe gestures for tabs on mobile
- [ ] Bottom sheet modals instead of centered modals on mobile

### V3.4 Missing Small Features
- [ ] Password strength indicator on register (already partial)
- [ ] Remember me on login (extends token TTL)
- [ ] Dark/Light theme toggle (CSS variables already support it)
- [ ] Keyboard accessibility (tab navigation, focus rings)
- [ ] Print stylesheet for trade history

## V4: Documentation

### V4.1 API Reference Completion
- [ ] Every endpoint in ApiDocsPage.tsx documented
- [ ] Request/response examples for ALL endpoints
- [ ] Error code reference table
- [ ] WebSocket protocol documentation page
- [ ] Rate limit documentation

### V4.2 Code Documentation
- [ ] README.md update with full setup instructions
- [ ] Contributing guide
- [ ] Architecture diagram (text-based)
- [ ] Module dependency graph

## V5: Performance Validation

### V5.1 Frontend Performance
- [ ] Lighthouse audit: aim for 90+ performance score
- [ ] Bundle size check: `npm run build && ls -la dist/`
- [ ] No unnecessary re-renders (React DevTools profiler)
- [ ] Orderbook: measure render time at 25 levels, 10 updates/sec

### V5.2 Backend Load Test
- [ ] Run `cargo run -p exchange-load-test -- --users 10 --duration 60 --rate 10`
- [ ] Record baseline: p50/p95/p99 latency, error rate
- [ ] Identify top 3 bottlenecks
- [ ] Fix and re-measure

## Priority Order

V1 → V2 → V3 → V4 → V5
(validation → flows → polish → docs → performance)
