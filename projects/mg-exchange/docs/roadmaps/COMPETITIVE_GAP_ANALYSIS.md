# Competitive Gap Analysis & Development Roadmap

**Date:** 2026-03-27
**Author:** PM / Tech Lead
**Scope:** Full feature gap analysis against Bybit, Binance, OKX, Bitget, KuCoin, Hyperliquid

---

## Executive Summary

The exchange has reached a genuinely impressive baseline: ~456 REST routes, 29 background crates, a production-quality matching engine (450K orders/sec, 109 tests), full order type coverage, comprehensive surveillance, and 2,086 tests passing. The backend logic for roughly 30 distinct products is functional.

What it does not have yet is the operational infrastructure to handle real money and real users. The gaps fall into four buckets:

1. **Custody gap** — no real blockchain integration. Every deposit and withdrawal is simulated. This is the single biggest P0 blocker.
2. **External integration gap** — sanctions feeds, KYC providers, and travel rule VASPs are stubs. Good logic, no data.
3. **Mobile / UX completeness gap** — no native mobile app, several web features are frontend-only stubs without backend routes wired.
4. **Competitive feature gap** — roughly 15-20 features that top-4 competitors offer that we have not yet built at all.

The overall verdict: the exchange is ready for a limited testnet launch, is 4-6 months away from a safe production launch with real custody, and is 12-18 months away from full competitive parity with Bybit/OKX.

---

## Section 1: What We Have (Honest Inventory)

### 1.1 Trading Products

| Product | Backend Status | Frontend Status | Notes |
|---------|---------------|----------------|-------|
| Spot Trading | FUNCTIONAL | DONE | No double-entry ledger in matching path (tracked) |
| USDT-M Perpetual Futures | FUNCTIONAL | DONE | Index feed health gate active |
| COIN-M Inverse Perpetuals | FUNCTIONAL | DONE | Same health gate |
| Delivery Futures | PARTIAL | DONE (page) | Settlement logic exists; no edge-case tests for expiry |
| Options (European, USDC/USDT) | PARTIAL | DONE (page) | Vol surface wired; needs order book and full margin integration |
| Spot Margin Trading | FUNCTIONAL | DONE | Cross-asset collateral with haircuts |
| Leveraged Tokens | STUB | DONE (page) | REST handlers return placeholder data |

### 1.2 Order Types

| Order Type | Backend | Frontend | Notes |
|-----------|---------|---------|-------|
| Limit / Market | COMPLETE | DONE | Price-time FIFO, 450K/sec |
| IOC / FOK / PostOnly / GTC / GTD | COMPLETE | DONE | All TIF variants |
| Stop-Limit / Stop-Market | COMPLETE | DONE | Via trigger service |
| Take-Profit (Limit + Market) | COMPLETE | DONE | Bracket TP leg |
| Trailing Stop (fixed + %) | COMPLETE | DONE | TrailingStopTracker |
| Bracket (TP+SL) | COMPLETE | DONE | BracketManager |
| OCO | COMPLETE | DONE | OcoManager, two legs |
| Iceberg | COMPLETE | Not exposed in UI | IcebergManager in ME; no frontend control |
| TWAP / VWAP | FUNCTIONAL | DONE | 15 adversarial tests; jitter randomization |
| Chain (conditional chain) | FUNCTIONAL | Not in UI | ChainManager exists; no web UI |
| SOR (Smart Order Routing) | STUB | Not in UI | REST endpoint wired, handler is placeholder |
| Spread Order | STUB | DONE (page) | Backend handler is placeholder |

### 1.3 Financial Products

| Product | Backend Status | Frontend | Notes |
|---------|---------------|---------|-------|
| Flexible Savings / Earn | FUNCTIONAL | DONE | Yield pool, interest accrual |
| Fixed-Term Savings | FUNCTIONAL | DONE | Same |
| Staking (ETH, SOL) | FUNCTIONAL (simulated) | DONE | No Lido/protocol integration |
| Dual Investment | FUNCTIONAL | DONE | Settlement uses mark price |
| Liquidity Mining | FUNCTIONAL | DONE | LP fee accrual, reward distribution |
| Launchpad | FUNCTIONAL | DONE | commit/allocation flow |
| Launchpool | FUNCTIONAL | DONE | stake/unstake/claim |
| Shark Fin (structured product) | FUNCTIONAL | DONE | Principal-protected range products |
| Lending / Borrowing | FUNCTIONAL | DONE | Per-asset rate, daily interest |
| P2P Trading | FUNCTIONAL | DONE | Create ad, create order, dispute |
| Grid Bot | FUNCTIONAL | DONE | on_fill() wired |
| DCA Bot | FUNCTIONAL | DONE | |
| Infinity Grid | FUNCTIONAL | DONE | |
| Futures Grid | FUNCTIONAL | DONE | |
| Martingale Bot | FUNCTIONAL | DONE | |
| Signal Bot / Webhook | FUNCTIONAL | DONE | |
| Recurring Buy | FUNCTIONAL | DONE | |
| Copy Trading | FUNCTIONAL | DONE | propagate_copy_trade() verified |
| Convert (instant swap) | FUNCTIONAL | DONE | Quote + execute |
| Fiat On/Off Ramp | STUB | DONE | Methods/rates exist; no real PSP |
| Gift Cards | FUNCTIONAL | DONE | create + redeem |
| Referral / Affiliate | FUNCTIONAL | DONE | Commission distribution fixed |
| Competitions / Trading Tournaments | FUNCTIONAL | DONE | Leaderboard, teams |
| Airdrops / Bonus Campaigns | FUNCTIONAL | DONE | Claim flow |
| Proof of Reserves | FUNCTIONAL | DONE | Merkle ledger consistency; no on-chain |
| Insurance Fund | FUNCTIONAL | N/A | Socialized loss, ADL, waterfall |

