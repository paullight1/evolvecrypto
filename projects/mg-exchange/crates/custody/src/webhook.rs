//! Fireblocks webhook handler for deposit detection.
//!
//! Fireblocks sends webhooks for:
//! - TRANSACTION_CREATED — new tx detected
//! - TRANSACTION_STATUS_UPDATED — status change (confirming, completed, failed)
//!
//! For deposits: source=UNKNOWN/EXTERNAL, destination=VAULT_ACCOUNT
//! We detect these and credit user balances.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::fireblocks::WebhookTransactionData;

/// Processed deposit event (extracted from Fireblocks webhook).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepositEvent {
    pub fireblocks_tx_id: String,
    pub vault_id: String,
    pub asset_id: String,
    pub amount: Decimal,
    pub tx_hash: Option<String>,
    pub confirmations: u32,
    pub status: DepositEventStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DepositEventStatus {
    Detected,
    Confirming,
    Confirmed,
    Failed,
}

/// Parse a Fireblocks webhook transaction into a deposit event (if applicable).
///
/// Returns None if this transaction is not an incoming deposit.
pub fn parse_deposit_event(data: &WebhookTransactionData) -> Option<DepositEvent> {
    // Only process incoming deposits (destination = our vault)
    let dest_type = data.destination_type.as_deref()?;
    if dest_type != "VAULT_ACCOUNT" {
        return None;
    }

    // Source must be external (not internal transfer)
    let source_type = data.source_type.as_deref().unwrap_or("UNKNOWN");
    if source_type == "VAULT_ACCOUNT" {
        return None; // Internal transfer, not a deposit
    }

    let vault_id = data.destination_id.clone()?;
    let amount: Decimal = data.amount.parse().ok()?;

    let status = match data.status.as_str() {
        "COMPLETED" => DepositEventStatus::Confirmed,
        "CONFIRMING" => DepositEventStatus::Confirming,
        "FAILED" | "REJECTED" | "CANCELLED" | "BLOCKED" => DepositEventStatus::Failed,
        _ => DepositEventStatus::Detected,
    };

    Some(DepositEvent {
        fireblocks_tx_id: data.id.clone(),
        vault_id,
        asset_id: data.asset_id.clone(),
        amount,
        tx_hash: data.tx_hash.clone(),
        confirmations: data.num_of_confirmations.unwrap_or(0),
        status,
    })
}

/// Map Fireblocks asset ID back to our internal asset name.
pub fn fireblocks_id_to_asset(fb_asset: &str) -> Option<(&'static str, &'static str)> {
    match fb_asset {
        "BTC" => Some(("BTC", "bitcoin")),
        "ETH" => Some(("ETH", "ethereum")),
        "USDT_ERC20" => Some(("USDT", "ethereum")),
        "USDT_TRC20" => Some(("USDT", "tron")),
        "USDT_SOL" => Some(("USDT", "solana")),
        "SOL" => Some(("SOL", "solana")),
        "USDC" => Some(("USDC", "ethereum")),
        "USDC_SOL" => Some(("USDC", "solana")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fireblocks::WebhookTransactionData;

    #[test]
    fn test_parse_incoming_deposit() {
        let data = WebhookTransactionData {
            id: "tx-1".to_string(),
            status: "COMPLETED".to_string(),
            asset_id: "BTC".to_string(),
            amount: "0.5".to_string(),
            source_type: Some("UNKNOWN".to_string()),
            destination_type: Some("VAULT_ACCOUNT".to_string()),
            destination_id: Some("42".to_string()),
            tx_hash: Some("0xabc".to_string()),
            num_of_confirmations: Some(6),
        };

        let event = parse_deposit_event(&data).unwrap();
        assert_eq!(event.vault_id, "42");
        assert_eq!(event.amount, Decimal::new(5, 1));
        assert_eq!(event.status, DepositEventStatus::Confirmed);
        assert_eq!(event.tx_hash, Some("0xabc".to_string()));
    }

    #[test]
    fn test_internal_transfer_not_deposit() {
        let data = WebhookTransactionData {
            id: "tx-2".to_string(),
            status: "COMPLETED".to_string(),
            asset_id: "ETH".to_string(),
            amount: "10".to_string(),
            source_type: Some("VAULT_ACCOUNT".to_string()), // internal
            destination_type: Some("VAULT_ACCOUNT".to_string()),
            destination_id: Some("42".to_string()),
            tx_hash: None,
            num_of_confirmations: None,
        };

        assert!(parse_deposit_event(&data).is_none());
    }

    #[test]
    fn test_outgoing_withdrawal_not_deposit() {
        let data = WebhookTransactionData {
            id: "tx-3".to_string(),
            status: "COMPLETED".to_string(),
            asset_id: "BTC".to_string(),
            amount: "1".to_string(),
            source_type: Some("VAULT_ACCOUNT".to_string()),
            destination_type: Some("ONE_TIME_ADDRESS".to_string()), // external
            destination_id: None,
            tx_hash: None,
            num_of_confirmations: None,
        };

        assert!(parse_deposit_event(&data).is_none());
    }

    #[test]
    fn test_confirming_status() {
        let data = WebhookTransactionData {
            id: "tx-4".to_string(),
            status: "CONFIRMING".to_string(),
            asset_id: "ETH".to_string(),
            amount: "5".to_string(),
            source_type: Some("UNKNOWN".to_string()),
            destination_type: Some("VAULT_ACCOUNT".to_string()),
            destination_id: Some("10".to_string()),
            tx_hash: Some("0xdef".to_string()),
            num_of_confirmations: Some(3),
        };

        let event = parse_deposit_event(&data).unwrap();
        assert_eq!(event.status, DepositEventStatus::Confirming);
        assert_eq!(event.confirmations, 3);
    }

    #[test]
    fn test_fireblocks_id_mapping() {
        assert_eq!(fireblocks_id_to_asset("BTC"), Some(("BTC", "bitcoin")));
        assert_eq!(fireblocks_id_to_asset("USDT_TRC20"), Some(("USDT", "tron")));
        assert_eq!(fireblocks_id_to_asset("SOL"), Some(("SOL", "solana")));
        assert_eq!(fireblocks_id_to_asset("UNKNOWN_COIN"), None);
    }
}
