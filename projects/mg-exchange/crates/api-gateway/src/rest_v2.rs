//! API v2 REST handlers.
//!
//! All responses use the unified `ApiV2Response` envelope from `crate::v2`.
//! Routes are registered under `/api/v2/*`.
//!
//! New in v2:
//! - Standardized `{ retCode, retMsg, result, time }` envelope on all responses.
//! - Cursor-based pagination on list endpoints.
//! - `GET /api/v2/server-info` — exchange info + supported features + limits.
//! - `GET /api/v2/orders` — paginated open orders.
//! - `GET /api/v2/orders/history` — paginated order history.
//! - `GET /api/v2/trades` — paginated trade history.
//! - `GET /api/v2/positions` — paginated positions.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::response::IntoResponse;
use serde::Serialize;

use crate::auth::AuthenticatedUser;
use crate::exchange::Exchange;
use crate::v2::{
    err, ok, CursorQuery, PaginatedList, RateLimitInfo, RetCode, ServerInfo,
};

/// Shared app state.
pub type AppState = Arc<Exchange>;

// ---------------------------------------------------------------------------
// Server info
// ---------------------------------------------------------------------------

/// GET /api/v2/server-info
///
/// Returns exchange version, currently enabled feature flags, and rate limit tiers.
pub async fn server_info(
    State(exchange): State<AppState>,
) -> impl IntoResponse {
    let enabled_features: Vec<String> = exchange
        .feature_flags
        .all()
        .into_iter()
        .filter(|(_, v)| *v)
        .map(|(k, _)| k)
        .collect();

    let info = ServerInfo {
        version: env!("CARGO_PKG_VERSION"),
        supported_features: enabled_features,
        rate_limits: RateLimitInfo::default(),
        server_time_ms: chrono::Utc::now().timestamp_millis(),
    };

    match ok(info) {
        Ok(resp) => resp.into_response(),
        Err(e) => e.into_response(),
    }
}

// ---------------------------------------------------------------------------
// Orders — paginated
// ---------------------------------------------------------------------------

/// Serialisable open-order record for the v2 response.
#[derive(Debug, Serialize)]
pub struct V2OrderRecord {
    pub order_id: String,
    pub symbol: String,
    pub side: String,
    pub order_type: String,
    pub price: String,
    pub qty: String,
    pub filled_qty: String,
    pub status: String,
    pub created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_link_id: Option<String>,
}

/// GET /api/v2/orders
///
/// Returns the authenticated user's open orders with cursor-based pagination.
pub async fn list_orders(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Query(pagination): Query<CursorQuery>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let limit = pagination.effective_limit();

    let mut all_orders: Vec<V2OrderRecord> = exchange
        .order_locks
        .iter()
        .filter(|r| r.user_id == user_id)
        .filter_map(|lock_ref| {
            let order_id = *lock_ref.key();
            let info = exchange.pending_orders.get(&order_id)?;
            let status = if lock_ref.filled_qty >= lock_ref.qty {
                "filled"
            } else if lock_ref.filled_qty > rust_decimal::Decimal::ZERO {
                "partially_filled"
            } else {
                "open"
            };
            Some(V2OrderRecord {
                order_id: order_id.to_string(),
                symbol: lock_ref.symbol.to_string(),
                side: format!("{:?}", lock_ref.side).to_lowercase(),
                order_type: format!("{:?}", info.order_type).to_lowercase(),
                price: lock_ref.price.to_string(),
                qty: lock_ref.qty.to_string(),
                filled_qty: lock_ref.filled_qty.to_string(),
                status: status.to_string(),
                created_at: info.created_at.timestamp_millis(),
                order_link_id: info.order_link_id.clone(),
            })
        })
        .collect();
    // Newest first.
    all_orders.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    let page = PaginatedList::paginate(all_orders, &pagination.cursor, limit);
    match ok(page) {
        Ok(resp) => resp.into_response(),
        Err(e) => e.into_response(),
    }
}

// ---------------------------------------------------------------------------
// Order history — paginated
// ---------------------------------------------------------------------------

/// Serialisable order history record for v2.
#[derive(Debug, Serialize)]
pub struct V2OrderHistoryRecord {
    pub order_id: String,
    pub symbol: String,
    pub side: String,
    pub order_type: String,
    pub price: String,
    pub qty: String,
    pub filled_qty: String,
    pub status: String,
    pub created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_link_id: Option<String>,
}

/// GET /api/v2/orders/history
///
/// Returns the authenticated user's terminal orders (filled, cancelled, rejected)
/// with cursor-based pagination.
pub async fn order_history(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Query(pagination): Query<CursorQuery>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let limit = pagination.effective_limit();

    let history = exchange.order_history.lock().unwrap_or_else(|e| e.into_inner());
    let records: Vec<V2OrderHistoryRecord> = history
        .iter()
        .filter(|h| h.user_id == user_id)
        .map(|h| V2OrderHistoryRecord {
            order_id: h.order_id.to_string(),
            symbol: h.symbol.to_string(),
            side: h.side.clone(),
            order_type: h.order_type.clone(),
            price: h.price.to_string(),
            qty: h.qty.to_string(),
            filled_qty: h.filled_qty.to_string(),
            status: h.status.clone(),
            created_at: h.created_at.timestamp_millis(),
            order_link_id: h.order_link_id.clone(),
        })
        .rev()
        .collect();
    drop(history);

    let page = PaginatedList::paginate(records, &pagination.cursor, limit);
    match ok(page) {
        Ok(resp) => resp.into_response(),
        Err(e) => e.into_response(),
    }
}

