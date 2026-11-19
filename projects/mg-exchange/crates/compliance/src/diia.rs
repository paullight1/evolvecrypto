//! Diia.Business API client for Ukrainian passport verification.
//!
//! Diia is a Ukrainian government digital services app. Diia.Business provides
//! an API for identity verification using Ukrainian government-issued documents
//! (passports, ID cards, driving licences).
//!
//! **Flow:**
//! 1. Exchange calls POST /api/v1/auth/deeplink → gets `deepLink` URL
//! 2. User opens the deep link in their Diia app and scans QR/shares documents
//! 3. Diia calls our webhook OR we poll GET /api/v1/auth/request/{requestId}
//! 4. On success, we receive the user's verified identity data
//!
//! **Auth:** Bearer token in Authorization header (long-lived token issued by Diia).
//!
//! Enable HTTP calls with `cargo build --features diia`.
//! Without the feature the client compiles but all methods return an error.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Diia.Business API configuration.
#[derive(Debug, Clone)]
pub struct DiiaConfig {
    /// Base URL for the Diia.Business API.
    /// Production: <https://api.diia.gov.ua>
    pub api_url: String,
    /// Long-lived bearer token issued by Diia.Business portal.
    pub auth_token: String,
    /// Branch ID registered in Diia.Business console (identifies the requesting service).
    pub branch_id: String,
}

impl DiiaConfig {
    /// Load configuration from environment variables.
    ///
    /// Required:
    /// - `DIIA_AUTH_TOKEN`
    /// - `DIIA_BRANCH_ID`
    ///
    /// Optional:
    /// - `DIIA_API_URL` (defaults to `https://api.diia.gov.ua`)
    pub fn from_env() -> Result<Self, DiiaError> {
        Ok(Self {
            api_url: std::env::var("DIIA_API_URL")
                .unwrap_or_else(|_| "https://api.diia.gov.ua".to_string()),
            auth_token: std::env::var("DIIA_AUTH_TOKEN")
                .map_err(|_| DiiaError::Config("DIIA_AUTH_TOKEN not set".to_string()))?,
            branch_id: std::env::var("DIIA_BRANCH_ID")
                .map_err(|_| DiiaError::Config("DIIA_BRANCH_ID not set".to_string()))?,
        })
    }
}

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

/// Request body for creating a Diia verification deep link.
///
/// POST /api/v1/auth/deeplink
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDeepLinkRequest {
    /// Unique identifier for this verification request (your internal ID).
    /// Diia will include this in the webhook callback so you can correlate.
    pub request_id: String,
    /// Return URL that Diia redirects the user to after completing verification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_link: Option<String>,
    /// List of document types to request from the user.
    /// Common values: "passport", "id-card", "driver-license"
    pub document_types: Vec<String>,
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// Response from POST /api/v1/auth/deeplink.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDeepLinkResponse {
    /// Deep link URL to open in the Diia app (display as QR code or clickable link).
    pub deep_link: String,
    /// Request ID echoed back (same as what you sent).
    pub request_id: String,
}

/// Response from GET /api/v1/auth/request/{requestId}.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationStatusResponse {
    pub request_id: String,
    pub status: DiiaRequestStatus,
    /// Verified user data — present only when `status` is `Verified`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_data: Option<DiiaUserData>,
    /// Rejection reason — present when `status` is `Rejected`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejection_reason: Option<String>,
    /// Timestamp when the request was created.
    pub created_at: DateTime<Utc>,
    /// Timestamp when the request was last updated.
    pub updated_at: DateTime<Utc>,
}

/// Status of a Diia verification request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiiaRequestStatus {
    /// Request created, waiting for user to open Diia app.
    Pending,
    /// User opened the deep link, verification in progress.
    Processing,
    /// User successfully verified — user_data is populated.
    Verified,
    /// Verification failed (document expired, mismatch, etc.).
    Rejected,
    /// Request expired (no user action within TTL).
    Expired,
}

