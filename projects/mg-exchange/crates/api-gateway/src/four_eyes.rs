//! Four-eyes (dual-control) approval for irreversible admin operations.
//!
//! When a protected admin action is requested, it does NOT execute immediately.
//! Instead it's placed in a pending queue with a 5-minute TTL. A second admin
//! must call `POST /admin/approvals/{id}/approve` to confirm. If not approved
//! within the TTL, the action expires and must be re-requested.
//!
//! This prevents a single compromised admin credential from executing
//! irreversible operations (deposit credits, API key revocations, etc.).

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// How long a pending action stays valid before auto-expiring.
const APPROVAL_TTL_SECS: i64 = 300; // 5 minutes

/// An admin action awaiting a second approval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingAction {
    pub id: Uuid,
    pub action_type: String,
    pub payload: serde_json::Value,
    pub requested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// Manager for the four-eyes approval queue.
pub struct FourEyesManager {
    pending: HashMap<Uuid, PendingAction>,
}

impl FourEyesManager {
    pub fn new() -> Self {
        Self {
            pending: HashMap::new(),
        }
    }

    /// Submit an action for approval. Returns the pending action ID.
    ///
    /// The action will NOT execute until a second admin calls `approve(id)`.
    pub fn submit(&mut self, action_type: &str, payload: serde_json::Value) -> PendingAction {
        let now = Utc::now();
        let action = PendingAction {
            id: Uuid::new_v4(),
            action_type: action_type.to_string(),
            payload,
            requested_at: now,
            expires_at: now + Duration::seconds(APPROVAL_TTL_SECS),
        };

        tracing::info!(
            action_id = %action.id,
            action_type = %action.action_type,
            expires_at = %action.expires_at,
            "Four-eyes: action submitted, awaiting second approval"
        );

        self.pending.insert(action.id, action.clone());
        action
    }

    /// Approve a pending action. Returns the action payload if found and not
    /// expired, or an error description.
    pub fn approve(&mut self, action_id: Uuid) -> Result<PendingAction, String> {
        self.expire_stale();

        let action = self.pending.remove(&action_id)
            .ok_or_else(|| format!("Action {action_id} not found or expired"))?;

        if Utc::now() > action.expires_at {
            return Err(format!("Action {action_id} has expired"));
        }

        tracing::warn!(
            action_id = %action.id,
            action_type = %action.action_type,
            "Four-eyes: action APPROVED by second admin"
        );

        Ok(action)
    }

    /// Reject (cancel) a pending action.
    pub fn reject(&mut self, action_id: Uuid) -> bool {
        let removed = self.pending.remove(&action_id).is_some();
        if removed {
            tracing::info!(action_id = %action_id, "Four-eyes: action rejected");
        }
        removed
    }

    /// List all non-expired pending actions.
    pub fn list_pending(&mut self) -> Vec<&PendingAction> {
        self.expire_stale();
        self.pending.values().collect()
    }

    /// Remove expired entries.
    fn expire_stale(&mut self) {
        let now = Utc::now();
        self.pending.retain(|_, a| a.expires_at > now);
    }

    pub fn count(&self) -> usize {
        self.pending.len()
    }
}

impl Default for FourEyesManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_creates_pending_action() {
        let mut mgr = FourEyesManager::new();
        let action = mgr.submit("credit_deposit", serde_json::json!({"deposit_id": "abc"}));
        assert_eq!(action.action_type, "credit_deposit");
        assert_eq!(mgr.count(), 1);
    }

    #[test]
    fn approve_returns_action_and_removes_it() {
        let mut mgr = FourEyesManager::new();
        let action = mgr.submit("revoke_api_keys", serde_json::json!({"user_id": "xyz"}));
        let id = action.id;

        let approved = mgr.approve(id).unwrap();
        assert_eq!(approved.action_type, "revoke_api_keys");
        assert_eq!(mgr.count(), 0, "approved action should be removed");
    }

    #[test]
    fn approve_unknown_id_fails() {
        let mut mgr = FourEyesManager::new();
        assert!(mgr.approve(Uuid::new_v4()).is_err());
    }

    #[test]
    fn reject_removes_action() {
        let mut mgr = FourEyesManager::new();
        let action = mgr.submit("block_ip", serde_json::json!({"ip": "1.2.3.4"}));
        assert!(mgr.reject(action.id));
        assert_eq!(mgr.count(), 0);
    }

    #[test]
    fn list_pending_returns_non_expired() {
        let mut mgr = FourEyesManager::new();
        mgr.submit("a", serde_json::json!({}));
        mgr.submit("b", serde_json::json!({}));
        let pending = mgr.list_pending();
        assert_eq!(pending.len(), 2);
    }

    #[test]
    fn double_approve_fails() {
        let mut mgr = FourEyesManager::new();
        let action = mgr.submit("test", serde_json::json!({}));
        let id = action.id;
        mgr.approve(id).unwrap();
        assert!(mgr.approve(id).is_err(), "second approve should fail");
    }
}
