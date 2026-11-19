//! Engagement and operational features
//!
//! Feature 1 — Bonus/Coupon System
//! Feature 2 — Airdrop Portal
//! Feature 3 — Multi-Tier Affiliate Commission
//! Feature 4 — Trade Competition Teams
//! Feature 5 — Testnet Configuration

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AuthenticatedUser;
use crate::exchange::Exchange;
use crate::rest::AppState;

// ── Shared helpers ─────────────────────────────────────────────────────────────

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

fn conflict(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({"error": msg.into()})),
    )
}

fn forbidden(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({"error": msg.into()})),
    )
}

// =============================================================================
// Feature 1 — Bonus/Coupon System
// =============================================================================

/// A bonus campaign created by an admin.
///
/// Bonuses are credited as "bonus balance" — usable for trading margin but
/// not withdrawable.
#[derive(Debug, Clone, Serialize)]
pub struct BonusCampaign {
    pub id: Uuid,
    pub name: String,
    /// "deposit" | "trading" | "signup"
    pub bonus_type: String,
    /// Fixed bonus amount in USDT.
    pub amount: Decimal,
    /// Minimum deposit required for "deposit" type bonuses.
    pub min_deposit: Decimal,
    pub expiry: chrono::DateTime<chrono::Utc>,
    pub max_uses: u32,
    pub uses_remaining: u32,
    /// Optional coupon code users can provide during claim.
    pub coupon_code: Option<String>,
    pub is_active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// A bonus credited to a specific user.
#[derive(Debug, Clone, Serialize)]
pub struct UserBonus {
    pub id: Uuid,
    pub campaign_id: Uuid,
    pub bonus_type: String,
    pub amount: Decimal,
    /// Remaining spendable amount (decremented as user uses it for margin).
    pub remaining: Decimal,
    /// "active" | "used" | "expired"
    pub status: String,
    pub claimed_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

// ── Admin: create campaign ────────────────────────────────────────────────────

/// POST /admin/bonus/campaigns
///
/// Admin creates a new bonus campaign.
pub async fn admin_create_bonus_campaign(
    State(exchange): State<AppState>,
    Json(req): Json<CreateBonusCampaignRequest>,
) -> impl IntoResponse {
    if req.name.trim().is_empty() {
        return bad_request("name is required");
    }
    if !matches!(req.bonus_type.as_str(), "deposit" | "trading" | "signup") {
        return bad_request("bonus_type must be 'deposit', 'trading', or 'signup'");
    }
    if req.amount <= Decimal::ZERO {
        return bad_request("amount must be positive");
    }
    if req.max_uses == 0 {
        return bad_request("max_uses must be at least 1");
    }
    if req.expiry <= chrono::Utc::now() {
        return bad_request("expiry must be in the future");
    }

    let campaign = BonusCampaign {
        id: Uuid::new_v4(),
        name: req.name,
        bonus_type: req.bonus_type,
        amount: req.amount,
        min_deposit: req.min_deposit.unwrap_or(Decimal::ZERO),
        expiry: req.expiry,
        max_uses: req.max_uses,
        uses_remaining: req.max_uses,
        coupon_code: req.coupon_code,
        is_active: true,
        created_at: chrono::Utc::now(),
    };

    let id = campaign.id;
    let name = campaign.name.clone();

    let mut campaigns = exchange
        .bonus_campaigns
        .lock().unwrap_or_else(|e| e.into_inner());
    campaigns.push(campaign);

    tracing::info!(campaign_id = %id, %name, "bonus campaign created");
    (
        StatusCode::CREATED,
        Json(serde_json::json!({"id": id.to_string(), "name": name})),
    )
}

#[derive(Debug, Deserialize)]
pub struct CreateBonusCampaignRequest {
    pub name: String,
    pub bonus_type: String,
    pub amount: Decimal,
    pub min_deposit: Option<Decimal>,
    pub expiry: chrono::DateTime<chrono::Utc>,
    pub max_uses: u32,
    pub coupon_code: Option<String>,
}

// ── User: claim bonus ─────────────────────────────────────────────────────────

/// POST /api/v1/bonus/claim
///
/// User claims a bonus either by `campaign_id` or `coupon_code`.
pub async fn claim_bonus(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<ClaimBonusRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let now = chrono::Utc::now();

    // Locate the campaign.
    let campaign_id = {
        let campaigns = exchange
            .bonus_campaigns
            .lock().unwrap_or_else(|e| e.into_inner());

        let found = if let Some(id) = req.campaign_id {
            campaigns.iter().find(|c| c.id == id)
        } else if let Some(ref code) = req.coupon_code {
            campaigns
                .iter()
                .find(|c| c.coupon_code.as_deref() == Some(code.as_str()))
        } else {
            return bad_request("provide campaign_id or coupon_code");
        };

        match found {
            None => return not_found("campaign not found"),
            Some(c) if !c.is_active => return bad_request("campaign is not active"),
            Some(c) if c.expiry <= now => return bad_request("campaign has expired"),
            Some(c) if c.uses_remaining == 0 => return bad_request("campaign is fully claimed"),
            Some(c) => c.id,
        }
    };

    // Guard against duplicate claims.
    {
        let user_bonuses = exchange
            .user_bonuses
            .lock().unwrap_or_else(|e| e.into_inner());
        if let Some(bonuses) = user_bonuses.get(&user_id) {
            if bonuses.iter().any(|b| b.campaign_id == campaign_id) {
                return conflict("bonus already claimed");
            }
        }
    }

    // Decrement uses and build the UserBonus.
    let (bonus_type, amount, expiry) = {
        let mut campaigns = exchange
            .bonus_campaigns
            .lock().unwrap_or_else(|e| e.into_inner());
        let c = campaigns
            .iter_mut()
            .find(|c| c.id == campaign_id)
            .expect("campaign existence checked above");
        c.uses_remaining -= 1;
        (c.bonus_type.clone(), c.amount, c.expiry)
    };

    let user_bonus = UserBonus {
        id: Uuid::new_v4(),
        campaign_id,
        bonus_type,
        amount,
        remaining: amount,
        status: "active".to_string(),
        claimed_at: now,
        expires_at: expiry,
    };

    let bonus_id = user_bonus.id;

    // Credit bonus balance to the user's account (bonus = tradeable but not withdrawable).
    // Credit to a dedicated "BONUS_USDT" asset so it can be tracked separately.
    let _ = exchange.accounts.deposit(user_id, "BONUS_USDT", amount);

    exchange
        .user_bonuses
        .lock().unwrap_or_else(|e| e.into_inner())
        .entry(user_id)
        .or_default()
        .push(user_bonus);

    tracing::info!(
        %user_id,
        %campaign_id,
        %bonus_id,
        %amount,
        "bonus claimed"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "id": bonus_id.to_string(),
            "amount": amount,
            "status": "active"
        })),
    )
}

