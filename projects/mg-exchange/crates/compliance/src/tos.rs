//! Terms of Service (ToS) versioning and user acceptance tracking.
//!
//! Phase 1: in-memory storage — acceptance records are lost on restart.
//! Phase 2: persist to database; wire mandatory-check middleware into order placement.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A published Terms of Service version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TosVersion {
    /// Semantic version string, e.g. "1.0", "1.1".
    pub version: String,
    /// SHA-256 of the full ToS text (for tamper-evidence).
    pub content_hash: String,
    /// When this version was published.
    pub published_at: DateTime<Utc>,
    /// Whether users must accept this version before they can trade.
    pub mandatory: bool,
    /// Human-readable summary of what changed.
    pub summary: String,
}

/// A user's recorded acceptance of a specific ToS version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TosAcceptance {
    pub user_id: Uuid,
    pub version: String,
    pub accepted_at: DateTime<Utc>,
    /// Client IP at time of acceptance (for audit trail).
    pub ip_address: Option<String>,
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

/// Terms of Service manager.
///
/// Tracks published versions and per-user acceptances. Thread-safety is
/// provided by the caller via `Mutex<TosManager>` on the Exchange struct.
pub struct TosManager {
    /// Published versions in chronological order (oldest first).
    versions: Vec<TosVersion>,
    /// Latest acceptance per user — only the most recent acceptance is kept.
    acceptances: HashMap<Uuid, TosAcceptance>,
}

impl TosManager {
    /// Create a manager bootstrapped with version 1.0.
    pub fn new() -> Self {
        let mut mgr = Self {
            versions: Vec::new(),
            acceptances: HashMap::new(),
        };
        mgr.versions.push(TosVersion {
            version: "1.0".into(),
            content_hash: "sha256_placeholder_v1".into(),
            published_at: Utc::now(),
            mandatory: true,
            summary: "Initial Terms of Service".into(),
        });
        mgr
    }

    /// Return the latest mandatory ToS version, or `None` if none exists.
    pub fn latest_mandatory(&self) -> Option<&TosVersion> {
        self.versions.iter().rev().find(|v| v.mandatory)
    }

    /// Return true if `user_id` has accepted the latest mandatory version.
    ///
    /// Returns true when there is no mandatory version (nothing to accept).
    pub fn has_accepted_latest(&self, user_id: Uuid) -> bool {
        let Some(latest) = self.latest_mandatory() else {
            return true;
        };
        self.acceptances
            .get(&user_id)
            .map(|a| a.version == latest.version)
            .unwrap_or(false)
    }

    /// Record that `user_id` has accepted the given `version`.
    ///
    /// Returns an error if the version string does not match any published version.
    pub fn accept(&mut self, user_id: Uuid, version: &str, ip: Option<String>) -> Result<(), String> {
        if !self.versions.iter().any(|v| v.version == version) {
            return Err(format!("Unknown ToS version: {version}"));
        }
        self.acceptances.insert(
            user_id,
            TosAcceptance {
                user_id,
                version: version.to_string(),
                accepted_at: Utc::now(),
                ip_address: ip,
            },
        );
        Ok(())
    }

    /// Return the acceptance record for a user, if any.
    pub fn get_acceptance(&self, user_id: Uuid) -> Option<&TosAcceptance> {
        self.acceptances.get(&user_id)
    }

    /// Return all published versions in chronological order.
    pub fn all_versions(&self) -> &[TosVersion] {
        &self.versions
    }

    /// Publish a new ToS version.
    pub fn add_version(&mut self, version: TosVersion) {
        self.versions.push(version);
    }
}

impl Default for TosManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_latest_mandatory_returns_v1() {
        let mgr = TosManager::new();
        let latest = mgr.latest_mandatory().expect("v1.0 should be mandatory");
        assert_eq!(latest.version, "1.0");
        assert!(latest.mandatory);
    }

    #[test]
    fn test_has_not_accepted_by_default() {
        let mgr = TosManager::new();
        assert!(!mgr.has_accepted_latest(Uuid::new_v4()));
    }

    #[test]
    fn test_accept_and_check() {
        let mut mgr = TosManager::new();
        let user = Uuid::new_v4();
        mgr.accept(user, "1.0", None).expect("accept v1.0");
        assert!(mgr.has_accepted_latest(user));
    }

    #[test]
    fn test_accept_unknown_version_returns_error() {
        let mut mgr = TosManager::new();
        let err = mgr.accept(Uuid::new_v4(), "99.0", None).unwrap_err();
        assert!(err.contains("Unknown ToS version"));
    }

    #[test]
    fn test_new_mandatory_version_requires_re_accept() {
        let mut mgr = TosManager::new();
        let user = Uuid::new_v4();
        mgr.accept(user, "1.0", None).expect("accept v1.0");
        assert!(mgr.has_accepted_latest(user));

        mgr.add_version(TosVersion {
            version: "2.0".into(),
            content_hash: "sha256_placeholder_v2".into(),
            published_at: Utc::now(),
            mandatory: true,
            summary: "Updated terms".into(),
        });

        // User accepted v1.0 but not the new v2.0.
        assert!(!mgr.has_accepted_latest(user));
    }

    #[test]
    fn test_non_mandatory_version_does_not_require_accept() {
        let mut mgr = TosManager::new();
        let user = Uuid::new_v4();
        mgr.accept(user, "1.0", None).expect("accept v1.0");

        mgr.add_version(TosVersion {
            version: "1.1".into(),
            content_hash: "sha256_placeholder_v1_1".into(),
            published_at: Utc::now(),
            mandatory: false,
            summary: "Minor clarification".into(),
        });

        // Latest mandatory is still v1.0; user already accepted it.
        assert!(mgr.has_accepted_latest(user));
    }

    #[test]
    fn test_get_acceptance_records_ip() {
        let mut mgr = TosManager::new();
        let user = Uuid::new_v4();
        mgr.accept(user, "1.0", Some("127.0.0.1".into())).expect("accept");
        let acc = mgr.get_acceptance(user).expect("acceptance stored");
        assert_eq!(acc.ip_address.as_deref(), Some("127.0.0.1"));
    }

    #[test]
    fn test_no_mandatory_version_always_accepted() {
        let mgr = TosManager {
            versions: vec![TosVersion {
                version: "1.0".into(),
                content_hash: "hash".into(),
                published_at: Utc::now(),
                mandatory: false,
                summary: "Optional".into(),
            }],
            acceptances: HashMap::new(),
        };
        // No mandatory version → every user is considered accepted.
        assert!(mgr.has_accepted_latest(Uuid::new_v4()));
    }
}
