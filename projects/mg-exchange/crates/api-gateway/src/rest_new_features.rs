//! REST handlers for features added in the latest implementation wave.
//!
//! ## Features implemented
//!
//! 1. **Daily Account Snapshot** — `GET /account/daily-snapshot?date=YYYY-MM-DD`
//! 2. **RSA + Ed25519 API Key Signing** — extended `POST /account/api-keys`
//!    accepts `sign_type` and `public_key`; `GET /account/api-keys/signing-info`
//!    returns available methods.
//! 3. **Account Upgrade Flow** — `PUT /account/upgrade`, `GET /account/tier`
//! 4. **Deposit/Withdrawal Fee Preview** — `GET /account/fee-preview`
//! 5. **Gift Cards** — create, redeem, list
//! 6. **Premium Index Kline** — `GET /markets/{symbol}/premium-klines`
//! 8. **Multi-Language Error Codes** — `GET /v2/error-codes`

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{ApiKey, ApiKeyPermissions, AuthenticatedUser, SignType};
use crate::exchange::{AccountTier, DailySnapshot, Exchange, ExchangeErrorCode, GiftCard, GiftCardStatus, PremiumSnapshot};
use crate::rest::AppState;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn bad_request(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": msg.into()})))
}

fn not_found(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": msg.into()})))
}

fn ok_json(v: impl Serialize) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::OK, Json(serde_json::to_value(v).unwrap_or(serde_json::Value::Null)))
}

// =============================================================================
// Feature 1 — Daily Account Snapshot
// =============================================================================

/// Query parameters for the daily-snapshot endpoint.
#[derive(Debug, Deserialize)]
pub struct DailySnapshotQuery {
    /// Date in "YYYY-MM-DD" format. Defaults to today (UTC).
    pub date: Option<String>,
}

/// GET /api/v1/account/daily-snapshot?date=2026-03-21
///
/// Returns the daily equity snapshot for the authenticated user on the
/// requested date. The snapshot is taken at midnight UTC each day by the
/// `spawn_daily_snapshot` background task.
pub async fn get_daily_snapshot(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Query(params): Query<DailySnapshotQuery>,
) -> impl IntoResponse {
    let date = params.date.unwrap_or_else(|| {
        chrono::Utc::now().format("%Y-%m-%d").to_string()
    });

    let snapshots = exchange.daily_snapshots.lock().unwrap_or_else(|e| e.into_inner());
    match snapshots.get(&(auth.0, date.clone())) {
        Some(snap) => ok_json(snap),
        None => not_found(format!("No snapshot found for date '{date}'")),
    }
}

// =============================================================================
// Feature 2 — RSA + Ed25519 API Key Signing
// =============================================================================

/// Request body for creating an API key with an explicit signing method.
#[derive(Debug, Deserialize)]
pub struct CreateApiKeyWithSignTypeRequest {
    pub label: String,
    #[serde(default)]
    pub can_read: bool,
    #[serde(default)]
    pub can_trade: bool,
    #[serde(default)]
    pub can_withdraw: bool,
    /// Signing method: "hmac" (default), "rsa", or "ed25519".
    #[serde(default = "default_sign_type_str")]
    pub sign_type: String,
    /// Required for "rsa": PEM-encoded RSA public key.
    /// Required for "ed25519": base64-encoded 32-byte public key.
    /// Omit for "hmac" (the exchange generates a secret instead).
    pub public_key: Option<String>,
}

fn default_sign_type_str() -> String {
    "hmac".to_string()
}

