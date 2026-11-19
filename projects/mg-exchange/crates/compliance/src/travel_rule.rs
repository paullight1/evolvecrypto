use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── VASP Directory (Task 18) ──────────────────────────────────────────────────

/// Information about a known counterparty VASP.
///
/// Used for VASP-to-VASP travel rule data transmission (FATF R.16 / TRISA).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaspInfo {
    /// Unique identifier assigned by the directory (e.g. TRISA member UUID).
    pub vasp_id: String,
    /// Human-readable name of the VASP.
    pub name: String,
    /// HTTPS endpoint that receives inbound travel rule payloads.
    pub endpoint: String,
    /// Public key used to encrypt outgoing travel rule data before transmission.
    pub public_key: String,
    /// ISO 3166-1 alpha-2 country code of the VASP's home jurisdiction.
    pub country: String,
}

/// In-memory directory of known counterparty VASPs.
///
/// Phase 1: manual registration via admin API.
/// Phase 2: integrate with OpenVASP or TRISA for automatic peer discovery.
pub struct VaspDirectory {
    vasps: Vec<VaspInfo>,
}

impl VaspDirectory {
    /// Create an empty directory.
    pub fn new() -> Self {
        Self { vasps: Vec::new() }
    }

    /// Register a new VASP in the directory.
    pub fn register(&mut self, vasp: VaspInfo) {
        self.vasps.push(vasp);
    }

    /// Attempt to identify the VASP that controls a given blockchain address.
    ///
    /// Returns `None` in Phase 1 — automatic address-to-VASP lookup requires
    /// integration with a blockchain analytics provider (Chainalysis, Elliptic)
    /// or the TRISA protocol in Phase 2.
    pub fn find_by_address(&self, _address: &str) -> Option<&VaspInfo> {
        None
    }

    /// Return all registered VASPs.
    pub fn all(&self) -> &[VaspInfo] {
        &self.vasps
    }

    /// Look up a VASP by its ID.
    pub fn find_by_id(&self, vasp_id: &str) -> Option<&VaspInfo> {
        self.vasps.iter().find(|v| v.vasp_id == vasp_id)
    }
}

impl Default for VaspDirectory {
    fn default() -> Self {
        Self::new()
    }
}

/// Transfer direction from the exchange's perspective.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferDirection {
    /// User withdrawing funds to an external address.
    Outgoing,
    /// Deposit arriving from an external address.
    Incoming,
}

/// Lifecycle status of a Travel Rule record.
///
/// ```text
/// NotRequired  (terminal — below threshold)
///
/// DataCollected → Transmitted → (terminal for outgoing)
///              → Failed       (counterparty unreachable)
///
/// DataCollected → Received    (terminal for incoming)
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TravelRuleStatus {
    /// Transfer is below the reporting threshold — no action required.
    NotRequired,
    /// Originator/beneficiary information has been collected.
    DataCollected,
    /// Record has been transmitted to the counterparty VASP (outgoing).
    Transmitted,
    /// Record has been received from the counterparty VASP (incoming).
    Received,
    /// Transmission failed — counterparty VASP unreachable or rejected.
    Failed,
}

/// Travel Rule data that must accompany qualifying transfers.
///
/// FATF Recommendation 16 requires VASPs to collect and transmit
/// originator and beneficiary information for transfers ≥ $1,000 USD.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TravelRuleData {
    pub transfer_id: Uuid,
    pub direction: TransferDirection,
    pub amount_usd: Decimal,
    pub asset: String,

    // --- Originator (sender) ---
    pub originator_name: Option<String>,
    /// Wallet address or internal account ID.
    pub originator_account: Option<String>,
    /// VASP name if the originator is at another institution.
    pub originator_institution: Option<String>,

    // --- Beneficiary (receiver) ---
    pub beneficiary_name: Option<String>,
    /// Wallet address or internal account ID.
    pub beneficiary_account: Option<String>,
    /// VASP name if the beneficiary is at another institution.
    pub beneficiary_institution: Option<String>,

    pub created_at: DateTime<Utc>,
    pub status: TravelRuleStatus,
}

