//! Financial products REST handlers.
//!
//! Five product areas implemented here:
//!
//! 1. **Launchpad** (`/launchpad/*`, `/admin/launchpad/*`) — IEO token sales with
//!    pro-rata allocation on oversubscription.
//!
//! 2. **Launchpool** (`/launchpool/*`, `/admin/launchpool/*`) — stake EXC tokens to
//!    earn newly-issued reward tokens proportional to stake share.
//!
//! 3. **Shark Fin** (`/earn/shark-fin/*`) — principal-protected structured product
//!    with capped upside tied to whether spot price stays within a price band.
//!
//! 4. **Liquidity Mining** (`/earn/liquidity/*`) — constant-product AMM LP positions
//!    earning a pro-rata share of trading fees plus bonus rewards.
//!
//! 5. **Stablecoin Auto-Conversion** (`/account/auto-convert`) — automatically convert
//!    a source coin (e.g. USDC) into a target coin (e.g. USDT) on receipt.

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Extension;
use axum::Json;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AuthenticatedUser;
use crate::rest::AppState;

// ── Shared response helpers ────────────────────────────────────────────────────

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

// =============================================================================
// ── Launchpad — IEO Token Sale ───────────────────────────────────────────────
// =============================================================================

/// Status of a Launchpad project through its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchpadStatus {
    /// Upcoming — commitment window not yet open.
    Upcoming,
    /// Active — users can commit USDT now.
    Active,
    /// Ended — commitment window closed, awaiting distribution.
    Ended,
    /// Distributed — tokens credited to winners, unused USDT returned.
    Distributed,
}

/// An IEO token-sale project created by an admin.
#[derive(Debug, Clone, Serialize)]
pub struct LaunchpadProject {
    pub id: Uuid,
    pub token_name: String,
    pub token_symbol: String,
    /// Total number of tokens available for sale.
    pub total_supply: Decimal,
    /// Fixed price per token expressed in USDT.
    pub price_per_token: Decimal,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    /// Minimum USDT a user must commit.
    pub min_allocation: Decimal,
    /// Maximum USDT a single user may commit.
    pub max_allocation: Decimal,
    /// Minimum EXC the user must have staked to participate (0 = no requirement).
    pub required_staking: Decimal,
    pub status: LaunchpadStatus,
    pub created_at: DateTime<Utc>,
}

/// A user's USDT commitment to a Launchpad project.
#[derive(Debug, Clone, Serialize)]
pub struct LaunchpadCommitment {
    pub id: Uuid,
    pub project_id: Uuid,
    pub user_id: Uuid,
    /// USDT committed and locked at time of submission.
    pub amount_usdt: Decimal,
    /// Tokens actually allocated at distribution (None until distributed).
    pub tokens_allocated: Option<Decimal>,
    /// USDT returned because of oversubscription (None until distributed).
    pub usdt_returned: Option<Decimal>,
    pub committed_at: DateTime<Utc>,
}

// ── Admin: create Launchpad project ──────────────────────────────────────────

/// Request body for POST /admin/launchpad/projects.
#[derive(Debug, Deserialize)]
pub struct CreateLaunchpadProjectRequest {
    pub token_name: String,
    pub token_symbol: String,
    pub total_supply: Decimal,
    pub price_per_token: Decimal,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub min_allocation: Decimal,
    pub max_allocation: Decimal,
    #[serde(default)]
    pub required_staking: Decimal,
}

/// POST /admin/launchpad/projects
///
/// Create a new IEO token-sale project. Validates all numeric fields and the
/// time window before persisting.
pub async fn admin_create_launchpad_project(
    State(exchange): State<AppState>,
    Json(req): Json<CreateLaunchpadProjectRequest>,
) -> impl IntoResponse {
    if req.token_name.trim().is_empty() {
        return bad_request("token_name is required");
    }
    if req.token_symbol.trim().is_empty() {
        return bad_request("token_symbol is required");
    }
    if req.total_supply <= Decimal::ZERO {
        return bad_request("total_supply must be positive");
    }
    if req.price_per_token <= Decimal::ZERO {
        return bad_request("price_per_token must be positive");
    }
    if req.min_allocation <= Decimal::ZERO {
        return bad_request("min_allocation must be positive");
    }
    if req.max_allocation < req.min_allocation {
        return bad_request("max_allocation must be >= min_allocation");
    }
    if req.end_time <= req.start_time {
        return bad_request("end_time must be after start_time");
    }

    let now = Utc::now();
    let status = if req.start_time > now {
        LaunchpadStatus::Upcoming
    } else {
        LaunchpadStatus::Active
    };

    let project = LaunchpadProject {
        id: Uuid::new_v4(),
        token_name: req.token_name.trim().to_string(),
        token_symbol: req.token_symbol.trim().to_uppercase(),
        total_supply: req.total_supply,
        price_per_token: req.price_per_token,
        start_time: req.start_time,
        end_time: req.end_time,
        min_allocation: req.min_allocation,
        max_allocation: req.max_allocation,
        required_staking: req.required_staking,
        status,
        created_at: now,
    };

    tracing::info!(
        project_id = %project.id,
        token_symbol = %project.token_symbol,
        "Launchpad project created"
    );

    let mut projects = exchange.launchpad_projects.lock().unwrap_or_else(|e| e.into_inner());
    projects.push(project.clone());

    created(serde_json::json!({"project": project}))
}

