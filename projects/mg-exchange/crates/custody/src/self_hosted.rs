//! Self-hosted custody provider.
//!
//! Implements `CustodyProvider` by calling the MG Custody REST API.
//! This is the bridge between the exchange and the self-hosted custody service.
//!
//! API contract:
//!   POST /v1/addresses          → create deposit address
//!   POST /v1/withdrawals        → submit withdrawal
//!   GET  /v1/withdrawals/:id    → check withdrawal status
//!   GET  /v1/wallets/:asset     → hot wallet balance
//!
//! Auth: HMAC-SHA256 of (method + path + timestamp + body)

use async_trait::async_trait;
use hmac::{Hmac, Mac};
use reqwest::Client;
use rust_decimal::Decimal;
use sha2::Sha256;
use uuid::Uuid;

use crate::{CustodyError, CustodyProvider, DepositAddress, ProviderTxStatus};

type HmacSha256 = Hmac<Sha256>;

/// Self-hosted custody provider — calls our own custody service REST API.
pub struct SelfHostedCustodyProvider {
    base_url: String,
    hmac_secret: Vec<u8>,
    client: Client,
}

impl SelfHostedCustodyProvider {
    /// Create a new self-hosted custody provider.
    ///
    /// `base_url`: e.g., "http://custody:8443"
    /// `hmac_secret`: shared secret for HMAC-SHA256 request signing
    pub fn new(base_url: &str, hmac_secret: &[u8]) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            hmac_secret: hmac_secret.to_vec(),
            client: Client::new(),
        }
    }

    /// Sign a request with HMAC-SHA256.
    fn sign_request(&self, method: &str, path: &str, body: &str) -> (String, String) {
        let timestamp = chrono::Utc::now().timestamp().to_string();
        let message = format!("{}{}{}{}", method, path, timestamp, body);

        let mut mac = HmacSha256::new_from_slice(&self.hmac_secret)
            .expect("HMAC key length");
        mac.update(message.as_bytes());
        let signature = hex::encode(mac.finalize().into_bytes());

        (timestamp, signature)
    }

    /// Make an authenticated GET request.
    async fn get(&self, path: &str) -> Result<serde_json::Value, CustodyError> {
        let url = format!("{}{}", self.base_url, path);
        let (timestamp, signature) = self.sign_request("GET", path, "");

        let resp = self.client
            .get(&url)
            .header("X-Custody-Timestamp", &timestamp)
            .header("X-Custody-Signature", &signature)
            .send()
            .await
            .map_err(|e| CustodyError::ProviderError(format!("HTTP error: {e}")))?;

        if !resp.status().is_success() {
            return Err(CustodyError::ProviderError(
                format!("HTTP {}: {}", resp.status(), resp.text().await.unwrap_or_default())
            ));
        }

        resp.json().await.map_err(|e| CustodyError::ProviderError(format!("JSON parse: {e}")))
    }

    /// Make an authenticated POST request.
    async fn post(&self, path: &str, body: &serde_json::Value) -> Result<serde_json::Value, CustodyError> {
        let url = format!("{}{}", self.base_url, path);
        let body_str = serde_json::to_string(body)
            .map_err(|e| CustodyError::ProviderError(format!("JSON serialize: {e}")))?;
        let (timestamp, signature) = self.sign_request("POST", path, &body_str);

        let resp = self.client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("X-Custody-Timestamp", &timestamp)
            .header("X-Custody-Signature", &signature)
            .body(body_str)
            .send()
            .await
            .map_err(|e| CustodyError::ProviderError(format!("HTTP error: {e}")))?;

        if !resp.status().is_success() {
            return Err(CustodyError::ProviderError(
                format!("HTTP {}: {}", resp.status(), resp.text().await.unwrap_or_default())
            ));
        }

        resp.json().await.map_err(|e| CustodyError::ProviderError(format!("JSON parse: {e}")))
    }

    /// Map network string to custody chain name.
    fn map_network(network: &str) -> &str {
        match network {
            "bitcoin" | "btc" => "bitcoin",
            "ethereum" | "erc20" | "eth" => "ethereum",
            "tron" | "trc20" => "tron",
            "solana" | "spl" | "sol" => "solana",
            other => other,
        }
    }
}

