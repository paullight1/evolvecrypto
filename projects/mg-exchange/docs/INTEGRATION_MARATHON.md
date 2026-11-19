# Integration Marathon — Wire Everything Together

82 issues found in frontend audit. This marathon closes every gap.

## Wave 1: Missing Backend Endpoints (CRITICAL — frontend is decoration without these)

### W1.1 Admin Endpoints (8 endpoints)
- [ ] GET /admin/users — paginated user list with search
- [ ] GET /admin/users/{id}/balances — user's balances
- [ ] GET /admin/users/{id}/orders — user's order history
- [ ] GET /admin/users/{id}/positions — user's open positions
- [ ] GET /admin/kyc/pending — pending KYC reviews
- [ ] POST /admin/kyc/{user_id}/approve — approve KYC
- [ ] POST /admin/kyc/{user_id}/reject — reject KYC with reason
- [ ] GET /admin/withdrawals/pending — pending withdrawal approvals
- [ ] POST /admin/withdrawals/{id}/approve — approve withdrawal
- [ ] POST /admin/withdrawals/{id}/reject — reject withdrawal
- [ ] GET /admin/aml/alerts — unreviewed AML alerts
- [ ] POST /admin/aml/alerts/{id}/review — mark reviewed with notes
- [ ] POST /admin/surveillance/action — freeze/cancel/flag user

### W1.2 Wallet & Account Endpoints (8 endpoints)
- [ ] GET /api/v1/account/wallet — portfolio overview (equity, per-asset breakdown)
- [ ] GET /api/v1/account/wallet/equity-curve — equity history (7d/30d/90d)
- [ ] GET /api/v1/account/address-book — whitelisted addresses
- [ ] POST /api/v1/account/address-book — add address (24h cooldown)
- [ ] DELETE /api/v1/account/address-book/{id} — remove address
- [ ] GET /api/v1/account/pnl — PnL analysis (daily breakdown, per-symbol)
- [ ] GET /api/v1/fees — fee schedule (tiers, withdrawal fees, funding)
- [ ] GET /api/v1/status — system health for users

### W1.3 Copy Trading Endpoints (5 endpoints)
- [ ] GET /api/v1/copy-trading/leaderboard — top traders with stats
- [ ] POST /api/v1/copy-trading/follow — follow a master trader
- [ ] DELETE /api/v1/copy-trading/follow/{id} — unfollow
- [ ] GET /api/v1/copy-trading/following — my followed traders
- [ ] GET /api/v1/copy-trading/masters/{id} — master profile + positions

### W1.4 Trading Bot Endpoints (6 endpoints)
- [ ] POST /api/v1/bots/grid — create grid bot
- [ ] POST /api/v1/bots/dca — create DCA bot
- [ ] GET /api/v1/bots — list my bots
- [ ] POST /api/v1/bots/{id}/pause — pause bot
- [ ] POST /api/v1/bots/{id}/resume — resume bot
- [ ] POST /api/v1/bots/{id}/stop — stop and close bot

### W1.5 Settings & Referral Endpoints (5 endpoints)
- [ ] GET /api/v1/account/settings — user preferences
- [ ] PUT /api/v1/account/settings — update preferences
- [ ] GET /api/v1/referral — my code + stats + commissions
- [ ] POST /api/v1/referral/invite — send invite email
- [ ] GET /api/v1/markets/all — all pairs with ticker data (for landing/markets page)

## Wave 2: Frontend → Real API (Remove all mocks)

### W2.1 Replace Mock Data with API Calls
- [ ] WalletPage: fetch real balances, compute equity from API
- [ ] CopyTradingPage: fetch leaderboard + following from API
- [ ] BotsPage: fetch bots from API, create via API
- [ ] PnlPage: fetch PnL analysis from API
- [ ] FeesPage: fetch fee schedule from API
- [ ] StatusPage: fetch system status from API
- [ ] ReferralPage: fetch stats + commissions from API
- [ ] AdminPage: all 6 tabs use real endpoints
- [ ] DepositPage: real QR codes (add qrcode library)
- [ ] KycPage: wire real start + webhook polling