/// POST /admin/launchpad/projects/{id}/distribute — distribute tokens to committed users.
///
/// Pro-rata allocation: each user gets `(their_usdt / total_usdt) * total_supply` tokens.
/// If oversubscribed, excess USDT is returned. Sets project status to Distributed.
pub async fn admin_distribute_launchpad(
    State(exchange): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> impl IntoResponse {
    // Find and validate project.
    let project = {
        let mut projects = exchange.launchpad_projects.lock().unwrap_or_else(|e| e.into_inner());
        let Some(proj) = projects.iter_mut().find(|p| p.id == project_id) else {
            return bad_request("project not found");
        };
        if matches!(proj.status, LaunchpadStatus::Distributed) {
            return bad_request("project already distributed");
        }
        proj.status = LaunchpadStatus::Distributed;
        proj.clone()
    };

    // Calculate total committed USDT.
    let mut commitments = exchange.launchpad_commitments.lock().unwrap_or_else(|e| e.into_inner());
    let total_usdt: Decimal = commitments.iter()
        .filter(|c| c.project_id == project_id)
        .map(|c| c.amount_usdt)
        .sum();

    if total_usdt.is_zero() {
        return bad_request("no commitments to distribute");
    }

    // Max USDT the project can absorb = total_supply * price_per_token.
    let max_usdt = project.total_supply * project.price_per_token;
    let is_oversubscribed = total_usdt > max_usdt;

    let mut distributed_count = 0u32;
    let mut total_tokens_distributed = Decimal::ZERO;
    let mut total_usdt_returned = Decimal::ZERO;

    for commitment in commitments.iter_mut().filter(|c| c.project_id == project_id) {
        let user_share = commitment.amount_usdt / total_usdt;
        let tokens = if is_oversubscribed {
            // Pro-rata: user gets proportional share of total supply.
            project.total_supply * user_share
        } else {
            // Under-subscribed: user gets exactly what they paid for.
            commitment.amount_usdt / project.price_per_token
        };

        let usdt_used = tokens * project.price_per_token;
        let usdt_refund = commitment.amount_usdt - usdt_used;

        // Credit tokens + refund under one shard lock for this user.
        exchange.accounts.with_user(commitment.user_id, |am| {
            let _ = am.deposit(commitment.user_id, &project.token_symbol, tokens);
            if usdt_refund > Decimal::ZERO {
                let _ = am.deposit(commitment.user_id, "USDT", usdt_refund);
            }
        });
        if usdt_refund > Decimal::ZERO {
            total_usdt_returned += usdt_refund;
        }

        commitment.tokens_allocated = Some(tokens);
        commitment.usdt_returned = Some(usdt_refund);

        total_tokens_distributed += tokens;
        distributed_count += 1;
    }

    tracing::info!(
        project_id = %project_id,
        token_symbol = %project.token_symbol,
        total_tokens = %total_tokens_distributed,
        usdt_returned = %total_usdt_returned,
        users = distributed_count,
        "launchpad distribution complete"
    );

    (StatusCode::OK, Json(serde_json::json!({
        "projectId": project_id.to_string(),
        "status": "distributed",
        "totalTokens": total_tokens_distributed.to_string(),
        "usdtReturned": total_usdt_returned.to_string(),
        "users": distributed_count,
    })))
}

// ── User: commit USDT to Launchpad project ────────────────────────────────────

/// Request body for POST /launchpad/commit.
#[derive(Debug, Deserialize)]
pub struct LaunchpadCommitRequest {
    pub project_id: Uuid,
    /// USDT amount to commit.
    pub amount_usdt: Decimal,
}

/// POST /api/v1/launchpad/commit
///
/// Lock USDT against an active Launchpad project. The user's trading account
/// balance is debited immediately. Allocation is determined at end_time via
/// the settlement task.
pub async fn launchpad_commit(
    State(exchange): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(req): Json<LaunchpadCommitRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;

    if req.amount_usdt <= Decimal::ZERO {
        return bad_request("amount_usdt must be positive");
    }

    // Validate the project exists and is accepting commitments.
    let (min_alloc, max_alloc) = {
        let projects = exchange.launchpad_projects.lock().unwrap_or_else(|e| e.into_inner());
        let project = match projects.iter().find(|p| p.id == req.project_id) {
            Some(p) => p,
            None => return not_found("project not found"),
        };
        let now = Utc::now();
        if now < project.start_time {
            return bad_request("commitment window has not opened yet");
        }
        if now > project.end_time {
            return bad_request("commitment window has closed");
        }
        if project.status == LaunchpadStatus::Distributed {
            return bad_request("project has already been distributed");
        }
        (project.min_allocation, project.max_allocation)
    };

    if req.amount_usdt < min_alloc {
        return bad_request(format!("amount_usdt must be at least {min_alloc}"));
    }

    // Check if user already has a commitment and enforce max_allocation.
    {
        let commitments = exchange.launchpad_commitments.lock().unwrap_or_else(|e| e.into_inner());
        let existing_total: Decimal = commitments
            .iter()
            .filter(|c| c.project_id == req.project_id && c.user_id == user_id)
            .map(|c| c.amount_usdt)
            .sum();
        if existing_total + req.amount_usdt > max_alloc {
            return bad_request(format!(
                "total commitment would exceed max_allocation of {max_alloc}"
            ));
        }
    }

    // Deduct USDT from the user's trading account.
    if let Err(e) = exchange.accounts.withdraw(user_id, "USDT", req.amount_usdt) {
        return bad_request(format!("insufficient USDT balance: {e}"));
    }

    let commitment = LaunchpadCommitment {
        id: Uuid::new_v4(),
        project_id: req.project_id,
        user_id,
        amount_usdt: req.amount_usdt,
        tokens_allocated: None,
        usdt_returned: None,
        committed_at: Utc::now(),
    };

    tracing::info!(
        commitment_id = %commitment.id,
        user_id = %user_id,
        project_id = %req.project_id,
        amount_usdt = %req.amount_usdt,
        "Launchpad commitment recorded"
    );

    exchange
        .launchpad_commitments
        .lock().unwrap_or_else(|e| e.into_inner())
        .push(commitment.clone());

    created(serde_json::json!({"commitment": commitment}))
}

// ── Public: list Launchpad projects ──────────────────────────────────────────

/// GET /api/v1/launchpad/projects
///
/// Return all projects, optionally filtered by `status` query parameter.
pub async fn list_launchpad_projects(
    State(exchange): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let now = Utc::now();

    let mut projects = exchange.launchpad_projects.lock().unwrap_or_else(|e| e.into_inner());

    // Refresh statuses based on current time.
    for p in projects.iter_mut() {
        p.status = match p.status {
            LaunchpadStatus::Distributed => LaunchpadStatus::Distributed,
            _ if now < p.start_time => LaunchpadStatus::Upcoming,
            _ if now <= p.end_time => LaunchpadStatus::Active,
            _ => LaunchpadStatus::Ended,
        };
    }

    let filter = params.get("status").map(|s| s.as_str());
    let result: Vec<&LaunchpadProject> = projects
        .iter()
        .filter(|p| match filter {
            Some("upcoming") => p.status == LaunchpadStatus::Upcoming,
            Some("active") => p.status == LaunchpadStatus::Active,
            Some("ended") => p.status == LaunchpadStatus::Ended,
            Some("distributed") => p.status == LaunchpadStatus::Distributed,
            _ => true,
        })
        .collect();

    ok(serde_json::json!({"projects": result}))
}

// ── User: list own Launchpad commitments ─────────────────────────────────────

/// GET /api/v1/launchpad/my-commitments
pub async fn list_my_launchpad_commitments(
    State(exchange): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let commitments = exchange.launchpad_commitments.lock().unwrap_or_else(|e| e.into_inner());
    let user_commitments: Vec<&LaunchpadCommitment> =
        commitments.iter().filter(|c| c.user_id == user_id).collect();
    ok(serde_json::json!({"commitments": user_commitments}))
}

// =============================================================================
// ── Launchpool — Stake to Earn New Tokens ────────────────────────────────────
// =============================================================================

/// Status of a Launchpool staking pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchpoolStatus {
    /// Pool exists but has not started yet.
    Upcoming,
    /// Pool is accepting stakes and distributing rewards.
    Active,
    /// Reward distribution is complete; stakers may still unstake.
    Ended,
}