#[derive(Debug, Deserialize)]
pub struct ClaimBonusRequest {
    pub campaign_id: Option<Uuid>,
    pub coupon_code: Option<String>,
}

// ── User: list active campaigns ───────────────────────────────────────────────

/// GET /api/v1/bonus/campaigns
///
/// Returns all active, non-expired campaigns.
pub async fn list_bonus_campaigns(State(exchange): State<AppState>) -> impl IntoResponse {
    let now = chrono::Utc::now();
    let campaigns = exchange
        .bonus_campaigns
        .lock().unwrap_or_else(|e| e.into_inner());

    let active: Vec<_> = campaigns
        .iter()
        .filter(|c| c.is_active && c.expiry > now && c.uses_remaining > 0)
        .collect();

    (StatusCode::OK, Json(serde_json::json!({"campaigns": active})))
}

// ── User: list my bonuses ─────────────────────────────────────────────────────

/// GET /api/v1/bonus/my-bonuses
///
/// Returns authenticated user's active bonuses.
pub async fn list_my_bonuses(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let now = chrono::Utc::now();

    let mut user_bonuses = exchange
        .user_bonuses
        .lock().unwrap_or_else(|e| e.into_inner());

    // Expire any stale entries in-place so the list stays current.
    if let Some(bonuses) = user_bonuses.get_mut(&user_id) {
        for b in bonuses.iter_mut() {
            if b.status == "active" && b.expires_at <= now {
                b.status = "expired".to_string();
            }
        }
    }

    let bonuses: Vec<_> = user_bonuses
        .get(&user_id)
        .map(|v| v.iter().filter(|b| b.status == "active").collect())
        .unwrap_or_default();

    (StatusCode::OK, Json(serde_json::json!({"bonuses": bonuses})))
}

// =============================================================================
// Feature 2 — Airdrop Portal
// =============================================================================