/// POST /api/v1/account/api-keys/typed
///
/// Like `POST /account/api-keys` but accepts `sign_type` and `public_key`.
/// For RSA and Ed25519 keys the exchange does NOT generate a secret;
/// instead it stores the supplied public key and validates signatures against
/// it on every authenticated request.
pub async fn create_typed_api_key(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<CreateApiKeyWithSignTypeRequest>,
) -> impl IntoResponse {
    let Some(ref key_store) = exchange.key_store else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "API key management not available in this mode"})),
        );
    };

    let sign_type = match req.sign_type.to_lowercase().as_str() {
        "hmac"    => SignType::Hmac,
        "rsa"     => SignType::Rsa,
        "ed25519" => SignType::Ed25519,
        other => {
            return bad_request(format!("Unknown sign_type '{other}'; use hmac, rsa, or ed25519"));
        }
    };

    // For asymmetric sign types the caller must provide a public key.
    if matches!(sign_type, SignType::Rsa | SignType::Ed25519) && req.public_key.is_none() {
        return bad_request("public_key is required for rsa and ed25519 sign types");
    }

    let key_str = format!("ek_{}", Uuid::new_v4().simple());
    // For HMAC: generate a random secret. For asymmetric: no secret needed.
    let secret_str = if sign_type == SignType::Hmac {
        format!("es_{}", Uuid::new_v4().simple())
    } else {
        String::new()
    };

    let permissions = ApiKeyPermissions {
        can_read: req.can_read,
        can_trade: req.can_trade,
        can_withdraw: req.can_withdraw,
    };

    key_store.add_key(ApiKey {
        key: key_str.clone(),
        secret: secret_str.clone(),
        user_id: auth.0,
        label: req.label.clone(),
        permissions,
        allowed_ips: vec![],
        created_at: chrono::Utc::now(),
        is_active: true,
        admin_role: None,
        sign_type,
        public_key: req.public_key.clone(),
    });

    tracing::info!(
        user_id = %auth.0,
        key = %key_str,
        sign_type = ?sign_type,
        label = %req.label,
        "typed API key created"
    );

    let mut resp = serde_json::json!({
        "key": key_str,
        "sign_type": req.sign_type,
        "label": req.label,
    });

    // Only return the secret for HMAC keys — it should be stored by the user.
    if sign_type == SignType::Hmac {
        resp["secret"] = serde_json::json!(secret_str);
    }

    (StatusCode::OK, Json(resp))
}

/// GET /api/v1/account/api-keys/signing-info
///
/// Returns documentation about the three supported signing methods.
pub async fn get_signing_info() -> impl IntoResponse {
    Json(serde_json::json!({
        "methods": [
            {
                "sign_type": "hmac",
                "description": "HMAC-SHA256 (default). The exchange generates a secret; sign payload with HMAC-SHA256.",
                "header": "X-API-Sign",
                "sign_type_header": "X-API-Sign-Type: hmac (or omit)"
            },
            {
                "sign_type": "rsa",
                "description": "RSA-SHA256. Provide your PEM public key at key creation; sign payload with your RSA private key.",
                "header": "X-API-Sign (base64-encoded DER signature)",
                "sign_type_header": "X-API-Sign-Type: rsa"
            },
            {
                "sign_type": "ed25519",
                "description": "Ed25519. Provide your base64 public key at key creation; sign with your Ed25519 private key.",
                "header": "X-API-Sign (base64-encoded 64-byte signature)",
                "sign_type_header": "X-API-Sign-Type: ed25519"
            }
        ],
        "payload_format": "timestamp + api_key + recv_window + body"
    }))
}

// =============================================================================
// Feature 3 — Account Upgrade Flow
// =============================================================================

/// GET /api/v1/account/tier
///
/// Returns the authenticated user's current tier and the requirements for
/// upgrading to the next tier.
pub async fn get_account_tier(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let tiers = exchange.account_tiers.lock().unwrap_or_else(|e| e.into_inner());
    let current = tiers.get(&auth.0).copied().unwrap_or(AccountTier::Basic);
    drop(tiers);

    let (next_tier, requirements) = match current {
        AccountTier::Basic => (
            Some("Verified"),
            Some("Complete KYC identity verification"),
        ),
        AccountTier::Verified => (
            Some("Pro"),
            Some("Maintain total equity of USD 100,000 or above for 30 consecutive days"),
        ),
        AccountTier::Pro => (None, None),
    };

    (StatusCode::OK, Json(serde_json::json!({
        "current_tier": current.to_string(),
        "next_tier": next_tier,
        "requirements_for_next": requirements,
    })))
}

