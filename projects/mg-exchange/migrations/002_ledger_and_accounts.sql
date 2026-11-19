-- Double-Entry Ledger + System Accounts
-- Based on research: TigerBeetle, pgledger, Stripe Ledger patterns

-- System/internal accounts (exchange fee wallet, insurance fund, wallets)
CREATE TABLE IF NOT EXISTS system_accounts (
    id UUID PRIMARY KEY,
    name VARCHAR(50) UNIQUE NOT NULL,
    account_type VARCHAR(20) NOT NULL,  -- 'fee_revenue', 'insurance_fund', 'hot_wallet', 'cold_wallet'
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Seed system accounts
INSERT INTO system_accounts (id, name, account_type, description) VALUES
    ('00000000-0000-0000-0000-000000000001', 'fee_revenue', 'revenue', 'Trading fee revenue (maker + taker fees)'),
    ('00000000-0000-0000-0000-000000000002', 'insurance_fund_btc', 'insurance', 'BTC perpetual insurance fund'),
    ('00000000-0000-0000-0000-000000000003', 'insurance_fund_eth', 'insurance', 'ETH perpetual insurance fund'),
    ('00000000-0000-0000-0000-000000000004', 'insurance_fund_sol', 'insurance', 'SOL perpetual insurance fund'),
    ('00000000-0000-0000-0000-000000000005', 'hot_wallet', 'wallet', 'Hot wallet for automated withdrawals'),
    ('00000000-0000-0000-0000-000000000006', 'warm_wallet', 'wallet', 'Warm wallet buffer'),
    ('00000000-0000-0000-0000-000000000007', 'cold_wallet', 'wallet', 'Cold storage'),
    ('00000000-0000-0000-0000-000000000008', 'margin_escrow', 'escrow', 'Locked margin for open futures positions'),
    ('00000000-0000-0000-0000-000000000009', 'futures_pnl_pool', 'pool', 'Counterparty pool for realized PnL settlement')
ON CONFLICT (id) DO NOTHING;

-- User wallets (spot vs derivatives separation)
CREATE TABLE IF NOT EXISTS wallets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id),
    wallet_type VARCHAR(15) NOT NULL CHECK (wallet_type IN ('spot', 'derivatives')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    UNIQUE (user_id, wallet_type)
);

CREATE INDEX IF NOT EXISTS idx_wallets_user ON wallets(user_id);

