-- Exchange initial schema
-- All monetary values use DECIMAL(28,12) — matches rust_decimal precision

-- Users
CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY,
    email VARCHAR(255) UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Balances (one row per user per asset)
CREATE TABLE IF NOT EXISTS balances (
    user_id UUID NOT NULL REFERENCES users(id),
    asset VARCHAR(20) NOT NULL,
    total DECIMAL(28,12) NOT NULL DEFAULT 0,
    locked DECIMAL(28,12) NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    PRIMARY KEY (user_id, asset),

    -- CRITICAL invariants enforced at DB level
    CONSTRAINT balance_total_non_negative CHECK (total >= 0),
    CONSTRAINT balance_locked_non_negative CHECK (locked >= 0),
    CONSTRAINT balance_available_non_negative CHECK (total - locked >= 0)
);

-- Index for fast balance lookups
CREATE INDEX IF NOT EXISTS idx_balances_user ON balances(user_id);

-- Products (trading pairs)
CREATE TABLE IF NOT EXISTS products (
    symbol VARCHAR(20) PRIMARY KEY,
    base_asset VARCHAR(20) NOT NULL,
    quote_asset VARCHAR(20) NOT NULL,
    tick_size DECIMAL(28,12) NOT NULL,
    lot_size DECIMAL(28,12) NOT NULL,
    min_notional DECIMAL(28,12) NOT NULL,
    maker_fee DECIMAL(28,12) NOT NULL,
    taker_fee DECIMAL(28,12) NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Orders
CREATE TABLE IF NOT EXISTS orders (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id),
    symbol VARCHAR(20) NOT NULL REFERENCES products(symbol),
    side VARCHAR(4) NOT NULL CHECK (side IN ('Buy', 'Sell')),
    order_type VARCHAR(10) NOT NULL CHECK (order_type IN ('Limit', 'Market')),
    time_in_force VARCHAR(10) NOT NULL DEFAULT 'Gtc',
    price DECIMAL(28,12),
    qty DECIMAL(28,12) NOT NULL,
    filled_qty DECIMAL(28,12) NOT NULL DEFAULT 0,
    status VARCHAR(20) NOT NULL DEFAULT 'Pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_orders_user ON orders(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_orders_symbol_status ON orders(symbol, status);

-- Trades (fills)
CREATE TABLE IF NOT EXISTS trades (
    id BIGSERIAL PRIMARY KEY,
    symbol VARCHAR(20) NOT NULL REFERENCES products(symbol),
    price DECIMAL(28,12) NOT NULL,
    qty DECIMAL(28,12) NOT NULL,
    maker_order_id UUID NOT NULL REFERENCES orders(id),
    taker_order_id UUID NOT NULL REFERENCES orders(id),
    maker_user_id UUID NOT NULL REFERENCES users(id),
    taker_user_id UUID NOT NULL REFERENCES users(id),
    taker_side VARCHAR(4) NOT NULL,
    maker_fee DECIMAL(28,12) NOT NULL DEFAULT 0,
    taker_fee DECIMAL(28,12) NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_trades_symbol ON trades(symbol, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_trades_maker ON trades(maker_user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_trades_taker ON trades(taker_user_id, created_at DESC);

-- KYC records
CREATE TABLE IF NOT EXISTS kyc_records (
    user_id UUID PRIMARY KEY REFERENCES users(id),
    level VARCHAR(10) NOT NULL DEFAULT 'None' CHECK (level IN ('None', 'Basic', 'Full')),
    status VARCHAR(15) NOT NULL DEFAULT 'Unverified' CHECK (status IN ('Unverified', 'Pending', 'Verified', 'Rejected', 'Suspended')),
    provider_ref VARCHAR(255),
    reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- AML alerts
CREATE TABLE IF NOT EXISTS aml_alerts (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id),
    alert_type VARCHAR(30) NOT NULL,
    amount_usd DECIMAL(28,12) NOT NULL,
    description TEXT NOT NULL,
    requires_str BOOLEAN NOT NULL DEFAULT false,
    reviewed BOOLEAN NOT NULL DEFAULT false,
    reviewed_by UUID REFERENCES users(id),
    reviewed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_aml_alerts_user ON aml_alerts(user_id);
CREATE INDEX IF NOT EXISTS idx_aml_alerts_unreviewed ON aml_alerts(reviewed, created_at) WHERE NOT reviewed;

-- Deposits
CREATE TABLE IF NOT EXISTS deposits (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id),
    asset VARCHAR(20) NOT NULL,
    network VARCHAR(20) NOT NULL,
    amount DECIMAL(28,12) NOT NULL,
    address VARCHAR(255) NOT NULL,
    tx_hash VARCHAR(255),
    status VARCHAR(15) NOT NULL DEFAULT 'Pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    credited_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_deposits_user ON deposits(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_deposits_address ON deposits(address);

-- Deposit addresses (cached per user+asset+network)
CREATE TABLE IF NOT EXISTS deposit_addresses (
    user_id UUID NOT NULL REFERENCES users(id),
    asset VARCHAR(20) NOT NULL,
    network VARCHAR(20) NOT NULL,
    address VARCHAR(255) NOT NULL,
    memo VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    PRIMARY KEY (user_id, asset, network)
);

CREATE INDEX IF NOT EXISTS idx_deposit_addresses_address ON deposit_addresses(address);

-- Withdrawals
CREATE TABLE IF NOT EXISTS withdrawals (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id),
    asset VARCHAR(20) NOT NULL,
    network VARCHAR(20) NOT NULL,
    address VARCHAR(255) NOT NULL,
    amount DECIMAL(28,12) NOT NULL,
    fee DECIMAL(28,12) NOT NULL DEFAULT 0,
    status VARCHAR(20) NOT NULL DEFAULT 'PendingApproval',
    provider_tx_id VARCHAR(255),
    tx_hash VARCHAR(255),
    reason TEXT, -- for rejection
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_withdrawals_user ON withdrawals(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_withdrawals_pending ON withdrawals(status) WHERE status = 'PendingApproval';

-- API keys
CREATE TABLE IF NOT EXISTS api_keys (
    key VARCHAR(64) PRIMARY KEY,
    secret VARCHAR(128) NOT NULL,
    user_id UUID NOT NULL REFERENCES users(id),
    label VARCHAR(100) NOT NULL DEFAULT '',
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_api_keys_user ON api_keys(user_id);

-- Audit log (immutable, append-only)
CREATE TABLE IF NOT EXISTS audit_log (
    id BIGSERIAL PRIMARY KEY,
    user_id UUID,
    action VARCHAR(50) NOT NULL,
    detail JSONB NOT NULL DEFAULT '{}',
    ip_address INET,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_audit_log_user ON audit_log(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_log_action ON audit_log(action, created_at DESC);
