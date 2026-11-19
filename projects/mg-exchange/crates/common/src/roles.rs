use serde::{Deserialize, Serialize};

/// Admin roles with hierarchical permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AdminRole {
    SuperAdmin,
    Admin,
    RiskManager,
    ComplianceOfficer,
    SupportAgent,
    Finance,
    Surveillance,
    Marketing,
}

/// Fine-grained admin permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdminPermission {
    // User management
    UsersRead,
    UsersFreeze,
    UsersUnfreeze,
    UsersAdjustBalance,
    UsersForceClose,

    // Trading
    TradingHalt,
    TradingResume,

    // Pair management
    PairsCreate,
    PairsConfigure,
    PairsDisable,

    // Compliance
    KycReview,
    KycApprove,
    KycReject,
    AmlReview,
    StrGenerate,

    // Finance
    WalletsView,
    Reconcile,
    FeeReports,
    PorGenerate,

    // Surveillance
    SurveillanceView,
    SurveillanceReview,

    // System
    FeatureFlags,
    RateLimits,
    SystemConfig,
    AuditLogView,
    AuditLogExport,

    // Stats (all admin roles can view basic stats)
    StatsView,
}

impl AdminRole {
    /// Get all permissions for this role.
    pub fn permissions(&self) -> Vec<AdminPermission> {
        use AdminPermission::*;
        match self {
            AdminRole::SuperAdmin => vec![
                UsersRead,
                UsersFreeze,
                UsersUnfreeze,
                UsersAdjustBalance,
                UsersForceClose,
                TradingHalt,
                TradingResume,
                PairsCreate,
                PairsConfigure,
                PairsDisable,
                KycReview,
                KycApprove,
                KycReject,
                AmlReview,
                StrGenerate,
                WalletsView,
                Reconcile,
                FeeReports,
                PorGenerate,
                SurveillanceView,
                SurveillanceReview,
                FeatureFlags,
                RateLimits,
                SystemConfig,
                AuditLogView,
                AuditLogExport,
                StatsView,
            ],
            AdminRole::Admin => vec![
                UsersRead,
                PairsCreate,
                PairsConfigure,
                PairsDisable,
                FeatureFlags,
                RateLimits,
                SystemConfig,
                AuditLogView,
                StatsView,
            ],
            AdminRole::RiskManager => vec![
                UsersRead,
                TradingHalt,
                TradingResume,
                UsersForceClose,
                PairsConfigure,
                StatsView,
                AuditLogView,
            ],
            AdminRole::ComplianceOfficer => vec![
                UsersRead,
                UsersFreeze,
                UsersUnfreeze,
                KycReview,
                KycApprove,
                KycReject,
                AmlReview,
                StrGenerate,
                AuditLogView,
                AuditLogExport,
                StatsView,
            ],
            AdminRole::SupportAgent => vec![UsersRead, StatsView],
            AdminRole::Finance => vec![
                UsersRead,
                WalletsView,
                Reconcile,
                FeeReports,
                PorGenerate,
                AuditLogView,
                StatsView,
            ],
            AdminRole::Surveillance => vec![
                UsersRead,
                SurveillanceView,
                SurveillanceReview,
                AuditLogView,
                StatsView,
            ],
            AdminRole::Marketing => vec![StatsView],
        }
    }

    /// Check if this role has a specific permission.
    pub fn has_permission(&self, perm: AdminPermission) -> bool {
        self.permissions().contains(&perm)
    }

    /// Parse from a string. Returns `None` for unrecognised values.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "super_admin" | "superadmin" => Some(Self::SuperAdmin),
            "admin" => Some(Self::Admin),
            "risk_manager" | "risk" => Some(Self::RiskManager),
            "compliance_officer" | "compliance" => Some(Self::ComplianceOfficer),
            "support_agent" | "support" => Some(Self::SupportAgent),
            "finance" => Some(Self::Finance),
            "surveillance" => Some(Self::Surveillance),
            "marketing" => Some(Self::Marketing),
            _ => None,
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn super_admin_has_all_permissions() {
        // Verify every variant in AdminPermission is held by SuperAdmin.
        let all = [
            AdminPermission::UsersRead,
            AdminPermission::UsersFreeze,
            AdminPermission::UsersUnfreeze,
            AdminPermission::UsersAdjustBalance,
            AdminPermission::UsersForceClose,
            AdminPermission::TradingHalt,
            AdminPermission::TradingResume,
            AdminPermission::PairsCreate,
            AdminPermission::PairsConfigure,
            AdminPermission::PairsDisable,
            AdminPermission::KycReview,
            AdminPermission::KycApprove,
            AdminPermission::KycReject,
            AdminPermission::AmlReview,
            AdminPermission::StrGenerate,
            AdminPermission::WalletsView,
            AdminPermission::Reconcile,
            AdminPermission::FeeReports,
            AdminPermission::PorGenerate,
            AdminPermission::SurveillanceView,
            AdminPermission::SurveillanceReview,
            AdminPermission::FeatureFlags,
            AdminPermission::RateLimits,
            AdminPermission::SystemConfig,
            AdminPermission::AuditLogView,
            AdminPermission::AuditLogExport,
            AdminPermission::StatsView,
        ];
        for perm in all {
            assert!(
                AdminRole::SuperAdmin.has_permission(perm),
                "SuperAdmin is missing permission {:?}",
                perm
            );
        }
    }

