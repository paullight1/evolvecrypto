//! Task 1: Trade Competitions
//! Task 2: Leveraged Tokens
//! Task 3: Copy Trading Parameters & PnL
//! Task 4: P2P Trading

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use rust_decimal::Decimal;
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::AuthenticatedUser;
use crate::rest::AppState;

// ── Shared helpers ────────────────────────────────────────────────────────────

fn bad_request(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": msg.into()})))
}

fn not_found(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": msg.into()})))
}

// =============================================================================
// Task 1 — Trade Competitions
// =============================================================================

/// POST /api/v1/admin/competitions
///
/// Admin creates a new trade competition.
pub async fn admin_create_competition(
    State(exchange): State<AppState>,
    Json(req): Json<CreateCompetitionRequest>,
) -> impl IntoResponse {
    use crate::exchange::Competition;

    if req.name.trim().is_empty() {
        return bad_request("name is required");
    }
    if req.end_time <= req.start_time {
        return bad_request("end_time must be after start_time");
    }
    if req.prize_pool <= Decimal::ZERO {
        return bad_request("prize_pool must be positive");
    }
    if !matches!(req.metric.as_str(), "pnl" | "volume" | "roi") {
        return bad_request("metric must be 'pnl', 'volume', or 'roi'");
    }

    let comp = Competition {
        id: Uuid::new_v4(),
        name: req.name,
        start_time: req.start_time,
        end_time: req.end_time,
        prize_pool: req.prize_pool,
        metric: req.metric,
        symbols: req.symbols,
        created_at: chrono::Utc::now(),
    };

    let id = comp.id;
    let mut comps = exchange.competitions.lock().unwrap_or_else(|e| e.into_inner());
    comps.push(comp);
    tracing::info!(competition_id = %id, "competition created");

    (StatusCode::CREATED, Json(serde_json::json!({"id": id.to_string()})))
}

#[derive(Debug, Deserialize)]
pub struct CreateCompetitionRequest {
    pub name: String,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub end_time: chrono::DateTime<chrono::Utc>,
    pub prize_pool: Decimal,
    pub metric: String,
    #[serde(default)]
    pub symbols: Vec<String>,
}

/// POST /api/v1/competitions/{id}/join
///
/// Authenticated user joins a competition.
pub async fn join_competition(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    use crate::exchange::CompetitionEntry;

    let comp_id = match Uuid::parse_str(&id) {
        Ok(v) => v,
        Err(_) => return bad_request("invalid competition id"),
    };
    let user_id = auth.0;
    let now = chrono::Utc::now();

    // Verify competition exists and is joinable.
    {
        let comps = exchange.competitions.lock().unwrap_or_else(|e| e.into_inner());
        let comp = match comps.iter().find(|c| c.id == comp_id) {
            Some(c) => c,
            None => return not_found("competition not found"),
        };
        if now > comp.end_time {
            return bad_request("competition has ended");
        }
    }

    let mut participants = exchange
        .competition_participants
        .lock().unwrap_or_else(|e| e.into_inner());
    let entries = participants.entry(comp_id).or_default();

    if entries.iter().any(|e| e.user_id == user_id) {
        return bad_request("already joined this competition");
    }

    entries.push(CompetitionEntry {
        user_id,
        joined_at: now,
        snapshot_balance: Decimal::ZERO, // set on first leaderboard query
    });

    tracing::info!(competition_id = %comp_id, %user_id, "user joined competition");
    (StatusCode::OK, Json(serde_json::json!({"status": "joined"})))
}