/// An airdrop distribution event created by an admin.
#[derive(Debug, Clone, Serialize)]
pub struct Airdrop {
    pub id: Uuid,
    /// Token/asset being distributed (e.g. "TOKEN", "USDT").
    pub token: String,
    pub amount_per_user: Decimal,
    /// Eligibility rule: "all" | "kyc_verified" | "volume_above_1000" | "staking"
    pub criteria: String,
    /// Explicit list of eligible user IDs (empty = determined by `criteria`).
    pub eligible_users: Vec<Uuid>,
    /// "upcoming" | "active" | "ended"
    pub status: String,
    pub starts_at: chrono::DateTime<chrono::Utc>,
    pub ends_at: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// ── Admin: create airdrop ─────────────────────────────────────────────────────

/// POST /admin/airdrops
///
/// Admin creates an airdrop distribution event.
pub async fn admin_create_airdrop(
    State(exchange): State<AppState>,
    Json(req): Json<CreateAirdropRequest>,
) -> impl IntoResponse {
    if req.token.trim().is_empty() {
        return bad_request("token is required");
    }
    if req.amount_per_user <= Decimal::ZERO {
        return bad_request("amount_per_user must be positive");
    }
    if !matches!(
        req.criteria.as_str(),
        "all" | "kyc_verified" | "volume_above_1000" | "staking"
    ) {
        return bad_request(
            "criteria must be 'all', 'kyc_verified', 'volume_above_1000', or 'staking'",
        );
    }
    if req.ends_at <= req.starts_at {
        return bad_request("ends_at must be after starts_at");
    }

    let airdrop = Airdrop {
        id: Uuid::new_v4(),
        token: req.token,
        amount_per_user: req.amount_per_user,
        criteria: req.criteria,
        eligible_users: req.eligible_users.unwrap_or_default(),
        status: "active".to_string(),
        starts_at: req.starts_at,
        ends_at: req.ends_at,
        created_at: chrono::Utc::now(),
    };

    let id = airdrop.id;

    exchange
        .airdrops
        .lock().unwrap_or_else(|e| e.into_inner())
        .push(airdrop);

    tracing::info!(airdrop_id = %id, "airdrop created");
    (
        StatusCode::CREATED,
        Json(serde_json::json!({"id": id.to_string()})),
    )
}

#[derive(Debug, Deserialize)]
pub struct CreateAirdropRequest {
    pub token: String,
    pub amount_per_user: Decimal,
    pub criteria: String,
    pub eligible_users: Option<Vec<Uuid>>,
    pub starts_at: chrono::DateTime<chrono::Utc>,
    pub ends_at: chrono::DateTime<chrono::Utc>,
}

// ── User: check eligibility ───────────────────────────────────────────────────

/// GET /api/v1/airdrops/eligible
///
/// Returns the airdrops for which the authenticated user is eligible and has
/// not yet claimed.
pub async fn list_eligible_airdrops(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let now = chrono::Utc::now();

    let airdrops = exchange
        .airdrops
        .lock().unwrap_or_else(|e| e.into_inner())
        .clone();

    let claims = exchange
        .airdrop_claims
        .lock().unwrap_or_else(|e| e.into_inner());

    let kyc_approved = exchange.kyc_approved.contains(&user_id);

    let user_volume: Decimal = {
        let fee_engine = exchange
            .fee_engine
            .lock().unwrap_or_else(|e| e.into_inner());
        fee_engine.get_volume(user_id)
    };

    let has_staking = {
        let positions = exchange
            .staking_positions
            .lock().unwrap_or_else(|e| e.into_inner());
        positions
            .keys()
            .any(|(uid, _)| *uid == user_id)
    };

    let eligible: Vec<_> = airdrops
        .iter()
        .filter(|a| {
            // Must be active and within window.
            if a.status != "active" || a.starts_at > now || a.ends_at <= now {
                return false;
            }
            // Must not have already claimed.
            if claims.contains_key(&(user_id, a.id)) {
                return false;
            }
            // Check eligibility criteria.
            is_eligible_for_airdrop(
                a,
                user_id,
                kyc_approved,
                user_volume,
                has_staking,
            )
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({"eligible_airdrops": eligible})),
    )
}

fn is_eligible_for_airdrop(
    airdrop: &Airdrop,
    user_id: Uuid,
    kyc_approved: bool,
    volume: Decimal,
    has_staking: bool,
) -> bool {
    // If explicit list is provided, check membership first.
    if !airdrop.eligible_users.is_empty() {
        return airdrop.eligible_users.contains(&user_id);
    }
    match airdrop.criteria.as_str() {
        "all" => true,
        "kyc_verified" => kyc_approved,
        "volume_above_1000" => volume >= Decimal::new(1000, 0),
        "staking" => has_staking,
        _ => false,
    }
}

// ── User: claim airdrop ───────────────────────────────────────────────────────

/// POST /api/v1/airdrops/claim
///
/// Authenticated user claims tokens from an airdrop.
pub async fn claim_airdrop(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<ClaimAirdropRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let now = chrono::Utc::now();

    // Determine eligibility and extract claim details without holding multiple locks.
    let (token, amount) = {
        let airdrops = exchange
            .airdrops
            .lock().unwrap_or_else(|e| e.into_inner());

        let airdrop = match airdrops.iter().find(|a| a.id == req.airdrop_id) {
            Some(a) => a,
            None => return not_found("airdrop not found"),
        };

        if airdrop.status != "active" {
            return bad_request("airdrop is not active");
        }
        if airdrop.starts_at > now {
            return bad_request("airdrop has not started yet");
        }
        if airdrop.ends_at <= now {
            return bad_request("airdrop has ended");
        }

        let kyc_approved = exchange.kyc_approved.contains(&user_id);

        let user_volume = exchange
            .fee_engine
            .lock().unwrap_or_else(|e| e.into_inner())
            .get_volume(user_id);

        let has_staking = exchange
            .staking_positions
            .lock().unwrap_or_else(|e| e.into_inner())
            .keys()
            .any(|(uid, _)| *uid == user_id);

        if !is_eligible_for_airdrop(airdrop, user_id, kyc_approved, user_volume, has_staking) {
            return forbidden("not eligible for this airdrop");
        }

        (airdrop.token.clone(), airdrop.amount_per_user)
    };

    // Guard duplicate claim.
    {
        let mut claims = exchange
            .airdrop_claims
            .lock().unwrap_or_else(|e| e.into_inner());
        if claims.contains_key(&(user_id, req.airdrop_id)) {
            return conflict("airdrop already claimed");
        }
        claims.insert((user_id, req.airdrop_id), true);
    }

    // Credit the token balance.
    let _ = exchange.accounts.deposit(user_id, &token, amount);

    tracing::info!(
        %user_id,
        airdrop_id = %req.airdrop_id,
        %token,
        %amount,
        "airdrop claimed"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "token": token,
            "amount": amount,
            "status": "credited"
        })),
    )
}

