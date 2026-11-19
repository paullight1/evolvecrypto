//! Withdrawal address ownership verification.
//!
//! When a user adds a new withdrawal address, it cannot be used immediately.
//! Either:
//! - A 24-hour cooldown must pass (configurable), OR
//! - The user completes a micro-transaction verification challenge.
//!
//! This prevents an attacker who gains temporary account access from immediately
//! withdrawing to their own address.
//!
//! # Reference
//! - Binance: 24h withdrawal address whitelist lock.
//! - Bybit: address verification required for new addresses.
//! - FATF Recommendation 16: originator/beneficiary identification.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Status of an address verification challenge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationStatus {
    /// Waiting for cooldown to expire or challenge to be completed.
    Pending,
    /// Micro-transaction or message signing challenge verified.
    Verified,
    /// Cooldown period expired — address is now usable.
    CooldownComplete,
    /// Challenge expired without completion — must re-initiate.
    Expired,
}

/// A pending address verification record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressVerification {
    pub id: Uuid,
    pub user_id: Uuid,
    /// The withdrawal address being verified.
    pub address: String,
    /// The asset/chain for this address.
    pub asset: String,
    pub status: VerificationStatus,
    /// When the verification was initiated.
    pub created_at: DateTime<Utc>,
    /// When the cooldown expires (address becomes usable without challenge).
    pub cooldown_until: DateTime<Utc>,
}

/// Manages withdrawal address verification challenges.
pub struct AddressVerificationManager {
    /// Cooldown duration before a new address can be used.
    cooldown_hours: i64,
    /// Pending and completed verifications: (user_id, address) → record.
    verifications: HashMap<(Uuid, String), AddressVerification>,
}

impl AddressVerificationManager {
    pub fn new(cooldown_hours: i64) -> Self {
        Self {
            cooldown_hours,
            verifications: HashMap::new(),
        }
    }

    /// Default: 24-hour cooldown.
    pub fn with_defaults() -> Self {
        Self::new(24)
    }

    /// Initiate verification for a new withdrawal address.
    /// Returns the verification record.
    pub fn initiate(
        &mut self,
        user_id: Uuid,
        address: &str,
        asset: &str,
    ) -> AddressVerification {
        let now = Utc::now();
        let record = AddressVerification {
            id: Uuid::new_v4(),
            user_id,
            address: address.to_string(),
            asset: asset.to_string(),
            status: VerificationStatus::Pending,
            created_at: now,
            cooldown_until: now + Duration::hours(self.cooldown_hours),
        };

        self.verifications
            .insert((user_id, address.to_lowercase()), record.clone());

        record
    }

    /// Mark an address as verified (challenge completed successfully).
    pub fn verify(&mut self, user_id: Uuid, address: &str) -> bool {
        let key = (user_id, address.to_lowercase());
        if let Some(record) = self.verifications.get_mut(&key) {
            if record.status == VerificationStatus::Pending {
                record.status = VerificationStatus::Verified;
                return true;
            }
        }
        false
    }

    /// Check if an address is usable for withdrawal.
    ///
    /// An address is usable if:
    /// - It has been explicitly verified (challenge completed), OR
    /// - The cooldown period has expired.
    ///
    /// Returns `false` if the address has never been registered or is still
    /// in the pending cooldown period.
    pub fn can_withdraw(&mut self, user_id: Uuid, address: &str) -> bool {
        let key = (user_id, address.to_lowercase());
        let now = Utc::now();

        match self.verifications.get_mut(&key) {
            None => false, // Never registered
            Some(record) => {
                match record.status {
                    VerificationStatus::Verified | VerificationStatus::CooldownComplete => true,
                    VerificationStatus::Pending => {
                        if now >= record.cooldown_until {
                            record.status = VerificationStatus::CooldownComplete;
                            true
                        } else {
                            false
                        }
                    }
                    VerificationStatus::Expired => false,
                }
            }
        }
    }

    /// Get the verification record for an address, if any.
    pub fn get_verification(&self, user_id: Uuid, address: &str) -> Option<&AddressVerification> {
        let key = (user_id, address.to_lowercase());
        self.verifications.get(&key)
    }

    /// Number of pending verifications for a user.
    pub fn pending_count(&self, user_id: Uuid) -> usize {
        self.verifications
            .values()
            .filter(|v| v.user_id == user_id && v.status == VerificationStatus::Pending)
            .count()
    }
}

impl Default for AddressVerificationManager {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> Uuid {
        Uuid::new_v4()
    }

    #[test]
    fn new_address_not_immediately_usable() {
        let mut mgr = AddressVerificationManager::with_defaults();
        let u = user();

        mgr.initiate(u, "0xabc123", "ETH");
        assert!(!mgr.can_withdraw(u, "0xabc123"));
    }

    #[test]
    fn verified_address_is_usable() {
        let mut mgr = AddressVerificationManager::with_defaults();
        let u = user();

        mgr.initiate(u, "0xabc123", "ETH");
        assert!(mgr.verify(u, "0xabc123"));
        assert!(mgr.can_withdraw(u, "0xabc123"));
    }

    #[test]
    fn cooldown_expired_address_is_usable() {
        let mut mgr = AddressVerificationManager::new(0); // 0h cooldown = immediate
        let u = user();

        mgr.initiate(u, "0xabc123", "ETH");
        // With 0h cooldown, the address should be immediately usable.
        assert!(mgr.can_withdraw(u, "0xabc123"));
    }

    #[test]
    fn unregistered_address_not_usable() {
        let mut mgr = AddressVerificationManager::with_defaults();
        let u = user();

        assert!(!mgr.can_withdraw(u, "0xunknown"));
    }

    #[test]
    fn case_insensitive_matching() {
        let mut mgr = AddressVerificationManager::with_defaults();
        let u = user();

        mgr.initiate(u, "0xABC123", "ETH");
        assert!(mgr.verify(u, "0xabc123")); // different case
        assert!(mgr.can_withdraw(u, "0xABC123"));
    }

    #[test]
    fn pending_count_tracks_correctly() {
        let mut mgr = AddressVerificationManager::with_defaults();
        let u = user();

        mgr.initiate(u, "0xaddr1", "ETH");
        mgr.initiate(u, "0xaddr2", "BTC");
        assert_eq!(mgr.pending_count(u), 2);

        mgr.verify(u, "0xaddr1");
        assert_eq!(mgr.pending_count(u), 1);
    }

    #[test]
    fn different_users_independent() {
        let mut mgr = AddressVerificationManager::with_defaults();
        let u1 = user();
        let u2 = user();

        mgr.initiate(u1, "0xaddr", "ETH");
        mgr.verify(u1, "0xaddr");

        // u2 never registered this address.
        assert!(!mgr.can_withdraw(u2, "0xaddr"));
    }
}
