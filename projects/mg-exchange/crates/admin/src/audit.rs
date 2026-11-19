use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use exchange_common::UserId;

// ---------------------------------------------------------------------------
// Action enum
// ---------------------------------------------------------------------------

/// Every significant action the system can record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditAction {
    // Auth
    Login,
    LoginFailed,
    ApiKeyCreated,
    ApiKeyRevoked,
    TwoFactorEnabled,
    TwoFactorDisabled,

    // Trading
    OrderPlaced,
    OrderCancelled,
    OrderFilled,

    // Finance
    DepositDetected,
    DepositCredited,
    WithdrawalRequested,
    WithdrawalApproved,
    WithdrawalRejected,

    // Account
    LeverageChanged,
    MarginAdded,

    // Admin
    AccountFrozen,
    AccountUnfrozen,
    TradingHalted,
    TradingResumed,
    FeatureFlagChanged,

    // Compliance
    KycSubmitted,
    KycApproved,
    KycRejected,
    AmlAlertCreated,
    AmlAlertReviewed,
}

// ---------------------------------------------------------------------------
// Entry
// ---------------------------------------------------------------------------

/// A single immutable record in the audit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Monotonically increasing, starting at 1.
    pub id: u64,
    /// `None` for system-generated actions that have no associated user.
    pub user_id: Option<UserId>,
    pub action: AuditAction,
    /// Structured detail — keep only what is needed for a regulator query.
    pub detail: Value,
    pub ip_address: Option<String>,
    pub timestamp: DateTime<Utc>,
    /// SHA-256 hash of the previous entry (or "genesis" for the first entry).
    /// Forms a tamper-evident chain: changing any historical entry breaks all
    /// subsequent hashes.
    pub prev_hash: String,
    /// SHA-256 hash of this entry's content, including `prev_hash`.
    pub entry_hash: String,
}

/// Compute the SHA-256 hash for an entry given its content fields.
fn compute_entry_hash(
    prev_hash: &str,
    user_id: Option<UserId>,
    timestamp: &DateTime<Utc>,
    action: &AuditAction,
    detail: &Value,
) -> String {
    let payload = format!(
        "{}|{:?}|{}|{:?}|{}",
        prev_hash,
        user_id,
        timestamp.timestamp_nanos_opt().unwrap_or(0),
        action,
        detail,
    );
    let hash = Sha256::digest(payload.as_bytes());
    format!("{:x}", hash)
}

// ---------------------------------------------------------------------------
// Filter
// ---------------------------------------------------------------------------

/// Predicate for querying the audit log.
///
/// All set fields are ANDed together. A `limit` of 0 means "return all".
pub struct AuditFilter {
    pub user_id: Option<UserId>,
    pub action: Option<AuditAction>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    /// Maximum number of entries to return. 0 = no limit.
    pub limit: usize,
}