### 1.4 Account & Security

| Feature | Status | Notes |
|---------|--------|-------|
| Sub-accounts | FUNCTIONAL | Create, transfer, separate keys |
| Unified Trading Account | FUNCTIONAL | One-way upgrade, cross-asset collateral |
| Portfolio Margin | FUNCTIONAL | SPAN-style scenario analysis |
| 2FA (TOTP) | COMPLETE | Setup/enable/disable/verify |
| API Key Management | COMPLETE | Create, list, revoke, IP allowlist |
| Withdrawal Whitelist | COMPLETE | Address book, 2FA confirmation |
| Anti-Phishing Code | COMPLETE | In emails |
| Device Management | COMPLETE | List, revoke |
| Session Management | COMPLETE | List, revoke |
| Post-2FA Security Hold | COMPLETE | 48h withdrawal hold |
| IP Allowlist on API Keys | COMPLETE | Wired into rate_limit middleware |
| Request Signature Verification | COMPLETE | HMAC middleware |
| KYC (Individual + Corporate) | FUNCTIONAL | Logic solid; webhook integration is stub |
| AML / Sanctions | FUNCTIONAL | Fuzzy matching, structuring, expanded lists; no live feed |
| Travel Rule | FUNCTIONAL | Logic complete; no live VASP integration |
| Tax Report (CSV) | FUNCTIONAL | Trade history export |
| Dust Conversion | FUNCTIONAL | Convert small balances to quote asset |

### 1.5 Infrastructure

| Component | Status | Notes |
|-----------|--------|-------|
| Matching Engine | COMPLETE | 450K ops/sec, 109 tests, all A grades |
| WAL (Write-Ahead Log) | COMPLETE | Crash recovery, position snapshots |
| WebSocket Server | COMPLETE | 15 topics, 50 sub limit, auth, reconnect |
| FIX 4.4 Gateway | FUNCTIONAL | Auth, drop copy, session limit; parser fully tested |
| Rate Limiting | COMPLETE | Tiered (30/60/120/300 rps), IP limiter, order limiter |
| Prometheus Metrics | FUNCTIONAL | /metrics endpoint, latency/throughput stats |
| Docker / docker-compose | FUNCTIONAL | Single-node; no K8s, no multi-region |
| CI/CD (GitHub Actions) | FUNCTIONAL | Build, test, lint; no auto-deploy |
| Custody (Deposits/Withdrawals) | STUB | MockProvider only — P0 blocker |
| TLS / Reverse Proxy | FUNCTIONAL | nginx config, HSTS, CSP headers |
| DeFi Yield (Aave/Lido/Compound) | SIMULATED | Handlers written; no real on-chain calls |
| DEX Aggregator | SIMULATED | Router/swap written; no real provider calls |
| Web3 Wallet | SIMULATED | MPC stub; no real key derivation |
| Telegram Bot | FUNCTIONAL | Commands: price, buy, sell, positions, balance, pnl |
| TradFi Oracle | FUNCTIONAL | Multi-feed weighted-median; simulated feeds |

### 1.6 Web Client

33 feature modules, all with tests. Highlights:
- Trading terminal: order book, order entry (all types), chart (21 indicators, 47 drawing tools, WebGL), positions, order history
- 78 Playwright E2E tests
- PWA-ready (manifest.json, service worker declared)
- Social trading page: frontend exists, no backend routes registered
- Web3 page: frontend exists, backend in `rest_dex.rs`/`rest_defi.rs` but not registered in router

---

## Section 2: Competitor Feature Matrix

### 2.1 Core Trading — Where We Stand

| Feature | Bybit | Binance | OKX | Bitget | KuCoin | Hyperliquid | Us |
|---------|-------|---------|-----|--------|--------|-------------|-----|
| Spot | Yes | Yes | Yes | Yes | Yes | No | Yes |
| Perps (linear) | Yes | Yes | Yes | Yes | Yes | Yes (DEX) | Yes |
| Perps (inverse/coin-m) | Yes | Yes | Yes | Yes | Yes | No | Yes |
| Delivery Futures | Yes | Yes | Yes | Yes | Yes | No | Partial |
| Options | Yes | Yes | Yes | No | No | No | Partial |
| Spot Margin | Yes | Yes | Yes | Yes | Yes | No | Yes |
| Portfolio Margin | Yes | Yes | Yes | No | No | No | Yes |
| Unified Account | Yes | Yes | Yes | Yes | No | N/A | Yes |
| Copy Trading | Yes | Yes | Yes | Yes | Yes | Vaults | Yes |
| Leveraged Tokens | Yes | Yes | Yes | Yes | Yes | No | Stub |

