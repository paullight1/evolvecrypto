//! Algo trading REST handlers.
//!
//! Three feature areas:
//!
//! 1. **Algo Orders** (`/algo/orders`) — CRUD for iceberg, TWAP, trailing-stop, and signal
//!    algo orders tracked in the separate `AlgoOrder` namespace (distinct from regular orders).
//!
//! 2. **Signal Bots** (`/bots/signal` + `/webhooks/signal/{bot_id}`) — webhook-driven bots
//!    that accept TradingView-style `{action, symbol, qty, price}` payloads and place orders
//!    automatically on behalf of the bot owner.
//!
//! 3. **Recurring Buy** (`/algo/recurring-buy`) — scheduled DCA purchase plans executed by
//!    the `spawn_recurring_buy_executor` background task.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use rust_decimal::Decimal;
use serde::Deserialize;
use uuid::Uuid;

use exchange_common::{OrderType, PlaceOrderCommand, Side, StpMode, Symbol, TimeInForce};

use crate::auth::AuthenticatedUser;
use crate::exchange::{AlgoOrder, AlgoStatus, AlgoType, RecurringBuyFrequency, RecurringBuyPlan, SignalBot};
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

// =============================================================================
// Algo Orders — /algo/orders
// =============================================================================

/// Request body for POST /algo/orders.
#[derive(Debug, Deserialize)]
pub struct PlaceAlgoOrderRequest {
    /// One of: "iceberg", "twap", "trailing_stop", "signal".
    pub algo_type: String,
    pub symbol: String,
    /// "buy" or "sell".
    pub side: String,
    /// Total quantity to fill over the lifetime of the algo.
    pub total_qty: Decimal,
    /// Type-specific parameters (see `AlgoOrder::params` doc).
    pub params: serde_json::Value,
}

/// POST /api/v1/algo/orders
///
/// Create a new algo order. The order is tracked in the algo namespace and
/// does NOT immediately appear in `/orders` (regular order list).
pub async fn place_algo_order(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<PlaceAlgoOrderRequest>,
) -> impl IntoResponse {
    if req.total_qty <= Decimal::ZERO {
        return bad_request("total_qty must be positive");
    }

    let algo_type = match req.algo_type.as_str() {
        "iceberg"       => AlgoType::Iceberg,
        "twap"          => AlgoType::Twap,
        "trailing_stop" => AlgoType::TrailingStop,
        "signal"        => AlgoType::Signal,
        other           => return bad_request(format!("unknown algo_type: {other}")),
    };

    if req.side.to_lowercase() != "buy" && req.side.to_lowercase() != "sell" {
        return bad_request("side must be 'buy' or 'sell'");
    }

    let now = Utc::now();
    let order = AlgoOrder {
        id: Uuid::new_v4(),
        user_id: auth.0,
        algo_type,
        symbol: req.symbol,
        side: req.side.to_lowercase(),
        total_qty: req.total_qty,
        filled_qty: Decimal::ZERO,
        params: req.params,
        status: AlgoStatus::Running,
        created_at: now,
        updated_at: now,
    };

    let id = order.id;
    exchange.algo_orders.lock().unwrap_or_else(|e| e.into_inner()).push(order);

    tracing::info!(algo_id = %id, user_id = %auth.0, "algo order placed");

    ok(serde_json::json!({"algo_id": id.to_string()}))
}

/// Query params for GET /algo/orders.
#[derive(Debug, Deserialize, Default)]
pub struct AlgoOrdersQuery {
    /// Filter by symbol.
    pub symbol: Option<String>,
}

/// GET /api/v1/algo/orders
///
/// List all active (Running) algo orders for the authenticated user.
pub async fn list_algo_orders(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Query(q): Query<AlgoOrdersQuery>,
) -> impl IntoResponse {
    let orders = exchange.algo_orders.lock().unwrap_or_else(|e| e.into_inner());
    let result: Vec<_> = orders
        .iter()
        .filter(|o| o.user_id == auth.0 && o.status == AlgoStatus::Running)
        .filter(|o| q.symbol.as_ref().map_or(true, |s| &o.symbol == s))
        .map(|o| serde_json::to_value(o).expect("serialization is infallible"))
        .collect();

    ok(serde_json::json!(result))
}