/// Travel Rule engine — manages FATF Recommendation 16 compliance records.
///
/// Tracks all qualifying transfers, stores originator/beneficiary data, and
/// surfaces records that still need to be transmitted to counterparty VASPs.
pub struct TravelRuleEngine {
    /// Minimum transfer amount (USD) that triggers the Travel Rule.
    threshold_usd: Decimal,
    records: Vec<TravelRuleData>,
}

impl TravelRuleEngine {
    /// Create a new engine with the FATF default threshold of $1,000 USD.
    pub fn new() -> Self {
        Self {
            threshold_usd: Decimal::new(1_000, 0),
            records: Vec::new(),
        }
    }

    /// Create an engine with a custom threshold (useful for stricter jurisdictions).
    pub fn with_threshold(threshold_usd: Decimal) -> Self {
        Self {
            threshold_usd,
            records: Vec::new(),
        }
    }

    // --- Core checks ---

    /// Returns `true` if the Travel Rule applies to a transfer of this amount.
    ///
    /// The threshold is inclusive — exactly $1,000 is covered.
    pub fn check_required(&self, amount_usd: Decimal) -> bool {
        amount_usd >= self.threshold_usd
    }

    // --- Record creation ---

    /// Create a Travel Rule record for an outgoing transfer (user withdrawal).
    ///
    /// The exchange is the **originator VASP**. We know the originator
    /// (our customer) and the beneficiary address they provided.
    ///
    /// Status is set to `DataCollected` when above threshold, `NotRequired` otherwise.
    pub fn create_outgoing(
        &mut self,
        transfer_id: Uuid,
        amount_usd: Decimal,
        asset: impl Into<String>,
        originator_name: impl Into<String>,
        beneficiary_address: impl Into<String>,
    ) -> TravelRuleData {
        let status = if self.check_required(amount_usd) {
            TravelRuleStatus::DataCollected
        } else {
            TravelRuleStatus::NotRequired
        };

        let record = TravelRuleData {
            transfer_id,
            direction: TransferDirection::Outgoing,
            amount_usd,
            asset: asset.into(),
            originator_name: Some(originator_name.into()),
            originator_account: None,
            originator_institution: None,
            beneficiary_name: None,
            beneficiary_account: Some(beneficiary_address.into()),
            beneficiary_institution: None,
            created_at: Utc::now(),
            status,
        };

        self.records.push(record.clone());
        record
    }

    /// Create a Travel Rule record for an incoming transfer (deposit).
    ///
    /// The exchange is the **beneficiary VASP**. We know only the originator
    /// address at creation time; the counterparty VASP may send more info later.
    ///
    /// Status is set to `DataCollected` when above threshold, `NotRequired` otherwise.
    pub fn create_incoming(
        &mut self,
        transfer_id: Uuid,
        amount_usd: Decimal,
        asset: impl Into<String>,
        originator_address: impl Into<String>,
    ) -> TravelRuleData {
        let status = if self.check_required(amount_usd) {
            TravelRuleStatus::DataCollected
        } else {
            TravelRuleStatus::NotRequired
        };

        let record = TravelRuleData {
            transfer_id,
            direction: TransferDirection::Incoming,
            amount_usd,
            asset: asset.into(),
            originator_name: None,
            originator_account: Some(originator_address.into()),
            originator_institution: None,
            beneficiary_name: None,
            beneficiary_account: None,
            beneficiary_institution: None,
            created_at: Utc::now(),
            status,
        };

        self.records.push(record.clone());
        record
    }

    // --- Status management ---

    /// Update the status of an existing Travel Rule record.
    ///
    /// Returns `true` if the record was found and updated.
    pub fn update_status(&mut self, transfer_id: Uuid, status: TravelRuleStatus) -> bool {
        if let Some(record) = self.records.iter_mut().find(|r| r.transfer_id == transfer_id) {
            tracing::debug!(
                transfer_id = %transfer_id,
                old_status = ?record.status,
                new_status = ?status,
                "Travel Rule status updated",
            );
            record.status = status;
            true
        } else {
            tracing::warn!(transfer_id = %transfer_id, "Travel Rule record not found for status update");
            false
        }
    }

    // --- Queries ---