### 2.2 Order Types — Where We Stand

| Order Type | Bybit | Binance | OKX | Us |
|-----------|-------|---------|-----|----|
| Limit/Market | Yes | Yes | Yes | Yes |
| Stop-Limit/Market | Yes | Yes | Yes | Yes |
| OCO | Yes | Yes | Yes | Yes |
| TWAP | Yes | Yes | Yes | Yes |
| Iceberg | Yes | Yes | Yes | Yes (no UI) |
| SOR | No | Yes | Yes | Stub |
| Bracket (TP+SL linked) | Yes | Yes | Yes | Yes |
| Trailing Stop | Yes | Yes | Yes | Yes |
| Reserve (hidden qty) | Yes | Yes | Yes | Iceberg covers this |
| Conditional (trigger by index/mark/last) | Yes | Yes | Yes | Yes |

### 2.3 Earn Products — Where We Stand

| Product | Bybit | Binance | OKX | KuCoin | Us |
|---------|-------|---------|-----|--------|-----|
| Flexible Savings | Yes | Yes | Yes | Yes | Yes |
| Fixed-Term | Yes | Yes | Yes | Yes | Yes |
| Staking (ETH/SOL) | Yes | Yes | Yes | Yes | Simulated |
| Dual Investment | Yes | Yes | Yes | Yes | Yes |
| Structured Products (Shark Fin) | Yes | Yes | Yes | No | Yes |
| Liquidity Mining | No | Yes | Yes | Yes | Yes |
| Launchpad / IEO | Yes | Yes | Yes | Yes | Yes |
| Launchpool | Yes | Yes | Yes | No | Yes |
| DeFi Yield Integration | Yes | No | Yes | No | Simulated |
| P2P Trading | Yes | Yes | Yes | Yes | Yes |
| Convert / Swap | Yes | Yes | Yes | Yes | Yes |
| Fiat On/Off Ramp | Yes | Yes | Yes | Yes | Stub |

### 2.4 Infrastructure & API — Gaps

| Feature | Bybit | Binance | OKX | Us |
|---------|-------|---------|-----|----|
| REST API | ~300 ep | ~500 ep | ~400 ep | ~456 ep |
| WebSocket streams | 25+ | 30+ | 25+ | 15 |
| FIX Protocol | Yes | No | Yes | Yes |
| Rate limit (weight-based) | Yes | Yes | Yes | Token bucket (no weight) |
| Testnet environment | Yes | Yes | Yes | Dev mode only (no testnet URL) |
| WebSocket API (place orders via WS) | Yes | Yes | Yes | No |
| Market data history API (deep) | Yes | Yes | Yes | Yes |
| API key permission scopes | Yes | Yes | Yes | Partial |
| Webhook order placement (signal bots) | Yes | Partial | Yes | Yes |
| gRPC / binary protocol | No | No | No | No |

### 2.5 Competitive Features We Lack Entirely

The following are features that at least two top-tier competitors offer and that we have not implemented even as stubs:

1. **Native Mobile App (iOS/Android)** — All top exchanges have 10M+ download apps. We have a PWA (web app installable on mobile) which covers the minimum, but native push notifications and biometric auth are not available via PWA on iOS.
2. **WebSocket Order Placement** — Bybit, Binance, OKX all support placing/amending/canceling orders over WebSocket. We only support market data streaming over WS.
3. **Testnet / Sandbox Environment** — A publicly accessible testnet with faucet endpoint. We have a `/testnet/faucet` REST endpoint but no deployed testnet instance.
4. **Spot Grid Bot Backtesting** — Bybit, Bitget, KuCoin show historical performance simulations before deploying a grid. We have no backtesting.
5. **Copy Trading: TradFi (Forex/Indices via MT5)** — Bybit Pro copy trading includes Forex and equity indices via MetaTrader 5 bridge. Not relevant to our scope yet.
6. **AI Trading Assistant** — OKX and Bitget launched AI-assisted trading signals in 2025. This is emerging, not yet table-stakes.
7. **NFT Marketplace** — Bybit and OKX have embedded NFT venues. We have none, and it is not clearly in scope.
8. **Social Feed / Trader Profiles** — We have the frontend page but no backend. OKX and Bybit have trader profile and social following features tightly integrated with copy trading.
9. **Real DeFi Yield Integration** — OKX routes user funds to Aave, Lido, etc. We have the code architecture but no real on-chain calls.
10. **Real DEX Aggregation** — OKX's X Routing, aggregating 100+ DEXes. We have the router abstraction but no real providers connected.
11. **Real Fiat PSP Integration** — Stripe, Banxa, MoonPay, or similar. Our fiat endpoints are stubs.
12. **On-Chain Proof of Reserves** — Merkle tree proofs published on-chain or signed by an auditor. We have ledger-consistency PoR but no on-chain attestation.
13. **Real Staking Protocol Integration** — Lido, EigenLayer, Rocket Pool. Our staking is simulated APY.
14. **Advanced Order Modification History** — Binance and Bybit expose amendment history per order. Our orders track cancel/fill but not amendment audit trail.
15. **Cross-Chain Bridging (native)** — OKX has a native bridge across 130+ chains. We have the code stub but no real providers.

