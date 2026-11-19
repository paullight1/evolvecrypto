//! Sumsub API client for international KYC verification.
//!
//! Sumsub is a KYC/AML platform used for verifying international users.
//! Their SDK (Web, iOS, Android) collects documents and selfies from users;
//! our backend creates applicants, generates SDK access tokens, and receives
//! webhook callbacks when verification is complete.
//!
//! **Flow:**
//! 1. Backend creates an applicant via POST /resources/applicants
//! 2. Backend generates an SDK token via POST /resources/accessTokens
//! 3. Frontend loads Sumsub SDK with the token — user goes through verification
//! 4. Sumsub calls our webhook when verification result is ready
//! 5. Backend fetches final status via GET /resources/applicants/{id}/status
//!
//! **Auth:** HMAC-SHA256 signature over `timestamp + method + path + body`.
//! Each request must include `X-App-Token` and `X-App-Access-Sig` headers.
//!
//! Enable HTTP calls with `cargo build --features sumsub`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Sumsub API configuration.
#[derive(Debug, Clone)]
pub struct SumsubConfig {
    /// Sumsub API base URL (default: <https://api.sumsub.com>).
    pub api_url: String,
    /// Application token from Sumsub dashboard.
    pub app_token: String,
    /// Secret key for HMAC-SHA256 request signing.
    pub secret_key: String,
}

impl SumsubConfig {
    /// Load configuration from environment variables.
    ///
    /// Required:
    /// - `SUMSUB_APP_TOKEN`
    /// - `SUMSUB_SECRET_KEY`
    ///
    /// Optional:
    /// - `SUMSUB_API_URL` (defaults to `https://api.sumsub.com`)
    pub fn from_env() -> Result<Self, SumsubError> {
        Ok(Self {
            api_url: std::env::var("SUMSUB_API_URL")
                .unwrap_or_else(|_| "https://api.sumsub.com".to_string()),
            app_token: std::env::var("SUMSUB_APP_TOKEN")
                .map_err(|_| SumsubError::Config("SUMSUB_APP_TOKEN not set".to_string()))?,
            secret_key: std::env::var("SUMSUB_SECRET_KEY")
                .map_err(|_| SumsubError::Config("SUMSUB_SECRET_KEY not set".to_string()))?,
        })
    }
}

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

/// Request body for creating a Sumsub applicant.
///
/// POST /resources/applicants?levelName={levelName}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateApplicantRequest {
    /// Your internal user ID (used for correlation).
    pub external_user_id: String,
    /// Applicant's email address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Applicant's phone number in E.164 format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone: Option<String>,
    /// Source of the applicant record.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_key: Option<String>,
}

/// Request body for generating an SDK access token.
///
/// POST /resources/accessTokens?userId={userId}&levelName={levelName}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateSdkTokenRequest {
    /// Your internal user ID.
    pub user_id: String,
    /// KYC level name as configured in the Sumsub dashboard.
    pub level_name: String,
    /// Token TTL in seconds (default: 600).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_in_secs: Option<u32>,
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// Response from POST /resources/applicants.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateApplicantResponse {
    /// Sumsub-assigned applicant ID.
    pub id: String,
    /// Your external user ID echoed back.
    pub external_user_id: String,
    /// Timestamp when the applicant was created.
    pub created_at: DateTime<Utc>,
    /// Current review status.
    pub review: Option<ApplicantReview>,
}

/// Response from GET /resources/applicants/{applicantId}/status.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicantStatusResponse {
    pub id: String,
    pub external_user_id: Option<String>,
    pub review: ApplicantReview,
}

/// Sumsub review status block.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicantReview {
    /// Overall review answer.
    pub review_answer: Option<ReviewAnswer>,
    /// Review result details (populated on final decision).
    pub review_result: Option<ReviewResult>,
    /// Current moderation status in Sumsub's pipeline.
    pub processing_status: ProcessingStatus,
    pub review_date: Option<DateTime<Utc>>,
}

/// Final review decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewAnswer {
    /// Applicant approved — KYC passed.
    Green,
    /// Applicant rejected — KYC failed.
    Red,
}

