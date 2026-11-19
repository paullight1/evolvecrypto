//! Institutional and operational endpoints.
//!
//! This module contains:
//!  - Extended block trading: counterparties, RFQ cancel-all, public feed, block tickers.
//!  - System status API: component status, incidents.
//!  - Order pre-check (dry-run validation).
//!  - Close position shortcut.
//!  - Asset valuation and monthly trading statement.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use rust_decimal::Decimal;
use serde::Deserialize;
use uuid::Uuid;

use exchange_common::{OrderType, Side, Symbol, TimeInForce};

use crate::auth::AuthenticatedUser;
use crate::rest::AppState;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn bad_request(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": msg.into()})))
}

fn not_found(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": msg.into()})))
}

// =============================================================================
// Task 1 — Extended Block Trading
// =============================================================================

// ---------------------------------------------------------------------------
// GET /block-trade/counterparties
// ---------------------------------------------------------------------------

/// GET /api/v1/block-trade/counterparties
///
/// Returns the list of registered block-trade counterparties (market makers
/// and institutional accounts). Market makers registered in the MM program
/// are automatically included. Additional counterparties can be registered
/// by admin.
///
/// The caller needs to be authenticated.
pub async fn block_trade_counterparties(
    State(exchange): State<AppState>,
    axum::Extension(_auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    // Collect dedicated counterparty registrations.
    let mut result: Vec<serde_json::Value> = {
        let store = exchange
            .block_trade_counterparties
            .lock().unwrap_or_else(|e| e.into_inner());
        store
            .iter()
            .map(|c| {
                serde_json::json!({
                    "user_id": c.user_id.to_string(),
                    "name": c.name,
                    "symbols": c.symbols,
                    "is_online": c.is_online,
                })
            })
            .collect()
    };

    // Also include active market makers from the MM program as counterparties.
    {
        let mmp = exchange
            .market_maker_program
            .lock().unwrap_or_else(|e| e.into_inner());
        for record in mmp.records.values() {
            if !record.is_active {
                continue;
            }
            // Avoid duplicates if the MM was also registered as a counterparty.
            let already_present = result
                .iter()
                .any(|v| v["user_id"].as_str() == Some(&record.user_id.to_string()));
            if !already_present {
                result.push(serde_json::json!({
                    "user_id": record.user_id.to_string(),
                    "name": record.name,
                    "symbols": record.symbol_configs.keys().map(|s| s.to_string()).collect::<Vec<_>>(),
                    "is_online": true,
                }));
            }
        }
    }

    (StatusCode::OK, Json(serde_json::json!({ "counterparties": result })))
}

// ---------------------------------------------------------------------------
// POST /block-trade/rfq/cancel-all
// ---------------------------------------------------------------------------

/// POST /api/v1/block-trade/rfq/cancel-all
///
/// Cancels all open RFQs created by the authenticated user.
pub async fn block_trade_rfq_cancel_all(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let mut rfqs = exchange
        .block_trade_rfqs
        .lock().unwrap_or_else(|e| e.into_inner());

    let mut cancelled_count = 0usize;
    for rfq in rfqs.values_mut() {
        if rfq.taker_id != auth.0 {
            continue;
        }
        use crate::exchange::RfqStatus;
        if rfq.status == RfqStatus::AwaitingQuotes || rfq.status == RfqStatus::Quoted {
            rfq.status = RfqStatus::Cancelled;
            cancelled_count += 1;
        }
    }

    tracing::info!(
        user_id = %auth.0,
        cancelled = cancelled_count,
        "block_trade_rfq_cancel_all",
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({ "cancelled": cancelled_count })),
    )
}

// ---------------------------------------------------------------------------
// GET /market/block-trades/{symbol}
// ---------------------------------------------------------------------------

/// Query parameters for the public block trade feed endpoint.
#[derive(Debug, Deserialize)]
pub struct BlockTradeFeedQuery {
    /// Maximum number of trades to return (default 50, max 200).
    #[serde(default = "default_block_feed_limit")]
    pub limit: usize,
}

fn default_block_feed_limit() -> usize {
    50
}

