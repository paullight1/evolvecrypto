//! Web3 wallet REST handlers.
//!
//! Endpoints:
//!   POST /api/v1/web3/wallet/create    — create Web3 wallet
//!   GET  /api/v1/web3/wallet           — get wallet info + addresses
//!   GET  /api/v1/web3/balances         — token balances across all chains
//!   GET  /api/v1/web3/bridge/routes    — available bridge routes
//!   GET  /api/v1/web3/bridge/quote     — get bridge quote
//!   POST /api/v1/web3/bridge/execute   — execute bridge transfer
//!   POST /api/v1/web3/dapp/connect     — create dApp session
//!   GET  /api/v1/web3/dapp/sessions    — list active dApp sessions
//!   DELETE /api/v1/web3/dapp/sessions/{id} — revoke session
//!   POST /api/v1/web3/send             — send tokens on-chain
//!   GET  /api/v1/web3/transactions     — transaction history

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use rust_decimal::Decimal;
use serde::Deserialize;

use exchange_web3_wallet::{
    all_balances, total_value_usd, BridgeManager, Chain, DAppPermission, TransactionBuilder,
};

use crate::auth::AuthenticatedUser;
use crate::rest::AppState;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn bad_request(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": msg.into()})))
}

fn not_found(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": msg.into()})))
}

fn ok(body: serde_json::Value) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::OK, Json(body))
}

fn parse_chain(s: &str) -> Result<Chain, String> {
    Chain::from_label(s)
        .ok_or_else(|| format!("unknown chain '{s}'; see GET /api/v1/dex/chains"))
}

fn parse_permissions(raw: &[String]) -> Result<Vec<DAppPermission>, String> {
    raw.iter()
        .map(|s| match s.to_lowercase().as_str() {
            "view_address"      => Ok(DAppPermission::ViewAddress),
            "sign_transaction"  => Ok(DAppPermission::SignTransaction),
            "sign_message"      => Ok(DAppPermission::SignMessage),
            other => Err(format!("unknown permission '{other}'; valid: view_address, sign_transaction, sign_message")),
        })
        .collect()
}

// ── POST /api/v1/web3/wallet/create ──────────────────────────────────────────

/// POST /api/v1/web3/wallet/create
///
/// Creates a new MPC Web3 wallet for the authenticated user.
/// Returns 409 Conflict if the user already has a wallet.
pub async fn create_web3_wallet(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let mut mgr = exchange.web3_wallets.lock().unwrap_or_else(|e| e.into_inner());

    match mgr.create_wallet(user_id) {
        Ok(wallet) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "wallet_id":     wallet.wallet_id,
                "user_id":       wallet.user_id,
                "addresses":     wallet.addresses
                    .iter()
                    .map(|(chain, addr)| (chain.label().to_string(), addr.clone()))
                    .collect::<std::collections::HashMap<_,_>>(),
                "backup_status": wallet.backup_status,
                "created_at":    wallet.created_at,
            })),
        ),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("already has") {
                (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({"error": msg})),
                )
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": msg})),
                )
            }
        }
    }
}

// ── GET /api/v1/web3/wallet ───────────────────────────────────────────────────

/// GET /api/v1/web3/wallet
///
/// Returns wallet info including all chain addresses.
pub async fn get_web3_wallet(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let mgr = exchange.web3_wallets.lock().unwrap_or_else(|e| e.into_inner());

    match mgr.get_wallet(user_id) {
        Some(wallet) => ok(serde_json::json!({
            "wallet_id":     wallet.wallet_id,
            "user_id":       wallet.user_id,
            "addresses":     wallet.addresses
                .iter()
                .map(|(chain, addr)| (chain.label().to_string(), addr.clone()))
                .collect::<std::collections::HashMap<_,_>>(),
            "backup_status": wallet.backup_status,
            "created_at":    wallet.created_at,
        })),
        None => not_found("no Web3 wallet found — call POST /api/v1/web3/wallet/create"),
    }
}

// ── GET /api/v1/web3/balances ─────────────────────────────────────────────────

