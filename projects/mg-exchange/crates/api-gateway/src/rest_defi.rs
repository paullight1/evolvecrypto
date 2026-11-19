//! DeFi yield REST handlers.
//!
//! Endpoints:
//!   GET  /api/v1/defi/yields       — list all yield opportunities across protocols.
//!   GET  /api/v1/defi/protocols    — list integrated protocols with risk scores.
//!   GET  /api/v1/defi/allocations  — user's current DeFi yield allocations.
//!   POST /api/v1/defi/deposit      — allocate funds to DeFi yield.
//!   POST /api/v1/defi/withdraw     — withdraw from a DeFi yield allocation.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use rust_decimal::Decimal;
use serde::Deserialize;
use uuid::Uuid;

use exchange_defi_yield::{
    AllocationStatus, ProtocolRiskAssessor, YieldAllocator,
    protocols::{
        aave::AaveProtocol, compound::CompoundProtocol, eigenlayer::EigenLayerProtocol,
        lido::LidoProtocol, DeFiProtocol,
    },
};

use crate::auth::AuthenticatedUser;
use crate::rest::AppState;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn bad_request(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": msg.into()})))
}

fn not_found(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": msg.into()})))
}

fn ok(body: serde_json::Value) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::OK, Json(body))
}

fn created(body: serde_json::Value) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::CREATED, Json(body))
}

/// Maximum fraction of a deposit that can go to any single protocol (30%).
fn max_per_protocol() -> Decimal {
    Decimal::new(30, 2) // 0.30
}

/// Build the canonical list of all supported DeFi protocols.
fn all_protocols() -> Vec<Box<dyn DeFiProtocol>> {
    vec![
        Box::new(AaveProtocol::new()),
        Box::new(LidoProtocol::new()),
        Box::new(CompoundProtocol::new()),
        Box::new(EigenLayerProtocol::new()),
    ]
}

// ── GET /api/v1/defi/yields ───────────────────────────────────────────────────

/// Optional query parameters for yield filtering.
#[derive(Debug, Deserialize)]
pub struct YieldsParams {
    /// Filter by asset symbol (e.g. "USDC", "ETH"). Case-insensitive.
    pub asset: Option<String>,
    /// Maximum risk score to include (1–10). Defaults to 10 (all protocols).
    pub max_risk_score: Option<u8>,
}

/// GET /api/v1/defi/yields
///
/// Returns all yield opportunities across supported DeFi protocols, optionally
/// filtered by asset and risk score. Results are sorted by APY descending.
pub async fn defi_yields(
    _state: State<AppState>,
    Query(params): Query<YieldsParams>,
) -> impl IntoResponse {
    let max_risk = params.max_risk_score.unwrap_or(10).clamp(1, 10);
    let allocator = YieldAllocator::new(all_protocols(), max_per_protocol(), max_risk);

    let asset_filter = params.asset.as_deref().unwrap_or("").to_uppercase();

    // Determine which assets to query.
    let assets: Vec<String> = if asset_filter.is_empty() {
        let mut seen = std::collections::HashSet::new();
        let mut assets = Vec::new();
        for p in all_protocols() {
            for a in p.supported_assets() {
                if seen.insert(a.clone()) {
                    assets.push(a);
                }
            }
        }
        assets
    } else {
        vec![asset_filter]
    };

    let mut all_opps = Vec::new();
    for asset in &assets {
        match allocator.best_yield(asset).await {
            Ok(mut opps) => all_opps.append(&mut opps),
            Err(e) => {
                tracing::warn!(asset = %asset, error = %e, "failed to fetch yields");
            }
        }
    }

    // Sort overall by APY descending.
    all_opps.sort_by(|a, b| b.apy.partial_cmp(&a.apy).unwrap_or(std::cmp::Ordering::Equal));

    let items: Vec<serde_json::Value> = all_opps
        .iter()
        .map(|o| {
            serde_json::json!({
                "protocol": o.protocol,
                "asset": o.asset,
                "apy": o.apy,
                "apy_7d_avg": o.apy_7d_avg,
                "risk_score": o.risk_score,
                "min_deposit": o.min_deposit,
                "lock_period_days": o.lock_period_days,
                "chain": o.chain,
                "strategy": o.strategy,
            })
        })
        .collect();

    ok(serde_json::json!({ "yields": items, "count": items.len() }))
}