/// GET /api/v1/market/block-trades/{symbol}
///
/// Anonymized recent block trades for the given symbol (no user IDs exposed).
pub async fn public_block_trades(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
    Query(params): Query<BlockTradeFeedQuery>,
) -> impl IntoResponse {
    let limit = params.limit.min(200);
    let trades = exchange
        .block_trades
        .lock().unwrap_or_else(|e| e.into_inner());

    let result: Vec<serde_json::Value> = trades
        .iter()
        .filter(|t| t.symbol.eq_ignore_ascii_case(&symbol))
        .rev() // most recent first
        .take(limit)
        .map(|t| {
            serde_json::json!({
                "trade_id": t.id.to_string(),
                "symbol": t.symbol,
                "price": t.price,
                "qty": t.qty,
                "taker_side": t.taker_side,
                "timestamp": t.timestamp.timestamp_millis(),
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!({ "trades": result })))
}

// ---------------------------------------------------------------------------
// GET /market/block-tickers
// ---------------------------------------------------------------------------

/// GET /api/v1/market/block-tickers
///
/// 24h block trade volume per symbol.
pub async fn block_tickers(State(exchange): State<AppState>) -> impl IntoResponse {
    let trades = exchange
        .block_trades
        .lock().unwrap_or_else(|e| e.into_inner());

    let cutoff = chrono::Utc::now() - chrono::Duration::hours(24);

    // Aggregate volume and count per symbol.
    let mut stats: std::collections::HashMap<String, (Decimal, Decimal, u64)> =
        std::collections::HashMap::new();

    for trade in trades.iter().filter(|t| t.timestamp >= cutoff) {
        let entry = stats.entry(trade.symbol.clone()).or_default();
        entry.0 += trade.qty;                  // base volume
        entry.1 += trade.price * trade.qty;    // quote (notional) volume
        entry.2 += 1;                          // count
    }

    let tickers: Vec<serde_json::Value> = stats
        .into_iter()
        .map(|(symbol, (base_vol, quote_vol, count))| {
            serde_json::json!({
                "symbol": symbol,
                "base_volume_24h": base_vol,
                "quote_volume_24h": quote_vol,
                "trade_count_24h": count,
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!({ "tickers": tickers })))
}

// =============================================================================
// Task 2 — System Status API
// =============================================================================

// ---------------------------------------------------------------------------
// GET /status  (enhanced — backed by the SystemStatus store)
// ---------------------------------------------------------------------------

/// GET /api/v1/status — rich system status backed by the persistent store.
pub async fn get_rich_system_status(State(exchange): State<AppState>) -> impl IntoResponse {
    let status = exchange
        .system_status
        .lock().unwrap_or_else(|e| e.into_inner())
        .clone();

    // Derive the string representation of each component status.
    let components: serde_json::Value = status
        .components
        .iter()
        .map(|(name, s)| (name.clone(), serde_json::Value::String(s.to_string())))
        .collect::<serde_json::Map<String, serde_json::Value>>()
        .into();

    let incidents: Vec<serde_json::Value> = status
        .incidents
        .iter()
        .map(|inc| {
            serde_json::json!({
                "id": inc.id.to_string(),
                "title": inc.title,
                "status": inc.status,
                "message": inc.message,
                "affected_components": inc.affected_components,
                "created_at": inc.created_at.timestamp_millis(),
                "updated_at": inc.updated_at.timestamp_millis(),
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": status.overall.to_string(),
            "message": status.message,
            "components": components,
            "incidents": incidents,
            "updated_at": status.updated_at.timestamp_millis(),
        })),
    )
}

// ---------------------------------------------------------------------------
// POST /admin/status/component
// ---------------------------------------------------------------------------

/// Request body for admin component status update.
#[derive(Debug, Deserialize)]
pub struct UpdateComponentStatusRequest {
    /// Component name, e.g. "matching_engine", "deposits".
    pub component: String,
    /// New status: "operational", "degraded", "maintenance", or "unavailable".
    pub status: String,
    /// Optional human-readable message.
    #[serde(default)]
    pub message: Option<String>,
}

/// POST /api/v1/admin/status/component
///
/// Allows admins to update the status of an individual system component.
pub async fn admin_update_component_status(
    State(exchange): State<AppState>,
    Json(req): Json<UpdateComponentStatusRequest>,
) -> impl IntoResponse {
    use crate::exchange::ComponentStatus;

    let new_status = match req.status.as_str() {
        "operational" => ComponentStatus::Operational,
        "degraded"    => ComponentStatus::Degraded,
        "maintenance" => ComponentStatus::Maintenance,
        "unavailable" => ComponentStatus::Unavailable,
        other => {
            return bad_request(format!(
                "Invalid status '{}'. Use: operational, degraded, maintenance, unavailable",
                other
            ))
            .into_response();
        }
    };

    let mut status = exchange
        .system_status
        .lock().unwrap_or_else(|e| e.into_inner());

    status.components.insert(req.component.clone(), new_status);
    status.updated_at = chrono::Utc::now();
    if let Some(msg) = req.message {
        status.message = Some(msg);
    }

    // Re-derive the overall status as the worst of all components.
    status.overall = derive_overall_status(&status.components);

    tracing::info!(
        component = %req.component,
        status = %new_status,
        "admin updated component status",
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "component": req.component,
            "status": new_status.to_string(),
            "overall": status.overall.to_string(),
        })),
    )
    .into_response()
}

/// Derive the overall status as the worst-case of all component statuses.
fn derive_overall_status(
    components: &std::collections::HashMap<String, crate::exchange::ComponentStatus>,
) -> crate::exchange::ComponentStatus {
    use crate::exchange::ComponentStatus;
    let mut worst = ComponentStatus::Operational;
    for &s in components.values() {
        worst = match (worst, s) {
            (ComponentStatus::Unavailable, _) | (_, ComponentStatus::Unavailable) => {
                ComponentStatus::Unavailable
            }
            (ComponentStatus::Degraded, _) | (_, ComponentStatus::Degraded) => {
                ComponentStatus::Degraded
            }
            (ComponentStatus::Maintenance, _) | (_, ComponentStatus::Maintenance) => {
                ComponentStatus::Maintenance
            }
            _ => ComponentStatus::Operational,
        };
    }
    worst
}

// ---------------------------------------------------------------------------
// GET /status/incidents
// ---------------------------------------------------------------------------

/// GET /api/v1/status/incidents — list recent incidents.
pub async fn list_status_incidents(State(exchange): State<AppState>) -> impl IntoResponse {
    let status = exchange
        .system_status
        .lock().unwrap_or_else(|e| e.into_inner());

    let incidents: Vec<serde_json::Value> = status
        .incidents
        .iter()
        .map(|inc| {
            serde_json::json!({
                "id": inc.id.to_string(),
                "title": inc.title,
                "status": inc.status,
                "message": inc.message,
                "affected_components": inc.affected_components,
                "created_at": inc.created_at.timestamp_millis(),
                "updated_at": inc.updated_at.timestamp_millis(),
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!({ "incidents": incidents })))
}

// ---------------------------------------------------------------------------
// POST /admin/status/incident
// ---------------------------------------------------------------------------

/// Request body for creating or updating an incident.
#[derive(Debug, Deserialize)]
pub struct UpsertIncidentRequest {
    /// Incident UUID — when provided, updates the existing incident;
    /// when absent, creates a new one.
    #[serde(default)]
    pub id: Option<Uuid>,
    pub title: String,
    /// One of: "investigating", "identified", "monitoring", "resolved".
    pub status: String,
    pub message: String,
    /// Component names affected by this incident.
    #[serde(default)]
    pub affected_components: Vec<String>,
}

/// POST /api/v1/admin/status/incident — create or update an incident.
pub async fn admin_upsert_incident(
    State(exchange): State<AppState>,
    Json(req): Json<UpsertIncidentRequest>,
) -> impl IntoResponse {
    let valid_statuses = ["investigating", "identified", "monitoring", "resolved"];
    if !valid_statuses.contains(&req.status.as_str()) {
        return bad_request(format!(
            "Invalid incident status '{}'. Use: {}",
            req.status,
            valid_statuses.join(", "),
        ))
        .into_response();
    }

    let mut system = exchange
        .system_status
        .lock().unwrap_or_else(|e| e.into_inner());

    let now = chrono::Utc::now();
    let incident_id;

    if let Some(existing_id) = req.id {
        // Update existing incident.
        if let Some(inc) = system.incidents.iter_mut().find(|i| i.id == existing_id) {
            inc.status = req.status.clone();
            inc.message = req.message.clone();
            inc.affected_components = req.affected_components.clone();
            inc.updated_at = now;
            incident_id = existing_id;
        } else {
            return not_found(format!("Incident {} not found", existing_id)).into_response();
        }
    } else {
        // Create new incident.
        incident_id = Uuid::new_v4();
        let inc = crate::exchange::Incident {
            id: incident_id,
            title: req.title.clone(),
            status: req.status.clone(),
            message: req.message.clone(),
            affected_components: req.affected_components.clone(),
            created_at: now,
            updated_at: now,
        };
        // Enforce cap of 50 incidents.
        if system.incidents.len() >= 50 {
            system.incidents.remove(0);
        }
        system.incidents.push(inc);
    }

    system.updated_at = now;

    tracing::info!(
        incident_id = %incident_id,
        status = %req.status,
        "admin upserted incident",
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "id": incident_id.to_string(),
            "title": req.title,
            "status": req.status,
        })),
    )
    .into_response()
}