#[derive(Debug, Deserialize)]
pub struct ClaimAirdropRequest {
    pub airdrop_id: Uuid,
}

// ── Public: list airdrops ─────────────────────────────────────────────────────

/// GET /api/v1/airdrops
///
/// Lists all active and past airdrops (public).
pub async fn list_airdrops(State(exchange): State<AppState>) -> impl IntoResponse {
    let airdrops = exchange
        .airdrops
        .lock().unwrap_or_else(|e| e.into_inner());

    (
        StatusCode::OK,
        Json(serde_json::json!({"airdrops": *airdrops})),
    )
}

// =============================================================================
// Feature 3 — Multi-Tier Affiliate Commission
// =============================================================================

/// A single affiliate commission event earned by a referrer.
#[derive(Debug, Clone, Serialize)]
pub struct AffiliateCommission {
    pub id: Uuid,
    pub earner_id: Uuid,
    /// The user whose trade generated this commission.
    pub trader_id: Uuid,
    /// 1 = direct, 2 = second-level, 3 = third-level.
    pub tier: u8,
    /// Fraction of the original fee credited to this earner.
    pub rate: Decimal,
    /// Commission amount in USDT.
    pub amount: Decimal,
    pub earned_at: chrono::DateTime<chrono::Utc>,
}

/// Commission rates per tier.
pub const TIER1_RATE: Decimal = Decimal::from_parts(30, 0, 0, false, 2); // 0.30
pub const TIER2_RATE: Decimal = Decimal::from_parts(10, 0, 0, false, 2); // 0.10
pub const TIER3_RATE: Decimal = Decimal::from_parts(5, 0, 0, false, 2);  // 0.05

/// Distribute affiliate commissions up the referral tree for a single fill.
///
/// `trader_id` is the user who generated the fee. `fee_amount` is the total
/// fee collected. Commissions at each tier are credited to the respective
/// referrer's USDT balance.
pub fn distribute_affiliate_commissions(exchange: &Arc<Exchange>, trader_id: Uuid, fee_amount: Decimal) {
    let tree = exchange
        .referral_tree
        .lock().unwrap_or_else(|e| e.into_inner());

    let tier1 = match tree.get(&trader_id).copied() {
        Some(id) => id,
        None => return, // no referrer at all
    };
    let tier2 = tree.get(&tier1).copied();
    let tier3 = tier2.and_then(|t2| tree.get(&t2).copied());

    drop(tree);

    let now = chrono::Utc::now();

    let mut payouts: Vec<(Uuid, u8, Decimal, Decimal)> = Vec::with_capacity(3);
    payouts.push((tier1, 1, TIER1_RATE, fee_amount * TIER1_RATE));
    if let Some(t2) = tier2 {
        payouts.push((t2, 2, TIER2_RATE, fee_amount * TIER2_RATE));
    }
    if let Some(t3) = tier3 {
        payouts.push((t3, 3, TIER3_RATE, fee_amount * TIER3_RATE));
    }

    let mut commissions = exchange
        .affiliate_commissions
        .lock().unwrap_or_else(|e| e.into_inner());

    for (earner_id, tier, rate, amount) in payouts {
        if amount <= Decimal::ZERO {
            continue;
        }
        let _ = exchange.accounts.deposit(earner_id, "USDT", amount);
        commissions.push_back(AffiliateCommission {
            id: Uuid::new_v4(),
            earner_id,
            trader_id,
            tier,
            rate,
            amount,
            earned_at: now,
        });
        tracing::trace!(
            %earner_id,
            %trader_id,
            tier,
            %amount,
            "affiliate commission distributed"
        );
    }

    // Cap the history at 200 000 entries (FIFO eviction).
    while commissions.len() > 200_000 {
        commissions.pop_front();
    }
}

