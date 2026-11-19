use async_trait::async_trait;
use rust_decimal::Decimal;
use uuid::Uuid;

/// Trait for custody providers (Fireblocks, BitGo, or self-hosted).
///
/// Design: the exchange NEVER holds private keys directly.
/// All crypto operations go through the custody provider.
///
/// Async because real providers make HTTP calls to external custody services.
#[async_trait]
pub trait CustodyProvider: Send + Sync {
    /// Generate a deposit address for a user + asset.
    /// Returns (address, memo/tag if needed).
    async fn create_deposit_address(
        &self,
        user_id: Uuid,
        asset: &str,
        network: &str,
    ) -> Result<DepositAddress, CustodyError>;

    /// Submit a withdrawal request to the custody provider.
    /// Returns a provider-specific transaction ID.
    async fn submit_withdrawal(
        &self,
        withdrawal_id: Uuid,
        asset: &str,
        network: &str,
        address: &str,
        amount: Decimal,
    ) -> Result<String, CustodyError>;

    /// Check withdrawal status by provider transaction ID.
    async fn check_withdrawal_status(&self, provider_tx_id: &str) -> Result<ProviderTxStatus, CustodyError>;

    /// Get the hot wallet balance for an asset.
    async fn hot_wallet_balance(&self, asset: &str) -> Result<Decimal, CustodyError>;
}

/// Deposit address info.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DepositAddress {
    pub address: String,
    /// Memo/tag for networks that require it (e.g., XRP, COSMOS).
    pub memo: Option<String>,
    pub network: String,
}

/// Provider transaction status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderTxStatus {
    Pending,
    Confirming { confirmations: u32, required: u32 },
    Completed { tx_hash: String },
    Failed { reason: String },
}

#[derive(Debug, thiserror::Error)]
pub enum CustodyError {
    #[error("Unsupported asset: {0}")]
    UnsupportedAsset(String),

    #[error("Unsupported network: {0}")]
    UnsupportedNetwork(String),

    #[error("Insufficient hot wallet balance: need {needed}, have {available}")]
    InsufficientBalance { needed: Decimal, available: Decimal },

    #[error("Provider error: {0}")]
    ProviderError(String),

    #[error("Transaction not found: {0}")]
    TxNotFound(String),
}

/// Mock custody provider for development and testing.
///
/// Generates deterministic addresses, auto-approves withdrawals.
pub struct MockCustodyProvider {
    /// Simulated hot wallet balances.
    balances: std::collections::HashMap<String, Decimal>,
}

impl MockCustodyProvider {
    pub fn new() -> Self {
        let mut balances = std::collections::HashMap::new();
        balances.insert("BTC".to_string(), Decimal::new(100, 0));
        balances.insert("ETH".to_string(), Decimal::new(1000, 0));
        balances.insert("USDT".to_string(), Decimal::new(1_000_000, 0));
        balances.insert("SOL".to_string(), Decimal::new(10_000, 0));

        Self { balances }
    }
}

impl Default for MockCustodyProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CustodyProvider for MockCustodyProvider {
    async fn create_deposit_address(
        &self,
        user_id: Uuid,
        asset: &str,
        network: &str,
    ) -> Result<DepositAddress, CustodyError> {
        // Deterministic address from user_id + asset (two hashes for enough chars)
        let h1 = md5_simple(format!("{}:{}", user_id, asset).as_bytes());
        let h2 = md5_simple(format!("{}:{}:2", user_id, asset).as_bytes());
        let hash = format!("{:032x}{:032x}", h1, h2); // 64 hex chars
        let address = match network {
            "bitcoin" => format!("bc1q{}", &hash[..20]),
            "ethereum" | "erc20" => format!("0x{}", &hash[..40]),
            "solana" | "spl" => format!("{}", &hash[..32]),
            "tron" | "trc20" => format!("T{}", &hash[..33]),
            _ => return Err(CustodyError::UnsupportedNetwork(network.to_string())),
        };

        Ok(DepositAddress {
            address,
            memo: None,
            network: network.to_string(),
        })
    }

    async fn submit_withdrawal(
        &self,
        withdrawal_id: Uuid,
        asset: &str,
        _network: &str,
        _address: &str,
        amount: Decimal,
    ) -> Result<String, CustodyError> {
        let balance = self.balances.get(asset).copied().unwrap_or(Decimal::ZERO);
        if amount > balance {
            return Err(CustodyError::InsufficientBalance {
                needed: amount,
                available: balance,
            });
        }

        // Return mock tx ID
        Ok(format!("mock-tx-{}", withdrawal_id))
    }

