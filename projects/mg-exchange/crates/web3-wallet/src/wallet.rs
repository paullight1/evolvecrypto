//! MPC wallet management (Phase 1: simulated deterministic derivation).
//!
//! # Phase 1 (current)
//! Address generation is deterministic from `user_id` + chain via SHA-256.
//! No real keys are created; this is a simulation of MPC key sharding.
//!
//! # Phase 2 (future)
//! Replace `generate_addresses` with a real threshold-ECDSA library
//! (e.g. `tss-ecdsa` or a hardware-backed HSM integration).

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use chrono::Utc;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::types::{BackupStatus, Chain, Web3Wallet};

/// Manages Web3 custodial wallets for exchange users.
///
/// One wallet per user, one address per supported chain.
pub struct WalletManager {
    wallets: HashMap<Uuid, Web3Wallet>,
}

impl WalletManager {
    pub fn new() -> Self {
        Self { wallets: HashMap::new() }
    }

    /// Create a new Web3 wallet for the given user.
    ///
    /// Returns an error if the user already has a wallet.
    pub fn create_wallet(&mut self, user_id: Uuid) -> Result<Web3Wallet> {
        if self.wallets.contains_key(&user_id) {
            return Err(anyhow!("user {user_id} already has a Web3 wallet"));
        }

        let addresses = Self::generate_addresses(user_id);
        let wallet = Web3Wallet {
            wallet_id: Uuid::new_v4(),
            user_id,
            addresses,
            created_at: Utc::now(),
            backup_status: BackupStatus::CloudBackup,
        };

        tracing::info!(user_id = %user_id, wallet_id = %wallet.wallet_id, "Web3 wallet created");
        self.wallets.insert(user_id, wallet.clone());
        Ok(wallet)
    }

    /// Return the wallet for a user, if one exists.
    pub fn get_wallet(&self, user_id: Uuid) -> Option<&Web3Wallet> {
        self.wallets.get(&user_id)
    }

    /// Return the on-chain address for a specific chain, if the user has a wallet.
    pub fn get_address(&self, user_id: Uuid, chain: Chain) -> Option<&str> {
        self.wallets
            .get(&user_id)?
            .addresses
            .get(&chain)
            .map(|s| s.as_str())
    }

    /// Update the backup status of a user's wallet.
    pub fn set_backup_status(&mut self, user_id: Uuid, status: BackupStatus) -> Result<()> {
        let wallet = self
            .wallets
            .get_mut(&user_id)
            .ok_or_else(|| anyhow!("no Web3 wallet for user {user_id}"))?;
        wallet.backup_status = status;
        Ok(())
    }

    /// Derive one address per chain from the user's ID.
    ///
    /// Phase 1: deterministic via SHA-256(user_id_bytes || chain_label).
    /// EVM chains share address format (0x-prefixed, 20 bytes = 40 hex chars).
    /// Solana uses a 32-byte base58-like hex address (simplified simulation).
    fn generate_addresses(user_id: Uuid) -> HashMap<Chain, String> {
        Chain::all()
            .iter()
            .map(|&chain| {
                let address = Self::derive_address(user_id, chain);
                (chain, address)
            })
            .collect()
    }

    fn derive_address(user_id: Uuid, chain: Chain) -> String {
        let mut hasher = Sha256::new();
        hasher.update(user_id.as_bytes());
        hasher.update(chain.label().as_bytes());
        let digest = hasher.finalize();

        if chain == Chain::Solana {
            // Solana: 32 bytes expressed as hex (simplified — Phase 2 will use base58)
            hex::encode(&digest[..])
        } else {
            // EVM: "0x" + last 20 bytes (40 hex chars)
            format!("0x{}", hex::encode(&digest[12..]))
        }
    }
}

impl Default for WalletManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_wallet_generates_all_chains() {
        let mut mgr = WalletManager::new();
        let user_id = Uuid::new_v4();
        let wallet = mgr.create_wallet(user_id).unwrap();

        assert_eq!(wallet.user_id, user_id);
        assert_eq!(wallet.addresses.len(), Chain::all().len());

        // Every chain must have an entry.
        for &chain in Chain::all() {
            assert!(wallet.addresses.contains_key(&chain), "missing chain {chain}");
        }
    }

    #[test]
    fn evm_address_has_correct_format() {
        let mut mgr = WalletManager::new();
        let user_id = Uuid::new_v4();
        let wallet = mgr.create_wallet(user_id).unwrap();

        let eth_addr = &wallet.addresses[&Chain::Ethereum];
        assert!(eth_addr.starts_with("0x"), "EVM address must start with 0x");
        assert_eq!(eth_addr.len(), 42, "EVM address must be 42 chars (0x + 40 hex)");
    }

    #[test]
    fn solana_address_has_64_hex_chars() {
        let mut mgr = WalletManager::new();
        let user_id = Uuid::new_v4();
        let wallet = mgr.create_wallet(user_id).unwrap();

        let sol_addr = &wallet.addresses[&Chain::Solana];
        assert!(!sol_addr.starts_with("0x"), "Solana address must not start with 0x");
        assert_eq!(sol_addr.len(), 64, "Solana address must be 64 hex chars (32 bytes)");
    }

    #[test]
    fn address_generation_is_deterministic() {
        let user_id = Uuid::from_u128(0x1234_5678_9abc_def0_1234_5678_9abc_def0);

        let addr1 = WalletManager::derive_address(user_id, Chain::Ethereum);
        let addr2 = WalletManager::derive_address(user_id, Chain::Ethereum);
        assert_eq!(addr1, addr2, "address derivation must be deterministic");
    }

    #[test]
    fn different_chains_produce_different_addresses() {
        let user_id = Uuid::new_v4();
        let eth = WalletManager::derive_address(user_id, Chain::Ethereum);
        let arb = WalletManager::derive_address(user_id, Chain::Arbitrum);
        assert_ne!(eth, arb, "different chains must produce different addresses");
    }

    #[test]
    fn duplicate_wallet_creation_is_rejected() {
        let mut mgr = WalletManager::new();
        let user_id = Uuid::new_v4();
        mgr.create_wallet(user_id).unwrap();
        let err = mgr.create_wallet(user_id).unwrap_err();
        assert!(err.to_string().contains("already has a Web3 wallet"));
    }

    #[test]
    fn get_wallet_returns_none_for_unknown_user() {
        let mgr = WalletManager::new();
        assert!(mgr.get_wallet(Uuid::new_v4()).is_none());
    }

    #[test]
    fn get_address_returns_correct_chain_address() {
        let mut mgr = WalletManager::new();
        let user_id = Uuid::new_v4();
        let wallet = mgr.create_wallet(user_id).unwrap();

        let expected = wallet.addresses[&Chain::Polygon].clone();
        let got = mgr.get_address(user_id, Chain::Polygon).unwrap();
        assert_eq!(got, expected);
    }
}
