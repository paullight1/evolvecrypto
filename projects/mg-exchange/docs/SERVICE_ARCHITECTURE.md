# Service Decomposition Architecture

## Overview

This document defines the target service architecture for decomposing the current monolith into independently deployable services. It is an implementation guide, not a whitepaper — each section covers the current state, decomposition boundary, concrete API contracts, data ownership rules, and migration steps grounded in the actual codebase.

### Current State

The monolith is a single Rust binary (`exchange` crate) anchored by an `Exchange` god struct in `crates/api-gateway/src/exchange.rs`. At time of writing that struct has approximately 90 `Mutex`-wrapped fields covering every domain from order matching to compliance to Web3 wallets. The matching engine already runs each symbol on its own OS thread via `EnginePool` (`crates/matching-engine/src/engine_pool.rs`), with lock-free communication over crossbeam bounded channels — this is the single strongest architectural seam in the codebase and the natural first extraction point.

Key observations from reading the source:

- **Matching engine** (`EnginePool` + `EngineHandle`) is already fully isolated: zero async, zero network, communicates via typed channels. It can be extracted with minimal interface changes.
- **Background tasks** (`crates/api-gateway/src/tasks.rs`) hold `Arc<Exchange>` and access 10–15 different Mutex fields per tick. Liquidation alone touches: `positions`, `accounts`, `insurance_funds`, `perp_configs`, `market_tx`, `private_tx`, `liquidation_events`, `notifications`, `device_registry`. This tight coupling is the main decomposition risk.
- **Settlement, risk, and account state are co-entangled.** The liquidation task writes to `accounts` (balance debits), `positions` (position reduction), and `insurance_funds` simultaneously with no transactional boundary between them.
- **Market data** (`MarketDataService` behind `Mutex<MarketDataService>`) is read-heavy and already logically separate — no financial mutations flow through it.
- **Compliance, custody, and KYC** are operationally independent from trading latency requirements and are natural candidates for async, eventually-consistent service extraction.

### Target: Five Services

```
+-----------------+     gRPC (sync)    +-------------------+
|   API Gateway   |<------------------>|   Risk Service    |
|  (axum, REST,   |                    |  (pre-trade, CB,  |
|   WebSocket,    |     gRPC (sync)    |   margin check)   |
|   FIX gateway)  |<---------+---------+-------------------+
+--------+--------+          |               |
         |                   v               |  gRPC (sync)
         |          +------------------+     |
         |          | Matching Service |-----+
         |          | (EnginePool,     |
         |          |  WAL, per-symbol |
         |          |  OS threads)     |
         |          +--------+---------+
         |                   |
         |           fill events (NATS)
         |                   |
         |     +-------------+------------------+
         |     |             |                  |
         v     v             v                  v
  +----------+ +-----------+ +------------+
  | Account  | | Settlement| | Market Data|
  | Service  | | Service   | | Service    |
  | (balance,| | (funding, | | (tickers,  |
  |  margin, | |  insur.   | |  klines,   |
  |  KYC,    | |  fund,    | |  WS fanout)|
  |  custody)| |  ADL,     | |            |
  +----------+ |  liq fee) | +------------+
               +-----------+
```

---

## 1. Service Boundaries

### 1.1 Matching Service

**Responsibility**

Order book management and price-time-priority matching for all listed symbols. This service is the sole writer to the order book. It produces fill events and order lifecycle events (accepted, rejected, cancelled, expired) and publishes them to a message bus. It owns the Write-Ahead Log.

**State it manages**

| Data | Storage | Notes |
|------|---------|-------|
| Order book per symbol | In-memory (`OrderBook` in `engine_thread.rs`) | Reconstructable from WAL replay |
| WAL (append-only command log) | Local disk / durable block store | Source of truth for crash recovery |
| Symbol registry | In-memory, loaded from Account Service at boot | Symbols added via gRPC admin call |
| Circuit breaker state | In-memory (`CircuitBreaker` per symbol) | Reset on restart unless persisted |
| Iceberg slice metadata | In-memory (`IcebergManager`) | Derived state, reconstructable from WAL |
| GTD expiry registry | In-memory | Reconstructable from WAL |

**What it does NOT own**

Balances, positions, user accounts, fees. It receives pre-validated, pre-margin-checked commands from Risk Service. It returns raw fill events; settlement consequences (PnL, balance updates, position mutation) are handled downstream.

**API contract — gRPC**

```protobuf
service MatchingService {
  // Submit a validated order for matching. Risk Service calls this.
  rpc PlaceOrder(PlaceOrderRequest) returns (PlaceOrderResponse);

  // Cancel an order by ID. Can be called by API Gateway or Risk Service.
  rpc CancelOrder(CancelOrderRequest) returns (CancelOrderResponse);

  // Amend an order (cancel + re-submit atomically within the engine thread).
  rpc AmendOrder(AmendOrderRequest) returns (AmendOrderResponse);

  // L2 order book snapshot at requested depth.
  rpc GetSnapshot(SnapshotRequest) returns (SnapshotResponse);

  // Register a new symbol (triggers EngineHandle::spawn).
  rpc AddSymbol(AddSymbolRequest) returns (AddSymbolResponse);

  // Admin: restart engine for a symbol (e.g. after WAL recovery).
  rpc RestartSymbol(RestartSymbolRequest) returns (RestartSymbolResponse);
}
```

Published events (NATS subjects):

```
matching.fills.{symbol}          — FillEvent (trade_id, maker_id, taker_id, price, qty, timestamp)
matching.order_accepted.{symbol} — OrderAcceptedEvent
matching.order_cancelled.{symbol}— OrderCancelledEvent
matching.order_rejected.{symbol} — OrderRejectedEvent
matching.book_update.{symbol}    — L2DiffEvent (for market data fanout)
```

**Dependencies**

- Reads nothing from other services at order-processing time (zero-dependency hot path).
- At startup: queries Account Service for the list of active symbols.
- Emits events consumed by Settlement Service, Market Data Service, Account Service.

**Scaling model**

Scale by symbol sharding. Each symbol's engine thread is entirely independent — no shared state across symbols. Shard the symbol namespace across Matching Service instances by consistent-hash on the symbol string. A symbol director (lightweight routing layer in the API Gateway or a sidecar) maps `symbol → matching-node-id`. Typical deployment: 1 Matching Service instance per 100–500 symbols, sized to keep each CPU core at <40% load.