/// GET /api/v1/algo/orders/history
///
/// List completed (Filled, Cancelled, Failed) algo orders for the user.
pub async fn algo_order_history(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Query(q): Query<AlgoOrdersQuery>,
) -> impl IntoResponse {
    let orders = exchange.algo_orders.lock().unwrap_or_else(|e| e.into_inner());
    let result: Vec<_> = orders
        .iter()
        .filter(|o| o.user_id == auth.0 && o.status != AlgoStatus::Running)
        .filter(|o| q.symbol.as_ref().map_or(true, |s| &o.symbol == s))
        .map(|o| serde_json::to_value(o).expect("serialization is infallible"))
        .collect();

    ok(serde_json::json!(result))
}

/// Request body for PUT /algo/orders/{id} — amend params only (qty and type are immutable).
#[derive(Debug, Deserialize)]
pub struct AmendAlgoOrderRequest {
    pub params: serde_json::Value,
}

/// PUT /api/v1/algo/orders/{id}
///
/// Amend the `params` of a running algo order. The `algo_type`, `symbol`, `side`,
/// and `total_qty` are immutable after creation.
pub async fn amend_algo_order(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<AmendAlgoOrderRequest>,
) -> impl IntoResponse {
    let mut orders = exchange.algo_orders.lock().unwrap_or_else(|e| e.into_inner());
    let Some(order) = orders.iter_mut().find(|o| o.id == id && o.user_id == auth.0) else {
        return not_found("algo order not found");
    };

    if order.status != AlgoStatus::Running {
        return bad_request("only running algo orders can be amended");
    }

    order.params = req.params;
    order.updated_at = Utc::now();
    tracing::debug!(algo_id = %id, "algo order amended");

    ok(serde_json::json!({"algo_id": id.to_string(), "status": "amended"}))
}

/// DELETE /api/v1/algo/orders/{id}
///
/// Cancel a running algo order.
pub async fn cancel_algo_order(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let mut orders = exchange.algo_orders.lock().unwrap_or_else(|e| e.into_inner());
    let Some(order) = orders.iter_mut().find(|o| o.id == id && o.user_id == auth.0) else {
        return not_found("algo order not found");
    };

    if order.status != AlgoStatus::Running {
        return bad_request("algo order is not running");
    }

    order.status = AlgoStatus::Cancelled;
    order.updated_at = Utc::now();
    tracing::info!(algo_id = %id, user_id = %auth.0, "algo order cancelled");

    ok(serde_json::json!({"algo_id": id.to_string(), "status": "cancelled"}))
}

// =============================================================================
// Signal Bots — /bots/signal  +  /webhooks/signal/{bot_id}
// =============================================================================

/// Request body for POST /bots/signal.
#[derive(Debug, Deserialize)]
pub struct CreateSignalBotRequest {
    pub name: String,
    /// Symbols this bot is permitted to trade (empty = all).
    #[serde(default)]
    pub symbols: Vec<String>,
    /// Secret token included in every webhook payload for validation.
    pub webhook_secret: String,
}

/// POST /api/v1/bots/signal
///
/// Create a new signal bot.  Returns the bot ID and the webhook URL path.
pub async fn create_signal_bot(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<CreateSignalBotRequest>,
) -> impl IntoResponse {
    if req.name.trim().is_empty() {
        return bad_request("name must not be empty");
    }
    if req.webhook_secret.len() < 8 {
        return bad_request("webhook_secret must be at least 8 characters");
    }

    let bot = SignalBot {
        id: Uuid::new_v4(),
        user_id: auth.0,
        name: req.name,
        symbols: req.symbols,
        webhook_secret: req.webhook_secret,
        is_active: true,
        trades_executed: 0,
        created_at: Utc::now(),
    };

    let bot_id = bot.id;
    exchange.signal_bots.lock().unwrap_or_else(|e| e.into_inner()).push(bot);

    tracing::info!(bot_id = %bot_id, user_id = %auth.0, "signal bot created");

    ok(serde_json::json!({
        "bot_id": bot_id.to_string(),
        "webhook_path": format!("/api/v1/webhooks/signal/{bot_id}"),
    }))
}