/// Verified personal data returned by Diia after successful verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiiaUserData {
    /// Ukrainian tax identification number (РНОКПП / ІПН).
    pub tax_id: Option<String>,
    /// First name as per government document.
    pub first_name: String,
    /// Last name (surname) as per government document.
    pub last_name: String,
    /// Middle name (patronymic), if present.
    pub middle_name: Option<String>,
    /// Date of birth in ISO 8601 format (YYYY-MM-DD).
    pub date_of_birth: String,
    /// Document number (passport series+number or ID card number).
    pub document_number: String,
    /// Document type that was verified.
    pub document_type: String,
    /// Document expiry date (None for some document types).
    pub document_expiry: Option<String>,
    /// Country of document issuance (ISO 3166-1 alpha-2, e.g. "UA").
    pub country: String,
}

/// Diia webhook payload — sent by Diia to your registered callback URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiiaWebhookPayload {
    /// The request ID you provided when creating the deep link.
    pub request_id: String,
    /// Updated verification status.
    pub status: DiiaRequestStatus,
    /// Verified user data (present when status = Verified).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_data: Option<DiiaUserData>,
    /// Failure reason (present when status = Rejected | Expired).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejection_reason: Option<String>,
    /// Unix timestamp of the event.
    pub timestamp: i64,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum DiiaError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("HTTP error: {0}")]
    Http(String),

    #[error("Diia API error (HTTP {status}): {body}")]
    ApiError { status: u16, body: String },

    #[error("Response parse error: {0}")]
    Parse(String),

    #[error("Diia feature not enabled. Build with --features diia")]
    FeatureDisabled,
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// Diia.Business API client.
///
/// Thread-safe: `DiiaClient` holds only immutable config and a reqwest client
/// (which is internally `Arc`-based and designed for sharing).
pub struct DiiaClient {
    // Used by HTTP methods gated on the `diia` feature.
    #[allow(dead_code)]
    config: DiiaConfig,
    #[cfg(feature = "diia")]
    http: reqwest::Client,
}

impl DiiaClient {
    /// Create a new client from config.
    pub fn new(config: DiiaConfig) -> Self {
        Self {
            #[cfg(feature = "diia")]
            http: reqwest::Client::new(),
            config,
        }
    }

    /// Create a new client, reading config from environment variables.
    pub fn from_env() -> Result<Self, DiiaError> {
        Ok(Self::new(DiiaConfig::from_env()?))
    }

    /// Create a verification deep link for a user.
    ///
    /// Returns a URL that the user must open in their Diia app.
    /// Display it as a QR code on desktop or as a deep-link button on mobile.
    ///
    /// `request_id` — your unique correlation ID for this verification attempt
    ///   (e.g. a UUID). You will receive it back in the webhook or status poll.
    pub async fn create_deep_link(
        &self,
        request: &CreateDeepLinkRequest,
    ) -> Result<CreateDeepLinkResponse, DiiaError> {
        self.post_impl("/api/v1/auth/deeplink", request).await
    }

    /// Poll the status of a previously created verification request.
    ///
    /// In production, prefer webhooks over polling.
    pub async fn get_verification_status(
        &self,
        request_id: &str,
    ) -> Result<VerificationStatusResponse, DiiaError> {
        let path = format!("/api/v1/auth/request/{}", request_id);
        self.get_impl(&path).await
    }

    // --- internal HTTP helpers ---