    /// Returns all outgoing records in `DataCollected` state that have not yet
    /// been transmitted to the counterparty VASP.
    pub fn pending_transmissions(&self) -> Vec<&TravelRuleData> {
        self.records
            .iter()
            .filter(|r| {
                r.direction == TransferDirection::Outgoing
                    && r.status == TravelRuleStatus::DataCollected
            })
            .collect()
    }

    /// Look up a Travel Rule record by transfer ID.
    pub fn get(&self, transfer_id: Uuid) -> Option<&TravelRuleData> {
        self.records.iter().find(|r| r.transfer_id == transfer_id)
    }

    /// Return all records where the originator name matches the given name.
    ///
    /// Useful for compliance audits — finding all transfers for a specific customer.
    pub fn records_for_user(&self, name: &str) -> Vec<&TravelRuleData> {
        self.records
            .iter()
            .filter(|r| r.originator_name.as_deref() == Some(name))
            .collect()
    }

    /// Total number of records (for testing / metrics).
    pub fn total_records(&self) -> usize {
        self.records.len()
    }
}

impl Default for TravelRuleEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// TravelRuleProvider trait — VASP-to-VASP transmission abstraction
// ---------------------------------------------------------------------------

/// Payload sent to the counterparty VASP when transmitting travel rule data.
///
/// Maps to the core fields of IVMS 101 (interVASP Messaging Standard).
/// Phase 1: flat struct. Phase 2: full IVMS 101 JSON Schema with nested
/// NaturalPerson/LegalPerson objects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransmissionPayload {
    pub transfer_id: String,
    pub direction: TransferDirection,
    pub asset: String,
    pub amount_usd: Decimal,
    pub originator_name: Option<String>,
    pub originator_account: Option<String>,
    pub originator_institution: Option<String>,
    pub beneficiary_name: Option<String>,
    pub beneficiary_account: Option<String>,
    pub beneficiary_institution: Option<String>,
}

impl From<&TravelRuleData> for TransmissionPayload {
    fn from(data: &TravelRuleData) -> Self {
        Self {
            transfer_id: data.transfer_id.to_string(),
            direction: data.direction.clone(),
            asset: data.asset.clone(),
            amount_usd: data.amount_usd,
            originator_name: data.originator_name.clone(),
            originator_account: data.originator_account.clone(),
            originator_institution: data.originator_institution.clone(),
            beneficiary_name: data.beneficiary_name.clone(),
            beneficiary_account: data.beneficiary_account.clone(),
            beneficiary_institution: data.beneficiary_institution.clone(),
        }
    }
}

/// Result of a transmission attempt.
#[derive(Debug, Clone)]
pub enum TransmissionResult {
    /// Successfully delivered to counterparty.
    Success { provider_ref: String },
    /// Counterparty VASP not found or unresponsive.
    VaspNotFound,
    /// Provider returned an error.
    Error { message: String },
}

/// Abstraction for VASP-to-VASP travel rule data transmission.
///
/// Phase 1: `MockTravelRuleProvider` (logs, always succeeds).
/// Phase 2: `NotabeneTravelRuleProvider` (real HTTP client).
pub trait TravelRuleProvider: Send + Sync {
    /// Transmit travel rule data for an outgoing transfer.
    ///
    /// `beneficiary_vasp` is the VASP info from the directory, if known.
    /// If unknown (self-hosted wallet), the provider should handle accordingly.
    fn transmit(
        &self,
        payload: &TransmissionPayload,
        beneficiary_vasp: Option<&VaspInfo>,
    ) -> TransmissionResult;
}

/// Mock provider: logs and returns success. Used in dev/test mode.
pub struct MockTravelRuleProvider;

impl TravelRuleProvider for MockTravelRuleProvider {
    fn transmit(
        &self,
        payload: &TransmissionPayload,
        beneficiary_vasp: Option<&VaspInfo>,
    ) -> TransmissionResult {
        let vasp_name = beneficiary_vasp.map(|v| v.name.as_str()).unwrap_or("unknown/self-hosted");
        tracing::info!(
            transfer_id = %payload.transfer_id,
            amount_usd = %payload.amount_usd,
            asset = %payload.asset,
            vasp = vasp_name,
            "MockTravelRuleProvider: travel rule data transmitted (mock)"
        );
        TransmissionResult::Success {
            provider_ref: format!("mock-{}", payload.transfer_id),
        }
    }
}