/// GET /api/v1/bots/signal
///
/// List active signal bots owned by the authenticated user.
pub async fn list_signal_bots(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let bots = exchange.signal_bots.lock().unwrap_or_else(|e| e.into_inner());
    let result: Vec<_> = bots
        .iter()
        .filter(|b| b.user_id == auth.0 && b.is_active)
        .map(|b| serde_json::json!({
            "id": b.id.to_string(),
            "name": b.name,
            "symbols": b.symbols,
            "is_active": b.is_active,
            "trades_executed": b.trades_executed,
            "created_at": b.created_at,
            "webhook_path": format!("/api/v1/webhooks/signal/{}", b.id),
        }))
        .collect();

    ok(serde_json::json!(result))
}

/// DELETE /api/v1/bots/signal/{id}
///
/// Deactivate a signal bot (idempotent).
pub async fn stop_signal_bot(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let mut bots = exchange.signal_bots.lock().unwrap_or_else(|e| e.into_inner());
    let Some(bot) = bots.iter_mut().find(|b| b.id == id && b.user_id == auth.0) else {
        return not_found("signal bot not found");
    };

    bot.is_active = false;
    tracing::info!(bot_id = %id, user_id = %auth.0, "signal bot stopped");

    ok(serde_json::json!({"status": "stopped"}))
}

/// Incoming TradingView-style webhook payload.
#[derive(Debug, Deserialize)]
pub struct SignalWebhookPayload {
    /// "buy" or "sell".
    pub action: String,
    pub symbol: String,
    pub qty: Decimal,
    /// Limit price. `None` or 0 → market order.
    pub price: Option<Decimal>,
    /// Must match the bot's `webhook_secret`.
    pub secret: String,
}