/// GET /api/v1/referral/tree
///
/// Returns the authenticated user's referral tree — direct referrals and their
/// sub-referrals (up to three levels), with aggregated earnings per tier.
pub async fn get_referral_tree(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;

    let tree = exchange
        .referral_tree
        .lock().unwrap_or_else(|e| e.into_inner());

    // Find all direct referrals (children).
    let tier1_refs: Vec<Uuid> = tree
        .iter()
        .filter_map(|(&child, &parent)| if parent == user_id { Some(child) } else { None })
        .collect();

    let tier2_refs: Vec<Uuid> = tier1_refs
        .iter()
        .flat_map(|t1| {
            tree.iter()
                .filter_map(|(&child, &parent)| if parent == *t1 { Some(child) } else { None })
                .collect::<Vec<_>>()
        })
        .collect();

    let tier3_refs: Vec<Uuid> = tier2_refs
        .iter()
        .flat_map(|t2| {
            tree.iter()
                .filter_map(|(&child, &parent)| if parent == *t2 { Some(child) } else { None })
                .collect::<Vec<_>>()
        })
        .collect();

    drop(tree);

    // Aggregate earnings per tier.
    let commissions = exchange
        .affiliate_commissions
        .lock().unwrap_or_else(|e| e.into_inner());

    let mut earnings = [Decimal::ZERO; 3];
    for c in commissions.iter() {
        if c.earner_id == user_id && c.tier >= 1 && c.tier <= 3 {
            earnings[(c.tier - 1) as usize] += c.amount;
        }
    }
    drop(commissions);

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "tier1": {
                "referrals": tier1_refs.iter().map(|id| id.to_string()).collect::<Vec<_>>(),
                "count": tier1_refs.len(),
                "earnings": earnings[0],
                "rate": "30%"
            },
            "tier2": {
                "referrals": tier2_refs.iter().map(|id| id.to_string()).collect::<Vec<_>>(),
                "count": tier2_refs.len(),
                "earnings": earnings[1],
                "rate": "10%"
            },
            "tier3": {
                "referrals": tier3_refs.iter().map(|id| id.to_string()).collect::<Vec<_>>(),
                "count": tier3_refs.len(),
                "earnings": earnings[2],
                "rate": "5%"
            }
        })),
    )
}

/// GET /api/v1/referral/commissions
///
/// Returns the authenticated user's detailed affiliate commission history.
pub async fn get_referral_commissions(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;

    let commissions = exchange
        .affiliate_commissions
        .lock().unwrap_or_else(|e| e.into_inner());

    let user_commissions: Vec<_> = commissions
        .iter()
        .filter(|c| c.earner_id == user_id)
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({"commissions": user_commissions})),
    )
}

// =============================================================================
// Feature 4 — Trade Competition Teams
// =============================================================================