/// A Launchpool created by an admin.
#[derive(Debug, Clone, Serialize)]
pub struct LaunchpoolPool {
    pub id: Uuid,
    /// Token symbol that participants receive as rewards (e.g. "NEWCOIN").
    pub reward_token: String,
    /// Token symbol participants must stake (always "EXC" in Phase 1).
    pub staking_token: String,
    /// Total reward tokens allocated for the pool's lifetime.
    pub total_rewards: Decimal,
    /// Pool duration in calendar days.
    pub duration_days: u32,
    pub start_time: DateTime<Utc>,
    /// Computed end time: start_time + duration_days.
    pub end_time: DateTime<Utc>,
    pub status: LaunchpoolStatus,
    /// Total staking_token currently staked across all participants.
    pub total_staked: Decimal,
    pub created_at: DateTime<Utc>,
}

/// A user's stake record in a single Launchpool.
#[derive(Debug, Clone, Serialize)]
pub struct LaunchpoolStake {
    pub id: Uuid,
    pub pool_id: Uuid,
    pub user_id: Uuid,
    /// Amount of staking_token currently staked.
    pub amount: Decimal,
    /// Cumulative rewards already claimed by this user.
    pub claimed_rewards: Decimal,
    /// Rewards accrued but not yet claimed.
    pub pending_rewards: Decimal,
    /// When the stake was last updated (used for reward accrual).
    pub last_accrued_at: DateTime<Utc>,
    pub staked_at: DateTime<Utc>,
}

// ── Admin: create Launchpool ──────────────────────────────────────────────────

/// Request body for POST /admin/launchpool/pools.
#[derive(Debug, Deserialize)]
pub struct CreateLaunchpoolRequest {
    pub reward_token: String,
    pub staking_token: String,
    pub total_rewards: Decimal,
    pub duration_days: u32,
    pub start_time: DateTime<Utc>,
}

/// POST /admin/launchpool/pools
pub async fn admin_create_launchpool(
    State(exchange): State<AppState>,
    Json(req): Json<CreateLaunchpoolRequest>,
) -> impl IntoResponse {
    if req.reward_token.trim().is_empty() {
        return bad_request("reward_token is required");
    }
    if req.staking_token.trim().is_empty() {
        return bad_request("staking_token is required");
    }
    if req.total_rewards <= Decimal::ZERO {
        return bad_request("total_rewards must be positive");
    }
    if req.duration_days == 0 {
        return bad_request("duration_days must be at least 1");
    }

    let end_time = req.start_time + chrono::Duration::days(req.duration_days as i64);
    let now = Utc::now();
    let status = if req.start_time > now {
        LaunchpoolStatus::Upcoming
    } else {
        LaunchpoolStatus::Active
    };

    let pool = LaunchpoolPool {
        id: Uuid::new_v4(),
        reward_token: req.reward_token.trim().to_uppercase(),
        staking_token: req.staking_token.trim().to_uppercase(),
        total_rewards: req.total_rewards,
        duration_days: req.duration_days,
        start_time: req.start_time,
        end_time,
        status,
        total_staked: Decimal::ZERO,
        created_at: now,
    };

    tracing::info!(pool_id = %pool.id, reward_token = %pool.reward_token, "Launchpool created");

    exchange
        .launchpool_pools
        .lock().unwrap_or_else(|e| e.into_inner())
        .push(pool.clone());

    created(serde_json::json!({"pool": pool}))
}

// ── User: stake into Launchpool ───────────────────────────────────────────────

/// Request body for POST /launchpool/stake.
#[derive(Debug, Deserialize)]
pub struct LaunchpoolStakeRequest {
    pub pool_id: Uuid,
    pub amount: Decimal,
}