---

## Section 3: Gap Priority Classification

### P0 — Launch Blockers (Cannot go live without these)

| # | Gap | Why It Blocks Launch | Effort |
|---|-----|---------------------|--------|
| P0-1 | Real custody (Fireblocks or self-hosted) | Without real deposits/withdrawals, no real users | XL (4-6 weeks) |
| P0-2 | Real index price feeds (live-feeds feature flag) | Perpetual orders silently blocked in production without index | M (1 week, infra work) |
| P0-3 | KYC webhook integration (Sumsub/Diia) | Approving KYC manually at scale is impossible | M (1-2 weeks) |
| P0-4 | Live sanctions feed (OFAC SDN XML or Chainalysis) | Hardcoded list is not compliant for regulated operation | S-M (3-5 days) |
| P0-5 | Production deployment configuration | No deploy.sh, no infra-as-code for multi-node, no SSL cert provisioning | M (1-2 weeks) |
| P0-6 | Email delivery (SMTP or transactional API) | 2FA codes, withdrawal confirmations, KYC notifications require working email | S (2-3 days) |
| P0-7 | Spot double-entry ledger | Audit requirement for any regulated exchange | M (1-2 weeks) |

### P1 — Critical for Competitiveness (Month 1-3 post-launch)

| # | Gap | Impact | Effort |
|---|-----|--------|--------|
| P1-1 | WebSocket order placement | Pro traders expect this; Bybit/Binance/OKX all have it | L (1 week) |
| P1-2 | Real fiat on/off ramp (MoonPay/Banxa integration) | Without fiat rails, user onboarding requires pre-existing crypto | M (2-3 weeks) |
| P1-3 | Native mobile app OR production-grade PWA | 60%+ of crypto trading volume is mobile | XL (6-12 weeks for native; M for PWA hardening) |
| P1-4 | Travel Rule VASP integration (TRISA/OpenVASP) | Required for compliance in EU (MiCA), FATF member states | M (2-3 weeks) |
| P1-5 | Real on-chain PoR attestation | "Proof of Reserves" without on-chain component is marketing liability | L (2-3 weeks) |
| P1-6 | Iceberg orders exposed in web UI | Professional traders use iceberg; backend is complete | S (1-2 days) |
| P1-7 | Weight-based API rate limiting | Token bucket is simpler but doesn't match industry standard; pro traders expect weight headers | S-M (3-5 days) |
| P1-8 | API key permission scopes (read/trade/withdraw) | Security best practice; all competitors have this | S-M (3-5 days) |
| P1-9 | Testnet with public URL and faucet | Onboarding institutional API integrators requires a sandbox | M (1 week, mostly infra) |
| P1-10 | Grid bot backtesting | Major conversion driver; users want to see historical P&L before committing funds | L (2-3 weeks) |
| P1-11 | Social trading backend (trader profiles, follow, feed) | Frontend is built; backend routes are not registered | M (1-2 weeks) |
| P1-12 | Options order book UI | Options page exists but no order placement for market makers | L (2-3 weeks) |
| P1-13 | Delivery futures: full expiry flow testing and UI polish | Settlement logic exists but expiry edge cases not fully tested | M (1-2 weeks) |

### P2 — Important Differentiators (Months 3-6)

| # | Gap | Impact | Effort |
|---|-----|--------|--------|
| P2-1 | Real staking protocol integration (Lido, EigenLayer) | Simulated APY is not competitive once real users notice yields don't move | XL (4-8 weeks) |
| P2-2 | Real DeFi yield integration (Aave, Compound) | Same issue as staking; architecture exists, no on-chain calls | XL (4-8 weeks) |
| P2-3 | Real DEX aggregation (1inch, Paraswap, or own router) | Web3/DeFi-native users expect real on-chain routing | XL (4-8 weeks) |
| P2-4 | Real cross-chain bridging | Multi-chain asset management is increasingly expected | XL (6-12 weeks) |
| P2-5 | Advanced API key scopes and webhook subscriptions | Institutional API clients need fine-grained permissioning | M (1-2 weeks) |
| P2-6 | Bot strategy marketplace / copy bot trading | Bitget launched Bot Copy Trading in Jan 2026; emerging expectation | L-XL (3-5 weeks) |
| P2-7 | Order amendment history audit trail | Compliance requirement for regulated venues | S (2-3 days) |
| P2-8 | Multi-signature withdrawal approvals (institutional) | Needed for institutional custody workflows | M (1-2 weeks) |
| P2-9 | Grafana dashboards pre-configured | Ops team needs ready-made dashboards on day one | S-M (3-5 days) |
| P2-10 | Kubernetes / horizontal scaling | Single-node docker-compose is not production-grade beyond 10K concurrent users | XL (4-6 weeks) |
| P2-11 | SAR filing workflow with regulatory authority integration | MiCA requires automated STR submission; we have generation but no submission | M (2-3 weeks) |
| P2-12 | Announcement system (maintenance windows, news) | Backend exists; needs frontend integration and push notifications | S (1-2 days) |
| P2-13 | Leveraged token rebalancing mechanism | Currently stub; need daily NAV calculation and rebalancing | L (2-3 weeks) |
| P2-14 | Chain order UI (conditional execution chains) | Backend ChainManager is complete; no web UI | M (1-2 weeks) |
| P2-15 | Advanced copy trading: profit-share settlement, follower controls | Logic exists but no UI for follower risk limits, max drawdown, pause | M (1-2 weeks) |