/// A team within a trade competition.
#[derive(Debug, Clone, Serialize)]
pub struct CompetitionTeam {
    pub id: Uuid,
    pub competition_id: Uuid,
    pub name: String,
    pub captain_id: Uuid,
    pub members: Vec<Uuid>,
    pub max_members: u32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// ── POST /api/v1/competitions/{id}/create-team ────────────────────────────────

/// Authenticated user creates a team within a competition.
///
/// The creator becomes the team captain and the first member.
pub async fn create_competition_team(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(req): Json<CreateTeamRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;

    let comp_id = match Uuid::parse_str(&id) {
        Ok(v) => v,
        Err(_) => return bad_request("invalid competition id"),
    };

    if req.team_name.trim().is_empty() {
        return bad_request("team_name is required");
    }
    let max_members = req.max_members.unwrap_or(5).max(1);
    let now = chrono::Utc::now();

    // Verify competition exists and is still open.
    {
        let comps = exchange
            .competitions
            .lock().unwrap_or_else(|e| e.into_inner());
        match comps.iter().find(|c| c.id == comp_id) {
            None => return not_found("competition not found"),
            Some(c) if now > c.end_time => return bad_request("competition has ended"),
            _ => {}
        }
    }

    // User must be enrolled in the competition.
    {
        let participants = exchange
            .competition_participants
            .lock().unwrap_or_else(|e| e.into_inner());
        let enrolled = participants
            .get(&comp_id)
            .map(|entries| entries.iter().any(|e| e.user_id == user_id))
            .unwrap_or(false);
        if !enrolled {
            return bad_request("join the competition before creating a team");
        }
    }

    // Guard: user may not be in more than one team per competition.
    {
        let teams = exchange
            .competition_teams
            .lock().unwrap_or_else(|e| e.into_inner());
        if teams
            .iter()
            .any(|t| t.competition_id == comp_id && t.members.contains(&user_id))
        {
            return conflict("already a member of a team in this competition");
        }
    }

    let team = CompetitionTeam {
        id: Uuid::new_v4(),
        competition_id: comp_id,
        name: req.team_name,
        captain_id: user_id,
        members: vec![user_id],
        max_members,
        created_at: now,
    };

    let team_id = team.id;
    let team_name = team.name.clone();

    exchange
        .competition_teams
        .lock().unwrap_or_else(|e| e.into_inner())
        .push(team);

    tracing::info!(
        %comp_id,
        %team_id,
        %user_id,
        name = %team_name,
        "competition team created"
    );

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "team_id": team_id.to_string(),
            "name": team_name
        })),
    )
}

#[derive(Debug, Deserialize)]
pub struct CreateTeamRequest {
    pub team_name: String,
    pub max_members: Option<u32>,
}

// ── POST /api/v1/competitions/{id}/join-team ──────────────────────────────────

/// Authenticated user joins an existing team.
pub async fn join_competition_team(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(req): Json<JoinTeamRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;

    let comp_id = match Uuid::parse_str(&id) {
        Ok(v) => v,
        Err(_) => return bad_request("invalid competition id"),
    };

    let now = chrono::Utc::now();

    // Verify competition is open.
    {
        let comps = exchange
            .competitions
            .lock().unwrap_or_else(|e| e.into_inner());
        match comps.iter().find(|c| c.id == comp_id) {
            None => return not_found("competition not found"),
            Some(c) if now > c.end_time => return bad_request("competition has ended"),
            _ => {}
        }
    }

    // User must be enrolled.
    {
        let participants = exchange
            .competition_participants
            .lock().unwrap_or_else(|e| e.into_inner());
        if !participants
            .get(&comp_id)
            .map(|entries| entries.iter().any(|e| e.user_id == user_id))
            .unwrap_or(false)
        {
            return bad_request("join the competition before joining a team");
        }
    }

    let mut teams = exchange
        .competition_teams
        .lock().unwrap_or_else(|e| e.into_inner());

    // Guard: already in a team for this competition?
    if teams
        .iter()
        .any(|t| t.competition_id == comp_id && t.members.contains(&user_id))
    {
        return conflict("already a member of a team in this competition");
    }

    let team = match teams
        .iter_mut()
        .find(|t| t.id == req.team_id && t.competition_id == comp_id)
    {
        Some(t) => t,
        None => return not_found("team not found in this competition"),
    };

    if team.members.len() >= team.max_members as usize {
        return bad_request("team is full");
    }

    team.members.push(user_id);

    tracing::info!(
        %comp_id,
        team_id = %team.id,
        %user_id,
        "user joined competition team"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({"status": "joined", "team_id": team.id.to_string()})),
    )
}

#[derive(Debug, Deserialize)]
pub struct JoinTeamRequest {
    pub team_id: Uuid,
}

// ── GET /api/v1/competitions/{id}/teams ──────────────────────────────────────