/// POST /api/v1/launchpool/stake
///
/// Lock staking_token (EXC) from the user's trading account into the pool.
pub async fn launchpool_stake(
    State(exchange): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(req): Json<LaunchpoolStakeRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;

    if req.amount <= Decimal::ZERO {
        return bad_request("amount must be positive");
    }

    // Validate pool and retrieve staking_token.
    let staking_token = {
        let mut pools = exchange.launchpool_pools.lock().unwrap_or_else(|e| e.into_inner());
        let pool = match pools.iter_mut().find(|p| p.id == req.pool_id) {
            Some(p) => p,
            None => return not_found("pool not found"),
        };
        let now = Utc::now();
        if now < pool.start_time {
            return bad_request("pool has not started yet");
        }
        if now > pool.end_time {
            return bad_request("pool has ended");
        }
        pool.total_staked += req.amount;
        pool.staking_token.clone()
    };

    // Deduct staking_token from user balance.
    if let Err(e) = exchange.accounts.withdraw(user_id, &staking_token, req.amount) {
        // Roll back total_staked on failure.
        let mut pools = exchange.launchpool_pools.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(pool) = pools.iter_mut().find(|p| p.id == req.pool_id) {
            pool.total_staked -= req.amount;
        }
        return bad_request(format!("insufficient {staking_token} balance: {e}"));
    }

    let now = Utc::now();
    let mut stakes = exchange.launchpool_stakes.lock().unwrap_or_else(|e| e.into_inner());

    // Increase an existing stake record if one exists, otherwise create a new one.
    if let Some(existing) = stakes.iter_mut().find(|s| s.pool_id == req.pool_id && s.user_id == user_id) {
        existing.amount += req.amount;
        existing.last_accrued_at = now;
        let stake_clone = existing.clone();
        tracing::info!(
            pool_id = %req.pool_id,
            user_id = %user_id,
            amount = %req.amount,
            "Launchpool stake increased"
        );
        return ok(serde_json::json!({"stake": stake_clone}));
    }

    let stake = LaunchpoolStake {
        id: Uuid::new_v4(),
        pool_id: req.pool_id,
        user_id,
        amount: req.amount,
        claimed_rewards: Decimal::ZERO,
        pending_rewards: Decimal::ZERO,
        last_accrued_at: now,
        staked_at: now,
    };

    tracing::info!(stake_id = %stake.id, pool_id = %req.pool_id, user_id = %user_id, amount = %req.amount, "Launchpool stake created");

    stakes.push(stake.clone());
    created(serde_json::json!({"stake": stake}))
}

// ── User: unstake from Launchpool ─────────────────────────────────────────────

/// Request body for POST /launchpool/unstake.
#[derive(Debug, Deserialize)]
pub struct LaunchpoolUnstakeRequest {
    pub pool_id: Uuid,
    pub amount: Decimal,
}

/// POST /api/v1/launchpool/unstake
///
/// Remove staked tokens from the pool. Pending rewards are preserved but not
/// claimed automatically; the user must call /launchpool/claim separately.
pub async fn launchpool_unstake(
    State(exchange): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(req): Json<LaunchpoolUnstakeRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;

    if req.amount <= Decimal::ZERO {
        return bad_request("amount must be positive");
    }

    let staking_token = {
        let pools = exchange.launchpool_pools.lock().unwrap_or_else(|e| e.into_inner());
        let pool = match pools.iter().find(|p| p.id == req.pool_id) {
            Some(p) => p,
            None => return not_found("pool not found"),
        };
        pool.staking_token.clone()
    };

    {
        let mut stakes = exchange.launchpool_stakes.lock().unwrap_or_else(|e| e.into_inner());
        let stake = match stakes.iter_mut().find(|s| s.pool_id == req.pool_id && s.user_id == user_id) {
            Some(s) => s,
            None => return not_found("no active stake found in this pool"),
        };
        if req.amount > stake.amount {
            return bad_request("unstake amount exceeds staked balance");
        }
        stake.amount -= req.amount;
    }

    // Update pool total_staked.
    {
        let mut pools = exchange.launchpool_pools.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(pool) = pools.iter_mut().find(|p| p.id == req.pool_id) {
            pool.total_staked -= req.amount;
        }
    }

    // Return staking tokens to user balance.
    let _ = exchange.accounts.deposit(user_id, &staking_token, req.amount);

    tracing::info!(pool_id = %req.pool_id, user_id = %user_id, amount = %req.amount, "Launchpool unstake");

    ok(serde_json::json!({"unstaked": req.amount, "token": staking_token}))
}

// ── User: claim Launchpool rewards ────────────────────────────────────────────

/// Request body for POST /launchpool/claim.
#[derive(Debug, Deserialize)]
pub struct LaunchpoolClaimRequest {
    pub pool_id: Uuid,
}

/// POST /api/v1/launchpool/claim
///
/// Credit pending_rewards to the user's balance and reset the pending counter.
/// Reward accrual is calculated proportionally to the user's share of
/// total_staked over the elapsed time since last_accrued_at.
pub async fn launchpool_claim(
    State(exchange): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(req): Json<LaunchpoolClaimRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let now = Utc::now();

    // Look up pool to compute rewards_per_second.
    let (reward_token, rewards_per_sec, total_staked) = {
        let pools = exchange.launchpool_pools.lock().unwrap_or_else(|e| e.into_inner());
        let pool = match pools.iter().find(|p| p.id == req.pool_id) {
            Some(p) => p,
            None => return not_found("pool not found"),
        };
        let duration_secs = Decimal::from(pool.duration_days as u64 * 86_400);
        let rps = if duration_secs > Decimal::ZERO {
            pool.total_rewards / duration_secs
        } else {
            Decimal::ZERO
        };
        (pool.reward_token.clone(), rps, pool.total_staked)
    };

    let claimed_amount = {
        let mut stakes = exchange.launchpool_stakes.lock().unwrap_or_else(|e| e.into_inner());
        let stake = match stakes.iter_mut().find(|s| s.pool_id == req.pool_id && s.user_id == user_id) {
            Some(s) => s,
            None => return not_found("no active stake found in this pool"),
        };

        // Accrue rewards since last_accrued_at.
        let elapsed_secs = Decimal::from(
            (now - stake.last_accrued_at).num_seconds().max(0) as u64,
        );
        if elapsed_secs > Decimal::ZERO && total_staked > Decimal::ZERO {
            let share = stake.amount / total_staked;
            let accrued = rewards_per_sec * elapsed_secs * share;
            stake.pending_rewards += accrued;
        }
        stake.last_accrued_at = now;

        let to_claim = stake.pending_rewards;
        stake.pending_rewards = Decimal::ZERO;
        stake.claimed_rewards += to_claim;
        to_claim
    };

    if claimed_amount > Decimal::ZERO {
        let _ = exchange.accounts.deposit(user_id, &reward_token, claimed_amount);
    }

    tracing::info!(
        pool_id = %req.pool_id,
        user_id = %user_id,
        claimed = %claimed_amount,
        token = %reward_token,
        "Launchpool rewards claimed"
    );

    ok(serde_json::json!({"claimed": claimed_amount, "token": reward_token}))
}