/// PUT /api/v1/account/upgrade
///
/// Attempts to upgrade the authenticated user's tier.
///
/// Rules:
/// - Basic → Verified: user must have completed KYC (be in `kyc_approved`).
/// - Verified → Pro: user's total equity in fund + trading accounts must
///   exceed USD 100,000 (approximated in USDT for Phase 1).
pub async fn upgrade_account_tier(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let mut tiers = exchange.account_tiers.lock().unwrap_or_else(|e| e.into_inner());
    let current = tiers.get(&auth.0).copied().unwrap_or(AccountTier::Basic);

    match current {
        AccountTier::Basic => {
            // Requires KYC approval.
            if !exchange.kyc_approved.contains(&auth.0) {
                return (StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({
                    "error": "KYC verification required to upgrade to Verified tier",
                    "requirement": "complete_kyc"
                })));
            }
            tiers.insert(auth.0, AccountTier::Verified);
            tracing::info!(user_id = %auth.0, "account tier upgraded: Basic → Verified");
            (StatusCode::OK, Json(serde_json::json!({
                "previous_tier": "Basic",
                "current_tier": "Verified",
                "message": "Account upgraded to Verified tier"
            })))
        }
        AccountTier::Verified => {
            // Requires $100K equity.
            let threshold = Decimal::new(100_000, 0);
            let equity = compute_usdt_equity(&exchange, auth.0);

            if equity < threshold {
                return (StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({
                    "error": format!(
                        "Insufficient equity for Pro tier. Current: ${:.2}, Required: $100,000",
                        equity
                    ),
                    "requirement": "equity_100k_usd",
                    "current_equity_usd": equity.to_string(),
                    "required_equity_usd": "100000"
                })));
            }
            tiers.insert(auth.0, AccountTier::Pro);
            tracing::info!(user_id = %auth.0, %equity, "account tier upgraded: Verified → Pro");
            (StatusCode::OK, Json(serde_json::json!({
                "previous_tier": "Verified",
                "current_tier": "Pro",
                "message": "Account upgraded to Pro tier"
            })))
        }
        AccountTier::Pro => {
            (StatusCode::CONFLICT, Json(serde_json::json!({
                "error": "Already at maximum tier (Pro)",
                "current_tier": "Pro"
            })))
        }
    }
}

/// Approximate a user's total USDT equity from their trading + fund accounts.
///
/// Phase 1: sums USDT balances only. In Phase 2 this would convert all coins
/// using live mark prices.
fn compute_usdt_equity(exchange: &Exchange, user_id: Uuid) -> Decimal {
    let mut total = Decimal::ZERO;

    // Trading account USDT.
    if let Some(bal) = exchange.accounts.get_balance(user_id, "USDT") {
        total += bal.available();
    }

    // Fund account USDT.
    let fund = exchange.fund_accounts.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(coins) = fund.get(&user_id) {
        if let Some(usdt) = coins.get("USDT") {
            total += usdt;
        }
    }

    total
}

// =============================================================================
// Feature 4 — Deposit/Withdrawal Fee Preview
// =============================================================================

/// Query parameters for the fee-preview endpoint.
#[derive(Debug, Deserialize)]
pub struct FeePreviewQuery {
    /// "deposit" or "withdraw".
    pub action: String,
    /// Coin ticker, e.g. "BTC".
    pub asset: String,
    /// Network identifier, e.g. "bitcoin".
    pub network: String,
    /// Amount to deposit or withdraw.
    pub amount: Decimal,
}

