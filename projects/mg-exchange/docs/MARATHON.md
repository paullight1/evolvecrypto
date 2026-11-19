# Exchange Marathon Roadmap — 12 Months to Production

## Honest Current State Assessment

### What Actually Works (as of March 2026)

The prototype has 22 crates and ~925 unit tests, all passing. The tests are genuine — matching logic, balance arithmetic, margin calculations, AML detection, KYC state machines. But every crate operates in isolation. There is no end-to-end flow that touches PostgreSQL, WebSocket, and the matching engine in a single operation.

**The integration gap is the central problem.** `market-data` is literally one line: `//! Exchange market-data — TODO: implement`. The `AccountManager` is a `HashMap` wrapped in a `Mutex`. DB repos exist in `crates/db/` but are only called in `main.rs` for a seed operation — they are not wired into any business logic path. The server starts, accepts REST requests, matches orders, but restarts wipe all data.

**The WAL exists but recovery does not.** The journal appends commands but startup never replays them. If the process crashes, the orderbook is gone.

**The `Exchange` god struct** in `crates/api-gateway/src/exchange.rs` is 15+ `Mutex<T>` fields sharing one process and one lock domain. Every operation contends. A market maker placing 500 orders/sec will serialize against the liquidation monitor, which serializes against the surveillance engine.

**Crates that are stubs:**
- `crates/market-data/` — one TODO comment
- `crates/fix-gateway/` — parser + session exist, no TCP listener, no order routing
- `crates/copy-trading/` — full logic, zero integration into Exchange
- `crates/trading-bots/` — full logic, zero integration
- `crates/algo-execution/` — TWAP/VWAP logic, zero integration
- `crates/proof-of-reserves/` — Merkle tree logic, no scheduled job, no endpoint
- `crates/index-price/` — aggregator exists, mock feed only, no real WebSocket client to Binance/Coinbase/Kraken

**Security holes that would be P0 on day one:**
- Rate limiter exists in `rate_limit.rs` but is not wired into the axum router
- 2FA (TOTP) logic is in `compliance/src/totp.rs` but no endpoint, not enforced anywhere
- API key permissions (`can_read`, `can_trade`, `can_withdraw`) are defined but not checked in handlers
- IP whitelist defined but not enforced
- CORS is likely permissive (not audited)

**What can be demoed today:** Place a limit order, have it fill against another order, see the balance change, subscribe to WebSocket and see the trade event. That is the demo. Everything else requires the DB and integration work described below.

---

## Month 1: Persistence Foundation

**Theme:** Make the exchange stateful. Data must survive restarts.

### Goals
- PostgreSQL is the single source of truth for all account state
- AccountManager, KycManager, order history, trade history all backed by DB
- WAL replay rebuilds matching engine state on startup
- Zero in-memory-only data paths for anything that matters

### Research (before coding)
- Study `sqlx::query!` compile-time checking workflow — understand `sqlx prepare` and offline mode, required for CI without a live DB
- Study PostgreSQL `SELECT FOR UPDATE SKIP LOCKED` for withdrawal queue and approval flows — this is the pattern for safe concurrent balance mutation
- Study PostgreSQL advisory locks vs row-level locks for balance operations — decide which to use for the `lock_balance` path
- Read PostgreSQL isolation level documentation — confirm which isolation level `sqlx` defaults to and whether REPEATABLE READ is needed for balance operations

### Tasks

#### T1.1 — Wire AccountManager to PostgreSQL (effort: 3d, risk: H, deps: none)
**File:** `crates/account/src/account_manager.rs`, `crates/db/src/balance_repo.rs`

The current `AccountManager` is a `HashMap<UserId, HashMap<Asset, Balance>>`. Replace the storage layer with `BalanceRepo`. The interface stays identical — callers (`exchange.rs`) do not change.

- Add `async fn deposit(pool, user_id, asset, amount)` to `BalanceRepo` using `UPDATE balances SET total = total + $3 WHERE ... RETURNING *` — never read then write, always atomic UPDATE
- Add `async fn lock(pool, user_id, asset, amount)` with `SELECT FOR UPDATE` to prevent double-spend
- Add `async fn unlock(pool, user_id, asset, amount)`
- Add `async fn settle(pool, user_id, base_asset, quote_asset, base_delta, quote_delta, fee)` as a single transaction
- Add `async fn get_balance(pool, user_id, asset) -> Balance`
- Write 5 integration tests using a test PgPool (sqlx `PgPool::connect` with a `TEST_DATABASE_URL` env var): deposit, lock exceeds balance returns error, lock + settle invariant holds, concurrent lock attempts (two tasks, one must fail)
- Acceptance: `cargo test -p exchange-db` passes; `cargo test -p exchange-account` passes; `SELECT total - locked FROM balances` never goes negative during concurrent test

#### T1.2 — Wire KycManager, AmlEngine, AuditLog to PostgreSQL (effort: 2d, risk: M, deps: T1.1)
**Files:** `crates/compliance/src/kyc.rs`, `crates/compliance/src/aml.rs`, `crates/db/`

- Add `kyc_repo.rs` to `crates/db/` with upsert, status fetch, list-pending
- Add `aml_repo.rs` to `crates/db/` with insert alert, mark reviewed
- Add `audit_repo.rs` to `crates/db/` with append-only insert (no UPDATE, no DELETE — enforce via Postgres role `REVOKE UPDATE, DELETE ON audit_log FROM exchange_app`)
- Wire `KycManager::set_status` to call `kyc_repo::upsert` before updating in-memory state
- Wire `AmlEngine::flag` to call `aml_repo::insert`
- Every balance mutation calls `audit_repo::append` with action, user_id, amount
- Acceptance: KYC status change survives server restart; AML alert persisted and queryable via GET /admin/aml-alerts

#### T1.3 — Order and Trade Persistence (effort: 2d, risk: H, deps: T1.1)
**Files:** `crates/db/src/order_repo.rs`, `crates/db/src/trade_repo.rs`, `crates/api-gateway/src/exchange.rs`

Current `order_history` and `trade_history` are in-memory Vecs/HashMaps in the Exchange struct. Replace with DB writes.

- `order_repo::insert(pool, order)` called when order is accepted
- `order_repo::update_status(pool, order_id, status, filled_qty)` called on each fill event and cancel
- `trade_repo::insert(pool, trade)` called on each fill
- Remove `order_history: Mutex<Vec<OrderHistoryEntry>>` and `trade_history: Mutex<HashMap<...>>` from Exchange struct (big win: fewer Mutex fields)
- Wire `GET /api/v1/orders/history` to query `order_repo` instead of in-memory Vec
- Acceptance: place 10 orders, restart server, `GET /api/v1/orders/history` returns all 10

#### T1.4 — WAL Replay on Startup (effort: 3d, risk: H, deps: T1.3)
**Files:** `crates/journal/src/wal.rs`, `crates/server/src/main.rs`

The WAL appends `PlaceOrderCommand` and `CancelOrderCommand` as bincode-encoded bytes. On startup, read from position 0, deserialize each entry, and replay into a fresh MatchingEngine. Skip commands that produced orders already in terminal state in the DB (filled, cancelled — check order_repo before replaying).

- Add `WriteAheadLog::replay(path) -> Result<Vec<Command>>` that reads all entries from a WAL file, handles EOF gracefully, skips corrupted trailing bytes (partial write on crash)
- In `main.rs`: replay WAL → for each command → check if order_id exists in DB with terminal status → if not, replay into engine
- Add WAL segment rotation: when WAL exceeds 1GB or 1M entries, start new segment, keep last 3 segments
- Chaos test: write 100 orders, kill -9 the process, restart, verify orderbook has correct state
- Acceptance: process kill + restart preserves open orders in matching engine; `cargo test -p exchange-journal` includes a crash recovery test

#### T1.5 — API Key and User Records in PostgreSQL (effort: 1d, risk: L, deps: T1.1)
**Files:** `crates/api-gateway/src/auth.rs`, `crates/db/`

Current `ApiKeyStore` is in-memory. Replace with `api_key_repo` that reads from the `api_keys` table.

- Add `api_key_repo::find(pool, key) -> Option<ApiKeyRecord>` with Redis cache layer (hash map + TTL 60s, invalidate on revoke)
- User creation creates a DB record in `users` table and seeds default balances
- Acceptance: create API key, restart server, authenticate with that key successfully

### Definition of Done — Month 1
- [ ] `cargo test` passes with `TEST_DATABASE_URL` set (integration tests run in CI)
- [ ] Server restart preserves: account balances, order history, KYC status, open orders (via WAL replay)
- [ ] `SELECT SUM(total) FROM balances WHERE asset = 'USDT'` equals sum of all deposits minus withdrawals
- [ ] No in-memory-only state for any financial data
- [ ] `cargo clippy -- -D warnings` passes

---

## Month 2: Architecture Integrity

**Theme:** Fix the Mutex contention problem and the broken business logic chains.

### Goals
- Matching engine runs on a dedicated thread per symbol with SPSC channels
- Settlement chain is correct: fill → position update → margin lock → balance
- Rate limiter and circuit breaker are actually active
- Liquidation trigger actually executes an order (currently only logs a warning)

### Research (before coding)
- Study `crossbeam_channel` SPSC vs `tokio::sync::mpsc` — understand the tradeoff (crossbeam is lock-free and works outside tokio, mpsc requires tokio runtime). The matching engine must stay synchronous, so crossbeam is correct.
- Study how production exchanges handle the "liquidation injects an order" problem without creating a feedback loop — the liquidation engine must go through the order gateway, not directly into the matching engine
- Study `crossbeam_channel::bounded` capacity tuning — too small causes back-pressure, too large causes memory spikes. Research what gitbitex uses.

### Tasks

#### T2.1 — Matching Engine Per-Symbol Thread (effort: 4d, risk: H, deps: T1.3)
**Files:** `crates/matching-engine/src/engine.rs`, `crates/api-gateway/src/exchange.rs`

This is the most structurally important change. Currently: one `Mutex<MatchingEngine>` holds ALL symbols. The matching engine must become one thread per symbol.