/// Notabene provider: transmits via the Notabene REST API.
///
/// Configuration:
/// - `NOTABENE_API_KEY` — API key for authentication
/// - `NOTABENE_BASE_URL` — base URL (default: `https://api.notabene.id`)
/// - `NOTABENE_VASP_DID` — our VASP's DID in the Notabene network
///
/// When API key is not configured, falls back to mock behavior with a warning.
pub struct NotabeneTravelRuleProvider {
    api_key: Option<String>,
    base_url: String,
    our_vasp_did: String,
}

impl NotabeneTravelRuleProvider {
    pub fn from_env(secrets: &dyn exchange_common::SecretProvider) -> Self {
        let api_key = secrets.get("NOTABENE_API_KEY");
        let base_url = std::env::var("NOTABENE_BASE_URL")
            .unwrap_or_else(|_| "https://api.notabene.id".to_string());
        let our_vasp_did = std::env::var("NOTABENE_VASP_DID")
            .unwrap_or_else(|_| "did:ethr:notconfigured".to_string());

        if api_key.is_none() {
            tracing::warn!("NOTABENE_API_KEY not set — Travel Rule transmissions will be mocked");
        }

        Self {
            api_key,
            base_url,
            our_vasp_did,
        }
    }
}

impl TravelRuleProvider for NotabeneTravelRuleProvider {
    fn transmit(
        &self,
        payload: &TransmissionPayload,
        _beneficiary_vasp: Option<&VaspInfo>,
    ) -> TransmissionResult {
        let Some(ref api_key) = self.api_key else {
            tracing::warn!(
                transfer_id = %payload.transfer_id,
                "Notabene: no API key configured — cannot transmit"
            );
            return TransmissionResult::Error {
                message: "NOTABENE_API_KEY not configured".to_string(),
            };
        };

        let url = format!("{}/tf/create", self.base_url);

        // Build the Notabene transfer creation payload.
        // Ref: https://docs.notabene.id/reference/create-a-transfer
        let body = serde_json::json!({
            "transactionType": "BELOW_THRESHOLD", // Notabene determines actual type
            "transactionAsset": payload.asset,
            "transactionAmount": payload.amount_usd.to_string(),
            "originatorVASPdid": self.our_vasp_did,
            "originatorEqualsBeneficiary": false,
            "originator": {
                "originatorPersons": [{
                    "naturalPerson": {
                        "name": [{
                            "nameIdentifier": [{
                                "primaryIdentifier": payload.originator_name.as_deref().unwrap_or("Unknown"),
                                "nameIdentifierType": "LEGL"
                            }]
                        }]
                    }
                }],
                "accountNumber": [payload.originator_account.as_deref().unwrap_or("")]
            },
            "beneficiary": {
                "accountNumber": [payload.beneficiary_account.as_deref().unwrap_or("")]
            }
        });

        let client = reqwest::blocking::Client::new();
        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .json(&body)
            .send();

        match response {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    let resp_body: serde_json::Value = resp.json().unwrap_or_default();
                    let provider_ref = resp_body["id"]
                        .as_str()
                        .unwrap_or("unknown")
                        .to_string();

                    tracing::info!(
                        transfer_id = %payload.transfer_id,
                        provider_ref = %provider_ref,
                        "Notabene: travel rule data transmitted successfully"
                    );

                    TransmissionResult::Success { provider_ref }
                } else {
                    let error_body = resp.text().unwrap_or_default();
                    tracing::error!(
                        transfer_id = %payload.transfer_id,
                        status = %status,
                        body = %error_body,
                        "Notabene: API returned error"
                    );

                    if status.as_u16() == 404 {
                        TransmissionResult::VaspNotFound
                    } else {
                        TransmissionResult::Error {
                            message: format!("Notabene API {status}: {error_body}"),
                        }
                    }
                }
            }
            Err(e) => {
                tracing::error!(
                    transfer_id = %payload.transfer_id,
                    error = %e,
                    "Notabene: HTTP request failed"
                );
                TransmissionResult::Error {
                    message: format!("HTTP request failed: {e}"),
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TravelRuleEngine: transmit pending records
// ---------------------------------------------------------------------------

impl TravelRuleEngine {
    /// Attempt to transmit all pending outgoing records via the provider.
    ///
    /// Returns the number of successfully transmitted records.
    pub fn transmit_pending(
        &mut self,
        provider: &dyn TravelRuleProvider,
        vasp_directory: &VaspDirectory,
    ) -> usize {
        let pending_ids: Vec<Uuid> = self
            .pending_transmissions()
            .iter()
            .map(|r| r.transfer_id)
            .collect();

        let mut transmitted = 0;
        for id in pending_ids {
            let record = match self.get(id) {
                Some(r) => r,
                None => continue,
            };

            let payload = TransmissionPayload::from(record);
            let beneficiary_vasp = record
                .beneficiary_account
                .as_deref()
                .and_then(|addr| vasp_directory.find_by_address(addr));

            match provider.transmit(&payload, beneficiary_vasp) {
                TransmissionResult::Success { provider_ref } => {
                    tracing::info!(
                        transfer_id = %id,
                        provider_ref = %provider_ref,
                        "Travel Rule record transmitted successfully"
                    );
                    self.update_status(id, TravelRuleStatus::Transmitted);
                    transmitted += 1;
                }
                TransmissionResult::VaspNotFound => {
                    tracing::warn!(transfer_id = %id, "Travel Rule: counterparty VASP not found — will retry");
                }
                TransmissionResult::Error { message } => {
                    tracing::error!(transfer_id = %id, error = %message, "Travel Rule transmission failed");
                    self.update_status(id, TravelRuleStatus::Failed);
                }
            }
        }

        transmitted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    // --- Threshold checks ---

    #[test]
    fn test_below_threshold_not_required() {
        let engine = TravelRuleEngine::new();
        assert!(!engine.check_required(dec!(999.99)));
    }

    #[test]
    fn test_above_threshold_required() {
        let engine = TravelRuleEngine::new();
        assert!(engine.check_required(dec!(1001)));
    }

    #[test]
    fn test_exact_threshold_is_required() {
        let engine = TravelRuleEngine::new();
        assert!(engine.check_required(dec!(1000)));
    }

    #[test]
    fn test_zero_amount_not_required() {
        let engine = TravelRuleEngine::new();
        assert!(!engine.check_required(dec!(0)));
    }

    #[test]
    fn test_custom_threshold() {
        let engine = TravelRuleEngine::with_threshold(dec!(3000));
        assert!(!engine.check_required(dec!(2999)));
        assert!(engine.check_required(dec!(3000)));
    }

    // --- Outgoing record creation ---

    #[test]
    fn test_create_outgoing_above_threshold() {
        let mut engine = TravelRuleEngine::new();
        let id = Uuid::new_v4();

        let record = engine.create_outgoing(
            id,
            dec!(5000),
            "BTC",
            "Alice Smith",
            "bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh",
        );

        assert_eq!(record.transfer_id, id);
        assert_eq!(record.direction, TransferDirection::Outgoing);
        assert_eq!(record.amount_usd, dec!(5000));
        assert_eq!(record.asset, "BTC");
        assert_eq!(record.originator_name.as_deref(), Some("Alice Smith"));
        assert_eq!(
            record.beneficiary_account.as_deref(),
            Some("bc1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh")
        );
        assert_eq!(record.status, TravelRuleStatus::DataCollected);
    }

    #[test]
    fn test_create_outgoing_below_threshold_is_not_required() {
        let mut engine = TravelRuleEngine::new();

        let record = engine.create_outgoing(
            Uuid::new_v4(),
            dec!(50),
            "USDT",
            "Bob",
            "0xabc",
        );

        assert_eq!(record.status, TravelRuleStatus::NotRequired);
    }

    // --- Incoming record creation ---

    #[test]
    fn test_create_incoming_above_threshold() {
        let mut engine = TravelRuleEngine::new();
        let id = Uuid::new_v4();

        let record = engine.create_incoming(
            id,
            dec!(2500),
            "ETH",
            "0xdeadbeef",
        );

        assert_eq!(record.transfer_id, id);
        assert_eq!(record.direction, TransferDirection::Incoming);
        assert_eq!(record.amount_usd, dec!(2500));
        assert_eq!(record.asset, "ETH");
        assert_eq!(record.originator_account.as_deref(), Some("0xdeadbeef"));
        assert!(record.beneficiary_account.is_none()); // We are the beneficiary VASP
        assert_eq!(record.status, TravelRuleStatus::DataCollected);
    }

    #[test]
    fn test_create_incoming_below_threshold_is_not_required() {
        let mut engine = TravelRuleEngine::new();

        let record = engine.create_incoming(Uuid::new_v4(), dec!(10), "ETH", "0xabc");

        assert_eq!(record.status, TravelRuleStatus::NotRequired);
    }

    // --- Status transitions ---

    #[test]
    fn test_update_status_transmitted() {
        let mut engine = TravelRuleEngine::new();
        let id = Uuid::new_v4();
        engine.create_outgoing(id, dec!(2000), "BTC", "Carol", "bc1q...");

        let updated = engine.update_status(id, TravelRuleStatus::Transmitted);

        assert!(updated);
        assert_eq!(engine.get(id).unwrap().status, TravelRuleStatus::Transmitted);
    }

    #[test]
    fn test_update_status_failed() {
        let mut engine = TravelRuleEngine::new();
        let id = Uuid::new_v4();
        engine.create_outgoing(id, dec!(1500), "ETH", "Dave", "0x1234");

        engine.update_status(id, TravelRuleStatus::Failed);

        assert_eq!(engine.get(id).unwrap().status, TravelRuleStatus::Failed);
    }

    #[test]
    fn test_update_status_received_for_incoming() {
        let mut engine = TravelRuleEngine::new();
        let id = Uuid::new_v4();
        engine.create_incoming(id, dec!(1200), "USDT", "0xsender");

        engine.update_status(id, TravelRuleStatus::Received);

        assert_eq!(engine.get(id).unwrap().status, TravelRuleStatus::Received);
    }

    #[test]
    fn test_update_status_nonexistent_returns_false() {
        let mut engine = TravelRuleEngine::new();
        let result = engine.update_status(Uuid::new_v4(), TravelRuleStatus::Transmitted);
        assert!(!result);
    }

    // --- Pending transmissions ---

    #[test]
    fn test_pending_transmissions_only_outgoing_data_collected() {
        let mut engine = TravelRuleEngine::new();

        // Should appear in pending
        let outgoing_id = Uuid::new_v4();
        engine.create_outgoing(outgoing_id, dec!(2000), "BTC", "Eve", "bc1q...");

        // Should NOT appear — incoming
        engine.create_incoming(Uuid::new_v4(), dec!(3000), "ETH", "0xabc");

        // Should NOT appear — already transmitted
        let transmitted_id = Uuid::new_v4();
        engine.create_outgoing(transmitted_id, dec!(1500), "USDT", "Frank", "0xdef");
        engine.update_status(transmitted_id, TravelRuleStatus::Transmitted);

        // Should NOT appear — below threshold (NotRequired)
        engine.create_outgoing(Uuid::new_v4(), dec!(50), "BTC", "Grace", "bc1q...");

        let pending = engine.pending_transmissions();

        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].transfer_id, outgoing_id);
    }

    #[test]
    fn test_pending_transmissions_empty_when_all_transmitted() {
        let mut engine = TravelRuleEngine::new();
        let id = Uuid::new_v4();
        engine.create_outgoing(id, dec!(5000), "BTC", "Henry", "bc1q...");
        engine.update_status(id, TravelRuleStatus::Transmitted);

        assert!(engine.pending_transmissions().is_empty());
    }

    // --- Records for user ---

    #[test]
    fn test_records_for_user_returns_matching_records() {
        let mut engine = TravelRuleEngine::new();

        engine.create_outgoing(Uuid::new_v4(), dec!(2000), "BTC", "Alice Smith", "bc1q...");
        engine.create_outgoing(Uuid::new_v4(), dec!(3000), "ETH", "Alice Smith", "0xabc");
        engine.create_outgoing(Uuid::new_v4(), dec!(1500), "USDT", "Bob Jones", "0xdef");

        let alice_records = engine.records_for_user("Alice Smith");

        assert_eq!(alice_records.len(), 2);
        assert!(alice_records
            .iter()
            .all(|r| r.originator_name.as_deref() == Some("Alice Smith")));
    }

    #[test]
    fn test_records_for_user_empty_when_not_found() {
        let mut engine = TravelRuleEngine::new();
        engine.create_outgoing(Uuid::new_v4(), dec!(2000), "BTC", "Carol", "bc1q...");

        let records = engine.records_for_user("Nobody");
        assert!(records.is_empty());
    }

    // --- Lookup ---

    #[test]
    fn test_get_existing_record() {
        let mut engine = TravelRuleEngine::new();
        let id = Uuid::new_v4();
        engine.create_outgoing(id, dec!(1000), "BTC", "Ivan", "bc1q...");

        let record = engine.get(id);
        assert!(record.is_some());
        assert_eq!(record.unwrap().transfer_id, id);
    }

    #[test]
    fn test_get_nonexistent_record_returns_none() {
        let engine = TravelRuleEngine::new();
        assert!(engine.get(Uuid::new_v4()).is_none());
    }

    // --- Record storage integrity ---

    #[test]
    fn test_records_stored_on_creation() {
        let mut engine = TravelRuleEngine::new();

        engine.create_outgoing(Uuid::new_v4(), dec!(2000), "BTC", "Judy", "bc1q...");
        engine.create_incoming(Uuid::new_v4(), dec!(500), "ETH", "0xabc");

        assert_eq!(engine.total_records(), 2);
    }

    // --- Provider + transmission ---

    #[test]
    fn test_mock_provider_transmits_successfully() {
        let provider = MockTravelRuleProvider;
        let payload = TransmissionPayload {
            transfer_id: Uuid::new_v4().to_string(),
            direction: TransferDirection::Outgoing,
            asset: "BTC".to_string(),
            amount_usd: dec!(5000),
            originator_name: Some("Alice".to_string()),
            originator_account: None,
            originator_institution: None,
            beneficiary_name: None,
            beneficiary_account: Some("bc1q...".to_string()),
            beneficiary_institution: None,
        };

        let result = provider.transmit(&payload, None);
        assert!(matches!(result, TransmissionResult::Success { .. }));
    }

    #[test]
    fn test_transmit_pending_uses_provider() {
        let mut engine = TravelRuleEngine::new();
        let provider = MockTravelRuleProvider;
        let directory = VaspDirectory::new();

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        engine.create_outgoing(id1, dec!(2000), "BTC", "Alice", "bc1q...");
        engine.create_outgoing(id2, dec!(3000), "ETH", "Bob", "0xabc");

        // Both should be pending.
        assert_eq!(engine.pending_transmissions().len(), 2);

        // Transmit.
        let transmitted = engine.transmit_pending(&provider, &directory);
        assert_eq!(transmitted, 2);

        // Both should now be Transmitted.
        assert_eq!(engine.get(id1).unwrap().status, TravelRuleStatus::Transmitted);
        assert_eq!(engine.get(id2).unwrap().status, TravelRuleStatus::Transmitted);

        // No more pending.
        assert!(engine.pending_transmissions().is_empty());
    }

    #[test]
    fn test_transmit_pending_skips_below_threshold() {
        let mut engine = TravelRuleEngine::new();
        let provider = MockTravelRuleProvider;
        let directory = VaspDirectory::new();

        engine.create_outgoing(Uuid::new_v4(), dec!(50), "BTC", "Charlie", "bc1q...");

        // Below threshold → NotRequired → not pending.
        let transmitted = engine.transmit_pending(&provider, &directory);
        assert_eq!(transmitted, 0);
    }

    #[test]
    fn test_transmission_payload_from_travel_rule_data() {
        let mut engine = TravelRuleEngine::new();
        let id = Uuid::new_v4();
        engine.create_outgoing(id, dec!(1500), "USDT", "Dave", "0xdef");

        let record = engine.get(id).unwrap();
        let payload = TransmissionPayload::from(record);

        assert_eq!(payload.transfer_id, id.to_string());
        assert_eq!(payload.asset, "USDT");
        assert_eq!(payload.originator_name.as_deref(), Some("Dave"));
        assert_eq!(payload.beneficiary_account.as_deref(), Some("0xdef"));
    }
}
