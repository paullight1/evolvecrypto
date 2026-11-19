//! Multi-account detection — identifies when a single person operates multiple
//! exchange accounts to abuse referral programs, evade limits, or wash trade.
//!
//! # Detection Heuristics
//!
//! 1. **Deposit address reuse** — two accounts depositing from the same on-chain
//!    address almost certainly belong to the same person.
//! 2. **Withdrawal address reuse** — two accounts withdrawing to the same destination.
//! 3. **Email domain clustering** — multiple accounts using the same unusual email
//!    domain (e.g., custom domains) may indicate one person.
//!
//! # Reference
//!
//! Standard exchange compliance practice. Binance, Coinbase, and Kraken all
//! implement similar multi-account detection for referral abuse and AML.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// User ID type alias for clarity.
pub type UserId = Uuid;

/// Alert emitted when multi-account behaviour is detected.
#[derive(Debug, Clone)]
pub struct MultiAccountAlert {
    pub id: Uuid,
    /// The accounts suspected of belonging to the same person.
    pub user_ids: Vec<UserId>,
    /// What triggered the alert.
    pub reason: MultiAccountReason,
    /// The shared value that linked the accounts (address, domain, etc.).
    pub shared_value: String,
    pub created_at: DateTime<Utc>,
    pub reviewed: bool,
}

/// Reason a multi-account alert was triggered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MultiAccountReason {
    /// Two+ accounts deposited from the same on-chain address.
    SharedDepositAddress,
    /// Two+ accounts withdrew to the same on-chain address.
    SharedWithdrawalAddress,
    /// Two+ accounts use the same unusual email domain.
    SharedEmailDomain,
}

/// Detects when multiple accounts share identifying information that suggests
/// they belong to the same person.
pub struct MultiAccountDetector {
    /// deposit_address → set of user IDs who deposited from it
    deposit_address_users: HashMap<String, HashSet<UserId>>,
    /// withdrawal_address → set of user IDs who withdrew to it
    withdrawal_address_users: HashMap<String, HashSet<UserId>>,
    /// email_domain → set of user IDs using that domain
    email_domain_users: HashMap<String, HashSet<UserId>>,
    /// Domains that are too common to be useful signals.
    common_domains: HashSet<String>,
    /// Emitted alerts.
    alerts: Vec<MultiAccountAlert>,
}