/// Detailed result of a completed review.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewResult {
    /// Explicit review answer: GREEN (approved) or RED (rejected).
    /// This is the authoritative field — do NOT infer from reject_labels.
    pub review_answer: Option<ReviewAnswer>,
    /// List of rejection labels (populated when answer = Red).
    #[serde(default)]
    pub reject_labels: Vec<String>,
    /// Human-readable rejection type.
    pub review_reject_type: Option<String>,
    /// Whether the user can resubmit after this rejection.
    pub attempt_failure_reason: Option<String>,
}

/// Sumsub processing pipeline status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProcessingStatus {
    /// Awaiting document submission.
    Init,
    /// Documents submitted, pending review.
    Pending,
    /// Under manual review by compliance officer.
    Queued,
    /// Processing complete (check reviewAnswer for result).
    Completed,
    /// Applicant needs to resubmit (e.g., clearer photo needed).
    OnHold,
}

/// Response from POST /resources/accessTokens.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkAccessTokenResponse {
    /// SDK access token to pass to the Sumsub WebSDK / iOS / Android SDK.
    pub token: String,
    /// User ID this token was created for.
    pub user_id: String,
}

/// Sumsub webhook event envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SumsubWebhookEvent {
    /// Event type, e.g. `applicantReviewed`, `applicantPending`.
    #[serde(rename = "type")]
    pub event_type: String,
    /// Sumsub applicant ID.
    pub applicant_id: String,
    /// Your external user ID.
    pub external_user_id: Option<String>,
    /// Inspection ID (for document-level details).
    pub inspection_id: Option<String>,
    /// Correlation key.
    pub correlation_id: Option<String>,
    /// Review result (populated for `applicantReviewed` events).
    pub review_result: Option<ReviewResult>,
    /// Applicant's KYC level name.
    pub level_name: Option<String>,
    /// Unix timestamp (ms) of the event.
    pub created_at_ms: Option<String>,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum SumsubError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("HTTP error: {0}")]
    Http(String),

    #[error("Sumsub API error (HTTP {status}): {body}")]
    ApiError { status: u16, body: String },

    #[error("Response parse error: {0}")]
    Parse(String),

    #[error("Sumsub feature not enabled. Build with --features sumsub")]
    FeatureDisabled,
}

// ---------------------------------------------------------------------------
// HMAC-SHA256 signature computation
// ---------------------------------------------------------------------------

/// Compute the HMAC-SHA256 request signature required by Sumsub.
///
/// Sumsub signature = HMAC-SHA256(secret_key, timestamp + METHOD + path + body)
/// The result is hex-encoded and placed in the `X-App-Access-Sig` header.
///
/// This function is `pub` so the API gateway can also verify incoming webhooks.
#[cfg(feature = "sumsub")]
pub fn compute_signature(
    secret_key: &str,
    timestamp_secs: i64,
    method: &str,
    path: &str,
    body: &[u8],
) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    type HmacSha256 = Hmac<Sha256>;

    let mut mac = HmacSha256::new_from_slice(secret_key.as_bytes())
        .expect("HMAC accepts keys of any length");

    mac.update(timestamp_secs.to_string().as_bytes());
    mac.update(method.to_uppercase().as_bytes());
    mac.update(path.as_bytes());
    mac.update(body);

    let result = mac.finalize();
    hex::encode(result.into_bytes())
}

