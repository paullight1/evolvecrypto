# Exchange

## What
Complete cryptocurrency exchange platform. Spot + perpetual futures.
~190 REST endpoints, 15 WS topics, 12 background tasks, 24 crates, 1353+ tests.

## Structure
```
crates/
  common/            # Shared types: Price, Qty, Side, Order, Position, TimeInForce, etc.
  matching-engine/   # Order book + matching (per-symbol threads, price-time FIFO)
  journal/           # WAL (append-only command log for crash recovery)
  order-gateway/     # Pre-trade risk checks, STP, balance lock
  account/           # User accounts, balances, lock/unlock/settle, wallet summary
  risk-engine/       # Margin calc, liquidation, ADL, position manager
  market-data/       # Klines, tickers, candle aggregation
  api-gateway/       # axum REST + WebSocket server, Exchange god struct
  settlement/        # Funding rate, mark-to-market, insurance fund
  surveillance/      # Spoofing, wash trading, quote stuffing detection
  admin/             # Metrics, audit log, email, notifications, capital tracking
  compliance/        # KYC, AML, sanctions, PEP, Travel Rule, SAR, EDD, ToS, risk warnings, jurisdiction
  custody/           # Deposits, withdrawals, address whitelist, wallet management
  trigger-service/   # Stop/TP/trailing/bracket/OCO orders
  index-price/       # Multi-feed weighted-median index aggregator
  proof-of-reserves/ # Merkle tree + liability reports
  copy-trading/      # Master/follower replication
  trading-bots/      # Grid, DCA, rebalancer bots
  algo-execution/    # TWAP, VWAP execution
  market-maker/      # SLA, toxicity, rebates, PnL attribution
  fix-gateway/       # FIX 4.4 protocol (parser, session, TCP server)
  asset-registry/    # Coin/token registry with network configs
  db/                # PostgreSQL repos (orders, trades, positions, KYC, ledger, etc.)
  server/            # Binary entry point, product registration, demo mode
```

## Commands
```bash
cargo build --release
cargo test
cargo test -p exchange-api-gateway    # API + integration tests
cargo test -p exchange-matching-engine
cargo bench -p exchange-matching-engine
cargo run --release --bin exchange     # Start server on :8080
```

## Code Style
- Rust: `anyhow::Result` for errors, `tracing` for logs
- NEVER use f64 for money — `rust_decimal` everywhere
- Matching engine: zero async, zero network in hot path
- All balance mutations must maintain invariant: `available >= 0`
- Initialize ALL new Exchange fields in BOTH `new()` and `with_journal()`
- When adding fields to PlaceOrderCommand/Order: update ALL construction sites (50+)
- TypeScript: strict mode, no `any`, CSS Modules for styling

## Design System Rules (CRITICAL)

All UI work MUST use the shared design system. No exceptions.

### Mandatory Usage
- **ALWAYS** use components from `@shared/ui` — NEVER create custom buttons, inputs, modals, toggles, checkboxes, dropdowns, etc. in feature code
- **ALWAYS** use CSS token variables (`var(--pad-sm)`, `var(--text-0)`, `var(--accent)`, etc.) — NEVER hardcode hex colors, px font sizes, or px spacing in `.module.css`
- **ALWAYS** run `npx stylelint 'src/**/*.module.css'` before considering web client CSS changes done — must be 0 violations
- **ALWAYS** write a `.stories.tsx` file when creating a new shared/ui component

### Available Components (33 total)
```
@shared/ui exports:
  Button, Input, Modal, Select, Table, Tabs, Badge, Card, Toast, Tooltip,
  Spinner, Skeleton, Pagination, ConfirmDialog, CopyableText, ErrorBoundary,
  Toggle, ProgressBar, StatCard, Countdown, Checkbox, RadioGroup,
  NumberInput, SearchInput, Dropdown (+ DropdownItem, DropdownSeparator),
  EmptyState, Divider, Alert, Slider, Tag, Avatar, Accordion, Steps
```

### What NOT To Do
- NEVER create a custom toggle/switch — use `<Toggle>`
- NEVER use `<input type="checkbox">` directly — use `<Checkbox>`
- NEVER use `<input type="number">` directly — use `<NumberInput>`
- NEVER build custom progress bars with `width: ${pct}%` — use `<ProgressBar>`
- NEVER duplicate countdown logic with setInterval — use `<Countdown>`
- NEVER build label+value stat cards manually — use `<StatCard>`
- NEVER create custom dropdown menus — use `<Dropdown>`
- NEVER hardcode empty state markup — use `<EmptyState>`
- NEVER use raw `<hr>` or pipe characters as dividers — use `<Divider>`
- NEVER build custom alert/warning banners — use `<Alert>`
- NEVER implement custom radio/segmented controls — use `<RadioGroup variant="button">`
- NEVER build custom search inputs — use `<SearchInput>`
- NEVER create custom slider/range controls — use `<Slider>`
- If a component you need doesn't exist in shared/ui — ADD IT there first, then use it