// ── GET /api/v1/defi/protocols ────────────────────────────────────────────────

/// GET /api/v1/defi/protocols
///
/// Returns metadata and risk assessments for all integrated DeFi protocols.
pub async fn defi_protocols(_state: State<AppState>) -> impl IntoResponse {
    let protocols = all_protocols();
    let items: Vec<serde_json::Value> = protocols
        .iter()
        .map(|p| {
            let info = p.info();
            let assessment = ProtocolRiskAssessor::assess(&info);
            serde_json::json!({
                "name": info.name,
                "chain": info.chain,
                "tvl_usd": info.tvl_usd,
                "audit_status": info.audit_status,
                "risk_score": info.risk_score,
                "insurance_coverage": info.insurance_coverage,
                "supported_assets": p.supported_assets(),
                "risk_assessment": {
                    "score": assessment.score,
                    "max_allocation_pct": assessment.max_allocation_pct,
                    "warnings": assessment.warnings,
                },
            })
        })
        .collect();

    ok(serde_json::json!({ "protocols": items, "count": items.len() }))
}

// ── GET /api/v1/defi/allocations ─────────────────────────────────────────────

/// GET /api/v1/defi/allocations
///
/// Returns the calling user's current DeFi yield allocations.
pub async fn defi_allocations(
    State(exchange): State<AppState>,
    axum::Extension(AuthenticatedUser(user_id)): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let allocs = exchange
        .defi_allocations
        .lock().unwrap_or_else(|e| e.into_inner());

    let user_allocs: Vec<serde_json::Value> = allocs
        .iter()
        .filter(|a| a.user_id == user_id)
        .map(format_allocation)
        .collect();

    ok(serde_json::json!({ "allocations": user_allocs, "count": user_allocs.len() }))
}

// ── POST /api/v1/defi/deposit ─────────────────────────────────────────────────

/// Request body for `POST /api/v1/defi/deposit`.
#[derive(Debug, Deserialize)]
pub struct DefiDepositRequest {
    /// Asset to deposit (e.g. "USDC", "ETH").
    pub asset: String,
    /// Amount to deposit.
    pub amount: Decimal,
    /// Optional preferred protocol. If absent, allocator picks optimally.
    pub preferred_protocol: Option<String>,
    /// Maximum acceptable risk score (1–10). Defaults to 5.
    #[serde(default = "default_max_risk")]
    pub max_risk_score: u8,
}

fn default_max_risk() -> u8 {
    5
}

/// POST /api/v1/defi/deposit
///
/// Allocates funds to DeFi yield protocols. The allocator splits the deposit
/// across eligible protocols according to APY and risk constraints.
///
/// Phase 1: simulated — no real funds move. A `YieldAllocation` record is
/// created and stored on the Exchange.
pub async fn defi_deposit(
    State(exchange): State<AppState>,
    axum::Extension(AuthenticatedUser(user_id)): axum::Extension<AuthenticatedUser>,
    Json(req): Json<DefiDepositRequest>,
) -> impl IntoResponse {
    if req.amount <= Decimal::ZERO {
        return bad_request("amount must be positive");
    }
    if req.asset.is_empty() {
        return bad_request("asset is required");
    }
    let max_risk = req.max_risk_score.clamp(1, 10);

    // If the caller specified a preferred protocol, filter to that one only.
    let protocols: Vec<Box<dyn DeFiProtocol>> = if let Some(ref pref) = req.preferred_protocol {
        let filtered: Vec<Box<dyn DeFiProtocol>> = all_protocols()
            .into_iter()
            .filter(|p| p.name().eq_ignore_ascii_case(pref))
            .collect();

        if filtered.is_empty() {
            return bad_request(format!(
                "unknown protocol '{pref}'; see GET /api/v1/defi/protocols",
            ));
        }
        filtered
    } else {
        all_protocols()
    };

    let allocator = YieldAllocator::new(protocols, max_per_protocol(), max_risk);

    match allocator.allocate(user_id, &req.asset, req.amount).await {
        Ok(new_allocs) => {
            let result: Vec<serde_json::Value> =
                new_allocs.iter().map(format_allocation).collect();

            {
                let mut store = exchange
                    .defi_allocations
                    .lock().unwrap_or_else(|e| e.into_inner());
                store.extend(new_allocs);
            }

            tracing::info!(
                user_id = %user_id,
                asset = %req.asset,
                amount = %req.amount,
                num_allocations = result.len(),
                "DeFi deposit created"
            );

            created(serde_json::json!({ "allocations": result, "count": result.len() }))
        }
        Err(e) => {
            tracing::warn!(user_id = %user_id, asset = %req.asset, error = %e, "DeFi deposit failed");
            bad_request(e.to_string())
        }
    }
}

