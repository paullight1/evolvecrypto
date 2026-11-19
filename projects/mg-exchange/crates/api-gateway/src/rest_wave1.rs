//! W1.3 Copy Trading, W1.4 Trading Bots, W1.5 Settings & Referral endpoints.
//!
//! These handlers are registered in `server::build_router` alongside the
//! existing REST handlers.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use exchange_trading_bots::{BotStatus, DcaBot, DcaBotConfig, GridBot, GridBotConfig, GridSide};
use rust_decimal::Decimal;
use serde::Deserialize;
use uuid::Uuid;

use exchange_common::Symbol;

use crate::auth::AuthenticatedUser;
use crate::rest::AppState;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn bad_request(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": msg.into()})))
}

fn not_found(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": msg.into()})))
}

// =============================================================================
// W1.3 — Copy Trading
// =============================================================================

/// Query parameters for the leaderboard endpoint.
#[derive(Debug, Deserialize)]
pub struct LeaderboardQuery {
    /// Time-frame filter (accepted but not yet applied server-side — stored for
    /// future use when historical per-period stats are tracked).
    #[serde(default)]
    pub timeframe: Option<String>,
    /// Maximum number of masters to return. Defaults to 20.
    #[serde(default = "default_leaderboard_limit")]
    pub limit: usize,
}

fn default_leaderboard_limit() -> usize {
    20
}

/// GET /api/v1/copy-trading/leaderboard?timeframe=30d&limit=20
///
/// Returns the top `limit` master traders sorted by total PnL descending.
/// Public endpoint — no authentication required.
pub async fn copy_trading_leaderboard(
    State(exchange): State<AppState>,
    Query(params): Query<LeaderboardQuery>,
) -> impl IntoResponse {
    let limit = params.limit.min(100);
    let masters = exchange.copy_trading_masters.lock().unwrap_or_else(|e| e.into_inner());
    let board = masters.leaderboard(limit);

    let result: Vec<serde_json::Value> = board
        .iter()
        .map(|m| {
            serde_json::json!({
                "user_id": m.user_id.to_string(),
                "display_name": m.display_name,
                "total_pnl": m.total_pnl,
                "win_rate": m.win_rate,
                "followers_count": m.followers_count,
                "profit_share_rate": m.profit_share_rate,
                "is_active": m.is_active,
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(result)))
}

/// Request body for POST /api/v1/copy-trading/follow.
#[derive(Debug, Deserialize)]
pub struct FollowRequest {
    pub master_id: Uuid,
    pub allocation: Decimal,
    #[serde(default = "default_proportional")]
    pub proportional: bool,
    pub stop_loss_pct: Option<Decimal>,
    /// M23-4: Must be true to acknowledge copy-trading risk disclosure.
    pub risk_acknowledged: Option<bool>,
}

fn default_proportional() -> bool {
    true
}

/// POST /api/v1/copy-trading/follow  (auth required)
///
/// Start following a master trader.  Returns the new relation ID.
pub async fn copy_trading_follow(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<FollowRequest>,
) -> impl IntoResponse {
    if req.allocation <= Decimal::ZERO {
        return bad_request("allocation must be positive");
    }

    let follower_id = auth.0;
    let mut followers = exchange.copy_trading_followers.lock().unwrap_or_else(|e| e.into_inner());

    let risk_acknowledged = req.risk_acknowledged.unwrap_or(false);
    match followers.follow(follower_id, req.master_id, req.allocation, req.proportional, req.stop_loss_pct, risk_acknowledged) {
        Ok(relation) => (
            StatusCode::OK,
            Json(serde_json::json!({"relation_id": relation.id.to_string()})),
        ),
        Err(e) => bad_request(e.to_string()),
    }
}

/// DELETE /api/v1/copy-trading/follow/:relation_id  (auth required)
///
/// Stop following a master. The relation is deactivated, not deleted.
pub async fn copy_trading_unfollow(
    State(exchange): State<AppState>,
    axum::Extension(_auth): axum::Extension<AuthenticatedUser>,
    Path(relation_id): Path<String>,
) -> impl IntoResponse {
    let relation_id = match Uuid::parse_str(&relation_id) {
        Ok(id) => id,
        Err(_) => return bad_request("invalid relation_id"),
    };

    let mut followers = exchange.copy_trading_followers.lock().unwrap_or_else(|e| e.into_inner());
    followers.unfollow(relation_id);

    (StatusCode::OK, Json(serde_json::json!({"status": "unfollowed"})))
}

/// GET /api/v1/copy-trading/following  (auth required)
///
/// Returns all active follow relations for the authenticated user.
pub async fn copy_trading_following(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let follower_id = auth.0;
    let followers = exchange.copy_trading_followers.lock().unwrap_or_else(|e| e.into_inner());
    let relations = followers.follower_masters(follower_id);

    let result: Vec<serde_json::Value> = relations
        .iter()
        .map(|r| {
            serde_json::json!({
                "relation_id": r.id.to_string(),
                "master_id": r.master_id.to_string(),
                "allocation": r.allocation,
                "used_allocation": r.used_allocation,
                "proportional": r.proportional,
                "stop_loss_pct": r.stop_loss_pct,
                "total_pnl": r.total_pnl,
                "is_active": r.is_active,
                "created_at": r.created_at.to_rfc3339(),
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(result)))
}

/// GET /api/v1/copy-trading/masters/:user_id  (public)
///
/// Returns the public profile of a specific master trader.
pub async fn copy_trading_master_profile(
    State(exchange): State<AppState>,
    Path(user_id): Path<String>,
) -> impl IntoResponse {
    let user_id = match Uuid::parse_str(&user_id) {
        Ok(id) => id,
        Err(_) => return bad_request("invalid user_id"),
    };

    let masters = exchange.copy_trading_masters.lock().unwrap_or_else(|e| e.into_inner());

    match masters.get(user_id) {
        Some(m) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "user_id": m.user_id.to_string(),
                "display_name": m.display_name,
                "total_pnl": m.total_pnl,
                "total_trades": m.total_trades,
                "win_rate": m.win_rate,
                "followers_count": m.followers_count,
                "max_followers": m.max_followers,
                "profit_share_rate": m.profit_share_rate,
                "is_active": m.is_active,
                "registered_at": m.registered_at.to_rfc3339(),
            })),
        ),
        None => not_found("master not found"),
    }
}