// =============================================================================
// Task 3 — Order Pre-check, Close Position, Asset Valuation, Monthly Statement
// =============================================================================

// ---------------------------------------------------------------------------
// POST /orders/precheck
// ---------------------------------------------------------------------------

/// Request body for order pre-check (mirrors PlaceOrderRequest).
#[derive(Debug, Deserialize)]
pub struct OrderPrecheckRequest {
    pub symbol: String,
    pub side: String,
    pub order_type: String,
    #[serde(default)]
    pub time_in_force: Option<String>,
    #[serde(default)]
    pub price: Option<Decimal>,
    pub qty: Decimal,
    #[serde(default)]
    pub reduce_only: Option<bool>,
    #[serde(default)]
    pub position_idx: Option<u8>,
}

/// POST /api/v1/orders/precheck
///
/// Dry-run order validation: runs all pre-trade checks (balance, margin,
/// risk limits, symbol validation) but does NOT place the order.
pub async fn order_precheck(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<OrderPrecheckRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let mut warnings: Vec<String> = Vec::new();

    // 1. Validate symbol format.
    let symbol = Symbol::new(&req.symbol);
    if req.symbol.len() < 3
        || req.symbol.len() > 20
        || !req.symbol.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return bad_request(format!(
            "Invalid symbol '{}': must be 3-20 uppercase ASCII letters/digits",
            req.symbol
        ))
        .into_response();
    }

    // 2. Validate side.
    let side = match req.side.to_lowercase().as_str() {
        "buy"  => Side::Buy,
        "sell" => Side::Sell,
        _      => return bad_request("Invalid side. Use 'buy' or 'sell'").into_response(),
    };

    // 3. Validate order_type.
    let order_type = match req.order_type.to_lowercase().as_str() {
        "limit"  => OrderType::Limit,
        "market" => OrderType::Market,
        _        => return bad_request("Invalid order_type. Use 'limit' or 'market'").into_response(),
    };

    // 4. Validate qty.
    if req.qty <= Decimal::ZERO {
        return bad_request("qty must be positive").into_response();
    }

    // 5. Validate price for limit orders.
    let price = match order_type {
        OrderType::Limit => match req.price {
            Some(p) if p > Decimal::ZERO => p,
            _ => return bad_request("Limit order requires a positive price").into_response(),
        },
        OrderType::Market => Decimal::ZERO,
    };

    // 6. Validate time_in_force.
    let tif_str = req.time_in_force.as_deref().unwrap_or("gtc");
    let _tif = match tif_str {
        "gtc"       => TimeInForce::Gtc,
        "ioc"       => TimeInForce::Ioc,
        "fok"       => TimeInForce::Fok,
        "post_only" => TimeInForce::PostOnly,
        "gtd"       => TimeInForce::Gtd,
        _           => return bad_request("Invalid time_in_force").into_response(),
    };

    // 7. Check if symbol is a known perpetual.
    let is_perp = exchange
        .perp_configs
        .load()
        .contains_key(&symbol);

    // 8. Estimate fee.
    let mark_price = exchange.get_mark_price(&symbol).map(|mp| mp.price);
    let notional = match order_type {
        OrderType::Limit => price * req.qty,
        OrderType::Market => {
            mark_price.unwrap_or(Decimal::ZERO) * req.qty
        }
    };

    // Use taker fee as conservative estimate.
    let fee_engine = exchange.fee_engine.lock().unwrap_or_else(|e| e.into_inner());
    let (_maker_rate, taker_rate) = fee_engine.get_fees(user_id);
    let estimated_fee = notional * taker_rate;
    drop(fee_engine);

    // 9. Estimate margin requirement (perpetuals only).
    let estimated_margin;
    if is_perp {
        let perp_configs = exchange.perp_configs.load();
        if let Some(cfg) = perp_configs.get(&symbol) {
            let leverage = cfg.max_leverage;
            estimated_margin = notional / Decimal::from(leverage);
        } else {
            estimated_margin = Decimal::ZERO;
        }
    } else {
        estimated_margin = notional;
    }

    // 10. Check available balance.
    let (base_asset, quote_asset) = match exchange.accounts.get_product(&symbol) {
        Some(p) => (p.base_asset.clone(), p.quote_asset.clone()),
        None => {
            let s = req.symbol.as_str();
            let split = s.len().saturating_sub(4);
            (s[..split].to_string(), s[split..].to_string())
        }
    };

    if is_perp {
        // For futures, check USDT balance for margin.
        let avail = exchange.accounts.get_balance(user_id, "USDT")
            .map(|b| b.available())
            .unwrap_or(Decimal::ZERO);
        let required = estimated_margin + estimated_fee;
        if avail < required {
            warnings.push(format!(
                "Insufficient USDT balance: available {:.4}, required {:.4}",
                avail, required
            ));
        }
    } else {
        // For spot, check the relevant balance.
        let (check_asset, check_amount) = match side {
            Side::Buy  => (quote_asset.as_str(), notional + estimated_fee),
            Side::Sell => (base_asset.as_str(), req.qty),
        };
        let avail = exchange.accounts.get_balance(user_id, check_asset)
            .map(|b| b.available())
            .unwrap_or(Decimal::ZERO);
        if avail < check_amount {
            warnings.push(format!(
                "Insufficient {} balance: available {:.4}, required {:.4}",
                check_asset, avail, check_amount
            ));
        }
    }

    // 11. reduce_only sanity check.
    if req.reduce_only.unwrap_or(false) {
        let pm = exchange.positions.lock();
        let position_idx = req.position_idx.unwrap_or(0);
        match pm.get_position(user_id, &symbol, position_idx) {
            None => {
                warnings.push("reduce_only specified but no open position found".to_string());
            }
            Some(pos) if pos.is_closed() => {
                warnings.push("reduce_only specified but position is already closed".to_string());
            }
            _ => {}
        }
    }

    let valid = warnings.iter().all(|w| !w.starts_with("Insufficient"));

    tracing::debug!(
        user_id = %user_id,
        symbol = %req.symbol,
        qty = %req.qty,
        valid = valid,
        "order_precheck",
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "valid": valid,
            "estimated_fee": estimated_fee,
            "estimated_margin": estimated_margin,
            "notional": notional,
            "warnings": warnings,
        })),
    )
    .into_response()
}