### P3 — Nice to Have (6-12 months)

| # | Gap | Notes |
|---|-----|-------|
| P3-1 | AI trading signals / assistant | Emerging feature; OKX/Bitget have it but not yet table-stakes |
| P3-2 | NFT marketplace | Bybit/OKX have it; outside our current product scope |
| P3-3 | TradFi copy trading (Forex/Indices via MT5 bridge) | Bybit's differentiator; complex regulatory territory |
| P3-4 | Native iOS / Android app | PWA covers 90% of use cases; native is a significant investment |
| P3-5 | Perpetual DEX (on-chain order book, Hyperliquid model) | Architectural rethink; not a gap to close but a separate product |
| P3-6 | gRPC/binary API (ultra-low latency co-location) | Needed only for HFT co-location offering; premature |
| P3-7 | Crypto credit card | Binance Card, Bybit Card; complex fintech licensing |
| P3-8 | Futures basis trading UI | Spread between spot and futures; backend spread order exists as stub |
| P3-9 | Social feed full backend (posts, likes, follows) | Routes exist in web frontend; backend not wired. Engagement product, not core trading |
| P3-10 | Multi-language UI (i18n) | All top exchanges support 10+ languages |
| P3-11 | Dark pool / block trade matching (automated) | RFQ/block trade exists; no automated dark pool |
| P3-12 | Options strategy builder UI | Multi-leg options (straddle, strangle, butterfly) as guided flows |
| P3-13 | Economic calendar integration (real data feed) | `/public/economic-calendar` endpoint exists; data source is stub |

---

## Section 4: Phased Development Roadmap

### Phase 1 — Production-Safe Launch (8-12 weeks)

**Goal:** Enable the first real users to deposit, trade spot/perps, and withdraw safely. All P0 items complete.

**Priority order within Phase 1 is strict: P0-1 (custody) blocks everything downstream.**

#### Phase 1A — Custody & Blockchain (Weeks 1-6)

| # | Task | Size | Depends | Acceptance Criteria |
|---|------|------|---------|---------------------|
| 1A-1 | Select and integrate custody provider (Fireblocks vs self-hosted MPC) | XL | - | Decision documented, Fireblocks SDK or self-custody keys functional in testnet |
| 1A-2 | Real deposit address generation per user per asset | L | 1A-1 | `POST /account/deposit-address` returns a real on-chain address; verified with faucet TX |
| 1A-3 | Deposit confirmation via blockchain events / webhooks | L | 1A-2 | Deposit credited after N confirmations; test with testnet BTC/ETH |
| 1A-4 | Withdrawal processing with blockchain signing | L | 1A-1 | `POST /account/withdraw` triggers real TX; TX hash returned and stored |
| 1A-5 | Hot/warm/cold wallet sweep automation | M | 1A-3 | Deposits above threshold auto-swept to cold storage |
| 1A-6 | Withdrawal confirmation email with 2FA | S | 1A-4 | User receives email with confirmation link; link validates TOTP before signing TX |
| 1A-7 | On-chain Proof of Reserves attestation | M | 1A-2 | Merkle root published on-chain or signed by independent auditor; verifiable by users |

#### Phase 1B — External Integrations (Weeks 2-4, parallel with 1A)

| # | Task | Size | Depends | Acceptance Criteria |
|---|------|------|---------|---------------------|
| 1B-1 | Live OFAC/EU/UN sanctions feed (OFAC SDN XML parser) | M | - | Sanctions list auto-refreshes daily; `sanctions_force_update` admin endpoint triggers manual refresh |
| 1B-2 | Sumsub KYC webhook integration | M | - | `POST /webhooks/kyc` receives Sumsub events; auto-approves/rejects KYC status |
| 1B-3 | Live index price feeds (enable `live-feeds` feature flag) | M | - | WebSocket price feed from at least 3 sources; `is_index_healthy()` returns true in production |
| 1B-4 | SMTP / transactional email (SendGrid or AWS SES) | S | - | 2FA setup, withdrawal confirmation, KYC notification emails delivered |
| 1B-5 | Fiat on/off ramp stub replacement (MoonPay or Banxa) | M | - | `POST /fiat/deposit` redirects to real provider; webhook credited on completion |

#### Phase 1C — Compliance & Deployment (Weeks 3-5, parallel)

