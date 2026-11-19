# Exchange

A complete cryptocurrency exchange platform — spot and perpetual futures — written in Rust.

> **Project status:** Reference implementation under active development. It has **not**
> been independently security-audited. Operating a cryptocurrency exchange is a regulated
> activity in most jurisdictions — see [Disclaimer](#disclaimer).

## Overview

Exchange is a full trading platform built as a Rust workspace of 24 crates plus a
React/TypeScript web client. It implements the core systems a real exchange needs:
a price-time-priority matching engine, a write-ahead log for crash recovery, a
double-entry ledger, perpetual-futures risk and liquidation, funding settlement,
compliance (KYC/AML/Travel Rule), market surveillance, and a FIX 4.4 gateway.

### Features

- **Matching engine** — per-symbol threads, strict price-time FIFO, zero async / zero network in the hot path
- **Spot & perpetual futures** — linear and inverse contracts, cross/isolated margin, auto-deleveraging
- **Crash recovery** — write-ahead log (WAL) as source of truth, write-behind DB persistence, periodic order-book snapshots
- **Risk engine** — margin calculation, liquidation, ADL, portfolio (SPAN-like) margin
- **Double-entry ledger** — every credit has a matching debit; no value created from nothing
- **Settlement** — funding rate (premium index + clamp), mark-to-market, insurance fund
- **Compliance** — KYC tiers, AML monitoring, sanctions/PEP screening, Travel Rule, SAR, EDD, jurisdiction rules
- **Surveillance** — spoofing, wash trading, and quote-stuffing detection
- **Order types** — market, limit, stop, take-profit, trailing, bracket, OCO
- **Products** — copy trading, grid/DCA/rebalancer bots, TWAP/VWAP algo execution, earn/staking
- **Connectivity** — ~190 REST endpoints, 15 WebSocket topics, FIX 4.4 gateway
- **Proof of reserves** — Merkle-tree liability reports
- **Web client** — React 18 + Vite + TypeScript trading terminal with a custom design system

### Engineering principles

- All money is `rust_decimal` — never `f64`
- Every financial formula cites its origin (industry standard, competitor reference,
  regulation, or a documented first-principles derivation) — see `docs/references/`
- Comprehensive automated test suite across the workspace

## Architecture

Phase 1 is a single binary (`exchange`) composed of cleanly separated crates;
Phase 2 splits into services. See `docs/ARCHITECTURE.md` for detail.

```
crates/
  common/            Shared domain types (Price, Qty, Order, Position, ...)
  matching-engine/   Order book + matching
  journal/           Write-ahead log
  order-gateway/     Pre-trade risk, self-trade prevention, balance locks
  account/           Accounts, balances, settlement
  risk-engine/       Margin, liquidation, ADL
  market-data/       Klines, tickers, aggregation
  api-gateway/       REST + WebSocket server
  settlement/        Funding, mark-to-market, insurance fund
  surveillance/      Market-abuse detection
  compliance/        KYC, AML, sanctions, Travel Rule
  custody/           Deposits, withdrawals, address whitelisting
  ...                24 crates total — see CLAUDE.md for the full map
```

## Quick start

Requires a recent stable Rust toolchain, and Node.js for the web client.

```bash
# Build
cargo build --release

# Run the test suite
cargo test

# Start the server (in-memory mode, no database required)
cargo run --release --bin exchange     # listens on :8080

# Web client
cd clients/web && npm install && npm run dev
```

Set `DATABASE_URL` to a PostgreSQL instance for persistent mode. See `docs/DEVELOPMENT.md`.

## Documentation

- `docs/ARCHITECTURE.md` — system architecture
- `docs/API_REFERENCE.md` — REST and WebSocket API
- `docs/references/` — provenance for every financial mechanism
- `CLAUDE.md` — full repository map and engineering rules

## Contributing

Contributions are welcome. All contributors must agree to the
[Contributor License Agreement](CLA.md) — this is required so the project can be
offered under both the open-source and commercial licenses (see below).
See [CONTRIBUTING.md](CONTRIBUTING.md) for the development workflow.

## Security

Please report vulnerabilities responsibly — see [SECURITY.md](SECURITY.md).
Do **not** open public issues for security problems.

## Licensing

Exchange is available under a **dual license**:

1. **GNU Affero General Public License v3.0** — see [LICENSE](LICENSE). Free and
   open-source. Note the AGPL network clause: if you run a modified version to
   provide a service over a network, you must make the modified source available
   to that service's users.

2. **Commercial license** — for organisations that wish to use Exchange in a
   proprietary product or service without the AGPL's source-disclosure
   obligations. Contact the maintainer for terms.

Copyright © 2026 Dmytro Chystiakov. The "Exchange" name and associated branding
are not covered by the AGPL.

## Disclaimer

This software is provided "as is", without warranty of any kind. It has **not**
been independently security-audited. Operating a cryptocurrency exchange is a
regulated financial activity in most jurisdictions; you are solely responsible for
obtaining all required licenses and registrations and for complying with all
applicable laws — including securities, money-transmission, AML/CFT, and
consumer-protection regulations. Nothing in this repository constitutes financial,
legal, or investment advice.