// =============================================================================
// W1.4 — Trading Bots
// =============================================================================

/// Request body for POST /api/v1/bots/grid.
#[derive(Debug, Deserialize)]
pub struct CreateGridBotRequest {
    pub symbol: String,
    pub upper_price: Decimal,
    pub lower_price: Decimal,
    pub grid_count: u32,
    pub total_investment: Decimal,
    /// "neutral", "long", or "short". Defaults to "neutral".
    #[serde(default = "default_grid_side_str")]
    pub side: String,
}

fn default_grid_side_str() -> String {
    "neutral".to_string()
}

/// POST /api/v1/bots/grid  (auth required)
///
/// Create a new grid trading bot for the authenticated user.
pub async fn create_grid_bot(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<CreateGridBotRequest>,
) -> impl IntoResponse {
    if req.upper_price <= req.lower_price {
        return bad_request("upper_price must be greater than lower_price");
    }
    if req.grid_count < 2 {
        return bad_request("grid_count must be at least 2");
    }
    if req.total_investment <= Decimal::ZERO {
        return bad_request("total_investment must be positive");
    }

    let side = match req.side.to_lowercase().as_str() {
        "neutral" => GridSide::Neutral,
        "long" => GridSide::Long,
        "short" => GridSide::Short,
        _ => return bad_request("side must be 'neutral', 'long', or 'short'"),
    };

    let user_id = auth.0;
    let config = GridBotConfig {
        user_id,
        symbol: Symbol::new(&req.symbol),
        upper_price: req.upper_price,
        lower_price: req.lower_price,
        grid_count: req.grid_count,
        total_investment: req.total_investment,
        side,
    };

    let bot = GridBot::new(config);
    let bot_id = bot.id;

    exchange.grid_bots.lock().unwrap_or_else(|e| e.into_inner()).push(bot);

    tracing::info!(
        user_id = %user_id,
        bot_id = %bot_id,
        symbol = %req.symbol,
        "grid bot created"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "bot_id": bot_id.to_string(),
            "status": "active",
        })),
    )
}

