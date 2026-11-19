//! Tests for previously untested compliance modules.

use uuid::Uuid;

// ── Risk Warnings ────────────────────────────────────────────────────────────

mod risk_warnings_tests {
    use super::*;
    use exchange_compliance::risk_warnings::*;

    #[test]
    fn new_user_has_pending_warnings() {
        let mgr = RiskWarningManager::new();
        assert!(mgr.pending_warnings(Uuid::new_v4()).len() >= 3);
    }

    #[test]
    fn acknowledge_reduces_pending() {
        let mut mgr = RiskWarningManager::new();
        let user = Uuid::new_v4();
        let before = mgr.pending_warnings(user).len();
        mgr.acknowledge(user, WarningType::FuturesTrading);
        assert_eq!(mgr.pending_warnings(user).len(), before - 1);
    }

    #[test]
    fn cannot_trade_futures_without_ack() {
        let mgr = RiskWarningManager::new();
        assert!(!mgr.can_trade_futures(Uuid::new_v4()));
    }

    #[test]
    fn can_trade_futures_after_both_acks() {
        let mut mgr = RiskWarningManager::new();
        let user = Uuid::new_v4();
        mgr.acknowledge(user, WarningType::FuturesTrading);
        mgr.acknowledge(user, WarningType::LiquidationRisk);
        assert!(mgr.can_trade_futures(user));
    }
}

// ── Risk Scoring ─────────────────────────────────────────────────────────────

mod risk_scoring_tests {
    use super::*;
    use exchange_compliance::risk_scoring::*;

    #[test]
    fn low_risk_new_account() {
        let mut engine = RiskScoringEngine::new();
        let score = engine.assess(Uuid::new_v4(), vec![RiskFactor::NewAccount]);
        assert_eq!(score.level, RiskLevel::Low);
    }

    #[test]
    fn high_risk_combined() {
        let mut engine = RiskScoringEngine::new();
        let score = engine.assess(Uuid::new_v4(), vec![
            RiskFactor::PepMatch,
            RiskFactor::HighRiskJurisdiction("IR".to_string()),
        ]);
        assert!(matches!(score.level, RiskLevel::High | RiskLevel::Critical));
    }

    #[test]
    fn score_persisted() {
        let mut engine = RiskScoringEngine::new();
        let user = Uuid::new_v4();
        engine.assess(user, vec![RiskFactor::NewAccount]);
        assert!(engine.get_score(user).is_some());
    }

    #[test]
    fn high_risk_users_filter() {
        let mut engine = RiskScoringEngine::new();
        let risky = Uuid::new_v4();
        let safe = Uuid::new_v4();
        engine.assess(risky, vec![RiskFactor::PepMatch, RiskFactor::SanctionsNearMatch, RiskFactor::HighRiskJurisdiction("IR".to_string())]);
        engine.assess(safe, vec![RiskFactor::NewAccount]);
        let high = engine.high_risk_users();
        assert!(high.iter().any(|s| s.user_id == risky));
        assert!(!high.iter().any(|s| s.user_id == safe));
    }
}

// ── EDD ──────────────────────────────────────────────────────────────────────

mod edd_tests {
    use super::*;
    use exchange_compliance::edd::*;

    #[test]
    fn open_and_transition() {
        let mut mgr = EddManager::new();
        let case_id = mgr.open_case(Uuid::new_v4(), "PEP match").id;
        assert!(mgr.update_status(case_id, EddStatus::InProgress).is_ok());
        assert!(mgr.update_status(case_id, EddStatus::Approved).is_ok());
    }

    #[test]
    fn add_note_audit_trail() {
        let mut mgr = EddManager::new();
        let case_id = mgr.open_case(Uuid::new_v4(), "test").id;
        assert!(mgr.add_note(case_id, "Reviewed").is_ok());
    }

    #[test]
    fn has_pending_edd() {
        let mut mgr = EddManager::new();
        let user = Uuid::new_v4();
        assert!(!mgr.has_pending_edd(user));
        mgr.open_case(user, "trigger");
        assert!(mgr.has_pending_edd(user));
    }
}

// ── SAR ──────────────────────────────────────────────────────────────────────

mod sar_tests {
    use super::*;
    use exchange_compliance::sar::*;

    #[test]
    fn generate_draft() {
        let mut mgr = SarManager::new();
        let sar = mgr.generate(
            Uuid::new_v4(),
            vec![],
            "Suspicious pattern".to_string(),
            "SFMS_UA",
        );
        assert_eq!(sar.status, SarStatus::Draft);
    }

    #[test]
    fn review_and_file() {
        let mut mgr = SarManager::new();
        let id = mgr.generate(Uuid::new_v4(), vec![], "narrative".to_string(), "SFMS_UA").id;
        assert!(mgr.review(id, "officer").is_ok());
        assert!(mgr.mark_filed(id, "REF-001").is_ok());
    }

    #[test]
    fn export_csv_not_empty() {
        let mut mgr = SarManager::new();
        mgr.generate(Uuid::new_v4(), vec![], "test".to_string(), "SFMS_UA");
        let csv = mgr.export_csv();
        assert!(!csv.is_empty());
    }
}

// ── Complaints ───────────────────────────────────────────────────────────────

mod complaints_tests {
    use super::*;
    use exchange_compliance::complaints::*;

    #[test]
    fn submit_creates_open() {
        let mut mgr = ComplaintManager::new();
        let c = mgr.submit(
            Uuid::new_v4(),
            ComplaintCategory::Withdrawal,
            "Delayed".to_string(),
            "Been 3 days".to_string(),
        );
        assert_eq!(c.status, ComplaintStatus::Open);
    }

    #[test]
    fn acknowledge_sets_status() {
        let mut mgr = ComplaintManager::new();
        let id = mgr.submit(
            Uuid::new_v4(),
            ComplaintCategory::Technical,
            "Bug".to_string(),
            "Details".to_string(),
        ).id;
        assert!(mgr.acknowledge(id, "agent_1").is_ok());
    }

    #[test]
    fn user_complaints_filtered() {
        let mut mgr = ComplaintManager::new();
        let user = Uuid::new_v4();
        mgr.submit(user, ComplaintCategory::Fees, "Fee issue".to_string(), "d".to_string());
        mgr.submit(Uuid::new_v4(), ComplaintCategory::Other, "Other".to_string(), "d".to_string());
        assert_eq!(mgr.user_complaints(user).len(), 1);
    }
}