| # | Task | Size | Depends | Acceptance Criteria |
|---|------|------|---------|---------------------|
| 1C-1 | Spot double-entry ledger in matching path | M | - | Every fill produces two ledger entries (debit base, credit quote or vice versa); reconciliation task reports 0 delta |
| 1C-2 | Production deployment playbook (infra-as-code) | M | - | One-command deploy to cloud VM; SSL cert auto-provisioned; health check passes |
| 1C-3 | Jurisdiction blocking enforcement (REST wiring) | S | - | `/jurisdiction/rules` admin endpoint wired; requests from blocked countries return 451 |
| 1C-4 | TRISA / OpenVASP travel rule integration | M | 1B-2 | Outgoing transfers above FATF threshold include IVMS101 message; receive endpoint handles incoming |
| 1C-5 | Grafana dashboards (pre-configured, provisioned) | S | - | Exchange dashboard with order rate, fill rate, latency p99, open interest, insurance fund visible on first boot |

### Phase 2 — Competitive API & Pro Trading (Weeks 8-16)

**Goal:** Close the gaps that cause professional traders and API clients to choose competitors.

| # | Task | Size | Depends | Acceptance Criteria |
|---|------|------|---------|---------------------|
| 2-1 | WebSocket order placement (place/amend/cancel over WS) | L | - | `{"op":"order.place",...}` accepted over existing WS connection; response within 5ms on local bench |
| 2-2 | API key permission scopes (read/trade/withdraw tiers) | S | - | API key has `permissions: [read, trade, withdraw]` bitmask; withdraw blocked without withdraw scope |
| 2-3 | Weight-based API rate limiting | S | - | Each endpoint has a documented weight; X-RateLimit-Used-Weight header returned; 429 at 1200/min |
| 2-4 | Public testnet deployment + faucet | M | Phase 1 infra | `testnet.exchange.io` accessible; `/testnet/faucet` credited; documented in API docs |
| 2-5 | Iceberg orders in web UI | S | - | "Iceberg" toggle in OrderEntry reveals display qty input; order submitted with iceberg_qty |
| 2-6 | Grid bot backtesting | L | - | Given symbol + grid params + date range, returns simulated P&L curve; displayed in bot creation flow |
| 2-7 | Social trading backend (trader profiles + follow + feed) | M | - | `GET /social/profile/:id`, `POST /social/follow/:id`, `GET /social/feed` registered in router |
| 2-8 | Options order book UI (full market maker interface) | L | - | Options page shows live bid/ask per strike/expiry; users can place limit/market options orders |
| 2-9 | Delivery futures: expiry edge case tests + UI polish | M | - | 10+ tests for settlement at expiry: partial fill, zero OI, inverse; UI shows countdown to delivery |
| 2-10 | Advanced copy trading UI (follower risk controls) | M | - | Follower can set max drawdown %, max position size, pause; `copy-trading/settings` page complete |
| 2-11 | Order amendment audit trail | S | - | Each `PUT /orders/{id}` appended to order history with old/new price/qty and timestamp |
| 2-12 | Chain order (conditional chain) web UI | M | - | Chain builder in terminal: "if price hits X, place Y, then Z"; uses `POST /orders/chain` |
| 2-13 | Leveraged tokens: rebalancing mechanism | L | - | Daily NAV recalculation task; rebalancing triggers if leverage drifts >5%; admin override |
| 2-14 | Multi-sig withdrawal approval (institutional) | M | - | Institution-tier accounts require N-of-M approver sign-off before `admin/withdrawals/{id}/approve` |

### Phase 3 — Ecosystem Expansion (Weeks 16-32)

**Goal:** Close web3/DeFi gaps, improve mobile UX, build differentiated products.

| # | Task | Size | Depends | Acceptance Criteria |
|---|------|------|---------|---------------------|
| 3-1 | Real Lido staking integration | L | Custody | `earn/staking/stake` submits real Lido deposit; APY sourced from `eth-api.lido.fi` |
| 3-2 | Real EigenLayer restaking | L | 3-1 | EigenLayer AVS reward rates from RPC; deposits/withdrawals real |
| 3-3 | Real Aave v3 DeFi yield integration | XL | Custody | `defi/deposit` allocates to real Aave pool; live APY from The Graph subgraph |
| 3-4 | Real DEX aggregation (1inch or Paraswap) | XL | Custody | `dex/swap` routes through real aggregator; slippage and gas estimated before confirmation |
| 3-5 | Production-grade PWA (biometric, push notifications) | M | - | FIDO2/WebAuthn login; browser push notifications for price alerts and fill confirmations |
| 3-6 | Bot strategy marketplace (copy-bot) | L | - | Strategy creators publish bot configs; followers copy with one click; profit share settles on-chain |
| 3-7 | SAR automated submission (FinCEN XML / goAML) | M | Compliance | `admin/compliance/sar/{id}/file` submits real XML payload to regulator API endpoint |
| 3-8 | Kubernetes Helm chart + auto-scaling | XL | Phase 1 infra | Matching engine scales horizontally; API gateway load-balanced; demonstrated under 50K concurrent users |
| 3-9 | Cross-chain bridging via Circle CCTP / LayerZero | XL | Custody | `bridge/quote` returns real route; `bridge/execute` initiates on-chain bridge TX |
| 3-10 | Options strategy builder (multi-leg UI) | L | Phase 2 options | "Buy straddle", "sell strangle" guided flows; bundled as single bracket-style order |
| 3-11 | Economic calendar with real data feed (Econoday API) | S | - | `GET /public/economic-calendar` returns actual event data; cached and refreshed daily |
| 3-12 | Multi-language UI (i18n framework) | L | - | i18n framework installed; English + 2 additional languages (Chinese, Russian) |
| 3-13 | Announcements + push notifications | S | - | Admin creates announcements; users with push consent receive them; in-app notification bell |