// ── Public: list Launchpool pools ─────────────────────────────────────────────

/// GET /api/v1/launchpool/pools
pub async fn list_launchpool_pools(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let now = Utc::now();
    let mut pools = exchange.launchpool_pools.lock().unwrap_or_else(|e| e.into_inner());

    for pool in pools.iter_mut() {
        pool.status = match pool.status {
            LaunchpoolStatus::Ended => LaunchpoolStatus::Ended,
            _ if now < pool.start_time => LaunchpoolStatus::Upcoming,
            _ if now <= pool.end_time => LaunchpoolStatus::Active,
            _ => LaunchpoolStatus::Ended,
        };
    }

    ok(serde_json::json!({"pools": *pools}))
}

// ── User: list own Launchpool stakes ─────────────────────────────────────────

/// GET /api/v1/launchpool/my-stakes
pub async fn list_my_launchpool_stakes(
    State(exchange): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let stakes = exchange.launchpool_stakes.lock().unwrap_or_else(|e| e.into_inner());
    let user_stakes: Vec<&LaunchpoolStake> = stakes.iter().filter(|s| s.user_id == user_id).collect();
    ok(serde_json::json!({"stakes": user_stakes}))
}

// =============================================================================
// ── Shark Fin — Principal-Protected Structured Product ───────────────────────
// =============================================================================

/// Status of a Shark Fin position through its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharkFinStatus {
    /// Position is active and price is being monitored.
    Active,
    /// Knocked out — price breached knock_out; guaranteed_apy applies.
    KnockedOut,
    /// Knocked in — price breached knock_in; guaranteed_apy applies.
    KnockedIn,
    /// Settled — principal + yield returned to user.
    Settled,
}

