//! Post-security-change withdrawal hold.
//!
//! When a user changes their 2FA device, resets their password, or performs
//! other security-sensitive actions, withdrawals are frozen for a configurable
//! hold period (default 48 hours). This is the standard defense against SIM
//! swap attacks used by Coinbase, Kraken, and Binance.
//!
//! # Reference
//!
//! - Coinbase: 48-hour withdrawal hold after 2FA reset
//! - Kraken: 24-72 hour hold depending on action
//! - Binance: 24 hour hold after password change, 2FA change

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

pub type UserId = Uuid;

/// Reason for the security hold — determines the hold duration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityHoldReason {
    /// User changed or reset their TOTP/2FA device.
    TwoFactorChange,
    /// User changed their password.
    PasswordChange,
    /// User changed their email address.
    EmailChange,
    /// Admin manually imposed a hold.
    AdminHold,
}

impl SecurityHoldReason {
    /// Default hold duration for each reason.
    pub fn hold_duration(&self) -> Duration {
        match self {
            SecurityHoldReason::TwoFactorChange => Duration::hours(48),
            SecurityHoldReason::PasswordChange => Duration::hours(24),
            SecurityHoldReason::EmailChange => Duration::hours(24),
            SecurityHoldReason::AdminHold => Duration::hours(72),
        }
    }
}

/// A single security hold record.
#[derive(Debug, Clone)]
pub struct SecurityHold {
    pub user_id: UserId,
    pub reason: SecurityHoldReason,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl SecurityHold {
    /// Returns true if the hold is still active.
    pub fn is_active(&self, now: DateTime<Utc>) -> bool {
        now < self.expires_at
    }
}

/// Manages post-security-change withdrawal holds across all users.
pub struct SecurityHoldManager {
    /// Active holds per user. A user may have multiple holds (e.g., changed
    /// password AND 2FA in the same session). The most restrictive (latest
    /// expiry) applies.
    holds: HashMap<UserId, Vec<SecurityHold>>,
}

impl Default for SecurityHoldManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SecurityHoldManager {
    pub fn new() -> Self {
        Self {
            holds: HashMap::new(),
        }
    }

    /// Impose a security hold on a user. Called when a security-sensitive
    /// action occurs (2FA change, password reset, etc.).
    pub fn impose_hold(&mut self, user_id: UserId, reason: SecurityHoldReason) -> SecurityHold {
        let now = Utc::now();
        let hold = SecurityHold {
            user_id,
            reason,
            created_at: now,
            expires_at: now + reason.hold_duration(),
        };

        self.holds
            .entry(user_id)
            .or_default()
            .push(hold.clone());

        tracing::info!(
            user_id = %user_id,
            reason = ?reason,
            expires_at = %hold.expires_at,
            "Security hold imposed — withdrawals frozen"
        );

        hold
    }

    /// Check if a user has an active security hold preventing withdrawals.
    /// Returns the hold with the latest expiry if active, None otherwise.
    pub fn check_hold(&self, user_id: &UserId, now: DateTime<Utc>) -> Option<&SecurityHold> {
        self.holds
            .get(user_id)?
            .iter()
            .filter(|h| h.is_active(now))
            .max_by_key(|h| h.expires_at)
    }

    /// Returns true if withdrawals are blocked for this user.
    pub fn is_withdrawal_blocked(&self, user_id: &UserId) -> bool {
        self.check_hold(user_id, Utc::now()).is_some()
    }

    /// Clean up expired holds to free memory.
    pub fn cleanup_expired(&mut self) {
        let now = Utc::now();
        for holds in self.holds.values_mut() {
            holds.retain(|h| h.is_active(now));
        }
        self.holds.retain(|_, v| !v.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_impose_hold_blocks_withdrawal() {
        let mut mgr = SecurityHoldManager::new();
        let user = Uuid::new_v4();

        mgr.impose_hold(user, SecurityHoldReason::TwoFactorChange);

        assert!(mgr.is_withdrawal_blocked(&user));
    }

    #[test]
    fn test_hold_expires_after_duration() {
        let mut mgr = SecurityHoldManager::new();
        let user = Uuid::new_v4();

        mgr.impose_hold(user, SecurityHoldReason::TwoFactorChange);

        // 47 hours later — still blocked
        let in_47h = Utc::now() + Duration::hours(47);
        assert!(mgr.check_hold(&user, in_47h).is_some());

        // 49 hours later — expired
        let in_49h = Utc::now() + Duration::hours(49);
        assert!(mgr.check_hold(&user, in_49h).is_none());
    }

    #[test]
    fn test_password_change_hold_24h() {
        let mut mgr = SecurityHoldManager::new();
        let user = Uuid::new_v4();

        mgr.impose_hold(user, SecurityHoldReason::PasswordChange);

        let in_23h = Utc::now() + Duration::hours(23);
        assert!(mgr.check_hold(&user, in_23h).is_some());

        let in_25h = Utc::now() + Duration::hours(25);
        assert!(mgr.check_hold(&user, in_25h).is_none());
    }

    #[test]
    fn test_no_hold_for_unaffected_user() {
        let mgr = SecurityHoldManager::new();
        let user = Uuid::new_v4();
        assert!(!mgr.is_withdrawal_blocked(&user));
    }

    #[test]
    fn test_multiple_holds_most_restrictive_wins() {
        let mut mgr = SecurityHoldManager::new();
        let user = Uuid::new_v4();

        // Password change (24h) then 2FA change (48h)
        mgr.impose_hold(user, SecurityHoldReason::PasswordChange);
        mgr.impose_hold(user, SecurityHoldReason::TwoFactorChange);

        // After 25h, password hold expired but 2FA hold still active
        let in_25h = Utc::now() + Duration::hours(25);
        let hold = mgr.check_hold(&user, in_25h);
        assert!(hold.is_some());
        assert_eq!(hold.unwrap().reason, SecurityHoldReason::TwoFactorChange);
    }

    #[test]
    fn test_cleanup_expired() {
        let mut mgr = SecurityHoldManager::new();
        let user_a = Uuid::new_v4();
        let user_b = Uuid::new_v4();

        // Manually create expired holds by backdating
        let old_hold = SecurityHold {
            user_id: user_a,
            reason: SecurityHoldReason::PasswordChange,
            created_at: Utc::now() - Duration::hours(48),
            expires_at: Utc::now() - Duration::hours(24),
        };
        mgr.holds.entry(user_a).or_default().push(old_hold);

        // Active hold for user B
        mgr.impose_hold(user_b, SecurityHoldReason::TwoFactorChange);

        mgr.cleanup_expired();

        assert!(!mgr.holds.contains_key(&user_a), "expired hold must be cleaned up");
        assert!(mgr.holds.contains_key(&user_b), "active hold must remain");
    }

    #[test]
    fn test_hold_reason_durations() {
        assert_eq!(SecurityHoldReason::TwoFactorChange.hold_duration(), Duration::hours(48));
        assert_eq!(SecurityHoldReason::PasswordChange.hold_duration(), Duration::hours(24));
        assert_eq!(SecurityHoldReason::EmailChange.hold_duration(), Duration::hours(24));
        assert_eq!(SecurityHoldReason::AdminHold.hold_duration(), Duration::hours(72));
    }
}
