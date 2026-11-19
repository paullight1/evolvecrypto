use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Identifies whether an exchange account belongs to an individual or a
/// corporate entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccountType {
    Individual,
    Corporate,
}

/// A beneficial owner who ultimately controls ≥25 % of the corporate entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UltimateBeneficialOwner {
    pub name: String,
    pub nationality: String,
    /// Ownership percentage, e.g. `Decimal::new(3125, 2)` for 31.25 %.
    pub ownership_pct: Decimal,
    /// One of `"pending"`, `"verified"`, `"rejected"`.
    pub kyc_status: String,
    /// Whether a PEP check has been performed for this individual.
    pub pep_checked: bool,
}

/// Corporate KYC submission for a single company account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorporateKycRecord {
    pub id: Uuid,
    pub user_id: Uuid,
    pub company_name: String,
    pub registration_number: String,
    pub jurisdiction: String,
    pub directors: Vec<String>,
    pub ubos: Vec<UltimateBeneficialOwner>,
    /// One of `"pending"`, `"verified"`, `"rejected"`.
    pub status: String,
    pub created_at: DateTime<Utc>,
}

/// Corporate KYC manager.
///
/// Holds corporate entity submissions and their review status.  Approval
/// requires all UBOs to have a verified KYC status to prevent shell-company
/// evasion.
pub struct CorporateKycManager {
    records: Vec<CorporateKycRecord>,
}

impl CorporateKycManager {
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
        }
    }

    /// Submit a new corporate KYC application.
    pub fn submit(
        &mut self,
        user_id: Uuid,
        company: String,
        reg_number: String,
        jurisdiction: String,
        directors: Vec<String>,
        ubos: Vec<UltimateBeneficialOwner>,
    ) -> &CorporateKycRecord {
        let record = CorporateKycRecord {
            id: Uuid::new_v4(),
            user_id,
            company_name: company,
            registration_number: reg_number,
            jurisdiction,
            directors,
            ubos,
            status: "pending".into(),
            created_at: Utc::now(),
        };
        self.records.push(record);
        // SAFETY: we just pushed an element.
        self.records.last().expect("just pushed")
    }

    /// Retrieve the corporate KYC record for a given user, if one exists.
    pub fn get_by_user(&self, user_id: Uuid) -> Option<&CorporateKycRecord> {
        self.records.iter().find(|r| r.user_id == user_id)
    }

    /// Returns all records still awaiting review.
    pub fn pending(&self) -> Vec<&CorporateKycRecord> {
        self.records
            .iter()
            .filter(|r| r.status == "pending")
            .collect()
    }

    /// Approve a corporate KYC record.
    ///
    /// Returns an error if any UBO does not yet have a `"verified"` KYC
    /// status, preventing approval of incomplete applications.
    pub fn approve(&mut self, record_id: Uuid) -> Result<(), String> {
        let r = self
            .records
            .iter_mut()
            .find(|r| r.id == record_id)
            .ok_or("Not found")?;

        let all_verified = r.ubos.iter().all(|u| u.kyc_status == "verified");
        if !all_verified {
            return Err("Not all UBOs have verified KYC".into());
        }

        r.status = "verified".into();
        Ok(())
    }
}

impl Default for CorporateKycManager {
    fn default() -> Self {
        Self::new()
    }
}