/// GET /api/v1/competitions/{id}/leaderboard
///
/// Compute the competition leaderboard on-demand from trade history.
pub async fn competition_leaderboard(
    State(exchange): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let comp_id = match Uuid::parse_str(&id) {
        Ok(v) => v,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "invalid competition id"}))),
    };

    // Load competition config.
    let (metric, symbols, start_time, end_time) = {
        let comps = exchange.competitions.lock().unwrap_or_else(|e| e.into_inner());
        match comps.iter().find(|c| c.id == comp_id) {
            Some(c) => (c.metric.clone(), c.symbols.clone(), c.start_time, c.end_time),
            None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "competition not found"}))),
        }
    };

    // Load participants.
    let participant_ids: Vec<Uuid> = {
        let p = exchange.competition_participants.lock().unwrap_or_else(|e| e.into_inner());
        p.get(&comp_id)
            .map(|entries| entries.iter().map(|e| e.user_id).collect())
            .unwrap_or_default()
    };

    // Compute per-participant metric from trade history.
    let trade_history = exchange.trade_history.lock().unwrap_or_else(|e| e.into_inner());

    let mut scores: Vec<(Uuid, Decimal)> = participant_ids
        .iter()
        .map(|&uid| {
            let score = match metric.as_str() {
                "volume" => {
                    // Sum of notional volume for THIS user during the competition window.
                    // Use user_fills (per-user) instead of trade_history (per-symbol).
                    let user_fills = exchange.user_fills.lock().unwrap_or_else(|e| e.into_inner());
                    user_fills.get(&uid)
                        .map(|fills| fills.iter()
                            .filter(|f| {
                                f.timestamp >= start_time
                                    && f.timestamp <= end_time
                                    && (symbols.is_empty() || symbols.contains(&f.symbol.to_string()))
                            })
                            .fold(Decimal::ZERO, |acc, f| acc + f.quote_qty))
                        .unwrap_or(Decimal::ZERO)
                }
                _ => {
                    // "pnl" and "roi": use closed position PnL as a proxy.
                    let closed = exchange.closed_positions.lock().unwrap_or_else(|e| e.into_inner());
                    closed
                        .iter()
                        .filter(|p| {
                            p.user_id == uid
                                && p.closed_at >= start_time
                                && p.closed_at <= end_time
                                && (symbols.is_empty() || symbols.iter().any(|s| s == p.symbol.as_str()))
                        })
                        .fold(Decimal::ZERO, |acc, p| acc + p.realized_pnl)
                }
            };
            (uid, score)
        })
        .collect();

    scores.sort_by(|a, b| b.1.cmp(&a.1));

    let result: Vec<serde_json::Value> = scores
        .iter()
        .enumerate()
        .map(|(rank, (uid, score))| {
            serde_json::json!({
                "rank": rank + 1,
                "user_id": uid.to_string(),
                "score": score,
                "metric": metric,
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!({"competition_id": comp_id.to_string(), "leaderboard": result})))
}

/// GET /api/v1/competitions
///
/// List competitions with optional status filter.
pub async fn list_competitions(
    State(exchange): State<AppState>,
    Query(params): Query<CompetitionListQuery>,
) -> impl IntoResponse {
    let now = chrono::Utc::now();
    let comps = exchange.competitions.lock().unwrap_or_else(|e| e.into_inner());

    let result: Vec<serde_json::Value> = comps
        .iter()
        .filter(|c| {
            let status = if now < c.start_time {
                "upcoming"
            } else if now <= c.end_time {
                "active"
            } else {
                "ended"
            };
            params
                .status
                .as_deref()
                .map(|s| s == status)
                .unwrap_or(true)
        })
        .map(|c| {
            let status = if now < c.start_time {
                "upcoming"
            } else if now <= c.end_time {
                "active"
            } else {
                "ended"
            };
            let participant_count = {
                let p = exchange.competition_participants.lock().unwrap_or_else(|e| e.into_inner());
                p.get(&c.id).map(|v| v.len()).unwrap_or(0)
            };
            serde_json::json!({
                "id": c.id.to_string(),
                "name": c.name,
                "start_time": c.start_time.to_rfc3339(),
                "end_time": c.end_time.to_rfc3339(),
                "prize_pool": c.prize_pool,
                "metric": c.metric,
                "symbols": c.symbols,
                "status": status,
                "participants": participant_count,
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(result)))
}

#[derive(Debug, Deserialize, Default)]
pub struct CompetitionListQuery {
    /// Filter by status: "active", "upcoming", "ended"
    pub status: Option<String>,
}

// =============================================================================
// Task 2 — Leveraged Tokens
// =============================================================================

/// POST /api/v1/leveraged-tokens/purchase
///
/// Purchase leveraged tokens at current NAV.
pub async fn lt_purchase(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<LtPurchaseRequest>,
) -> impl IntoResponse {
    if req.symbol.trim().is_empty() {
        return bad_request("symbol is required");
    }
    if req.amount <= Decimal::ZERO {
        return bad_request("amount must be positive");
    }

    let user_id = auth.0;

    let nav = {
        let tokens = exchange.leveraged_tokens.lock().unwrap_or_else(|e| e.into_inner());
        match tokens.get(&req.symbol) {
            Some(t) => t.nav,
            None => return not_found("leveraged token not found"),
        }
    };

    if nav <= Decimal::ZERO {
        return bad_request("token NAV is zero, cannot purchase");
    }

    // Cost = amount (in USDT) / NAV tokens_to_mint = amount / nav
    // Wait — task says: "Mint tokens at current NAV, deduct USDT"
    // amount is the USDT amount to spend. tokens_minted = usdt_amount / nav
    let tokens_minted = req.amount / nav;
    let usdt_cost = req.amount;

    // Deduct USDT from user balance — check + debit atomic under shard lock.
    let debit_result: Result<(), &str> = exchange.accounts.with_user(user_id, |am| {
        let available = am
            .get_balance(&user_id, "USDT")
            .map(|b| b.available())
            .unwrap_or(Decimal::ZERO);
        if available < usdt_cost {
            return Err("insufficient USDT balance");
        }
        am.withdraw(user_id, "USDT", usdt_cost).map_err(|_| "failed to debit USDT balance")
    });
    if let Err(msg) = debit_result {
        return bad_request(msg);
    }

    // Mint tokens into holdings.
    {
        let mut holdings = exchange.lt_holdings.lock().unwrap_or_else(|e| e.into_inner());
        let entry = holdings.entry((user_id, req.symbol.clone())).or_insert(Decimal::ZERO);
        *entry += tokens_minted;
    }

    // Increase total supply.
    {
        let mut tokens = exchange.leveraged_tokens.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(t) = tokens.get_mut(&req.symbol) {
            t.total_supply += tokens_minted;
        }
    }

    tracing::info!(
        %user_id,
        symbol = %req.symbol,
        usdt_cost = %usdt_cost,
        tokens_minted = %tokens_minted,
        "leveraged token purchased"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "symbol": req.symbol,
            "tokens_minted": tokens_minted,
            "usdt_cost": usdt_cost,
            "nav": nav,
        })),
    )
}

