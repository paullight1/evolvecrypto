//! Fireblocks API client for crypto custody.
//!
//! Fireblocks architecture:
//! - **Vault Accounts**: logical wallets (one per user or per purpose)
//! - **Assets**: crypto assets within a vault (BTC, ETH, etc.)
//! - **Transactions**: transfers between vaults, to external addresses, etc.
//! - **Webhooks**: real-time notifications for deposits, tx status changes
//!
//! Auth: API Key + RSA private key → JWT signed requests
//!
//! Enable with `cargo build --features fireblocks`

use std::collections::HashMap;
use std::sync::Mutex;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::provider::{CustodyError, CustodyProvider, DepositAddress, ProviderTxStatus};

/// Fireblocks asset ID mapping.
/// Maps our internal asset names to Fireblocks asset IDs.
fn asset_to_fireblocks_id(asset: &str, network: &str) -> Result<&'static str, CustodyError> {
    match (asset, network) {
        ("BTC", "bitcoin") => Ok("BTC"),
        ("ETH", "ethereum") => Ok("ETH"),
        ("USDT", "ethereum" | "erc20") => Ok("USDT_ERC20"),
        ("USDT", "tron" | "trc20") => Ok("USDT_TRC20"),
        ("USDT", "solana" | "spl") => Ok("USDT_SOL"),
        ("SOL", "solana" | "spl") => Ok("SOL"),
        ("USDC", "ethereum" | "erc20") => Ok("USDC"),
        ("USDC", "solana" | "spl") => Ok("USDC_SOL"),
        _ => Err(CustodyError::UnsupportedAsset(format!("{}:{}", asset, network))),
    }
}

/// Fireblocks API configuration.
#[derive(Debug, Clone)]
pub struct FireblocksConfig {
    /// Fireblocks API base URL.
    pub base_url: String,
    /// API key from Fireblocks console.
    pub api_key: String,
    /// RSA private key (PEM) for JWT signing.
    pub private_key_pem: String,
    /// Vault account ID for the exchange's omnibus wallet.
    pub omnibus_vault_id: String,
}

impl FireblocksConfig {
    pub fn from_env() -> Result<Self, CustodyError> {
        Ok(Self {
            base_url: std::env::var("FIREBLOCKS_API_URL")
                .unwrap_or_else(|_| "https://api.fireblocks.io".to_string()),
            api_key: std::env::var("FIREBLOCKS_API_KEY")
                .map_err(|_| CustodyError::ProviderError("FIREBLOCKS_API_KEY not set".to_string()))?,
            private_key_pem: std::env::var("FIREBLOCKS_PRIVATE_KEY")
                .map_err(|_| CustodyError::ProviderError("FIREBLOCKS_PRIVATE_KEY not set".to_string()))?,
            omnibus_vault_id: std::env::var("FIREBLOCKS_OMNIBUS_VAULT_ID")
                .unwrap_or_else(|_| "0".to_string()),
        })
    }
}

