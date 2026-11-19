# Exchange Architecture Requirements

## Vision

Crypto exchange for the Ukrainian market. Spot + futures (perpetuals). Competitive advantage: Rust performance + market microstructure intelligence from bot-trader.

## Non-Functional Requirements

### Performance
- Matching engine latency: < 500µs p99 (limit order round-trip)
- Throughput: 100K+ orders/sec per symbol
- Market data latency: < 10ms from match to WebSocket client
- API response: < 50ms p99 for REST, < 5ms for WS push

### Reliability
- Zero data loss: every order durably persisted (WAL) before matching
- Crash recovery: deterministic replay from WAL
- Hot standby: matching engine failover < 1 second
- Balance invariant: `available >= 0` enforced at DB + app level

### Security
- No f64 for money — `rust_decimal` everywhere
- HMAC-SHA256 API key authentication
- Rate limiting per IP + per API key
- Self-trade prevention (configurable: cancel-newest or cancel-oldest)
- Segregated client funds

### Regulatory (Ukraine Virtual Assets Law)
- VASP registration with NCSSM
- KYC: Diia API for Ukrainian users, Sumsub for international
- AML: sanctions screening, STR to SFMS for >400K UAH
- Audit log: immutable, signed, 5-year retention
- Data residency: Ukraine or EU servers

## System Components

```
┌─────────────────────────────────────────────────────────┐
│                      CLIENTS                            │
│              Web / Mobile / API (FIX)                   │
└─────────────────────┬───────────────────────────────────┘
                      │
┌─────────────────────▼───────────────────────────────────┐
│               API GATEWAY (axum)                        │
│   Auth · Rate limit · TLS · REST + WebSocket            │
└─────────────────────┬───────────────────────────────────┘
                      │
┌─────────────────────▼───────────────────────────────────┐
│              ORDER GATEWAY                              │
│   Validation · Balance lock · Sequence numbering        │
│   Pre-trade risk checks · Self-trade prevention         │
└─────────────────────┬───────────────────────────────────┘
                      │
┌─────────────────────▼───────────────────────────────────┐
│              COMMAND JOURNAL (WAL)                       │
│   Append-only · NVMe · Crash recovery source            │
└─────────────────────┬───────────────────────────────────┘
                      │  SPSC ring buffer (per symbol)
┌─────────────────────▼───────────────────────────────────┐
│            MATCHING ENGINE                              │
│   Single thread per symbol · Zero alloc hot path        │
│   BTreeMap<Price, PriceLevel> · FIFO within level       │
│                                                         │
│   Order types: Limit, Market, IOC, FOK, PostOnly        │
│   Future: StopLimit, StopMarket, Iceberg                │
└─────────────────────┬───────────────────────────────────┘
                      │  SPMC ring buffer (events fan-out)
          ┌───────────┼───────────┬──────────────┐
          ▼           ▼           ▼              ▼
    ┌──────────┐ ┌──────────┐ ┌────────┐ ┌─────────────┐
    │Persist   │ │Market    │ │Risk    │ │Surveillance │
    │(Postgres │ │Data      │ │Engine  │ │(bot-trader  │
    │+ScyllaDB)│ │(WS push) │ │        │ │ core)       │
    └──────────┘ └──────────┘ └───┬────┘ └─────────────┘
                                  │
                          liquidation orders
                                  │
                                  ▼
                           ORDER GATEWAY
                           (re-entry)
```

## Crate Dependency Rules

- `matching-engine` → depends ONLY on `common`. Zero async, zero network, zero allocation in hot path.
- `surveillance` → depends on bot-trader `crates/core/` (git submodule)
- `risk-engine` → reads matching events, injects liquidation commands into order-gateway (NEVER directly into matching engine)
- `api-gateway` → depends on `account`, `market-data`. NEVER touches matching engine directly.
- `journal` → zero dependencies beyond common + filesystem. No async runtime.

## Technology Stack

| Layer | Choice | Rationale |
|-------|--------|-----------|
| Runtime | tokio | Industry standard, networking layer only |
| HTTP/WS | axum + tower | Type-safe, middleware ecosystem |
| DB (OLTP) | PostgreSQL + sqlx | ACID for balances, compile-time queries |
| DB (timeseries) | ScyllaDB | Trades, candles, funding history |
| Cache | Redis | Sessions, rate limits, L2 snapshots |
| Internal bus | crossbeam SPSC/SPMC | Lock-free, in-process, < 1µs |
| External bus | NATS JetStream (Phase 2) | When splitting into microservices |
| Serialization (hot) | flatbuffers/rkyv | Zero-copy for internal events |
| Serialization (API) | serde_json | Standard for REST/WS |
| Money | rust_decimal | NEVER f64 |
| Time | quanta (internal), chrono (API) | TSC nanoseconds in hot path |
| Metrics | prometheus | Grafana dashboards |
| Auth | JWT (web) + HMAC-SHA256 (API) | Standard exchange auth |

## Matching Engine Design Decisions

Based on gitbitex-new analysis:

1. **Fill at maker's price** — taker gets price improvement
2. **Market order remainder cancelled** — not left on book
3. **Single thread per symbol** — no locks, no races
4. **Price-time priority (FIFO)** — strict within price level
5. **Cancel is silent for non-existent orders** — no error response
6. **No order modify** (Phase 1) — cancel + replace only
7. **Integer ticks internally** (future optimization) — Decimal for now, migrate to i64 ticks when profiling shows need

## Phases

### Phase 1: Spot MVP
- Matching: Limit + Market + IOC + FOK + PostOnly
- Spot trading: 5-10 pairs (BTC, ETH, SOL, USDT)
- KYC via Diia + Sumsub
- Crypto deposit/withdrawal via Fireblocks (no own hot wallet)
- UAH on/off-ramp via bank integration (Monobank API?)
- Basic web UI
- VASP registration (parallel)

### Phase 2: Perpetual Futures
- Risk engine: cross + isolated margin
- Funding rate (8h settlement)
- Liquidation engine with insurance fund
- Mark price from index
- Stop orders: StopLimit, StopMarket, TakeProfit
- FIX 4.4 API for market makers
- Maker rebate program

### Phase 3: Institutional
- Options (European-style)
- Portfolio margin
- Copy trading
- Smart order routing (TWAP, VWAP from bot-trader)
- OTC desk with RFQ
- Mobile app (reuse MeatGrinder SwiftUI)
- Market surveillance API for regulators