// ---------------------------------------------------------------------------
// Trades — paginated
// ---------------------------------------------------------------------------

/// Serialisable trade record for v2.
#[derive(Debug, Serialize)]
pub struct V2TradeRecord {
    pub trade_id: u64,
    pub symbol: String,
    pub price: String,
    pub qty: String,
    pub taker_side: String,
    pub timestamp: i64,
}

/// GET /api/v2/trades
///
/// Returns public trade history for a symbol with cursor-based pagination.
/// Query parameters:
///   - `symbol` (required)
///   - `cursor`, `limit` (pagination)
pub async fn list_trades(
    State(exchange): State<AppState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let symbol_str = match params.get("symbol") {
        Some(s) => s.clone(),
        None => {
            return err::<PaginatedList<V2TradeRecord>>(
                RetCode::INVALID_PARAM,
                "Missing required parameter: symbol",
            )
            .into_response();
        }
    };

    let cursor = params.get("cursor").cloned().unwrap_or_default();
    let limit: usize = params
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(20)
        .min(200)
        .max(1);

    let sym = exchange_common::Symbol::new(&symbol_str);
    let trade_history = exchange.trade_history.lock().unwrap_or_else(|e| e.into_inner());

    let records: Vec<V2TradeRecord> = trade_history
        .get(&sym)
        .map(|trades| {
            trades
                .iter()
                .rev()
                .map(|t| V2TradeRecord {
                    trade_id: t.trade_id,
                    symbol: t.symbol.to_string(),
                    price: t.price.to_string(),
                    qty: t.qty.to_string(),
                    taker_side: t.taker_side.clone(),
                    timestamp: t.timestamp.timestamp_millis(),
                })
                .collect()
        })
        .unwrap_or_default();
    drop(trade_history);

    let page = PaginatedList::paginate(records, &cursor, limit);
    match ok(page) {
        Ok(resp) => resp.into_response(),
        Err(e) => e.into_response(),
    }
}

// ---------------------------------------------------------------------------
// Positions — paginated
// ---------------------------------------------------------------------------

/// Serialisable position record for v2.
#[derive(Debug, Serialize)]
pub struct V2PositionRecord {
    pub symbol: String,
    pub side: String,
    pub size: String,
    pub entry_price: String,
    pub mark_price: String,
    pub unrealized_pnl: String,
    pub margin: String,
    pub leverage: String,
}

/// GET /api/v2/positions
///
/// Returns the authenticated user's open positions with cursor-based pagination.
pub async fn list_positions(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Query(pagination): Query<CursorQuery>,
) -> impl IntoResponse {
    let user_id = auth.0;
    let limit = pagination.effective_limit();

    let pos_manager = exchange.positions.lock();
    let positions: Vec<V2PositionRecord> = pos_manager
        .user_positions(user_id)
        .iter()
        .map(|p| {
            let mark = exchange
                .get_mark_price(&p.symbol)
                .map(|m| m.price)
                .unwrap_or(p.entry_price);
            let upnl = p.unrealized_pnl(mark);

            V2PositionRecord {
                symbol: p.symbol.to_string(),
                side: format!("{:?}", p.side).to_lowercase(),
                size: p.size.to_string(),
                entry_price: p.entry_price.to_string(),
                mark_price: mark.to_string(),
                unrealized_pnl: upnl.to_string(),
                margin: p.margin.to_string(),
                leverage: p.leverage.to_string(),
            }
        })
        .collect();
    drop(pos_manager);

    let page = PaginatedList::paginate(positions, &pagination.cursor, limit);
    match ok(page) {
        Ok(resp) => resp.into_response(),
        Err(e) => e.into_response(),
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::{PaginatedList, RetCode};

    #[test]
    fn server_info_version_is_non_empty() {
        let version = env!("CARGO_PKG_VERSION");
        assert!(!version.is_empty());
    }

    #[test]
    fn v2_ret_code_ok_is_zero() {
        assert_eq!(RetCode::OK.0, 0);
    }

    #[test]
    fn v2_error_codes_are_non_zero() {
        assert!(RetCode::INVALID_PARAM.0 > 0);
        assert!(RetCode::UNAUTHORIZED.0 > 0);
        assert!(RetCode::RATE_LIMITED.0 > 0);
        assert!(RetCode::INSUFFICIENT_BALANCE.0 > 0);
        assert!(RetCode::ORDER_NOT_FOUND.0 > 0);
    }

    #[test]
    fn paginated_list_cursor_pagination_works() {
        let items: Vec<u32> = (0..50).collect();
        let page1: PaginatedList<u32> = PaginatedList::paginate(items.clone(), "", 20);
        assert_eq!(page1.list.len(), 20);
        assert_eq!(page1.total, 50);
        assert!(!page1.next_page_cursor.is_empty());

        let page2: PaginatedList<u32> = PaginatedList::paginate(items.clone(), &page1.next_page_cursor, 20);
        assert_eq!(page2.list[0], 20);

        let page3: PaginatedList<u32> = PaginatedList::paginate(items, &page2.next_page_cursor, 20);
        assert_eq!(page3.list.len(), 10);
        assert!(page3.next_page_cursor.is_empty());
    }
}