/// A Shark Fin structured product definition.
#[derive(Debug, Clone, Serialize)]
pub struct SharkFinProduct {
    pub id: Uuid,
    /// "bullish" or "bearish" — determines which barrier is checked.
    pub direction: String,
    /// Price below which the knock-in event fires (bearish: price hits floor).
    pub knock_in: Decimal,
    /// Price above which the knock-out event fires (bullish: price hits ceiling).
    pub knock_out: Decimal,
    /// APY applied if price breaches either barrier.
    pub guaranteed_apy: Decimal,
    /// APY applied if price stays within the range until expiry.
    pub max_apy: Decimal,
    /// Settlement date/time.
    pub settle_at: DateTime<Utc>,
    /// Whether new subscriptions are still accepted.
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

/// A user's Shark Fin position.
#[derive(Debug, Clone, Serialize)]
pub struct SharkFinPosition {
    pub id: Uuid,
    pub product_id: Uuid,
    pub user_id: Uuid,
    /// "bullish" or "bearish".
    pub direction: String,
    /// USDT principal locked.
    pub amount: Decimal,
    pub status: SharkFinStatus,
    /// Final yield paid at settlement (None until settled).
    pub yield_paid: Option<Decimal>,
    pub subscribed_at: DateTime<Utc>,
    pub settled_at: Option<DateTime<Utc>>,
}

// ── Admin: create Shark Fin product ──────────────────────────────────────────

/// Request body for POST /admin/earn/shark-fin/products.
#[derive(Debug, Deserialize)]
pub struct CreateSharkFinProductRequest {
    pub direction: String,
    pub knock_in: Decimal,
    pub knock_out: Decimal,
    pub guaranteed_apy: Decimal,
    pub max_apy: Decimal,
    pub settle_at: DateTime<Utc>,
}

/// POST /admin/earn/shark-fin/products
pub async fn admin_create_shark_fin_product(
    State(exchange): State<AppState>,
    Json(req): Json<CreateSharkFinProductRequest>,
) -> impl IntoResponse {
    if !matches!(req.direction.as_str(), "bullish" | "bearish") {
        return bad_request("direction must be 'bullish' or 'bearish'");
    }
    if req.knock_in <= Decimal::ZERO {
        return bad_request("knock_in must be positive");
    }
    if req.knock_out <= Decimal::ZERO {
        return bad_request("knock_out must be positive");
    }
    if req.knock_out <= req.knock_in {
        return bad_request("knock_out must be greater than knock_in");
    }
    if req.guaranteed_apy < Decimal::ZERO {
        return bad_request("guaranteed_apy must be non-negative");
    }
    if req.max_apy < req.guaranteed_apy {
        return bad_request("max_apy must be >= guaranteed_apy");
    }
    if req.settle_at <= Utc::now() {
        return bad_request("settle_at must be in the future");
    }

    let product = SharkFinProduct {
        id: Uuid::new_v4(),
        direction: req.direction,
        knock_in: req.knock_in,
        knock_out: req.knock_out,
        guaranteed_apy: req.guaranteed_apy,
        max_apy: req.max_apy,
        settle_at: req.settle_at,
        is_active: true,
        created_at: Utc::now(),
    };

    tracing::info!(product_id = %product.id, direction = %product.direction, "Shark Fin product created");

    exchange
        .shark_fin_products
        .lock().unwrap_or_else(|e| e.into_inner())
        .push(product.clone());

    created(serde_json::json!({"product": product}))
}

// ── User: subscribe to Shark Fin ─────────────────────────────────────────────

/// Request body for POST /earn/shark-fin/subscribe.
#[derive(Debug, Deserialize)]
pub struct SharkFinSubscribeRequest {
    pub product_id: Uuid,
    pub direction: String,
    pub amount: Decimal,
}

/// POST /api/v1/earn/shark-fin/subscribe
///
/// Lock USDT principal into a Shark Fin product. The principal is returned in
/// full at settlement plus the applicable yield.
pub async fn shark_fin_subscribe(
    State(exchange): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(req): Json<SharkFinSubscribeRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;

    if !matches!(req.direction.as_str(), "bullish" | "bearish") {
        return bad_request("direction must be 'bullish' or 'bearish'");
    }
    if req.amount <= Decimal::ZERO {
        return bad_request("amount must be positive");
    }

    {
        let products = exchange.shark_fin_products.lock().unwrap_or_else(|e| e.into_inner());
        let product = match products.iter().find(|p| p.id == req.product_id) {
            Some(p) => p,
            None => return not_found("product not found"),
        };
        if !product.is_active {
            return bad_request("product is no longer accepting subscriptions");
        }
        if product.direction != req.direction {
            return bad_request("direction does not match product direction");
        }
        if product.settle_at <= Utc::now() {
            return bad_request("product has already passed its settlement time");
        }
    }

    // Lock USDT principal.
    if let Err(e) = exchange.accounts.withdraw(user_id, "USDT", req.amount) {
        return bad_request(format!("insufficient USDT balance: {e}"));
    }

    let position = SharkFinPosition {
        id: Uuid::new_v4(),
        product_id: req.product_id,
        user_id,
        direction: req.direction,
        amount: req.amount,
        status: SharkFinStatus::Active,
        yield_paid: None,
        subscribed_at: Utc::now(),
        settled_at: None,
    };

    tracing::info!(
        position_id = %position.id,
        user_id = %user_id,
        product_id = %req.product_id,
        amount = %req.amount,
        "Shark Fin subscription created"
    );

    exchange
        .shark_fin_positions
        .lock().unwrap_or_else(|e| e.into_inner())
        .push(position.clone());

    created(serde_json::json!({"position": position}))
}

// ── Public: list Shark Fin products ──────────────────────────────────────────

/// GET /api/v1/earn/shark-fin/products
pub async fn list_shark_fin_products(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let products = exchange.shark_fin_products.lock().unwrap_or_else(|e| e.into_inner());
    ok(serde_json::json!({"products": *products}))
}

// ── User: list own Shark Fin positions ───────────────────────────────────────

/// GET /api/v1/earn/shark-fin/positions
pub async fn list_shark_fin_positions(
    State(exchange): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let positions = exchange.shark_fin_positions.lock().unwrap_or_else(|e| e.into_inner());
    let user_positions: Vec<&SharkFinPosition> =
        positions.iter().filter(|p| p.user_id == user_id).collect();
    ok(serde_json::json!({"positions": user_positions}))
}

// =============================================================================
// ── Liquidity Mining / LP Rewards ────────────────────────────────────────────
// =============================================================================

/// An LP pool definition — one entry per trading pair.
#[derive(Debug, Clone, Serialize)]
pub struct LiquidityPool {
    pub id: Uuid,
    /// Trading pair, e.g. "BTC/USDT".
    pub pair: String,
    /// Total liquidity of asset A currently in the pool.
    pub reserve_a: Decimal,
    /// Total liquidity of asset B currently in the pool.
    pub reserve_b: Decimal,
    /// Cumulative trading fees earned (for APR display purposes).
    pub total_fees_earned: Decimal,
    /// Annualised percentage return estimate (fees + bonus rewards).
    pub apr: Decimal,
    /// Total LP shares outstanding (constant-product k = reserve_a * reserve_b).
    pub total_lp_shares: Decimal,
    pub created_at: DateTime<Utc>,
}

/// A user's liquidity position in a single pool.
#[derive(Debug, Clone, Serialize)]
pub struct LiquidityPosition {
    pub id: Uuid,
    pub pool_id: Uuid,
    pub user_id: Uuid,
    /// LP share tokens held — proportional claim on the pool's reserves.
    pub lp_shares: Decimal,
    /// Asset A amount at time of provision (for reference).
    pub amount_a_provided: Decimal,
    /// Asset B amount at time of provision (for reference).
    pub amount_b_provided: Decimal,
    /// Accumulated fee rewards not yet withdrawn.
    pub pending_rewards: Decimal,
    pub provided_at: DateTime<Utc>,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Split a pair string ("BTC/USDT") into (base, quote) tokens.
fn split_pair(pair: &str) -> Option<(String, String)> {
    let mut parts = pair.splitn(2, '/');
    let a = parts.next()?.trim().to_uppercase();
    let b = parts.next()?.trim().to_uppercase();
    if a.is_empty() || b.is_empty() {
        return None;
    }
    Some((a, b))
}

// ── User: provide liquidity ───────────────────────────────────────────────────

/// Request body for POST /earn/liquidity/provide.
#[derive(Debug, Deserialize)]
pub struct ProvideLiquidityRequest {
    /// Trading pair, e.g. "BTC/USDT".
    pub pair: String,
    /// Amount of asset A (base) to deposit.
    pub amount_a: Decimal,
    /// Amount of asset B (quote) to deposit.
    pub amount_b: Decimal,
}

/// POST /api/v1/earn/liquidity/provide
///
/// Deposit `amount_a` and `amount_b` into the constant-product AMM pool for
/// `pair`. Mints LP shares proportional to the deposit relative to existing
/// reserves. Both assets are debited from the user's trading account.
pub async fn liquidity_provide(
    State(exchange): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(req): Json<ProvideLiquidityRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;

    let (coin_a, coin_b) = match split_pair(&req.pair) {
        Some(parts) => parts,
        None => return bad_request("pair must be in format 'BASE/QUOTE' (e.g. 'BTC/USDT')"),
    };

    if req.amount_a <= Decimal::ZERO {
        return bad_request("amount_a must be positive");
    }
    if req.amount_b <= Decimal::ZERO {
        return bad_request("amount_b must be positive");
    }

    // Deduct both assets from the user — both under one shard lock so the
    // refund-on-partial-failure is atomic.
    let deduct_result: Result<(), String> = exchange.accounts.with_user(user_id, |am| {
        if let Err(e) = am.withdraw(user_id, &coin_a, req.amount_a) {
            return Err(format!("insufficient {coin_a} balance: {e}"));
        }
        if let Err(e) = am.withdraw(user_id, &coin_b, req.amount_b) {
            let _ = am.deposit(user_id, &coin_a, req.amount_a);
            return Err(format!("insufficient {coin_b} balance: {e}"));
        }
        Ok(())
    });
    if let Err(msg) = deduct_result {
        return bad_request(msg);
    }

    let (pool_id, lp_shares) = {
        let mut pools = exchange.liquidity_pools.lock().unwrap_or_else(|e| e.into_inner());

        // Find or create the pool.
        let pool = if let Some(p) = pools.iter_mut().find(|p| p.pair == req.pair) {
            p
        } else {
            pools.push(LiquidityPool {
                id: Uuid::new_v4(),
                pair: req.pair.clone(),
                reserve_a: Decimal::ZERO,
                reserve_b: Decimal::ZERO,
                total_fees_earned: Decimal::ZERO,
                apr: Decimal::new(15, 2), // 15 % default APR estimate
                total_lp_shares: Decimal::ZERO,
                created_at: Utc::now(),
            });
            pools.last_mut().expect("just pushed")
        };

        // Constant-product share minting:
        //   If the pool is empty, first depositor defines the price; shares = sqrt(a * b).
        //   Otherwise, shares = min(a/reserve_a, b/reserve_b) * total_shares.
        let new_shares = if pool.total_lp_shares == Decimal::ZERO {
            // Geometric mean — approximate with (a * b) / (a + b) as a simple seed.
            (req.amount_a * req.amount_b)
                / (req.amount_a + req.amount_b)
                * Decimal::new(2, 0)
        } else {
            let share_a = req.amount_a / pool.reserve_a * pool.total_lp_shares;
            let share_b = req.amount_b / pool.reserve_b * pool.total_lp_shares;
            share_a.min(share_b)
        };

        pool.reserve_a += req.amount_a;
        pool.reserve_b += req.amount_b;
        pool.total_lp_shares += new_shares;

        (pool.id, new_shares)
    };

    let position = LiquidityPosition {
        id: Uuid::new_v4(),
        pool_id,
        user_id,
        lp_shares,
        amount_a_provided: req.amount_a,
        amount_b_provided: req.amount_b,
        pending_rewards: Decimal::ZERO,
        provided_at: Utc::now(),
    };

    tracing::info!(
        position_id = %position.id,
        pool_id = %pool_id,
        user_id = %user_id,
        lp_shares = %lp_shares,
        "Liquidity provided"
    );

    exchange
        .liquidity_positions
        .lock().unwrap_or_else(|e| e.into_inner())
        .push(position.clone());

    created(serde_json::json!({"position": position, "lp_shares": lp_shares}))
}

// ── User: withdraw liquidity ──────────────────────────────────────────────────

/// Request body for POST /earn/liquidity/withdraw.
#[derive(Debug, Deserialize)]
pub struct WithdrawLiquidityRequest {
    /// The LP position ID to redeem.
    pub position_id: Uuid,
}

/// POST /api/v1/earn/liquidity/withdraw
///
/// Burn LP shares and return the proportional share of pool reserves to the
/// user's trading account. Pending fee rewards are also credited.
pub async fn liquidity_withdraw(
    State(exchange): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(req): Json<WithdrawLiquidityRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;

    // Locate the position.
    let (pool_id, lp_shares, pending_rewards) = {
        let positions = exchange.liquidity_positions.lock().unwrap_or_else(|e| e.into_inner());
        let pos = match positions.iter().find(|p| p.id == req.position_id && p.user_id == user_id) {
            Some(p) => p,
            None => return not_found("position not found"),
        };
        (pos.pool_id, pos.lp_shares, pos.pending_rewards)
    };

    // Compute and withdraw from pool.
    let (coin_a, coin_b, amount_a, amount_b) = {
        let mut pools = exchange.liquidity_pools.lock().unwrap_or_else(|e| e.into_inner());
        let pool = match pools.iter_mut().find(|p| p.id == pool_id) {
            Some(p) => p,
            None => return not_found("pool not found"),
        };

        let (coin_a, coin_b) = match split_pair(&pool.pair) {
            Some(parts) => parts,
            None => return bad_request("pool pair is malformed"),
        };

        let share_fraction = if pool.total_lp_shares > Decimal::ZERO {
            lp_shares / pool.total_lp_shares
        } else {
            Decimal::ZERO
        };

        let amount_a = pool.reserve_a * share_fraction;
        let amount_b = pool.reserve_b * share_fraction;

        pool.reserve_a -= amount_a;
        pool.reserve_b -= amount_b;
        pool.total_lp_shares -= lp_shares;

        (coin_a, coin_b, amount_a, amount_b)
    };

    // Credit assets back to user under one shard lock.
    exchange.accounts.with_user(user_id, |am| {
        let _ = am.deposit(user_id, &coin_a, amount_a);
        let _ = am.deposit(user_id, &coin_b, amount_b);
        if pending_rewards > Decimal::ZERO {
            // Bonus rewards are denominated in USDT.
            let _ = am.deposit(user_id, "USDT", pending_rewards);
        }
    });

    // Remove the position record.
    {
        let mut positions = exchange.liquidity_positions.lock().unwrap_or_else(|e| e.into_inner());
        positions.retain(|p| !(p.id == req.position_id && p.user_id == user_id));
    }

    tracing::info!(
        position_id = %req.position_id,
        user_id = %user_id,
        amount_a = %amount_a,
        amount_b = %amount_b,
        rewards = %pending_rewards,
        "Liquidity withdrawn"
    );

    ok(serde_json::json!({
        "withdrawn_a": amount_a,
        "coin_a": coin_a,
        "withdrawn_b": amount_b,
        "coin_b": coin_b,
        "rewards_usdt": pending_rewards,
    }))
}

// ── Public: list LP pools ─────────────────────────────────────────────────────

/// GET /api/v1/earn/liquidity/pools
pub async fn list_liquidity_pools(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let pools = exchange.liquidity_pools.lock().unwrap_or_else(|e| e.into_inner());
    ok(serde_json::json!({"pools": *pools}))
}

// ── User: list own LP positions ───────────────────────────────────────────────

/// GET /api/v1/earn/liquidity/my-positions
pub async fn list_my_liquidity_positions(
    State(exchange): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let positions = exchange.liquidity_positions.lock().unwrap_or_else(|e| e.into_inner());
    let user_positions: Vec<&LiquidityPosition> =
        positions.iter().filter(|p| p.user_id == user_id).collect();
    ok(serde_json::json!({"positions": user_positions}))
}

// =============================================================================
// ── Stablecoin Auto-Conversion ───────────────────────────────────────────────
// =============================================================================

/// Auto-conversion configuration for one user.
///
/// When `enabled` is true, any incoming balance of `from_coin` (e.g. USDC) is
/// immediately converted to `to_coin` (e.g. USDT) at a 1:1 rate. The
/// background deposit task calls [`Exchange::apply_auto_convert`] to trigger
/// the conversion after crediting the incoming asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoConvertConfig {
    pub user_id: Uuid,
    /// Source coin to convert from, e.g. "USDC".
    pub from: String,
    /// Target coin to convert to, e.g. "USDT".
    pub to: String,
    pub enabled: bool,
    pub updated_at: DateTime<Utc>,
}

// ── User: set auto-convert settings ──────────────────────────────────────────

/// Request body for PUT /account/auto-convert.
#[derive(Debug, Deserialize)]
pub struct SetAutoConvertRequest {
    /// Source coin to watch, e.g. "USDC".
    pub from: String,
    /// Destination coin, e.g. "USDT".
    pub to: String,
    pub enabled: bool,
}

/// PUT /api/v1/account/auto-convert
///
/// Enable or disable automatic 1:1 stablecoin conversion. The conversion fires
/// whenever the user's `from` balance increases (deposit or trade settlement).
pub async fn set_auto_convert(
    State(exchange): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
    Json(req): Json<SetAutoConvertRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;

    if req.from.trim().is_empty() {
        return bad_request("from is required");
    }
    if req.to.trim().is_empty() {
        return bad_request("to is required");
    }
    let from = req.from.trim().to_uppercase();
    let to = req.to.trim().to_uppercase();
    if from == to {
        return bad_request("from and to must be different coins");
    }

    let config = AutoConvertConfig {
        user_id,
        from: from.clone(),
        to: to.clone(),
        enabled: req.enabled,
        updated_at: Utc::now(),
    };

    tracing::info!(
        user_id = %user_id,
        from = %from,
        to = %to,
        enabled = %req.enabled,
        "Auto-convert settings updated"
    );

    exchange
        .auto_convert_settings
        .lock().unwrap_or_else(|e| e.into_inner())
        .insert(user_id, config.clone());

    ok(serde_json::json!({"auto_convert": config}))
}

// ── User: get auto-convert settings ──────────────────────────────────────────

/// GET /api/v1/account/auto-convert
pub async fn get_auto_convert(
    State(exchange): State<AppState>,
    Extension(auth): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let settings = exchange.auto_convert_settings.lock().unwrap_or_else(|e| e.into_inner());
    match settings.get(&user_id) {
        Some(config) => ok(serde_json::json!({"auto_convert": config})),
        None => ok(serde_json::json!({"auto_convert": null})),
    }
}

// =============================================================================
// ── Tests ─────────────────────────────────────────────────────────────────────
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_pair_valid() {
        let (a, b) = split_pair("BTC/USDT").unwrap();
        assert_eq!(a, "BTC");
        assert_eq!(b, "USDT");
    }

