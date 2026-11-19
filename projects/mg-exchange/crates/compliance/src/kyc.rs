use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::limits::TradingLimits;

/// KYC verification level.
///
/// Ukrainian Virtual Assets Law requires tiered verification:
/// - Level 0: email only — view-only, no trading
/// - Level 1: basic identity — limited trading
/// - Level 2: full verification — full access
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum KycLevel {
    /// No verification. Can browse, cannot trade or deposit.
    None,
    /// Basic: email + phone verified. Limited trading.
    Basic,
    /// Full: government ID + selfie. Full access.
    /// Required for transactions > 30,000 UAH (~$750).
    Full,
}

/// KYC verification status (state machine).
///
/// ```text
/// Unverified → Pending → Verified
///                ↓           ↓
///             Rejected   Suspended
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KycStatus {
    /// No verification submitted.
    Unverified,
    /// Verification documents submitted, awaiting review.
    Pending,
    /// Verification approved.
    Verified,
    /// Verification rejected (can resubmit).
    Rejected,
    /// Account suspended by compliance (manual review needed).
    Suspended,
}

/// KYC record for a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KycRecord {
    pub user_id: Uuid,
    pub level: KycLevel,
    pub status: KycStatus,
    /// Provider reference (e.g., Diia request ID or Sumsub applicant ID).
    pub provider_ref: Option<String>,
    /// Reason for rejection/suspension.
    pub reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// KYC manager — handles verification state transitions.
///
/// Phase 1: in-memory store with manual approval.
/// Phase 2: Diia API integration for Ukrainian users,
///          Sumsub for international users.
pub struct KycManager {
    records: HashMap<Uuid, KycRecord>,
}

impl KycManager {
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
        }
    }

    /// Create initial KYC record for a new user.
    pub fn create_user(&mut self, user_id: Uuid) {
        let now = Utc::now();
        self.records.insert(user_id, KycRecord {
            user_id,
            level: KycLevel::None,
            status: KycStatus::Unverified,
            provider_ref: None,
            reason: None,
            created_at: now,
            updated_at: now,
        });
    }

    /// Submit basic verification (email + phone).
    pub fn submit_basic(&mut self, user_id: Uuid) -> Result<(), KycError> {
        let record = self.records.get_mut(&user_id).ok_or(KycError::UserNotFound)?;

        match record.status {
            KycStatus::Unverified | KycStatus::Rejected => {
                record.status = KycStatus::Pending;
                record.level = KycLevel::Basic;
                record.updated_at = Utc::now();
                Ok(())
            }
            KycStatus::Suspended => Err(KycError::AccountSuspended),
            _ => Err(KycError::InvalidTransition {
                from: record.status,
                to: KycStatus::Pending,
            }),
        }
    }

    /// Submit full verification (government ID via Diia/Sumsub).
    pub fn submit_full(&mut self, user_id: Uuid, provider_ref: String) -> Result<(), KycError> {
        let record = self.records.get_mut(&user_id).ok_or(KycError::UserNotFound)?;

        if record.status == KycStatus::Suspended {
            return Err(KycError::AccountSuspended);
        }

        record.status = KycStatus::Pending;
        record.level = KycLevel::Full;
        record.provider_ref = Some(provider_ref);
        record.updated_at = Utc::now();
        Ok(())
    }

    /// Approve verification (called by compliance officer or webhook).
    pub fn approve(&mut self, user_id: Uuid) -> Result<(), KycError> {
        let record = self.records.get_mut(&user_id).ok_or(KycError::UserNotFound)?;

        if record.status != KycStatus::Pending {
            return Err(KycError::InvalidTransition {
                from: record.status,
                to: KycStatus::Verified,
            });
        }

        record.status = KycStatus::Verified;
        record.updated_at = Utc::now();
        tracing::info!("KYC approved for user {} (level: {:?})", user_id, record.level);
        Ok(())
    }

    /// Reject verification (with reason).
    pub fn reject(&mut self, user_id: Uuid, reason: String) -> Result<(), KycError> {
        let record = self.records.get_mut(&user_id).ok_or(KycError::UserNotFound)?;

        if record.status != KycStatus::Pending {
            return Err(KycError::InvalidTransition {
                from: record.status,
                to: KycStatus::Rejected,
            });
        }

        record.status = KycStatus::Rejected;
        record.reason = Some(reason);
        record.updated_at = Utc::now();
        Ok(())
    }

    /// Suspend account (compliance action).
    pub fn suspend(&mut self, user_id: Uuid, reason: String) -> Result<(), KycError> {
        let record = self.records.get_mut(&user_id).ok_or(KycError::UserNotFound)?;
        record.status = KycStatus::Suspended;
        record.reason = Some(reason);
        record.updated_at = Utc::now();
        tracing::warn!("Account suspended: user {} — {}", user_id, record.reason.as_deref().unwrap_or(""));
        Ok(())
    }

    /// Get KYC record.
    pub fn get(&self, user_id: &Uuid) -> Option<&KycRecord> {
        self.records.get(user_id)
    }

    /// Iterate over all KYC records.
    pub fn records(&self) -> impl Iterator<Item = &KycRecord> {
        self.records.values()
    }

    /// Get the effective trading limits for a user based on KYC level.
    pub fn trading_limits(&self, user_id: &Uuid) -> TradingLimits {
        match self.records.get(user_id) {
            Some(record) if record.status == KycStatus::Verified => {
                TradingLimits::for_level(record.level)
            }
            Some(record) if record.status == KycStatus::Suspended => {
                TradingLimits::suspended()
            }
            _ => TradingLimits::for_level(KycLevel::None),
        }
    }

    /// Check if user can trade.
    pub fn can_trade(&self, user_id: &Uuid) -> bool {
        self.trading_limits(user_id).can_trade
    }

    /// Check if user can withdraw.
    pub fn can_withdraw(&self, user_id: &Uuid) -> bool {
        self.trading_limits(user_id).can_withdraw
    }

    /// Bulk-load KYC records from an external source (e.g. database rows).
    ///
    /// Overwrites any existing in-memory record for each user_id. Used at
    /// startup to hydrate the cache from PostgreSQL.
    pub fn restore(&mut self, records: Vec<KycRecord>) {
        for record in records {
            self.records.insert(record.user_id, record);
        }
    }

    /// Return the number of records in the manager.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// True if the manager has no records.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

