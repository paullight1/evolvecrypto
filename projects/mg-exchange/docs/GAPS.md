# Exchange Gaps — Everything That Needs To Be Done

Honest assessment of what's built vs what's needed for production.

## Status: PROTOTYPE — Not Production Ready

We have ~25 crates with 925 unit tests, but most are standalone modules
with shallow integration. This document tracks every gap.

---

## P0 — Architecture (must fix before anything else)

### A1. Matching Engine Scalability
- **Problem**: Single `Mutex<MatchingEngine>` — one thread for ALL symbols
- **Fix**: Dedicated thread per symbol with SPSC channel (as designed in ARCHITECTURE.md)
- **Impact**: Without this, >100 orders/sec will cause Mutex contention
- [ ] Move matching engine to dedicated thread per symbol
- [ ] SPSC ring buffer for command input
- [ ] SPMC ring buffer for event output
- [ ] Remove Mutex, use channel-based communication

### A2. PostgreSQL Actually Used
- **Problem**: DB repos exist but Exchange coordinator uses in-memory HashMap
- **Fix**: Replace in-memory stores with DB-backed implementations
- [ ] AccountManager → PostgreSQL (balance_repo with SELECT FOR UPDATE)
- [ ] Order history → PostgreSQL (order_repo)
- [ ] Trade history → PostgreSQL (trade_repo)
- [ ] KYC records → PostgreSQL
- [ ] AML alerts → PostgreSQL
- [ ] Audit log → PostgreSQL (immutable)
- [ ] User records → PostgreSQL
- [ ] API keys → PostgreSQL
- [ ] Deposits/Withdrawals → PostgreSQL

### A3. WAL Recovery on Startup
- **Problem**: WAL writes commands but startup doesn't replay them
- **Fix**: On server start, replay WAL into fresh matching engine
- [ ] Read WAL entries on startup
- [ ] Rebuild matching engine state from WAL
- [ ] Rebuild position state from WAL
- [ ] Handle partial writes / corruption
- [ ] Snapshot + WAL compaction

### A4. Error Handling
- **Problem**: Many `.unwrap()` calls that will panic in production
- **Fix**: Replace all `.unwrap()` with proper error handling
- [ ] Audit ALL `.unwrap()` in non-test code
- [ ] Replace with `?` operator or proper error returns
- [ ] Add error context with `anyhow::Context`
- [ ] Log errors with tracing, don't panic

---

## P1 — API Completeness

### API Documentation
- [ ] OpenAPI/Swagger spec for ALL REST endpoints
- [ ] Auto-generate from code (utoipa crate)
- [ ] WebSocket protocol documentation
- [ ] FIX protocol documentation
- [ ] SDK examples (curl, Python, JavaScript)
- [ ] Rate limit documentation per endpoint
- [ ] Error code reference

### Missing REST Endpoints
- [ ] GET /api/v1/orders — list OPEN orders (not just history)
- [ ] GET /api/v1/orders/{id} — get single order by ID
- [ ] PUT /api/v1/orders/{id} — amend order (price/qty change)
- [ ] GET /api/v1/account/deposits — deposit history
- [ ] GET /api/v1/account/withdrawals — withdrawal history
- [ ] POST /api/v1/account/withdraw — initiate withdrawal
- [ ] POST /api/v1/account/deposit-address — get/create deposit address
- [ ] GET /api/v1/markets — list all trading pairs
- [ ] GET /api/v1/markets/{symbol}/ticker — 24h stats
- [ ] GET /api/v1/markets/{symbol}/klines — candlestick data
- [ ] GET /api/v1/markets/{symbol}/trades — recent trades (exists but verify)
- [ ] POST /api/v1/trigger-orders — place stop/TP/trailing
- [ ] GET /api/v1/trigger-orders — list trigger orders
- [ ] DELETE /api/v1/trigger-orders/{id} — cancel trigger
- [ ] POST /api/v1/orders/batch — batch place orders (for MMs)
- [ ] DELETE /api/v1/orders/batch — batch cancel
- [ ] GET /api/v1/copy-trading/masters — leaderboard
- [ ] POST /api/v1/copy-trading/follow — follow master
- [ ] GET /api/v1/bots — list user's bots
- [ ] POST /api/v1/bots/grid — create grid bot
- [ ] POST /api/v1/bots/dca — create DCA bot
- [ ] GET /api/v1/por/verify — proof of reserves verification
- [ ] POST /api/v1/account/2fa/setup — setup 2FA
- [ ] POST /api/v1/account/2fa/verify — verify 2FA code
- [ ] GET /api/v1/account/api-keys — list API keys
- [ ] POST /api/v1/account/api-keys — create API key
- [ ] DELETE /api/v1/account/api-keys/{id} — revoke API key

### Missing WebSocket Topics
- [ ] kline.{interval}.{symbol} — candlestick stream
- [ ] ticker.{symbol} — 24h stats stream
- [ ] liquidation.{symbol} — already exists, verify working
- [ ] funding.{symbol} — already exists, verify working
- [ ] position.{symbol} — already exists, verify working
- [ ] wallet — balance changes stream
- [ ] triggerOrder — trigger order updates
- [ ] copyTrade — copy trade notifications

### WebSocket Protocol Issues
- [ ] L2 orderbook: send snapshot on subscribe, then incremental deltas
- [ ] Sequence numbers for gap detection
- [ ] Server-side heartbeat (not just client ping)
- [ ] Connection limit per user
- [ ] Message rate limit per connection
- [ ] Binary message support (MessagePack/Protobuf for low latency)

