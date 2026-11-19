# Development Approach

## Methodology: Vertical Slice

Each feature is delivered as a full vertical slice: from API to matching engine to persistence to WebSocket push. No "build all persistence, then all API, then all WS" — that leads to integration hell.

### Phase 1 — Spot Exchange MVP (COMPLETE)

All 10 slices implemented. 124 tests passing. Single binary monolith.

| # | Slice | Tests | Key Components |
|---|-------|-------|----------------|
| 1 | Matching Engine | 14 | BTreeMap orderbook, price-time FIFO, Limit/Market/IOC/FOK/PostOnly |
| 2 | Account & Balances | 23 | lock/unlock/settle, fee calculation, invariant `available >= 0` |
| 3+4 | REST + WebSocket | 8 | axum, place/cancel/query orders, L2 snapshot, trade/book/private WS |
| 5 | Command Journal | 6 | WAL (append-only, fsync, deterministic replay, crash recovery) |
| 6 | Order Gateway | 20 | tick/lot size, min notional, self-trade prevention (CancelTaker) |
| 7 | Auth + Private WS | 7 | HMAC-SHA256 API keys, per-user WS event filtering |
| 8 | Admin + Metrics | 7 | Prometheus /metrics, /admin/stats, lock-free atomic counters |
| 9 | KYC + AML | 19 | State machine (None→Basic→Full), trading limits, smurfing/structuring detection |
| 10 | Crypto Custody | 20 | CustodyProvider trait, deposit addresses, withdrawal approval flow |

### Phase 2 — Production Readiness

Next steps to go from MVP to production:

#### P2.1: PostgreSQL Persistence
- [ ] sqlx migrations for: accounts, balances, orders, trades, kyc_records, aml_alerts
- [ ] Replace in-memory AccountManager with PostgreSQL-backed
- [ ] Replace in-memory KycManager with PostgreSQL-backed
- [ ] `SELECT FOR UPDATE` for balance mutations (row-level locking)
- [ ] CHECK constraints: `available >= 0`, `total >= locked`
- [ ] Read replicas for reporting/admin queries

#### P2.2: Fireblocks Integration
- [ ] Implement `CustodyProvider` for Fireblocks API
- [ ] Webhook handler for deposit detection
- [ ] Vault structure: hot/warm/cold segregation
- [ ] Auto-sweep from deposit addresses to omnibus wallet

#### P2.3: Diia KYC Integration
- [ ] Diia.Business API client for Ukrainian passport verification
- [ ] Sumsub SDK for international users
- [ ] Webhook handlers for verification status updates
- [ ] Auto-approve flow: Diia verified → KycStatus::Verified

#### P2.4: Frontend
- [ ] React web app (or reuse electron-app patterns from bot-trader)
- [ ] Trading terminal: orderbook, chart, trade form
- [ ] Account management: balances, deposit/withdraw, order history
- [ ] Admin dashboard: metrics, user management, AML alerts

#### P2.5: Production Infrastructure
- [ ] Docker + docker-compose for local dev
- [ ] CI/CD pipeline (GitHub Actions: test + clippy + build)
- [ ] Bare-metal deployment scripts (Ukrainian DC)
- [ ] TLS termination (nginx or built-in rustls)
- [ ] Rate limiting (tower-governor, per-IP + per-API-key)
- [ ] Backup DC in Frankfurt

### Phase 3 — Perpetual Futures

- [ ] Risk engine: cross + isolated margin
- [ ] Funding rate (8h settlement)
- [ ] Liquidation engine with insurance fund
- [ ] Mark price from index
- [ ] Stop orders: StopLimit, StopMarket, TakeProfit
- [ ] FIX 4.4 API for market makers

## Principles

### 1. Test-Driven at Every Level
- Unit tests: matching engine logic, balance arithmetic
- Integration tests: full HTTP → match → WS flow
- Property-based tests: balance invariants (proptest)
- Load tests: matching engine throughput (criterion benchmarks)
- Chaos tests: kill processes, verify WAL recovery

### 2. Correctness Over Performance
- Get it RIGHT first, then make it FAST
- Profile before optimizing (criterion + flamegraph)
- The matching engine is already fast enough for Phase 1 (~100K ops/sec with Decimal)
- Migrate to integer ticks only when profiling shows Decimal is the bottleneck

### 3. No Premature Microservices
- Phase 1: single binary (monolith)
- All crates compile into one process
- Internal communication via crossbeam channels (not network)
- Split into services ONLY when scaling demands it (Phase 2+)

### 4. Database Migrations Are Code
- sqlx migrations checked into git
- Every schema change is a new migration file
- `sqlx::query!` for compile-time checked queries
- No ORM — raw SQL is more transparent for financial systems

### 5. Feature Flags for Gradual Rollout
- Futures trading behind feature flag
- New order types behind feature flag
- Surveillance actions (auto-restrict) behind feature flag
- Allows testing in production without full exposure

## Git Workflow

- `main` — always deployable
- `feat/*` — feature branches, squash-merge to main
- `fix/*` — bug fixes
- CI: `cargo test`, `cargo clippy`, `cargo fmt --check` on every PR

## Commands

```bash
# Development
cargo build                            # Build all crates
cargo test                             # Run all tests (124)
cargo test -p exchange-matching-engine # Test specific crate
cargo bench -p exchange-matching-engine # Benchmarks

# Run exchange
cargo run -p exchange-server           # Start on 0.0.0.0:8080

# Database (Phase 2)
sqlx migrate run                       # Apply migrations
sqlx prepare                           # Generate offline query data

# Quality
cargo clippy -- -D warnings            # Lint
cargo fmt --check                      # Format check
```
