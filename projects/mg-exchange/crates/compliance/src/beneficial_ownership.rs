//! Beneficial ownership registry — detects when multiple accounts share the
//! same underlying natural person based on government-issued identity documents.
//!
//! **Purpose:** If two accounts submit KYC with the same passport/national ID
//! number, they are treated as the same beneficial owner for Self-Trade
//! Prevention (STP) and wash-trading surveillance purposes.
//!
//! **Regulatory basis:** FATF Recommendation 10 requires identifying the
//! beneficial owner of accounts. Allowing the same person to operate multiple
//! accounts and trade with themselves undermines market integrity and can
//! constitute market manipulation under MiCA Article 12.
//!
//! **Design note:** This is an in-memory index keyed on normalised government
//! ID strings. A production implementation would back this with a database
//! unique-index on the KYC table's `gov_id` column, enforced at insertion time.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Outcome of a beneficial ownership registration attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegistrationResult {
    /// First time this government ID has been seen — registered successfully.
    Registered,
    /// This government ID is already linked to one or more other accounts.
    ///
    /// The `existing_users` set contains the user IDs already associated with
    /// this document. Callers should flag the new account for compliance review.
    Duplicate { existing_users: Vec<Uuid> },
}

/// Registry that maps government-issued document numbers to the set of user
/// accounts that submitted KYC with that document.
///
/// All government IDs are normalised (trimmed, uppercased) before storage so
/// that minor formatting differences (e.g., spaces, case) do not allow evasion.
pub struct BeneficialOwnershipRegistry {
    /// gov_id (normalised) → set of user_ids that submitted this document.
    gov_id_to_users: HashMap<String, HashSet<Uuid>>,
    /// user_id → normalised gov_id (reverse index for O(1) lookup).
    user_to_gov_id: HashMap<Uuid, String>,
}

impl BeneficialOwnershipRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            gov_id_to_users: HashMap::new(),
            user_to_gov_id: HashMap::new(),
        }
    }

    /// Normalise a government ID for consistent storage and lookup.
    ///
    /// Strips whitespace and converts to uppercase so that "AB 123456",
    /// "ab123456", and "AB123456" are all treated identically.
    fn normalise(gov_id: &str) -> String {
        gov_id
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_uppercase()
    }

    /// Register a user's government ID.
    ///
    /// Returns [`RegistrationResult::Duplicate`] if this document has already
    /// been registered by a *different* user. Registering the same `(user_id,
    /// gov_id)` pair a second time is idempotent and returns
    /// [`RegistrationResult::Registered`].
    pub fn register(&mut self, user_id: Uuid, gov_id: &str) -> RegistrationResult {
        let normalised = Self::normalise(gov_id);

        let users = self
            .gov_id_to_users
            .entry(normalised.clone())
            .or_default();

        // Idempotent: already registered for this user — no change.
        if users.contains(&user_id) {
            return RegistrationResult::Registered;
        }

        let existing: Vec<Uuid> = users.iter().copied().collect();
        users.insert(user_id);
        self.user_to_gov_id.insert(user_id, normalised);

        if existing.is_empty() {
            RegistrationResult::Registered
        } else {
            tracing::warn!(
                user_id = %user_id,
                existing_users = ?existing,
                "Beneficial ownership duplicate: government ID linked to multiple accounts"
            );
            RegistrationResult::Duplicate {
                existing_users: existing,
            }
        }
    }

    /// Return `true` if `user_a` and `user_b` share the same beneficial owner
    /// (i.e., they submitted KYC with the same government ID).
    ///
    /// Returns `false` if either user has not been registered, or if their
    /// government IDs differ.
    pub fn are_same_owner(&self, user_a: Uuid, user_b: Uuid) -> bool {
        if user_a == user_b {
            // Same account is trivially the same owner.
            return true;
        }
        match (
            self.user_to_gov_id.get(&user_a),
            self.user_to_gov_id.get(&user_b),
        ) {
            (Some(id_a), Some(id_b)) => id_a == id_b,
            _ => false,
        }
    }

    /// Return all user IDs associated with a given government ID, or an empty
    /// slice if the document has not been registered.
    pub fn users_for_gov_id(&self, gov_id: &str) -> Vec<Uuid> {
        let normalised = Self::normalise(gov_id);
        self.gov_id_to_users
            .get(&normalised)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Return the normalised government ID registered for a user, if any.
    pub fn gov_id_for_user(&self, user_id: &Uuid) -> Option<&str> {
        self.user_to_gov_id.get(user_id).map(String::as_str)
    }

    /// Total number of distinct government IDs registered.
    pub fn document_count(&self) -> usize {
        self.gov_id_to_users.len()
    }

    /// Total number of users registered.
    pub fn user_count(&self) -> usize {
        self.user_to_gov_id.len()
    }
}