Maximum per-node symbol count is bounded by available cores and RAM (each `OrderBook` holds up to `O(price_levels × orders)` state). At 100 symbols with 10K resting orders each, RAM footprint is well under 1 GB.

**Latency target**

End-to-end command → fill event: <200µs p99 within a single data center. The current crossbeam channel + synchronous matching path already achieves <100µs on a modern CPU. The main added latency in the service split is the gRPC call from Risk Service — this must be a direct TCP connection with no proxy on the hot path.

---

### 1.2 Account Service

**Responsibility**

All user account state: trading balances, fund (custody) balances, open positions, leverage settings, margin mode, KYC status, custody operations (deposits, withdrawals), compliance records, sub-accounts, referrals, sessions, API keys, and earned interest. This is the financial ledger owner.

**State it manages**

| Data | Storage | Notes |
|------|---------|-------|
| Trading balances per user/coin | PostgreSQL + in-memory cache | Double-entry ledger; cache is authoritative, DB is write-behind |
| Fund (custody) balances | PostgreSQL | Deposits arrive here; user transfers to trading |
| Open positions | PostgreSQL + in-memory | Written on fill settlement; position_db_ids side-map |
| Closed position PnL records | PostgreSQL | Append-only |
| User accounts (email, password hash, sessions) | PostgreSQL | Auth state |
| API keys | PostgreSQL | `ApiKeyStore` |
| KYC status, documents | PostgreSQL | `VerificationService` |
| Compliance state (sanctions, PEP, ToS) | PostgreSQL | `SanctionsScreener`, `PepScreener`, `TosManager` |
| Sub-accounts, transfers, referrals | PostgreSQL | |
| Earn/staking allocations | PostgreSQL + in-memory | |
| Custody records (addresses, whitelist) | PostgreSQL | `DepositManager`, `WithdrawalManager`, `AddressWhitelist` |
| User settings, notification prefs, devices | PostgreSQL | |
| Audit log | PostgreSQL (immutable append) | `AuditLog` |

**API contract — gRPC**

```protobuf
service AccountService {
  // Balance operations (called by Risk Service pre-trade and Settlement post-fill).
  rpc GetBalance(GetBalanceRequest) returns (GetBalanceResponse);
  rpc LockBalance(LockBalanceRequest) returns (LockBalanceResponse);     // reserve margin
  rpc UnlockBalance(UnlockBalanceRequest) returns (UnlockBalanceResponse);
  rpc Withdraw(WithdrawRequest) returns (WithdrawResponse);              // debit
  rpc Deposit(DepositRequest) returns (DepositResponse);                 // credit

  // Position operations (called by Settlement Service on fill events).
  rpc OpenPosition(OpenPositionRequest) returns (OpenPositionResponse);
  rpc ReducePosition(ReducePositionRequest) returns (ReducePositionResponse);
  rpc GetPositions(GetPositionsRequest) returns (GetPositionsResponse);
  rpc GetPosition(GetPositionRequest) returns (GetPositionResponse);

  // Margin queries (called by Risk Service).
  rpc GetCrossMarginBalance(CrossMarginBalanceRequest) returns (CrossMarginBalanceResponse);
  rpc GetEffectiveMargin(EffectiveMarginRequest) returns (EffectiveMarginResponse);

  // Auth (called by API Gateway).
  rpc ValidateApiKey(ValidateApiKeyRequest) returns (ValidateApiKeyResponse);
  rpc ValidateSession(ValidateSessionRequest) returns (ValidateSessionResponse);
  rpc GetKycStatus(GetKycStatusRequest) returns (GetKycStatusResponse);

  // Funding settlement (called by Settlement Service on funding tick).
  rpc ApplyFundingPayment(FundingPaymentRequest) returns (FundingPaymentResponse);

  // Liquidation balance operations (called by Settlement Service).
  rpc ApplyLiquidationSettlement(LiquidationSettlementRequest) returns (LiquidationSettlementResponse);
}
```

Published events (NATS subjects):

```
account.deposit.confirmed.{user_id}    — triggers push notification
account.withdrawal.processed.{user_id} — triggers push notification
account.kyc.approved.{user_id}         — consumed by API Gateway (cache invalidation)
account.frozen.{user_id}               — consumed by Risk Service (block further orders)
```

**Dependencies**

- Receives fill settlement commands from Settlement Service.
- Receives balance lock/unlock from Risk Service.
- Calls Custody Provider (external: Fireblocks, mg-custody) for deposit/withdrawal execution.
- Reads nothing from Matching Service directly.

**Scaling model**

Shard by user_id (consistent hash). Each shard owns a contiguous user_id range and its PostgreSQL partition. Balance mutations for a single user are serialized within a shard — this eliminates the main correctness risk of distributed account state. Read replicas for balance queries that don't need strict consistency (e.g., portfolio display). Risk Service requires a strongly-consistent balance read (for margin checks); it must always hit the primary shard replica.

Target: 10–20 shards, each handling ~100K active users. Add shards horizontally to scale user count.

**Critical invariant**

Every balance mutation must maintain `available >= 0`. This invariant is enforced inside the shard via optimistic locking on the PostgreSQL row (`UPDATE balances SET available = available - $1 WHERE user_id = $2 AND available >= $1`). A failed CAS returns an error that propagates back to Risk Service as `INSUFFICIENT_BALANCE`, which rejects the order before it ever reaches Matching.

---

### 1.3 Market Data Service

**Responsibility**

Aggregation and distribution of all derived market data: real-time tickers (24h rolling), OHLCV klines (all resolutions), L2 order book snapshots, recent trades, funding rate history, open interest history, long/short ratio, taker volume, index prices, premium index, mark prices. This service is read-only — it consumes events from Matching and Settlement and never initiates writes to financial state.

**State it manages**

| Data | Storage | Notes |
|------|---------|-------|
| 24h rolling tickers | In-memory (`MarketDataService`) | Rebuilt from fill stream on restart |
| Kline (OHLCV) buffers | In-memory + TimescaleDB / ClickHouse | In-memory for WS push; DB for historical queries |
| L2 snapshots | In-memory (derived from Matching book_update events) | Not persisted; rebuilt from event stream |
| Recent trades per symbol | In-memory ring buffer (10K per symbol) | |
| Funding rate history | TimescaleDB | Written by Settlement Service, read here |
| Index price feed | In-memory (`IndexPriceAggregator`) | Consumed from external feed |
| Mark price EMA | In-memory | Recalculated from index + fill events |
| OI / LS / taker-volume snapshots | In-memory + TimescaleDB | |