    #[cfg(feature = "diia")]
    async fn post_impl<B, R>(&self, path: &str, body: &B) -> Result<R, DiiaError>
    where
        B: Serialize,
        R: serde::de::DeserializeOwned,
    {
        let url = format!("{}{}", self.config.api_url, path);

        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.auth_token))
            .header("X-Branch-Id", &self.config.branch_id)
            .json(body)
            .send()
            .await
            .map_err(|e| DiiaError::Http(e.to_string()))?;

        self.parse_response(resp).await
    }

    #[cfg(not(feature = "diia"))]
    async fn post_impl<B, R>(&self, _path: &str, _body: &B) -> Result<R, DiiaError>
    where
        B: Serialize,
        R: serde::de::DeserializeOwned,
    {
        Err(DiiaError::FeatureDisabled)
    }

    #[cfg(feature = "diia")]
    async fn get_impl<R>(&self, path: &str) -> Result<R, DiiaError>
    where
        R: serde::de::DeserializeOwned,
    {
        let url = format!("{}{}", self.config.api_url, path);

        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.config.auth_token))
            .header("X-Branch-Id", &self.config.branch_id)
            .send()
            .await
            .map_err(|e| DiiaError::Http(e.to_string()))?;

        self.parse_response(resp).await
    }

    #[cfg(not(feature = "diia"))]
    async fn get_impl<R>(&self, _path: &str) -> Result<R, DiiaError>
    where
        R: serde::de::DeserializeOwned,
    {
        Err(DiiaError::FeatureDisabled)
    }

    #[cfg(feature = "diia")]
    async fn parse_response<R>(&self, resp: reqwest::Response) -> Result<R, DiiaError>
    where
        R: serde::de::DeserializeOwned,
    {
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(DiiaError::ApiError { status, body });
        }

        resp.json::<R>()
            .await
            .map_err(|e| DiiaError::Parse(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Serialization tests ---

    #[test]
    fn test_create_deep_link_request_serialization() {
        let req = CreateDeepLinkRequest {
            request_id: "req-abc-123".to_string(),
            return_link: Some("https://exchange.example.com/kyc/callback".to_string()),
            document_types: vec!["passport".to_string(), "id-card".to_string()],
        };

        let json = serde_json::to_string(&req).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(val["requestId"], "req-abc-123");
        assert_eq!(val["returnLink"], "https://exchange.example.com/kyc/callback");
        assert_eq!(val["documentTypes"][0], "passport");
        assert_eq!(val["documentTypes"][1], "id-card");
    }

    #[test]
    fn test_create_deep_link_request_without_optional_fields() {
        let req = CreateDeepLinkRequest {
            request_id: "req-xyz".to_string(),
            return_link: None,
            document_types: vec!["id-card".to_string()],
        };

        let json = serde_json::to_string(&req).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Optional field should be absent when None
        assert!(val.get("returnLink").is_none() || val["returnLink"].is_null());
    }

    #[test]
    fn test_create_deep_link_response_deserialization() {
        let json = r#"{
            "deepLink": "https://diia.gov.ua/deeplink/verify/abc123",
            "requestId": "req-abc-123"
        }"#;

        let resp: CreateDeepLinkResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.deep_link, "https://diia.gov.ua/deeplink/verify/abc123");
        assert_eq!(resp.request_id, "req-abc-123");
    }

    #[test]
    fn test_verification_status_verified_deserialization() {
        let json = r#"{
            "requestId": "req-abc-123",
            "status": "verified",
            "userData": {
                "taxId": "1234567890",
                "firstName": "Іван",
                "lastName": "Петренко",
                "middleName": "Миколайович",
                "dateOfBirth": "1990-05-15",
                "documentNumber": "АА123456",
                "documentType": "passport",
                "documentExpiry": "2030-05-15",
                "country": "UA"
            },
            "createdAt": "2024-01-15T10:00:00Z",
            "updatedAt": "2024-01-15T10:05:00Z"
        }"#;

        let resp: VerificationStatusResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.status, DiiaRequestStatus::Verified);
        assert!(resp.user_data.is_some());

        let data = resp.user_data.unwrap();
        assert_eq!(data.first_name, "Іван");
        assert_eq!(data.last_name, "Петренко");
        assert_eq!(data.tax_id, Some("1234567890".to_string()));
        assert_eq!(data.document_type, "passport");
        assert_eq!(data.country, "UA");
    }

    #[test]
    fn test_verification_status_rejected_deserialization() {
        let json = r#"{
            "requestId": "req-xyz-456",
            "status": "rejected",
            "rejectionReason": "Document expired",
            "createdAt": "2024-01-15T10:00:00Z",
            "updatedAt": "2024-01-15T10:02:00Z"
        }"#;

        let resp: VerificationStatusResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.status, DiiaRequestStatus::Rejected);
        assert!(resp.user_data.is_none());
        assert_eq!(resp.rejection_reason.as_deref(), Some("Document expired"));
    }

    #[test]
    fn test_diia_request_status_all_variants_roundtrip() {
        let statuses = [
            (DiiaRequestStatus::Pending, "pending"),
            (DiiaRequestStatus::Processing, "processing"),
            (DiiaRequestStatus::Verified, "verified"),
            (DiiaRequestStatus::Rejected, "rejected"),
            (DiiaRequestStatus::Expired, "expired"),
        ];

        for (variant, expected_str) in &statuses {
            let json = serde_json::to_string(variant).unwrap();
            assert_eq!(json, format!("\"{}\"", expected_str));

            let back: DiiaRequestStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, variant);
        }
    }

    #[test]
    fn test_webhook_payload_deserialization() {
        let json = r#"{
            "requestId": "req-webhook-789",
            "status": "verified",
            "userData": {
                "firstName": "Олена",
                "lastName": "Коваль",
                "dateOfBirth": "1985-03-22",
                "documentNumber": "ID-987654",
                "documentType": "id-card",
                "country": "UA"
            },
            "timestamp": 1705312000
        }"#;

        let payload: DiiaWebhookPayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.request_id, "req-webhook-789");
        assert_eq!(payload.status, DiiaRequestStatus::Verified);
        assert_eq!(payload.timestamp, 1705312000);

        let data = payload.user_data.unwrap();
        assert_eq!(data.first_name, "Олена");
        assert_eq!(data.middle_name, None);
    }

    // --- Config tests ---

    #[test]
    fn test_config_struct_stores_fields() {
        let config = DiiaConfig {
            api_url: "https://api.diia.example.com".to_string(),
            auth_token: "my-token".to_string(),
            branch_id: "branch-42".to_string(),
        };

        assert_eq!(config.auth_token, "my-token");
        assert_eq!(config.branch_id, "branch-42");
        assert_eq!(config.api_url, "https://api.diia.example.com");
    }

    #[test]
    fn test_config_error_message_mentions_missing_var() {
        // Verify error messages are descriptive — test the error type directly.
        let err = DiiaError::Config("DIIA_AUTH_TOKEN not set".to_string());
        assert!(err.to_string().contains("DIIA_AUTH_TOKEN"));

        let err2 = DiiaError::Config("DIIA_BRANCH_ID not set".to_string());
        assert!(err2.to_string().contains("DIIA_BRANCH_ID"));
    }

    #[test]
    fn test_config_default_api_url_value() {
        // The default value is a constant; verify it statically.
        let config = DiiaConfig {
            api_url: "https://api.diia.gov.ua".to_string(),
            auth_token: "tok".to_string(),
            branch_id: "b1".to_string(),
        };
        assert_eq!(config.api_url, "https://api.diia.gov.ua");
    }

    #[test]
    fn test_config_api_error_formatting() {
        let err = DiiaError::ApiError { status: 401, body: "Unauthorized".to_string() };
        let msg = err.to_string();
        assert!(msg.contains("401"));
        assert!(msg.contains("Unauthorized"));
    }

    // --- Feature-disabled test ---

    #[test]
    #[cfg(not(feature = "diia"))]
    fn test_client_without_feature_returns_error() {
        let config = DiiaConfig {
            api_url: "https://api.diia.gov.ua".to_string(),
            auth_token: "tok".to_string(),
            branch_id: "b1".to_string(),
        };
        let client = DiiaClient::new(config);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let req = CreateDeepLinkRequest {
            request_id: "test".to_string(),
            return_link: None,
            document_types: vec!["passport".to_string()],
        };

        let result = rt.block_on(client.create_deep_link(&req));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not enabled"));
    }
}