/// GET /api/v1/web3/balances
///
/// Returns token balances across all chains for the user's wallet.
pub async fn get_web3_balances(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let mgr = exchange.web3_wallets.lock().unwrap_or_else(|e| e.into_inner());

    let Some(wallet) = mgr.get_wallet(user_id) else {
        return not_found("no Web3 wallet found");
    };

    let balances = all_balances(user_id, &wallet.addresses);
    let total_usd = total_value_usd(&balances);

    ok(serde_json::json!({
        "total_value_usd": total_usd,
        "balances": balances,
    }))
}

// ── GET /api/v1/web3/bridge/routes ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct BridgeRoutesParams {
    pub from: String,
    pub to: String,
    pub token: String,
}

/// GET /api/v1/web3/bridge/routes?from=ethereum&to=arbitrum&token=ETH
///
/// Returns available bridge routes for the given from/to/token combination.
pub async fn get_bridge_routes(
    _state: State<AppState>,
    Query(params): Query<BridgeRoutesParams>,
) -> impl IntoResponse {
    let from = match parse_chain(&params.from) {
        Ok(c) => c,
        Err(e) => return bad_request(e),
    };
    let to = match parse_chain(&params.to) {
        Ok(c) => c,
        Err(e) => return bad_request(e),
    };

    if from == to {
        return bad_request("from and to chains must be different");
    }

    let mgr = BridgeManager::new();
    let routes: Vec<serde_json::Value> = mgr
        .get_routes(from, to, &params.token)
        .into_iter()
        .map(|r| serde_json::json!({
            "from_chain":          r.from_chain.label(),
            "to_chain":            r.to_chain.label(),
            "token":               r.token,
            "provider":            r.provider.label(),
            "estimated_time_secs": r.estimated_time_secs,
            "fee_usd":             r.fee_usd,
        }))
        .collect();

    ok(serde_json::json!({ "routes": routes }))
}

// ── GET /api/v1/web3/bridge/quote ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct BridgeQuoteParams {
    pub from: String,
    pub to: String,
    pub token: String,
    pub amount: Decimal,
}