- Create `EngineThread` struct: holds a `crossbeam_channel::Sender<EngineCommand>` and a `crossbeam_channel::Receiver<EngineEvent>` pair
- `EngineCommand` enum: `PlaceOrder(PlaceOrderCommand)`, `CancelOrder(CancelOrderCommand)`, `Shutdown`
- `EngineEvent` enum: wraps existing `Vec<MatchEvent>` response plus request correlation ID
- In `Exchange::new()`, spawn one OS thread per registered symbol via `std::thread::spawn`. The thread owns the `MatchingEngine` for that symbol. It loops on `recv()`, processes the command, sends `EngineEvent` back.
- Replace `exchange.engine: Mutex<MatchingEngine>` with `exchange.engines: HashMap<Symbol, EngineThread>`
- The Exchange coordinator sends commands to the right thread and awaits the response synchronously (using a `oneshot`-style response channel)
- Run existing matching engine tests through the new thread interface
- Benchmark before and after with `cargo bench -p exchange-matching-engine` — expect no regression (thread overhead is negligible at < 100K ops/sec)
- Acceptance: 3 symbols each get their own thread; placing BTCUSDT order does not block ETHUSDT matching; `cargo test` passes

#### T2.2 — Futures Settlement Chain (effort: 3d, risk: H, deps: T2.1)
**Files:** `crates/api-gateway/src/exchange.rs`, `crates/settlement/src/pnl.rs`, `crates/risk-engine/src/position_manager.rs`

Currently: a futures fill happens, the trade is recorded, but position is not updated, margin is not adjusted, and balance does not change. This is broken.

The correct chain: Fill event → update Position (size, avg_entry, unrealized_pnl) → if position closed: compute realized_pnl, release margin, credit/debit balance → write all to DB atomically.

- In `exchange.rs`, after processing a futures fill event, call `PositionManager::apply_fill(user_id, symbol, fill)`
- `apply_fill` updates the position and returns `Option<RealizedPnl>` (Some when position closes or reduces)
- If `RealizedPnl` is Some, call `AccountManager::settle_futures(user_id, realized_pnl)` which adjusts the margin balance
- Write a test: open long 1 BTC at 50000, price moves to 51000, close — verify realized_pnl = +1000 USDT credited to balance
- Write a test: open long 1 BTC at 50000 with 10x leverage, price moves to 45000 (below liquidation threshold), verify position is flagged for liquidation
- Acceptance: 5 settlement chain tests pass; balance is never modified without a corresponding trade record

#### T2.3 — Liquidation Actually Executes (effort: 2d, risk: H, deps: T2.2)
**Files:** `crates/api-gateway/src/tasks.rs`, `crates/risk-engine/src/liquidation.rs`

Currently: `spawn_liquidation_monitor` detects a liquidatable position, logs `tracing::warn!`, broadcasts a `MarketEvent::Liquidation` — and stops there. The position is never actually closed.

- `LiquidationBatch::orders` contains `LiquidationOrder` structs. Send each through the order gateway as a market order (the liquidation engine is already coded for this, just not wired).
- Use the same `exchange.place_order()` path, tagged with a special `user_id` representing the liquidation engine, so the balance unlock and settlement chain fires correctly.
- Handle the case where the insurance fund must cover a deficit: call `InsuranceFund::cover_deficit` and record the outcome.
- If the insurance fund is exhausted, record `socialised_loss` and emit an admin alert.
- Write test: position at liquidation threshold → liquidation monitor tick → position is closed, balance updated, insurance fund correct.
- Acceptance: end-to-end liquidation test passes; no position stays open past its liquidation price for more than one liquidation monitor tick

#### T2.4 — Rate Limiter Wired Into Router (effort: 1d, risk: M, deps: none)
**Files:** `crates/api-gateway/src/server.rs`, `crates/api-gateway/src/rate_limit.rs`

The rate limiter struct exists but is not applied as axum middleware. This is a one-day wiring task but it is a security prerequisite.

- Add `tower_governor` or use the existing `rate_limit.rs` implementation as a `tower::Layer`
- Apply per-IP rate limit on all public endpoints
- Apply per-API-key rate limit on authenticated endpoints (1000 req/min for regular, 5000 for VIP)
- Return HTTP 429 with `Retry-After` header when limit exceeded
- Acceptance: `curl` loop exceeding limit gets 429; legitimate traffic is unaffected

#### T2.5 — Funding Rate Settlement Actually Credits Balances (effort: 2d, risk: H, deps: T2.2)
**Files:** `crates/settlement/src/funding.rs`, `crates/settlement/src/scheduler.rs`, `crates/api-gateway/src/tasks.rs`

The funding scheduler fires every 8 hours. The funding rate is calculated. But `FundingSettlement::apply` is never called, so no user balance is ever debited or credited.

- In `spawn_funding_settlement_task`, after calculating the funding rate, call `FundingSettlement::apply(positions, funding_rate)` which iterates all open positions and computes each user's payment/receipt
- For each position: call `AccountManager::adjust_margin(user_id, symbol, delta)` with positive (receive) or negative (pay) delta
- Write the payment to `funding_history` in DB
- Emit `PrivateEvent::FundingPayment` to the user's WebSocket
- Acceptance: 3 users with open positions before an 8h funding tick all have correct balance adjustments afterwards

### Definition of Done — Month 2
- [ ] One thread per symbol, no global matching engine Mutex
- [ ] Futures fill → position update → balance change is one atomic operation in tests
- [ ] Liquidation closes the position, not just logs it
- [ ] Rate limiter returns 429 (verify with automated test)
- [ ] Funding payment actually debits/credits balances
- [ ] `cargo test` passes; `cargo bench` shows no regression vs Month 1 baseline

---

## Month 3: Security Hardening and Error Elimination

**Theme:** No panics. No unenforced permissions. No open CORS. This month is about correctness and security, not features.

### Goals
- Zero `.unwrap()` in non-test production code paths
- 2FA enforced on sensitive operations
- API key permissions checked on every request
- All user inputs sanitized
- CORS locked down

### Research (before coding)
- Study TOTP spec (RFC 6238) — understand time step, drift tolerance, secret storage requirements
- Study HMAC-SHA256 signature verification for API keys — understand the timestamp window (typically ±5 seconds), replay prevention via nonce
- Study JWT security pitfalls — `alg: none` attack, key confusion attacks. Decide: use JWT for web sessions or stick to HMAC-SHA256 for everything.
- Research Rust's `zeroize` crate — understand why cryptographic secrets must be zeroed on drop, apply to TOTP secrets and API key secrets

#### T3.1 — Audit and Eliminate All `.unwrap()` (effort: 3d, risk: M, deps: none)
**Files:** 60 files identified by grep (see GAPS.md A4)

Not all `unwrap()` are equal. Triage:

- `Mutex::lock().unwrap()` — acceptable only if the Mutex can never be poisoned. Replace with `lock().unwrap_or_else(|e| e.into_inner())` or restructure to avoid holding locks across `.await` points.
- `HashMap::get().unwrap()` on data that was just inserted — these should use `expect()` with a meaningful message minimum, or be restructured to return `Result`.
- `Option::unwrap()` on external data (user input, DB results, API responses) — must become `?` or explicit error.
- Critical files first: `crates/matching-engine/src/orderbook.rs`, `crates/matching-engine/src/engine.rs`, `crates/journal/src/wal.rs`, `crates/api-gateway/src/exchange.rs`
- Write a CI check: `cargo clippy -- -W clippy::unwrap_used` added to CI workflow. This will catch regressions.
- Acceptance: `cargo clippy -- -W clippy::unwrap_used` passes on all non-test code (use `#[cfg(not(test))]` exclusion for test helpers)

#### T3.2 — TOTP 2FA: Enforcement (effort: 2d, risk: M, deps: T1.5)
**Files:** `crates/compliance/src/totp.rs`, `crates/api-gateway/src/rest.rs`, `crates/api-gateway/src/auth.rs`

TOTP logic is written but enforced nowhere.

- Add `POST /api/v1/account/2fa/setup` — generates secret, returns QR code URI
- Add `POST /api/v1/account/2fa/enable` — verifies first TOTP code, stores encrypted secret in DB
- Add `POST /api/v1/account/2fa/disable` — requires 2FA code to disable
- Enforce 2FA check in: withdrawal handler, API key creation handler, login (if web sessions added later)
- Encrypt TOTP secret at rest using AES-256-GCM with a key from env var `TOTP_ENCRYPTION_KEY`
- Store 10 single-use recovery codes (bcrypt hashed) per user in a `totp_recovery_codes` table
- Acceptance: user with 2FA enabled cannot withdraw without providing a valid TOTP code; invalid codes return 403

#### T3.3 — API Key Permission Enforcement (effort: 1d, risk: H, deps: T1.5)
**Files:** `crates/api-gateway/src/auth.rs`, all handler files

The `ApiKeyPermissions` struct exists (`can_read`, `can_trade`, `can_withdraw`, `can_transfer`) but handlers never check it.

- Add permission check to `AuthenticatedUser` extractor: for write endpoints, verify `can_trade`; for withdrawal endpoints, verify `can_withdraw`
- Add IP whitelist check: if `allowed_ips` is non-empty, reject requests from IPs not in the list (extract from `ConnectInfo<SocketAddr>`)
- Add endpoints: `POST /api/v1/account/api-keys`, `GET /api/v1/account/api-keys`, `DELETE /api/v1/account/api-keys/{id}`
- Acceptance: API key with `can_read` only returns 403 on order placement; IP whitelist rejects correct IPs

#### T3.4 — CORS, Input Validation, Body Size Limits (effort: 1d, risk: M, deps: none)
**Files:** `crates/api-gateway/src/server.rs`

- Set CORS to explicit allowlist: configured via `ALLOWED_ORIGINS` env var, defaulting to `["https://exchange.ua"]` in production
- Add `tower_http::limit::RequestBodyLimitLayer` — max 64KB request body
- Validate all string inputs: `symbol` must match `[A-Z]{3,10}` regex; `asset` must be in known asset list; numeric fields must be positive
- SQL injection: confirm all queries use `sqlx::query!` with `$1` parameters (not string interpolation) — audit all DB calls
- Acceptance: oversized body returns 413; unknown symbol returns 400 with clear message; CORS preflight from unknown origin returns appropriate headers