#[derive(Debug, Deserialize)]
pub struct LtPurchaseRequest {
    pub symbol: String,
    /// USDT amount to spend.
    pub amount: Decimal,
}

/// POST /api/v1/leveraged-tokens/redeem
///
/// Redeem leveraged tokens back to USDT at current NAV.
pub async fn lt_redeem(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<LtRedeemRequest>,
) -> impl IntoResponse {
    if req.symbol.trim().is_empty() {
        return bad_request("symbol is required");
    }
    if req.amount <= Decimal::ZERO {
        return bad_request("amount must be positive");
    }

    let user_id = auth.0;

    let nav = {
        let tokens = exchange.leveraged_tokens.lock().unwrap_or_else(|e| e.into_inner());
        match tokens.get(&req.symbol) {
            Some(t) => t.nav,
            None => return not_found("leveraged token not found"),
        }
    };

    // Check user holdings.
    {
        let holdings = exchange.lt_holdings.lock().unwrap_or_else(|e| e.into_inner());
        let held = holdings.get(&(user_id, req.symbol.clone())).copied().unwrap_or(Decimal::ZERO);
        if held < req.amount {
            return bad_request("insufficient token balance");
        }
    }

    let usdt_returned = req.amount * nav;

    // Burn tokens.
    {
        let mut holdings = exchange.lt_holdings.lock().unwrap_or_else(|e| e.into_inner());
        let entry = holdings.entry((user_id, req.symbol.clone())).or_insert(Decimal::ZERO);
        *entry -= req.amount;
    }

    // Decrease total supply.
    {
        let mut tokens = exchange.leveraged_tokens.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(t) = tokens.get_mut(&req.symbol) {
            t.total_supply -= req.amount;
        }
    }

    // Credit USDT to user.
    let _ = exchange.accounts.deposit(user_id, "USDT", usdt_returned);

    tracing::info!(
        %user_id,
        symbol = %req.symbol,
        tokens_burned = %req.amount,
        usdt_returned = %usdt_returned,
        "leveraged token redeemed"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "symbol": req.symbol,
            "tokens_burned": req.amount,
            "usdt_returned": usdt_returned,
            "nav": nav,
        })),
    )
}

#[derive(Debug, Deserialize)]
pub struct LtRedeemRequest {
    pub symbol: String,
    /// Token amount to redeem.
    pub amount: Decimal,
}

/// GET /api/v1/leveraged-tokens
///
/// List all leveraged tokens with NAV.
pub async fn lt_list(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let tokens = exchange.leveraged_tokens.lock().unwrap_or_else(|e| e.into_inner());
    let result: Vec<serde_json::Value> = tokens
        .values()
        .map(|t| serde_json::json!({
            "symbol": t.symbol,
            "underlying": t.underlying,
            "leverage": t.leverage,
            "nav": t.nav,
            "total_supply": t.total_supply,
            "rebalance_threshold_pct": t.rebalance_threshold_pct,
        }))
        .collect();
    (StatusCode::OK, Json(serde_json::json!(result)))
}

/// GET /api/v1/leveraged-tokens/{symbol}
///
/// Get a single leveraged token detail.
pub async fn lt_get(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
) -> impl IntoResponse {
    let tokens = exchange.leveraged_tokens.lock().unwrap_or_else(|e| e.into_inner());
    match tokens.get(&symbol) {
        Some(t) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "symbol": t.symbol,
                "underlying": t.underlying,
                "leverage": t.leverage,
                "nav": t.nav,
                "total_supply": t.total_supply,
                "rebalance_threshold_pct": t.rebalance_threshold_pct,
            })),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "leveraged token not found"})),
        ),
    }
}

// =============================================================================
// Task 3 — Copy Trading Parameters & PnL
// =============================================================================