**API contract — REST + WebSocket** (no gRPC; this service is the public data plane)

```
GET  /v1/markets/{symbol}/orderbook    — L2 snapshot at depth
GET  /v1/markets/{symbol}/bbo          — best bid/offer
GET  /v1/markets/{symbol}/trades       — recent trades
GET  /v1/markets/{symbol}/klines       — OHLCV for a resolution + time range
GET  /v1/markets/{symbol}/ticker       — 24h stats
GET  /v1/markets/{symbol}/mark-price   — mark + index price
GET  /v1/markets                       — all tickers
GET  /v1/funding/rate                  — current funding rates
GET  /v1/funding/history/{symbol}      — historical funding payments
GET  /v1/markets/{symbol}/oi           — open interest history
GET  /v1/markets/{symbol}/long-short   — long/short ratio history

WebSocket /ws/v1:
  subscribe: { "op": "subscribe", "args": ["orderbook.1.BTCUSDT", "publicTrade.BTCUSDT", "kline.1.BTCUSDT"] }
  push: real-time L2 diffs, trades, kline updates, ticker updates
```

**Dependencies**

- Subscribes to: `matching.fills.*`, `matching.book_update.*`, `matching.order_accepted.*` from Matching Service.
- Subscribes to: `settlement.funding_rate.*`, `settlement.mark_price.*` from Settlement Service.
- Reads index prices from Index Price Aggregator (can be embedded or run as a sidecar).
- No writes to Account Service, Matching Service, or Settlement Service.

**Scaling model**

Stateless read path — scale horizontally behind a load balancer. Each instance maintains its own in-memory state rebuilt from the event stream. Use NATS JetStream consumer groups so each Market Data instance gets the full event feed independently (fan-out, not competing consumers). WebSocket connections are sticky to a single instance via consistent-hash load balancing on session ID. At 100K concurrent WS subscribers, 4–8 instances with 8 CPU cores each will handle the fanout comfortably.

**Latency target**

Trade fill to WebSocket client push: <5ms. The current monolith achieves <10ms. In the service split, the additional hop is one NATS publish + NATS deliver, which adds ~0.5ms on a co-located broker.

---

### 1.4 Settlement Service

**Responsibility**

All post-fill financial consequences: PnL settlement on fills, position open/close/reduce (via Account Service), funding rate calculation and payment, insurance fund management, ADL (auto-deleveraging) execution, liquidation fee collection, mark price computation, and the premium index TWAP buffer. This service is event-driven and consumes fill events from Matching Service.

**State it manages**

| Data | Storage | Notes |
|------|---------|-------|
| Insurance fund balance per symbol | PostgreSQL | `InsuranceFund`; append-only event log |
| Funding rate history per symbol | PostgreSQL + in-memory | `FundingHistory` |
| Funding scheduler (next payment time) | In-memory + PostgreSQL | `FundingScheduler` |
| Premium TWAP buffers | In-memory | `PremiumBuffer`; rebuilt from recent mark price feed on restart |
| Mark price EMA state | In-memory | Reset on restart (EMA converges quickly) |
| Perpetual configs (MMR schedules, funding params) | PostgreSQL + in-memory cache | |
| Insurance fund event log | PostgreSQL (append-only) | `InsuranceFundHistoryEntry` |

**Background tasks owned by this service**

| Task | Interval | Description |
|------|----------|-------------|
| Fill settlement processor | Event-driven | Consumes fill events; calls Account Service to settle PnL |
| Funding rate calculator | 1 min sampling, 8h payment | Computes premium index; pays/collects funding at 8h intervals |
| Mark price updater | 1s | Updates EMA; broadcasts to Market Data |
| Insurance fund snapshotter | 1h | Records hourly balance snapshot |
| ADL executor | Triggered by liquidation event | Ranks profitable positions; reduces them against socialised loss |
| Liquidation fee collector | On liquidation event | Debits user; credits insurance fund |

**API contract — gRPC** (called by API Gateway for read endpoints)

```protobuf
service SettlementService {
  // Funding rate queries (public).
  rpc GetCurrentFundingRate(FundingRateRequest) returns (FundingRateResponse);
  rpc GetFundingHistory(FundingHistoryRequest) returns (FundingHistoryResponse);

  // Insurance fund queries (admin/public).
  rpc GetInsuranceFundBalance(InsuranceFundRequest) returns (InsuranceFundResponse);
  rpc GetInsuranceFundHistory(InsuranceFundHistoryRequest) returns (InsuranceFundHistoryResponse);

  // Mark price queries.
  rpc GetMarkPrice(MarkPriceRequest) returns (MarkPriceResponse);
  rpc GetAllMarkPrices(Empty) returns (AllMarkPricesResponse);

  // Perpetual config (consumed by Risk Service).
  rpc GetPerpConfig(PerpConfigRequest) returns (PerpConfigResponse);
  rpc GetAllPerpConfigs(Empty) returns (AllPerpConfigsResponse);
}
```

Published events (NATS subjects):

```
settlement.fill_settled.{symbol}    — SettleFillEvent (user_id, pnl, fee, position_delta)
settlement.funding_rate.{symbol}    — FundingRateEvent (rate, timestamp, next_payment)
settlement.mark_price.{symbol}      — MarkPriceEvent (price, index_price, basis)
settlement.liquidation.{symbol}     — LiquidationEvent (user_id, qty, price, is_partial)
settlement.adl.{symbol}             — AdlEvent (user_id, qty, price)
```

**Dependencies**

- Subscribes to: `matching.fills.*` from Matching Service.
- Calls Account Service to: settle PnL (credit/debit USDT), update positions, apply funding payments, debit liquidation fees.
- Reads perpetual configs from its own PostgreSQL store (authoritative owner).
- Reads mark prices from its own EMA calculation.
- Publishes mark prices consumed by Risk Service (for margin checks) and Market Data.

**Single-leader constraint**

Settlement must run as a single active instance per symbol shard. Concurrent settlement processors for the same symbol would create race conditions on insurance fund accounting and ADL ranking. Use NATS JetStream queue groups with exactly-one consumer per symbol shard. Leader election via lease heartbeat (etcd or NATS KV) ensures only one Settlement instance processes fills for a given symbol at any time.