#### T3.5 — Address Whitelist Enforcement in Withdrawal Flow (effort: 1d, risk: H, deps: T3.2)
**Files:** `crates/custody/src/address_whitelist.rs`, `crates/api-gateway/src/rest.rs`

- Persist whitelist to PostgreSQL (add `address_whitelist` table to migrations)
- Enforce: withdrawal handler checks whitelist before accepting the request
- New address → insert with `activated_at = NOW() + INTERVAL '24 hours'` — the 24h cooldown
- Endpoint to add address sends email verification before inserting
- Acceptance: withdrawal to non-whitelisted address returns 403; withdrawal to address added < 24h ago returns 403 with time remaining

### Definition of Done — Month 3
- [ ] `cargo clippy -- -W clippy::unwrap_used` passes (excluding test code)
- [ ] `cargo clippy -- -D warnings` passes
- [ ] 2FA setup, enable, and enforcement tested end-to-end
- [ ] API key permissions tested: each permission flag independently verified
- [ ] Withdrawal flow requires whitelisted address + 2FA
- [ ] No new security issues introduced (self-review against OWASP API Top 10)

---

## Month 4: Market Data and WebSocket Completeness

**Theme:** Market data must be real-time, complete, and correctly sequenced. Without this, no trading terminal is possible.

### Goals
- `crates/market-data/` is fully implemented (currently a stub)
- Candlestick (OHLCV) aggregation from trade stream
- 24h ticker with correct rolling window
- WebSocket delivers: trades, L2 snapshot+delta, candles, ticker, private fills
- Index price from real external feeds (not mock)

### Research (before coding)
- Study how major exchanges implement the L2 snapshot + incremental delta protocol: Binance sends a snapshot when you subscribe, then numbered deltas. Gap detection: client checks sequence numbers, re-subscribes if a gap is detected. Implement the same.
- Study candlestick aggregation correctness: the "current" candle is always open and not persisted until closed. When does a 1m candle close? At the exact minute boundary. Tick events within the last 10ms of a minute require careful handling.
- Study Binance/Coinbase WebSocket reconnection — they send a sequence number per message. If you miss one, re-subscribe. Implement exponential backoff with jitter for reconnection.
- Study the mark price formula: `mark_price = median(spot_index, fair_price)` where `fair_price = spot_index * (1 + funding_rate * time_until_settlement / 8h)`. Understand why each component exists.

#### T4.1 — Implement `crates/market-data/` (effort: 3d, risk: M, deps: T2.1)
**Files:** `crates/market-data/src/lib.rs`, add `aggregator.rs`, `candle.rs`, `ticker.rs`

- `MarketDataService` receives `MatchEvent` stream from each matching engine thread
- On each `MatchEvent::Fill`: update the 1m/5m/1h/1d candle aggregators; update the 24h ticker (running high, low, volume, last price, open price 24h ago)
- `CandleAggregator`: BTreeMap of open candles keyed by `(Symbol, Interval, open_time)`. On minute close, finalize and emit a `CandleEvent`.
- `Ticker24h`: sliding window — maintain a `VecDeque<(timestamp, price, qty)>` of trades, drop entries older than 24h, recalculate stats on each update
- Add `GET /api/v1/markets/{symbol}/klines?interval=1m&limit=200` — returns last N candles from the aggregator's buffer (in-memory ring buffer of 1000 candles per interval per symbol; ScyllaDB for deeper history, Month 8)
- Add `GET /api/v1/markets/{symbol}/ticker` — returns 24h stats
- Add `GET /api/v1/markets` — returns all symbols with their tickers
- Acceptance: place 10 trades on BTCUSDT, GET /api/v1/markets/BTCUSDT/ticker shows correct high/low/volume/last

#### T4.2 — WebSocket L2 Snapshot + Delta Protocol (effort: 2d, risk: M, deps: T4.1)
**Files:** `crates/api-gateway/src/ws.rs`

- When client subscribes to `orderbook.{depth}.{symbol}`: immediately send a full snapshot `{"type":"snapshot","bids":[...],"asks":[...],"seq":N}`
- Subsequent book changes send `{"type":"delta","bids":[...],"asks":[...],"seq":N+1}` — levels with qty=0 mean remove
- Sequence number is a monotonically increasing u64 per symbol, incremented on every change
- If a client reconnects, they get a fresh snapshot (the server does not replay deltas)
- Add WebSocket topics: `kline.{interval}.{symbol}`, `ticker.{symbol}`
- Acceptance: WebSocket client subscribes to BTCUSDT orderbook, receives snapshot, then receives deltas after each order placement; no gaps in sequence numbers in automated test

#### T4.3 — Real Index Price Feeds (effort: 3d, risk: M, deps: none)
**Files:** `crates/index-price/src/source.rs`, `crates/index-price/src/mock_feed.rs`

`IndexPriceAggregator` exists with weighted median logic. The only feed is `mock_feed.rs`. Replace with real connections.

- Implement `BinanceSpotFeed`: WebSocket client to `wss://stream.binance.com:9443/ws/btcusdt@miniTicker` — handles reconnect with exponential backoff (1s, 2s, 4s, max 60s), health monitoring (alerts if no update in 30s)
- Implement `CoinbaseFeed`: WebSocket to `wss://advanced-trade-api.coinbase.com/ws` — same pattern
- Implement `KrakenFeed`: WebSocket to `wss://ws.kraken.com` — same pattern
- Register all three in `IndexPriceAggregator`; weighted median with equal weights initially
- Add `GET /api/v1/markets/{symbol}/mark-price` — returns current mark price and index price
- Add health endpoint: `GET /admin/index-price/health` — shows last update time per source and whether any source is stale
- Acceptance: mark price within 0.1% of Binance spot BTC price; if one feed disconnects, aggregator continues with remaining two

#### T4.4 — Funding Rate Endpoint and History (effort: 1d, risk: L, deps: T4.3)
**Files:** `crates/api-gateway/src/rest.rs`, `crates/settlement/src/funding.rs`

- `GET /api/v1/markets/{symbol}/funding-rate` — current rate, next settlement time, 8h interval
- `GET /api/v1/markets/{symbol}/funding-history?limit=100` — historical rates (from `funding_history` Mutex, backed to DB)
- `GET /api/v1/account/funding-history` — user's personal funding payments
- Acceptance: correct funding rate displayed; history shows last 100 settlements

### Definition of Done — Month 4
- [ ] `GET /api/v1/markets` returns all pairs with live ticker data
- [ ] 1m/5m/1h/1d candles accumulate correctly from trades
- [ ] WebSocket delivers L2 snapshot + numbered deltas — no sequence gaps in 1000-operation automated test
- [ ] Index price sourced from at least 2 real exchanges (Binance + Coinbase minimum)
- [ ] Mark price visible on API
- [ ] `cargo test` passes including new market-data tests (min 20 new tests)

---

## Month 5: REST API Completeness

**Theme:** Every endpoint a trading client needs exists and works correctly.

### Goals
- All missing REST endpoints from GAPS.md P1 are implemented
- Order amendment (price/qty change without losing queue priority)
- Trigger orders (stop-loss, take-profit) wired end-to-end
- Deposit and withdrawal endpoints complete
- OpenAPI spec generated from code

### Research (before coding)
- Study order amendment semantics: Binance cancels and replaces (loses queue position); some exchanges allow in-place amendment only if price is improved (stays in queue). Decide the policy and document it. Recommendation: cancel + replace (simpler, less error-prone).
- Study Fireblocks API for webhook authentication — they sign webhooks with their private key; verify with their public key. Understand the idempotency guarantees (can a webhook fire twice for the same deposit?).
- Study `utoipa` crate for auto-generating OpenAPI 3.0 spec from axum handler annotations. Understand what it can and cannot auto-generate (request/response examples must be manual).

#### T5.1 — Open Orders List and Single Order Endpoints (effort: 1d, risk: L, deps: T1.3)
**Files:** `crates/api-gateway/src/rest.rs`

- `GET /api/v1/orders` — returns user's currently OPEN orders (those not in terminal state). Query `order_repo` with `WHERE status = 'Open' AND user_id = $1`.
- `GET /api/v1/orders/{id}` — single order by ID with current status, filled_qty, created_at
- `GET /api/v1/orders/history?limit=50&before_id=X` — paginated history
- Acceptance: 5 open orders, GET /orders returns 5; cancel one, GET /orders returns 4

#### T5.2 — Order Amendment (effort: 2d, risk: M, deps: T2.1)
**Files:** `crates/api-gateway/src/rest.rs`, `crates/matching-engine/src/engine.rs`

- `PUT /api/v1/orders/{id}` — accepts `{"price": X, "qty": Y}` (either or both)
- Implementation: cancel the existing order (release locks), place a new order with the new parameters (acquire locks)
- The response includes the new `order_id` — clients must update their local state
- Queue position is not preserved (this is the declared policy — document it)
- Acceptance: amend an open limit order to a different price, verify original order cancelled and new order appears in the book at the new price

#### T5.3 — Trigger Orders End-to-End (effort: 3d, risk: H, deps: T2.2)
**Files:** `crates/trigger-service/src/trigger_engine.rs`, `crates/api-gateway/src/rest.rs`, `crates/api-gateway/src/tasks.rs`

`TriggerEngine` exists with full logic. It is not wired to receive price updates or to execute triggered orders.

- `spawn_trigger_monitor_task`: on each mark price update, call `TriggerEngine::check_triggers(symbol, price)`. This returns a `Vec<TriggeredOrder>`.
- For each `TriggeredOrder`, send a `PlaceOrderCommand` through the order gateway (same path as normal orders)
- `POST /api/v1/trigger-orders` — place stop-loss, take-profit, trailing stop. Parameters: `symbol`, `side`, `trigger_price`, `order_type` (Limit/Market), `price` (for stop-limit), `qty`, `close_on_trigger: bool`
- `GET /api/v1/trigger-orders` — list user's active trigger orders
- `DELETE /api/v1/trigger-orders/{id}` — cancel a trigger order (before it fires)
- OCO: if TP and SL are placed together, add them to an `OcoGroup`. When one triggers and fills, cancel the other.
- Acceptance: place long position, place SL below entry price, move mark price below SL trigger — SL executes as market order, position closes

