# Production Marathon V2 — From Prototype to Working Exchange

## Current State: Prototype
- 28 crates, 1255+ tests, 105K lines Rust
- 18 trading pairs (10 perp + 8 spot)
- Enriched API (better than Binance)
- Frontend exists but UI is far from production
- EVERYTHING is in-memory — no DB integration
- No proper accounting system
- No real price feeds

## What's Needed for a WORKING Exchange

### Priority 1: Accounting System (THE FOUNDATION)

Without this, nothing else matters. Every financial operation must be tracked.

#### A1. Double-Entry Ledger
- Every balance change = two entries (debit + credit)
- Ledger entries: id, account_id, asset, debit, credit, balance_after, reference_type, reference_id, timestamp
- Reference types: deposit, withdrawal, trade_fill, fee, funding_payment, liquidation, transfer, adjustment
- Sum of all debits MUST equal sum of all credits (invariant)
- Reconciliation: ledger balance == account balance at all times

#### A2. Account System (DB-backed)
- Replace in-memory AccountManager with PostgreSQL
- User Account → multiple Wallets (Spot Wallet, Derivatives Wallet)
- Each wallet has balances per asset
- Balance = sum of all ledger entries for that account+asset
- Available = total - locked (orders, positions)

#### A3. Margin Engine (DB-backed)
- Open position → lock initial margin from derivatives wallet
- Price moves → unrealized PnL changes available margin (cross mode)
- Close position → unlock margin + credit/debit realized PnL
- Liquidation → force close + insurance fund interaction
- All operations through ledger (auditable)

#### A4. Fee System (DB-backed)
- Trade fee → deduct from received asset, credit to exchange fee account
- Funding payment → debit payer, credit receiver (through ledger)
- Withdrawal fee → deduct from withdrawal amount, credit to exchange
- Maker rebate → credit maker from exchange fee pool

#### A5. Settlement Engine
- Fill → create ledger entries: buyer pays quote, receives base (minus fee)
- Futures fill → update position, adjust margin in ledger
- Funding → batch ledger entries every 8h
- All atomic in PostgreSQL transaction

### Priority 2: Database Integration

#### D1. PostgreSQL for Everything
- Users, accounts, wallets, balances → DB
- Orders (open + history) → DB
- Trades → DB
- Positions → DB
- Ledger entries → DB
- KYC records → DB
- Audit log → DB

#### D2. Matching Engine State Recovery
- WAL replay → rebuild orderbook on startup
- Open orders in DB → verify against WAL state
- Reconcile: DB balances == sum(ledger entries)

#### D3. Connection Pool + Transactions
- PgPool with proper sizing
- Balance operations: SELECT FOR UPDATE
- Settlement: single transaction (4 ledger entries per fill)
- Audit log: append-only (no UPDATE/DELETE permissions)

### Priority 3: Real Price Feeds

#### P1. External WebSocket Connections
- Binance: btcusdt@miniTicker (already coded, not connected)
- Coinbase, Kraken, OKX feeds
- Feed → IndexPriceAggregator → Mark Price → Funding Rate

#### P2. Mark Price Engine
- mark_price = f(index_price, last_price, funding_rate)
- Updates every second
- Used for: PnL calculation, liquidation checks, funding settlement

#### P3. Funding Rate Engine
- rate = clamp((perp_price - index_price) / index_price, ±max_rate)
- Settlement every 8h (00:00, 08:00, 16:00 UTC)
- Creates ledger entries for every open position

### Priority 4: UI Redesign (Bybit-level)

#### U1. Layout Restructure
- Remove sidebar pair list → dropdown selector (like Bybit)
- Orderbook + Order Form side by side (right column split)
- Trades under orderbook
- Chart gets full center space

#### U2. Component Polish
- Every component pixel-matched to Bybit reference
- Proper typography, spacing, colors
- Animations, transitions, hover states
- Responsive (desktop-first, then tablet/mobile)

#### U3. Real-Time Data
- All panels update via WebSocket
- No polling (except initial load)
- Price flash animations
- Orderbook smooth updates

### Priority 5: Security & Compliance

#### S1. Proper Auth Flow
- Register → verify email → login → 2FA
- JWT + refresh tokens (already coded)
- Session management

#### S2. Withdrawal Security
- Address whitelist with 24h cooldown
- 2FA verification on withdrawal
- Daily limits by KYC tier
- Admin approval for large amounts

## Sprint Plan

### Sprint 1 (Week 1-2): Accounting Ledger + DB
- Design ledger schema
- Implement LedgerService (double-entry)
- Migrate AccountManager to PostgreSQL
- Wire settlement to create ledger entries
- Test: full trade flow creates correct ledger entries
- Test: ledger always balances (sum debits == sum credits)

### Sprint 2 (Week 3-4): Margin + Funding + Fees in DB
- Margin engine writes to DB
- Fee deduction through ledger
- Funding settlement through ledger
- Proper position tracking in DB
- Test: position lifecycle fully in DB

### Sprint 3 (Week 5-6): Real Price Feeds + Mark Price
- Connect Binance WS feed
- Wire to IndexPriceAggregator
- Mark price updates every second
- Funding rate calculation
- Ticker shows real mark/index/funding

### Sprint 4 (Week 7-8): UI Redesign
- Layout restructure (Bybit-style)
- Component by component polish
- Real-time data wiring
- Demo mode with full data

### Sprint 5 (Week 9-10): Auth + Security + Polish
- Email verification
- 2FA enforcement
- Withdrawal security flow
- Admin panel wiring
- E2E testing

### Sprint 6 (Week 11-12): Load Testing + Launch Prep
- Load test with DB
- Performance optimization
- Backup/restore drill
- Beta user onboarding
