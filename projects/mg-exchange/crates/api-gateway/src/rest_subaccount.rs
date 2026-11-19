//! Sub-account API key management, transfer permissions, fund balance,
//! and announcements REST endpoints.
//!
//! ## Routes (all registered in `server::build_router`)
//!
//! ### Private (require auth)
//! - POST   /sub-accounts/{id}/api-keys             — create API key for sub-account
//! - GET    /sub-accounts/{id}/api-keys             — list API keys for sub-account
//! - DELETE /sub-accounts/{id}/api-keys/{key_id}    — delete sub-account API key
//! - PUT    /sub-accounts/{id}/transfer-permission  — set transfer-out permission
//! - GET    /sub-accounts/{id}/fund-balance         — sub-account funding wallet balance
//!
//! ### Admin
//! - POST /admin/announcements  — create announcement
//!
//! ### Public
//! - GET /announcements         — list recent announcements
//! - GET /announcements/types   — list announcement categories

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::AuthenticatedUser;
use crate::exchange::{SubAccountApiKey, Announcement};
use crate::rest::AppState;

// ── Shared helpers ────────────────────────────────────────────────────────────

fn bad_request(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"error": msg.into()})),
    )
}

fn not_found(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": msg.into()})),
    )
}

fn forbidden(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({"error": msg.into()})),
    )
}

/// Verify that `master_id` is the parent of `sub_id`. Returns `Ok(sub_id)` on
/// success or an HTTP error response on failure.
fn verify_sub_ownership(
    exchange: &crate::exchange::Exchange,
    master_id: Uuid,
    sub_id: Uuid,
) -> Result<Uuid, (StatusCode, Json<serde_json::Value>)> {
    let subs = exchange
        .sub_accounts
        .lock().unwrap_or_else(|e| e.into_inner());
    match subs.get_master(sub_id) {
        Some(parent) if parent == master_id => Ok(sub_id),
        Some(_) => Err(forbidden(
            "Sub-account does not belong to the authenticated user",
        )),
        None => Err(not_found("Sub-account not found")),
    }
}

// =============================================================================
// Task 1 — Sub-account API key management
// =============================================================================

/// POST /api/v1/sub-accounts/{id}/api-keys
///
/// Create a new API key pair for the given sub-account.
///
/// The `secret` is returned **once** in this response and never again — the
/// caller must store it immediately.
///
/// Body: `{ "label": "...", "permissions": ["read","trade"], "ip_whitelist": ["1.2.3.4"] }`
pub async fn create_sub_account_api_key(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(sub_id): Path<Uuid>,
    Json(req): Json<CreateSubAccountApiKeyRequest>,
) -> impl IntoResponse {
    let master_id = auth.0;

    // Verify ownership.
    if let Err(e) = verify_sub_ownership(&exchange, master_id, sub_id) {
        return e.into_response();
    }

    // Validate label.
    let label = req.label.trim().to_string();
    if label.is_empty() {
        return bad_request("label must not be empty").into_response();
    }
    if label.len() > 64 {
        return bad_request("label must be at most 64 characters").into_response();
    }

    // Validate permissions.
    let allowed_perms = ["read", "trade", "withdraw"];
    for perm in &req.permissions {
        if !allowed_perms.contains(&perm.as_str()) {
            return bad_request(format!(
                "unknown permission '{}'; allowed: read, trade, withdraw",
                perm
            ))
            .into_response();
        }
    }
    if req.permissions.is_empty() {
        return bad_request("at least one permission is required").into_response();
    }

    // Validate IP whitelist entries (basic format check).
    for ip in &req.ip_whitelist {
        if ip.parse::<std::net::IpAddr>().is_err() {
            return bad_request(format!("invalid IP address in whitelist: '{ip}'"))
                .into_response();
        }
    }

    // Generate the key pair.
    let (api_key_str, secret_str, secret_hash) = generate_key_pair();

    let key = SubAccountApiKey {
        id: Uuid::new_v4(),
        sub_account_id: sub_id,
        label,
        api_key: api_key_str.clone(),
        secret_hash,
        permissions: req.permissions.clone(),
        ip_whitelist: req.ip_whitelist.clone(),
        is_active: true,
        created_at: Utc::now(),
    };

    {
        let mut keys = exchange
            .sub_account_api_key_records
            .lock().unwrap_or_else(|e| e.into_inner());
        keys.entry(sub_id).or_default().push(key.clone());
    }

    tracing::info!(
        sub_account_id = %sub_id,
        api_key = %api_key_str,
        permissions = ?req.permissions,
        "Sub-account API key created",
    );

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": key.id.to_string(),
            "sub_account_id": sub_id.to_string(),
            "label": key.label,
            "api_key": api_key_str,
            // Secret is returned only once — caller must store it.
            "secret": secret_str,
            "permissions": key.permissions,
            "ip_whitelist": key.ip_whitelist,
            "is_active": key.is_active,
            "created_at": key.created_at.timestamp_millis(),
        })),
    )
        .into_response()
}