### Phase 4 — Scale & Differentiation (Months 8-18)

**Goal:** Native mobile app, institutional grade, potential on-chain expansion.

| # | Task | Size | Depends | Acceptance Criteria |
|---|------|------|---------|---------------------|
| 4-1 | React Native mobile app (iOS + Android) | XL | All Phase 2 | iOS App Store + Google Play; feature parity with web terminal for spot/perp trading |
| 4-2 | Co-location / FIX leased-line offering | XL | Phase 2 infra | Published SLA: <50 microsecond order entry from co-lo cage; tested with FIX client |
| 4-3 | Dark pool (automated block matching) | L | - | Resting block orders matched outside the lit book; reported in block-trade history |
| 4-4 | AI trading signals integration (3rd party API) | M | - | Signal feed from provider; displayed in terminal as non-binding alerts |
| 4-5 | Crypto credit/debit card (3rd party issuer) | XL | Compliance | Integrated with card issuer API; spend debits account balance; cashback in exchange token |
| 4-6 | TradFi oracle live feeds (real stock/forex/commodity) | L | TradFi oracle crate | `tradfi-oracle` connected to Bloomberg/Refinitiv feed; ETF-linked products available |
| 4-7 | Decentralized order book (permissioned L2) | XL | - | Separate product: orders settled on-chain; non-custodial for advanced users |

---

## Section 5: Sizing Summary

### By Phase

| Phase | Total Tasks | S | M | L | XL | Estimated Duration |
|-------|------------|---|---|---|----|--------------------|
| Phase 1 (launch) | 16 | 3 | 8 | 4 | 1 | 8-12 weeks |
| Phase 2 (pro API) | 14 | 4 | 6 | 4 | 0 | 8 weeks |
| Phase 3 (ecosystem) | 13 | 2 | 4 | 5 | 4 | 16 weeks |
| Phase 4 (scale) | 7 | 0 | 1 | 2 | 4 | Open-ended |

### By Priority

| Priority | Count | Phase Required |
|----------|-------|---------------|
| P0 (launch blocker) | 7 | Phase 1 |
| P1 (critical competitiveness) | 13 | Phase 1-2 |
| P2 (important differentiators) | 15 | Phase 2-3 |
| P3 (nice to have) | 13 | Phase 3-4 |

---

## Section 6: Risk Register

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| Fireblocks integration takes longer than 6 weeks | High (delays entire launch) | Medium | Start Fireblocks sales/onboarding week 1; parallel-path self-custody POC |
| Sumsub KYC webhook format changes break integration | Medium | Low | Abstract KYC provider behind interface; integration tests against Sumsub sandbox |
| OFAC SDN XML format changes break sanctions parser | High (compliance liability) | Low | Pin to versioned endpoint; alert on parse failure; daily integration test |
| Index feeds disconnect during high volatility | High (perp orders blocked) | Medium | Min 3 sources; health gate already active; circuit breaker already in place |
| Spot double-entry ledger migration causes balance inconsistency | High (financial loss) | Medium | Run reconciliation task in parallel before cutover; compare ledger vs balance state for 7 days |
| WebSocket order placement introduces race conditions under load | High (order duplication) | Medium | Extensive load testing at 10K concurrent WS connections; idempotency keys on all order commands |
| Fiat PSP (MoonPay/Banxa) charges high fees or rejects integration | Medium | Low | Negotiate before committing; have backup provider ready (Stripe + Onramper) |
| MiCA compliance deadline (July 1, 2026) | High (cannot serve EU) | High | Phase 1C compliance work must complete by June; engage EU legal counsel immediately |
| Leveraged token rebalancing causes adverse price impact | High (user harm) | Low | Max rebalance size 0.1% of daily volume; circuit breaker if slippage >2% |
| Single-node deployment fails under traffic | High | Medium | Phase 3 K8s migration; interim: vertical scaling + CDN for static assets |
| Options expiry settlement bug | High (financial loss) | Low | Mandatory test coverage on all expiry paths before any options user goes live |
| Real staking integration slashing event | High (user fund loss) | Low | Only integrate non-slashable validators first; slash insurance pool; disclosure in UI |

---

## Section 7: Definition of Done Per Phase

### Phase 1 Done Criteria
- [ ] `POST /account/deposit-address` returns a real on-chain address verified with a testnet transaction
- [ ] `POST /account/withdraw` signs and broadcasts a real testnet transaction
- [ ] Deposit credited within 3 confirmations on testnet
- [ ] `cargo test` passes (all 2,086+ tests)
- [ ] KYC webhook from Sumsub sandbox auto-approves a test identity
- [ ] OFAC SDN list loads at startup and blocks a known sanctioned name in test
- [ ] Index price feed healthy check returns true with 3 live sources connected
- [ ] Spot fills produce two matching ledger entries (reconciliation delta = 0)
- [ ] Production deployment completes in < 30 minutes from git push
- [ ] All EU/sanctioned jurisdiction requests return 451

