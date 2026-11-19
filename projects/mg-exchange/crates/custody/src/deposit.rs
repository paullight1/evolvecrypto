use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::provider::{CustodyProvider, DepositAddress};

/// Deposit status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DepositStatus {
    /// Awaiting on-chain confirmation.
    Pending,
    /// Confirmed on-chain, being processed.
    Confirming { confirmations: u32, required: u32 },
    /// Credited to user's exchange balance.
    Credited,
    /// Failed (e.g., dust amount, unsupported token).
    Failed,
}

/// Deposit record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepositRecord {
    pub id: Uuid,
    pub user_id: Uuid,
    pub asset: String,
    pub network: String,
    pub amount: Decimal,
    pub address: String,
    pub tx_hash: Option<String>,
    pub status: DepositStatus,
    pub created_at: DateTime<Utc>,
    pub credited_at: Option<DateTime<Utc>>,
}

/// Manages deposit addresses and incoming deposit tracking.
///
/// Phase 1: in-memory with MockCustodyProvider.
/// Phase 2: Fireblocks webhooks for real-time deposit detection.
pub struct DepositManager {
    /// user_id → asset → address
    addresses: HashMap<Uuid, HashMap<String, DepositAddress>>,
    /// All deposit records
    deposits: Vec<DepositRecord>,
    /// Reverse lookup: address → user_id
    address_to_user: HashMap<String, Uuid>,
}

impl DepositManager {
    pub fn new() -> Self {
        Self {
            addresses: HashMap::new(),
            deposits: Vec::new(),
            address_to_user: HashMap::new(),
        }
    }

    /// Check if a deposit address is already cached for this user + asset + network.
    pub fn get_cached_address(&self, user_id: Uuid, asset: &str, network: &str) -> Option<DepositAddress> {
        let key = format!("{}:{}", asset, network);
        self.addresses.get(&user_id)?.get(&key).cloned()
    }

    /// Register a deposit address in the cache and reverse-lookup map.
    pub fn register_address(&mut self, user_id: Uuid, asset: &str, network: &str, addr: DepositAddress) {
        let key = format!("{}:{}", asset, network);
        self.address_to_user.insert(addr.address.clone(), user_id);
        self.addresses.entry(user_id).or_default().insert(key, addr);
    }

    /// Get or create a deposit address for a user + asset.
    pub async fn get_or_create_address(
        &mut self,
        provider: &dyn CustodyProvider,
        user_id: Uuid,
        asset: &str,
        network: &str,
    ) -> Result<DepositAddress, crate::provider::CustodyError> {
        let user_addrs = self.addresses.entry(user_id).or_default();

        let key = format!("{}:{}", asset, network);
        if let Some(addr) = user_addrs.get(&key) {
            return Ok(addr.clone());
        }

        let addr = provider.create_deposit_address(user_id, asset, network).await?;
        self.address_to_user.insert(addr.address.clone(), user_id);
        user_addrs.insert(key, addr.clone());
        Ok(addr)
    }

    /// Record an incoming deposit (called by blockchain monitor / webhook).
    ///
    /// Idempotent: if a deposit with the same `tx_hash` already exists, returns
    /// the existing record without creating a duplicate. This prevents double-credit
    /// when custody retries webhook delivery (at-least-once semantics).
    pub fn record_deposit(
        &mut self,
        address: &str,
        asset: &str,
        network: &str,
        amount: Decimal,
        tx_hash: String,
    ) -> Option<DepositRecord> {
        let user_id = self.address_to_user.get(address)?.to_owned();

        // Dedup: return existing record if tx_hash already recorded.
        if let Some(existing) = self.deposits.iter().find(|d| d.tx_hash.as_deref() == Some(&tx_hash)) {
            tracing::debug!(tx_hash = %tx_hash, "deposit already recorded — returning existing (idempotent)");
            return Some(existing.clone());
        }

        let record = DepositRecord {
            id: Uuid::new_v4(),
            user_id,
            asset: asset.to_string(),
            network: network.to_string(),
            amount,
            address: address.to_string(),
            tx_hash: Some(tx_hash),
            status: DepositStatus::Pending,
            created_at: Utc::now(),
            credited_at: None,
        };

        self.deposits.push(record.clone());
        Some(record)
    }

    /// Credit a deposit (after sufficient confirmations).
    /// Returns the deposit record if found and credited.
    pub fn credit_deposit(&mut self, deposit_id: &Uuid) -> Option<&DepositRecord> {
        let deposit = self.deposits.iter_mut().find(|d| d.id == *deposit_id)?;

        if deposit.status == DepositStatus::Pending
            || matches!(deposit.status, DepositStatus::Confirming { .. })
        {
            deposit.status = DepositStatus::Credited;
            deposit.credited_at = Some(Utc::now());
            tracing::info!(
                "Deposit credited: {} {} for user {}",
                deposit.amount,
                deposit.asset,
                deposit.user_id
            );
        }

        Some(deposit)
    }