#### T5.4 — Deposit and Withdrawal Endpoints (effort: 2d, risk: M, deps: T1.1)
**Files:** `crates/api-gateway/src/rest.rs`, `crates/custody/src/deposit.rs`, `crates/custody/src/withdrawal.rs`

- `POST /api/v1/account/deposit-address` — body: `{"asset": "BTC", "network": "ERC20"}`. Returns existing cached address or requests new one from `CustodyProvider`. For now, mock provider returns deterministic test addresses. Month 7 wires Fireblocks.
- `GET /api/v1/account/deposits?limit=20` — query `deposits` table
- `GET /api/v1/account/withdrawals?limit=20` — query `withdrawals` table
- `POST /api/v1/account/withdraw` — validates: address is whitelisted, 2FA code provided, balance sufficient, AML check passes (amount < reporting threshold or auto-flag). Creates withdrawal record with status `PendingApproval`. Large amounts (> $10K) stay pending for admin approval. Small amounts auto-approve.
- Acceptance: request deposit address, receive address, simulate deposit webhook, balance increases; withdrawal request creates a DB record

#### T5.5 — OpenAPI Spec Generation (effort: 2d, risk: L, deps: T5.4)
**Files:** `crates/api-gateway/Cargo.toml`, all handler files

- Add `utoipa` and `utoipa-swagger-ui` to `api-gateway`
- Annotate all request/response types with `#[derive(ToSchema)]`
- Annotate all handlers with `#[utoipa::path(...)]`
- Serve spec at `GET /api/v1/openapi.json` and Swagger UI at `GET /docs`
- Write SDK examples: `docs/api/examples/` — curl examples for every endpoint
- Acceptance: Swagger UI accessible at /docs, shows all endpoints, request/response schemas; examples work when copy-pasted

### Definition of Done — Month 5
- [ ] Every endpoint in GAPS.md P1 REST section is implemented or explicitly deferred to Phase 3 with justification
- [ ] Trigger orders fire when mark price crosses trigger level (tested in automated test)
- [ ] OpenAPI spec at /docs with all endpoints documented
- [ ] Deposit flow: address → (mock) webhook → balance credited
- [ ] Withdrawal flow: request → 2FA verify → pending approval → admin approve → balance debited

---

## Month 6: Market Maker Infrastructure

**Theme:** An exchange without market makers has empty books. This month builds the infrastructure that attracts and retains professional MMs — SLA monitoring, dedicated API, toxicity detection, and rebate engine.

### Research (before coding)
- Study Bybit MM program requirements: uptime 90-99%, max spread 10-100 bps, min depth $2K-$75K per side, two-sided quoting mandatory, refill after fill within N seconds
- Study VPIN (Volume-Synchronized PIN) — measures informed trading flow toxicity. Our bot-trader already has this in `crates/core/src/analysis/indicators/vpin.rs`
- Study Kyle's Lambda — price impact per unit of order flow. Measures how much one dollar of net flow moves the price
- Study adverse selection: when MM orders are filled and price immediately moves against them, the counterparty was "informed" — need to detect and adapt
- Study Binance market maker API: batch order placement (up to 10 orders in one request), batch cancel, order amendment without losing queue position
- Study the FIX protocol for MM connectivity — our `fix-gateway` has parser+session but no TCP server or order routing

### Goals
- SLA engine monitors MM obligations in real-time
- MM dashboard: uptime, spread, depth, fill rate
- Dedicated batch API for high-frequency quoting
- Toxicity detection (VPIN, fill analysis) wired to mm metrics
- Dynamic rebate engine based on MM quality score

### Tasks

#### T6.1 — MM SLA Engine (effort: 5d, risk: H, deps: T2.1)
**Create:** `crates/market-maker/src/sla.rs`

Create new crate `crates/market-maker/`.

```rust
pub struct MmSlaConfig {
    pub mm_user_id: UserId,
    pub symbol: Symbol,
    pub min_uptime_pct: Decimal,         // e.g., 95%
    pub max_spread_bps: Decimal,         // max spread in basis points
    pub min_depth_per_side: Decimal,     // min USD on each side
    pub two_sided_required: bool,
    pub refill_after_fill_secs: u64,     // must re-quote within N secs of fill
    pub evaluation_window_secs: u64,     // rolling window for metrics
}

pub struct MmSlaMonitor {
    configs: HashMap<UserId, MmSlaConfig>,
    snapshots: HashMap<UserId, Vec<MmSnapshot>>,
}

pub struct MmSnapshot {
    pub timestamp: DateTime<Utc>,
    pub best_bid: Option<(Price, Qty)>,
    pub best_ask: Option<(Price, Qty)>,
    pub spread_bps: Option<Decimal>,
    pub bid_depth_usd: Decimal,
    pub ask_depth_usd: Decimal,
    pub is_two_sided: bool,
}
```

Methods:
- `register_mm(config)` — register a market maker with SLA terms
- `record_snapshot(user_id, snapshot)` — called every second from background task
- `evaluate(user_id) -> MmSlaReport` — compute uptime, avg spread, min depth compliance, fill refill times
- `is_compliant(user_id) -> bool` — meets all SLA thresholds
- `demote_if_non_compliant(user_id) -> Option<DemotionAction>` — warn, reduce rebates, or suspend MM status
- Tests: uptime calculation, spread averaging, two-sided detection, refill timing, demotion

#### T6.2 — MM Batch Order API (effort: 3d, risk: M, deps: T5.1)
**Files:** `crates/api-gateway/src/rest.rs`, `crates/api-gateway/src/exchange.rs`

Market makers need to update 20+ orders per tick. Individual REST calls add too much latency.

- `POST /api/v1/orders/batch` — accepts array of up to 20 PlaceOrderCommands, returns array of results
- `DELETE /api/v1/orders/batch` — accepts array of order IDs to cancel, returns results
- `PUT /api/v1/orders/batch-amend` — accepts array of (order_id, new_price, new_qty), returns results
- Each is atomic per order (individual orders may fail without rolling back others)
- Rate limit: MM-tier users get 5000 req/min (vs 1000 for regular)
- Acceptance: place 20 orders in one request, verify all created, cancel all in one request

#### T6.3 — Toxicity Detection for MM Protection (effort: 3d, risk: M, deps: T4.1)
**Create:** `crates/market-maker/src/toxicity.rs`

Detect when MM orders are being adversely selected (filled by informed traders):

```rust
pub struct ToxicityAnalyzer {
    fill_records: HashMap<UserId, Vec<MmFillRecord>>,
    window_secs: u64,
}

pub struct MmFillRecord {
    pub fill_price: Price,
    pub side: Side,
    pub price_after_1s: Option<Price>,   // mark price 1 second after fill
    pub price_after_5s: Option<Price>,
    pub price_after_30s: Option<Price>,
    pub adverse_pnl: Option<Decimal>,    // PnL 30s after fill
}

pub struct ToxicityReport {
    pub vpin: Decimal,                    // 0-1, higher = more toxic
    pub adverse_selection_pct: Decimal,   // % of fills where price moved against MM
    pub avg_adverse_move_bps: Decimal,    // avg adverse move in bps
    pub kyles_lambda: Decimal,            // price impact per unit flow
}
```

Methods:
- `on_mm_fill(user_id, fill_price, side, timestamp)` — record fill
- `update_post_fill_prices(user_id, timestamp, current_price)` — update 1s/5s/30s marks
- `analyze(user_id) -> ToxicityReport` — compute VPIN, adverse selection rate, Kyle's Lambda
- Use output to: adjust MM spread (widen in toxic flow), alert MM, adjust rebates

#### T6.4 — Dynamic Rebate Engine (effort: 2d, risk: M, deps: T6.1)
**Create:** `crates/market-maker/src/rebates.rs`

MM rebates should scale with quality of market making, not just volume:

```rust
pub struct RebateEngine {
    base_rebate: Decimal,         // e.g., -0.01% (maker pays 0.01% rebate)
    quality_bonus: Decimal,       // additional rebate for high-quality MMs
}

pub struct MmQualityScore {
    pub uptime_score: Decimal,     // 0-1
    pub spread_score: Decimal,     // 0-1 (tighter = higher)
    pub depth_score: Decimal,      // 0-1
    pub refill_score: Decimal,     // 0-1
    pub composite: Decimal,        // weighted average
}
```

Methods:
- `calculate_quality(sla_report) -> MmQualityScore`
- `rebate_for_quality(score) -> Decimal` — base + quality_bonus * composite
- `apply_rebate(fill, mm_user_id) -> Decimal` — actual rebate to credit

#### T6.5 — MM PnL Attribution (effort: 2d, risk: L, deps: T6.3)
**Create:** `crates/market-maker/src/pnl_attribution.rs`

Break down MM profits by source:

```rust
pub struct MmPnlAttribution {
    pub spread_capture: Decimal,       // profit from bid-ask spread
    pub inventory_pnl: Decimal,        // unrealized PnL from inventory
    pub rebate_income: Decimal,        // fee rebates earned
    pub adverse_selection_cost: Decimal, // losses from toxic flow
    pub net_pnl: Decimal,
}
```

Methods:
- `attribute(fills, inventory_changes, rebates, toxicity_costs) -> MmPnlAttribution`
- MM dashboard shows this breakdown in real-time

#### T6.6 — FIX Gateway TCP Server (effort: 3d, risk: H, deps: T6.2)
**Files:** `crates/fix-gateway/`, `crates/server/src/main.rs`

Our FIX gateway has parser + session + translator but no TCP listener:
- Add `FixTcpServer` using tokio TcpListener
- Accept connections, manage FIX sessions
- Route NewOrderSingle → exchange.place_order()
- Route CancelRequest → exchange.cancel_order()
- Send back ExecutionReports on fills
- Dedicated port (e.g., 8081)

### Definition of Done — Month 6
- [ ] At least one registered MM with SLA monitoring dashboard
- [ ] Batch order API handles 20 orders in <50ms
- [ ] Toxicity report shows VPIN and adverse selection for each MM
- [ ] Dynamic rebates adjust based on quality score
- [ ] FIX gateway accepts TCP connections and routes orders
- [ ] PnL attribution breaks down MM profits by source
- [ ] 25+ new tests across market-maker crate