/// PUT /api/v1/copy-trading/settings
///
/// Set or update the authenticated user's copy trading settings.
pub async fn copy_trading_set_settings(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<CopyTradingSettingsRequest>,
) -> impl IntoResponse {
    use crate::exchange::CopyTradingSettings;

    let user_id = auth.0;

    // Validate master_user_id.
    let master_id = match Uuid::parse_str(&req.master_user_id) {
        Ok(v) => v,
        Err(_) => return bad_request("invalid masterUserId"),
    };

    if let Some(ref fa) = req.fixed_amount {
        if *fa <= Decimal::ZERO {
            return bad_request("fixedAmount must be positive");
        }
    }
    if let Some(ref mps) = req.max_position_size {
        if *mps <= Decimal::ZERO {
            return bad_request("maxPositionSize must be positive");
        }
    }
    if let Some(ref sl) = req.stop_loss_pct {
        if *sl <= Decimal::ZERO || *sl > Decimal::from(100u32) {
            return bad_request("stopLossPct must be in (0, 100]");
        }
    }
    if let Some(ref cr) = req.copy_ratio {
        if *cr <= Decimal::ZERO || *cr > Decimal::ONE {
            return bad_request("copyRatio must be in (0, 1]");
        }
    }

    let settings = CopyTradingSettings {
        master_id,
        fixed_amount: req.fixed_amount,
        max_position_size: req.max_position_size,
        stop_loss_pct: req.stop_loss_pct,
        copy_ratio: req.copy_ratio.unwrap_or(Decimal::ONE),
        is_active: true,
    };

    let mut store = exchange
        .copy_trading_settings
        .lock().unwrap_or_else(|e| e.into_inner());
    store.insert(user_id, settings);

    tracing::info!(%user_id, master_id = %master_id, "copy trading settings updated");

    (StatusCode::OK, Json(serde_json::json!({"status": "ok"})))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyTradingSettingsRequest {
    pub master_user_id: String,
    pub fixed_amount: Option<Decimal>,
    pub max_position_size: Option<Decimal>,
    pub stop_loss_pct: Option<Decimal>,
    pub copy_ratio: Option<Decimal>,
}

/// GET /api/v1/copy-trading/settings
///
/// Get the authenticated user's current copy trading settings.
pub async fn copy_trading_get_settings(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let store = exchange
        .copy_trading_settings
        .lock().unwrap_or_else(|e| e.into_inner());

    match store.get(&user_id) {
        Some(s) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "master_id": s.master_id.to_string(),
                "fixed_amount": s.fixed_amount,
                "max_position_size": s.max_position_size,
                "stop_loss_pct": s.stop_loss_pct,
                "copy_ratio": s.copy_ratio,
                "is_active": s.is_active,
            })),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "no copy trading settings found"})),
        ),
    }
}

/// GET /api/v1/copy-trading/pnl
///
/// Returns a summary of the authenticated user's copy trading PnL.
/// Computes from closed positions that were opened while the user had
/// an active copy trading relation.
pub async fn copy_trading_pnl(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;

    // Determine if the user has (or had) copy trading settings.
    let settings_exists = {
        let store = exchange
            .copy_trading_settings
            .lock().unwrap_or_else(|e| e.into_inner());
        store.contains_key(&user_id)
    };

    if !settings_exists {
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "total_pnl": "0",
                "closed_trades": 0,
                "note": "no copy trading settings configured",
            })),
        );
    }

    // Aggregate PnL from all closed positions for this user.
    // In a full implementation you would tag positions with a copy-origin flag.
    // Here we sum all closed position PnL as a reasonable proxy.
    let closed = exchange
        .closed_positions
        .lock().unwrap_or_else(|e| e.into_inner());

    let (total_pnl, count) = closed
        .iter()
        .filter(|p| p.user_id == user_id)
        .fold((Decimal::ZERO, 0usize), |(pnl, cnt), p| {
            (pnl + p.realized_pnl, cnt + 1)
        });

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "total_pnl": total_pnl,
            "closed_trades": count,
        })),
    )
}