impl AuditFilter {
    fn matches(&self, entry: &AuditEntry) -> bool {
        if let Some(uid) = self.user_id {
            if entry.user_id != Some(uid) {
                return false;
            }
        }
        if let Some(ref action) = self.action {
            if &entry.action != action {
                return false;
            }
        }
        if let Some(from) = self.from {
            if entry.timestamp < from {
                return false;
            }
        }
        if let Some(to) = self.to {
            if entry.timestamp > to {
                return false;
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// Log
// ---------------------------------------------------------------------------

/// Immutable, append-only audit log. Every significant action is recorded.
///
/// Designed for regulatory compliance (5+ year retention).
///
/// Phase 1: in-memory. Phase 2: persist to an append-only PostgreSQL table
/// with a separate archival pipeline writing to object storage.
pub struct AuditLog {
    entries: Vec<AuditEntry>,
    /// Tracks the next id to assign. Starts at 1.
    next_id: u64,
    /// Hash to use as `prev_hash` for the first entry. Defaults to "genesis";
    /// set to the DB's last `entry_hash` at startup via `with_genesis_hash`.
    genesis_hash: String,
}

impl AuditLog {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_id: 1,
            genesis_hash: "genesis".to_string(),
        }
    }

    /// Create an `AuditLog` that continues the hash chain from a persisted
    /// database state. `last_db_hash` is the `entry_hash` of the most recent
    /// row in the `audit_log` table (or `"genesis"` if the table is empty).
    /// `next_id` is the DB's `max(id) + 1` (or 1 if empty).
    pub fn continuing_from(last_db_hash: String, next_id: u64) -> Self {
        Self {
            entries: Vec::new(),
            next_id,
            genesis_hash: last_db_hash,
        }
    }

    /// Append a new entry and return its assigned id.
    pub fn log(
        &mut self,
        user_id: Option<UserId>,
        action: AuditAction,
        detail: Value,
        ip_address: Option<String>,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let timestamp = Utc::now();

        let prev_hash = self
            .entries
            .last()
            .map(|e| e.entry_hash.clone())
            .unwrap_or_else(|| self.genesis_hash.clone());

        let entry_hash = compute_entry_hash(&prev_hash, user_id, &timestamp, &action, &detail);

        tracing::info!(
            audit_id = id,
            user_id = ?user_id,
            action = ?action,
            "audit entry recorded"
        );

        self.entries.push(AuditEntry {
            id,
            user_id,
            action,
            detail,
            ip_address,
            timestamp,
            prev_hash,
            entry_hash,
        });

        id
    }

    /// Walk the entire chain and verify each entry's hash matches its recorded value.
    ///
    /// Returns `true` only if every entry is intact. A tampered entry (or any
    /// entry inserted/removed out of order) will return `false`.
    pub fn verify_integrity(&self) -> bool {
        let mut expected_prev = self.genesis_hash.clone();

        for entry in &self.entries {
            if entry.prev_hash != expected_prev {
                tracing::warn!(
                    audit_id = entry.id,
                    "Integrity check failed: prev_hash mismatch"
                );
                return false;
            }

            let recomputed = compute_entry_hash(
                &entry.prev_hash,
                entry.user_id,
                &entry.timestamp,
                &entry.action,
                &entry.detail,
            );

            if recomputed != entry.entry_hash {
                tracing::warn!(
                    audit_id = entry.id,
                    "Integrity check failed: entry_hash mismatch"
                );
                return false;
            }

            expected_prev = entry.entry_hash.clone();
        }

        true
    }

    /// Return entries matching `filter`, oldest first.
    ///
    /// `filter.limit == 0` means no limit.
    pub fn query(&self, filter: &AuditFilter) -> Vec<&AuditEntry> {
        let iter = self.entries.iter().filter(|e| filter.matches(e));
        if filter.limit == 0 {
            iter.collect()
        } else {
            iter.take(filter.limit).collect()
        }
    }

    /// Total number of entries ever recorded.
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// All entries recorded at or after `timestamp`, oldest first.
    pub fn entries_since(&self, timestamp: DateTime<Utc>) -> Vec<&AuditEntry> {
        self.entries
            .iter()
            .filter(|e| e.timestamp >= timestamp)
            .collect()
    }

    /// All entries with `id > after_id`, oldest first.
    ///
    /// Used by the DB flusher to efficiently find only the entries that have
    /// not yet been persisted to PostgreSQL.
    pub fn entries_since_id(&self, after_id: u64) -> Vec<&AuditEntry> {
        self.entries
            .iter()
            .filter(|e| e.id > after_id)
            .collect()
    }

    /// Serialize matching entries to a JSON string suitable for regulator export.
    pub fn export_json(&self, filter: &AuditFilter) -> String {
        let entries = self.query(filter);
        serde_json::to_string(&entries).unwrap_or_else(|_| "[]".to_string())
    }
}

impl Default for AuditLog {
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
    use chrono::Duration;
    use uuid::Uuid;

    fn uid() -> UserId {
        Uuid::new_v4()
    }

    fn no_filter() -> AuditFilter {
        AuditFilter {
            user_id: None,
            action: None,
            from: None,
            to: None,
            limit: 0,
        }
    }

    // --- log / count ---

    #[test]
    fn test_log_entry_is_stored() {
        let mut log = AuditLog::new();
        let user = uid();

        log.log(
            Some(user),
            AuditAction::Login,
            serde_json::json!({"device": "desktop"}),
            Some("1.2.3.4".to_string()),
        );

        assert_eq!(log.count(), 1);
        let all = log.query(&no_filter());
        assert_eq!(all[0].action, AuditAction::Login);
        assert_eq!(all[0].user_id, Some(user));
        assert_eq!(all[0].ip_address.as_deref(), Some("1.2.3.4"));
    }

    #[test]
    fn test_log_returns_sequential_ids() {
        let mut log = AuditLog::new();

        let id1 = log.log(None, AuditAction::TradingHalted, Value::Null, None);
        let id2 = log.log(None, AuditAction::TradingResumed, Value::Null, None);
        let id3 = log.log(None, AuditAction::AmlAlertCreated, Value::Null, None);

        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id3, 3);
    }

    #[test]
    fn test_ids_are_sequential_across_multiple_users() {
        let mut log = AuditLog::new();
        let a = uid();
        let b = uid();

        let id1 = log.log(Some(a), AuditAction::Login, Value::Null, None);
        let id2 = log.log(Some(b), AuditAction::Login, Value::Null, None);
        let id3 = log.log(Some(a), AuditAction::OrderPlaced, Value::Null, None);

        assert_eq!(id1 + 1, id2);
        assert_eq!(id2 + 1, id3);
    }

    #[test]
    fn test_system_entry_has_no_user_id() {
        let mut log = AuditLog::new();
        log.log(None, AuditAction::TradingHalted, Value::Null, None);

        let all = log.query(&no_filter());
        assert!(all[0].user_id.is_none());
    }

    // --- query by user ---

    #[test]
    fn test_query_by_user_id() {
        let mut log = AuditLog::new();
        let alice = uid();
        let bob = uid();

        log.log(Some(alice), AuditAction::Login, Value::Null, None);
        log.log(Some(bob), AuditAction::Login, Value::Null, None);
        log.log(Some(alice), AuditAction::OrderPlaced, Value::Null, None);

        let filter = AuditFilter {
            user_id: Some(alice),
            action: None,
            from: None,
            to: None,
            limit: 0,
        };
        let result = log.query(&filter);
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|e| e.user_id == Some(alice)));
    }

    #[test]
    fn test_query_by_user_returns_empty_for_unknown_user() {
        let mut log = AuditLog::new();
        log.log(Some(uid()), AuditAction::Login, Value::Null, None);

        let filter = AuditFilter {
            user_id: Some(uid()), // different user
            action: None,
            from: None,
            to: None,
            limit: 0,
        };
        assert!(log.query(&filter).is_empty());
    }

    // --- query by action ---

    #[test]
    fn test_query_by_action() {
        let mut log = AuditLog::new();
        let user = uid();

        log.log(Some(user), AuditAction::Login, Value::Null, None);
        log.log(Some(user), AuditAction::OrderPlaced, Value::Null, None);
        log.log(Some(user), AuditAction::OrderPlaced, Value::Null, None);

        let filter = AuditFilter {
            user_id: None,
            action: Some(AuditAction::OrderPlaced),
            from: None,
            to: None,
            limit: 0,
        };
        let result = log.query(&filter);
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|e| e.action == AuditAction::OrderPlaced));
    }

    // --- query by time range ---

    #[test]
    fn test_query_by_from_excludes_earlier_entries() {
        let mut log = AuditLog::new();

        // Directly inject an entry with a known past timestamp for determinism.
        log.entries.push(AuditEntry {
            id: 1,
            user_id: None,
            action: AuditAction::Login,
            detail: Value::Null,
            ip_address: None,
            timestamp: Utc::now() - Duration::hours(2),
            prev_hash: "genesis".into(),
            entry_hash: "stub".into(),
        });
        log.entries.push(AuditEntry {
            id: 2,
            user_id: None,
            action: AuditAction::OrderPlaced,
            detail: Value::Null,
            ip_address: None,
            timestamp: Utc::now(),
            prev_hash: "stub".into(),
            entry_hash: "stub2".into(),
        });
        log.next_id = 3;

        let filter = AuditFilter {
            user_id: None,
            action: None,
            from: Some(Utc::now() - Duration::minutes(30)),
            to: None,
            limit: 0,
        };
        let result = log.query(&filter);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].action, AuditAction::OrderPlaced);
    }

    #[test]
    fn test_query_by_to_excludes_later_entries() {
        let mut log = AuditLog::new();

        log.entries.push(AuditEntry {
            id: 1,
            user_id: None,
            action: AuditAction::Login,
            detail: Value::Null,
            ip_address: None,
            timestamp: Utc::now() - Duration::hours(2),
            prev_hash: "genesis".into(),
            entry_hash: "stub".into(),
        });
        log.entries.push(AuditEntry {
            id: 2,
            user_id: None,
            action: AuditAction::OrderPlaced,
            detail: Value::Null,
            ip_address: None,
            timestamp: Utc::now(),
            prev_hash: "stub".into(),
            entry_hash: "stub2".into(),
        });
        log.next_id = 3;

        let filter = AuditFilter {
            user_id: None,
            action: None,
            from: None,
            to: Some(Utc::now() - Duration::hours(1)),
            limit: 0,
        };
        let result = log.query(&filter);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].action, AuditAction::Login);
    }

    #[test]
    fn test_query_limit_caps_results() {
        let mut log = AuditLog::new();
        for _ in 0..10 {
            log.log(None, AuditAction::OrderPlaced, Value::Null, None);
        }

        let filter = AuditFilter {
            user_id: None,
            action: None,
            from: None,
            to: None,
            limit: 3,
        };
        assert_eq!(log.query(&filter).len(), 3);
    }

    // --- entries_since ---

    #[test]
    fn test_entries_since_returns_correct_subset() {
        let mut log = AuditLog::new();

        let cutoff = Utc::now();

        log.entries.push(AuditEntry {
            id: 1,
            user_id: None,
            action: AuditAction::Login,
            detail: Value::Null,
            ip_address: None,
            timestamp: cutoff - Duration::minutes(5),
            prev_hash: "genesis".into(),
            entry_hash: "stub".into(),
        });
        log.entries.push(AuditEntry {
            id: 2,
            user_id: None,
            action: AuditAction::OrderPlaced,
            detail: Value::Null,
            ip_address: None,
            timestamp: cutoff,
            prev_hash: "stub".into(),
            entry_hash: "stub2".into(),
        });
        log.entries.push(AuditEntry {
            id: 3,
            user_id: None,
            action: AuditAction::OrderFilled,
            detail: Value::Null,
            ip_address: None,
            timestamp: cutoff + Duration::minutes(5),
            prev_hash: "stub2".into(),
            entry_hash: "stub3".into(),
        });
        log.next_id = 4;

        let since = log.entries_since(cutoff);
        assert_eq!(since.len(), 2);
        assert_eq!(since[0].id, 2);
        assert_eq!(since[1].id, 3);
    }

    // --- export_json ---

    #[test]
    fn test_export_json_is_valid_json_array() {
        let mut log = AuditLog::new();
        log.log(Some(uid()), AuditAction::Login, serde_json::json!({"ok": true}), None);

        let json = log.export_json(&no_filter());
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("must be valid JSON");
        assert!(parsed.is_array());
        assert_eq!(parsed.as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_export_json_respects_filter() {
        let mut log = AuditLog::new();
        let user = uid();
        log.log(Some(user), AuditAction::Login, Value::Null, None);
        log.log(Some(uid()), AuditAction::Login, Value::Null, None);

        let filter = AuditFilter {
            user_id: Some(user),
            action: None,
            from: None,
            to: None,
            limit: 0,
        };
        let json = log.export_json(&filter);
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 1);
    }

    #[test]
    fn test_export_json_empty_log_returns_empty_array() {
        let log = AuditLog::new();
        let json = log.export_json(&no_filter());
        assert_eq!(json, "[]");
    }

    // --- combined filters ---

    #[test]
    fn test_query_combines_user_and_action_filter() {
        let mut log = AuditLog::new();
        let alice = uid();
        let bob = uid();

        log.log(Some(alice), AuditAction::Login, Value::Null, None);
        log.log(Some(alice), AuditAction::OrderPlaced, Value::Null, None);
        log.log(Some(bob), AuditAction::Login, Value::Null, None);

        let filter = AuditFilter {
            user_id: Some(alice),
            action: Some(AuditAction::Login),
            from: None,
            to: None,
            limit: 0,
        };
        let result = log.query(&filter);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].user_id, Some(alice));
        assert_eq!(result[0].action, AuditAction::Login);
    }

    // --- hash chain integrity ---

    #[test]
    fn test_continuing_from_uses_db_hash_as_genesis() {
        let db_hash = "abc123deadbeef".to_string();
        let mut log = AuditLog::continuing_from(db_hash.clone(), 42);

        let id = log.log(None, AuditAction::TradingHalted, Value::Null, None);
        assert_eq!(id, 42, "next_id should start from the DB value");

        let entry = &log.query(&no_filter())[0];
        assert_eq!(entry.prev_hash, db_hash, "first entry should chain from DB hash");
        assert!(log.verify_integrity());
    }

    #[test]
    fn test_verify_integrity_passes_on_intact_log() {
        let mut log = AuditLog::new();
        log.log(Some(uid()), AuditAction::Login, Value::Null, None);
        log.log(None, AuditAction::TradingHalted, Value::Null, None);
        log.log(Some(uid()), AuditAction::OrderPlaced, Value::Null, None);

        assert!(log.verify_integrity(), "intact log must pass integrity check");
    }

    #[test]
    fn test_verify_integrity_empty_log() {
        let log = AuditLog::new();
        assert!(log.verify_integrity(), "empty log is trivially valid");
    }

    #[test]
    fn test_verify_integrity_fails_when_entry_hash_tampered() {
        let mut log = AuditLog::new();
        log.log(Some(uid()), AuditAction::Login, Value::Null, None);
        log.log(Some(uid()), AuditAction::OrderPlaced, Value::Null, None);

        // Tamper with the first entry's recorded hash.
        log.entries[0].entry_hash = "tampered".to_string();

        assert!(!log.verify_integrity(), "tampered hash must fail integrity check");
    }

    #[test]
    fn test_verify_integrity_fails_when_prev_hash_tampered() {
        let mut log = AuditLog::new();
        log.log(Some(uid()), AuditAction::Login, Value::Null, None);
        log.log(Some(uid()), AuditAction::OrderPlaced, Value::Null, None);

        // Break the chain by altering the second entry's prev_hash.
        log.entries[1].prev_hash = "broken".to_string();

        assert!(!log.verify_integrity(), "broken chain link must fail integrity check");
    }

    // --- entries_since_id ---

    #[test]
    fn test_entries_since_id_returns_only_newer_entries() {
        let mut log = AuditLog::new();
        log.log(Some(uid()), AuditAction::Login, Value::Null, None);         // id = 1
        log.log(Some(uid()), AuditAction::OrderPlaced, Value::Null, None);   // id = 2
        log.log(Some(uid()), AuditAction::OrderFilled, Value::Null, None);   // id = 3

        // after_id = 1 → should return ids 2 and 3
        let result = log.entries_since_id(1);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, 2);
        assert_eq!(result[1].id, 3);
    }

    #[test]
    fn test_entries_since_id_zero_returns_all() {
        let mut log = AuditLog::new();
        log.log(None, AuditAction::TradingHalted, Value::Null, None);
        log.log(None, AuditAction::TradingResumed, Value::Null, None);

        let result = log.entries_since_id(0);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_entries_since_id_past_last_returns_empty() {
        let mut log = AuditLog::new();
        log.log(None, AuditAction::Login, Value::Null, None);  // id = 1

        // after_id = 1 (last id) → nothing newer
        let result = log.entries_since_id(1);
        assert!(result.is_empty());
    }
}