/// GET /api/v1/web3/bridge/quote?from=ethereum&to=arbitrum&token=ETH&amount=1.5
///
/// Returns the best bridge quote (lowest fee) for the given transfer.
pub async fn get_bridge_quote(
    _state: State<AppState>,
    Query(params): Query<BridgeQuoteParams>,
) -> impl IntoResponse {
    if params.amount <= Decimal::ZERO {
        return bad_request("amount must be positive");
    }

    let from = match parse_chain(&params.from) {
        Ok(c) => c,
        Err(e) => return bad_request(e),
    };
    let to = match parse_chain(&params.to) {
        Ok(c) => c,
        Err(e) => return bad_request(e),
    };

    if from == to {
        return bad_request("from and to chains must be different");
    }

    let mgr = BridgeManager::new();
    match mgr.get_quote(from, to, &params.token, params.amount) {
        Ok(quote) => ok(serde_json::json!({
            "quote_id":            quote.id,
            "from_chain":          quote.route.from_chain.label(),
            "to_chain":            quote.route.to_chain.label(),
            "token":               quote.route.token,
            "provider":            quote.route.provider.label(),
            "amount_in":           quote.amount_in,
            "amount_out":          quote.amount_out,
            "fee":                 quote.fee,
            "estimated_time_secs": quote.estimated_time_secs,
            "expires_at":          quote.expires_at,
        })),
        Err(e) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

// ── POST /api/v1/web3/bridge/execute ─────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct BridgeExecuteRequest {
    pub from: String,
    pub to: String,
    pub token: String,
    pub amount: Decimal,
}

/// POST /api/v1/web3/bridge/execute
///
/// Executes a bridge transfer for the authenticated user.
pub async fn execute_bridge(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<BridgeExecuteRequest>,
) -> impl IntoResponse {
    if req.amount <= Decimal::ZERO {
        return bad_request("amount must be positive");
    }

    let from = match parse_chain(&req.from) {
        Ok(c) => c,
        Err(e) => return bad_request(e),
    };
    let to = match parse_chain(&req.to) {
        Ok(c) => c,
        Err(e) => return bad_request(e),
    };

    if from == to {
        return bad_request("from and to chains must be different");
    }

    let user_id = auth.0;

    // Fetch addresses from the user's wallet.
    let (from_address, to_address) = {
        let mgr = exchange.web3_wallets.lock().unwrap_or_else(|e| e.into_inner());
        let wallet = match mgr.get_wallet(user_id) {
            Some(w) => w,
            None => return not_found("no Web3 wallet found"),
        };
        let from_addr = wallet.addresses.get(&from).cloned().unwrap_or_default();
        let to_addr = wallet.addresses.get(&to).cloned().unwrap_or_default();
        (from_addr, to_addr)
    };

    let bridge_mgr = BridgeManager::new();
    let quote = match bridge_mgr.get_quote(from, to, &req.token, req.amount) {
        Ok(q) => q,
        Err(e) => return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    };

    match bridge_mgr.execute_bridge(&quote, user_id, &from_address, &to_address).await {
        Ok(tx) => {
            let mut history = exchange.web3_tx_history.lock().unwrap_or_else(|e| e.into_inner());
            history.entry(user_id).or_default().push(tx.clone());
            ok(serde_json::json!({
                "tx_id":          tx.id,
                "chain":          tx.chain.label(),
                "tx_type":        tx.tx_type,
                "from":           tx.from,
                "to":             tx.to,
                "amount":         tx.amount,
                "token":          tx.token,
                "gas_usd":        tx.gas_estimate_usd,
                "status":         tx.status,
                "tx_hash":        tx.tx_hash,
                "created_at":     tx.created_at,
            }))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

// ── POST /api/v1/web3/dapp/connect ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct DAppConnectRequest {
    pub dapp_url: String,
    pub dapp_name: String,
    pub chain: String,
    /// Requested permissions: "view_address", "sign_transaction", "sign_message".
    #[serde(default)]
    pub permissions: Vec<String>,
}

/// POST /api/v1/web3/dapp/connect
///
/// Creates a pending dApp session. The session must be approved before the
/// dApp can exercise its requested permissions.
pub async fn dapp_connect(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<DAppConnectRequest>,
) -> impl IntoResponse {
    if req.dapp_url.is_empty() {
        return bad_request("dapp_url must not be empty");
    }

    let chain = match parse_chain(&req.chain) {
        Ok(c) => c,
        Err(e) => return bad_request(e),
    };

    let permissions = match parse_permissions(&req.permissions) {
        Ok(p) => p,
        Err(e) => return bad_request(e),
    };

    let user_id = auth.0;
    let mut connector = exchange.dapp_connector.lock().unwrap_or_else(|e| e.into_inner());

    match connector.create_session(user_id, &req.dapp_url, &req.dapp_name, chain, permissions) {
        Ok(session) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "session_id":  session.session_id,
                "dapp_name":   session.dapp_name,
                "dapp_url":    session.dapp_url,
                "chain":       session.chain.label(),
                "permissions": session.permissions,
                "is_approved": session.is_approved,
                "expires_at":  session.expires_at,
            })),
        ),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("already exists") {
                (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({"error": msg})),
                )
            } else {
                bad_request(msg)
            }
        }
    }
}

// ── POST /api/v1/web3/dapp/sessions/{id}/approve ─────────────────────────────

/// POST /api/v1/web3/dapp/sessions/{id}/approve
///
/// Approves a pending dApp session, granting it the requested permissions.
pub async fn dapp_approve_session(
    State(exchange): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let mut connector = exchange.dapp_connector.lock().unwrap_or_else(|e| e.into_inner());
    match connector.approve_session(&session_id) {
        Ok(()) => ok(serde_json::json!({"status": "approved", "session_id": session_id})),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("not found") {
                not_found(msg)
            } else {
                bad_request(msg)
            }
        }
    }
}

// ── GET /api/v1/web3/dapp/sessions ───────────────────────────────────────────

/// GET /api/v1/web3/dapp/sessions
///
/// Returns all active (approved, non-expired) dApp sessions for the user.
pub async fn list_dapp_sessions(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let connector = exchange.dapp_connector.lock().unwrap_or_else(|e| e.into_inner());

    let sessions: Vec<serde_json::Value> = connector
        .list_sessions(user_id)
        .into_iter()
        .map(|s| serde_json::json!({
            "session_id":  s.session_id,
            "dapp_name":   s.dapp_name,
            "dapp_url":    s.dapp_url,
            "chain":       s.chain.label(),
            "permissions": s.permissions,
            "connected_at":s.connected_at,
            "expires_at":  s.expires_at,
        }))
        .collect();

    ok(serde_json::json!({ "sessions": sessions }))
}