// ---------------------------------------------------------------------------
// POST /positions/{symbol}/close
// ---------------------------------------------------------------------------

/// POST /api/v1/positions/{symbol}/close
///
/// Closes the full open position in `symbol` at market price.
/// This is a convenience shortcut that internally places a market
/// reduce-only order for the full position size.
pub async fn close_position(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(symbol_str): Path<String>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let symbol = Symbol::new(&symbol_str);

    // Find the open position.
    let (pos_size, pos_side) = {
        let pm = exchange.positions.lock();
        match pm.get_position(user_id, &symbol, 0) {
            None => {
                return not_found(format!("No open position for {}", symbol_str)).into_response();
            }
            Some(pos) if pos.is_closed() => {
                return not_found(format!("No open position for {}", symbol_str)).into_response();
            }
            Some(pos) => (pos.size, pos.side.clone()),
        }
    };

    // Closing side is opposite to position side.
    let close_side = match pos_side {
        exchange_common::PositionSide::Long => Side::Sell,
        exchange_common::PositionSide::Short => Side::Buy,
    };

    // Build a market reduce-only order.
    let order_id = Uuid::new_v4();
    let cmd = exchange_common::PlaceOrderCommand {
        order_id,
        user_id,
        symbol: symbol.clone(),
        side: close_side,
        order_type: OrderType::Market,
        time_in_force: TimeInForce::Ioc,
        price: Decimal::ZERO,
        qty: pos_size,
        reduce_only: true,
        expire_at: None,
        timestamp: chrono::Utc::now(),
        position_idx: 0,
        iceberg_qty: None,
        order_link_id: None,
        slippage_tolerance: None,
        close_position: true,
        stp_mode: exchange_common::StpMode::None,
        request_expire_time: None,
    };

    // Execute through the exchange.
    let result = exchange.place_order(cmd);

    match result {
        Ok(fills) => {
            tracing::info!(
                user_id = %user_id,
                symbol = %symbol_str,
                pos_size = %pos_size,
                fills = fills.len(),
                "close_position executed",
            );

            let fill_data: Vec<serde_json::Value> = fills
                .iter()
                .filter_map(|event| {
                    if let exchange_common::MatchEvent::Fill(f) = event {
                        // Determine role: user is taker if they are the aggressor.
                        let role = if f.taker_user_id == user_id { "taker" } else { "maker" };
                        Some(serde_json::json!({
                            "trade_id": f.trade_id,
                            "price": f.price,
                            "qty": f.qty,
                            "role": role,
                        }))
                    } else {
                        None
                    }
                })
                .collect();

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "order_id": order_id.to_string(),
                    "symbol": symbol_str,
                    "qty_closed": pos_size,
                    "fills": fill_data,
                })),
            )
            .into_response()
        }
        Err(e) => {
            tracing::warn!(
                user_id = %user_id,
                symbol = %symbol_str,
                error = %e,
                "close_position failed",
            );
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
            .into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// GET /account/asset-valuation
// ---------------------------------------------------------------------------

/// GET /api/v1/account/asset-valuation
///
/// Returns total portfolio value in USD: sum of all balances × mark prices
/// plus unrealized PnL from open perpetual positions.
pub async fn asset_valuation(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let mark_prices = exchange.get_mark_prices();

    // 1. Spot balances × mark price.
    let (spot_usd_value, asset_breakdown) = {
        let bals = exchange.accounts.get_all_balances(user_id);

        let mut total = Decimal::ZERO;
        let mut breakdown = Vec::new();

        for (asset, bal) in &bals {
            // Map coin to a USD price via the mark-price store.
            // USDT/USDC are pegged 1:1. Other assets use COINUSDT mark price.
            let usd_price = if asset == "USDT" || asset == "USDC" {
                Decimal::ONE
            } else {
                let sym = Symbol::new(&format!("{}USDT", asset));
                mark_prices.get(&sym).map(|mp| mp.price).unwrap_or(Decimal::ZERO)
            };

            let bal_total = bal.total();
            let bal_available = bal.available();
            let usd_value = bal_total * usd_price;
            total += usd_value;
            breakdown.push(serde_json::json!({
                "asset": asset,
                "total": bal_total,
                "available": bal_available,
                "usd_price": usd_price,
                "usd_value": usd_value,
            }));
        }

        (total, breakdown)
    };

    // 2. Unrealized PnL from open perpetual positions.
    let unrealized_pnl = {
        let pm = exchange.positions.lock();
        // user_positions returns positions for this user
        let positions = pm.user_positions(user_id);

        let mut total_upnl = Decimal::ZERO;
        for pos in positions {
            if pos.is_closed() {
                continue;
            }
            if let Some(mp) = mark_prices.get(&pos.symbol) {
                use exchange_common::futures::PositionSide;
                let upnl = match pos.side {
                    PositionSide::Long  => (mp.price - pos.entry_price) * pos.size,
                    PositionSide::Short => (pos.entry_price - mp.price) * pos.size,
                };
                total_upnl += upnl;
            }
        }
        total_upnl
    };

    let total_usd = spot_usd_value + unrealized_pnl;

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "total_usd_value": total_usd,
            "spot_usd_value": spot_usd_value,
            "unrealized_pnl_usd": unrealized_pnl,
            "assets": asset_breakdown,
        })),
    )
}

