//! Web3 wallet and cross-chain bridge for the exchange.
//!
//! # Features
//! - MPC wallet management (Phase 1: simulated deterministic key derivation).
//! - Multi-chain address generation (EVM + Solana).
//! - Token balance aggregation across all supported chains.
//! - Cross-chain bridge abstraction (native bridge, Circle CCTP, LayerZero, Wormhole).
//! - WalletConnect-like dApp session management.
//! - On-chain transaction building and signing (Phase 1: simulated).
//!
//! # Phase 2 upgrade path
//! - Replace `wallet::WalletManager::generate_addresses` with real threshold-ECDSA MPC.
//! - Replace `transactions::TransactionBuilder` with live chain submission.
//! - Replace `tokens::simulated_balances` with real RPC node queries.
//! - Replace `dapp_connect::DAppConnector` with WalletConnect v2 relay protocol.

pub mod bridge;
pub mod chains;
pub mod dapp_connect;
pub mod tokens;
pub mod transactions;
pub mod types;
pub mod wallet;

pub use bridge::{BridgeManager, BridgeProvider, BridgeQuote, BridgeRoute};
pub use chains::{all_chain_configs, chain_config, ChainConfig};
pub use dapp_connect::DAppConnector;
pub use tokens::{all_balances, simulated_balances, total_value_usd};
pub use transactions::TransactionBuilder;
pub use types::{
    BackupStatus, Chain, DAppPermission, DAppSession, TokenBalance, TxStatus, TxType,
    Web3Transaction, Web3Wallet,
};
pub use wallet::WalletManager;
