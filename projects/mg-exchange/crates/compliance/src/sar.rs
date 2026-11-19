use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SarStatus {
    Draft,
    PendingReview,
    Reviewed,
    Filed,
    Acknowledged,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SarReport {
    pub id: Uuid,
    pub user_id: Uuid,
    /// AML alert IDs that triggered this report.
    pub alert_ids: Vec<Uuid>,
    pub narrative: String,
    pub status: SarStatus,
    /// Target regulator, e.g. `"SFMS_UA"`, `"FinCEN"`, `"FCA"`.
    pub regulator: String,
    /// Reference number assigned by the regulator upon acceptance.
    pub reference_number: Option<String>,
    pub created_at: DateTime<Utc>,
    pub filed_at: Option<DateTime<Utc>>,
    pub reviewed_by: Option<String>,
}

/// Suspicious Activity Report (SAR / STR) manager.
///
/// Tracks the full lifecycle of regulatory filings: from draft generation
/// through internal review, submission to the regulator, and acknowledgement.
pub struct SarManager {
    reports: Vec<SarReport>,
}

impl SarManager {
    pub fn new() -> Self {
        Self {
            reports: Vec::new(),
        }
    }

    /// Generate a new SAR in `Draft` status.
    pub fn generate(
        &mut self,
        user_id: Uuid,
        alert_ids: Vec<Uuid>,
        narrative: String,
        regulator: &str,
    ) -> &SarReport {
        let report = SarReport {
            id: Uuid::new_v4(),
            user_id,
            alert_ids,
            narrative,
            status: SarStatus::Draft,
            regulator: regulator.into(),
            reference_number: None,
            created_at: Utc::now(),
            filed_at: None,
            reviewed_by: None,
        };
        self.reports.push(report);
        // SAFETY: we just pushed an element.
        self.reports.last().expect("just pushed")
    }

    /// Mark a SAR as reviewed by a named compliance officer.
    pub fn review(&mut self, sar_id: Uuid, reviewer: &str) -> Result<(), String> {
        let r = self
            .reports
            .iter_mut()
            .find(|r| r.id == sar_id)
            .ok_or("SAR not found")?;
        r.status = SarStatus::Reviewed;
        r.reviewed_by = Some(reviewer.into());
        Ok(())
    }

    /// Record that the SAR has been filed with the regulator and store the
    /// reference number returned.
    pub fn mark_filed(&mut self, sar_id: Uuid, reference: &str) -> Result<(), String> {
        let r = self
            .reports
            .iter_mut()
            .find(|r| r.id == sar_id)
            .ok_or("SAR not found")?;
        r.status = SarStatus::Filed;
        r.filed_at = Some(Utc::now());
        r.reference_number = Some(reference.into());
        Ok(())
    }

    /// Returns all reports that are not yet filed or acknowledged.
    pub fn pending(&self) -> Vec<&SarReport> {
        self.reports
            .iter()
            .filter(|r| !matches!(r.status, SarStatus::Filed | SarStatus::Acknowledged))
            .collect()
    }

    pub fn all_reports(&self) -> &[SarReport] {
        &self.reports
    }

    /// Export all reports as a CSV string suitable for regulatory submission.
    pub fn export_csv(&self) -> String {
        let mut csv =
            "id,user_id,status,regulator,narrative,created_at,filed_at\n".to_string();
        for r in &self.reports {
            csv.push_str(&format!(
                "{},{},{:?},{},{},{},{}\n",
                r.id,
                r.user_id,
                r.status,
                r.regulator,
                r.narrative.replace(',', ";"),
                r.created_at.to_rfc3339(),
                r.filed_at
                    .map(|t| t.to_rfc3339())
                    .unwrap_or_default(),
            ));
        }
        csv
    }
}

impl Default for SarManager {
    fn default() -> Self {
        Self::new()
    }
}
