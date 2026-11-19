-- Rename secret column to secret_encrypted to signal it stores ciphertext.
-- Existing plaintext values will be migrated by the application on first startup.
ALTER TABLE api_keys RENAME COLUMN secret TO secret_encrypted;