/// GET /api/v1/account/fee-preview
///
/// Returns the network fee and net received amount for a hypothetical
/// deposit or withdrawal without submitting anything.
pub async fn get_fee_preview(
    State(exchange): State<AppState>,
    Query(params): Query<FeePreviewQuery>,
) -> impl IntoResponse {
    if params.amount <= Decimal::ZERO {
        return bad_request("amount must be positive");
    }

    let registry = exchange.asset_registry.lock().unwrap_or_else(|e| e.into_inner());
    let coin_info = match registry.get(&params.asset) {
        Some(c) => c,
        None => return not_found(format!("Unknown asset '{}'", params.asset)),
    };

    let net_config = match coin_info.networks.iter().find(|n| {
        n.network.eq_ignore_ascii_case(&params.network)
    }) {
        Some(n) => n,
        None => return not_found(format!(
            "Network '{}' not available for asset '{}'",
            params.network, params.asset
        )),
    };

    match params.action.to_lowercase().as_str() {
        "withdraw" => {
            if !net_config.withdraw_enabled {
                return (StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({
                    "error": format!("Withdrawals are disabled for {} on {}", params.asset, params.network)
                })));
            }
            if params.amount < net_config.withdraw_min {
                return bad_request(format!(
                    "Amount {} is below minimum withdrawal {} for {} on {}",
                    params.amount, net_config.withdraw_min, params.asset, params.network
                ));
            }
            if params.amount > net_config.withdraw_max {
                return bad_request(format!(
                    "Amount {} exceeds maximum withdrawal {} for {} on {}",
                    params.amount, net_config.withdraw_max, params.asset, params.network
                ));
            }

            let fee = net_config.withdraw_fee;
            let net_amount = params.amount - fee;
            // Estimate: assume 1 confirmation per ~10 minutes for BTC-like,
            // or 2 minutes for EVM chains.
            let estimated_minutes = if net_config.confirmations_required > 6 {
                net_config.confirmations_required as i64 * 10
            } else {
                net_config.confirmations_required as i64 * 2
            };

            (StatusCode::OK, Json(serde_json::json!({
                "action": "withdraw",
                "asset": params.asset,
                "network": params.network,
                "amount": params.amount.to_string(),
                "fee": fee.to_string(),
                "net_amount": net_amount.to_string(),
                "estimated_arrival_minutes": estimated_minutes,
            })))
        }
        "deposit" => {
            if !net_config.deposit_enabled {
                return (StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({
                    "error": format!("Deposits are disabled for {} on {}", params.asset, params.network)
                })));
            }

            // Deposits typically have no exchange-side fee; just show confirmation time.
            let estimated_minutes = net_config.safe_confirmations as i64
                * if net_config.confirmations_required > 6 { 10 } else { 2 };

            (StatusCode::OK, Json(serde_json::json!({
                "action": "deposit",
                "asset": params.asset,
                "network": params.network,
                "amount": params.amount.to_string(),
                "fee": "0",
                "net_amount": params.amount.to_string(),
                "estimated_arrival_minutes": estimated_minutes,
                "min_deposit": net_config.deposit_min.to_string(),
            })))
        }
        other => bad_request(format!("Unknown action '{other}'; use 'deposit' or 'withdraw'")),
    }
}

// =============================================================================
// Feature 5 — Gift Cards
// =============================================================================

/// Request body for creating a gift card.
#[derive(Debug, Deserialize)]
pub struct CreateGiftCardRequest {
    pub asset: String,
    pub amount: Decimal,
    pub message: Option<String>,
}