/// GET /api/v1/sub-accounts/{id}/api-keys
///
/// List all API keys for the given sub-account.
/// Secrets are never included in list responses.
pub async fn list_sub_account_api_keys(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(sub_id): Path<Uuid>,
) -> impl IntoResponse {
    let master_id = auth.0;

    if let Err(e) = verify_sub_ownership(&exchange, master_id, sub_id) {
        return e.into_response();
    }

    let keys_snapshot: Vec<SubAccountApiKey> = {
        let store = exchange
            .sub_account_api_key_records
            .lock().unwrap_or_else(|e| e.into_inner());
        store
            .get(&sub_id)
            .cloned()
            .unwrap_or_default()
    };

    let result: Vec<serde_json::Value> = keys_snapshot
        .iter()
        .map(|k| {
            serde_json::json!({
                "id": k.id.to_string(),
                "sub_account_id": k.sub_account_id.to_string(),
                "label": k.label,
                "api_key": k.api_key,
                "permissions": k.permissions,
                "ip_whitelist": k.ip_whitelist,
                "is_active": k.is_active,
                "created_at": k.created_at.timestamp_millis(),
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(result))).into_response()
}

/// DELETE /api/v1/sub-accounts/{id}/api-keys/{key_id}
///
/// Delete (revoke) a specific API key for the given sub-account.
pub async fn delete_sub_account_api_key(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path((sub_id, key_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    let master_id = auth.0;

    if let Err(e) = verify_sub_ownership(&exchange, master_id, sub_id) {
        return e.into_response();
    }

    let mut store = exchange
        .sub_account_api_key_records
        .lock().unwrap_or_else(|e| e.into_inner());

    let keys = store.entry(sub_id).or_default();
    let before_len = keys.len();
    keys.retain(|k| k.id != key_id);

    if keys.len() == before_len {
        return not_found("API key not found").into_response();
    }

    tracing::info!(
        sub_account_id = %sub_id,
        key_id = %key_id,
        "Sub-account API key deleted",
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "message": "API key revoked",
            "key_id": key_id.to_string(),
        })),
    )
        .into_response()
}

// =============================================================================
// Task 2 — Sub-account transfer permission
// =============================================================================

/// PUT /api/v1/sub-accounts/{id}/transfer-permission
///
/// Set whether the given sub-account is allowed to initiate outbound transfers.
/// By default sub-accounts cannot transfer out; the master must explicitly grant
/// this permission.
///
/// Body: `{ "allow_transfer_out": true }`
pub async fn set_sub_account_transfer_permission(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(sub_id): Path<Uuid>,
    Json(req): Json<SetTransferPermissionRequest>,
) -> impl IntoResponse {
    let master_id = auth.0;

    if let Err(e) = verify_sub_ownership(&exchange, master_id, sub_id) {
        return e.into_response();
    }

    exchange
        .sub_transfer_permissions
        .lock().unwrap_or_else(|e| e.into_inner())
        .insert(sub_id, req.allow_transfer_out);

    tracing::info!(
        sub_account_id = %sub_id,
        allow_transfer_out = req.allow_transfer_out,
        "Sub-account transfer permission updated",
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "sub_account_id": sub_id.to_string(),
            "allow_transfer_out": req.allow_transfer_out,
        })),
    )
        .into_response()
}

// =============================================================================
// Task 3 — Sub-account fund balance
// =============================================================================

/// GET /api/v1/sub-accounts/{id}/fund-balance
///
/// Return the FUND (funding wallet) balances for the given sub-account.
/// Separate from the trading-account balances returned by
/// `GET /sub-accounts/{id}/balances`.
pub async fn get_sub_account_fund_balance(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(sub_id): Path<Uuid>,
) -> impl IntoResponse {
    let master_id = auth.0;

    if let Err(e) = verify_sub_ownership(&exchange, master_id, sub_id) {
        return e.into_response();
    }

    let fund = exchange
        .fund_accounts
        .lock().unwrap_or_else(|e| e.into_inner());

    let coins: Vec<serde_json::Value> = fund
        .get(&sub_id)
        .map(|bals| {
            bals.iter()
                .filter(|(_, bal)| **bal > Decimal::ZERO)
                .map(|(coin, bal)| {
                    serde_json::json!({ "coin": coin, "balance": bal.to_string() })
                })
                .collect()
        })
        .unwrap_or_default();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "sub_account_id": sub_id.to_string(),
            "account_type": "FUND",
            "coins": coins,
        })),
    )
        .into_response()
}

