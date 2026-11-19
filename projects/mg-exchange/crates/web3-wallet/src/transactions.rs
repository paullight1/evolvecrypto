//! Transaction building and signing (Phase 1: simulated).
//!
//! Phase 2 will replace the simulation with real EVM/Solana transaction signing
//! via the MPC key infrastructure.

use anyhow::{anyhow, Result};
use chrono::Utc;
use rust_decimal::Decimal;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::chains::chain_config;
use crate::types::{Chain, TxStatus, TxType, Web3Transaction};

/// Builds and "signs" (Phase 1: simulates) on-chain transactions.
pub struct TransactionBuilder;

impl TransactionBuilder {
    /// Construct a send-token transaction record.
    ///
    /// Validates that:
    /// - `amount` is positive
    /// - the chain is supported (has a gas config)
    ///
    /// The transaction is immediately set to `TxStatus::Confirmed` in Phase 1
    /// (simulated instant confirmation). Phase 2 will set `Pending` and poll
    /// for confirmation via a chain indexer.
    pub fn build_send(
        chain: Chain,
        from: &str,
        to: &str,
        token: &str,
        amount: Decimal,
    ) -> Result<Web3Transaction> {
        if amount <= Decimal::ZERO {
            return Err(anyhow!("amount must be positive (got {amount})"));
        }
        if to.is_empty() {
            return Err(anyhow!("destination address must not be empty"));
        }

        let gas_usd = Self::estimate_gas_usd(chain)?;
        let tx_hash = Self::simulate_tx_hash(chain, from, to, amount);

        Ok(Web3Transaction {
            id: Uuid::new_v4(),
            chain,
            tx_type: TxType::Send,
            from: from.to_string(),
            to: to.to_string(),
            amount,
            token: token.to_string(),
            gas_estimate_usd: gas_usd,
            status: TxStatus::Confirmed,
            tx_hash: Some(tx_hash),
            created_at: Utc::now(),
        })
    }

    /// Estimate gas cost for a transaction on the given chain.
    fn estimate_gas_usd(chain: Chain) -> Result<Decimal> {
        chain_config(chain)
            .map(|c| c.gas_transfer_usd)
            .ok_or_else(|| anyhow!("unsupported chain: {chain}"))
    }

    /// Derive a deterministic-looking transaction hash from parameters.
    ///
    /// Phase 1: SHA-256 of (chain || from || to || amount || timestamp_micros).
    fn simulate_tx_hash(chain: Chain, from: &str, to: &str, amount: Decimal) -> String {
        let mut hasher = Sha256::new();
        hasher.update(chain.label().as_bytes());
        hasher.update(from.as_bytes());
        hasher.update(to.as_bytes());
        hasher.update(amount.to_string().as_bytes());
        hasher.update(Utc::now().timestamp_micros().to_le_bytes());
        let digest = hasher.finalize();

        if chain == Chain::Solana {
            // Solana tx signatures are 64 bytes (simplified as hex here)
            hex::encode(&digest[..])
        } else {
            // EVM tx hashes are "0x" + 32 bytes hex
            format!("0x{}", hex::encode(&digest[..]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn build_send_succeeds_for_valid_inputs() {
        let tx = TransactionBuilder::build_send(
            Chain::Ethereum,
            "0xabc",
            "0xdef",
            "ETH",
            dec!(1.5),
        ).unwrap();

        assert_eq!(tx.chain, Chain::Ethereum);
        assert_eq!(tx.token, "ETH");
        assert_eq!(tx.amount, dec!(1.5));
        assert!(tx.tx_hash.is_some());
        assert!(matches!(tx.status, TxStatus::Confirmed));
    }

    #[test]
    fn build_send_rejects_zero_amount() {
        let err = TransactionBuilder::build_send(
            Chain::Ethereum, "0xabc", "0xdef", "ETH", Decimal::ZERO,
        ).unwrap_err();
        assert!(err.to_string().contains("amount must be positive"));
    }

    #[test]
    fn build_send_rejects_empty_destination() {
        let err = TransactionBuilder::build_send(
            Chain::Ethereum, "0xabc", "", "ETH", dec!(1),
        ).unwrap_err();
        assert!(err.to_string().contains("destination address"));
    }

    #[test]
    fn evm_tx_hash_starts_with_0x() {
        let tx = TransactionBuilder::build_send(
            Chain::Arbitrum, "0xaaa", "0xbbb", "USDC", dec!(100),
        ).unwrap();
        let hash = tx.tx_hash.unwrap();
        assert!(hash.starts_with("0x"), "EVM hash must start with 0x");
    }

    #[test]
    fn solana_tx_hash_does_not_start_with_0x() {
        let tx = TransactionBuilder::build_send(
            Chain::Solana, "alice", "bob", "SOL", dec!(10),
        ).unwrap();
        let hash = tx.tx_hash.unwrap();
        assert!(!hash.starts_with("0x"), "Solana hash must not start with 0x");
    }
}