    #[test]
    fn split_pair_case_normalised() {
        let (a, b) = split_pair("eth/usdt").unwrap();
        assert_eq!(a, "ETH");
        assert_eq!(b, "USDT");
    }

    #[test]
    fn split_pair_invalid_returns_none() {
        assert!(split_pair("BTCUSDT").is_none());
        assert!(split_pair("/USDT").is_none());
        assert!(split_pair("BTC/").is_none());
    }

    #[test]
    fn launchpad_status_serialises() {
        let s = serde_json::to_string(&LaunchpadStatus::Active).unwrap();
        assert_eq!(s, r#""active""#);
    }

    #[test]
    fn shark_fin_status_serialises() {
        let s = serde_json::to_string(&SharkFinStatus::KnockedOut).unwrap();
        assert_eq!(s, r#""knocked_out""#);
    }

    #[test]
    fn auto_convert_config_round_trips() {
        let config = AutoConvertConfig {
            user_id: Uuid::nil(),
            from: "USDC".into(),
            to: "USDT".into(),
            enabled: true,
            updated_at: DateTime::from_timestamp(0, 0).unwrap(),
        };
        let json = serde_json::to_string(&config).unwrap();
        let decoded: AutoConvertConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.from, "USDC");
        assert!(decoded.enabled);
    }

    #[test]
    fn launchpool_share_proportionality() {
        // Verify LP share minting formula: second provider gets fewer shares
        // if they deposit less than the first.
        let reserve_a = Decimal::new(100, 0);
        let reserve_b = Decimal::new(1000, 0);
        let total_shares = Decimal::new(200, 0);

        // Deposit half of reserve_a's proportion.
        let deposit_a = Decimal::new(10, 0); // 10 % of reserve_a
        let deposit_b = Decimal::new(100, 0); // 10 % of reserve_b

        let share_a = deposit_a / reserve_a * total_shares;
        let share_b = deposit_b / reserve_b * total_shares;
        let new_shares = share_a.min(share_b);

        assert_eq!(new_shares, Decimal::new(20, 0)); // 10 % of 200
    }
}