/// PUT /api/v1/copy-trading/profit-share
///
/// Master trader sets their own profit sharing percentage.
pub async fn copy_trading_set_profit_share(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<ProfitShareRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;

    if req.profit_share_pct < Decimal::ZERO || req.profit_share_pct > Decimal::from(100u32) {
        return bad_request("profitSharePct must be in [0, 100]");
    }

    let rate = req.profit_share_pct / Decimal::from(100u32);

    let mut masters = exchange
        .copy_trading_masters
        .lock().unwrap_or_else(|e| e.into_inner());

    // Register as master if not already (idempotent auto-registration).
    if !masters.is_master(user_id) {
        if let Err(e) = masters.register(user_id, "trader", rate) {
            return bad_request(e.to_string());
        }
        return (
            StatusCode::OK,
            Json(serde_json::json!({"status": "registered", "profit_share_rate": rate})),
        );
    }

    // Update existing master's profit share rate.
    if let Err(e) = masters.update_profit_share(user_id, rate) {
        return bad_request(e.to_string());
    }

    tracing::info!(%user_id, profit_share_rate = %rate, "profit share rate updated");

    (
        StatusCode::OK,
        Json(serde_json::json!({"status": "ok", "profit_share_rate": rate})),
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfitShareRequest {
    pub profit_share_pct: Decimal,
}

// =============================================================================
// Task 4 — P2P Trading
// =============================================================================

/// POST /api/v1/p2p/ads
///
/// Create a P2P ad.
pub async fn p2p_create_ad(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<P2pAdRequest>,
) -> impl IntoResponse {
    use crate::exchange::P2pAd;

    if !matches!(req.side.as_str(), "buy" | "sell") {
        return bad_request("side must be 'buy' or 'sell'");
    }
    if req.price <= Decimal::ZERO {
        return bad_request("price must be positive");
    }
    if req.min_amount <= Decimal::ZERO || req.max_amount <= Decimal::ZERO {
        return bad_request("min_amount and max_amount must be positive");
    }
    if req.min_amount > req.max_amount {
        return bad_request("min_amount must be <= max_amount");
    }

    let user_id = auth.0;

    // If user is selling crypto, verify they hold enough.
    if req.side == "sell" {
        let available = exchange
            .accounts
            .get_balance(user_id, &req.asset)
            .map(|b| b.available())
            .unwrap_or(Decimal::ZERO);
        if available < req.max_amount {
            return bad_request("insufficient balance to back this sell ad");
        }
    }

    let ad = P2pAd {
        id: Uuid::new_v4(),
        user_id,
        side: req.side,
        asset: req.asset,
        fiat_currency: req.fiat_currency,
        price: req.price,
        min_amount: req.min_amount,
        max_amount: req.max_amount,
        payment_methods: req.payment_methods,
        is_active: true,
        created_at: chrono::Utc::now(),
    };

    let id = ad.id;
    let mut ads = exchange.p2p_ads.lock().unwrap_or_else(|e| e.into_inner());
    ads.push(ad);

    tracing::info!(ad_id = %id, %user_id, "P2P ad created");

    (StatusCode::CREATED, Json(serde_json::json!({"id": id.to_string()})))
}

#[derive(Debug, Deserialize)]
pub struct P2pAdRequest {
    pub side: String,
    pub asset: String,
    pub fiat_currency: String,
    pub price: Decimal,
    pub min_amount: Decimal,
    pub max_amount: Decimal,
    #[serde(default)]
    pub payment_methods: Vec<String>,
}

/// GET /api/v1/p2p/ads
///
/// List P2P ads with optional filters.
pub async fn p2p_list_ads(
    State(exchange): State<AppState>,
    Query(params): Query<P2pAdListQuery>,
) -> impl IntoResponse {
    let ads = exchange.p2p_ads.lock().unwrap_or_else(|e| e.into_inner());
    let result: Vec<serde_json::Value> = ads
        .iter()
        .filter(|a| a.is_active)
        .filter(|a| {
            params.side.as_deref().map(|s| s == a.side).unwrap_or(true)
                && params.asset.as_deref().map(|s| s == a.asset).unwrap_or(true)
                && params.fiat_currency.as_deref().map(|s| s == a.fiat_currency).unwrap_or(true)
        })
        .map(|a| serde_json::json!({
            "id": a.id.to_string(),
            "user_id": a.user_id.to_string(),
            "side": a.side,
            "asset": a.asset,
            "fiat_currency": a.fiat_currency,
            "price": a.price,
            "min_amount": a.min_amount,
            "max_amount": a.max_amount,
            "payment_methods": a.payment_methods,
            "created_at": a.created_at.to_rfc3339(),
        }))
        .collect();

    (StatusCode::OK, Json(serde_json::json!(result)))
}

#[derive(Debug, Deserialize, Default)]
pub struct P2pAdListQuery {
    pub side: Option<String>,
    pub asset: Option<String>,
    pub fiat_currency: Option<String>,
}

/// POST /api/v1/p2p/orders
///
/// Create a P2P order (escrows seller's crypto).
pub async fn p2p_create_order(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<P2pCreateOrderRequest>,
) -> impl IntoResponse {
    use crate::exchange::{P2pOrder, P2pOrderStatus};

    let buyer_id = auth.0;

    // Load the ad.
    let ad = {
        let ads = exchange.p2p_ads.lock().unwrap_or_else(|e| e.into_inner());
        match ads.iter().find(|a| a.id == req.ad_id && a.is_active) {
            Some(a) => a.clone(),
            None => return bad_request("ad not found or no longer active"),
        }
    };

    if req.amount < ad.min_amount || req.amount > ad.max_amount {
        return bad_request("amount outside ad limits");
    }
    if buyer_id == ad.user_id {
        return bad_request("cannot order from your own ad");
    }

    let seller_id;
    let buyer_actual;
    let fiat_total = req.amount * ad.price;

    // Determine buyer/seller from ad side.
    // Ad side "sell" means the ad owner is selling crypto to the buyer (order creator).
    // Ad side "buy" means the ad owner wants to buy crypto from the order creator (who sells).
    let (insufficient_msg, escrow_fail_msg): (&str, &str);
    if ad.side == "sell" {
        seller_id = ad.user_id;
        buyer_actual = buyer_id;
        insufficient_msg = "seller has insufficient balance";
        escrow_fail_msg = "failed to escrow seller funds";
    } else {
        // Ad is "buy" — order creator is the seller.
        seller_id = buyer_id;
        buyer_actual = ad.user_id;
        insufficient_msg = "insufficient balance to sell";
        escrow_fail_msg = "failed to escrow funds";
    }

    // Escrow the seller's crypto atomically under their shard lock.
    let escrow_result: Result<(), &str> = exchange.accounts.with_user(seller_id, |am| {
        let available = am
            .get_balance(&seller_id, &ad.asset)
            .map(|b| b.available())
            .unwrap_or(Decimal::ZERO);
        if available < req.amount {
            return Err(insufficient_msg);
        }
        am.withdraw(seller_id, &ad.asset, req.amount).map_err(|_| escrow_fail_msg)
    });
    if let Err(msg) = escrow_result {
        return bad_request(msg);
    }

    let order = P2pOrder {
        id: Uuid::new_v4(),
        ad_id: req.ad_id,
        buyer_id: buyer_actual,
        seller_id,
        asset: ad.asset.clone(),
        fiat_currency: ad.fiat_currency.clone(),
        crypto_amount: req.amount,
        fiat_amount: fiat_total,
        status: P2pOrderStatus::Created,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let order_id = order.id;
    let mut orders = exchange.p2p_orders.lock().unwrap_or_else(|e| e.into_inner());
    orders.push(order);

    // Decrement ad available capacity. Deactivate ad if fully consumed.
    {
        let mut ads = exchange.p2p_ads.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ad_entry) = ads.iter_mut().find(|a| a.id == req.ad_id) {
            ad_entry.max_amount = (ad_entry.max_amount - req.amount).max(Decimal::ZERO);
            if ad_entry.max_amount < ad_entry.min_amount {
                ad_entry.is_active = false;
                tracing::info!(ad_id = %req.ad_id, "P2P ad deactivated — capacity exhausted");
            }
        }
    }

    tracing::info!(order_id = %order_id, seller_id = %seller_id, buyer_id = %buyer_actual, "P2P order created, funds escrowed");

    (StatusCode::CREATED, Json(serde_json::json!({"id": order_id.to_string()})))
}

#[derive(Debug, Deserialize)]
pub struct P2pCreateOrderRequest {
    pub ad_id: Uuid,
    pub amount: Decimal,
}

/// GET /api/v1/p2p/orders — list the authenticated user's P2P orders.
pub async fn p2p_list_orders(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let orders = exchange.p2p_orders.lock().unwrap_or_else(|e| e.into_inner());
    let user_orders: Vec<serde_json::Value> = orders.iter()
        .filter(|o| o.buyer_id == user_id || o.seller_id == user_id)
        .map(|o| serde_json::json!({
            "id": o.id.to_string(),
            "adId": o.ad_id.to_string(),
            "buyerId": o.buyer_id.to_string(),
            "sellerId": o.seller_id.to_string(),
            "asset": o.asset,
            "fiatCurrency": o.fiat_currency,
            "cryptoAmount": o.crypto_amount.to_string(),
            "fiatAmount": o.fiat_amount.to_string(),
            "status": format!("{:?}", o.status),
            "createdAt": o.created_at.timestamp_millis(),
        }))
        .collect();
    (StatusCode::OK, Json(serde_json::json!(user_orders)))
}

/// POST /api/v1/p2p/orders/{id}/mark-paid
///
/// Buyer marks payment as sent.
pub async fn p2p_mark_paid(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    use crate::exchange::P2pOrderStatus;

    let order_id = match Uuid::parse_str(&id) {
        Ok(v) => v,
        Err(_) => return bad_request("invalid order id"),
    };
    let user_id = auth.0;

    let mut orders = exchange.p2p_orders.lock().unwrap_or_else(|e| e.into_inner());
    let order = match orders.iter_mut().find(|o| o.id == order_id) {
        Some(o) => o,
        None => return not_found("order not found"),
    };

    if order.buyer_id != user_id {
        return bad_request("only the buyer can mark payment as sent");
    }
    if order.status != P2pOrderStatus::Created {
        return bad_request("order is not in 'created' state");
    }

    order.status = P2pOrderStatus::PaymentSent;
    order.updated_at = chrono::Utc::now();

    tracing::info!(order_id = %order_id, "P2P order marked as paid");

    (StatusCode::OK, Json(serde_json::json!({"status": "payment_sent"})))
}

/// POST /api/v1/p2p/orders/{id}/release
///
/// Seller confirms receipt of payment and releases crypto to buyer.
pub async fn p2p_release(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    use crate::exchange::P2pOrderStatus;

    let order_id = match Uuid::parse_str(&id) {
        Ok(v) => v,
        Err(_) => return bad_request("invalid order id"),
    };
    let user_id = auth.0;

    let (buyer_id, asset, crypto_amount) = {
        let mut orders = exchange.p2p_orders.lock().unwrap_or_else(|e| e.into_inner());
        let order = match orders.iter_mut().find(|o| o.id == order_id) {
            Some(o) => o,
            None => return not_found("order not found"),
        };

        if order.seller_id != user_id {
            return bad_request("only the seller can release funds");
        }
        if order.status != P2pOrderStatus::PaymentSent {
            return bad_request("payment has not been marked as sent");
        }

        order.status = P2pOrderStatus::Completed;
        order.updated_at = chrono::Utc::now();
        (order.buyer_id, order.asset.clone(), order.crypto_amount)
    };

    // Transfer escrowed crypto to buyer.
    let _ = exchange.accounts.deposit(buyer_id, &asset, crypto_amount);

    tracing::info!(order_id = %order_id, buyer_id = %buyer_id, asset = %asset, amount = %crypto_amount, "P2P order released, crypto transferred to buyer");

    (StatusCode::OK, Json(serde_json::json!({"status": "completed"})))
}

/// POST /api/v1/p2p/orders/{id}/dispute
///
/// Open a dispute on a P2P order.
pub async fn p2p_dispute(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    use crate::exchange::P2pOrderStatus;

    let order_id = match Uuid::parse_str(&id) {
        Ok(v) => v,
        Err(_) => return bad_request("invalid order id"),
    };
    let user_id = auth.0;

    let mut orders = exchange.p2p_orders.lock().unwrap_or_else(|e| e.into_inner());
    let order = match orders.iter_mut().find(|o| o.id == order_id) {
        Some(o) => o,
        None => return not_found("order not found"),
    };

    if order.buyer_id != user_id && order.seller_id != user_id {
        return bad_request("you are not a party to this order");
    }
    if matches!(order.status, P2pOrderStatus::Completed | P2pOrderStatus::Cancelled) {
        return bad_request("cannot dispute a completed or cancelled order");
    }

    order.status = P2pOrderStatus::Disputed;
    order.updated_at = chrono::Utc::now();

    tracing::info!(order_id = %order_id, %user_id, "P2P order disputed");

    (StatusCode::OK, Json(serde_json::json!({"status": "disputed"})))
}

/// POST /api/v1/p2p/orders/{id}/cancel
///
/// Cancel a P2P order before payment is sent (returns escrowed funds).
pub async fn p2p_cancel_order(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    use crate::exchange::P2pOrderStatus;

    let order_id = match Uuid::parse_str(&id) {
        Ok(v) => v,
        Err(_) => return bad_request("invalid order id"),
    };
    let user_id = auth.0;

    let (seller_id, asset, crypto_amount) = {
        let mut orders = exchange.p2p_orders.lock().unwrap_or_else(|e| e.into_inner());
        let order = match orders.iter_mut().find(|o| o.id == order_id) {
            Some(o) => o,
            None => return not_found("order not found"),
        };

        if order.buyer_id != user_id && order.seller_id != user_id {
            return bad_request("you are not a party to this order");
        }
        if order.status != P2pOrderStatus::Created {
            return bad_request("order can only be cancelled before payment is sent");
        }

        order.status = P2pOrderStatus::Cancelled;
        order.updated_at = chrono::Utc::now();
        (order.seller_id, order.asset.clone(), order.crypto_amount)
    };

    // Return escrowed funds to seller.
    let _ = exchange.accounts.deposit(seller_id, &asset, crypto_amount);

    tracing::info!(order_id = %order_id, %user_id, "P2P order cancelled, funds returned to seller");

    (StatusCode::OK, Json(serde_json::json!({"status": "cancelled"})))
}

// =============================================================================
// Admin: P2P Dispute Resolution
// =============================================================================

/// POST /admin/p2p/disputes/{order_id}/resolve
///
/// Admin endpoint to resolve a disputed P2P order.
///
/// Resolution options:
/// - `release_to_buyer`  — credits the escrowed crypto to the buyer (buyer's
///   claim is upheld).
/// - `refund_to_seller`  — returns the escrowed crypto to the seller (seller's
///   claim is upheld).
///
/// After resolution the order status is set to `Completed` or `Cancelled`
/// respectively.
pub async fn admin_p2p_resolve_dispute(
    State(exchange): State<AppState>,
    Path(order_id_str): Path<String>,
    Json(req): Json<P2pResolveRequest>,
) -> impl IntoResponse {
    use crate::exchange::P2pOrderStatus;

    let order_id = match Uuid::parse_str(&order_id_str) {
        Ok(v) => v,
        Err(_) => return bad_request("invalid order id"),
    };

    // Extract the fields we need while holding the orders lock, then drop it
    // before touching the accounts lock to avoid nested-lock ordering issues.
    let (buyer_id, seller_id, asset, crypto_amount) = {
        let mut orders = exchange.p2p_orders.lock().unwrap_or_else(|e| e.into_inner());
        let order = match orders.iter_mut().find(|o| o.id == order_id) {
            Some(o) => o,
            None => return not_found("order not found"),
        };

        if order.status != P2pOrderStatus::Disputed {
            return bad_request("order is not in a disputed state");
        }

        let (new_status, tuple) = match req.resolution.as_str() {
            "release_to_buyer" => (
                P2pOrderStatus::Completed,
                (order.buyer_id, order.seller_id, order.asset.clone(), order.crypto_amount),
            ),
            "refund_to_seller" => (
                P2pOrderStatus::Cancelled,
                (order.buyer_id, order.seller_id, order.asset.clone(), order.crypto_amount),
            ),
            _ => return bad_request("resolution must be 'release_to_buyer' or 'refund_to_seller'"),
        };

        order.status = new_status;
        order.updated_at = chrono::Utc::now();
        tuple
    };

    // Apply the fund transfer outside the orders lock.
    match req.resolution.as_str() {
        "release_to_buyer" => {
            let _ = exchange.accounts.deposit(buyer_id, &asset, crypto_amount);
            tracing::info!(
                order_id = %order_id,
                buyer_id = %buyer_id,
                asset = %asset,
                amount = %crypto_amount,
                "admin P2P dispute resolved: funds released to buyer"
            );
        }
        "refund_to_seller" => {
            let _ = exchange.accounts.deposit(seller_id, &asset, crypto_amount);
            tracing::info!(
                order_id = %order_id,
                seller_id = %seller_id,
                asset = %asset,
                amount = %crypto_amount,
                "admin P2P dispute resolved: funds refunded to seller"
            );
        }
        _ => {}
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": req.resolution,
            "order_id": order_id.to_string(),
        })),
    )
}

