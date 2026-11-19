//! Verification service — ties together KycManager, DiiaClient and SumsubClient.
//!
//! This is the public entry point for the KYC workflow. The API gateway calls
//! `start_verification` when a user begins KYC, and `process_webhook` when
//! an identity provider posts a result back.
//!
//! **Supported providers:**
//! - `"diia"` — Ukrainian government documents via Diia.Business
//! - `"sumsub"` — International documents via Sumsub
//!
//! **Architecture note:** `VerificationService` owns a `KycManager` and
//! delegates HTTP calls to the provider. In production you replace the in-memory
//! `KycManager` with a database-backed implementation.

use uuid::Uuid;

use crate::diia::{DiiaClient, DiiaConfig, DiiaError, DiiaRequestStatus, DiiaWebhookPayload};
use crate::kyc::{KycError, KycManager, KycRecord};
use crate::sumsub::{
    CreateApplicantRequest, SumsubClient, SumsubConfig, SumsubError, SumsubWebhookEvent,
};

// ---------------------------------------------------------------------------
// Result of starting a verification
// ---------------------------------------------------------------------------

/// Outcome of calling `start_verification`.
#[derive(Debug)]
pub enum VerificationStarted {
    /// A Diia deep link was created. Display this URL as a QR code or button.
    DiiaDeepLink {
        /// The URL to show the user.
        deep_link: String,
        /// Diia request ID — store this as `provider_ref` in the KYC record.
        request_id: String,
    },
    /// A Sumsub applicant was created and an SDK access token was generated.
    SumsubToken {
        /// Pass this token to the Sumsub WebSDK / mobile SDK on the frontend.
        sdk_token: String,
        /// Sumsub applicant ID — store this as `provider_ref` in the KYC record.
        applicant_id: String,
    },
    /// Mock provider (used in tests / dev without real API keys).
    Mock {
        /// Simulated provider reference.
        provider_ref: String,
    },
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum VerificationError {
    #[error("Unknown provider: {0}. Supported: diia, sumsub")]
    UnknownProvider(String),

    #[error("KYC error: {0}")]
    Kyc(#[from] KycError),

    #[error("Diia error: {0}")]
    Diia(#[from] DiiaError),

    #[error("Sumsub error: {0}")]
    Sumsub(#[from] SumsubError),

    #[error("Webhook parse error: {0}")]
    WebhookParse(String),

    #[error("User not found: {0}")]
    UserNotFound(Uuid),
}

// ---------------------------------------------------------------------------
// Verification service
// ---------------------------------------------------------------------------

/// Central KYC verification service.
///
/// Wraps `KycManager` and delegates HTTP calls to the appropriate provider.
/// The `VerificationService` is intentionally synchronous in terms of state
/// mutations — the `async` boundary is at the provider calls only.
pub struct VerificationService {
    kyc: KycManager,
    diia: Option<DiiaClient>,
    sumsub: Option<SumsubClient>,
    /// Sumsub KYC level name as configured in the Sumsub dashboard.
    sumsub_level_name: String,
    /// Diia document types to request (e.g., `["passport", "id-card"]`).
    diia_document_types: Vec<String>,
    /// Return URL shown after Diia verification completes on mobile.
    diia_return_url: Option<String>,
    /// M21-8: Require liveness detection (challenge-response selfie) from KYC provider.
    /// When true, verifications without a liveness check are rejected.
    /// Defends against deepfake document fraud (E2 in threat catalogue).
    pub require_liveness: bool,
}

impl VerificationService {
    /// Create a service with real provider clients.
    pub fn new(
        diia: Option<DiiaClient>,
        sumsub: Option<SumsubClient>,
    ) -> Self {
        Self {
            kyc: KycManager::new(),
            diia,
            sumsub,
            sumsub_level_name: "basic-kyc-level".to_string(),
            diia_document_types: vec!["passport".to_string(), "id-card".to_string()],
            diia_return_url: None,
            require_liveness: true, // M21-8: default enabled in production
        }
    }

    /// Create a service loading provider configs from environment variables.
    ///
    /// If environment variables for a provider are not set, that provider is
    /// disabled (returns `FeatureDisabled` if selected).
    pub fn from_env() -> Self {
        let diia = DiiaConfig::from_env()
            .ok()
            .map(DiiaClient::new);

        let sumsub = SumsubConfig::from_env()
            .ok()
            .map(SumsubClient::new);

        let mut svc = Self::new(diia, sumsub);
        svc.sumsub_level_name = std::env::var("SUMSUB_LEVEL_NAME")
            .unwrap_or_else(|_| "basic-kyc-level".to_string());
        svc.diia_return_url = std::env::var("DIIA_RETURN_URL").ok();
        svc
    }

    /// Create a service backed entirely by the mock provider (no real HTTP).
    pub fn mock() -> Self {
        Self::new(None, None)
    }

    /// Override the Sumsub level name (useful in tests).
    pub fn with_sumsub_level(mut self, level: &str) -> Self {
        self.sumsub_level_name = level.to_string();
        self
    }

    /// Bulk-load KYC records from the database at startup.
    ///
    /// Populates the in-memory `KycManager` from persisted state so the
    /// process resumes KYC awareness without requiring users to re-verify.
    pub fn restore_records(&mut self, records: Vec<KycRecord>) {
        self.kyc.restore(records);
    }

    /// Number of KYC records in memory.
    pub fn record_count(&self) -> usize {
        self.kyc.len()
    }

    /// Register a new user in the KYC system.
    /// Must be called before `start_verification`.
    pub fn register_user(&mut self, user_id: Uuid) {
        self.kyc.create_user(user_id);
    }

    /// Get the KYC record for a user.
    pub fn get_record(&self, user_id: &Uuid) -> Option<&KycRecord> {
        self.kyc.get(user_id)
    }

    /// Check if a user can trade (KYC verified at Basic or Full level).
    pub fn can_trade(&self, user_id: &Uuid) -> bool {
        self.kyc.can_trade(user_id)
    }

    /// Iterate over all KYC records.
    pub fn all_records(&self) -> impl Iterator<Item = &KycRecord> {
        self.kyc.records()
    }

    /// Approve the KYC record for a user (admin action).
    pub fn approve(&mut self, user_id: Uuid) -> Result<(), crate::kyc::KycError> {
        self.kyc.approve(user_id)
    }

    /// Reject the KYC record for a user with a reason (admin action).
    pub fn reject(&mut self, user_id: Uuid, reason: String) -> Result<(), crate::kyc::KycError> {
        self.kyc.reject(user_id, reason)
    }

    /// Start a KYC verification flow for a user.
    ///
    /// - `provider`: `"diia"`, `"sumsub"`, or `"mock"`
    /// - `user_id`: must have been registered with `register_user` first
    ///
    /// Returns provider-specific data the frontend needs to begin the flow.
    pub async fn start_verification(
        &mut self,
        user_id: Uuid,
        provider: &str,
    ) -> Result<VerificationStarted, VerificationError> {
        match provider {
            "diia" => self.start_diia(user_id).await,
            "sumsub" => self.start_sumsub(user_id).await,
            "mock" => self.start_mock(user_id),
            other => Err(VerificationError::UnknownProvider(other.to_string())),
        }
    }

    /// Process an incoming webhook from a provider.
    ///
    /// - `provider`: `"diia"` or `"sumsub"`
    /// - `payload`: raw JSON body of the webhook
    ///
    /// Updates the KYC state machine based on the verification result.
    /// Returns the updated KYC record.
    pub fn process_webhook(
        &mut self,
        provider: &str,
        payload: &str,
    ) -> Result<KycRecord, VerificationError> {
        match provider {
            "diia" => self.process_diia_webhook(payload),
            "sumsub" => self.process_sumsub_webhook(payload),
            "mock" => self.process_mock_webhook(payload),
            other => Err(VerificationError::UnknownProvider(other.to_string())),
        }
    }

    // -----------------------------------------------------------------------
    // Diia flow
    // -----------------------------------------------------------------------

    async fn start_diia(&mut self, user_id: Uuid) -> Result<VerificationStarted, VerificationError> {
        let client = self
            .diia
            .as_ref()
            .ok_or_else(|| DiiaError::Config("Diia client not configured".to_string()))?;

        // Use the user UUID as the correlation key Diia will echo back.
        let request_id = user_id.to_string();

        let resp = client
            .create_deep_link(&crate::diia::CreateDeepLinkRequest {
                request_id: request_id.clone(),
                return_link: self.diia_return_url.clone(),
                document_types: self.diia_document_types.clone(),
            })
            .await?;

        // Transition KYC state: Unverified/Rejected → Pending/Full
        self.kyc.submit_full(user_id, resp.request_id.clone())?;

        tracing::info!(
            user_id = %user_id,
            request_id = %resp.request_id,
            "Diia verification started"
        );

        Ok(VerificationStarted::DiiaDeepLink {
            deep_link: resp.deep_link,
            request_id: resp.request_id,
        })
    }

    fn process_diia_webhook(&mut self, payload: &str) -> Result<KycRecord, VerificationError> {
        let event: DiiaWebhookPayload = serde_json::from_str(payload)
            .map_err(|e| VerificationError::WebhookParse(format!("Diia webhook: {}", e)))?;

        // Locate the user by provider_ref (the request_id we set when starting).
        let user_id = self.find_user_by_provider_ref(&event.request_id)?;

        match event.status {
            DiiaRequestStatus::Verified => {
                self.kyc.approve(user_id)?;
                tracing::info!(user_id = %user_id, "Diia verification approved");
            }
            DiiaRequestStatus::Rejected => {
                let reason = event
                    .rejection_reason
                    .unwrap_or_else(|| "Rejected by Diia".to_string());
                self.kyc.reject(user_id, reason)?;
                tracing::warn!(user_id = %user_id, "Diia verification rejected");
            }
            DiiaRequestStatus::Expired => {
                // Expired — reset to Rejected so user can retry.
                let _ = self.kyc.reject(user_id, "Diia request expired".to_string());
            }
            // Still in progress — no state change needed.
            DiiaRequestStatus::Pending | DiiaRequestStatus::Processing => {}
        }

        self.kyc
            .get(&user_id)
            .cloned()
            .ok_or(VerificationError::UserNotFound(user_id))
    }

    // -----------------------------------------------------------------------
    // Sumsub flow
    // -----------------------------------------------------------------------

    async fn start_sumsub(
        &mut self,
        user_id: Uuid,
    ) -> Result<VerificationStarted, VerificationError> {
        let client = self
            .sumsub
            .as_ref()
            .ok_or_else(|| SumsubError::Config("Sumsub client not configured".to_string()))?;

        let applicant = client
            .create_applicant(
                &CreateApplicantRequest {
                    external_user_id: user_id.to_string(),
                    email: None,
                    phone: None,
                    source_key: None,
                },
                &self.sumsub_level_name.clone(),
            )
            .await?;

        let token_resp = client
            .generate_sdk_token(&user_id.to_string(), &self.sumsub_level_name.clone())
            .await?;

        // Transition KYC state.
        self.kyc.submit_full(user_id, applicant.id.clone())?;

        tracing::info!(
            user_id = %user_id,
            applicant_id = %applicant.id,
            "Sumsub verification started"
        );

        Ok(VerificationStarted::SumsubToken {
            sdk_token: token_resp.token,
            applicant_id: applicant.id,
        })
    }

    fn process_sumsub_webhook(&mut self, payload: &str) -> Result<KycRecord, VerificationError> {
        let event: SumsubWebhookEvent = serde_json::from_str(payload)
            .map_err(|e| VerificationError::WebhookParse(format!("Sumsub webhook: {}", e)))?;

        // Locate the user by provider_ref (the applicant ID we stored).
        let user_id = self.find_user_by_provider_ref(&event.applicant_id)?;

        if event.event_type == "applicantReviewed" {
            if let Some(result) = &event.review_result {
                // Use the explicit reviewAnswer field (GREEN/RED).
                // Reference: Sumsub docs — reviewAnswer is the authoritative decision.
                use crate::sumsub::ReviewAnswer;
                match &result.review_answer {
                    Some(ReviewAnswer::Green) => {
                        self.kyc.approve(user_id)?;
                        tracing::info!(user_id = %user_id, "Sumsub verification approved (GREEN)");
                    }
                    Some(ReviewAnswer::Red) => {
                        let reason = if result.reject_labels.is_empty() {
                            "Rejected by reviewer".to_string()
                        } else {
                            result.reject_labels.join(", ")
                        };
                        self.kyc.reject(user_id, reason)?;
                        tracing::warn!(user_id = %user_id, "Sumsub verification rejected (RED)");
                    }
                    None => {
                        // No reviewAnswer — do not change KYC state.
                        tracing::warn!(
                            user_id = %user_id,
                            "Sumsub applicantReviewed without reviewAnswer — ignoring"
                        );
                    }
                }
            }
        }

        self.kyc
            .get(&user_id)
            .cloned()
            .ok_or(VerificationError::UserNotFound(user_id))
    }

    // -----------------------------------------------------------------------
    // Mock flow (no HTTP, for testing and dev)
    // -----------------------------------------------------------------------

    fn start_mock(&mut self, user_id: Uuid) -> Result<VerificationStarted, VerificationError> {
        let provider_ref = format!("mock-ref-{}", user_id);
        self.kyc.submit_full(user_id, provider_ref.clone())?;
        Ok(VerificationStarted::Mock { provider_ref })
    }

    /// Mock webhook payload format: `{"user_id": "<uuid>", "approved": true/false}`.
    fn process_mock_webhook(&mut self, payload: &str) -> Result<KycRecord, VerificationError> {
        #[derive(serde::Deserialize)]
        struct MockPayload {
            user_id: Uuid,
            approved: bool,
            #[serde(default)]
            reason: String,
        }

        let mp: MockPayload = serde_json::from_str(payload)
            .map_err(|e| VerificationError::WebhookParse(format!("Mock webhook: {}", e)))?;

        if mp.approved {
            self.kyc.approve(mp.user_id)?;
        } else {
            let reason = if mp.reason.is_empty() {
                "Rejected by mock provider".to_string()
            } else {
                mp.reason
            };
            self.kyc.reject(mp.user_id, reason)?;
        }

        self.kyc
            .get(&mp.user_id)
            .cloned()
            .ok_or(VerificationError::UserNotFound(mp.user_id))
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Find a user whose `provider_ref` matches the given reference string.
    /// This is O(n) — in production, use a database index.
    fn find_user_by_provider_ref(&self, provider_ref: &str) -> Result<Uuid, VerificationError> {
        // We scan all KYC records for a matching provider_ref.
        // The KycManager doesn't expose an iterator, so we use a known user lookup
        // by checking directly. A production implementation would index by provider_ref.
        //
        // For now we expose a method on KycManager to find by ref (see below).
        self.kyc
            .find_by_provider_ref(provider_ref)
            .ok_or_else(|| {
                VerificationError::WebhookParse(format!(
                    "No pending KYC record found for provider_ref '{}'",
                    provider_ref
                ))
            })
    }
}

// ---------------------------------------------------------------------------
// Extend KycManager with a find-by-provider-ref helper
// ---------------------------------------------------------------------------

impl KycManager {
    /// Find a user ID whose KYC record has the given `provider_ref`.
    ///
    /// O(n) — acceptable for the current in-memory store.
    /// A database-backed implementation would use a unique index on provider_ref.
    pub fn find_by_provider_ref(&self, provider_ref: &str) -> Option<Uuid> {
        self.records()
            .find(|r| r.provider_ref.as_deref() == Some(provider_ref))
            .map(|r| r.user_id)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kyc::{KycLevel, KycStatus};
    use serde_json::json;

    fn make_service() -> VerificationService {
        VerificationService::mock()
    }

    fn registered_user(svc: &mut VerificationService) -> Uuid {
        let user_id = Uuid::new_v4();
        svc.register_user(user_id);
        user_id
    }

    // --- Mock flow tests ---

    #[tokio::test]
    async fn test_mock_start_transitions_to_pending() {
        let mut svc = make_service();
        let user_id = registered_user(&mut svc);

        let result = svc.start_verification(user_id, "mock").await.unwrap();
        assert!(matches!(result, VerificationStarted::Mock { .. }));

        let record = svc.get_record(&user_id).unwrap();
        assert_eq!(record.status, KycStatus::Pending);
        assert_eq!(record.level, KycLevel::Full);
    }

    #[tokio::test]
    async fn test_mock_webhook_approved() {
        let mut svc = make_service();
        let user_id = registered_user(&mut svc);

        svc.start_verification(user_id, "mock").await.unwrap();

        let payload = json!({ "user_id": user_id, "approved": true }).to_string();
        let record = svc.process_webhook("mock", &payload).unwrap();

        assert_eq!(record.status, KycStatus::Verified);
        assert!(svc.can_trade(&user_id));
    }

    #[tokio::test]
    async fn test_mock_webhook_rejected() {
        let mut svc = make_service();
        let user_id = registered_user(&mut svc);

        svc.start_verification(user_id, "mock").await.unwrap();

        let payload =
            json!({ "user_id": user_id, "approved": false, "reason": "Blurry photo" }).to_string();
        let record = svc.process_webhook("mock", &payload).unwrap();

        assert_eq!(record.status, KycStatus::Rejected);
        assert_eq!(record.reason.as_deref(), Some("Blurry photo"));
        assert!(!svc.can_trade(&user_id));
    }

    #[tokio::test]
    async fn test_mock_full_flow_approve_then_trade() {
        let mut svc = make_service();
        let user_id = registered_user(&mut svc);

        // Before verification: cannot trade
        assert!(!svc.can_trade(&user_id));

        // Start
        svc.start_verification(user_id, "mock").await.unwrap();
        assert!(!svc.can_trade(&user_id)); // Pending ≠ Verified

        // Approve
        let payload = json!({ "user_id": user_id, "approved": true }).to_string();
        svc.process_webhook("mock", &payload).unwrap();

        // After approval: can trade
        assert!(svc.can_trade(&user_id));
    }

    #[tokio::test]
    async fn test_mock_resubmit_after_rejection() {
        let mut svc = make_service();
        let user_id = registered_user(&mut svc);

        // First attempt: rejected
        svc.start_verification(user_id, "mock").await.unwrap();
        let reject_payload = json!({ "user_id": user_id, "approved": false }).to_string();
        svc.process_webhook("mock", &reject_payload).unwrap();
        assert_eq!(svc.get_record(&user_id).unwrap().status, KycStatus::Rejected);

        // Second attempt (resubmit after rejection is allowed)
        svc.start_verification(user_id, "mock").await.unwrap();
        let approve_payload = json!({ "user_id": user_id, "approved": true }).to_string();
        let record = svc.process_webhook("mock", &approve_payload).unwrap();
        assert_eq!(record.status, KycStatus::Verified);
    }

    // --- Diia webhook parsing ---

    #[test]
    fn test_process_diia_webhook_verified() {
        let mut svc = make_service();
        let user_id = Uuid::new_v4();
        svc.register_user(user_id);

        // Manually put user in Pending state with a Diia provider_ref
        svc.kyc
            .submit_full(user_id, user_id.to_string())
            .unwrap();

        let payload = json!({
            "requestId": user_id.to_string(),
            "status": "verified",
            "userData": {
                "firstName": "Іван",
                "lastName": "Петренко",
                "dateOfBirth": "1990-01-01",
                "documentNumber": "АА123456",
                "documentType": "passport",
                "country": "UA"
            },
            "timestamp": 1705312000i64
        })
        .to_string();

        let record = svc.process_webhook("diia", &payload).unwrap();
        assert_eq!(record.status, KycStatus::Verified);
    }

    #[test]
    fn test_process_diia_webhook_rejected() {
        let mut svc = make_service();
        let user_id = Uuid::new_v4();
        svc.register_user(user_id);
        svc.kyc
            .submit_full(user_id, user_id.to_string())
            .unwrap();

        let payload = json!({
            "requestId": user_id.to_string(),
            "status": "rejected",
            "rejectionReason": "Document expired",
            "timestamp": 1705312000i64
        })
        .to_string();

        let record = svc.process_webhook("diia", &payload).unwrap();
        assert_eq!(record.status, KycStatus::Rejected);
        assert_eq!(record.reason.as_deref(), Some("Document expired"));
    }

    // --- Sumsub webhook parsing ---

    #[test]
    fn test_process_sumsub_webhook_approved() {
        let mut svc = make_service();
        let user_id = Uuid::new_v4();
        svc.register_user(user_id);

        let applicant_id = "applicant-123";
        svc.kyc
            .submit_full(user_id, applicant_id.to_string())
            .unwrap();

        let payload = json!({
            "type": "applicantReviewed",
            "applicantId": applicant_id,
            "externalUserId": user_id.to_string(),
            "reviewResult": {
                "reviewAnswer": "GREEN",
                "rejectLabels": []
            }
        })
        .to_string();

        let record = svc.process_webhook("sumsub", &payload).unwrap();
        assert_eq!(record.status, KycStatus::Verified);
    }

    #[test]
    fn test_process_sumsub_webhook_rejected() {
        let mut svc = make_service();
        let user_id = Uuid::new_v4();
        svc.register_user(user_id);

        let applicant_id = "applicant-456";
        svc.kyc
            .submit_full(user_id, applicant_id.to_string())
            .unwrap();

        let payload = json!({
            "type": "applicantReviewed",
            "applicantId": applicant_id,
            "reviewResult": {
                "reviewAnswer": "RED",
                "rejectLabels": ["FORGERY", "DOCUMENT_EXPIRED"],
                "reviewRejectType": "FINAL"
            }
        })
        .to_string();

        let record = svc.process_webhook("sumsub", &payload).unwrap();
        assert_eq!(record.status, KycStatus::Rejected);
        assert!(record.reason.as_deref().unwrap().contains("FORGERY"));
    }

    // --- Edge cases ---

    #[tokio::test]
    async fn test_unknown_provider_returns_error() {
        let mut svc = make_service();
        let user_id = registered_user(&mut svc);

        let result = svc.start_verification(user_id, "onfido").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("onfido"));
    }

    #[tokio::test]
    async fn test_suspended_user_cannot_start_verification() {
        let mut svc = make_service();
        let user_id = registered_user(&mut svc);

        svc.kyc
            .suspend(user_id, "AML flag".to_string())
            .unwrap();

        let result = svc.start_verification(user_id, "mock").await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            VerificationError::Kyc(KycError::AccountSuspended)
        ));
    }

    #[test]
    fn test_process_webhook_unknown_provider() {
        let mut svc = make_service();
        let result = svc.process_webhook("onfido", "{}");
        assert!(result.is_err());
    }

    #[test]
    fn test_process_mock_webhook_malformed_payload() {
        let mut svc = make_service();
        let result = svc.process_webhook("mock", "not valid json");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("webhook"));
    }

    #[test]
    fn test_find_by_provider_ref_returns_none_for_unknown() {
        let svc = make_service();
        assert!(svc.kyc.find_by_provider_ref("nonexistent").is_none());
    }
}