---

## Month 7: Infrastructure, Observability, and CI/CD

**Theme:** Make the system operable. A production exchange needs monitoring, deployment automation, and incident response tooling.

### Goals
- Docker Compose for local dev with Postgres + Redis
- GitHub Actions CI: test + clippy + build on every PR
- Prometheus metrics wired to real counters (not just the metrics struct)
- Grafana dashboard for matching engine health
- Error alerting (PagerDuty or equivalent)
- Email notifications for users (fills, withdrawals, logins)

### Research (before coding)
- Study Prometheus histogram vs summary for latency — histograms are more composable (can aggregate across instances), summaries are per-instance. Use histograms for matching engine latency.
- Study `tower-http` tracing middleware — understand how to propagate request IDs through the async chain for log correlation. The `TraceLayer` adds spans automatically; add request_id header manually.
- Study `lettre` crate for SMTP email — understand TLS configuration, connection pooling, retry on SMTP failure.
- Study GitLab CI vs GitHub Actions — the project is currently on GitHub. Stay on GitHub Actions.

#### T6.1 — Docker Compose for Local Development (effort: 1d, risk: L, deps: none)
**Files:** `docker-compose.yml`, `Dockerfile`, `.env.example`

- `docker-compose.yml` with services: `postgres` (16-alpine, persisted volume), `redis` (7-alpine), `exchange` (built from Dockerfile)
- `Dockerfile`: multi-stage — `rust:1.85` builder → `debian:bookworm-slim` runtime image
- `.env.example` with all required env vars: `DATABASE_URL`, `REDIS_URL`, `TOTP_ENCRYPTION_KEY`, `ALLOWED_ORIGINS`, `EXCHANGE_DATA_DIR`
- `make dev` shortcut: `docker-compose up --build`
- `make test` shortcut: `docker-compose run --rm exchange cargo test`
- Health check endpoint: `GET /health` returns `{"status":"ok","db":"connected","wal":"ok"}` — wired to real checks
- Acceptance: `docker-compose up` from clean checkout starts exchange on port 8080 with all dependencies

#### T6.2 — GitHub Actions CI Pipeline (effort: 1d, risk: L, deps: T6.1)
**Files:** `.github/workflows/ci.yml`

- On every PR: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`
- Integration tests run against a `postgres` service container (GitHub Actions supports `services:` syntax)
- Cache cargo registry and build artifacts (`actions/cache` with `~/.cargo` + `target/` keyed by `Cargo.lock`)
- On push to `main`: build release binary, build Docker image, push to GitHub Container Registry
- Acceptance: CI runs in < 10 minutes; a PR with a failing test blocks merge

#### T6.3 — Real Prometheus Metrics (effort: 2d, risk: L, deps: T2.1)
**Files:** `crates/admin/src/metrics.rs`, `crates/api-gateway/src/server.rs`

Current `ExchangeMetrics` has atomic counters but they're not exposed as Prometheus types.

- Add `prometheus` crate dependency
- Replace atomic counters with `prometheus::IntCounter`, `prometheus::Histogram` (for latencies), `prometheus::Gauge` (for open positions count, order book depth)
- Key metrics to instrument:
  - `exchange_orders_total{symbol,side,type,result}` — counter
  - `exchange_order_latency_seconds{symbol}` — histogram (WAL write + match + settle)
  - `exchange_active_orders{symbol}` — gauge
  - `exchange_open_positions{symbol}` — gauge
  - `exchange_liquidations_total{symbol}` — counter
  - `exchange_websocket_connections` — gauge
  - `exchange_matching_engine_queue_depth{symbol}` — gauge
- `GET /metrics` returns Prometheus text format
- Acceptance: `curl /metrics` shows all gauges with real values; order placement increments `exchange_orders_total`

#### T6.4 — Grafana Dashboard Templates (effort: 1d, risk: L, deps: T6.3)
**Files:** `monitoring/grafana/dashboards/`

- Matching Engine dashboard: orders/sec per symbol, p50/p95/p99 latency, queue depth, liquidations/min
- Account dashboard: active users, open positions, total balance per asset (requires reconciliation job)
- Infrastructure dashboard: DB connection pool utilization, CPU, memory, WebSocket connections
- Export as JSON and check into `monitoring/grafana/dashboards/`
- Add to docker-compose: Grafana + Prometheus containers with pre-loaded dashboards
- Acceptance: `docker-compose up monitoring` shows dashboards with live data from exchange

#### T6.5 — Email Notification System (effort: 2d, risk: L, deps: none)
**Files:** `crates/admin/src/notifications.rs`

Current `NotificationManager` is in-memory, no actual emails sent.

- Add `lettre` for SMTP
- Email templates (plain text + HTML): fill notification, withdrawal confirmation, withdrawal approval/rejection, login from new IP, liquidation warning
- `SMTP_HOST`, `SMTP_PORT`, `SMTP_USER`, `SMTP_PASSWORD`, `FROM_EMAIL` env vars
- Async send with retry (3 attempts, 5s backoff) — failure logs error but does not fail the operation that triggered it
- Acceptance: with SMTP configured, place order that fills → email received within 10 seconds; liquidation warning email fires at 80% margin usage

### Definition of Done — Month 6
- [ ] `docker-compose up` starts the full stack in < 2 minutes
- [ ] CI runs and fails on broken code
- [ ] Grafana dashboard shows live exchange metrics
- [ ] Email notifications fire for: fill, withdrawal, liquidation warning
- [ ] `GET /health` shows real DB and WAL status
- [ ] `/metrics` Prometheus endpoint has at least 15 meaningful metrics

---

## Month 8: Trading Terminal Frontend

**Theme:** A working trading terminal that real traders can use. Not pretty — functional.

### Goals
- React web app with: order book, chart, order form, position panel, order history
- Live WebSocket connection for all panels
- Account management: deposit, withdraw, order history
- The UI is the integration test — if it works in the browser, the backend is wired correctly

### Research (before coding)
- Study TradingView Charting Library licensing — it is free for non-commercial use but requires a commercial license for paid products. Evaluate alternatives: Lightweight Charts (open source by TradingView), Recharts, or uPlot. **Decision needed before coding.** Lightweight Charts is the recommendation for a startup — same quality, open source.
- Study React WebSocket patterns — `useWebSocket` hook vs `EventSource` vs raw WebSocket. The `useWebSocket` package handles reconnection and cleanup. Evaluate if it meets needs.
- Study order book rendering performance — rendering 25 levels 10 times per second in React is expensive. `react-window` for virtualization, or canvas-based rendering. Research what other terminals use.

#### T7.1 — Project Setup and Authentication (effort: 2d, risk: L, deps: T3.3)
**Files:** `frontend/` (new directory)

- React + Vite + TypeScript + Tailwind
- Authentication flow: API key entry (for traders) or email/password (not yet — Month 9)
- Store API key in `sessionStorage` (not `localStorage` — cleared on tab close)
- `useExchange` hook: typed API client generated from the OpenAPI spec (using `openapi-typescript`)
- `useWebSocket` hook: connects to `ws://localhost:8080/ws`, subscribes to topics, reconnects on disconnect
- Acceptance: login screen accepts API key, makes authenticated GET /api/v1/account/balances, shows balance

#### T7.2 — Order Book Panel (effort: 2d, risk: M, deps: T4.2)
**Files:** `frontend/src/components/OrderBook.tsx`

- Subscribe to `orderbook.25.{symbol}` WebSocket topic
- Handle snapshot on subscribe, apply deltas
- Render bids (green) and asks (red) with price, qty, cumulative qty columns
- Highlight price levels that changed in the last update (flash animation)
- Cumulative depth bar as background color fill
- Re-render at most 10fps (throttle via `requestAnimationFrame`)
- Acceptance: order book updates in real-time when orders are placed via REST; no visible lag at 10 updates/sec

#### T7.3 — Chart Panel (effort: 2d, risk: M, deps: T4.1)
**Files:** `frontend/src/components/Chart.tsx`

- Use TradingView Lightweight Charts
- Initial load: fetch last 200 candles via `GET /api/v1/markets/{symbol}/klines?interval=1m`
- Subscribe to `kline.1m.{symbol}` WebSocket topic, update chart in real-time
- Symbol selector: click switches chart to that symbol
- Timeframe selector: 1m, 5m, 1h, 1d (each fetches historical candles)
- Acceptance: chart shows live candles; opening a position moves the chart's price line

#### T7.4 — Order Form and Position Panel (effort: 2d, risk: M, deps: T5.3)
**Files:** `frontend/src/components/OrderForm.tsx`, `frontend/src/components/Positions.tsx`

- Order form: limit/market selector, buy/sell toggle, price input, qty input, leverage slider (for futures), TP/SL inputs
- Submit calls `POST /api/v1/orders` or `POST /api/v1/trigger-orders` (for TP/SL)
- Error display: field-level validation errors from API response
- Position panel: subscribes to `position.{symbol}` WebSocket, shows: size, avg_entry, mark_price, unrealized_pnl (color-coded), liquidation_price, close button
- Close button: sends market order with `reduce_only: true` for full position size
- Acceptance: open a position, see it in position panel with correct unrealized PnL, close it, position disappears

#### T7.5 — Account Management Pages (effort: 2d, risk: L, deps: T5.4)
**Files:** `frontend/src/pages/`

- Balance page: shows all assets with total, locked, available
- Deposit page: select asset + network → GET /account/deposit-address → show QR code + address
- Withdrawal page: asset, address, amount, 2FA code field → POST /account/withdraw
- Order history page: paginated table with filters
- API key management: list keys, create key with permission checkboxes, revoke key
- Acceptance: full deposit-to-trade-to-withdraw flow completable without touching the API directly

### Definition of Done — Month 7
- [ ] Trading terminal accessible at `http://localhost:3000` (dev) or built into docker-compose
- [ ] Order book updates in real-time without page refresh
- [ ] Can place a limit order, see it in the order book, have it fill, see the fill in trade history
- [ ] Can open and close a futures position with PnL shown correctly
- [ ] Deposit address page shows a real address (mock custody provider)
- [ ] `npm run build` produces deployable static assets