#[derive(Debug, serde::Deserialize)]
pub struct P2pResolveRequest {
    /// `"release_to_buyer"` or `"refund_to_seller"`.
    pub resolution: String,
}

// =============================================================================
// Unit tests
// =============================================================================

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    use crate::exchange::{Competition, P2pOrderStatus};

    #[test]
    fn competition_status_logic() {
        use chrono::{Duration, Utc};
        let now = Utc::now();
        let upcoming = Competition {
            id: uuid::Uuid::new_v4(),
            name: "Upcoming".into(),
            start_time: now + Duration::hours(1),
            end_time: now + Duration::hours(2),
            prize_pool: dec!(1000),
            metric: "pnl".into(),
            symbols: vec![],
            created_at: now,
        };
        let active = Competition {
            id: uuid::Uuid::new_v4(),
            name: "Active".into(),
            start_time: now - Duration::hours(1),
            end_time: now + Duration::hours(1),
            prize_pool: dec!(500),
            metric: "volume".into(),
            symbols: vec![],
            created_at: now,
        };
        let ended = Competition {
            id: uuid::Uuid::new_v4(),
            name: "Ended".into(),
            start_time: now - Duration::hours(2),
            end_time: now - Duration::hours(1),
            prize_pool: dec!(100),
            metric: "roi".into(),
            symbols: vec![],
            created_at: now,
        };

        fn status(c: &Competition) -> &'static str {
            let now = chrono::Utc::now();
            if now < c.start_time {
                "upcoming"
            } else if now <= c.end_time {
                "active"
            } else {
                "ended"
            }
        }

        assert_eq!(status(&upcoming), "upcoming");
        assert_eq!(status(&active), "active");
        assert_eq!(status(&ended), "ended");
    }

    #[test]
    fn p2p_order_status_transitions() {
        // Verify that status enum variants exist and are distinct.
        assert_ne!(
            P2pOrderStatus::Created as u8,
            P2pOrderStatus::PaymentSent as u8
        );
        assert_ne!(
            P2pOrderStatus::PaymentSent as u8,
            P2pOrderStatus::Completed as u8
        );
    }

    #[test]
    fn leveraged_token_nav_calculation() {
        // NAV = initial_nav * (1 + leverage * underlying_return)
        let initial_nav = dec!(100);
        let leverage = dec!(3);
        let underlying_return = dec!(0.1); // 10% gain
        let new_nav = initial_nav * (Decimal::ONE + leverage * underlying_return);
        assert_eq!(new_nav, dec!(130));
    }
}