### Design Tokens
```
Colors:   var(--bg-0..4), var(--text-0..3), var(--border-0..2)
          var(--long/*), var(--short/*), var(--accent/*)
          var(--info/*), var(--warning/*), var(--error/*), var(--success/*), var(--purple/*)
          var(--chart-*), var(--margin-*), var(--pnl-*), var(--status-*), var(--tier-*)
Spacing:  var(--pad-2xs), var(--pad-xs), var(--pad-sm), var(--pad), var(--pad-lg), var(--pad-xl), var(--pad-xxl)
Type:     var(--text-xs..xl), var(--font-normal..bold), var(--font-ui/data/mono)
Borders:  var(--radius-sm..full), var(--shadow-sm..overlay)
Motion:   var(--transition-fast/normal/slow)
```

### Storybook
- Run `npm run storybook` to see all components and their variants
- Token documentation at Design System > Tokens story
- Every component has stories showing all variants, sizes, and states

## Financial Engineering Rules (CRITICAL)

Every financial mechanism must have a documented origin and rationale. This is not a toy — it is a regulated financial system handling real money and real risk.

### Provenance Requirement
- Every formula, rate, threshold, or financial mechanic MUST trace to one of:
  - **Industry standard** (e.g., SPAN margining, Black-Scholes, funding rate = premium index + clamp)
  - **Competitor reference** (Bybit/Binance/OKX documentation with specific section)
  - **Regulatory requirement** (MiCA, FATF, specific jurisdiction rule)
  - **First-principles derivation** with explicit assumptions documented
- If a mechanic cannot cite its origin, it does not ship
- All references documented per-crate in `crates/*/REFERENCES.md` — index at `docs/references/README.md`
- 30 mechanisms with formulas, worked examples, competitor comparison
- Legacy monolith: `docs/BUSINESS_LOGIC_REFERENCES.md` (to be removed after migration verified)

### Margin & Liquidation
- Funding rate mechanics: Bybit perpetual swap docs (premium index + interest rate, clamped ±0.05%)
- Effective margin = margin + unrealized PnL - unpaid funding debt (industry standard: under-margined positions liquidate sooner)
- Liquidation price formulas: must match Bybit's published formulas for linear/inverse contracts
- ADL ranking: by profit ratio × leverage, descending (Bybit ADL mechanism)
- Portfolio margin: SPAN-like scenario analysis across correlated positions

### Money Flow Integrity
- No money created from nothing — every credit has a corresponding debit (double-entry ledger)
- Yield pool (earn/staking): funded by margin interest income, payouts capped to pool balance
- Insurance fund: funded by liquidation surplus, drawn on socialized loss events
- Fee revenue: treasury account receives all trading fees, auditable
- If a payment fails (e.g., funding debit on insufficient balance), track the debt — never silently forgive

### Precision & Rounding
- All monetary calculations use `rust_decimal` (128-bit, 28 significant digits)
- Rounding: truncate in user's favor for debits, truncate against user for credits (conservative)
- Tick size and lot size enforced at order entry, not at matching
- Fee calculations: notional × rate, rounded to 8 decimal places

### What NOT To Do
- NEVER invent financial formulas without reference implementation
- NEVER use fallback/default values for prices, rates, or balances in production paths
- NEVER silently swallow financial errors — log, track, alert
- NEVER hardcode APY/rates — they must come from configuration or market data
- NEVER create "simplified versions" of financial logic for different platforms
- NEVER write tests against imaginary API methods — verify the method exists first

## Workflow & Development Principles

### Planning
- Enter plan mode for ANY non-trivial task (3+ steps or architectural decisions)
- If implementation goes sideways — STOP, re-plan, don't keep pushing broken approach
- Write plan before coding. Think first, code second.

### Development
- **Simplicity first**: make every change as simple as possible, touch minimal code
- **No temporary fixes**: find root causes, senior developer standards
- **Minimal impact**: changes touch only what's necessary, avoid introducing new bugs
- For non-trivial changes: pause and ask "is there a more elegant way?"
- If a fix feels hacky — step back and implement the clean solution