---

## Month 9: Admin Panel and Compliance Tooling

**Theme:** Operators need to see everything and act on compliance events. This is required for VASP registration.

### Goals
- Admin panel for user management, KYC review, AML alerts, withdrawal approval
- Market surveillance dashboard
- Reconciliation job running daily
- Proof of reserves endpoint

### Research (before coding)
- Study Ukrainian Virtual Assets Law requirements for VASP reporting — specifically what data must be retained, what must be reported to NCSSM, and in what format. Engage a Ukrainian crypto lawyer if not already done. This is not a research task that can be finished from documentation alone.
- Study FATF Travel Rule (VASP-to-VASP transfers) — understand what data must be transmitted, which counterparty VASPs are in scope, what threshold applies (UAH 30K equivalent). Wire this to the `travel_rule.rs` crate.
- Study Merkle proof-of-reserves construction — understand why a simple "sum of balances" is insufficient (does not prove individual inclusion) and why a Merkle tree with sorted leaves is the standard approach.

#### T8.1 — Admin Panel Frontend (effort: 4d, risk: M, deps: T7.1)
**Files:** `frontend/src/admin/`

Separate React app or sub-routes protected by an admin-only role check.

- User list: search by email/user_id, click user → see KYC status, balance, order history, AML flags
- KYC review queue: list `status = 'Pending'` KYC records, show submitted document info (from Sumsub webhook), Approve/Reject buttons
- AML alerts queue: list unreviewed alerts with details, Mark-Reviewed with notes, escalate to STR
- Withdrawal approval queue: list `PendingApproval` withdrawals with user info, amount, destination address, Approve/Reject
- System health: show `/health` endpoint data, DB connection count, matching engine queue depths, WebSocket connections
- Acceptance: admin can review and approve a withdrawal from the UI without touching the database directly

#### T8.2 — Market Surveillance Dashboard (effort: 2d, risk: M, deps: T8.1)
**Files:** `frontend/src/admin/Surveillance.tsx`, `crates/surveillance/`

Wire `SurveillanceEngine` events to an admin WebSocket stream.

- Spoofing alerts: user places > X orders and cancels > Y% within Z seconds → flag
- Wash trading: same entity on both sides of a trade (cross-account — requires account linking via email/IP/device)
- Alert stream on admin WebSocket topic `admin.surveillance`
- Dashboard: real-time feed of surveillance alerts, click alert → view user's trading pattern
- Action buttons: Freeze account (blocks new orders), Cancel all orders, Flag for manual review
- Acceptance: trigger spoofing pattern in test → alert appears in dashboard within 5 seconds

#### T8.3 — Daily Reconciliation Job (effort: 2d, risk: H, deps: T1.1)
**Files:** new `crates/reconciliation/` or add to `crates/admin/`

Critical for detecting bugs before they compound.

- Scheduled daily at 00:00 UTC: `SELECT user_id, asset, SUM(total) FROM balances GROUP BY user_id, asset`
- Compare with: sum of deposits - sum of withdrawals + sum of realized_pnl + sum of funding_payments - sum of fees for each user+asset
- If discrepancy > 0.001% of asset total: emit `CRITICAL` alert, halt new deposits for that asset pending investigation
- Store reconciliation report in `audit_log` with full detail
- `GET /admin/reconciliation?date=2026-04-01` returns that day's report
- Acceptance: inject a synthetic discrepancy (directly UPDATE a balance in test DB), reconciliation job detects it

#### T8.4 — Proof of Reserves (effort: 2d, risk: M, deps: T8.3)
**Files:** `crates/proof-of-reserves/src/`

The Merkle tree logic exists. Wire it to scheduled execution and a public endpoint.

- Daily job: build Merkle tree from all user balances (one leaf per user_id + asset combination). Root hash + all leaf hashes stored in `audit_log`.
- `GET /api/v1/por/verify?user_id=X` — returns the user's balance, their leaf hash, and the Merkle path proving inclusion in the root
- `GET /api/v1/por/latest` — returns root hash + timestamp + total_balances per asset
- Publish root hash to a public Twitter/X account or blockchain (optional but shows commitment)
- Acceptance: generate PoR, verify one user's inclusion proof, verify that altering any balance invalidates the root

#### T8.5 — Travel Rule and STR Reporting (effort: 2d, risk: M, deps: T1.2)
**Files:** `crates/compliance/src/travel_rule.rs`, `crates/compliance/src/aml.rs`

`travel_rule.rs` exists with data structures. Not wired to withdrawal flow.

- On withdrawal > UAH 30K equivalent: collect Travel Rule data (sender name, account, VASP), attach to withdrawal record
- For withdrawals to unknown VASPs: flag for manual review
- STR (Suspicious Transaction Report) generation: when AML flags a transaction for STR, generate a structured report in the format required by SFMS (Ukrainian financial monitoring service)
- `GET /admin/aml/str-queue` — pending STR reports
- `POST /admin/aml/str/{id}/submit` — marks as submitted (actual submission to SFMS is manual in Phase 1)
- Acceptance: large withdrawal triggers travel rule data collection; AML rule triggers STR report generation

### Definition of Done — Month 8
- [ ] Admin can perform all critical operations (approve/reject KYC, AML review, withdrawal approval) without DB access
- [ ] Reconciliation job runs daily and has never missed a discrepancy in 30-day test run
- [ ] Proof of reserves endpoint returns verifiable Merkle proof
- [ ] Market surveillance fires alerts for spoofing/wash-trading test patterns
- [ ] Travel Rule data attached to qualifying withdrawals

---

## Month 10: Security Audit, Pen Test Preparation, and Auth Hardening

**Theme:** Before going live with real money, have an adversarial review of the entire security surface. Fix everything found.

### Goals
- External security audit (or structured internal red team)
- Web login with proper session management (JWT + refresh tokens)
- Device management and login notifications
- All findings from audit resolved

### Research (before coding)
- Study OWASP API Security Top 10 (2023) — go through each item and verify the exchange is not vulnerable. Document the review.
- Study PostgreSQL row-level security (RLS) — evaluate whether to add a DB-level defense to prevent cross-user data access if the application layer has a bug. RLS is a strong defense-in-depth measure for financial data.
- Study refresh token rotation — understand the "rotation + revocation" pattern. A compromised refresh token should be detectable if the old one is used after rotation.
- Study bcrypt vs Argon2id for password hashing — Argon2id is the current recommendation. Understand memory/time parameters appropriate for a VPS server.

#### T9.1 — Email/Password Authentication (effort: 3d, risk: M, deps: T3.2)
**Files:** `crates/api-gateway/src/auth.rs`, `crates/compliance/src/`, `crates/db/`

For the web UI, API key auth is not suitable. Add proper web session auth.

- `POST /api/v1/auth/register` — email + password. Hash with Argon2id. Create user record. Send verification email.
- `POST /api/v1/auth/login` — verify password hash. If 2FA enabled, return `{"status":"requires_2fa","session_token":"..."}`. Otherwise return JWT access token (15min TTL) + refresh token (30d TTL, stored in HttpOnly cookie).
- `POST /api/v1/auth/refresh` — exchange refresh token for new access token + rotated refresh token. Old refresh token invalidated.
- `POST /api/v1/auth/logout` — invalidate refresh token.
- `POST /api/v1/auth/2fa-verify` — verify TOTP code, exchange session token for full JWT
- Device fingerprint recorded on login. New device → email notification.
- Acceptance: full login flow testable; refresh token rotation working; 2FA gate working

#### T9.2 — Session Management and Device Tracking (effort: 1d, risk: M, deps: T9.1)
**Files:** `crates/db/` (add sessions table), `crates/api-gateway/src/`

- `sessions` table: session_id, user_id, device_fingerprint, ip, user_agent, created_at, last_seen, revoked_at
- `GET /api/v1/account/sessions` — list active sessions with device info
- `DELETE /api/v1/account/sessions/{id}` — revoke a session (log out a device)
- `DELETE /api/v1/account/sessions` — revoke all sessions except current
- Acceptance: login from two browsers, see both in sessions list, revoke one, that browser is logged out

#### T9.3 — Internal Security Audit (effort: 5d, risk: H, deps: all Month 1-8 tasks)
This is not coding — it is a structured security review. One developer (ideally the most security-conscious on the team, or an external consultant) does this full-time for a week.

**Scope:**
- Authentication and authorization: can user A access user B's data?
- Balance manipulation: is there any path to credit a balance without a corresponding debit?
- Order injection: can a user place orders on behalf of another user?
- WAL integrity: can the WAL be tampered with? Is it append-only enforced at OS level? (Use a write-only file descriptor opened with `O_APPEND`)
- SQL injection: are all queries parameterized? Run `sqlx::query!` compile-time check confirms this.
- DoS surface: can a single user exhaust server resources (e.g., WebSocket subscriptions, trigger orders)?
- Cryptographic: HMAC-SHA256 key length sufficient? Signature timing attack? (Use constant-time comparison from `subtle` crate)

**Output:** Security audit report. Every finding categorized P0/P1/P2. P0 findings block launch.

#### T9.4 — Fix All P0 and P1 Audit Findings (effort: variable, risk: H, deps: T9.3)

Allocate the remainder of Month 9 to fixing findings. Common expected findings:

- Timing attacks in HMAC comparison — fix with `hmac::Mac::verify_slice`
- Missing `Secure` flag on cookies — add `cookie::CookieBuilder::secure(true)`
- Verbose error messages leaking internal paths — replace with generic "Internal error" + request ID
- Missing request ID propagation — add `x-request-id` header, log it with every operation
- Admin endpoint not protected by admin role — add `admin_only` middleware

### Definition of Done — Month 9
- [ ] Web login flow works end-to-end with 2FA
- [ ] Security audit report produced with all findings documented
- [ ] All P0 audit findings resolved (verified by the auditor)
- [ ] All P1 audit findings resolved
- [ ] `cargo clippy -- -D warnings` still passes after all changes
- [ ] Pen test checklist from OWASP verified

---

## Month 11: Performance, Load Testing, and Optimization