-- Double-entry ledger — THE CORE ACCOUNTING TABLE
-- Every balance change creates entries that sum to zero per transaction_id
CREATE TABLE IF NOT EXISTS ledger_entries (
    id BIGSERIAL PRIMARY KEY,
    -- Groups entries that belong to the same business event
    -- For every transaction_id: SUM(amount) MUST equal zero
    transaction_id UUID NOT NULL,
    -- The account affected
    account_id UUID NOT NULL,  -- user_id or system_account.id
    account_type VARCHAR(20) NOT NULL,  -- 'user_spot', 'user_derivatives', 'system'
    -- Asset and amount
    asset VARCHAR(20) NOT NULL,
    amount DECIMAL(28,12) NOT NULL,  -- signed: positive = credit (inflow), negative = debit (outflow)
    -- What caused this entry
    entry_type VARCHAR(30) NOT NULL,
    -- entry_types: 'trade_fill', 'deposit', 'withdrawal', 'funding_payment',
    --              'fee', 'rebate', 'liquidation', 'transfer', 'margin_lock',
    --              'margin_release', 'realized_pnl', 'insurance_surplus',
    --              'insurance_deficit', 'adjustment'
    -- Reference to the source event
    reference_id VARCHAR(100),  -- order_id, trade_id, deposit_id, etc.
    reference_type VARCHAR(20),  -- 'trade', 'order', 'deposit', 'withdrawal', 'funding', 'transfer'
    -- Balance tracking for audit trail
    balance_before DECIMAL(28,12) NOT NULL,
    balance_after DECIMAL(28,12) NOT NULL,
    -- Metadata
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ledger_account ON ledger_entries(account_id, asset, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_ledger_transaction ON ledger_entries(transaction_id);
CREATE INDEX IF NOT EXISTS idx_ledger_reference ON ledger_entries(reference_type, reference_id);
CREATE INDEX IF NOT EXISTS idx_ledger_type ON ledger_entries(entry_type, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_ledger_created ON ledger_entries(created_at DESC);

-- Positions table (persisted, not in-memory)
CREATE TABLE IF NOT EXISTS positions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id),
    symbol VARCHAR(20) NOT NULL,
    side VARCHAR(5) NOT NULL CHECK (side IN ('Long', 'Short')),
    size DECIMAL(28,12) NOT NULL,
    entry_price DECIMAL(28,12) NOT NULL,
    leverage DECIMAL(28,12) NOT NULL,
    margin_mode VARCHAR(10) NOT NULL CHECK (margin_mode IN ('Isolated', 'Cross')),
    margin DECIMAL(28,12) NOT NULL,
    realized_pnl DECIMAL(28,12) NOT NULL DEFAULT 0,
    funding_accumulated DECIMAL(28,12) NOT NULL DEFAULT 0,
    liquidation_price DECIMAL(28,12),
    status VARCHAR(10) NOT NULL DEFAULT 'Open' CHECK (status IN ('Open', 'Closed', 'Liquidated')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    closed_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_positions_user ON positions(user_id, status);
CREATE INDEX IF NOT EXISTS idx_positions_symbol ON positions(symbol, status);

-- Funding rate history (persisted)
CREATE TABLE IF NOT EXISTS funding_rates (
    id BIGSERIAL PRIMARY KEY,
    symbol VARCHAR(20) NOT NULL,
    rate DECIMAL(28,12) NOT NULL,
    mark_price DECIMAL(28,12) NOT NULL,
    index_price DECIMAL(28,12) NOT NULL,
    settled_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_funding_rates_symbol ON funding_rates(symbol, settled_at DESC);

-- Funding payments (one per position per settlement)
CREATE TABLE IF NOT EXISTS funding_payments (
    id BIGSERIAL PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id),
    symbol VARCHAR(20) NOT NULL,
    position_id UUID NOT NULL,
    rate DECIMAL(28,12) NOT NULL,
    amount DECIMAL(28,12) NOT NULL,  -- signed: negative = paid, positive = received
    mark_price DECIMAL(28,12) NOT NULL,
    ledger_transaction_id UUID,  -- links to ledger_entries
    settled_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_funding_payments_user ON funding_payments(user_id, settled_at DESC);

-- Fee revenue tracking
CREATE TABLE IF NOT EXISTS fee_revenue (
    id BIGSERIAL PRIMARY KEY,
    trade_id BIGINT,
    user_id UUID NOT NULL,
    asset VARCHAR(20) NOT NULL,
    amount DECIMAL(28,12) NOT NULL,  -- positive = revenue, negative = rebate
    fee_type VARCHAR(10) NOT NULL CHECK (fee_type IN ('maker', 'taker', 'rebate', 'withdrawal')),
    ledger_transaction_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_fee_revenue_created ON fee_revenue(created_at DESC);

-- Reconciliation reports (stored for audit)
CREATE TABLE IF NOT EXISTS reconciliation_reports (
    id BIGSERIAL PRIMARY KEY,
    report_date DATE NOT NULL,
    status VARCHAR(15) NOT NULL CHECK (status IN ('Balanced', 'WithinTolerance', 'Discrepancy')),
    details JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- View: verify double-entry invariant
-- Every transaction_id should sum to zero
CREATE OR REPLACE VIEW ledger_invariant_check AS
SELECT
    transaction_id,
    SUM(amount) as total,
    COUNT(*) as entry_count,
    CASE WHEN ABS(SUM(amount)) < 0.000000000001 THEN 'OK' ELSE 'VIOLATION' END as status
FROM ledger_entries
GROUP BY transaction_id
HAVING ABS(SUM(amount)) >= 0.000000000001;

-- View: account balances derived from ledger (for reconciliation)
CREATE OR REPLACE VIEW ledger_balances AS
SELECT
    account_id,
    account_type,
    asset,
    SUM(amount) as computed_balance
FROM ledger_entries
GROUP BY account_id, account_type, asset;
