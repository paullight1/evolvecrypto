use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

/// Duration that must elapse after adding an address before it can be used.
///
/// A 24-hour delay gives the account holder time to notice and cancel an
/// unauthorized address addition before funds can be sent to it.
const COOLDOWN_HOURS: i64 = 24;

// ── Types ─────────────────────────────────────────────────────────────────────

/// A single whitelisted withdrawal destination.
#[derive(Debug, Clone)]
pub struct WhitelistedAddress {
    /// On-chain address string.
    pub address: String,
    /// Network identifier (e.g. `"ethereum"`, `"bitcoin"`, `"tron"`).
    pub network: String,
    /// Human-readable label set by the user.
    pub label: String,
    /// When the address was added to the whitelist.
    pub added_at: DateTime<Utc>,
    /// Earliest time the address may be used (`added_at + 24 h`).
    pub active_at: DateTime<Utc>,
    /// Set to false when the address is removed (soft-delete for audit trail).
    pub is_active: bool,
}

// ── Manager ───────────────────────────────────────────────────────────────────

/// Per-user withdrawal address whitelist with a 24-hour activation cooldown.
///
/// Phase 1: in-memory store.
/// Phase 2: PostgreSQL-backed with email notifications on add/remove.
pub struct AddressWhitelist {
    /// `user_id → Vec<WhitelistedAddress>`  (includes removed entries for audit)
    addresses: HashMap<Uuid, Vec<WhitelistedAddress>>,
}

impl AddressWhitelist {
    pub fn new() -> Self {
        Self {
            addresses: HashMap::new(),
        }
    }

    /// Add a new address for a user.
    ///
    /// The address is immediately persisted but cannot be used for withdrawals
    /// until the 24-hour cooldown has elapsed.
    pub fn add_address(
        &mut self,
        user_id: Uuid,
        address: impl Into<String>,
        network: impl Into<String>,
        label: impl Into<String>,
    ) {
        let now = Utc::now();
        let entry = WhitelistedAddress {
            address: address.into(),
            network: network.into(),
            label: label.into(),
            added_at: now,
            active_at: now + Duration::hours(COOLDOWN_HOURS),
            is_active: true,
        };

        tracing::info!(
            user_id = %user_id,
            address = %entry.address,
            network = %entry.network,
            active_at = %entry.active_at,
            "Withdrawal address added — cooldown pending"
        );

        self.addresses.entry(user_id).or_default().push(entry);
    }

    /// Returns true if the address exists for the user, is active, and the
    /// 24-hour cooldown has fully elapsed.
    pub fn is_whitelisted(&self, user_id: Uuid, address: &str) -> bool {
        let now = Utc::now();
        self.addresses
            .get(&user_id)
            .map(|list| {
                list.iter().any(|a| {
                    a.is_active && a.address == address && now >= a.active_at
                })
            })
            .unwrap_or(false)
    }

    /// Remove an address from the whitelist (soft-delete).
    ///
    /// If the address is not found for the user this is a no-op; callers
    /// should not branch on this — idempotent removal is intentional.
    pub fn remove_address(&mut self, user_id: Uuid, address: &str) {
        if let Some(list) = self.addresses.get_mut(&user_id) {
            for entry in list.iter_mut() {
                if entry.address == address && entry.is_active {
                    entry.is_active = false;
                    tracing::info!(
                        user_id = %user_id,
                        address = %address,
                        "Withdrawal address removed"
                    );
                }
            }
        }
    }

    /// Returns all currently-active addresses for a user (including those still
    /// in the cooldown period).
    pub fn user_addresses(&self, user_id: Uuid) -> Vec<&WhitelistedAddress> {
        self.addresses
            .get(&user_id)
            .map(|list| list.iter().filter(|a| a.is_active).collect())
            .unwrap_or_default()
    }
}

impl Default for AddressWhitelist {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    // Helper: create a whitelist with an address whose `active_at` is
    // artificially set in the past (bypassing the real-time 24 h wait).
    fn add_address_already_active(
        wl: &mut AddressWhitelist,
        user_id: Uuid,
        address: &str,
        network: &str,
        label: &str,
    ) {
        wl.add_address(user_id, address, network, label);
        // Backdate active_at so the address is immediately usable
        let list = wl.addresses.get_mut(&user_id).unwrap();
        let entry = list.last_mut().unwrap();
        entry.active_at = Utc::now() - Duration::seconds(1);
    }

    // ── add_address ───────────────────────────────────────────────────────────

    #[test]
    fn test_add_address_appears_in_user_addresses() {
        let mut wl = AddressWhitelist::new();
        let user = Uuid::new_v4();

        wl.add_address(user, "0xabc", "ethereum", "My ETH wallet");

        let addrs = wl.user_addresses(user);
        assert_eq!(addrs.len(), 1);
        assert_eq!(addrs[0].address, "0xabc");
        assert_eq!(addrs[0].network, "ethereum");
        assert!(addrs[0].is_active);
    }