**Theme:** Verify the exchange can handle the expected load. Find the bottlenecks before users do.

### Goals
- Sustained load test: 1000 orders/sec across 3 symbols for 1 hour
- p99 order latency < 5ms (WAL write + match + balance lock + WebSocket push)
- Matching engine throughput: > 50K orders/sec per symbol in benchmark
- Identify and fix top 3 bottlenecks found in load test

### Research (before coding)
- Study `criterion` benchmarking methodology — understand warmup periods, statistical significance, how to benchmark async code correctly
- Study `tokio-console` for async task inspection — essential for finding tasks that are blocking the executor
- Study PostgreSQL EXPLAIN ANALYZE for the hot balance query paths — `SELECT FOR UPDATE` with high concurrency has known performance cliffs. Understand connection pool sizing (`PgPoolOptions::max_connections`)
- Study Linux `io_uring` for WAL writes — the WAL is the primary latency contributor. `io_uring` can reduce fsync overhead significantly. Research `tokio-uring` crate and whether the tradeoffs are acceptable for this stage.

#### T10.1 — Load Test Infrastructure (effort: 2d, risk: L, deps: T6.2)
**Files:** `load-tests/` (new directory)

- `k6` or `Locust` load test scripts
- Scenario 1: "Market Maker" — 10 users each placing + cancelling limit orders continuously on BTCUSDT
- Scenario 2: "Mixed load" — 100 users: 60% market orders, 40% limit orders, 3 symbols
- Scenario 3: "Liquidation storm" — create 1000 positions near current price, drop mark price 10% in one tick, observe liquidation handling
- Metrics collected: order latency histogram, WebSocket message delivery latency, DB connection pool utilization, error rate
- Acceptance: load test framework runs and collects metrics; baseline recorded before optimization

#### T10.2 — Matching Engine Benchmark and Profiling (effort: 2d, risk: M, deps: T2.1)
**Files:** `crates/matching-engine/benches/`

- Run `cargo bench -p exchange-matching-engine` and record baseline
- Profile with `cargo flamegraph -- bench` to identify hot paths
- Expected bottleneck: `Decimal` arithmetic for price comparisons. If confirmed: prototype integer tick representation as an alternative path (multiply all prices by `1/tick_size` to get i64). Compare benchmark results.
- If integer ticks improve throughput > 20%: implement the migration. Document the conversion logic carefully.
- Target: > 50K limit order placements/sec in single-threaded benchmark
- Acceptance: benchmark numbers documented; if migration done, all existing tests pass with integer ticks

#### T10.3 — Database Hot Path Optimization (effort: 2d, risk: M, deps: T1.1)
**Files:** `crates/db/src/balance_repo.rs`, database query tuning

- Run EXPLAIN ANALYZE on `lock_balance` and `settle` queries under load
- Tune `PgPoolOptions::max_connections` to match the number of concurrent balance operations (start at 20, benchmark, increase until diminishing returns)
- Consider: for the matching engine settlement path, batch DB writes. Instead of writing each fill immediately, buffer 10ms of fills and write in a single transaction. This reduces DB round trips 10x at the cost of 10ms additional settlement latency.
- Add a DB-side materialized view for balance summary (total per asset across all users) — refreshed every minute, used by reconciliation and admin without scanning all rows
- Acceptance: balance settlement throughput > 1000 operations/sec (measured with load test)

#### T10.4 — WebSocket Fanout Optimization (effort: 2d, risk: M, deps: T4.2)
**Files:** `crates/api-gateway/src/ws.rs`

At high connection count (1000+ WebSocket clients), the broadcast channel can become a bottleneck.

- Profile: measure time from fill event to WebSocket delivery at 100, 500, 1000 connections
- If broadcast channel is the bottleneck: switch from `tokio::sync::broadcast` to a pub-sub model where each symbol has its own channel and only subscribing clients receive messages (reduces serialization work)
- For the L2 orderbook stream: coalesce updates at 50ms intervals (do not send every single order placement; aggregate all book changes in the window and send one delta message). This reduces message rate by 10-50x for active symbols without visible degradation.
- Acceptance: 1000 WebSocket connections, trading at 100 orders/sec, message delivery latency < 10ms p99

#### T10.5 — WAL Optimization (effort: 2d, risk: M, deps: T1.4)
**Files:** `crates/journal/src/wal.rs`

The WAL fsync is on the critical path: every order waits for disk before matching begins.

- Profile: measure WAL write latency with `tokio::time::Instant`
- Option 1: group commit — buffer 10ms of commands, fsync once for the batch. This is what most databases do. Implementation: a background task drains the buffer and fsyncs, notifying all waiters via channels.
- Option 2: `io_uring` for async fsync — higher performance but complex. Evaluate only if Option 1 is insufficient.
- Target: WAL write latency < 1ms p99 on NVMe storage
- Acceptance: WAL throughput > 10K commands/sec (measured with dedicated benchmark)

### Definition of Done — Month 10
- [ ] Load test runs 1000 orders/sec for 1 hour with 0% error rate
- [ ] p99 order latency < 5ms end-to-end (API receive to WebSocket deliver) measured under load
- [ ] Matching engine benchmark > 50K orders/sec
- [ ] DB settlement throughput > 1000 ops/sec
- [ ] Load test results documented as the performance baseline for regression tracking

---

## Month 12: Regulatory Preparation and VASP Registration

**Theme:** The exchange cannot legally operate in Ukraine without VASP registration. This month is about the legal and technical compliance work required.

### Goals
- All technical requirements for NCSSM VASP registration are met
- Diia API integration for Ukrainian KYC
- Sumsub integration for international KYC
- AML rules configured for Ukrainian law (400K UAH threshold, NSDC sanctions list)
- Audit log export for regulators
- Lawyer review of the system

### Research (before coding)
- Study Diia.Business API documentation — specifically the flow for passport verification, what data is returned, how to handle the QR code scanning flow, what data residency requirements apply
- Study Sumsub API — understand the workflow for onboarding, what webhooks they send on approval/rejection, how to handle the different verification levels they support
- Study OFAC, EU, NSDC (Ukraine) sanctions lists — understand the format of each list and how to screen against them. Evaluate Chainalysis Reactor vs building an in-house screening tool. **Recommendation: use a third-party AML/sanctions provider at this stage** — building in-house sanctions screening is a significant undertaking with legal liability if incorrect.
- Contact NCSSM (Ukrainian financial regulator) to understand the technical assessment process for VASP registration. Get the official checklist of requirements.

#### T11.1 — Diia KYC Integration (effort: 4d, risk: H, deps: T8.1)
**Files:** `crates/compliance/src/diia.rs`

The `diia.rs` file exists with structs. No actual API calls.

- Implement OAuth flow with Diia.Business API (sandbox first)
- `POST /api/v1/kyc/diia/init` — starts verification, returns deeplink/QR code for Diia app
- Webhook handler at `POST /webhooks/diia` — receives verification result from Diia
- On success: extract `first_name`, `last_name`, `tax_id` (РНОКПП), `dob`, `document_type`, `document_number`
- Store extracted data (encrypted at rest) in `kyc_records` table
- Auto-approve Ukrainian KYC if Diia verification succeeds (no manual review needed — Diia is authoritative)
- Acceptance: Ukrainian user can scan QR with Diia app → KYC status moves to Verified automatically

#### T11.2 — Sumsub Integration for International Users (effort: 3d, risk: M, deps: T8.1)
**Files:** `crates/compliance/src/sumsub.rs`

- Implement Sumsub SDK (REST API calls to `api.sumsub.com`)
- `POST /api/v1/kyc/sumsub/init` — create applicant, return SDK token for frontend WebSDK
- Webhook at `POST /webhooks/sumsub` — receives status updates
- Map Sumsub levels to our KYC levels: `basic-kyc-level` → Basic, `advanced-kyc-level` → Full
- Rejection reasons stored in `kyc_records.reason` field
- Admin can trigger manual re-verification request
- Acceptance: international user completes Sumsub flow → KYC updated → trading limits unlocked

#### T11.3 — Sanctions Screening (effort: 2d, risk: H, deps: T11.1)
**Files:** `crates/compliance/src/aml.rs`

- Integrate with Chainalysis or Elliptic API for sanctions screening (or at minimum, OFAC SDN list)
- Screen user name against sanctions list on KYC submission
- Screen withdrawal address against known malicious/sanctioned addresses
- Positive match: freeze account, flag for manual review, do NOT tell the user they're on a sanctions list
- Daily refresh of the sanctions list (scheduled job)
- Acceptance: test with a known OFAC SDN name → account flagged; clean user → proceeds normally

#### T11.4 — Audit Log Export (effort: 1d, risk: L, deps: T1.2)
**Files:** `crates/api-gateway/src/rest.rs` (admin endpoint)

- `GET /admin/audit-log?from=2026-01-01&to=2026-04-01&user_id=X&action=withdrawal` — paginated, filterable
- Export format: JSON Lines (one JSON object per line) — standard for regulatory reporting
- The audit_log table already exists. This is just the query + pagination layer.
- Ensure the DB user used by the application does NOT have UPDATE or DELETE on `audit_log` (enforce via PostgreSQL GRANT/REVOKE)
- Acceptance: admin can export 90 days of a user's activity in < 5 seconds

#### T11.5 — Legal Review and VASP Application (effort: variable, not coding)
This is not a development task — it is a business task. But it has technical prerequisites.

**Prerequisites that must be met before filing:**
- [ ] KYC for all Ukrainian users via Diia
- [ ] AML rules implemented per Ukrainian law (Law on Prevention of Money Laundering No. 361-IX)
- [ ] STR reporting capability (Month 8)
- [ ] Audit log immutable and exportable (T11.4)
- [ ] Travel Rule implemented (Month 8)
- [ ] Data residency: servers in Ukraine or EU (verify current hosting)

**Tasks:**
- Engage a Ukrainian crypto/fintech lawyer (firms: Juscutum, KM Partners)
- Prepare the VASP registration package per NCSSM requirements
- Submit and track the registration