/// Request body for POST /api/v1/bots/dca.
#[derive(Debug, Deserialize)]
pub struct CreateDcaBotRequest {
    pub symbol: String,
    pub amount_per_order: Decimal,
    pub interval_secs: u64,
    pub total_orders: Option<u32>,
    pub price_deviation_pct: Option<Decimal>,
}

/// POST /api/v1/bots/dca  (auth required)
///
/// Create a new DCA (dollar-cost averaging) bot for the authenticated user.
pub async fn create_dca_bot(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<CreateDcaBotRequest>,
) -> impl IntoResponse {
    if req.amount_per_order <= Decimal::ZERO {
        return bad_request("amount_per_order must be positive");
    }
    if req.interval_secs == 0 {
        return bad_request("interval_secs must be greater than 0");
    }

    let user_id = auth.0;
    let config = DcaBotConfig {
        user_id,
        symbol: Symbol::new(&req.symbol),
        amount_per_order: req.amount_per_order,
        interval_secs: req.interval_secs,
        total_orders: req.total_orders,
        price_deviation_pct: req.price_deviation_pct,
    };

    let bot = DcaBot::new(config);
    let bot_id = bot.id;

    exchange.dca_bots.lock().unwrap_or_else(|e| e.into_inner()).push(bot);

    tracing::info!(
        user_id = %user_id,
        bot_id = %bot_id,
        symbol = %req.symbol,
        "dca bot created"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "bot_id": bot_id.to_string(),
            "status": "active",
        })),
    )
}

/// GET /api/v1/bots  (auth required)
///
/// Returns all bots (grid + DCA) belonging to the authenticated user.
pub async fn list_bots(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;

    let grid_bots = exchange.grid_bots.lock().unwrap_or_else(|e| e.into_inner());
    let dca_bots = exchange.dca_bots.lock().unwrap_or_else(|e| e.into_inner());

    let mut result: Vec<serde_json::Value> = Vec::new();

    for bot in grid_bots.iter().filter(|b| b.config.user_id == user_id) {
        result.push(serde_json::json!({
            "bot_id": bot.id.to_string(),
            "type": "grid",
            "symbol": bot.config.symbol.to_string(),
            "status": bot_status_str(bot.status),
            "realized_pnl": bot.realized_pnl,
            "total_trades": bot.total_trades,
            "created_at": bot.created_at.to_rfc3339(),
        }));
    }

    for bot in dca_bots.iter().filter(|b| b.config.user_id == user_id) {
        result.push(serde_json::json!({
            "bot_id": bot.id.to_string(),
            "type": "dca",
            "symbol": bot.config.symbol.to_string(),
            "status": bot_status_str(bot.status),
            "orders_placed": bot.orders_placed,
            "avg_price": bot.avg_price,
            "total_invested": bot.total_invested,
            "created_at": bot.created_at.to_rfc3339(),
        }));
    }

    (StatusCode::OK, Json(serde_json::json!(result)))
}

fn bot_status_str(status: BotStatus) -> &'static str {
    match status {
        BotStatus::Active => "active",
        BotStatus::Paused => "paused",
        BotStatus::Stopped => "stopped",
    }
}

/// POST /api/v1/bots/:id/pause  (auth required)
pub async fn pause_bot(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let bot_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => return bad_request("invalid bot id"),
    };

    let user_id = auth.0;

    // Try grid bots first, then DCA bots.
    {
        let mut grid_bots = exchange.grid_bots.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(bot) = grid_bots.iter_mut().find(|b| b.id == bot_id && b.config.user_id == user_id) {
            if bot.status == BotStatus::Stopped {
                return bad_request("cannot pause a stopped bot");
            }
            bot.status = BotStatus::Paused;
            return (StatusCode::OK, Json(serde_json::json!({"status": "paused"})));
        }
    }

    {
        let mut dca_bots = exchange.dca_bots.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(bot) = dca_bots.iter_mut().find(|b| b.id == bot_id && b.config.user_id == user_id) {
            if bot.status == BotStatus::Stopped {
                return bad_request("cannot pause a stopped bot");
            }
            bot.status = BotStatus::Paused;
            return (StatusCode::OK, Json(serde_json::json!({"status": "paused"})));
        }
    }

    not_found("bot not found")
}

