//! API v2 response standardization.
//!
//! All `/api/v2/*` endpoints return a uniform envelope:
//!
//! ```json
//! {
//!   "retCode": 0,
//!   "retMsg": "OK",
//!   "result": { ... },
//!   "time": 1234567890123
//! }
//! ```
//!
//! Error responses replace `"result"` with `null` and set `retCode` to a
//! well-known error code.
//!
//! Pagination: list endpoints that support a `cursor` query parameter return:
//!
//! ```json
//! {
//!   "retCode": 0,
//!   "retMsg": "OK",
//!   "result": {
//!     "list": [ ... ],
//!     "nextPageCursor": "base64-opaque-token",
//!     "total": 100
//!   },
//!   "time": 1234567890123
//! }
//! ```

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use tracing::debug;

// ---------------------------------------------------------------------------
// Error code enum
// ---------------------------------------------------------------------------

/// Well-known API v2 error codes.
///
/// 0 means success. All non-zero codes indicate an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RetCode(pub u32);

impl RetCode {
    /// Success.
    pub const OK: RetCode = RetCode(0);
    /// Invalid parameter (missing field, bad value, etc).
    pub const INVALID_PARAM: RetCode = RetCode(10001);
    /// Authentication required or token invalid.
    pub const UNAUTHORIZED: RetCode = RetCode(10002);
    /// Rate limit exceeded.
    pub const RATE_LIMITED: RetCode = RetCode(10003);
    /// Insufficient balance to place the order or withdrawal.
    pub const INSUFFICIENT_BALANCE: RetCode = RetCode(10004);
    /// Order not found.
    pub const ORDER_NOT_FOUND: RetCode = RetCode(10005);
    /// Symbol not found or not trading.
    pub const SYMBOL_NOT_FOUND: RetCode = RetCode(10006);
    /// Feature not enabled / unavailable.
    pub const FEATURE_UNAVAILABLE: RetCode = RetCode(10007);
    /// Internal server error.
    pub const INTERNAL_ERROR: RetCode = RetCode(10008);
    /// The requested resource was not found.
    pub const NOT_FOUND: RetCode = RetCode(10009);
    /// The operation is not permitted in the current state.
    pub const FORBIDDEN: RetCode = RetCode(10010);
}

impl std::fmt::Display for RetCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// Response envelope
// ---------------------------------------------------------------------------

/// The unified API v2 response envelope.
#[derive(Debug, Serialize)]
pub struct ApiV2Response<T: Serialize> {
    /// 0 = success, non-zero = error.
    #[serde(rename = "retCode")]
    pub ret_code: RetCode,
    /// Human-readable status message.
    #[serde(rename = "retMsg")]
    pub ret_msg: String,
    /// The response payload, or `null` on error.
    pub result: Option<T>,
    /// Server-side Unix timestamp in milliseconds.
    pub time: i64,
}

impl<T: Serialize> ApiV2Response<T> {
    /// Create a success response.
    pub fn ok(result: T) -> Self {
        Self {
            ret_code: RetCode::OK,
            ret_msg: "OK".into(),
            result: Some(result),
            time: chrono::Utc::now().timestamp_millis(),
        }
    }
}

/// Create an error envelope (result = null).
#[derive(Debug, Serialize)]
pub struct ApiV2Error {
    #[serde(rename = "retCode")]
    pub ret_code: RetCode,
    #[serde(rename = "retMsg")]
    pub ret_msg: String,
    pub result: Option<()>,
    pub time: i64,
}

impl ApiV2Error {
    pub fn new(code: RetCode, msg: impl Into<String>) -> Self {
        Self {
            ret_code: code,
            ret_msg: msg.into(),
            result: None,
            time: chrono::Utc::now().timestamp_millis(),
        }
    }
}