### Definition of Done — Month 11
- [ ] Ukrainian user can complete KYC via Diia app without manual admin intervention
- [ ] International user can complete KYC via Sumsub
- [ ] Sanctions screening runs on every KYC submission and every withdrawal
- [ ] Audit log covers all required event types and is exportable to JSON Lines
- [ ] VASP registration application filed with NCSSM (or timeline confirmed with lawyer)

---

## Month 13: Launch Preparation and Soft Launch

**Theme:** Controlled launch with real users and real money, but with guardrails that limit blast radius.

### Goals
- Feature flags in place for all risky features
- Withdrawal limits enforced (daily limit per user until trust is established)
- Spot trading only for launch (futures behind flag)
- 10 beta users invited, all flows tested with real money
- Runbook for every incident type
- On-call rotation established

### Research (before coding)
- Study exchange launch strategies — most exchanges do an invite-only beta with withdrawal limits and cold-start liquidity from a market maker partnership. Research whether to launch with own liquidity or partner with a market maker for initial order book depth.
- Study database backup strategies — WAL archiving to S3, point-in-time recovery (PITR). Understand RTO (recovery time objective) and RPO (recovery point objective) targets. For a financial exchange, RPO should be < 1 minute.
- Study incident response playbooks from published exchange post-mortems — BitFinex, Kraken, FTX (the last one for what NOT to do). Understand what monitoring thresholds should trigger which responses.

#### T12.1 — Feature Flag System (effort: 1d, risk: L, deps: none)
**Files:** `crates/common/src/`, `crates/api-gateway/src/server.rs`

- Simple feature flag config loaded from env vars and/or a `flags.toml` file at startup
- Flags: `ENABLE_FUTURES=false`, `ENABLE_WITHDRAWALS=true`, `ENABLE_COPY_TRADING=false`, `ENABLE_TRADING_BOTS=false`
- Handlers check the flag before processing the request, return 503 "Feature not available" if disabled
- Flags can be toggled without restart (watch the config file with `notify` crate)
- Acceptance: set `ENABLE_FUTURES=false`, attempt to place a perp order → 503; set to true → works

#### T12.2 — Withdrawal Daily Limits (effort: 1d, risk: M, deps: T5.4)
**Files:** `crates/custody/src/withdrawal.rs`, `crates/db/`

For launch, limit withdrawals to reduce risk from bugs.

- Default per-user daily withdrawal limit: $1000 (configurable)
- KYC verified users: $10K/day
- VIP users: $100K/day
- Track in DB: `SELECT SUM(amount_usd) FROM withdrawals WHERE user_id=$1 AND created_at > NOW() - INTERVAL '24h' AND status != 'Rejected'`
- Admin can override per-user limit
- Acceptance: user attempts to exceed daily limit → 400 with remaining limit in response

#### T12.3 — Backup and Recovery (effort: 2d, risk: H, deps: T6.1)
**Files:** `scripts/backup.sh`, `scripts/restore.sh`, documentation

- PostgreSQL continuous WAL archiving to S3/compatible storage using `pgbackrest` or `wal-g`
- Full base backup daily at 03:00 UTC
- Test restore procedure: restore to a staging environment from backup, verify data integrity
- WAL files for the exchange journal: back up alongside PostgreSQL WAL
- `scripts/restore.sh` documented with step-by-step instructions
- Recovery drill: simulate server loss, restore from backup, verify exchange state
- Acceptance: can restore the exchange to within 1 minute of any point in the last 24 hours; drill documented

#### T12.4 — Runbooks and Incident Response (effort: 2d, risk: L, deps: T6.4)
**Files:** `docs/runbooks/`

Write runbooks for each incident type:
- `runbook-db-down.md`: PostgreSQL unavailable — exchange goes into read-only mode? Halt trading? Steps to restore.
- `runbook-wal-corruption.md`: WAL replay fails on startup — how to identify the corruption, how to rebuild from DB state
- `runbook-matching-engine-halt.md`: matching engine thread panics — automatic restart? Manual intervention? How to verify book state is consistent
- `runbook-liquidation-cascade.md`: large market move triggers mass liquidations — circuit breaker procedure, manual halt, communication to users
- `runbook-suspected-hack.md`: suspicious balance changes detected — immediate steps, who to call, how to halt withdrawals
- On-call schedule: who is paged for P0, P1 alerts — at least 2 people

#### T12.5 — Beta Launch (soft launch) (effort: ongoing)
Not a coding task — an operational phase.

- Invite 10 beta users (ideally people you trust, with real but small amounts)
- Spot trading only, withdrawal limit $500/day
- Monitor Grafana dashboards 24/7 for first 2 weeks
- Daily reconciliation review (verify no discrepancies)
- Weekly security review: check audit log for anomalies
- Collect feedback on UI, latency, missing features
- Fix P0/P1 issues within 24 hours during beta

### Definition of Done — Month 12
- [ ] Feature flags working — futures can be disabled without touching code
- [ ] Withdrawal limits enforced — tested with real API calls
- [ ] Backup and restore procedure tested end-to-end on staging
- [ ] Runbooks exist for all 5 incident types
- [ ] 10 beta users onboarded with real accounts
- [ ] Exchange has run for 2 weeks in beta with zero data integrity issues

---

## Cross-Cutting Concerns

### Things That Run Every Month (Non-Negotiable)
1. `cargo test` must pass — no exceptions, no "we'll fix it later"
2. `cargo clippy -- -D warnings` must pass
3. `cargo fmt --check` must pass
4. New code gets tests — minimum coverage proportional to risk (high-risk financial code: > 80%; UI glue code: > 30%)
5. Security consideration for every external-facing change

### Things Explicitly Deferred

These are scoped out of the 12-month roadmap deliberately. They become the 13-18 month roadmap.

**Perpetual Futures (Phase 3 in ARCHITECTURE.md):**
- The matching engine, risk engine, position manager, and settlement all exist. The deferral is about readiness, not code. Futures introduce liquidation risk and socialized loss — launch with spot, prove the system is stable, then enable futures with invitation-only access.

**FIX 4.4 Gateway (`crates/fix-gateway/`):**
- Parser and session management exist. TCP listener and order routing are missing. This is a Month 13+ task — market makers will request it; implement when you have a market maker ready to use it.

**Copy Trading and Trading Bots:**
- Full implementations exist in their respective crates. Integration into Exchange is missing. Defer until the core spot + futures product is stable. These are differentiation features, not table stakes.

**Algo Execution (TWAP/VWAP):**
- Useful but not required at launch. Defer to Month 14+.

**Mobile App (MeatGrinder reuse):**
- The bot-trader mobile app uses the same Rust core. Adapting it for exchange functionality is a significant project. Month 14+.

**Options Trading:**
- Phase 3 in ARCHITECTURE.md. Year 2+.

### Risk Register

| Risk | Impact | Likelihood | Mitigation | Month |
|------|--------|------------|------------|-------|
| VASP registration rejected or delayed | Blocks legal launch | Medium | Engage lawyer Month 1 not Month 11; run shadow compliance review monthly | 11 |
| PostgreSQL data loss during development | Lose weeks of work | Medium | Daily pg_dump from Month 1; even local dev should have a cron job | 1 |
| Matching engine bug causes incorrect fills | Financial loss, user trust | Medium | Property-based tests (proptest) for balance invariants, Month 3; chaos tests Month 10 | 3 |
| Liquidation engine bug | Positions not closed, exchange takes loss | High | Liquidation tests Month 2; load test Month 10; conservative liquidation price threshold | 2 |
| Security breach before audit | Catastrophic | Low (no public traffic yet) | No public traffic until Month 9 audit is complete | 9 |
| WAL corruption on crash | Orderbook state lost | Low | WAL replay tests Month 1; backup Month 12 | 1 |
| Index price manipulation | Incorrect mark price, wrong liquidations | Medium | Weighted median of 3+ sources; outlier rejection Month 4 | 4 |
| Key developer leaves | Timeline slip | Medium | Document all architectural decisions; no single point of knowledge | ongoing |
| Regulatory change in Ukraine | Compliance requirements shift | Medium | Monitor NCSSM updates monthly; engage lawyer for early warning | ongoing |

### Monthly Test Count Targets

| Month | New Tests Target | Cumulative Target | Focus Area |
|-------|-----------------|-------------------|------------|
| 1 | +40 | 965 | DB integration tests, WAL replay |
| 2 | +30 | 995 | Settlement chain, liquidation end-to-end |
| 3 | +20 | 1015 | Security enforcement, 2FA flow |
| 4 | +30 | 1045 | Market data, WebSocket protocol |
| 5 | +25 | 1070 | REST endpoint coverage |
| 6 | +15 | 1085 | Integration tests for infra |
| 7 | +10 | 1095 | Frontend component tests (Vitest) |
| 8 | +20 | 1115 | Reconciliation, PoR, surveillance |
| 9 | +20 | 1135 | Auth flows, session management |
| 10 | +10 | 1145 | Benchmark regressions (criterion) |
| 11 | +15 | 1160 | KYC/AML integration tests (against sandboxes) |
| 12 | +10 | 1170 | Runbook verification scripts |

### Critical Path (Sequential — Cannot Be Parallelized)

```
T1.1 (DB balance) → T2.2 (settlement chain) → T2.3 (liquidation executes) → T10.1 (load test)
T1.5 (API keys in DB) → T3.3 (permission enforcement) → T9.1 (web auth)
T4.1 (market data) → T4.2 (L2 WebSocket) → T7.2 (order book panel)
T11.1 (Diia KYC) → T11.3 (sanctions) → VASP registration
```

### What Success Looks Like at Month 12

A Ukrainian crypto trader can:
1. Register with email + password
2. Complete KYC via Diia (Ukrainian passport) in under 5 minutes
3. Deposit BTC to their exchange address and see it credited within 1 block confirmation
4. Place a limit buy order and see it fill when price is reached
5. Withdraw their funds to a whitelisted address with 2FA confirmation
6. See their trade history, fees paid, and current balances

The exchange can:
1. Handle 100 concurrent users without latency degradation
2. Survive a server restart without losing any user data
3. Pass a reconciliation check: sum of all balances equals sum of all deposits minus all withdrawals
4. Detect and alert on market manipulation patterns
5. Export a user's complete history for regulatory review

That is the MVP. Everything else is iteration.