#[async_trait]
impl CustodyProvider for SelfHostedCustodyProvider {
    async fn create_deposit_address(
        &self,
        user_id: Uuid,
        asset: &str,
        network: &str,
    ) -> Result<DepositAddress, CustodyError> {
        let chain = Self::map_network(network);
        let body = serde_json::json!({
            "user_id": user_id,
            "asset": asset,
            "chain": chain,
        });

        let resp = self.post("/v1/addresses", &body).await?;

        Ok(DepositAddress {
            address: resp["address"].as_str().unwrap_or("").to_string(),
            memo: resp["memo"].as_str().map(String::from),
            network: network.to_string(),
        })
    }

    async fn submit_withdrawal(
        &self,
        withdrawal_id: Uuid,
        asset: &str,
        network: &str,
        address: &str,
        amount: Decimal,
    ) -> Result<String, CustodyError> {
        let chain = Self::map_network(network);
        let body = serde_json::json!({
            "withdrawal_id": withdrawal_id,
            "asset": asset,
            "chain": chain,
            "address": address,
            "amount": amount.to_string(),
        });

        let resp = self.post("/v1/withdrawals", &body).await?;

        let custody_ref = resp["custody_ref"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(custody_ref)
    }

    async fn check_withdrawal_status(&self, provider_tx_id: &str) -> Result<ProviderTxStatus, CustodyError> {
        let path = format!("/v1/withdrawals/{}", provider_tx_id);
        let resp = self.get(&path).await?;

        let status = resp["status"].as_str().unwrap_or("pending");
        let tx_hash = resp["tx_hash"].as_str().map(String::from);
        let confirmations = resp["confirmations"].as_u64().unwrap_or(0) as u32;
        let required = resp["required_confirmations"].as_u64().unwrap_or(12) as u32;

        match status {
            "finalized" => Ok(ProviderTxStatus::Completed {
                tx_hash: tx_hash.unwrap_or_default(),
            }),
            "failed" => Ok(ProviderTxStatus::Failed {
                reason: resp["error"].as_str().unwrap_or("Unknown error").to_string(),
            }),
            "broadcast" | "confirming" => Ok(ProviderTxStatus::Confirming {
                confirmations,
                required,
            }),
            _ => Ok(ProviderTxStatus::Pending),
        }
    }

    async fn hot_wallet_balance(&self, asset: &str) -> Result<Decimal, CustodyError> {
        let path = format!("/v1/wallets/{}", asset);
        let resp = self.get(&path).await?;

        let balance_str = resp["hot_balance"].as_str().unwrap_or("0");
        balance_str.parse::<Decimal>()
            .map_err(|e| CustodyError::ProviderError(format!("Balance parse: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_mapping() {
        assert_eq!(SelfHostedCustodyProvider::map_network("bitcoin"), "bitcoin");
        assert_eq!(SelfHostedCustodyProvider::map_network("btc"), "bitcoin");
        assert_eq!(SelfHostedCustodyProvider::map_network("ethereum"), "ethereum");
        assert_eq!(SelfHostedCustodyProvider::map_network("erc20"), "ethereum");
        assert_eq!(SelfHostedCustodyProvider::map_network("tron"), "tron");
        assert_eq!(SelfHostedCustodyProvider::map_network("trc20"), "tron");
        assert_eq!(SelfHostedCustodyProvider::map_network("solana"), "solana");
        assert_eq!(SelfHostedCustodyProvider::map_network("spl"), "solana");
    }

    #[test]
    fn hmac_signing() {
        let provider = SelfHostedCustodyProvider::new("http://localhost:8443", b"test-secret");
        let (ts, sig) = provider.sign_request("POST", "/v1/addresses", r#"{"test":true}"#);

        assert!(!ts.is_empty());
        assert_eq!(sig.len(), 64); // SHA-256 hex
        assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));

        // Same input = same signature (deterministic for same timestamp)
        // Can't test exactly because timestamp changes, but format is correct
    }

    #[test]
    fn provider_creation() {
        let provider = SelfHostedCustodyProvider::new("http://custody:8443", b"secret");
        assert_eq!(provider.base_url, "http://custody:8443");
    }
}