// ── DELETE /api/v1/web3/dapp/sessions/{id} ───────────────────────────────────

/// DELETE /api/v1/web3/dapp/sessions/{id}
///
/// Revokes an active dApp session.
pub async fn revoke_dapp_session(
    State(exchange): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let mut connector = exchange.dapp_connector.lock().unwrap_or_else(|e| e.into_inner());
    match connector.revoke_session(&session_id) {
        Ok(()) => ok(serde_json::json!({"status": "revoked", "session_id": session_id})),
        Err(e) => not_found(e.to_string()),
    }
}

// ── POST /api/v1/web3/send ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct Web3SendRequest {
    pub chain: String,
    pub to: String,
    pub token: String,
    pub amount: Decimal,
}

/// POST /api/v1/web3/send
///
/// Sends tokens on-chain from the user's Web3 wallet.
pub async fn web3_send(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<Web3SendRequest>,
) -> impl IntoResponse {
    if req.amount <= Decimal::ZERO {
        return bad_request("amount must be positive");
    }
    if req.to.is_empty() {
        return bad_request("to address must not be empty");
    }

    let chain = match parse_chain(&req.chain) {
        Ok(c) => c,
        Err(e) => return bad_request(e),
    };

    let user_id = auth.0;

    // Get sender address from user's wallet.
    let from_address = {
        let mgr = exchange.web3_wallets.lock().unwrap_or_else(|e| e.into_inner());
        match mgr.get_address(user_id, chain) {
            Some(addr) => addr.to_string(),
            None => return not_found("no Web3 wallet found"),
        }
    };

    match TransactionBuilder::build_send(chain, &from_address, &req.to, &req.token, req.amount) {
        Ok(tx) => {
            let mut history = exchange.web3_tx_history.lock().unwrap_or_else(|e| e.into_inner());
            history.entry(user_id).or_default().push(tx.clone());
            ok(serde_json::json!({
                "tx_id":      tx.id,
                "chain":      tx.chain.label(),
                "tx_type":    tx.tx_type,
                "from":       tx.from,
                "to":         tx.to,
                "amount":     tx.amount,
                "token":      tx.token,
                "gas_usd":    tx.gas_estimate_usd,
                "status":     tx.status,
                "tx_hash":    tx.tx_hash,
                "created_at": tx.created_at,
            }))
        }
        Err(e) => bad_request(e.to_string()),
    }
}

// ── GET /api/v1/web3/transactions ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct TxHistoryParams {
    /// Optional chain filter.
    #[serde(default)]
    pub chain: Option<String>,
    /// Maximum number of records to return (default: 50).
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize { 50 }

/// GET /api/v1/web3/transactions
///
/// Returns the on-chain transaction history for the user's Web3 wallet.
pub async fn get_web3_transactions(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Query(params): Query<TxHistoryParams>,
) -> impl IntoResponse {
    let user_id = auth.0;

    let chain_filter = match params.chain.as_deref() {
        Some(s) => match parse_chain(s) {
            Ok(c) => Some(c),
            Err(e) => return bad_request(e),
        },
        None => None,
    };

    let history = exchange.web3_tx_history.lock().unwrap_or_else(|e| e.into_inner());
    let txs: Vec<serde_json::Value> = history
        .get(&user_id)
        .map(|v| v.as_slice())
        .unwrap_or(&[])
        .iter()
        .filter(|tx| chain_filter.is_none_or(|c| tx.chain == c))
        .rev()
        .take(params.limit)
        .map(|tx| serde_json::json!({
            "tx_id":      tx.id,
            "chain":      tx.chain.label(),
            "tx_type":    tx.tx_type,
            "from":       tx.from,
            "to":         tx.to,
            "amount":     tx.amount,
            "token":      tx.token,
            "gas_usd":    tx.gas_estimate_usd,
            "status":     tx.status,
            "tx_hash":    tx.tx_hash,
            "created_at": tx.created_at,
        }))
        .collect();

    ok(serde_json::json!({ "transactions": txs, "count": txs.len() }))
}