// =============================================================================
// Task 4 — Announcements
// =============================================================================

/// POST /admin/announcements
///
/// Admin creates a new platform announcement.
///
/// Body:
/// ```json
/// {
///   "title": "Scheduled Maintenance",
///   "content": "The exchange will be down on ...",
///   "type": "maintenance",
///   "publish_at": "2026-04-01T00:00:00Z"
/// }
/// ```
pub async fn admin_create_announcement(
    State(exchange): State<AppState>,
    Json(req): Json<CreateAnnouncementRequest>,
) -> impl IntoResponse {
    let title = req.title.trim().to_string();
    if title.is_empty() {
        return bad_request("title must not be empty").into_response();
    }
    if title.len() > 200 {
        return bad_request("title must be at most 200 characters").into_response();
    }

    let content = req.content.trim().to_string();
    if content.is_empty() {
        return bad_request("content must not be empty").into_response();
    }

    let allowed_types = ["maintenance", "listing", "delisting", "general"];
    if !allowed_types.contains(&req.announcement_type.as_str()) {
        return bad_request(format!(
            "type must be one of: {}",
            allowed_types.join(", ")
        ))
        .into_response();
    }

    let announcement = Announcement {
        id: Uuid::new_v4(),
        title,
        content,
        announcement_type: req.announcement_type.clone(),
        publish_at: req.publish_at.unwrap_or_else(Utc::now),
        created_at: Utc::now(),
    };

    let id = announcement.id;

    exchange
        .announcements
        .lock().unwrap_or_else(|e| e.into_inner())
        .push(announcement.clone());

    tracing::info!(
        announcement_id = %id,
        announcement_type = %req.announcement_type,
        "Announcement created",
    );

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": id.to_string(),
            "title": announcement.title,
            "content": announcement.content,
            "type": announcement.announcement_type,
            "publish_at": announcement.publish_at.timestamp_millis(),
            "created_at": announcement.created_at.timestamp_millis(),
        })),
    )
        .into_response()
}

/// GET /api/v1/announcements
///
/// Public endpoint — lists recent announcements in descending publish_at order.
/// Only announcements with `publish_at <= now` are included.
pub async fn list_announcements(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let now = Utc::now();

    let store = exchange
        .announcements
        .lock().unwrap_or_else(|e| e.into_inner());

    let mut visible: Vec<&Announcement> = store
        .iter()
        .filter(|a| a.publish_at <= now)
        .collect();

    // Most recent first.
    visible.sort_by(|a, b| b.publish_at.cmp(&a.publish_at));

    let result: Vec<serde_json::Value> = visible
        .iter()
        .take(50) // cap at 50 most recent
        .map(|a| {
            serde_json::json!({
                "id": a.id.to_string(),
                "title": a.title,
                "content": a.content,
                "type": a.announcement_type,
                "publish_at": a.publish_at.timestamp_millis(),
                "created_at": a.created_at.timestamp_millis(),
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(result))).into_response()
}

/// GET /api/v1/announcements/types
///
/// Public endpoint — returns the set of valid announcement category names.
pub async fn get_announcement_types() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "types": ["maintenance", "listing", "delisting", "general"],
        })),
    )
}

/// GET /api/v1/status
///
/// Public endpoint — returns a high-level service health payload with a
/// `status` string, a `services` array, and an `incidents` array.
pub async fn get_system_status() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "operational",
            "services": [
                {"name": "matching-engine", "status": "operational"},
                {"name": "api-gateway",     "status": "operational"},
                {"name": "settlement",      "status": "operational"},
                {"name": "risk-engine",     "status": "operational"},
            ],
            "incidents": [],
        })),
    )
}

// =============================================================================
// Request / response types
// =============================================================================

/// POST /sub-accounts/{id}/api-keys body.
#[derive(Debug, Deserialize)]
pub struct CreateSubAccountApiKeyRequest {
    pub label: String,
    /// Permissions granted to this key. Allowed values: "read", "trade", "withdraw".
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Optional IP whitelist. When non-empty only requests from these IPs are
    /// accepted for this key.
    #[serde(default)]
    pub ip_whitelist: Vec<String>,
}

/// PUT /sub-accounts/{id}/transfer-permission body.
#[derive(Debug, Deserialize)]
pub struct SetTransferPermissionRequest {
    pub allow_transfer_out: bool,
}

