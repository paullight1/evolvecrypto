//! Chainalysis KYT (Know Your Transaction) API client.
//!
//! Screens blockchain addresses for risk indicators: sanctions exposure,
//! darknet market activity, ransomware, mixer usage, etc.
//!
//! # API Reference
//! - Chainalysis KYT API v2: `GET /v2/entities/{address}`
//! - Authentication: `X-API-Key` header
//! - Response includes `risk` field: "Severe", "High", "Medium", "Low"
//!
//! # Configuration
//! - `CHAINALYSIS_API_KEY` env var (required)
//! - `CHAINALYSIS_BASE_URL` env var (default: https://api.chainalysis.com)
//!
//! # Fail-closed policy
//! If the API call fails (timeout, error), the address is treated as
//! `Suspicious` — not `Clean`. This prevents an attacker from bypassing
//! screening by DDoS-ing the Chainalysis API.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// Risk level returned by Chainalysis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChainalysisRisk {
    Severe,
    High,
    Medium,
    Low,
    /// API call failed — fail-closed as suspicious.
    Unknown,
}

impl ChainalysisRisk {
    /// Returns true if the risk level should block the transaction.
    pub fn is_blocked(&self) -> bool {
        matches!(self, ChainalysisRisk::Severe | ChainalysisRisk::High)
    }

    /// Returns true if the risk level should trigger a review.
    pub fn needs_review(&self) -> bool {
        matches!(self, ChainalysisRisk::Medium | ChainalysisRisk::Unknown)
    }

    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "severe" => ChainalysisRisk::Severe,
            "high" | "highvalue" => ChainalysisRisk::High,
            "medium" => ChainalysisRisk::Medium,
            "low" => ChainalysisRisk::Low,
            _ => ChainalysisRisk::Unknown,
        }
    }
}

/// Result of a Chainalysis address screening.
#[derive(Debug, Clone)]
pub struct ScreeningResult {
    pub address: String,
    pub risk: ChainalysisRisk,
    /// Human-readable risk description.
    pub cluster_name: Option<String>,
    /// Category (e.g., "sanctions", "darknet market", "mixer").
    pub category: Option<String>,
}

/// Chainalysis KYT API client.
///
/// Requires the `chainalysis` cargo feature and `CHAINALYSIS_API_KEY` env var.
pub struct ChainalysisClient {
    api_key: String,
    base_url: String,
    timeout_secs: u64,
}

impl ChainalysisClient {
    /// Create a client from environment variables.
    ///
    /// Returns `None` if `CHAINALYSIS_API_KEY` is not set.
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("CHAINALYSIS_API_KEY").ok()?;
        let base_url = std::env::var("CHAINALYSIS_BASE_URL")
            .unwrap_or_else(|_| "https://api.chainalysis.com".to_string());
        Some(Self {
            api_key,
            base_url,
            timeout_secs: 5,
        })
    }

    /// Create a client with explicit config (for testing).
    pub fn new(api_key: String, base_url: String) -> Self {
        Self {
            api_key,
            base_url,
            timeout_secs: 5,
        }
    }

    /// Screen an address against Chainalysis KYT.
    ///
    /// **Fail-closed**: on timeout or error, returns `ChainalysisRisk::Unknown`
    /// (which triggers a manual review). Never returns `Low` on failure.
    #[cfg(feature = "chainalysis")]
    pub async fn screen_address(&self, address: &str) -> ScreeningResult {
        let url = format!("{}/v2/entities/{}", self.base_url, address);

        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(error = %e, "Chainalysis: failed to build HTTP client");
                return ScreeningResult {
                    address: address.to_string(),
                    risk: ChainalysisRisk::Unknown,
                    cluster_name: None,
                    category: Some("client_error".to_string()),
                };
            }
        };

        match client
            .get(&url)
            .header("X-API-Key", &self.api_key)
            .send()
            .await
        {
            Ok(resp) => {
                if !resp.status().is_success() {
                    tracing::warn!(
                        address = %address,
                        status = %resp.status(),
                        "Chainalysis: non-200 response — treating as suspicious"
                    );
                    return ScreeningResult {
                        address: address.to_string(),
                        risk: ChainalysisRisk::Unknown,
                        cluster_name: None,
                        category: Some(format!("http_{}", resp.status().as_u16())),
                    };
                }

                match resp.json::<serde_json::Value>().await {
                    Ok(json) => {
                        let risk_str = json.get("risk")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let cluster = json.get("clusterName")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        let category = json.get("category")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());

                        ScreeningResult {
                            address: address.to_string(),
                            risk: ChainalysisRisk::from_str(risk_str),
                            cluster_name: cluster,
                            category,
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Chainalysis: failed to parse response");
                        ScreeningResult {
                            address: address.to_string(),
                            risk: ChainalysisRisk::Unknown,
                            cluster_name: None,
                            category: Some("parse_error".to_string()),
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    address = %address,
                    error = %e,
                    "Chainalysis: request failed — fail-closed as suspicious"
                );
                ScreeningResult {
                    address: address.to_string(),
                    risk: ChainalysisRisk::Unknown,
                    cluster_name: None,
                    category: Some("timeout_or_network".to_string()),
                }
            }
        }
    }

    /// Stub: when chainalysis feature is not enabled, always returns Unknown.
    #[cfg(not(feature = "chainalysis"))]
    pub async fn screen_address(&self, address: &str) -> ScreeningResult {
        tracing::debug!("Chainalysis feature not enabled — returning Unknown for {}", address);
        ScreeningResult {
            address: address.to_string(),
            risk: ChainalysisRisk::Unknown,
            cluster_name: None,
            category: Some("feature_disabled".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_from_str_parses_correctly() {
        assert_eq!(ChainalysisRisk::from_str("severe"), ChainalysisRisk::Severe);
        assert_eq!(ChainalysisRisk::from_str("High"), ChainalysisRisk::High);
        assert_eq!(ChainalysisRisk::from_str("MEDIUM"), ChainalysisRisk::Medium);
        assert_eq!(ChainalysisRisk::from_str("low"), ChainalysisRisk::Low);
        assert_eq!(ChainalysisRisk::from_str("garbage"), ChainalysisRisk::Unknown);
    }

    #[test]
    fn severe_and_high_are_blocked() {
        assert!(ChainalysisRisk::Severe.is_blocked());
        assert!(ChainalysisRisk::High.is_blocked());
        assert!(!ChainalysisRisk::Medium.is_blocked());
        assert!(!ChainalysisRisk::Low.is_blocked());
    }

    #[test]
    fn medium_and_unknown_need_review() {
        assert!(ChainalysisRisk::Medium.needs_review());
        assert!(ChainalysisRisk::Unknown.needs_review());
        assert!(!ChainalysisRisk::Low.needs_review());
        assert!(!ChainalysisRisk::Severe.needs_review());
    }

    #[test]
    fn client_from_env_returns_none_without_key() {
        // In test env, CHAINALYSIS_API_KEY is not set.
        assert!(ChainalysisClient::from_env().is_none());
    }

    #[tokio::test]
    async fn stub_screen_returns_unknown() {
        let client = ChainalysisClient::new("fake-key".into(), "http://localhost:9999".into());
        let result = client.screen_address("0xabc123").await;
        // Without the chainalysis feature, this returns Unknown.
        // With the feature, it would fail to connect and also return Unknown (fail-closed).
        assert!(matches!(result.risk, ChainalysisRisk::Unknown));
    }
}
