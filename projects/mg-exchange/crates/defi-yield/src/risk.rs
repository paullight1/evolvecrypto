//! Protocol risk assessment.
//!
//! The `ProtocolRiskAssessor` takes static protocol metadata and produces a
//! `RiskAssessment` with a composite score, a recommended maximum allocation
//! percentage, and human-readable warnings.
//!
//! The scoring is intentionally simple for Phase 1. Phase 2 can add dynamic
//! inputs such as real-time TVL changes, recent exploit history, and oracle
//! deviation alerts.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use crate::types::{AuditStatus, DeFiProtocolInfo};

/// Result of a risk assessment for a single DeFi protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAssessment {
    /// Composite risk score: 1 (safest) – 10 (highest risk).
    pub score: u8,
    /// Recommended maximum allocation as a fraction of total portfolio.
    /// E.g. `0.30` means "allocate at most 30% of available capital here".
    pub max_allocation_pct: Decimal,
    /// Human-readable warnings for operators / users.
    pub warnings: Vec<String>,
}

/// Stateless risk assessor.
pub struct ProtocolRiskAssessor;

impl ProtocolRiskAssessor {
    /// Calculate a `RiskAssessment` from static protocol metadata.
    ///
    /// # Scoring rules
    /// - Score is taken directly from `protocol.risk_score` (set per-protocol).
    /// - TVL < $100 M adds +1 (lower liquidity = higher exit risk).
    /// - Unaudited adds +3 and a warning.
    /// - No insurance adds a warning (not a score change).
    /// - Score is clamped to [1, 10].
    ///
    /// # Max allocation
    /// | Score | Cap  |
    /// |-------|------|
    /// | 1–2   | 40%  |
    /// | 3–4   | 30%  |
    /// | 5–6   | 20%  |
    /// | 7–8   | 10%  |
    /// | 9–10  |  5%  |
    pub fn assess(protocol: &DeFiProtocolInfo) -> RiskAssessment {
        let mut score = protocol.risk_score;
        let mut warnings = Vec::new();

        // Low TVL increases exit / liquidity risk.
        if protocol.tvl_usd < dec!(100_000_000) {
            score = score.saturating_add(1);
            warnings.push(format!(
                "Low TVL (${:.0}M) — limited liquidity may cause slippage on large withdrawals",
                protocol.tvl_usd / dec!(1_000_000)
            ));
        }

        // Unaudited protocols carry materially higher smart-contract risk.
        match &protocol.audit_status {
            AuditStatus::Unaudited => {
                score = score.saturating_add(3);
                warnings.push("Protocol has not been audited — exercise extreme caution".into());
            }
            AuditStatus::Audited { firms, latest_date } => {
                // Warn if the latest audit is more than 18 months ago (rough heuristic).
                if let Some(year) = latest_date.split('-').next().and_then(|y| y.parse::<u32>().ok()) {
                    if year < 2023 {
                        warnings.push(format!(
                            "Audit by {} was in {} — consider whether significant code changes have occurred since",
                            firms.join(", "),
                            latest_date
                        ));
                    }
                }
            }
        }

        // Missing insurance coverage is notable for user communication.
        if !protocol.insurance_coverage {
            warnings.push(
                "Protocol is not covered by on-chain insurance (e.g. Nexus Mutual) — \
                 funds are at risk of permanent loss in case of exploit"
                    .into(),
            );
        }

        // Clamp to valid range.
        let score = score.clamp(1, 10);

        let max_allocation_pct = match score {
            1..=2 => dec!(0.40),
            3..=4 => dec!(0.30),
            5..=6 => dec!(0.20),
            7..=8 => dec!(0.10),
            _      => dec!(0.05),
        };

        RiskAssessment {
            score,
            max_allocation_pct,
            warnings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AuditStatus;
    use rust_decimal_macros::dec;

    fn audited_protocol(risk_score: u8, tvl: Decimal, insurance: bool) -> DeFiProtocolInfo {
        DeFiProtocolInfo {
            name: "Test".into(),
            chain: "ethereum".into(),
            tvl_usd: tvl,
            audit_status: AuditStatus::Audited {
                firms: vec!["OpenZeppelin".into()],
                latest_date: "2024-06".into(),
            },
            risk_score,
            insurance_coverage: insurance,
        }
    }

    #[test]
    fn low_risk_protocol_high_cap() {
        let info = audited_protocol(2, dec!(10_000_000_000), true);
        let a = ProtocolRiskAssessor::assess(&info);
        assert_eq!(a.score, 2);
        assert_eq!(a.max_allocation_pct, dec!(0.40));
        assert!(a.warnings.is_empty(), "expected no warnings, got: {:?}", a.warnings);
    }

    #[test]
    fn unaudited_protocol_score_bumped() {
        let mut info = audited_protocol(3, dec!(500_000_000), false);
        info.audit_status = AuditStatus::Unaudited;
        let a = ProtocolRiskAssessor::assess(&info);
        // 3 base + 3 unaudited = 6
        assert_eq!(a.score, 6);
        assert!(a.warnings.iter().any(|w| w.contains("not been audited")));
    }

    #[test]
    fn low_tvl_bumps_score() {
        let info = audited_protocol(4, dec!(50_000_000), false);
        let a = ProtocolRiskAssessor::assess(&info);
        // 4 + 1 (low TVL) = 5
        assert_eq!(a.score, 5);
        assert!(a.warnings.iter().any(|w| w.contains("Low TVL")));
    }

    #[test]
    fn no_insurance_warning() {
        let info = audited_protocol(2, dec!(5_000_000_000), false);
        let a = ProtocolRiskAssessor::assess(&info);
        assert!(a.warnings.iter().any(|w| w.contains("not covered")));
    }

    #[test]
    fn score_clamped_to_ten() {
        let mut info = audited_protocol(9, dec!(10_000_000), false);
        info.audit_status = AuditStatus::Unaudited;
        let a = ProtocolRiskAssessor::assess(&info);
        // 9 + 3 unaudited + 1 low TVL = 13, clamped to 10
        assert_eq!(a.score, 10);
        assert_eq!(a.max_allocation_pct, dec!(0.05));
    }

    #[test]
    fn stale_audit_warning() {
        let info = DeFiProtocolInfo {
            name: "Old Protocol".into(),
            chain: "ethereum".into(),
            tvl_usd: dec!(1_000_000_000),
            audit_status: AuditStatus::Audited {
                firms: vec!["SomeFirm".into()],
                latest_date: "2021-03".into(),
            },
            risk_score: 3,
            insurance_coverage: true,
        };
        let a = ProtocolRiskAssessor::assess(&info);
        assert!(a.warnings.iter().any(|w| w.contains("2021-03")));
    }

    #[test]
    fn eigenlayer_profile() {
        // EigenLayer-style: score 6, no insurance, large TVL, audited.
        let info = audited_protocol(6, dec!(15_000_000_000), false);
        let a = ProtocolRiskAssessor::assess(&info);
        assert_eq!(a.score, 6);
        assert_eq!(a.max_allocation_pct, dec!(0.20));
    }
}