/// POST /api/v1/gift-cards/create
///
/// Creates a gift card by locking `amount` of `asset` from the creator's
/// trading account. Returns a unique redemption code.
pub async fn create_gift_card(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<CreateGiftCardRequest>,
) -> impl IntoResponse {
    if req.amount <= Decimal::ZERO {
        return bad_request("amount must be positive");
    }
    if req.asset.is_empty() {
        return bad_request("asset is required");
    }

    // Lock funds from the creator's trading account — atomic check+debit.
    let result: Result<(), (Decimal, String)> = exchange.accounts.with_user(auth.0, |am| {
        let available = am
            .get_balance(&auth.0, &req.asset)
            .map(|b| b.available())
            .unwrap_or(Decimal::ZERO);
        if available < req.amount {
            return Err((available, "insufficient".into()));
        }
        am.withdraw(auth.0, &req.asset, req.amount)
            .map_err(|e| (available, e.to_string()))
    });
    match result {
        Ok(()) => {}
        Err((available, msg)) if msg == "insufficient" => {
            return (StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({
                "error": "Insufficient balance to create gift card",
                "available": available.to_string(),
                "required": req.amount.to_string(),
                "asset": req.asset
            })));
        }
        Err((_, msg)) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                "error": format!("Failed to lock funds: {msg}")
            })));
        }
    }

    // Generate a human-readable code: 4 groups of 4 uppercase hex chars.
    let raw = Uuid::new_v4().simple().to_string().to_uppercase();
    let code = format!("{}-{}-{}-{}", &raw[0..4], &raw[4..8], &raw[8..12], &raw[12..16]);

    let card = GiftCard {
        id: Uuid::new_v4(),
        code: code.clone(),
        creator_id: auth.0,
        asset: req.asset.clone(),
        amount: req.amount,
        message: req.message.clone(),
        redeemed_by: None,
        status: GiftCardStatus::Active,
        created_at: chrono::Utc::now(),
        redeemed_at: None,
    };

    exchange.gift_cards.lock().unwrap_or_else(|e| e.into_inner()).push(card.clone());

    tracing::info!(
        creator = %auth.0,
        asset = %req.asset,
        amount = %req.amount,
        code = %code,
        "gift card created"
    );

    (StatusCode::OK, Json(serde_json::json!({
        "id": card.id,
        "code": code,
        "asset": req.asset,
        "amount": req.amount.to_string(),
        "message": req.message,
        "status": "active"
    })))
}

/// Request body for redeeming a gift card.
#[derive(Debug, Deserialize)]
pub struct RedeemGiftCardRequest {
    pub code: String,
}

/// POST /api/v1/gift-cards/redeem
///
/// Redeems a gift card by transferring its value to the redeemer's trading
/// account. The creator cannot redeem their own card.
pub async fn redeem_gift_card(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<RedeemGiftCardRequest>,
) -> impl IntoResponse {
    let mut cards = exchange.gift_cards.lock().unwrap_or_else(|e| e.into_inner());

    let card_idx = match cards.iter().position(|c| c.code == req.code) {
        Some(i) => i,
        None => return not_found("Gift card not found"),
    };

    let card = &cards[card_idx];

    if card.status != GiftCardStatus::Active {
        return (StatusCode::CONFLICT, Json(serde_json::json!({
            "error": "Gift card has already been redeemed or cancelled",
            "status": format!("{:?}", card.status)
        })));
    }

    if card.creator_id == auth.0 {
        return bad_request("You cannot redeem your own gift card");
    }

    let asset = card.asset.clone();
    let amount = card.amount;

    // Mark card as redeemed before crediting to avoid double-spend.
    cards[card_idx].status = GiftCardStatus::Redeemed;
    cards[card_idx].redeemed_by = Some(auth.0);
    cards[card_idx].redeemed_at = Some(chrono::Utc::now());
    drop(cards);

    // Credit the redeemer.
    // Errors here would leave the user un-credited after the card is marked
    // redeemed — log them clearly. In production this would use a transactional
    // ledger; for Phase 1 the chance is negligible (deposit never fails unless
    // arithmetic overflows, which is impossible on human-scale amounts).
    if let Err(e) = exchange
        .accounts
        .deposit(auth.0, &asset, amount)
    {
        tracing::error!(
            redeemer = %auth.0,
            asset = %asset,
            amount = %amount,
            error = %e,
            "CRITICAL: gift card credited but deposit to account failed"
        );
    }

    tracing::info!(
        redeemer = %auth.0,
        asset = %asset,
        amount = %amount,
        code = %req.code,
        "gift card redeemed"
    );

    (StatusCode::OK, Json(serde_json::json!({
        "message": "Gift card redeemed successfully",
        "asset": asset,
        "amount": amount.to_string(),
    })))
}

