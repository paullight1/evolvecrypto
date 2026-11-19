use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A regulatory report summarising exchange activity over a period.
///
/// Covers KYC throughput, AML alerts, suspicious transaction reports (STR),
/// sanctions hits, and aggregated volume — the data regulators require in
/// periodic filings (daily STR, monthly AML, quarterly).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegulatoryReport {
    pub report_type: String,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub generated_at: DateTime<Utc>,
    pub total_users: u64,
    pub new_kyc_applications: u64,
    pub kyc_approved: u64,
    pub kyc_rejected: u64,
    pub aml_alerts: u64,
    pub str_filed: u64,
    pub sanctions_hits: u64,
    pub total_deposits_usd: String,
    pub total_withdrawals_usd: String,
    pub total_trading_volume_usd: String,
}

/// Live compliance counts passed to [`RegulatoryReporter::generate_summary`].
///
/// All fields default to zero.  Callers fill in whichever counts they have
/// access to; fields left at zero are reported as zero.
#[derive(Debug, Clone, Default)]
pub struct ComplianceCounts {
    pub kyc_total: u64,
    pub kyc_pending: u64,
    pub kyc_approved: u64,
    pub kyc_rejected: u64,
    /// Total AML / surveillance alerts ever raised.
    pub aml_alerts_total: u64,
    /// Alerts that have been reviewed (approved or dismissed).
    pub aml_alerts_reviewed: u64,
    /// SAR / STR reports that have been filed with the regulator.
    pub sar_filed: u64,
    /// SAR / STR reports pending review or filing.
    pub sar_pending: u64,
    /// Number of sanctions name / address matches detected in the period.
    pub sanctions_matches: u64,
}

/// Generates regulatory reports for submission to financial authorities.
///
/// Accepts live compliance counts and formats them into a [`RegulatoryReport`].
pub struct RegulatoryReporter;

impl RegulatoryReporter {
    /// Generate a regulatory summary report for the given period.
    ///
    /// `report_type` examples: `"daily_str"`, `"monthly_aml"`, `"quarterly"`.
    ///
    /// Pass real counts via [`ComplianceCounts`].  Fields left at their default
    /// (zero) are reported as zero — no silent guessing.
    pub fn generate_summary(
        report_type: &str,
        period_start: DateTime<Utc>,
        period_end: DateTime<Utc>,
        counts: ComplianceCounts,
    ) -> RegulatoryReport {
        RegulatoryReport {
            report_type: report_type.into(),
            period_start,
            period_end,
            generated_at: Utc::now(),
            total_users: counts.kyc_total,
            new_kyc_applications: counts.kyc_pending,
            kyc_approved: counts.kyc_approved,
            kyc_rejected: counts.kyc_rejected,
            aml_alerts: counts.aml_alerts_total,
            str_filed: counts.sar_filed,
            sanctions_hits: counts.sanctions_matches,
            total_deposits_usd: "0".into(),
            total_withdrawals_usd: "0".into(),
            total_trading_volume_usd: "0".into(),
        }
    }

    /// Render a regulatory report as CSV for archiving or submission.
    pub fn to_csv(report: &RegulatoryReport) -> String {
        format!(
            "Report Type,Period Start,Period End,Generated,Users,New KYC,KYC Approved,KYC Rejected,AML Alerts,STR Filed,Sanctions Hits,Deposits USD,Withdrawals USD,Volume USD\n\
             {},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            report.report_type,
            report.period_start.to_rfc3339(),
            report.period_end.to_rfc3339(),
            report.generated_at.to_rfc3339(),
            report.total_users,
            report.new_kyc_applications,
            report.kyc_approved,
            report.kyc_rejected,
            report.aml_alerts,
            report.str_filed,
            report.sanctions_hits,
            report.total_deposits_usd,
            report.total_withdrawals_usd,
            report.total_trading_volume_usd,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn period() -> (DateTime<Utc>, DateTime<Utc>) {
        let start = Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 3, 31, 23, 59, 59).unwrap();
        (start, end)
    }

    #[test]
    fn test_generate_summary_sets_type() {
        let (start, end) = period();
        let report = RegulatoryReporter::generate_summary(
            "monthly_aml",
            start,
            end,
            ComplianceCounts::default(),
        );
        assert_eq!(report.report_type, "monthly_aml");
        assert_eq!(report.period_start, start);
        assert_eq!(report.period_end, end);
        assert_eq!(report.total_users, 0);
    }

    #[test]
    fn test_to_csv_has_header() {
        let (start, end) = period();
        let report = RegulatoryReporter::generate_summary(
            "quarterly",
            start,
            end,
            ComplianceCounts::default(),
        );
        let csv = RegulatoryReporter::to_csv(&report);
        assert!(csv.starts_with("Report Type,Period Start,Period End"));
        assert_eq!(csv.lines().count(), 2);
    }

    #[test]
    fn test_generate_summary_populates_counts() {
        let (start, end) = period();
        let counts = ComplianceCounts {
            kyc_total: 500,
            kyc_pending: 12,
            kyc_approved: 450,
            kyc_rejected: 38,
            aml_alerts_total: 7,
            aml_alerts_reviewed: 5,
            sar_filed: 2,
            sar_pending: 1,
            sanctions_matches: 3,
        };
        let report = RegulatoryReporter::generate_summary("daily_str", start, end, counts);
        assert_eq!(report.total_users, 500);
        assert_eq!(report.new_kyc_applications, 12);
        assert_eq!(report.kyc_approved, 450);
        assert_eq!(report.kyc_rejected, 38);
        assert_eq!(report.aml_alerts, 7);
        assert_eq!(report.str_filed, 2);
        assert_eq!(report.sanctions_hits, 3);
    }
}