impl Default for KycManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum KycError {
    #[error("User not found")]
    UserNotFound,

    #[error("Account is suspended")]
    AccountSuspended,

    #[error("Invalid KYC transition: {from:?} → {to:?}")]
    InvalidTransition { from: KycStatus, to: KycStatus },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_user_cannot_trade() {
        let mut mgr = KycManager::new();
        let user = Uuid::new_v4();
        mgr.create_user(user);

        assert!(!mgr.can_trade(&user));
        assert!(!mgr.can_withdraw(&user));

        let record = mgr.get(&user).unwrap();
        assert_eq!(record.level, KycLevel::None);
        assert_eq!(record.status, KycStatus::Unverified);
    }

    #[test]
    fn test_basic_verification_flow() {
        let mut mgr = KycManager::new();
        let user = Uuid::new_v4();
        mgr.create_user(user);

        // Submit → Pending
        mgr.submit_basic(user).unwrap();
        assert_eq!(mgr.get(&user).unwrap().status, KycStatus::Pending);
        assert!(!mgr.can_trade(&user)); // Pending ≠ Verified

        // Approve → Verified
        mgr.approve(user).unwrap();
        assert_eq!(mgr.get(&user).unwrap().status, KycStatus::Verified);
        assert!(mgr.can_trade(&user));
    }

    #[test]
    fn test_full_verification_flow() {
        let mut mgr = KycManager::new();
        let user = Uuid::new_v4();
        mgr.create_user(user);

        mgr.submit_full(user, "diia-ref-123".to_string()).unwrap();
        assert_eq!(mgr.get(&user).unwrap().level, KycLevel::Full);

        mgr.approve(user).unwrap();
        assert!(mgr.can_trade(&user));
        assert!(mgr.can_withdraw(&user));

        let limits = mgr.trading_limits(&user);
        assert!(limits.daily_withdraw_usd > rust_decimal::Decimal::ZERO);
    }

    #[test]
    fn test_rejection_and_resubmit() {
        let mut mgr = KycManager::new();
        let user = Uuid::new_v4();
        mgr.create_user(user);

        mgr.submit_basic(user).unwrap();
        mgr.reject(user, "Blurry photo".to_string()).unwrap();

        assert_eq!(mgr.get(&user).unwrap().status, KycStatus::Rejected);
        assert!(!mgr.can_trade(&user));

        // Can resubmit after rejection
        mgr.submit_basic(user).unwrap();
        assert_eq!(mgr.get(&user).unwrap().status, KycStatus::Pending);
    }

    #[test]
    fn test_suspension_blocks_everything() {
        let mut mgr = KycManager::new();
        let user = Uuid::new_v4();
        mgr.create_user(user);

        // Get verified first
        mgr.submit_basic(user).unwrap();
        mgr.approve(user).unwrap();
        assert!(mgr.can_trade(&user));

        // Suspend
        mgr.suspend(user, "AML flag".to_string()).unwrap();
        assert!(!mgr.can_trade(&user));
        assert!(!mgr.can_withdraw(&user));

        // Cannot submit new verification while suspended
        let err = mgr.submit_basic(user).unwrap_err();
        assert_eq!(err, KycError::AccountSuspended);
    }

    #[test]
    fn test_cannot_approve_non_pending() {
        let mut mgr = KycManager::new();
        let user = Uuid::new_v4();
        mgr.create_user(user);

        let err = mgr.approve(user).unwrap_err();
        assert!(matches!(err, KycError::InvalidTransition { .. }));
    }

    #[test]
    fn test_unknown_user() {
        let mgr = KycManager::new();
        assert!(!mgr.can_trade(&Uuid::new_v4()));
    }

    #[test]
    fn test_kyc_level_ordering() {
        assert!(KycLevel::None < KycLevel::Basic);
        assert!(KycLevel::Basic < KycLevel::Full);
    }
}