    #[test]
    fn test_add_address_sets_cooldown() {
        let mut wl = AddressWhitelist::new();
        let user = Uuid::new_v4();

        let before = Utc::now();
        wl.add_address(user, "bc1q123", "bitcoin", "Cold storage");
        let after = Utc::now();

        let entry = &wl.user_addresses(user)[0];
        let expected_min = before + Duration::hours(COOLDOWN_HOURS);
        let expected_max = after + Duration::hours(COOLDOWN_HOURS);

        assert!(
            entry.active_at >= expected_min && entry.active_at <= expected_max,
            "active_at should be ~24h from now"
        );
    }

    // ── is_whitelisted during cooldown ────────────────────────────────────────

    #[test]
    fn test_address_rejected_during_cooldown() {
        let mut wl = AddressWhitelist::new();
        let user = Uuid::new_v4();

        wl.add_address(user, "0xabc", "ethereum", "My wallet");

        // active_at is 24 h in the future — should NOT be whitelisted yet
        assert!(
            !wl.is_whitelisted(user, "0xabc"),
            "Address should be blocked during the 24-hour cooldown"
        );
    }

    // ── is_whitelisted after cooldown ─────────────────────────────────────────

    #[test]
    fn test_address_active_after_cooldown() {
        let mut wl = AddressWhitelist::new();
        let user = Uuid::new_v4();

        add_address_already_active(&mut wl, user, "0xabc", "ethereum", "label");

        assert!(
            wl.is_whitelisted(user, "0xabc"),
            "Address should be whitelisted after cooldown"
        );
    }

    #[test]
    fn test_unknown_address_not_whitelisted() {
        let mut wl = AddressWhitelist::new();
        let user = Uuid::new_v4();

        add_address_already_active(&mut wl, user, "0xabc", "ethereum", "label");

        assert!(!wl.is_whitelisted(user, "0xDEAD"));
    }

    #[test]
    fn test_unknown_user_not_whitelisted() {
        let wl = AddressWhitelist::new();
        assert!(!wl.is_whitelisted(Uuid::new_v4(), "0xabc"));
    }

    // ── remove_address ────────────────────────────────────────────────────────

    #[test]
    fn test_remove_address() {
        let mut wl = AddressWhitelist::new();
        let user = Uuid::new_v4();

        add_address_already_active(&mut wl, user, "0xabc", "ethereum", "label");
        assert!(wl.is_whitelisted(user, "0xabc"));

        wl.remove_address(user, "0xabc");

        assert!(
            !wl.is_whitelisted(user, "0xabc"),
            "Removed address must no longer be whitelisted"
        );
        assert_eq!(
            wl.user_addresses(user).len(),
            0,
            "Removed address must not appear in user_addresses"
        );
    }

    #[test]
    fn test_remove_nonexistent_address_is_noop() {
        let mut wl = AddressWhitelist::new();
        let user = Uuid::new_v4();

        // Should not panic
        wl.remove_address(user, "0xnope");
    }

    #[test]
    fn test_remove_only_affects_target_address() {
        let mut wl = AddressWhitelist::new();
        let user = Uuid::new_v4();

        add_address_already_active(&mut wl, user, "0xaaa", "ethereum", "first");
        add_address_already_active(&mut wl, user, "0xbbb", "ethereum", "second");

        wl.remove_address(user, "0xaaa");

        assert!(!wl.is_whitelisted(user, "0xaaa"));
        assert!(wl.is_whitelisted(user, "0xbbb"));
        assert_eq!(wl.user_addresses(user).len(), 1);
    }

    // ── user_addresses ────────────────────────────────────────────────────────

    #[test]
    fn test_user_addresses_empty_for_unknown_user() {
        let wl = AddressWhitelist::new();
        assert!(wl.user_addresses(Uuid::new_v4()).is_empty());
    }

    #[test]
    fn test_user_addresses_includes_cooldown_addresses() {
        let mut wl = AddressWhitelist::new();
        let user = Uuid::new_v4();

        // Add an address that is still in cooldown
        wl.add_address(user, "0xnew", "ethereum", "pending");

        // user_addresses shows active addresses including those in cooldown
        let addrs = wl.user_addresses(user);
        assert_eq!(addrs.len(), 1);
        assert!(addrs[0].is_active);
    }

    #[test]
    fn test_multiple_users_are_isolated() {
        let mut wl = AddressWhitelist::new();
        let alice = Uuid::new_v4();
        let bob = Uuid::new_v4();

        add_address_already_active(&mut wl, alice, "0xalice", "ethereum", "Alice wallet");
        add_address_already_active(&mut wl, bob, "0xbob", "ethereum", "Bob wallet");

        assert!(wl.is_whitelisted(alice, "0xalice"));
        assert!(!wl.is_whitelisted(alice, "0xbob"));
        assert!(wl.is_whitelisted(bob, "0xbob"));
        assert!(!wl.is_whitelisted(bob, "0xalice"));
    }
}