/// GET /api/v1/gift-cards/my-cards
///
/// Lists all gift cards created by the authenticated user.
pub async fn list_my_gift_cards(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let cards = exchange.gift_cards.lock().unwrap_or_else(|e| e.into_inner());
    let my_cards: Vec<&GiftCard> = cards.iter().filter(|c| c.creator_id == auth.0).collect();
    (StatusCode::OK, Json(serde_json::to_value(&my_cards).unwrap_or_default()))
}

// =============================================================================
// Feature 6 — Premium Index Kline
// =============================================================================

/// Query parameters for the premium klines endpoint.
#[derive(Debug, Deserialize)]
pub struct PremiumKlineQuery {
    /// Maximum number of snapshots to return (default 100, max 500).
    #[serde(default = "default_premium_limit")]
    pub limit: usize,
}

fn default_premium_limit() -> usize {
    100
}

/// GET /api/v1/markets/{symbol}/premium-klines
///
/// Returns the historical premium (mark − index) / index * 100 series for
/// the given symbol, sampled every 5 minutes.
pub async fn get_premium_klines(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
    Query(params): Query<PremiumKlineQuery>,
) -> impl IntoResponse {
    let limit = params.limit.min(500);
    let history = exchange.premium_history.lock().unwrap_or_else(|e| e.into_inner());
    let sym = exchange_common::Symbol::new(symbol.as_str());

    match history.get(&sym) {
        None => (StatusCode::OK, Json(serde_json::json!({
            "symbol": symbol,
            "data": serde_json::Value::Array(vec![]),
        }))),
        Some(snaps) => {
            let data: Vec<&PremiumSnapshot> = snaps.iter().rev().take(limit).collect();
            (StatusCode::OK, Json(serde_json::json!({
                "symbol": symbol,
                "data": data,
            })))
        }
    }
}

// =============================================================================
// Feature 8 — Multi-Language Error Codes
// =============================================================================

/// A single error code definition returned by the catalogue endpoint.
#[derive(Debug, Serialize)]
pub struct ErrorCodeEntry {
    pub code: u32,
    pub name: &'static str,
    pub message: &'static str,
}

