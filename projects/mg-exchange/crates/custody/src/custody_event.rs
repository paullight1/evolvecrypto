//! CustodyEvent types matching the mg-custody webhook payload format.
//!
//! The custody service sends these as tagged JSON enums:
//!   `{ "event": "deposit_confirmed", "data": { ... } }`
//!
//! Reference: mg-custody/crates/common/src/types.rs

use rust_decimal::Decimal;
use serde::Deserialize;
use uuid::Uuid;

/// Events received from the custody service via webhook.
///
/// Wire format: `#[serde(tag = "event", content = "data")]` with `rename_all = "snake_case"`.
/// This must match mg-custody's `CustodyEvent` enum exactly.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "event", content = "data")]
#[serde(rename_all = "snake_case")]
pub enum CustodyEvent {
    /// A deposit has reached the required number of confirmations.
    DepositConfirmed {
        deposit_id: Uuid,
        user_address: String,
        user_id: Uuid,
        asset: String,
        chain: String,
        amount: Decimal,
        tx_hash: String,
        confirmations: u64,
        block_height: u64,
    },

    /// A withdrawal transaction has been broadcast on-chain.
    WithdrawalBroadcast {
        withdrawal_id: Uuid,
        exchange_withdrawal_id: Uuid,
        tx_hash: String,
        asset: String,
        chain: String,
        amount: Decimal,
        fee: Decimal,
    },

    /// A withdrawal transaction has reached finality.
    WithdrawalConfirmed {
        withdrawal_id: Uuid,
        exchange_withdrawal_id: Uuid,
        tx_hash: String,
        block_height: u64,
        confirmations: u64,
    },

    /// A withdrawal has failed (signing error, insufficient gas, chain rejection, etc.).
    WithdrawalFailed {
        withdrawal_id: Uuid,
        exchange_withdrawal_id: Uuid,
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_deposit_confirmed() {
        let json = r#"{
            "event": "deposit_confirmed",
            "data": {
                "deposit_id": "550e8400-e29b-41d4-a716-446655440000",
                "user_address": "0x1234567890abcdef1234567890abcdef12345678",
                "user_id": "660e8400-e29b-41d4-a716-446655440000",
                "asset": "ETH",
                "chain": "ethereum",
                "amount": "1.5",
                "tx_hash": "0xabc123",
                "confirmations": 12,
                "block_height": 19000000
            }
        }"#;

        let event: CustodyEvent = serde_json::from_str(json).unwrap();
        match event {
            CustodyEvent::DepositConfirmed { asset, amount, .. } => {
                assert_eq!(asset, "ETH");
                assert_eq!(amount, Decimal::new(15, 1));
            }
            _ => panic!("Expected DepositConfirmed"),
        }
    }

    #[test]
    fn deserialize_withdrawal_broadcast() {
        let json = r#"{
            "event": "withdrawal_broadcast",
            "data": {
                "withdrawal_id": "550e8400-e29b-41d4-a716-446655440000",
                "exchange_withdrawal_id": "660e8400-e29b-41d4-a716-446655440000",
                "tx_hash": "0xdef456",
                "asset": "BTC",
                "chain": "bitcoin",
                "amount": "0.5",
                "fee": "0.00005"
            }
        }"#;

        let event: CustodyEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(event, CustodyEvent::WithdrawalBroadcast { .. }));
    }

    #[test]
    fn deserialize_withdrawal_failed() {
        let json = r#"{
            "event": "withdrawal_failed",
            "data": {
                "withdrawal_id": "550e8400-e29b-41d4-a716-446655440000",
                "exchange_withdrawal_id": "660e8400-e29b-41d4-a716-446655440000",
                "reason": "insufficient gas"
            }
        }"#;

        let event: CustodyEvent = serde_json::from_str(json).unwrap();
        match event {
            CustodyEvent::WithdrawalFailed { reason, .. } => {
                assert_eq!(reason, "insufficient gas");
            }
            _ => panic!("Expected WithdrawalFailed"),
        }
    }
}