impl Default for MultiAccountDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl MultiAccountDetector {
    pub fn new() -> Self {
        let common_domains: HashSet<String> = [
            "gmail.com", "yahoo.com", "outlook.com", "hotmail.com",
            "protonmail.com", "icloud.com", "mail.com", "aol.com",
            "yandex.ru", "mail.ru", "ukr.net", "i.ua",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        Self {
            deposit_address_users: HashMap::new(),
            withdrawal_address_users: HashMap::new(),
            email_domain_users: HashMap::new(),
            common_domains,
            alerts: Vec::new(),
        }
    }

    /// Record a deposit from an on-chain address for a user.
    /// Returns an alert if this address was previously used by a different user.
    pub fn on_deposit(&mut self, user_id: UserId, address: &str) -> Option<MultiAccountAlert> {
        let normalized = address.to_lowercase();
        let users = self.deposit_address_users
            .entry(normalized.clone())
            .or_default();
        users.insert(user_id);

        if users.len() >= 2 {
            let alert = MultiAccountAlert {
                id: Uuid::new_v4(),
                user_ids: users.iter().copied().collect(),
                reason: MultiAccountReason::SharedDepositAddress,
                shared_value: normalized,
                created_at: Utc::now(),
                reviewed: false,
            };
            self.alerts.push(alert.clone());
            Some(alert)
        } else {
            None
        }
    }

    /// Record a withdrawal to an on-chain address for a user.
    /// Returns an alert if this address was previously used by a different user.
    pub fn on_withdrawal(&mut self, user_id: UserId, address: &str) -> Option<MultiAccountAlert> {
        let normalized = address.to_lowercase();
        let users = self.withdrawal_address_users
            .entry(normalized.clone())
            .or_default();
        users.insert(user_id);

        if users.len() >= 2 {
            let alert = MultiAccountAlert {
                id: Uuid::new_v4(),
                user_ids: users.iter().copied().collect(),
                reason: MultiAccountReason::SharedWithdrawalAddress,
                shared_value: normalized,
                created_at: Utc::now(),
                reviewed: false,
            };
            self.alerts.push(alert.clone());
            Some(alert)
        } else {
            None
        }
    }

    /// Record a user's email for domain clustering.
    /// Returns an alert if the domain is unusual and shared with another account.
    pub fn on_registration(&mut self, user_id: UserId, email: &str) -> Option<MultiAccountAlert> {
        let domain = match email.rsplit_once('@') {
            Some((_, d)) => d.to_lowercase(),
            None => return None,
        };

        // Common domains are too noisy for multi-account detection.
        if self.common_domains.contains(&domain) {
            return None;
        }

        let users = self.email_domain_users
            .entry(domain.clone())
            .or_default();
        users.insert(user_id);

        if users.len() >= 2 {
            let alert = MultiAccountAlert {
                id: Uuid::new_v4(),
                user_ids: users.iter().copied().collect(),
                reason: MultiAccountReason::SharedEmailDomain,
                shared_value: domain,
                created_at: Utc::now(),
                reviewed: false,
            };
            self.alerts.push(alert.clone());
            Some(alert)
        } else {
            None
        }
    }

    /// Get all unreviewed alerts.
    pub fn unreviewed_alerts(&self) -> Vec<&MultiAccountAlert> {
        self.alerts.iter().filter(|a| !a.reviewed).collect()
    }

    /// Mark an alert as reviewed.
    pub fn review_alert(&mut self, alert_id: &Uuid) -> bool {
        if let Some(alert) = self.alerts.iter_mut().find(|a| a.id == *alert_id) {
            alert.reviewed = true;
            true
        } else {
            false
        }
    }

    /// Total number of alerts.
    pub fn alert_count(&self) -> usize {
        self.alerts.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_alert_for_single_user_deposit() {
        let mut det = MultiAccountDetector::new();
        let user = Uuid::new_v4();
        assert!(det.on_deposit(user, "0xABC123").is_none());
    }

    #[test]
    fn test_alert_on_shared_deposit_address() {
        let mut det = MultiAccountDetector::new();
        let alice = Uuid::new_v4();
        let bob = Uuid::new_v4();

        assert!(det.on_deposit(alice, "0xABC123").is_none());
        let alert = det.on_deposit(bob, "0xABC123");
        assert!(alert.is_some(), "shared deposit address must trigger alert");

        let alert = alert.unwrap();
        assert_eq!(alert.reason, MultiAccountReason::SharedDepositAddress);
        assert_eq!(alert.user_ids.len(), 2);
        assert!(alert.user_ids.contains(&alice));
        assert!(alert.user_ids.contains(&bob));
    }

    #[test]
    fn test_case_insensitive_address_matching() {
        let mut det = MultiAccountDetector::new();
        let alice = Uuid::new_v4();
        let bob = Uuid::new_v4();

        det.on_deposit(alice, "0xAbC123");
        let alert = det.on_deposit(bob, "0xabc123");
        assert!(alert.is_some(), "case-insensitive match must trigger");
    }

    #[test]
    fn test_alert_on_shared_withdrawal_address() {
        let mut det = MultiAccountDetector::new();
        let alice = Uuid::new_v4();
        let bob = Uuid::new_v4();

        det.on_withdrawal(alice, "bc1qxyz");
        let alert = det.on_withdrawal(bob, "bc1qxyz");
        assert!(alert.is_some());
        assert_eq!(alert.unwrap().reason, MultiAccountReason::SharedWithdrawalAddress);
    }

    #[test]
    fn test_no_alert_for_common_email_domain() {
        let mut det = MultiAccountDetector::new();
        let alice = Uuid::new_v4();
        let bob = Uuid::new_v4();

        assert!(det.on_registration(alice, "alice@gmail.com").is_none());
        assert!(det.on_registration(bob, "bob@gmail.com").is_none(),
            "gmail.com is too common to be a useful signal");
    }

    #[test]
    fn test_alert_on_unusual_email_domain() {
        let mut det = MultiAccountDetector::new();
        let alice = Uuid::new_v4();
        let bob = Uuid::new_v4();

        assert!(det.on_registration(alice, "alice@company-xyz.com").is_none());
        let alert = det.on_registration(bob, "bob@company-xyz.com");
        assert!(alert.is_some(), "unusual shared domain must trigger alert");
        assert_eq!(alert.unwrap().reason, MultiAccountReason::SharedEmailDomain);
    }

    #[test]
    fn test_three_users_same_address() {
        let mut det = MultiAccountDetector::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();

        det.on_deposit(a, "0xShared");
        det.on_deposit(b, "0xShared");
        let alert = det.on_deposit(c, "0xShared");
        assert!(alert.is_some());
        assert_eq!(alert.unwrap().user_ids.len(), 3);
    }

    #[test]
    fn test_review_alert() {
        let mut det = MultiAccountDetector::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();

        det.on_deposit(a, "0xABC");
        det.on_deposit(b, "0xABC");

        assert_eq!(det.unreviewed_alerts().len(), 1);
        let alert_id = det.unreviewed_alerts()[0].id;

        assert!(det.review_alert(&alert_id));
        assert_eq!(det.unreviewed_alerts().len(), 0);
    }

    #[test]
    fn test_same_user_multiple_deposits_no_alert() {
        let mut det = MultiAccountDetector::new();
        let user = Uuid::new_v4();

        // Same user depositing from the same address multiple times — not multi-account.
        assert!(det.on_deposit(user, "0xABC").is_none());
        assert!(det.on_deposit(user, "0xABC").is_none());
        assert_eq!(det.alert_count(), 0);
    }
}