**Fill settlement atomicity**

The current monolith has a correctness risk: `settle_futures_fill` calls `accounts.withdraw`, `positions.reduce`, and `insurance_funds` in separate Mutex acquisitions with no transaction boundary. In the service split, Settlement Service must issue a two-phase settlement:

1. Call Account Service `ApplyLiquidationSettlement` (single gRPC call that atomically: debits margin, credits PnL, and updates position) — Account Service handles this as a single PostgreSQL transaction.
2. On failure (e.g. Account Service returns error), publish a compensating event to the dead-letter subject for manual review. Never silently swallow financial errors.

---

### 1.5 Risk Service

**Responsibility**

Pre-trade risk enforcement: margin validation, balance sufficiency check, STP (self-trade prevention), rate limiting, circuit breaker checks, KYC gate, position limit enforcement, and order validation. This service sits on the critical latency path between API Gateway and Matching Service. It must complete its checks in <1ms p99.

**State it manages** (mostly read-only, cached locally)

| Data | Source | Cache TTL | Notes |
|------|--------|-----------|-------|
| User balance (available) | Account Service | 0ms (synchronous query) | Must be fresh; no stale reads for margin |
| Open positions per user | Account Service | 0ms (synchronous query) | Required for reduce-only / position limit checks |
| Mark prices | Settlement Service event stream | <1s | Subscribed via NATS, held in memory |
| Perpetual configs (MMR, leverage limits) | Settlement Service | 30s | Refreshed periodically |
| KYC status per user | Account Service | 60s | Cached; invalidated on `account.kyc.approved` event |
| Frozen accounts set | Account Service | 0ms (event-driven invalidation) | Subscribed to `account.frozen.*` |
| Circuit breaker state | Matching Service | 0ms (event-driven) | Subscribed to matching circuit break events |
| Rate limiter state | Local in-memory (`OrderRateLimiter`) | n/a | Per-user sliding window; not persisted |
| STP guard | Local in-memory (`SelfTradeGuard`) | n/a | Per-session state |

**API contract — gRPC** (called synchronously by API Gateway)

```protobuf
service RiskService {
  // Full pre-trade check: margin, STP, rate limit, KYC, position limits.
  // Returns either Approved (with locked margin amount) or Rejected (with reason).
  rpc CheckOrder(CheckOrderRequest) returns (CheckOrderResponse);

  // Release a locked margin reservation (on cancel or rejection after lock).
  rpc ReleaseMarginLock(ReleaseMarginLockRequest) returns (ReleaseMarginLockResponse);

  // Portfolio margin check for users with portfolio margin enabled.
  rpc CheckPortfolioMargin(PortfolioMarginRequest) returns (PortfolioMarginResponse);
}
```

**Internal flow for CheckOrder**

```
1. Validate order fields (OrderValidator) — pure computation, ~1µs
2. Check KYC gate (cached) — ~1µs cache hit
3. Check frozen/halted status (cached) — ~1µs cache hit
4. Check rate limit (local in-memory) — ~1µs
5. Check circuit breaker for symbol (cached from event stream) — ~1µs
6. Fetch current mark price (in-memory) — ~1µs
7. Call Account Service GetBalance (gRPC) — ~200µs target
8. Compute required margin (pure math, rust_decimal) — ~5µs
9. Call Account Service LockBalance (gRPC) — ~200µs target
10. Check STP (local in-memory SelfTradeGuard) — ~1µs
11. Check position limits (local cache + Account Service GetPositions if miss) — ~200µs
Total: ~600µs typical, <1ms p99 with Account Service collocated in same AZ
```

**Collocation requirement**

Risk Service must be deployed in the same availability zone as Account Service and Matching Service. The two synchronous gRPC calls (GetBalance + LockBalance) account for the bulk of latency. Cross-AZ gRPC adds 1–3ms and would push total risk check latency above 1ms. Use Kubernetes node affinity to pin Risk, Account, and Matching pods to the same AZ.

**Dependencies**

- Calls Account Service synchronously: `GetBalance`, `LockBalance`, `GetPositions`, `GetCrossMarginBalance`.
- Reads Settlement Service events: mark prices, perp configs (subscribed via NATS).
- Reads Account Service events: `account.frozen.*`, `account.kyc.approved.*` (for cache invalidation).
- Calls Matching Service: not directly — Risk Service approves the order and API Gateway forwards it to Matching.

**Scaling model**

Risk Service is stateless except for local caches (mark prices, perp configs, frozen sets, rate limiters). Rate limiter state is local per-instance — acceptable because rate limits are per-API-key and clients are sticky to an instance via consistent-hash on `user_id`. If stickiness is broken (e.g. instance restarts), the sliding window resets — this is acceptable; the window refills within seconds and there is no security risk, only a brief window of excess requests.

Scale horizontally: add instances behind an L7 load balancer. Each instance handles ~10K orders/sec (bounded by the two gRPC roundtrips to Account Service). For 100K orders/sec: 10 Risk Service instances sharing load.

---

## 2. Communication Patterns

### 2.1 Synchronous: API Gateway → Risk Service → Account Service

The order placement hot path is fully synchronous. Latency must be minimised at every step.

```
Client
  │  HTTP POST /v1/orders
  ▼
API Gateway (axum)
  │  gRPC PlaceOrder (validates auth + parses request)
  ▼
Risk Service
  │  gRPC GetBalance + LockBalance  (sequential, same AZ)
  ▼
Account Service (shard for user_id)
  │  returns locked_amount
  ▼
Risk Service  (returns CheckOrderResponse: Approved + locked_amount)
  ▼
API Gateway
  │  gRPC PlaceOrder (forwards validated+approved command)
  ▼
Matching Service (engine thread for symbol)
  │  returns PlaceOrderResponse: order_id + accepted/rejected
  ▼
API Gateway
  │  HTTP 200 {"orderId": "...", "status": "New"}
  ▼
Client
```

Total synchronous chain: API Gateway → Risk → Account → Risk → API Gateway → Matching → API Gateway. Target: <2ms p99 for the full REST round-trip from the exchange's load balancer.

### 2.2 Asynchronous: Matching → Settlement + Market Data

Fill events are fire-and-forget from Matching's perspective. Matching publishes to NATS immediately after the engine thread returns, without waiting for settlement confirmation.

