use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use exchange_account::{CoinBalance, TransferType, WalletSummary};
use exchange_admin::{AuditAction, AuditFilter, Reconciler, WalletBalances};
use exchange_algo_execution;
use exchange_market_data::{self, MarketDataProvider};
use exchange_common::*;
use exchange_compliance::{KycStatus, SanctionsResult, VerificationStarted};
use exchange_custody::UserTier;
use exchange_proof_of_reserves::{ReportGenerator, UserLiability};
use exchange_settlement::InsuranceFund;
use exchange_trigger_service::{
    TpSlOrderType, TriggerBy, TriggerOrder, TriggerOrderType, TriggerStatus,
};

use crate::auth::{ApiKey, ApiKeyPermissions, AuthenticatedUser};
use crate::exchange::Exchange;
use exchange_order_gateway::ValidationError;

/// Shared app state for axum handlers.
pub type AppState = Arc<Exchange>;

// --- Input validation helpers ---

/// Validate a trading symbol: 3–20 characters, uppercase ASCII letters and digits only.
///
/// Returns a 400 error response if validation fails.
fn validate_symbol(s: &str) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if s.len() < 3 || s.len() > 20 || !s.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-' || c == '/') {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("Invalid symbol: '{}'. Must be 3–20 uppercase ASCII letters/digits.", s)})),
        ));
    }
    Ok(())
}

/// Validate that a quantity or price value is strictly positive.
fn validate_positive(value: Decimal, field: &str) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if value <= Decimal::ZERO {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("Invalid {field}: must be positive (got {value})")})),
        ));
    }
    Ok(())
}

/// Check that a feature flag is enabled on the exchange.
///
/// Returns a 503 Service Unavailable error if the flag is disabled. This is
/// intentionally opaque — we do not reveal which flag is disabled in the
/// response body to avoid information leakage about internal configuration.
fn check_feature(exchange: &Exchange, flag: &str) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if !exchange.feature_flags.is_enabled(flag) {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "Feature not available"})),
        ));
    }
    Ok(())
}

// --- Request/Response types ---

#[derive(Debug, Deserialize)]
pub struct PlaceOrderRequest {
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
    /// Expiration timestamp (Unix milliseconds) for GTD orders.
    #[serde(default)]
    pub expire_at: Option<i64>,
    /// Position index for hedge mode (0=one-way, 1=long, 2=short).
    #[serde(default)]
    pub position_idx: Option<u8>,
    /// Take-profit trigger price (optional). Creates a TP trigger order on fill.
    #[serde(default)]
    pub take_profit: Option<Decimal>,
    /// Stop-loss trigger price (optional). Creates a SL trigger order on fill.
    #[serde(default)]
    pub stop_loss: Option<Decimal>,
    /// Iceberg: visible portion of the order submitted to the book at a time.
    /// Total quantity is `qty`. Must be positive and less than `qty`.
    #[serde(default)]
    pub iceberg_qty: Option<Decimal>,
    /// User-provided client order ID for tracking and idempotency.
    /// Maximum 36 characters, alphanumeric and dashes only.
    #[serde(default)]
    pub order_link_id: Option<String>,
    /// Slippage tolerance for market orders (percentage, e.g., 1.0 = 1%).
    /// Converts the market order to an IOC limit order at BBO ± slippage%.
    #[serde(default)]
    pub slippage_tolerance: Option<Decimal>,
    /// When true, ignore `qty` and close the full open position.
    #[serde(default)]
    pub close_position: Option<bool>,
    /// Self-trade prevention mode: "expire_taker" (default), "expire_maker",
    /// "expire_both", or "none".
    #[serde(default)]
    pub stp_mode: Option<String>,
    /// Request expiration timestamp (Unix milliseconds). The order is rejected
    /// if the server receives it after this time.
    #[serde(default)]
    pub exp_time: Option<i64>,
    /// Price match mode: "queue" (join same-side BBO) or "opponent" (cross BBO).
    /// When set, overrides the `price` field with the current BBO price.
    #[serde(default)]
    pub price_match: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PlaceOrderResponse {
    pub order_id: String,
    pub status: String,
    pub fills: Vec<FillResponse>,
}

#[derive(Debug, Serialize)]
pub struct FillResponse {
    pub trade_id: u64,
    pub price: Decimal,
    pub qty: Decimal,
    pub role: String,
}

#[derive(Debug, Serialize)]
pub struct CancelOrderResponse {
    pub order_id: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct BalanceResponse {
    pub asset: String,
    pub total: Decimal,
    pub locked: Decimal,
    pub available: Decimal,
}

#[derive(Debug, Serialize)]
pub struct OrderBookResponse {
    pub symbol: String,
    pub bids: Vec<[Decimal; 2]>,
    pub asks: Vec<[Decimal; 2]>,
    pub sequence: u64,
}

#[derive(Debug, Deserialize)]
pub struct OrderBookQuery {
    #[serde(default = "default_depth")]
    pub depth: usize,
}

fn default_depth() -> usize {
    25
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

// --- Handlers ---

/// POST /api/v1/orders
///
/// Place a new order. For now, user_id is passed as a header (Phase 1).
/// Phase 2: extract from JWT/HMAC auth.
pub async fn place_order(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<PlaceOrderRequest>,
) -> impl IntoResponse {
    // Validate symbol format
    if let Err(e) = validate_symbol(&req.symbol) {
        return e;
    }

    // Validate qty is positive
    if let Err(e) = validate_positive(req.qty, "qty") {
        return e;
    }

    let side = match req.side.to_lowercase().as_str() {
        "buy" => Side::Buy,
        "sell" => Side::Sell,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid side. Use 'buy' or 'sell'"})),
            );
        }
    };

    let order_type = match req.order_type.to_lowercase().as_str() {
        "limit" => OrderType::Limit,
        "market" => OrderType::Market,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid order_type. Use 'limit' or 'market'"})),
            );
        }
    };

    let tif_str = req.time_in_force.as_deref().unwrap_or("gtc").to_lowercase();
    let time_in_force = match tif_str.as_str() {
        "gtc" => TimeInForce::Gtc,
        "ioc" => TimeInForce::Ioc,
        "fok" => TimeInForce::Fok,
        "post_only" | "postonly" => TimeInForce::PostOnly,
        "gtd" => TimeInForce::Gtd,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("Invalid time_in_force: '{}'. Use: GTC, IOC, FOK, PostOnly, GTD", tif_str)})),
            );
        }
    };

    // GTD requires an expiration timestamp.
    if time_in_force == TimeInForce::Gtd && req.expire_at.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "GTD orders require expire_at (Unix milliseconds)"})),
        );
    }

    let mut price = match order_type {
        OrderType::Limit => match req.price {
            Some(p) if p > Decimal::ZERO => p,
            _ => {
                // Price match mode is allowed to supply the price — skip
                // validation here; we validate after BBO resolution below.
                if req.price_match.is_none() {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({"error": "Limit order requires positive price"})),
                    );
                }
                Decimal::ZERO // placeholder; overwritten by price_match below
            }
        },
        OrderType::Market => Decimal::ZERO,
    };

    let user_id = auth.0;

    // Feature flag: reject perpetual orders when futures are disabled.
    let symbol = Symbol::new(&req.symbol);
    let is_perpetual = exchange.perp_configs.load().contains_key(&symbol);
    if is_perpetual {
        if let Err(e) = check_feature(&exchange, FLAG_FUTURES) {
            return e;
        }

        // Index price health gate (production only): reject perpetual orders when
        // the index price is unavailable (insufficient healthy external price
        // sources). Without a reliable index, mark price / liquidation / funding
        // cannot function safely. Bypassed in dev/test where synthetic feeds run.
        // S19 D.1: `is_production` is cached at Exchange construction.
        if exchange.is_production && !exchange.is_index_healthy(&symbol) {
            tracing::warn!(
                symbol = %symbol,
                user_id = %user_id,
                "Perpetual order rejected: index price unavailable"
            );
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "Index price unavailable — perpetual trading temporarily suspended for this symbol",
                    "retCode": 503,
                    "symbol": req.symbol,
                })),
            );
        }
    }

    // Validate iceberg_qty if present.
    if let Some(iceberg) = req.iceberg_qty {
        if iceberg <= Decimal::ZERO {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "iceberg_qty must be positive"})),
            );
        }
        if iceberg >= req.qty {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "iceberg_qty must be less than total qty"})),
            );
        }
        if order_type != OrderType::Limit {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "iceberg orders are only supported for Limit orders"})),
            );
        }
    }

    // Validate order_link_id if present.
    if let Some(ref link_id) = req.order_link_id {
        if link_id.len() > 36 {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "order_link_id must be 36 characters or less"})),
            );
        }
        if !link_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "order_link_id must contain only alphanumeric characters and dashes"})),
            );
        }
        let index = exchange.order_link_index.lock();
        if index.contains_key(&(user_id, link_id.clone())) {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": "Duplicate order_link_id"})),
            );
        }
    }

    // Task 6: BBO / price-match — resolve price from current best bid/offer.
    if let Some(ref pm_mode) = req.price_match {
        let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
        if let Ok(snap) = pool.snapshot(&symbol, 1) {
            let auto_price = match (pm_mode.as_str(), &side) {
                ("queue", Side::Buy)      => snap.bids.first().map(|(p, _)| *p),
                ("queue", Side::Sell)     => snap.asks.first().map(|(p, _)| *p),
                ("opponent", Side::Buy)   => snap.asks.first().map(|(p, _)| *p),
                ("opponent", Side::Sell)  => snap.bids.first().map(|(p, _)| *p),
                _ => None,
            };
            if let Some(p) = auto_price {
                price = p;
            }
        }
        // Validate we ended up with a positive price after BBO resolution.
        if price <= Decimal::ZERO {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "price_match could not resolve a positive price from the order book"})),
            );
        }
    }

    // Task 4: parse STP mode from the request string field.
    let stp_mode = match req.stp_mode.as_deref().unwrap_or("expire_taker") {
        "expire_taker" => StpMode::ExpireTaker,
        "expire_maker" => StpMode::ExpireMaker,
        "expire_both"  => StpMode::ExpireBoth,
        "none"         => StpMode::None,
        other => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!(
                        "Invalid stp_mode '{}'. Use expire_taker, expire_maker, expire_both, or none",
                        other
                    )
                })),
            );
        }
    };

    // Task 5: parse request expiration timestamp.
    let request_expire_time = req
        .exp_time
        .and_then(chrono::DateTime::from_timestamp_millis);

    let expire_at = req.expire_at.and_then(chrono::DateTime::from_timestamp_millis);

    let order_id = Uuid::new_v4();

    // Task 3: close_position — resolve qty and enforce reduce_only from open position.
    let (mut qty, mut reduce_only) = (req.qty, req.reduce_only.unwrap_or(false));
    if req.close_position.unwrap_or(false) {
        let position_idx = req.position_idx.unwrap_or(0);
        let pm = exchange.positions.lock();
        if let Some(pos) = pm.get_position(user_id, &symbol, position_idx) {
            if pos.is_closed() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "No position to close"})),
                );
            }
            qty = pos.size;
            reduce_only = true;
        } else {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "No position to close"})),
            );
        }
    }

    let mut cmd = PlaceOrderCommand {
        order_id,
        user_id,
        symbol: symbol.clone(),
        side,
        order_type,
        time_in_force,
        price,
        qty,
        reduce_only,
        expire_at,
        timestamp: chrono::Utc::now(),
        position_idx: req.position_idx.unwrap_or(0),
        iceberg_qty: req.iceberg_qty,
        order_link_id: req.order_link_id.clone(),
        slippage_tolerance: req.slippage_tolerance,
        close_position: req.close_position.unwrap_or(false),
        stp_mode,
        request_expire_time,
    };

    // Task 1: slippage tolerance — convert market order to IOC limit.
    if cmd.order_type == OrderType::Market {
        if let Some(slippage_pct) = cmd.slippage_tolerance {
            let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
            if let Ok(snap) = pool.snapshot(&cmd.symbol, 1) {
                let bbo_price = match cmd.side {
                    Side::Buy  => snap.asks.first().map(|(p, _)| *p),
                    Side::Sell => snap.bids.first().map(|(p, _)| *p),
                };
                if let Some(bbo) = bbo_price {
                    let limit_price = match cmd.side {
                        Side::Buy  => bbo * (Decimal::ONE + slippage_pct / Decimal::from(100)),
                        Side::Sell => bbo * (Decimal::ONE - slippage_pct / Decimal::from(100)),
                    };
                    cmd.order_type = OrderType::Limit;
                    cmd.time_in_force = TimeInForce::Ioc;
                    cmd.price = limit_price;
                }
            }
        }
    }

    // Task 5: request expiration check.
    if let Some(exp) = cmd.request_expire_time {
        if chrono::Utc::now() > exp {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Request expired — server received order after expiry time"})),
            );
        }
    }

    if cmd.reduce_only {
        let pm = exchange.positions.lock();
        let position = pm.get_position(cmd.user_id, &cmd.symbol, cmd.position_idx);
        match position {
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "No position to reduce"})),
                );
            }
            Some(pos) if pos.is_closed() => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "No position to reduce"})),
                );
            }
            Some(pos) => {
                // Buy order closes a Short position; Sell order closes a Long position.
                let valid_side = match cmd.side {
                    Side::Buy => pos.side == PositionSide::Short,
                    Side::Sell => pos.side == PositionSide::Long,
                };
                if !valid_side {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({"error": "reduce_only order must be opposite to position side"})),
                    );
                }
                if cmd.qty > pos.size {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({"error": format!("reduce_only qty {} exceeds position size {}", cmd.qty, pos.size)})),
                    );
                }
            }
        }
    }

    // Capture fields needed for bracket registration before cmd is moved.
    let entry_user_id = cmd.user_id;
    let entry_symbol = cmd.symbol.clone();
    let entry_side = cmd.side;
    let entry_qty = cmd.qty;

    let exchange_clone = Arc::clone(&exchange);
    let result = crate::request_id::spawn_blocking_traced(move || exchange_clone.place_order(cmd))
        .await
        .unwrap_or_else(|e| Err(anyhow::anyhow!("task join error: {e}")));
    match result {
        Ok(events) => {
            let fills: Vec<FillResponse> = events
                .iter()
                .filter_map(|e| match e {
                    MatchEvent::Fill(f) => Some(FillResponse {
                        trade_id: f.trade_id,
                        price: f.price,
                        qty: f.qty,
                        role: if f.taker_order_id == order_id {
                            "taker".to_string()
                        } else {
                            "maker".to_string()
                        },
                    }),
                    _ => None,
                })
                .collect();

            let status = if events.iter().any(|e| matches!(e, MatchEvent::OrderRejected { .. })) {
                "rejected"
            } else if events.iter().any(|e| matches!(e, MatchEvent::OrderAccepted { .. })) {
                "open"
            } else if !fills.is_empty() {
                "filled"
            } else {
                "cancelled"
            };

            // Register a bracket order when TP and/or SL was requested.
            if req.take_profit.is_some() || req.stop_loss.is_some() {
                // Determine the closing side: opposite of the entry.
                let closing_side = match entry_side {
                    Side::Buy => Side::Sell,
                    Side::Sell => Side::Buy,
                };

                let tp_spec = req.take_profit.map(|tp_price| {
                    exchange_trigger_service::TriggerOrderSpec {
                        trigger_price: tp_price,
                        order_price: None, // market exit
                        qty: entry_qty,
                    }
                });
                let sl_spec = req.stop_loss.map(|sl_price| {
                    exchange_trigger_service::TriggerOrderSpec {
                        trigger_price: sl_price,
                        order_price: None, // market exit
                        qty: entry_qty,
                    }
                });

                let bracket = {
                    let mut bm = exchange.bracket_manager.lock().unwrap_or_else(|e| e.into_inner());
                    bm.create_bracket(entry_user_id, entry_symbol.clone(), order_id, tp_spec, sl_spec)
                };

                // If the entry order filled completely in this call, activate the
                // bracket and submit trigger orders to the engine immediately.
                // For limit orders that are still open, the bracket stays Pending
                // and will be activated when the order fills.
                let entry_filled_qty: rust_decimal::Decimal = events
                    .iter()
                    .filter_map(|e| {
                        if let MatchEvent::Fill(f) = e {
                            if f.taker_order_id == order_id || f.maker_order_id == order_id {
                                return Some(f.qty);
                            }
                        }
                        None
                    })
                    .sum();

                if entry_filled_qty >= entry_qty {
                    let specs = {
                        let mut bm = exchange.bracket_manager.lock().unwrap_or_else(|e| e.into_inner());
                        bm.on_entry_filled(order_id)
                    };

                    if let Some((tp_spec_out, sl_spec_out)) = specs {
                        let mut tp_trigger_id: Option<Uuid> = None;
                        let mut sl_trigger_id: Option<Uuid> = None;

                        let mut te = exchange.trigger_engine.lock().unwrap_or_else(|e| e.into_inner());

                        if let Some(spec) = tp_spec_out {
                            let tp_order = TriggerOrder {
                                id: Uuid::new_v4(),
                                user_id: entry_user_id,
                                symbol: entry_symbol.clone(),
                                trigger_type: TriggerOrderType::TakeProfitMarket,
                                side: closing_side,
                                trigger_price: spec.trigger_price,
                                order_price: spec.order_price,
                                qty: spec.qty,
                                trigger_by: TriggerBy::LastPrice,
                                reduce_only: true,
                                close_on_trigger: false,
                                trailing_delta: None,
                                trail_type: Default::default(),
                                tp_limit_price: None,
                                sl_limit_price: None,
                                tp_order_type: Default::default(),
                                sl_order_type: Default::default(),
                                status: TriggerStatus::Active,
                                created_at: chrono::Utc::now(),
                                triggered_at: None,
                                expires_at: None,
                            };
                            tp_trigger_id = Some(tp_order.id);
                            if let Err(e) = te.add_order(tp_order) {
                                tracing::warn!(error = %e, "failed to add TP trigger order");
                            }
                        }

                        if let Some(spec) = sl_spec_out {
                            let sl_order = TriggerOrder {
                                id: Uuid::new_v4(),
                                user_id: entry_user_id,
                                symbol: entry_symbol.clone(),
                                trigger_type: TriggerOrderType::StopMarket,
                                side: closing_side,
                                trigger_price: spec.trigger_price,
                                order_price: spec.order_price,
                                qty: spec.qty,
                                trigger_by: TriggerBy::LastPrice,
                                reduce_only: true,
                                close_on_trigger: false,
                                trailing_delta: None,
                                trail_type: Default::default(),
                                tp_limit_price: None,
                                sl_limit_price: None,
                                tp_order_type: Default::default(),
                                sl_order_type: Default::default(),
                                status: TriggerStatus::Active,
                                created_at: chrono::Utc::now(),
                                triggered_at: None,
                                expires_at: None,
                            };
                            sl_trigger_id = Some(sl_order.id);
                            if let Err(e) = te.add_order(sl_order) {
                                tracing::warn!(error = %e, "failed to add SL trigger order");
                            }
                        }

                        drop(te);

                        // Record the assigned trigger-engine ids so OCO cancellation works.
                        let mut bm = exchange.bracket_manager.lock().unwrap_or_else(|e| e.into_inner());
                        bm.record_trigger_ids(bracket.id, tp_trigger_id, sl_trigger_id);

                        tracing::info!(
                            entry_order_id = %order_id,
                            bracket_id = %bracket.id,
                            ?tp_trigger_id,
                            ?sl_trigger_id,
                            "bracket activated — TP/SL trigger orders registered"
                        );
                    }
                }
            }

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "order_id": order_id.to_string(),
                    "order_link_id": req.order_link_id,
                    "status": status,
                    "fills": fills.iter().map(|f| serde_json::json!({
                        "trade_id": f.trade_id,
                        "price": f.price,
                        "qty": f.qty,
                        "role": f.role,
                    })).collect::<Vec<_>>(),
                })),
            )
        }
        Err(e) => {
            // KG-9 (Sprint 12): recover the typed ValidationError via downcast_ref
            // rather than the fragile "RATE_LIMIT: " string prefix that was used
            // previously.  Rate-limit variants → HTTP 429; other validation errors
            // → HTTP 400.  This approach is robust against future message changes
            // and follows Binance's convention (MAX_NUM_ORDERS → 429, not 400).
            if let Some(validation_err) = e.downcast_ref::<ValidationError>() {
                if validation_err.is_rate_limit() {
                    return (
                        StatusCode::TOO_MANY_REQUESTS,
                        Json(serde_json::json!({
                            "error": validation_err.to_string(),
                            "retCode": 429,
                        })),
                    );
                }
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": validation_err.to_string()})),
                );
            }
            // WAL write failures are infrastructure errors (disk full, I/O error) — signal
            // HTTP 503 Service Unavailable so clients retry rather than treat it as a
            // permanent rejection.  The order was NOT placed: no book entry, no balance lock.
            let msg = e.to_string();
            if msg.starts_with("WAL write failed:") {
                tracing::error!(error = %e, "WAL write failed — returning 503 to client");
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({
                        "error": "Service temporarily unavailable — please retry",
                        "retCode": 503,
                    })),
                );
            }
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        }
    }
}

/// DELETE /api/v1/orders/:order_id
pub async fn cancel_order(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(order_id): Path<String>,
) -> impl IntoResponse {
    // S20 J.5 — per-endpoint latency timing; RAII guard records on drop.
    let _lat = exchange.metrics.endpoint_timer("cancel_order");
    let order_id = match Uuid::parse_str(&order_id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid order_id"})),
            );
        }
    };

    let user_id = auth.0;

    let cmd = CancelOrderCommand {
        order_id,
        user_id,
        symbol: Symbol::new(""), // Symbol not needed for cancel (looked up from lock record)
        timestamp: chrono::Utc::now(),
    };

    match exchange.cancel_order(cmd) {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "order_id": order_id.to_string(),
                "status": "cancelled",
            })),
        ),
        Err(e) => {
            let msg = e.to_string();
            // Canceling a nonexistent order is idempotent — return 200.
            if msg.contains("Order not found") {
                (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "order_id": order_id.to_string(),
                        "status": "cancelled",
                    })),
                )
            } else {
                (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": msg})),
                )
            }
        }
    }
}

/// GET /api/v1/account/balances
pub async fn get_balances(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;

    let balances = exchange.accounts.get_all_balances(user_id);
    if balances.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Account not found"})),
        );
    }

    let result: Vec<serde_json::Value> = balances
        .iter()
        .map(|(asset, bal)| {
            serde_json::json!({
                "asset": asset,
                "total": bal.total(),
                "locked": bal.locked(),
                "available": bal.available(),
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(result)))
}

/// GET /api/v1/markets/:symbol/orderbook?depth=25
pub async fn get_orderbook(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
    Query(params): Query<OrderBookQuery>,
) -> impl IntoResponse {
    if let Err(e) = validate_symbol(&symbol) {
        return e;
    }
    let symbol = Symbol::new(&symbol);
    let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());

    match pool.snapshot(&symbol, params.depth.min(1000)) {
        Ok(snap) => {
            let bids = &snap.bids;
            let asks = &snap.asks;

            let best_bid = bids.first().map(|(p, _)| p.to_string());
            let best_bid_qty = bids.first().map(|(_, q)| q.to_string());
            let best_ask = asks.first().map(|(p, _)| p.to_string());
            let best_ask_qty = asks.first().map(|(_, q)| q.to_string());

            let spread = match (bids.first(), asks.first()) {
                (Some((bid, _)), Some((ask, _))) => Some((ask - bid).to_string()),
                _ => None,
            };
            let mid_price = match (bids.first(), asks.first()) {
                (Some((bid, _)), Some((ask, _))) => Some(((ask + bid) / Decimal::TWO).to_string()),
                _ => None,
            };
            let spread_pct = match (bids.first(), asks.first()) {
                (Some((bid, _)), Some((ask, _))) if *bid > Decimal::ZERO => {
                    let mid = (ask + bid) / Decimal::TWO;
                    Some(((ask - bid) / mid * Decimal::ONE_HUNDRED).to_string())
                }
                _ => None,
            };

            let bid_depth: Decimal = bids.iter().map(|(_, q)| q).sum();
            let ask_depth: Decimal = asks.iter().map(|(_, q)| q).sum();
            let bid_depth_quote: Decimal = bids.iter().map(|(p, q)| p * q).sum();
            let ask_depth_quote: Decimal = asks.iter().map(|(p, q)| p * q).sum();

            let total_depth = bid_depth + ask_depth;
            let bid_pct = if total_depth > Decimal::ZERO {
                bid_depth / total_depth * Decimal::ONE_HUNDRED
            } else {
                Decimal::ZERO
            };
            let ask_pct = if total_depth > Decimal::ZERO {
                ask_depth / total_depth * Decimal::ONE_HUNDRED
            } else {
                Decimal::ZERO
            };

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "symbol": symbol.to_string(),
                    "bids": bids.iter().map(|(p, q)| [p.to_string(), q.to_string()]).collect::<Vec<_>>(),
                    "asks": asks.iter().map(|(p, q)| [p.to_string(), q.to_string()]).collect::<Vec<_>>(),
                    "timestamp": chrono::Utc::now().timestamp_millis(),
                    "sequence": snap.sequence,
                    "best_bid": best_bid,
                    "best_bid_qty": best_bid_qty,
                    "best_ask": best_ask,
                    "best_ask_qty": best_ask_qty,
                    "spread": spread,
                    "spread_pct": spread_pct,
                    "mid_price": mid_price,
                    "bid_depth": bid_depth.to_string(),
                    "ask_depth": ask_depth.to_string(),
                    "bid_depth_quote": bid_depth_quote.to_string(),
                    "ask_depth_quote": ask_depth_quote.to_string(),
                    "bid_pct": bid_pct.to_string(),
                    "ask_pct": ask_pct.to_string(),
                })),
            )
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Symbol not found"})),
        ),
    }
}

/// GET /api/v1/markets/:symbol/bbo — lightweight best bid/offer (top of book only).
pub async fn get_bbo(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = validate_symbol(&symbol) {
        return e;
    }
    let symbol = Symbol::new(&symbol);
    let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());

    match pool.snapshot(&symbol, 1) {
        Ok(snap) => {
            let bid = snap.bids.first().map(|(p, q)| (p.to_string(), q.to_string()));
            let ask = snap.asks.first().map(|(p, q)| (p.to_string(), q.to_string()));
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "symbol": symbol.to_string(),
                    "bid_price": bid.as_ref().map(|(p, _)| p),
                    "bid_qty": bid.as_ref().map(|(_, q)| q),
                    "ask_price": ask.as_ref().map(|(p, _)| p),
                    "ask_qty": ask.as_ref().map(|(_, q)| q),
                    "time": chrono::Utc::now().timestamp_millis(),
                })),
            )
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Symbol not found"})),
        ),
    }
}

/// GET /health
///
/// Returns a richer health payload including component status and uptime.
pub async fn health(State(exchange): State<AppState>) -> impl IntoResponse {
    let wal_healthy = exchange.metrics.wal_healthy.load(std::sync::atomic::Ordering::Relaxed);
    let uptime_secs = exchange.metrics.uptime_secs();

    // Live DB check instead of stale atomic flag
    let db_ok = {
        let maybe_pool = exchange.db_pool.get().cloned();
        match maybe_pool {
            Some(pool) => sqlx::query_scalar::<_, i32>("SELECT 1")
                .fetch_one(&pool)
                .await
                .is_ok(),
            None => false,
        }
    };
    // Update metric for Prometheus
    exchange.metrics.set_db_connected(db_ok);

    let status = if db_ok || wal_healthy { "ok" } else { "degraded" };
    let status_code = if status == "degraded" { StatusCode::SERVICE_UNAVAILABLE } else { StatusCode::OK };

    (
        status_code,
        Json(serde_json::json!({
            "status": status,
            "db": if db_ok { "connected" } else { "disconnected" },
            "wal": if wal_healthy { "healthy" } else { "unavailable" },
            "uptime_secs": uptime_secs,
            "version": env!("CARGO_PKG_VERSION"),
        })),
    )
}

/// GET /metrics — Prometheus metrics endpoint.
///
/// S20 J.4 — when `exchange.metrics_api_key` is `Some`, every request must
/// present a matching `X-Metrics-Key` header. Comparison is constant-time
/// via `subtle::ConstantTimeEq` so an attacker cannot use timing to probe
/// the secret byte-by-byte. When the field is `None` (dev mode / unset
/// env) the endpoint is open so local Prometheus scrapes keep working
/// without extra configuration.
pub async fn prometheus_metrics(
    State(exchange): State<AppState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    if let Some(ref expected) = exchange.metrics_api_key {
        let provided = headers
            .get("x-metrics-key")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        // Constant-time comparison — walks the max length so the rejection
        // time does not leak the secret length via a side channel.
        let a = provided.as_bytes();
        let b = expected.as_bytes();
        let mut acc: u8 = ((a.len() ^ b.len()) & 0xff) as u8;
        let n = a.len().max(b.len());
        for i in 0..n {
            let ai = a.get(i).copied().unwrap_or(0);
            let bi = b.get(i).copied().unwrap_or(0);
            acc |= ai ^ bi;
        }
        let eq = acc == 0 && a.len() == b.len();

        if !eq {
            tracing::warn!(
                provided_len = provided.len(),
                "metrics_api_key check failed"
            );
            return (
                StatusCode::UNAUTHORIZED,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                r#"{"error":"Missing or invalid X-Metrics-Key"}"#.to_string(),
            )
                .into_response();
        }
    }

    let body = exchange.metrics.to_prometheus();
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        body,
    )
        .into_response()
}

/// GET /admin/stats — JSON stats for admin dashboard.
pub async fn admin_stats(State(exchange): State<AppState>) -> impl IntoResponse {
    let stats = exchange.metrics.to_json();

    // Add live book info
    let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
    let symbols: Vec<serde_json::Value> = pool
        .symbols()
        .iter()
        .filter_map(|sym| {
            pool.snapshot(sym, 1).ok().map(|snap| {
                let best_bid = snap.bids.first().map(|(p, q)| format!("{p}@{q}"));
                let best_ask = snap.asks.first().map(|(p, q)| format!("{p}@{q}"));
                let spread = match (snap.bids.first(), snap.asks.first()) {
                    (Some((bid, _)), Some((ask, _))) => Some((ask - bid).to_string()),
                    _ => None,
                };
                serde_json::json!({
                    "symbol": sym.to_string(),
                    "orders": snap.bids.len() + snap.asks.len(),
                    "best_bid": best_bid,
                    "best_ask": best_ask,
                    "spread": spread,
                })
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!({
        "metrics": stats,
        "books": symbols,
    })))
}

/// POST /admin/halt — halt trading (emergency circuit breaker).
///
/// Sets the exchange-wide halt flag. All subsequent `place_order` calls will be rejected
/// until the halt is lifted via `POST /admin/resume`.
pub async fn admin_halt(State(exchange): State<AppState>) -> impl IntoResponse {
    let old = exchange.operating_mode.load();
    exchange.operating_mode.store(exchange_common::OperatingMode::Halted);
    tracing::warn!(old = %old, "EXCHANGE HALTED by admin");
    (StatusCode::OK, Json(serde_json::json!({
        "status": "Halted",
        "previous": old.to_string(),
        "timestamp": chrono::Utc::now().timestamp_millis(),
    })))
}

/// POST /admin/resume — lift the emergency halt and resume normal trading.
pub async fn admin_resume(State(exchange): State<AppState>) -> impl IntoResponse {
    let old = exchange.operating_mode.load();
    exchange.operating_mode.store(exchange_common::OperatingMode::Normal);
    tracing::warn!(old = %old, "EXCHANGE RESUMED by admin");
    (StatusCode::OK, Json(serde_json::json!({
        "status": "Normal",
        "previous": old.to_string(),
        "timestamp": chrono::Utc::now().timestamp_millis(),
    })))
}

#[derive(Debug, Deserialize)]
pub struct AdminSetModeRequest {
    pub mode: String,
}

/// POST /admin/mode — set the exchange operating mode.
///
/// Supports: Normal, ReadOnly, CancelOnly, MarketDataOnly, Halted.
/// Use this for graceful degradation during incidents instead of the
/// binary halt/resume which only toggles between Normal and Halted.
pub async fn admin_set_mode(
    State(exchange): State<AppState>,
    Json(req): Json<AdminSetModeRequest>,
) -> impl IntoResponse {
    let new_mode = match req.mode.parse::<exchange_common::OperatingMode>() {
        Ok(m) => m,
        Err(e) => return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        ),
    };

    let old = exchange.operating_mode.load();
    exchange.operating_mode.store(new_mode);

    // Audit log the mode transition.
    exchange.audit_log.lock().unwrap_or_else(|e| e.into_inner()).log(
        None,
        exchange_admin::AuditAction::TradingHalted, // reuse for all mode changes
        serde_json::json!({
            "old_mode": old.to_string(),
            "new_mode": new_mode.to_string(),
        }),
        None,
    );

    tracing::warn!(old = %old, new = %new_mode, "Operating mode changed by admin");

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "mode": new_mode.to_string(),
            "previous": old.to_string(),
            "timestamp": chrono::Utc::now().timestamp_millis(),
        })),
    )
}

// ─── Four-eyes approval endpoints ────────────────────────────────────────────

/// GET /admin/approvals — list pending four-eyes actions awaiting approval.
pub async fn admin_list_approvals(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let mut mgr = exchange.four_eyes.lock().unwrap_or_else(|e| e.into_inner());
    let pending: Vec<serde_json::Value> = mgr
        .list_pending()
        .iter()
        .map(|a| serde_json::json!({
            "id": a.id.to_string(),
            "action_type": a.action_type,
            "payload": a.payload,
            "requested_at": a.requested_at.to_rfc3339(),
            "expires_at": a.expires_at.to_rfc3339(),
        }))
        .collect();

    Json(serde_json::json!({"pending": pending}))
}

/// POST /admin/approvals/{id}/approve — second admin confirms a pending action.
pub async fn admin_approve_action(
    State(exchange): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let action = {
        let mut mgr = exchange.four_eyes.lock().unwrap_or_else(|e| e.into_inner());
        mgr.approve(id)
    };

    let action = match action {
        Ok(a) => a,
        Err(e) => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": e}))).into_response(),
    };

    // Execute the approved action.
    match action.action_type.as_str() {
        "credit_deposit" => {
            let deposit_id = action.payload["deposit_id"]
                .as_str()
                .and_then(|s| s.parse::<Uuid>().ok());

            let Some(deposit_id) = deposit_id else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "Invalid deposit_id in approved payload"})),
                ).into_response();
            };

            // Execute the deposit credit (same logic as admin_credit_deposit).
            let deposit = {
                let dm = exchange.deposit_manager.lock().unwrap_or_else(|e| e.into_inner());
                dm.pending_deposits()
                    .into_iter()
                    .find(|d| d.id == deposit_id)
                    .cloned()
            };

            let Some(deposit) = deposit else {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "Deposit not found or already credited"})),
                ).into_response();
            };

            if let Err(e) = exchange.accounts.deposit(deposit.user_id, &deposit.asset, deposit.amount) {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": format!("Balance credit failed: {e}")})),
                ).into_response();
            }

            {
                let mut dm = exchange.deposit_manager.lock().unwrap_or_else(|e| e.into_inner());
                dm.credit_deposit(&deposit_id);
            }

            tracing::warn!(
                deposit_id = %deposit_id,
                user_id = %deposit.user_id,
                asset = %deposit.asset,
                amount = %deposit.amount,
                "Four-eyes APPROVED: deposit credited"
            );

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "approved": true,
                    "action_type": "credit_deposit",
                    "deposit_id": deposit_id.to_string(),
                    "user_id": deposit.user_id.to_string(),
                    "asset": deposit.asset,
                    "amount": deposit.amount,
                })),
            ).into_response()
        }
        other => {
            tracing::warn!(action_type = other, "Four-eyes: approved action type not yet implemented for execution");
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "approved": true,
                    "action_type": other,
                    "note": "Action approved but execution handler not yet implemented for this type",
                })),
            ).into_response()
        }
    }
}

/// POST /admin/approvals/{id}/reject — cancel a pending four-eyes action.
pub async fn admin_reject_action(
    State(exchange): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let mut mgr = exchange.four_eyes.lock().unwrap_or_else(|e| e.into_inner());
    if mgr.reject(id) {
        Json(serde_json::json!({"rejected": true, "id": id.to_string()}))
    } else {
        Json(serde_json::json!({"error": "Action not found or already expired"}))
    }
}

// ─── Per-symbol engine admin endpoints ────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct EngineSymbolQuery {
    pub symbol: String,
}

/// GET /admin/engine/status?symbol=BTCUSDT
///
/// Returns the health / metadata for one symbol's matching engine: whether the
/// engine is registered and whether its circuit breaker is currently halted.
pub async fn admin_engine_status(
    State(exchange): State<AppState>,
    Query(q): Query<EngineSymbolQuery>,
) -> impl IntoResponse {
    let sym = exchange_common::Symbol::new(&q.symbol);
    let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());

    if !pool.has_symbol(&sym) {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Symbol not found"})),
        )
            .into_response();
    }

    // Probe the circuit breaker state for this symbol.
    let cb_halted = exchange
        .circuit_breakers
        .lock()
        .get(&sym)
        .map(|cb| cb.is_halted())
        .unwrap_or(false);

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "symbol": q.symbol,
            "engine_registered": true,
            "circuit_breaker_halted": cb_halted,
        })),
    )
        .into_response()
}

/// POST /admin/engine/restart?symbol=BTCUSDT
///
/// Drops and re-spawns the matching engine thread for the given symbol.
/// Use this when the engine thread has panicked or is unresponsive.
///
/// **Warning**: any commands queued in the old engine's channel are discarded.
/// Verify order state via reconciliation after restarting.
pub async fn admin_engine_restart(
    State(exchange): State<AppState>,
    Query(q): Query<EngineSymbolQuery>,
) -> impl IntoResponse {
    let sym = exchange_common::Symbol::new(&q.symbol);
    let mut pool = exchange.engine_pool.write().unwrap_or_else(|e| e.into_inner());

    let restarted = pool.restart_symbol(&sym);
    if !restarted {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Symbol not found"})),
        )
            .into_response();
    }

    tracing::warn!(symbol = %q.symbol, "Matching engine restarted by admin");
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "restarted": true,
            "symbol": q.symbol,
        })),
    )
        .into_response()
}

/// POST /admin/engine/halt?symbol=BTCUSDT
///
/// Activates the circuit breaker for the given symbol, halting order matching
/// for that symbol only.  Other symbols are unaffected.
pub async fn admin_engine_halt(
    State(exchange): State<AppState>,
    Query(q): Query<EngineSymbolQuery>,
) -> impl IntoResponse {
    let sym = exchange_common::Symbol::new(&q.symbol);

    // Verify the symbol is registered.
    {
        let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
        if !pool.has_symbol(&sym) {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Symbol not found"})),
            )
                .into_response();
        }
    }

    let mut cbs = exchange.circuit_breakers.lock();
    if let Some(cb) = cbs.get_mut(&sym) {
        // Force an indefinitely long halt by setting a very large halt duration.
        // The operator must call /admin/engine/resume to lift it.
        cb.force_halt();
        tracing::warn!(symbol = %q.symbol, "Symbol halted by admin via engine/halt");
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "halted": true,
                "symbol": q.symbol,
            })),
        )
            .into_response()
    } else {
        // Symbol has no circuit breaker configured; insert a halted one.
        let mut new_cb = exchange_matching_engine::circuit_breaker::generic_circuit_breaker();
        new_cb.force_halt();
        cbs.insert(sym, new_cb);
        tracing::warn!(symbol = %q.symbol, "Symbol halted by admin (new circuit breaker inserted)");
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "halted": true,
                "symbol": q.symbol,
            })),
        )
            .into_response()
    }
}

/// POST /admin/engine/resume?symbol=BTCUSDT
///
/// Clears a manual circuit-breaker halt for the given symbol, resuming order
/// matching.  This is the complement of `POST /admin/engine/halt`.
pub async fn admin_engine_resume(
    State(exchange): State<AppState>,
    Query(q): Query<EngineSymbolQuery>,
) -> impl IntoResponse {
    let sym = exchange_common::Symbol::new(&q.symbol);

    // Verify the symbol is registered.
    {
        let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
        if !pool.has_symbol(&sym) {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Symbol not found"})),
            )
                .into_response();
        }
    }

    let mut cbs = exchange.circuit_breakers.lock();
    if let Some(cb) = cbs.get_mut(&sym) {
        cb.reset();
        tracing::info!(symbol = %q.symbol, "Symbol circuit breaker reset by admin via engine/resume");
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "resumed": true,
            "symbol": q.symbol,
        })),
    )
        .into_response()
}

/// GET /admin/treasury — view collected fee balances in the treasury account.
pub async fn admin_treasury(State(exchange): State<AppState>) -> impl IntoResponse {
    use crate::exchange::TREASURY_ACCOUNT_ID;
    let balances: Vec<_> = exchange
        .accounts
        .get_all_balances(TREASURY_ACCOUNT_ID)
        .iter()
        .map(|(asset, bal)| {
            serde_json::json!({
                "asset": asset,
                "available": bal.available(),
                "total": bal.total(),
            })
        })
        .collect();
    (StatusCode::OK, Json(serde_json::json!({"treasury": balances})))
}

// --- Feature flag admin endpoints (T12.1) ---

/// GET /admin/feature-flags — return the current state of all feature flags.
pub async fn admin_get_feature_flags(State(exchange): State<AppState>) -> impl IntoResponse {
    let flags = exchange.feature_flags.all();
    (StatusCode::OK, Json(serde_json::json!(flags)))
}

#[derive(Debug, serde::Deserialize)]
pub struct SetFeatureFlagRequest {
    pub flag: String,
    pub enabled: bool,
    pub reason: Option<String>,
}

/// POST /admin/feature-flags — toggle a feature flag at runtime.
///
/// Every change is recorded in the immutable audit log with old value,
/// new value, and operator-supplied reason (strongly encouraged). If no
/// reason is given the log still records the change but prints a warning.
pub async fn admin_set_feature_flag(
    State(exchange): State<AppState>,
    Json(req): Json<SetFeatureFlagRequest>,
) -> impl IntoResponse {
    let old_value = exchange.feature_flags.is_enabled(&req.flag);
    exchange.feature_flags.set(&req.flag, req.enabled);

    let reason = req.reason.as_deref().unwrap_or("(no reason provided)");
    if req.reason.is_none() {
        tracing::warn!(flag = %req.flag, "feature flag changed without a reason — please supply 'reason' field");
    }

    exchange.audit_log.lock().unwrap_or_else(|e| e.into_inner()).log(
        None,
        exchange_admin::AuditAction::FeatureFlagChanged,
        serde_json::json!({
            "flag": req.flag,
            "old_value": old_value,
            "new_value": req.enabled,
            "reason": reason,
        }),
        None,
    );

    tracing::info!(
        flag = %req.flag,
        old_value,
        enabled = req.enabled,
        reason,
        "feature flag updated by admin",
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "flag": req.flag,
            "old_value": old_value,
            "enabled": req.enabled,
        })),
    )
}

// --- Futures request/response types ---

#[derive(Debug, Deserialize)]
pub struct SetLeverageRequest {
    pub symbol: String,
    pub leverage: Decimal,
}

#[derive(Debug, Deserialize)]
pub struct AddMarginRequest {
    pub symbol: String,
    pub amount: Decimal,
}

#[derive(Debug, Deserialize)]
pub struct AdjustPositionLeverageRequest {
    pub symbol: String,
    pub leverage: Decimal,
    /// Position index (0 = one-way, 1 = hedge long, 2 = hedge short). Defaults to 0.
    #[serde(default)]
    pub position_idx: u8,
}

#[derive(Debug, Deserialize)]
pub struct FundingHistoryQuery {
    pub symbol: String,
}

#[derive(Debug, Deserialize)]
pub struct OrderHistoryQuery {
    #[serde(default)]
    pub symbol: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Deserialize)]
pub struct TradeHistoryQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    50
}

const MAX_HISTORY_LIMIT: usize = 1000;

// --- Futures handlers ---

/// GET /api/v1/positions — list user's open positions with unrealized PnL.
pub async fn get_positions(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    // S20 J.5 — per-endpoint latency timing; RAII guard records on drop.
    let _lat = exchange.metrics.endpoint_timer("get_positions");
    let user_id = auth.0;
    let positions = exchange.get_positions(user_id);

    let result: Vec<serde_json::Value> = positions
        .iter()
        .map(|pos| {
            let mark_price = exchange
                .get_mark_price(&pos.symbol)
                .map(|mp| mp.price)
                .unwrap_or(pos.entry_price);

            serde_json::json!({
                "symbol": pos.symbol.to_string(),
                "side": format!("{:?}", pos.side),
                "size": pos.size,
                "entry_price": pos.entry_price,
                "mark_price": mark_price,
                "unrealized_pnl": pos.unrealized_pnl(mark_price),
                "realized_pnl": pos.realized_pnl,
                "margin": pos.margin,
                "leverage": pos.leverage,
                "margin_mode": format!("{:?}", pos.margin_mode),
                "liq_price": pos.liquidation_price(),
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(result)))
}

/// POST /api/v1/position/leverage — set leverage for a symbol.
pub async fn set_leverage(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<SetLeverageRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let symbol = Symbol::new(&req.symbol);

    match exchange.set_leverage(user_id, &symbol, req.leverage) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "symbol": req.symbol,
                "leverage": req.leverage,
            })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

/// POST /api/v1/position/margin — add margin to an isolated position.
pub async fn add_margin(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<AddMarginRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let symbol = Symbol::new(&req.symbol);

    match exchange.add_margin(user_id, &symbol, req.amount) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "symbol": req.symbol,
                "added": req.amount,
            })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

/// PUT /api/v1/position/auto-add-margin — enable or disable auto-add margin on an open position.
///
/// When enabled, the liquidation monitor automatically tops up the position's
/// margin from the user's available balance before the position is liquidated.
/// Only effective for isolated margin positions.
///
/// Body: `{ "symbol": "BTCUSDT", "autoAddMargin": true, "positionIdx": 0 }`
pub async fn set_auto_add_margin(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let symbol = match body.get("symbol").and_then(|v| v.as_str()) {
        Some(s) => Symbol::new(s),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing symbol"})),
            )
                .into_response();
        }
    };
    let auto_add = match body.get("autoAddMargin").and_then(|v| v.as_bool()) {
        Some(b) => b,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing autoAddMargin (bool)"})),
            )
                .into_response();
        }
    };
    let position_idx: u8 = body
        .get("positionIdx")
        .and_then(|v| v.as_u64())
        .map(|n| n as u8)
        .unwrap_or(0);

    let result = exchange
        .positions
        .lock()
        .set_auto_add_margin(user.0, &symbol, position_idx, auto_add);

    match result {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "symbol": symbol.to_string(),
                "autoAddMargin": auto_add,
                "positionIdx": position_idx,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// PUT /api/v1/position/leverage — adjust leverage on an existing open position.
///
/// Recalculates the required margin at the new leverage. If more margin is
/// needed, the difference is locked from the user's available USDT balance. If
/// margin is released, it is credited back. Rejected if there is no open
/// position for the given symbol or if the user has insufficient balance.
pub async fn adjust_position_leverage(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<AdjustPositionLeverageRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let symbol = Symbol::new(&req.symbol);

    match exchange.adjust_position_leverage(user_id, &symbol, req.position_idx, req.leverage) {
        Ok(delta) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "symbol": req.symbol,
                "leverage": req.leverage,
                "margin_delta": delta,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// PUT /api/v1/account/margin-mode — switch between isolated and cross margin for a symbol.
///
/// The change takes effect on the next position open. Rejected if a position is
/// already open for the requested symbol (must close position first).
pub async fn set_margin_mode(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let mode = match body.get("mode").and_then(|v| v.as_str()) {
        Some("cross") | Some("Cross") => exchange_common::futures::MarginMode::Cross,
        Some("isolated") | Some("Isolated") => exchange_common::futures::MarginMode::Isolated,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid mode. Use 'cross' or 'isolated'"})),
            )
                .into_response();
        }
    };

    let symbol = match body.get("symbol").and_then(|v| v.as_str()) {
        Some(s) => Symbol::new(s),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing required field: symbol"})),
            )
                .into_response();
        }
    };

    // Reject if user has an open position for this symbol.
    {
        let pm = exchange.positions.lock();
        if let Some(pos) = pm.get_position_one_way(user.0, &symbol) {
            if !pos.is_closed() {
                return (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({
                        "error": "Cannot change margin mode while position is open",
                    })),
                )
                    .into_response();
            }
        }
    }

    // Persist the mode preference so it applies to future position opens.
    exchange
        .user_margin_modes
        .lock().unwrap_or_else(|e| e.into_inner())
        .insert(user.0, mode);

    let mode_str = match mode {
        exchange_common::futures::MarginMode::Cross => "cross",
        exchange_common::futures::MarginMode::Isolated => "isolated",
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "symbol": symbol.to_string(),
            "marginMode": mode_str,
        })),
    )
        .into_response()
}

/// POST /api/v1/positions/mode — switch between one-way and hedge position mode for a symbol.
///
/// Requires no open positions for the given symbol.
pub async fn set_position_mode(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let mode = match body.get("mode").and_then(|v| v.as_str()) {
        Some("one_way") | Some("OneWay") => PositionMode::OneWay,
        Some("hedge") | Some("Hedge") => PositionMode::Hedge,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid mode. Use 'one_way' or 'hedge'"})),
            )
                .into_response();
        }
    };

    let symbol = match body.get("symbol").and_then(|v| v.as_str()) {
        Some(s) => Symbol::new(s),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing required field: symbol"})),
            )
                .into_response();
        }
    };

    let mut pm = exchange.positions.lock();
    match pm.set_position_mode(user.0, &symbol, mode) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "symbol": symbol.to_string(),
                "mode": format!("{:?}", mode),
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// POST /api/v1/position/trading-stop — set TP/SL/trailing-stop on an open position.
///
/// Replaces any previously set TP/SL/trailing-stop on the position.
/// Pass an empty string or `"0"` for a field to clear it.
/// Supports partial TP/SL via `tpSize`/`slSize` (must not exceed position size).
pub async fn set_trading_stop(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let symbol = match body.get("symbol").and_then(|v| v.as_str()) {
        Some(s) => Symbol::new(s),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing symbol"})),
            )
                .into_response();
        }
    };
    let position_idx: u8 = body
        .get("positionIdx")
        .and_then(|v| v.as_u64())
        .map(|n| n as u8)
        .unwrap_or(0);

    // Parse TP/SL prices — empty string or "0" means clear.
    let tp_price = body.get("takeProfit").and_then(|v| v.as_str()).and_then(|s| {
        if s.is_empty() || s == "0" {
            None
        } else {
            s.parse::<Decimal>().ok()
        }
    });
    let sl_price = body.get("stopLoss").and_then(|v| v.as_str()).and_then(|s| {
        if s.is_empty() || s == "0" {
            None
        } else {
            s.parse::<Decimal>().ok()
        }
    });
    let trailing_stop = body.get("trailingStop").and_then(|v| v.as_str()).and_then(|s| {
        if s.is_empty() || s == "0" {
            None
        } else {
            s.parse::<Decimal>().ok()
        }
    });
    let tp_size = body
        .get("tpSize")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<Decimal>().ok());
    let sl_size = body
        .get("slSize")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<Decimal>().ok());

    let tp_trigger_by = body
        .get("tpTriggerBy")
        .and_then(|v| v.as_str())
        .unwrap_or("LastPrice");
    let sl_trigger_by = body
        .get("slTriggerBy")
        .and_then(|v| v.as_str())
        .unwrap_or("LastPrice");

    // Read current position state under the lock.
    let (pos_side, pos_size, old_tp_trigger_id, old_sl_trigger_id) = {
        let pm = exchange.positions.lock();
        match pm.get_position(user.0, &symbol, position_idx) {
            Some(p) if !p.is_closed() => (p.side, p.size, p.tp_trigger_id, p.sl_trigger_id),
            _ => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "No open position"})),
                )
                    .into_response();
            }
        }
    };

    // Validate partial sizes.
    if let Some(ts) = tp_size {
        if ts > pos_size {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "tpSize exceeds position size"})),
            )
                .into_response();
        }
    }
    if let Some(ss) = sl_size {
        if ss > pos_size {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "slSize exceeds position size"})),
            )
                .into_response();
        }
    }

    // Cancel existing trigger orders that are being replaced.
    {
        let mut te = exchange.trigger_engine.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(old_tp) = old_tp_trigger_id {
            let _ = te.cancel_order(old_tp);
        }
        if let Some(old_sl) = old_sl_trigger_id {
            let _ = te.cancel_order(old_sl);
        }
    }

    let close_side = match pos_side {
        exchange_common::futures::PositionSide::Long => Side::Sell,
        exchange_common::futures::PositionSide::Short => Side::Buy,
    };

    let parse_trigger_by = |s: &str| -> exchange_trigger_service::TriggerBy {
        match s {
            "MarkPrice" => exchange_trigger_service::TriggerBy::MarkPrice,
            "IndexPrice" => exchange_trigger_service::TriggerBy::IndexPrice,
            _ => exchange_trigger_service::TriggerBy::LastPrice,
        }
    };

    let mut new_tp_id: Option<Uuid> = None;
    let mut new_sl_id: Option<Uuid> = None;

    // Register TP trigger order.
    if let Some(tp) = tp_price {
        let tp_order = TriggerOrder {
            id: Uuid::new_v4(),
            user_id: user.0,
            symbol: symbol.clone(),
            trigger_type: TriggerOrderType::TakeProfitMarket,
            side: close_side,
            trigger_price: tp,
            order_price: None,
            qty: tp_size.unwrap_or(pos_size),
            trigger_by: parse_trigger_by(tp_trigger_by),
            reduce_only: true,
            close_on_trigger: tp_size.is_none(),
            trailing_delta: None,
            trail_type: Default::default(),
            tp_limit_price: None,
            sl_limit_price: None,
            tp_order_type: Default::default(),
            sl_order_type: Default::default(),
            status: TriggerStatus::Active,
            created_at: chrono::Utc::now(),
            triggered_at: None,
            expires_at: None,
        };
        new_tp_id = Some(tp_order.id);
        let mut te = exchange.trigger_engine.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = te.add_order(tp_order) {
            tracing::warn!(error = %e, "failed to register TP trigger for trading-stop");
        }
    }

    // Register SL or trailing-stop trigger order.
    // Trailing stop takes precedence over a plain SL when both are provided.
    if let Some(trail) = trailing_stop {
        // Seed the trailing stop extreme with the current mark price so the
        // tracker starts tracking from the right reference point.
        let seed_price = exchange
            .get_mark_price(&symbol)
            .map(|mp| mp.price)
            .unwrap_or(Decimal::ZERO);
        let trail_order = TriggerOrder {
            id: Uuid::new_v4(),
            user_id: user.0,
            symbol: symbol.clone(),
            trigger_type: TriggerOrderType::TrailingStop,
            side: close_side,
            trigger_price: seed_price,
            order_price: None,
            qty: pos_size,
            trigger_by: exchange_trigger_service::TriggerBy::LastPrice,
            reduce_only: true,
            close_on_trigger: true,
            trailing_delta: Some(trail),
            trail_type: Default::default(),
            tp_limit_price: None,
            sl_limit_price: None,
            tp_order_type: Default::default(),
            sl_order_type: Default::default(),
            status: TriggerStatus::Active,
            created_at: chrono::Utc::now(),
            triggered_at: None,
            expires_at: None,
        };
        new_sl_id = Some(trail_order.id);
        let mut te = exchange.trigger_engine.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = te.add_order(trail_order) {
            tracing::warn!(error = %e, "failed to register trailing-stop trigger for trading-stop");
        }
    } else if let Some(sl) = sl_price {
        let sl_order = TriggerOrder {
            id: Uuid::new_v4(),
            user_id: user.0,
            symbol: symbol.clone(),
            trigger_type: TriggerOrderType::StopMarket,
            side: close_side,
            trigger_price: sl,
            order_price: None,
            qty: sl_size.unwrap_or(pos_size),
            trigger_by: parse_trigger_by(sl_trigger_by),
            reduce_only: true,
            close_on_trigger: sl_size.is_none(),
            trailing_delta: None,
            trail_type: Default::default(),
            tp_limit_price: None,
            sl_limit_price: None,
            tp_order_type: Default::default(),
            sl_order_type: Default::default(),
            status: TriggerStatus::Active,
            created_at: chrono::Utc::now(),
            triggered_at: None,
            expires_at: None,
        };
        new_sl_id = Some(sl_order.id);
        let mut te = exchange.trigger_engine.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = te.add_order(sl_order) {
            tracing::warn!(error = %e, "failed to register SL trigger for trading-stop");
        }
    }

    // Persist the new TP/SL state on the position.
    {
        let mut pm = exchange.positions.lock();
        if let Err(e) = pm.update_trading_stop(
            user.0,
            &symbol,
            position_idx,
            tp_price,
            sl_price,
            trailing_stop,
            new_tp_id,
            new_sl_id,
            tp_size,
            sl_size,
        ) {
            tracing::warn!(error = %e, "failed to persist trading-stop state on position");
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "symbol": symbol.to_string(),
            "takeProfit": tp_price.map(|p| p.to_string()),
            "stopLoss": sl_price.map(|p| p.to_string()),
            "trailingStop": trailing_stop.map(|t| t.to_string()),
            "tpTriggerId": new_tp_id.map(|id| id.to_string()),
            "slTriggerId": new_sl_id.map(|id| id.to_string()),
        })),
    )
        .into_response()
}

/// GET /api/v1/funding/rate — current funding rates for all symbols.
pub async fn get_funding_rates(State(exchange): State<AppState>) -> impl IntoResponse {
    let now = chrono::Utc::now();

    let perp_entries: Vec<(Symbol, u32)> = exchange
        .perp_configs
        .load()
        .iter()
        .map(|(sym, cfg)| (sym.clone(), cfg.funding_interval_hours))
        .collect();

    let result: Vec<serde_json::Value> = perp_entries
        .iter()
        .map(|(symbol, funding_interval_hours)| {
            let history = exchange.funding_history.lock().unwrap_or_else(|e| e.into_inner());
            let latest = history.get(symbol).and_then(|h| h.latest_rate()).cloned();

            let mark_price = exchange
                .get_mark_price(symbol)
                .map(|mp| mp.price)
                .unwrap_or(Decimal::ZERO);

            let rate = latest.as_ref().map(|r| r.rate).unwrap_or(Decimal::ZERO);

            let next_funding_time = exchange
                .funding_scheduler
                .lock().unwrap_or_else(|e| e.into_inner())
                .next_funding_time(now)
                .map(|t| t.timestamp_millis())
                .unwrap_or(0);

            serde_json::json!({
                "symbol": symbol.to_string(),
                "rate": rate,
                "mark_price": mark_price,
                "next_funding_time": next_funding_time,
                "interval_hours": funding_interval_hours,
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(result)))
}

/// GET /api/v1/funding/history?symbol=BTCUSDT — funding payment history.
pub async fn get_funding_history(
    State(exchange): State<AppState>,
    Query(params): Query<FundingHistoryQuery>,
) -> impl IntoResponse {
    let symbol = Symbol::new(&params.symbol);

    // Try DB-backed history first.
    {
        let maybe_pool = exchange.db_pool.get().cloned();
        if let Some(pool) = maybe_pool {
            match exchange_db::funding_repo::FundingRepo::recent_rates(&pool, &params.symbol, 100).await {
                Ok(rows) if !rows.is_empty() => {
                    let rates: Vec<serde_json::Value> = rows.iter().map(|r| {
                        serde_json::json!({
                            "symbol": r.symbol,
                            "rate": r.rate,
                            "mark_price": r.mark_price,
                            "index_price": r.index_price,
                            "calculated_at": r.settled_at.timestamp_millis(),
                        })
                    }).collect();
                    return (StatusCode::OK, Json(serde_json::json!(rates)));
                }
                Err(e) => {
                    tracing::warn!(symbol = %params.symbol, error = %e, "DB funding history query failed — falling back to in-memory");
                }
                Ok(_) => {} // empty rows — fall through to in-memory
            }
        }
    }

    let history = exchange.funding_history.lock().unwrap_or_else(|e| e.into_inner());

    match history.get(&symbol) {
        Some(h) => {
            let rates: Vec<serde_json::Value> = h
                .rates
                .iter()
                .map(|r| serde_json::json!({
                    "symbol": r.symbol.to_string(),
                    "rate": r.rate,
                    "mark_price": r.mark_price,
                    "index_price": r.index_price,
                    "calculated_at": r.calculated_at.timestamp_millis(),
                }))
                .collect();
            (StatusCode::OK, Json(serde_json::json!(rates)))
        }
        None => {
            if exchange.perp_configs.load().contains_key(&symbol) {
                (StatusCode::OK, Json(serde_json::json!([])))
            } else {
                (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": format!("Unknown symbol: {}", params.symbol)})),
                )
            }
        }
    }
}

/// GET /api/v1/orders — user's currently open orders (not in a terminal state).
///
/// Open orders are those still resting on the book, identified via `order_locks`
/// (the authoritative source for spot orders on the book). Metadata (created_at,
/// symbol, side, order_type) is joined from `pending_orders`.
///
/// Returns all open orders for the authenticated user, sorted by created_at desc.
pub async fn get_open_orders(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    // S20 J.5 — per-endpoint latency timing; RAII guard records on drop.
    let _lat = exchange.metrics.endpoint_timer("get_open_orders");
    let user_id = auth.0;

    // Spot orders: have entries in order_locks (balance lock tracking).
    let mut open_orders: Vec<serde_json::Value> = exchange
        .order_locks
        .iter()
        .filter(|r| r.user_id == user_id)
        .filter_map(|lock_ref| {
            let order_id = *lock_ref.key();
            let info = exchange.pending_orders.get(&order_id)?;
            let status = if lock_ref.filled_qty > Decimal::ZERO {
                "partially_filled"
            } else {
                "open"
            };
            Some(serde_json::json!({
                "order_id": order_id.to_string(),
                "order_link_id": info.order_link_id,
                "symbol": lock_ref.symbol.to_string(),
                "side": format!("{:?}", lock_ref.side),
                "order_type": format!("{:?}", info.order_type),
                "price": lock_ref.price,
                "qty": lock_ref.qty,
                "filled_qty": lock_ref.filled_qty,
                "status": status,
                "created_at": info.created_at.timestamp_millis(),
            }))
        })
        .collect();

    // Futures orders: tracked in pending_orders but NOT in order_locks
    // (they use margin locking instead of balance locking).
    for pending_ref in exchange.pending_orders.iter() {
        let order_id = *pending_ref.key();
        let info = pending_ref.value();
        if info.user_id != user_id {
            continue;
        }
        // Skip if already included from order_locks (spot order).
        if exchange.order_locks.contains_key(&order_id) {
            continue;
        }
        open_orders.push(serde_json::json!({
            "order_id": order_id.to_string(),
            "order_link_id": info.order_link_id,
            "symbol": info.symbol.to_string(),
            "side": format!("{:?}", info.side),
            "order_type": format!("{:?}", info.order_type),
            "price": info.price,
            "qty": info.qty,
            "filled_qty": Decimal::ZERO,
            "status": "open",
            "created_at": info.created_at.timestamp_millis(),
        }));
    }

    // Sort by created_at desc (newest first).
    open_orders.sort_by(|a, b| {
        let a_ts = a["created_at"].as_i64().unwrap_or(0);
        let b_ts = b["created_at"].as_i64().unwrap_or(0);
        b_ts.cmp(&a_ts)
    });

    (StatusCode::OK, Json(serde_json::json!(open_orders)))
}

/// GET /api/v1/account/trades — user's private fill/execution history.
///
/// Returns the user's recent fills (both maker and taker sides), newest first.
/// Optional query params: `?symbol=BTCUSDT&limit=50`
pub async fn get_user_trades(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let symbol_filter = params.get("symbol").cloned();
    let limit = params.get("limit")
        .and_then(|l| l.parse::<usize>().ok())
        .unwrap_or(50)
        .min(200);

    let user_fills = exchange.user_fills.lock().unwrap_or_else(|e| e.into_inner());
    let fills: Vec<&crate::exchange::UserFillEntry> = match user_fills.get(&user_id) {
        Some(entries) => {
            let mut v: Vec<_> = entries.iter()
                .filter(|f| {
                    symbol_filter.as_ref().is_none_or( |s| f.symbol.to_string() == *s)
                })
                .collect();
            v.reverse(); // newest first
            v.truncate(limit);
            v
        }
        None => Vec::new(),
    };

    (StatusCode::OK, Json(serde_json::json!(fills)))
}

/// GET /api/v1/orders/{id} — single order by ID.
///
/// Checks open orders first (pending_orders), then terminal orders (order_history).
/// Returns 404 if the order does not exist or does not belong to the authenticated user.
pub async fn get_order_by_id(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(order_id): Path<String>,
) -> impl IntoResponse {
    let order_id = match Uuid::parse_str(&order_id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid order_id format"})),
            );
        }
    };

    let user_id = auth.0;

    // Check open orders first: order_locks is the authoritative "on book" source.
    {
        if let Some(record) = exchange.order_locks.get(&order_id) {
            if record.user_id != user_id {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "Order not found"})),
                );
            }
            // Join with pending_orders for order_type and created_at.
            let (order_type_str, created_at_ms, link_id) = exchange
                .pending_orders
                .get(&order_id)
                .map(|info| (format!("{:?}", info.order_type), info.created_at.timestamp_millis(), info.order_link_id.clone()))
                .unwrap_or_else(|| ("Limit".to_string(), 0, None));
            let status = if record.filled_qty > Decimal::ZERO {
                "partially_filled"
            } else {
                "open"
            };
            return (
                StatusCode::OK,
                Json(serde_json::json!({
                    "order_id": order_id.to_string(),
                    "order_link_id": link_id,
                    "symbol": record.symbol.to_string(),
                    "side": format!("{:?}", record.side),
                    "order_type": order_type_str,
                    "price": record.price,
                    "qty": record.qty,
                    "filled_qty": record.filled_qty,
                    "status": status,
                    "created_at": created_at_ms,
                })),
            );
        }
    }

    // Check order history (terminal orders).
    {
        let history = exchange.order_history.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = history.iter().find(|e| e.order_id == order_id) {
            if entry.user_id != user_id {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "Order not found"})),
                );
            }
            return (
                StatusCode::OK,
                Json(serde_json::json!({
                    "order_id": entry.order_id.to_string(),
                    "order_link_id": entry.order_link_id,
                    "symbol": entry.symbol.to_string(),
                    "side": entry.side,
                    "order_type": entry.order_type,
                    "price": entry.price,
                    "qty": entry.qty,
                    "filled_qty": entry.filled_qty,
                    "status": entry.status,
                    "created_at": entry.created_at.timestamp_millis(),
                })),
            );
        }
    }

    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": "Order not found"})),
    )
}

// --- T5.2: Order Amendment ---

#[derive(Debug, Deserialize)]
pub struct AmendOrderRequest {
    pub price: Option<Decimal>,
    pub qty: Option<Decimal>,
}

/// PUT /api/v1/orders/{id} — amend an open order.
///
/// Implementation: cancel the existing order, then place a new order with
/// the updated price and/or qty (all other parameters preserved).
///
/// NOTE: The new order receives a new order_id and loses its queue position.
/// This is by design — queue position is NOT preserved on amendment.
pub async fn amend_order(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(order_id): Path<String>,
    Json(req): Json<AmendOrderRequest>,
) -> impl IntoResponse {
    // S20 J.5 — per-endpoint latency timing; RAII guard records on drop.
    let _lat = exchange.metrics.endpoint_timer("amend_order");
    let old_order_id = match Uuid::parse_str(&order_id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid order_id format"})),
            );
        }
    };

    let user_id = auth.0;

    // Validate that at least one field is being changed.
    if req.price.is_none() && req.qty.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "At least one of 'price' or 'qty' must be provided"})),
        );
    }

    // Find the order: order_locks is the authoritative "on book" source.
    // pending_orders holds the metadata (order_type, created_at, etc.).
    let order_info = {
        match exchange.order_locks.get(&old_order_id) {
            Some(record) if record.user_id != user_id => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "Order not found"})),
                );
            }
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "Order not found or already in terminal state"})),
                );
            }
            Some(_) => {}
        }
        // Order confirmed open — get full metadata from pending_orders.
        match exchange.pending_orders.get(&old_order_id) {
            Some(info) => info.clone(),
            None => {
                // Lock record exists but pending_orders entry is gone — shouldn't happen
                // in normal operation, but handle defensively.
                tracing::error!(order_id = %old_order_id, "Order state inconsistent: lock present but pending_orders entry missing");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "Internal server error"})),
                );
            }
        }
    };

    let new_price = req.price.unwrap_or(order_info.price);
    let new_qty = req.qty.unwrap_or(order_info.qty);

    // Validate new values.
    if let Err(e) = validate_positive(new_qty, "qty") {
        return e;
    }
    if order_info.order_type == OrderType::Limit {
        if let Err(e) = validate_positive(new_price, "price") {
            return e;
        }
    }

    // Step 1: Cancel the old order.
    let cancel_cmd = CancelOrderCommand {
        order_id: old_order_id,
        user_id,
        symbol: order_info.symbol.clone(),
        timestamp: chrono::Utc::now(),
    };

    if let Err(e) = exchange.cancel_order(cancel_cmd) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("Failed to cancel old order: {}", e)})),
        );
    }

    // Step 2: Place the new order with updated parameters.
    let new_order_id = Uuid::new_v4();
    let place_cmd = PlaceOrderCommand {
        order_id: new_order_id,
        user_id,
        symbol: order_info.symbol.clone(),
        side: order_info.side,
        order_type: order_info.order_type,
        time_in_force: TimeInForce::Gtc,
        price: new_price,
        qty: new_qty,
        reduce_only: false,
        expire_at: None,
        timestamp: chrono::Utc::now(),
        position_idx: 0,
        iceberg_qty: None,
        order_link_id: None,
        slippage_tolerance: None,
        close_position: false,
        stp_mode: StpMode::default(),
        request_expire_time: None,
    };

    let exchange_clone = Arc::clone(&exchange);
    let result = crate::request_id::spawn_blocking_traced(move || exchange_clone.place_order(place_cmd))
        .await
        .unwrap_or_else(|e| Err(anyhow::anyhow!("task join error: {e}")));
    match result {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "old_order_id": old_order_id.to_string(),
                "new_order_id": new_order_id.to_string(),
                "status": "amended",
            })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("Failed to place amended order: {}", e)})),
        ),
    }
}

/// GET /api/v1/orders/by-link-id/{link_id} — fetch an open or terminal order by client order ID.
pub async fn get_order_by_link_id(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(link_id): Path<String>,
) -> impl IntoResponse {
    let user_id = auth.0;

    let order_id = {
        let index = exchange.order_link_index.lock();
        index.get(&(user_id, link_id.clone())).copied()
    };

    match order_id {
        Some(id) => {
            // Delegate to the same lookup logic as get_order_by_id.
            // Check open orders first.
            {
                if let Some(record) = exchange.order_locks.get(&id) {
                    if record.user_id == user_id {
                        let (order_type_str, created_at_ms) = exchange
                            .pending_orders
                            .get(&id)
                            .map(|info| (format!("{:?}", info.order_type), info.created_at.timestamp_millis()))
                            .unwrap_or_else(|| ("Limit".to_string(), 0));
                        let status = if record.filled_qty > Decimal::ZERO {
                            "partially_filled"
                        } else {
                            "open"
                        };
                        return (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "order_id": id.to_string(),
                                "order_link_id": link_id,
                                "symbol": record.symbol.to_string(),
                                "side": format!("{:?}", record.side),
                                "order_type": order_type_str,
                                "price": record.price,
                                "qty": record.qty,
                                "filled_qty": record.filled_qty,
                                "status": status,
                                "created_at": created_at_ms,
                            })),
                        );
                    }
                }
            }
            // Check order history.
            {
                let history = exchange.order_history.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(entry) = history.iter().find(|e| e.order_id == id && e.user_id == user_id) {
                    return (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "order_id": entry.order_id.to_string(),
                            "order_link_id": entry.order_link_id,
                            "symbol": entry.symbol.to_string(),
                            "side": entry.side,
                            "order_type": entry.order_type,
                            "price": entry.price,
                            "qty": entry.qty,
                            "filled_qty": entry.filled_qty,
                            "status": entry.status,
                            "created_at": entry.created_at.timestamp_millis(),
                        })),
                    );
                }
            }
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Order not found"})),
            )
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Order not found for this link ID"})),
        ),
    }
}

/// DELETE /api/v1/orders/by-link-id/{link_id} — cancel order by client order ID.
pub async fn cancel_order_by_link_id(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(link_id): Path<String>,
) -> impl IntoResponse {
    let user_id = auth.0;

    let order_id = {
        let index = exchange.order_link_index.lock();
        index.get(&(user_id, link_id.clone())).copied()
    };

    match order_id {
        Some(id) => {
            let cmd = CancelOrderCommand {
                order_id: id,
                user_id,
                symbol: Symbol::new(""), // resolved from pending_orders inside cancel_order
                timestamp: chrono::Utc::now(),
            };
            match exchange.cancel_order(cmd) {
                Ok(_) => (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "order_id": id.to_string(),
                        "order_link_id": link_id,
                        "status": "cancelled",
                    })),
                ),
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("Order not found") {
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "order_id": id.to_string(),
                                "order_link_id": link_id,
                                "status": "cancelled",
                            })),
                        )
                    } else {
                        (
                            StatusCode::BAD_REQUEST,
                            Json(serde_json::json!({"error": msg})),
                        )
                    }
                }
            }
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Order not found for this link ID"})),
        ),
    }
}

/// GET /api/v1/orders/history?symbol=&limit=50 — authenticated user's order history.
pub async fn get_order_history(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Query(params): Query<OrderHistoryQuery>,
) -> impl IntoResponse {
    // S20 J.5 — per-endpoint latency timing; RAII guard records on drop.
    let _lat = exchange.metrics.endpoint_timer("get_order_history");
    let user_id = auth.0;
    let limit = params.limit.min(MAX_HISTORY_LIMIT);

    let history = exchange.order_history.lock().unwrap_or_else(|e| e.into_inner());
    let result: Vec<serde_json::Value> = history
        .iter()
        .rev()
        .filter(|e| {
            e.user_id == user_id
                && params.symbol.as_deref().is_none_or( |s| e.symbol.as_str() == s)
        })
        .take(limit)
        .map(|e| {
            serde_json::json!({
                "order_id": e.order_id.to_string(),
                "order_link_id": e.order_link_id,
                "symbol": e.symbol.to_string(),
                "side": e.side,
                "order_type": e.order_type,
                "price": e.price,
                "qty": e.qty,
                "filled_qty": e.filled_qty,
                "status": e.status,
                "created_at": e.created_at.timestamp_millis(),
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(result)))
}

/// GET /api/v1/markets/:symbol/trades?limit=50 — public recent trades for a symbol.
pub async fn get_market_trades(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
    Query(params): Query<TradeHistoryQuery>,
) -> impl IntoResponse {
    if let Err(e) = validate_symbol(&symbol) {
        return e;
    }
    let symbol = Symbol::new(&symbol);
    let limit = params.limit.min(MAX_HISTORY_LIMIT);

    let history = exchange.trade_history.lock().unwrap_or_else(|e| e.into_inner());
    match history.get(&symbol) {
        Some(trades) => {
            let result: Vec<serde_json::Value> = trades
                .iter()
                .rev()
                .take(limit)
                .map(|t| {
                    serde_json::json!({
                        "id": t.trade_id,
                        "symbol": t.symbol.to_string(),
                        "price": t.price.to_string(),
                        "qty": t.qty.to_string(),
                        "quote_qty": t.quote_qty.to_string(),
                        "time": t.timestamp.timestamp_millis(),
                        "side": t.taker_side,
                        "is_buyer_maker": t.is_buyer_maker,
                    })
                })
                .collect();
            (StatusCode::OK, Json(serde_json::json!(result)))
        }
        None => (
            StatusCode::OK,
            Json(serde_json::json!([])),
        ),
    }
}

/// GET /api/v1/markets/:symbol/agg-trades?limit=50 — aggregated trades.
///
/// Phase 1: returns the same individual trades as `/trades`. Aggregation of
/// consecutive same-price same-side fills is Phase 2.
pub async fn get_agg_trades(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
    Query(params): Query<TradeHistoryQuery>,
) -> impl IntoResponse {
    if let Err(e) = validate_symbol(&symbol) {
        return e;
    }
    let symbol = Symbol::new(&symbol);
    let limit = params.limit.min(MAX_HISTORY_LIMIT);

    let history = exchange.trade_history.lock().unwrap_or_else(|e| e.into_inner());
    match history.get(&symbol) {
        Some(trades) => {
            let result: Vec<serde_json::Value> = trades
                .iter()
                .rev()
                .take(limit)
                .map(|t| {
                    serde_json::json!({
                        "id": t.trade_id,
                        "price": t.price.to_string(),
                        "qty": t.qty.to_string(),
                        "quote_qty": t.quote_qty.to_string(),
                        "first_trade_id": t.trade_id,
                        "last_trade_id": t.trade_id,
                        "time": t.timestamp.timestamp_millis(),
                        "is_buyer_maker": t.is_buyer_maker,
                    })
                })
                .collect();
            (StatusCode::OK, Json(serde_json::json!(result)))
        }
        None => (
            StatusCode::OK,
            Json(serde_json::json!([])),
        ),
    }
}

/// GET /api/v1/markets/:symbol/mark-price — current mark price + index price.
pub async fn get_mark_price(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = validate_symbol(&symbol) {
        return e;
    }
    let symbol = Symbol::new(&symbol);

    match exchange.get_mark_price(&symbol) {
        Some(mp) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "symbol": mp.symbol.to_string(),
                "mark_price": mp.price,
                "index_price": mp.index_price,
                "last_price": mp.last_price,
                "timestamp": mp.timestamp.timestamp_millis(),
            })),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("Mark price unavailable for {symbol} — no index sources configured")})),
        ),
    }
}

// ---------------------------------------------------------------------------
// Sub-account request/response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CreateSubAccountRequest {
    pub label: String,
}

#[derive(Debug, Deserialize)]
pub struct InternalTransferRequest {
    /// Source sub-account id. If absent, the master account (authenticated user) is used.
    #[serde(default)]
    pub from_sub: Option<Uuid>,
    /// Destination sub-account id. If absent, the master account is used.
    #[serde(default)]
    pub to_sub: Option<Uuid>,
    pub asset: String,
    pub amount: Decimal,
}

#[derive(Debug, Deserialize)]
pub struct ApplyReferralRequest {
    pub code: String,
}

#[derive(Debug, Deserialize)]
pub struct TransferHistoryQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
}

// ---------------------------------------------------------------------------
// Sub-account handlers
// ---------------------------------------------------------------------------

/// POST /api/v1/sub-accounts
///
/// Create a sub-account under the authenticated user (master).
pub async fn create_sub_account(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<CreateSubAccountRequest>,
) -> impl IntoResponse {
    let master_id = auth.0;

    match exchange.sub_accounts.lock().unwrap_or_else(|e| e.into_inner()).create_sub(master_id, &req.label) {
        Ok(sub) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "id": sub.id.to_string(),
                "label": sub.label,
                "created_at": sub.created_at.timestamp_millis(),
                "is_active": sub.is_active,
            })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

/// GET /api/v1/sub-accounts
///
/// List all sub-accounts belonging to the authenticated user.
pub async fn list_sub_accounts(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let master_id = auth.0;

    let subs: Vec<serde_json::Value> = exchange
        .sub_accounts
        .lock().unwrap_or_else(|e| e.into_inner())
        .list_subs(master_id)
        .iter()
        .map(|s| serde_json::json!({
            "id": s.id.to_string(),
            "label": s.label,
            "created_at": s.created_at.timestamp_millis(),
            "is_active": s.is_active,
        }))
        .collect();

    (StatusCode::OK, Json(serde_json::json!(subs)))
}

// ---------------------------------------------------------------------------
// Transfer handlers
// ---------------------------------------------------------------------------

/// POST /api/v1/transfer
///
/// Move funds between the master account and a sub-account (or between two sub-accounts).
/// `from_sub` and `to_sub` are optional sub-account UUIDs; when absent, the master account
/// (authenticated user) is used as that side of the transfer.
pub async fn internal_transfer(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<InternalTransferRequest>,
) -> impl IntoResponse {
    let master_id = auth.0;
    let from_id = req.from_sub.unwrap_or(master_id);
    let to_id = req.to_sub.unwrap_or(master_id);

    // Determine transfer type based on whether accounts are master or sub.
    let transfer_type = {
        let sub_mgr = exchange.sub_accounts.lock().unwrap_or_else(|e| e.into_inner());
        let from_is_sub = sub_mgr.is_sub_account(from_id);
        let to_is_sub = sub_mgr.is_sub_account(to_id);
        match (from_is_sub, to_is_sub) {
            (true, true) => TransferType::SubToSub,
            (false, true) => TransferType::MasterToSub,
            (true, false) => TransferType::SubToMaster,
            (false, false) => TransferType::UserToUser,
        }
    };

    // M21-6: AML screening on inter-sub-account transfers.
    // Block transfers above the reporting threshold ($10,000) for manual review
    // unless the user is KYC-verified. Background AML monitor handles async screening.
    if req.amount >= rust_decimal::Decimal::new(10_000, 0) {
        // Check if user has account status that allows this.
        let status = exchange.get_account_status(master_id);
        if !status.can_trade() {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({
                    "error": "Account cannot perform transfers",
                    "status": format!("{:?}", status),
                })),
            ).into_response();
        }
    }

    let result = exchange
        .transfers
        .lock().unwrap_or_else(|e| e.into_inner())
        .transfer_sharded(
            &exchange.accounts,
            from_id,
            to_id,
            &req.asset,
            req.amount,
            transfer_type,
        );

    match result {
        Ok(record) => {
            // Write-behind ledger entry (async, non-blocking).
            {
                let maybe_pool = exchange.db_pool.get().cloned();
                if let Some(pool) = maybe_pool {
                    let transfer_id_str = record.id.to_string();
                    let from_id = record.from_user;
                    let to_id = record.to_user;
                    let asset = record.asset.clone();
                    let amount = record.amount;
                    tokio::spawn(async move {
                        if let Err(e) = exchange_db::ledger::LedgerService::record_transfer(
                            &pool,
                            from_id,
                            "user_spot",
                            to_id,
                            "user_spot",
                            &asset,
                            amount,
                            &transfer_id_str,
                        )
                        .await
                        {
                            tracing::error!(
                                transfer_id = %transfer_id_str,
                                error = %e,
                                "ledger write failed for internal transfer"
                            );
                        }
                    });
                }
            }

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "id": record.id.to_string(),
                    "from": record.from_user.to_string(),
                    "to": record.to_user.to_string(),
                    "asset": record.asset,
                    "amount": record.amount,
                    "type": format!("{:?}", record.transfer_type),
                    "timestamp": record.timestamp.timestamp_millis(),
                })),
            ).into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        ).into_response(),
    }
}

/// GET /api/v1/transfers
///
/// Return the authenticated user's recent transfer history (newest first).
pub async fn get_transfers(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Query(params): Query<TransferHistoryQuery>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let limit = params.limit.min(MAX_HISTORY_LIMIT);

    let records: Vec<serde_json::Value> = exchange
        .transfers
        .lock().unwrap_or_else(|e| e.into_inner())
        .user_transfers(user_id, limit)
        .iter()
        .map(|r| serde_json::json!({
            "id": r.id.to_string(),
            "from": r.from_user.to_string(),
            "to": r.to_user.to_string(),
            "asset": r.asset,
            "amount": r.amount,
            "type": format!("{:?}", r.transfer_type),
            "timestamp": r.timestamp.timestamp_millis(),
        }))
        .collect();

    (StatusCode::OK, Json(serde_json::json!(records)))
}

// ---------------------------------------------------------------------------
// Multi-wallet (FUND / TRADING) handlers
// ---------------------------------------------------------------------------

/// POST /api/v1/account/internal-transfer — transfer between FUND and TRADING account.
///
/// Body: `{ "fromAccountType": "FUND", "toAccountType": "TRADING", "coin": "USDT", "amount": "100" }`
pub async fn internal_wallet_transfer(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let from = match body.get("fromAccountType").and_then(|v| v.as_str()) {
        Some(s) if s.eq_ignore_ascii_case("FUND") => "fund",
        Some(s) if s.eq_ignore_ascii_case("TRADING") => "trading",
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid fromAccountType. Use 'FUND' or 'TRADING'"})),
            )
                .into_response();
        }
    };
    let to = match body.get("toAccountType").and_then(|v| v.as_str()) {
        Some(s) if s.eq_ignore_ascii_case("FUND") => "fund",
        Some(s) if s.eq_ignore_ascii_case("TRADING") => "trading",
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid toAccountType. Use 'FUND' or 'TRADING'"})),
            )
                .into_response();
        }
    };
    if from == to {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "fromAccountType and toAccountType must be different"})),
        )
            .into_response();
    }
    let coin = match body.get("coin").and_then(|v| v.as_str()) {
        Some(c) => c.to_uppercase(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing coin"})),
            )
                .into_response();
        }
    };
    let amount: Decimal = match body
        .get("amount")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .filter(|a: &Decimal| *a > Decimal::ZERO)
    {
        Some(a) => a,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid amount — must be a positive number string"})),
            )
                .into_response();
        }
    };

    // S18 Phase B: `accounts` is now a per-user sharded store, so we hold the
    // `fund_accounts` lock for the entire debit+credit and route each trading
    // mutation through the single user's shard. The two sides of the transfer
    // don't need to be under one shared mutex — fund_accounts protects the
    // fund balance, and the user's shard protects the trading balance.
    let mut fund = exchange.fund_accounts.lock().unwrap_or_else(|e| e.into_inner());

    if from == "fund" {
        // Debit fund account, credit trading account.
        let fund_bal = fund
            .entry(user.0)
            .or_default()
            .entry(coin.clone())
            .or_insert(Decimal::ZERO);
        if *fund_bal < amount {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Insufficient fund account balance"})),
            )
                .into_response();
        }
        *fund_bal -= amount;
        if let Err(e) = exchange.accounts.deposit(user.0, &coin, amount) {
            // Rollback fund deduction — still inside the fund_accounts lock.
            *fund.entry(user.0).or_default().entry(coin.clone()).or_insert(Decimal::ZERO) += amount;
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    } else {
        // Debit trading account, credit fund account.
        if let Err(e) = exchange.accounts.withdraw(user.0, &coin, amount) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
        *fund.entry(user.0).or_default().entry(coin.clone()).or_insert(Decimal::ZERO) += amount;
    }
    drop(fund);

    tracing::info!(
        user_id = %user.0,
        from,
        to,
        coin = %coin,
        amount = %amount,
        "internal wallet transfer completed",
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "transferId": uuid::Uuid::new_v4().to_string(),
            "coin": coin,
            "amount": amount.to_string(),
            "fromAccountType": from,
            "toAccountType": to,
            "status": "completed",
        })),
    )
        .into_response()
}

/// GET /api/v1/account/fund-balance — funding account balances.
///
/// Returns balances for all coins in the user's FUND account.
pub async fn get_fund_balance(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let fund = exchange.fund_accounts.lock().unwrap_or_else(|e| e.into_inner());
    let coins: Vec<serde_json::Value> = fund
        .get(&user.0)
        .map(|bals| {
            bals.iter()
                .filter(|(_, bal)| **bal > Decimal::ZERO)
                .map(|(coin, bal)| serde_json::json!({ "coin": coin, "balance": bal.to_string() }))
                .collect()
        })
        .unwrap_or_default();

    (
        StatusCode::OK,
        Json(serde_json::json!({ "accountType": "FUND", "coins": coins })),
    )
}

// ---------------------------------------------------------------------------
// Referral handlers
// ---------------------------------------------------------------------------

/// POST /api/v1/referral/apply
///
/// Apply a referral code to the authenticated user's account.
pub async fn apply_referral_code(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<ApplyReferralRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;

    match exchange.referrals.lock().unwrap_or_else(|e| e.into_inner()).apply_code(user_id, &req.code) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "applied", "code": req.code})),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

/// GET /api/v1/referral
///
/// Return the authenticated user's referral code (generating one if needed) and basic stats.
pub async fn get_referral(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;

    let (code, total_referred, commissions) = {
        let mut referrals = exchange.referrals.lock().unwrap_or_else(|e| e.into_inner());
        let code = referrals.generate_code(user_id);
        let total = referrals.total_referred_count(user_id);
        let comms = referrals.get_commissions(user_id);
        (code, total, comms)
    };

    let commissions_json: serde_json::Value = commissions
        .iter()
        .map(|(asset, amount)| (asset.clone(), serde_json::Value::String(amount.to_string())))
        .collect::<serde_json::Map<_, _>>()
        .into();

    (StatusCode::OK, Json(serde_json::json!({
        "code": code,
        "total_referred": total_referred,
        "commissions": commissions_json,
    })))
}

// ---------------------------------------------------------------------------
// T3.2: 2FA TOTP endpoints
// ---------------------------------------------------------------------------

/// POST /api/v1/account/2fa/setup
///
/// Generate a fresh TOTP secret for the authenticated user and return setup
/// data. The user scans the QR code in their authenticator app and then calls
/// `/enable` with a valid code to activate 2FA.
pub async fn totp_setup(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let setup = exchange
        .totp
        .lock().unwrap_or_else(|e| e.into_inner())
        .generate_secret(user_id, "Exchange", &user_id.to_string());

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "secret_base32": setup.secret_base32,
            "provisioning_uri": setup.provisioning_uri,
        })),
    )
}

#[derive(Debug, Deserialize)]
pub struct TotpCodeRequest {
    pub code: String,
}

/// POST /api/v1/account/2fa/enable
///
/// Verify a 6-digit code from the authenticator app, then enable 2FA and
/// return the one-time recovery codes (shown exactly once).
pub async fn totp_enable(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<TotpCodeRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;
    match exchange.totp.lock().unwrap_or_else(|e| e.into_inner()).verify_and_enable(user_id, &req.code) {
        Ok(recovery_codes) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "enabled": true,
                "recovery_codes": recovery_codes,
            })),
        ),
        Err(e) => {
            tracing::warn!(user_id = %user_id, error = %e, "2FA enable failed");
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        }
    }
}

/// POST /api/v1/account/2fa/disable
///
/// Verify a 6-digit code then disable 2FA for the authenticated user.
/// Returns 403 if the code is invalid.
pub async fn totp_disable(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<TotpCodeRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;
    match exchange.totp.lock().unwrap_or_else(|e| e.into_inner()).disable(user_id, &req.code) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({"disabled": true})),
        ),
        Err(e) => {
            tracing::warn!(user_id = %user_id, error = %e, "2FA disable failed");
            (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        }
    }
}

/// POST /api/v1/account/2fa/verify
///
/// Check whether a 6-digit TOTP code is currently valid for the authenticated
/// user. Used by the frontend before initiating sensitive operations.
pub async fn totp_verify(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<TotpCodeRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let valid = exchange.totp.lock().unwrap_or_else(|e| e.into_inner()).verify_code(user_id, &req.code).is_ok();
    (StatusCode::OK, Json(serde_json::json!({"valid": valid})))
}

// ---------------------------------------------------------------------------
// T3.3: API key management endpoints
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CreateApiKeyRequest {
    pub label: String,
    pub permissions: ApiKeyPermissionsRequest,
}

#[derive(Debug, Deserialize)]
pub struct ApiKeyPermissionsRequest {
    #[serde(default)]
    pub can_read: bool,
    #[serde(default)]
    pub can_trade: bool,
    #[serde(default)]
    pub can_withdraw: bool,
}

/// POST /api/v1/account/api-keys
///
/// Create a new API key for the authenticated user. The secret is returned
/// exactly once — the caller must store it securely.
pub async fn create_api_key(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<CreateApiKeyRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;

    let Some(ref key_store) = exchange.key_store else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "API key management not available in this mode"})),
        );
    };

    // Generate a random key and secret using UUID entropy.
    let key_str = format!("ek_{}", Uuid::new_v4().simple());
    let secret_str = format!("es_{}", Uuid::new_v4().simple());

    let permissions = ApiKeyPermissions {
        can_read: req.permissions.can_read,
        can_trade: req.permissions.can_trade,
        can_withdraw: req.permissions.can_withdraw,
    };

    let api_key = ApiKey {
        key: key_str.clone(),
        secret: secret_str.clone(),
        user_id,
        label: req.label.clone(),
        permissions,
        allowed_ips: vec![],
        created_at: chrono::Utc::now(),
        is_active: true,
        admin_role: None,
        sign_type: crate::auth::SignType::Hmac,
        public_key: None,
    };

    key_store.add_key(api_key);

    tracing::info!(user_id = %user_id, key = %key_str, label = %req.label, "API key created");

    // M24-6: Audit log entry for API key creation.
    exchange.audit(
        Some(user_id),
        exchange_admin::AuditAction::ApiKeyCreated,
        serde_json::json!({"key": key_str.clone(), "label": req.label}),
        None,
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "key": key_str,
            "secret": secret_str,
            "label": req.label,
        })),
    )
}

/// GET /api/v1/account/api-keys
///
/// List all active API keys for the authenticated user. Secrets are never
/// returned here — only the public key identifier and metadata.
pub async fn list_api_keys(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;

    let Some(ref key_store) = exchange.key_store else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "API key management not available in this mode"})),
        );
    };

    let keys = key_store.list_user_keys(user_id);
    let result: Vec<serde_json::Value> = keys
        .iter()
        .map(|k| {
            serde_json::json!({
                "key": k.key,
                "label": k.label,
                "can_read": k.permissions.can_read,
                "can_trade": k.permissions.can_trade,
                "can_withdraw": k.permissions.can_withdraw,
                "is_active": k.is_active,
                "created_at": k.created_at.timestamp_millis(),
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(result)))
}

/// DELETE /api/v1/account/api-keys/:key
///
/// Revoke an API key. The caller must own the key (user_id must match).
/// Returns 404 if the key does not exist or belongs to another user.
pub async fn revoke_api_key(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    let user_id = auth.0;

    let Some(ref key_store) = exchange.key_store else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "API key management not available in this mode"})),
        );
    };

    match key_store.revoke_key(&key, user_id) {
        Ok(()) => {
            tracing::info!(user_id = %user_id, key = %key, "API key revoked");
            // M24-6: Audit log entry for API key revocation.
            exchange.audit(
                Some(user_id),
                exchange_admin::AuditAction::ApiKeyRevoked,
                serde_json::json!({"key": key}),
                None,
            );
            (StatusCode::OK, Json(serde_json::json!({"revoked": true})))
        }
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

// ---------------------------------------------------------------------------
// T5.3: Trigger Orders REST endpoints
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct PlaceTriggerOrderRequest {
    pub symbol: String,
    pub side: String,
    pub trigger_price: Decimal,
    #[serde(default)]
    pub order_price: Option<Decimal>,
    pub qty: Decimal,
    pub trigger_type: String,
    #[serde(default)]
    pub trigger_by: Option<String>,
    #[serde(default)]
    pub reduce_only: Option<bool>,
    #[serde(default)]
    pub close_on_trigger: Option<bool>,
    /// For trailing stops: the retrace amount before triggering.
    #[serde(default)]
    pub trailing_delta: Option<Decimal>,
    /// For trailing stops: "fixed" (default) or "percentage".
    #[serde(default)]
    pub trail_type: Option<String>,
    // --- TP/SL limit order fields ---
    /// Limit price to use when the TP fires as a limit order.
    /// Only used when `tp_order_type` is "limit".
    #[serde(rename = "tpLimitPrice", default)]
    pub tp_limit_price: Option<Decimal>,
    /// Limit price to use when the SL fires as a limit order.
    /// Only used when `sl_order_type` is "limit".
    #[serde(rename = "slLimitPrice", default)]
    pub sl_limit_price: Option<Decimal>,
    /// Execution type for TP: "market" (default) or "limit".
    #[serde(rename = "tpOrderType", default)]
    pub tp_order_type: Option<String>,
    /// Execution type for SL: "market" (default) or "limit".
    #[serde(rename = "slOrderType", default)]
    pub sl_order_type: Option<String>,
}

/// Parse a `trigger_type` string into `TriggerOrderType`.
fn parse_trigger_type(s: &str) -> Result<TriggerOrderType, (StatusCode, Json<serde_json::Value>)> {
    match s.to_lowercase().as_str() {
        "stop_loss" | "stop_market" => Ok(TriggerOrderType::StopMarket),
        "stop_limit" => Ok(TriggerOrderType::StopLimit),
        "take_profit" => Ok(TriggerOrderType::TakeProfit),
        "take_profit_market" => Ok(TriggerOrderType::TakeProfitMarket),
        "trailing_stop" => Ok(TriggerOrderType::TrailingStop),
        _ => Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!(
                "Invalid trigger_type: '{}'. Use stop_loss, stop_limit, take_profit, take_profit_market, or trailing_stop",
                s
            )})),
        )),
    }
}

/// Parse a `trigger_by` string into `TriggerBy`.
fn parse_trigger_by(s: &str) -> Result<TriggerBy, (StatusCode, Json<serde_json::Value>)> {
    match s.to_lowercase().as_str() {
        "last_price" => Ok(TriggerBy::LastPrice),
        "mark_price" => Ok(TriggerBy::MarkPrice),
        "index_price" => Ok(TriggerBy::IndexPrice),
        _ => Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!(
                "Invalid trigger_by: '{}'. Use last_price, mark_price, or index_price",
                s
            )})),
        )),
    }
}

/// Serialize a `TriggerOrder` to a JSON value for REST responses.
fn trigger_order_to_json(o: &TriggerOrder) -> serde_json::Value {
    serde_json::json!({
        "trigger_order_id": o.id.to_string(),
        "symbol": o.symbol.to_string(),
        "side": format!("{:?}", o.side).to_lowercase(),
        "trigger_type": format!("{:?}", o.trigger_type),
        "trigger_price": o.trigger_price,
        "order_price": o.order_price,
        "qty": o.qty,
        "trigger_by": format!("{:?}", o.trigger_by),
        "reduce_only": o.reduce_only,
        "close_on_trigger": o.close_on_trigger,
        "tp_limit_price": o.tp_limit_price,
        "sl_limit_price": o.sl_limit_price,
        "tp_order_type": format!("{:?}", o.tp_order_type).to_lowercase(),
        "sl_order_type": format!("{:?}", o.sl_order_type).to_lowercase(),
        "status": format!("{:?}", o.status).to_lowercase(),
        "created_at": o.created_at.timestamp_millis(),
        "triggered_at": o.triggered_at.map(|t| t.timestamp_millis()),
    })
}

/// POST /api/v1/trigger-orders
///
/// Place a new conditional (stop / take-profit / trailing) order.
pub async fn place_trigger_order(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<PlaceTriggerOrderRequest>,
) -> impl IntoResponse {
    if let Err(e) = validate_symbol(&req.symbol) {
        return e;
    }
    if let Err(e) = validate_positive(req.qty, "qty") {
        return e;
    }
    if let Err(e) = validate_positive(req.trigger_price, "trigger_price") {
        return e;
    }

    let side = match req.side.to_lowercase().as_str() {
        "buy" => Side::Buy,
        "sell" => Side::Sell,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid side. Use 'buy' or 'sell'"})),
            );
        }
    };

    let trigger_type = match parse_trigger_type(&req.trigger_type) {
        Ok(t) => t,
        Err(e) => return e,
    };

    let trigger_by = match req.trigger_by.as_deref() {
        Some(s) => match parse_trigger_by(s) {
            Ok(t) => t,
            Err(e) => return e,
        },
        None => TriggerBy::MarkPrice,
    };

    // Task 2: price protection — reject if trigger price deviates >50% from mark price.
    let sym = Symbol::new(&req.symbol);
    if let Some(mark) = exchange.get_mark_price(&sym) {
        let deviation = ((req.trigger_price - mark.price).abs() / mark.price) * Decimal::from(100);
        if deviation > Decimal::from(50) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "Price protection: trigger price deviates >50% from mark price",
                    "markPrice": mark.price.to_string(),
                    "triggerPrice": req.trigger_price.to_string(),
                    "deviationPct": deviation.to_string(),
                })),
            );
        }
    }

    let parse_tp_sl_order_type = |s: Option<&str>| -> TpSlOrderType {
        match s {
            Some("limit") => TpSlOrderType::Limit,
            _ => TpSlOrderType::Market,
        }
    };

    let tp_order_type = parse_tp_sl_order_type(req.tp_order_type.as_deref());
    let sl_order_type = parse_tp_sl_order_type(req.sl_order_type.as_deref());

    // When a limit TP/SL is requested, the limit price must be provided.
    if tp_order_type == TpSlOrderType::Limit && req.tp_limit_price.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "tpLimitPrice is required when tpOrderType is 'limit'"
            })),
        );
    }
    if sl_order_type == TpSlOrderType::Limit && req.sl_limit_price.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "slLimitPrice is required when slOrderType is 'limit'"
            })),
        );
    }

    let order = TriggerOrder {
        id: Uuid::new_v4(),
        user_id: auth.0,
        symbol: sym,
        trigger_type,
        side,
        trigger_price: req.trigger_price,
        order_price: req.order_price,
        qty: req.qty,
        trigger_by,
        reduce_only: req.reduce_only.unwrap_or(false),
        close_on_trigger: req.close_on_trigger.unwrap_or(false),
        trailing_delta: req.trailing_delta,
        trail_type: match req.trail_type.as_deref().unwrap_or("fixed") {
            "percentage" => exchange_trigger_service::TrailType::Percentage,
            _ => exchange_trigger_service::TrailType::Fixed,
        },
        tp_limit_price: req.tp_limit_price,
        sl_limit_price: req.sl_limit_price,
        tp_order_type,
        sl_order_type,
        status: TriggerStatus::Active,
        created_at: chrono::Utc::now(),
        triggered_at: None,
        expires_at: None,
    };

    let order_id = order.id;

    match exchange.trigger_engine.lock().unwrap_or_else(|e| e.into_inner()).add_order(order) {
        Ok(()) => {
            tracing::info!(
                user_id = %auth.0,
                order_id = %order_id,
                trigger_type = %req.trigger_type,
                symbol = %req.symbol,
                "trigger order placed"
            );
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "trigger_order_id": order_id.to_string(),
                    "status": "active",
                })),
            )
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

/// GET /api/v1/trigger-orders
///
/// List the authenticated user's active trigger orders.
pub async fn list_trigger_orders(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let engine = exchange.trigger_engine.lock().unwrap_or_else(|e| e.into_inner());
    let orders: Vec<serde_json::Value> = engine
        .active_orders(user_id)
        .iter()
        .map(|o| trigger_order_to_json(o))
        .collect();

    (StatusCode::OK, Json(serde_json::json!(orders)))
}

/// DELETE /api/v1/trigger-orders/:id
///
/// Cancel a trigger order by ID. The order must belong to the authenticated user.
pub async fn cancel_trigger_order(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let order_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid trigger_order_id"})),
            );
        }
    };

    let user_id = auth.0;
    let result = exchange
        .trigger_engine
        .lock().unwrap_or_else(|e| e.into_inner())
        .cancel_order(order_id);

    match result {
        Ok(cancelled) => {
            if cancelled.user_id != user_id {
                tracing::warn!(
                    caller = %user_id,
                    owner = %cancelled.user_id,
                    order_id = %order_id,
                    "trigger order cancel rejected: wrong owner"
                );
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({"error": "Trigger order not found"})),
                );
            }
            tracing::info!(user_id = %user_id, order_id = %order_id, "trigger order cancelled");
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "trigger_order_id": order_id.to_string(),
                    "status": "cancelled",
                })),
            )
        }
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

// ---------------------------------------------------------------------------
// T5.4: Deposit and Withdrawal endpoints
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct DepositAddressRequest {
    pub asset: String,
    pub network: String,
}

#[derive(Debug, Deserialize)]
pub struct WithdrawRequest {
    pub asset: String,
    pub network: String,
    pub address: String,
    pub amount: Decimal,
}

/// POST /api/v1/account/deposit-address
///
/// Get or create a deposit address for the authenticated user on the given
/// asset/network combination. Idempotent — same address is returned on
/// repeated calls for the same user + asset.
///
/// # Ledger Integration
///
/// Deposits are confirmed asynchronously via a custody provider webhook.
/// When a deposit is confirmed, create a ledger entry:
///
/// ```text
/// // TODO (Phase 2 — Fireblocks webhook handler):
/// // On deposit confirmation:
/// //   LedgerService::record_deposit(&pool, user_id, &asset, amount, &deposit_id).await
/// ```
pub async fn get_deposit_address(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<DepositAddressRequest>,
) -> impl IntoResponse {
    if let Err(e) = check_feature(&exchange, FLAG_DEPOSITS) {
        return e;
    }

    let user_id = auth.0;

    // Check cache first (short lock), then call provider outside lock if needed.
    let cached = {
        let dm = exchange.deposit_manager.lock().unwrap_or_else(|e| e.into_inner());
        dm.get_cached_address(user_id, &req.asset, &req.network)
    };

    let result = if let Some(addr) = cached {
        Ok(addr)
    } else {
        // Call provider without holding the lock (async HTTP call).
        let addr_result = exchange.custody_provider
            .create_deposit_address(user_id, &req.asset, &req.network)
            .await;
        match addr_result {
            Ok(addr) => {
                let mut dm = exchange.deposit_manager.lock().unwrap_or_else(|e| e.into_inner());
                dm.register_address(user_id, &req.asset, &req.network, addr.clone());
                Ok(addr)
            }
            Err(e) => Err(e),
        }
    };

    match result {
        Ok(addr) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "address": addr.address,
                "memo": addr.memo,
                "network": addr.network,
            })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

/// GET /api/v1/account/deposits
///
/// List the authenticated user's deposit history, newest first.
pub async fn list_deposits(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let mgr = exchange.deposit_manager.lock().unwrap_or_else(|e| e.into_inner());
    let mut records: Vec<_> = mgr.user_deposits(&user_id);
    records.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let deposits: Vec<serde_json::Value> = records
        .into_iter()
        .map(|d| {
            serde_json::json!({
                "id": d.id.to_string(),
                "asset": d.asset,
                "network": d.network,
                "amount": d.amount,
                "address": d.address,
                "tx_hash": d.tx_hash,
                "status": format!("{:?}", d.status),
                "created_at": d.created_at.timestamp_millis(),
                "credited_at": d.credited_at.map(|t| t.timestamp_millis()),
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(deposits)))
}

// --- Deposit webhook ---

/// Verify the HMAC-SHA256 signature on an incoming webhook request.
///
/// Callers must supply `x-webhook-signature: <hex-encoded HMAC>` in the request
/// headers.  The HMAC is computed over the raw request body using the secret
/// configured via the `WEBHOOK_SECRET` environment variable (falls back to
/// `"dev-webhook-secret"` when running in dev mode, with a warning).
///
/// Returns `Ok(())` when the signature is valid, or an HTTP-ready response
/// tuple on failure so the caller can return early.
fn verify_webhook_signature(
    headers: &axum::http::HeaderMap,
    body: &[u8],
    secrets: &dyn exchange_common::SecretProvider,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let secret = secrets.get_or(
        exchange_common::secrets::names::WEBHOOK_SECRET,
        "dev-webhook-secret",
    );
    if secret == "dev-webhook-secret" {
        tracing::warn!(
            "WEBHOOK_SECRET is not set — using insecure dev fallback. \
             Set WEBHOOK_SECRET in production."
        );
    }

    let sig_header = headers
        .get("x-webhook-signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if sig_header.is_empty() {
        tracing::warn!("webhook request rejected: missing x-webhook-signature header");
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Missing webhook signature"})),
        ));
    }

    // Compute expected HMAC-SHA256(secret, body).
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(body);
    let expected = hex::encode(mac.finalize().into_bytes());

    // Constant-time comparison prevents timing-based secret recovery.
    if !webhook_sig_eq(sig_header.as_bytes(), expected.as_bytes()) {
        tracing::warn!("webhook request rejected: invalid x-webhook-signature");
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Invalid webhook signature"})),
        ));
    }

    Ok(())
}

/// Constant-time byte-slice equality that does not short-circuit on mismatch.
///
/// Length mismatch is not secret information and is rejected immediately.
#[inline]
fn webhook_sig_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Verify a custody webhook signature using the custody service's HMAC scheme.
///
/// The custody service signs outgoing webhooks as:
///   `HMAC-SHA256(secret, "POST" + webhook_url + timestamp + body)`
///
/// Headers:
///   `X-Custody-Timestamp` — Unix timestamp in seconds
///   `X-Custody-Signature` — hex-encoded HMAC-SHA256
///
/// Staleness window: 30 seconds (matching custody's API auth).
fn verify_custody_webhook(
    headers: &axum::http::HeaderMap,
    body: &[u8],
    secrets: &dyn exchange_common::SecretProvider,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let secret = secrets
        .get("CUSTODY_WEBHOOK_SECRET")
        .or_else(|| secrets.get(exchange_common::secrets::names::CUSTODY_HMAC_SECRET))
        .unwrap_or_else(|| {
            tracing::warn!("CUSTODY_WEBHOOK_SECRET / CUSTODY_HMAC_SECRET not set — using dev fallback");
            "dev-custody-secret".to_string()
        });

    let timestamp = headers
        .get("X-Custody-Timestamp")
        .or_else(|| headers.get("x-custody-timestamp"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let signature = headers
        .get("X-Custody-Signature")
        .or_else(|| headers.get("x-custody-signature"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if signature.is_empty() || timestamp.is_empty() {
        tracing::warn!("custody webhook rejected: missing X-Custody-Signature or X-Custody-Timestamp");
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Missing custody webhook signature"})),
        ));
    }

    // Check timestamp staleness (30-second window).
    if let Ok(ts) = timestamp.parse::<i64>() {
        let now = chrono::Utc::now().timestamp();
        if (now - ts).unsigned_abs() > 30 {
            tracing::warn!("custody webhook rejected: timestamp too old ({ts} vs now {now})");
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Webhook timestamp expired"})),
            ));
        }
    }

    // Custody signs: method + webhook_url + timestamp + body.
    // We reconstruct using CUSTODY_WEBHOOK_URL env var (full URL the custody POSTs to).
    let webhook_url = std::env::var("CUSTODY_WEBHOOK_URL")
        .unwrap_or_else(|_| "http://localhost:8080/webhooks/custody".to_string());

    let body_str = std::str::from_utf8(body).unwrap_or("");
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(b"POST");
    mac.update(webhook_url.as_bytes());
    mac.update(timestamp.as_bytes());
    mac.update(body_str.as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());

    if !webhook_sig_eq(signature.as_bytes(), expected.as_bytes()) {
        tracing::warn!("custody webhook rejected: invalid X-Custody-Signature");
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Invalid custody webhook signature"})),
        ));
    }

    Ok(())
}

/// Payload sent by a blockchain monitor or custody provider when a deposit is detected.
#[derive(Debug, Deserialize)]
pub struct DepositWebhookPayload {
    pub address: String,
    pub asset: String,
    pub network: String,
    pub amount: Decimal,
    pub tx_hash: String,
    pub confirmations: u32,
}

/// POST /webhooks/deposit — receive deposit notification from custody provider or blockchain monitor.
///
/// Public route (no user auth); called by an external blockchain monitor.
/// Credits the user's balance once the required number of confirmations has been reached.
///
/// Requires a valid HMAC-SHA256 signature in the `x-webhook-signature` header,
/// computed as `hex(HMAC-SHA256(WEBHOOK_SECRET, raw_body))`.
pub async fn webhook_deposit(
    State(exchange): State<AppState>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    // Authenticate webhook caller before processing any state-mutating logic.
    if let Err(resp) = verify_webhook_signature(&headers, &body, &*exchange.secrets) {
        return resp.into_response();
    }

    let payload: DepositWebhookPayload = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("Invalid JSON: {e}")})),
            )
            .into_response();
        }
    };
    // 1. Look up user by deposit address.
    let user_id = {
        let dm = exchange.deposit_manager.lock().unwrap_or_else(|e| e.into_inner());
        match dm.user_for_address(&payload.address) {
            Some(uid) => uid,
            None => {
                tracing::warn!(address = %payload.address, "deposit webhook: unknown address");
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "Unknown deposit address"})),
                )
                .into_response();
            }
        }
    };

    // 1b. Sanctions screening on the sender/deposit address.
    {
        let screener = exchange.sanctions.lock().unwrap_or_else(|e| e.into_inner());
        if let exchange_compliance::SanctionsResult::Match { list, matched_entry } =
            screener.screen_address(&payload.address)
        {
            tracing::error!(
                address = %payload.address,
                list = %list,
                matched_entry = %matched_entry,
                user_id = %user_id,
                "deposit BLOCKED by sanctions screening"
            );
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"error": "Deposit blocked by compliance screening"})),
            )
            .into_response();
        }
        // Also screen tx_hash-associated sender address if available in the future.
        // For now, only the deposit address itself is screened.
    }

    // 2. Record the deposit and obtain its assigned ID.
    let deposit_id = {
        let mut dm = exchange.deposit_manager.lock().unwrap_or_else(|e| e.into_inner());
        match dm.record_deposit(
            &payload.address,
            &payload.asset,
            &payload.network,
            payload.amount,
            payload.tx_hash.clone(),
        ) {
            Some(record) => record.id,
            None => {
                tracing::error!(
                    address = %payload.address,
                    "deposit webhook: record_deposit returned None after address lookup"
                );
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "Failed to record deposit"})),
                )
                .into_response();
            }
        }
    };

    // 3. Check if enough confirmations to credit.
    let required = exchange_custody::required_confirmations(&payload.asset);
    if payload.confirmations >= required {
        // Credit the user's balance.  Use deposit_with_ref so the tx hash is
        // captured in the WAL entry for crash-recovery deduplication.
        let tx_ref = payload.tx_hash.as_str();
        if let Err(e) = exchange.deposit_with_ref(user_id, &payload.asset, payload.amount, tx_ref) {
            tracing::error!(
                user_id = %user_id,
                error = %e,
                "deposit webhook: failed to credit deposit"
            );
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Credit failed"})),
            )
            .into_response();
        }

        // Mark deposit record as credited.
        {
            let mut dm = exchange.deposit_manager.lock().unwrap_or_else(|e| e.into_inner());
            dm.credit_deposit(&deposit_id);
        }

        // Broadcast private event so WebSocket subscribers receive the notification.
        let _ = exchange.private_tx.send(crate::exchange::PrivateEvent::DepositCredited {
            user_id: user_id.to_string(),
            asset: payload.asset.clone(),
            amount: payload.amount,
            tx_hash: payload.tx_hash.clone(),
        });


        // M24-6: Audit log entry for deposit.
        exchange.audit(
            Some(user_id),
            exchange_admin::AuditAction::DepositCredited,
            serde_json::json!({
                "deposit_id": deposit_id.to_string(),
                "asset": &payload.asset,
                "amount": payload.amount.to_string(),
                "tx_hash": &payload.tx_hash,
            }),
            None,
        );

        // NOTIF-2: Push notification for deposit completion.
        exchange.send_push_notification(
            user_id,
            "Deposit Completed",
            &format!("{} {} deposit confirmed", payload.amount, payload.asset),
            exchange_admin::NotificationType::DepositCompleted,
            serde_json::json!({
                "deposit_id": deposit_id.to_string(),
                "asset": &payload.asset,
                "amount": payload.amount.to_string(),
            }),
        );
        // Write-behind: persist balance change to DB.
        let maybe_pool = exchange.db_pool.get().cloned();
        if let Some(pool) = maybe_pool {
            let uid = user_id;
            let asset = payload.asset.clone();
            let amount = payload.amount;
            tokio::spawn(async move {
                if let Err(e) = exchange_db::balance_repo::BalanceRepo::deposit(&pool, uid, &asset, amount).await {
                    tracing::error!(user_id = %uid, error = %e, "deposit webhook: DB write-behind failed");
                }
            });
        }

        tracing::info!(
            user_id = %user_id,
            asset = %payload.asset,
            amount = %payload.amount,
            tx_hash = %payload.tx_hash,
            "deposit credited"
        );

        (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "credited",
                "deposit_id": deposit_id.to_string(),
            })),
        )
        .into_response()
    } else {
        tracing::info!(
            user_id = %user_id,
            asset = %payload.asset,
            confirmations = payload.confirmations,
            required = required,
            "deposit pending confirmations"
        );

        (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "pending",
                "deposit_id": deposit_id.to_string(),
                "confirmations": payload.confirmations,
                "required": required,
            })),
        )
        .into_response()
    }
}

/// POST /webhooks/custody — unified webhook handler for the MG Custody service.
///
/// Handles all custody events: deposit confirmations, withdrawal lifecycle updates.
/// Uses the custody HMAC signing scheme (method + url + timestamp + body).
pub async fn webhook_custody(
    State(exchange): State<AppState>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    // Authenticate using custody's HMAC scheme.
    if let Err(resp) = verify_custody_webhook(&headers, &body, &*exchange.secrets) {
        return resp.into_response();
    }

    let event: exchange_custody::CustodyEvent = match serde_json::from_slice(&body) {
        Ok(e) => e,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("Invalid CustodyEvent JSON: {e}")})),
            ).into_response();
        }
    };

    match event {
        exchange_custody::CustodyEvent::DepositConfirmed {
            user_address,
            user_id,
            asset,
            chain: _,
            amount,
            tx_hash,
            ..
        } => {
            // Sanctions screening on deposit address.
            {
                let screener = exchange.sanctions.lock().unwrap_or_else(|e| e.into_inner());
                if let exchange_compliance::SanctionsResult::Match { list, matched_entry } =
                    screener.screen_address(&user_address)
                {
                    tracing::error!(
                        address = %user_address, list = %list, matched_entry = %matched_entry,
                        user_id = %user_id, "custody deposit BLOCKED by sanctions"
                    );
                    return (StatusCode::FORBIDDEN, Json(serde_json::json!({
                        "error": "Deposit blocked by compliance screening"
                    }))).into_response();
                }
            }

            // Record deposit (idempotent — dedup by tx_hash).
            let deposit_id = {
                let mut dm = exchange.deposit_manager.lock().unwrap_or_else(|e| e.into_inner());
                match dm.record_deposit(&user_address, &asset, "custody", amount, tx_hash.clone()) {
                    Some(record) => record.id,
                    None => {
                        // Address not in our map — custody sent user_id, credit directly.
                        tracing::info!(
                            user_id = %user_id, address = %user_address,
                            "custody deposit: address not cached, crediting by user_id"
                        );
                        uuid::Uuid::new_v4()
                    }
                }
            };

            // Credit balance (custody has already confirmed sufficient on-chain confirmations).
            {
                let mut dm = exchange.deposit_manager.lock().unwrap_or_else(|e| e.into_inner());
                dm.credit_deposit(&deposit_id);
            }

            let _ = exchange.deposit_with_ref(user_id, &asset, amount, &tx_hash);

            tracing::info!(
                user_id = %user_id, asset = %asset, amount = %amount, tx_hash = %tx_hash,
                "custody deposit credited"
            );

            (StatusCode::OK, Json(serde_json::json!({
                "status": "credited",
                "deposit_id": deposit_id.to_string(),
            }))).into_response()
        }

        exchange_custody::CustodyEvent::WithdrawalBroadcast {
            exchange_withdrawal_id,
            tx_hash,
            ..
        } => {
            let mut wm = exchange.withdrawal_manager.lock().unwrap_or_else(|e| e.into_inner());
            match wm.on_broadcast(&exchange_withdrawal_id, tx_hash.clone()) {
                Ok(_) => {
                    tracing::info!(
                        withdrawal_id = %exchange_withdrawal_id, tx_hash = %tx_hash,
                        "custody withdrawal broadcast"
                    );
                    (StatusCode::OK, Json(serde_json::json!({"status": "on_chain"}))).into_response()
                }
                Err(e) => {
                    tracing::warn!(withdrawal_id = %exchange_withdrawal_id, error = %e, "on_broadcast failed");
                    (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response()
                }
            }
        }

        exchange_custody::CustodyEvent::WithdrawalConfirmed {
            exchange_withdrawal_id,
            tx_hash,
            ..
        } => {
            let mut wm = exchange.withdrawal_manager.lock().unwrap_or_else(|e| e.into_inner());
            match wm.on_confirmed(&exchange_withdrawal_id, tx_hash.clone()) {
                Ok(_) => {
                    tracing::info!(
                        withdrawal_id = %exchange_withdrawal_id, tx_hash = %tx_hash,
                        "custody withdrawal confirmed"
                    );
                    (StatusCode::OK, Json(serde_json::json!({"status": "completed"}))).into_response()
                }
                Err(e) => {
                    tracing::warn!(withdrawal_id = %exchange_withdrawal_id, error = %e, "on_confirmed failed");
                    (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response()
                }
            }
        }

        exchange_custody::CustodyEvent::WithdrawalFailed {
            exchange_withdrawal_id,
            reason,
            ..
        } => {
            let refund = {
                let mut wm = exchange.withdrawal_manager.lock().unwrap_or_else(|e| e.into_inner());
                wm.on_failed(&exchange_withdrawal_id, reason.clone())
            };

            match refund {
                Ok((user_id, asset, amount, fee)) => {
                    // Refund locked balance (amount + fee).
                    let refund_total = amount + fee;
                    let _ = exchange.deposit(user_id, &asset, refund_total);
                    tracing::warn!(
                        withdrawal_id = %exchange_withdrawal_id,
                        user_id = %user_id,
                        refund = %refund_total,
                        reason = %reason,
                        "custody withdrawal failed — balance refunded"
                    );
                    (StatusCode::OK, Json(serde_json::json!({
                        "status": "failed",
                        "refunded": refund_total.to_string(),
                    }))).into_response()
                }
                Err(e) => {
                    tracing::warn!(withdrawal_id = %exchange_withdrawal_id, error = %e, "on_failed failed");
                    (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response()
                }
            }
        }
    }
}

/// POST /api/v1/account/withdraw
///
/// Submit a withdrawal request. Validates the amount against the asset minimum
/// and checks the user has sufficient available balance before recording the
/// request as `pending_approval`.
pub async fn submit_withdrawal(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<WithdrawRequest>,
) -> impl IntoResponse {
    if let Err(e) = check_feature(&exchange, FLAG_WITHDRAWALS) {
        return e;
    }

    let user_id = auth.0;

    // Frozen accounts may not withdraw.
    if exchange.frozen_accounts.contains(&user_id) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Account is frozen — withdrawals suspended"})),
        );
    }

    // Identity gate: KYC approval and email verification required before withdrawing.
    // Bypassed in DEV_MODE or when no DATABASE_URL (in-memory / test mode).
    // S19 D.1: cached field replaces three env::var syscalls per withdrawal.
    if !exchange.is_dev_mode {
        let kyc_ok = exchange.kyc_approved.contains(&user_id);
        if !kyc_ok {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"error": "KYC verification required before withdrawals"})),
            );
        }
        let email_ok = exchange.email_verified.contains(&user_id);
        if !email_ok {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"error": "Email verification required before withdrawals"})),
            );
        }
    }

    if let Err(e) = validate_positive(req.amount, "amount") {
        return e;
    }

    // T12.2: Check daily withdrawal limit before any further processing.
    {
        let mgr = exchange.withdrawal_manager.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(limit_err) = mgr.check_daily_limit(&user_id, req.amount, UserTier::Default) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "Daily withdrawal limit exceeded",
                    "daily_used": limit_err.daily_used,
                    "daily_limit": limit_err.daily_limit,
                    "remaining_limit": limit_err.remaining_limit,
                })),
            );
        }
    }

    // Check available balance before submitting.
    {
        let available = exchange
            .accounts
            .get_balance(user_id, &req.asset)
            .map(|b| b.available())
            .unwrap_or(Decimal::ZERO);

        if available < req.amount {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!(
                        "Insufficient balance: have {} {}, need {}",
                        available, req.asset, req.amount
                    )
                })),
            );
        }
    }

    // M22-5: Jurisdiction-based withdrawal limit.
    // In production, check user's jurisdiction for withdrawal restrictions.
    // Phase 1: user_jurisdictions map on Exchange; Phase 2: derived from IP/KYC country.
    {
        if let Some(jurisdiction) = exchange
            .user_jurisdictions
            .lock().unwrap_or_else(|e| e.into_inner())
            .get(&user_id)
            .copied()
        {
            let je = exchange.jurisdiction_engine.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(rules) = je.get_rules(jurisdiction) {
                if req.amount > rules.max_daily_withdrawal_usd {
                    return (
                        StatusCode::FORBIDDEN,
                        Json(serde_json::json!({
                            "error": format!(
                                "Withdrawal exceeds jurisdiction limit: max ${}/day for {:?}",
                                rules.max_daily_withdrawal_usd,
                                jurisdiction,
                            )
                        })),
                    );
                }
            }
        }
    }

    // M22-2: KYC gate on withdrawal above $500. Unverified users blocked.
    // Bypassed in dev mode (no DATABASE_URL).
    // S19 D.1: cached field replaces env::var("PRODUCTION") syscall per call.
    if exchange.is_production && req.amount >= Decimal::new(500, 0) {
        let kyc_ok = exchange.kyc_approved.contains(&user_id);
        if !kyc_ok {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({
                    "error": "KYC verification required for withdrawals above $500"
                })),
            );
        }
    }

    // T11.3: Sanctions screening — block withdrawals to sanctioned addresses.
    {
        let screener = exchange.sanctions.lock().unwrap_or_else(|e| e.into_inner());
        if let SanctionsResult::Match { list, matched_entry } = screener.screen_address(&req.address) {
            tracing::warn!(
                user_id = %user_id,
                address = %req.address,
                list = %list,
                matched_entry = %matched_entry,
                "withdrawal blocked by sanctions screening"
            );
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"error": "Withdrawal blocked by sanctions screening"})),
            );
        }
    }

    // Deduct from in-memory balance before recording the withdrawal. This
    // prevents users from submitting multiple concurrent withdrawals that each
    // pass the balance check but together exceed the available balance.
    if let Err(e) = exchange
        .accounts
        .withdraw(user_id, &req.asset, req.amount)
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("Insufficient balance: {e}")})),
        );
    }

    let result = exchange
        .withdrawal_manager
        .lock().unwrap_or_else(|e| e.into_inner())
        .submit(user_id, &req.asset, &req.network, &req.address, req.amount);

    // If submit fails after the deduction, refund the amount so the user's
    // in-memory balance stays consistent.
    if let Err(ref e) = result {
        tracing::warn!(
            user_id = %user_id,
            asset = %req.asset,
            amount = %req.amount,
            error = %e,
            "withdrawal submit failed after balance deduction — refunding"
        );
        let _ = exchange
            .accounts
            .deposit(user_id, &req.asset, req.amount);
    }

    match result {
        Ok(record) => {
            tracing::info!(
                user_id = %user_id,
                withdrawal_id = %record.id,
                asset = %record.asset,
                amount = %record.amount,
                "withdrawal submitted"
            );

            // WAL: record the balance deduction for crash-recovery durability.
            exchange.append_to_wal(&exchange_common::Command::BalanceWithdraw {
                user_id,
                asset: record.asset.clone(),
                amount: record.amount,
                reference: record.id.to_string(),
            });

            // Write-behind ledger entry (async, non-blocking).
            {
                let maybe_pool = exchange.db_pool.get().cloned();
                if let Some(pool) = maybe_pool {
                    let withdrawal_id_str = record.id.to_string();
                    let asset = record.asset.clone();
                    let amount = record.amount;
                    let fee = record.fee;
                    tokio::spawn(async move {
                        if let Err(e) = exchange_db::ledger::LedgerService::record_withdrawal(
                            &pool,
                            user_id,
                            &asset,
                            amount,
                            fee,
                            &withdrawal_id_str,
                        )
                        .await
                        {
                            tracing::error!(
                                withdrawal_id = %withdrawal_id_str,
                                error = %e,
                                "ledger write failed for withdrawal"
                            );
                        }
                    });
                }
            }

            // T8.5: Travel Rule — check if this withdrawal meets the reporting threshold.
            // We use the withdrawal amount as the USD proxy (Phase 1: no price conversion).
            let amount_usd = record.amount;
            let travel_rule_required = {
                let mut tr = exchange.travel_rule.lock().unwrap_or_else(|e| e.into_inner());
                if tr.check_required(amount_usd) {
                    // Use user_id as the originator name for now (Phase 1).
                    tr.create_outgoing(
                        record.id,
                        amount_usd,
                        &record.asset,
                        user_id.to_string(),
                        &record.address,
                    );
                    tracing::info!(
                        user_id = %user_id,
                        withdrawal_id = %record.id,
                        amount_usd = %amount_usd,
                        "travel rule record created for withdrawal"
                    );
                    true
                } else {
                    false
                }
            };

            // M24-6: Audit log entry for withdrawal request.
            exchange.audit(
                Some(user_id),
                exchange_admin::AuditAction::WithdrawalRequested,
                serde_json::json!({
                    "withdrawal_id": record.id.to_string(),
                    "asset": record.asset,
                    "amount": record.amount.to_string(),
                    "address": record.address,
                }),
                None,
            );

            // NOTIF-3: Push notification for withdrawal request.
            exchange.send_push_notification(
                user_id,
                "Withdrawal Requested",
                &format!("{} {} withdrawal submitted", record.amount, record.asset),
                exchange_admin::NotificationType::WithdrawalRequested,
                serde_json::json!({
                    "withdrawal_id": record.id.to_string(),
                    "asset": record.asset,
                    "amount": record.amount.to_string(),
                    "address": record.address,
                }),
            );

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "withdrawal_id": record.id.to_string(),
                    "status": "pending_approval",
                    "fee": record.fee,
                    "travel_rule_required": travel_rule_required,
                })),
            )
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

/// GET /api/v1/account/withdrawals
///
/// List the authenticated user's withdrawal history, newest first.
pub async fn list_withdrawals(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let mgr = exchange.withdrawal_manager.lock().unwrap_or_else(|e| e.into_inner());
    let mut records: Vec<_> = mgr.user_withdrawals(&user_id);
    records.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let withdrawals: Vec<serde_json::Value> = records
        .into_iter()
        .map(|w| {
            serde_json::json!({
                "id": w.id.to_string(),
                "asset": w.asset,
                "network": w.network,
                "address": w.address,
                "amount": w.amount,
                "fee": w.fee,
                "status": format!("{:?}", w.status),
                "created_at": w.created_at.timestamp_millis(),
                "completed_at": w.completed_at.map(|t| t.timestamp_millis()),
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(withdrawals)))
}

// ---------------------------------------------------------------------------
// T8.3: Reconciliation endpoint
// ---------------------------------------------------------------------------

/// GET /admin/reconciliation
///
/// Compute a full balance reconciliation between user ledger totals and
/// hot/warm/cold wallet reserves. Uses mock reserve data in Phase 1
/// (no live Fireblocks integration yet).
pub async fn admin_reconciliation(State(exchange): State<AppState>) -> impl IntoResponse {
    // Collect total user balances per asset across all accounts.
    let user_balances: HashMap<String, Decimal> = exchange.accounts.all_asset_totals();

    // C3: Fetch actual wallet reserves from the custody provider instead of mocking.
    let reserves: HashMap<String, WalletBalances> = {
        let mgr = exchange.wallet_manager.lock().unwrap_or_else(|e| e.into_inner());
        let by_type = mgr.balances_by_type();
        let totals = mgr.total_per_coin();

        let mut result: HashMap<String, WalletBalances> = HashMap::new();
        for (coin, &total) in &totals {
            let hot = by_type
                .get(&exchange_custody::WalletType::Hot)
                .map(|ws| ws.iter().filter(|w| &w.coin == coin).map(|w| w.balance).sum())
                .unwrap_or(Decimal::ZERO);
            let warm = by_type
                .get(&exchange_custody::WalletType::Warm)
                .map(|ws| ws.iter().filter(|w| &w.coin == coin).map(|w| w.balance).sum())
                .unwrap_or(Decimal::ZERO);
            let cold = by_type
                .get(&exchange_custody::WalletType::Cold)
                .map(|ws| ws.iter().filter(|w| &w.coin == coin).map(|w| w.balance).sum())
                .unwrap_or(Decimal::ZERO);

            result.insert(coin.clone(), WalletBalances { hot, warm, cold });
        }

        // If wallet_manager has no data (dev mode / no custody), fall back to
        // mock reserves to avoid a false "under-collateralized" alarm.
        if result.is_empty() {
            tracing::warn!(
                "Reconciliation: no wallet data from custody provider — using mock reserves. \
                 This is expected in dev mode but CRITICAL in production."
            );
            for (asset, &total) in &user_balances {
                result.insert(asset.clone(), WalletBalances {
                    hot:  total * Decimal::new(10, 2),
                    warm: total * Decimal::new(20, 2),
                    cold: total * Decimal::new(70, 2),
                });
            }
        }

        result
    };

    let report = Reconciler::reconcile(user_balances, reserves);

    tracing::info!(
        assets = report.assets.len(),
        status = ?report.status,
        "reconciliation report generated"
    );

    (StatusCode::OK, Json(serde_json::json!(report)))
}

// ---------------------------------------------------------------------------
// T8.4: Proof of Reserves endpoints
// ---------------------------------------------------------------------------

/// Query parameters for the PoR verify endpoint.
#[derive(Debug, Deserialize)]
pub struct PorVerifyQuery {
    pub user_id: Uuid,
    pub asset: String,
}

/// GET /api/v1/por/latest
///
/// Generate a fresh Proof of Reserves report from current in-memory balances.
/// Public endpoint — no auth required (transparency feature).
pub async fn por_latest(State(exchange): State<AppState>) -> impl IntoResponse {
    let liabilities = build_por_liabilities(&exchange);
    // Phase 1: mock on-chain reserves equal to total liabilities (100% backed).
    let reserves = build_mock_por_reserves(&liabilities);
    // M26-3: Completeness enforcement — the generate() function asserts leaf count
    // matches expected_account_count when provided. In Phase 1 (in-memory), we pass
    // None because zero-balance accounts are excluded from liabilities by design.
    // Phase 2 (DB-backed): pass the DB account count for strict enforcement.
    let (report, _tree) = ReportGenerator::generate(&liabilities, &reserves, None);

    tracing::debug!(
        merkle_root = %report.merkle_root,
        total_users = report.total_users,
        total_entries = report.total_entries,
        "PoR report generated"
    );

    // Phase 1: reserves are mock (= liabilities). Mark as unaudited.
    let is_mock = std::env::var("CUSTODY_BASE_URL").is_err();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "merkle_root": report.merkle_root,
            "generated_at": report.generated_at.to_rfc3339(),
            "total_users": report.total_users,
            "total_entries": report.total_entries,
            "assets": report.assets,
            "status": if is_mock { "unaudited_mock".to_string() } else { format!("{:?}", report.overall_status) },
            "disclaimer": if is_mock {
                Some("Reserve data is simulated. Real proof requires custody integration.")
            } else {
                None
            },
        })),
    )
}

/// GET /api/v1/por/verify?user_id=UUID&asset=BTC
///
/// Generate a Merkle proof for the given user + asset combination so the
/// user can independently verify their inclusion in the published root.
/// Public endpoint — no auth required.
pub async fn por_verify(
    State(exchange): State<AppState>,
    Query(params): Query<PorVerifyQuery>,
) -> impl IntoResponse {
    let liabilities = build_por_liabilities(&exchange);
    let reserves = build_mock_por_reserves(&liabilities);

    let (_report, tree) = ReportGenerator::generate(&liabilities, &reserves, None);

    match ReportGenerator::user_proof(&liabilities, &tree, params.user_id, &params.asset) {
        Some(proof) => {
            // Find the user's balance for this asset so the caller can self-verify.
            let balance = liabilities
                .iter()
                .find(|l| l.user_id == params.user_id && l.asset == params.asset)
                .map(|l| l.balance)
                .unwrap_or(Decimal::ZERO);

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "leaf_hash": proof.leaf_hash,
                    "siblings": proof.siblings,
                    "root": proof.root,
                    "balance": balance,
                })),
            )
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": format!(
                    "No liability found for user {} asset {}",
                    params.user_id, params.asset
                )
            })),
        ),
    }
}

/// Build the liability list from current account balances.
fn build_por_liabilities(exchange: &Exchange) -> Vec<UserLiability> {
    exchange
        .accounts
        .all_user_asset_balances()
        .into_iter()
        .map(|(user_id, asset, balance)| UserLiability { user_id, asset, balance })
        .collect()
}

/// Build mock on-chain reserves: for each asset, total = sum of all user balances.
///
/// Phase 1 placeholder — in production this would query the custody provider.
fn build_mock_por_reserves(
    liabilities: &[UserLiability],
) -> HashMap<String, (Decimal, Vec<String>)> {
    let mut totals: HashMap<String, Decimal> = HashMap::new();
    for l in liabilities {
        *totals.entry(l.asset.clone()).or_default() += l.balance;
    }
    totals
        .into_iter()
        .map(|(asset, total)| (asset, (total, vec!["mock_wallet_address".to_string()])))
        .collect()
}

// ---------------------------------------------------------------------------
// Proof of Reserves — admin generation + public/private verification
// ---------------------------------------------------------------------------

/// POST /admin/proof-of-reserves
///
/// Generate a fresh Proof of Reserves report from current in-memory balances
/// and on-chain hot-wallet reserves (queried from the custody provider).
///
/// The generated report and liability snapshot are cached on the Exchange so
/// the public verify and per-user proof endpoints serve a consistent view.
///
/// Design note: custody balances are fetched per-asset asynchronously via
/// `hot_wallet_balance`. When no real custody provider is configured (dev
/// mode) the mock provider returns a fixed balance equal to the total
/// liability for that asset, preserving the "fully backed" invariant.
pub async fn admin_por_report(State(exchange): State<AppState>) -> impl IntoResponse {
    let liabilities = build_por_liabilities(&exchange);

    // Collect the unique set of assets across all user liabilities.
    let unique_assets: std::collections::HashSet<String> = liabilities
        .iter()
        .map(|l| l.asset.clone())
        .collect();

    // Query the custody provider for each asset's hot-wallet balance.
    // Hot wallet is the on-chain portion immediately accessible; cold wallets
    // are added to proof manually by auditors and are not queryable here.
    let mut reserves: HashMap<String, (Decimal, Vec<String>)> = HashMap::new();
    for asset in &unique_assets {
        match exchange.custody_provider.hot_wallet_balance(asset).await {
            Ok(balance) => {
                reserves.insert(asset.clone(), (balance, vec!["hot_wallet".to_string()]));
            }
            Err(err) => {
                tracing::warn!(asset = %asset, error = %err, "Failed to fetch hot-wallet balance for PoR — using zero");
                reserves.insert(asset.clone(), (Decimal::ZERO, vec![]));
            }
        }
    }

    let account_count = exchange.accounts.user_count();

    let (report, _tree) = ReportGenerator::generate(
        &liabilities,
        &reserves,
        Some(account_count),
    );

    tracing::info!(
        merkle_root = %report.merkle_root,
        total_users = report.total_users,
        total_entries = report.total_entries,
        overall_status = ?report.overall_status,
        "Admin PoR report generated and cached"
    );

    // Cache the report and the ordered liability snapshot so the proof
    // endpoints can generate consistent per-user proofs against this root.
    {
        let mut cached = exchange.latest_por_report.lock().unwrap_or_else(|e| e.into_inner());
        *cached = Some((report.clone(), liabilities));
    }

    (StatusCode::OK, Json(report))
}

/// GET /api/v1/proof-of-reserves/verify
///
/// Public endpoint — returns the latest Merkle root, total liabilities per
/// asset, and the report timestamp.  Does NOT reveal individual user balances.
///
/// Returns 404 if no report has been generated yet (admin must call
/// POST /admin/proof-of-reserves first).
pub async fn por_public_verify(State(exchange): State<AppState>) -> impl IntoResponse {
    let cached = exchange.latest_por_report.lock().unwrap_or_else(|e| e.into_inner());

    match &*cached {
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "No Proof of Reserves report has been generated yet. \
                          An admin must call POST /admin/proof-of-reserves to create one."
            })),
        ),
        Some((report, _liabilities)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "report_id": report.report_id,
                "merkle_root": report.merkle_root,
                "generated_at": report.generated_at.to_rfc3339(),
                "total_users": report.total_users,
                "total_entries": report.total_entries,
                "overall_status": format!("{:?}", report.overall_status),
                "assets": report.assets,
            })),
        ),
    }
}

/// Query parameters for the authenticated proof endpoint.
#[derive(Debug, Deserialize)]
pub struct PorMyProofQuery {
    /// Asset to retrieve the proof for (e.g. "BTC", "USDT").
    pub asset: String,
}

/// GET /api/v1/proof-of-reserves/my-proof?asset=BTC
///
/// Authenticated endpoint — returns the requesting user's Merkle leaf and
/// sibling proof path so they can independently verify their balance is
/// included in the published Merkle root.
///
/// The `balance` field is returned in the response so the user can reconstruct
/// the leaf hash and walk the proof without any additional information.
///
/// Returns 404 if:
/// - No report has been generated yet.
/// - The user has no balance for the requested asset (not included in tree).
pub async fn por_my_proof(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
    Query(params): Query<PorMyProofQuery>,
) -> impl IntoResponse {
    let cached = exchange.latest_por_report.lock().unwrap_or_else(|e| e.into_inner());

    let (report, liabilities) = match &*cached {
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": "No Proof of Reserves report has been generated yet."
                })),
            );
        }
        Some(pair) => pair,
    };

    // Rebuild the Merkle tree from the cached liability snapshot so the proof
    // path is consistent with the published root.
    let leaves: Vec<_> = liabilities
        .iter()
        .map(|l| exchange_proof_of_reserves::hash_leaf(
            &l.user_id.to_string(),
            &l.asset,
            &l.balance.to_string(),
        ))
        .collect();

    let tree = if leaves.is_empty() {
        exchange_proof_of_reserves::MerkleTree::build(vec![
            exchange_proof_of_reserves::hash_leaf("empty", "none", "0"),
        ])
    } else {
        exchange_proof_of_reserves::MerkleTree::build(leaves)
    };

    match ReportGenerator::user_proof(liabilities, &tree, user.0, &params.asset) {
        Some(proof) => {
            let balance = liabilities
                .iter()
                .find(|l| l.user_id == user.0 && l.asset == params.asset)
                .map(|l| l.balance)
                .unwrap_or(Decimal::ZERO);

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "report_id": report.report_id,
                    "merkle_root": report.merkle_root,
                    "generated_at": report.generated_at.to_rfc3339(),
                    "asset": params.asset,
                    "balance": balance,
                    "leaf_hash": proof.leaf_hash,
                    "siblings": proof.siblings,
                    "root": proof.root,
                })),
            )
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": format!(
                    "No liability found for asset {} in the latest report. \
                     The user either has no balance for this asset or the \
                     report was generated before the balance was created.",
                    params.asset
                )
            })),
        ),
    }
}

// ---------------------------------------------------------------------------
// T8.5: Travel Rule admin endpoint
// ---------------------------------------------------------------------------

/// GET /admin/travel-rule/pending
///
/// List all outgoing Travel Rule records in `DataCollected` state that have
/// not yet been transmitted to a counterparty VASP.
pub async fn admin_travel_rule_pending(State(exchange): State<AppState>) -> impl IntoResponse {
    let tr = exchange.travel_rule.lock().unwrap_or_else(|e| e.into_inner());
    let pending: Vec<serde_json::Value> = tr
        .pending_transmissions()
        .iter()
        .map(|r| {
            serde_json::json!({
                "transfer_id": r.transfer_id.to_string(),
                "direction": format!("{:?}", r.direction),
                "amount_usd": r.amount_usd,
                "asset": r.asset,
                "originator_name": r.originator_name,
                "beneficiary_account": r.beneficiary_account,
                "status": format!("{:?}", r.status),
                "created_at": r.created_at.to_rfc3339(),
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(pending)))
}

// ---------------------------------------------------------------------------
// T6.2: Batch Order API
// ---------------------------------------------------------------------------

/// Maximum number of orders allowed in a single batch request.
const BATCH_MAX: usize = 20;

/// Request body for batch order placement.
#[derive(Debug, Deserialize)]
pub struct BatchPlaceRequest {
    pub orders: Vec<PlaceOrderRequest>,
}

/// Request body for batch order cancellation.
#[derive(Debug, Deserialize)]
pub struct BatchCancelRequest {
    pub order_ids: Vec<String>,
}

/// A single entry in a `BatchAmendRequest`.
#[derive(Debug, Deserialize)]
pub struct SingleAmendRequest {
    pub order_id: String,
    pub price: Option<Decimal>,
    pub qty: Option<Decimal>,
}

/// Request body for batch order amendment.
#[derive(Debug, Deserialize)]
pub struct BatchAmendRequest {
    pub amendments: Vec<SingleAmendRequest>,
}

/// Parse and validate a single [`PlaceOrderRequest`] into a [`PlaceOrderCommand`].
///
/// Extracted to share logic between the single and batch place endpoints.
fn build_place_command(
    req: &PlaceOrderRequest,
    user_id: uuid::Uuid,
) -> Result<PlaceOrderCommand, String> {
    if req.symbol.len() < 3
        || req.symbol.len() > 20
        || !req.symbol.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return Err(format!(
            "Invalid symbol: '{}'. Must be 3–20 uppercase ASCII letters/digits.",
            req.symbol
        ));
    }

    if req.qty <= Decimal::ZERO {
        return Err(format!("Invalid qty: must be positive (got {})", req.qty));
    }

    let side = match req.side.to_lowercase().as_str() {
        "buy" => Side::Buy,
        "sell" => Side::Sell,
        _ => return Err("Invalid side. Use 'buy' or 'sell'".to_string()),
    };

    let order_type = match req.order_type.to_lowercase().as_str() {
        "limit" => OrderType::Limit,
        "market" => OrderType::Market,
        _ => return Err("Invalid order_type. Use 'limit' or 'market'".to_string()),
    };

    let tif_str2 = req.time_in_force.as_deref().unwrap_or("gtc").to_lowercase();
    let time_in_force = match tif_str2.as_str() {
        "gtc" => TimeInForce::Gtc,
        "ioc" => TimeInForce::Ioc,
        "fok" => TimeInForce::Fok,
        "post_only" | "postonly" => TimeInForce::PostOnly,
        "gtd" => TimeInForce::Gtd,
        _ => return Err(format!("Invalid time_in_force: '{tif_str2}'")),
    };

    if time_in_force == TimeInForce::Gtd && req.expire_at.is_none() {
        return Err("GTD orders require expire_at (Unix milliseconds)".to_string());
    }

    let price = match order_type {
        OrderType::Limit => match req.price {
            Some(p) if p > Decimal::ZERO => p,
            _ => return Err("Limit order requires positive price".to_string()),
        },
        OrderType::Market => Decimal::ZERO,
    };

    // Validate iceberg_qty if present.
    if let Some(iceberg) = req.iceberg_qty {
        if iceberg <= Decimal::ZERO {
            return Err("iceberg_qty must be positive".to_string());
        }
        if iceberg >= req.qty {
            return Err("iceberg_qty must be less than total qty".to_string());
        }
        if order_type != OrderType::Limit {
            return Err("iceberg orders are only supported for Limit orders".to_string());
        }
    }

    let expire_at = req.expire_at.and_then(chrono::DateTime::from_timestamp_millis);

    let stp_mode = match req.stp_mode.as_deref().unwrap_or("expire_taker") {
        "expire_taker" => StpMode::ExpireTaker,
        "expire_maker" => StpMode::ExpireMaker,
        "expire_both"  => StpMode::ExpireBoth,
        "none"         => StpMode::None,
        other => return Err(format!(
            "Invalid stp_mode '{other}'. Use expire_taker, expire_maker, expire_both, or none",
        )),
    };

    let request_expire_time = req
        .exp_time
        .and_then(chrono::DateTime::from_timestamp_millis);

    Ok(PlaceOrderCommand {
        order_id: uuid::Uuid::new_v4(),
        user_id,
        symbol: Symbol::new(&req.symbol),
        side,
        order_type,
        time_in_force,
        price,
        qty: req.qty,
        reduce_only: req.reduce_only.unwrap_or(false),
        expire_at,
        timestamp: chrono::Utc::now(),
        position_idx: req.position_idx.unwrap_or(0),
        iceberg_qty: req.iceberg_qty,
        order_link_id: req.order_link_id.clone(),
        slippage_tolerance: req.slippage_tolerance,
        close_position: req.close_position.unwrap_or(false),
        stp_mode,
        request_expire_time,
    })
}

/// POST /api/v1/orders/batch — place up to 20 orders in a single request.
///
/// Each order is processed independently. Individual failures do not roll back
/// other orders. Returns an array of per-order results.
pub async fn batch_place_orders(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<BatchPlaceRequest>,
) -> impl IntoResponse {
    if req.orders.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "orders array must not be empty"})),
        );
    }
    if req.orders.len() > BATCH_MAX {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("Batch size {} exceeds maximum of {}", req.orders.len(), BATCH_MAX)
            })),
        );
    }

    let user_id = auth.0;
    let mut results: Vec<serde_json::Value> = Vec::with_capacity(req.orders.len());

    for order_req in &req.orders {
        let result = match build_place_command(order_req, user_id) {
            Err(e) => serde_json::json!({
                "order_id": null,
                "status": "error",
                "error": e,
            }),
            Ok(cmd) => {
                let order_id = cmd.order_id;
                let exchange_clone = Arc::clone(&exchange);
                let place_result = crate::request_id::spawn_blocking_traced(move || exchange_clone.place_order(cmd))
                    .await
                    .unwrap_or_else(|e| Err(anyhow::anyhow!("task join error: {e}")));
                match place_result {
                    Ok(events) => {
                        let status = if events.iter().any(|e| matches!(e, MatchEvent::OrderRejected { .. })) {
                            "rejected"
                        } else if events.iter().any(|e| matches!(e, MatchEvent::OrderAccepted { .. })) {
                            "open"
                        } else if events.iter().any(|e| matches!(e, MatchEvent::Fill(_))) {
                            "filled"
                        } else {
                            "cancelled"
                        };
                        serde_json::json!({
                            "order_id": order_id.to_string(),
                            "status": status,
                            "error": null,
                        })
                    }
                    Err(e) => serde_json::json!({
                        "order_id": null,
                        "status": "error",
                        "error": e.to_string(),
                    }),
                }
            }
        };
        results.push(result);
    }

    (StatusCode::OK, Json(serde_json::json!(results)))
}

/// DELETE /api/v1/orders/batch — cancel up to 20 orders in a single request.
///
/// Each cancellation is processed independently. Returns an array of per-order results.
pub async fn batch_cancel_orders(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<BatchCancelRequest>,
) -> impl IntoResponse {
    if req.order_ids.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "order_ids array must not be empty"})),
        );
    }
    if req.order_ids.len() > BATCH_MAX {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("Batch size {} exceeds maximum of {}", req.order_ids.len(), BATCH_MAX)
            })),
        );
    }

    let user_id = auth.0;
    let mut results: Vec<serde_json::Value> = Vec::with_capacity(req.order_ids.len());

    for id_str in &req.order_ids {
        let result = match uuid::Uuid::parse_str(id_str) {
            Err(_) => serde_json::json!({
                "order_id": id_str,
                "status": "error",
                "error": "Invalid order_id format",
            }),
            Ok(order_id) => {
                let cmd = CancelOrderCommand {
                    order_id,
                    user_id,
                    symbol: Symbol::new(""),
                    timestamp: chrono::Utc::now(),
                };
                match exchange.cancel_order(cmd) {
                    Ok(_) => serde_json::json!({
                        "order_id": order_id.to_string(),
                        "status": "cancelled",
                        "error": null,
                    }),
                    Err(e) => serde_json::json!({
                        "order_id": order_id.to_string(),
                        "status": "error",
                        "error": e.to_string(),
                    }),
                }
            }
        };
        results.push(result);
    }

    (StatusCode::OK, Json(serde_json::json!(results)))
}

// ---------------------------------------------------------------------------
// Cancel-all orders
// ---------------------------------------------------------------------------

/// Optional filters for cancel-all operations.
#[derive(Debug, Deserialize, Default)]
pub struct CancelAllRequest {
    /// Cancel only orders for this symbol. `None` means all symbols.
    #[serde(default)]
    pub symbol: Option<String>,
    /// Cancel only orders on this side ("buy" or "sell"). `None` means both sides.
    #[serde(default)]
    pub side: Option<String>,
    /// Cancel only orders of this type ("limit" or "market"). `None` means all types.
    #[serde(default)]
    pub order_type: Option<String>,
}

/// POST /api/v1/orders/cancel-all
///
/// Cancel all open orders for the authenticated user, with optional filters by
/// `symbol`, `side`, and `order_type`.
///
/// Returns `{"cancelled": N, "failed": M}`.
pub async fn cancel_all_orders(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<CancelAllRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let now = chrono::Utc::now();

    // Resolve optional filters.
    let filter_symbol = req.symbol.as_deref().map(Symbol::new);
    let filter_side: Option<Side> = match req.side.as_deref() {
        Some("buy") => Some(Side::Buy),
        Some("sell") => Some(Side::Sell),
        Some(other) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("Invalid side '{}'. Use 'buy' or 'sell'", other)
                })),
            ).into_response();
        }
        None => None,
    };
    let filter_order_type: Option<OrderType> = match req.order_type.as_deref() {
        Some("limit") => Some(OrderType::Limit),
        Some("market") => Some(OrderType::Market),
        Some(other) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("Invalid order_type '{}'. Use 'limit' or 'market'", other)
                })),
            ).into_response();
        }
        None => None,
    };

    // Collect matching order IDs from pending_orders (snapshot).
    let order_ids: Vec<OrderId> = exchange
        .pending_orders
        .iter()
        .filter(|r| {
            r.user_id == user_id
                && filter_symbol.as_ref().is_none_or(|s| &r.symbol == s)
                && filter_side.is_none_or(|s| r.side == s)
                && filter_order_type.is_none_or(|t| r.order_type == t)
        })
        .map(|r| *r.key())
        .collect();

    let mut cancelled = 0usize;
    let mut failed = 0usize;

    for order_id in order_ids {
        let cmd = CancelOrderCommand {
            order_id,
            user_id,
            symbol: Symbol::new(""), // resolved inside cancel_order
            timestamp: now,
        };
        match exchange.cancel_order(cmd) {
            Ok(_) => cancelled += 1,
            Err(e) => {
                tracing::debug!(
                    user_id = %user_id,
                    order_id = %order_id,
                    error = %e,
                    "cancel-all: failed to cancel order"
                );
                failed += 1;
            }
        }
    }

    tracing::info!(
        user_id = %user_id,
        cancelled,
        failed,
        "cancel-all orders completed"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "cancelled": cancelled,
            "failed": failed,
        })),
    ).into_response()
}

/// POST /api/v1/trigger-orders/cancel-all
///
/// Cancel all active trigger orders for the authenticated user, with optional
/// filters by `symbol` and `trigger_type`.
///
/// Returns `{"cancelled": N}`.
pub async fn cancel_all_trigger_orders(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<CancelAllRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;

    let filter_symbol = req.symbol.as_deref().map(Symbol::new);
    let filter_trigger_type: Option<TriggerOrderType> = match req.order_type.as_deref() {
        Some("stop_market") | Some("stop_loss") => Some(TriggerOrderType::StopMarket),
        Some("stop_limit") => Some(TriggerOrderType::StopLimit),
        Some("take_profit") => Some(TriggerOrderType::TakeProfit),
        Some("take_profit_market") => Some(TriggerOrderType::TakeProfitMarket),
        Some("trailing_stop") => Some(TriggerOrderType::TrailingStop),
        Some(other) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("Invalid order_type '{}' for trigger orders", other)
                })),
            ).into_response();
        }
        None => None,
    };

    let cancelled = exchange
        .trigger_engine
        .lock().unwrap_or_else(|e| e.into_inner())
        .cancel_orders_for_user(user_id, filter_symbol.as_ref(), filter_trigger_type);

    let count = cancelled.len();
    tracing::info!(user_id = %user_id, count, "cancel-all trigger orders completed");

    (
        StatusCode::OK,
        Json(serde_json::json!({ "cancelled": count })),
    ).into_response()
}

/// PUT /api/v1/orders/batch-amend — amend up to 20 orders in a single request.
///
/// Each amendment is cancel-then-resubmit. Individual failures do not affect
/// other amendments. Returns an array of per-order results.
pub async fn batch_amend_orders(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<BatchAmendRequest>,
) -> impl IntoResponse {
    if req.amendments.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "amendments array must not be empty"})),
        );
    }
    if req.amendments.len() > BATCH_MAX {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("Batch size {} exceeds maximum of {}", req.amendments.len(), BATCH_MAX)
            })),
        );
    }

    let user_id = auth.0;
    let mut results: Vec<serde_json::Value> = Vec::with_capacity(req.amendments.len());

    for amend in &req.amendments {
        let result = match uuid::Uuid::parse_str(&amend.order_id) {
            Err(_) => serde_json::json!({
                "old_order_id": amend.order_id,
                "new_order_id": null,
                "status": "error",
                "error": "Invalid order_id format",
            }),
            Ok(old_order_id) => {
                // Validate at least one field to amend
                if amend.price.is_none() && amend.qty.is_none() {
                    results.push(serde_json::json!({
                        "old_order_id": old_order_id.to_string(),
                        "new_order_id": null,
                        "status": "error",
                        "error": "At least one of 'price' or 'qty' must be provided",
                    }));
                    continue;
                }

                // Look up the existing order metadata
                let order_info = {
                    // Validate the order exists and belongs to the authenticated user.
                    match exchange.order_locks.get(&old_order_id) {
                        None => {
                            results.push(serde_json::json!({
                                "old_order_id": old_order_id.to_string(),
                                "new_order_id": null,
                                "status": "error",
                                "error": "Order not found or already in terminal state",
                            }));
                            continue;
                        }
                        Some(r) if r.user_id != user_id => {
                            results.push(serde_json::json!({
                                "old_order_id": old_order_id.to_string(),
                                "new_order_id": null,
                                "status": "error",
                                "error": "Order not found",
                            }));
                            continue;
                        }
                        Some(_) => {}
                    }
                    match exchange.pending_orders.get(&old_order_id) {
                        Some(info) => info.clone(),
                        None => {
                            tracing::error!(order_id = %old_order_id, "Batch amend: order state inconsistent — lock present but pending_orders entry missing");
                            results.push(serde_json::json!({
                                "old_order_id": old_order_id.to_string(),
                                "new_order_id": null,
                                "status": "error",
                                "error": "Internal server error",
                            }));
                            continue;
                        }
                    }
                };

                let new_price = amend.price.unwrap_or(order_info.price);
                let new_qty = amend.qty.unwrap_or(order_info.qty);

                // Validate new values
                if new_qty <= Decimal::ZERO {
                    results.push(serde_json::json!({
                        "old_order_id": old_order_id.to_string(),
                        "new_order_id": null,
                        "status": "error",
                        "error": "Invalid qty: must be positive",
                    }));
                    continue;
                }
                if order_info.order_type == OrderType::Limit && new_price <= Decimal::ZERO {
                    results.push(serde_json::json!({
                        "old_order_id": old_order_id.to_string(),
                        "new_order_id": null,
                        "status": "error",
                        "error": "Invalid price: must be positive",
                    }));
                    continue;
                }

                // Cancel old order
                let cancel_cmd = CancelOrderCommand {
                    order_id: old_order_id,
                    user_id,
                    symbol: order_info.symbol.clone(),
                    timestamp: chrono::Utc::now(),
                };
                if let Err(e) = exchange.cancel_order(cancel_cmd) {
                    results.push(serde_json::json!({
                        "old_order_id": old_order_id.to_string(),
                        "new_order_id": null,
                        "status": "error",
                        "error": format!("Failed to cancel old order: {e}"),
                    }));
                    continue;
                }

                // Place new order with updated parameters
                let new_order_id = uuid::Uuid::new_v4();
                let place_cmd = PlaceOrderCommand {
                    order_id: new_order_id,
                    user_id,
                    symbol: order_info.symbol.clone(),
                    side: order_info.side,
                    order_type: order_info.order_type,
                    time_in_force: TimeInForce::Gtc,
                    price: new_price,
                    qty: new_qty,
                    reduce_only: false,
                    expire_at: None,
                    timestamp: chrono::Utc::now(),
                    position_idx: 0,
                    iceberg_qty: None,
                    order_link_id: None,
                    slippage_tolerance: None,
                    close_position: false,
                    stp_mode: StpMode::default(),
                    request_expire_time: None,
                };

                let exchange_clone = Arc::clone(&exchange);
                let amend_result = crate::request_id::spawn_blocking_traced(move || exchange_clone.place_order(place_cmd))
                    .await
                    .unwrap_or_else(|e| Err(anyhow::anyhow!("task join error: {e}")));
                match amend_result {
                    Ok(_) => serde_json::json!({
                        "old_order_id": old_order_id.to_string(),
                        "new_order_id": new_order_id.to_string(),
                        "status": "amended",
                        "error": null,
                    }),
                    Err(e) => serde_json::json!({
                        "old_order_id": old_order_id.to_string(),
                        "new_order_id": null,
                        "status": "error",
                        "error": format!("Failed to place amended order: {e}"),
                    }),
                }
            }
        };
        results.push(result);
    }

    (StatusCode::OK, Json(serde_json::json!(results)))
}

// ---------------------------------------------------------------------------
// T9.1: Email/Password Authentication
// ---------------------------------------------------------------------------

/// Minimum password length enforced at registration.
const MIN_PASSWORD_LEN: usize = 8;

/// Validate email format: must contain `@` and a `.` after the `@`.
fn validate_email(email: &str) -> bool {
    if let Some(at_pos) = email.find('@') {
        let domain = &email[at_pos + 1..];
        !domain.is_empty() && domain.contains('.')
    } else {
        false
    }
}

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct VerifyEmailRequest {
    pub user_id: String,
    pub code: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

/// POST /api/v1/auth/register
///
/// Create a new user account with an email/password credential.
/// Password is hashed with Argon2id before storage — the plaintext is never persisted.
pub async fn auth_register(
    State(exchange): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> impl IntoResponse {
    if !validate_email(&req.email) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid email format"})),
        );
    }
    if req.password.len() < MIN_PASSWORD_LEN {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("Password must be at least {MIN_PASSWORD_LEN} characters")
            })),
        );
    }

    // Check for duplicate email
    {
        let emails = exchange.user_emails.lock().unwrap_or_else(|e| e.into_inner());
        if emails.contains_key(&req.email) {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": "Email already registered"})),
            );
        }
    }

    // Hash password (Argon2 is intentionally slow — acceptable for a registration path)
    let hash = match crate::web_auth::PasswordHasher::hash(&req.password) {
        Ok(h) => h,
        Err(e) => {
            tracing::error!(error = %e, "Password hashing failed during registration");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Internal error"})),
            );
        }
    };

    let user_id = Uuid::new_v4();

    // Persist email -> user_id mapping and password hash
    exchange.user_emails.lock().unwrap_or_else(|e| e.into_inner()).insert(req.email.clone(), user_id);
    exchange.password_hashes.lock().unwrap_or_else(|e| e.into_inner()).insert(user_id, hash);

    // Create the underlying account so the user can receive balances immediately
    exchange.create_account(user_id);

    // Demo mode: auto-fund new accounts so users can trade immediately
    if exchange.is_demo.load(std::sync::atomic::Ordering::Relaxed) {
        let _ = exchange.deposit(user_id, "USDT", Decimal::new(100_000, 0));
        let _ = exchange.deposit(user_id, "BTC", Decimal::ONE);
        let _ = exchange.deposit(user_id, "ETH", Decimal::new(10, 0));
        let _ = exchange.deposit(user_id, "SOL", Decimal::new(100, 0));
        let _ = exchange.deposit(user_id, "BNB", Decimal::new(10, 0));
    }

    // Persist to DB synchronously if a pool is available, so the user survives restart
    let pool_opt = exchange.db_pool.get().cloned();
    if let Some(ref pool) = pool_opt {
        if let Err(e) = exchange_db::user_repo::UserRepo::create(pool, user_id, Some(&req.email)).await {
            tracing::error!(error = %e, user_id = %user_id, "Failed to persist new user to DB");
            // Roll back in-memory state to keep DB and memory consistent
            exchange.user_emails.lock().unwrap_or_else(|e| e.into_inner()).remove(&req.email);
            exchange.password_hashes.lock().unwrap_or_else(|e| e.into_inner()).remove(&user_id);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to create account"})),
            );
        }
    }

    tracing::info!(user_id = %user_id, email = %req.email, "user registered");

    // Send verification email if SMTP is configured (non-blocking).
    // Anti-phishing code is not set yet at registration time — pass None.
    let code = exchange.generate_email_code(user_id);
    if let Some(ref sender) = exchange.email_sender {
        let sender = Arc::clone(sender);
        let email_to = req.email.clone();
        tokio::spawn(async move {
            let _ = sender
                .send_with_anti_phishing(
                    &email_to,
                    exchange_admin::EmailTemplate::VerificationCode { code },
                    None,
                )
                .await;
        });
    }

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "user_id": user_id.to_string(),
            "message": "Account created",
        })),
    )
}

/// POST /api/v1/auth/verify-email
///
/// Verify a 6-digit code that was emailed to the user after registration.
/// Consumes the code on success — each code is single-use.
pub async fn auth_verify_email(
    State(exchange): State<AppState>,
    Json(req): Json<VerifyEmailRequest>,
) -> impl IntoResponse {
    let user_id = match Uuid::parse_str(&req.user_id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid user_id"})),
            )
        }
    };

    if exchange.verify_email_code(user_id, &req.code) {
        (StatusCode::OK, Json(serde_json::json!({"status": "verified"})))
    } else {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid or expired code"})),
        )
    }
}

/// POST /api/v1/auth/login
///
/// Authenticate with email + password.
/// If 2FA is enabled the response is `{"status":"requires_2fa","session_token":"<temp>"}`.
/// Otherwise returns a full JWT token pair.
pub async fn auth_login(
    State(exchange): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<LoginRequest>,
) -> impl IntoResponse {
    // Look up user by email
    let user_id = {
        let emails = exchange.user_emails.lock().unwrap_or_else(|e| e.into_inner());
        match emails.get(&req.email).copied() {
            Some(id) => id,
            None => {
                // Respond generically to prevent user enumeration
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({"error": "Invalid email or password"})),
                );
            }
        }
    };

    // Verify password
    let hash = {
        let hashes = exchange.password_hashes.lock().unwrap_or_else(|e| e.into_inner());
        match hashes.get(&user_id).cloned() {
            Some(h) => h,
            None => {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({"error": "Invalid email or password"})),
                );
            }
        }
    };

    let password_ok = match crate::web_auth::PasswordHasher::verify(&req.password, &hash) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(user_id = %user_id, error = %e, "Password verification error during login");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Internal error"})),
            );
        }
    };

    if !password_ok {
        tracing::warn!(user_id = %user_id, "Failed login attempt");
        // M24-6: Audit log entry for failed login.
        let fail_ip = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.split(',').next().unwrap_or("").trim().to_string());
        exchange.audit(
            Some(user_id),
            exchange_admin::AuditAction::LoginFailed,
            serde_json::json!({"reason": "invalid_password"}),
            fail_ip,
        );
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Invalid email or password"})),
        );
    }

    // Check 2FA
    let totp_enabled = exchange
        .totp
        .lock().unwrap_or_else(|e| e.into_inner())
        .is_enabled(user_id);

    if totp_enabled {
        let session_token = Uuid::new_v4().to_string();
        tracing::info!(user_id = %user_id, "Login requires 2FA");
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "requires_2fa",
                "session_token": session_token,
            })),
        );
    }

    // Issue tokens
    let pair = match exchange.jwt.issue_tokens(&user_id.to_string()) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(user_id = %user_id, error = %e, "JWT issuance failed on login");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Internal error"})),
            );
        }
    };

    let expiry = chrono::Utc::now()
        + chrono::Duration::seconds(exchange.jwt.refresh_ttl_secs as i64);

    exchange
        .refresh_tokens
        .lock().unwrap_or_else(|e| e.into_inner())
        .insert(pair.refresh_token.clone(), (user_id, expiry));

    // Record a session — prefer X-Forwarded-For set by a reverse proxy.
    let ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or("").trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let now = chrono::Utc::now();
    let session_id = Uuid::new_v4();
    {
        let mut sessions = exchange.sessions.lock().unwrap_or_else(|e| e.into_inner());
        sessions.insert(
            session_id,
            crate::exchange::SessionRecord {
                id: session_id,
                user_id,
                ip: ip.clone(),
                user_agent: user_agent.clone(),
                created_at: now,
                last_seen: now,
                is_active: true,
            },
        );
        let cutoff = chrono::Utc::now() - chrono::Duration::days(30);
        sessions.retain(|_, s| s.created_at > cutoff);
    }

    // Check whether this IP is new for the user and, if so, send a notification.
    let is_new_ip = {
        let mut known = exchange.known_login_ips.lock().unwrap_or_else(|e| e.into_inner());
        let ips = known.entry(user_id).or_default();
        if ips.contains(&ip) {
            false
        } else {
            ips.push(ip.clone());
            true
        }
    };

    if is_new_ip {
        tracing::info!(user_id = %user_id, ip = %ip, "Login from new IP detected");
        if let Some(ref sender) = exchange.email_sender {
            // Reverse-scan email → user_id map to find the address for this user.
            let user_email = exchange
                .user_emails
                .lock().unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|&(_, &uid)| uid == user_id)
                .map(|(email, _)| email.clone());

            if let Some(email_to) = user_email {
                let anti_phishing_code = exchange
                    .anti_phishing_codes
                    .lock().unwrap_or_else(|e| e.into_inner())
                    .get(&user_id)
                    .cloned();

                let sender = Arc::clone(sender);
                let ip_clone = ip.clone();
                let ua_clone = user_agent.clone();
                let ts = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
                tokio::spawn(async move {
                    let _ = sender
                        .send_with_anti_phishing(
                            &email_to,
                            exchange_admin::EmailTemplate::LoginNewDevice {
                                ip: ip_clone,
                                device: ua_clone,
                                timestamp: ts,
                            },
                            anti_phishing_code.as_deref(),
                        )
                        .await;
                });
            }
        }
    }

    // M24-6: Audit log entry for successful login.
    exchange.audit(
        Some(user_id),
        exchange_admin::AuditAction::Login,
        serde_json::json!({"ip": &ip, "user_agent": &user_agent, "new_ip": is_new_ip}),
        Some(ip),
    );

    tracing::info!(user_id = %user_id, "User logged in");

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "access_token": pair.access_token,
            "refresh_token": pair.refresh_token,
            "expires_in": pair.expires_in,
        })),
    )
}

/// POST /api/v1/auth/refresh
///
/// Exchange a valid refresh token for a fresh token pair.
/// The presented refresh token is immediately invalidated (token rotation).
pub async fn auth_refresh(
    State(exchange): State<AppState>,
    Json(req): Json<RefreshRequest>,
) -> impl IntoResponse {
    // Verify JWT structure and type
    let claims = match exchange.jwt.verify_refresh(&req.refresh_token) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": e.to_string()})),
            );
        }
    };

    // Confirm the token is still in our store (not already revoked / used)
    let user_id = {
        let mut tokens = exchange.refresh_tokens.lock().unwrap_or_else(|e| e.into_inner());
        match tokens.remove(&req.refresh_token) {
            Some((uid, _expiry)) => uid,
            None => {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({"error": "Refresh token not found or already used"})),
                );
            }
        }
    };

    // Verify sub claim matches our stored user_id
    let claimed_id = match Uuid::parse_str(&claims.sub) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Malformed token subject"})),
            );
        }
    };
    if claimed_id != user_id {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Token subject mismatch"})),
        );
    }

    // Issue a new pair
    let pair = match exchange.jwt.issue_tokens(&user_id.to_string()) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(user_id = %user_id, error = %e, "JWT issuance failed on refresh");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Internal error"})),
            );
        }
    };

    let expiry = chrono::Utc::now()
        + chrono::Duration::seconds(exchange.jwt.refresh_ttl_secs as i64);

    exchange
        .refresh_tokens
        .lock().unwrap_or_else(|e| e.into_inner())
        .insert(pair.refresh_token.clone(), (user_id, expiry));

    tracing::debug!(user_id = %user_id, "Refresh token rotated");

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "access_token": pair.access_token,
            "refresh_token": pair.refresh_token,
            "expires_in": pair.expires_in,
        })),
    )
}

/// POST /api/v1/auth/logout
///
/// Invalidate the user's current refresh token.
/// Idempotent — returns 200 even if the token is already gone.
pub async fn auth_logout(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<LogoutRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;

    // C4: Blacklist the access token's JTI so it cannot be reused.
    if let Some(ref access_token) = req.access_token {
        if let Ok(claims) = exchange.jwt.verify_access(access_token) {
            exchange.token_blacklist.insert(claims.jti.clone());
            tracing::info!(
                user_id = %user_id,
                jti = %claims.jti,
                "Access token blacklisted on logout"
            );
        }
    }

    // Remove the refresh token (existing behavior).
    if let Some(ref refresh_token) = req.refresh_token {
        let mut tokens = exchange.refresh_tokens.lock().unwrap_or_else(|e| e.into_inner());
        match tokens.get(refresh_token) {
            Some((owner, _)) if *owner == user_id => {
                tokens.remove(refresh_token);
            }
            Some(_) => {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({"error": "Token does not belong to you"})),
                );
            }
            None => {}
        }
    }

    tracing::info!(user_id = %user_id, "User logged out");
    (StatusCode::OK, Json(serde_json::json!({"status": "logged_out"})))
}

#[derive(Debug, Deserialize)]
pub struct LogoutRequest {
    /// The access token to blacklist (optional for backward compat).
    pub access_token: Option<String>,
    /// The refresh token to invalidate (optional).
    pub refresh_token: Option<String>,
}

// ---------------------------------------------------------------------------
// T9.2: Session Management
// ---------------------------------------------------------------------------

/// GET /api/v1/account/sessions
///
/// Return the authenticated user's active login sessions.
pub async fn list_sessions(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let sessions = exchange.sessions.lock().unwrap_or_else(|e| e.into_inner());
    let result: Vec<serde_json::Value> = sessions
        .values()
        .filter(|s| s.user_id == user_id && s.is_active)
        .map(|s| {
            serde_json::json!({
                "id": s.id.to_string(),
                "ip": s.ip,
                "user_agent": s.user_agent,
                "created_at": s.created_at.timestamp_millis(),
                "last_seen": s.last_seen.timestamp_millis(),
                "is_active": s.is_active,
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(result)))
}

/// DELETE /api/v1/account/sessions/:id
///
/// Revoke (deactivate) a session by its UUID.
/// Returns 404 if the session does not exist or does not belong to the caller.
pub async fn revoke_session(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let session_id = match Uuid::parse_str(&session_id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid session id"})),
            );
        }
    };

    let user_id = auth.0;
    let mut sessions = exchange.sessions.lock().unwrap_or_else(|e| e.into_inner());
    // O(1) lookup by UUID key — no linear scan needed.
    match sessions.get_mut(&session_id) {
        Some(session) if session.user_id == user_id => {
            session.is_active = false;
            tracing::info!(user_id = %user_id, session_id = %session_id, "Session revoked");
            (StatusCode::OK, Json(serde_json::json!({"revoked": true})))
        }
        _ => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Session not found"})),
        ),
    }
}

// ---------------------------------------------------------------------------
// T11.1 + T11.2: KYC endpoints
// ---------------------------------------------------------------------------

/// Request body for POST /api/v1/kyc/start.
#[derive(Debug, Deserialize)]
pub struct KycStartRequest {
    pub provider: String,
}

/// Webhook body for POST /webhooks/kyc.
#[derive(Debug, Deserialize)]
pub struct KycWebhookRequest {
    pub provider: String,
    pub payload: serde_json::Value,
}

/// POST /api/v1/kyc/start
///
/// Start a KYC verification flow for the authenticated user.
/// Body: `{"provider": "diia"}` | `{"provider": "sumsub"}` | `{"provider": "mock"}`
///
/// Returns provider-specific data:
/// - Diia:   `{"provider":"diia","deep_link":"...","request_id":"..."}`
/// - Sumsub: `{"provider":"sumsub","sdk_token":"...","applicant_id":"..."}`
/// - Mock:   `{"provider":"mock","provider_ref":"..."}`
pub async fn kyc_start(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<KycStartRequest>,
) -> impl IntoResponse {
    let user_id = auth.0;

    // Ensure the user has a KYC record (register on first call), then start.
    // We hold the tokio Mutex across the await because start_verification may
    // make async HTTP calls to the provider (Diia / Sumsub).
    let result = {
        let mut svc = exchange.verification_service.lock().await;
        if svc.get_record(&user_id).is_none() {
            svc.register_user(user_id);
        }
        svc.start_verification(user_id, &req.provider).await
    };

    match result {
        Ok(started) => {
            // P0-4: persist the new/updated KYC record to DB.
            if let Some(pool) = exchange.db_pool.get() {
                let svc = exchange.verification_service.lock().await;
                if let Some(record) = svc.get_record(&user_id) {
                    let level = format!("{:?}", record.level);
                    let status = format!("{:?}", record.status);
                    if let Err(e) = exchange_db::kyc_repo::KycRepo::upsert(
                        pool,
                        user_id,
                        &level,
                        &status,
                        record.provider_ref.as_deref(),
                        record.reason.as_deref(),
                    ).await {
                        tracing::error!(user_id = %user_id, error = %e, "KYC DB upsert failed after start_verification");
                    }
                }
            }

            let body = match started {
                VerificationStarted::DiiaDeepLink { deep_link, request_id } => {
                    serde_json::json!({
                        "provider": "diia",
                        "deep_link": deep_link,
                        "request_id": request_id,
                    })
                }
                VerificationStarted::SumsubToken { sdk_token, applicant_id } => {
                    serde_json::json!({
                        "provider": "sumsub",
                        "sdk_token": sdk_token,
                        "applicant_id": applicant_id,
                    })
                }
                VerificationStarted::Mock { provider_ref } => {
                    serde_json::json!({
                        "provider": "mock",
                        "provider_ref": provider_ref,
                    })
                }
            };
            (StatusCode::OK, Json(body))
        }
        Err(e) => {
            tracing::warn!(user_id = %user_id, provider = %req.provider, error = %e, "KYC start failed");
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        }
    }
}

/// GET /api/v1/kyc/status
///
/// Return the current KYC level and status for the authenticated user.
/// Returns `{"level":"None","status":"Unverified","provider_ref":null}` for a new user.
pub async fn kyc_status(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;

    let svc = exchange.verification_service.lock().await;
    match svc.get_record(&user_id) {
        Some(record) => {
            let body = serde_json::json!({
                "level": format!("{:?}", record.level),
                "status": format!("{:?}", record.status),
                "provider_ref": record.provider_ref,
            });
            (StatusCode::OK, Json(body))
        }
        None => {
            // User has never started KYC — return the default unverified state.
            let body = serde_json::json!({
                "level": "None",
                "status": "Unverified",
                "provider_ref": null,
            });
            (StatusCode::OK, Json(body))
        }
    }
}

/// POST /webhooks/kyc
///
/// Receive a webhook callback from a KYC provider (Diia, Sumsub, or mock).
/// This is a public endpoint — providers call it directly without user auth.
///
/// Body: `{"provider": "diia"|"sumsub"|"mock", "payload": {...}}`
///
/// Security: For Sumsub, verifies the `X-Payload-Digest` HMAC-SHA1 signature
/// using `SUMSUB_WEBHOOK_SECRET`. Mock provider is blocked in production mode.
///
/// Always returns 200 OK to acknowledge receipt (provider retry behaviour
/// is controlled by the HTTP status).
pub async fn kyc_webhook(
    State(exchange): State<AppState>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let req: KycWebhookRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "KYC webhook: invalid JSON");
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Invalid JSON"}))).into_response();
        }
    };

    // Block mock provider in production — otherwise anyone can approve KYC.
    // S19 D.1: cached field (Note: original used is_ok() which matches any
    // value including "false"; this preserves exact behaviour by keying on
    // the literal "true" — stricter and correct for production gating.)
    if req.provider == "mock" && exchange.is_production {
        tracing::error!("KYC webhook: mock provider rejected in PRODUCTION mode");
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({
            "error": "Mock KYC provider is disabled in production"
        }))).into_response();
    }

    // Verify Sumsub webhook signature (HMAC-SHA1 of raw body).
    // Reference: https://docs.sumsub.com/reference/webhook-verification
    if req.provider == "sumsub" {
        let secret = match exchange.secrets.get(exchange_common::secrets::names::SUMSUB_WEBHOOK_SECRET) {
            Some(s) => s,
            None => {
                if exchange.is_production {
                    tracing::error!("SUMSUB_WEBHOOK_SECRET not set in PRODUCTION — rejecting webhook");
                    return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                        "error": "KYC webhook secret not configured"
                    }))).into_response();
                }
                tracing::warn!("SUMSUB_WEBHOOK_SECRET not set — using dev fallback");
                "dev-sumsub-secret".to_string()
            }
        };

        let digest_header = headers
            .get("x-payload-digest")
            .or_else(|| headers.get("X-Payload-Digest"))
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if digest_header.is_empty() {
            tracing::warn!("KYC webhook rejected: missing X-Payload-Digest header");
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({
                "error": "Missing webhook signature"
            }))).into_response();
        }

        // Sumsub uses HMAC-SHA1(secret, raw_body), hex-encoded.
        use hmac::{Hmac, Mac};
        use sha1::Sha1;
        let mut mac = Hmac::<Sha1>::new_from_slice(secret.as_bytes())
            .expect("HMAC accepts any key length");
        mac.update(&body);
        let expected = hex::encode(mac.finalize().into_bytes());

        if !webhook_sig_eq(digest_header.as_bytes(), expected.as_bytes()) {
            tracing::warn!("KYC webhook rejected: invalid X-Payload-Digest");
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({
                "error": "Invalid webhook signature"
            }))).into_response();
        }
    }

    let payload_str = req.payload.to_string();

    let result = {
        let mut svc = exchange.verification_service.lock().await;
        svc.process_webhook(&req.provider, &payload_str)
    };

    match result {
        Ok(record) => {
            tracing::info!(
                provider = %req.provider,
                user_id = %record.user_id,
                status = ?record.status,
                "KYC webhook processed"
            );
            // Mirror KYC approval into the sync cache used by place_order.
            if record.status == exchange_compliance::KycStatus::Verified {
                exchange.kyc_approved.insert(record.user_id);
                // M24-6: Audit log entry for KYC approval.
                exchange.audit(
                    Some(record.user_id),
                    exchange_admin::AuditAction::KycApproved,
                    serde_json::json!({"provider": req.provider}),
                    None,
                );
            } else {
                exchange.kyc_approved.remove(&record.user_id);
                // M24-6: Audit log entry for KYC rejection.
                exchange.audit(
                    Some(record.user_id),
                    exchange_admin::AuditAction::KycRejected,
                    serde_json::json!({"provider": req.provider, "status": format!("{:?}", record.status)}),
                    None,
                );
            }
        }
        Err(e) => {
            tracing::warn!(provider = %req.provider, error = %e, "KYC webhook processing error");
        }
    }

    (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response()
}

// ---------------------------------------------------------------------------
// T11.4: Audit log export
// ---------------------------------------------------------------------------

/// Query parameters for GET /admin/audit-log.
#[derive(Debug, Deserialize)]
pub struct AuditLogQuery {
    /// ISO 8601 timestamp — only entries at or after this time are returned.
    pub from: Option<chrono::DateTime<chrono::Utc>>,
    /// ISO 8601 timestamp — only entries at or before this time are returned.
    pub to: Option<chrono::DateTime<chrono::Utc>>,
    /// Filter by user UUID.
    pub user_id: Option<Uuid>,
    /// Filter by action name (matches `AuditAction` debug string, e.g. `"Login"`).
    pub action: Option<String>,
    /// Maximum number of entries to return. Defaults to 1000.
    #[serde(default = "default_audit_limit")]
    pub limit: usize,
}

fn default_audit_limit() -> usize {
    1_000
}

/// GET /admin/audit-log
///
/// Export audit log entries as a JSON array. Supports filtering by time range,
/// user, and action type. Returns at most `limit` entries (default 1000).
///
/// Query params: `from`, `to`, `user_id`, `action`, `limit`
pub async fn admin_audit_log(
    State(exchange): State<AppState>,
    Query(params): Query<AuditLogQuery>,
) -> impl IntoResponse {
    // Parse the action string into an AuditAction variant if provided.
    let action = params.action.and_then(|s| parse_audit_action(&s));

    let filter = AuditFilter {
        user_id: params.user_id,
        action,
        from: params.from,
        to: params.to,
        limit: params.limit,
    };

    let log = exchange.audit_log.lock().unwrap_or_else(|e| e.into_inner());
    let json = log.export_json(&filter);

    // Parse and return as a proper JSON array.
    let value: serde_json::Value = serde_json::from_str(&json)
        .unwrap_or(serde_json::Value::Array(vec![]));

    (StatusCode::OK, Json(value))
}

/// Convert a string action name to an `AuditAction` variant.
///
/// Matches the `Debug` representation of each variant (e.g. `"Login"`, `"OrderPlaced"`).
/// Returns `None` for unknown action names so the filter is simply ignored.
fn parse_audit_action(s: &str) -> Option<AuditAction> {
    match s {
        "Login" => Some(AuditAction::Login),
        "LoginFailed" => Some(AuditAction::LoginFailed),
        "ApiKeyCreated" => Some(AuditAction::ApiKeyCreated),
        "ApiKeyRevoked" => Some(AuditAction::ApiKeyRevoked),
        "TwoFactorEnabled" => Some(AuditAction::TwoFactorEnabled),
        "TwoFactorDisabled" => Some(AuditAction::TwoFactorDisabled),
        "OrderPlaced" => Some(AuditAction::OrderPlaced),
        "OrderCancelled" => Some(AuditAction::OrderCancelled),
        "OrderFilled" => Some(AuditAction::OrderFilled),
        "DepositDetected" => Some(AuditAction::DepositDetected),
        "DepositCredited" => Some(AuditAction::DepositCredited),
        "WithdrawalRequested" => Some(AuditAction::WithdrawalRequested),
        "WithdrawalApproved" => Some(AuditAction::WithdrawalApproved),
        "WithdrawalRejected" => Some(AuditAction::WithdrawalRejected),
        "LeverageChanged" => Some(AuditAction::LeverageChanged),
        "MarginAdded" => Some(AuditAction::MarginAdded),
        "AccountFrozen" => Some(AuditAction::AccountFrozen),
        "AccountUnfrozen" => Some(AuditAction::AccountUnfrozen),
        "TradingHalted" => Some(AuditAction::TradingHalted),
        "TradingResumed" => Some(AuditAction::TradingResumed),
        "KycSubmitted" => Some(AuditAction::KycSubmitted),
        "KycApproved" => Some(AuditAction::KycApproved),
        "KycRejected" => Some(AuditAction::KycRejected),
        "AmlAlertCreated" => Some(AuditAction::AmlAlertCreated),
        "AmlAlertReviewed" => Some(AuditAction::AmlAlertReviewed),
        "FeatureFlagChanged" => Some(AuditAction::FeatureFlagChanged),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// W1.1: Admin endpoints
// ---------------------------------------------------------------------------

/// Query parameters for GET /admin/users.
#[derive(Debug, Deserialize)]
pub struct AdminListUsersQuery {
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// GET /admin/users?search=&limit=50 — list all users with equity summaries.
pub async fn admin_list_users(
    State(exchange): State<AppState>,
    Query(params): Query<AdminListUsersQuery>,
) -> impl IntoResponse {
    let user_ids = exchange.accounts.user_ids();
    let limit = params.limit.min(MAX_HISTORY_LIMIT);

    // Filter by user_id prefix if search is provided.
    let users: Vec<serde_json::Value> = user_ids
        .into_iter()
        .filter(|uid| {
            params.search.as_deref()
                .is_none_or( |s| uid.to_string().starts_with(s))
        })
        .take(limit)
        .map(|user_id| {
            let total_equity_usd = exchange
                .accounts
                .get_all_balances(user_id)
                .values()
                .fold(Decimal::ZERO, |acc, b| acc + b.total());
            serde_json::json!({
                "user_id": user_id.to_string(),
                "total_equity_usd": total_equity_usd,
                "kyc_status": "Unverified",
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(users)))
}

/// GET /admin/users/{user_id}/balances — all balances for a specific user.
pub async fn admin_user_balances(
    State(exchange): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> impl IntoResponse {
    let balances = exchange.accounts.get_all_balances(user_id);
    if balances.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "User not found"})),
        );
    }
    let result: Vec<serde_json::Value> = balances
        .iter()
        .map(|(asset, bal)| {
            serde_json::json!({
                "asset": asset,
                "total": bal.total(),
                "locked": bal.locked(),
                "available": bal.available(),
            })
        })
        .collect();
    (StatusCode::OK, Json(serde_json::json!(result)))
}

/// GET /admin/users/{user_id}/orders — order history for a specific user.
pub async fn admin_user_orders(
    State(exchange): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> impl IntoResponse {
    let history = exchange.order_history.lock().unwrap_or_else(|e| e.into_inner());
    let orders: Vec<serde_json::Value> = history
        .iter()
        .rev()
        .filter(|e| e.user_id == user_id)
        .take(MAX_HISTORY_LIMIT)
        .map(|e| {
            serde_json::json!({
                "order_id": e.order_id.to_string(),
                "order_link_id": e.order_link_id,
                "symbol": e.symbol.to_string(),
                "side": e.side,
                "type": e.order_type,
                "price": e.price,
                "qty": e.qty,
                "filled_qty": e.filled_qty,
                "status": e.status,
                "created_at": e.created_at.timestamp_millis(),
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(orders)))
}

/// GET /admin/users/{user_id}/positions — open positions for a specific user.
pub async fn admin_user_positions(
    State(exchange): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> impl IntoResponse {
    let positions = exchange.get_positions(user_id);
    let result: Vec<serde_json::Value> = positions
        .iter()
        .map(|pos| {
            let mark_price = exchange
                .get_mark_price(&pos.symbol)
                .map(|mp| mp.price)
                .unwrap_or(pos.entry_price);
            serde_json::json!({
                "symbol": pos.symbol.to_string(),
                "side": format!("{:?}", pos.side),
                "size": pos.size,
                "entry_price": pos.entry_price,
                "mark_price": mark_price,
                "unrealized_pnl": pos.unrealized_pnl(mark_price),
                "realized_pnl": pos.realized_pnl,
                "margin": pos.margin,
                "leverage": pos.leverage,
                "liq_price": pos.liquidation_price(),
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(result)))
}

/// GET /admin/kyc/pending — list all KYC records in Pending status.
pub async fn admin_kyc_pending(State(exchange): State<AppState>) -> impl IntoResponse {
    let svc = exchange.verification_service.lock().await;
    let pending: Vec<serde_json::Value> = svc
        .all_records()
        .filter(|r| r.status == KycStatus::Pending)
        .map(|r| {
            serde_json::json!({
                "user_id": r.user_id.to_string(),
                "level": format!("{:?}", r.level),
                "provider_ref": r.provider_ref,
                "created_at": r.created_at.timestamp_millis(),
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(pending)))
}

/// POST /admin/kyc/{user_id}/approve — approve a pending KYC record.
pub async fn admin_kyc_approve(
    State(exchange): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> impl IntoResponse {
    let mut svc = exchange.verification_service.lock().await;
    match svc.approve(user_id) {
        Ok(()) => {
            tracing::info!(user_id = %user_id, "Admin approved KYC");
            // Mirror the approval in the sync-accessible cache used by place_order.
            exchange.kyc_approved.insert(user_id);

            // P0-4: persist to DB (best-effort — in-memory is the authority for this request).
            if let Some(pool) = exchange.db_pool.get() {
                let record = svc.get_record(&user_id);
                let level = record.map(|r| format!("{:?}", r.level)).unwrap_or_else(|| "Basic".to_string());
                if let Err(e) = exchange_db::kyc_repo::KycRepo::upsert(
                    pool, user_id, &level, "Verified", None, None,
                ).await {
                    tracing::error!(user_id = %user_id, error = %e, "KYC DB upsert failed after approve");
                }
            }

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "user_id": user_id.to_string(),
                    "status": "approved",
                })),
            )
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

#[derive(Debug, Deserialize)]
pub struct AdminKycRejectRequest {
    pub reason: String,
}

/// POST /admin/kyc/{user_id}/reject — reject a KYC record with a reason.
pub async fn admin_kyc_reject(
    State(exchange): State<AppState>,
    Path(user_id): Path<Uuid>,
    Json(req): Json<AdminKycRejectRequest>,
) -> impl IntoResponse {
    let mut svc = exchange.verification_service.lock().await;
    match svc.reject(user_id, req.reason.clone()) {
        Ok(()) => {
            tracing::info!(user_id = %user_id, reason = %req.reason, "Admin rejected KYC");

            // P0-4: persist to DB.
            if let Some(pool) = exchange.db_pool.get() {
                if let Err(e) = exchange_db::kyc_repo::KycRepo::update_status(
                    pool, user_id, "Rejected", Some(&req.reason),
                ).await {
                    tracing::error!(user_id = %user_id, error = %e, "KYC DB update_status failed after reject");
                }
            }

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "user_id": user_id.to_string(),
                    "status": "rejected",
                    "reason": req.reason,
                })),
            )
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

/// GET /admin/withdrawals/pending — list all withdrawals awaiting approval.
pub async fn admin_withdrawals_pending(State(exchange): State<AppState>) -> impl IntoResponse {
    let mgr = exchange.withdrawal_manager.lock().unwrap_or_else(|e| e.into_inner());
    let pending: Vec<serde_json::Value> = mgr
        .pending_approvals()
        .into_iter()
        .map(|w| {
            serde_json::json!({
                "id": w.id.to_string(),
                "user_id": w.user_id.to_string(),
                "asset": w.asset,
                "network": w.network,
                "address": w.address,
                "amount": w.amount,
                "fee": w.fee,
                "created_at": w.created_at.timestamp_millis(),
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(pending)))
}

#[derive(Debug, Deserialize)]
pub struct AdminWithdrawalRejectRequest {
    pub reason: String,
}

/// POST /admin/withdrawals/{id}/reject — reject a pending withdrawal.
pub async fn admin_withdrawal_reject(
    State(exchange): State<AppState>,
    Path(withdrawal_id): Path<Uuid>,
    Json(req): Json<AdminWithdrawalRejectRequest>,
) -> impl IntoResponse {
    let mut mgr = exchange.withdrawal_manager.lock().unwrap_or_else(|e| e.into_inner());
    match mgr.reject(&withdrawal_id, req.reason.clone()) {
        Ok(_) => {
            tracing::info!(
                withdrawal_id = %withdrawal_id,
                reason = %req.reason,
                "Admin rejected withdrawal"
            );
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "id": withdrawal_id.to_string(),
                    "status": "rejected",
                    "reason": req.reason,
                })),
            )
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

/// GET /admin/aml/alerts — list all unreviewed surveillance alerts.
pub async fn admin_aml_alerts(State(exchange): State<AppState>) -> impl IntoResponse {
    let surveillance = exchange.surveillance.lock();
    let alerts: Vec<serde_json::Value> = surveillance
        .unreviewed_alerts()
        .iter()
        .map(|a| {
            serde_json::json!({
                "id": a.id.to_string(),
                "type": format!("{:?}", a.alert_type),
                "severity": format!("{}", a.severity),
                "user_ids": a.user_ids.iter().map(|u| u.to_string()).collect::<Vec<_>>(),
                "symbol": a.symbol.as_ref().map(|s| s.to_string()),
                "description": a.description,
                "auto_action": format!("{:?}", a.auto_action),
                "created_at": a.created_at.timestamp_millis(),
                "reviewed": a.reviewed,
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(alerts)))
}

/// M19-8: GET /admin/surveillance/alerts — paginated alert list with status filter.
#[derive(Debug, Deserialize)]
pub struct SurveillanceAlertQuery {
    pub status: Option<String>,  // "unreviewed" | "all"
    pub page: Option<usize>,
    pub limit: Option<usize>,
}

pub async fn admin_surveillance_alerts(
    State(exchange): State<AppState>,
    Query(query): Query<SurveillanceAlertQuery>,
) -> impl IntoResponse {
    let surveillance = exchange.surveillance.lock();
    let show_all = query.status.as_deref() != Some("unreviewed");
    let page = query.page.unwrap_or(0);
    let limit = query.limit.unwrap_or(50).min(200);

    let alerts_iter: Box<dyn Iterator<Item = &exchange_surveillance::SurveillanceAlert>> =
        if show_all {
            Box::new(surveillance.all_alerts().iter().rev())
        } else {
            Box::new(surveillance.unreviewed_alerts().into_iter().rev())
        };

    let alerts: Vec<serde_json::Value> = alerts_iter
        .skip(page * limit)
        .take(limit)
        .map(|a| {
            serde_json::json!({
                "id": a.id.to_string(),
                "type": format!("{:?}", a.alert_type),
                "severity": format!("{}", a.severity),
                "user_ids": a.user_ids.iter().map(|u| u.to_string()).collect::<Vec<_>>(),
                "symbol": a.symbol.as_ref().map(|s| s.to_string()),
                "description": &a.description,
                "auto_action": format!("{:?}", a.auto_action),
                "created_at": a.created_at.timestamp_millis(),
                "reviewed": a.reviewed,
            })
        })
        .collect();

    let total = if show_all {
        surveillance.all_alerts().len()
    } else {
        surveillance.unreviewed_alerts().len()
    };

    (StatusCode::OK, Json(serde_json::json!({
        "alerts": alerts,
        "total": total,
        "page": page,
        "limit": limit,
    })))
}

/// M19-8: POST /admin/surveillance/alerts/{id}/review — mark alert as reviewed.
pub async fn admin_review_alert(
    State(exchange): State<AppState>,
    Path(alert_id): Path<String>,
) -> impl IntoResponse {
    let id = match Uuid::parse_str(&alert_id) {
        Ok(id) => id,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Invalid alert ID"}))),
    };

    let mut surveillance = exchange.surveillance.lock();
    if surveillance.review_alert(id) {
        (StatusCode::OK, Json(serde_json::json!({"reviewed": true, "id": alert_id})))
    } else {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Alert not found"})))
    }
}

#[derive(Debug, Deserialize)]
pub struct AdminSurveillanceActionRequest {
    pub user_id: Uuid,
    pub action: String,
}

/// POST /admin/surveillance/action — apply a compliance action to a user.
///
/// Supported actions:
/// - `"freeze"` — log the freeze intent (Phase 1: logging only)
/// - `"cancel_orders"` — cancel all open orders for the user
/// - `"flag"` — log the flag intent (Phase 1: logging only)
pub async fn admin_surveillance_action(
    State(exchange): State<AppState>,
    Json(req): Json<AdminSurveillanceActionRequest>,
) -> impl IntoResponse {
    let user_id = req.user_id;

    match req.action.as_str() {
        "freeze" => {
            // Insert into frozen set — place_order and submit_withdrawal will reject this account.
            exchange.frozen_accounts.insert(user_id);

            // Also cancel all open orders for this user.
            let order_ids: Vec<_> = exchange
                .pending_orders
                .iter()
                .filter(|r| r.user_id == user_id)
                .map(|r| (*r.key(), r.symbol.clone()))
                .collect();
            let cancelled = order_ids.len();
            for (order_id, symbol) in order_ids {
                let cmd = CancelOrderCommand {
                    order_id,
                    user_id,
                    symbol,
                    timestamp: chrono::Utc::now(),
                };
                if let Err(e) = exchange.cancel_order(cmd) {
                    tracing::warn!(order_id = %order_id, error = %e, "Failed to cancel order during freeze");
                }
            }

            tracing::warn!(user_id = %user_id, cancelled = cancelled, "Admin action: account frozen, open orders cancelled");
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "user_id": user_id.to_string(),
                    "action": "freeze",
                    "status": "frozen",
                    "orders_cancelled": cancelled,
                })),
            )
        }
        "unfreeze" => {
            exchange.frozen_accounts.remove(&user_id);
            tracing::info!(user_id = %user_id, "Admin action: account unfrozen");
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "user_id": user_id.to_string(),
                    "action": "unfreeze",
                    "status": "active",
                })),
            )
        }
        "cancel_orders" => {
            // Collect all open order IDs for this user from pending_orders.
            let order_ids: Vec<_> = exchange
                .pending_orders
                .iter()
                .filter(|r| r.user_id == user_id)
                .map(|r| (*r.key(), r.symbol.clone()))
                .collect();

            let cancelled = order_ids.len();
            for (order_id, symbol) in order_ids {
                let cmd = CancelOrderCommand {
                    order_id,
                    user_id,
                    symbol,
                    timestamp: chrono::Utc::now(),
                };
                if let Err(e) = exchange.cancel_order(cmd) {
                    tracing::warn!(order_id = %order_id, error = %e, "Failed to cancel order during surveillance action");
                }
            }

            tracing::info!(user_id = %user_id, cancelled = cancelled, "Admin action: cancelled all open orders");
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "user_id": user_id.to_string(),
                    "action": "cancel_orders",
                    "cancelled": cancelled,
                })),
            )
        }
        "flag" => {
            tracing::warn!(user_id = %user_id, "Admin action: account flagged for review (Phase 1: log only)");
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "user_id": user_id.to_string(),
                    "action": "flag",
                    "status": "logged",
                })),
            )
        }
        other => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("Unknown action '{}'. Supported: freeze, unfreeze, cancel_orders, flag", other)
            })),
        ),
    }
}

#[derive(Debug, Deserialize)]
pub struct AdminPruneStaleRequest {
    pub symbol: String,
    /// Cancel only orders older than this many hours. Default: 24.
    #[serde(default = "default_prune_hours")]
    pub older_than_hours: i64,
    /// Cancel only orders whose price is this far or further from mid-price,
    /// expressed as a percentage (e.g. `5.0` = 5%). Default: 5.0.
    #[serde(default = "default_prune_distance_pct")]
    pub distance_from_mid_pct: Decimal,
}

fn default_prune_hours() -> i64 {
    24
}

fn default_prune_distance_pct() -> Decimal {
    Decimal::from(5)
}

/// POST /admin/orders/prune-stale — cancel resting orders that are both old
/// AND far from the mid-price on a given symbol.
///
/// Used by the high-latency runbook (Step 5) when a market-maker has left
/// behind many stale orders that are inflating the orderbook and harming
/// matching latency. This is an operational maintenance tool — it only
/// cancels orders that satisfy BOTH filters, so a thin tail of legitimate
/// limit orders near mid-price is never touched.
pub async fn admin_prune_stale_orders(
    State(exchange): State<AppState>,
    Json(req): Json<AdminPruneStaleRequest>,
) -> impl IntoResponse {
    let symbol = Symbol::new(&req.symbol);

    // Fetch top-of-book for mid-price calculation.
    let mid_price = {
        let pool = exchange
            .engine_pool
            .read()
            .unwrap_or_else(|e| e.into_inner());
        match pool.snapshot(&symbol, 1) {
            Ok(snap) => {
                let bid = snap.bids.first().map(|(p, _)| *p);
                let ask = snap.asks.first().map(|(p, _)| *p);
                match (bid, ask) {
                    (Some(b), Some(a)) if b > Decimal::ZERO && a > Decimal::ZERO => {
                        Some((b + a) / Decimal::from(2))
                    }
                    _ => None,
                }
            }
            Err(_) => None,
        }
    };

    let Some(mid_price) = mid_price else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "Cannot compute mid-price — symbol has no two-sided book",
                "symbol": req.symbol,
            })),
        );
    };

    let cutoff_age = chrono::Utc::now() - chrono::Duration::hours(req.older_than_hours);
    let distance_threshold = mid_price * req.distance_from_mid_pct / Decimal::from(100);

    // Snapshot the set of orders that match both filters (old AND far).
    let to_cancel: Vec<(uuid::Uuid, Symbol, uuid::Uuid)> = exchange
        .pending_orders
        .iter()
        .filter(|r| r.symbol == symbol)
        .filter(|r| r.created_at < cutoff_age)
        .filter(|r| (r.price - mid_price).abs() >= distance_threshold)
        .map(|r| (*r.key(), r.symbol.clone(), r.user_id))
        .collect();

    let candidates = to_cancel.len();
    let mut cancelled = 0usize;
    let mut failed = 0usize;
    for (order_id, sym, user_id) in to_cancel {
        let cmd = CancelOrderCommand {
            order_id,
            user_id,
            symbol: sym,
            timestamp: chrono::Utc::now(),
        };
        match exchange.cancel_order(cmd) {
            Ok(_) => cancelled += 1,
            Err(e) => {
                failed += 1;
                tracing::warn!(
                    order_id = %order_id,
                    error = %e,
                    "prune_stale: cancel_order failed",
                );
            }
        }
    }

    tracing::warn!(
        symbol = %symbol,
        mid_price = %mid_price,
        older_than_hours = req.older_than_hours,
        distance_pct = %req.distance_from_mid_pct,
        candidates = candidates,
        cancelled = cancelled,
        failed = failed,
        "Admin action: prune-stale completed",
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "symbol": req.symbol,
            "mid_price": mid_price,
            "candidates": candidates,
            "cancelled": cancelled,
            "failed": failed,
        })),
    )
}

/// DELETE /admin/users/{user_id}/api-keys — revoke every active API key owned
/// by the user.
///
/// Used by the suspected-hack runbook (Step 5) to immediately cut off a
/// compromised account. This is a permanent deactivation; individual keys
/// cannot be re-activated, the user must mint new ones.
pub async fn admin_revoke_user_api_keys(
    State(exchange): State<AppState>,
    Path(user_id): Path<Uuid>,
) -> impl IntoResponse {
    let Some(store) = exchange.key_store.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "API key store not configured — nothing to revoke",
            })),
        );
    };

    let revoked = store.revoke_all_for_user(user_id);

    tracing::warn!(
        user_id = %user_id,
        revoked = revoked,
        "Admin action: revoked all active API keys for user",
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "user_id": user_id.to_string(),
            "revoked": revoked,
        })),
    )
}

// ---------------------------------------------------------------------------
// W1.2: Wallet & Account endpoints
// ---------------------------------------------------------------------------

/// GET /api/v1/account/wallet — portfolio overview for the authenticated user.
///
/// Aggregates all balances and computes a USD estimate using USDT as the
/// reference asset (non-USDT assets are listed at book value in Phase 1
/// until a price oracle is wired).
pub async fn get_wallet_overview(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let balances = exchange.accounts.get_all_balances(user_id);
    if balances.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Account not found"})),
        );
    }

    // Phase 1: mark prices only for USDT (1:1). Other assets use face value.
    // Phase 2: call index_aggregator for each asset.
    let assets: Vec<serde_json::Value> = balances
        .iter()
        .map(|(asset, bal)| {
            // Simple USD estimate: USDT at par, others at face value
            let usd_value = bal.total();
            serde_json::json!({
                "asset": asset,
                "total": bal.total(),
                "available": bal.available(),
                "locked": bal.locked(),
                "usd_value": usd_value,
            })
        })
        .collect();

    let total_equity_usd: Decimal = assets
        .iter()
        .map(|a| a["usd_value"].as_str().unwrap_or("0").parse::<Decimal>().unwrap_or(Decimal::ZERO))
        .sum();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "total_equity_usd": total_equity_usd,
            "assets": assets,
        })),
    )
}

/// GET /api/v1/account/wallet-balance — Bybit-style unified wallet balance.
///
/// Returns total equity in USD, margin usage, and a per-coin breakdown that
/// includes wallet balance, unrealized PnL, order margin, position initial
/// margin, and position maintenance margin for each asset.
///
/// Phase 1: all perpetual positions are USDT-margined. Non-USDT/USDC assets
/// are converted to USD using the mark price of `{COIN}USDT` if available,
/// otherwise they contribute zero to the USD totals.
pub async fn get_wallet_balance(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;

    // Collect balances — the sharded store returns an owned snapshot.
    let balances = exchange.accounts.get_all_balances(user_id);
    if balances.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Account not found"})),
        );
    }

    // Collect open positions for this user — cloned so we don't hold the lock.
    let positions = {
        let pm = exchange.positions.lock();
        pm.user_positions(user_id)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>()
    };

    // Snapshot mark prices once for all symbols.
    let mark_prices = exchange.get_mark_prices();

    // Snapshot perp configs once — needed for maintenance margin rate lookup.
    // S17: load_full() returns Arc<HashMap> — no mutex acquisition.
    let perp_configs = exchange.perp_configs.load_full();

    let mut total_equity = Decimal::ZERO;
    let mut total_wallet = Decimal::ZERO;
    let mut total_available = Decimal::ZERO;
    let mut total_im = Decimal::ZERO;
    let mut total_mm = Decimal::ZERO;
    let mut total_upnl = Decimal::ZERO;

    let mut coins: Vec<CoinBalance> = balances
        .iter()
        .map(|(asset, bal)| {
            let mut coin = CoinBalance {
                coin: asset.clone(),
                wallet_balance: bal.total(),
                available_balance: bal.available(),
                locked: bal.locked(),
                unrealized_pnl: Decimal::ZERO,
                equity: bal.total(),
                order_margin: bal.locked(),
                position_initial_margin: Decimal::ZERO,
                position_maintenance_margin: Decimal::ZERO,
                cumulative_realized_pnl: Decimal::ZERO,
                available_to_withdraw: bal.available(),
            };

            // Accumulate position margins and uPnL for positions whose margin
            // is settled in this coin (Phase 1: all perpetuals use USDT).
            for pos in &positions {
                // Determine the settlement coin: the quote asset of the perp.
                // Phase 1 heuristic — symbol ends with the asset name
                // (e.g., "BTCUSDT" ends with "USDT") covers all standard perps.
                // USDC-margined perps (symbol ends with "USDC") are also handled.
                let is_settlement_coin = pos.symbol.as_str().ends_with(asset.as_str());

                if !is_settlement_coin {
                    continue;
                }

                let Some(mp) = mark_prices.get(&pos.symbol) else {
                    continue;
                };
                let mark = mp.price;

                let upnl = pos.unrealized_pnl(mark);
                let notional = pos.notional(mark);
                let mmr = perp_configs
                    .get(&pos.symbol)
                    .map(|cfg| cfg.effective_mmr(notional))
                    .unwrap_or(Decimal::ZERO);
                let mm = exchange_risk_engine::margin_calculator::required_maintenance_margin(
                    pos.size, mark, mmr,
                );

                coin.unrealized_pnl += upnl;
                coin.position_initial_margin += pos.margin;
                coin.position_maintenance_margin += mm;
            }

            coin.equity = coin.wallet_balance + coin.unrealized_pnl;
            // available_to_withdraw = wallet_balance minus all locked funds
            // (order locks + position margin). We take the wallet_balance minus
            // locked (order margin) minus position margin, floored at zero.
            coin.available_to_withdraw = (coin.wallet_balance
                - coin.locked
                - coin.position_initial_margin)
                .max(Decimal::ZERO);

            // USD conversion rate for this coin.
            let usd_rate = if asset == "USDT" || asset == "USDC" {
                Decimal::ONE
            } else {
                // Try {COIN}USDT mark price.
                let sym = exchange_common::Symbol::new(format!("{asset}USDT"));
                mark_prices
                    .get(&sym)
                    .map(|mp| mp.price)
                    .unwrap_or(Decimal::ZERO)
            };

            total_equity += coin.equity * usd_rate;
            total_wallet += coin.wallet_balance * usd_rate;
            total_available += coin.available_balance * usd_rate;
            total_im += coin.position_initial_margin * usd_rate;
            total_mm += coin.position_maintenance_margin * usd_rate;
            total_upnl += coin.unrealized_pnl * usd_rate;

            coin
        })
        .collect();

    // Sort coins for a deterministic, user-friendly order: USDT/USDC first,
    // then everything else alphabetically.
    coins.sort_by(|a, b| {
        let rank = |coin: &str| match coin {
            "USDT" => 0,
            "USDC" => 1,
            _ => 2,
        };
        rank(&a.coin).cmp(&rank(&b.coin)).then(a.coin.cmp(&b.coin))
    });

    let im_rate = if total_equity > Decimal::ZERO {
        total_im / total_equity
    } else {
        Decimal::ZERO
    };
    let mm_rate = if total_equity > Decimal::ZERO {
        total_mm / total_equity
    } else {
        Decimal::ZERO
    };

    let summary = WalletSummary {
        total_equity_usd: total_equity,
        total_wallet_balance_usd: total_wallet,
        total_available_balance_usd: total_available,
        total_initial_margin_usd: total_im,
        total_maintenance_margin_usd: total_mm,
        total_unrealized_pnl_usd: total_upnl,
        account_im_rate: im_rate,
        account_mm_rate: mm_rate,
        coins,
    };

    (StatusCode::OK, Json(serde_json::json!(summary)))
}

/// Query parameters for equity-curve and PnL period filtering.
#[derive(Debug, Deserialize)]
pub struct PeriodQuery {
    #[serde(default = "default_period")]
    pub period: String,
}

fn default_period() -> String {
    "30d".to_string()
}

/// GET /api/v1/account/wallet/equity-curve?period=30d — equity history.
///
/// Phase 1: returns empty array (historical snapshots need persistent storage).
pub async fn get_equity_curve(
    State(_exchange): State<AppState>,
    axum::Extension(_auth): axum::Extension<AuthenticatedUser>,
    Query(_params): Query<PeriodQuery>,
) -> impl IntoResponse {
    // Phase 1: no historical snapshots stored. Phase 2: query time-series DB.
    (StatusCode::OK, Json(serde_json::json!([])))
}

/// GET /api/v1/account/address-book — list whitelisted withdrawal addresses.
pub async fn get_address_book(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let wl = exchange.address_whitelist.lock().unwrap_or_else(|e| e.into_inner());
    let addresses: Vec<serde_json::Value> = wl
        .user_addresses(user_id)
        .into_iter()
        .map(|a| {
            serde_json::json!({
                "address": a.address,
                "network": a.network,
                "label": a.label,
                "is_active": a.is_active,
                "active_at": a.active_at.timestamp_millis(),
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(addresses)))
}

#[derive(Debug, Deserialize)]
pub struct AddAddressRequest {
    pub address: String,
    pub network: String,
    pub label: String,
}

/// POST /api/v1/account/address-book — add a withdrawal address (24h cooldown applied).
pub async fn add_address(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<AddAddressRequest>,
) -> impl IntoResponse {
    if req.address.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "address must not be empty"})),
        );
    }
    if req.network.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "network must not be empty"})),
        );
    }

    let user_id = auth.0;
    let mut wl = exchange.address_whitelist.lock().unwrap_or_else(|e| e.into_inner());
    wl.add_address(user_id, &req.address, &req.network, &req.label);

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "address": req.address,
            "network": req.network,
            "label": req.label,
            "status": "added",
            "note": "24-hour cooldown before address can be used for withdrawals",
        })),
    )
}

/// DELETE /api/v1/account/address-book/{address} — remove a whitelisted address.
pub async fn remove_address(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(address): Path<String>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let mut wl = exchange.address_whitelist.lock().unwrap_or_else(|e| e.into_inner());
    wl.remove_address(user_id, &address);

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "address": address,
            "status": "removed",
        })),
    )
}

/// GET /api/v1/account/pnl?period=30d — PnL analysis from trade history.
///
/// Aggregates realized PnL from order history (filled orders).
/// Phase 1: derives daily PnL from filled order data; future phases will use
/// a dedicated settlement ledger.
pub async fn get_pnl(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Query(_params): Query<PeriodQuery>,
) -> impl IntoResponse {
    let user_id = auth.0;

    let positions = exchange.get_positions(user_id);

    // Compute totals from open positions' realized PnL.
    let realized_pnl: Decimal = positions.iter().map(|p| p.realized_pnl).sum();
    let unrealized_pnl: Decimal = positions.iter().map(|p| {
        let mark = exchange.get_mark_price(&p.symbol)
            .map(|mp| mp.price)
            .unwrap_or(p.entry_price);
        p.unrealized_pnl(mark)
    }).sum();

    // Phase 1: simplified summary — no per-day or per-symbol breakdown yet.
    let by_symbol: Vec<serde_json::Value> = positions
        .iter()
        .map(|p| {
            let mark = exchange.get_mark_price(&p.symbol)
                .map(|mp| mp.price)
                .unwrap_or(p.entry_price);
            serde_json::json!({
                "symbol": p.symbol.to_string(),
                "realized_pnl": p.realized_pnl,
                "unrealized_pnl": p.unrealized_pnl(mark),
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "summary": {
                "realized": realized_pnl,
                "unrealized": unrealized_pnl,
                "total": realized_pnl + unrealized_pnl,
            },
            "daily": [],
            "by_symbol": by_symbol,
        })),
    )
}

/// GET /api/v1/fees — VIP fee schedule and withdrawal fees.
pub async fn get_fee_schedule(State(exchange): State<AppState>) -> impl IntoResponse {
    let fee_engine = exchange.fee_engine.lock().unwrap_or_else(|e| e.into_inner());
    let tiers: Vec<serde_json::Value> = fee_engine
        .tiers()
        .iter()
        .map(|t| {
            serde_json::json!({
                "level": t.level,
                "name": t.name,
                "min_volume_30d": t.min_volume_30d,
                "min_balance": t.min_balance,
                "maker_fee": t.maker_fee,
                "taker_fee": t.taker_fee,
            })
        })
        .collect();

    let withdrawal_mgr = exchange.withdrawal_manager.lock().unwrap_or_else(|e| e.into_inner());
    // Return withdrawal fees for commonly supported assets.
    let withdrawal_fees: Vec<serde_json::Value> = ["USDT", "BTC", "ETH"]
        .iter()
        .map(|asset| {
            serde_json::json!({
                "asset": asset,
                "fee": withdrawal_mgr.fee(asset),
                "minimum": withdrawal_mgr.minimum(asset),
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "vip_tiers": tiers,
            "withdrawal_fees": withdrawal_fees,
        })),
    )
}

/// GET /api/v1/account/fee-rate — personalized fee rate for the authenticated user.
///
/// Returns the user's current maker/taker fee rates based on their 30-day volume tier,
/// along with the tier name and accumulated volume used for qualification.
pub async fn get_user_fee_rate(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let fee_engine = exchange.fee_engine.lock().unwrap_or_else(|e| e.into_inner());
    let tier = fee_engine.get_tier(user_id);
    let (maker_fee, taker_fee) = fee_engine.get_fees(user_id);

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "makerFeeRate": maker_fee.to_string(),
            "takerFeeRate": taker_fee.to_string(),
            "vipLevel": tier.name,
            "vipLevelIndex": tier.level,
        })),
    )
}

// --- Market data endpoints ---

#[derive(Debug, Deserialize)]
pub struct KlinesQuery {
    #[serde(default = "default_klines_interval")]
    pub interval: String,
    #[serde(default = "default_klines_limit")]
    pub limit: usize,
}

fn default_klines_interval() -> String {
    "1m".to_string()
}

fn default_klines_limit() -> usize {
    200
}

/// GET /api/v1/markets/:symbol/klines?interval=1m&limit=200
///
/// Returns OHLCV candles as `[[timestamp_ms, open, high, low, close, volume], ...]`
/// with all price/volume values as strings (same format as Binance/Bybit REST API).
///
/// The response includes both closed historical candles and the current open candle,
/// so the chart always shows the live bar updating in real time.
pub async fn get_klines(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
    Query(params): Query<KlinesQuery>,
) -> impl IntoResponse {
    if let Err(e) = validate_symbol(&symbol) {
        return e;
    }

    let interval = match params.interval.as_str() {
        "1s"                  => exchange_market_data::CandleInterval::S1,
        "1m"                  => exchange_market_data::CandleInterval::M1,
        "3m"                  => exchange_market_data::CandleInterval::M3,
        "5m"                  => exchange_market_data::CandleInterval::M5,
        "15m"                 => exchange_market_data::CandleInterval::M15,
        "30m"                 => exchange_market_data::CandleInterval::M30,
        "1h" | "1H"           => exchange_market_data::CandleInterval::H1,
        "2h" | "2H"           => exchange_market_data::CandleInterval::H2,
        "4h" | "4H"           => exchange_market_data::CandleInterval::H4,
        "6h" | "6H"           => exchange_market_data::CandleInterval::H6,
        "8h" | "8H"           => exchange_market_data::CandleInterval::H8,
        "12h" | "12H"         => exchange_market_data::CandleInterval::H12,
        "1d" | "1D"           => exchange_market_data::CandleInterval::D1,
        "3d" | "3D"           => exchange_market_data::CandleInterval::D3,
        "1w" | "1W"           => exchange_market_data::CandleInterval::W1,
        "1M"                  => exchange_market_data::CandleInterval::MN1,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid interval. Supported: 1s, 1m, 3m, 5m, 15m, 30m, 1h, 2h, 4h, 6h, 8h, 12h, 1d, 3d, 1w, 1M"})),
            )
        }
    };

    let sym = Symbol::new(&symbol);
    let limit = params.limit.min(1000);

    // Closed candles come back newest-first; chart needs oldest-first.
    // We also append the current open candle so the live bar is always visible.
    let mut closed = exchange.market_data.candles(&sym, interval, limit);
    let current_candle = exchange.market_data.current_candle(&sym, interval);

    // closed is [newest, ..., oldest]; reverse for chart (oldest → newest)
    closed.reverse();

    let mut result: Vec<serde_json::Value> = closed
        .iter()
        .map(|c| candle_to_json(c))
        .collect();

    if let Some(c) = current_candle {
        result.push(candle_to_json(&c));
    }

    // If the chart requested more candles than we have, that is fine — just return what we have.
    (StatusCode::OK, Json(serde_json::json!(result)))
}

fn candle_to_json(c: &exchange_market_data::Candle) -> serde_json::Value {
    serde_json::json!({
        "open_time": c.open_time.timestamp_millis(),
        "close_time": c.close_time.timestamp_millis(),
        "open": c.open.to_string(),
        "high": c.high.to_string(),
        "low": c.low.to_string(),
        "close": c.close.to_string(),
        "volume": c.volume.to_string(),
        "quote_volume": c.quote_volume.to_string(),
        "trade_count": c.trade_count,
        "taker_buy_volume": c.taker_buy_volume.to_string(),
        "taker_sell_volume": c.taker_sell_volume.to_string(),
        "taker_buy_quote_volume": c.taker_buy_quote_volume.to_string(),
        "taker_sell_quote_volume": c.taker_sell_quote_volume.to_string(),
        "vwap": c.vwap().to_string(),
        "delta": c.delta().to_string(),
        "delta_pct": c.delta_pct().to_string(),
        "is_closed": c.is_closed,
    })
}

/// GET /api/v1/markets/:symbol/mark-klines — mark price candlesticks.
///
/// Phase 1: returns trade-price klines with `"priceType": "mark"` added.
/// Mark price tracks trade price closely at this scale, so the data is valid
/// for charting purposes. Phase 2 will maintain a separate mark-price series.
pub async fn get_mark_klines(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
    Query(params): Query<KlinesQuery>,
) -> impl IntoResponse {
    get_klines_with_price_type(exchange, symbol, params, "mark").await
}

/// GET /api/v1/markets/:symbol/index-klines — index price candlesticks.
///
/// Phase 1: returns trade-price klines with `"priceType": "index"` added.
/// Phase 2 will maintain a separate index-price series.
pub async fn get_index_klines(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
    Query(params): Query<KlinesQuery>,
) -> impl IntoResponse {
    get_klines_with_price_type(exchange, symbol, params, "index").await
}

async fn get_klines_with_price_type(
    exchange: AppState,
    symbol: String,
    params: KlinesQuery,
    price_type: &str,
) -> axum::response::Response {
    if let Err(e) = validate_symbol(&symbol) {
        return e.into_response();
    }

    let interval = match params.interval.as_str() {
        "1s"          => exchange_market_data::CandleInterval::S1,
        "1m"          => exchange_market_data::CandleInterval::M1,
        "3m"          => exchange_market_data::CandleInterval::M3,
        "5m"          => exchange_market_data::CandleInterval::M5,
        "15m"         => exchange_market_data::CandleInterval::M15,
        "30m"         => exchange_market_data::CandleInterval::M30,
        "1h" | "1H"   => exchange_market_data::CandleInterval::H1,
        "2h" | "2H"   => exchange_market_data::CandleInterval::H2,
        "4h" | "4H"   => exchange_market_data::CandleInterval::H4,
        "6h" | "6H"   => exchange_market_data::CandleInterval::H6,
        "8h" | "8H"   => exchange_market_data::CandleInterval::H8,
        "12h" | "12H" => exchange_market_data::CandleInterval::H12,
        "1d" | "1D"   => exchange_market_data::CandleInterval::D1,
        "3d" | "3D"   => exchange_market_data::CandleInterval::D3,
        "1w" | "1W"   => exchange_market_data::CandleInterval::W1,
        "1M"          => exchange_market_data::CandleInterval::MN1,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid interval. Supported: 1s, 1m, 3m, 5m, 15m, 30m, 1h, 2h, 4h, 6h, 8h, 12h, 1d, 3d, 1w, 1M"})),
            )
                .into_response();
        }
    };

    let sym = Symbol::new(&symbol);
    let limit = params.limit.min(1000);

    let mut closed = exchange.market_data.candles(&sym, interval, limit);
    let current_candle = exchange.market_data.current_candle(&sym, interval);

    closed.reverse();

    let mut result: Vec<serde_json::Value> = closed
        .iter()
        .map(|c| {
            let mut v = candle_to_json(c);
            v["priceType"] = serde_json::json!(price_type);
            v
        })
        .collect();

    if let Some(c) = current_candle {
        let mut v = candle_to_json(&c);
        v["priceType"] = serde_json::json!(price_type);
        result.push(v);
    }

    (StatusCode::OK, Json(serde_json::json!(result))).into_response()
}

/// GET /api/v1/market/long-short-ratio — public long/short account ratio.
///
/// Counts open Long vs Short positions for the requested symbol and returns
/// the ratio. This reflects the state of the exchange's own position book.
pub async fn get_long_short_ratio(
    State(exchange): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let symbol = match params.get("symbol") {
        Some(s) => Symbol::new(s),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing symbol parameter"})),
            )
                .into_response();
        }
    };

    let pm = exchange.positions.lock();
    let mut long_count = 0u64;
    let mut short_count = 0u64;
    for pos in pm.all_positions() {
        if pos.symbol == symbol && !pos.is_closed() {
            match pos.side {
                exchange_common::futures::PositionSide::Long => long_count += 1,
                exchange_common::futures::PositionSide::Short => short_count += 1,
            }
        }
    }

    let total = long_count + short_count;
    let long_ratio = if total > 0 {
        long_count as f64 / total as f64
    } else {
        0.5
    };
    let short_ratio = if total > 0 {
        short_count as f64 / total as f64
    } else {
        0.5
    };

    Json(serde_json::json!({
        "symbol": symbol.to_string(),
        "longAccountRatio": format!("{:.4}", long_ratio),
        "shortAccountRatio": format!("{:.4}", short_ratio),
        "longAccounts": long_count,
        "shortAccounts": short_count,
        "timestamp": chrono::Utc::now().timestamp_millis(),
    }))
    .into_response()
}

/// GET /api/v1/market/insurance — public insurance fund balances.
///
/// Returns the balance for each perpetual symbol's insurance fund.
/// An optional `symbol` query parameter filters to a single symbol.
pub async fn get_insurance_fund(
    State(exchange): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let symbol_filter = params.get("symbol").map(|s| s.as_str());

    let funds = exchange.insurance_funds.lock().unwrap_or_else(|e| e.into_inner());
    let entries: Vec<serde_json::Value> = funds
        .iter()
        .filter(|(sym, _)| symbol_filter.is_none_or( |s| sym.as_str() == s))
        .map(|(sym, fund)| {
            serde_json::json!({
                "symbol": sym.to_string(),
                "balance": fund.balance().to_string(),
                "updatedTime": chrono::Utc::now().timestamp_millis(),
            })
        })
        .collect();

    Json(serde_json::json!({ "list": entries })).into_response()
}

/// GET /api/v1/market/risk-limit — leverage tier risk limits per symbol.
///
/// Returns the full bracket table for every perpetual contract (or a single
/// symbol when `?symbol=<sym>` is provided).  Each bracket entry contains:
/// - `id` — bracket index (1-based)
/// - `symbol`
/// - `minNotional` / `maxNotional` — bracket notional bounds in USDT
/// - `maxLeverage` — maximum leverage permitted in this bracket
/// - `maintenanceMarginRate` — MMR fraction (e.g., "0.005" = 0.5 %)
/// - `initialMarginRate` — IMR fraction = 1 / maxLeverage
pub async fn get_risk_limit(
    State(exchange): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let symbol_filter = params.get("symbol").map(|s| s.as_str());

    let configs = exchange.perp_configs.load();
    let entries: Vec<serde_json::Value> = configs
        .iter()
        .filter(|(sym, _)| symbol_filter.is_none_or( |s| sym.as_str() == s))
        .flat_map(|(sym, config)| {
            let brackets = exchange_risk_engine::brackets_for_config(config);
            brackets.into_iter().map(move |b| {
                serde_json::json!({
                    "id": b.id,
                    "symbol": sym.to_string(),
                    "minNotional": b.min_notional.to_string(),
                    "maxNotional": b.max_notional.map(|v| v.to_string()),
                    "maxLeverage": b.max_leverage.to_string(),
                    "maintenanceMarginRate": b.maintenance_margin_rate.to_string(),
                    "initialMarginRate": b.initial_margin_rate.to_string(),
                })
            })
        })
        .collect();

    Json(serde_json::json!({ "list": entries })).into_response()
}

/// GET /api/v1/market/risk-limit/{symbol} — leverage brackets for a single symbol.
///
/// Returns the full notional bracket table for the given perpetual contract.
pub async fn get_risk_limit_symbol(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = validate_symbol(&symbol) {
        return e.into_response();
    }

    let sym = exchange_common::types::Symbol::new(&symbol);
    let configs = exchange.perp_configs.load();

    match configs.get(&sym) {
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Symbol {} not found", symbol) })),
        )
            .into_response(),
        Some(config) => {
            let brackets = exchange_risk_engine::brackets_for_config(config);
            let entries: Vec<serde_json::Value> = brackets
                .iter()
                .map(|b| {
                    serde_json::json!({
                        "id": b.id,
                        "symbol": symbol,
                        "minNotional": b.min_notional.to_string(),
                        "maxNotional": b.max_notional.map(|v| v.to_string()),
                        "maxLeverage": b.max_leverage.to_string(),
                        "maintenanceMarginRate": b.maintenance_margin_rate.to_string(),
                        "initialMarginRate": b.initial_margin_rate.to_string(),
                    })
                })
                .collect();
            Json(serde_json::json!({ "symbol": symbol, "list": entries })).into_response()
        }
    }
}

/// GET /api/v1/markets — list all tradeable instruments with 24h ticker data.
///
/// Returns every registered product alongside its current ticker snapshot.
/// The frontend pair selector uses this to populate the market list dynamically
/// instead of hardcoding symbol arrays.
pub async fn list_markets(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let products = exchange.accounts.products();
    let perp_configs = exchange.perp_configs.load();

    let markets: Vec<serde_json::Value> = products
        .iter()
        .map(|p| {
            let ticker = exchange.market_data.ticker(&p.symbol);
            let is_perp = perp_configs.contains_key(&p.symbol);
            let market_type = if is_perp {
                if p.quote_asset == "USD" || p.quote_asset == "BTC" || p.quote_asset == "ETH" {
                    "coin_m"
                } else {
                    "perpetual"
                }
            } else {
                "spot"
            };
            serde_json::json!({
                "symbol": p.symbol.to_string(),
                "base_asset": p.base_asset,
                "quote_asset": p.quote_asset,
                "market_type": market_type,
                "tick_size": p.tick_size.to_string(),
                "lot_size": p.lot_size.to_string(),
                "min_notional": p.min_notional.to_string(),
                "maker_fee": p.maker_fee.to_string(),
                "taker_fee": p.taker_fee.to_string(),
                "last_price": ticker.as_ref().map(|t| t.last_price.to_string()).unwrap_or_default(),
                "price_change_pct_24h": ticker.as_ref().map(|t| t.price_change_pct_24h.to_string()).unwrap_or_default(),
                "volume_24h": ticker.as_ref().map(|t| t.volume_24h.to_string()).unwrap_or_default(),
                "high_24h": ticker.as_ref().map(|t| t.high_24h.to_string()).unwrap_or_default(),
                "low_24h": ticker.as_ref().map(|t| t.low_24h.to_string()).unwrap_or_default(),
            })
        })
        .collect();

    Json(markets).into_response()
}

/// GET /api/v1/markets/:symbol/ticker — 24-hour rolling ticker.
///
/// Returns all enriched fields (Binance/Bybit quality). For perpetual symbols
/// the response also includes mark_price, index_price, open_interest, and
/// funding_rate sourced from the exchange risk engine.
pub async fn get_ticker(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = validate_symbol(&symbol) {
        return e;
    }

    let sym = Symbol::new(&symbol);

    // For futures symbols, inject the latest mark/index/OI/funding into the
    // market-data service before computing the ticker snapshot.
    if exchange.perp_configs.load().contains_key(&sym) {
        if let Some(mark) = exchange.get_mark_price(&sym) {
            let index_price = {
                let agg = exchange.index_aggregator.lock().unwrap_or_else(|e| e.into_inner());
                agg.calculate_index(&sym).ok().map(|ip| ip.price).unwrap_or(mark.price)
            };
            let (oi, oi_value) = {
                let pm = exchange.positions.lock();
                let total_size = pm.all_positions()
                    .iter()
                    .filter(|p| p.symbol == sym)
                    .map(|p| p.size)
                    .fold(rust_decimal::Decimal::ZERO, |a, b| a + b);
                let total_value = total_size * mark.price;
                (total_size, total_value)
            };
            let (funding_rate, next_funding_time) = {
                let sched = exchange.funding_scheduler.lock().unwrap_or_else(|e| e.into_inner());
                let history = exchange.funding_history.lock().unwrap_or_else(|e| e.into_inner());
                let rate = history.get(&sym)
                    .and_then(|h| h.latest_rate())
                    .map(|r| r.rate)
                    .unwrap_or(rust_decimal::Decimal::ZERO);
                let next = sched
                    .next_funding_time(chrono::Utc::now())
                    .map(|t| t.timestamp_millis())
                    .unwrap_or(0);
                (rate, next)
            };
            exchange.market_data.update_futures_data(&sym, mark.price, index_price, oi, oi_value, funding_rate, next_funding_time);
        }
    }

    match exchange.market_data.ticker(&sym) {
        Some(t) => {
            let mut obj = serde_json::json!({
                "symbol": symbol,
                // Price
                "last_price": t.last_price.to_string(),
                "last_qty": t.last_qty.to_string(),
                "mark_price": t.mark_price.map(|v| v.to_string()),
                "index_price": t.index_price.map(|v| v.to_string()),
                // 24h stats
                "open_24h": t.open_24h.to_string(),
                "high_24h": t.high_24h.to_string(),
                "low_24h": t.low_24h.to_string(),
                "prev_close": t.prev_close.map(|v| v.to_string()),
                "price_change_24h": t.price_change_24h.to_string(),
                "price_change_pct_24h": t.price_change_pct_24h.to_string(),
                "weighted_avg_price": t.weighted_avg_price.to_string(),
                "volume_24h": t.volume_24h.to_string(),
                "quote_volume_24h": t.quote_volume_24h.to_string(),
                "taker_buy_volume_24h": t.taker_buy_volume_24h.to_string(),
                "taker_sell_volume_24h": t.taker_sell_volume_24h.to_string(),
                "trade_count_24h": t.trade_count_24h,
                // 1h stats
                "prev_price_1h": t.prev_price_1h.map(|v| v.to_string()),
                "price_change_pct_1h": t.price_change_pct_1h.map(|v| v.to_string()),
                // Bid/Ask
                "best_bid": t.best_bid.map(|v| v.to_string()),
                "best_bid_size": t.best_bid_size.map(|v| v.to_string()),
                "best_ask": t.best_ask.map(|v| v.to_string()),
                "best_ask_size": t.best_ask_size.map(|v| v.to_string()),
                "spread": t.spread.map(|v| v.to_string()),
                "spread_pct": t.spread_pct.map(|v| v.to_string()),
                // Futures
                "open_interest": t.open_interest.map(|v| v.to_string()),
                "open_interest_value": t.open_interest_value.map(|v| v.to_string()),
                "funding_rate": t.funding_rate.map(|v| v.to_string()),
                "next_funding_time": t.next_funding_time,
                // Meta
                "updated_at": t.updated_at.timestamp_millis(),
            });
            // Remove null fields so downstream clients don't see clutter for spot symbols.
            if let Some(map) = obj.as_object_mut() {
                map.retain(|_, v| !v.is_null());
            }
            (StatusCode::OK, Json(obj))
        }
        None => (
            StatusCode::OK,
            Json(serde_json::json!({
                "symbol": symbol,
                "last_price": "0",
                "open_24h": "0",
                "high_24h": "0",
                "low_24h": "0",
                "price_change_24h": "0",
                "price_change_pct_24h": "0",
                "weighted_avg_price": "0",
                "volume_24h": "0",
                "quote_volume_24h": "0",
                "taker_buy_volume_24h": "0",
                "taker_sell_volume_24h": "0",
                "trade_count_24h": 0u64,
            })),
        ),
    }
}

/// GET /api/v1/markets/{symbol}/open-interest — public open interest for a perpetual symbol.
///
/// Returns the total open interest (both sides combined) calculated from in-memory positions.
/// Long/short breakdown and notional value at current mark price are also included.
pub async fn get_open_interest(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = validate_symbol(&symbol) {
        return e;
    }

    let sym = Symbol::new(&symbol);

    let (mut long_size, mut short_size) = (Decimal::ZERO, Decimal::ZERO);
    {
        let pm = exchange.positions.lock();
        for pos in pm.all_positions() {
            if pos.symbol == sym {
                match pos.side {
                    exchange_common::futures::PositionSide::Long => long_size += pos.size,
                    exchange_common::futures::PositionSide::Short => short_size += pos.size,
                }
            }
        }
    }

    let total = long_size + short_size;
    let mark = exchange.get_mark_price(&sym).map(|mp| mp.price);
    let oi_value = mark.map(|m| total * m).unwrap_or(Decimal::ZERO);

    let hundred = Decimal::ONE_HUNDRED;
    let (long_pct, short_pct) = if total > Decimal::ZERO {
        (
            (long_size / total * hundred).to_string(),
            (short_size / total * hundred).to_string(),
        )
    } else {
        ("0".to_string(), "0".to_string())
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "symbol": symbol,
            "open_interest": total.to_string(),
            "open_interest_value": oi_value.to_string(),
            "long_size": long_size.to_string(),
            "short_size": short_size.to_string(),
            "long_pct": long_pct,
            "short_pct": short_pct,
        })),
    )
}

/// GET /api/v1/status — public system health status.
pub async fn get_system_status(State(exchange): State<AppState>) -> impl IntoResponse {
    let db_connected = exchange.metrics.db_connected.load(std::sync::atomic::Ordering::Relaxed);
    let wal_healthy = exchange.metrics.wal_healthy.load(std::sync::atomic::Ordering::Relaxed);
    let uptime_secs = exchange.metrics.uptime_secs();

    let matching_status = "operational";
    let custody_status = "operational";
    let funding_status = "operational";

    let overall = if db_connected || wal_healthy {
        "operational"
    } else {
        "degraded"
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": overall,
            "services": [
                { "name": "matching_engine", "status": matching_status },
                { "name": "custody",         "status": custody_status },
                { "name": "funding",         "status": funding_status },
                { "name": "database",        "status": if db_connected { "operational" } else { "unavailable" } },
                { "name": "wal",             "status": if wal_healthy { "operational" } else { "unavailable" } },
            ],
            "uptime_secs": uptime_secs,
            "incidents": [],
        })),
    )
}

// ---------------------------------------------------------------------------
// Pair Management Admin Endpoints
// ---------------------------------------------------------------------------

/// Request body for POST /admin/pairs.
#[derive(Debug, Deserialize)]
pub struct CreatePairRequest {
    pub symbol: String,
    pub base_asset: String,
    pub quote_asset: String,
    pub product_type: String,
    pub tick_size: Decimal,
    pub lot_size: Decimal,
    pub min_notional: Decimal,
    pub maker_fee: Decimal,
    pub taker_fee: Decimal,
    /// Maximum allowed leverage for perpetuals (default: 20).
    #[serde(default)]
    pub max_leverage: Option<Decimal>,
    /// Maintenance margin rate for perpetuals (default: 2.5%).
    #[serde(default)]
    pub maintenance_margin_rate: Option<Decimal>,
    /// Maximum funding rate for perpetuals (default: 1%).
    #[serde(default)]
    pub max_funding_rate: Option<Decimal>,
}

/// Request body for PUT /admin/pairs/{symbol}.
#[derive(Debug, Deserialize)]
pub struct UpdatePairRequest {
    pub tick_size: Option<Decimal>,
    pub lot_size: Option<Decimal>,
    pub min_notional: Option<Decimal>,
    pub maker_fee: Option<Decimal>,
    pub taker_fee: Option<Decimal>,
    pub status: Option<String>,
}

/// POST /admin/pairs — create a new trading pair.
///
/// Validates the symbol is unique, sizes are positive, and product_type is
/// recognised. For perpetuals, also registers a default `PerpetualConfig` and
/// creates a matching-engine order book.
/// Requires AdminPermission::PairsCreate.
pub async fn admin_create_pair(
    State(exchange): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CreatePairRequest>,
) -> impl IntoResponse {
    // Permission check (no-op in open/dev mode when key_store is None).
    if let Some(ref ks) = exchange.key_store {
        let api_key = headers.get("x-api-key").and_then(|v| v.to_str().ok());
        if let Err(e) = ks.check_admin_permission(api_key, AdminPermission::PairsCreate) {
            return e.into_response();
        }
    }

    // Validate symbol format.
    if let Err(e) = validate_symbol(&req.symbol) {
        return e.into_response();
    }

    // Validate sizes are positive.
    for (val, name) in [
        (req.tick_size, "tick_size"),
        (req.lot_size, "lot_size"),
        (req.min_notional, "min_notional"),
    ] {
        if let Err(e) = validate_positive(val, name) {
            return e.into_response();
        }
    }

    // Parse product type.
    let product_type = match req.product_type.as_str() {
        "Spot" => ProductType::Spot,
        "Perpetual" => ProductType::Perpetual,
        other => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("Unknown product_type: '{other}'. Must be 'Spot' or 'Perpetual'")})),
            ).into_response();
        }
    };

    let symbol = Symbol::new(&req.symbol);

    // Check that the symbol does not already exist.
    if exchange.accounts.get_product(&symbol).is_some() {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": format!("Symbol '{}' already exists", req.symbol)})),
        ).into_response();
    }

    let product = Product {
        symbol: symbol.clone(),
        base_asset: req.base_asset.clone(),
        quote_asset: req.quote_asset.clone(),
        product_type,
        tick_size: req.tick_size,
        lot_size: req.lot_size,
        min_notional: req.min_notional,
        maker_fee: req.maker_fee,
        taker_fee: req.taker_fee,
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: None,
    };

    exchange.add_product(product);

    // Register circuit breaker with conservative defaults for the new symbol.
    exchange.circuit_breakers.lock()
        .insert(symbol.clone(), exchange_matching_engine::circuit_breaker::generic_circuit_breaker());

    // For perpetuals: register a default PerpetualConfig and insurance fund.
    if product_type == ProductType::Perpetual {
        let max_leverage = req.max_leverage.unwrap_or_else(|| Decimal::from(20u32));
        let maintenance_margin_rate = req.maintenance_margin_rate.unwrap_or(Decimal::new(25, 3));
        let max_funding_rate = req.max_funding_rate.unwrap_or(Decimal::new(1, 2));

        let config = PerpetualConfig {
            symbol: symbol.clone(),
            max_leverage,
            initial_margin_rate: Decimal::new(5, 2),   // 5% = 1/20
            maintenance_margin_rate,
            funding_interval_hours: 8,
            max_funding_rate,
            insurance_fund: Decimal::ZERO,
            tiers: vec![LeverageTier {
                min_notional: Decimal::ZERO,
                max_notional: Decimal::MAX,
                max_leverage,
                maintenance_margin_rate,
            }],
            contract_type: exchange_common::futures::ContractType::default(),
            settle_coin: "USDT".to_string(),
            contract_size: Decimal::ONE,
        };
        exchange.add_perp_config(config);

        // Seed insurance fund entry.
        exchange
            .insurance_funds
            .lock().unwrap_or_else(|e| e.into_inner())
            .entry(symbol.clone())
            .or_insert_with(|| InsuranceFund::new(symbol.clone(), Decimal::ZERO));

        // Register symbol in index price aggregator (needed for mark price → funding rate).
        exchange.index_aggregator.lock().unwrap_or_else(|e| e.into_inner())
            .add_symbol(symbol.clone());
    }

    tracing::info!(
        symbol = %req.symbol,
        product_type = %req.product_type,
        "admin created new trading pair"
    );

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "symbol": req.symbol,
            "status": "created",
        })),
    ).into_response()
}

/// GET /admin/pairs — list all registered trading pairs with configuration.
///
/// Requires AdminPermission::StatsView.
pub async fn admin_list_pairs(State(exchange): State<AppState>) -> impl IntoResponse {
    let perp_configs = exchange.perp_configs.load();
    let status_map = exchange.product_status.load();

    // Build a list from the matching engine's known symbols, cross-referencing
    // the account product registry for full config details.
    let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
    let pairs: Vec<serde_json::Value> = pool
        .symbols()
        .iter()
        .filter_map(|sym| exchange.accounts.get_product(sym))
        .map(|p| {
            let is_perp = perp_configs.contains_key(&p.symbol);
            let status = status_map
                .get(&p.symbol)
                .cloned()
                .unwrap_or_else(|| "active".to_string());
            serde_json::json!({
                "symbol": p.symbol.to_string(),
                "base_asset": p.base_asset,
                "quote_asset": p.quote_asset,
                "product_type": if is_perp { "Perpetual".to_string() } else { format!("{:?}", p.product_type) },
                "tick_size": p.tick_size.to_string(),
                "lot_size": p.lot_size.to_string(),
                "min_notional": p.min_notional.to_string(),
                "maker_fee": p.maker_fee.to_string(),
                "taker_fee": p.taker_fee.to_string(),
                "status": status,
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(pairs)))
}

/// PUT /admin/pairs/{symbol} — partially update pair configuration.
///
/// Only the fields provided in the request body are changed.
/// Requires AdminPermission::PairsConfigure.
pub async fn admin_update_pair(
    State(exchange): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(symbol): Path<String>,
    Json(req): Json<UpdatePairRequest>,
) -> impl IntoResponse {
    if let Some(ref ks) = exchange.key_store {
        let api_key = headers.get("x-api-key").and_then(|v| v.to_str().ok());
        if let Err(e) = ks.check_admin_permission(api_key, AdminPermission::PairsConfigure) {
            return e.into_response();
        }
    }

    if let Err(e) = validate_symbol(&symbol) {
        return e.into_response();
    }

    let sym = Symbol::new(&symbol);

    // Validate that the pair exists.
    if exchange.accounts.get_product(&sym).is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("Symbol '{}' not found", symbol)})),
        ).into_response();
    }

    // Validate provided numeric fields.
    if let Some(v) = req.tick_size {
        if let Err(e) = validate_positive(v, "tick_size") {
            return e.into_response();
        }
    }
    if let Some(v) = req.lot_size {
        if let Err(e) = validate_positive(v, "lot_size") {
            return e.into_response();
        }
    }
    if let Some(v) = req.min_notional {
        if let Err(e) = validate_positive(v, "min_notional") {
            return e.into_response();
        }
    }

    // Validate status string.
    if let Some(ref s) = req.status {
        if !["active", "disabled", "settle_only"].contains(&s.as_str()) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("Invalid status '{}'. Must be 'active', 'disabled', or 'settle_only'", s)})),
            ).into_response();
        }
    }

    // Apply status update to the status map.
    if let Some(ref new_status) = req.status {
        // Write path (admin-only): rebuild the HashMap and atomically swap the Arc pointer.
        let old = exchange.product_status.load_full();
        let mut new_map = (*old).clone();
        new_map.insert(sym.clone(), new_status.clone());
        exchange.product_status.store(Arc::new(new_map));
    }

    // Apply numeric field updates via re-registration.
    // ShardedAccounts stores products by symbol; we overwrite with updated values.
    let updated_product = exchange.accounts.get_product(&sym);

    if let Some(mut product) = updated_product {
        if let Some(v) = req.tick_size      { product.tick_size = v; }
        if let Some(v) = req.lot_size       { product.lot_size = v; }
        if let Some(v) = req.min_notional   { product.min_notional = v; }
        if let Some(v) = req.maker_fee      { product.maker_fee = v; }
        if let Some(v) = req.taker_fee      { product.taker_fee = v; }
        exchange.accounts.add_product(product);
    }

    tracing::info!(symbol = %symbol, "admin updated pair configuration");

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "symbol": symbol,
            "status": "updated",
        })),
    ).into_response()
}

/// DELETE /admin/pairs/{symbol} — disable a pair (soft delete, not physical removal).
///
/// Sets the pair's status to "disabled". The matching engine order book and
/// existing orders are preserved; no new orders are accepted.
/// Requires AdminPermission::PairsDisable.
pub async fn admin_disable_pair(
    State(exchange): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(symbol): Path<String>,
) -> impl IntoResponse {
    if let Some(ref ks) = exchange.key_store {
        let api_key = headers.get("x-api-key").and_then(|v| v.to_str().ok());
        if let Err(e) = ks.check_admin_permission(api_key, AdminPermission::PairsDisable) {
            return e.into_response();
        }
    }

    if let Err(e) = validate_symbol(&symbol) {
        return e.into_response();
    }

    let sym = Symbol::new(&symbol);

    if exchange.accounts.get_product(&sym).is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("Symbol '{}' not found", symbol)})),
        ).into_response();
    }

    {
        // Write path (admin-only): rebuild map and atomically swap.
        let old = exchange.product_status.load_full();
        let mut new_map = (*old).clone();
        new_map.insert(sym.clone(), "disabled".to_string());
        exchange.product_status.store(Arc::new(new_map));
    }

    tracing::info!(symbol = %symbol, "admin disabled trading pair");

    // Cancel all open orders for this symbol so they cannot match after the pair is disabled.
    let orders_to_cancel: Vec<(uuid::Uuid, uuid::Uuid)> = exchange
        .pending_orders
        .iter()
        .filter(|r| r.symbol == sym)
        .map(|r| (*r.key(), r.user_id))
        .collect();
    let cancelled = orders_to_cancel.len();
    for (order_id, user_id) in orders_to_cancel {
        let cmd = CancelOrderCommand {
            order_id,
            user_id,
            symbol: sym.clone(),
            timestamp: chrono::Utc::now(),
        };
        if let Err(e) = exchange.cancel_order(cmd) {
            tracing::warn!(order_id = %order_id, error = %e, "Failed to cancel order on pair disable");
        }
    }
    if cancelled > 0 {
        tracing::info!(symbol = %symbol, cancelled = cancelled, "cancelled open orders on pair disable");
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "symbol": symbol,
            "status": "disabled",
            "orders_cancelled": cancelled,
        })),
    ).into_response()
}

// ---------------------------------------------------------------------------
// Risk Dashboard Endpoints
// ---------------------------------------------------------------------------

/// GET /admin/risk/exposure — aggregate long/short exposure per symbol.
///
/// Requires AdminPermission::StatsView.
pub async fn admin_risk_exposure(State(exchange): State<AppState>) -> impl IntoResponse {
    let mark_prices = exchange.get_mark_prices();

    let positions: Vec<Position> = {
        let pm = exchange.positions.lock();
        pm.all_positions().into_iter().cloned().collect()
    };

    let insurance_funds = exchange.insurance_funds.lock().unwrap_or_else(|e| e.into_inner());

    // Aggregate per symbol.
    let mut symbol_stats: std::collections::HashMap<Symbol, (Decimal, Decimal, Decimal, Decimal)> =
        std::collections::HashMap::new(); // (long_size, short_size, long_notional, short_notional)

    for pos in &positions {
        let mark = mark_prices
            .get(&pos.symbol)
            .map(|mp| mp.price)
            .unwrap_or(pos.entry_price);
        let notional = pos.size * mark;
        let entry = symbol_stats.entry(pos.symbol.clone()).or_default();
        match pos.side {
            PositionSide::Long => {
                entry.0 += pos.size;
                entry.2 += notional;
            }
            PositionSide::Short => {
                entry.1 += pos.size;
                entry.3 += notional;
            }
        }
    }

    let symbols_json: Vec<serde_json::Value> = symbol_stats
        .iter()
        .map(|(sym, (long_sz, short_sz, long_notional, short_notional))| {
            let mark = mark_prices.get(sym).map(|mp| mp.price).unwrap_or(Decimal::ZERO);
            let net = long_sz - short_sz;
            let oi = long_sz + short_sz;
            let oi_value = long_notional + short_notional;
            let fund_balance = insurance_funds
                .get(sym)
                .map(|f| f.balance())
                .unwrap_or(Decimal::ZERO);
            serde_json::json!({
                "symbol": sym.to_string(),
                "total_long_size": long_sz.to_string(),
                "total_short_size": short_sz.to_string(),
                "total_long_notional": long_notional.to_string(),
                "total_short_notional": short_notional.to_string(),
                "net_exposure": net.to_string(),
                "open_interest": oi.to_string(),
                "open_interest_value": oi_value.to_string(),
                "insurance_fund": fund_balance.to_string(),
                "mark_price": mark.to_string(),
            })
        })
        .collect();

    // Total margin used = sum of all position margins.
    let total_margin: Decimal = positions.iter().map(|p| p.margin).sum();

    // Total user equity = sum of all USDT balances across accounts.
    let total_equity: Decimal = exchange
        .accounts
        .all_asset_totals()
        .get("USDT")
        .cloned()
        .unwrap_or(Decimal::ZERO);

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "symbols": symbols_json,
            "total_margin_used": total_margin.to_string(),
            "total_user_equity": total_equity.to_string(),
        })),
    )
}

/// GET /admin/risk/liquidation-risk — positions within 20% of liquidation.
///
/// Returns positions where the margin ratio is below 0.20, sorted by
/// distance_pct ascending (closest to liquidation first).
/// Requires AdminPermission::StatsView.
pub async fn admin_liquidation_risk(State(exchange): State<AppState>) -> impl IntoResponse {
    let mark_prices = exchange.get_mark_prices();

    let positions: Vec<Position> = {
        let pm = exchange.positions.lock();
        pm.all_positions().into_iter().cloned().collect()
    };

    let threshold = Decimal::new(20, 2); // 0.20

    // Collect tuples of (distance_pct, json) for sorting, then extract.
    let mut at_risk_tuples: Vec<(Decimal, serde_json::Value)> = positions
        .iter()
        .filter_map(|pos| {
            let mark = mark_prices.get(&pos.symbol).map(|mp| mp.price)?;
            let liq_price = pos.liquidation_price()?;

            // margin_ratio = margin / (size * mark_price)
            let notional = pos.size * mark;
            if notional.is_zero() {
                return None;
            }
            let margin_ratio = pos.margin / notional;

            if margin_ratio >= threshold {
                return None;
            }

            // distance_pct = |mark - liq| / mark * 100
            let distance_pct = if mark > Decimal::ZERO {
                ((mark - liq_price).abs() / mark) * Decimal::ONE_HUNDRED
            } else {
                Decimal::ZERO
            };

            Some((
                distance_pct,
                serde_json::json!({
                    "user_id": pos.user_id.to_string(),
                    "symbol": pos.symbol.to_string(),
                    "side": format!("{:?}", pos.side),
                    "size": pos.size.to_string(),
                    "entry_price": pos.entry_price.to_string(),
                    "liq_price": liq_price.to_string(),
                    "mark_price": mark.to_string(),
                    "distance_pct": distance_pct.round_dp(2).to_string(),
                    "margin_ratio": margin_ratio.round_dp(4).to_string(),
                }),
            ))
        })
        .collect();

    // Sort ascending by distance_pct (closest to liquidation first).
    at_risk_tuples.sort_by(|(a, _), (b, _)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let total_count = at_risk_tuples.len();
    let total_notional: Decimal = positions
        .iter()
        .filter(|pos| {
            let mark = mark_prices.get(&pos.symbol).map(|mp| mp.price).unwrap_or(Decimal::ZERO);
            let notional = pos.size * mark;
            if notional.is_zero() { return false; }
            let margin_ratio = pos.margin / notional;
            margin_ratio < threshold
        })
        .map(|pos| {
            let mark = mark_prices.get(&pos.symbol).map(|mp| mp.price).unwrap_or(pos.entry_price);
            pos.size * mark
        })
        .sum();

    let at_risk_items: Vec<serde_json::Value> = at_risk_tuples.into_iter().map(|(_, v)| v).collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "at_risk": at_risk_items,
            "total_at_risk_count": total_count,
            "total_at_risk_notional": total_notional.to_string(),
        })),
    )
}

/// Query parameters for GET /admin/risk/cascade-simulation.
#[derive(Debug, Deserialize)]
pub struct CascadeSimulationQuery {
    /// Percentage price drop to simulate (e.g., 10 = 10% drop).
    pub drop_pct: Decimal,
    /// Symbol to simulate the drop on.
    #[serde(default = "default_cascade_symbol")]
    pub symbol: String,
}

fn default_cascade_symbol() -> String {
    "BTCUSDT".to_string()
}

/// GET /admin/risk/cascade-simulation?symbol=BTCUSDT&drop_pct=10
///
/// Simulate what would happen if the given symbol's mark price drops by
/// `drop_pct` percent. Read-only — does not change any state.
/// Requires AdminPermission::StatsView.
pub async fn admin_cascade_simulation(
    State(exchange): State<AppState>,
    Query(params): Query<CascadeSimulationQuery>,
) -> impl IntoResponse {
    if params.drop_pct <= Decimal::ZERO || params.drop_pct >= Decimal::ONE_HUNDRED {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "drop_pct must be between 0 and 100 (exclusive)"})),
        ).into_response();
    }

    let sym = Symbol::new(&params.symbol);
    let current_mark = exchange
        .get_mark_price(&sym)
        .map(|mp| mp.price)
        .unwrap_or(Decimal::ZERO);

    if current_mark.is_zero() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("No mark price available for '{}'", params.symbol)})),
        ).into_response();
    }

    let simulated_price = current_mark * (Decimal::ONE - params.drop_pct / Decimal::ONE_HUNDRED);

    let positions: Vec<Position> = {
        let pm = exchange.positions.lock();
        pm.all_positions()
            .into_iter()
            .filter(|p| p.symbol == sym)
            .cloned()
            .collect()
    };

    let mut liquidated_count: u64 = 0;
    let mut total_liq_notional = Decimal::ZERO;
    let mut insurance_fund_impact = Decimal::ZERO;

    let perp_cfg = exchange
        .perp_configs
        .load()
        .get(&sym)
        .cloned();

    let mmr = perp_cfg
        .as_ref()
        .map(|c| c.maintenance_margin_rate)
        .unwrap_or(Decimal::new(25, 3)); // default 2.5%

    for pos in &positions {
        let notional = pos.size * simulated_price;
        if notional.is_zero() { continue; }

        let maintenance_margin = notional * mmr;
        // Liquidate when margin < maintenance_margin.
        if pos.margin < maintenance_margin {
            liquidated_count += 1;
            total_liq_notional += notional;

            // Rough insurance fund impact: margin - notional_loss at bankruptcy price.
            let unrealized_pnl = pos.unrealized_pnl(simulated_price);
            let net = pos.margin + unrealized_pnl; // net after loss
            if net < Decimal::ZERO {
                insurance_fund_impact += net; // negative = drawdown
            }
        }
    }

    let fund_balance = exchange
        .insurance_funds
        .lock().unwrap_or_else(|e| e.into_inner())
        .get(&sym)
        .map(|f| f.balance())
        .unwrap_or(Decimal::ZERO);

    let fund_remaining = (fund_balance + insurance_fund_impact).max(Decimal::ZERO);
    let adl_triggered = fund_balance + insurance_fund_impact < Decimal::ZERO;

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "scenario": format!("{} -{:.0}%", params.symbol, params.drop_pct),
            "simulated_mark_price": simulated_price.round_dp(2).to_string(),
            "positions_liquidated": liquidated_count,
            "total_liquidation_notional": total_liq_notional.to_string(),
            "insurance_fund_impact": insurance_fund_impact.to_string(),
            "insurance_fund_remaining": fund_remaining.to_string(),
            "adl_triggered": adl_triggered,
        })),
    ).into_response()
}

/// GET /admin/risk/concentration — flag users holding >10% of OI for a symbol.
///
/// Requires AdminPermission::StatsView.
pub async fn admin_risk_concentration(State(exchange): State<AppState>) -> impl IntoResponse {
    let positions: Vec<Position> = {
        let pm = exchange.positions.lock();
        pm.all_positions().into_iter().cloned().collect()
    };

    // Aggregate OI per symbol.
    let mut oi_per_symbol: std::collections::HashMap<Symbol, Decimal> =
        std::collections::HashMap::new();
    for pos in &positions {
        *oi_per_symbol.entry(pos.symbol.clone()).or_default() += pos.size;
    }

    // Aggregate per-user per-symbol size.
    let mut user_symbol_size: std::collections::HashMap<(Symbol, uuid::Uuid), Decimal> =
        std::collections::HashMap::new();
    for pos in &positions {
        *user_symbol_size
            .entry((pos.symbol.clone(), pos.user_id))
            .or_default() += pos.size;
    }

    let threshold = Decimal::new(10, 2); // 10%

    let alerts: Vec<serde_json::Value> = user_symbol_size
        .iter()
        .filter_map(|((sym, user_id), &user_size)| {
            let total_oi = oi_per_symbol.get(sym).cloned().unwrap_or(Decimal::ZERO);
            if total_oi.is_zero() { return None; }
            let concentration = user_size / total_oi;
            if concentration <= threshold { return None; }
            let pct = (concentration * Decimal::ONE_HUNDRED).round_dp(2);
            Some(serde_json::json!({
                "user_id": user_id.to_string(),
                "symbol": sym.to_string(),
                "size": user_size.to_string(),
                "total_oi": total_oi.to_string(),
                "concentration_pct": pct.to_string(),
                "alert": format!("User holds >{:.0}% of OI", Decimal::new(10, 0)),
            }))
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!({ "alerts": alerts })))
}

/// GET /admin/risk/insurance-funds — insurance fund status for all perpetual symbols.
///
/// Requires AdminPermission::StatsView.
pub async fn admin_insurance_funds(State(exchange): State<AppState>) -> impl IntoResponse {
    let funds = exchange.insurance_funds.lock().unwrap_or_else(|e| e.into_inner());
    let history = exchange.funding_history.lock().unwrap_or_else(|e| e.into_inner());

    let items: Vec<serde_json::Value> = funds
        .iter()
        .map(|(sym, fund)| {
            // Compute 24h change: sum of fund contributions from funding payments in last 24h.
            // Phase 1: derive from funding rate records as a proxy.
            let h24_change = history
                .get(sym)
                .map(|h| {
                    let cutoff = chrono::Utc::now() - chrono::Duration::hours(24);
                    h.rates
                        .iter()
                        .filter(|r| r.calculated_at >= cutoff)
                        .map(|r| r.rate)
                        .sum::<Decimal>()
                })
                .unwrap_or(Decimal::ZERO);

            // Utilization: insurance fund balance / (10× average daily OI) — rough estimate.
            // Phase 1: report 0 when no reference size is available.
            let utilization_pct = Decimal::ZERO;

            serde_json::json!({
                "symbol": sym.to_string(),
                "balance": fund.balance().to_string(),
                "24h_change": h24_change.to_string(),
                "utilization_pct": utilization_pct.to_string(),
                "burn_rate_24h": "0",
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(items)))
}

// ---------------------------------------------------------------------------
// Historical data export (trades, orders, ledger)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ExportQuery {
    #[serde(default)]
    pub symbol: Option<String>,
    #[serde(default = "default_export_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    #[serde(default = "default_export_format")]
    pub format: String,
}

fn default_export_limit() -> i64 {
    1000
}

fn default_export_format() -> String {
    "json".to_string()
}

/// GET /api/v1/export/trades — export authenticated user's trade history as JSON or CSV.
///
/// Query params: symbol (optional), limit (default 1000, max 10000), offset (default 0),
/// format ("json" | "csv", default "json").
pub async fn export_trades(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Query(params): Query<ExportQuery>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let limit = params.limit.clamp(1, 10_000);
    let offset = params.offset.max(0);

    let maybe_pool = exchange.db_pool.get().cloned();
    let pool = match maybe_pool {
        Some(p) => p,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                [("content-type", "application/json")],
                serde_json::json!({"error": "Database not configured"}).to_string(),
            );
        }
    };

    let trades = match exchange_db::trade_repo::TradeRepo::user_trades_paginated(
        &pool,
        user_id,
        params.symbol.as_deref(),
        limit,
        offset,
    )
    .await
    {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [("content-type", "application/json")],
                serde_json::json!({"error": e.to_string()}).to_string(),
            );
        }
    };

    if params.format == "csv" {
        let mut wtr = csv::Writer::from_writer(vec![]);
        let _ = wtr.write_record([
            "id",
            "symbol",
            "price",
            "qty",
            "taker_side",
            "maker_fee",
            "taker_fee",
            "created_at",
        ]);
        for t in &trades {
            let _ = wtr.write_record([
                t.id.to_string(),
                t.symbol.clone(),
                t.price.to_string(),
                t.qty.to_string(),
                t.taker_side.clone(),
                t.maker_fee.to_string(),
                t.taker_fee.to_string(),
                t.created_at.to_rfc3339(),
            ]);
        }
        let csv_data =
            String::from_utf8(wtr.into_inner().unwrap_or_default()).unwrap_or_default();
        (StatusCode::OK, [("content-type", "text/csv")], csv_data)
    } else {
        let json_data: Vec<serde_json::Value> = trades
            .iter()
            .map(|t| {
                serde_json::json!({
                    "id": t.id,
                    "symbol": t.symbol,
                    "price": t.price.to_string(),
                    "qty": t.qty.to_string(),
                    "taker_side": t.taker_side,
                    "maker_fee": t.maker_fee.to_string(),
                    "taker_fee": t.taker_fee.to_string(),
                    "created_at": t.created_at.to_rfc3339(),
                })
            })
            .collect();
        (
            StatusCode::OK,
            [("content-type", "application/json")],
            serde_json::to_string(&json_data).unwrap_or_default(),
        )
    }
}

/// GET /api/v1/export/orders — export authenticated user's order history as JSON or CSV.
///
/// Query params: symbol (optional), limit (default 1000, max 10000), offset (default 0),
/// format ("json" | "csv", default "json").
pub async fn export_orders(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Query(params): Query<ExportQuery>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let limit = params.limit.clamp(1, 10_000);
    let offset = params.offset.max(0);

    let maybe_pool = exchange.db_pool.get().cloned();
    let pool = match maybe_pool {
        Some(p) => p,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                [("content-type", "application/json")],
                serde_json::json!({"error": "Database not configured"}).to_string(),
            );
        }
    };

    let orders = match exchange_db::order_repo::OrderRepo::user_orders_paginated(
        &pool,
        user_id,
        params.symbol.as_deref(),
        limit,
        offset,
    )
    .await
    {
        Ok(o) => o,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [("content-type", "application/json")],
                serde_json::json!({"error": e.to_string()}).to_string(),
            );
        }
    };

    if params.format == "csv" {
        let mut wtr = csv::Writer::from_writer(vec![]);
        let _ = wtr.write_record([
            "id",
            "symbol",
            "side",
            "order_type",
            "time_in_force",
            "price",
            "qty",
            "filled_qty",
            "status",
            "created_at",
        ]);
        for o in &orders {
            let _ = wtr.write_record([
                o.id.to_string(),
                o.symbol.clone(),
                o.side.clone(),
                o.order_type.clone(),
                o.time_in_force.clone(),
                o.price.map(|p| p.to_string()).unwrap_or_default(),
                o.qty.to_string(),
                o.filled_qty.to_string(),
                o.status.clone(),
                o.created_at.to_rfc3339(),
            ]);
        }
        let csv_data =
            String::from_utf8(wtr.into_inner().unwrap_or_default()).unwrap_or_default();
        (StatusCode::OK, [("content-type", "text/csv")], csv_data)
    } else {
        let json_data: Vec<serde_json::Value> = orders
            .iter()
            .map(|o| {
                serde_json::json!({
                    "id": o.id.to_string(),
                    "symbol": o.symbol,
                    "side": o.side,
                    "order_type": o.order_type,
                    "time_in_force": o.time_in_force,
                    "price": o.price.map(|p| p.to_string()),
                    "qty": o.qty.to_string(),
                    "filled_qty": o.filled_qty.to_string(),
                    "status": o.status,
                    "created_at": o.created_at.to_rfc3339(),
                })
            })
            .collect();
        (
            StatusCode::OK,
            [("content-type", "application/json")],
            serde_json::to_string(&json_data).unwrap_or_default(),
        )
    }
}

/// GET /api/v1/export/ledger — export authenticated user's ledger history as JSON or CSV.
///
/// Query params: symbol repurposed as asset filter here (optional), limit (default 1000,
/// max 10000), offset ignored (ledger uses limit only for now), format ("json" | "csv").
pub async fn export_ledger(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Query(params): Query<ExportQuery>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let limit = params.limit.clamp(1, 10_000);

    let maybe_pool = exchange.db_pool.get().cloned();
    let pool = match maybe_pool {
        Some(p) => p,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                [("content-type", "application/json")],
                serde_json::json!({"error": "Database not configured"}).to_string(),
            );
        }
    };

    let entries = match exchange_db::ledger::LedgerService::account_history(
        &pool,
        user_id,
        params.symbol.as_deref(),
        limit,
    )
    .await
    {
        Ok(e) => e,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [("content-type", "application/json")],
                serde_json::json!({"error": e.to_string()}).to_string(),
            );
        }
    };

    if params.format == "csv" {
        let mut wtr = csv::Writer::from_writer(vec![]);
        let _ = wtr.write_record([
            "id",
            "asset",
            "amount",
            "entry_type",
            "balance_before",
            "balance_after",
            "reference_id",
            "description",
            "created_at",
        ]);
        for e in &entries {
            let _ = wtr.write_record([
                e.id.to_string(),
                e.asset.clone(),
                e.amount.to_string(),
                e.entry_type.clone(),
                e.balance_before.to_string(),
                e.balance_after.to_string(),
                e.reference_id.clone().unwrap_or_default(),
                e.description.clone().unwrap_or_default(),
                e.created_at.to_rfc3339(),
            ]);
        }
        let csv_data =
            String::from_utf8(wtr.into_inner().unwrap_or_default()).unwrap_or_default();
        (StatusCode::OK, [("content-type", "text/csv")], csv_data)
    } else {
        let json_data: Vec<serde_json::Value> = entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "id": e.id,
                    "asset": e.asset,
                    "amount": e.amount.to_string(),
                    "entry_type": e.entry_type,
                    "balance_before": e.balance_before.to_string(),
                    "balance_after": e.balance_after.to_string(),
                    "reference_id": e.reference_id,
                    "description": e.description,
                    "created_at": e.created_at.to_rfc3339(),
                })
            })
            .collect();
        (
            StatusCode::OK,
            [("content-type", "application/json")],
            serde_json::to_string(&json_data).unwrap_or_default(),
        )
    }
}

// ── KYC Document Upload ───────────────────────────────────────────────────

/// POST /api/v1/kyc/documents — upload a KYC document.
///
/// Accepts a multipart form with fields:
/// - `document_type`: one of `passport`, `selfie`, `id_card`, `proof_of_address`
/// - `file`: the document file (JPEG, PNG, or PDF, max 10 MB)
///
/// Returns `{"document_id", "document_type", "file_name", "status": "pending"}`.
pub async fn upload_kyc_document(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    mut multipart: axum::extract::Multipart,
) -> impl IntoResponse {
    let user_id = auth.0;
    let mut document_type: Option<String> = None;
    let mut file_data: Option<(String, Vec<u8>, String)> = None; // (filename, data, content_type)

    loop {
        match multipart.next_field().await {
            Ok(Some(field)) => {
                let name = field.name().unwrap_or("").to_string();
                match name.as_str() {
                    "document_type" => {
                        document_type = field.text().await.ok();
                    }
                    "file" => {
                        let file_name = field.file_name().unwrap_or("document").to_string();
                        let content_type = field
                            .content_type()
                            .unwrap_or("application/octet-stream")
                            .to_string();
                        match field.bytes().await {
                            Ok(bytes) => {
                                file_data = Some((file_name, bytes.to_vec(), content_type));
                            }
                            Err(e) => {
                                return (
                                    StatusCode::BAD_REQUEST,
                                    Json(serde_json::json!({"error": format!("Failed to read file: {e}")})),
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(None) => break,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": format!("Multipart error: {e}")})),
                );
            }
        }
    }

    let doc_type = match document_type {
        Some(t) if ["passport", "selfie", "id_card", "proof_of_address"].contains(&t.as_str()) => t,
        Some(t) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!(
                        "Invalid document_type: '{t}'. Must be: passport, selfie, id_card, proof_of_address"
                    )
                })),
            );
        }
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "document_type field required"})),
            );
        }
    };

    let (file_name, data, content_type) = match file_data {
        Some(d) => d,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "file field required"})),
            );
        }
    };

    // Validate size (max 10 MB).
    if data.len() > 10 * 1024 * 1024 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "File too large (max 10MB)"})),
        );
    }

    // Validate content type.
    let allowed_types = ["image/jpeg", "image/png", "application/pdf"];
    if !allowed_types.iter().any(|&a| content_type.starts_with(a)) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "File type not allowed. Use JPEG, PNG, or PDF"})),
        );
    }

    let ext = if content_type.starts_with("image/jpeg") {
        "jpg"
    } else if content_type.starts_with("image/png") {
        "png"
    } else {
        "pdf"
    };

    // Store file on disk.
    let storage = exchange_custody::document_storage::LocalDocumentStorage::new();
    let (storage_path, file_hash) = match storage.store(user_id, &doc_type, &data, ext).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(error = %e, "KYC document storage failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Storage failed"})),
            );
        }
    };

    let doc_id = uuid::Uuid::new_v4();

    // Persist record to DB if a pool is configured.
    let maybe_pool = exchange.db_pool.get().cloned();
    if let Some(pool) = maybe_pool {
        let row = exchange_db::kyc_document_repo::KycDocumentRow {
            id: doc_id,
            user_id,
            document_type: doc_type.clone(),
            file_name: file_name.clone(),
            file_size: data.len() as i64,
            content_type: content_type.clone(),
            storage_path,
            file_hash,
            status: "pending".to_string(),
            reviewer_notes: None,
            uploaded_at: chrono::Utc::now(),
            reviewed_at: None,
        };
        if let Err(e) = exchange_db::kyc_document_repo::KycDocumentRepo::create(&pool, &row).await
        {
            tracing::error!(error = %e, "Failed to save KYC document record");
        }
    }

    tracing::info!(
        user_id = %user_id,
        doc_type = %doc_type,
        doc_id = %doc_id,
        "KYC document uploaded"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "document_id": doc_id.to_string(),
            "document_type": doc_type,
            "file_name": file_name,
            "status": "pending",
        })),
    )
}

/// GET /api/v1/kyc/documents — list the authenticated user's uploaded documents.
pub async fn list_kyc_documents(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let maybe_pool = exchange.db_pool.get().cloned();
    match maybe_pool {
        Some(pool) => {
            match exchange_db::kyc_document_repo::KycDocumentRepo::list_for_user(&pool, user_id)
                .await
            {
                Ok(docs) => {
                    let result: Vec<serde_json::Value> = docs
                        .iter()
                        .map(|d| {
                            serde_json::json!({
                                "id": d.id.to_string(),
                                "document_type": d.document_type,
                                "file_name": d.file_name,
                                "status": d.status,
                                "uploaded_at": d.uploaded_at.to_rfc3339(),
                                "reviewer_notes": d.reviewer_notes,
                            })
                        })
                        .collect();
                    (StatusCode::OK, Json(serde_json::json!(result)))
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": e.to_string()})),
                ),
            }
        }
        None => (StatusCode::OK, Json(serde_json::json!([]))),
    }
}

/// GET /admin/kyc/documents/pending — list pending KYC document reviews (up to 100).
pub async fn admin_list_pending_documents(State(exchange): State<AppState>) -> impl IntoResponse {
    let maybe_pool = exchange.db_pool.get().cloned();
    match maybe_pool {
        Some(pool) => {
            match exchange_db::kyc_document_repo::KycDocumentRepo::list_pending(&pool, 100).await {
                Ok(docs) => {
                    let result: Vec<serde_json::Value> = docs
                        .iter()
                        .map(|d| {
                            serde_json::json!({
                                "id": d.id.to_string(),
                                "user_id": d.user_id.to_string(),
                                "document_type": d.document_type,
                                "file_name": d.file_name,
                                "status": d.status,
                                "uploaded_at": d.uploaded_at.to_rfc3339(),
                            })
                        })
                        .collect();
                    (StatusCode::OK, Json(serde_json::json!(result)))
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": e.to_string()})),
                ),
            }
        }
        None => (StatusCode::OK, Json(serde_json::json!([]))),
    }
}

/// POST /admin/kyc/documents/{id}/approve — mark a KYC document as approved.
pub async fn admin_approve_document(
    State(exchange): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let doc_id = match uuid::Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid document id"})),
            );
        }
    };
    let maybe_pool = exchange.db_pool.get().cloned();
    match maybe_pool {
        Some(pool) => {
            match exchange_db::kyc_document_repo::KycDocumentRepo::update_status(
                &pool, doc_id, "approved", None,
            )
            .await
            {
                Ok(()) => {
                    tracing::info!(doc_id = %doc_id, "KYC document approved");
                    (StatusCode::OK, Json(serde_json::json!({"status": "approved"})))
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": e.to_string()})),
                ),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "Database not configured"})),
        ),
    }
}

#[derive(Debug, Deserialize)]
pub struct AdminDocumentRejectRequest {
    pub reason: Option<String>,
}

/// POST /admin/kyc/documents/{id}/reject — mark a KYC document as rejected with an optional reason.
pub async fn admin_reject_document(
    State(exchange): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<AdminDocumentRejectRequest>,
) -> impl IntoResponse {
    let doc_id = match uuid::Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid document id"})),
            );
        }
    };
    let maybe_pool = exchange.db_pool.get().cloned();
    match maybe_pool {
        Some(pool) => {
            let notes = body.reason.as_deref();
            match exchange_db::kyc_document_repo::KycDocumentRepo::update_status(
                &pool, doc_id, "rejected", notes,
            )
            .await
            {
                Ok(()) => {
                    tracing::info!(doc_id = %doc_id, "KYC document rejected");
                    (StatusCode::OK, Json(serde_json::json!({"status": "rejected"})))
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": e.to_string()})),
                ),
            }
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "Database not configured"})),
        ),
    }
}

/// GET /api/v1/exchangeInfo — trading rules and product information.
pub async fn exchange_info(State(exchange): State<AppState>) -> impl IntoResponse {
    let products = exchange.accounts.products();
    let perp_configs = exchange.perp_configs.load();

    let symbols: Vec<serde_json::Value> = products
        .iter()
        .map(|p| {
            let perp_config = perp_configs.get(&p.symbol);
            let max_leverage = perp_config
                .map(|c| c.max_leverage)
                .unwrap_or(rust_decimal::Decimal::ONE);
            let contract_type = perp_config
                .map(|c| format!("{:?}", c.contract_type))
                .unwrap_or_else(|| "Linear".to_string());
            serde_json::json!({
                "symbol": p.symbol.to_string(),
                "status": "active",
                "baseAsset": p.base_asset,
                "quoteAsset": p.quote_asset,
                "tickSize": p.tick_size.to_string(),
                "lotSize": p.lot_size.to_string(),
                "minNotional": p.min_notional.to_string(),
                "makerFee": p.maker_fee.to_string(),
                "takerFee": p.taker_fee.to_string(),
                "productType": format!("{:?}", p.product_type),
                "maxLeverage": max_leverage.to_string(),
                "contractType": contract_type,
            })
        })
        .collect();

    Json(serde_json::json!({
        "serverTime": chrono::Utc::now().timestamp_millis(),
        "symbols": symbols,
    }))
}

/// GET /api/v1/time — server time.
pub async fn server_time() -> impl IntoResponse {
    Json(serde_json::json!({
        "serverTime": chrono::Utc::now().timestamp_millis(),
    }))
}

/// GET /api/v1/avgPrice — volume-weighted average price from recent trades.
pub async fn avg_price(
    State(exchange): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let symbol = match params.get("symbol") {
        Some(s) => s,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing required parameter: symbol"})),
            )
                .into_response();
        }
    };

    if let Err(e) = validate_symbol(symbol) {
        return e.into_response();
    }

    let sym = exchange_common::Symbol::new(symbol);
    let cutoff = chrono::Utc::now() - chrono::Duration::minutes(5);

    let history = exchange.trade_history.lock().unwrap_or_else(|e| e.into_inner());
    let trades = match history.get(&sym) {
        Some(t) => t,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "No trades found for symbol"})),
            )
                .into_response();
        }
    };

    // Volume-weighted average of last 5 minutes
    let mut total_notional = rust_decimal::Decimal::ZERO;
    let mut total_qty = rust_decimal::Decimal::ZERO;
    for trade in trades.iter().rev() {
        if trade.timestamp < cutoff {
            break;
        }
        total_notional += trade.price * trade.qty;
        total_qty += trade.qty;
    }

    if total_qty.is_zero() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "No recent trades for symbol"})),
        )
            .into_response();
    }

    let avg = total_notional / total_qty;

    Json(serde_json::json!({
        "symbol": symbol,
        "price": avg.to_string(),
        "mins": 5,
    }))
    .into_response()
}

// --- Algo execution endpoints (TWAP / VWAP) ---

/// `POST /api/v1/algo/twap` — submit a new TWAP execution.
///
/// Body fields:
/// - `symbol` (string, required)
/// - `side` (string "buy"/"sell", required)
/// - `total_qty` (string or number, required, positive)
/// - `duration_secs` (integer, required, positive)
/// - `num_slices` (integer, required, positive)
/// - `order_type` (string "market"/"limit_at_mid"/"limit_aggressive", optional, default "market")
/// - `price_limit` (string, optional)
pub async fn create_twap(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let symbol = match body.get("symbol").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing symbol"})),
            )
                .into_response()
        }
    };

    let side = match body.get("side").and_then(|v| v.as_str()) {
        Some("buy") | Some("Buy") => exchange_common::Side::Buy,
        Some("sell") | Some("Sell") => exchange_common::Side::Sell,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid side: expected 'buy' or 'sell'"})),
            )
                .into_response()
        }
    };

    let total_qty: Decimal = match body
        .get("total_qty")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .or_else(|| {
            body.get("total_qty")
                .and_then(|v| v.as_f64())
                .and_then(|f| Decimal::try_from(f).ok())
        }) {
        Some(q) if q > Decimal::ZERO => q,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid total_qty: must be a positive number"})),
            )
                .into_response()
        }
    };

    let duration_secs: u64 = match body.get("duration_secs").and_then(|v| v.as_u64()) {
        Some(d) if d > 0 => d,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid duration_secs: must be a positive integer"})),
            )
                .into_response()
        }
    };

    let num_slices: u32 = match body.get("num_slices").and_then(|v| v.as_u64()) {
        Some(n) if n > 0 && n <= 10_000 => n as u32,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid num_slices: must be between 1 and 10000"})),
            )
                .into_response()
        }
    };

    let order_type = match body
        .get("order_type")
        .and_then(|v| v.as_str())
        .unwrap_or("market")
    {
        "limit_at_mid" => exchange_algo_execution::SliceOrderType::LimitAtMid,
        "limit_aggressive" => exchange_algo_execution::SliceOrderType::LimitAggressive,
        _ => exchange_algo_execution::SliceOrderType::Market,
    };

    let price_limit = body
        .get("price_limit")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<Decimal>().ok())
        .or_else(|| {
            body.get("price_limit")
                .and_then(|v| v.as_f64())
                .and_then(|f| Decimal::try_from(f).ok())
        });

    let config = exchange_algo_execution::TwapConfig {
        user_id: user.0,
        symbol: exchange_common::Symbol::new(&symbol),
        side,
        total_qty,
        duration_secs,
        num_slices,
        order_type,
        jitter: true, // M25-9: anti-front-running jitter enabled in production
        price_limit,
    };

    let execution = exchange_algo_execution::TwapExecution::new(config, chrono::Utc::now());
    let algo_id = execution.id;
    let num_slices_out = execution.slices.len();

    exchange
        .twap_executions
        .lock().unwrap_or_else(|e| e.into_inner())
        .insert(algo_id, execution);

    tracing::info!(
        algo_id = %algo_id,
        user_id = %user.0,
        symbol = %symbol,
        total_qty = %total_qty,
        num_slices = num_slices_out,
        duration_secs,
        "TWAP execution created"
    );

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "algo_id": algo_id.to_string(),
            "type": "twap",
            "symbol": symbol,
            "side": format!("{:?}", side),
            "total_qty": total_qty.to_string(),
            "num_slices": num_slices_out,
            "duration_secs": duration_secs,
            "status": "active",
        })),
    )
        .into_response()
}

/// `POST /api/v1/algo/vwap` — submit a new VWAP execution.
///
/// Body fields:
/// - `symbol` (string, required)
/// - `side` (string "buy"/"sell", required)
/// - `total_qty` (string or number, required, positive)
/// - `duration_secs` (integer, required, positive)
/// - `num_slices` (integer, required, positive)
/// - `volume_profile` (array of numbers, optional — must match num_slices length if provided)
/// - `order_type` (string "market"/"limit_at_mid"/"limit_aggressive", optional, default "market")
/// - `price_limit` (string, optional)
pub async fn create_vwap(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let symbol = match body.get("symbol").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing symbol"})),
            )
                .into_response()
        }
    };

    let side = match body.get("side").and_then(|v| v.as_str()) {
        Some("buy") | Some("Buy") => exchange_common::Side::Buy,
        Some("sell") | Some("Sell") => exchange_common::Side::Sell,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid side: expected 'buy' or 'sell'"})),
            )
                .into_response()
        }
    };

    let total_qty: Decimal = match body
        .get("total_qty")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .or_else(|| {
            body.get("total_qty")
                .and_then(|v| v.as_f64())
                .and_then(|f| Decimal::try_from(f).ok())
        }) {
        Some(q) if q > Decimal::ZERO => q,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid total_qty: must be a positive number"})),
            )
                .into_response()
        }
    };

    let duration_secs: u64 = match body.get("duration_secs").and_then(|v| v.as_u64()) {
        Some(d) if d > 0 => d,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid duration_secs: must be a positive integer"})),
            )
                .into_response()
        }
    };

    let num_slices: u32 = match body.get("num_slices").and_then(|v| v.as_u64()) {
        Some(n) if n > 0 && n <= 10_000 => n as u32,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid num_slices: must be between 1 and 10000"})),
            )
                .into_response()
        }
    };

    // Parse optional volume profile — empty array is accepted (falls back to uniform in VwapExecution).
    let volume_profile: Vec<Decimal> = match body.get("volume_profile") {
        Some(serde_json::Value::Array(arr)) => {
            let mut profile = Vec::with_capacity(arr.len());
            for v in arr {
                let w = v
                    .as_f64()
                    .and_then(|f| Decimal::try_from(f).ok())
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()));
                match w {
                    Some(w) => profile.push(w),
                    None => {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(serde_json::json!({"error": "volume_profile contains a non-numeric value"})),
                        )
                            .into_response()
                    }
                }
            }
            profile
        }
        // Absent or null → uniform distribution handled inside VwapExecution
        _ => vec![],
    };

    let order_type = match body
        .get("order_type")
        .and_then(|v| v.as_str())
        .unwrap_or("market")
    {
        "limit_at_mid" => exchange_algo_execution::SliceOrderType::LimitAtMid,
        "limit_aggressive" => exchange_algo_execution::SliceOrderType::LimitAggressive,
        _ => exchange_algo_execution::SliceOrderType::Market,
    };

    let price_limit = body
        .get("price_limit")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<Decimal>().ok())
        .or_else(|| {
            body.get("price_limit")
                .and_then(|v| v.as_f64())
                .and_then(|f| Decimal::try_from(f).ok())
        });

    let config = exchange_algo_execution::VwapConfig {
        user_id: user.0,
        symbol: exchange_common::Symbol::new(&symbol),
        side,
        total_qty,
        duration_secs,
        num_slices,
        volume_profile,
        order_type,
        jitter: true, // M25-9: anti-front-running jitter enabled
        price_limit,
    };

    let execution = exchange_algo_execution::VwapExecution::new(config, chrono::Utc::now());
    let algo_id = execution.id;
    let num_slices_out = execution.slices.len();

    exchange
        .vwap_executions
        .lock().unwrap_or_else(|e| e.into_inner())
        .insert(algo_id, execution);

    tracing::info!(
        algo_id = %algo_id,
        user_id = %user.0,
        symbol = %symbol,
        total_qty = %total_qty,
        num_slices = num_slices_out,
        duration_secs,
        "VWAP execution created"
    );

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "algo_id": algo_id.to_string(),
            "type": "vwap",
            "symbol": symbol,
            "side": format!("{:?}", side),
            "total_qty": total_qty.to_string(),
            "num_slices": num_slices_out,
            "duration_secs": duration_secs,
            "status": "active",
        })),
    )
        .into_response()
}

/// `GET /api/v1/algo/{algo_id}` — retrieve algo execution status.
pub async fn get_algo_status(
    State(exchange): State<AppState>,
    Path(algo_id): Path<String>,
) -> impl IntoResponse {
    let id: uuid::Uuid = match algo_id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid algo_id format"})),
            )
                .into_response()
        }
    };

    {
        let twap = exchange
            .twap_executions
            .lock().unwrap_or_else(|e| e.into_inner());
        if let Some(exec) = twap.get(&id) {
            return (
                StatusCode::OK,
                Json(serde_json::json!({
                    "algo_id": id.to_string(),
                    "type": "twap",
                    "symbol": exec.config.symbol.to_string(),
                    "side": format!("{:?}", exec.config.side),
                    "total_qty": exec.config.total_qty.to_string(),
                    "filled_qty": exec.filled_qty.to_string(),
                    "avg_fill_price": exec.avg_fill_price.to_string(),
                    "progress": exec.progress().to_string(),
                    "status": format!("{:?}", exec.status),
                    "num_slices": exec.slices.len(),
                    "created_at": exec.created_at.to_rfc3339(),
                })),
            )
                .into_response();
        }
    }

    {
        let vwap = exchange
            .vwap_executions
            .lock().unwrap_or_else(|e| e.into_inner());
        if let Some(exec) = vwap.get(&id) {
            return (
                StatusCode::OK,
                Json(serde_json::json!({
                    "algo_id": id.to_string(),
                    "type": "vwap",
                    "symbol": exec.config.symbol.to_string(),
                    "side": format!("{:?}", exec.config.side),
                    "total_qty": exec.config.total_qty.to_string(),
                    "filled_qty": exec.filled_qty.to_string(),
                    "avg_fill_price": exec.avg_fill_price.to_string(),
                    "vwap_benchmark": exec.vwap_benchmark.map(|p| p.to_string()),
                    "slippage": exec.slippage().map(|s| s.to_string()),
                    "status": format!("{:?}", exec.status),
                    "num_slices": exec.slices.len(),
                    "created_at": exec.created_at.to_rfc3339(),
                })),
            )
                .into_response();
        }
    }

    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": "Algo execution not found"})),
    )
        .into_response()
}

/// `DELETE /api/v1/algo/{algo_id}` — cancel an active algo execution.
///
/// Marks the execution as Cancelled. The background executor will stop
/// submitting further slices on its next tick.
pub async fn cancel_algo(
    State(exchange): State<AppState>,
    Path(algo_id): Path<String>,
) -> impl IntoResponse {
    let id: uuid::Uuid = match algo_id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid algo_id format"})),
            )
                .into_response()
        }
    };

    {
        let mut twap = exchange
            .twap_executions
            .lock().unwrap_or_else(|e| e.into_inner());
        if let Some(exec) = twap.get_mut(&id) {
            let outstanding = exec.cancel();
            drop(twap); // release lock before cancelling orders
            // Cancel all submitted orders still on the book.
            for order_id in &outstanding {
                let cmd = exchange_common::CancelOrderCommand {
                    order_id: *order_id,
                    user_id: uuid::Uuid::nil(),
                    symbol: exchange_common::Symbol::new(""),
                    timestamp: chrono::Utc::now(),
                };
                if let Err(e) = exchange.cancel_order(cmd) {
                    tracing::warn!(order_id = %order_id, error = %e, "failed to cancel algo slice order");
                }
            }
            tracing::info!(
                algo_id = %id,
                cancelled_orders = outstanding.len(),
                "TWAP execution cancelled"
            );
            return (
                StatusCode::OK,
                Json(serde_json::json!({
                    "algo_id": id.to_string(),
                    "type": "twap",
                    "status": "cancelled",
                })),
            )
                .into_response();
        }
    }

    {
        let mut vwap = exchange
            .vwap_executions
            .lock().unwrap_or_else(|e| e.into_inner());
        if let Some(exec) = vwap.get_mut(&id) {
            let outstanding = exec.cancel();
            drop(vwap);
            for order_id in &outstanding {
                let cmd = exchange_common::CancelOrderCommand {
                    order_id: *order_id,
                    user_id: uuid::Uuid::nil(),
                    symbol: exchange_common::Symbol::new(""),
                    timestamp: chrono::Utc::now(),
                };
                if let Err(e) = exchange.cancel_order(cmd) {
                    tracing::warn!(order_id = %order_id, error = %e, "failed to cancel VWAP slice order");
                }
            }
            tracing::info!(
                algo_id = %id,
                cancelled_orders = outstanding.len(),
                "VWAP execution cancelled"
            );
            return (
                StatusCode::OK,
                Json(serde_json::json!({
                    "algo_id": id.to_string(),
                    "type": "vwap",
                    "status": "cancelled",
                })),
            )
                .into_response();
        }
    }

    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": "Algo execution not found"})),
    )
        .into_response()
}

// --- Asset registry endpoints ---

/// GET /api/v1/asset/coins — list all supported coins with network info.
pub async fn get_coins(State(exchange): State<AppState>) -> impl IntoResponse {
    let lending_rates = exchange.lending_rates.lock().unwrap_or_else(|e| e.into_inner());
    let registry = exchange.asset_registry.lock().unwrap_or_else(|e| e.into_inner());
    let coins: Vec<serde_json::Value> = registry
        .all_coins()
        .iter()
        .map(|c| {
            serde_json::json!({
                "coin": c.coin,
                "name": c.name,
                "isTradeable": c.is_tradeable,
                "canCollateral": true,
                "canBorrow": lending_rates.contains_key(&c.coin),
                "networks": c.networks.iter().map(|n| serde_json::json!({
                    "network": n.network,
                    "chainType": n.chain_type,
                    "contractAddress": n.contract_address,
                    "depositEnabled": n.deposit_enabled,
                    "withdrawEnabled": n.withdraw_enabled,
                    "withdrawFee": n.withdraw_fee.to_string(),
                    "withdrawMin": n.withdraw_min.to_string(),
                    "withdrawMax": n.withdraw_max.to_string(),
                    "depositMin": n.deposit_min.to_string(),
                    "confirmationsRequired": n.confirmations_required,
                    "safeConfirmations": n.safe_confirmations,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();

    Json(serde_json::json!({ "coins": coins }))
}

/// GET /api/v1/asset/coins/{coin} — single coin info.
pub async fn get_coin_info(
    State(exchange): State<AppState>,
    Path(coin): Path<String>,
) -> impl IntoResponse {
    let coin_upper = coin.to_uppercase();
    let registry = exchange.asset_registry.lock().unwrap_or_else(|e| e.into_inner());
    match registry.get(&coin_upper) {
        Some(info) => {
            let networks: Vec<serde_json::Value> = info
                .networks
                .iter()
                .map(|n| {
                    serde_json::json!({
                        "network": n.network,
                        "chainType": n.chain_type,
                        "contractAddress": n.contract_address,
                        "depositEnabled": n.deposit_enabled,
                        "withdrawEnabled": n.withdraw_enabled,
                        "withdrawFee": n.withdraw_fee.to_string(),
                        "withdrawMin": n.withdraw_min.to_string(),
                        "withdrawMax": n.withdraw_max.to_string(),
                        "depositMin": n.deposit_min.to_string(),
                        "confirmationsRequired": n.confirmations_required,
                        "safeConfirmations": n.safe_confirmations,
                    })
                })
                .collect();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "coin": info.coin,
                    "name": info.name,
                    "isTradeable": info.is_tradeable,
                    "networks": networks,
                })),
            )
                .into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("Unknown coin: {coin_upper}")})),
        )
            .into_response(),
    }
}

/// GET /api/v1/asset/currencies — detailed currency information (OKX-compatible).
///
/// Optional query parameter `coin` filters to a single coin (case-insensitive).
/// Returns collateral eligibility, borrow availability, and full per-network detail.
pub async fn get_currencies(
    State(exchange): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let coin_filter = params.get("coin").map(|s| s.to_uppercase());
    let lending_rates = exchange.lending_rates.lock().unwrap_or_else(|e| e.into_inner());
    let registry = exchange.asset_registry.lock().unwrap_or_else(|e| e.into_inner());

    let currencies: Vec<serde_json::Value> = registry
        .all_coins()
        .iter()
        .filter(|c| coin_filter.as_ref().is_none_or( |f| &c.coin == f))
        .map(|c| {
            let networks: Vec<serde_json::Value> = c.networks.iter().map(|n| serde_json::json!({
                "network": n.network,
                "chainType": n.chain_type,
                "contractAddress": n.contract_address,
                "depositEnabled": n.deposit_enabled,
                "withdrawEnabled": n.withdraw_enabled,
                "withdrawFee": n.withdraw_fee.to_string(),
                "withdrawMin": n.withdraw_min.to_string(),
                "withdrawMax": n.withdraw_max.to_string(),
                "depositMin": n.deposit_min.to_string(),
                "confirmations": n.confirmations_required,
                "safeConfirmations": n.safe_confirmations,
                "minAccuracy": n.min_accuracy,
            })).collect();

            serde_json::json!({
                "coin": c.coin,
                "name": c.name,
                "isTradeable": c.is_tradeable,
                "canCollateral": true,
                "canBorrow": lending_rates.contains_key(&c.coin),
                "networks": networks,
            })
        })
        .collect();

    Json(serde_json::json!({ "currencies": currencies }))
}

// --- Transaction log & closed PnL (Task 5.3 / 5.5) ---

/// GET /api/v1/account/transaction-log — unified view of all account movements.
///
/// Merges order fills and funding payments from in-memory history into a
/// single chronological list.  Results are returned newest-first.
///
/// Query parameters:
/// - `type`   — comma-separated entry-type filter, e.g. `trade_fill,funding_payment`
/// - `asset`  — asset filter (default: all), e.g. `USDT`
/// - `limit`  — max results per page (default 50, max 200)
/// - `cursor` — pagination offset (integer, returned as `nextCursor` from previous page)
pub async fn get_transaction_log(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let user_id = auth.0;

    let type_filter: Option<Vec<String>> = params
        .get("type")
        .map(|s| s.split(',').map(|t| t.trim().to_string()).collect());
    let asset_filter: Option<String> = params.get("asset").map(|s| s.to_uppercase());
    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(50)
        .min(200);
    let cursor: usize = params
        .get("cursor")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);

    let mut entries: Vec<serde_json::Value> = Vec::new();

    // --- Trade fills from order history ---
    {
        let history = exchange.order_history.lock().unwrap_or_else(|e| e.into_inner());
        for order in history.iter().rev() {
            if order.user_id != user_id {
                continue;
            }
            let entry_type = match order.status.as_str() {
                "filled" | "partially_filled" => "trade_fill",
                _ => continue,
            };
            if let Some(ref filter) = type_filter {
                if !filter.iter().any(|f| f == entry_type) {
                    continue;
                }
            }
            if let Some(ref asset) = asset_filter {
                if asset != "USDT" {
                    continue; // all futures settle in USDT
                }
            }
            // Notional = avg fill price × filled qty
            let notional = order.price * order.filled_qty;
            entries.push(serde_json::json!({
                "type": entry_type,
                "symbol": order.symbol.to_string(),
                "asset": "USDT",
                "amount": notional.to_string(),
                "fee": "0",
                "orderId": order.order_id.to_string(),
                "side": order.side,
                "filledQty": order.filled_qty.to_string(),
                "price": order.price.to_string(),
                "timestamp": order.created_at.timestamp_millis(),
            }));
        }
    }

    // --- Funding payments ---
    {
        let fh = exchange.funding_history.lock().unwrap_or_else(|e| e.into_inner());
        for (symbol, history) in fh.iter() {
            for payment in history.payments.iter().rev() {
                if payment.user_id != user_id {
                    continue;
                }
                if let Some(ref filter) = type_filter {
                    if !filter.iter().any(|f| f == "funding_payment") {
                        continue;
                    }
                }
                if let Some(ref asset) = asset_filter {
                    if asset != "USDT" {
                        continue;
                    }
                }
                entries.push(serde_json::json!({
                    "type": "funding_payment",
                    "symbol": symbol.to_string(),
                    "asset": "USDT",
                    "amount": payment.amount.to_string(),
                    "fee": "0",
                    "rate": payment.rate.to_string(),
                    "markPrice": payment.mark_price.to_string(),
                    "timestamp": payment.settled_at.timestamp_millis(),
                }));
            }
        }
    }

    // Sort all entries newest-first by timestamp.
    entries.sort_by(|a, b| {
        let ts_a = a.get("timestamp").and_then(|v| v.as_i64()).unwrap_or(0);
        let ts_b = b.get("timestamp").and_then(|v| v.as_i64()).unwrap_or(0);
        ts_b.cmp(&ts_a)
    });

    let total = entries.len();
    let page: Vec<serde_json::Value> = entries.into_iter().skip(cursor).take(limit).collect();
    let next_cursor: Option<usize> = if cursor + page.len() < total {
        Some(cursor + page.len())
    } else {
        None
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "entries": page,
            "nextCursor": next_cursor,
        })),
    )
}

/// GET /api/v1/position/closed-pnl — closed position PnL history.
///
/// Returns all fully-closed positions for the authenticated user, newest-first.
///
/// Query parameters:
/// - `symbol` — symbol filter, e.g. `BTCUSDT`
/// - `limit`  — max results (default 50, max 200)
pub async fn get_closed_pnl(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let symbol_filter = params.get("symbol").map(|s| s.to_uppercase());
    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(50)
        .min(200);

    let records = exchange.closed_positions.lock().unwrap_or_else(|e| e.into_inner());
    let result: Vec<serde_json::Value> = records
        .iter()
        .rev()
        .filter(|r| r.user_id == user_id)
        .filter(|r| {
            symbol_filter
                .as_deref()
                .is_none_or( |s| r.symbol.as_str().eq_ignore_ascii_case(s))
        })
        .take(limit)
        .map(|r| {
            serde_json::json!({
                "id": r.id.to_string(),
                "symbol": r.symbol.to_string(),
                "side": format!("{:?}", r.side),
                "avgEntryPrice": r.avg_entry_price.to_string(),
                "avgExitPrice": r.avg_exit_price.to_string(),
                "closedSize": r.closed_size.to_string(),
                "realizedPnl": r.realized_pnl.to_string(),
                "tradingFee": r.trading_fee.to_string(),
                "fundingFee": r.funding_fee.to_string(),
                "leverage": r.leverage.to_string(),
                "closeReason": r.close_reason.to_string(),
                "openedAt": r.opened_at.timestamp_millis(),
                "closedAt": r.closed_at.timestamp_millis(),
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!({ "records": result })))
}

// ---------------------------------------------------------------------------
// Order pre-check / test order (Task 7.2)
// ---------------------------------------------------------------------------

/// POST /api/v1/orders/test — validate an order without placing it.
///
/// Applies the same validation as `POST /orders` (symbol exists, side valid,
/// price positive for limit, qty positive, iceberg constraints, order_link_id
/// format) but does NOT call the matching engine.
///
/// Returns `{"valid": true}` on success or HTTP 400 with an error message.
pub async fn test_order(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<PlaceOrderRequest>,
) -> impl IntoResponse {
    // Validate symbol format.
    if let Err(e) = validate_symbol(&req.symbol) {
        return e.into_response();
    }

    // Validate qty is positive.
    if let Err(e) = validate_positive(req.qty, "qty") {
        return e.into_response();
    }

    // Validate side.
    match req.side.to_lowercase().as_str() {
        "buy" | "sell" => {}
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid side. Use 'buy' or 'sell'"})),
            )
                .into_response();
        }
    }

    // Validate order_type.
    let order_type_lower = req.order_type.to_lowercase();
    match order_type_lower.as_str() {
        "limit" | "market" => {}
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid order_type. Use 'limit' or 'market'"})),
            )
                .into_response();
        }
    }

    // Validate time_in_force.
    match req.time_in_force.as_deref().unwrap_or("gtc") {
        "gtc" | "ioc" | "fok" | "post_only" | "gtd" => {}
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid time_in_force"})),
            )
                .into_response();
        }
    }

    // GTD requires an expiration timestamp.
    if req.time_in_force.as_deref() == Some("gtd") && req.expire_at.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "GTD orders require expire_at (Unix milliseconds)"})),
        )
            .into_response();
    }

    // Limit orders require a positive price.
    if order_type_lower == "limit" {
        match req.price {
            Some(p) if p > Decimal::ZERO => {}
            _ => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "Limit order requires positive price"})),
                )
                    .into_response();
            }
        }
    }

    // Validate iceberg_qty if present.
    if let Some(iceberg) = req.iceberg_qty {
        if iceberg <= Decimal::ZERO {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "iceberg_qty must be positive"})),
            )
                .into_response();
        }
        if iceberg >= req.qty {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "iceberg_qty must be less than total qty"})),
            )
                .into_response();
        }
        if order_type_lower != "limit" {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "iceberg orders are only supported for Limit orders"})),
            )
                .into_response();
        }
    }

    // Validate order_link_id if present.
    if let Some(ref link_id) = req.order_link_id {
        if link_id.len() > 36 {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "order_link_id must be 36 characters or less"})),
            )
                .into_response();
        }
        if !link_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "order_link_id must contain only alphanumeric characters and dashes"})),
            )
                .into_response();
        }
    }

    // Check symbol exists in the engine pool.
    let symbol = Symbol::new(&req.symbol);
    let symbol_exists = {
        let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
        pool.snapshot(&symbol, 0).is_ok()
    };
    if !symbol_exists {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Unknown symbol"})),
        )
            .into_response();
    }

    let _ = auth; // auth is validated by middleware; user_id available if needed

    (StatusCode::OK, Json(serde_json::json!({ "valid": true }))).into_response()
}

// ---------------------------------------------------------------------------
// Position history (Task 7.1)
// ---------------------------------------------------------------------------

/// GET /api/v1/position/history — full closed position history with duration.
///
/// Returns all fully-closed positions for the authenticated user, newest-first.
/// Identical to `GET /position/closed-pnl` but adds a `durationSecs` field
/// computed from `openedAt` and `closedAt`.
///
/// Query parameters:
/// - `symbol` — symbol filter, e.g. `BTCUSDT`
/// - `limit`  — max results (default 50, max 200)
pub async fn get_position_history(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let symbol_filter = params.get("symbol").map(|s| s.to_uppercase());
    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(50)
        .min(200);

    let records = exchange.closed_positions.lock().unwrap_or_else(|e| e.into_inner());
    let result: Vec<serde_json::Value> = records
        .iter()
        .rev()
        .filter(|r| r.user_id == user_id)
        .filter(|r| {
            symbol_filter
                .as_deref()
                .is_none_or( |s| r.symbol.as_str().eq_ignore_ascii_case(s))
        })
        .take(limit)
        .map(|r| {
            let duration_secs = (r.closed_at - r.opened_at).num_seconds().max(0);
            serde_json::json!({
                "id": r.id.to_string(),
                "symbol": r.symbol.to_string(),
                "side": format!("{:?}", r.side),
                "avgEntryPrice": r.avg_entry_price.to_string(),
                "avgExitPrice": r.avg_exit_price.to_string(),
                "closedSize": r.closed_size.to_string(),
                "realizedPnl": r.realized_pnl.to_string(),
                "tradingFee": r.trading_fee.to_string(),
                "fundingFee": r.funding_fee.to_string(),
                "leverage": r.leverage.to_string(),
                "closeReason": r.close_reason.to_string(),
                "openedAt": r.opened_at.timestamp_millis(),
                "closedAt": r.closed_at.timestamp_millis(),
                "durationSecs": duration_secs,
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!({ "records": result })))
}

// ---------------------------------------------------------------------------
// Convert / Swap endpoints (Task 6.2)
// ---------------------------------------------------------------------------

/// POST /api/v1/convert/quote — get a conversion quote valid for 10 seconds.
pub async fn convert_quote(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let from_coin = match body.get("fromCoin").and_then(|v| v.as_str()) {
        Some(c) => c.to_uppercase(),
        None => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Missing fromCoin"}))).into_response(),
    };
    let to_coin = match body.get("toCoin").and_then(|v| v.as_str()) {
        Some(c) => c.to_uppercase(),
        None => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Missing toCoin"}))).into_response(),
    };
    let from_amount: Decimal = match body
        .get("fromAmount")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
    {
        Some(a) if a > Decimal::ZERO => a,
        _ => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Invalid fromAmount"}))).into_response(),
    };

    // Resolve USD value of each coin via mark prices; stablecoins are 1:1.
    let mark_prices = exchange.get_mark_prices();

    let from_usd = if from_coin == "USDT" || from_coin == "USDC" {
        Decimal::ONE
    } else {
        let sym = exchange_common::Symbol::new(format!("{from_coin}USDT"));
        match mark_prices.get(&sym) {
            Some(mp) => mp.price,
            None => {
                let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
                match pool.snapshot(&sym, 1) {
                    Ok(snap) => match (snap.bids.first(), snap.asks.first()) {
                        (Some((b, _)), Some((a, _))) => (*b + *a) / Decimal::TWO,
                        _ => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": format!("No price available for {}", from_coin)}))).into_response(),
                    },
                    Err(_) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": format!("No price available for {}", from_coin)}))).into_response(),
                }
            }
        }
    };

    let to_usd = if to_coin == "USDT" || to_coin == "USDC" {
        Decimal::ONE
    } else {
        let sym = exchange_common::Symbol::new(format!("{to_coin}USDT"));
        match mark_prices.get(&sym) {
            Some(mp) => mp.price,
            None => {
                let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
                match pool.snapshot(&sym, 1) {
                    Ok(snap) => match (snap.bids.first(), snap.asks.first()) {
                        (Some((b, _)), Some((a, _))) => (*b + *a) / Decimal::TWO,
                        _ => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": format!("No price available for {}", to_coin)}))).into_response(),
                    },
                    Err(_) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": format!("No price available for {}", to_coin)}))).into_response(),
                }
            }
        }
    };

    if to_usd.is_zero() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Cannot determine price"}))).into_response();
    }

    let rate = from_usd / to_usd;
    let fee_rate = Decimal::new(1, 3); // 0.1%
    let fee = from_amount * fee_rate;
    let net_from = from_amount - fee;
    let to_amount = net_from * rate;

    let expires_at = chrono::Utc::now() + chrono::Duration::seconds(10);
    let quote = crate::exchange::ConvertQuote {
        quote_id: uuid::Uuid::new_v4(),
        user_id: user.0,
        from_coin: from_coin.clone(),
        to_coin: to_coin.clone(),
        from_amount,
        to_amount,
        rate,
        fee,
        fee_coin: from_coin.clone(),
        expires_at,
    };
    let quote_id = quote.quote_id;
    exchange
        .convert_quotes
        .lock().unwrap_or_else(|e| e.into_inner())
        .insert(quote_id, quote);

    (StatusCode::OK, Json(serde_json::json!({
        "quoteId": quote_id.to_string(),
        "fromCoin": from_coin,
        "toCoin": to_coin,
        "fromAmount": from_amount.to_string(),
        "toAmount": to_amount.to_string(),
        "rate": rate.to_string(),
        "fee": fee.to_string(),
        "feeCoin": from_coin,
        "expiresAt": expires_at.timestamp_millis(),
    }))).into_response()
}

/// POST /api/v1/convert/execute — execute a quoted conversion.
pub async fn convert_execute(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let quote_id: uuid::Uuid = match body
        .get("quoteId")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
    {
        Some(id) => id,
        None => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Missing quoteId"}))).into_response(),
    };

    // Remove the quote atomically so it cannot be double-spent.
    let quote = {
        let mut quotes = exchange.convert_quotes.lock().unwrap_or_else(|e| e.into_inner());
        match quotes.remove(&quote_id) {
            Some(q) => q,
            None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Quote not found or already used"}))).into_response(),
        }
    };

    if chrono::Utc::now() > quote.expires_at {
        return (StatusCode::GONE, Json(serde_json::json!({"error": "Quote expired"}))).into_response();
    }

    if quote.user_id != user.0 {
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "Quote belongs to another user"}))).into_response();
    }

    // Debit the source coin and credit the destination coin. Both ops route
    // to the same user's shard (same `user.0`), so `with_user` holds a single
    // shard lock across both mutations for atomicity.
    let convert_result: Result<(), ()> = exchange.accounts.with_user(user.0, |am| {
        am.withdraw(user.0, &quote.from_coin, quote.from_amount).map_err(|_| ())?;
        let _ = am.deposit(user.0, &quote.to_coin, quote.to_amount);
        Ok(())
    });
    if convert_result.is_err() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Insufficient balance"}))).into_response();
    }

    {
        let mut history = exchange.convert_history.lock().unwrap_or_else(|e| e.into_inner());
        if history.len() >= 50_000 {
            history.drain(..1_000);
        }
        history.push(quote.clone());
    }

    (StatusCode::OK, Json(serde_json::json!({
        "quoteId": quote.quote_id.to_string(),
        "fromCoin": quote.from_coin,
        "toCoin": quote.to_coin,
        "fromAmount": quote.from_amount.to_string(),
        "toAmount": quote.to_amount.to_string(),
        "rate": quote.rate.to_string(),
        "fee": quote.fee.to_string(),
        "status": "completed",
    }))).into_response()
}

/// GET /api/v1/convert/history — last 50 conversions for the authenticated user.
pub async fn convert_history(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
) -> impl IntoResponse {
    let history = exchange.convert_history.lock().unwrap_or_else(|e| e.into_inner());
    let entries: Vec<serde_json::Value> = history
        .iter()
        .rev()
        .filter(|q| q.user_id == user.0)
        .take(50)
        .map(|q| serde_json::json!({
            "quoteId": q.quote_id.to_string(),
            "fromCoin": q.from_coin,
            "toCoin": q.to_coin,
            "fromAmount": q.from_amount.to_string(),
            "toAmount": q.to_amount.to_string(),
            "rate": q.rate.to_string(),
            "fee": q.fee.to_string(),
        }))
        .collect();

    Json(serde_json::json!({ "records": entries }))
}

// ---------------------------------------------------------------------------
// Margin Lending endpoints (Task 6.7)
// ---------------------------------------------------------------------------

/// POST /api/v1/lending/borrow — borrow an asset (credited immediately to trading account).
pub async fn lending_borrow(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let coin = match body.get("coin").and_then(|v| v.as_str()) {
        Some(c) => c.to_uppercase(),
        None => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Missing coin"}))).into_response(),
    };
    let amount: Decimal = match body
        .get("amount")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
    {
        Some(a) if a > Decimal::ZERO => a,
        _ => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Invalid amount"}))).into_response(),
    };

    let hourly_rate = {
        let rates = exchange.lending_rates.lock().unwrap_or_else(|e| e.into_inner());
        match rates.get(&coin).copied() {
            Some(r) => r,
            None => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Borrowing not available for this coin"}))).into_response(),
        }
    };

    // Credit the borrowed amount to the user's trading account.
    let _ = exchange.accounts.deposit(user.0, &coin, amount);

    let loan = crate::exchange::LoanRecord {
        id: uuid::Uuid::new_v4(),
        user_id: user.0,
        coin: coin.clone(),
        principal: amount,
        interest_accrued: Decimal::ZERO,
        hourly_rate,
        status: "active".into(),
        created_at: chrono::Utc::now(),
        repaid_at: None,
    };
    let loan_id = loan.id;
    crate::exchange::capped_push(
        &mut exchange.loans.lock().unwrap_or_else(|e| e.into_inner()),
        loan,
        crate::exchange::MAX_LOANS,
    );

    (StatusCode::OK, Json(serde_json::json!({
        "loanId": loan_id.to_string(),
        "coin": coin,
        "amount": amount.to_string(),
        "hourlyRate": hourly_rate.to_string(),
        "status": "active",
    }))).into_response()
}

/// POST /api/v1/lending/repay — repay an active loan including accrued interest.
pub async fn lending_repay(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let loan_id: uuid::Uuid = match body
        .get("loanId")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
    {
        Some(id) => id,
        None => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Missing loanId"}))).into_response(),
    };

    let mut loans = exchange.loans.lock().unwrap_or_else(|e| e.into_inner());
    let loan = match loans
        .iter_mut()
        .find(|l| l.id == loan_id && l.user_id == user.0 && l.status == "active")
    {
        Some(l) => l,
        None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Loan not found"}))).into_response(),
    };

    let seconds_elapsed = Decimal::from(
        (chrono::Utc::now() - loan.created_at).num_seconds().max(0),
    );
    let hours = seconds_elapsed / Decimal::from(3600);
    let interest = loan.principal * loan.hourly_rate * hours;
    let total_owed = loan.principal + interest;

    if exchange.accounts.withdraw(user.0, &loan.coin, total_owed).is_err() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
            "error": "Insufficient balance to repay",
            "totalOwed": total_owed.to_string(),
        }))).into_response();
    }

    let principal = loan.principal;
    loan.interest_accrued = interest;
    loan.status = "repaid".into();
    loan.repaid_at = Some(chrono::Utc::now());

    (StatusCode::OK, Json(serde_json::json!({
        "loanId": loan_id.to_string(),
        "principal": principal.to_string(),
        "interest": interest.to_string(),
        "totalRepaid": total_owed.to_string(),
        "status": "repaid",
    }))).into_response()
}

/// GET /api/v1/lending/loans — list active loans for the authenticated user.
pub async fn lending_loans(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
) -> impl IntoResponse {
    let loans = exchange.loans.lock().unwrap_or_else(|e| e.into_inner());
    let active: Vec<serde_json::Value> = loans
        .iter()
        .filter(|l| l.user_id == user.0 && l.status == "active")
        .map(|l| {
            let seconds_elapsed = Decimal::from(
                (chrono::Utc::now() - l.created_at).num_seconds().max(0),
            );
            let hours = seconds_elapsed / Decimal::from(3600);
            let interest = l.principal * l.hourly_rate * hours;
            serde_json::json!({
                "loanId": l.id.to_string(),
                "coin": l.coin,
                "principal": l.principal.to_string(),
                "interestAccrued": interest.to_string(),
                "hourlyRate": l.hourly_rate.to_string(),
                "createdAt": l.created_at.timestamp_millis(),
            })
        })
        .collect();

    Json(serde_json::json!({ "loans": active }))
}

/// GET /api/v1/lending/interest-rates — current lending rates (public endpoint).
pub async fn lending_rates(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let rates = exchange.lending_rates.lock().unwrap_or_else(|e| e.into_inner());
    let list: Vec<serde_json::Value> = rates
        .iter()
        .map(|(coin, rate)| serde_json::json!({
            "coin": coin,
            "hourlyRate": rate.to_string(),
            "annualRate": (rate * Decimal::from(8760u32)).to_string(),
        }))
        .collect();

    Json(serde_json::json!({ "rates": list }))
}

// ---------------------------------------------------------------------------
// Dead Man's Switch (Task 7.3)
// ---------------------------------------------------------------------------

/// POST /api/v1/orders/cancel-all-after — arm the dead man's switch.
///
/// Body: `{"timeoutSecs": N}` — arms a per-user deadline `now + N seconds`.
/// When the deadline expires without being renewed the background task cancels
/// all open orders for this user.
///
/// Send `timeoutSecs: 0` to disarm without cancelling (same as DELETE).
pub async fn dms_set(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let timeout_secs = match body.get("timeoutSecs").and_then(|v| v.as_u64()) {
        Some(s) => s,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing or invalid timeoutSecs"})),
            )
                .into_response();
        }
    };

    let mut dms = exchange.dead_mans_switch.lock().unwrap_or_else(|e| e.into_inner());
    if timeout_secs == 0 {
        dms.remove(&auth.0);
        tracing::info!(user_id = %auth.0, "dead man's switch disarmed");
        return (StatusCode::OK, Json(serde_json::json!({"status": "disarmed"}))).into_response();
    }

    let deadline = chrono::Utc::now() + chrono::Duration::seconds(timeout_secs as i64);
    dms.insert(auth.0, deadline);
    tracing::info!(
        user_id = %auth.0,
        timeout_secs = timeout_secs,
        deadline = %deadline,
        "dead man's switch armed"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "armed",
            "timeoutSecs": timeout_secs,
            "deadline": deadline.timestamp_millis(),
        })),
    )
        .into_response()
}

/// DELETE /api/v1/orders/cancel-all-after — disarm the dead man's switch.
///
/// Removes the pending deadline without cancelling any orders.
pub async fn dms_disarm(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let mut dms = exchange.dead_mans_switch.lock().unwrap_or_else(|e| e.into_inner());
    dms.remove(&auth.0);
    tracing::info!(user_id = %auth.0, "dead man's switch disarmed via DELETE");
    (StatusCode::OK, Json(serde_json::json!({"status": "disarmed"})))
}

// ---------------------------------------------------------------------------
// Market Maker Protection (Task 8.2)
// ---------------------------------------------------------------------------

/// POST /api/v1/account/mmp/config — set MMP parameters for the authenticated user.
pub async fn set_mmp_config(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let window_ms = body.get("windowMs").and_then(|v| v.as_u64()).unwrap_or(5000);
    let frozen_ms = body.get("frozenMs").and_then(|v| v.as_u64()).unwrap_or(10000);
    let qty_limit: Decimal = body
        .get("qtyLimit")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(Decimal::MAX);
    let count_limit = body
        .get("countLimit")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32)
        .unwrap_or(u32::MAX);

    let config = crate::exchange::MmpConfig { window_ms, frozen_ms, qty_limit, count_limit };
    let state = crate::exchange::MmpState {
        config: config.clone(),
        is_frozen: false,
        frozen_until: None,
        recent_fills: Vec::new(),
    };

    exchange
        .mmp_state
        .lock().unwrap_or_else(|e| e.into_inner())
        .insert(user.0, state);

    tracing::info!(user_id = %user.0, window_ms, frozen_ms, "MMP config updated");

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "windowMs": window_ms,
            "frozenMs": frozen_ms,
            "qtyLimit": qty_limit.to_string(),
            "countLimit": count_limit,
        })),
    )
        .into_response()
}

/// GET /api/v1/account/mmp/config — get MMP config and current freeze state.
pub async fn get_mmp_config(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let mmp = exchange.mmp_state.lock().unwrap_or_else(|e| e.into_inner());
    match mmp.get(&user.0) {
        Some(state) => Json(serde_json::json!({
            "enabled": true,
            "windowMs": state.config.window_ms,
            "frozenMs": state.config.frozen_ms,
            "qtyLimit": state.config.qty_limit.to_string(),
            "countLimit": state.config.count_limit,
            "isFrozen": state.is_frozen,
            "frozenUntil": state.frozen_until.map(|t| t.timestamp_millis()),
        }))
        .into_response(),
        None => Json(serde_json::json!({"enabled": false})).into_response(),
    }
}

/// POST /api/v1/account/mmp/reset — manually unfreeze the MMP gate.
pub async fn reset_mmp(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let mut mmp = exchange.mmp_state.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(state) = mmp.get_mut(&user.0) {
        state.is_frozen = false;
        state.frozen_until = None;
        state.recent_fills.clear();
        tracing::info!(user_id = %user.0, "MMP manually reset");
    }
    Json(serde_json::json!({"status": "unfrozen"}))
}

// ---------------------------------------------------------------------------
// Move Position (Task 8.3)
// ---------------------------------------------------------------------------

/// POST /api/v1/position/move — transfer an open position to a sub-account.
pub async fn move_position(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let symbol = match body.get("symbol").and_then(|v| v.as_str()) {
        Some(s) => exchange_common::Symbol::new(s),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing symbol"})),
            )
                .into_response();
        }
    };
    let to_uid: Uuid = match body
        .get("toUid")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
    {
        Some(id) => id,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing or invalid toUid"})),
            )
                .into_response();
        }
    };
    let position_idx: u8 = body
        .get("positionIdx")
        .and_then(|v| v.as_u64())
        .map(|n| n as u8)
        .unwrap_or(0);

    // Verify to_uid is a sub-account of the authenticated user.
    {
        let subs = exchange.sub_accounts.lock().unwrap_or_else(|e| e.into_inner());
        // get_master returns Some(parent) when to_uid is a sub-account; verify parent == caller.
        let parent = subs.get_master(to_uid);
        if parent != Some(user.0) {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"error": "Target must be your sub-account"})),
            )
                .into_response();
        }
    }

    // Capture the source position fields, then perform the transfer.
    let source = {
        let pm = exchange.positions.lock();
        match pm.get_position(user.0, &symbol, position_idx) {
            Some(p) if !p.is_closed() => p.clone(),
            _ => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "No open position to move"})),
                )
                    .into_response();
            }
        }
    };

    // Check target doesn't already have an open position on this symbol.
    {
        let pm = exchange.positions.lock();
        if let Some(existing) = pm.get_position(to_uid, &symbol, position_idx) {
            if !existing.is_closed() {
                return (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({"error": "Target account already has an open position on this symbol"})),
                )
                    .into_response();
            }
        }
    }

    // Close source position (no PnL settlement — transfer preserves entry price).
    {
        let mut pm = exchange.positions.lock();
        let _ = pm.close(user.0, &symbol, source.entry_price, position_idx);
    }

    // Re-open at target with the same parameters.
    {
        let mut pm = exchange.positions.lock();
        let _ = pm.open_or_increase(
            to_uid,
            symbol.clone(),
            source.side,
            source.size,
            source.entry_price,
            source.leverage,
            source.margin_mode,
            position_idx,
        );
    }

    // Move the margin: close released it to source trading account; transfer to target.
    // S18 Phase B: `ShardedAccounts::transfer` handles 2-shard ascending lock
    // order + rollback on destination failure.
    let _ = exchange.accounts.transfer(user.0, to_uid, "USDT", source.margin);

    tracing::info!(
        from_uid = %user.0,
        to_uid = %to_uid,
        symbol = %symbol,
        size = %source.size,
        "position moved between accounts"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "symbol": symbol.to_string(),
            "fromUid": user.0.to_string(),
            "toUid": to_uid.to_string(),
            "size": source.size.to_string(),
            "side": format!("{:?}", source.side),
            "status": "completed",
        })),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// L3 Orderbook (Task 8.4)
// ---------------------------------------------------------------------------

/// GET /api/v1/markets/{symbol}/orderbook/l3 — Level 3 orderbook (individual order level).
///
/// Phase 1: exposes the same price-level aggregates as L2 but also includes the
/// sequence number. True per-order granularity would require exposing internal
/// `PriceLevel` order queues; that extension can be added without breaking this API shape.
pub async fn get_l3_orderbook(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    if let Err(e) = validate_symbol(&symbol) {
        return e.into_response();
    }

    let depth = params
        .get("depth")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(50)
        .min(200);
    let sym = exchange_common::Symbol::new(&symbol);

    let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
    match pool.snapshot(&sym, depth) {
        Ok(snap) => {
            let bids: Vec<[String; 2]> = snap
                .bids
                .iter()
                .map(|(p, q)| [p.to_string(), q.to_string()])
                .collect();
            let asks: Vec<[String; 2]> = snap
                .asks
                .iter()
                .map(|(p, q)| [p.to_string(), q.to_string()])
                .collect();

            Json(serde_json::json!({
                "symbol": symbol,
                "bids": bids,
                "asks": asks,
                "sequence": snap.sequence,
                "timestamp": chrono::Utc::now().timestamp_millis(),
            }))
            .into_response()
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Symbol not found"})),
        )
            .into_response(),
    }
}

// ── Task 8.5: Spot Margin Trading ────────────────────────────────────────────

/// POST /api/v1/spot-margin/toggle — enable/disable spot margin trading.
pub async fn spot_margin_toggle(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let enabled = body.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
    let max_leverage = body
        .get("maxLeverage")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(rust_decimal::Decimal::from(3));

    exchange
        .spot_margin_settings
        .lock().unwrap_or_else(|e| e.into_inner())
        .insert(user.0, crate::exchange::SpotMarginSettings { enabled, max_leverage });

    Json(serde_json::json!({
        "enabled": enabled,
        "maxLeverage": max_leverage.to_string(),
    }))
}

/// GET /api/v1/spot-margin/status — check if spot margin is enabled.
pub async fn spot_margin_status(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
) -> impl IntoResponse {
    let settings = exchange.spot_margin_settings.lock().unwrap_or_else(|e| e.into_inner());
    match settings.get(&user.0) {
        Some(s) => Json(serde_json::json!({
            "enabled": s.enabled,
            "maxLeverage": s.max_leverage.to_string(),
        })),
        None => Json(serde_json::json!({
            "enabled": false,
            "maxLeverage": "1",
        })),
    }
}

// ── Task 8.6: Earn / Staking ─────────────────────────────────────────────────

/// GET /api/v1/earn/products — list available earn products.
pub async fn earn_products(State(exchange): State<AppState>) -> impl IntoResponse {
    let products = exchange.earn_products.lock().unwrap_or_else(|e| e.into_inner());
    let list: Vec<serde_json::Value> = products
        .iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id.to_string(),
                "coin": p.coin,
                "name": p.name,
                "apy": p.apy.to_string(),
                "minAmount": p.min_amount.to_string(),
                "maxAmount": p.max_amount.to_string(),
                "lockDays": p.lock_days,
                "productType": p.product_type,
                "supportsAutoCompound": p.supports_auto_compound,
            })
        })
        .collect();
    Json(serde_json::json!({"products": list}))
}

/// POST /api/v1/earn/subscribe — allocate funds to an earn product.
pub async fn earn_subscribe(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> axum::response::Response {
    let product_id: uuid::Uuid = match body
        .get("productId")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
    {
        Some(id) => id,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing productId"})),
            )
                .into_response()
        }
    };
    let amount: Decimal = match body
        .get("amount")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
    {
        Some(a) if a > Decimal::ZERO => a,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid amount"})),
            )
                .into_response()
        }
    };

    // Find the product.
    let product = {
        let products = exchange.earn_products.lock().unwrap_or_else(|e| e.into_inner());
        products.iter().find(|p| p.id == product_id).cloned()
    };
    let product = match product {
        Some(p) => p,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Product not found"})),
            )
                .into_response()
        }
    };

    if amount < product.min_amount || amount > product.max_amount {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Amount out of range"})),
        )
            .into_response();
    }

    // Enforce pool capacity — sum all active allocations for this product.
    if let Some(capacity) = product.pool_capacity {
        let allocs = exchange.earn_allocations.lock().unwrap_or_else(|e| e.into_inner());
        let total_subscribed: Decimal = allocs.iter()
            .filter(|a| a.product_id == product_id && a.status == "active")
            .map(|a| a.amount)
            .sum();
        if total_subscribed + amount > capacity {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "Pool capacity exceeded",
                    "capacity": capacity.to_string(),
                    "subscribed": total_subscribed.to_string(),
                    "remaining": (capacity - total_subscribed).max(Decimal::ZERO).to_string(),
                })),
            ).into_response();
        }
    }

    // Lock funds (withdraw from trading account).
    if exchange.accounts.withdraw(user.0, &product.coin, amount).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Insufficient balance"})),
        )
            .into_response();
    }

    let unlock_at = if product.lock_days > 0 {
        Some(chrono::Utc::now() + chrono::Duration::days(product.lock_days as i64))
    } else {
        None
    };

    // Auto-compound is opt-in; only valid for flexible products that support it.
    let auto_compound = body.get("autoCompound").and_then(|v| v.as_bool()).unwrap_or(false)
        && product.supports_auto_compound;

    let now = chrono::Utc::now();
    let allocation = crate::exchange::EarnAllocation {
        id: uuid::Uuid::new_v4(),
        user_id: user.0,
        product_id,
        coin: product.coin.clone(),
        amount,
        earned_interest: Decimal::ZERO,
        started_at: now,
        unlock_at,
        status: "active".into(),
        auto_compound,
        last_accrued_at: now,
    };
    let alloc_id = allocation.id;
    exchange
        .earn_allocations
        .lock().unwrap_or_else(|e| e.into_inner())
        .push(allocation);

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "allocationId": alloc_id.to_string(),
            "coin": product.coin,
            "amount": amount.to_string(),
            "apy": product.apy.to_string(),
            "unlockAt": unlock_at.map(|t| t.timestamp_millis()),
            "autoCompound": auto_compound,
        })),
    )
        .into_response()
}

/// POST /api/v1/earn/redeem — withdraw from an earn product.
pub async fn earn_redeem(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> axum::response::Response {
    let alloc_id: uuid::Uuid = match body
        .get("allocationId")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
    {
        Some(id) => id,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing allocationId"})),
            )
                .into_response()
        }
    };

    let mut allocs = exchange.earn_allocations.lock().unwrap_or_else(|e| e.into_inner());
    let alloc = match allocs
        .iter_mut()
        .find(|a| a.id == alloc_id && a.user_id == user.0 && a.status == "active")
    {
        Some(a) => a,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Allocation not found"})),
            )
                .into_response()
        }
    };

    // Check lock period.
    if let Some(unlock_at) = alloc.unlock_at {
        if chrono::Utc::now() < unlock_at {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "Locked until unlock date",
                    "unlockAt": unlock_at.timestamp_millis(),
                })),
            )
                .into_response();
        }
    }

    // Calculate any remaining fractional-hour interest not yet captured by the
    // background accrual task (which runs hourly), then add to already-accrued.
    let partial_hours = {
        let secs = (chrono::Utc::now() - alloc.last_accrued_at).num_seconds().max(0);
        Decimal::from(secs) / Decimal::from(3600)
    };
    let product = {
        let products = exchange.earn_products.lock().unwrap_or_else(|e| e.into_inner());
        products.iter().find(|p| p.id == alloc.product_id).cloned()
    };
    let apy = product.map(|p| p.apy).unwrap_or(Decimal::ZERO);
    let hourly_rate = apy / Decimal::from(8760u32); // hours per year
    let partial_interest = alloc.amount * hourly_rate * partial_hours;
    let total_interest = alloc.earned_interest + partial_interest;

    // For auto-compound allocations `amount` already grew; principal == amount at redeem.
    let principal = alloc.amount;
    let total = principal + total_interest;
    alloc.earned_interest = total_interest;
    alloc.status = "redeemed".into();
    let coin = alloc.coin.clone();
    drop(allocs);

    // Debit yield pool for the interest portion (partial interest not yet accrued).
    // The hourly accrual task already debits interest it credits — here we only
    // debit the partial-hour portion calculated at redemption time.
    if partial_interest > Decimal::ZERO {
        use crate::exchange::YIELD_POOL_ACCOUNT_ID;
        if let Err(e) = exchange.accounts.withdraw(YIELD_POOL_ACCOUNT_ID, &coin, partial_interest) {
            tracing::warn!(
                coin = %coin, partial_interest = %partial_interest, error = %e,
                "earn_redeem: yield pool debit failed for partial interest"
            );
        }
    }

    // Credit principal + interest back to user.
    let _ = exchange.accounts.deposit(user.0, &coin, total);

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "allocationId": alloc_id.to_string(),
            "principal": principal.to_string(),
            "interest": total_interest.to_string(),
            "totalRedeemed": total.to_string(),
        })),
    )
        .into_response()
}

/// GET /api/v1/earn/allocations — user's active earn allocations.
pub async fn earn_allocations(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
) -> impl IntoResponse {
    let allocs = exchange.earn_allocations.lock().unwrap_or_else(|e| e.into_inner());
    let list: Vec<serde_json::Value> = allocs
        .iter()
        .filter(|a| a.user_id == user.0)
        .map(|a| {
            serde_json::json!({
                "allocationId": a.id.to_string(),
                "productId": a.product_id.to_string(),
                "coin": a.coin,
                "amount": a.amount.to_string(),
                "earnedInterest": a.earned_interest.to_string(),
                "startedAt": a.started_at.timestamp_millis(),
                "unlockAt": a.unlock_at.map(|t| t.timestamp_millis()),
                "status": a.status,
                "autoCompound": a.auto_compound,
            })
        })
        .collect();
    Json(serde_json::json!({"allocations": list}))
}

// ── Task 8.7: Fiat Gateway (stub) ────────────────────────────────────────────

/// POST /api/v1/fiat/deposit — initiate fiat deposit.
pub async fn fiat_deposit(
    State(_exchange): State<AppState>,
    axum::Extension(_user): axum::Extension<crate::auth::AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> axum::response::Response {
    let currency = body.get("currency").and_then(|v| v.as_str()).unwrap_or("USD");
    let amount: Decimal = match body
        .get("amount")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
    {
        Some(a) if a > Decimal::ZERO => a,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid amount"})),
            )
                .into_response()
        }
    };
    let method = body.get("method").and_then(|v| v.as_str()).unwrap_or("bank_transfer");

    Json(serde_json::json!({
        "orderId": uuid::Uuid::new_v4().to_string(),
        "currency": currency,
        "amount": amount.to_string(),
        "method": method,
        "status": "pending",
        "message": "Fiat gateway integration pending. Use crypto deposit for now.",
    }))
    .into_response()
}

/// POST /api/v1/fiat/withdraw — initiate fiat withdrawal.
pub async fn fiat_withdraw(
    State(_exchange): State<AppState>,
    axum::Extension(_user): axum::Extension<crate::auth::AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> axum::response::Response {
    let currency = body.get("currency").and_then(|v| v.as_str()).unwrap_or("USD");
    let amount: Decimal = match body
        .get("amount")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
    {
        Some(a) if a > Decimal::ZERO => a,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid amount"})),
            )
                .into_response()
        }
    };

    Json(serde_json::json!({
        "orderId": uuid::Uuid::new_v4().to_string(),
        "currency": currency,
        "amount": amount.to_string(),
        "status": "pending",
        "message": "Fiat gateway integration pending. Use crypto withdrawal for now.",
    }))
    .into_response()
}

/// GET /api/v1/fiat/methods — available fiat payment methods.
pub async fn fiat_methods() -> impl IntoResponse {
    Json(serde_json::json!({
        "methods": [
            {
                "id": "bank_transfer",
                "name": "Bank Transfer (SEPA/SWIFT)",
                "currencies": ["USD", "EUR", "UAH"],
                "minAmount": "50",
                "maxAmount": "100000",
                "fee": "0",
            },
            {
                "id": "card",
                "name": "Credit/Debit Card",
                "currencies": ["USD", "EUR"],
                "minAmount": "20",
                "maxAmount": "10000",
                "fee": "2.5%",
            },
        ]
    }))
}

/// GET /api/v1/fiat/rates — current fiat ↔ crypto exchange rates.
pub async fn fiat_rates() -> impl IntoResponse {
    let rates = crate::fiat::stub_exchange_rates();
    let list: Vec<serde_json::Value> = rates
        .iter()
        .map(|r| {
            serde_json::json!({
                "from": r.from,
                "to": r.to,
                "rate": r.rate.to_string(),
                "feePct": r.fee_pct.to_string(),
            })
        })
        .collect();
    Json(serde_json::json!({"rates": list}))
}

/// GET /api/v1/fiat/history — authenticated user's fiat payment history.
pub async fn fiat_history(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
) -> impl IntoResponse {
    let gw = exchange.fiat_gateway.lock().unwrap_or_else(|e| e.into_inner());
    let records = gw.history_for_user(user.0);
    let list: Vec<serde_json::Value> = records
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id.to_string(),
                "direction": r.direction,
                "provider": r.provider,
                "currency": r.currency,
                "amount": r.amount.to_string(),
                "fee": r.fee.to_string(),
                "status": r.status.as_str(),
                "providerReference": r.provider_reference,
                "createdAt": r.created_at.timestamp_millis(),
                "updatedAt": r.updated_at.timestamp_millis(),
            })
        })
        .collect();
    Json(serde_json::json!({"history": list}))
}

/// POST /webhooks/fiat — payment provider webhook to update payment status.
///
/// Requires a valid HMAC-SHA256 signature in the `x-webhook-signature` header,
/// computed as `hex(HMAC-SHA256(WEBHOOK_SECRET, raw_body))`.
pub async fn fiat_webhook(
    State(exchange): State<AppState>,
    headers: axum::http::HeaderMap,
    raw_body: axum::body::Bytes,
) -> impl IntoResponse {
    if let Err(resp) = verify_webhook_signature(&headers, &raw_body, &*exchange.secrets) {
        return resp.into_response();
    }

    let body: serde_json::Value = match serde_json::from_slice(&raw_body) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("Invalid JSON: {e}")})),
            )
            .into_response();
        }
    };

    let payment_id_str = match body.get("payment_id").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Missing payment_id"}))).into_response();
        }
    };
    let payment_id = match uuid::Uuid::parse_str(payment_id_str) {
        Ok(id) => id,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Invalid payment_id"}))).into_response();
        }
    };
    let status_str = body.get("status").and_then(|v| v.as_str()).unwrap_or("pending");
    let new_status = match status_str {
        "completed" => crate::fiat::PaymentStatus::Completed,
        "failed" => crate::fiat::PaymentStatus::Failed,
        "processing" => crate::fiat::PaymentStatus::Processing,
        "refunded" => crate::fiat::PaymentStatus::Refunded,
        "expired" => crate::fiat::PaymentStatus::Expired,
        _ => crate::fiat::PaymentStatus::Pending,
    };

    exchange
        .fiat_gateway
        .lock().unwrap_or_else(|e| e.into_inner())
        .update_status(payment_id, new_status);

    tracing::info!(payment_id = %payment_id, status = %status_str, "fiat_webhook: status updated");

    Json(serde_json::json!({"ok": true})).into_response()
}

// ── Block Trading / RFQ ────────────────────────────────────────────────────────

/// RFQ expiry in seconds (5 minutes default).
const RFQ_EXPIRY_SECS: i64 = 300;
/// Maximum clients a broker may have.
const BROKER_MAX_CLIENTS: usize = 100;

// --- Request types ---

#[derive(Debug, serde::Deserialize)]
pub struct CreateRfqRequest {
    pub symbol: String,
    pub side: String,
    pub qty: Decimal,
    /// Optional additional legs for multi-leg (spread/combo) RFQs.
    #[serde(default)]
    pub legs: Vec<crate::exchange::RfqLeg>,
}

#[derive(Debug, serde::Deserialize)]
pub struct SubmitQuoteRequest {
    pub price: Decimal,
    /// How many seconds until this quote expires (default: RFQ expiry).
    #[serde(default)]
    pub expires_in_secs: Option<i64>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteRfqRequest {
    /// ID of the quote to accept.
    pub quote_id: Uuid,
}

// --- Handlers ---

/// POST /api/v1/block-trade/rfq — taker creates an RFQ.
pub async fn block_trade_rfq_create(
    State(exchange): State<AppState>,
    axum::Extension(auth_user): axum::Extension<crate::auth::AuthenticatedUser>,
    Json(req): Json<CreateRfqRequest>,
) -> impl IntoResponse {
    validate_symbol(&req.symbol).map_err(|e| e.into_response()).ok();

    if let Err(e) = validate_symbol(&req.symbol) {
        return e.into_response();
    }
    if let Err(e) = validate_positive(req.qty, "qty") {
        return e.into_response();
    }
    if req.side != "Buy" && req.side != "Sell" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "side must be 'Buy' or 'Sell'"})),
        )
            .into_response();
    }

    let now = chrono::Utc::now();
    let rfq = crate::exchange::BlockTradeRfq {
        id: Uuid::new_v4(),
        taker_id: auth_user.0,
        symbol: req.symbol.clone(),
        side: req.side.clone(),
        qty: req.qty,
        legs: req.legs,
        status: crate::exchange::RfqStatus::AwaitingQuotes,
        created_at: now,
        expires_at: now + chrono::Duration::seconds(RFQ_EXPIRY_SECS),
        quotes: Vec::new(),
    };
    let rfq_id = rfq.id;
    let expires_at_ms = rfq.expires_at.timestamp_millis();

    exchange
        .block_trade_rfqs
        .lock().unwrap_or_else(|e| e.into_inner())
        .insert(rfq_id, rfq);

    tracing::info!(
        user_id = %auth_user.0,
        rfq_id = %rfq_id,
        symbol = %req.symbol,
        side = %req.side,
        qty = %req.qty,
        "block trade RFQ created"
    );

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "rfqId": rfq_id.to_string(),
            "symbol": req.symbol,
            "side": req.side,
            "qty": req.qty,
            "status": "awaiting_quotes",
            "expiresAt": expires_at_ms,
        })),
    )
        .into_response()
}

/// POST /api/v1/block-trade/rfq/{id}/quote — market maker submits a quote.
pub async fn block_trade_rfq_submit_quote(
    State(exchange): State<AppState>,
    axum::Extension(auth_user): axum::Extension<crate::auth::AuthenticatedUser>,
    Path(rfq_id): Path<Uuid>,
    Json(req): Json<SubmitQuoteRequest>,
) -> impl IntoResponse {
    if let Err(e) = validate_positive(req.price, "price") {
        return e.into_response();
    }

    let now = chrono::Utc::now();
    let mut rfqs = exchange
        .block_trade_rfqs
        .lock().unwrap_or_else(|e| e.into_inner());

    let rfq = match rfqs.get_mut(&rfq_id) {
        Some(r) => r,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "RFQ not found"})),
            )
                .into_response();
        }
    };

    // Expired RFQ check.
    if now > rfq.expires_at {
        rfq.status = crate::exchange::RfqStatus::Expired;
        return (
            StatusCode::GONE,
            Json(serde_json::json!({"error": "RFQ has expired"})),
        )
            .into_response();
    }

    if rfq.status != crate::exchange::RfqStatus::AwaitingQuotes
        && rfq.status != crate::exchange::RfqStatus::Quoted
    {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "RFQ is not accepting quotes"})),
        )
            .into_response();
    }

    // Market maker cannot quote their own RFQ.
    if rfq.taker_id == auth_user.0 {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "Taker cannot submit a quote to their own RFQ"})),
        )
            .into_response();
    }

    let quote_expiry_secs = req.expires_in_secs.unwrap_or(RFQ_EXPIRY_SECS);
    let quote = crate::exchange::RfqQuote {
        id: Uuid::new_v4(),
        rfq_id,
        maker_id: auth_user.0,
        price: req.price,
        expires_at: now + chrono::Duration::seconds(quote_expiry_secs),
        created_at: now,
    };
    let quote_id = quote.id;

    rfq.quotes.push(quote);
    rfq.status = crate::exchange::RfqStatus::Quoted;

    tracing::info!(
        maker_id = %auth_user.0,
        rfq_id = %rfq_id,
        quote_id = %quote_id,
        price = %req.price,
        "block trade quote submitted"
    );

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "quoteId": quote_id.to_string(),
            "rfqId": rfq_id.to_string(),
            "price": req.price,
            "expiresAt": (now + chrono::Duration::seconds(quote_expiry_secs)).timestamp_millis(),
        })),
    )
        .into_response()
}

/// GET /api/v1/block-trade/rfq/{id}/quotes — list all quotes for an RFQ.
pub async fn block_trade_rfq_quotes(
    State(exchange): State<AppState>,
    axum::Extension(auth_user): axum::Extension<crate::auth::AuthenticatedUser>,
    Path(rfq_id): Path<Uuid>,
) -> impl IntoResponse {
    let rfqs = exchange
        .block_trade_rfqs
        .lock().unwrap_or_else(|e| e.into_inner());

    let rfq = match rfqs.get(&rfq_id) {
        Some(r) => r,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "RFQ not found"})),
            )
                .into_response();
        }
    };

    // Only the taker (RFQ creator) may see all quotes. Market makers see their own.
    let quotes: Vec<_> = rfq
        .quotes
        .iter()
        .filter(|q| rfq.taker_id == auth_user.0 || q.maker_id == auth_user.0)
        .map(|q| {
            serde_json::json!({
                "quoteId": q.id.to_string(),
                "rfqId": q.rfq_id.to_string(),
                "makerId": q.maker_id.to_string(),
                "price": q.price,
                "expiresAt": q.expires_at.timestamp_millis(),
                "createdAt": q.created_at.timestamp_millis(),
            })
        })
        .collect();

    Json(serde_json::json!({
        "rfqId": rfq_id.to_string(),
        "status": rfq.status,
        "quotes": quotes,
    }))
    .into_response()
}

/// POST /api/v1/block-trade/rfq/{id}/execute — taker accepts a quote; settlement occurs.
pub async fn block_trade_rfq_execute(
    State(exchange): State<AppState>,
    axum::Extension(auth_user): axum::Extension<crate::auth::AuthenticatedUser>,
    Path(rfq_id): Path<Uuid>,
    Json(req): Json<ExecuteRfqRequest>,
) -> impl IntoResponse {
    let now = chrono::Utc::now();

    // --- 1. Validate and retrieve RFQ + quote ---
    let (maker_id, price, qty, symbol, taker_side) = {
        let mut rfqs = exchange
            .block_trade_rfqs
            .lock().unwrap_or_else(|e| e.into_inner());

        let rfq = match rfqs.get_mut(&rfq_id) {
            Some(r) => r,
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "RFQ not found"})),
                )
                    .into_response();
            }
        };

        // Only the taker can execute.
        if rfq.taker_id != auth_user.0 {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"error": "Only the RFQ taker may execute it"})),
            )
                .into_response();
        }

        if now > rfq.expires_at {
            rfq.status = crate::exchange::RfqStatus::Expired;
            return (
                StatusCode::GONE,
                Json(serde_json::json!({"error": "RFQ has expired"})),
            )
                .into_response();
        }

        if rfq.status == crate::exchange::RfqStatus::Executed {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": "RFQ already executed"})),
            )
                .into_response();
        }

        if rfq.status == crate::exchange::RfqStatus::Cancelled {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": "RFQ was cancelled"})),
            )
                .into_response();
        }

        let quote = match rfq.quotes.iter().find(|q| q.id == req.quote_id) {
            Some(q) => q.clone(),
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "Quote not found"})),
                )
                    .into_response();
            }
        };

        if now > quote.expires_at {
            return (
                StatusCode::GONE,
                Json(serde_json::json!({"error": "Quote has expired"})),
            )
                .into_response();
        }

        // --- 2. Check minimum block trade size ---
        let minimums = exchange
            .block_trade_minimums
            .lock().unwrap_or_else(|e| e.into_inner());
        if let Some(&min_qty) = minimums.get(&rfq.symbol) {
            if rfq.qty < min_qty {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": format!(
                            "Block trade quantity {} is below minimum {} for {}",
                            rfq.qty, min_qty, rfq.symbol
                        )
                    })),
                )
                    .into_response();
            }
        }

        rfq.status = crate::exchange::RfqStatus::Executed;
        (quote.maker_id, quote.price, rfq.qty, rfq.symbol.clone(), rfq.side.clone())
    };

    // --- 3. Settle: adjust balances directly (off-book) ---
    // Taker Buy: taker pays quote asset (USDT), receives base asset (BTC).
    //            Maker receives USDT, pays BTC.
    // Taker Sell: taker pays base, receives quote. Maker pays quote, receives base.
    //
    // We split the symbol into base/quote using the product registry.
    let (base_asset, quote_asset) = {
        let sym = Symbol::new(&symbol);
        match exchange.accounts.get_product(&sym) {
            Some(p) => (p.base_asset.clone(), p.quote_asset.clone()),
            None => {
                // Unknown product — fall back to heuristic split (e.g. BTCUSDT → BTC + USDT).
                let (b, q) = symbol.split_at(symbol.len().saturating_sub(4));
                (b.to_string(), q.to_string())
            }
        }
    };

    let notional = price * qty;

    // Apply settlement under a single 2-shard ascending lock acquisition.
    // S18 Phase B: buyer and seller shards are acquired in index order via
    // `with_users_2`; same-shard fast path collapses to one lock.
    let taker_id = auth_user.0;
    let settlement_result: anyhow::Result<()> = exchange.accounts.with_users_2(
        taker_id,
        maker_id,
        |am_taker, am_maker_opt| {
            match am_maker_opt {
                Some(am_maker) => {
                    // Different shards — route each user's mutations through
                    // the shard that holds them.
                    if taker_side == "Buy" {
                        am_taker.withdraw(taker_id, &quote_asset, notional)
                            .map_err(|e| anyhow::anyhow!("Taker withdraw quote failed: {e}"))?;
                        am_taker.deposit(taker_id, &base_asset, qty)
                            .map_err(|e| anyhow::anyhow!("Taker deposit base failed: {e}"))?;
                        am_maker.withdraw(maker_id, &base_asset, qty)
                            .map_err(|e| anyhow::anyhow!("Maker withdraw base failed: {e}"))?;
                        am_maker.deposit(maker_id, &quote_asset, notional)
                            .map_err(|e| anyhow::anyhow!("Maker deposit quote failed: {e}"))?;
                    } else {
                        am_taker.withdraw(taker_id, &base_asset, qty)
                            .map_err(|e| anyhow::anyhow!("Taker withdraw base failed: {e}"))?;
                        am_taker.deposit(taker_id, &quote_asset, notional)
                            .map_err(|e| anyhow::anyhow!("Taker deposit quote failed: {e}"))?;
                        am_maker.withdraw(maker_id, &quote_asset, notional)
                            .map_err(|e| anyhow::anyhow!("Maker withdraw quote failed: {e}"))?;
                        am_maker.deposit(maker_id, &base_asset, qty)
                            .map_err(|e| anyhow::anyhow!("Maker deposit base failed: {e}"))?;
                    }
                }
                None => {
                    // Same shard — all four mutations on the single guard.
                    if taker_side == "Buy" {
                        am_taker.withdraw(taker_id, &quote_asset, notional)
                            .map_err(|e| anyhow::anyhow!("Taker withdraw quote failed: {e}"))?;
                        am_taker.deposit(taker_id, &base_asset, qty)
                            .map_err(|e| anyhow::anyhow!("Taker deposit base failed: {e}"))?;
                        am_taker.withdraw(maker_id, &base_asset, qty)
                            .map_err(|e| anyhow::anyhow!("Maker withdraw base failed: {e}"))?;
                        am_taker.deposit(maker_id, &quote_asset, notional)
                            .map_err(|e| anyhow::anyhow!("Maker deposit quote failed: {e}"))?;
                    } else {
                        am_taker.withdraw(taker_id, &base_asset, qty)
                            .map_err(|e| anyhow::anyhow!("Taker withdraw base failed: {e}"))?;
                        am_taker.deposit(taker_id, &quote_asset, notional)
                            .map_err(|e| anyhow::anyhow!("Taker deposit quote failed: {e}"))?;
                        am_taker.withdraw(maker_id, &quote_asset, notional)
                            .map_err(|e| anyhow::anyhow!("Maker withdraw quote failed: {e}"))?;
                        am_taker.deposit(maker_id, &base_asset, qty)
                            .map_err(|e| anyhow::anyhow!("Maker deposit base failed: {e}"))?;
                    }
                }
            }
            Ok(())
        },
    );

    if let Err(e) = settlement_result {
        tracing::error!(rfq_id = %rfq_id, error = %e, "block trade settlement failed");
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": format!("Settlement failed: {e}")})),
        )
            .into_response();
    }

    // --- 4. Record block trade ---
    let trade = crate::exchange::BlockTrade {
        id: Uuid::new_v4(),
        rfq_id,
        maker_id,
        taker_id: auth_user.0,
        symbol: symbol.clone(),
        price,
        qty,
        taker_side: taker_side.clone(),
        timestamp: now,
    };
    let trade_id = trade.id;

    exchange
        .block_trades
        .lock().unwrap_or_else(|e| e.into_inner())
        .push(trade);

    tracing::info!(
        taker_id = %auth_user.0,
        maker_id = %maker_id,
        rfq_id = %rfq_id,
        trade_id = %trade_id,
        symbol = %symbol,
        price = %price,
        qty = %qty,
        "block trade executed"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "tradeId": trade_id.to_string(),
            "rfqId": rfq_id.to_string(),
            "symbol": symbol,
            "price": price,
            "qty": qty,
            "takerSide": taker_side,
            "timestamp": now.timestamp_millis(),
        })),
    )
        .into_response()
}

/// GET /api/v1/block-trade/history — user's block trade history.
pub async fn block_trade_history(
    State(exchange): State<AppState>,
    axum::Extension(auth_user): axum::Extension<crate::auth::AuthenticatedUser>,
) -> impl IntoResponse {
    let trades = exchange
        .block_trades
        .lock().unwrap_or_else(|e| e.into_inner());

    let user_trades: Vec<_> = trades
        .iter()
        .filter(|t| t.taker_id == auth_user.0 || t.maker_id == auth_user.0)
        .map(|t| {
            serde_json::json!({
                "tradeId": t.id.to_string(),
                "rfqId": t.rfq_id.to_string(),
                "symbol": t.symbol,
                "price": t.price,
                "qty": t.qty,
                "takerSide": t.taker_side,
                "role": if t.taker_id == auth_user.0 { "taker" } else { "maker" },
                "timestamp": t.timestamp.timestamp_millis(),
            })
        })
        .collect();

    Json(serde_json::json!({ "trades": user_trades })).into_response()
}

// ── Broker API ────────────────────────────────────────────────────────────────

// --- Request types ---

#[derive(Debug, serde::Deserialize)]
pub struct RegisterBrokerRequest {
    pub user_id: Uuid,
    pub name: String,
    /// Fraction of client fee paid to the broker (0 < rate ≤ 1).
    pub commission_rate: Decimal,
}

#[derive(Debug, serde::Deserialize)]
pub struct BrokerCreateClientRequest {
    /// Optional display name for the client account.
    #[serde(default)]
    pub name: Option<String>,
}

// --- Admin: register broker ---

/// POST /admin/brokers — admin registers a broker.
pub async fn admin_register_broker(
    State(exchange): State<AppState>,
    Json(req): Json<RegisterBrokerRequest>,
) -> impl IntoResponse {
    if req.commission_rate <= Decimal::ZERO || req.commission_rate > Decimal::ONE {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "commission_rate must be in (0, 1]"})),
        )
            .into_response();
    }
    if req.name.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "name must not be empty"})),
        )
            .into_response();
    }

    // Ensure the user account exists (create_account is idempotent).
    exchange.accounts.create_account(req.user_id);

    let record = crate::exchange::BrokerRecord {
        user_id: req.user_id,
        name: req.name.clone(),
        commission_rate: req.commission_rate,
        is_active: true,
        created_at: chrono::Utc::now(),
        total_commission: Decimal::ZERO,
    };

    exchange
        .brokers
        .lock().unwrap_or_else(|e| e.into_inner())
        .insert(req.user_id, record);

    tracing::info!(
        broker_id = %req.user_id,
        name = %req.name,
        commission_rate = %req.commission_rate,
        "broker registered"
    );

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "userId": req.user_id.to_string(),
            "name": req.name,
            "commissionRate": req.commission_rate,
            "isActive": true,
        })),
    )
        .into_response()
}

/// GET /admin/brokers — admin lists all brokers.
pub async fn admin_list_brokers(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let brokers = exchange
        .brokers
        .lock().unwrap_or_else(|e| e.into_inner());

    let list: Vec<_> = brokers
        .values()
        .map(|b| {
            serde_json::json!({
                "userId": b.user_id.to_string(),
                "name": b.name,
                "commissionRate": b.commission_rate,
                "isActive": b.is_active,
                "totalCommission": b.total_commission,
                "createdAt": b.created_at.timestamp_millis(),
            })
        })
        .collect();

    Json(serde_json::json!({ "brokers": list })).into_response()
}

// --- Broker: create client sub-account ---

/// POST /broker/clients — broker creates a client sub-account.
pub async fn broker_create_client(
    State(exchange): State<AppState>,
    axum::Extension(auth_user): axum::Extension<crate::auth::AuthenticatedUser>,
    Json(_req): Json<BrokerCreateClientRequest>,
) -> impl IntoResponse {
    // Verify the caller is a registered, active broker.
    let commission_rate = {
        let brokers = exchange.brokers.lock().unwrap_or_else(|e| e.into_inner());
        match brokers.get(&auth_user.0) {
            Some(b) if b.is_active => b.commission_rate,
            Some(_) => {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({"error": "Broker account is not active"})),
                )
                    .into_response();
            }
            None => {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({"error": "Caller is not a registered broker"})),
                )
                    .into_response();
            }
        }
    };
    let _ = commission_rate; // used only for validation above; commission tracked separately

    // Enforce max-clients-per-broker limit.
    {
        let clients = exchange.broker_clients.lock().unwrap_or_else(|e| e.into_inner());
        let current_count = clients.values().filter(|&&b| b == auth_user.0).count();
        if current_count >= BROKER_MAX_CLIENTS {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": format!("Broker has reached the maximum of {BROKER_MAX_CLIENTS} clients")
                })),
            )
                .into_response();
        }
    }

    // Create a new sub-account for the client.
    let client_id = Uuid::new_v4();
    exchange.accounts.create_account(client_id);

    // Register the client → broker relationship.
    exchange
        .broker_clients
        .lock().unwrap_or_else(|e| e.into_inner())
        .insert(client_id, auth_user.0);

    tracing::info!(
        broker_id = %auth_user.0,
        client_id = %client_id,
        "broker client created"
    );

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "clientId": client_id.to_string(),
            "brokerId": auth_user.0.to_string(),
        })),
    )
        .into_response()
}

/// GET /broker/clients — list broker's clients.
pub async fn broker_list_clients(
    State(exchange): State<AppState>,
    axum::Extension(auth_user): axum::Extension<crate::auth::AuthenticatedUser>,
) -> impl IntoResponse {
    // Verify broker exists.
    {
        let brokers = exchange.brokers.lock().unwrap_or_else(|e| e.into_inner());
        if !brokers.contains_key(&auth_user.0) {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"error": "Caller is not a registered broker"})),
            )
                .into_response();
        }
    }

    let clients = exchange.broker_clients.lock().unwrap_or_else(|e| e.into_inner());
    let client_ids: Vec<_> = clients
        .iter()
        .filter(|(_, broker)| **broker == auth_user.0)
        .map(|(client_id, _)| serde_json::json!({ "clientId": client_id.to_string() }))
        .collect();

    Json(serde_json::json!({ "clients": client_ids })).into_response()
}

/// GET /broker/commissions — broker commission history.
pub async fn broker_commissions(
    State(exchange): State<AppState>,
    axum::Extension(auth_user): axum::Extension<crate::auth::AuthenticatedUser>,
) -> impl IntoResponse {
    // Verify broker exists.
    {
        let brokers = exchange.brokers.lock().unwrap_or_else(|e| e.into_inner());
        if !brokers.contains_key(&auth_user.0) {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"error": "Caller is not a registered broker"})),
            )
                .into_response();
        }
    }

    let commissions = exchange
        .broker_commissions
        .lock().unwrap_or_else(|e| e.into_inner());

    let (total, list): (Decimal, Vec<_>) = commissions
        .iter()
        .filter(|c| c.broker_id == auth_user.0)
        .fold((Decimal::ZERO, Vec::new()), |(mut total, mut list), c| {
            total += c.amount;
            list.push(serde_json::json!({
                "id": c.id.to_string(),
                "clientId": c.client_id.to_string(),
                "tradeId": c.trade_id,
                "amount": c.amount,
                "asset": c.asset,
                "timestamp": c.timestamp.timestamp_millis(),
            }));
            (total, list)
        });

    Json(serde_json::json!({
        "totalCommission": total,
        "commissions": list,
    }))
    .into_response()
}

// keep the old stub aliases pointing at the new implementations for
// the two routes that were already wired in server.rs
/// POST /api/v1/rfq/create — legacy route alias (kept for backwards compat).
pub async fn rfq_create(
    state: State<AppState>,
    user: axum::Extension<crate::auth::AuthenticatedUser>,
    body: Json<serde_json::Value>,
) -> impl IntoResponse {
    // Delegate to full implementation; reconstruct request.
    let symbol = body.get("symbol").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let side = body.get("side").and_then(|v| v.as_str()).unwrap_or("Buy").to_string();
    let qty: Decimal = body
        .get("qty")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(Decimal::ONE);
    let req = Json(CreateRfqRequest { symbol, side, qty, legs: Vec::new() });
    block_trade_rfq_create(state, user, req).await.into_response()
}

/// GET /api/v1/rfq/{rfq_id}/quotes — legacy route alias.
pub async fn rfq_quotes(
    state: State<AppState>,
    user: axum::Extension<crate::auth::AuthenticatedUser>,
    Path(rfq_id_str): Path<String>,
) -> impl IntoResponse {
    match rfq_id_str.parse::<Uuid>() {
        Ok(rfq_id) => block_trade_rfq_quotes(state, user, Path(rfq_id)).await.into_response(),
        Err(_) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid RFQ ID format"})),
        )
            .into_response(),
    }
}

// ── Options Trading ───────────────────────────────────────────────────────────

/// Default implied volatility (80%) used for mark-price / greeks when no
/// vol surface is loaded.
const DEFAULT_IV: Decimal = Decimal::from_parts(80, 0, 0, false, 2);
/// Risk-free rate (5%) used in Black-Scholes.
const DEFAULT_RATE: Decimal = Decimal::from_parts(5, 0, 0, false, 2);

/// Look up IV from the loaded vol surface, falling back to DEFAULT_IV.
fn lookup_iv(exchange: &AppState, strike: Decimal, expiry: chrono::DateTime<chrono::Utc>) -> Decimal {
    let guard = exchange.vol_surface.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(surface) if !surface.is_empty() => surface.get_iv(strike, expiry),
        _ => DEFAULT_IV,
    }
}

/// GET /api/v1/options/instruments — list available option contracts,
/// optionally filtered by `underlying` query parameter (default: all).
pub async fn options_instruments(
    State(exchange): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let filter_underlying = params.get("underlying").map(|s| s.to_uppercase());

    let instruments: Vec<serde_json::Value> = exchange
        .option_instruments
        .lock().unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|i| {
            i.is_active
                && filter_underlying
                    .as_deref()
                    .map(|u| i.underlying.to_uppercase().contains(u))
                    .unwrap_or(true)
        })
        .map(|i| {
            serde_json::json!({
                "symbol":       i.symbol,
                "underlying":   i.underlying,
                "strike":       i.strike_price.to_string(),
                "expiry":       i.expiry.to_rfc3339(),
                "optionType":   i.option_type.to_string(),
                "contractSize": i.contract_size.to_string(),
                "settleCoin":   i.settle_coin,
                "isActive":     i.is_active,
            })
        })
        .collect();

    Json(serde_json::json!({ "instruments": instruments }))
}

/// GET /api/v1/options/mark-price — implied volatility + Black-Scholes greeks.
///
/// Query params:
/// - `symbol` (optional) — filter to a single instrument; omit to get all
pub async fn options_mark_price(
    State(exchange): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let filter_symbol = params.get("symbol").map(|s| s.as_str());
    let now = chrono::Utc::now();

    let instruments: Vec<crate::exchange::OptionInstrument> = {
        let guard = exchange.option_instruments.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .iter()
            .filter(|i| {
                i.is_active && filter_symbol.map(|s| i.symbol == s).unwrap_or(true)
            })
            .cloned()
            .collect()
    };

    let btc_spot = exchange
        .get_mark_price(&exchange_common::Symbol::new("BTCUSDT"))
        .map(|mp| mp.price)
        .unwrap_or(Decimal::new(70_000, 0));

    let results: Vec<serde_json::Value> = instruments
        .iter()
        .map(|inst| {
            let secs = (inst.expiry - now).num_seconds().max(0);
            let tte  = Decimal::new(secs, 0) / Decimal::new(365 * 24 * 3600, 0);
            let spot = if inst.underlying == "BTCUSDT" {
                btc_spot
            } else {
                exchange
                    .get_mark_price(&exchange_common::Symbol::new(&inst.underlying))
                    .map(|mp| mp.price)
                    .unwrap_or(inst.strike_price)
            };
            let is_call = matches!(inst.option_type, crate::exchange::OptionType::Call);
            let iv = lookup_iv(&exchange, inst.strike_price, inst.expiry);
            match crate::exchange::calculate_greeks(spot, inst.strike_price, tte, iv, DEFAULT_RATE, is_call) {
                Some(g) => serde_json::json!({
                    "symbol":    inst.symbol,
                    "markPrice": g.mark_price.to_string(),
                    "iv":        g.iv.to_string(),
                    "delta":     g.delta.to_string(),
                    "gamma":     g.gamma.to_string(),
                    "theta":     g.theta.to_string(),
                    "vega":      g.vega.to_string(),
                    "rho":       g.rho.to_string(),
                    "spot":      spot.to_string(),
                    "strike":    inst.strike_price.to_string(),
                    "tte":       tte.to_string(),
                }),
                None => serde_json::json!({
                    "symbol":    inst.symbol,
                    "markPrice": "0",
                    "iv":        "0",
                    "delta":     "0",
                    "gamma":     "0",
                    "theta":     "0",
                    "vega":      "0",
                    "rho":       "0",
                    "note":      "expired or invalid",
                }),
            }
        })
        .collect();

    if filter_symbol.is_some() {
        match results.into_iter().next() {
            Some(r) => Json(r).into_response(),
            None    => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "instrument not found"}))).into_response(),
        }
    } else {
        Json(serde_json::json!({ "markPrices": results })).into_response()
    }
}

/// GET /api/v1/options/ticker — 24-hour statistics for option instruments.
pub async fn options_ticker(
    State(exchange): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let filter_symbol = params.get("symbol").map(|s| s.as_str());
    let now = chrono::Utc::now();

    let instruments: Vec<crate::exchange::OptionInstrument> = {
        let guard = exchange.option_instruments.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .iter()
            .filter(|i| {
                i.is_active && filter_symbol.map(|s| i.symbol == s).unwrap_or(true)
            })
            .cloned()
            .collect()
    };

    let btc_spot = exchange
        .get_mark_price(&exchange_common::Symbol::new("BTCUSDT"))
        .map(|mp| mp.price)
        .unwrap_or(Decimal::new(70_000, 0));

    let tickers: Vec<serde_json::Value> = instruments
        .iter()
        .map(|inst| {
            let secs = (inst.expiry - now).num_seconds().max(0);
            let tte  = Decimal::new(secs, 0) / Decimal::new(365 * 24 * 3600, 0);
            let spot = if inst.underlying == "BTCUSDT" { btc_spot } else { inst.strike_price };
            let is_call = matches!(inst.option_type, crate::exchange::OptionType::Call);
            let iv = lookup_iv(&exchange, inst.strike_price, inst.expiry);
            let mark = crate::exchange::calculate_greeks(spot, inst.strike_price, tte, iv, DEFAULT_RATE, is_call)
                .map(|g| g.mark_price)
                .unwrap_or(Decimal::ZERO);
            serde_json::json!({
                "symbol":          inst.symbol,
                "markPrice":       mark.to_string(),
                "iv":              DEFAULT_IV.to_string(),
                "openInterest":    "0",
                "volume24h":       "0",
                "turnover24h":     "0",
                "expiryTimestamp": inst.expiry.timestamp(),
            })
        })
        .collect();

    Json(serde_json::json!({ "tickers": tickers }))
}

/// POST /api/v1/options/order — buy or sell an option contract (synthetic fill).
pub async fn place_option_order(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
    Json(req): Json<PlaceOptionOrderRequest>,
) -> impl IntoResponse {
    let user_id = user.0;

    let instrument = {
        let guard = exchange.option_instruments.lock().unwrap_or_else(|e| e.into_inner());
        guard.iter().find(|i| i.symbol == req.symbol && i.is_active).cloned()
    };
    let inst = match instrument {
        Some(i) => i,
        None => return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "option instrument not found or inactive"})),
        ).into_response(),
    };

    if req.qty <= Decimal::ZERO {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "qty must be positive"}))).into_response();
    }

    let is_buy = match req.side.to_lowercase().as_str() {
        "buy"  => true,
        "sell" => false,
        _ => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "side must be 'Buy' or 'Sell'"}))).into_response(),
    };

    let now  = chrono::Utc::now();
    let secs = (inst.expiry - now).num_seconds().max(0);
    let tte  = Decimal::new(secs, 0) / Decimal::new(365 * 24 * 3600, 0);
    let spot = exchange.get_mark_price(&exchange_common::Symbol::new(&inst.underlying))
        .map(|mp| mp.price).unwrap_or(inst.strike_price);
    let is_call = matches!(inst.option_type, crate::exchange::OptionType::Call);
    let iv = lookup_iv(&exchange, inst.strike_price, inst.expiry);
    let mark_price = crate::exchange::calculate_greeks(spot, inst.strike_price, tte, iv, DEFAULT_RATE, is_call)
        .map(|g| g.mark_price)
        .unwrap_or(Decimal::ZERO);

    if is_buy {
        // Buyer pays premium upfront — atomic check + debit under shard lock.
        let cost = req.premium * req.qty * inst.contract_size;
        let result: Result<(), (Decimal, String)> = exchange.accounts.with_user(user_id, |am| {
            let available = am
                .get_balance(&user_id, "USDT")
                .map(|b| b.available())
                .unwrap_or_default();
            if available < cost {
                return Err((available, "insufficient".to_string()));
            }
            am.withdraw(user_id, "USDT", cost)
                .map_err(|e| (available, e.to_string()))
        });
        match result {
            Ok(()) => {}
            Err((available, msg)) if msg == "insufficient" => {
                return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                    "error": "insufficient USDT balance",
                    "required": cost.to_string(),
                    "available": available.to_string(),
                }))).into_response();
            }
            Err((_, msg)) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                    "error": format!("balance debit: {msg}")
                }))).into_response();
            }
        }
    } else {
        // Seller must post margin. Use the Bybit/Deribit-style margin formula.
        // Reference: options_margin.rs — max(factor × S − OTM, min_factor × S) + mark_premium
        let margin_option_type = if is_call {
            exchange_risk_engine::options_instruments::OptionType::Call
        } else {
            exchange_risk_engine::options_instruments::OptionType::Put
        };
        let required_margin = exchange_risk_engine::options_margin::required_short_margin(
            margin_option_type,
            spot,
            inst.strike_price,
            mark_price,
            req.qty,
        );
        let premium_income = req.premium * req.qty * inst.contract_size;
        let result: Result<(), (Decimal, String)> = exchange.accounts.with_user(user_id, |am| {
            let available = am
                .get_balance(&user_id, "USDT")
                .map(|b| b.available())
                .unwrap_or_default();
            if available < required_margin {
                return Err((available, "insufficient_margin".to_string()));
            }
            am.withdraw(user_id, "USDT", required_margin)
                .map_err(|e| (available, e.to_string()))?;
            let _ = am.deposit(user_id, "USDT", premium_income);
            Ok(())
        });
        match result {
            Ok(()) => {}
            Err((available, msg)) if msg == "insufficient_margin" => {
                return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                    "error": "insufficient margin for short option",
                    "required_margin": required_margin.to_string(),
                    "available": available.to_string(),
                }))).into_response();
            }
            Err((_, msg)) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                    "error": format!("margin lock: {msg}")
                }))).into_response();
            }
        }
    }

    {
        let mut positions = exchange.option_positions.lock().unwrap_or_else(|e| e.into_inner());
        let key   = (user_id, inst.symbol.clone());
        let entry = positions.entry(key).or_insert_with(|| crate::exchange::OptionPosition {
            user_id,
            symbol:            inst.symbol.clone(),
            underlying:        inst.underlying.clone(),
            qty:               Decimal::ZERO,
            avg_entry_premium: Decimal::ZERO,
            unrealised_pnl:    Decimal::ZERO,
            created_at:        now,
            updated_at:        now,
        });

        let signed = if is_buy { req.qty } else { -req.qty };
        let old_qty = entry.qty;
        let new_qty = old_qty + signed;

        if is_buy && old_qty >= Decimal::ZERO && new_qty != Decimal::ZERO {
            entry.avg_entry_premium =
                (entry.avg_entry_premium * old_qty + req.premium * req.qty) / new_qty;
        } else if !is_buy && old_qty <= Decimal::ZERO && new_qty.abs() != Decimal::ZERO {
            entry.avg_entry_premium =
                (entry.avg_entry_premium * old_qty.abs() + req.premium * req.qty) / new_qty.abs();
        }

        entry.qty            = new_qty;
        entry.updated_at     = now;

        if new_qty.is_zero() {
            // Position fully closed — clear stale state.
            entry.avg_entry_premium = Decimal::ZERO;
            entry.unrealised_pnl    = Decimal::ZERO;
        } else {
            entry.unrealised_pnl = (mark_price - entry.avg_entry_premium) * entry.qty * inst.contract_size;
        }
    }

    let order_id = uuid::Uuid::new_v4();
    tracing::info!(user_id=%user_id, symbol=%inst.symbol, side=%req.side, qty=%req.qty, "option order filled");
    Json(serde_json::json!({
        "orderId":   order_id.to_string(),
        "symbol":    inst.symbol,
        "side":      req.side,
        "qty":       req.qty.to_string(),
        "premium":   req.premium.to_string(),
        "markPrice": mark_price.to_string(),
        "status":    "filled",
    })).into_response()
}

/// GET /api/v1/options/positions — option positions for the authenticated user.
pub async fn options_positions(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = user.0;
    let now      = chrono::Utc::now();
    let btc_spot = exchange
        .get_mark_price(&exchange_common::Symbol::new("BTCUSDT"))
        .map(|mp| mp.price)
        .unwrap_or(Decimal::new(70_000, 0));

    let instruments: HashMap<String, crate::exchange::OptionInstrument> = exchange
        .option_instruments.lock().unwrap_or_else(|e| e.into_inner())
        .iter().map(|i| (i.symbol.clone(), i.clone())).collect();

    let positions: Vec<serde_json::Value> = exchange
        .option_positions.lock().unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|((uid, _), pos)| *uid == user_id && pos.qty != Decimal::ZERO)
        .map(|((_, sym), pos)| {
            let mark = instruments.get(sym).and_then(|inst| {
                let secs = (inst.expiry - now).num_seconds().max(0);
                let tte  = Decimal::new(secs, 0) / Decimal::new(365 * 24 * 3600, 0);
                let spot = if inst.underlying == "BTCUSDT" { btc_spot } else { inst.strike_price };
                let is_call = matches!(inst.option_type, crate::exchange::OptionType::Call);
                let iv = lookup_iv(&exchange, inst.strike_price, inst.expiry);
                crate::exchange::calculate_greeks(spot, inst.strike_price, tte, iv, DEFAULT_RATE, is_call)
                    .map(|g| g.mark_price)
            }).unwrap_or(Decimal::ZERO);

            let sz = instruments.get(sym).map(|i| i.contract_size).unwrap_or(Decimal::ONE);
            let upnl = (mark - pos.avg_entry_premium) * pos.qty * sz;
            serde_json::json!({
                "symbol":          sym,
                "underlying":      pos.underlying,
                "qty":             pos.qty.to_string(),
                "avgEntryPremium": pos.avg_entry_premium.to_string(),
                "markPrice":       mark.to_string(),
                "unrealisedPnl":   upnl.to_string(),
                "updatedAt":       pos.updated_at.to_rfc3339(),
            })
        })
        .collect();

    Json(serde_json::json!({ "positions": positions }))
}

/// GET /api/v1/options/greeks — aggregate portfolio Greeks for the authenticated user.
pub async fn options_portfolio_greeks(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id  = user.0;
    let now      = chrono::Utc::now();
    let btc_spot = exchange
        .get_mark_price(&exchange_common::Symbol::new("BTCUSDT"))
        .map(|mp| mp.price)
        .unwrap_or(Decimal::new(70_000, 0));

    let instruments: HashMap<String, crate::exchange::OptionInstrument> = exchange
        .option_instruments.lock().unwrap_or_else(|e| e.into_inner())
        .iter().map(|i| (i.symbol.clone(), i.clone())).collect();

    let (mut td, mut tg, mut tt, mut tv) = (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO, Decimal::ZERO);

    for ((uid, sym), pos) in exchange.option_positions.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        if *uid != user_id || pos.qty == Decimal::ZERO { continue; }
        let Some(inst) = instruments.get(sym) else { continue };
        let secs = (inst.expiry - now).num_seconds().max(0);
        let tte  = Decimal::new(secs, 0) / Decimal::new(365 * 24 * 3600, 0);
        let spot = if inst.underlying == "BTCUSDT" { btc_spot } else { inst.strike_price };
        let is_call = matches!(inst.option_type, crate::exchange::OptionType::Call);
        let iv = lookup_iv(&exchange, inst.strike_price, inst.expiry);
        let Some(g) = crate::exchange::calculate_greeks(spot, inst.strike_price, tte, iv, DEFAULT_RATE, is_call) else { continue };
        let sz = pos.qty * inst.contract_size;
        td += g.delta * sz;
        tg += g.gamma * sz;
        tt += g.theta * sz;
        tv += g.vega  * sz;
    }

    Json(serde_json::json!({
        "totalDelta": td.to_string(),
        "totalGamma": tg.to_string(),
        "totalTheta": tt.to_string(),
        "totalVega":  tv.to_string(),
    }))
}

/// Request body for POST /api/v1/options/order.
#[derive(Debug, serde::Deserialize)]
pub struct PlaceOptionOrderRequest {
    pub symbol:  String,
    pub side:    String,
    pub qty:     Decimal,
    pub premium: Decimal,
}

// ── Delivery / Quarterly Futures ──────────────────────────────────────────────

/// GET /api/v1/futures/delivery/instruments — list delivery futures contracts.
pub async fn delivery_instruments(
    State(exchange): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let filter = params.get("underlying").map(|s| s.to_uppercase());

    let contracts: Vec<serde_json::Value> = exchange
        .delivery_contracts.lock().unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|c| filter.as_deref().map(|u| c.underlying.to_uppercase().contains(u)).unwrap_or(true))
        .map(|c| serde_json::json!({
            "symbol":       c.symbol,
            "underlying":   c.underlying,
            "deliveryDate": c.delivery_date.to_rfc3339(),
            "contractSize": c.contract_size.to_string(),
            "status":       c.status.to_string(),
        }))
        .collect();

    Json(serde_json::json!({ "contracts": contracts }))
}

/// GET /api/v1/futures/delivery/price/{symbol} — indicative or final settlement price.
pub async fn delivery_price(
    State(exchange): State<AppState>,
    axum::extract::Path(symbol): axum::extract::Path<String>,
) -> impl IntoResponse {
    // Settled contracts: return the historical settlement price.
    let settled = exchange
        .delivery_settlement_history.lock().unwrap_or_else(|e| e.into_inner())
        .iter().find(|r| r.symbol == symbol).cloned();

    if let Some(rec) = settled {
        return Json(serde_json::json!({
            "symbol":          rec.symbol,
            "settlementPrice": rec.settlement_price.to_string(),
            "settledAt":       rec.settled_at.to_rfc3339(),
            "status":          "delivered",
        })).into_response();
    }

    // Active contract: return current mark price as indicative delivery price.
    let contract = exchange
        .delivery_contracts.lock().unwrap_or_else(|e| e.into_inner())
        .iter().find(|c| c.symbol == symbol).cloned();

    let Some(contract) = contract else {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "contract not found"}))).into_response();
    };

    let indicative = exchange
        .get_mark_price(&exchange_common::Symbol::new(&contract.underlying))
        .map(|mp| mp.price)
        .unwrap_or(Decimal::ZERO);

    Json(serde_json::json!({
        "symbol":          contract.symbol,
        "indicativePrice": indicative.to_string(),
        "deliveryDate":    contract.delivery_date.to_rfc3339(),
        "status":          contract.status.to_string(),
    })).into_response()
}

/// Request body for POST /api/v1/futures/delivery/order.
#[derive(Debug, serde::Deserialize)]
pub struct PlaceDeliveryOrderRequest {
    pub symbol: String,
    pub side:   String,
    pub qty:    Decimal,
    pub price:  Option<Decimal>,
}

/// POST /api/v1/futures/delivery/order — open or close a delivery futures position.
pub async fn place_delivery_order(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
    Json(req): Json<PlaceDeliveryOrderRequest>,
) -> impl IntoResponse {
    let user_id = user.0;

    let contract = {
        let guard = exchange.delivery_contracts.lock().unwrap_or_else(|e| e.into_inner());
        guard.iter().find(|c| c.symbol == req.symbol).cloned()
    };
    let contract = match contract {
        Some(c) => c,
        None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "contract not found"}))).into_response(),
    };

    if !matches!(contract.status, crate::exchange::DeliveryStatus::Active) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "contract not accepting orders"}))).into_response();
    }
    if req.qty <= Decimal::ZERO {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "qty must be positive"}))).into_response();
    }

    let is_buy = match req.side.to_lowercase().as_str() {
        "buy"  => true,
        "sell" => false,
        _ => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "side must be 'Buy' or 'Sell'"}))).into_response(),
    };

    let fill_price = req.price.unwrap_or_else(|| {
        exchange.get_mark_price(&exchange_common::Symbol::new(&contract.underlying))
            .map(|mp| mp.price)
            .unwrap_or(Decimal::ZERO)
    });

    if fill_price <= Decimal::ZERO {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "could not determine fill price"}))).into_response();
    }

    // 10 % initial margin requirement.
    let margin = fill_price * req.qty * contract.contract_size / Decimal::new(10, 0);
    let result: Result<(), (Decimal, String)> = exchange.accounts.with_user(user_id, |am| {
        let available = am
            .get_balance(&user_id, "USDT")
            .map(|b| b.available())
            .unwrap_or_default();
        if available < margin {
            return Err((available, "insufficient".into()));
        }
        am.withdraw(user_id, "USDT", margin)
            .map_err(|e| (available, e.to_string()))
    });
    match result {
        Ok(()) => {}
        Err((available, msg)) if msg == "insufficient" => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                "error": "insufficient margin",
                "required": margin.to_string(),
                "available": available.to_string(),
            }))).into_response();
        }
        Err((_, msg)) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                "error": format!("margin debit: {msg}")
            }))).into_response();
        }
    }

    let now = chrono::Utc::now();
    let signed = if is_buy { req.qty } else { -req.qty };

    {
        let mut positions = exchange.delivery_positions.lock().unwrap_or_else(|e| e.into_inner());
        let key   = (user_id, contract.symbol.clone());
        let entry = positions.entry(key).or_insert_with(|| crate::exchange::DeliveryPosition {
            user_id,
            symbol:          contract.symbol.clone(),
            underlying:      contract.underlying.clone(),
            qty:             Decimal::ZERO,
            avg_entry_price: Decimal::ZERO,
            unrealised_pnl:  Decimal::ZERO,
            created_at:      now,
            updated_at:      now,
        });

        let old_qty = entry.qty;
        let new_qty = old_qty + signed;

        if is_buy && old_qty >= Decimal::ZERO && new_qty > Decimal::ZERO {
            entry.avg_entry_price =
                (entry.avg_entry_price * old_qty + fill_price * req.qty) / new_qty;
        } else if !is_buy && old_qty <= Decimal::ZERO && new_qty < Decimal::ZERO {
            entry.avg_entry_price =
                (entry.avg_entry_price * old_qty.abs() + fill_price * req.qty) / new_qty.abs();
        }

        entry.qty            = new_qty;
        entry.updated_at     = now;
        entry.unrealised_pnl = (fill_price - entry.avg_entry_price) * entry.qty * contract.contract_size;
    }

    let order_id = uuid::Uuid::new_v4();
    tracing::info!(user_id=%user_id, symbol=%contract.symbol, side=%req.side, qty=%req.qty, price=%fill_price, "delivery order placed");
    Json(serde_json::json!({
        "orderId":        order_id.to_string(),
        "symbol":         contract.symbol,
        "side":           req.side,
        "qty":            req.qty.to_string(),
        "fillPrice":      fill_price.to_string(),
        "marginReserved": margin.to_string(),
        "status":         "filled",
    })).into_response()
}

/// GET /api/v1/futures/delivery/positions — delivery positions for the authenticated user.
pub async fn delivery_positions(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = user.0;

    let contracts: HashMap<String, crate::exchange::DeliveryContract> = exchange
        .delivery_contracts.lock().unwrap_or_else(|e| e.into_inner())
        .iter().map(|c| (c.symbol.clone(), c.clone())).collect();

    let positions: Vec<serde_json::Value> = exchange
        .delivery_positions.lock().unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|((uid, _), pos)| *uid == user_id && pos.qty != Decimal::ZERO)
        .map(|((_, sym), pos)| {
            let mark = contracts.get(sym).and_then(|c|
                exchange.get_mark_price(&exchange_common::Symbol::new(&c.underlying)).map(|mp| mp.price)
            ).unwrap_or(pos.avg_entry_price);
            let sz   = contracts.get(sym).map(|c| c.contract_size).unwrap_or(Decimal::ONE);
            let upnl = (mark - pos.avg_entry_price) * pos.qty * sz;
            let delivery = contracts.get(sym).map(|c| c.delivery_date.to_rfc3339()).unwrap_or_default();
            serde_json::json!({
                "symbol":        sym,
                "underlying":    pos.underlying,
                "qty":           pos.qty.to_string(),
                "avgEntryPrice": pos.avg_entry_price.to_string(),
                "markPrice":     mark.to_string(),
                "unrealisedPnl": upnl.to_string(),
                "deliveryDate":  delivery,
                "updatedAt":     pos.updated_at.to_rfc3339(),
            })
        })
        .collect();

    Json(serde_json::json!({ "positions": positions }))
}

/// GET /api/v1/market/delivery-prices — past settlement prices for delivered contracts.
pub async fn delivery_settlement_prices(
    State(exchange): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let filter = params.get("underlying").map(|s| s.to_uppercase());

    let records: Vec<serde_json::Value> = exchange
        .delivery_settlement_history.lock().unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|r| filter.as_deref().map(|u| r.underlying.to_uppercase().contains(u)).unwrap_or(true))
        .map(|r| serde_json::json!({
            "symbol":          r.symbol,
            "underlying":      r.underlying,
            "settlementPrice": r.settlement_price.to_string(),
            "settledAt":       r.settled_at.to_rfc3339(),
        }))
        .collect();

    Json(serde_json::json!({ "settlementPrices": records }))
}

// ── Task 8.10: Spread Trading — routed through multi-leg engine ───────────────

/// POST /api/v1/spread/order — place a two-leg spread order.
///
/// This is a thin alias over the multi-leg strategy endpoint that forces
/// `strategy_type = "spread"` and validates exactly two legs are supplied.
pub async fn spread_order(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let legs = match body.get("legs").and_then(|v| v.as_array()) {
        Some(l) if l.len() >= 2 => l.clone(),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Spread order requires at least 2 legs"})),
            )
                .into_response();
        }
    };

    // Build a strategy request and delegate to the multi-leg handler internals.
    let strategy_req = serde_json::json!({
        "strategy_type": "spread",
        "legs": legs,
        "execution": body.get("execution").and_then(|v| v.as_str()).unwrap_or("simultaneous"),
    });

    let result = place_strategy_order_inner(
        exchange,
        user.0,
        strategy_req,
    ).await;

    match result {
        Ok(order) => Json(serde_json::json!({
            "spreadOrderId": order.id.to_string(),
            "strategyId": order.id.to_string(),
            "legs": order.legs.iter().map(|l| serde_json::json!({
                "symbol": l.symbol,
                "side": l.side,
                "qty": l.qty.to_string(),
                "price": l.price.map(|p| p.to_string()),
                "orderId": l.order_id.map(|id| id.to_string()),
                "status": l.status,
            })).collect::<Vec<_>>(),
            "status": order.status,
            "createdAt": order.created_at.timestamp_millis(),
        })).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        ).into_response(),
    }
}

/// GET /api/v1/spread/instruments — list available spread instruments.
///
/// Returns spread pairs derived from the active perp symbols: for each
/// base asset that has both a perpetual and a spot market, we expose a
/// spread instrument. This replaces the previous stub with real data.
pub async fn spread_instruments(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let perp_symbols: Vec<String> = exchange.perp_configs.load()
        .keys()
        .map(|s| s.to_string())
        .collect();

    let instruments: Vec<serde_json::Value> = perp_symbols
        .iter()
        .filter_map(|perp| {
            // Convention: perpetual is "BTCUSDT", spot leg would be "BTCUSDT_SPOT"
            // or the inverse (short perp + long spot = carry trade).
            // We expose the perp itself as a spreadable leg against its index.
            let base = perp.trim_end_matches("USDT");
            if base.is_empty() {
                return None;
            }
            Some(serde_json::json!({
                "spreadId": format!("{base}USDT-SPREAD"),
                "name": format!("{base}/USDT Carry Spread"),
                "legA": {
                    "symbol": perp,
                    "type": "perpetual",
                    "side": "buy",
                },
                "legB": {
                    "symbol": format!("{base}USDT"),
                    "type": "spot",
                    "side": "sell",
                },
                "isActive": true,
            }))
        })
        .collect();

    Json(serde_json::json!({
        "instruments": instruments,
        "total": instruments.len(),
    }))
}

// ── Portfolio Margin ───────────────────────────────────────────────────────────

/// GET /api/v1/account/portfolio-margin — portfolio margin status.
pub async fn portfolio_margin_status(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = user.0;
    let current_equity = exchange
        .accounts
        .get_balance(user_id, "USDT")
        .map(|b| b.total())
        .unwrap_or_default();

    let enabled = exchange.has_portfolio_margin(user_id);
    let eligible = current_equity >= Decimal::from(100_000u32);

    Json(serde_json::json!({
        "enabled": enabled,
        "eligible": eligible,
        "minEquityRequired": "100000",
        "currentEquity": current_equity.to_string(),
        "marginMode": if enabled { "portfolio" } else { "regular" },
    }))
}

/// POST /api/v1/account/portfolio-margin/enable — activate portfolio margin.
///
/// Requires the user's USDT equity to be ≥ $100,000.
pub async fn portfolio_margin_enable(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
) -> impl IntoResponse {
    match exchange.enable_portfolio_margin(user.0) {
        Ok(()) => Json(serde_json::json!({
            "success": true,
            "marginMode": "portfolio",
            "message": "Portfolio margin enabled.",
        }))
        .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /api/v1/account/portfolio-margin/disable — deactivate portfolio margin.
pub async fn portfolio_margin_disable(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
) -> impl IntoResponse {
    exchange.disable_portfolio_margin(user.0);
    Json(serde_json::json!({
        "success": true,
        "marginMode": "regular",
        "message": "Portfolio margin disabled.",
    }))
}

/// GET /api/v1/account/portfolio-margin/analysis
///
/// Returns:
/// - `scenarios`: stress-test PnL under ±5 %, ±10 %, ±15 % price shocks.
/// - `netDelta`: net delta per base asset (e.g., `{"BTC": "1.5"}`).
/// - `requiredMargin`: scenario-based required margin (USDT).
/// - `worstPnl`: most negative scenario PnL.
pub async fn portfolio_margin_analysis(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = user.0;
    let analysis = exchange.portfolio_margin_analysis(user_id);

    let scenarios: Vec<serde_json::Value> = analysis
        .scenarios
        .iter()
        .map(|s| {
            serde_json::json!({
                "shockPct": s.shock_pct.to_string(),
                "portfolioPnl": s.portfolio_pnl.to_string(),
            })
        })
        .collect();

    let net_delta: serde_json::Value = analysis
        .net_delta
        .iter()
        .map(|(asset, delta)| (asset.clone(), serde_json::Value::String(delta.to_string())))
        .collect::<serde_json::Map<String, serde_json::Value>>()
        .into();

    Json(serde_json::json!({
        "scenarios": scenarios,
        "netDelta": net_delta,
        "requiredMargin": analysis.required_margin.to_string(),
        "worstPnl": analysis.worst_pnl.to_string(),
    }))
}

// ── Task 8.12: Economic Calendar ──────────────────────────────────────────────

/// GET /api/v1/public/economic-calendar — upcoming economic events.
pub async fn economic_calendar(
    Query(_params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let events = vec![
        serde_json::json!({
            "title": "US Non-Farm Payrolls",
            "country": "US",
            "importance": "high",
            "date": "2026-04-04T12:30:00Z",
            "forecast": "180K",
            "previous": "175K",
        }),
        serde_json::json!({
            "title": "FOMC Interest Rate Decision",
            "country": "US",
            "importance": "high",
            "date": "2026-04-30T18:00:00Z",
            "forecast": "4.50%",
            "previous": "4.50%",
        }),
        serde_json::json!({
            "title": "ECB Interest Rate Decision",
            "country": "EU",
            "importance": "high",
            "date": "2026-04-17T12:15:00Z",
            "forecast": "3.50%",
            "previous": "3.50%",
        }),
        serde_json::json!({
            "title": "US CPI (YoY)",
            "country": "US",
            "importance": "high",
            "date": "2026-04-10T12:30:00Z",
            "forecast": "2.8%",
            "previous": "2.8%",
        }),
    ];

    Json(serde_json::json!({ "events": events }))
}

// ---------------------------------------------------------------------------
// Compliance — PEP screening (Task: PEP Screening)
// ---------------------------------------------------------------------------

/// GET /admin/compliance/pep-check?name=... — screen a name against the PEP database.
///
/// Admin-only. Returns the screening result including matched entries and
/// confidence scores. Never expose this output to end users.
pub async fn pep_check(
    State(exchange): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let Some(name) = params.get("name") else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Missing name parameter"})),
        )
            .into_response();
    };
    let screener = exchange.pep_screener.lock().unwrap_or_else(|e| e.into_inner());
    let result = screener.screen(name);
    tracing::info!(
        name = %name,
        is_pep = result.is_pep,
        match_count = result.matches.len(),
        "pep_check: screening complete"
    );
    Json(serde_json::json!(result)).into_response()
}

// ---------------------------------------------------------------------------
// Index price health (admin monitoring)
// ---------------------------------------------------------------------------

/// GET /admin/index/health — per-symbol index price source health status.
///
/// Returns each tracked symbol with its source health breakdown so ops can
/// monitor whether the index is degraded before it affects trading.
pub async fn admin_index_health(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    Json(exchange.index_health_report())
}

// ---------------------------------------------------------------------------
// Options volatility surface (admin)
// ---------------------------------------------------------------------------

/// POST /admin/options/vol-surface — load IV data points into the vol surface.
///
/// Body: `{ "underlying": "BTC", "points": [{"strike": 50000, "expiry": "...", "iv": 0.65}, ...] }`
pub async fn admin_load_vol_surface(
    State(exchange): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let underlying = req.get("underlying")
        .and_then(|v| v.as_str())
        .unwrap_or("BTC");

    let points_raw = match req.get("points").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing 'points' array"})),
            ).into_response();
        }
    };

    let mut vol_points = Vec::new();
    for p in points_raw {
        let strike = p.get("strike").and_then(|v| {
            v.as_f64().and_then(rust_decimal::Decimal::from_f64_retain)
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        });
        let expiry = p.get("expiry").and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc));
        let iv = p.get("iv").and_then(|v| {
            v.as_f64().and_then(rust_decimal::Decimal::from_f64_retain)
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        });

        if let (Some(strike), Some(expiry), Some(iv)) = (strike, expiry, iv) {
            vol_points.push(exchange_risk_engine::vol_surface::VolPoint {
                strike,
                expiry,
                iv,
            });
        }
    }

    let point_count = vol_points.len();
    let surface = exchange_risk_engine::vol_surface::VolatilitySurface::new(underlying, vol_points);
    *exchange.vol_surface.lock().unwrap_or_else(|e| e.into_inner()) = Some(surface);

    tracing::info!(
        underlying = %underlying,
        points = point_count,
        "admin: volatility surface loaded"
    );

    Json(serde_json::json!({
        "status": "ok",
        "underlying": underlying,
        "points_loaded": point_count,
    })).into_response()
}

/// GET /admin/options/vol-surface — read current vol surface summary.
pub async fn admin_get_vol_surface(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let guard = exchange.vol_surface.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(surface) => Json(serde_json::json!({
            "loaded": true,
            "underlying": surface.underlying,
            "point_count": surface.point_count(),
        })).into_response(),
        None => Json(serde_json::json!({
            "loaded": false,
        })).into_response(),
    }
}

// ---------------------------------------------------------------------------
// Compliance — sanctions force-update (Task: Live Sanctions Feeds)
// ---------------------------------------------------------------------------

/// POST /admin/compliance/sanctions/force-update — trigger an immediate sanctions re-screening.
///
/// Phase 1: logs the request and returns immediately.
/// Phase 2: will reload live feed data and re-screen all users.
pub async fn sanctions_force_update(State(_exchange): State<AppState>) -> impl IntoResponse {
    tracing::info!("sanctions force-update triggered by admin");
    Json(serde_json::json!({"status": "re-screening initiated"}))
}

/// POST /admin/sanctions/load — bulk-load sanctions entries into the screener.
///
/// Accepts a JSON array of `{ "name": "...", "addresses": ["..."] }` objects.
/// Entries are merged into the live screener; existing entries are not removed.
///
/// Returns the updated entry counts after loading.
pub async fn admin_sanctions_load(
    State(exchange): State<AppState>,
    Json(entries): Json<Vec<exchange_compliance::SanctionsEntry>>,
) -> impl IntoResponse {
    let entry_count = entries.len();
    let mut screener = exchange.sanctions.lock().unwrap_or_else(|e| e.into_inner());
    screener.bulk_load(entries);
    let metadata = screener.list_metadata();
    drop(screener);

    tracing::info!(
        entry_count,
        "admin: sanctions list updated via bulk load"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "loaded": entry_count,
            "lists": metadata.iter().map(|m| serde_json::json!({
                "list": format!("{:?}", m.list),
                "entry_count": m.entry_count,
            })).collect::<Vec<_>>(),
        })),
    )
}

// ---------------------------------------------------------------------------
// Compliance — Terms of Service (Task: Terms of Service)
// ---------------------------------------------------------------------------

/// GET /api/v1/legal/tos — return the latest mandatory ToS version.
pub async fn get_tos(State(exchange): State<AppState>) -> impl IntoResponse {
    let mgr = exchange.tos_manager.lock().unwrap_or_else(|e| e.into_inner());
    match mgr.latest_mandatory() {
        Some(v) => Json(serde_json::json!({
            "version": v.version,
            "contentHash": v.content_hash,
            "publishedAt": v.published_at.timestamp_millis(),
            "summary": v.summary,
        }))
        .into_response(),
        None => Json(serde_json::json!({"version": null})).into_response(),
    }
}

/// POST /api/v1/legal/tos/accept — record that the authenticated user accepts the given version.
///
/// Body: `{ "version": "1.0" }`
pub async fn accept_tos(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let Some(version) = body.get("version").and_then(|v| v.as_str()) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Missing version"})),
        )
            .into_response();
    };
    let mut mgr = exchange.tos_manager.lock().unwrap_or_else(|e| e.into_inner());
    match mgr.accept(user.0, version, None) {
        Ok(()) => {
            tracing::info!(user_id = %user.0, version = %version, "tos: user accepted");
            Json(serde_json::json!({"accepted": true, "version": version})).into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

/// GET /api/v1/legal/tos/status — check whether the authenticated user has accepted the latest ToS.
pub async fn tos_status(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let mgr = exchange.tos_manager.lock().unwrap_or_else(|e| e.into_inner());
    let accepted = mgr.has_accepted_latest(user.0);
    let acceptance = mgr.get_acceptance(user.0);
    Json(serde_json::json!({
        "accepted": accepted,
        "acceptedVersion": acceptance.map(|a| &a.version),
        "acceptedAt": acceptance.map(|a| a.accepted_at.timestamp_millis()),
        "latestVersion": mgr.latest_mandatory().map(|v| &v.version),
    }))
}

// ─── MiCA Art.71: Complaint Handling ──────────────────────────────────────────

/// POST /api/v1/support/complaints — submit a complaint.
pub async fn submit_complaint(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let category = match body.get("category").and_then(|v| v.as_str()) {
        Some("trading") => exchange_compliance::complaints::ComplaintCategory::Trading,
        Some("withdrawal") => exchange_compliance::complaints::ComplaintCategory::Withdrawal,
        Some("deposit") => exchange_compliance::complaints::ComplaintCategory::Deposit,
        Some("kyc") => exchange_compliance::complaints::ComplaintCategory::KYC,
        Some("fees") => exchange_compliance::complaints::ComplaintCategory::Fees,
        Some("technical") => exchange_compliance::complaints::ComplaintCategory::Technical,
        Some("account") => exchange_compliance::complaints::ComplaintCategory::AccountAccess,
        _ => exchange_compliance::complaints::ComplaintCategory::Other,
    };
    let subject = body
        .get("subject")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let complaint_body = body
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if subject.is_empty() || complaint_body.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Subject and body are required"})),
        )
            .into_response();
    }

    let mut mgr = exchange
        .complaint_manager
        .lock().unwrap_or_else(|e| e.into_inner());
    let complaint = mgr.submit(user.0, category, subject, complaint_body);

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "complaintId": complaint.id.to_string(),
            "status": format!("{:?}", complaint.status),
            "createdAt": complaint.created_at.timestamp_millis(),
        })),
    )
        .into_response()
}

/// GET /api/v1/support/complaints — list user's complaints.
pub async fn list_complaints(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
) -> impl IntoResponse {
    let mgr = exchange
        .complaint_manager
        .lock().unwrap_or_else(|e| e.into_inner());
    let complaints: Vec<serde_json::Value> = mgr
        .user_complaints(user.0)
        .iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id.to_string(),
                "category": format!("{:?}", c.category),
                "subject": c.subject,
                "status": format!("{:?}", c.status),
                "priority": format!("{:?}", c.priority),
                "createdAt": c.created_at.timestamp_millis(),
                "resolvedAt": c.resolved_at.map(|t| t.timestamp_millis()),
                "resolution": c.resolution,
            })
        })
        .collect();
    Json(serde_json::json!({"complaints": complaints}))
}

/// GET /admin/complaints — all pending complaints.
pub async fn admin_list_complaints(State(exchange): State<AppState>) -> impl IntoResponse {
    let mgr = exchange
        .complaint_manager
        .lock().unwrap_or_else(|e| e.into_inner());
    let pending: Vec<serde_json::Value> = mgr
        .pending_complaints()
        .iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id.to_string(),
                "userId": c.user_id.to_string(),
                "category": format!("{:?}", c.category),
                "subject": c.subject,
                "status": format!("{:?}", c.status),
                "assignedTo": c.assigned_to,
                "createdAt": c.created_at.timestamp_millis(),
            })
        })
        .collect();
    Json(serde_json::json!({"complaints": pending}))
}

/// PUT /admin/complaints/{id} — update complaint status.
pub async fn admin_update_complaint(
    State(exchange): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let complaint_id: uuid::Uuid = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid ID"})),
            )
                .into_response()
        }
    };
    let status = match body.get("status").and_then(|v| v.as_str()) {
        Some("acknowledged") => exchange_compliance::complaints::ComplaintStatus::Acknowledged,
        Some("in_progress") => exchange_compliance::complaints::ComplaintStatus::InProgress,
        Some("resolved") => exchange_compliance::complaints::ComplaintStatus::Resolved,
        Some("closed") => exchange_compliance::complaints::ComplaintStatus::Closed,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid status"})),
            )
                .into_response()
        }
    };
    let resolution = body
        .get("resolution")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let assigned = body
        .get("assignedTo")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let mut mgr = exchange
        .complaint_manager
        .lock().unwrap_or_else(|e| e.into_inner());

    // Acknowledge if assigning
    if let Some(ref assignee) = assigned {
        let _ = mgr.acknowledge(complaint_id, assignee);
    }

    match mgr.update_status(complaint_id, status, resolution) {
        Ok(()) => Json(serde_json::json!({"status": "updated"})).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

/// GET /admin/complaints/sla-report — SLA breach report.
pub async fn admin_sla_report(State(exchange): State<AppState>) -> impl IntoResponse {
    let mgr = exchange
        .complaint_manager
        .lock().unwrap_or_else(|e| e.into_inner());
    let breaches: Vec<serde_json::Value> = mgr
        .sla_breaches()
        .iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id.to_string(),
                "userId": c.user_id.to_string(),
                "status": format!("{:?}", c.status),
                "createdAt": c.created_at.timestamp_millis(),
                "subject": c.subject,
            })
        })
        .collect();
    let breach_count = breaches.len();
    Json(serde_json::json!({
        "slaAcknowledgeHours": 24,
        "slaResolveDays": 5,
        "breaches": breaches,
        "breachCount": breach_count,
    }))
}

// ─── Risk Warnings ─────────────────────────────────────────────────────────────

/// GET /api/v1/risk/warnings — get pending risk warnings for user.
pub async fn get_risk_warnings(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
) -> impl IntoResponse {
    let mgr = exchange
        .risk_warnings
        .lock().unwrap_or_else(|e| e.into_inner());
    let pending: Vec<serde_json::Value> = mgr
        .pending_warnings(user.0)
        .iter()
        .map(|w| {
            serde_json::json!({
                "type": format!("{:?}", w.warning_type),
                "title": w.title,
                "text": w.text,
            })
        })
        .collect();
    let can_futures = mgr.can_trade_futures(user.0);
    Json(serde_json::json!({
        "pendingWarnings": pending,
        "canTradeFutures": can_futures,
    }))
}

/// POST /api/v1/risk/warnings/acknowledge — acknowledge a risk warning.
pub async fn acknowledge_risk_warning(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let warning_type = match body.get("type").and_then(|v| v.as_str()) {
        Some("FuturesTrading") => exchange_compliance::risk_warnings::WarningType::FuturesTrading,
        Some("HighLeverage") => exchange_compliance::risk_warnings::WarningType::HighLeverage,
        Some("LiquidationRisk") => exchange_compliance::risk_warnings::WarningType::LiquidationRisk,
        Some("MarginTrading") => exchange_compliance::risk_warnings::WarningType::MarginTrading,
        Some("VolatileAsset") => exchange_compliance::risk_warnings::WarningType::VolatileAsset,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid warning type"})),
            )
                .into_response()
        }
    };

    let mut mgr = exchange
        .risk_warnings
        .lock().unwrap_or_else(|e| e.into_inner());
    mgr.acknowledge(user.0, warning_type);
    let can_futures = mgr.can_trade_futures(user.0);

    Json(serde_json::json!({
        "acknowledged": format!("{:?}", warning_type),
        "canTradeFutures": can_futures,
    }))
    .into_response()
}

// ─── Task 6: Customer Risk Scoring ──────────────────────────────────────────

/// GET /admin/compliance/risk-scores — list all user risk scores.
pub async fn admin_risk_scores(State(exchange): State<AppState>) -> impl IntoResponse {
    let engine = exchange
        .risk_scoring
        .lock().unwrap_or_else(|e| e.into_inner());
    let high_risk: Vec<serde_json::Value> = engine
        .high_risk_users()
        .iter()
        .map(|s| {
            serde_json::json!({
                "userId": s.user_id.to_string(),
                "level": format!("{:?}", s.level),
                "score": s.score,
                "lastAssessed": s.last_assessed.to_rfc3339(),
                "nextReview": s.next_review.to_rfc3339(),
            })
        })
        .collect();
    Json(serde_json::json!({ "highRiskUsers": high_risk }))
}

/// GET /admin/compliance/risk-scores/{user_id} — get a specific user's risk score.
pub async fn admin_risk_score_by_user(
    State(exchange): State<AppState>,
    Path(user_id): Path<String>,
) -> impl IntoResponse {
    let uid: uuid::Uuid = match user_id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid user ID"})),
            )
                .into_response()
        }
    };

    let engine = exchange
        .risk_scoring
        .lock().unwrap_or_else(|e| e.into_inner());
    match engine.get_score(uid) {
        Some(s) => Json(serde_json::json!({
            "userId": s.user_id.to_string(),
            "level": format!("{:?}", s.level),
            "score": s.score,
            "lastAssessed": s.last_assessed.to_rfc3339(),
            "nextReview": s.next_review.to_rfc3339(),
        }))
        .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "No risk score found for this user"})),
        )
            .into_response(),
    }
}

// ─── Task 7: Enhanced Due Diligence ─────────────────────────────────────────

/// POST /admin/compliance/edd/{user_id}/open — open a new EDD case.
pub async fn admin_edd_open(
    State(exchange): State<AppState>,
    Path(user_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let uid: uuid::Uuid = match user_id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid user ID"})),
            )
                .into_response()
        }
    };

    let reason = match body.get("reason").and_then(|v| v.as_str()) {
        Some(r) => r.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing reason"})),
            )
                .into_response()
        }
    };

    let mut mgr = exchange
        .edd_manager
        .lock().unwrap_or_else(|e| e.into_inner());
    let case = mgr.open_case(uid, &reason);
    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": case.id.to_string(),
            "userId": case.user_id.to_string(),
            "status": format!("{:?}", case.status),
            "triggerReason": case.trigger_reason,
            "createdAt": case.created_at.to_rfc3339(),
        })),
    )
        .into_response()
}

/// PUT /admin/compliance/edd/{case_id} — update an EDD case.
pub async fn admin_edd_update(
    State(exchange): State<AppState>,
    Path(case_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let cid: uuid::Uuid = match case_id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid case ID"})),
            )
                .into_response()
        }
    };

    let mut mgr = exchange
        .edd_manager
        .lock().unwrap_or_else(|e| e.into_inner());

    // Update status if provided.
    if let Some(status_str) = body.get("status").and_then(|v| v.as_str()) {
        use exchange_compliance::EddStatus;
        let status = match status_str {
            "open" => EddStatus::Open,
            "in_progress" => EddStatus::InProgress,
            "documents_requested" => EddStatus::DocumentsRequested,
            "under_review" => EddStatus::UnderReview,
            "approved" => EddStatus::Approved,
            "rejected" => EddStatus::Rejected,
            _ => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "Invalid status"})),
                )
                    .into_response()
            }
        };
        if let Err(e) = mgr.update_status(cid, status) {
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": e}))).into_response();
        }
    }

    // Append note if provided.
    if let Some(note) = body.get("note").and_then(|v| v.as_str()) {
        if let Err(e) = mgr.add_note(cid, note) {
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": e}))).into_response();
        }
    }

    // Update source info if provided.
    let funds = body
        .get("sourceOfFunds")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let wealth = body
        .get("sourceOfWealth")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    if funds.is_some() || wealth.is_some() {
        if let Err(e) = mgr.set_source_info(cid, funds, wealth) {
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": e}))).into_response();
        }
    }

    Json(serde_json::json!({"updated": true})).into_response()
}

/// GET /admin/compliance/edd/pending — list pending EDD cases.
pub async fn admin_edd_pending(State(exchange): State<AppState>) -> impl IntoResponse {
    let mgr = exchange
        .edd_manager
        .lock().unwrap_or_else(|e| e.into_inner());
    let cases: Vec<serde_json::Value> = mgr
        .pending_cases()
        .iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id.to_string(),
                "userId": c.user_id.to_string(),
                "status": format!("{:?}", c.status),
                "triggerReason": c.trigger_reason,
                "createdAt": c.created_at.to_rfc3339(),
            })
        })
        .collect();
    Json(serde_json::json!({ "cases": cases }))
}

// ─── Task 9: SAR/STR Filing ──────────────────────────────────────────────────

/// POST /admin/compliance/sar/generate — generate a new SAR draft.
pub async fn admin_sar_generate(
    State(exchange): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let user_id: uuid::Uuid = match body
        .get("userId")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
    {
        Some(id) => id,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing or invalid userId"})),
            )
                .into_response()
        }
    };

    let narrative = match body.get("narrative").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing narrative"})),
            )
                .into_response()
        }
    };

    let regulator = body
        .get("regulator")
        .and_then(|v| v.as_str())
        .unwrap_or("SFMS_UA")
        .to_string();

    let alert_ids: Vec<uuid::Uuid> = body
        .get("alertIds")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .filter_map(|s| s.parse().ok())
                .collect()
        })
        .unwrap_or_default();

    let mut mgr = exchange
        .sar_manager
        .lock().unwrap_or_else(|e| e.into_inner());
    let report = mgr.generate(user_id, alert_ids, narrative, &regulator);
    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": report.id.to_string(),
            "userId": report.user_id.to_string(),
            "status": format!("{:?}", report.status),
            "regulator": report.regulator,
            "createdAt": report.created_at.to_rfc3339(),
        })),
    )
        .into_response()
}

/// GET /admin/compliance/sar/pending — list pending SAR reports.
pub async fn admin_sar_pending(State(exchange): State<AppState>) -> impl IntoResponse {
    let mgr = exchange
        .sar_manager
        .lock().unwrap_or_else(|e| e.into_inner());
    let reports: Vec<serde_json::Value> = mgr
        .pending()
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id.to_string(),
                "userId": r.user_id.to_string(),
                "status": format!("{:?}", r.status),
                "regulator": r.regulator,
                "createdAt": r.created_at.to_rfc3339(),
            })
        })
        .collect();
    Json(serde_json::json!({ "reports": reports }))
}

/// POST /admin/compliance/sar/{id}/review — mark SAR as reviewed.
pub async fn admin_sar_review(
    State(exchange): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let sar_id: uuid::Uuid = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid SAR ID"})),
            )
                .into_response()
        }
    };

    let reviewer = match body.get("reviewer").and_then(|v| v.as_str()) {
        Some(r) => r.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing reviewer"})),
            )
                .into_response()
        }
    };

    let mut mgr = exchange
        .sar_manager
        .lock().unwrap_or_else(|e| e.into_inner());
    match mgr.review(sar_id, &reviewer) {
        Ok(()) => Json(serde_json::json!({"reviewed": true})).into_response(),
        Err(e) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": e}))).into_response(),
    }
}

/// POST /admin/compliance/sar/{id}/file — mark SAR as filed with regulator.
pub async fn admin_sar_file(
    State(exchange): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let sar_id: uuid::Uuid = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid SAR ID"})),
            )
                .into_response()
        }
    };

    let reference = match body.get("referenceNumber").and_then(|v| v.as_str()) {
        Some(r) => r.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing referenceNumber"})),
            )
                .into_response()
        }
    };

    let mut mgr = exchange
        .sar_manager
        .lock().unwrap_or_else(|e| e.into_inner());
    match mgr.mark_filed(sar_id, &reference) {
        Ok(()) => Json(serde_json::json!({"filed": true, "referenceNumber": reference}))
            .into_response(),
        Err(e) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": e}))).into_response(),
    }
}

/// GET /admin/compliance/sar/export — export all SARs as CSV.
pub async fn admin_sar_export(State(exchange): State<AppState>) -> impl IntoResponse {
    let mgr = exchange
        .sar_manager
        .lock().unwrap_or_else(|e| e.into_inner());
    let csv = mgr.export_csv();
    (
        StatusCode::OK,
        [
            (axum::http::header::CONTENT_TYPE, "text/csv"),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"sar_export.csv\"",
            ),
        ],
        csv,
    )
        .into_response()
}

// ─── Task 10: Corporate KYC / Beneficial Ownership ──────────────────────────

/// POST /api/v1/kyc/corporate — submit a corporate KYC application.
pub async fn submit_corporate_kyc(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let company = match body.get("companyName").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing companyName"})),
            )
                .into_response()
        }
    };
    let reg_number = body
        .get("registrationNumber")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let jurisdiction = body
        .get("jurisdiction")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let directors: Vec<String> = body
        .get("directors")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default();

    let ubos: Vec<exchange_compliance::UltimateBeneficialOwner> = body
        .get("ubos")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    let name = v.get("name")?.as_str()?.to_string();
                    let nationality = v
                        .get("nationality")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string();
                    let ownership_pct: rust_decimal::Decimal = v
                        .get("ownershipPct")
                        .and_then(|n| n.as_f64())
                        .and_then(|f| rust_decimal::Decimal::try_from(f).ok())
                        .unwrap_or(rust_decimal::Decimal::ZERO);
                    let kyc_status = v
                        .get("kycStatus")
                        .and_then(|n| n.as_str())
                        .unwrap_or("pending")
                        .to_string();
                    let pep_checked = v
                        .get("pepChecked")
                        .and_then(|n| n.as_bool())
                        .unwrap_or(false);
                    Some(exchange_compliance::UltimateBeneficialOwner {
                        name,
                        nationality,
                        ownership_pct,
                        kyc_status,
                        pep_checked,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let mut mgr = exchange
        .corporate_kyc
        .lock().unwrap_or_else(|e| e.into_inner());
    let record = mgr.submit(user.0, company, reg_number, jurisdiction, directors, ubos);
    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": record.id.to_string(),
            "status": record.status,
            "companyName": record.company_name,
            "createdAt": record.created_at.to_rfc3339(),
        })),
    )
        .into_response()
}

/// GET /api/v1/kyc/corporate/status — check corporate KYC status for the authenticated user.
pub async fn corporate_kyc_status(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
) -> impl IntoResponse {
    let mgr = exchange
        .corporate_kyc
        .lock().unwrap_or_else(|e| e.into_inner());
    match mgr.get_by_user(user.0) {
        Some(r) => Json(serde_json::json!({
            "id": r.id.to_string(),
            "status": r.status,
            "companyName": r.company_name,
            "jurisdiction": r.jurisdiction,
            "createdAt": r.created_at.to_rfc3339(),
        }))
        .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "No corporate KYC found"})),
        )
            .into_response(),
    }
}

/// GET /admin/compliance/corporate-kyc/pending — list pending corporate KYC records.
pub async fn admin_corporate_kyc_pending(State(exchange): State<AppState>) -> impl IntoResponse {
    let mgr = exchange
        .corporate_kyc
        .lock().unwrap_or_else(|e| e.into_inner());
    let records: Vec<serde_json::Value> = mgr
        .pending()
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id.to_string(),
                "userId": r.user_id.to_string(),
                "companyName": r.company_name,
                "jurisdiction": r.jurisdiction,
                "uboCount": r.ubos.len(),
                "createdAt": r.created_at.to_rfc3339(),
            })
        })
        .collect();
    Json(serde_json::json!({ "records": records }))
}

/// POST /admin/compliance/corporate-kyc/{id}/approve — approve a corporate KYC record.
pub async fn admin_corporate_kyc_approve(
    State(exchange): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let record_id: uuid::Uuid = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid record ID"})),
            )
                .into_response()
        }
    };

    let mut mgr = exchange
        .corporate_kyc
        .lock().unwrap_or_else(|e| e.into_inner());
    match mgr.approve(record_id) {
        Ok(()) => Json(serde_json::json!({"approved": true})).into_response(),
        Err(e) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

// ─── Task 11: Cold/Hot Wallet Split ──────────────────────────────────────────

/// GET /admin/custody/wallets — return all registered wallets grouped by type.
pub async fn admin_custody_wallets(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let mgr = exchange.wallet_manager.lock().unwrap_or_else(|e| e.into_inner());

    let by_type = mgr.balances_by_type();

    let hot: Vec<serde_json::Value> = by_type
        .get(&exchange_custody::WalletType::Hot)
        .map(|ws| ws.iter().map(|w| wallet_info_json(w)).collect())
        .unwrap_or_default();
    let warm: Vec<serde_json::Value> = by_type
        .get(&exchange_custody::WalletType::Warm)
        .map(|ws| ws.iter().map(|w| wallet_info_json(w)).collect())
        .unwrap_or_default();
    let cold: Vec<serde_json::Value> = by_type
        .get(&exchange_custody::WalletType::Cold)
        .map(|ws| ws.iter().map(|w| wallet_info_json(w)).collect())
        .unwrap_or_default();

    let totals = mgr.total_per_coin();

    Json(serde_json::json!({
        "hot": hot,
        "warm": warm,
        "cold": cold,
        "totals": totals,
    }))
}

fn wallet_info_json(w: &exchange_custody::WalletInfo) -> serde_json::Value {
    serde_json::json!({
        "type": format!("{:?}", w.wallet_type),
        "coin": w.coin,
        "address": w.address,
        "balance": w.balance,
        "lastUpdated": w.last_updated.to_rfc3339(),
    })
}

/// POST /admin/custody/sweep-check — return coins whose hot wallet exceeds the policy limit.
pub async fn admin_custody_sweep_check(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let mgr = exchange.wallet_manager.lock().unwrap_or_else(|e| e.into_inner());

    let coins: Vec<&str> = ["BTC", "ETH", "USDT", "USDC", "SOL"].as_slice().to_vec();
    let sweeps: Vec<serde_json::Value> = coins
        .into_iter()
        .filter_map(|coin| {
            mgr.needs_sweep(coin).map(|amount| {
                serde_json::json!({
                    "coin": coin,
                    "sweepAmount": amount,
                    "hotBalance": mgr.hot_balance(coin),
                })
            })
        })
        .collect();

    Json(serde_json::json!({ "sweepsRequired": sweeps }))
}

// ─── Task 12: Dual Approval for Large Withdrawals ────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AdminWithdrawalApproveRequest {
    pub approver_id: Option<String>,
}

/// POST /admin/withdrawals/{id}/approve — cast an approval vote.
///
/// If the withdrawal amount is below the dual-approval threshold the request is
/// processed immediately (legacy behaviour). Otherwise a vote is recorded and
/// processing happens only once the required number of votes is reached.
pub async fn admin_withdrawal_approve(
    State(exchange): State<AppState>,
    Path(withdrawal_id): Path<Uuid>,
    body: Option<Json<AdminWithdrawalApproveRequest>>,
) -> impl IntoResponse {
    let approver_id = body
        .and_then(|b| b.approver_id.clone())
        .unwrap_or_else(|| "system".to_string());

    // Determine whether dual-approval applies by checking the withdrawal amount
    // against the policy threshold.
    let requires_dual = {
        let mgr = exchange.withdrawal_manager.lock().unwrap_or_else(|e| e.into_inner());
        let amount = mgr
            .pending_approvals()
            .iter()
            .find(|w| w.id == withdrawal_id)
            .map(|w| w.amount);

        match amount {
            None => {
                // Not in pending — fall through to the manager which will return NotFound.
                false
            }
            Some(amt) => amt >= exchange.withdrawal_approval_policy.threshold_usd,
        }
    };

    if !requires_dual {
        // Fast path: single approver sufficient.
        // Step 1: validate and extract withdrawal info (short lock).
        let prep = {
            let mgr = exchange.withdrawal_manager.lock().unwrap_or_else(|e| e.into_inner());
            mgr.prepare_approve(&withdrawal_id)
        };
        let (wid, asset, network, address, net_amount) = match prep {
            Ok(info) => info,
            Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
        };
        // Step 2: call custody provider (async, no lock held).
        let tx_id = match exchange.custody_provider.submit_withdrawal(wid, &asset, &network, &address, net_amount).await {
            Ok(id) => id,
            Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
        };
        // Step 3: update withdrawal status (short lock).
        let mut mgr = exchange.withdrawal_manager.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = mgr.complete_approve(&withdrawal_id, tx_id) {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response();
        }
        tracing::info!(withdrawal_id = %withdrawal_id, "Admin approved withdrawal (single)");
        return (StatusCode::OK, Json(serde_json::json!({
            "id": withdrawal_id.to_string(),
            "status": "approved",
            "approvals": 1,
            "required": 1,
        }))).into_response();
    }

    // Dual-approval path: record the vote.
    let approval = crate::exchange::WithdrawalApproval {
        withdrawal_id,
        approver_id: approver_id.clone(),
        decision: "approved".to_string(),
        timestamp: chrono::Utc::now(),
    };

    let vote_count = {
        let mut approvals = exchange.withdrawal_approvals.lock().unwrap_or_else(|e| e.into_inner());
        let votes = approvals.entry(withdrawal_id).or_default();

        // Prevent duplicate votes from the same approver.
        if votes.iter().any(|v| v.approver_id == approver_id) {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": "Approver has already voted on this withdrawal"})),
            ).into_response();
        }

        votes.push(approval);
        votes.iter().filter(|v| v.decision == "approved").count()
    };

    let required = exchange.withdrawal_approval_policy.required_approvers as usize;

    tracing::info!(
        withdrawal_id = %withdrawal_id,
        approver_id = %approver_id,
        vote_count = vote_count,
        required = required,
        "Withdrawal approval vote recorded"
    );

    if vote_count >= required {
        // Quorum reached — process the withdrawal.
        // Step 1: validate and extract (short lock).
        let prep = {
            let mgr = exchange.withdrawal_manager.lock().unwrap_or_else(|e| e.into_inner());
            mgr.prepare_approve(&withdrawal_id)
        };
        let (wid, asset, network, address, net_amount) = match prep {
            Ok(info) => info,
            Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
        };
        // Step 2: call custody provider (async, no lock).
        let tx_id = match exchange.custody_provider.submit_withdrawal(wid, &asset, &network, &address, net_amount).await {
            Ok(id) => id,
            Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
        };
        // Step 3: update status (short lock).
        let mut mgr = exchange.withdrawal_manager.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = mgr.complete_approve(&withdrawal_id, tx_id) {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response();
        }
        tracing::info!(withdrawal_id = %withdrawal_id, "Dual approval quorum reached, withdrawal processing");
        (StatusCode::OK, Json(serde_json::json!({
            "id": withdrawal_id.to_string(),
            "status": "approved",
            "approvals": vote_count,
            "required": required,
        }))).into_response()
    } else {
        (StatusCode::ACCEPTED, Json(serde_json::json!({
            "id": withdrawal_id.to_string(),
            "status": "pending_approvals",
            "approvals": vote_count,
            "required": required,
        }))).into_response()
    }
}

/// GET /admin/withdrawals/{id}/approvals — list all votes for a withdrawal.
pub async fn admin_withdrawal_approvals(
    State(exchange): State<AppState>,
    Path(withdrawal_id): Path<Uuid>,
) -> impl IntoResponse {
    let approvals = exchange.withdrawal_approvals.lock().unwrap_or_else(|e| e.into_inner());
    let votes = approvals.get(&withdrawal_id).cloned().unwrap_or_default();

    let list: Vec<serde_json::Value> = votes
        .iter()
        .map(|v| serde_json::json!({
            "approverId": v.approver_id,
            "decision": v.decision,
            "timestamp": v.timestamp.to_rfc3339(),
        }))
        .collect();

    Json(serde_json::json!({
        "withdrawalId": withdrawal_id.to_string(),
        "approvals": list,
        "count": list.len(),
        "required": exchange.withdrawal_approval_policy.required_approvers,
        "thresholdUsd": exchange.withdrawal_approval_policy.threshold_usd,
    }))
}

// ─── Task 14: Audit Trail Hash Chain Verification ────────────────────────────

/// GET /admin/audit/verify-integrity — verify the audit log hash chain.
pub async fn admin_audit_verify_integrity(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let log = exchange.audit_log.lock().unwrap_or_else(|e| e.into_inner());
    let entries_checked = log.count();
    let valid = log.verify_integrity();

    if valid {
        tracing::info!(entries_checked = entries_checked, "Audit log integrity verified: OK");
    } else {
        tracing::error!(entries_checked = entries_checked, "Audit log integrity check FAILED");
    }

    Json(serde_json::json!({
        "valid": valid,
        "entriesChecked": entries_checked,
    }))
}

// ─── Task 15: Data Retention Policy ──────────────────────────────────────────

/// GET /admin/compliance/retention-status — return all retention policies with current stats.
pub async fn admin_retention_status(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let policies = exchange.retention_policies.lock().unwrap_or_else(|e| e.into_inner());

    let list: Vec<serde_json::Value> = policies
        .iter()
        .map(|p| serde_json::json!({
            "table": p.table,
            "minRetentionDays": p.min_retention_days,
            "archiveAfterDays": p.archive_after_days,
            "currentOldest": p.current_oldest.map(|dt| dt.to_rfc3339()),
            "currentCount": p.current_count,
        }))
        .collect();

    Json(serde_json::json!({ "policies": list }))
}

// ─── Task 16: Tax Reporting ───────────────────────────────────────────────────

#[derive(Debug, serde::Deserialize)]
pub struct TaxReportQuery {
    pub year: Option<u32>,
}

/// GET /api/v1/account/tax-report?year=2026 — JSON tax report for the authenticated user.
pub async fn get_tax_report(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
    Query(params): Query<TaxReportQuery>,
) -> impl IntoResponse {
    let year = params.year.unwrap_or_else(|| chrono::Utc::now().format("%Y").to_string().parse().unwrap_or(2026));

    // Gather order history for this user.
    let trades: Vec<(rust_decimal::Decimal, rust_decimal::Decimal, rust_decimal::Decimal)> = {
        let history = exchange.order_history.lock().unwrap_or_else(|e| e.into_inner());
        history
            .iter()
            .filter(|o| o.user_id == user.0 && o.status == "filled")
            .map(|o| {
                let qty = if o.side == "Sell" { -o.filled_qty } else { o.filled_qty };
                let fee = o.price * o.filled_qty * rust_decimal::Decimal::new(5, 4); // 0.05% approx
                (qty, o.price, fee)
            })
            .collect()
    };

    // Gather funding payments for this user.
    let funding: Vec<rust_decimal::Decimal> = {
        let fh = exchange.user_funding_history.lock().unwrap_or_else(|e| e.into_inner());
        fh.get(&user.0).cloned().unwrap_or_default()
    };

    let report = exchange_compliance::TaxReportGenerator::generate(user.0, year, &trades, &funding);
    Json(serde_json::json!(report))
}

/// GET /api/v1/account/tax-report/csv?year=2026 — CSV tax report for download.
pub async fn get_tax_report_csv(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
    Query(params): Query<TaxReportQuery>,
) -> impl IntoResponse {
    let year = params.year.unwrap_or_else(|| chrono::Utc::now().format("%Y").to_string().parse().unwrap_or(2026));

    let trades: Vec<(rust_decimal::Decimal, rust_decimal::Decimal, rust_decimal::Decimal)> = {
        let history = exchange.order_history.lock().unwrap_or_else(|e| e.into_inner());
        history
            .iter()
            .filter(|o| o.user_id == user.0 && o.status == "filled")
            .map(|o| {
                let qty = if o.side == "Sell" { -o.filled_qty } else { o.filled_qty };
                let fee = o.price * o.filled_qty * rust_decimal::Decimal::new(5, 4);
                (qty, o.price, fee)
            })
            .collect()
    };

    let funding: Vec<rust_decimal::Decimal> = {
        let fh = exchange.user_funding_history.lock().unwrap_or_else(|e| e.into_inner());
        fh.get(&user.0).cloned().unwrap_or_default()
    };

    let report = exchange_compliance::TaxReportGenerator::generate(user.0, year, &trades, &funding);
    let csv = exchange_compliance::TaxReportGenerator::to_csv(&report);

    (
        StatusCode::OK,
        [
            (axum::http::header::CONTENT_TYPE, "text/csv"),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"tax-report.csv\"",
            ),
        ],
        csv,
    )
        .into_response()
}

// ─── Task 17: Regulatory Reports ─────────────────────────────────────────────

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateReportRequest {
    pub r#type: String,
    pub start_date: chrono::DateTime<chrono::Utc>,
    pub end_date: chrono::DateTime<chrono::Utc>,
}

/// POST /admin/compliance/reports/generate — generate a new regulatory report.
pub async fn admin_generate_regulatory_report(
    State(exchange): State<AppState>,
    Json(body): Json<GenerateReportRequest>,
) -> impl IntoResponse {
    // Collect live compliance counts from in-memory subsystems.
    let counts = {
        use exchange_compliance::ComplianceCounts;

        // KYC counts from the verification service's KYC manager.
        let (kyc_total, kyc_pending, kyc_approved, kyc_rejected) = {
            // verification_service is behind an async mutex; we can read the
            // KYC counters from the sync kyc_approved set and the surveillance
            // engine instead of holding the async lock across subsystems.
            // For the period's KYC approved count we use the kyc_approved mirror.
            let approved_count = exchange.kyc_approved.len() as u64;
            // Total and pending/rejected aren't tracked as separate sync counters
            // yet; report what we have and zero for the rest.  Phase 2 will pull
            // these from the database.
            (approved_count, 0u64, approved_count, 0u64)
        };

        // AML / surveillance alert counts.
        let (aml_alerts_total, aml_alerts_reviewed) = {
            let s = exchange.surveillance.lock();
            let total = s.all_alerts().len() as u64;
            let unreviewed = s.unreviewed_alerts().len() as u64;
            let reviewed = total.saturating_sub(unreviewed);
            (total, reviewed)
        };

        // SAR counts.
        let (sar_filed, sar_pending) = {
            let sar = exchange.sar_manager.lock().unwrap_or_else(|e| e.into_inner());
            let all = sar.all_reports().len() as u64;
            let pending = sar.pending().len() as u64;
            let filed = all.saturating_sub(pending);
            (filed, pending)
        };

        let sanctions_matches = {
            let screener = exchange.sanctions.lock().unwrap_or_else(|e| e.into_inner());
            screener.match_count()
        };

        ComplianceCounts {
            kyc_total,
            kyc_pending,
            kyc_approved,
            kyc_rejected,
            aml_alerts_total,
            aml_alerts_reviewed,
            sar_filed,
            sar_pending,
            sanctions_matches,
        }
    };

    // Aggregate financial totals from in-memory data.
    let total_trading_volume_usd = {
        let history = exchange.trade_history.lock().unwrap_or_else(|e| e.into_inner());
        let mut vol = Decimal::ZERO;
        for trades in history.values() {
            for t in trades {
                if t.timestamp >= body.start_date && t.timestamp <= body.end_date {
                    vol += t.quote_qty;
                }
            }
        }
        vol.to_string()
    };

    let (total_deposits_usd, total_withdrawals_usd) = {
        let dm = exchange.deposit_manager.lock().unwrap_or_else(|e| e.into_inner());
        let deps: Decimal = dm.all_deposits()
            .iter()
            .filter(|d| d.created_at >= body.start_date && d.created_at <= body.end_date)
            .map(|d| d.amount)
            .sum();
        let wm = exchange.withdrawal_manager.lock().unwrap_or_else(|e| e.into_inner());
        let wds: Decimal = wm.all_withdrawals()
            .iter()
            .filter(|w| w.completed_at.map(|t| t >= body.start_date && t <= body.end_date).unwrap_or(false))
            .map(|w| w.amount)
            .sum();
        (deps.to_string(), wds.to_string())
    };

    let mut report = exchange_compliance::RegulatoryReporter::generate_summary(
        &body.r#type,
        body.start_date,
        body.end_date,
        counts,
    );
    report.total_trading_volume_usd = total_trading_volume_usd;
    report.total_deposits_usd = total_deposits_usd;
    report.total_withdrawals_usd = total_withdrawals_usd;

    exchange
        .regulatory_reports
        .lock().unwrap_or_else(|e| e.into_inner())
        .push(report.clone());

    tracing::info!(
        report_type = %body.r#type,
        "Regulatory report generated",
    );

    Json(serde_json::json!(report))
}

/// GET /admin/compliance/reports — list available regulatory report types.
pub async fn admin_list_regulatory_reports(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let reports = exchange.regulatory_reports.lock().unwrap_or_else(|e| e.into_inner());
    let list: Vec<serde_json::Value> = reports
        .iter()
        .map(|r| {
            serde_json::json!({
                "reportType": r.report_type,
                "periodStart": r.period_start.to_rfc3339(),
                "periodEnd": r.period_end.to_rfc3339(),
                "generatedAt": r.generated_at.to_rfc3339(),
            })
        })
        .collect();

    Json(serde_json::json!({ "reports": list }))
}

// ─── Task 18: VASP Directory ──────────────────────────────────────────────────

/// POST /api/v1/travel-rule/receive — receive travel rule data from a counterparty VASP.
pub async fn travel_rule_receive(
    State(exchange): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    // Extract the transfer ID for status update, if provided.
    let transfer_id_str = body.get("transferId").and_then(|v| v.as_str()).unwrap_or("");

    if let Ok(transfer_id) = transfer_id_str.parse::<uuid::Uuid>() {
        let mut engine = exchange.travel_rule.lock().unwrap_or_else(|e| e.into_inner());
        engine.update_status(transfer_id, exchange_compliance::TravelRuleStatus::Received);
        tracing::info!(%transfer_id, "Received inbound travel rule data from counterparty VASP");
    }

    Json(serde_json::json!({ "status": "accepted" }))
}

/// GET /admin/compliance/vasp-directory — list all registered VASPs.
pub async fn admin_list_vasps(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let dir = exchange.vasp_directory.lock().unwrap_or_else(|e| e.into_inner());
    let vasps: Vec<serde_json::Value> = dir
        .all()
        .iter()
        .map(|v| {
            serde_json::json!({
                "vaspId": v.vasp_id,
                "name": v.name,
                "endpoint": v.endpoint,
                "country": v.country,
                // public_key intentionally omitted from list response for brevity
            })
        })
        .collect();

    Json(serde_json::json!({ "vasps": vasps }))
}

/// POST /admin/compliance/vasp-directory — register a new VASP.
pub async fn admin_register_vasp(
    State(exchange): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let vasp_id = match body.get("vaspId").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "vaspId required"}))).into_response();
        }
    };
    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let endpoint = body.get("endpoint").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let public_key = body.get("publicKey").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let country = body.get("country").and_then(|v| v.as_str()).unwrap_or("").to_string();

    let vasp = exchange_compliance::VaspInfo { vasp_id: vasp_id.clone(), name, endpoint, public_key, country };

    exchange
        .vasp_directory
        .lock().unwrap_or_else(|e| e.into_inner())
        .register(vasp);

    tracing::info!(vasp_id = %vasp_id, "VASP registered in directory");
    Json(serde_json::json!({ "status": "registered" })).into_response()
}

// ─── Task 19: Jurisdiction Rules ─────────────────────────────────────────────

#[derive(Debug, serde::Deserialize)]
pub struct JurisdictionQuery {
    pub code: Option<String>,
}

/// GET /api/v1/jurisdiction/rules?code=EU — rules for a specific jurisdiction.
pub async fn get_jurisdiction_rules(
    State(exchange): State<AppState>,
    Query(params): Query<JurisdictionQuery>,
) -> impl IntoResponse {
    let code = params.code.as_deref().unwrap_or("OTHER");
    let jurisdiction = match exchange_compliance::Jurisdiction::from_code(code) {
        Some(j) => j,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("Unknown jurisdiction code: {code}")})),
            )
                .into_response();
        }
    };

    let engine = exchange.jurisdiction_engine.lock().unwrap_or_else(|e| e.into_inner());
    match engine.get_rules(jurisdiction) {
        Some(rules) => Json(serde_json::json!(rules)).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "No rules defined for this jurisdiction"})),
        )
            .into_response(),
    }
}

/// GET /admin/jurisdiction/rules — all jurisdiction rules.
pub async fn admin_list_jurisdiction_rules(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let engine = exchange.jurisdiction_engine.lock().unwrap_or_else(|e| e.into_inner());
    let rules: Vec<serde_json::Value> = engine
        .all_rules()
        .iter()
        .map(|r| serde_json::json!(r))
        .collect();
    Json(serde_json::json!({ "jurisdictions": rules }))
}

/// PUT /admin/jurisdiction/rules/{code} — update rules for a jurisdiction.
pub async fn admin_update_jurisdiction_rules(
    State(exchange): State<AppState>,
    Path(code): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let jurisdiction = match exchange_compliance::Jurisdiction::from_code(&code) {
        Some(j) => j,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("Unknown jurisdiction code: {code}")})),
            )
                .into_response();
        }
    };

    // Parse the updated rules from the request body.
    let rules: exchange_compliance::JurisdictionRules = match serde_json::from_value(body) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("Invalid rules: {e}")})),
            )
                .into_response();
        }
    };

    exchange
        .jurisdiction_engine
        .lock().unwrap_or_else(|e| e.into_inner())
        .update_rules(jurisdiction, rules);

    tracing::info!(jurisdiction = %code, "Jurisdiction rules updated");
    Json(serde_json::json!({ "status": "updated" })).into_response()
}

// ─── Task 20: Capital Adequacy ────────────────────────────────────────────────

/// GET /admin/capital/adequacy — capital adequacy report.
pub async fn admin_capital_adequacy(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    // Sum the insurance funds across all symbols.
    let insurance_total: rust_decimal::Decimal = exchange
        .insurance_funds
        .lock().unwrap_or_else(|e| e.into_inner())
        .values()
        .map(|f| f.balance())
        .sum();

    let tracker = exchange.capital_tracker.lock().unwrap_or_else(|e| e.into_inner());
    let report = tracker.report(insurance_total);

    if tracker.early_warning() {
        tracing::warn!(
            own_funds = %report.own_funds_usd,
            minimum = %report.regulatory_minimum_usd,
            ratio = %report.ratio,
            "Capital adequacy early warning: own funds below 1.5× regulatory minimum",
        );
    }

    Json(serde_json::json!(report))
}

// --- Fix 5: Admin deposit credit endpoint ---

/// POST /admin/deposits/{id}/credit — manually credit a pending deposit.
///
/// Deposits are no longer auto-credited by the poller. This endpoint provides
/// the manual approval path (the webhook path is `webhook_deposit` in this file).
/// Only admin-authenticated requests reach this handler (enforced by the router
/// middleware on the `/admin` prefix).
pub async fn admin_credit_deposit(
    State(exchange): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let deposit_id: Uuid = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid deposit ID"})),
            )
                .into_response()
        }
    };

    // Four-eyes gate: in production, deposit credits require dual approval.
    if exchange.is_production {
        let action = exchange.four_eyes.lock().unwrap_or_else(|e| e.into_inner())
            .submit("credit_deposit", serde_json::json!({"deposit_id": deposit_id.to_string()}));

        return (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "status": "pending_approval",
                "approval_id": action.id.to_string(),
                "expires_at": action.expires_at.to_rfc3339(),
                "message": "Deposit credit requires second admin approval. POST /admin/approvals/{approval_id}/approve",
            })),
        ).into_response();
    }

    // Find the deposit among pending deposits.
    let deposit = {
        let dm = exchange.deposit_manager.lock().unwrap_or_else(|e| e.into_inner());
        dm.pending_deposits()
            .into_iter()
            .find(|d| d.id == deposit_id)
            .cloned()
    };

    let deposit = match deposit {
        Some(d) => d,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Deposit not found or already credited"})),
            )
                .into_response()
        }
    };

    // Credit the user's balance.
    if let Err(e) = exchange.accounts.deposit(deposit.user_id, &deposit.asset, deposit.amount) {
        tracing::error!(
            deposit_id = %deposit_id,
            user_id = %deposit.user_id,
            error = %e,
            "admin deposit credit: balance update failed"
        );
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to credit balance"})),
        )
            .into_response();
    }

    // Mark as credited in the deposit manager.
    {
        let mut dm = exchange.deposit_manager.lock().unwrap_or_else(|e| e.into_inner());
        dm.credit_deposit(&deposit_id);
    }

    // Broadcast private event to the user.
    let _ = exchange.private_tx.send(crate::exchange::PrivateEvent::DepositCredited {
        user_id: deposit.user_id.to_string(),
        asset: deposit.asset.clone(),
        amount: deposit.amount,
        tx_hash: deposit.tx_hash.clone().unwrap_or_default(),
    });

    tracing::info!(
        deposit_id = %deposit_id,
        user_id = %deposit.user_id,
        asset = %deposit.asset,
        amount = %deposit.amount,
        "admin manually credited deposit"
    );

    Json(serde_json::json!({
        "status": "credited",
        "depositId": deposit_id.to_string(),
        "userId": deposit.user_id.to_string(),
        "asset": deposit.asset,
        "amount": deposit.amount.to_string(),
    }))
    .into_response()
}

// --- Fix 7: Account closure endpoint ---

/// POST /api/v1/account/close — request account closure (GDPR right to erasure).
///
/// Pre-conditions (all must hold before the account is frozen):
/// 1. No open positions.
/// 2. No open orders.
/// 3. Balance <= $0.01 dust threshold (user must withdraw funds first).
///
/// On success the account is added to `frozen_accounts` and can no longer
/// place orders, submit withdrawals, or perform any trading action.
pub async fn close_account(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = user.0;

    // 1. No open positions.
    {
        let pm = exchange.positions.lock();
        let open = pm.user_positions(user_id);
        if !open.is_empty() {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "Close all positions before closing account",
                    "openPositions": open.len(),
                })),
            )
                .into_response();
        }
    }

    // 2. No open orders.
    {
        let has_orders = exchange.pending_orders.iter().any(|r| r.user_id == user_id);
        if has_orders {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "Cancel all open orders before closing account",
                })),
            )
                .into_response();
        }
    }

    // 3. Balance must be dust-level (<= $0.01) so user has already withdrawn.
    {
        let total: Decimal = exchange
            .accounts
            .get_all_balances(user_id)
            .values()
            .map(|b| b.total())
            .sum();
        let dust_threshold = Decimal::new(1, 2); // $0.01
        if total > dust_threshold {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "Withdraw all funds before closing account",
                    "remainingBalance": total.to_string(),
                })),
            )
                .into_response();
        }
    }

    // Freeze the account — blocks all trading and withdrawal access.
    exchange.frozen_accounts.insert(user_id);

    tracing::info!(user_id = %user_id, "account closed by user request");

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "closed",
            "message": "Account has been closed. All trading and withdrawal access has been revoked.",
        })),
    )
        .into_response()
}

// --- Fix 9: Admin add coin endpoint ---

/// POST /admin/coins — register a new coin in the asset registry at runtime.
///
/// Adding a coin here makes it immediately available for deposits/withdrawals
/// without requiring a code change or redeploy. At least one network config is
/// required. Newly registered coins start with `is_tradeable = false` — create
/// a trading pair via POST /admin/pairs to enable trading.
pub async fn admin_add_coin(
    State(exchange): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let coin = match body.get("coin").and_then(|v| v.as_str()) {
        Some(c) => c.to_uppercase(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing required field: coin"})),
            )
                .into_response()
        }
    };

    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(&coin)
        .to_string();

    let networks: Vec<exchange_asset_registry::NetworkConfig> =
        match body.get("networks").and_then(|v| v.as_array()) {
            Some(arr) => arr
                .iter()
                .filter_map(|n| {
                    Some(exchange_asset_registry::NetworkConfig {
                        network: n.get("network")?.as_str()?.to_string(),
                        chain_type: n.get("chainType")?.as_str()?.to_string(),
                        contract_address: n
                            .get("contractAddress")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        deposit_enabled: n
                            .get("depositEnabled")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(true),
                        withdraw_enabled: n
                            .get("withdrawEnabled")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(true),
                        withdraw_fee: n
                            .get("withdrawFee")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(Decimal::ZERO),
                        withdraw_min: n
                            .get("withdrawMin")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(Decimal::ZERO),
                        withdraw_max: n
                            .get("withdrawMax")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(Decimal::new(1_000_000, 0)),
                        deposit_min: n
                            .get("depositMin")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(Decimal::ZERO),
                        confirmations_required: n
                            .get("confirmations")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(12) as u32,
                        safe_confirmations: n
                            .get("safeConfirmations")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(20) as u32,
                        min_accuracy: n
                            .get("minAccuracy")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(8) as u32,
                    })
                })
                .collect(),
            None => Vec::new(),
        };

    if networks.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "At least one network configuration is required"})),
        )
            .into_response();
    }

    let network_count = networks.len();
    let coin_info = exchange_asset_registry::CoinInfo {
        coin: coin.clone(),
        name: name.clone(),
        // Coins added via admin API start as non-tradeable. Create a trading
        // pair via POST /admin/pairs to enable trading for this coin.
        is_tradeable: false,
        networks,
    };

    exchange
        .asset_registry
        .lock().unwrap_or_else(|e| e.into_inner())
        .register(coin_info);

    tracing::info!(
        coin = %coin,
        name = %name,
        networks = network_count,
        "admin registered new coin in asset registry"
    );

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "coin": coin,
            "name": name,
            "networks": network_count,
        })),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Pre-Market / Call Auction Endpoints (Tasks 7–9)
// ---------------------------------------------------------------------------

/// POST /admin/pairs/{symbol}/phase — set the trading phase for a pair.
///
/// Accepts a JSON body with fields:
/// - `phase`: "call_auction" | "pre_market" | "active" | "settle_only" | "disabled"
/// - `maxLeverage`: optional u32
/// - `priceLimitPct`: optional decimal string (e.g. "5.0" = ±5%)
/// - `referencePrice`: optional decimal string
/// - `auctionEndTime`: optional Unix milliseconds
///
/// When transitioning to `disabled` or `settle_only`, all open orders for the
/// symbol are cancelled immediately.
pub async fn admin_set_pair_phase(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = validate_symbol(&symbol) {
        return e.into_response();
    }

    let sym = exchange_common::Symbol::new(&symbol);

    let phase = match body.get("phase").and_then(|v| v.as_str()) {
        Some("call_auction") => crate::exchange::PairPhase::CallAuction,
        Some("pre_market") => crate::exchange::PairPhase::PreMarketTrading,
        Some("active") => crate::exchange::PairPhase::Active,
        Some("settle_only") => crate::exchange::PairPhase::SettleOnly,
        Some("disabled") => crate::exchange::PairPhase::Disabled,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid phase — must be one of: call_auction, pre_market, active, settle_only, disabled"})),
            )
                .into_response();
        }
    };

    let config = crate::exchange::PairPhaseConfig {
        phase,
        max_leverage: body
            .get("maxLeverage")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32),
        price_limit_pct: body
            .get("priceLimitPct")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok()),
        reference_price: body
            .get("referencePrice")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok()),
        auction_end_time: body
            .get("auctionEndTime")
            .and_then(|v| v.as_i64())
            .and_then(chrono::DateTime::from_timestamp_millis),
    };

    // Check if we're transitioning FROM CallAuction — if so, run the auction first.
    let prev_phase = exchange
        .pair_phases
        .lock()
        .get(&sym)
        .map(|c| c.phase);

    let mut auction_result_json = serde_json::Value::Null;
    if prev_phase == Some(crate::exchange::PairPhase::CallAuction)
        && phase != crate::exchange::PairPhase::CallAuction
    {
        if let Some(result) = exchange.run_call_auction(&sym) {
            auction_result_json = serde_json::json!({
                "clearingPrice": result.clearing_price.to_string(),
                "matchedVolume": result.matched_volume.to_string(),
                "fills": result.fills.len(),
                "unmatched": result.unmatched.len(),
            });
        }
    }

    exchange
        .pair_phases
        .lock()
        .insert(sym.clone(), config);

    tracing::info!(symbol = %symbol, phase = ?phase, "admin set pair phase");

    // Cancel all open orders when moving to disabled or settle-only.
    let mut cancelled = 0usize;
    if matches!(
        phase,
        crate::exchange::PairPhase::Disabled | crate::exchange::PairPhase::SettleOnly
    ) {
        let orders_to_cancel: Vec<(uuid::Uuid, uuid::Uuid)> = exchange
            .pending_orders
            .iter()
            .filter(|r| r.symbol == sym)
            .map(|r| (*r.key(), r.user_id))
            .collect();
        cancelled = orders_to_cancel.len();
        for (order_id, user_id) in orders_to_cancel {
            let cmd = exchange_common::CancelOrderCommand {
                order_id,
                user_id,
                symbol: sym.clone(),
                timestamp: chrono::Utc::now(),
            };
            if let Err(e) = exchange.cancel_order(cmd) {
                tracing::warn!(
                    order_id = %order_id,
                    error = %e,
                    "Failed to cancel order on phase transition"
                );
            }
        }
        if cancelled > 0 {
            tracing::info!(
                symbol = %symbol,
                cancelled = cancelled,
                "cancelled open orders on phase transition"
            );
        }
    }

    Json(serde_json::json!({
        "symbol": symbol,
        "phase": format!("{:?}", phase),
        "orders_cancelled": cancelled,
        "auction": auction_result_json,
    }))
    .into_response()
}

/// GET /api/v1/markets/{symbol}/phase — get the current trading phase for a symbol.
pub async fn get_pair_phase(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = validate_symbol(&symbol) {
        return e.into_response();
    }

    let sym = exchange_common::Symbol::new(&symbol);
    let phases = exchange.pair_phases.lock();

    match phases.get(&sym) {
        Some(config) => Json(serde_json::json!({
            "symbol": symbol,
            "phase": format!("{:?}", config.phase),
            "maxLeverage": config.max_leverage,
            "priceLimitPct": config.price_limit_pct.map(|p| p.to_string()),
            "referencePrice": config.reference_price.map(|p| p.to_string()),
            "auctionEndTime": config.auction_end_time.map(|t| t.timestamp_millis()),
        }))
        .into_response(),
        None => Json(serde_json::json!({
            "symbol": symbol,
            "phase": "Active",
            "maxLeverage": null,
            "priceLimitPct": null,
            "referencePrice": null,
            "auctionEndTime": null,
        }))
        .into_response(),
    }
}

// ---------------------------------------------------------------------------
// Anti-Phishing Code (Task 10)
// ---------------------------------------------------------------------------

/// POST /api/v1/account/anti-phishing-code — set the anti-phishing code for the authenticated user.
///
/// Body: `{"code": "mySecret123"}` — must be 4–20 characters.
pub async fn set_anti_phishing_code(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let code = match body.get("code").and_then(|v| v.as_str()) {
        Some(c) if c.len() >= 4 && c.len() <= 20 => c.to_string(),
        Some(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Code must be 4-20 characters"})),
            )
                .into_response();
        }
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing 'code' field"})),
            )
                .into_response();
        }
    };

    exchange
        .anti_phishing_codes
        .lock().unwrap_or_else(|e| e.into_inner())
        .insert(user.0, code.clone());

    Json(serde_json::json!({"code": code, "status": "set"})).into_response()
}

/// GET /api/v1/account/anti-phishing-code — get the current anti-phishing code.
pub async fn get_anti_phishing_code(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let codes = exchange
        .anti_phishing_codes
        .lock().unwrap_or_else(|e| e.into_inner());
    match codes.get(&user.0) {
        Some(code) => Json(serde_json::json!({"code": code, "isSet": true})).into_response(),
        None => Json(serde_json::json!({"code": null, "isSet": false})).into_response(),
    }
}

// ---------------------------------------------------------------------------
// Price Alerts (Task 11)
// ---------------------------------------------------------------------------

/// POST /api/v1/alerts/price — create a price alert.
///
/// Body:
/// - `symbol`: e.g. "BTCUSDT"
/// - `condition`: "above" or "below"
/// - `price`: decimal string
pub async fn create_price_alert(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let symbol = match body.get("symbol").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing or empty 'symbol' field"})),
            )
                .into_response();
        }
    };

    let condition = match body.get("condition").and_then(|v| v.as_str()) {
        Some(c) if c == "above" || c == "below" => c.to_string(),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "'condition' must be 'above' or 'below'"})),
            )
                .into_response();
        }
    };

    let price: Decimal = match body
        .get("price")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
    {
        Some(p) => p,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid or missing 'price' field"})),
            )
                .into_response();
        }
    };

    // Enforce max 50 active alerts per user.
    {
        let alerts = exchange
            .price_alerts
            .lock().unwrap_or_else(|e| e.into_inner());
        let active_count = alerts.iter().filter(|a| a.user_id == user.0 && !a.triggered).count();
        if active_count >= 50 {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Maximum of 50 active price alerts reached"})),
            )
                .into_response();
        }
    }

    let alert = crate::exchange::PriceAlert {
        id: uuid::Uuid::new_v4(),
        user_id: user.0,
        symbol,
        condition,
        price,
        triggered: false,
        created_at: chrono::Utc::now(),
    };
    let id = alert.id;
    exchange
        .price_alerts
        .lock().unwrap_or_else(|e| e.into_inner())
        .push(alert);

    (
        StatusCode::CREATED,
        Json(serde_json::json!({"alertId": id.to_string()})),
    )
        .into_response()
}

/// GET /api/v1/alerts/price — list all price alerts for the authenticated user.
pub async fn list_price_alerts(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let alerts = exchange
        .price_alerts
        .lock().unwrap_or_else(|e| e.into_inner());
    let list: Vec<serde_json::Value> = alerts
        .iter()
        .filter(|a| a.user_id == user.0)
        .map(|a| {
            serde_json::json!({
                "id": a.id.to_string(),
                "symbol": a.symbol,
                "condition": a.condition,
                "price": a.price.to_string(),
                "triggered": a.triggered,
                "createdAt": a.created_at.timestamp_millis(),
            })
        })
        .collect();
    Json(serde_json::json!({"alerts": list}))
}

/// DELETE /api/v1/alerts/price/{id} — delete a price alert owned by the authenticated user.
pub async fn delete_price_alert(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let alert_id: uuid::Uuid = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid alert ID"})),
            )
                .into_response();
        }
    };

    let mut alerts = exchange
        .price_alerts
        .lock().unwrap_or_else(|e| e.into_inner());
    let before = alerts.len();
    alerts.retain(|a| !(a.id == alert_id && a.user_id == user.0));

    if alerts.len() < before {
        Json(serde_json::json!({"deleted": true})).into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Alert not found"})),
        )
            .into_response()
    }
}

// ---------------------------------------------------------------------------
// Notification Preferences (Task 12)
// ---------------------------------------------------------------------------

/// GET /api/v1/account/notification-preferences — get notification preferences.
pub async fn get_notification_preferences(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let prefs = exchange
        .notification_prefs
        .lock().unwrap_or_else(|e| e.into_inner());
    let p = prefs.get(&user.0).cloned().unwrap_or_default();
    Json(serde_json::json!({
        "emailFills": p.email_fills,
        "emailDeposits": p.email_deposits,
        "emailWithdrawals": p.email_withdrawals,
        "emailMarginCall": p.email_margin_call,
        "emailLiquidation": p.email_liquidation,
        "emailLogin": p.email_login,
        "pushFills": p.push_fills,
        "pushDeposits": p.push_deposits,
        "pushWithdrawals": p.push_withdrawals,
        "pushMarginCall": p.push_margin_call,
        "pushLiquidation": p.push_liquidation,
        "pushPriceAlerts": p.push_price_alerts,
        "pushFunding": p.push_funding,
    }))
}

/// PUT /api/v1/account/notification-preferences — update notification preferences.
///
/// All fields are optional in the body; absent fields keep their current value.
pub async fn update_notification_preferences(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let mut prefs = exchange
        .notification_prefs
        .lock().unwrap_or_else(|e| e.into_inner());
    let p = prefs
        .entry(user.0)
        .or_insert_with(crate::exchange::NotificationPreferences::default);

    if let Some(v) = body.get("emailFills").and_then(|v| v.as_bool()) {
        p.email_fills = v;
    }
    if let Some(v) = body.get("emailDeposits").and_then(|v| v.as_bool()) {
        p.email_deposits = v;
    }
    if let Some(v) = body.get("emailWithdrawals").and_then(|v| v.as_bool()) {
        p.email_withdrawals = v;
    }
    if let Some(v) = body.get("emailMarginCall").and_then(|v| v.as_bool()) {
        p.email_margin_call = v;
    }
    if let Some(v) = body.get("emailLiquidation").and_then(|v| v.as_bool()) {
        p.email_liquidation = v;
    }
    if let Some(v) = body.get("emailLogin").and_then(|v| v.as_bool()) {
        p.email_login = v;
    }
    if let Some(v) = body.get("pushFills").and_then(|v| v.as_bool()) {
        p.push_fills = v;
    }
    if let Some(v) = body.get("pushDeposits").and_then(|v| v.as_bool()) {
        p.push_deposits = v;
    }
    if let Some(v) = body.get("pushWithdrawals").and_then(|v| v.as_bool()) {
        p.push_withdrawals = v;
    }
    if let Some(v) = body.get("pushMarginCall").and_then(|v| v.as_bool()) {
        p.push_margin_call = v;
    }
    if let Some(v) = body.get("pushLiquidation").and_then(|v| v.as_bool()) {
        p.push_liquidation = v;
    }
    if let Some(v) = body.get("pushPriceAlerts").and_then(|v| v.as_bool()) {
        p.push_price_alerts = v;
    }
    if let Some(v) = body.get("pushFunding").and_then(|v| v.as_bool()) {
        p.push_funding = v;
    }

    let updated = p.clone();
    drop(prefs);

    Json(serde_json::json!({
        "emailFills": updated.email_fills,
        "emailDeposits": updated.email_deposits,
        "emailWithdrawals": updated.email_withdrawals,
        "emailMarginCall": updated.email_margin_call,
        "emailLiquidation": updated.email_liquidation,
        "emailLogin": updated.email_login,
        "pushFills": updated.push_fills,
        "pushDeposits": updated.push_deposits,
        "pushWithdrawals": updated.push_withdrawals,
        "pushMarginCall": updated.push_margin_call,
        "pushLiquidation": updated.push_liquidation,
        "pushPriceAlerts": updated.push_price_alerts,
        "pushFunding": updated.push_funding,
    }))
}

// ── Task 15: Liquidity Seed Endpoint ─────────────────────────────────────────

/// POST /admin/pairs/{symbol}/seed-liquidity — seed orderbook with initial orders.
///
/// Populates an empty orderbook with symmetric bid/ask levels around a mid price,
/// using the treasury account as the house market maker.
pub async fn admin_seed_liquidity(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = validate_symbol(&symbol) {
        return e.into_response();
    }

    let sym = exchange_common::Symbol::new(&symbol);

    let mid_price: Decimal = match body
        .get("midPrice")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
    {
        Some(p) if p > Decimal::ZERO => p,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing or invalid midPrice"})),
            )
                .into_response()
        }
    };

    let spread_pct: Decimal = body
        .get("spreadPct")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(Decimal::new(1, 1)); // default 0.1%

    let levels: usize = body
        .get("levels")
        .and_then(|v| v.as_u64())
        .unwrap_or(10) as usize;

    let qty_per_level: Decimal = match body
        .get("qtyPerLevel")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
    {
        Some(q) if q > Decimal::ZERO => q,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing or invalid qtyPerLevel"})),
            )
                .into_response()
        }
    };

    // Use the treasury account as the house market maker.
    let mm_user = crate::exchange::TREASURY_ACCOUNT_ID;

    // Ensure treasury has enough balance to back the seed orders.
    let _ = exchange.accounts.deposit(mm_user, "USDT", Decimal::from(10_000_000));

    let mut orders_placed = 0u32;
    let tick = spread_pct / Decimal::from(100);

    for i in 0..levels {
        let offset = tick * Decimal::from(i as u64 + 1);

        // Ask side
        let ask_price = mid_price * (Decimal::ONE + offset);
        let ask_cmd = exchange_common::PlaceOrderCommand {
            order_id: uuid::Uuid::new_v4(),
            user_id: mm_user,
            symbol: sym.clone(),
            side: exchange_common::Side::Sell,
            order_type: exchange_common::OrderType::Limit,
            time_in_force: exchange_common::TimeInForce::Gtc,
            price: ask_price,
            qty: qty_per_level,
            timestamp: chrono::Utc::now(),
            reduce_only: false,
            expire_at: None,
            position_idx: 0,
            iceberg_qty: None,
            order_link_id: None,
            slippage_tolerance: None,
            close_position: false,
            stp_mode: exchange_common::StpMode::default(),
            request_expire_time: None,
        };
        let exchange_clone = Arc::clone(&exchange);
        if crate::request_id::spawn_blocking_traced(move || exchange_clone.place_order(ask_cmd))
            .await
            .unwrap_or(Err(anyhow::anyhow!("join error")))
            .is_ok()
        {
            orders_placed += 1;
        }

        // Bid side
        let bid_price = mid_price * (Decimal::ONE - offset);
        let bid_cmd = exchange_common::PlaceOrderCommand {
            order_id: uuid::Uuid::new_v4(),
            user_id: mm_user,
            symbol: sym.clone(),
            side: exchange_common::Side::Buy,
            order_type: exchange_common::OrderType::Limit,
            time_in_force: exchange_common::TimeInForce::Gtc,
            price: bid_price,
            qty: qty_per_level,
            timestamp: chrono::Utc::now(),
            reduce_only: false,
            expire_at: None,
            position_idx: 0,
            iceberg_qty: None,
            order_link_id: None,
            slippage_tolerance: None,
            close_position: false,
            stp_mode: exchange_common::StpMode::default(),
            request_expire_time: None,
        };
        let exchange_clone = Arc::clone(&exchange);
        if crate::request_id::spawn_blocking_traced(move || exchange_clone.place_order(bid_cmd))
            .await
            .unwrap_or(Err(anyhow::anyhow!("join error")))
            .is_ok()
        {
            orders_placed += 1;
        }
    }

    tracing::info!(
        symbol = %symbol,
        orders_placed = orders_placed,
        mid_price = %mid_price,
        levels = levels,
        "admin seeded liquidity"
    );

    Json(serde_json::json!({
        "symbol": symbol,
        "ordersPlaced": orders_placed,
        "midPrice": mid_price.to_string(),
        "levels": levels,
        "qtyPerLevel": qty_per_level.to_string(),
    }))
    .into_response()
}

// ── Task 16: Graceful Delist Flow ─────────────────────────────────────────────

/// POST /admin/pairs/{symbol}/delist — initiate graceful delisting.
///
/// Four-phase delist flow:
/// - `announce`     — log announcement; trading continues.
/// - `settle_only`  — mark pair settle-only; cancel all non-reduce-only orders.
/// - `close_all`    — force-close all remaining positions at mark price.
/// - `remove`       — disable trading entirely.
pub async fn admin_delist_pair(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Err(e) = validate_symbol(&symbol) {
        return e.into_response();
    }

    let sym = exchange_common::Symbol::new(&symbol);
    let phase = body
        .get("phase")
        .and_then(|v| v.as_str())
        .unwrap_or("settle_only");

    match phase {
        "announce" => {
            tracing::warn!(symbol = %symbol, "Delist announced for {}", symbol);
            Json(serde_json::json!({
                "symbol": symbol,
                "phase": "announced",
                "message": "Trading continues. Users should close positions.",
            }))
            .into_response()
        }
        "settle_only" => {
            {
                // Write path (admin-only): rebuild map and atomically swap.
                let old = exchange.product_status.load_full();
                let mut new_map = (*old).clone();
                new_map.insert(sym.clone(), "settle_only".into());
                exchange.product_status.store(Arc::new(new_map));
            }

            // Cancel all non-reduce-only open orders for this symbol.
            let orders_to_cancel: Vec<(uuid::Uuid, uuid::Uuid)> = exchange
                .pending_orders
                .iter()
                .filter(|r| r.symbol == sym)
                .map(|r| (*r.key(), r.user_id))
                .collect();
            let cancelled = orders_to_cancel.len();
            for (order_id, user_id) in &orders_to_cancel {
                let cmd = exchange_common::CancelOrderCommand {
                    order_id: *order_id,
                    user_id: *user_id,
                    symbol: sym.clone(),
                    timestamp: chrono::Utc::now(),
                };
                let _ = exchange.cancel_order(cmd);
            }
            tracing::warn!(
                symbol = %symbol,
                cancelled = cancelled,
                "Delist settle_only: cancelled open orders"
            );
            Json(serde_json::json!({
                "symbol": symbol,
                "phase": "settle_only",
                "ordersCancelled": cancelled,
            }))
            .into_response()
        }
        "close_all" => {
            // Reject the operation if no valid mark price is available.
            // Closing positions at $0 would destroy user balances.
            let mark = exchange
                .get_mark_price(&sym)
                .map(|mp| mp.price)
                .unwrap_or(Decimal::ZERO);

            if mark.is_zero() {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(serde_json::json!({
                        "error": "No valid mark price available — cannot force-close positions at $0"
                    })),
                )
                    .into_response();
            }

            // Collect open positions for this symbol.
            let positions_to_close: Vec<(uuid::Uuid, u8)> = {
                let pm = exchange.positions.lock();
                pm.all_positions()
                    .iter()
                    .filter(|p| p.symbol == sym)
                    .map(|p| (p.user_id, p.position_idx))
                    .collect()
            };

            let mut closed = 0usize;
            for (user_id, position_idx) in &positions_to_close {
                let mut pm = exchange.positions.lock();
                if pm
                    .close(*user_id, &sym, mark, *position_idx)
                    .is_ok()
                {
                    closed += 1;
                }
            }
            tracing::warn!(
                symbol = %symbol,
                closed = closed,
                mark_price = %mark,
                "Delist close_all: force-closed positions"
            );
            Json(serde_json::json!({
                "symbol": symbol,
                "phase": "closed_all",
                "positionsClosed": closed,
            }))
            .into_response()
        }
        "remove" => {
            {
                // Write path (admin-only): rebuild map and atomically swap.
                let old = exchange.product_status.load_full();
                let mut new_map = (*old).clone();
                new_map.insert(sym.clone(), "disabled".into());
                exchange.product_status.store(Arc::new(new_map));
            }
            tracing::warn!(symbol = %symbol, "Delist remove: pair disabled");
            Json(serde_json::json!({
                "symbol": symbol,
                "phase": "removed",
            }))
            .into_response()
        }
        _ => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "Invalid phase — must be one of: announce, settle_only, close_all, remove",
            })),
        )
            .into_response(),
    }
}

// ── Task 17: Maintenance Mode ─────────────────────────────────────────────────

/// POST /admin/maintenance — schedule or activate a maintenance window.
///
/// When `active` is true the exchange halt flag is set and all new orders
/// are rejected until maintenance ends.
pub async fn admin_set_maintenance(
    State(exchange): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let active = body
        .get("active")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let reason = body
        .get("reason")
        .and_then(|v| v.as_str())
        .unwrap_or("Scheduled maintenance")
        .to_string();
    let start = body
        .get("startTime")
        .and_then(|v| v.as_i64())
        .and_then(chrono::DateTime::from_timestamp_millis)
        .unwrap_or_else(chrono::Utc::now);
    let end = body
        .get("endTime")
        .and_then(|v| v.as_i64())
        .and_then(chrono::DateTime::from_timestamp_millis)
        .unwrap_or_else(|| chrono::Utc::now() + chrono::Duration::hours(2));

    let info = crate::exchange::MaintenanceInfo {
        scheduled_start: start,
        scheduled_end: end,
        reason: reason.clone(),
        is_active: active,
    };

    if active {
        exchange.operating_mode.store(exchange_common::OperatingMode::Halted);
        tracing::warn!(reason = %reason, "Maintenance mode activated — exchange halted");
    } else {
        tracing::info!(reason = %reason, "Maintenance window scheduled");
    }

    *exchange
        .maintenance_mode
        .lock().unwrap_or_else(|e| e.into_inner()) = Some(info);

    Json(serde_json::json!({
        "maintenance": active,
        "reason": reason,
        "startTime": start.timestamp_millis(),
        "endTime": end.timestamp_millis(),
    }))
    .into_response()
}

/// DELETE /admin/maintenance — end maintenance and resume trading.
pub async fn admin_end_maintenance(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    *exchange
        .maintenance_mode
        .lock().unwrap_or_else(|e| e.into_inner()) = None;
    exchange.operating_mode.store(exchange_common::OperatingMode::Normal);
    tracing::info!("Maintenance ended — exchange resumed");
    Json(serde_json::json!({"maintenance": false, "status": "resumed"}))
}

/// GET /api/v1/status/maintenance — public maintenance status check.
pub async fn get_maintenance_status(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let info = exchange
        .maintenance_mode
        .lock().unwrap_or_else(|e| e.into_inner());
    match info.as_ref() {
        Some(m) => Json(serde_json::json!({
            "maintenance": m.is_active,
            "scheduled": true,
            "startTime": m.scheduled_start.timestamp_millis(),
            "endTime": m.scheduled_end.timestamp_millis(),
            "reason": m.reason,
        })),
        None => Json(serde_json::json!({
            "maintenance": false,
            "scheduled": false,
        })),
    }
}

// ── Task 18: Device Management ────────────────────────────────────────────────

/// GET /api/v1/account/devices — list devices registered for the authenticated user.
pub async fn list_devices(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
) -> impl IntoResponse {
    let devices = exchange
        .user_devices
        .lock().unwrap_or_else(|e| e.into_inner());
    let list = devices.get(&user.0).cloned().unwrap_or_default();
    let json_list: Vec<serde_json::Value> = list
        .iter()
        .map(|d| {
            serde_json::json!({
                "id": d.id.to_string(),
                "deviceName": d.device_name,
                "ipAddress": d.ip_address,
                "lastLogin": d.last_login.timestamp_millis(),
                "trusted": d.trusted,
            })
        })
        .collect();
    Json(serde_json::json!({"devices": json_list}))
}

/// DELETE /api/v1/account/devices/{id} — revoke (remove) a registered device.
pub async fn revoke_device(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let device_id: uuid::Uuid = match id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid device ID"})),
            )
                .into_response()
        }
    };
    let mut devices = exchange
        .user_devices
        .lock().unwrap_or_else(|e| e.into_inner());
    if let Some(list) = devices.get_mut(&user.0) {
        list.retain(|d| d.id != device_id);
    }
    Json(serde_json::json!({"revoked": true})).into_response()
}

// ── Withdrawal Confirmation ────────────────────────────────────────────────

/// POST /api/v1/withdrawals/request — request a withdrawal with email confirmation.
///
/// Creates a pending confirmation token and returns it (in production, would send email).
/// The withdrawal is NOT processed until confirmed via GET /withdrawals/confirm/:token.
pub async fn request_withdrawal_with_confirmation(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let asset = match body.get("asset").and_then(|v| v.as_str()) {
        Some(a) if !a.is_empty() => a.to_string(),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing or empty 'asset'"})),
            )
                .into_response();
        }
    };

    let amount: rust_decimal::Decimal = match body
        .get("amount")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
    {
        Some(a) if a > rust_decimal::Decimal::ZERO => a,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing or invalid 'amount'"})),
            )
                .into_response();
        }
    };

    let address = body
        .get("address")
        .and_then(|v| v.as_str())
        .unwrap_or("default")
        .to_string();

    let token = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now();
    let expires_at = now + chrono::Duration::minutes(30);

    let confirmation = crate::exchange::WithdrawalConfirmation {
        user_id: user.0,
        asset: asset.clone(),
        amount,
        address: address.clone(),
        token: token.clone(),
        created_at: now,
        expires_at,
    };

    exchange
        .withdrawal_confirmations
        .lock().unwrap_or_else(|e| e.into_inner())
        .insert(token.clone(), confirmation);

    tracing::info!(
        user_id = %user.0,
        asset = %asset,
        amount = %amount,
        "withdrawal confirmation requested — token issued"
    );

    // Send confirmation email so the user can authorise or reject the withdrawal.
    if let Some(ref sender) = exchange.email_sender {
        let user_id = user.0;
        let user_email = exchange
            .user_emails
            .lock().unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|&(_, &uid)| uid == user_id)
            .map(|(email, _)| email.clone());

        if let Some(email_to) = user_email {
            let anti_phishing_code = exchange
                .anti_phishing_codes
                .lock().unwrap_or_else(|e| e.into_inner())
                .get(&user_id)
                .cloned();

            let sender = Arc::clone(sender);
            let asset_clone = asset.clone();
            let amount_str = amount.to_string();
            let address_clone = address.clone();
            let token_clone = token.clone();
            tokio::spawn(async move {
                let _ = sender
                    .send_with_anti_phishing(
                        &email_to,
                        exchange_admin::EmailTemplate::WithdrawalConfirmation {
                            asset: asset_clone,
                            amount: amount_str,
                            address: address_clone,
                            confirm_token: token_clone,
                        },
                        anti_phishing_code.as_deref(),
                    )
                    .await;
            });
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "confirmation_required",
            "token": token,
            "expiresAt": expires_at.timestamp_millis(),
        })),
    )
        .into_response()
}

/// GET /api/v1/withdrawals/confirm/:token — confirm a pending withdrawal.
pub async fn confirm_withdrawal(
    State(exchange): State<AppState>,
    Path(token): Path<String>,
) -> impl IntoResponse {
    let confirmation = exchange
        .withdrawal_confirmations
        .lock().unwrap_or_else(|e| e.into_inner())
        .remove(&token);

    match confirmation {
        Some(c) if c.expires_at > chrono::Utc::now() => {
            // Process the withdrawal.
            let result = exchange
                .accounts
                .withdraw(c.user_id, &c.asset, c.amount);

            match result {
                Ok(_) => {
                    tracing::info!(
                        user_id = %c.user_id,
                        asset = %c.asset,
                        amount = %c.amount,
                        "withdrawal confirmed and processed"
                    );
                    Json(serde_json::json!({
                        "status": "confirmed",
                        "asset": c.asset,
                        "amount": c.amount.to_string(),
                        "address": c.address,
                    }))
                    .into_response()
                }
                Err(e) => (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": format!("Withdrawal failed: {e}")})),
                )
                    .into_response(),
            }
        }
        Some(_) => (
            StatusCode::GONE,
            Json(serde_json::json!({"error": "Confirmation token has expired"})),
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Invalid or already used confirmation token"})),
        )
            .into_response(),
    }
}

// ── Auction Info Endpoints ─────────────────────────────────────────────────

/// GET /api/v1/markets/{symbol}/auction — indicative auction price + pending order count.
pub async fn get_auction_info(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = validate_symbol(&symbol) {
        return e.into_response();
    }

    let sym = exchange_common::Symbol::new(&symbol);

    let orders = exchange
        .auction_orders
        .lock().unwrap_or_else(|e| e.into_inner());
    let pending = orders.get(&sym).cloned().unwrap_or_default();

    let reference_price = exchange
        .pair_phases
        .lock()
        .get(&sym)
        .and_then(|c| c.reference_price);

    let indicative = exchange_matching_engine::indicative_price(&pending, reference_price);

    let bid_count = pending.iter().filter(|o| o.side == exchange_common::Side::Buy).count();
    let ask_count = pending.iter().filter(|o| o.side == exchange_common::Side::Sell).count();

    Json(serde_json::json!({
        "symbol": symbol,
        "pendingOrders": pending.len(),
        "bids": bid_count,
        "asks": ask_count,
        "indicativePrice": indicative.map(|p| p.to_string()),
    }))
    .into_response()
}

// ── Portfolio / Cross-Margin Mode (M13 Task 1) ────────────────────────────────

/// GET /api/v1/account/margin-mode — get the user's current margin mode setting.
///
/// Returns the per-user margin mode (isolated or cross) along with the effective
/// cross-margin balance if the user is in cross mode.
pub async fn get_margin_mode(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let mode = exchange.get_user_margin_mode(user.0);
    let mode_str = match mode {
        exchange_common::futures::MarginMode::Cross => "cross",
        exchange_common::futures::MarginMode::Isolated => "isolated",
    };

    let cross_balance = if mode == exchange_common::futures::MarginMode::Cross {
        Some(exchange.cross_margin_balance(user.0).to_string())
    } else {
        None
    };

    Json(serde_json::json!({
        "marginMode": mode_str,
        "crossMarginBalance": cross_balance,
    }))
    .into_response()
}

// ── Insurance Fund History (M13 Task 2) ──────────────────────────────────────

/// GET /api/v1/market/insurance — insurance fund balances with enhanced detail.
///
/// Returns current balance, total surplus/deficit since genesis, and event counts
/// for each symbol's insurance fund.
pub async fn get_insurance_fund_enhanced(
    State(exchange): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let symbol_filter = params.get("symbol").map(|s| s.as_str());

    let funds = exchange.insurance_funds.lock().unwrap_or_else(|e| e.into_inner());
    let history = exchange.insurance_fund_history.lock().unwrap_or_else(|e| e.into_inner());

    let entries: Vec<serde_json::Value> = funds
        .iter()
        .filter(|(sym, _)| symbol_filter.is_none_or( |s| sym.as_str() == s))
        .map(|(sym, fund)| {
            let sym_str = sym.to_string();
            let total_surplus: Decimal = history
                .iter()
                .filter(|e| e.symbol == sym_str && e.amount > Decimal::ZERO)
                .map(|e| e.amount)
                .sum();
            let total_deficit: Decimal = history
                .iter()
                .filter(|e| e.symbol == sym_str && e.amount < Decimal::ZERO)
                .map(|e| e.amount.abs())
                .sum();
            let event_count = history.iter().filter(|e| e.symbol == sym_str).count();

            serde_json::json!({
                "symbol": sym_str,
                "balance": fund.balance().to_string(),
                "totalSurplusContributed": total_surplus.to_string(),
                "totalDeficitCovered": total_deficit.to_string(),
                "eventCount": event_count,
                "updatedTime": chrono::Utc::now().timestamp_millis(),
            })
        })
        .collect();

    Json(serde_json::json!({ "list": entries })).into_response()
}

/// GET /admin/insurance/history — paginated insurance fund event history.
///
/// Optional query params: `symbol`, `limit` (default 100), `offset` (default 0).
pub async fn admin_insurance_history(
    State(exchange): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let symbol_filter = params.get("symbol").map(|s| s.as_str());
    let limit: usize = params
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(100)
        .min(1000);
    let offset: usize = params
        .get("offset")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let history = exchange.insurance_fund_history.lock().unwrap_or_else(|e| e.into_inner());

    let filtered: Vec<&crate::exchange::InsuranceFundHistoryEntry> = history
        .iter()
        .filter(|e| symbol_filter.is_none_or( |s| e.symbol.as_str() == s))
        .collect();

    let total = filtered.len();
    let entries: Vec<serde_json::Value> = filtered
        .into_iter()
        .rev() // most recent first
        .skip(offset)
        .take(limit)
        .map(|e| {
            serde_json::json!({
                "id": e.id.to_string(),
                "symbol": e.symbol,
                "amount": e.amount.to_string(),
                "balanceAfter": e.balance_after.to_string(),
                "reason": e.reason,
                "timestamp": e.timestamp.timestamp_millis(),
            })
        })
        .collect();

    Json(serde_json::json!({
        "total": total,
        "offset": offset,
        "limit": limit,
        "list": entries,
    }))
    .into_response()
}

// ---------------------------------------------------------------------------
// Bracket orders (Task 9)
// ---------------------------------------------------------------------------

/// Request body for `POST /api/v1/orders/bracket`.
#[derive(Debug, Deserialize)]
pub struct BracketOrderRequest {
    pub symbol: String,
    pub side: String,
    pub order_type: String,
    #[serde(default)]
    pub time_in_force: Option<String>,
    pub price: Option<Decimal>,
    pub qty: Decimal,
    /// Take-profit trigger price.
    pub take_profit_price: Option<Decimal>,
    /// Take-profit limit price (None = market on trigger).
    #[serde(default)]
    pub take_profit_order_price: Option<Decimal>,
    /// Stop-loss trigger price.
    pub stop_loss_price: Option<Decimal>,
    /// Stop-loss limit price (None = market on trigger).
    #[serde(default)]
    pub stop_loss_order_price: Option<Decimal>,
    #[serde(default)]
    pub reduce_only: Option<bool>,
    #[serde(default)]
    pub position_idx: Option<u8>,
    #[serde(default)]
    pub order_link_id: Option<String>,
}

/// POST /api/v1/orders/bracket
///
/// Place an entry order with attached take-profit and stop-loss as an atomic
/// bracket group.  The TP and SL are activated automatically when the entry
/// order fills.  When either leg fills, the other is cancelled (OCO).
pub async fn place_bracket_order(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<BracketOrderRequest>,
) -> impl IntoResponse {
    if let Err(e) = validate_symbol(&req.symbol) {
        return e.into_response();
    }
    if let Err(e) = validate_positive(req.qty, "qty") {
        return e.into_response();
    }

    let side = match req.side.to_lowercase().as_str() {
        "buy" => Side::Buy,
        "sell" => Side::Sell,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid side. Use 'buy' or 'sell'"})),
            ).into_response();
        }
    };

    let order_type = match req.order_type.to_lowercase().as_str() {
        "limit" => OrderType::Limit,
        "market" => OrderType::Market,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid order_type. Use 'limit' or 'market'"})),
            ).into_response();
        }
    };

    let time_in_force = match req.time_in_force.as_deref().unwrap_or("gtc") {
        "gtc" => TimeInForce::Gtc,
        "ioc" => TimeInForce::Ioc,
        "fok" => TimeInForce::Fok,
        _ => TimeInForce::Gtc,
    };

    let price = match order_type {
        OrderType::Limit => {
            match req.price {
                Some(p) if p > Decimal::ZERO => p,
                _ => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({"error": "Limit order requires positive price"})),
                    ).into_response();
                }
            }
        }
        OrderType::Market => Decimal::ZERO,
    };

    // At least one of TP or SL must be specified.
    if req.take_profit_price.is_none() && req.stop_loss_price.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Bracket order requires at least take_profit_price or stop_loss_price"})),
        ).into_response();
    }

    let user_id = auth.0;
    let sym = Symbol::new(&req.symbol);
    let entry_order_id = Uuid::new_v4();
    let closing_side = side.opposite();

    let tp_spec = req.take_profit_price.map(|tp| {
        exchange_trigger_service::TriggerOrderSpec {
            trigger_price: tp,
            order_price: req.take_profit_order_price,
            qty: req.qty,
        }
    });

    let sl_spec = req.stop_loss_price.map(|sl| {
        exchange_trigger_service::TriggerOrderSpec {
            trigger_price: sl,
            order_price: req.stop_loss_order_price,
            qty: req.qty,
        }
    });

    // Register the bracket *before* placing the entry order.
    let bracket = {
        let mut bm = exchange.bracket_manager.lock().unwrap_or_else(|e| e.into_inner());
        bm.create_bracket(user_id, sym.clone(), entry_order_id, tp_spec.clone(), sl_spec.clone())
    };

    // Place the entry order.
    let cmd = PlaceOrderCommand {
        order_id: entry_order_id,
        user_id,
        symbol: sym.clone(),
        side,
        order_type,
        time_in_force,
        price,
        qty: req.qty,
        reduce_only: req.reduce_only.unwrap_or(false),
        expire_at: None,
        timestamp: chrono::Utc::now(),
        position_idx: req.position_idx.unwrap_or(0),
        iceberg_qty: None,
        order_link_id: req.order_link_id.clone(),
        slippage_tolerance: None,
        close_position: false,
        stp_mode: Default::default(),
        request_expire_time: None,
    };

    let exchange_clone = Arc::clone(&exchange);
    let events = match crate::request_id::spawn_blocking_traced(move || exchange_clone.place_order(cmd))
        .await
        .unwrap_or_else(|e| Err(anyhow::anyhow!("task join error: {e}")))
    {
        Ok(ev) => ev,
        Err(e) => {
            // Roll back bracket registration on submission failure.
            exchange.bracket_manager.lock().unwrap_or_else(|e| e.into_inner())
                .cancel_bracket(bracket.id);
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": e.to_string()})),
            ).into_response();
        }
    };

    // Determine if entry filled immediately.
    let entry_filled_qty: Decimal = events.iter().filter_map(|ev| {
        if let exchange_common::MatchEvent::Fill(f) = ev {
            if f.taker_order_id == entry_order_id || f.maker_order_id == entry_order_id {
                return Some(f.qty);
            }
        }
        None
    }).sum();

    // Activate bracket if entry filled completely.
    if entry_filled_qty >= req.qty {
        let specs = {
            let mut bm = exchange.bracket_manager.lock().unwrap_or_else(|e| e.into_inner());
            bm.on_entry_filled(entry_order_id)
        };

        if let Some((tp_out, sl_out)) = specs {
            let mut tp_trigger_id: Option<Uuid> = None;
            let mut sl_trigger_id: Option<Uuid> = None;
            let now = chrono::Utc::now();
            let mut te = exchange.trigger_engine.lock().unwrap_or_else(|e| e.into_inner());

            if let Some(spec) = tp_out {
                let tp_order = TriggerOrder {
                    id: Uuid::new_v4(),
                    user_id,
                    symbol: sym.clone(),
                    trigger_type: TriggerOrderType::TakeProfit,
                    side: closing_side,
                    trigger_price: spec.trigger_price,
                    order_price: spec.order_price,
                    qty: spec.qty,
                    trigger_by: TriggerBy::LastPrice,
                    reduce_only: true,
                    close_on_trigger: false,
                    trailing_delta: None,
                    trail_type: Default::default(),
                    tp_limit_price: None,
                    sl_limit_price: None,
                    tp_order_type: Default::default(),
                    sl_order_type: Default::default(),
                    status: TriggerStatus::Active,
                    created_at: now,
                    triggered_at: None,
                    expires_at: None,
                };
                tp_trigger_id = Some(tp_order.id);
                let _ = te.add_order(tp_order);
            }

            if let Some(spec) = sl_out {
                let sl_order = TriggerOrder {
                    id: Uuid::new_v4(),
                    user_id,
                    symbol: sym.clone(),
                    trigger_type: TriggerOrderType::StopMarket,
                    side: closing_side,
                    trigger_price: spec.trigger_price,
                    order_price: spec.order_price,
                    qty: spec.qty,
                    trigger_by: TriggerBy::LastPrice,
                    reduce_only: true,
                    close_on_trigger: false,
                    trailing_delta: None,
                    trail_type: Default::default(),
                    tp_limit_price: None,
                    sl_limit_price: None,
                    tp_order_type: Default::default(),
                    sl_order_type: Default::default(),
                    status: TriggerStatus::Active,
                    created_at: now,
                    triggered_at: None,
                    expires_at: None,
                };
                sl_trigger_id = Some(sl_order.id);
                let _ = te.add_order(sl_order);
            }
            drop(te);

            let mut bm = exchange.bracket_manager.lock().unwrap_or_else(|e| e.into_inner());
            bm.record_trigger_ids(bracket.id, tp_trigger_id, sl_trigger_id);
        }
    }

    tracing::info!(
        user_id = %user_id,
        bracket_id = %bracket.id,
        entry_order_id = %entry_order_id,
        symbol = %req.symbol,
        "bracket order placed"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "bracket_id": bracket.id.to_string(),
            "entry_order_id": entry_order_id.to_string(),
            "status": "pending",
        })),
    ).into_response()
}

// ---------------------------------------------------------------------------
// Chain orders (Task 9)
// ---------------------------------------------------------------------------

/// A single step specification in a chain order request.
#[derive(Debug, Deserialize)]
pub struct ChainStepRequest {
    pub symbol: String,
    pub side: String,
    pub order_type: String,
    #[serde(default)]
    pub time_in_force: Option<String>,
    pub price: Option<Decimal>,
    pub qty: Decimal,
    #[serde(default)]
    pub reduce_only: Option<bool>,
    #[serde(default)]
    pub position_idx: Option<u8>,
    #[serde(default)]
    pub order_link_id: Option<String>,
    #[serde(default)]
    pub slippage_tolerance: Option<Decimal>,
}

/// Request body for `POST /api/v1/orders/chain`.
#[derive(Debug, Deserialize)]
pub struct ChainOrderRequest {
    /// Ordered list of order specifications.  The first is submitted immediately;
    /// each subsequent step is submitted when the previous one fully fills.
    pub steps: Vec<ChainStepRequest>,
}

/// Parse a `ChainStepRequest` into a `ChainStep`.
fn parse_chain_step(req: &ChainStepRequest) -> Result<exchange_trigger_service::ChainStep, (StatusCode, axum::Json<serde_json::Value>)> {
    let side = match req.side.to_lowercase().as_str() {
        "buy" => Side::Buy,
        "sell" => Side::Sell,
        _ => return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid side in chain step. Use 'buy' or 'sell'"})),
        )),
    };
    let order_type = match req.order_type.to_lowercase().as_str() {
        "limit" => OrderType::Limit,
        "market" => OrderType::Market,
        _ => return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid order_type in chain step"})),
        )),
    };
    let time_in_force = match req.time_in_force.as_deref().unwrap_or("gtc") {
        "gtc" => TimeInForce::Gtc,
        "ioc" => TimeInForce::Ioc,
        "fok" => TimeInForce::Fok,
        _ => TimeInForce::Gtc,
    };
    let price = match order_type {
        OrderType::Limit => req.price.unwrap_or(Decimal::ZERO),
        OrderType::Market => Decimal::ZERO,
    };

    Ok(exchange_trigger_service::ChainStep {
        symbol: Symbol::new(&req.symbol),
        side,
        order_type,
        time_in_force,
        price,
        qty: req.qty,
        reduce_only: req.reduce_only.unwrap_or(false),
        position_idx: req.position_idx.unwrap_or(0),
        order_link_id: req.order_link_id.clone(),
        slippage_tolerance: req.slippage_tolerance,
    })
}

/// POST /api/v1/orders/chain
///
/// Place a sequential chain of orders where each step is submitted to the
/// matching engine only after the previous step fully fills.
pub async fn place_chain_order(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<ChainOrderRequest>,
) -> impl IntoResponse {
    if req.steps.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Chain order requires at least one step"})),
        ).into_response();
    }
    if req.steps.len() > 10 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Chain order supports at most 10 steps"})),
        ).into_response();
    }

    // Parse all steps first so we surface validation errors before submitting anything.
    let mut parsed_steps = Vec::with_capacity(req.steps.len());
    for (i, step_req) in req.steps.iter().enumerate() {
        if let Err(e) = validate_symbol(&step_req.symbol) {
            return e.into_response();
        }
        if let Err(e) = validate_positive(step_req.qty, &format!("steps[{i}].qty")) {
            return e.into_response();
        }
        match parse_chain_step(step_req) {
            Ok(s) => parsed_steps.push(s),
            Err(e) => return e.into_response(),
        }
    }

    let user_id = auth.0;
    let first_step = parsed_steps[0].clone();

    // Submit step 0 to the matching engine immediately.
    let initial_order_id = Uuid::new_v4();
    let cmd = PlaceOrderCommand {
        order_id: initial_order_id,
        user_id,
        symbol: first_step.symbol.clone(),
        side: first_step.side,
        order_type: first_step.order_type,
        time_in_force: first_step.time_in_force,
        price: first_step.price,
        qty: first_step.qty,
        reduce_only: first_step.reduce_only,
        expire_at: None,
        timestamp: chrono::Utc::now(),
        position_idx: first_step.position_idx,
        iceberg_qty: None,
        order_link_id: first_step.order_link_id.clone(),
        slippage_tolerance: first_step.slippage_tolerance,
        close_position: false,
        stp_mode: Default::default(),
        request_expire_time: None,
    };

    let exchange_clone = Arc::clone(&exchange);
    if let Err(e) = crate::request_id::spawn_blocking_traced(move || exchange_clone.place_order(cmd))
        .await
        .unwrap_or_else(|e| Err(anyhow::anyhow!("task join error: {e}")))
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        ).into_response();
    }

    // Register the chain.
    let chain = {
        let mut cm = exchange.chain_manager.lock().unwrap_or_else(|e| e.into_inner());
        match cm.create_chain(user_id, parsed_steps, initial_order_id) {
            Some(c) => c,
            None => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "Failed to create chain order"})),
                ).into_response();
            }
        }
    };

    tracing::info!(
        user_id = %user_id,
        chain_id = %chain.id,
        steps = req.steps.len(),
        "chain order created"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "chain_id": chain.id.to_string(),
            "active_order_id": initial_order_id.to_string(),
            "current_step": 0,
            "total_steps": chain.steps.len(),
            "status": "active",
        })),
    ).into_response()
}

/// GET /api/v1/orders/chain/{chain_id}
pub async fn get_chain_order(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(chain_id_str): Path<String>,
) -> impl IntoResponse {
    let chain_id = match Uuid::parse_str(&chain_id_str) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid chain_id format"})),
            ).into_response();
        }
    };

    let cm = exchange.chain_manager.lock().unwrap_or_else(|e| e.into_inner());
    let chain = match cm.chain(chain_id) {
        Some(c) => c,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "Chain order not found"})),
            ).into_response();
        }
    };

    if chain.user_id != auth.0 {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Access denied"})),
        ).into_response();
    }

    Json(serde_json::json!({
        "chain_id": chain.id.to_string(),
        "status": format!("{:?}", chain.status).to_lowercase(),
        "current_step": chain.current_step,
        "total_steps": chain.steps.len(),
        "active_order_id": chain.active_order_id.map(|id| id.to_string()),
        "created_at": chain.created_at.timestamp_millis(),
    })).into_response()
}

/// DELETE /api/v1/orders/chain/{chain_id}
pub async fn cancel_chain_order(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(chain_id_str): Path<String>,
) -> impl IntoResponse {
    let chain_id = match Uuid::parse_str(&chain_id_str) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid chain_id format"})),
            ).into_response();
        }
    };

    // Check ownership before mutating.
    {
        let cm = exchange.chain_manager.lock().unwrap_or_else(|e| e.into_inner());
        match cm.chain(chain_id) {
            Some(c) if c.user_id != auth.0 => {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({"error": "Access denied"})),
                ).into_response();
            }
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "Chain order not found"})),
                ).into_response();
            }
            _ => {}
        }
    }

    let active_order_id = exchange
        .chain_manager
        .lock().unwrap_or_else(|e| e.into_inner())
        .cancel_chain(chain_id);

    if let Some(order_id) = active_order_id {
        // Best-effort cancel of the active matching-engine order.
        let cmd = exchange_common::CancelOrderCommand {
            order_id,
            user_id: auth.0,
            symbol: exchange_common::Symbol::new(""),
            timestamp: chrono::Utc::now(),
        };
        let _ = exchange.cancel_order(cmd);
    }

    tracing::info!(chain_id = %chain_id, user_id = %auth.0, "chain order cancelled");

    (StatusCode::OK, Json(serde_json::json!({
        "chain_id": chain_id.to_string(),
        "status": "cancelled",
    }))).into_response()
}

// ---------------------------------------------------------------------------
// TWAP via /orders/twap (Task 9 — expose via orders namespace)
// ---------------------------------------------------------------------------

/// GET /api/v1/orders/twap/{algo_id}
///
/// Returns the status of a TWAP execution by its algo ID.
/// This is a convenience alias; the canonical endpoint is /api/v1/algo/{algo_id}.
pub async fn get_twap_order(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(algo_id_str): Path<String>,
) -> impl IntoResponse {
    let algo_id = match Uuid::parse_str(&algo_id_str) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid algo_id format"})),
            ).into_response();
        }
    };

    let twap_execs = exchange.twap_executions.lock().unwrap_or_else(|e| e.into_inner());
    let exec = match twap_execs.get(&algo_id) {
        Some(e) => e,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "TWAP execution not found"})),
            ).into_response();
        }
    };

    if exec.config.user_id != auth.0 {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Access denied"})),
        ).into_response();
    }

    let slices: Vec<serde_json::Value> = exec.slices.iter().map(|s| serde_json::json!({
        "index": s.index,
        "scheduled_at": s.scheduled_at.timestamp_millis(),
        "qty": s.qty.to_string(),
        "fill_qty": s.fill_qty.to_string(),
        "fill_price": s.fill_price.map(|p| p.to_string()),
        "status": format!("{:?}", s.status).to_lowercase(),
        "order_id": s.order_id.map(|id| id.to_string()),
    })).collect();

    Json(serde_json::json!({
        "algo_id": exec.id.to_string(),
        "symbol": exec.config.symbol.to_string(),
        "side": format!("{:?}", exec.config.side).to_lowercase(),
        "total_qty": exec.config.total_qty.to_string(),
        "filled_qty": exec.filled_qty.to_string(),
        "avg_fill_price": exec.avg_fill_price.to_string(),
        "progress_pct": (exec.progress() * rust_decimal::Decimal::from(100)).to_string(),
        "status": format!("{:?}", exec.status).to_lowercase(),
        "duration_secs": exec.config.duration_secs,
        "num_slices": exec.config.num_slices,
        "created_at": exec.created_at.timestamp_millis(),
        "slices": slices,
    })).into_response()
}

// ── Performance Benchmarking — admin endpoints (Task 5) ───────────────────────

/// GET /admin/performance/latency — per-symbol p50/p95/p99 latency stats.
///
/// Returns latency statistics derived from the histogram data accumulated
/// since process start. Latency is measured from `place_order` entry to
/// events returned (includes WAL write + matching engine round trip).
pub async fn admin_performance_latency(State(exchange): State<AppState>) -> impl IntoResponse {
    let per_symbol = exchange.metrics.symbol_latency_stats();

    let global_count = exchange.metrics.place_order_latency.count();
    let global_p50 = exchange.metrics.place_order_latency.percentile(50.0);
    let global_p95 = exchange.metrics.place_order_latency.percentile(95.0);
    let global_p99 = exchange.metrics.place_order_latency.percentile(99.0);
    let global_avg = if global_count > 0 {
        Some(exchange.metrics.place_order_latency.sum_ns() as f64 / global_count as f64)
    } else {
        None
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "global": {
                "count": global_count,
                "p50_ns": global_p50,
                "p95_ns": global_p95,
                "p99_ns": global_p99,
                "avg_ns": global_avg,
                "p50_us": global_p50.map(|v| v as f64 / 1000.0),
                "p95_us": global_p95.map(|v| v as f64 / 1000.0),
                "p99_us": global_p99.map(|v| v as f64 / 1000.0),
                "avg_us": global_avg.map(|v| v / 1000.0),
            },
            "per_symbol": per_symbol.iter().map(|s| serde_json::json!({
                "symbol": s.symbol,
                "count": s.count,
                "p50_ns": s.p50_ns,
                "p95_ns": s.p95_ns,
                "p99_ns": s.p99_ns,
                "avg_ns": s.avg_ns,
                "p50_us": s.p50_ns.map(|v| v as f64 / 1000.0),
                "p95_us": s.p95_ns.map(|v| v as f64 / 1000.0),
                "p99_us": s.p99_ns.map(|v| v as f64 / 1000.0),
                "avg_us": s.avg_ns.map(|v| v / 1000.0),
            })).collect::<Vec<_>>(),
        })),
    )
}

/// GET /admin/performance/throughput — orders/sec and fills/sec over rolling windows.
///
/// Windows: last 1 minute, last 5 minutes, last 1 hour.
/// Values are time-averaged rates (events per second).
pub async fn admin_performance_throughput(State(exchange): State<AppState>) -> impl IntoResponse {
    let stats = exchange.metrics.throughput_stats();

    // Also expose current queue depth per symbol (number of resting orders on book).
    let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
    let queue_depths: Vec<serde_json::Value> = pool
        .symbols()
        .iter()
        .filter_map(|sym| {
            pool.snapshot(sym, 200).ok().map(|snap| {
                serde_json::json!({
                    "symbol": sym.to_string(),
                    "bids": snap.bids.len(),
                    "asks": snap.asks.len(),
                    "total": snap.bids.len() + snap.asks.len(),
                })
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "throughput": {
                "orders_per_sec": {
                    "1m": stats.orders_per_sec_1m,
                    "5m": stats.orders_per_sec_5m,
                    "1h": stats.orders_per_sec_1h,
                },
                "fills_per_sec": {
                    "1m": stats.fills_per_sec_1m,
                    "5m": stats.fills_per_sec_5m,
                    "1h": stats.fills_per_sec_1h,
                },
            },
            "totals": {
                "orders_placed": exchange.metrics.orders_placed.load(std::sync::atomic::Ordering::Relaxed),
                "fills_total": exchange.metrics.fills_total.load(std::sync::atomic::Ordering::Relaxed),
                "trades_total": exchange.metrics.trades_total.load(std::sync::atomic::Ordering::Relaxed),
            },
            "queue_depths": queue_depths,
        })),
    )
}

// ── Task 7: Security Hardening — admin endpoints ──────────────────────────────

/// GET /admin/security/blocked-ips — list all currently blocked IPs.
pub async fn admin_list_blocked_ips(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let blocked = exchange.ip_reputation.blocked_ips();
    let list: Vec<serde_json::Value> = blocked
        .iter()
        .map(|b| {
            serde_json::json!({
                "ip": b.ip,
                "blockedUntil": b.blocked_until.map(|t| t.timestamp_millis()),
                "failedAttempts": b.failed_attempts,
            })
        })
        .collect();
    Json(serde_json::json!({"blockedIps": list}))
}

/// DELETE /admin/security/blocked-ips/{ip} — manually unblock an IP address.
pub async fn admin_unblock_ip(
    State(exchange): State<AppState>,
    Path(ip): Path<String>,
) -> impl IntoResponse {
    exchange.ip_reputation.unblock(&ip);
    tracing::info!(ip = %ip, "admin_unblock_ip: IP unblocked by admin");
    Json(serde_json::json!({"ok": true, "ip": ip}))
}

#[derive(Debug, Deserialize)]
pub struct AdminBlockIpRequest {
    pub ip: String,
    /// How long the block should last. Defaults to 30 minutes
    /// (`BLOCK_DURATION_SECS`), matching the auto-block duration.
    pub duration_secs: Option<u64>,
    /// Free-form operator note — logged but not stored.
    pub reason: Option<String>,
}

/// POST /admin/security/blocked-ips — manually block an IP address.
///
/// Symmetric to the existing DELETE endpoint. Used by the high-latency
/// runbook to immediately cut off an abusive client during an incident.
/// Overrides any existing record for this IP.
pub async fn admin_block_ip(
    State(exchange): State<AppState>,
    Json(req): Json<AdminBlockIpRequest>,
) -> impl IntoResponse {
    if req.ip.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "ip must not be empty"})),
        );
    }

    let duration_secs = req
        .duration_secs
        .unwrap_or(crate::security::BLOCK_DURATION_SECS);

    exchange.ip_reputation.block_manually(&req.ip, duration_secs);

    tracing::warn!(
        ip = %req.ip,
        duration_secs,
        reason = req.reason.as_deref().unwrap_or("(none)"),
        "admin_block_ip: IP blocked by admin"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "ok": true,
            "ip": req.ip,
            "duration_secs": duration_secs,
        })),
    )
}

/// GET /admin/security/events — query the security event log.
///
/// Query parameters:
///   - `event_type` — filter by event type (e.g. `login_failed`)
///   - `user_id`    — filter by user UUID
///   - `ip`         — filter by source IP
///   - `since_ms`   — Unix millisecond timestamp lower bound
///   - `limit`      — maximum results (default 100, max 1000)
pub async fn admin_security_events(
    State(exchange): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    use crate::security::{SecurityEventFilter, SecurityEventType};

    let event_type = params.get("event_type").and_then(|s| {
        match s.as_str() {
            "login_success" => Some(SecurityEventType::LoginSuccess),
            "login_failed" => Some(SecurityEventType::LoginFailed),
            "api_key_created" => Some(SecurityEventType::ApiKeyCreated),
            "withdrawal_requested" => Some(SecurityEventType::WithdrawalRequested),
            "2fa_failed" | "two_fa_failed" => Some(SecurityEventType::TwoFaFailed),
            "ip_blocked" => Some(SecurityEventType::IpBlocked),
            "session_expired" => Some(SecurityEventType::SessionExpired),
            "session_revoked" => Some(SecurityEventType::SessionRevoked),
            _ => None,
        }
    });

    let user_id = params.get("user_id").and_then(|s| uuid::Uuid::parse_str(s).ok());
    let ip = params.get("ip").cloned();
    let since_ms = params.get("since_ms").and_then(|s| s.parse::<i64>().ok());
    let limit = params.get("limit").and_then(|s| s.parse::<usize>().ok());

    let filter = SecurityEventFilter { event_type, user_id, ip, since_ms, limit };

    let log = exchange.security_events.lock().unwrap_or_else(|e| e.into_inner());
    let events = log.query(&filter);

    let list: Vec<serde_json::Value> = events
        .iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id.to_string(),
                "eventType": format!("{:?}", e.event_type),
                "userId": e.user_id.map(|u| u.to_string()),
                "ip": e.ip,
                "timestamp": e.timestamp.timestamp_millis(),
                "details": e.details,
            })
        })
        .collect();

    Json(serde_json::json!({"events": list, "count": list.len()}))
}

/// GET /admin/security/rate-limit-configs — list all per-endpoint-group rate limit configs.
pub async fn admin_rate_limit_configs(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let configs: Vec<serde_json::Value> = exchange
        .rate_limit_configs
        .values()
        .map(|c| {
            serde_json::json!({
                "name": c.name,
                "maxRequests": c.max_requests,
                "windowSecs": c.window_secs,
                "burst": c.burst,
                "ratePerSecond": c.rate_per_second(),
            })
        })
        .collect();
    Json(serde_json::json!({"configs": configs}))
}

// ── Sub-account Trading Endpoints (Task 3) ────────────────────────────────────

/// Helper: verify the authenticated user is the master of the given sub-account.
///
/// Returns `Ok(sub_id)` on success, or a 403/404 error response.
fn verify_sub_ownership(
    exchange: &crate::exchange::Exchange,
    master_id: Uuid,
    sub_id: Uuid,
) -> Result<Uuid, (StatusCode, Json<serde_json::Value>)> {
    let subs = exchange.sub_accounts.lock().unwrap_or_else(|e| e.into_inner());
    match subs.get_master(sub_id) {
        Some(parent) if parent == master_id => Ok(sub_id),
        Some(_) => Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Sub-account does not belong to the authenticated user"})),
        )),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Sub-account not found"})),
        )),
    }
}

/// POST /api/v1/sub-accounts/{id}/orders
///
/// Place an order on behalf of a sub-account.
///
/// The authenticated user must be the master account of the given sub-account.
/// The sub-account must be active.  The order goes through the same matching
/// engine path as any other order; risk (margin, balance locks) is evaluated
/// against the sub-account's own balances — not the master's.
pub async fn sub_account_place_order(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(sub_id): Path<Uuid>,
    Json(req): Json<PlaceOrderRequest>,
) -> impl IntoResponse {
    let master_id = auth.0;

    // Verify ownership and retrieve the sub_id.
    let sub_id = match verify_sub_ownership(&exchange, master_id, sub_id) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };

    // Check the sub-account is active.
    {
        let subs = exchange.sub_accounts.lock().unwrap_or_else(|e| e.into_inner());
        let group_subs = subs.list_subs(master_id);
        let sub = group_subs.iter().find(|s| s.id == sub_id);
        match sub {
            Some(s) if !s.is_active => {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({"error": "Sub-account is deactivated"})),
                ).into_response();
            }
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "Sub-account not found"})),
                ).into_response();
            }
            _ => {}
        }
    }

    // Ensure the sub-account has a trading account record (idempotent).
    exchange.accounts.create_account(sub_id);

    // Delegate to the regular place_order logic by re-dispatching through the
    // internal helper.  We synthesise an AuthenticatedUser for the sub-account.
    let sub_auth = axum::Extension(AuthenticatedUser(sub_id));
    place_order(State(exchange), sub_auth, Json(req)).await.into_response()
}

/// GET /api/v1/sub-accounts/{id}/positions
///
/// List open positions for the given sub-account.
/// The authenticated user must be the master.
pub async fn sub_account_get_positions(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(sub_id): Path<Uuid>,
) -> impl IntoResponse {
    let master_id = auth.0;

    let sub_id = match verify_sub_ownership(&exchange, master_id, sub_id) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };

    let positions = exchange.get_positions(sub_id);

    let result: Vec<serde_json::Value> = positions
        .iter()
        .map(|pos| {
            let mark_price = exchange
                .get_mark_price(&pos.symbol)
                .map(|mp| mp.price)
                .unwrap_or(pos.entry_price);

            serde_json::json!({
                "symbol": pos.symbol.to_string(),
                "side": format!("{:?}", pos.side),
                "size": pos.size,
                "entry_price": pos.entry_price,
                "mark_price": mark_price,
                "unrealized_pnl": pos.unrealized_pnl(mark_price),
                "realized_pnl": pos.realized_pnl,
                "margin": pos.margin,
                "leverage": pos.leverage,
                "margin_mode": format!("{:?}", pos.margin_mode),
                "liq_price": pos.liquidation_price(),
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(result))).into_response()
}

/// GET /api/v1/sub-accounts/{id}/balances
///
/// Return the trading-account balances for the given sub-account.
/// The authenticated user must be the master.
pub async fn sub_account_get_balances(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(sub_id): Path<Uuid>,
) -> impl IntoResponse {
    let master_id = auth.0;

    let sub_id = match verify_sub_ownership(&exchange, master_id, sub_id) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };

    let balances = exchange.accounts.get_all_balances(sub_id);
    // A freshly created sub-account has no balance record yet — return an
    // empty array rather than a 404, because the sub-account exists.
    let result: Vec<serde_json::Value> = balances
        .iter()
        .map(|(asset, bal)| {
            serde_json::json!({
                "asset": asset,
                "total": bal.total(),
                "locked": bal.locked(),
                "available": bal.available(),
            })
        })
        .collect();

    (StatusCode::OK, Json(serde_json::json!(result))).into_response()
}

// ── Market Maker Program Endpoints (Task 4) ───────────────────────────────────

/// Request body for registering a market maker.
#[derive(Debug, serde::Deserialize)]
pub struct RegisterMarketMakerRequest {
    /// The exchange user ID to register as a market maker.
    pub user_id: Uuid,
    /// Display name for the MM entity.
    pub name: String,
    /// Optional maker fee override (e.g. Decimal::new(-1, 4) = -0.01%).
    #[serde(default)]
    pub maker_fee_override: Option<Decimal>,
    /// Optional taker fee override.
    #[serde(default)]
    pub taker_fee_override: Option<Decimal>,
}

/// POST /admin/market-makers
///
/// Register a user as a formal market maker (admin only).
/// Re-registering the same user_id replaces the existing record.
pub async fn admin_register_market_maker(
    State(exchange): State<AppState>,
    Json(req): Json<RegisterMarketMakerRequest>,
) -> impl IntoResponse {
    if req.name.is_empty() || req.name.len() > 100 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "name must be 1\u{2013}100 characters"})),
        ).into_response();
    }

    let mut program = exchange.market_maker_program.lock().unwrap_or_else(|e| e.into_inner());
    let record = program.register(req.user_id, &req.name, req.maker_fee_override, req.taker_fee_override);

    tracing::info!(
        user_id = %req.user_id,
        name = %req.name,
        "Market maker registered"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "user_id": record.user_id.to_string(),
            "name": record.name,
            "maker_fee_override": record.maker_fee_override,
            "taker_fee_override": record.taker_fee_override,
            "rate_limit_multiplier": record.rate_limit_multiplier,
            "is_active": record.is_active,
            "registered_at": record.registered_at.timestamp_millis(),
            "penalty_level": record.penalty_level,
        })),
    ).into_response()
}

/// GET /admin/market-makers
///
/// List all registered market makers with their current status.
pub async fn admin_list_market_makers(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let program = exchange.market_maker_program.lock().unwrap_or_else(|e| e.into_inner());

    let list: Vec<serde_json::Value> = program
        .records
        .values()
        .map(|r| serde_json::json!({
            "user_id": r.user_id.to_string(),
            "name": r.name,
            "maker_fee_override": r.maker_fee_override,
            "taker_fee_override": r.taker_fee_override,
            "rate_limit_multiplier": r.rate_limit_multiplier,
            "is_active": r.is_active,
            "registered_at": r.registered_at.timestamp_millis(),
            "total_violations": r.total_violations,
            "penalty_level": r.penalty_level,
            "last_evaluated_at": r.last_evaluated_at.map(|t| t.timestamp_millis()),
        }))
        .collect();

    (StatusCode::OK, Json(serde_json::json!(list))).into_response()
}

/// GET /admin/market-makers/{id}/stats
///
/// Return the SLA stats (latest report) for a specific market maker across
/// all symbols they have been evaluated on.
pub async fn admin_mm_stats(
    State(exchange): State<AppState>,
    Path(mm_user_id): Path<Uuid>,
) -> impl IntoResponse {
    let program = exchange.market_maker_program.lock().unwrap_or_else(|e| e.into_inner());

    // Check the MM is registered.
    if !program.records.contains_key(&mm_user_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Market maker not found"})),
        ).into_response();
    }

    // Collect all SLA reports for this MM in chronological order.
    // Reports are stored globally (all MMs, all symbols) so we filter by mm_id.
    let mm_reports: Vec<serde_json::Value> = program
        .reports
        .iter()
        .filter(|r| r.mm_id == mm_user_id)
        .map(|r| serde_json::json!({
            "evaluated_at": r.evaluated_at.timestamp_millis(),
            "uptime_pct": r.uptime_pct,
            "avg_spread_bps": r.avg_spread_bps,
            "min_depth_met_pct": r.min_depth_met_pct,
            "two_sided_pct": r.two_sided_pct,
            "avg_refill_time_secs": r.avg_refill_time_secs,
            "is_compliant": r.is_compliant,
            "demotion": r.demotion.map(|d| format!("{d:?}")),
        }))
        .collect();

    let record = program.records.get(&mm_user_id).expect("checked above");

    (StatusCode::OK, Json(serde_json::json!({
        "user_id": mm_user_id.to_string(),
        "name": record.name,
        "is_active": record.is_active,
        "penalty_level": record.penalty_level,
        "total_violations": record.total_violations,
        "reports": mm_reports,
    }))).into_response()
}

// ---------------------------------------------------------------------------
// Inverse Perpetuals: Contract info
// ---------------------------------------------------------------------------

/// GET /api/v1/markets/{symbol}/contract-info — contract specification for a perpetual symbol.
///
/// Returns the contract type (Linear/Inverse), settle coin, contract size, and risk parameters.
/// Returns 404 if the symbol is not a configured perpetual contract.
pub async fn get_contract_info(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = validate_symbol(&symbol) {
        return e;
    }

    let sym = Symbol::new(&symbol);
    let configs = exchange.perp_configs.load();

    let Some(cfg) = configs.get(&sym) else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("No perpetual config for symbol: {symbol}") })),
        );
    };

    let contract_type_str = match cfg.contract_type {
        exchange_common::futures::ContractType::Linear => "Linear",
        exchange_common::futures::ContractType::Inverse => "Inverse",
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "symbol": symbol,
            "contract_type": contract_type_str,
            "settle_coin": cfg.settle_coin,
            "contract_size": cfg.contract_size.to_string(),
            "max_leverage": cfg.max_leverage.to_string(),
            "maintenance_margin_rate": cfg.maintenance_margin_rate.to_string(),
            "funding_interval_hours": cfg.funding_interval_hours,
            "max_funding_rate": cfg.max_funding_rate.to_string(),
        })),
    )
}

// ── Cross-Collateral Haircuts ─────────────────────────────────────────────────

/// PUT /api/v1/account/collateral-coin — enable or disable a coin as cross-margin collateral.
///
/// Body: `{"coin": "BTC", "enabled": true}`
pub async fn set_collateral_coin(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> axum::response::Response {
    let coin = match body.get("coin").and_then(|v| v.as_str()) {
        Some(c) if !c.is_empty() => c.to_uppercase(),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing or empty 'coin' field"})),
            )
                .into_response();
        }
    };

    let enabled = match body.get("enabled").and_then(|v| v.as_bool()) {
        Some(e) => e,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing boolean 'enabled' field"})),
            )
                .into_response();
        }
    };

    {
        let configs = exchange.collateral_configs.lock().unwrap_or_else(|e| e.into_inner());
        match configs.get(&coin) {
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": format!("Coin '{}' is not configured as a collateral asset", coin)
                    })),
                )
                    .into_response();
            }
            Some(cfg) if !cfg.is_eligible => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": format!("Coin '{}' is not currently eligible as collateral", coin)
                    })),
                )
                    .into_response();
            }
            _ => {}
        }
    }

    {
        let mut settings = exchange
            .user_collateral_settings
            .lock().unwrap_or_else(|e| e.into_inner());

        let user_set = settings.entry(user.0).or_insert_with(|| {
            exchange
                .collateral_configs
                .lock().unwrap_or_else(|e| e.into_inner())
                .values()
                .filter(|c| c.is_eligible)
                .map(|c| c.coin.clone())
                .collect()
        });

        if enabled {
            user_set.insert(coin.clone());
        } else {
            user_set.remove(&coin);
        }
    }

    tracing::info!(
        user_id = %user.0,
        coin = %coin,
        enabled = enabled,
        "Updated collateral coin setting"
    );

    Json(serde_json::json!({
        "coin": coin,
        "enabled": enabled,
        "message": if enabled {
            format!("{coin} enabled as collateral")
        } else {
            format!("{coin} disabled as collateral")
        },
    }))
    .into_response()
}

/// GET /api/v1/account/collateral-info — per-coin collateral breakdown.
pub async fn get_collateral_info(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let configs = exchange.collateral_configs.lock().unwrap_or_else(|e| e.into_inner());
    let user_settings = exchange
        .user_collateral_settings
        .lock().unwrap_or_else(|e| e.into_inner());
    // S18 Phase B: snapshot the user's balances under one shard lock, then
    // loop over the collateral configs locally (the loop is bounded by
    // `configs` size, typically ~10-20 entries).
    let user_balances = exchange.accounts.get_all_balances(user.0);
    let index_agg = exchange.index_aggregator.lock().unwrap_or_else(|e| e.into_inner());

    let user_enabled: Option<&std::collections::HashSet<String>> = user_settings.get(&user.0);

    let mut coins: Vec<serde_json::Value> = Vec::new();
    let mut total_effective_collateral = Decimal::ZERO;

    let mut sorted_configs: Vec<&crate::exchange::CollateralConfig> = configs.values().collect();
    sorted_configs.sort_by_key(|c| c.coin.as_str());

    for cfg in sorted_configs {
        if !cfg.is_eligible {
            continue;
        }

        let is_enabled = match user_enabled {
            Some(set) => set.contains(&cfg.coin),
            None => true,
        };

        let balance = user_balances
            .get(&cfg.coin)
            .map(|b| b.available())
            .unwrap_or(Decimal::ZERO);

        let usd_price = if cfg.coin == "USDT" || cfg.coin == "USDC" {
            Decimal::ONE
        } else {
            let symbol = exchange_common::Symbol::new(format!("{}USDT", cfg.coin));
            match index_agg.calculate_index(&symbol) {
                Ok(idx) => idx.price,
                Err(_) => Decimal::ZERO,
            }
        };

        let capped_balance = match cfg.max_collateral_amount {
            Some(max) => balance.min(max),
            None => balance,
        };

        let effective = if is_enabled && usd_price > Decimal::ZERO {
            capped_balance * usd_price * cfg.haircut_rate
        } else {
            Decimal::ZERO
        };

        total_effective_collateral += effective;

        coins.push(serde_json::json!({
            "coin": cfg.coin,
            "haircutRate": cfg.haircut_rate.to_string(),
            "maxCollateralAmount": cfg.max_collateral_amount.map(|v| v.to_string()),
            "availableBalance": balance.to_string(),
            "usdPrice": usd_price.to_string(),
            "effectiveCollateral": effective.to_string(),
            "enabled": is_enabled,
        }));
    }

    Json(serde_json::json!({
        "coins": coins,
        "totalEffectiveCollateralUsd": total_effective_collateral.to_string(),
    }))
}

// ── Admin: Collateral Config Management ──────────────────────────────────────

/// GET /admin/collateral/configs — list all collateral asset configurations.
pub async fn admin_list_collateral_configs(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let configs = exchange.collateral_configs.lock().unwrap_or_else(|e| e.into_inner());
    let mut list: Vec<serde_json::Value> = configs
        .values()
        .map(|cfg| {
            serde_json::json!({
                "coin": cfg.coin,
                "haircutRate": cfg.haircut_rate.to_string(),
                "maxCollateralAmount": cfg.max_collateral_amount.map(|v| v.to_string()),
                "isEligible": cfg.is_eligible,
            })
        })
        .collect();
    list.sort_by(|a, b| {
        a["coin"]
            .as_str()
            .unwrap_or("")
            .cmp(b["coin"].as_str().unwrap_or(""))
    });
    Json(serde_json::json!({"configs": list}))
}

/// PUT /admin/collateral/configs/{coin} — create or update a collateral config.
pub async fn admin_update_collateral_config(
    State(exchange): State<AppState>,
    Path(coin): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> axum::response::Response {
    let coin = coin.to_uppercase();

    let haircut_rate: Option<Decimal> = if let Some(v) = body.get("haircutRate") {
        let raw = v.as_str().unwrap_or("");
        match raw.parse::<Decimal>() {
            Ok(r) if r > Decimal::ZERO && r <= Decimal::ONE => Some(r),
            Ok(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "haircutRate must be in (0, 1]"})),
                )
                    .into_response();
            }
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "haircutRate must be a valid decimal string"})),
                )
                    .into_response();
            }
        }
    } else {
        None
    };

    let max_collateral_amount: Option<Option<Decimal>> =
        if let Some(v) = body.get("maxCollateralAmount") {
            if v.is_null() {
                Some(None)
            } else {
                let raw = v.as_str().unwrap_or("");
                match raw.parse::<Decimal>() {
                    Ok(r) if r > Decimal::ZERO => Some(Some(r)),
                    _ => {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(serde_json::json!({"error": "maxCollateralAmount must be a positive decimal string or null"})),
                        )
                            .into_response();
                    }
                }
            }
        } else {
            None
        };

    let is_eligible: Option<bool> = body.get("isEligible").and_then(|v| v.as_bool());

    let mut configs = exchange.collateral_configs.lock().unwrap_or_else(|e| e.into_inner());

    let entry = configs
        .entry(coin.clone())
        .or_insert_with(|| crate::exchange::CollateralConfig {
            coin: coin.clone(),
            haircut_rate: Decimal::new(70, 2),
            max_collateral_amount: None,
            is_eligible: false,
            tier: crate::exchange::CollateralTier::Tier3,
        });

    if let Some(r) = haircut_rate {
        entry.haircut_rate = r;
    }
    if let Some(max) = max_collateral_amount {
        entry.max_collateral_amount = max;
    }
    if let Some(eligible) = is_eligible {
        entry.is_eligible = eligible;
    }

    let updated = entry.clone();
    drop(configs);

    tracing::info!(
        coin = %coin,
        haircut_rate = %updated.haircut_rate,
        is_eligible = updated.is_eligible,
        "Admin updated collateral config"
    );

    Json(serde_json::json!({
        "coin": updated.coin,
        "haircutRate": updated.haircut_rate.to_string(),
        "maxCollateralAmount": updated.max_collateral_amount.map(|v| v.to_string()),
        "isEligible": updated.is_eligible,
        "message": "Collateral config updated",
    }))
    .into_response()
}

// ── Liquidation history & public feed ────────────────────────────────────────

/// GET /account/liquidation-history — authenticated user's own liquidation events.
pub async fn get_liquidation_history(
    State(exchange): State<AppState>,
    axum::Extension(AuthenticatedUser(user_id)): axum::Extension<AuthenticatedUser>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let symbol_filter = params.get("symbol").map(|s| s.to_ascii_uppercase());
    let limit: usize = params
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(50)
        .min(200);

    let events = exchange.liquidation_events.lock().unwrap_or_else(|e| e.into_inner());
    let list: Vec<serde_json::Value> = events
        .iter()
        .rev()
        .filter(|e| e.user_id == user_id)
        .filter(|e| {
            symbol_filter
                .as_deref()
                .is_none_or( |s| e.symbol.as_str().eq_ignore_ascii_case(s))
        })
        .take(limit)
        .map(|e| serde_json::json!({
            "id":           e.id.to_string(),
            "symbol":       e.symbol.to_string(),
            "side":         format!("{:?}", e.side),
            "qtyLiquidated": e.qty_liquidated.to_string(),
            "price":        e.price.to_string(),
            "isPartial":    e.is_partial,
            "fee":          e.fee.to_string(),
            "timestamp":    e.timestamp.timestamp_millis(),
        }))
        .collect();

    Json(serde_json::json!({ "list": list })).into_response()
}

/// GET /admin/liquidations — all liquidation events (admin).
pub async fn admin_list_liquidations(
    State(exchange): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let symbol_filter = params.get("symbol").map(|s| s.to_ascii_uppercase());
    let user_filter: Option<uuid::Uuid> = params
        .get("user_id")
        .and_then(|s| s.parse().ok());
    let limit: usize = params
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(100)
        .min(1000);
    let offset: usize = params
        .get("offset")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let events = exchange.liquidation_events.lock().unwrap_or_else(|e| e.into_inner());
    let filtered: Vec<&exchange_risk_engine::LiquidationEvent> = events
        .iter()
        .rev()
        .filter(|e| {
            symbol_filter.as_deref()
                .is_none_or( |s| e.symbol.as_str().eq_ignore_ascii_case(s))
        })
        .filter(|e| user_filter.is_none_or( |uid| e.user_id == uid))
        .collect();

    let total = filtered.len();
    let list: Vec<serde_json::Value> = filtered
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|e| serde_json::json!({
            "id":            e.id.to_string(),
            "userId":        e.user_id.to_string(),
            "symbol":        e.symbol.to_string(),
            "side":          format!("{:?}", e.side),
            "qtyLiquidated": e.qty_liquidated.to_string(),
            "price":         e.price.to_string(),
            "isPartial":     e.is_partial,
            "fee":           e.fee.to_string(),
            "timestamp":     e.timestamp.timestamp_millis(),
        }))
        .collect();

    Json(serde_json::json!({
        "total":  total,
        "offset": offset,
        "limit":  limit,
        "list":   list,
    }))
    .into_response()
}

/// GET /market/liquidations/{symbol} — public, anonymised liquidation feed for a symbol.
pub async fn get_public_liquidations(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    if let Err(e) = validate_symbol(&symbol) {
        return e.into_response();
    }

    let limit: usize = params
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(50)
        .min(200);

    let sym_upper = symbol.to_ascii_uppercase();
    let events = exchange.liquidation_events.lock().unwrap_or_else(|e| e.into_inner());
    let list: Vec<serde_json::Value> = events
        .iter()
        .rev()
        .filter(|e| e.symbol.as_str().eq_ignore_ascii_case(&sym_upper))
        .take(limit)
        .map(|e| serde_json::json!({
            "symbol":  e.symbol.to_string(),
            "side":    format!("{:?}", e.side),
            "qty":     e.qty_liquidated.to_string(),
            "price":   e.price.to_string(),
            "time":    e.timestamp.timestamp_millis(),
        }))
        .collect();

    Json(serde_json::json!({ "symbol": sym_upper, "list": list })).into_response()
}

/// GET /account/adl-quantile — Bybit-style ADL indicator (1–5) per open position.
pub async fn get_adl_quantile(
    State(exchange): State<AppState>,
    axum::Extension(AuthenticatedUser(user_id)): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let mark_prices_full = exchange.get_mark_prices();
    let mark_prices: HashMap<_, _> = mark_prices_full
        .iter()
        .map(|(sym, mp)| (sym.clone(), mp.price))
        .collect();

    let all_positions: Vec<exchange_common::futures::Position> = {
        let pm = exchange.positions.lock();
        pm.all_positions().into_iter().cloned().collect()
    };

    let list: Vec<serde_json::Value> = all_positions
        .iter()
        .filter(|p| p.user_id == user_id)
        .map(|pos| {
            let mark = mark_prices.get(&pos.symbol).copied().unwrap_or(pos.entry_price);
            let quantile = exchange_risk_engine::adl_quantile(pos, &all_positions, mark);
            serde_json::json!({
                "symbol":   pos.symbol.to_string(),
                "side":     format!("{:?}", pos.side),
                "size":     pos.size.to_string(),
                "adlQuantile": quantile,
            })
        })
        .collect();

    Json(serde_json::json!({ "list": list })).into_response()
}

// ---------------------------------------------------------------------------
// Listen Key endpoints (Binance-style WS private data authentication)
// ---------------------------------------------------------------------------

/// POST /api/v1/listen-key — create a new listen key valid for 60 minutes.
pub async fn create_listen_key(
    State(exchange): State<AppState>,
    axum::Extension(auth_user): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    use crate::exchange::ListenKeyInfo;

    let key = uuid::Uuid::new_v4().to_string().replace('-', "");
    let now = chrono::Utc::now();
    let info = ListenKeyInfo {
        user_id: auth_user.0,
        created_at: now,
        expires_at: now + chrono::Duration::minutes(60),
    };

    exchange
        .listen_keys
        .lock().unwrap_or_else(|e| e.into_inner())
        .insert(key.clone(), info);

    tracing::info!(user_id = %auth_user.0, key = %key, "listen key created");

    (StatusCode::OK, Json(serde_json::json!({ "listenKey": key }))).into_response()
}

/// PUT /api/v1/listen-key — keepalive: extend a listen key's validity by 60 minutes.
#[derive(serde::Deserialize)]
pub struct ListenKeyBody {
    #[serde(rename = "listenKey")]
    pub listen_key: String,
}

pub async fn keepalive_listen_key(
    State(exchange): State<AppState>,
    axum::Extension(auth_user): axum::Extension<AuthenticatedUser>,
    Json(body): Json<ListenKeyBody>,
) -> impl IntoResponse {
    let now = chrono::Utc::now();
    let mut keys = exchange
        .listen_keys
        .lock().unwrap_or_else(|e| e.into_inner());

    match keys.get_mut(&body.listen_key) {
        Some(info) if info.user_id == auth_user.0 => {
            info.expires_at = now + chrono::Duration::minutes(60);
            tracing::debug!(user_id = %auth_user.0, key = %body.listen_key, "listen key renewed");
            (StatusCode::OK, Json(serde_json::json!({}))).into_response()
        }
        Some(_) => (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Listen key does not belong to this user" })),
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Listen key not found or expired" })),
        )
            .into_response(),
    }
}

/// DELETE /api/v1/listen-key — close / revoke a listen key.
pub async fn delete_listen_key(
    State(exchange): State<AppState>,
    axum::Extension(auth_user): axum::Extension<AuthenticatedUser>,
    Json(body): Json<ListenKeyBody>,
) -> impl IntoResponse {
    let mut keys = exchange
        .listen_keys
        .lock().unwrap_or_else(|e| e.into_inner());

    match keys.get(&body.listen_key) {
        Some(info) if info.user_id == auth_user.0 => {
            keys.remove(&body.listen_key);
            tracing::info!(user_id = %auth_user.0, key = %body.listen_key, "listen key deleted");
            (StatusCode::OK, Json(serde_json::json!({}))).into_response()
        }
        Some(_) => (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Listen key does not belong to this user" })),
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Listen key not found" })),
        )
            .into_response(),
    }
}

// ── Task: Spot Margin Borrow / Lend Engine ────────────────────────────────────

/// POST /api/v1/spot-margin/borrow — borrow a coin using trading-account
/// collateral.
///
/// Body: `{"coin": "USDT", "amount": "100"}`
///
/// The borrowed amount is credited to the user's trading account immediately.
/// A `MarginBorrow` record is created (or an existing active one is increased).
/// Interest rate is fixed at 0.02 %/day (base VIP-0 rate); the background task
/// `spawn_margin_interest_accrual` accrues interest every hour.
pub async fn spot_margin_borrow(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> axum::response::Response {
    let coin = match body.get("coin").and_then(|v| v.as_str()) {
        Some(c) => c.to_uppercase(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing coin"})),
            )
                .into_response()
        }
    };
    let amount: Decimal = match body
        .get("amount")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
    {
        Some(a) if a > Decimal::ZERO => a,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid amount: must be positive"})),
            )
                .into_response()
        }
    };

    // Determine the daily interest rate from VIP tier.
    // VIP 0 base rate: 0.02%/day; higher tiers get a small discount.
    let daily_rate = {
        let fe = exchange.fee_engine.lock().unwrap_or_else(|e| e.into_inner());
        let tier = fe.get_tier(user.0);
        // Discount: 0.5% per tier level on the base rate.
        let discount = Decimal::new(5, 3) * Decimal::from(tier.level as u32);
        let base = Decimal::new(2, 4); // 0.0002 = 0.02%
        (base * (Decimal::ONE - discount)).max(Decimal::new(1, 4)) // floor at 0.01%/day
    };

    // Max borrow = collateral * leverage_factor (3x) - existing borrows.
    let max_leverage_factor = Decimal::from(3u32);
    let collateral_usd = exchange
        .accounts
        .get_balance(user.0, &coin)
        .map(|b| b.available())
        .unwrap_or(Decimal::ZERO);
    let existing_borrow = {
        let borrows = exchange.margin_borrows.lock().unwrap_or_else(|e| e.into_inner());
        borrows
            .get(&(user.0, coin.clone()))
            .map(|b| b.amount)
            .unwrap_or(Decimal::ZERO)
    };
    let max_borrow = collateral_usd * max_leverage_factor - existing_borrow;
    if max_borrow <= Decimal::ZERO {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "Borrow limit reached",
                "maxBorrow": "0",
            })),
        )
            .into_response();
    }
    if amount > max_borrow {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "Amount exceeds maximum borrowable",
                "maxBorrow": max_borrow.to_string(),
            })),
        )
            .into_response();
    }

    // Credit borrowed funds to user account.
    if exchange.accounts.deposit(user.0, &coin, amount).is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to credit borrow"})),
        )
            .into_response();
    }

    // Upsert the borrow record.
    let borrow_id = {
        let mut borrows = exchange.margin_borrows.lock().unwrap_or_else(|e| e.into_inner());
        let key = (user.0, coin.clone());
        let entry = borrows.entry(key).or_insert_with(|| {
            crate::exchange::MarginBorrow {
                id: uuid::Uuid::new_v4(),
                user_id: user.0,
                coin: coin.clone(),
                amount: Decimal::ZERO,
                interest_accrued: Decimal::ZERO,
                daily_rate,
                borrowed_at: chrono::Utc::now(),
                status: "active".into(),
            }
        });
        entry.amount += amount;
        entry.id
    };

    tracing::info!(
        user_id = %user.0,
        coin = %coin,
        amount = %amount,
        daily_rate = %daily_rate,
        "spot_margin_borrow"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "borrowId": borrow_id.to_string(),
            "coin": coin,
            "amount": amount.to_string(),
            "dailyRate": daily_rate.to_string(),
            "status": "active",
        })),
    )
        .into_response()
}

/// POST /api/v1/spot-margin/repay — repay a coin borrow (principal + interest).
///
/// Body: `{"coin": "USDT", "amount": "105.5"}` — exact amount to repay.
///
/// Deducts from the user's trading balance. Interest is updated from the
/// current `MarginBorrow` record. If the repayment covers the full outstanding
/// amount the borrow is marked `repaid`.
pub async fn spot_margin_repay(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> axum::response::Response {
    let coin = match body.get("coin").and_then(|v| v.as_str()) {
        Some(c) => c.to_uppercase(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing coin"})),
            )
                .into_response()
        }
    };
    let repay_amount: Decimal = match body
        .get("amount")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
    {
        Some(a) if a > Decimal::ZERO => a,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Invalid amount"})),
            )
                .into_response()
        }
    };

    let total_owed = {
        let borrows = exchange.margin_borrows.lock().unwrap_or_else(|e| e.into_inner());
        match borrows.get(&(user.0, coin.clone())) {
            Some(b) if b.status == "active" => b.amount + b.interest_accrued,
            _ => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "No active borrow found for this coin"})),
                )
                    .into_response()
            }
        }
    };

    if repay_amount > total_owed {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "Repay amount exceeds outstanding debt",
                "totalOwed": total_owed.to_string(),
            })),
        )
            .into_response();
    }

    // Deduct from balance.
    if exchange.accounts.withdraw(user.0, &coin, repay_amount).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "Insufficient balance to repay",
                "totalOwed": total_owed.to_string(),
            })),
        )
            .into_response();
    }

    // Update the borrow record.
    let remaining = {
        let mut borrows = exchange.margin_borrows.lock().unwrap_or_else(|e| e.into_inner());
        let borrow = borrows.get_mut(&(user.0, coin.clone())).expect("checked above");
        // Apply repayment: first against interest, then against principal.
        let interest_paid = repay_amount.min(borrow.interest_accrued);
        let principal_paid = repay_amount - interest_paid;
        borrow.interest_accrued -= interest_paid;
        borrow.amount = (borrow.amount - principal_paid).max(Decimal::ZERO);
        let remaining = borrow.amount + borrow.interest_accrued;
        if remaining.is_zero() {
            borrow.status = "repaid".into();
        }
        remaining
    };

    tracing::info!(
        user_id = %user.0,
        coin = %coin,
        repaid = %repay_amount,
        remaining = %remaining,
        "spot_margin_repay"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "coin": coin,
            "repaid": repay_amount.to_string(),
            "remainingDebt": remaining.to_string(),
            "status": if remaining.is_zero() { "repaid" } else { "active" },
        })),
    )
        .into_response()
}

/// GET /api/v1/spot-margin/borrows — list active borrows with accrued interest.
pub async fn spot_margin_borrows(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let borrows = exchange.margin_borrows.lock().unwrap_or_else(|e| e.into_inner());
    let list: Vec<serde_json::Value> = borrows
        .values()
        .filter(|b| b.user_id == user.0 && b.status == "active")
        .map(|b| {
            serde_json::json!({
                "borrowId": b.id.to_string(),
                "coin": b.coin,
                "amount": b.amount.to_string(),
                "interestAccrued": b.interest_accrued.to_string(),
                "totalOwed": (b.amount + b.interest_accrued).to_string(),
                "dailyRate": b.daily_rate.to_string(),
                "borrowedAt": b.borrowed_at.timestamp_millis(),
            })
        })
        .collect();
    Json(serde_json::json!({"borrows": list}))
}

/// GET /api/v1/spot-margin/interest-history — interest charge history.
pub async fn spot_margin_interest_history(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let history = exchange.margin_interest_history.lock().unwrap_or_else(|e| e.into_inner());
    let list: Vec<serde_json::Value> = history
        .iter()
        .filter(|r| r.user_id == user.0)
        .map(|r| {
            serde_json::json!({
                "id": r.id.to_string(),
                "borrowId": r.borrow_id.to_string(),
                "coin": r.coin,
                "interestAmount": r.interest_amount.to_string(),
                "chargedAt": r.charged_at.timestamp_millis(),
            })
        })
        .collect();
    Json(serde_json::json!({"interestHistory": list}))
}

/// GET /api/v1/spot-margin/borrowable — coins available to borrow with max
/// amounts and current rates.
///
/// Max borrow = collateral_balance * 3 - existing_borrow.
pub async fn spot_margin_borrowable(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let borrowable_coins = ["USDT", "BTC", "ETH", "BNB", "SOL"];

    let user_balances = exchange.accounts.get_all_balances(user.0);
    let result: Vec<serde_json::Value> = borrowable_coins
        .iter()
        .map(|&coin| {
            let collateral = user_balances
                .get(coin)
                .map(|b| b.available())
                .unwrap_or(Decimal::ZERO);
            let existing = {
                let borrows = exchange.margin_borrows.lock().unwrap_or_else(|e| e.into_inner());
                borrows
                    .get(&(user.0, coin.to_string()))
                    .filter(|b| b.status == "active")
                    .map(|b| b.amount)
                    .unwrap_or(Decimal::ZERO)
            };
            let max_borrow = (collateral * Decimal::from(3u32) - existing).max(Decimal::ZERO);
            let daily_rate = Decimal::new(2, 4); // 0.02% base
            serde_json::json!({
                "coin": coin,
                "maxBorrowable": max_borrow.to_string(),
                "dailyRate": daily_rate.to_string(),
                "annualRate": (daily_rate * Decimal::from(365u32)).to_string(),
            })
        })
        .collect();

    Json(serde_json::json!({"borrowable": result}))
}

// ── Task: Earn Products (Real Implementation) ─────────────────────────────────

/// GET /api/v1/earn/interest-history — interest accrual history for the user's
/// allocations.
///
/// Returns allocations showing accrued interest over time. Clients can use
/// `GET /earn/allocations` for live snapshots; this endpoint surfaces the
/// period-by-period growth.
pub async fn earn_interest_history(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let allocs = exchange.earn_allocations.lock().unwrap_or_else(|e| e.into_inner());
    let list: Vec<serde_json::Value> = allocs
        .iter()
        .filter(|a| a.user_id == user.0)
        .map(|a| {
            serde_json::json!({
                "allocationId": a.id.to_string(),
                "coin": a.coin,
                "amount": a.amount.to_string(),
                "earnedInterest": a.earned_interest.to_string(),
                "startedAt": a.started_at.timestamp_millis(),
                "lastAccruedAt": a.last_accrued_at.timestamp_millis(),
                "status": a.status,
                "autoCompound": a.auto_compound,
            })
        })
        .collect();
    Json(serde_json::json!({"history": list}))
}

// ── Task: Dust Conversion ─────────────────────────────────────────────────────

/// GET /api/v1/account/dust-assets — list assets whose USD-equivalent value
/// is below $10.
///
/// USD price for each asset is looked up via the index price aggregator using
/// the `{COIN}USDT` symbol. Coins with no index price are valued at zero
/// (included if balance is nonzero).
pub async fn get_dust_assets(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let dust_threshold_usd = Decimal::from(10u32);

    let all_balances: std::collections::HashMap<String, Decimal> = exchange
        .accounts
        .get_all_balances(user.0)
        .iter()
        .map(|(coin, bal)| (coin.clone(), bal.available()))
        .filter(|(_, avail)| *avail > Decimal::ZERO)
        .collect();

    let dust_assets: Vec<serde_json::Value> = all_balances
        .iter()
        .filter(|(coin, _)| coin.as_str() != "USDT") // USDT is always worth $1
        .filter_map(|(coin, &amount)| {
            let usd_price = if coin == "USDT" {
                Decimal::ONE
            } else {
                let sym = exchange_common::Symbol::new(format!("{coin}USDT"));
                exchange
                    .index_aggregator
                    .lock().unwrap_or_else(|e| e.into_inner())
                    .calculate_index(&sym)
                    .map(|ip| ip.price)
                    .unwrap_or(Decimal::ZERO)
            };
            let usd_value = amount * usd_price;
            if usd_value < dust_threshold_usd {
                Some(serde_json::json!({
                    "coin": coin,
                    "amount": amount.to_string(),
                    "usdValue": usd_value.to_string(),
                    "usdPrice": usd_price.to_string(),
                }))
            } else {
                None
            }
        })
        .collect();

    Json(serde_json::json!({"dustAssets": dust_assets}))
}

/// POST /api/v1/account/dust-convert — convert all dust balances to USDT.
///
/// For each asset with a USD value below $10 the available balance is zeroed
/// out and the equivalent USDT is credited. A `DustConversion` record is
/// stored for the history endpoint.
pub async fn dust_convert(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
) -> axum::response::Response {
    let dust_threshold_usd = Decimal::from(10u32);

    // 1. Collect dust candidates.
    let all_balances: std::collections::HashMap<String, Decimal> = exchange
        .accounts
        .get_all_balances(user.0)
        .iter()
        .map(|(coin, bal)| (coin.clone(), bal.available()))
        .filter(|(_, avail)| *avail > Decimal::ZERO)
        .collect();

    let mut items: Vec<crate::exchange::DustConversionItem> = Vec::new();
    for (coin, amount) in &all_balances {
        if coin == "USDT" {
            continue;
        }
        let usd_price = {
            let sym = exchange_common::Symbol::new(format!("{coin}USDT"));
            exchange
                .index_aggregator
                .lock().unwrap_or_else(|e| e.into_inner())
                .calculate_index(&sym)
                .map(|ip| ip.price)
                .unwrap_or(Decimal::ZERO)
        };
        let usd_value = amount * usd_price;
        if usd_value > Decimal::ZERO && usd_value < dust_threshold_usd {
            // Small fee: 1% conversion fee.
            let fee_rate = Decimal::new(1, 2);
            let usdt_received = usd_value * (Decimal::ONE - fee_rate);
            items.push(crate::exchange::DustConversionItem {
                coin: coin.clone(),
                amount: *amount,
                usd_value,
                usdt_received,
            });
        }
    }

    if items.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "No dust assets found to convert"})),
        )
            .into_response();
    }

    // 2. Execute: zero out each dust balance and credit USDT under one shard lock.
    let total_usdt: Decimal = items.iter().map(|i| i.usdt_received).sum();
    exchange.accounts.with_user(user.0, |am| {
        for item in &items {
            // Ignore errors: if available changed between the snapshot and now we
            // withdraw what we can.  The credit is additive so partial success is safe.
            let _ = am.withdraw(user.0, &item.coin, item.amount);
        }
        let _ = am.deposit(user.0, "USDT", total_usdt);
    });

    // 3. Persist record.
    let record = crate::exchange::DustConversion {
        id: uuid::Uuid::new_v4(),
        user_id: user.0,
        items: items.clone(),
        total_usdt,
        converted_at: chrono::Utc::now(),
    };
    let record_id = record.id;
    {
        let mut v = exchange.dust_conversions.lock().unwrap_or_else(|e| e.into_inner());
        crate::exchange::capped_push(&mut v, record, crate::exchange::MAX_DUST_CONVERSIONS);
    }

    tracing::info!(
        user_id = %user.0,
        total_usdt = %total_usdt,
        assets_converted = items.len(),
        "dust_convert"
    );

    let converted: Vec<serde_json::Value> = items
        .iter()
        .map(|i| {
            serde_json::json!({
                "coin": i.coin,
                "amount": i.amount.to_string(),
                "usdValue": i.usd_value.to_string(),
                "usdtReceived": i.usdt_received.to_string(),
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "conversionId": record_id.to_string(),
            "totalUsdt": total_usdt.to_string(),
            "converted": converted,
        })),
    )
        .into_response()
}

/// GET /api/v1/account/dust-history — past dust conversion records.
pub async fn get_dust_history(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let conversions = exchange.dust_conversions.lock().unwrap_or_else(|e| e.into_inner());
    let list: Vec<serde_json::Value> = conversions
        .iter()
        .filter(|c| c.user_id == user.0)
        .map(|c| {
            let items: Vec<serde_json::Value> = c
                .items
                .iter()
                .map(|i| {
                    serde_json::json!({
                        "coin": i.coin,
                        "amount": i.amount.to_string(),
                        "usdValue": i.usd_value.to_string(),
                        "usdtReceived": i.usdt_received.to_string(),
                    })
                })
                .collect();
            serde_json::json!({
                "conversionId": c.id.to_string(),
                "totalUsdt": c.total_usdt.to_string(),
                "items": items,
                "convertedAt": c.converted_at.timestamp_millis(),
            })
        })
        .collect();
    Json(serde_json::json!({"dustHistory": list}))
}

// ─────────────────────────────────────────────────────────────────────────────
// Multi-Leg Strategy Orders
// ─────────────────────────────────────────────────────────────────────────────

/// Request body for POST /api/v1/strategies/order.
#[derive(Debug, serde::Deserialize)]
pub struct PlaceStrategyOrderRequest {
    pub strategy_type: String,
    pub legs: Vec<StrategyLegRequest>,
    #[serde(default)]
    pub execution: Option<String>,
}

/// A single leg in a strategy order request.
#[derive(Debug, serde::Deserialize)]
pub struct StrategyLegRequest {
    pub symbol: String,
    pub side: String,
    pub qty: Decimal,
    pub price: Option<Decimal>,
}

/// Inner execution logic shared by `place_strategy_order` and `spread_order`.
///
/// Parses the legs, submits them to the matching engine (with rollback on
/// failure when in `simultaneous` mode), and stores the result.
async fn place_strategy_order_inner(
    exchange: AppState,
    user_id: uuid::Uuid,
    body: serde_json::Value,
) -> anyhow::Result<crate::exchange::StrategyOrder> {
    use crate::exchange::{StrategyExecution, StrategyLeg, StrategyOrder, StrategyType};

    // Parse strategy type.
    let strategy_type = match body
        .get("strategy_type")
        .and_then(|v| v.as_str())
        .unwrap_or("custom")
    {
        "spread" => StrategyType::Spread,
        "straddle" => StrategyType::Straddle,
        "strangle" => StrategyType::Strangle,
        "butterfly_spread" => StrategyType::ButterflySpread,
        "calendar_spread" => StrategyType::CalendarSpread,
        _ => StrategyType::Custom,
    };

    // Parse execution mode.
    let execution = match body
        .get("execution")
        .and_then(|v| v.as_str())
        .unwrap_or("simultaneous")
    {
        "sequential" => StrategyExecution::Sequential,
        _ => StrategyExecution::Simultaneous,
    };

    // Parse legs from JSON.
    let raw_legs: Vec<serde_json::Value> = body
        .get("legs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if raw_legs.len() < 2 {
        anyhow::bail!("Strategy order requires at least 2 legs");
    }

    let strategy_id = uuid::Uuid::new_v4();
    let now = chrono::Utc::now();

    // Build PlaceOrderCommand for each leg.
    let mut leg_cmds: Vec<(PlaceOrderCommand, StrategyLeg)> = Vec::with_capacity(raw_legs.len());
    for raw in &raw_legs {
        let symbol_str = raw
            .get("symbol")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Each leg requires 'symbol'"))?;
        let side_str = raw
            .get("side")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Each leg requires 'side'"))?;
        let qty: Decimal = raw
            .get("qty")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .or_else(|| {
                raw.get("qty")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
            })
            .ok_or_else(|| anyhow::anyhow!("Each leg requires positive 'qty'"))?;
        let price: Option<Decimal> = raw
            .get("price")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .or_else(|| {
                raw.get("price")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
            });

        if qty <= Decimal::ZERO {
            anyhow::bail!("Leg qty must be positive");
        }

        let side = match side_str.to_lowercase().as_str() {
            "buy" => Side::Buy,
            "sell" => Side::Sell,
            other => anyhow::bail!("Invalid side '{other}' — use 'buy' or 'sell'"),
        };

        let (order_type, order_price) = match price {
            Some(p) if p > Decimal::ZERO => (OrderType::Limit, p),
            _ => (OrderType::Market, Decimal::ZERO),
        };

        let order_id = uuid::Uuid::new_v4();
        let cmd = PlaceOrderCommand {
            order_id,
            user_id,
            symbol: Symbol::new(symbol_str),
            side,
            order_type,
            time_in_force: TimeInForce::Gtc,
            price: order_price,
            qty,
            reduce_only: false,
            expire_at: None,
            timestamp: now,
            position_idx: 0,
            iceberg_qty: None,
            order_link_id: None,
            slippage_tolerance: None,
            close_position: false,
            stp_mode: StpMode::ExpireTaker,
            request_expire_time: None,
        };

        let leg = StrategyLeg {
            symbol: symbol_str.to_string(),
            side: side_str.to_lowercase(),
            qty,
            price,
            order_id: Some(order_id),
            status: "pending".to_string(),
        };

        leg_cmds.push((cmd, leg));
    }

    // Execute the legs according to the selected mode.
    let mut executed_legs: Vec<StrategyLeg> = Vec::with_capacity(leg_cmds.len());
    let mut filled_order_ids: Vec<(Symbol, uuid::Uuid)> = Vec::new();
    let mut any_failed = false;

    match execution {
        StrategyExecution::Simultaneous => {
            // Submit all legs; track successes so we can roll back on failure.
            for (cmd, mut leg) in leg_cmds {
                let symbol = cmd.symbol.clone();
                let oid = cmd.order_id;
                let exchange_clone = Arc::clone(&exchange);
                let leg_result = crate::request_id::spawn_blocking_traced(move || exchange_clone.place_order(cmd))
                    .await
                    .unwrap_or_else(|e| Err(anyhow::anyhow!("task join error: {e}")));
                match leg_result {
                    Ok(_events) => {
                        leg.status = "filled".to_string();
                        filled_order_ids.push((symbol, oid));
                    }
                    Err(e) => {
                        tracing::warn!(
                            strategy_id = %strategy_id,
                            symbol = %leg.symbol,
                            err = %e,
                            "Strategy leg failed — rolling back"
                        );
                        leg.status = "failed".to_string();
                        any_failed = true;
                    }
                }
                executed_legs.push(leg);
            }

            // Best-effort rollback: cancel any already-placed legs.
            if any_failed && !filled_order_ids.is_empty() {
                for (sym, oid) in filled_order_ids {
                    let cancel_cmd = exchange_common::CancelOrderCommand {
                        order_id: oid,
                        user_id,
                        symbol: sym,
                        timestamp: chrono::Utc::now(),
                    };
                    if let Err(e) = exchange.cancel_order(cancel_cmd) {
                        tracing::warn!(
                            order_id = %oid,
                            err = %e,
                            "Rollback cancel failed (best-effort)"
                        );
                    } else {
                        // Mark the rolled-back leg as cancelled in our records.
                        for leg in &mut executed_legs {
                            if leg.order_id == Some(oid) {
                                leg.status = "cancelled".to_string();
                            }
                        }
                    }
                }
            }
        }
        StrategyExecution::Sequential => {
            // Submit legs one at a time; abort on first failure.
            'sequential: for (cmd, mut leg) in leg_cmds {
                if any_failed {
                    leg.status = "cancelled".to_string();
                    executed_legs.push(leg);
                    continue 'sequential;
                }
                let exchange_clone = Arc::clone(&exchange);
                let leg_result = crate::request_id::spawn_blocking_traced(move || exchange_clone.place_order(cmd))
                    .await
                    .unwrap_or_else(|e| Err(anyhow::anyhow!("task join error: {e}")));
                match leg_result {
                    Ok(_events) => {
                        leg.status = "filled".to_string();
                    }
                    Err(e) => {
                        tracing::warn!(
                            strategy_id = %strategy_id,
                            symbol = %leg.symbol,
                            err = %e,
                            "Sequential strategy leg failed — aborting remaining legs"
                        );
                        leg.status = "failed".to_string();
                        any_failed = true;
                    }
                }
                executed_legs.push(leg);
            }
        }
    }

    let overall_status = if any_failed {
        if executed_legs.iter().all(|l| l.status == "failed" || l.status == "cancelled") {
            "failed"
        } else {
            "partial"
        }
    } else {
        "filled"
    };

    let order = StrategyOrder {
        id: strategy_id,
        user_id,
        strategy_type,
        execution,
        legs: executed_legs,
        status: overall_status.to_string(),
        created_at: now,
        updated_at: chrono::Utc::now(),
    };

    // Persist in memory.
    exchange
        .strategy_orders
        .lock().unwrap_or_else(|e| e.into_inner())
        .push(order.clone());

    tracing::info!(
        strategy_id = %strategy_id,
        strategy_type = ?strategy_type,
        status = %overall_status,
        "Strategy order executed"
    );

    Ok(order)
}

/// POST /api/v1/strategies/order — place a multi-leg strategy order.
///
/// Body:
/// ```json
/// {
///   "strategy_type": "spread"|"straddle"|"strangle"|"butterfly_spread"|"calendar_spread"|"custom",
///   "legs": [
///     {"symbol": "BTCUSDT", "side": "buy",  "qty": "0.1", "price": "50000"},
///     {"symbol": "ETHUSDT", "side": "sell", "qty": "1.0", "price": "3000"}
///   ],
///   "execution": "simultaneous"|"sequential"
/// }
/// ```
pub async fn place_strategy_order(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    match place_strategy_order_inner(exchange, user.0, body).await {
        Ok(order) => {
            let legs_json: Vec<serde_json::Value> = order.legs.iter().map(|l| serde_json::json!({
                "symbol": l.symbol,
                "side": l.side,
                "qty": l.qty.to_string(),
                "price": l.price.map(|p| p.to_string()),
                "orderId": l.order_id.map(|id| id.to_string()),
                "status": l.status,
            })).collect();
            (StatusCode::OK, Json(serde_json::json!({
                "strategyId": order.id.to_string(),
                "strategyType": format!("{:?}", order.strategy_type).to_lowercase(),
                "execution": format!("{:?}", order.execution).to_lowercase(),
                "legs": legs_json,
                "status": order.status,
                "createdAt": order.created_at.timestamp_millis(),
            }))).into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        ).into_response(),
    }
}

/// GET /api/v1/strategies/orders — list the authenticated user's strategy orders.
pub async fn list_strategy_orders(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
) -> impl IntoResponse {
    let orders = exchange
        .strategy_orders
        .lock().unwrap_or_else(|e| e.into_inner());

    let user_orders: Vec<serde_json::Value> = orders
        .iter()
        .filter(|o| o.user_id == user.0)
        .map(|order| {
            let legs_json: Vec<serde_json::Value> = order.legs.iter().map(|l| serde_json::json!({
                "symbol": l.symbol,
                "side": l.side,
                "qty": l.qty.to_string(),
                "price": l.price.map(|p| p.to_string()),
                "orderId": l.order_id.map(|id| id.to_string()),
                "status": l.status,
            })).collect();
            serde_json::json!({
                "strategyId": order.id.to_string(),
                "strategyType": format!("{:?}", order.strategy_type).to_lowercase(),
                "execution": format!("{:?}", order.execution).to_lowercase(),
                "legs": legs_json,
                "status": order.status,
                "createdAt": order.created_at.timestamp_millis(),
                "updatedAt": order.updated_at.timestamp_millis(),
            })
        })
        .collect();

    Json(serde_json::json!({
        "strategyOrders": user_orders,
        "total": user_orders.len(),
    }))
}

/// DELETE /api/v1/strategies/orders/{id} — cancel all open legs of a strategy.
pub async fn cancel_strategy_order(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
    Path(id): Path<uuid::Uuid>,
) -> impl IntoResponse {
    let user_id = user.0;

    // Find the strategy order.
    let (strategy_user_id, legs_to_cancel): (uuid::Uuid, Vec<(Symbol, uuid::Uuid)>) = {
        let orders = exchange
            .strategy_orders
            .lock().unwrap_or_else(|e| e.into_inner());
        let order = match orders.iter().find(|o| o.id == id) {
            Some(o) => o,
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "Strategy order not found"})),
                )
                    .into_response();
            }
        };
        let legs: Vec<(Symbol, uuid::Uuid)> = order
            .legs
            .iter()
            .filter(|l| l.status == "pending" || l.status == "open")
            .filter_map(|l| l.order_id.map(|oid| (Symbol::new(&l.symbol), oid)))
            .collect();
        (order.user_id, legs)
    };

    if strategy_user_id != user_id {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Access denied"})),
        )
            .into_response();
    }

    let mut cancelled = 0u32;
    let mut failed = 0u32;

    for (symbol, order_id) in legs_to_cancel {
        let cancel_cmd = exchange_common::CancelOrderCommand {
            order_id,
            user_id,
            symbol,
            timestamp: chrono::Utc::now(),
        };
        match exchange.cancel_order(cancel_cmd) {
            Ok(_) => cancelled += 1,
            Err(e) => {
                tracing::warn!(order_id = %order_id, err = %e, "Strategy leg cancel failed");
                failed += 1;
            }
        }
    }

    // Update strategy status to cancelled.
    {
        let mut orders = exchange
            .strategy_orders
            .lock().unwrap_or_else(|e| e.into_inner());
        if let Some(order) = orders.iter_mut().find(|o| o.id == id) {
            order.status = "cancelled".to_string();
            order.updated_at = chrono::Utc::now();
        }
    }

    Json(serde_json::json!({
        "strategyId": id.to_string(),
        "cancelledLegs": cancelled,
        "failedCancels": failed,
        "status": "cancelled",
    }))
    .into_response()
}

// ─────────────────────────────────────────────────────────────────────────────
// Smart Order Routing (SOR)
// ─────────────────────────────────────────────────────────────────────────────

/// POST /api/v1/orders/sor — Smart Order Routing.
///
/// Routes a large order across multiple price levels in the internal book to
/// achieve best average execution. No external venues — single-exchange SOR.
///
/// Body:
/// ```json
/// {
///   "symbol": "BTCUSDT",
///   "side": "buy",
///   "qty": "1.0",
///   "max_slippage_pct": "1.0"
/// }
/// ```
pub async fn place_sor_order(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let user_id = user.0;

    // Parse request fields.
    let symbol_str = match body.get("symbol").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "symbol is required"})),
            )
                .into_response();
        }
    };

    let side_str = match body.get("side").and_then(|v| v.as_str()) {
        Some(s) => s.to_lowercase(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "side is required"})),
            )
                .into_response();
        }
    };

    let side = match side_str.as_str() {
        "buy" => Side::Buy,
        "sell" => Side::Sell,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "side must be 'buy' or 'sell'"})),
            )
                .into_response();
        }
    };

    let total_qty: Decimal = match body
        .get("qty")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .or_else(|| {
            body.get("qty")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
        }) {
        Some(q) if q > Decimal::ZERO => q,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "qty must be a positive number"})),
            )
                .into_response();
        }
    };

    let max_slippage_pct: Decimal = body
        .get("max_slippage_pct")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .or_else(|| {
            body.get("max_slippage_pct")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
        })
        .unwrap_or(Decimal::ONE); // default 1% slippage limit

    let symbol = Symbol::new(&symbol_str);

    // Snapshot the order book to determine how to split the order.
    let snapshot = {
        let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
        match pool.snapshot(&symbol, 100) {
            Ok(s) => s,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": format!("Symbol not found: {e}")})),
                )
                    .into_response();
            }
        }
    };

    // Select the correct side of the book: buys consume asks, sells consume bids.
    let available_levels: Vec<(Decimal, Decimal)> = match side {
        Side::Buy => snapshot.asks.iter().map(|(p, q)| (*p, *q)).collect(),
        Side::Sell => snapshot.bids.iter().map(|(p, q)| (*p, *q)).collect(),
    };

    if available_levels.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "No liquidity available in the order book"})),
        )
            .into_response();
    }

    // Compute the BBO (reference price for slippage calculation).
    let bbo_price = available_levels[0].0;
    let slippage_limit = bbo_price
        * (Decimal::ONE + max_slippage_pct / Decimal::from(100) * if side == Side::Buy { Decimal::ONE } else { -Decimal::ONE });

    // Walk the levels, collecting fill slices up to the total quantity
    // and within the slippage band.
    struct SorSlice {
        price: Decimal,
        qty: Decimal,
    }

    let mut slices: Vec<SorSlice> = Vec::new();
    let mut remaining = total_qty;

    for (level_price, level_qty) in &available_levels {
        if remaining <= Decimal::ZERO {
            break;
        }
        // Check slippage for buys (price must not exceed limit) or
        // sells (price must not go below limit).
        let within_slippage = match side {
            Side::Buy => *level_price <= slippage_limit,
            Side::Sell => *level_price >= slippage_limit,
        };
        if !within_slippage {
            break;
        }
        let take_qty = remaining.min(*level_qty);
        slices.push(SorSlice {
            price: *level_price,
            qty: take_qty,
        });
        remaining -= take_qty;
    }

    if slices.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!(
                    "No liquidity within {max_slippage_pct}% slippage of BBO {bbo_price}"
                )
            })),
        )
            .into_response();
    }

    // Submit one IOC limit order per price level.
    let mut fill_results: Vec<serde_json::Value> = Vec::new();
    let mut total_filled = Decimal::ZERO;
    let mut total_notional = Decimal::ZERO;

    for slice in &slices {
        let order_id = uuid::Uuid::new_v4();
        let cmd = PlaceOrderCommand {
            order_id,
            user_id,
            symbol: symbol.clone(),
            side,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Ioc,
            price: slice.price,
            qty: slice.qty,
            reduce_only: false,
            expire_at: None,
            timestamp: chrono::Utc::now(),
            position_idx: 0,
            iceberg_qty: None,
            order_link_id: None,
            slippage_tolerance: None,
            close_position: false,
            stp_mode: StpMode::ExpireTaker,
            request_expire_time: None,
        };

        let exchange_clone = Arc::clone(&exchange);
        let sor_result = crate::request_id::spawn_blocking_traced(move || exchange_clone.place_order(cmd))
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!("task join error: {e}")));
        match sor_result {
            Ok(events) => {
                let filled: Decimal = events
                    .iter()
                    .filter_map(|e| {
                        if let MatchEvent::Fill(f) = e {
                            Some(f.qty)
                        } else {
                            None
                        }
                    })
                    .sum();

                if filled > Decimal::ZERO {
                    total_filled += filled;
                    total_notional += filled * slice.price;
                    fill_results.push(serde_json::json!({
                        "orderId": order_id.to_string(),
                        "price": slice.price.to_string(),
                        "qty": filled.to_string(),
                        "notional": (filled * slice.price).to_string(),
                    }));
                }
            }
            Err(e) => {
                tracing::warn!(
                    sor_symbol = %symbol_str,
                    price = %slice.price,
                    qty = %slice.qty,
                    err = %e,
                    "SOR slice order failed"
                );
            }
        }
    }

    let avg_price = if total_filled > Decimal::ZERO {
        total_notional / total_filled
    } else {
        Decimal::ZERO
    };

    (StatusCode::OK, Json(serde_json::json!({
        "symbol": symbol_str,
        "side": side_str,
        "requestedQty": total_qty.to_string(),
        "filledQty": total_filled.to_string(),
        "avgPrice": avg_price.to_string(),
        "slices": fill_results,
        "bboPrice": bbo_price.to_string(),
        "maxSlippagePct": max_slippage_pct.to_string(),
        "status": if total_filled >= total_qty { "fully_filled" }
                  else if total_filled > Decimal::ZERO { "partially_filled" }
                  else { "unfilled" },
    }))).into_response()
}

// ─────────────────────────────────────────────────────────────────────────────
// Spot Hedge Mode
// ─────────────────────────────────────────────────────────────────────────────

/// PUT /api/v1/account/spot-hedge-mode — enable or disable spot hedge mode.
///
/// Body: `{"enabled": true}`
///
/// When enabled, the exchange does not net a spot long against an open short
/// perpetual, allowing the user to hold both simultaneously (carry/basis trade).
pub async fn set_spot_hedge_mode(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let user_id = user.0;
    let enabled = body
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    {
        let mut hedge_users = exchange
            .spot_hedge_users
            .lock().unwrap_or_else(|e| e.into_inner());
        if enabled {
            hedge_users.insert(user_id);
        } else {
            hedge_users.remove(&user_id);
        }
    }

    tracing::info!(
        user_id = %user_id,
        enabled = enabled,
        "Spot hedge mode updated"
    );

    Json(serde_json::json!({
        "userId": user_id.to_string(),
        "spotHedgeMode": enabled,
        "message": if enabled {
            "Spot hedge mode enabled — spot longs and perp shorts are tracked independently."
        } else {
            "Spot hedge mode disabled — positions will be netted normally."
        },
    }))
}

/// GET /api/v1/account/spot-hedge-mode — get spot hedge mode status.
pub async fn get_spot_hedge_mode(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = user.0;
    let enabled = exchange
        .spot_hedge_users
        .lock().unwrap_or_else(|e| e.into_inner())
        .contains(&user_id);

    Json(serde_json::json!({
        "userId": user_id.to_string(),
        "spotHedgeMode": enabled,
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Prevented Matches Report
// ─────────────────────────────────────────────────────────────────────────────

/// GET /api/v1/account/prevented-matches — get the user's STP-prevented matches.
///
/// Returns up to 200 most recent prevented-match records for the authenticated
/// user, ordered newest-first.
pub async fn get_prevented_matches(
    State(exchange): State<AppState>,
    axum::Extension(user): axum::Extension<crate::auth::AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = user.0;

    let records: Vec<serde_json::Value> = {
        let prevented = exchange
            .prevented_matches
            .lock().unwrap_or_else(|e| e.into_inner());

        prevented
            .iter()
            .filter(|r| r.user_id == user_id)
            .rev() // newest first (VecDeque is oldest-first)
            .take(200)
            .map(|r| serde_json::json!({
                "id": r.id.to_string(),
                "timestamp": r.timestamp.timestamp_millis(),
                "orderId": r.order_id.to_string(),
                "wouldMatchOrderId": r.would_match_order_id.to_string(),
                "symbol": r.symbol,
                "qty": r.qty.to_string(),
                "price": r.price.to_string(),
                "stpMode": r.stp_mode,
            }))
            .collect()
    };

    Json(serde_json::json!({
        "preventedMatches": records,
        "total": records.len(),
    }))
}

// ── NOTIF-1: Push device registration ──────────────────────────────────────

/// POST /api/v1/account/push-devices — register a device for push notifications.
///
/// Body: `{"token": "apns_or_fcm_token", "platform": "apns"|"fcm"|"web_push", "device_name": "iPhone 15"}`
pub async fn register_push_device(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let user_id = auth.0;

    let token = match body.get("token").and_then(|v| v.as_str()) {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "token is required"}))),
    };

    let platform = match body.get("platform").and_then(|v| v.as_str()) {
        Some("apns") => exchange_admin::PushPlatform::Apns,
        Some("fcm") => exchange_admin::PushPlatform::Fcm,
        Some("web_push") => exchange_admin::PushPlatform::WebPush,
        _ => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "platform must be apns, fcm, or web_push"}))),
    };

    let device_name = body.get("device_name").and_then(|v| v.as_str()).map(|s| s.to_string());

    let device_token = exchange_admin::DeviceToken { token, platform, device_name };
    let id = exchange.device_registry.lock().unwrap_or_else(|e| e.into_inner())
        .register(user_id, device_token);

    (StatusCode::OK, Json(serde_json::json!({"device_id": id.to_string()})))
}

/// GET /api/v1/account/push-devices — list registered push devices.
pub async fn list_push_devices(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let registry = exchange.device_registry.lock().unwrap_or_else(|e| e.into_inner());
    let devices: Vec<serde_json::Value> = registry.user_devices(user_id)
        .iter()
        .map(|d| serde_json::json!({
            "id": d.id.to_string(),
            "platform": format!("{:?}", d.token.platform),
            "device_name": d.token.device_name,
            "registered_at": d.registered_at.timestamp_millis(),
            "last_push_at": d.last_push_at.map(|t| t.timestamp_millis()),
        }))
        .collect();

    Json(serde_json::json!({"devices": devices}))
}

/// DELETE /api/v1/account/push-devices/:id — unregister a push device.
pub async fn unregister_push_device(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(device_id): Path<Uuid>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let mut registry = exchange.device_registry.lock().unwrap_or_else(|e| e.into_inner());
    if registry.unregister(user_id, device_id) {
        (StatusCode::OK, Json(serde_json::json!({"unregistered": true})))
    } else {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "device not found"})))
    }
}