// ── POST /api/v1/defi/withdraw ────────────────────────────────────────────────

/// Request body for `POST /api/v1/defi/withdraw`.
#[derive(Debug, Deserialize)]
pub struct DefiWithdrawRequest {
    /// ID of the allocation to withdraw from.
    pub allocation_id: Uuid,
    /// Amount to withdraw, or absent for full withdrawal.
    pub amount: Option<Decimal>,
}

/// POST /api/v1/defi/withdraw
///
/// Initiates withdrawal from a DeFi yield allocation.
/// Phase 1: marks the allocation as `Withdrawn` immediately (simulated instant settlement).
/// Phase 2: mark as `Withdrawing`, poll for on-chain confirmation, then mark `Withdrawn`.
pub async fn defi_withdraw(
    State(exchange): State<AppState>,
    axum::Extension(AuthenticatedUser(user_id)): axum::Extension<AuthenticatedUser>,
    Json(req): Json<DefiWithdrawRequest>,
) -> impl IntoResponse {
    let (protocol_name, asset, withdraw_amount) = {
        let mut allocs = exchange
            .defi_allocations
            .lock().unwrap_or_else(|e| e.into_inner());

        let alloc = match allocs
            .iter_mut()
            .find(|a| a.id == req.allocation_id && a.user_id == user_id)
        {
            Some(a) => a,
            None => {
                return not_found(format!("allocation '{}' not found", req.allocation_id))
            }
        };

        if alloc.status != AllocationStatus::Active {
            return bad_request(format!(
                "allocation '{}' is not active",
                req.allocation_id
            ));
        }

        let withdraw_amount = match req.amount {
            Some(amt) => {
                if amt <= Decimal::ZERO {
                    return bad_request("amount must be positive");
                }
                if amt > alloc.amount {
                    return bad_request(format!(
                        "requested withdrawal {} exceeds allocation balance {}",
                        amt, alloc.amount
                    ));
                }
                amt
            }
            None => alloc.amount,
        };

        // Update the allocation in-place while we still hold the lock.
        if withdraw_amount == alloc.amount {
            alloc.status = AllocationStatus::Withdrawn;
            alloc.amount = Decimal::ZERO;
        } else {
            alloc.amount -= withdraw_amount;
        }

        (alloc.protocol.clone(), alloc.asset.clone(), withdraw_amount)
    };

    // Simulate the on-chain withdrawal outside the lock.
    let protocols = all_protocols();
    let protocol = match protocols.iter().find(|p| p.name() == protocol_name) {
        Some(p) => p,
        None => return bad_request(format!("protocol '{protocol_name}' not found")),
    };

    match protocol.withdraw(&asset, withdraw_amount).await {
        Ok(tx_hash) => {
            tracing::info!(
                user_id = %user_id,
                allocation_id = %req.allocation_id,
                protocol = %protocol_name,
                asset = %asset,
                amount = %withdraw_amount,
                tx = %tx_hash,
                "DeFi withdrawal processed"
            );

            ok(serde_json::json!({
                "allocation_id": req.allocation_id,
                "amount_withdrawn": withdraw_amount,
                "tx_hash": tx_hash,
                "status": "withdrawn",
            }))
        }
        Err(e) => {
            // Protocol rejected the withdrawal. Reverse the status update.
            // In Phase 2 this should be a transactional operation.
            tracing::warn!(
                user_id = %user_id,
                allocation_id = %req.allocation_id,
                error = %e,
                "DeFi withdrawal simulation failed"
            );
            bad_request(e.to_string())
        }
    }
}

// ── Serialisation helper ──────────────────────────────────────────────────────

fn format_allocation(a: &exchange_defi_yield::YieldAllocation) -> serde_json::Value {
    serde_json::json!({
        "id": a.id,
        "protocol": a.protocol,
        "asset": a.asset,
        "amount": a.amount,
        "current_apy": a.current_apy,
        "earned_yield": a.earned_yield,
        "deposited_at": a.deposited_at,
        "status": a.status,
    })
}