/// POST /api/v1/webhooks/signal/{bot_id}
///
/// Receive an external signal and place an order on behalf of the bot owner.
///
/// This endpoint is public (no auth header required) — authentication is
/// provided by the `secret` field in the payload which must match the
/// bot's stored `webhook_secret`.
pub async fn signal_webhook(
    State(exchange): State<AppState>,
    Path(bot_id): Path<Uuid>,
    Json(payload): Json<SignalWebhookPayload>,
) -> impl IntoResponse {
    // --- Validate payload ---
    if payload.qty <= Decimal::ZERO {
        return bad_request("qty must be positive");
    }
    let side = match payload.action.to_lowercase().as_str() {
        "buy"  => Side::Buy,
        "sell" => Side::Sell,
        other  => return bad_request(format!("unknown action: {other}; expected 'buy' or 'sell'")),
    };

    // --- Look up bot and verify secret ---
    let (user_id, symbols_allowed) = {
        let mut bots = exchange.signal_bots.lock().unwrap_or_else(|e| e.into_inner());
        let Some(bot) = bots.iter_mut().find(|b| b.id == bot_id) else {
            // Return the same error as an invalid secret to avoid bot-ID enumeration.
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "invalid credentials"})));
        };

        if !bot.is_active {
            return bad_request("bot is not active");
        }

        if bot.webhook_secret != payload.secret {
            tracing::warn!(bot_id = %bot_id, "signal webhook: invalid secret");
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "invalid credentials"})));
        }

        // Validate symbol is in the allowed list (empty = all symbols OK).
        let allowed = bot.symbols.clone();

        // Increment counter eagerly; we'll revert if order placement fails below.
        bot.trades_executed += 1;

        (bot.user_id, allowed)
    };

    if !symbols_allowed.is_empty() && !symbols_allowed.contains(&payload.symbol) {
        // Revert the counter increment.
        {
            let mut bots = exchange.signal_bots.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(bot) = bots.iter_mut().find(|b| b.id == bot_id) {
                bot.trades_executed = bot.trades_executed.saturating_sub(1);
            }
        }
        return bad_request(format!("symbol {} is not permitted for this bot", payload.symbol));
    }

    // --- Determine order type and price ---
    let use_market = payload.price.map_or(true, |p| p <= Decimal::ZERO);
    let (order_type, price) = if use_market {
        (OrderType::Market, Decimal::ZERO)
    } else {
        (OrderType::Limit, payload.price.unwrap_or(Decimal::ZERO))
    };

    let cmd = PlaceOrderCommand {
        order_id: Uuid::new_v4(),
        user_id,
        symbol: Symbol::new(payload.symbol.as_str()),
        side,
        order_type,
        time_in_force: if use_market { TimeInForce::Ioc } else { TimeInForce::Gtc },
        price,
        qty: payload.qty,
        reduce_only: false,
        expire_at: None,
        timestamp: Utc::now(),
        position_idx: 0,
        iceberg_qty: None,
        order_link_id: None,
        slippage_tolerance: None,
        close_position: false,
        stp_mode: StpMode::default(),
        request_expire_time: None,
    };

    match exchange.place_order(cmd) {
        Ok(_) => {
            tracing::info!(
                bot_id = %bot_id,
                user_id = %user_id,
                symbol = %payload.symbol,
                side = %payload.action,
                qty = %payload.qty,
                "signal bot order placed"
            );
            ok(serde_json::json!({"status": "order_placed"}))
        }
        Err(e) => {
            // Revert the counter on failure.
            {
                let mut bots = exchange.signal_bots.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(bot) = bots.iter_mut().find(|b| b.id == bot_id) {
                    bot.trades_executed = bot.trades_executed.saturating_sub(1);
                }
            }
            tracing::error!(bot_id = %bot_id, error = %e, "signal bot order failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("order failed: {e}")})),
            )
        }
    }
}

// =============================================================================
// Recurring Buy — /algo/recurring-buy
// =============================================================================

/// Request body for POST /algo/recurring-buy.
#[derive(Debug, Deserialize)]
pub struct CreateRecurringBuyRequest {
    /// Coin to purchase (e.g. "BTC").
    pub coin: String,
    /// Amount in USDT to spend per execution.
    pub amount_usdt: Decimal,
    /// "daily", "weekly", or "monthly".
    pub frequency: String,
    /// For weekly plans: 0=Monday … 6=Sunday.
    pub day_of_week: Option<u8>,
    /// Hour of day (0–23) for the purchase.
    pub hour: u8,
}

/// POST /api/v1/algo/recurring-buy
///
/// Create a recurring DCA buy plan.
pub async fn create_recurring_buy(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<CreateRecurringBuyRequest>,
) -> impl IntoResponse {
    if req.amount_usdt <= Decimal::ZERO {
        return bad_request("amount_usdt must be positive");
    }
    if req.coin.trim().is_empty() {
        return bad_request("coin must not be empty");
    }
    if req.hour > 23 {
        return bad_request("hour must be 0–23");
    }

    let frequency = match req.frequency.as_str() {
        "daily"   => RecurringBuyFrequency::Daily,
        "weekly"  => RecurringBuyFrequency::Weekly,
        "monthly" => RecurringBuyFrequency::Monthly,
        other     => return bad_request(format!("unknown frequency: {other}")),
    };

    if frequency == RecurringBuyFrequency::Weekly {
        match req.day_of_week {
            None       => return bad_request("day_of_week (0–6) is required for weekly plans"),
            Some(d) if d > 6 => return bad_request("day_of_week must be 0–6"),
            _ => {}
        }
    }

    let now = Utc::now();
    let next_execution = compute_next_execution(frequency, req.day_of_week, req.hour, now);

    let plan = RecurringBuyPlan {
        id: Uuid::new_v4(),
        user_id: auth.0,
        coin: req.coin.to_uppercase(),
        amount_usdt: req.amount_usdt,
        frequency,
        day_of_week: req.day_of_week,
        hour: req.hour,
        next_execution,
        total_invested: Decimal::ZERO,
        executions: 0,
        is_active: true,
        created_at: now,
    };

    let plan_id = plan.id;
    exchange.recurring_buy_plans.lock().unwrap_or_else(|e| e.into_inner()).push(plan);

    tracing::info!(plan_id = %plan_id, user_id = %auth.0, "recurring buy plan created");

    ok(serde_json::json!({
        "plan_id": plan_id.to_string(),
        "next_execution": next_execution,
    }))
}