// ---------------------------------------------------------------------------
// GET /account/monthly-statement/{year}/{month}
// ---------------------------------------------------------------------------

/// Path parameters for the monthly statement endpoint.
#[derive(Debug, Deserialize)]
pub struct MonthlyStatementPath {
    pub year: i32,
    pub month: u32,
}

/// GET /api/v1/account/monthly-statement/{year}/{month}
///
/// Returns a trading summary for the given calendar month:
/// total volume, fees paid, realized PnL, deposits and withdrawals.
pub async fn monthly_statement(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(params): Path<MonthlyStatementPath>,
) -> impl IntoResponse {
    let user_id = auth.0;

    // Validate month range.
    if params.month < 1 || params.month > 12 {
        return bad_request("month must be 1–12").into_response();
    }

    // Calculate start and end of the requested month.
    let start = match chrono::NaiveDate::from_ymd_opt(params.year, params.month, 1) {
        Some(d) => d.and_hms_opt(0, 0, 0).unwrap().and_utc(),
        None    => return bad_request("Invalid year/month combination").into_response(),
    };
    let end_month = if params.month == 12 {
        chrono::NaiveDate::from_ymd_opt(params.year + 1, 1, 1)
    } else {
        chrono::NaiveDate::from_ymd_opt(params.year, params.month + 1, 1)
    };
    let end = match end_month {
        Some(d) => d.and_hms_opt(0, 0, 0).unwrap().and_utc(),
        None    => return bad_request("Invalid year/month combination").into_response(),
    };

    // Aggregate trades for the month.
    let mut total_volume = Decimal::ZERO;
    let mut total_fees   = Decimal::ZERO;
    let mut trade_count  = 0u64;

    {
        // Scan in-memory trade history for this user and period.
        let trade_history = exchange
            .trade_history
            .lock().unwrap_or_else(|e| e.into_inner());

        for trades in trade_history.values() {
            for entry in trades {
                if entry.timestamp < start || entry.timestamp >= end {
                    continue;
                }
                // We don't have direct user_id on TradeHistoryEntry (it's public data);
                // use order history to correlate fees via the user's orders filled in range.
                total_volume += entry.qty * entry.price;
                trade_count += 1;
            }
        }
    }

    // Aggregate fees from order history (orders filled in this period).
    {
        let order_history = exchange
            .order_history
            .lock().unwrap_or_else(|e| e.into_inner());
        for entry in order_history.iter() {
            if entry.user_id != user_id {
                continue;
            }
            if entry.created_at < start || entry.created_at >= end {
                continue;
            }
            if entry.filled_qty > Decimal::ZERO {
                // Estimate fee: filled_qty * price * taker_rate.
                // This is an approximation — precise fee tracking requires
                // a dedicated ledger query (DB mode).
                let fee_engine = exchange.fee_engine.lock().unwrap_or_else(|e| e.into_inner());
                let (_maker_rate, rate) = fee_engine.get_fees(user_id);
                total_fees += entry.filled_qty * entry.price * rate;
            }
        }
    }

    // Deposits and withdrawals in the period.
    let deposit_total = {
        let dm = exchange.deposit_manager.lock().unwrap_or_else(|e| e.into_inner());
        dm.user_deposits(&user_id)
            .iter()
            .filter(|d| {
                d.created_at >= start
                    && d.created_at < end
                    && d.status == exchange_custody::DepositStatus::Credited
            })
            .map(|d| d.amount)
            .sum::<Decimal>()
    };

    let withdrawal_total = {
        let wm = exchange.withdrawal_manager.lock().unwrap_or_else(|e| e.into_inner());
        wm.user_withdrawals(&user_id)
            .iter()
            .filter(|w| {
                w.created_at >= start
                    && w.created_at < end
                    && matches!(
                        w.status,
                        exchange_custody::WithdrawalStatus::Completed { .. }
                    )
            })
            .map(|w| w.amount)
            .sum::<Decimal>()
    };

    // Realized PnL from closed positions in this period.
    let realized_pnl = {
        let closed = exchange
            .closed_positions
            .lock().unwrap_or_else(|e| e.into_inner());
        closed
            .iter()
            .filter(|p| p.user_id == user_id && p.closed_at >= start && p.closed_at < end)
            .map(|p| p.realized_pnl)
            .sum::<Decimal>()
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "year": params.year,
            "month": params.month,
            "total_volume_usd": total_volume,
            "total_fees_paid": total_fees,
            "trade_count": trade_count,
            "realized_pnl": realized_pnl,
            "deposits": deposit_total,
            "withdrawals": withdrawal_total,
        })),
    )
    .into_response()
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Method, Request};
    use axum::Router;
    use serde_json::Value;
    use tower::ServiceExt;

    use crate::exchange::Exchange;
    use crate::server::build_router;

    fn setup() -> Router {
        build_router(Exchange::new())
    }

    async fn body_json(resp: axum::response::Response) -> Value {
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    // ── Counterparties ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_block_trade_counterparties_empty() {
        let app = setup();
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/block-trade/counterparties")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json["counterparties"].is_array());
    }

    #[tokio::test]
    async fn test_block_trade_counterparties_includes_mm() {
        let exchange = Exchange::new();
        // Register a market maker.
        {
            let mut mmp = exchange.market_maker_program.lock().unwrap_or_else(|e| e.into_inner());
            mmp.register(uuid::Uuid::new_v4(), "TestMM", None, None);
        }
        let app = build_router(exchange);
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/block-trade/counterparties")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let list = json["counterparties"].as_array().unwrap();
        assert!(!list.is_empty(), "Should include the registered MM");
        assert_eq!(list[0]["name"], "TestMM");
    }

    // ── Public block trade feed ───────────────────────────────────────────────

    #[tokio::test]
    async fn test_public_block_trades_empty() {
        let app = setup();
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/market/block-trades/BTCUSDT")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["trades"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_public_block_trades_filters_by_symbol() {
        let exchange = Exchange::new();
        {
            let mut trades = exchange.block_trades.lock().unwrap_or_else(|e| e.into_inner());
            trades.push(crate::exchange::BlockTrade {
                id: uuid::Uuid::new_v4(),
                rfq_id: uuid::Uuid::new_v4(),
                maker_id: uuid::Uuid::nil(),
                taker_id: uuid::Uuid::nil(),
                symbol: "BTCUSDT".into(),
                price: Decimal::from(50000),
                qty: Decimal::from(5),
                taker_side: "Buy".into(),
                timestamp: chrono::Utc::now(),
            });
            trades.push(crate::exchange::BlockTrade {
                id: uuid::Uuid::new_v4(),
                rfq_id: uuid::Uuid::new_v4(),
                maker_id: uuid::Uuid::nil(),
                taker_id: uuid::Uuid::nil(),
                symbol: "ETHUSDT".into(),
                price: Decimal::from(3000),
                qty: Decimal::from(50),
                taker_side: "Sell".into(),
                timestamp: chrono::Utc::now(),
            });
        }
        let app = build_router(exchange);
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/market/block-trades/BTCUSDT")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let list = json["trades"].as_array().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["symbol"], "BTCUSDT");
        // User IDs must NOT be exposed.
        assert!(list[0].get("maker_id").is_none());
        assert!(list[0].get("taker_id").is_none());
    }

    // ── Block tickers ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_block_tickers() {
        let exchange = Exchange::new();
        {
            let mut trades = exchange.block_trades.lock().unwrap_or_else(|e| e.into_inner());
            for _ in 0..3 {
                trades.push(crate::exchange::BlockTrade {
                    id: uuid::Uuid::new_v4(),
                    rfq_id: uuid::Uuid::new_v4(),
                    maker_id: uuid::Uuid::nil(),
                    taker_id: uuid::Uuid::nil(),
                    symbol: "BTCUSDT".into(),
                    price: Decimal::from(50000),
                    qty: Decimal::from(5),
                    taker_side: "Buy".into(),
                    timestamp: chrono::Utc::now(),
                });
            }
        }
        let app = build_router(exchange);
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/market/block-tickers")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let tickers = json["tickers"].as_array().unwrap();
        assert!(!tickers.is_empty());
        let btc = tickers.iter().find(|t| t["symbol"] == "BTCUSDT").unwrap();
        assert_eq!(btc["trade_count_24h"], 3);
    }

    // ── System status ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_get_rich_system_status() {
        let app = setup();
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["status"], "operational");
        assert!(json["components"].is_object());
        assert!(json["incidents"].is_array());
    }

    #[tokio::test]
    async fn test_admin_update_component_status() {
        let app = setup();
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/admin/status/component")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "component": "deposits",
                            "status": "degraded",
                            "message": "Slow deposit processing"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["status"], "degraded");
        assert_eq!(json["overall"], "degraded");
    }

    #[tokio::test]
    async fn test_admin_create_incident() {
        let app = setup();
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/admin/status/incident")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "title": "Elevated API latency",
                            "status": "investigating",
                            "message": "We are investigating increased latency on the REST API.",
                            "affected_components": ["api"]
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["status"], "investigating");
        assert!(json["id"].is_string());
    }

    #[tokio::test]
    async fn test_list_status_incidents() {
        let exchange = Exchange::new();
        {
            let mut status = exchange.system_status.lock().unwrap_or_else(|e| e.into_inner());
            status.incidents.push(crate::exchange::Incident {
                id: uuid::Uuid::new_v4(),
                title: "Test incident".into(),
                status: "resolved".into(),
                message: "All good now".into(),
                affected_components: vec!["api".into()],
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            });
        }
        let app = build_router(exchange);
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/status/incidents")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let list = json["incidents"].as_array().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["title"], "Test incident");
    }

    // ── Order precheck ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_order_precheck_valid_limit() {
        let exchange = Exchange::new();
        // Fund the user (account must exist before deposit).
        let user_id = uuid::Uuid::nil();
        exchange.create_account(user_id);
        exchange.deposit(user_id, "USDT", Decimal::from(100_000)).unwrap();

        let app = build_router(exchange);
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/orders/precheck")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "symbol": "BTCUSDT",
                            "side": "buy",
                            "order_type": "limit",
                            "price": "50000",
                            "qty": "1"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        // Decimal values serialize as strings in JSON.
        assert!(!json["estimated_fee"].is_null(), "estimated_fee must be present");
        assert!(!json["estimated_margin"].is_null(), "estimated_margin must be present");
        assert!(!json["notional"].is_null(), "notional must be present");
    }

    #[tokio::test]
    async fn test_order_precheck_invalid_side() {
        let app = setup();
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/orders/precheck")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "symbol": "BTCUSDT",
                            "side": "long",
                            "order_type": "limit",
                            "price": "50000",
                            "qty": "1"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // ── Asset valuation ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_asset_valuation() {
        let exchange = Exchange::new();
        let user_id = uuid::Uuid::nil();
        exchange.create_account(user_id);
        exchange.deposit(user_id, "USDT", Decimal::from(10_000)).unwrap();

        let app = build_router(exchange);
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/account/asset-valuation")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(!json["total_usd_value"].is_null(), "total_usd_value must be present");
        // Decimal values serialize as strings — parse the value.
        let total: Decimal = json["total_usd_value"].as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| json["total_usd_value"].as_f64().map(|v| Decimal::try_from(v).unwrap_or(Decimal::ZERO)))
            .unwrap_or(Decimal::ZERO);
        // At minimum the 10,000 USDT we deposited should appear.
        assert!(total >= Decimal::from(10_000), "Expected at least 10000 USDT, got {}", total);
    }

    // ── Monthly statement ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_monthly_statement_no_activity() {
        let app = setup();
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/account/monthly-statement/2025/3")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["year"], 2025);
        assert_eq!(json["month"], 3);
    }

    #[tokio::test]
    async fn test_monthly_statement_invalid_month() {
        let app = setup();
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/account/monthly-statement/2025/13")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // ── RFQ cancel-all ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_rfq_cancel_all() {
        let exchange = Exchange::new();
        let user_id = uuid::Uuid::nil();
        // Seed two open RFQs for our user.
        {
            let mut rfqs = exchange.block_trade_rfqs.lock().unwrap_or_else(|e| e.into_inner());
            for _ in 0..2 {
                let id = uuid::Uuid::new_v4();
                rfqs.insert(
                    id,
                    crate::exchange::BlockTradeRfq {
                        id,
                        taker_id: user_id,
                        symbol: "BTCUSDT".into(),
                        side: "Buy".into(),
                        qty: Decimal::from(5),
                        legs: Vec::new(),
                        status: crate::exchange::RfqStatus::AwaitingQuotes,
                        created_at: chrono::Utc::now(),
                        expires_at: chrono::Utc::now() + chrono::Duration::minutes(5),
                        quotes: Vec::new(),
                    },
                );
            }
        }

        let app = build_router(exchange);
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/block-trade/rfq/cancel-all")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["cancelled"], 2);
    }
}