    /// Get deposits for a user.
    pub fn user_deposits(&self, user_id: &Uuid) -> Vec<&DepositRecord> {
        self.deposits.iter().filter(|d| d.user_id == *user_id).collect()
    }

    /// Get pending deposits (need crediting).
    /// All deposits (all statuses).
    pub fn all_deposits(&self) -> &[DepositRecord] {
        &self.deposits
    }

    pub fn pending_deposits(&self) -> Vec<&DepositRecord> {
        self.deposits
            .iter()
            .filter(|d| d.status == DepositStatus::Pending)
            .collect()
    }

    /// Lookup user by deposit address.
    pub fn user_for_address(&self, address: &str) -> Option<Uuid> {
        self.address_to_user.get(address).copied()
    }
}

impl Default for DepositManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Minimum blockchain confirmations required before crediting a deposit.
pub fn required_confirmations(asset: &str) -> u32 {
    match asset {
        "BTC" => 3,
        "ETH" => 12,
        "USDT" | "USDC" => 1,
        "SOL" => 1,
        _ => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::MockCustodyProvider;
    use rust_decimal_macros::dec;

    #[tokio::test]
    async fn test_get_or_create_address() {
        let mut mgr = DepositManager::new();
        let provider = MockCustodyProvider::new();
        let user = Uuid::new_v4();

        let addr1 = mgr.get_or_create_address(&provider, user, "BTC", "bitcoin").await.unwrap();
        let addr2 = mgr.get_or_create_address(&provider, user, "BTC", "bitcoin").await.unwrap();

        // Same user+asset returns cached address
        assert_eq!(addr1.address, addr2.address);
    }

    #[tokio::test]
    async fn test_different_assets_different_addresses() {
        let mut mgr = DepositManager::new();
        let provider = MockCustodyProvider::new();
        let user = Uuid::new_v4();

        let btc = mgr.get_or_create_address(&provider, user, "BTC", "bitcoin").await.unwrap();
        let eth = mgr.get_or_create_address(&provider, user, "ETH", "ethereum").await.unwrap();

        assert_ne!(btc.address, eth.address);
    }

    #[tokio::test]
    async fn test_record_and_credit_deposit() {
        let mut mgr = DepositManager::new();
        let provider = MockCustodyProvider::new();
        let user = Uuid::new_v4();

        let addr = mgr.get_or_create_address(&provider, user, "BTC", "bitcoin").await.unwrap();

        // Simulate incoming deposit
        let record = mgr
            .record_deposit(&addr.address, "BTC", "bitcoin", dec!(0.5), "tx123".to_string())
            .unwrap();

        assert_eq!(record.amount, dec!(0.5));
        assert_eq!(record.status, DepositStatus::Pending);
        assert_eq!(mgr.pending_deposits().len(), 1);

        // Credit it
        let credited = mgr.credit_deposit(&record.id).unwrap();
        assert_eq!(credited.status, DepositStatus::Credited);
        assert!(credited.credited_at.is_some());
        assert_eq!(mgr.pending_deposits().len(), 0);
    }

    #[tokio::test]
    async fn test_deposit_dedup_by_tx_hash() {
        let mut mgr = DepositManager::new();
        let provider = MockCustodyProvider::new();
        let user = Uuid::new_v4();

        let addr = mgr.get_or_create_address(&provider, user, "BTC", "bitcoin").await.unwrap();

        // First record
        let r1 = mgr.record_deposit(&addr.address, "BTC", "bitcoin", dec!(0.5), "tx_dup".to_string()).unwrap();
        assert_eq!(mgr.pending_deposits().len(), 1);

        // Retry with same tx_hash — should return existing, not create duplicate
        let r2 = mgr.record_deposit(&addr.address, "BTC", "bitcoin", dec!(0.5), "tx_dup".to_string()).unwrap();
        assert_eq!(r1.id, r2.id); // Same record returned
        assert_eq!(mgr.pending_deposits().len(), 1); // Still only 1 deposit

        // Different tx_hash — should create new record
        let r3 = mgr.record_deposit(&addr.address, "BTC", "bitcoin", dec!(0.3), "tx_other".to_string()).unwrap();
        assert_ne!(r1.id, r3.id);
        assert_eq!(mgr.pending_deposits().len(), 2);
    }

    #[test]
    fn test_unknown_address_returns_none() {
        let mut mgr = DepositManager::new();
        let result = mgr.record_deposit("unknown_addr", "BTC", "bitcoin", dec!(1), "tx".to_string());
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_user_deposits() {
        let mut mgr = DepositManager::new();
        let provider = MockCustodyProvider::new();
        let user = Uuid::new_v4();

        let addr = mgr.get_or_create_address(&provider, user, "ETH", "ethereum").await.unwrap();
        mgr.record_deposit(&addr.address, "ETH", "ethereum", dec!(10), "tx1".to_string());
        mgr.record_deposit(&addr.address, "ETH", "ethereum", dec!(20), "tx2".to_string());

        assert_eq!(mgr.user_deposits(&user).len(), 2);
    }
}