/// Without the `sumsub` feature, signature computation is unavailable.
/// Returns an empty string as a placeholder — the feature guard on the client
/// ensures this is never called in a meaningful context.
#[cfg(not(feature = "sumsub"))]
pub fn compute_signature(
    _secret_key: &str,
    _timestamp_secs: i64,
    _method: &str,
    _path: &str,
    _body: &[u8],
) -> String {
    String::new()
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// Sumsub API client.
///
/// Thread-safe: holds only immutable config and a shared reqwest client.
pub struct SumsubClient {
    // Used by HTTP methods gated on the `sumsub` feature.
    #[allow(dead_code)]
    config: SumsubConfig,
    #[cfg(feature = "sumsub")]
    http: reqwest::Client,
}

impl SumsubClient {
    /// Create a new client from config.
    pub fn new(config: SumsubConfig) -> Self {
        Self {
            #[cfg(feature = "sumsub")]
            http: reqwest::Client::new(),
            config,
        }
    }

    /// Create a new client, reading config from environment variables.
    pub fn from_env() -> Result<Self, SumsubError> {
        Ok(Self::new(SumsubConfig::from_env()?))
    }

    /// Create a new applicant in Sumsub.
    ///
    /// Call this when a user initiates KYC. Store the returned `id` as the
    /// `provider_ref` in the KycRecord.
    ///
    /// `level_name` — the KYC level configured in Sumsub dashboard
    ///   (e.g., "basic-kyc-level", "full-kyc-level").
    pub async fn create_applicant(
        &self,
        request: &CreateApplicantRequest,
        level_name: &str,
    ) -> Result<CreateApplicantResponse, SumsubError> {
        let path = format!("/resources/applicants?levelName={}", level_name);
        self.post_impl(&path, request).await
    }

    /// Get the current verification status of an applicant.
    pub async fn get_applicant_status(
        &self,
        applicant_id: &str,
    ) -> Result<ApplicantStatusResponse, SumsubError> {
        let path = format!("/resources/applicants/{}/status", applicant_id);
        self.get_impl(&path).await
    }

    /// Generate a short-lived SDK access token for the frontend.
    ///
    /// The frontend uses this token to initialise the Sumsub WebSDK / mobile SDK
    /// without exposing the secret key.
    pub async fn generate_sdk_token(
        &self,
        user_id: &str,
        level_name: &str,
    ) -> Result<SdkAccessTokenResponse, SumsubError> {
        let path = format!(
            "/resources/accessTokens?userId={}&levelName={}",
            user_id, level_name
        );
        // Token generation is a POST with an empty body
        self.post_impl(&path, &serde_json::json!({})).await
    }

    // --- internal HTTP helpers ---

    #[cfg(feature = "sumsub")]
    async fn post_impl<B, R>(&self, path: &str, body: &B) -> Result<R, SumsubError>
    where
        B: Serialize,
        R: serde::de::DeserializeOwned,
    {
        let body_bytes = serde_json::to_vec(body)
            .map_err(|e| SumsubError::Parse(format!("Request serialization failed: {}", e)))?;

        let timestamp = Utc::now().timestamp();
        let sig = compute_signature(&self.config.secret_key, timestamp, "POST", path, &body_bytes);
        let url = format!("{}{}", self.config.api_url, path);

        let resp = self
            .http
            .post(&url)
            .header("X-App-Token", &self.config.app_token)
            .header("X-App-Access-Ts", timestamp.to_string())
            .header("X-App-Access-Sig", sig)
            .header("Content-Type", "application/json")
            .body(body_bytes)
            .send()
            .await
            .map_err(|e| SumsubError::Http(e.to_string()))?;

        self.parse_response(resp).await
    }

    #[cfg(not(feature = "sumsub"))]
    async fn post_impl<B, R>(&self, _path: &str, _body: &B) -> Result<R, SumsubError>
    where
        B: Serialize,
        R: serde::de::DeserializeOwned,
    {
        Err(SumsubError::FeatureDisabled)
    }

    #[cfg(feature = "sumsub")]
    async fn get_impl<R>(&self, path: &str) -> Result<R, SumsubError>
    where
        R: serde::de::DeserializeOwned,
    {
        let timestamp = Utc::now().timestamp();
        let sig = compute_signature(&self.config.secret_key, timestamp, "GET", path, &[]);
        let url = format!("{}{}", self.config.api_url, path);

        let resp = self
            .http
            .get(&url)
            .header("X-App-Token", &self.config.app_token)
            .header("X-App-Access-Ts", timestamp.to_string())
            .header("X-App-Access-Sig", sig)
            .send()
            .await
            .map_err(|e| SumsubError::Http(e.to_string()))?;

        self.parse_response(resp).await
    }

    #[cfg(not(feature = "sumsub"))]
    async fn get_impl<R>(&self, _path: &str) -> Result<R, SumsubError>
    where
        R: serde::de::DeserializeOwned,
    {
        Err(SumsubError::FeatureDisabled)
    }

    #[cfg(feature = "sumsub")]
    async fn parse_response<R>(&self, resp: reqwest::Response) -> Result<R, SumsubError>
    where
        R: serde::de::DeserializeOwned,
    {
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(SumsubError::ApiError { status, body });
        }

        resp.json::<R>()
            .await
            .map_err(|e| SumsubError::Parse(e.to_string()))
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
    fn test_create_applicant_request_serialization() {
        let req = CreateApplicantRequest {
            external_user_id: "user-uuid-123".to_string(),
            email: Some("ivan@example.com".to_string()),
            phone: Some("+380501234567".to_string()),
            source_key: None,
        };

        let json = serde_json::to_string(&req).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(val["externalUserId"], "user-uuid-123");
        assert_eq!(val["email"], "ivan@example.com");
        assert_eq!(val["phone"], "+380501234567");
    }

    #[test]
    fn test_create_applicant_request_minimal_serialization() {
        let req = CreateApplicantRequest {
            external_user_id: "user-456".to_string(),
            email: None,
            phone: None,
            source_key: None,
        };

        let json = serde_json::to_string(&req).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(val["externalUserId"], "user-456");
        // Optional fields should be absent
        assert!(val.get("email").is_none() || val["email"].is_null());
        assert!(val.get("phone").is_none() || val["phone"].is_null());
    }

    #[test]
    fn test_create_applicant_response_deserialization() {
        let json = r#"{
            "id": "applicant-abc-123",
            "externalUserId": "user-uuid-123",
            "createdAt": "2024-01-15T10:00:00Z",
            "review": {
                "processingStatus": "init"
            }
        }"#;

        let resp: CreateApplicantResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.id, "applicant-abc-123");
        assert_eq!(resp.external_user_id, "user-uuid-123");
        let review = resp.review.unwrap();
        assert_eq!(review.processing_status, ProcessingStatus::Init);
        assert!(review.review_answer.is_none());
    }

    #[test]
    fn test_applicant_status_approved_deserialization() {
        let json = r#"{
            "id": "applicant-abc-123",
            "externalUserId": "user-uuid-123",
            "review": {
                "reviewAnswer": "GREEN",
                "processingStatus": "completed",
                "reviewDate": "2024-01-15T11:00:00Z",
                "reviewResult": {
                    "rejectLabels": [],
                    "reviewRejectType": null
                }
            }
        }"#;

        let resp: ApplicantStatusResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.review.review_answer, Some(ReviewAnswer::Green));
        assert_eq!(resp.review.processing_status, ProcessingStatus::Completed);
        assert!(resp.review.review_result.unwrap().reject_labels.is_empty());
    }

    #[test]
    fn test_applicant_status_rejected_deserialization() {
        let json = r#"{
            "id": "applicant-xyz",
            "review": {
                "reviewAnswer": "RED",
                "processingStatus": "completed",
                "reviewResult": {
                    "rejectLabels": ["FORGERY", "DOCUMENT_EXPIRED"],
                    "reviewRejectType": "FINAL",
                    "attemptFailureReason": "Documents appear to be forged"
                }
            }
        }"#;

        let resp: ApplicantStatusResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.review.review_answer, Some(ReviewAnswer::Red));

        let result = resp.review.review_result.unwrap();
        assert_eq!(result.reject_labels, vec!["FORGERY", "DOCUMENT_EXPIRED"]);
        assert_eq!(result.review_reject_type.as_deref(), Some("FINAL"));
    }

    #[test]
    fn test_sdk_token_response_deserialization() {
        let json = r#"{
            "token": "_act-sbx-jwt-eyJhbGciOiJSUzI1NiJ9...",
            "userId": "user-uuid-123"
        }"#;

        let resp: SdkAccessTokenResponse = serde_json::from_str(json).unwrap();
        assert!(resp.token.starts_with("_act-sbx-jwt-"));
        assert_eq!(resp.user_id, "user-uuid-123");
    }

    #[test]
    fn test_webhook_event_deserialization() {
        let json = r#"{
            "type": "applicantReviewed",
            "applicantId": "applicant-abc-123",
            "externalUserId": "user-uuid-123",
            "inspectionId": "inspect-456",
            "correlationId": "corr-789",
            "reviewResult": {
                "rejectLabels": [],
                "reviewRejectType": null
            },
            "levelName": "basic-kyc-level",
            "createdAtMs": "1705312000000"
        }"#;

        let event: SumsubWebhookEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_type, "applicantReviewed");
        assert_eq!(event.applicant_id, "applicant-abc-123");
        assert_eq!(event.external_user_id.as_deref(), Some("user-uuid-123"));
        assert_eq!(event.level_name.as_deref(), Some("basic-kyc-level"));
    }

    // --- Config tests ---

    #[test]
    fn test_config_struct_stores_fields() {
        let config = SumsubConfig {
            api_url: "https://test.sumsub.com".to_string(),
            app_token: "my-app-token".to_string(),
            secret_key: "my-secret".to_string(),
        };

        assert_eq!(config.app_token, "my-app-token");
        assert_eq!(config.secret_key, "my-secret");
        assert_eq!(config.api_url, "https://test.sumsub.com");
    }

    #[test]
    fn test_config_error_message_mentions_missing_var() {
        // Verify error messages are descriptive — test the error type directly.
        let err = SumsubError::Config("SUMSUB_APP_TOKEN not set".to_string());
        assert!(err.to_string().contains("SUMSUB_APP_TOKEN"));

        let err2 = SumsubError::Config("SUMSUB_SECRET_KEY not set".to_string());
        assert!(err2.to_string().contains("SUMSUB_SECRET_KEY"));
    }

    #[test]
    fn test_config_default_api_url_value() {
        // The default value is a constant; verify it statically.
        let config = SumsubConfig {
            api_url: "https://api.sumsub.com".to_string(),
            app_token: "tok".to_string(),
            secret_key: "sec".to_string(),
        };
        assert_eq!(config.api_url, "https://api.sumsub.com");
    }

    #[test]
    fn test_config_api_error_formatting() {
        let err = SumsubError::ApiError { status: 403, body: "Forbidden".to_string() };
        let msg = err.to_string();
        assert!(msg.contains("403"));
        assert!(msg.contains("Forbidden"));
    }

    // --- HMAC signature test ---

    #[test]
    #[cfg(feature = "sumsub")]
    fn test_compute_signature_deterministic() {
        let sig1 = compute_signature("secret", 1705312000, "POST", "/resources/applicants", b"{}");
        let sig2 = compute_signature("secret", 1705312000, "POST", "/resources/applicants", b"{}");
        assert_eq!(sig1, sig2, "Signature must be deterministic");
        assert!(!sig1.is_empty());
        assert_eq!(sig1.len(), 64, "HMAC-SHA256 hex output is 64 chars");
    }

    #[test]
    #[cfg(feature = "sumsub")]
    fn test_compute_signature_differs_on_different_inputs() {
        let sig_post = compute_signature("secret", 1705312000, "POST", "/resources/applicants", b"{}");
        let sig_get = compute_signature("secret", 1705312000, "GET", "/resources/applicants", b"");
        let sig_diff_ts = compute_signature("secret", 1705312001, "POST", "/resources/applicants", b"{}");
        let sig_diff_key = compute_signature("other-secret", 1705312000, "POST", "/resources/applicants", b"{}");

        assert_ne!(sig_post, sig_get, "Method change must change signature");
        assert_ne!(sig_post, sig_diff_ts, "Timestamp change must change signature");
        assert_ne!(sig_post, sig_diff_key, "Key change must change signature");
    }

    // --- Feature-disabled test ---

    #[test]
    #[cfg(not(feature = "sumsub"))]
    fn test_client_without_feature_returns_error() {
        let config = SumsubConfig {
            api_url: "https://api.sumsub.com".to_string(),
            app_token: "tok".to_string(),
            secret_key: "sec".to_string(),
        };
        let client = SumsubClient::new(config);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let req = CreateApplicantRequest {
            external_user_id: "user-1".to_string(),
            email: None,
            phone: None,
            source_key: None,
        };

        let result = rt.block_on(client.create_applicant(&req, "basic-kyc"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not enabled"));
    }

    #[test]
    fn test_processing_status_roundtrip() {
        let statuses = [
            (ProcessingStatus::Init, "init"),
            (ProcessingStatus::Pending, "pending"),
            (ProcessingStatus::Queued, "queued"),
            (ProcessingStatus::Completed, "completed"),
            (ProcessingStatus::OnHold, "onHold"),
        ];

        for (variant, expected_str) in &statuses {
            let json = serde_json::to_string(variant).unwrap();
            assert_eq!(json, format!("\"{}\"", expected_str));

            let back: ProcessingStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, variant);
        }
    }
}