```
Matching Service engine thread
  │  produces: Vec<MatchEvent>  (Fill, OrderAccepted, OrderCancelled, ...)
  │
  ▼  (NATS publish, non-blocking)
NATS JetStream
  │
  ├──► Settlement Service  (NATS consumer: "settlement-fills-consumer")
  │      ├── Calls Account Service to settle PnL and update position
  │      ├── Updates insurance fund
  │      └── Publishes settlement.fill_settled for downstream consumers
  │
  └──► Market Data Service  (NATS consumer: "marketdata-fills-consumer")
         ├── Updates 24h ticker
         ├── Updates kline buffers
         ├── Updates trade history ring buffer
         └── Pushes to WebSocket subscribers
```

NATS JetStream ensures at-least-once delivery. Settlement Service must be idempotent on fill events (use `trade_id` as deduplication key in the settlement PostgreSQL table: `INSERT ... ON CONFLICT (trade_id) DO NOTHING`).

### 2.3 Pub/Sub: Settlement → Market Data + Risk

Settlement publishes derived prices and rates that other services consume:

```
Settlement Service
  │
  ├── NATS: settlement.mark_price.{symbol}  (every 1s)
  │       └──► Risk Service (in-memory cache for margin calculations)
  │       └──► Market Data Service (serves mark price REST endpoint)
  │
  ├── NATS: settlement.funding_rate.{symbol}  (on rate change)
  │       └──► Market Data Service
  │       └──► API Gateway (for WebSocket funding push to subscribers)
  │
  └── NATS: settlement.liquidation.{symbol}  (on liquidation event)
          └──► Market Data Service (public liquidation feed)
          └──► API Gateway (private WS push to liquidated user)
```

### 2.4 Request/Reply: API Gateway → Account (for read endpoints)

REST endpoints that serve account data (balances, positions, trade history, funding history) call Account Service via gRPC request/reply. These calls are not on the order placement hot path and have a more relaxed latency budget (<50ms p99 for REST).

```
Client
  │  GET /v1/account/balances
  ▼
API Gateway
  │  gRPC GetBalance (Account Service)
  ▼
Account Service (primary replica for user's shard)
  │  returns balances
  ▼
API Gateway → Client
```

For endpoints that aggregate across services (e.g. wallet overview = balances + positions + mark prices), API Gateway issues parallel gRPC calls to Account Service and Market Data Service, then joins the results before responding. This is a scatter-gather pattern; the timeout for each sub-call should be 20ms so the total stays under 50ms.

### 2.5 WebSocket Private Events

Private events (fills, position updates, liquidation warnings, funding payments) are pushed to connected WebSocket clients. In the service split, these events flow:

```
Settlement Service  (publishes to NATS: settlement.fill_settled.{user_id})
  │
  ▼
API Gateway  (NATS subscriber → route to WS connection for user_id)
  │
  ▼
WebSocket client (PrivateEvent::Fill, PrivateEvent::PositionUpdate, ...)
```