    #[test]
    fn support_agent_has_only_read_and_stats() {
        assert!(AdminRole::SupportAgent.has_permission(AdminPermission::UsersRead));
        assert!(AdminRole::SupportAgent.has_permission(AdminPermission::StatsView));

        // Must not have any elevated permission.
        assert!(!AdminRole::SupportAgent.has_permission(AdminPermission::UsersFreeze));
        assert!(!AdminRole::SupportAgent.has_permission(AdminPermission::TradingHalt));
        assert!(!AdminRole::SupportAgent.has_permission(AdminPermission::KycApprove));
        assert!(!AdminRole::SupportAgent.has_permission(AdminPermission::WalletsView));
        assert!(!AdminRole::SupportAgent.has_permission(AdminPermission::FeatureFlags));
        assert!(!AdminRole::SupportAgent.has_permission(AdminPermission::AuditLogView));
        assert!(!AdminRole::SupportAgent.has_permission(AdminPermission::AuditLogExport));
        assert_eq!(AdminRole::SupportAgent.permissions().len(), 2);
    }

    #[test]
    fn marketing_has_only_stats_view() {
        assert!(AdminRole::Marketing.has_permission(AdminPermission::StatsView));
        assert_eq!(AdminRole::Marketing.permissions().len(), 1);

        assert!(!AdminRole::Marketing.has_permission(AdminPermission::UsersRead));
        assert!(!AdminRole::Marketing.has_permission(AdminPermission::AuditLogView));
    }

    #[test]
    fn compliance_officer_can_freeze_but_not_adjust_balance() {
        assert!(AdminRole::ComplianceOfficer.has_permission(AdminPermission::UsersFreeze));
        assert!(AdminRole::ComplianceOfficer.has_permission(AdminPermission::UsersUnfreeze));

        assert!(!AdminRole::ComplianceOfficer.has_permission(AdminPermission::UsersAdjustBalance));
        assert!(!AdminRole::ComplianceOfficer.has_permission(AdminPermission::UsersForceClose));
        assert!(!AdminRole::ComplianceOfficer.has_permission(AdminPermission::TradingHalt));
        assert!(!AdminRole::ComplianceOfficer.has_permission(AdminPermission::WalletsView));
        assert!(!AdminRole::ComplianceOfficer.has_permission(AdminPermission::FeatureFlags));
    }

    #[test]
    fn role_parsing_from_string() {
        assert_eq!(AdminRole::from_str("super_admin"), Some(AdminRole::SuperAdmin));
        assert_eq!(AdminRole::from_str("superadmin"), Some(AdminRole::SuperAdmin));
        assert_eq!(AdminRole::from_str("SUPER_ADMIN"), Some(AdminRole::SuperAdmin));

        assert_eq!(AdminRole::from_str("admin"), Some(AdminRole::Admin));
        assert_eq!(AdminRole::from_str("ADMIN"), Some(AdminRole::Admin));

        assert_eq!(AdminRole::from_str("risk_manager"), Some(AdminRole::RiskManager));
        assert_eq!(AdminRole::from_str("risk"), Some(AdminRole::RiskManager));

        assert_eq!(AdminRole::from_str("compliance_officer"), Some(AdminRole::ComplianceOfficer));
        assert_eq!(AdminRole::from_str("compliance"), Some(AdminRole::ComplianceOfficer));

        assert_eq!(AdminRole::from_str("support_agent"), Some(AdminRole::SupportAgent));
        assert_eq!(AdminRole::from_str("support"), Some(AdminRole::SupportAgent));

        assert_eq!(AdminRole::from_str("finance"), Some(AdminRole::Finance));
        assert_eq!(AdminRole::from_str("surveillance"), Some(AdminRole::Surveillance));
        assert_eq!(AdminRole::from_str("marketing"), Some(AdminRole::Marketing));

        assert_eq!(AdminRole::from_str("unknown"), None);
        assert_eq!(AdminRole::from_str(""), None);
    }

    #[test]
    fn finance_cannot_halt_trading() {
        assert!(!AdminRole::Finance.has_permission(AdminPermission::TradingHalt));
        assert!(!AdminRole::Finance.has_permission(AdminPermission::TradingResume));
        assert!(!AdminRole::Finance.has_permission(AdminPermission::UsersFreeze));
    }

    #[test]
    fn risk_manager_can_halt_and_force_close_but_not_kyc() {
        assert!(AdminRole::RiskManager.has_permission(AdminPermission::TradingHalt));
        assert!(AdminRole::RiskManager.has_permission(AdminPermission::TradingResume));
        assert!(AdminRole::RiskManager.has_permission(AdminPermission::UsersForceClose));

        assert!(!AdminRole::RiskManager.has_permission(AdminPermission::KycApprove));
        assert!(!AdminRole::RiskManager.has_permission(AdminPermission::UsersAdjustBalance));
        assert!(!AdminRole::RiskManager.has_permission(AdminPermission::WalletsView));
    }

    #[test]
    fn surveillance_cannot_freeze_or_configure_pairs() {
        assert!(!AdminRole::Surveillance.has_permission(AdminPermission::UsersFreeze));
        assert!(!AdminRole::Surveillance.has_permission(AdminPermission::PairsConfigure));
        assert!(!AdminRole::Surveillance.has_permission(AdminPermission::TradingHalt));
    }
}
