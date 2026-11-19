//! WalletConnect-like dApp session management.
//!
//! Phase 1: in-memory session store with 24-hour TTL.
//! Phase 2: implement the full WalletConnect v2 relay protocol.

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::types::{Chain, DAppPermission, DAppSession};

/// Manages WalletConnect-like sessions between users and dApps.
pub struct DAppConnector {
    /// Active sessions keyed by session_id.
    sessions: HashMap<String, DAppSession>,
}

impl DAppConnector {
    pub fn new() -> Self {
        Self { sessions: HashMap::new() }
    }

    /// Create a pending session for a user connecting to a dApp.
    ///
    /// The session starts as `is_approved = false`. The user must call
    /// `approve_session` to grant the dApp its requested permissions.
    pub fn create_session(
        &mut self,
        user_id: Uuid,
        dapp_url: &str,
        dapp_name: &str,
        chain: Chain,
        permissions: Vec<DAppPermission>,
    ) -> Result<DAppSession> {
        if dapp_url.is_empty() {
            return Err(anyhow!("dApp URL must not be empty"));
        }

        // Derive a session ID from user_id + url + timestamp so it is URL-safe
        // and collision-resistant.
        let session_id = Self::derive_session_id(user_id, dapp_url);

        if self.sessions.contains_key(&session_id) {
            return Err(anyhow!(
                "session already exists for user {user_id} and dApp {dapp_url}"
            ));
        }

        let now = Utc::now();
        let session = DAppSession {
            session_id: session_id.clone(),
            user_id,
            dapp_name: dapp_name.to_string(),
            dapp_url: dapp_url.to_string(),
            chain,
            permissions,
            connected_at: now,
            expires_at: now + Duration::hours(24),
            is_approved: false,
        };

        tracing::info!(
            user_id = %user_id,
            dapp_url = %dapp_url,
            chain = %chain,
            session_id = %session_id,
            "dApp session created (pending approval)"
        );

        self.sessions.insert(session_id, session.clone());
        Ok(session)
    }

    /// Approve a pending session, granting the dApp its requested permissions.
    pub fn approve_session(&mut self, session_id: &str) -> Result<()> {
        let session = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| anyhow!("session '{session_id}' not found"))?;

        if session.is_approved {
            return Err(anyhow!("session '{session_id}' is already approved"));
        }
        if Utc::now() > session.expires_at {
            return Err(anyhow!("session '{session_id}' has expired"));
        }

        session.is_approved = true;
        tracing::info!(session_id = %session_id, "dApp session approved");
        Ok(())
    }

    /// Revoke a session, disconnecting the dApp immediately.
    pub fn revoke_session(&mut self, session_id: &str) -> Result<()> {
        self.sessions
            .remove(session_id)
            .ok_or_else(|| anyhow!("session '{session_id}' not found"))?;
        tracing::info!(session_id = %session_id, "dApp session revoked");
        Ok(())
    }

    /// List all active (non-expired) approved sessions for a user.
    pub fn list_sessions(&self, user_id: Uuid) -> Vec<&DAppSession> {
        let now = Utc::now();
        self.sessions
            .values()
            .filter(|s| s.user_id == user_id && s.is_approved && now < s.expires_at)
            .collect()
    }

    /// Remove all expired sessions from memory (housekeeping).
    pub fn prune_expired(&mut self) {
        let now = Utc::now();
        let before = self.sessions.len();
        self.sessions.retain(|_, s| now < s.expires_at);
        let pruned = before - self.sessions.len();
        if pruned > 0 {
            tracing::debug!(pruned, "pruned expired dApp sessions");
        }
    }

    fn derive_session_id(user_id: Uuid, dapp_url: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(user_id.as_bytes());
        hasher.update(dapp_url.as_bytes());
        hasher.update(Utc::now().timestamp_micros().to_le_bytes());
        hex::encode(&hasher.finalize()[..16]) // 32-char hex
    }
}

impl Default for DAppConnector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_user() -> Uuid { Uuid::new_v4() }

    #[test]
    fn create_session_succeeds() {
        let mut conn = DAppConnector::new();
        let user_id = test_user();
        let session = conn.create_session(
            user_id, "https://app.uniswap.org", "Uniswap",
            Chain::Ethereum,
            vec![DAppPermission::ViewAddress, DAppPermission::SignTransaction],
        ).unwrap();

        assert_eq!(session.user_id, user_id);
        assert!(!session.is_approved);
        assert_eq!(session.chain, Chain::Ethereum);
    }

    #[test]
    fn create_session_rejects_empty_url() {
        let mut conn = DAppConnector::new();
        let err = conn.create_session(
            test_user(), "", "Unknown", Chain::Ethereum, vec![],
        ).unwrap_err();
        assert!(err.to_string().contains("must not be empty"));
    }

    #[test]
    fn approve_session_changes_flag() {
        let mut conn = DAppConnector::new();
        let session = conn.create_session(
            test_user(), "https://dapp.test", "Test",
            Chain::Arbitrum, vec![DAppPermission::SignMessage],
        ).unwrap();

        conn.approve_session(&session.session_id).unwrap();
        let approved = conn.sessions.get(&session.session_id).unwrap();
        assert!(approved.is_approved);
    }

    #[test]
    fn approve_unknown_session_fails() {
        let mut conn = DAppConnector::new();
        let err = conn.approve_session("nonexistent").unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn revoke_session_removes_it() {
        let mut conn = DAppConnector::new();
        let session = conn.create_session(
            test_user(), "https://dapp.test", "Test",
            Chain::Base, vec![],
        ).unwrap();

        conn.revoke_session(&session.session_id).unwrap();
        assert!(!conn.sessions.contains_key(&session.session_id));
    }

    #[test]
    fn list_sessions_returns_only_approved_non_expired() {
        let mut conn = DAppConnector::new();
        let user_id = test_user();

        // Create two sessions; approve only the first.
        let s1 = conn.create_session(
            user_id, "https://first.test", "First", Chain::Ethereum, vec![],
        ).unwrap();
        let _s2 = conn.create_session(
            user_id, "https://second.test", "Second", Chain::Polygon, vec![],
        ).unwrap();

        conn.approve_session(&s1.session_id).unwrap();

        let listed = conn.list_sessions(user_id);
        assert_eq!(listed.len(), 1, "only approved sessions should be listed");
        assert_eq!(listed[0].session_id, s1.session_id);
    }

    #[test]
    fn list_sessions_excludes_other_users() {
        let mut conn = DAppConnector::new();
        let user_a = test_user();
        let user_b = test_user();

        let sa = conn.create_session(
            user_a, "https://a.test", "A", Chain::Ethereum, vec![],
        ).unwrap();
        conn.approve_session(&sa.session_id).unwrap();

        assert_eq!(conn.list_sessions(user_b).len(), 0);
    }
}