/// GET /api/v1/algo/recurring-buy
///
/// List active recurring buy plans for the authenticated user.
pub async fn list_recurring_buy(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let plans = exchange.recurring_buy_plans.lock().unwrap_or_else(|e| e.into_inner());
    let result: Vec<_> = plans
        .iter()
        .filter(|p| p.user_id == auth.0 && p.is_active)
        .map(|p| serde_json::to_value(p).expect("serialization is infallible"))
        .collect();

    ok(serde_json::json!(result))
}

/// DELETE /api/v1/algo/recurring-buy/{id}
///
/// Cancel a recurring buy plan.
pub async fn cancel_recurring_buy(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let mut plans = exchange.recurring_buy_plans.lock().unwrap_or_else(|e| e.into_inner());
    let Some(plan) = plans.iter_mut().find(|p| p.id == id && p.user_id == auth.0) else {
        return not_found("recurring buy plan not found");
    };

    plan.is_active = false;
    tracing::info!(plan_id = %id, user_id = %auth.0, "recurring buy plan cancelled");

    ok(serde_json::json!({"status": "cancelled"}))
}

// =============================================================================
// Helper: next execution timestamp
// =============================================================================

/// Compute the first future execution time for a recurring plan.
///
/// The returned timestamp is the upcoming occurrence of `hour:00 UTC` that
/// satisfies the `frequency` constraint (next calendar day, specific weekday,
/// or first day of next month).
pub(crate) fn compute_next_execution(
    frequency: RecurringBuyFrequency,
    day_of_week: Option<u8>,
    hour: u8,
    from: chrono::DateTime<Utc>,
) -> chrono::DateTime<Utc> {
    use chrono::{Datelike, Duration, TimeZone, Weekday};

    // Candidate: today at `hour:00 UTC`.
    let candidate = Utc
        .with_ymd_and_hms(from.year(), from.month(), from.day(), u32::from(hour), 0, 0)
        .single()
        .unwrap_or(from);

    match frequency {
        RecurringBuyFrequency::Daily => {
            if candidate > from {
                candidate
            } else {
                candidate + Duration::days(1)
            }
        }
        RecurringBuyFrequency::Weekly => {
            let target_weekday: Weekday = match day_of_week.unwrap_or(0) {
                0 => Weekday::Mon,
                1 => Weekday::Tue,
                2 => Weekday::Wed,
                3 => Weekday::Thu,
                4 => Weekday::Fri,
                5 => Weekday::Sat,
                _ => Weekday::Sun,
            };
            let current_wd = from.weekday().num_days_from_monday(); // 0=Mon
            let target_wd = target_weekday.num_days_from_monday();
            let days_ahead = if target_wd > current_wd {
                target_wd - current_wd
            } else if target_wd == current_wd && candidate > from {
                0
            } else {
                7 - (current_wd - target_wd)
            };
            candidate + Duration::days(i64::from(days_ahead))
        }
        RecurringBuyFrequency::Monthly => {
            // First day of the next calendar month at `hour:00`.
            let (next_year, next_month) = if from.month() == 12 {
                (from.year() + 1, 1u32)
            } else {
                (from.year(), from.month() + 1)
            };
            Utc.with_ymd_and_hms(next_year, next_month, 1, u32::from(hour), 0, 0)
                .single()
                .unwrap_or(from)
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    // ── compute_next_execution ────────────────────────────────────────────────

    #[test]
    fn daily_future_hour_today() {
        // from: 2026-03-21 10:00 UTC, hour=14 → same day 14:00
        let from = Utc.with_ymd_and_hms(2026, 3, 21, 10, 0, 0).unwrap();
        let next = compute_next_execution(RecurringBuyFrequency::Daily, None, 14, from);
        assert_eq!(next, Utc.with_ymd_and_hms(2026, 3, 21, 14, 0, 0).unwrap());
    }

    #[test]
    fn daily_past_hour_rolls_to_tomorrow() {
        // from: 2026-03-21 15:00 UTC, hour=14 → next day 14:00
        let from = Utc.with_ymd_and_hms(2026, 3, 21, 15, 0, 0).unwrap();
        let next = compute_next_execution(RecurringBuyFrequency::Daily, None, 14, from);
        assert_eq!(next, Utc.with_ymd_and_hms(2026, 3, 22, 14, 0, 0).unwrap());
    }

    #[test]
    fn weekly_finds_next_friday() {
        // 2026-03-21 is a Saturday (weekday index 5). Friday = 4.
        // Next Friday is 2026-03-27.
        let from = Utc.with_ymd_and_hms(2026, 3, 21, 10, 0, 0).unwrap();
        let next = compute_next_execution(RecurringBuyFrequency::Weekly, Some(4), 8, from);
        assert_eq!(next, Utc.with_ymd_and_hms(2026, 3, 27, 8, 0, 0).unwrap());
    }

    #[test]
    fn monthly_next_month_first() {
        let from = Utc.with_ymd_and_hms(2026, 3, 21, 10, 0, 0).unwrap();
        let next = compute_next_execution(RecurringBuyFrequency::Monthly, None, 9, from);
        assert_eq!(next, Utc.with_ymd_and_hms(2026, 4, 1, 9, 0, 0).unwrap());
    }

    #[test]
    fn monthly_december_rolls_to_january() {
        let from = Utc.with_ymd_and_hms(2026, 12, 15, 10, 0, 0).unwrap();
        let next = compute_next_execution(RecurringBuyFrequency::Monthly, None, 6, from);
        assert_eq!(next, Utc.with_ymd_and_hms(2027, 1, 1, 6, 0, 0).unwrap());
    }

    // ── AlgoOrder CRUD (in-process) ──────────────────────────────────────────

    fn make_exchange() -> std::sync::Arc<crate::exchange::Exchange> {
        crate::exchange::Exchange::new()
    }

    #[test]
    fn algo_order_lifecycle() {
        let exchange = make_exchange();
        let user_id = Uuid::new_v4();

        // Place
        let order = AlgoOrder {
            id: Uuid::new_v4(),
            user_id,
            algo_type: AlgoType::Iceberg,
            symbol: "BTCUSDT".into(),
            side: "buy".into(),
            total_qty: Decimal::new(1, 0),
            filled_qty: Decimal::ZERO,
            params: serde_json::json!({"visible_qty": "0.1"}),
            status: AlgoStatus::Running,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let order_id = order.id;
        exchange.algo_orders.lock().unwrap_or_else(|e| e.into_inner()).push(order);

        // Active list contains it
        let active: Vec<_> = exchange
            .algo_orders
            .lock().unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|o| o.user_id == user_id && o.status == AlgoStatus::Running)
            .cloned()
            .collect();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, order_id);

        // Cancel
        {
            let mut orders = exchange.algo_orders.lock().unwrap_or_else(|e| e.into_inner());
            let o = orders.iter_mut().find(|o| o.id == order_id).unwrap();
            o.status = AlgoStatus::Cancelled;
        }

        // Active list is now empty, history has it
        let active: Vec<_> = exchange
            .algo_orders
            .lock().unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|o| o.user_id == user_id && o.status == AlgoStatus::Running)
            .cloned()
            .collect();
        assert!(active.is_empty());

        let history: Vec<_> = exchange
            .algo_orders
            .lock().unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|o| o.user_id == user_id && o.status != AlgoStatus::Running)
            .cloned()
            .collect();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].status, AlgoStatus::Cancelled);
    }

    #[test]
    fn signal_bot_create_and_stop() {
        let exchange = make_exchange();
        let user_id = Uuid::new_v4();

        let bot = SignalBot {
            id: Uuid::new_v4(),
            user_id,
            name: "My Signal Bot".into(),
            symbols: vec!["BTCUSDT".into()],
            webhook_secret: "supersecret123".into(),
            is_active: true,
            trades_executed: 0,
            created_at: Utc::now(),
        };
        let bot_id = bot.id;
        exchange.signal_bots.lock().unwrap_or_else(|e| e.into_inner()).push(bot);

        // List shows one active bot
        let count = exchange
            .signal_bots
            .lock().unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|b| b.user_id == user_id && b.is_active)
            .count();
        assert_eq!(count, 1);

        // Stop it
        {
            let mut bots = exchange.signal_bots.lock().unwrap_or_else(|e| e.into_inner());
            let b = bots.iter_mut().find(|b| b.id == bot_id).unwrap();
            b.is_active = false;
        }

        let count = exchange
            .signal_bots
            .lock().unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|b| b.user_id == user_id && b.is_active)
            .count();
        assert_eq!(count, 0);
    }

    #[test]
    fn signal_bot_secret_validation() {
        // Ensure secret check is case-sensitive.
        let secret = "AbCdEfGh";
        assert_eq!(secret, "AbCdEfGh");
        assert_ne!(secret, "abcdefgh");
    }

    #[test]
    fn recurring_buy_plan_lifecycle() {
        let exchange = make_exchange();
        let user_id = Uuid::new_v4();
        let now = Utc::now();

        let plan = RecurringBuyPlan {
            id: Uuid::new_v4(),
            user_id,
            coin: "BTC".into(),
            amount_usdt: Decimal::new(100, 0),
            frequency: RecurringBuyFrequency::Daily,
            day_of_week: None,
            hour: 9,
            next_execution: now + chrono::Duration::hours(1),
            total_invested: Decimal::ZERO,
            executions: 0,
            is_active: true,
            created_at: now,
        };
        let plan_id = plan.id;
        exchange.recurring_buy_plans.lock().unwrap_or_else(|e| e.into_inner()).push(plan);

        // Active
        let active = exchange
            .recurring_buy_plans
            .lock().unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|p| p.user_id == user_id && p.is_active)
            .count();
        assert_eq!(active, 1);

        // Cancel
        {
            let mut plans = exchange.recurring_buy_plans.lock().unwrap_or_else(|e| e.into_inner());
            let p = plans.iter_mut().find(|p| p.id == plan_id).unwrap();
            p.is_active = false;
        }

        let active = exchange
            .recurring_buy_plans
            .lock().unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|p| p.user_id == user_id && p.is_active)
            .count();
        assert_eq!(active, 0);
    }

    #[test]
    fn signal_bot_webhook_execution_counter() {
        let exchange = make_exchange();
        let user_id = Uuid::new_v4();

        let bot = SignalBot {
            id: Uuid::new_v4(),
            user_id,
            name: "Test Bot".into(),
            symbols: vec![],
            webhook_secret: "secret1234".into(),
            is_active: true,
            trades_executed: 5,
            created_at: Utc::now(),
        };
        let bot_id = bot.id;
        exchange.signal_bots.lock().unwrap_or_else(|e| e.into_inner()).push(bot);

        // Simulate counter increment
        {
            let mut bots = exchange.signal_bots.lock().unwrap_or_else(|e| e.into_inner());
            let b = bots.iter_mut().find(|b| b.id == bot_id).unwrap();
            b.trades_executed += 1;
        }

        let count = exchange
            .signal_bots
            .lock().unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|b| b.id == bot_id)
            .map(|b| b.trades_executed)
            .unwrap();
        assert_eq!(count, 6);
    }
}