### W2.2 Remove MOCK_* Arrays
- [ ] Remove all MOCK_* constants from pages
- [ ] Replace 404 fallbacks with proper error states ("Service unavailable")
- [ ] Remove mock data generation functions

## Wave 3: WebSocket Real-Time Updates

### W3.1 Connect Stores to WebSocket
- [ ] Market store: subscribe to ticker.{symbol}, update on every tick
- [ ] OrderBook store: subscribe to orderbook.{symbol}, apply deltas
- [ ] Trading store: get best bid/ask from orderbook for market order estimation
- [ ] Account store: subscribe to wallet balance changes

### W3.2 Real-Time Page Updates
- [ ] Positions: subscribe to position.{symbol} for live PnL
- [ ] Open Orders: subscribe to orders topic for status changes
- [ ] Trade History: subscribe to fills for new trades
- [ ] Bots: subscribe for bot status/PnL updates
- [ ] Copy Trading: subscribe for followed trader trade events
- [ ] Admin Surveillance: subscribe to admin.surveillance for alerts

### W3.3 WebSocket Enhancements
- [ ] Reconnection overlay ("Reconnecting..." with countdown)
- [ ] Sequence gap detection → auto resubscribe
- [ ] Web Worker for orderbook delta processing (off main thread)

## Wave 4: Missing Integrations

### W4.1 QR Code
- [ ] Add qrcode.react or canvas QR library
- [ ] Generate real QR from deposit address
- [ ] Replace placeholder canvas grid

### W4.2 Sumsub WebSDK
- [ ] Add @sumsub/websdk-react
- [ ] Replace placeholder div with actual iframe
- [ ] Handle verification callbacks

### W4.3 PnL Share Card (Canvas)
- [ ] Test canvas rendering on different screen sizes
- [ ] Add exchange logo as base64 embedded image
- [ ] Ensure Web Share API fallback works

### W4.4 Network Configs from Backend
- [ ] GET /api/v1/networks — supported networks per asset
- [ ] Move ASSET_NETWORKS from frontend hardcode to API response
- [ ] Withdrawal fees from API, not hardcoded

## Wave 5: Polish & Quality

### W5.1 Error States
- [ ] Every page: proper error boundary (crash one panel, not whole app)
- [ ] API error: show retry button, not silent failure
- [ ] Network offline: global banner "No connection"
- [ ] Session expired: redirect to login with message

### W5.2 Loading States
- [ ] Skeleton shimmer on EVERY data-dependent component
- [ ] Consistent loading pattern (not mix of spinners/skeletons/empty)

### W5.3 Empty States
- [ ] No positions: "Open your first position" CTA
- [ ] No orders: "Place your first order"
- [ ] No deposits: "Fund your account"
- [ ] No KYC: banner "Complete verification for full limits"
- [ ] No bots: "Create your first bot"
- [ ] No referrals: "Share your code to start earning"

### W5.4 Consistency Pass
- [ ] All buttons same height/padding/border-radius
- [ ] All tables same header style
- [ ] All badges same size/shape
- [ ] All modals same backdrop/animation
- [ ] All forms same input style
- [ ] All status colors consistent (green/yellow/red)
- [ ] Monospace for ALL numbers everywhere

### W5.5 Performance
- [ ] React.memo on OrderBook rows, Position rows
- [ ] useMemo for computed values
- [ ] useCallback for handlers
- [ ] Lazy load all pages (React.lazy + Suspense)
- [ ] Bundle size < 200KB gzipped initial load

## Estimated Effort

| Wave | Tasks | Days |
|------|-------|------|
| W1: Backend Endpoints | 32 endpoints | 5-7 |
| W2: Remove Mocks | 12 pages | 2-3 |
| W3: WebSocket Wiring | 12 subscriptions | 3-4 |
| W4: Integrations | 4 items | 2-3 |
| W5: Polish | 20+ items | 3-5 |
| **Total** | | **15-22** |