### Phase 2 Done Criteria
- [ ] Placing an order via WebSocket returns acknowledgement < 10ms on staging
- [ ] API key with `read`-only scope cannot place orders (403 returned)
- [ ] Testnet accessible at public URL; faucet endpoint credited to test account
- [ ] Iceberg order placed via UI; order book shows only display_qty to other traders
- [ ] Grid bot backtest returns P&L curve matching manual calculation for 3 test scenarios
- [ ] Delivery futures expiry settlement tested: 10 scenarios passing

### Phase 3 Done Criteria
- [ ] Lido stake via UI: TX hash returned, stETH balance updated in account
- [ ] DEX swap via UI: trade executes on real chain (testnet); user balance updated
- [ ] Push notification delivered to Chrome and Safari on mobile after a fill
- [ ] Kubernetes cluster handles 50K concurrent WebSocket connections without degradation

---

## Section 8: What We Should NOT Build (Scope Exclusions)

These items appeared in research but are explicitly out of scope to avoid distraction:

- **NFT Marketplace** — Complex content moderation, IP risk, minimal trading revenue. Not worth building.
- **Crypto Credit Card** — Requires card network membership (Visa/Mastercard), separate regulatory licenses, chargeback handling. Build in Year 2+ with a card-as-a-service partner.
- **Perpetual DEX (Hyperliquid-style on-chain order book)** — This is a different product, not a feature. Requires L1/L2 blockchain, smart contracts, and a completely different trust model.
- **Binance-compatible API aliases** — We are not building a Binance clone. Our API has its own design. No aliasing.
- **Full native iOS/Android app in Phase 1-2** — PWA hardening delivers 80% of the value at 10% of the cost. Native app is Phase 4.
- **TradFi copy trading (MT5 bridge)** — Forex and equity index CFDs require separate broker licensing in most jurisdictions.

---

## Appendix A: WebSocket Topics Inventory

Current topics (15):

**Public (no auth):**
- `trade.{symbol}` — real-time trades
- `orderbook.diff.{symbol}` — order book deltas
- `orderbook.{N}.{symbol}` — fixed-depth snapshots (1, 50)
- `kline.{interval}.{symbol}` — candlestick updates
- `ticker.{symbol}` — 24h ticker
- `ticker.mini.{symbol}` — mini ticker
- `ticker.all` — all mini tickers
- `markPrice.{symbol}` — mark price updates
- `funding.{symbol}` — funding rate updates
- `liquidation.{symbol}` — liquidation events

**Private (auth required):**
- `orders` — order updates
- `fills` — trade executions
- `positions` — position changes
- `wallet` — balance changes
- `deposits` — deposit events
- `adl` / `adl.{symbol}` — ADL warning

**Missing vs competitors:**
- `bookticker.{symbol}` (best bid/ask, no auth, ultra-low latency)
- `aggTrade.{symbol}` (aggregated trades stream — Binance standard)
- `openInterest.{symbol}` (real-time OI updates)
- `indexPrice.{symbol}` (separate index price stream)
- Order placement / amendment / cancel over WebSocket (P1-1 above)

---

## Appendix B: Crate Summary

| Crate | Purpose | Status |
|-------|---------|--------|
| common | Shared types | Complete |
| matching-engine | FIFO order book | Complete (450K/sec) |
| journal | WAL + crash recovery | Complete |
| order-gateway | Pre-trade risk, STP | Complete |
| account | Balances, lock/unlock | Complete |
| risk-engine | Margin, liquidation, ADL | Complete |
| market-data | Klines, tickers | Complete |
| api-gateway | axum REST + WebSocket | ~85% complete |
| settlement | Funding, insurance | Complete |
| surveillance | Spoofing, wash trade, manipulation | Complete |
| compliance | KYC, AML, sanctions, Travel Rule | Functional (stubs for external integrations) |
| custody | Deposits, withdrawals | Stub (MockProvider only) |
| trigger-service | Stop/TP/trailing/bracket/OCO | Complete |
| index-price | Multi-feed weighted-median | Functional (stubs for live feeds) |
| proof-of-reserves | Merkle tree | Functional (no on-chain) |
| copy-trading | Master/follower replication | Functional |
| trading-bots | Grid, DCA, advanced | Functional |
| algo-execution | TWAP, VWAP | Functional |
| market-maker | SLA, toxicity, rebates | Functional |
| fix-gateway | FIX 4.4 | Functional |
| asset-registry | Coin/token registry | Functional |
| defi-yield | Aave/Lido/Compound/EigenLayer | Simulated |
| dex-aggregator | DEX routing/swaps | Simulated |
| web3-wallet | MPC wallet, cross-chain bridge | Simulated |
| telegram-bot | Trading commands | Functional |
| tradfi-oracle | Forex/equity feeds | Simulated |
| admin | Metrics, audit, email | Functional |
| db | PostgreSQL repos | Functional |
| server | Binary entry point | Complete |