    async fn check_withdrawal_status(&self, provider_tx_id: &str) -> Result<ProviderTxStatus, CustodyError> {
        if provider_tx_id.starts_with("mock-tx-") {
            Ok(ProviderTxStatus::Completed {
                tx_hash: format!("0x{}", &provider_tx_id[8..]),
            })
        } else {
            Err(CustodyError::TxNotFound(provider_tx_id.to_string()))
        }
    }

    async fn hot_wallet_balance(&self, asset: &str) -> Result<Decimal, CustodyError> {
        Ok(self.balances.get(asset).copied().unwrap_or(Decimal::ZERO))
    }
}

/// Simple hash for deterministic mock addresses (NOT cryptographic).
fn md5_simple(data: &[u8]) -> u128 {
    let mut hash: u128 = 0xcbf2_9ce4_8422_2325;
    for &byte in data {
        hash ^= byte as u128;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
        hash = hash.rotate_left(13);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_create_btc_address() {
        let provider = MockCustodyProvider::new();
        let user = Uuid::new_v4();

        let addr = provider.create_deposit_address(user, "BTC", "bitcoin").await.unwrap();
        assert!(addr.address.starts_with("bc1q"));
        assert!(addr.memo.is_none());
    }

    #[tokio::test]
    async fn test_mock_create_eth_address() {
        let provider = MockCustodyProvider::new();
        let user = Uuid::new_v4();

        let addr = provider.create_deposit_address(user, "ETH", "ethereum").await.unwrap();
        assert!(addr.address.starts_with("0x"));
    }

    #[tokio::test]
    async fn test_mock_deterministic_address() {
        let provider = MockCustodyProvider::new();
        let user = Uuid::from_u128(0x1234);

        let addr1 = provider.create_deposit_address(user, "BTC", "bitcoin").await.unwrap();
        let addr2 = provider.create_deposit_address(user, "BTC", "bitcoin").await.unwrap();
        assert_eq!(addr1.address, addr2.address); // Same user+asset = same address
    }

    #[tokio::test]
    async fn test_mock_different_users_different_addresses() {
        let provider = MockCustodyProvider::new();
        let user1 = Uuid::from_u128(0x1);
        let user2 = Uuid::from_u128(0x2);

        let addr1 = provider.create_deposit_address(user1, "BTC", "bitcoin").await.unwrap();
        let addr2 = provider.create_deposit_address(user2, "BTC", "bitcoin").await.unwrap();
        assert_ne!(addr1.address, addr2.address);
    }

    #[tokio::test]
    async fn test_mock_unsupported_network() {
        let provider = MockCustodyProvider::new();
        let result = provider.create_deposit_address(Uuid::new_v4(), "BTC", "unknown").await;
        assert!(matches!(result, Err(CustodyError::UnsupportedNetwork(_))));
    }

    #[tokio::test]
    async fn test_mock_withdrawal() {
        let provider = MockCustodyProvider::new();
        let wid = Uuid::new_v4();

        let tx_id = provider
            .submit_withdrawal(wid, "BTC", "bitcoin", "bc1q123", Decimal::new(1, 0))
            .await
            .unwrap();

        assert!(tx_id.starts_with("mock-tx-"));

        let status = provider.check_withdrawal_status(&tx_id).await.unwrap();
        assert!(matches!(status, ProviderTxStatus::Completed { .. }));
    }

    #[tokio::test]
    async fn test_mock_withdrawal_insufficient() {
        let provider = MockCustodyProvider::new();
        let result = provider.submit_withdrawal(
            Uuid::new_v4(),
            "BTC",
            "bitcoin",
            "bc1q123",
            Decimal::new(999, 0), // 999 BTC > 100 BTC hot wallet
        ).await;
        assert!(matches!(result, Err(CustodyError::InsufficientBalance { .. })));
    }

    #[tokio::test]
    async fn test_hot_wallet_balance() {
        let provider = MockCustodyProvider::new();
        assert_eq!(provider.hot_wallet_balance("BTC").await.unwrap(), Decimal::new(100, 0));
        assert_eq!(provider.hot_wallet_balance("UNKNOWN").await.unwrap(), Decimal::ZERO);
    }
}