/// POST /api/v1/bots/:id/resume  (auth required)
pub async fn resume_bot(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let bot_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => return bad_request("invalid bot id"),
    };

    let user_id = auth.0;

    {
        let mut grid_bots = exchange.grid_bots.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(bot) = grid_bots.iter_mut().find(|b| b.id == bot_id && b.config.user_id == user_id) {
            if bot.status == BotStatus::Stopped {
                return bad_request("cannot resume a stopped bot");
            }
            bot.status = BotStatus::Active;
            return (StatusCode::OK, Json(serde_json::json!({"status": "active"})));
        }
    }

    {
        let mut dca_bots = exchange.dca_bots.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(bot) = dca_bots.iter_mut().find(|b| b.id == bot_id && b.config.user_id == user_id) {
            if bot.status == BotStatus::Stopped {
                return bad_request("cannot resume a stopped bot");
            }
            bot.status = BotStatus::Active;
            return (StatusCode::OK, Json(serde_json::json!({"status": "active"})));
        }
    }

    not_found("bot not found")
}

/// POST /api/v1/bots/:id/stop  (auth required)
pub async fn stop_bot(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let bot_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => return bad_request("invalid bot id"),
    };

    let user_id = auth.0;

    {
        let mut grid_bots = exchange.grid_bots.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(bot) = grid_bots.iter_mut().find(|b| b.id == bot_id && b.config.user_id == user_id) {
            let cancelled_orders = bot.stop();
            // Cancel all resting orders from the matching engine.
            drop(grid_bots); // release lock before calling cancel_order
            for oid in cancelled_orders {
                let cmd = exchange_common::CancelOrderCommand {
                    order_id: oid,
                    user_id,
                    symbol: exchange_common::Symbol::new(""),
                    timestamp: chrono::Utc::now(),
                };
                if let Err(e) = exchange.cancel_order(cmd) {
                    tracing::warn!(order_id = %oid, error = %e, "failed to cancel grid bot order");
                }
            }
            tracing::info!(bot_id = %bot_id, user_id = %user_id, "grid bot stopped");
            return (StatusCode::OK, Json(serde_json::json!({"status": "stopped"})));
        }
    }

    {
        let mut dca_bots = exchange.dca_bots.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(bot) = dca_bots.iter_mut().find(|b| b.id == bot_id && b.config.user_id == user_id) {
            bot.status = BotStatus::Stopped;
            tracing::info!(bot_id = %bot_id, user_id = %user_id, "dca bot stopped");
            return (StatusCode::OK, Json(serde_json::json!({"status": "stopped"})));
        }
    }

    not_found("bot not found")
}

// =============================================================================
// W1.5 — Account Settings & Referral
// =============================================================================

/// Default account settings returned when the user has no custom preferences.
fn default_settings() -> serde_json::Value {
    serde_json::json!({
        "default_leverage": 10,
        "default_order_type": "limit",
        "margin_mode": "isolated",
        "notifications": {
            "order_fill": true,
            "liquidation_warning": true,
            "funding_payment": false,
            "price_alert": true,
        }
    })
}

/// GET /api/v1/account/settings  (auth required)
///
/// Returns the authenticated user's trading preferences.
/// Merges stored settings over the defaults so unset keys always have a value.
pub async fn get_account_settings(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let settings_map = exchange.user_settings.lock().unwrap_or_else(|e| e.into_inner());

    let settings = match settings_map.get(&user_id) {
        Some(stored) => {
            let mut merged = default_settings();
            // Shallow-merge stored values over defaults.
            if let (Some(merged_obj), Some(stored_obj)) =
                (merged.as_object_mut(), stored.as_object())
            {
                for (k, v) in stored_obj {
                    merged_obj.insert(k.clone(), v.clone());
                }
            }
            merged
        }
        None => default_settings(),
    };

    (StatusCode::OK, Json(settings))
}

