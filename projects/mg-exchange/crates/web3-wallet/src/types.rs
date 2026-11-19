//! Core types for the Web3 wallet subsystem.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// EVM-compatible and non-EVM chains supported by the Web3 wallet.
///
/// Mirrors `exchange_dex_aggregator::Chain` but is intentionally a separate
/// type — the wallet crate must not depend on the DEX aggregator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Chain {
    Ethereum,
    Arbitrum,
    Base,
    Polygon,
    Bsc,
    Solana,
    Avalanche,
    Optimism,
}

impl Chain {
    /// Returns the EVM chain ID, or `None` for non-EVM chains (e.g. Solana).
    pub fn chain_id(self) -> Option<u64> {
        match self {
            Chain::Ethereum  => Some(1),
            Chain::Arbitrum  => Some(42161),
            Chain::Base      => Some(8453),
            Chain::Polygon   => Some(137),
            Chain::Bsc       => Some(56),
            Chain::Solana    => None,
            Chain::Avalanche => Some(43114),
            Chain::Optimism  => Some(10),
        }
    }

    /// All supported chains, in a stable canonical order.
    pub fn all() -> &'static [Chain] {
        &[
            Chain::Ethereum,
            Chain::Arbitrum,
            Chain::Base,
            Chain::Polygon,
            Chain::Bsc,
            Chain::Solana,
            Chain::Avalanche,
            Chain::Optimism,
        ]
    }

    /// Human-readable identifier used in logs and API responses.
    pub fn label(self) -> &'static str {
        match self {
            Chain::Ethereum  => "ethereum",
            Chain::Arbitrum  => "arbitrum",
            Chain::Base      => "base",
            Chain::Polygon   => "polygon",
            Chain::Bsc       => "bsc",
            Chain::Solana    => "solana",
            Chain::Avalanche => "avalanche",
            Chain::Optimism  => "optimism",
        }
    }

    /// Parse a chain label (case-insensitive).
    pub fn from_label(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "ethereum" | "eth" => Some(Chain::Ethereum),
            "arbitrum" | "arb" => Some(Chain::Arbitrum),
            "base"              => Some(Chain::Base),
            "polygon" | "matic" => Some(Chain::Polygon),
            "bsc" | "bnb"       => Some(Chain::Bsc),
            "solana" | "sol"    => Some(Chain::Solana),
            "avalanche" | "avax"=> Some(Chain::Avalanche),
            "optimism" | "op"   => Some(Chain::Optimism),
            _                   => None,
        }
    }
}

impl std::fmt::Display for Chain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Backup status of a Web3 wallet's key material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupStatus {
    /// Key material has not been backed up — user is at risk of loss.
    NotBackedUp,
    /// Key shares are stored in the exchange's cloud MPC infrastructure.
    CloudBackup,
    /// User has exported and stored a BIP-39 recovery phrase.
    RecoveryPhraseExported,
}

/// A Web3 custodial wallet: one address per supported chain, all derived
/// from the same MPC key material.
///
/// Phase 1: simulated with deterministic address derivation.
/// Phase 2: replace with real MPC / threshold-ECDSA.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Web3Wallet {
    pub wallet_id: Uuid,
    pub user_id: Uuid,
    /// One on-chain address per chain.
    pub addresses: HashMap<Chain, String>,
    pub created_at: DateTime<Utc>,
    pub backup_status: BackupStatus,
}

/// Token balance for a single asset on a single chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBalance {
    pub chain: Chain,
    /// Token symbol (e.g. "ETH", "USDC") or EVM contract address.
    pub token: String,
    pub balance: Decimal,
    /// Approximate USD value at query time.
    pub value_usd: Decimal,
}

/// Type of on-chain transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TxType {
    /// Token transfer to another address.
    Send,
    /// On-chain token swap.
    Swap,
    /// Cross-chain bridge transfer.
    Bridge,
    /// ERC-20 spend approval.
    Approve,
    /// Generic dApp interaction (contract call).
    DAppInteraction,
}

/// Lifecycle status of an on-chain transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status", content = "reason")]
pub enum TxStatus {
    /// Submitted but not yet included in a block.
    Pending,
    /// Included in a finalized block.
    Confirmed,
    /// Transaction reverted or dropped.
    Failed(String),
}

/// An on-chain transaction record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Web3Transaction {
    pub id: Uuid,
    pub chain: Chain,
    pub tx_type: TxType,
    pub from: String,
    pub to: String,
    pub amount: Decimal,
    /// Token symbol or contract address.
    pub token: String,
    /// Estimated gas cost in USD at submission time.
    pub gas_estimate_usd: Decimal,
    pub status: TxStatus,
    /// On-chain transaction hash, once submitted.
    pub tx_hash: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Permission that a dApp session may exercise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DAppPermission {
    /// Read the wallet address (no signing ability).
    ViewAddress,
    /// Sign and submit on-chain transactions.
    SignTransaction,
    /// Sign arbitrary messages (e.g. off-chain auth).
    SignMessage,
}

/// A WalletConnect-like session between the user's wallet and a dApp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DAppSession {
    /// Unique session identifier (URI-safe string).
    pub session_id: String,
    pub user_id: Uuid,
    pub dapp_name: String,
    pub dapp_url: String,
    pub chain: Chain,
    pub permissions: Vec<DAppPermission>,
    pub connected_at: DateTime<Utc>,
    /// Sessions expire after 24 hours unless the dApp refreshes them.
    pub expires_at: DateTime<Utc>,
    pub is_approved: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_round_trip_label() {
        for &chain in Chain::all() {
            let label = chain.label();
            let parsed = Chain::from_label(label).expect("label must round-trip");
            assert_eq!(chain, parsed);
        }
    }

    #[test]
    fn chain_id_solana_is_none() {
        assert_eq!(Chain::Solana.chain_id(), None);
    }

    #[test]
    fn chain_id_ethereum_is_1() {
        assert_eq!(Chain::Ethereum.chain_id(), Some(1));
    }

    #[test]
    fn chain_from_label_aliases() {
        assert_eq!(Chain::from_label("eth"),  Some(Chain::Ethereum));
        assert_eq!(Chain::from_label("arb"),  Some(Chain::Arbitrum));
        assert_eq!(Chain::from_label("avax"), Some(Chain::Avalanche));
        assert_eq!(Chain::from_label("bnb"),  Some(Chain::Bsc));
    }

    #[test]
    fn chain_from_label_unknown_is_none() {
        assert_eq!(Chain::from_label("cosmos"), None);
    }
}