API Gateway maintains the mapping from user_id to WebSocket connection. It subscribes to all relevant NATS private event subjects on startup and routes to the correct WS connection. If the user is not connected, the event is dropped (clients reconnect and use the REST history endpoint to catch up, exactly like Bybit's private stream model).

---

## 3. Data Ownership Matrix

This table is the authoritative source for "which service is the writer" for each data domain. A service that needs to read data it does not own must call the owning service via gRPC or consume its events via NATS. No service may write to another service's database directly.

| Data Domain | Owner | Other Readers |
|-------------|-------|---------------|
| Order book (live) | Matching Service | Market Data (via book_update events) |
| Fill events | Matching Service (producer) | Settlement, Market Data, Account (via NATS) |
| WAL | Matching Service | (recovery only) |
| Trading balances | Account Service | Risk (via gRPC), Settlement (via gRPC) |
| Positions | Account Service | Risk (via gRPC), Settlement (via gRPC), Market Data (via settlement events) |
| PnL records | Account Service | Settlement (writes via gRPC) |
| Insurance fund | Settlement Service | Market Data (via events), Admin |
| Funding rates | Settlement Service | Market Data (via events), Account (via events for payments) |
| Mark prices | Settlement Service | Risk (via NATS), Market Data (via NATS) |
| Perp configs | Settlement Service | Risk (via gRPC), Matching (read-only at startup) |
| Klines / tickers | Market Data Service | (read-only consumers via REST/WS) |
| KYC status | Account Service | Risk (cached), API Gateway (auth) |
| Audit log | Account Service | Admin API |
| User sessions / JWT | Account Service | API Gateway (validates via gRPC) |
| Sanctions lists | Account Service (Compliance) | (read-only; Admin updates) |

---

## 4. Order Lifecycle: End-to-End Data Flow

### 4.1 Limit Order Placement

```
1.  Client sends POST /v1/orders (HTTP, authenticated)

2.  API Gateway:
    - Authenticates (gRPC ValidateApiKey to Account Service, cached)
    - Parses + validates request shape (local, ~1µs)
    - Calls Risk Service gRPC CheckOrder

3.  Risk Service:
    - Validates fields (OrderValidator, local)
    - Checks KYC cache, frozen cache (local, ~1µs each)
    - Checks rate limit (local in-memory, ~1µs)
    - Checks circuit breaker cache (local, ~1µs)
    - gRPC GetBalance → Account Service (~200µs)
    - Computes required margin (local, ~5µs)
    - gRPC LockBalance → Account Service (~200µs)
    - Returns CheckOrderResponse::Approved { locked_amount }

4.  API Gateway:
    - Writes command to WAL (via gRPC WriteCommand → Matching Service, durable)
    - gRPC PlaceOrder → Matching Service

5.  Matching Service (engine thread for symbol):
    - Receives PlaceOrderCommand via crossbeam channel (<1µs)
    - Runs price-time FIFO matching
    - Returns Vec<MatchEvent> (OrderAccepted + optionally Fill events)
    - Publishes events to NATS subjects (non-blocking)

6a. If no fill (resting order):
    - API Gateway returns HTTP 200 { "orderId": "...", "status": "New" }
    - Matching Service event: matching.order_accepted.BTCUSDT

6b. If fill(s):
    - API Gateway returns HTTP 200 { "orderId": "...", "status": "Filled"/"PartiallyFilled" }
    - Matching Service publishes: matching.fills.BTCUSDT (FillEvent per fill)

7.  Settlement Service (consumes matching.fills.BTCUSDT):
    - For each FillEvent: computes PnL, fees, position delta
    - gRPC ApplyLiquidationSettlement / settle equivalent → Account Service
    - Account Service: updates balance, updates/creates position, records trade in DB
    - Settlement publishes: settlement.fill_settled.BTCUSDT

8.  Market Data Service (consumes matching.fills.BTCUSDT in parallel with step 7):
    - Updates 24h ticker
    - Updates kline buffers (all resolutions)
    - Appends to trade history ring buffer
    - Pushes L2 diff to orderbook WebSocket subscribers
    - Pushes trade update to publicTrade WebSocket subscribers

9.  API Gateway (consumes settlement.fill_settled.{user_id} via NATS):
    - Pushes PrivateEvent::Fill to maker's and taker's WebSocket connections
    - Pushes PrivateEvent::OrderUpdate to both users
```

### 4.2 Liquidation Flow

```
Settlement Service (liquidation tick, every 500ms):
│
├── gRPC GetAllPositions → Account Service (all open positions)
├── Reads mark prices (in-memory cache from settlement.mark_price events)
├── LiquidationEngine.check_all_positions(positions, mark_prices, perp_configs)
│
│   For each position requiring liquidation:
│   ├── gRPC PlaceOrder → Matching Service (market IOC, reduce_only)
│   ├── Matching Service returns Fill events
│   ├── Compute insurance fund delta (fill_price vs bankruptcy_price)
│   ├── Update insurance fund balance (local state, written to PostgreSQL)
│   ├── If socialised_loss > 0: run ADL
│   │     ├── gRPC GetPositions → Account Service (re-read live positions for ADL rank)
│   │     ├── Rank by profit_ratio × leverage (AdlRanking)
│   │     ├── For each ADL target: gRPC ReducePosition → Account Service
│   │     └── Publish settlement.adl.{symbol}
│   ├── gRPC ApplyLiquidationSettlement → Account Service
│   │     (debits margin, credits released amount to user, records liquidation fee)
│   └── Publish settlement.liquidation.{symbol}
│
└── API Gateway (subscribed to settlement.liquidation.{user_id}):
        └── Pushes PrivateEvent::Liquidation to user's WS connection
```

### 4.3 Funding Rate Payment Flow

```
Settlement Service (every 8 hours at 00:00, 08:00, 16:00 UTC):
│
├── Computes funding rate:
│     funding_rate = premium_index_8h_twap + clamp(interest_rate − premium_index, −0.05%, +0.05%)
│     (Reference: Bybit perpetual swap docs, clamped ±0.05%)
│
├── gRPC GetAllPositions → Account Service (all open positions for this symbol)
│
├── For each open position:
│   ├── payment = position_size × mark_price × funding_rate × sign(long=pay, short=receive)
│   ├── gRPC ApplyFundingPayment → Account Service
│   │     (debits or credits USDT balance; tracks debt if balance insufficient)
│   └── Emit settlement.funding_rate.{symbol} event
│
└── Publish FundingRateRecord to PostgreSQL (funding history)
```

---

## 5. Technology Choices

### 5.1 Inter-Service Communication

**gRPC (tonic) for synchronous RPC**

Used on the latency-critical order path: API Gateway → Risk Service, Risk Service → Account Service (balance check/lock), API Gateway → Matching Service. Rationale: binary framing (protobuf), multiplexed HTTP/2, native Rust support via `tonic`. The synchronous latency budget of <1ms for pre-trade checks mandates gRPC over REST.

Service definitions live in a shared `crates/proto/` crate, compiled to Rust by `tonic-build` in `build.rs`. Each service crate adds `exchange-proto` as a dependency.

**NATS JetStream for event streaming**

Used for all fill events, settlement events, market data events, and private notifications. Rationale: NATS is an order of magnitude lower latency than Kafka (<1ms vs 5–10ms at moderate load), has a simpler operational model, supports exactly-once processing via JetStream consumer sequences, and has first-class Rust support via `async-nats`. JetStream streams provide durable message replay, which is required for Settlement Service to catch up after a restart.

Stream configuration:
- `MATCHING_FILLS`: retention = limits (7 days), max_bytes = 10 GB per symbol shard, replicas = 3
- `SETTLEMENT_EVENTS`: retention = limits (7 days), replicas = 3
- `MARKET_DATA_EVENTS`: retention = limits (1 day), replicas = 1 (Market Data can rebuild from fills)

Kafka is explicitly avoided: it introduces ZooKeeper/KRaft operational complexity for no latency benefit in this deployment scale (100K events/sec is well within NATS JetStream's envelope).

### 5.2 Service Discovery

Kubernetes DNS-based service discovery. Each service exposes a stable `ClusterIP` service (`matching-service.exchange.svc.cluster.local`). For symbol-sharded services (Matching, Account), a custom routing layer in API Gateway maps symbol/user_id to pod index using consistent hash. The pod index is encoded as a deterministic DNS subdomain (`matching-0.matching-service`, `matching-1.matching-service`, etc.) using StatefulSets.

No service mesh (Envoy/Istio) in the initial decomposition — it adds latency and operational complexity. Mutual TLS between services is handled at the application layer via tonic channel credentials.

### 5.3 Observability

**OpenTelemetry distributed tracing** with Jaeger backend. Every inbound gRPC and HTTP request is traced. The trace context is propagated via gRPC metadata and NATS message headers. Key spans to instrument:

- API Gateway: `order.place` (full end-to-end span)
  - Child: `risk.check_order` (Risk Service)
    - Child: `account.get_balance` (Account Service)
    - Child: `account.lock_balance` (Account Service)
  - Child: `matching.place_order` (Matching Service)
- Settlement Service: `settlement.process_fill` (triggered by NATS consumer)
  - Child: `account.settle_fill` (Account Service)

Latency SLO alerts:
- `risk.check_order` p99 > 800µs → PagerDuty
- `matching.place_order` p99 > 200µs → PagerDuty
- `settlement.process_fill` lag > 100ms → PagerDuty (settlement falling behind)

**Prometheus + Grafana** for infrastructure metrics: order throughput, fill rate, WebSocket connection count, NATS consumer lag, gRPC error rates per method.

**Structured logging** with `tracing` crate (already used throughout the monolith). Logs are shipped to Loki. Correlation between logs and traces is by `trace_id` injected into the tracing span context.

---

## 6. Migration Path

The migration proceeds in three phases. Each phase delivers a running, tested deployment before the next begins. No big-bang cutovers.

### Phase A: Extract Market Data Service (Weeks 1–4)

**Risk level: LOW.** Market Data is read-only and has no financial state. A bug here causes display errors, not money loss.

**Steps:**

1. **Create `crates/market-data-service/` binary crate.** Move `MarketDataService` from `api-gateway/src/exchange.rs` into it. The struct already exists in `crates/market-data/`; this step wraps it in an HTTP + WebSocket server.

2. **Wire NATS publisher in Matching Service.** After the engine thread returns `Vec<MatchEvent>`, publish `FillEvent` and `BookUpdateEvent` to NATS. This is additive — the monolith continues to use the existing `broadcast::Sender<MarketEvent>` for WS clients in parallel.

3. **Wire NATS subscriber in Market Data Service.** The new service consumes `matching.fills.*` and rebuilds ticker/kline state. Run it in shadow mode (receives events, builds state, but API Gateway still uses the monolith's `MarketDataService` for responses).

4. **Traffic shadow testing.** For two weeks, compare Market Data Service responses to monolith responses for the same requests. Verify kline values, 24h tickers, and trade history match within acceptable rounding tolerance.

5. **Cut over public endpoints.** Route `/v1/markets/*` and the public WebSocket topics (`orderbook.*`, `publicTrade.*`, `kline.*`) to the new Market Data Service. Keep monolith for private endpoints. Monitor for 48 hours.

6. **Remove `market_data: Mutex<MarketDataService>` from `Exchange` struct.** Replace internal calls to `exchange.market_data` with local state in the monolith that will be removed when the monolith is eventually retired.

**Acceptance criteria:**
- Market Data Service serves 100% of public market data traffic for 72 hours with zero incidents.
- Kline values match reference exchange (Bybit) within 0.01% for the last 24h candle.
- WebSocket fanout latency (fill → WS push) < 5ms p99.

---

### Phase B: Extract Account + Settlement Services (Weeks 5–16)

**Risk level: MEDIUM.** These services own financial state. The primary risk is the settlement atomicity gap described in section 1.4.

**Steps:**

1. **Introduce PostgreSQL as the authoritative balance store.** The monolith already has a `db_pool` field and write-behind logic. Complete the migration to make PostgreSQL the primary store: all balance reads go to DB (via read replica for non-critical paths, primary for margin checks). This step is entirely within the monolith.

2. **Extract `AccountService` gRPC server.** Implement the gRPC contract from section 1.2. Initially, the Account Service is a thin wrapper around the existing `AccountManager` + `PositionManager` crates backed by PostgreSQL. No logic changes — only the network boundary changes.

3. **Dual-write period.** Monolith continues to write to its in-memory state AND calls Account Service gRPC for all balance mutations. Account Service writes to the shared PostgreSQL DB. Verify consistency by running reconciliation scripts comparing in-memory state to DB state every 5 minutes.

4. **Extract `SettlementService`.** Move funding scheduler, insurance fund, mark price EMA, and fill settlement logic into the new service. Wire it as a NATS consumer on `matching.fills.*`. During transition, Settlement Service runs in parallel with the existing `spawn_funding_task` and `spawn_liquidation_monitor` background tasks in the monolith — the monolith tasks are the authoritative actors, Settlement Service runs in shadow mode.

5. **Switch settlement authority to Settlement Service.** Disable `spawn_funding_task` and `spawn_liquidation_monitor` in the monolith. Settlement Service becomes the sole settlement actor. This is the highest-risk step — run in off-peak hours with a rollback plan (re-enable monolith tasks within 60 seconds via feature flag).

6. **Switch balance authority to Account Service.** Disable in-memory `AccountManager` in the monolith. All balance reads/writes go through Account Service gRPC.

7. **Smoke test financial invariants:**
   - No money created from nothing: `sum(all balances) + sum(locked_margin) + sum(insurance_funds) + treasury = constant` (verify before and after each large test).
   - `available >= 0` never violated.
   - Fills produce exactly one settlement event per fill (no duplicates, no misses).

**Acceptance criteria:**
- 24-hour soak test with simulated order flow at 10K orders/sec.
- Zero balance invariant violations in soak test.
- Settlement lag (fill → balance update) < 200ms p99.
- Full end-to-end order placement latency regression < 20% from monolith baseline.

---

### Phase C: Extract Matching + Risk Services (Weeks 17–24)

**Risk level: HIGH.** This phase changes the order placement hot path. Latency regressions directly impact market makers and latency-sensitive users.

**Steps:**

1. **Extract `RiskService` as a collocated sidecar.** Initially deploy Risk Service on the same Kubernetes node as the API Gateway pod (sidecar pattern via `hostNetwork: true`). This ensures gRPC calls to Risk Service have <100µs overhead (loopback or shared memory transport).

2. **Route a canary 1% of order traffic through Risk Service.** API Gateway uses a feature flag to send 1% of PlaceOrder calls through the new Risk → Account → Matching path instead of the monolith path. Monitor latency and error rate.

3. **Ramp canary to 10% → 50% → 100%** over three weeks, monitoring at each step. Roll back immediately if p99 latency exceeds 3ms or error rate exceeds 0.01%.

4. **Extract `MatchingService` binary.** The `EnginePool` + `EngineHandle` code is already self-contained. Wrap it in a gRPC server (tonic). The WAL moves with it to the Matching Service's local disk. Deploy alongside the monolith.

5. **Switch API Gateway's PlaceOrder path to use the new Matching Service.** Use the same canary ramp as step 3. This is the final cutover.

6. **Decommission the monolith binary.** After 2 weeks of stable 100% traffic through all five services, the monolith is shut down. The `Exchange` struct is archived.

**Latency validation protocol** (run before each canary ramp increase):
- Deploy a synthetic order sender at 1K orders/sec for the target symbol pair (BTCUSDT).
- Measure: p50, p95, p99, p999 of the full REST round-trip (client → response).
- Gate: p99 must be ≤ 2ms. p999 must be ≤ 10ms.
- Alert threshold: any single 1-minute window with p99 > 3ms triggers automatic rollback.

**Acceptance criteria:**
- 100% traffic through all five services for 7 consecutive days with no incidents.
- Order placement p99 ≤ 2ms (matching the monolith baseline).
- WAL replay tested: kill Matching Service pod mid-flight, verify it recovers and all in-flight orders are correctly resolved.
- Insurance fund balance before and after Phase C equals (within rounding): exact match required.

---

## 7. Operational Concerns

### 7.1 WAL Recovery Across Service Boundary

The WAL is the source of truth for the order book. In the monolith, WAL replay calls `exchange.place_order()` directly. In the service split, WAL replay in Matching Service must call Account Service to re-lock margin for recovered orders. This creates a startup dependency: Matching Service cannot complete WAL replay until Account Service is available.

Startup order: Account Service → Settlement Service → Risk Service → Matching Service → API Gateway.

Kubernetes `initContainers` with readiness probes enforce this order.

### 7.2 Distributed Transaction Gap

The monolith's fill settlement is not atomic (multiple Mutex acquisitions). The service split does not make this worse but it does make the failure modes more visible: if Account Service is unavailable during settlement, fills are acknowledged by Matching but not settled. Mitigation: Settlement Service uses NATS JetStream's `ack_wait` so unacknowledged fill events are redelivered. Settlement Service is idempotent on re-delivery via `trade_id` deduplication. Monitor `settlement_lag_seconds` metric; alert if > 5 seconds.

### 7.3 Balance Lock Expiry

Risk Service locks balance in Account Service before sending the order to Matching. If Matching rejects the order after the lock, Risk Service must call `ReleaseMarginLock`. If Risk Service crashes between locking and releasing, the balance remains locked indefinitely. Mitigation: Account Service applies a TTL of 30 seconds to all margin locks. Unconfirmed locks are released automatically after TTL expiry. Risk Service re-confirms each live lock every 10 seconds (heartbeat gRPC call).

### 7.4 Feature Flag Gating

The migration's canary routing is controlled by a feature flag in the `FeatureFlags` struct (already present on `Exchange`). Add flags:
- `use_external_risk_service: bool`
- `use_external_matching_service: bool`
- `use_external_account_service: bool`
- `use_external_settlement_service: bool`

These flags allow per-request routing decisions and instant rollback without redeployment.

### 7.5 Backward Compatibility During Migration

During Phases A–C, the monolith and the new services coexist. The monolith continues to serve most traffic while the new services are validated. To avoid double-processing of events (monolith AND Settlement Service both processing the same fill), use NATS consumer group names that partition responsibility cleanly: the monolith subscribes to `matching.fills.*` with consumer group `monolith-settlement`; the new Settlement Service subscribes with `settlement-service`. Both receive all events independently during the dual-write period. After cutover, the monolith consumer group is drained and deregistered.

---

## 8. What Remains in the Monolith During Migration

The following domains are lower priority for extraction (Phase 2 work after the five core services are stable):

| Domain | Current Location | Notes |
|--------|-----------------|-------|
| Compliance (KYC, AML, SAR, EDD) | `crates/compliance/` | No latency requirement; can be async microservice |
| Surveillance (spoofing, wash trading) | `crates/surveillance/` | Event-driven; processes fill stream |
| Copy trading | `crates/copy-trading/` | Depends on fill events + Account Service |
| Trading bots (grid, DCA) | `crates/trading-bots/` | Self-contained, low-frequency |
| Algo execution (TWAP, VWAP) | `crates/algo-execution/` | Submits orders via API Gateway loop |
| FIX gateway | `crates/fix-gateway/` | Stateless protocol adapter; wraps API Gateway |
| Proof of reserves | `crates/proof-of-reserves/` | Batch process, runs nightly |
| Admin / notifications | `crates/admin/` | Low volume; can stay in API Gateway |
| Market maker program | In-memory on Exchange | Move to Account Service when extracting |
| Options instruments | `crates/risk-engine/` | Future product; extract with Settlement |

The strategy is to stabilise the five core services first, validate the event streaming infrastructure in production, then progressively extract remaining domains using the same NATS + gRPC pattern.

---

## Appendix A: Crate-to-Service Mapping

| Existing Crate | Target Service | Migration Phase |
|----------------|----------------|----------------|
| `matching-engine` | Matching Service | Phase C |
| `journal` | Matching Service (internal) | Phase C |
| `order-gateway` | Risk Service | Phase C |
| `risk-engine` (liquidation, ADL, margin calc) | Settlement Service | Phase B |
| `risk-engine` (pre-trade margin check) | Risk Service | Phase C |
| `settlement` | Settlement Service | Phase B |
| `account` | Account Service | Phase B |
| `market-data` | Market Data Service | Phase A |
| `market-maker` (SLA, toxicity) | Risk Service (toxicity) + Account Service (SLA records) | Phase C |
| `index-price` | Market Data Service (sidecar) | Phase A |
| `trigger-service` | Merged into Risk Service or standalone | Phase C |
| `surveillance` | Standalone Surveillance Service | Post-Phase C |
| `compliance` | Standalone Compliance Service | Post-Phase C |
| `custody` | Account Service (internal) | Phase B |
| `copy-trading` | Standalone | Post-Phase C |
| `trading-bots` | Standalone | Post-Phase C |
| `algo-execution` | Standalone | Post-Phase C |
| `fix-gateway` | Stays in API Gateway layer | — |
| `admin` | Stays in API Gateway (admin routes) | — |
| `common` | Shared library (all services) | — |
| `db` | Shared library (Account + Settlement) | — |

---

## Appendix B: Key Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Settlement atomicity violation (fill settled partially) | MEDIUM | HIGH | NATS JetStream at-least-once + idempotent settlement via trade_id dedup |
| Balance lock leak (Risk crashes after lock, before release) | LOW | MEDIUM | 30s TTL on all margin locks in Account Service |
| Matching Service WAL replay fails on restart | LOW | HIGH | Account Service must be available before Matching starts; use Kubernetes init containers + readiness probes |
| Mark price staleness causes wrong margin check | LOW | HIGH | Risk Service subscribes to NATS mark price updates; rejects orders if last update > 5s old |
| Cross-AZ gRPC latency blows p99 budget | MEDIUM | MEDIUM | Node affinity rules: Risk + Account + Matching in same AZ; measure before each Phase C canary increase |
| NATS broker failure causes settlement lag | LOW | MEDIUM | NATS JetStream clustering (3 replicas); Settlement Service reconnects and reprocesses from last ACK offset |
| Schema mismatch between protobuf and business logic after refactor | MEDIUM | MEDIUM | Proto definitions in shared `crates/proto/`; CI compiles all services together to catch mismatches |