---

## P2 — Business Logic Completeness

### Futures Settlement Chain (BROKEN)
- [ ] Fill → position update → margin lock from balance
- [ ] Position close → realized PnL → credit/debit balance
- [ ] Funding payment → credit/debit balance (not just tracked)
- [ ] Liquidation → market order → fill → position close → insurance fund
- [ ] ADL → force close profitable positions → balance adjustment
- [ ] Cross-margin: unrealized PnL affects available margin for other positions

### Fee Handling
- [ ] Futures fees: deduct from margin, not from received asset
- [ ] Funding fees: deduct from/credit to margin or balance
- [ ] Maker rebate: actually credit to balance
- [ ] Fee revenue tracking: exchange's own revenue account

### Order Lifecycle Gaps
- [ ] Order amendment (modify price/qty without losing queue position)
- [ ] Good-till-date expiration
- [ ] Reduce-only enforcement in matching flow (validated but not enforced)
- [ ] Close-on-trigger wiring (trigger service → position close)
- [ ] OCO/Bracket wiring into exchange coordinator
- [ ] Iceberg refill wiring into matching engine

### Market Data
- [ ] Candlestick (OHLCV) aggregation from trades
- [ ] 24h ticker calculation (high, low, volume, change%)
- [ ] Funding rate history endpoint
- [ ] Mark price history
- [ ] Open interest tracking

---

## P3 — Infrastructure

### External Connections
- [ ] WebSocket client to Binance for BTC/ETH/SOL spot prices
- [ ] WebSocket client to Coinbase
- [ ] WebSocket client to Kraken
- [ ] WebSocket client to OKX
- [ ] Reconnection logic with backoff
- [ ] Health monitoring for each connection

### Email/SMS
- [ ] Email service integration (SendGrid/SES)
- [ ] Email templates: verification, login, withdrawal, liquidation warning
- [ ] SMS provider integration (Twilio) for critical alerts
- [ ] Push notification service (for mobile, future)

### Monitoring & Alerting
- [ ] PagerDuty/OpsGenie integration for critical alerts
- [ ] Grafana dashboard templates (not just Prometheus endpoint)
- [ ] Matching engine latency dashboard
- [ ] Order book health monitoring
- [ ] Balance reconciliation alerting
- [ ] Error rate monitoring

### Security Infrastructure
- [ ] Rate limiter actually connected to router middleware
- [ ] 2FA enforcement in REST handlers
- [ ] API key permission checks in handlers
- [ ] IP whitelist checks in handlers
- [ ] Address whitelist enforcement in withdrawal flow
- [ ] CORS properly configured (not permissive)
- [ ] Request body size limits
- [ ] SQL injection prevention (parameterized queries — sqlx does this)
- [ ] Input sanitization on all user input

---

## P4 — Code Quality

### Documentation
- [ ] Module-level doc comments on every crate
- [ ] Public API doc comments on every public function
- [ ] Architecture decision records (ADRs) for key decisions
- [ ] API reference (auto-generated from OpenAPI spec)
- [ ] Developer onboarding guide
- [ ] Deployment runbook

### Testing
- [ ] Integration tests (multi-crate, full flow)
- [ ] E2E tests (HTTP → match → settle → WS notification)
- [ ] Load tests (criterion benchmarks exist, but no sustained load test)
- [ ] Chaos tests (kill process mid-operation, verify WAL recovery)
- [ ] Concurrency tests (multiple simultaneous orders, race conditions)
- [ ] Fuzzing (matching engine inputs, FIX parser inputs)
- [ ] Property-based tests (proptest for balance invariants)

### Code Cleanup
- [ ] Audit and remove all `.unwrap()` in non-test code
- [ ] Run `cargo clippy -- -D warnings` and fix all warnings
- [ ] Run `cargo fmt` across entire workspace
- [ ] Remove dead code and unused imports
- [ ] Consistent error types across crates

---

## P5 — Frontend

### Trading Terminal
- [ ] TradingView charting library integration
- [ ] Advanced order form (TP/SL, OCO, bracket)
- [ ] Position management panel (close, add margin, set TP/SL)
- [ ] Open orders panel with cancel buttons
- [ ] Order history with filters
- [ ] Trade history with P&L
- [ ] Funding history
- [ ] Portfolio overview (equity, margin, PnL)

### Account Management
- [ ] KYC verification flow (upload documents, status tracking)
- [ ] 2FA setup page (QR code display, recovery codes)
- [ ] API key management page
- [ ] Security settings (password change, device management)
- [ ] Deposit/withdrawal pages
- [ ] Address whitelist management
- [ ] Sub-account management

### Admin Panel
- [ ] User management (search, freeze, unfreeze)
- [ ] Surveillance alerts dashboard
- [ ] KYC review queue
- [ ] AML alerts queue
- [ ] Withdrawal approval queue
- [ ] System health dashboard
- [ ] Reconciliation reports
- [ ] Trading pair management

---

## Priority Order

1. **P0**: Architecture fixes (Mutex → channels, DB integration, WAL recovery)
2. **P2**: Futures settlement chain (the business logic must be correct)
3. **P4**: Error handling audit (no panics in production)
4. **P1**: API completeness (REST + WS + documentation)
5. **P3**: Infrastructure (external connections, email, monitoring)
6. **P4**: Testing (integration, E2E, load, chaos)
7. **P5**: Frontend improvements

Estimated total: 6-12 months of focused development for production readiness.