/// PUT /api/v1/account/settings  (auth required)
///
/// Partially updates the authenticated user's trading preferences.
/// Only keys present in the request body are updated.
pub async fn update_account_settings(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(patch): Json<serde_json::Value>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let mut settings_map = exchange.user_settings.lock().unwrap_or_else(|e| e.into_inner());

    let entry = settings_map.entry(user_id).or_insert_with(default_settings);

    // Merge patch into stored settings.
    if let (Some(existing_obj), Some(patch_obj)) = (entry.as_object_mut(), patch.as_object()) {
        for (k, v) in patch_obj {
            existing_obj.insert(k.clone(), v.clone());
        }
    }

    tracing::debug!(user_id = %user_id, "account settings updated");

    (StatusCode::OK, Json(entry.clone()))
}

/// GET /api/v1/referral  (auth required)
///
/// Returns the authenticated user's referral code, referral count, total
/// earned commissions, and recent commission records.
pub async fn get_referral_info(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let mut referrals = exchange.referrals.lock().unwrap_or_else(|e| e.into_inner());

    // generate_code is idempotent — safe to call on every request.
    let code = referrals.generate_code(user_id);
    let total_referred = referrals.total_referred_count(user_id);
    let commissions_by_asset = referrals.get_commissions(user_id);
    let total_earned_usdt = commissions_by_asset
        .get("USDT")
        .copied()
        .unwrap_or(Decimal::ZERO);

    let history = referrals.commission_history(user_id, 20);
    let commissions_json: Vec<serde_json::Value> = history
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id.to_string(),
                "trader_id": r.trader_id.to_string(),
                "trade_id": r.trade_id,
                "fee_amount": r.fee_amount,
                "commission": r.commission,
                "asset": r.asset,
                "level": r.level,
                "timestamp": r.timestamp.to_rfc3339(),
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "code": code,
            "total_referred": total_referred,
            "total_earned_usdt": total_earned_usdt,
            "commissions_by_asset": commissions_by_asset,
            "commissions": commissions_json,
        })),
    )
}

/// Request body for POST /api/v1/referral/invite.
#[derive(Debug, Deserialize)]
pub struct ReferralInviteRequest {
    pub email: String,
}

/// POST /api/v1/referral/invite  (auth required)
///
/// Logs an invite intent.  Actual email delivery is handled by a separate
/// notification service and is out of scope for Phase 1.
pub async fn referral_invite(
    State(_exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<ReferralInviteRequest>,
) -> impl IntoResponse {
    if req.email.is_empty() || !req.email.contains('@') {
        return bad_request("invalid email address");
    }

    tracing::info!(
        user_id = %auth.0,
        email = %req.email,
        "referral invite logged — email delivery handled externally"
    );

    (StatusCode::OK, Json(serde_json::json!({"status": "invite_logged"})))
}

/// GET /api/v1/markets/all  (public)
///
/// Returns all configured trading pairs with basic ticker information derived
/// from the order book (best bid/ask, spread).
pub async fn get_all_markets(State(exchange): State<AppState>) -> impl IntoResponse {
    let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());

    let markets: Vec<serde_json::Value> = pool
        .symbols()
        .iter()
        .filter_map(|sym| {
            pool.snapshot(sym, 1).ok().map(|snap| {
                let best_bid = snap.bids.first().map(|(p, _)| *p);
                let best_ask = snap.asks.first().map(|(p, _)| *p);
                let spread = match (snap.bids.first(), snap.asks.first()) {
                    (Some((bid, _)), Some((ask, _))) => Some(ask - bid),
                    _ => None,
                };
                let is_perp = exchange.perp_configs.load().contains_key(sym);

                let product = exchange.accounts.get_product(sym);
                let product_type_str = product.as_ref()
                    .map(|p| format!("{:?}", p.product_type))
                    .unwrap_or_else(|| if is_perp { "Perpetual".to_string() } else { "Spot".to_string() });

                serde_json::json!({
                    "symbol": sym.to_string(),
                    "type": if is_perp { "perpetual" } else { "spot" },
                    "product_type": product_type_str,
                    "base_asset": product.as_ref().map(|p| p.base_asset.clone()).unwrap_or_default(),
                    "quote_asset": product.as_ref().map(|p| p.quote_asset.clone()).unwrap_or_default(),
                    "best_bid": best_bid,
                    "best_ask": best_ask,
                    "spread": spread,
                    "order_count": snap.bids.len() + snap.asks.len(),
                })
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(markets)))
}