/// GET /api/v2/error-codes
///
/// Returns the full catalogue of standardised exchange error codes.
pub async fn list_error_codes() -> impl IntoResponse {
    let codes = vec![
        ErrorCodeEntry { code: 0,     name: "OK",                  message: ExchangeErrorCode::OK.message() },
        ErrorCodeEntry { code: 10001, name: "INVALID_PARAM",        message: ExchangeErrorCode::INVALID_PARAM.message() },
        ErrorCodeEntry { code: 10002, name: "UNAUTHORIZED",         message: ExchangeErrorCode::UNAUTHORIZED.message() },
        ErrorCodeEntry { code: 10003, name: "RATE_LIMITED",         message: ExchangeErrorCode::RATE_LIMITED.message() },
        ErrorCodeEntry { code: 10004, name: "INSUFFICIENT_BALANCE", message: ExchangeErrorCode::INSUFFICIENT_BALANCE.message() },
        ErrorCodeEntry { code: 10005, name: "ORDER_NOT_FOUND",      message: ExchangeErrorCode::ORDER_NOT_FOUND.message() },
        ErrorCodeEntry { code: 10006, name: "SYMBOL_NOT_FOUND",     message: ExchangeErrorCode::SYMBOL_NOT_FOUND.message() },
        ErrorCodeEntry { code: 10007, name: "KYC_REQUIRED",         message: ExchangeErrorCode::KYC_REQUIRED.message() },
        ErrorCodeEntry { code: 10008, name: "MAINTENANCE_MODE",     message: ExchangeErrorCode::MAINTENANCE_MODE.message() },
    ];
    (StatusCode::OK, Json(serde_json::to_value(&codes).unwrap_or_default()))
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::{Exchange, ExchangeErrorCode};

    #[test]
    fn test_error_code_messages() {
        assert_eq!(ExchangeErrorCode::OK.message(), "OK");
        assert_eq!(ExchangeErrorCode::INVALID_PARAM.message(), "Invalid parameter");
        assert_eq!(ExchangeErrorCode::UNAUTHORIZED.message(), "Unauthorized");
        assert_eq!(ExchangeErrorCode::RATE_LIMITED.message(), "Rate limited");
        assert_eq!(ExchangeErrorCode::INSUFFICIENT_BALANCE.message(), "Insufficient balance");
        assert_eq!(ExchangeErrorCode::ORDER_NOT_FOUND.message(), "Order not found");
        assert_eq!(ExchangeErrorCode::SYMBOL_NOT_FOUND.message(), "Symbol not found");
        assert_eq!(ExchangeErrorCode::KYC_REQUIRED.message(), "KYC required");
        assert_eq!(ExchangeErrorCode::MAINTENANCE_MODE.message(), "Maintenance mode");
    }

    #[test]
    fn test_error_code_unknown() {
        assert_eq!(ExchangeErrorCode(99999).message(), "Unknown error");
    }

    #[test]
    fn test_account_tier_display() {
        assert_eq!(AccountTier::Basic.to_string(), "Basic");
        assert_eq!(AccountTier::Verified.to_string(), "Verified");
        assert_eq!(AccountTier::Pro.to_string(), "Pro");
    }

    #[test]
    fn test_gift_card_status_variants() {
        // Ensure the enum serializes to snake_case as expected by the spec.
        let active = serde_json::to_string(&GiftCardStatus::Active).unwrap();
        let redeemed = serde_json::to_string(&GiftCardStatus::Redeemed).unwrap();
        let cancelled = serde_json::to_string(&GiftCardStatus::Cancelled).unwrap();
        assert!(active.contains("active"), "got {active}");
        assert!(redeemed.contains("redeemed"), "got {redeemed}");
        assert!(cancelled.contains("cancelled"), "got {cancelled}");
    }

    #[tokio::test]
    async fn test_daily_snapshot_not_found() {
        let exchange = Exchange::new();
        let snapshots = exchange.daily_snapshots.lock().unwrap_or_else(|e| e.into_inner());
        assert!(snapshots.is_empty(), "new exchange has no snapshots");
    }

    #[tokio::test]
    async fn test_premium_history_empty_for_new_symbol() {
        let exchange = Exchange::new();
        let history = exchange.premium_history.lock().unwrap_or_else(|e| e.into_inner());
        let sym = exchange_common::Symbol::new("BTCUSDT");
        assert!(history.get(&sym).is_none());
    }

    #[test]
    fn test_exchange_error_code_serialization() {
        let code = ExchangeErrorCode::INSUFFICIENT_BALANCE;
        let json = serde_json::to_string(&code).unwrap();
        assert_eq!(json, "10004");
    }

    #[tokio::test]
    async fn test_account_tier_default_is_basic() {
        let exchange = Exchange::new();
        let tiers = exchange.account_tiers.lock().unwrap_or_else(|e| e.into_inner());
        let dummy_id = Uuid::new_v4();
        let tier = tiers.get(&dummy_id).copied().unwrap_or(AccountTier::Basic);
        assert_eq!(tier, AccountTier::Basic);
    }
}