impl IntoResponse for ApiV2Error {
    fn into_response(self) -> Response {
        let status = match self.ret_code {
            RetCode::INVALID_PARAM => StatusCode::BAD_REQUEST,
            RetCode::UNAUTHORIZED => StatusCode::UNAUTHORIZED,
            RetCode::RATE_LIMITED => StatusCode::TOO_MANY_REQUESTS,
            RetCode::INSUFFICIENT_BALANCE => StatusCode::UNPROCESSABLE_ENTITY,
            RetCode::ORDER_NOT_FOUND | RetCode::SYMBOL_NOT_FOUND | RetCode::NOT_FOUND => {
                StatusCode::NOT_FOUND
            }
            RetCode::FORBIDDEN => StatusCode::FORBIDDEN,
            RetCode::FEATURE_UNAVAILABLE => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(self)).into_response()
    }
}

// Convenience alias.
pub type V2Result<T> = Result<Json<ApiV2Response<T>>, ApiV2Error>;

/// Wrap a value in a success envelope.
pub fn ok<T: Serialize>(value: T) -> V2Result<T> {
    Ok(Json(ApiV2Response::ok(value)))
}

/// Return an error envelope.
pub fn err<T: Serialize>(code: RetCode, msg: impl Into<String>) -> V2Result<T> {
    Err(ApiV2Error::new(code, msg))
}

// ---------------------------------------------------------------------------
// Paginated list wrapper
// ---------------------------------------------------------------------------

/// A paginated list result embedded inside the `result` field.
#[derive(Debug, Serialize)]
pub struct PaginatedList<T: Serialize> {
    /// The items in the current page.
    pub list: Vec<T>,
    /// Opaque cursor for the next page.  `null` / empty string when no more pages.
    #[serde(rename = "nextPageCursor")]
    pub next_page_cursor: String,
    /// Total number of items across all pages.
    pub total: usize,
}

impl<T: Serialize> PaginatedList<T> {
    /// Build from a slice of all items, applying `cursor` + `limit` pagination.
    ///
    /// `cursor` is the raw string from the request (empty = start from the beginning).
    /// `limit` is the page size.
    ///
    /// Items are indexed sequentially; the cursor encodes the *next* start index
    /// as a base-64 string so it remains opaque to clients.
    pub fn paginate(all_items: Vec<T>, cursor: &str, limit: usize) -> Self {
        let total = all_items.len();
        let start = decode_cursor(cursor).unwrap_or(0).min(total);
        let end = (start + limit).min(total);
        let list: Vec<T> = all_items.into_iter().skip(start).take(end - start).collect();
        let next_page_cursor = if end < total {
            encode_cursor(end)
        } else {
            String::new()
        };

        debug!(
            start,
            end,
            total,
            next_page_cursor = %next_page_cursor,
            "paginated list built"
        );

        Self {
            list,
            next_page_cursor,
            total,
        }
    }
}

// ---------------------------------------------------------------------------
// Cursor encoding / decoding
// ---------------------------------------------------------------------------

/// Encode a numeric page offset as a base64 string.
fn encode_cursor(offset: usize) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(offset.to_string())
}

/// Decode a base64 cursor string back to a page offset.
///
/// Returns `None` if the cursor is empty or cannot be decoded.
fn decode_cursor(cursor: &str) -> Option<usize> {
    if cursor.is_empty() {
        return None;
    }
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(cursor)
        .ok()?;
    let s = std::str::from_utf8(&bytes).ok()?;
    s.parse::<usize>().ok()
}

// ---------------------------------------------------------------------------
// Query parameters for list endpoints
// ---------------------------------------------------------------------------

/// Common pagination query parameters added to v2 list endpoints.
#[derive(Debug, Deserialize)]
pub struct CursorQuery {
    /// Opaque cursor returned by a previous call.  Omit for the first page.
    #[serde(default)]
    pub cursor: String,
    /// Maximum number of items per page.  Defaults to 20; capped at 200.
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    20
}

impl CursorQuery {
    /// Return the effective limit, clamped to `[1, 200]`.
    pub fn effective_limit(&self) -> usize {
        self.limit.clamp(1, 200)
    }
}