### Verification
- **ALWAYS** run `cargo test` before considering Rust changes done
- **ALWAYS** run `npx vitest run` before considering web client changes done
- Never mark a task complete without proving it works (tests pass, no regressions)
- After adding new struct fields: grep ALL construction sites and update them
- After writing tests: verify they compile and call real API methods, not imaginary ones

### Reference Implementation
- Primary references: Bybit API docs, Binance API docs
- When implementing a feature, check how Bybit/Binance do it first
- Match their behavior unless we have a specific, documented reason to differ
- "How Bybit does it" is not blindly copied — understand WHY, then decide if we can do better

## Key Design Decisions
- Fill at maker's price (price improvement for taker)
- Market order remainder cancelled (not left on book)
- Single thread per symbol (no locks, no races in matching)
- Price-time priority (strict FIFO within price level)
- WAL before matching (crash recovery via replay)
- DB write-behind (async persistence, WAL is source of truth)
- Double-entry ledger for all financial movements
- Phase 1: monolith (single binary), Phase 2: services

## Environment
- `DATABASE_URL` — PostgreSQL (optional, in-memory without it)
- `DEV_MODE` — auto-set when no DATABASE_URL; enables nil-user bypass
- `PRODUCTION` — set to "true" for production mode (enforces DATABASE_URL, JWT_SECRET, WEBHOOK_SECRET; panics if DEV_MODE also set)
- `JWT_SECRET` — required in production for auth token signing
- `WEBHOOK_SECRET` — required in production for webhook verification
- `SMTP_HOST/PORT/USER/PASSWORD` — email notifications
- `FIX_PORT` — FIX gateway port (default 8081)
- `ALLOWED_ORIGINS` — CORS origins
- `RUST_LOG` — log level

## Architecture Notes
- `Exchange` struct in `api-gateway/src/exchange.rs` is the god object (70+ Mutex fields)
- Background tasks in `api-gateway/src/tasks.rs` (liquidation, funding, triggers, GTD, algos, bots, AML, deposits, DMS)
- Routes registered in `api-gateway/src/server.rs` (private, public, admin routers)
- REST handlers in `api-gateway/src/rest.rs` (~7000 lines) and `rest_wave1.rs`
- Well-known accounts: `TREASURY_ACCOUNT_ID` (fee revenue), `YIELD_POOL_ACCOUNT_ID` (earn/staking payouts)

## Web Client (clients/web/)
```
clients/web/src/
  app/              # Root App, Router, routes, layouts (FullPage, Terminal, Dashboard)
  features/         # Feature-sliced modules (~33 features)
    trading/        # Trading terminal (OrderBook, OrderEntry, Chart, BottomPanel, TickerBar)
    account/        # Auth, settings, security, API keys
    wallet/         # Balances, deposit, withdraw
    earn/           # Savings, staking
    ...             # Each feature: page + components/ + api.ts + types.ts + stores/
  shared/           # Cross-feature code
    api/            # HTTP client (auto token refresh), WebSocket (auto reconnect), types
    ui/             # Design system (Button, Modal, Table, Toast, Spinner, etc.)
    hooks/          # useFetch
    stores/         # theme
    utils/          # format helpers
  styles/           # CSS tokens, themes, base styles
```
- React 18 + Vite + TypeScript strict + Zustand + CSS Modules
- Lightweight Charts for trading charts
- Vitest + Testing Library for tests, Playwright for E2E
- No external UI framework — custom design system in shared/ui/
- PWA-ready (manifest.json, service worker)

## Expert Agents (slash commands)

Use these for specialized reviews and planning:
- `/architect` - system architecture review (scalability, latency, fault tolerance)
- `/trading-qa` - trading logic validation (signals, risk, edge cases)
- `/systems-qa` - reliability testing (failure modes, race conditions)
- `/developer` - code quality review (Rust/TS best practices)
- `/ui-designer` - trading terminal UX review
- `/pm` - task breakdown, planning, progress tracking

## Task Tracking

- Current work tracked in `docs/TASK_TRACKER.md` — update each session
- Each task must have: status (TODO/IN_PROGRESS/DONE), clear scope, acceptance criteria
- Before starting a session: read TASK_TRACKER.md to understand current state
- After completing work: update TASK_TRACKER.md with results
- No work starts without a corresponding task entry

## Related Projects

- `../bot-trader-2-0/crates/core/` — analytics engine, may be reused for surveillance
- `../exchange-charts/` — custom charting library (Canvas 2D, indicators, drawing tools). Replaces lightweight-charts in trading terminal when ready.