// --- Fireblocks API request/response types ---

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateVaultRequest {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    customer_ref_id: Option<String>,
    auto_fuel: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultAccount {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateTransactionRequest {
    asset_id: String,
    amount: String,
    source: TransferPeer,
    destination: TransferPeer,
    note: String,
    external_tx_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransferPeer {
    #[serde(rename = "type")]
    peer_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    one_time_address: Option<OneTimeAddress>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OneTimeAddress {
    address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tag: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionResponse {
    pub id: String,
    pub status: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionDetails {
    pub id: String,
    pub status: String,
    pub tx_hash: Option<String>,
    pub num_of_confirmations: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DepositAddressResponse {
    pub address: String,
    pub tag: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultAssetBalance {
    pub total: Option<String>,
    pub available: Option<String>,
    pub frozen: Option<String>,
}

// --- Webhook types ---

/// Fireblocks webhook event for deposit detection.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FireblocksWebhookEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub tenant_id: String,
    pub timestamp: i64,
    pub data: serde_json::Value,
}

/// Parsed transaction data from webhook.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookTransactionData {
    pub id: String,
    pub status: String,
    pub asset_id: String,
    pub amount: String,
    pub source_type: Option<String>,
    pub destination_type: Option<String>,
    pub destination_id: Option<String>,
    pub tx_hash: Option<String>,
    pub num_of_confirmations: Option<u32>,
}

/// Fireblocks custody provider.
///
/// Uses Fireblocks API for:
/// - Creating vault accounts per user
/// - Generating deposit addresses
/// - Submitting withdrawal transactions
/// - Checking transaction status
///
/// Thread-safe: all HTTP calls go through reqwest, vault mapping is behind Mutex.
pub struct FireblocksCustodyProvider {
    config: FireblocksConfig,
    /// Cache: user_id → Fireblocks vault account ID
    vault_map: Mutex<HashMap<Uuid, String>>,
    /// Tokio runtime handle for blocking the sync trait methods
    rt: tokio::runtime::Handle,
}

impl FireblocksCustodyProvider {
    pub fn new(config: FireblocksConfig, rt: tokio::runtime::Handle) -> Self {
        Self {
            config,
            vault_map: Mutex::new(HashMap::new()),
            rt,
        }
    }

    /// Build the JWT token for Fireblocks API authentication.
    #[cfg(feature = "fireblocks")]
    fn sign_jwt(&self, path: &str, body: &str) -> Result<String, CustodyError> {
        use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
        use sha2::{Digest, Sha256};

        let now = chrono::Utc::now().timestamp();
        let nonce = Uuid::new_v4().to_string();

        let body_hash = hex::encode(Sha256::digest(body.as_bytes()));

        let claims = serde_json::json!({
            "uri": path,
            "nonce": nonce,
            "iat": now,
            "exp": now + 30,
            "sub": self.config.api_key,
            "bodyHash": body_hash,
        });

        let key = EncodingKey::from_rsa_pem(self.config.private_key_pem.as_bytes())
            .map_err(|e| CustodyError::ProviderError(format!("Invalid RSA key: {}", e)))?;

        let token = encode(&Header::new(Algorithm::RS256), &claims, &key)
            .map_err(|e| CustodyError::ProviderError(format!("JWT signing failed: {}", e)))?;

        Ok(token)
    }

    /// Make an authenticated GET request to Fireblocks API.
    #[cfg(feature = "fireblocks")]
    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, CustodyError> {
        let url = format!("{}/v1{}", self.config.base_url, path);
        let token = self.sign_jwt(path, "")?;

        let resp = reqwest::Client::new()
            .get(&url)
            .header("X-API-Key", &self.config.api_key)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .map_err(|e| CustodyError::ProviderError(format!("HTTP error: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(CustodyError::ProviderError(format!(
                "Fireblocks API error {}: {}", status, body
            )));
        }

        resp.json::<T>()
            .await
            .map_err(|e| CustodyError::ProviderError(format!("JSON parse error: {}", e)))
    }

    /// Make an authenticated POST request to Fireblocks API.
    #[cfg(feature = "fireblocks")]
    async fn post<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &impl Serialize,
    ) -> Result<T, CustodyError> {
        let url = format!("{}/v1{}", self.config.base_url, path);
        let body_json = serde_json::to_string(body)
            .map_err(|e| CustodyError::ProviderError(format!("JSON serialize error: {}", e)))?;
        let token = self.sign_jwt(path, &body_json)?;

        let resp = reqwest::Client::new()
            .post(&url)
            .header("X-API-Key", &self.config.api_key)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .body(body_json)
            .send()
            .await
            .map_err(|e| CustodyError::ProviderError(format!("HTTP error: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(CustodyError::ProviderError(format!(
                "Fireblocks API error {}: {}", status, body
            )));
        }

        resp.json::<T>()
            .await
            .map_err(|e| CustodyError::ProviderError(format!("JSON parse error: {}", e)))
    }

    /// Get or create a Fireblocks vault account for a user.
    #[cfg(feature = "fireblocks")]
    async fn get_or_create_vault(&self, user_id: Uuid) -> Result<String, CustodyError> {
        // Check cache
        if let Some(vault_id) = self.vault_map.lock().unwrap().get(&user_id) {
            return Ok(vault_id.clone());
        }

        // Create vault account
        let vault: VaultAccount = self.post(
            "/vault/accounts",
            &CreateVaultRequest {
                name: format!("user-{}", user_id),
                customer_ref_id: Some(user_id.to_string()),
                auto_fuel: true,
            },
        ).await?;

        tracing::info!("Created Fireblocks vault {} for user {}", vault.id, user_id);

        self.vault_map.lock().unwrap().insert(user_id, vault.id.clone());
        Ok(vault.id)
    }

    /// Create deposit address (async version).
    #[cfg(feature = "fireblocks")]
    async fn create_deposit_address_async(
        &self,
        user_id: Uuid,
        asset: &str,
        network: &str,
    ) -> Result<DepositAddress, CustodyError> {
        let vault_id = self.get_or_create_vault(user_id).await?;
        let fb_asset = asset_to_fireblocks_id(asset, network)?;

        // Activate asset in vault (idempotent)
        let _: serde_json::Value = self.post(
            &format!("/vault/accounts/{}/{}",vault_id, fb_asset),
            &serde_json::json!({}),
        ).await.unwrap_or(serde_json::json!({}));

        // Get deposit address
        let addrs: Vec<DepositAddressResponse> = self.get(
            &format!("/vault/accounts/{}/{}/addresses", vault_id, fb_asset),
        ).await?;

        if let Some(addr) = addrs.first() {
            Ok(DepositAddress {
                address: addr.address.clone(),
                memo: addr.tag.clone(),
                network: network.to_string(),
            })
        } else {
            // Create new address
            let addr: DepositAddressResponse = self.post(
                &format!("/vault/accounts/{}/{}/addresses", vault_id, fb_asset),
                &serde_json::json!({}),
            ).await?;

            Ok(DepositAddress {
                address: addr.address,
                memo: addr.tag,
                network: network.to_string(),
            })
        }
    }

    /// Submit withdrawal (async version).
    #[cfg(feature = "fireblocks")]
    async fn submit_withdrawal_async(
        &self,
        withdrawal_id: Uuid,
        asset: &str,
        network: &str,
        address: &str,
        amount: Decimal,
    ) -> Result<String, CustodyError> {
        let fb_asset = asset_to_fireblocks_id(asset, network)?;

        let tx: TransactionResponse = self.post(
            "/transactions",
            &CreateTransactionRequest {
                asset_id: fb_asset.to_string(),
                amount: amount.to_string(),
                source: TransferPeer {
                    peer_type: "VAULT_ACCOUNT".to_string(),
                    id: Some(self.config.omnibus_vault_id.clone()),
                    one_time_address: None,
                },
                destination: TransferPeer {
                    peer_type: "ONE_TIME_ADDRESS".to_string(),
                    id: None,
                    one_time_address: Some(OneTimeAddress {
                        address: address.to_string(),
                        tag: None,
                    }),
                },
                note: format!("Withdrawal {}", withdrawal_id),
                external_tx_id: withdrawal_id.to_string(),
            },
        ).await?;

        tracing::info!("Fireblocks tx created: {} for withdrawal {}", tx.id, withdrawal_id);
        Ok(tx.id)
    }

    /// Check tx status (async version).
    #[cfg(feature = "fireblocks")]
    async fn check_status_async(&self, provider_tx_id: &str) -> Result<ProviderTxStatus, CustodyError> {
        let tx: TransactionDetails = self.get(
            &format!("/transactions/{}", provider_tx_id),
        ).await?;

        match tx.status.as_str() {
            "COMPLETED" => Ok(ProviderTxStatus::Completed {
                tx_hash: tx.tx_hash.unwrap_or_default(),
            }),
            "FAILED" | "REJECTED" | "CANCELLED" | "BLOCKED" => Ok(ProviderTxStatus::Failed {
                reason: format!("Fireblocks status: {}", tx.status),
            }),
            "CONFIRMING" => Ok(ProviderTxStatus::Confirming {
                confirmations: tx.num_of_confirmations.unwrap_or(0),
                required: 6, // varies by asset, simplified
            }),
            _ => Ok(ProviderTxStatus::Pending),
        }
    }

    /// Get vault balance (async version).
    #[cfg(feature = "fireblocks")]
    async fn balance_async(&self, asset: &str) -> Result<Decimal, CustodyError> {
        let fb_asset = asset_to_fireblocks_id(asset, "ethereum")
            .or_else(|_| asset_to_fireblocks_id(asset, "bitcoin"))
            .or_else(|_| asset_to_fireblocks_id(asset, "solana"))
            .unwrap_or("BTC");

        let bal: VaultAssetBalance = self.get(
            &format!("/vault/accounts/{}/{}", self.config.omnibus_vault_id, fb_asset),
        ).await?;

        let available = bal.available.unwrap_or_default();
        available.parse::<Decimal>().map_err(|e| {
            CustodyError::ProviderError(format!("Invalid balance '{}': {}", available, e))
        })
    }
}

/// Implement the async CustodyProvider trait by delegating to internal async methods.
#[cfg(feature = "fireblocks")]
#[async_trait::async_trait]
impl CustodyProvider for FireblocksCustodyProvider {
    async fn create_deposit_address(
        &self,
        user_id: Uuid,
        asset: &str,
        network: &str,
    ) -> Result<DepositAddress, CustodyError> {
        self.create_deposit_address_async(user_id, asset, network).await
    }

    async fn submit_withdrawal(
        &self,
        withdrawal_id: Uuid,
        asset: &str,
        network: &str,
        address: &str,
        amount: Decimal,
    ) -> Result<String, CustodyError> {
        self.submit_withdrawal_async(withdrawal_id, asset, network, address, amount).await
    }

    async fn check_withdrawal_status(&self, provider_tx_id: &str) -> Result<ProviderTxStatus, CustodyError> {
        self.check_status_async(provider_tx_id).await
    }

    async fn hot_wallet_balance(&self, asset: &str) -> Result<Decimal, CustodyError> {
        self.balance_async(asset).await
    }
}

/// Without the `fireblocks` feature, the provider compiles but panics at runtime.
/// This allows the rest of the codebase to reference the type without conditional compilation.
#[cfg(not(feature = "fireblocks"))]
#[async_trait::async_trait]
impl CustodyProvider for FireblocksCustodyProvider {
    async fn create_deposit_address(&self, _: Uuid, _: &str, _: &str) -> Result<DepositAddress, CustodyError> {
        Err(CustodyError::ProviderError("Fireblocks feature not enabled. Build with --features fireblocks".to_string()))
    }
    async fn submit_withdrawal(&self, _: Uuid, _: &str, _: &str, _: &str, _: Decimal) -> Result<String, CustodyError> {
        Err(CustodyError::ProviderError("Fireblocks feature not enabled".to_string()))
    }
    async fn check_withdrawal_status(&self, _: &str) -> Result<ProviderTxStatus, CustodyError> {
        Err(CustodyError::ProviderError("Fireblocks feature not enabled".to_string()))
    }
    async fn hot_wallet_balance(&self, _: &str) -> Result<Decimal, CustodyError> {
        Err(CustodyError::ProviderError("Fireblocks feature not enabled".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_asset_mapping() {
        assert_eq!(asset_to_fireblocks_id("BTC", "bitcoin").unwrap(), "BTC");
        assert_eq!(asset_to_fireblocks_id("ETH", "ethereum").unwrap(), "ETH");
        assert_eq!(asset_to_fireblocks_id("USDT", "tron").unwrap(), "USDT_TRC20");
        assert_eq!(asset_to_fireblocks_id("USDT", "ethereum").unwrap(), "USDT_ERC20");
        assert_eq!(asset_to_fireblocks_id("USDT", "solana").unwrap(), "USDT_SOL");
        assert_eq!(asset_to_fireblocks_id("SOL", "solana").unwrap(), "SOL");
        assert_eq!(asset_to_fireblocks_id("USDC", "ethereum").unwrap(), "USDC");
        assert!(asset_to_fireblocks_id("DOGE", "bitcoin").is_err());
    }

    #[test]
    fn test_config_fields() {
        let config = FireblocksConfig {
            base_url: "https://api.fireblocks.io".to_string(),
            api_key: "test-key".to_string(),
            private_key_pem: "test-pem".to_string(),
            omnibus_vault_id: "0".to_string(),
        };
        assert_eq!(config.base_url, "https://api.fireblocks.io");
    }

    #[test]
    fn test_webhook_event_parsing() {
        let json = r#"{
            "type": "TRANSACTION_STATUS_UPDATED",
            "tenantId": "tenant-123",
            "timestamp": 1700000000,
            "data": {
                "id": "tx-123",
                "status": "COMPLETED",
                "assetId": "BTC",
                "amount": "1.5",
                "txHash": "0xabc123"
            }
        }"#;

        let event: FireblocksWebhookEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type, "TRANSACTION_STATUS_UPDATED");
        assert_eq!(event.data["status"], "COMPLETED");
        assert_eq!(event.data["amount"], "1.5");
    }

    #[test]
    fn test_webhook_deposit_detection() {
        let json = r#"{
            "id": "tx-456",
            "status": "COMPLETED",
            "assetId": "ETH",
            "amount": "10.5",
            "sourceType": "UNKNOWN",
            "destinationType": "VAULT_ACCOUNT",
            "destinationId": "42",
            "txHash": "0xdef456",
            "numOfConfirmations": 12
        }"#;

        let data: WebhookTransactionData = serde_json::from_str(json).unwrap();
        assert_eq!(data.status, "COMPLETED");
        assert_eq!(data.destination_type, Some("VAULT_ACCOUNT".to_string()));
        assert_eq!(data.destination_id, Some("42".to_string()));
        assert_eq!(data.tx_hash, Some("0xdef456".to_string()));
    }

    #[tokio::test]
    #[cfg(not(feature = "fireblocks"))]
    async fn test_fireblocks_disabled_returns_error() {
        let config = FireblocksConfig {
            base_url: "https://api.fireblocks.io".to_string(),
            api_key: "test".to_string(),
            private_key_pem: "test".to_string(),
            omnibus_vault_id: "0".to_string(),
        };
        let rt = tokio::runtime::Handle::try_current()
            .unwrap_or_else(|_| {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.handle().clone()
            });
        let provider = FireblocksCustodyProvider::new(config, rt);

        let result = provider.hot_wallet_balance("BTC").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not enabled"));
    }
}