// ---------------------------------------------------------------------------
// Server info
// ---------------------------------------------------------------------------

/// Response body for `GET /api/v2/server-info`.
#[derive(Debug, Serialize)]
pub struct ServerInfo {
    /// Exchange software version string.
    pub version: &'static str,
    /// List of feature flags that are currently enabled.
    pub supported_features: Vec<String>,
    /// Rate limit tiers (requests per second by tier name).
    pub rate_limits: RateLimitInfo,
    /// Server time in milliseconds since Unix epoch.
    pub server_time_ms: i64,
}

#[derive(Debug, Serialize)]
pub struct RateLimitInfo {
    /// Default (unauthenticated) limit.
    pub default_rps: u32,
    /// Authenticated user limit.
    pub authenticated_rps: u32,
    /// VIP user limit.
    pub vip_rps: u32,
    /// Admin limit.
    pub admin_rps: u32,
}

impl Default for RateLimitInfo {
    fn default() -> Self {
        Self {
            default_rps: 10,
            authenticated_rps: 50,
            vip_rps: 200,
            admin_rps: 500,
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ok_response_has_ret_code_zero() {
        let resp = ApiV2Response::ok(json!({"key": "value"}));
        assert_eq!(resp.ret_code, RetCode::OK);
        assert_eq!(resp.ret_msg, "OK");
        assert!(resp.result.is_some());
    }

    #[test]
    fn error_response_has_non_zero_code() {
        let err = ApiV2Error::new(RetCode::INVALID_PARAM, "bad input");
        assert_eq!(err.ret_code, RetCode::INVALID_PARAM);
        assert_eq!(err.ret_msg, "bad input");
        assert!(err.result.is_none());
    }

    #[test]
    fn cursor_roundtrip() {
        let encoded = encode_cursor(42);
        let decoded = decode_cursor(&encoded);
        assert_eq!(decoded, Some(42));
    }

    #[test]
    fn cursor_decode_empty_returns_none() {
        assert_eq!(decode_cursor(""), None);
    }

    #[test]
    fn paginated_list_first_page() {
        let items: Vec<i32> = (0..50).collect();
        let page = PaginatedList::paginate(items, "", 20);
        assert_eq!(page.list.len(), 20);
        assert_eq!(page.total, 50);
        assert!(!page.next_page_cursor.is_empty());
        assert_eq!(page.list[0], 0);
        assert_eq!(page.list[19], 19);
    }

    #[test]
    fn paginated_list_second_page() {
        let items: Vec<i32> = (0..50).collect();
        let page1 = PaginatedList::paginate(items.clone(), "", 20);
        let page2 = PaginatedList::paginate(items, &page1.next_page_cursor, 20);
        assert_eq!(page2.list[0], 20);
        assert_eq!(page2.list.len(), 20);
        assert!(!page2.next_page_cursor.is_empty());
    }

    #[test]
    fn paginated_list_last_page_has_empty_cursor() {
        let items: Vec<i32> = (0..10).collect();
        let page = PaginatedList::paginate(items, "", 20);
        assert_eq!(page.list.len(), 10);
        assert_eq!(page.total, 10);
        assert!(page.next_page_cursor.is_empty());
    }

    #[test]
    fn paginated_list_empty_input() {
        let items: Vec<i32> = vec![];
        let page = PaginatedList::paginate(items, "", 20);
        assert_eq!(page.list.len(), 0);
        assert_eq!(page.total, 0);
        assert!(page.next_page_cursor.is_empty());
    }

    #[test]
    fn cursor_query_effective_limit_clamps() {
        let q = CursorQuery { cursor: String::new(), limit: 0 };
        assert_eq!(q.effective_limit(), 1);
        let q2 = CursorQuery { cursor: String::new(), limit: 9999 };
        assert_eq!(q2.effective_limit(), 200);
    }
}