impl Default for BeneficialOwnershipRegistry {
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
    fn test_single_user_registration() {
        let mut registry = BeneficialOwnershipRegistry::new();
        let user = Uuid::new_v4();

        let result = registry.register(user, "AB123456");
        assert_eq!(result, RegistrationResult::Registered);
        assert_eq!(registry.document_count(), 1);
        assert_eq!(registry.user_count(), 1);
    }

    #[test]
    fn test_same_passport_two_users_are_same_owner() {
        // Two users registered with the same passport number must be identified
        // as the same beneficial owner (M21-9 acceptance criterion).
        let mut registry = BeneficialOwnershipRegistry::new();
        let user_a = Uuid::new_v4();
        let user_b = Uuid::new_v4();

        registry.register(user_a, "PA1234567");
        let result = registry.register(user_b, "PA1234567");

        // Second registration must signal a duplicate.
        assert!(
            matches!(result, RegistrationResult::Duplicate { .. }),
            "Expected Duplicate for second user with same passport"
        );
        if let RegistrationResult::Duplicate { existing_users } = result {
            assert!(existing_users.contains(&user_a));
        }

        // Both users must be identified as the same beneficial owner.
        assert!(
            registry.are_same_owner(user_a, user_b),
            "Users with the same passport must be same_owner"
        );
    }

    #[test]
    fn test_different_passports_are_not_same_owner() {
        let mut registry = BeneficialOwnershipRegistry::new();
        let user_a = Uuid::new_v4();
        let user_b = Uuid::new_v4();

        registry.register(user_a, "PA0000001");
        registry.register(user_b, "PA0000002");

        assert!(
            !registry.are_same_owner(user_a, user_b),
            "Different passports must NOT be same_owner"
        );
    }

    #[test]
    fn test_same_owner_with_self_is_true() {
        let mut registry = BeneficialOwnershipRegistry::new();
        let user = Uuid::new_v4();
        registry.register(user, "ZZ999999");

        assert!(registry.are_same_owner(user, user));
    }

    #[test]
    fn test_unregistered_user_is_not_same_owner() {
        let registry = BeneficialOwnershipRegistry::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        assert!(!registry.are_same_owner(a, b));
    }

    #[test]
    fn test_gov_id_normalisation_ignores_case_and_spaces() {
        // "ab 123456", "AB123456", and "AB 123456" must resolve to the same identity.
        let mut registry = BeneficialOwnershipRegistry::new();
        let user_a = Uuid::new_v4();
        let user_b = Uuid::new_v4();

        registry.register(user_a, "ab 123456");
        let result = registry.register(user_b, "AB123456");

        assert!(
            matches!(result, RegistrationResult::Duplicate { .. }),
            "Normalisation must treat 'ab 123456' and 'AB123456' as the same document"
        );
        assert!(registry.are_same_owner(user_a, user_b));
    }

    #[test]
    fn test_idempotent_re_registration() {
        let mut registry = BeneficialOwnershipRegistry::new();
        let user = Uuid::new_v4();

        registry.register(user, "ID000001");
        let result = registry.register(user, "ID000001");

        // Re-registering the same (user, gov_id) pair must be idempotent.
        assert_eq!(result, RegistrationResult::Registered);
        assert_eq!(registry.user_count(), 1);
        assert_eq!(registry.document_count(), 1);
    }

    #[test]
    fn test_users_for_gov_id() {
        let mut registry = BeneficialOwnershipRegistry::new();
        let user_a = Uuid::new_v4();
        let user_b = Uuid::new_v4();

        registry.register(user_a, "SHARED-DOC");
        registry.register(user_b, "SHARED-DOC");

        let mut users = registry.users_for_gov_id("SHARED-DOC");
        users.sort(); // sort for deterministic assertion
        assert_eq!(users.len(), 2);
        assert!(users.contains(&user_a));
        assert!(users.contains(&user_b));
    }

    #[test]
    fn test_gov_id_for_user() {
        let mut registry = BeneficialOwnershipRegistry::new();
        let user = Uuid::new_v4();
        registry.register(user, "MY-DOC-123");

        assert_eq!(
            registry.gov_id_for_user(&user),
            Some("MY-DOC-123") // stored normalised (uppercase, no spaces)
        );
        assert_eq!(registry.gov_id_for_user(&Uuid::new_v4()), None);
    }
}