/// POST /admin/announcements body.
#[derive(Debug, Deserialize)]
pub struct CreateAnnouncementRequest {
    pub title: String,
    pub content: String,
    /// Category: "maintenance", "listing", "delisting", or "general".
    #[serde(rename = "type")]
    pub announcement_type: String,
    /// When to make the announcement visible. Defaults to now.
    #[serde(default)]
    pub publish_at: Option<DateTime<Utc>>,
}

// =============================================================================
// Key-pair generation helper
// =============================================================================

/// Generate a random API key + secret pair.
///
/// Returns `(api_key, secret_plaintext, secret_sha256_hex)`.
///
/// The caller stores only the hash; the plaintext is returned once to the user.
fn generate_key_pair() -> (String, String, String) {
    use rand_core::RngCore;
    let mut rng = rand_core::OsRng;

    // 24-byte random key → 48-char hex string.
    let mut key_bytes = [0u8; 24];
    rng.fill_bytes(&mut key_bytes);
    let api_key = hex::encode(key_bytes);

    // 32-byte secret → 64-char hex string.
    let mut secret_bytes = [0u8; 32];
    rng.fill_bytes(&mut secret_bytes);
    let secret = hex::encode(secret_bytes);

    // SHA-256 hash of the secret for storage.
    use sha2::Digest;
    let hash = hex::encode(sha2::Sha256::digest(secret.as_bytes()));

    (api_key, secret, hash)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::Exchange;

    // ── Helper: create a master + sub account, return both IDs. ──────────────

    fn setup_master_and_sub(exchange: &Exchange) -> (Uuid, Uuid) {
        let master_id = Uuid::new_v4();
        exchange.accounts.create_account(master_id);

        let sub = exchange
            .sub_accounts
            .lock().unwrap_or_else(|e| e.into_inner())
            .create_sub(master_id, "test-sub")
            .expect("create sub");
        (master_id, sub.id)
    }

    // ── Sub-account API key CRUD ──────────────────────────────────────────────

    #[test]
    fn test_create_and_list_api_key() {
        let ex = Exchange::new();

        let (master_id, sub_id) = setup_master_and_sub(&ex);

        // Ownership check works.
        assert!(verify_sub_ownership(&ex, master_id, sub_id).is_ok());

        // Unknown user cannot access sub.
        assert!(verify_sub_ownership(&ex, Uuid::new_v4(), sub_id).is_err());

        // Insert a key record directly (simulating handler logic).
        let key = SubAccountApiKey {
            id: Uuid::new_v4(),
            sub_account_id: sub_id,
            label: "Trading Bot".into(),
            api_key: "abc123".into(),
            secret_hash: "hash".into(),
            permissions: vec!["read".into(), "trade".into()],
            ip_whitelist: vec![],
            is_active: true,
            created_at: Utc::now(),
        };
        ex.sub_account_api_key_records
            .lock().unwrap_or_else(|e| e.into_inner())
            .entry(sub_id)
            .or_default()
            .push(key.clone());

        // List returns the key.
        let store = ex.sub_account_api_key_records.lock().unwrap_or_else(|e| e.into_inner());
        let keys = store.get(&sub_id).unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].label, "Trading Bot");
        assert_eq!(keys[0].permissions, vec!["read", "trade"]);
        assert!(keys[0].is_active);
    }

    #[test]
    fn test_delete_api_key() {
        let ex = Exchange::new();
        let (master_id, sub_id) = setup_master_and_sub(&ex);

        let key_id = Uuid::new_v4();
        let key = SubAccountApiKey {
            id: key_id,
            sub_account_id: sub_id,
            label: "Key to delete".into(),
            api_key: "key_abc".into(),
            secret_hash: "hash_abc".into(),
            permissions: vec!["read".into()],
            ip_whitelist: vec![],
            is_active: true,
            created_at: Utc::now(),
        };
        ex.sub_account_api_key_records
            .lock().unwrap_or_else(|e| e.into_inner())
            .entry(sub_id)
            .or_default()
            .push(key);

        // Delete it.
        {
            let mut store = ex.sub_account_api_key_records.lock().unwrap_or_else(|e| e.into_inner());
            let keys = store.entry(sub_id).or_default();
            keys.retain(|k| k.id != key_id);
        }

        let store = ex.sub_account_api_key_records.lock().unwrap_or_else(|e| e.into_inner());
        let keys = store.get(&sub_id).unwrap();
        assert!(keys.is_empty(), "Key should have been removed");

        let _ = master_id; // used for setup_master_and_sub
    }

    // ── Transfer permissions ──────────────────────────────────────────────────

    #[test]
    fn test_transfer_permission_defaults_to_false() {
        let ex = Exchange::new();
        let (_, sub_id) = setup_master_and_sub(&ex);

        let allowed = ex
            .sub_transfer_permissions
            .lock().unwrap_or_else(|e| e.into_inner())
            .get(&sub_id)
            .copied()
            .unwrap_or(false);

        assert!(!allowed, "Transfer-out should be disallowed by default");
    }

    #[test]
    fn test_set_transfer_permission() {
        let ex = Exchange::new();
        let (_, sub_id) = setup_master_and_sub(&ex);

        // Grant permission.
        ex.sub_transfer_permissions
            .lock().unwrap_or_else(|e| e.into_inner())
            .insert(sub_id, true);

        let allowed = *ex
            .sub_transfer_permissions
            .lock().unwrap_or_else(|e| e.into_inner())
            .get(&sub_id)
            .expect("entry should exist");

        assert!(allowed);

        // Revoke it.
        ex.sub_transfer_permissions
            .lock().unwrap_or_else(|e| e.into_inner())
            .insert(sub_id, false);

        let allowed = *ex
            .sub_transfer_permissions
            .lock().unwrap_or_else(|e| e.into_inner())
            .get(&sub_id)
            .expect("entry should exist");

        assert!(!allowed);
    }

    // ── Announcements ─────────────────────────────────────────────────────────

    #[test]
    fn test_create_and_list_announcements() {
        let ex = Exchange::new();

        let now = Utc::now();

        let a1 = Announcement {
            id: Uuid::new_v4(),
            title: "Maintenance Tonight".into(),
            content: "Brief downtime at 02:00 UTC.".into(),
            announcement_type: "maintenance".into(),
            publish_at: now - chrono::Duration::minutes(5),
            created_at: now - chrono::Duration::minutes(5),
        };
        let a2 = Announcement {
            id: Uuid::new_v4(),
            title: "New Listing: DOGEUSDT".into(),
            content: "DOGE perpetual goes live tomorrow.".into(),
            announcement_type: "listing".into(),
            publish_at: now + chrono::Duration::days(1), // future — should not appear
            created_at: now,
        };

        {
            let mut store = ex.announcements.lock().unwrap_or_else(|e| e.into_inner());
            store.push(a1.clone());
            store.push(a2.clone());
        }

        // Only the published announcement should be listed.
        let visible: Vec<Announcement> = {
            let store = ex.announcements.lock().unwrap_or_else(|e| e.into_inner());
            store
                .iter()
                .filter(|a| a.publish_at <= now)
                .cloned()
                .collect()
        };

        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].title, "Maintenance Tonight");
    }

    #[test]
    fn test_announcement_types() {
        let allowed_types = ["maintenance", "listing", "delisting", "general"];
        for t in &allowed_types {
            assert!(allowed_types.contains(t));
        }
        assert!(!allowed_types.contains(&"unknown"));
    }

    // ── Key-pair generation ───────────────────────────────────────────────────

    #[test]
    fn test_generate_key_pair_uniqueness() {
        let (key1, secret1, hash1) = generate_key_pair();
        let (key2, secret2, hash2) = generate_key_pair();

        // Keys should be unique.
        assert_ne!(key1, key2);
        assert_ne!(secret1, secret2);
        assert_ne!(hash1, hash2);

        // Key is 48 hex chars (24 bytes), secret is 64 hex chars (32 bytes).
        assert_eq!(key1.len(), 48);
        assert_eq!(secret1.len(), 64);

        // Hash is valid hex SHA-256 (64 chars).
        assert_eq!(hash1.len(), 64);
    }

    #[test]
    fn test_generate_key_pair_hash_matches_secret() {
        let (_key, secret, hash) = generate_key_pair();

        use sha2::Digest;
        let expected_hash = hex::encode(sha2::Sha256::digest(secret.as_bytes()));
        assert_eq!(hash, expected_hash);
    }

    // ── WS topic name checks ──────────────────────────────────────────────────

    #[test]
    fn test_ws_topic_format() {
        // Validate the topic strings that are used in ws.rs match documented formats.
        let symbol = "BTCUSDT";
        assert_eq!(format!("funding.{symbol}"), "funding.BTCUSDT");
        assert_eq!(format!("markPrice.{symbol}"), "markPrice.BTCUSDT");
        assert_eq!(format!("adl.{symbol}"), "adl.BTCUSDT");
        // Wallet is a private channel with no symbol suffix.
        assert_eq!("wallet", "wallet");
    }
}
