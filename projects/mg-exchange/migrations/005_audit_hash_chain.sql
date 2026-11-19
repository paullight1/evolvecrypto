-- Add hash-chain columns to audit_log for tamper-evident durability.
-- prev_hash/entry_hash form a SHA-256 chain; verify_integrity walks it.
ALTER TABLE audit_log ADD COLUMN IF NOT EXISTS prev_hash VARCHAR(64) NOT NULL DEFAULT 'genesis';
ALTER TABLE audit_log ADD COLUMN IF NOT EXISTS entry_hash VARCHAR(64) NOT NULL DEFAULT '';
