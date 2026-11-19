# Research: Exchange Accounting Systems

## Key Findings

### Double-Entry Ledger (THE CORE)
- Every balance change = debit + credit (zero-sum per transaction)
- `ledger_entries` table: transaction_id, account_id, amount (signed), balance_before, balance_after
- For every transaction_id: SUM(amounts) MUST be zero
- Balance is stored (for speed) but verified against SUM(ledger entries)

### Account Architecture
- User accounts + System accounts (fee revenue, insurance fund, hot/warm/cold wallets)
- Bybit UTA: single account with collateral value ratios per asset
- Binance: separate wallets (Spot, Margin, Futures) with internal transfers

### Settlement (atomic DB transaction per fill)
- Spot: 6-7 UPDATEs + 1 INSERT in one transaction
- Futures: update position, adjust margin, no asset transfer
- Fees credited to exchange fee account
- Maker rebates: reverse flow (exchange → maker)

### Missing in Our Schema
1. NO ledger_entries table (critical)
2. NO system/internal accounts (fees go nowhere)
3. NO positions table in DB
4. NO funding_payments table in DB
5. Balance not ledger-verified

### Reference Implementations
- TigerBeetle (Zig) — purpose-built financial DB
- pgledger (PostgreSQL) — pure SQL double-entry
- Formance Ledger (Go) — programmable financial core
- Peatio (Ruby) — full exchange with accounting
- Stripe Ledger — 5B events/day, 99.9999% explainability