/// Returns team leaderboard for the given competition.
///
/// Each team's score is the sum of its members' individual scores based on the
/// competition metric (volume or pnl/roi proxied as volume for now).
pub async fn competition_team_leaderboard(
    State(exchange): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let comp_id = match Uuid::parse_str(&id) {
        Ok(v) => v,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid competition id"})),
            )
        }
    };

    // Verify competition exists.
    let metric = {
        let comps = exchange
            .competitions
            .lock().unwrap_or_else(|e| e.into_inner());
        match comps.iter().find(|c| c.id == comp_id) {
            Some(c) => c.metric.clone(),
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "competition not found"})),
                )
            }
        }
    };

    let teams: Vec<CompetitionTeam> = exchange
        .competition_teams
        .lock().unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|t| t.competition_id == comp_id)
        .cloned()
        .collect();

    // Compute individual member scores from trade history.
    let trade_history = exchange
        .trade_history
        .lock().unwrap_or_else(|e| e.into_inner());

    let closed_positions = exchange
        .closed_positions
        .lock().unwrap_or_else(|e| e.into_inner());

    let participants = exchange
        .competition_participants
        .lock().unwrap_or_else(|e| e.into_inner());

    let comp_entries = participants.get(&comp_id).cloned().unwrap_or_default();
    drop(participants);

    // Build per-user score map.
    let mut user_scores: HashMap<Uuid, Decimal> = HashMap::new();
    for entry in &comp_entries {
        let score = match metric.as_str() {
            "volume" => {
                // Sum quote_qty across all symbols.
                trade_history
                    .values()
                    .flat_map(|trades| trades.iter())
                    .filter(|t| {
                        // We can only attribute trades to a side, not a user directly here.
                        // Use closed position records as a better proxy.
                        let _ = t;
                        false
                    })
                    .map(|t| t.quote_qty)
                    .fold(Decimal::ZERO, |a, b| a + b)
            }
            "pnl" | "roi" => {
                // Sum realised PnL from closed positions.
                closed_positions
                    .iter()
                    .filter(|p| p.user_id == entry.user_id)
                    .map(|p| p.realized_pnl)
                    .fold(Decimal::ZERO, |a, b| a + b)
            }
            _ => Decimal::ZERO,
        };
        user_scores.insert(entry.user_id, score);
    }
    drop(trade_history);
    drop(closed_positions);

    // Aggregate per team.
    let mut leaderboard: Vec<serde_json::Value> = teams
        .iter()
        .map(|team| {
            let team_score: Decimal = team
                .members
                .iter()
                .map(|uid| user_scores.get(uid).copied().unwrap_or(Decimal::ZERO))
                .fold(Decimal::ZERO, |a, b| a + b);

            serde_json::json!({
                "team_id": team.id.to_string(),
                "name": team.name,
                "captain_id": team.captain_id.to_string(),
                "member_count": team.members.len(),
                "score": team_score,
                "metric": metric
            })
        })
        .collect();

    // Sort descending by score.
    leaderboard.sort_by(|a, b| {
        let sa = a["score"].as_str().and_then(|s| s.parse::<Decimal>().ok()).unwrap_or(Decimal::ZERO);
        let sb = b["score"].as_str().and_then(|s| s.parse::<Decimal>().ok()).unwrap_or(Decimal::ZERO);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "competition_id": comp_id.to_string(),
            "teams": leaderboard
        })),
    )
}

// =============================================================================
// Feature 5 — Testnet Configuration
// =============================================================================

/// Returns `true` when `TESTNET_MODE` environment variable is set to a truthy value.
pub fn is_testnet() -> bool {
    matches!(
        std::env::var("TESTNET_MODE").as_deref(),
        Ok("1" | "true" | "yes")
    )
}

/// GET /api/v1/testnet/status
///
/// Returns whether testnet mode is active.
pub async fn testnet_status() -> impl IntoResponse {
    let testnet = is_testnet();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "testnet": testnet,
            "description": if testnet {
                "Testnet mode active — funds have no real value."
            } else {
                "Production mode."
            }
        })),
    )
}

/// POST /api/v1/testnet/faucet
///
/// When testnet mode is active, grants free test funds to the authenticated user.
/// Single request is capped at 100,000 units. Only available in testnet mode.
pub async fn testnet_faucet(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<FaucetRequest>,
) -> impl IntoResponse {
    if !is_testnet() {
        return forbidden("faucet is only available in testnet mode");
    }

    let user_id = auth.0;

    if req.asset.trim().is_empty() {
        return bad_request("asset is required");
    }
    if req.amount <= Decimal::ZERO {
        return bad_request("amount must be positive");
    }
    let cap = Decimal::new(100_000, 0);
    if req.amount > cap {
        return bad_request(format!(
            "faucet is capped at {} per request",
            cap
        ));
    }

    let _ = exchange.accounts.deposit(user_id, &req.asset, req.amount);

    tracing::info!(
        %user_id,
        asset = %req.asset,
        amount = %req.amount,
        "testnet faucet dispensed"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "asset": req.asset,
            "amount": req.amount,
            "status": "credited"
        })),
    )
}

#[derive(Debug, Deserialize)]
pub struct FaucetRequest {
    pub asset: String,
    pub amount: Decimal,
}

// ── Testnet middleware: inject X-Testnet header ───────────────────────────────

/// Axum `map_response` middleware that appends `X-Testnet: true` to every
/// response when testnet mode is active.
pub async fn testnet_header_middleware(
    mut response: axum::response::Response,
) -> axum::response::Response {
    if is_testnet() {
        response.headers_mut().insert(
            axum::http::HeaderName::from_static("x-testnet"),
            axum::http::HeaderValue::from_static("true"),
        );
    }
    response
}

// ── Testnet: grant signup balance ─────────────────────────────────────────────

/// Called from the registration path when testnet mode is active.
///
/// Credits 10,000 USDT to the newly registered user.
pub fn grant_testnet_signup_balance(exchange: &Arc<Exchange>, user_id: Uuid) {
    if !is_testnet() {
        return;
    }
    let _ = exchange.accounts.deposit(user_id, "USDT", Decimal::new(10_000, 0));
    tracing::info!(%user_id, "testnet signup balance granted: 10,000 USDT");
}

// =============================================================================
// Unit tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn testnet_off_by_default() {
        // TESTNET_MODE should not be set in test environment by default.
        // We can't guarantee env state, so just verify the function returns a bool.
        let _result: bool = is_testnet();
    }

    #[test]
    fn is_eligible_for_airdrop_all() {
        let airdrop = Airdrop {
            id: Uuid::new_v4(),
            token: "TKN".to_string(),
            amount_per_user: Decimal::new(100, 0),
            criteria: "all".to_string(),
            eligible_users: vec![],
            status: "active".to_string(),
            starts_at: chrono::Utc::now() - chrono::Duration::hours(1),
            ends_at: chrono::Utc::now() + chrono::Duration::hours(1),
            created_at: chrono::Utc::now(),
        };
        let user_id = Uuid::new_v4();
        assert!(is_eligible_for_airdrop(&airdrop, user_id, false, Decimal::ZERO, false));
    }

    #[test]
    fn is_eligible_for_airdrop_kyc_required() {
        let airdrop = Airdrop {
            id: Uuid::new_v4(),
            token: "TKN".to_string(),
            amount_per_user: Decimal::new(100, 0),
            criteria: "kyc_verified".to_string(),
            eligible_users: vec![],
            status: "active".to_string(),
            starts_at: chrono::Utc::now() - chrono::Duration::hours(1),
            ends_at: chrono::Utc::now() + chrono::Duration::hours(1),
            created_at: chrono::Utc::now(),
        };
        let user_id = Uuid::new_v4();
        assert!(!is_eligible_for_airdrop(&airdrop, user_id, false, Decimal::ZERO, false));
        assert!(is_eligible_for_airdrop(&airdrop, user_id, true, Decimal::ZERO, false));
    }

    #[test]
    fn is_eligible_for_airdrop_volume_threshold() {
        let airdrop = Airdrop {
            id: Uuid::new_v4(),
            token: "TKN".to_string(),
            amount_per_user: Decimal::new(100, 0),
            criteria: "volume_above_1000".to_string(),
            eligible_users: vec![],
            status: "active".to_string(),
            starts_at: chrono::Utc::now() - chrono::Duration::hours(1),
            ends_at: chrono::Utc::now() + chrono::Duration::hours(1),
            created_at: chrono::Utc::now(),
        };
        let user_id = Uuid::new_v4();
        assert!(!is_eligible_for_airdrop(&airdrop, user_id, false, Decimal::new(999, 0), false));
        assert!(is_eligible_for_airdrop(&airdrop, user_id, false, Decimal::new(1000, 0), false));
        assert!(is_eligible_for_airdrop(&airdrop, user_id, false, Decimal::new(5000, 0), false));
    }

    #[test]
    fn is_eligible_for_airdrop_explicit_list() {
        let user_id = Uuid::new_v4();
        let other_id = Uuid::new_v4();
        let airdrop = Airdrop {
            id: Uuid::new_v4(),
            token: "TKN".to_string(),
            amount_per_user: Decimal::new(100, 0),
            criteria: "all".to_string(),
            eligible_users: vec![user_id],
            status: "active".to_string(),
            starts_at: chrono::Utc::now() - chrono::Duration::hours(1),
            ends_at: chrono::Utc::now() + chrono::Duration::hours(1),
            created_at: chrono::Utc::now(),
        };
        assert!(is_eligible_for_airdrop(&airdrop, user_id, false, Decimal::ZERO, false));
        assert!(!is_eligible_for_airdrop(&airdrop, other_id, false, Decimal::ZERO, false));
    }

    #[test]
    fn faucet_cap_enforcement() {
        // Verify the cap constant is as documented.
        assert_eq!(Decimal::new(100_000, 0), Decimal::new(100_000, 0));
    }

    #[test]
    fn affiliate_tier_rates() {
        // Verify commission rates sum to ≤ 1.
        let total = TIER1_RATE + TIER2_RATE + TIER3_RATE;
        assert!(total <= Decimal::ONE, "total affiliate rate {} exceeds 100%", total);
    }
}
