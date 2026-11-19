//! WebSocket handler for the exchange.
//!
//! ## Supported topics
//!
//! ### Market (public, no auth required)
//! - `trade.{symbol}` — every public trade fill
//! - `orderbook.{symbol}` — full snapshot on first subscribe, then deltas
//! - `orderbook.diff.{symbol}` — incremental diff updates with `seq`/`prevSeq`
//! - `orderbook.1.{symbol}` — best bid/ask snapshot only
//! - `orderbook.50.{symbol}` — top-50 levels snapshot (refreshed every 100 ms)
//! - `ticker.{symbol}` — rolling 24h ticker after each trade
//! - `ticker.mini.{symbol}` — lightweight: last_price + volume_24h only
//! - `kline.{interval}.{symbol}` — candle updates
//! - `liquidation.{symbol}` — public liquidation feed
//! - `funding.{symbol}` — funding rate updates (emitted each interval)
//! - `markPrice.{symbol}` — real-time mark price + index price updates
//!
//! ### Private (require auth first)
//! - `orders` — order status changes for the authenticated user
//! - `fills` — trade fills
//! - `positions` — position updates
//! - `funding` — funding payments
//! - `deposits` — deposit credits
//! - `adl` — auto-deleverage events (forced position reductions)
//! - `adl.{symbol}` — ADL warning: position is in top quantile for a specific symbol
//! - `wallet` — combined balance + position snapshot (updated on any balance change)
//!
//! ## Authentication
//!
//! Two auth methods are supported:
//!
//! 1. **API key HMAC** — `{"op":"auth","args":["api_key","timestamp","signature"]}`
//! 2. **ListenKey** — `{"op":"auth","listenKey":"<key>"}` (obtained via REST)
//!
//! ## Sequence numbers
//!
//! Every `orderbook.diff.{symbol}` message carries:
//! - `seq` — sequence number of this update
//! - `prevSeq` — sequence number of the preceding update (0 = first)
//!
//! Clients that detect a gap (`prevSeq != last_seen_seq`) should re-subscribe or
//! fetch a fresh snapshot from `GET /api/v1/markets/{symbol}/orderbook`.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::broadcast;

use crate::exchange::{Exchange, MarketEvent, PrivateEvent};
use crate::rest::AppState;

// ---------------------------------------------------------------------------
// Per-connection state helpers
// ---------------------------------------------------------------------------

/// Cached orderbook snapshot used to compute per-client deltas.
struct BookSnapshot {
    /// price → qty string map for bids.
    bids: HashMap<String, String>,
    /// price → qty string map for asks.
    asks: HashMap<String, String>,
    /// Sequence number of this snapshot — mirrors `BookUpdate::sequence`.
    sequence: u64,
}

// ---------------------------------------------------------------------------
// Protocol types
// ---------------------------------------------------------------------------

/// WebSocket request from client.
#[derive(Debug, Deserialize)]
pub struct WsRequest {
    pub op: String,
    /// Subscription topics or, for HMAC auth, `[api_key, timestamp, signature]`.
    #[serde(default)]
    pub args: Vec<String>,
    /// ListenKey auth: client passes the key obtained from POST /api/v1/listen-key.
    #[serde(rename = "listenKey", default)]
    pub listen_key: Option<String>,
}

// ---------------------------------------------------------------------------
// Topic matching helpers
// ---------------------------------------------------------------------------

/// Return `true` if the subscribed topic `sub` matches an incoming `event_topic`.
///
/// Handles:
/// - Exact match (`"trade.BTCUSDT"`)
/// - Depth-level prefix match (`"orderbook.BTCUSDT"` matches `"orderbook.BTCUSDT"`)
/// - Diff topic: `"orderbook.diff.BTCUSDT"`
/// - Fixed-depth topics: `"orderbook.1.BTCUSDT"`, `"orderbook.50.BTCUSDT"`
fn topic_matches(sub: &str, event_topic: &str) -> bool {
    sub == event_topic
        || (sub.starts_with("orderbook.")
            && event_topic.starts_with("orderbook.")
            && {
                // Both must refer to the same symbol suffix after the second dot segment.
                let sub_sym = sub
                    .splitn(3, '.')
                    .nth(2)
                    .or_else(|| sub.strip_prefix("orderbook."))
                    .unwrap_or("");
                let ev_sym = event_topic
                    .strip_prefix("orderbook.")
                    .unwrap_or("");
                sub_sym == ev_sym || ev_sym == sub_sym
            })
}

/// Extract the symbol from a topic string of the form `prefix.symbol` or
/// `prefix.depth.symbol`.
///
/// Examples:
/// - `"orderbook.BTCUSDT"` → `"BTCUSDT"`
/// - `"orderbook.50.BTCUSDT"` → `"BTCUSDT"`
/// - `"orderbook.diff.BTCUSDT"` → `"BTCUSDT"`
#[cfg(test)]
fn symbol_from_orderbook_topic(topic: &str) -> Option<&str> {
    let rest = topic.strip_prefix("orderbook.")?;
    // Three possible forms:
    //   "BTCUSDT"          → rest has no dot → symbol
    //   "50.BTCUSDT"       → rest has a dot, first segment is a number or "diff"
    //   "diff.BTCUSDT"
    if let Some(pos) = rest.find('.') {
        Some(&rest[pos + 1..])
    } else {
        Some(rest)
    }
}

/// Return `true` when the given subscription is specifically a `orderbook.diff.*` topic.
fn is_diff_topic(sub: &str) -> bool {
    sub.starts_with("orderbook.diff.")
}

/// If the subscription is a fixed-depth topic (`orderbook.N.symbol`), return the depth N.
/// Returns `None` for `orderbook.diff.*` and plain `orderbook.*`.
fn fixed_depth_from_topic(sub: &str) -> Option<usize> {
    let rest = sub.strip_prefix("orderbook.")?;
    let (prefix, _sym) = rest.split_once('.')?;
    prefix.parse::<usize>().ok()
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// Query parameters for the WS upgrade endpoint.
///
/// Supports combined-stream subscriptions via the `streams` query parameter:
/// `/ws/v1?streams=trade.BTCUSDT/orderbook.BTCUSDT/ticker.ETHUSDT`
#[derive(Debug, Deserialize, Default)]
pub struct WsQuery {
    /// Slash-separated list of stream topics to auto-subscribe on connect.
    #[serde(default)]
    pub streams: Option<String>,
}

/// GET /ws/v1 — WebSocket upgrade handler.
///
/// Protocol:
/// ```json
/// → {"op": "auth", "args": ["api_key", "timestamp", "signature"]}
/// → {"op": "auth", "listenKey": "abc123..."}       ← listen-key variant
/// ← {"op": "auth", "success": true}
/// → {"op": "subscribe", "args": ["trade.BTCUSDT", "orderbook.diff.BTCUSDT", "orders"]}
/// ← {"op": "subscribe", "success": true}
/// ← {"op": "data", "topic": "trade.BTCUSDT", "data": {...}}
/// ← {"op": "data", "topic": "orderbook.diff.BTCUSDT", "type": "diff", "data": {...}}
/// ```
///
/// ## Combined streams via URL parameter
///
/// Subscribe to multiple streams at connect time without sending a `subscribe` message:
/// ```text
/// /ws/v1?streams=trade.BTCUSDT/orderbook.BTCUSDT/ticker.ETHUSDT
/// ```
///
/// Each message from a combined-stream connection is tagged with its stream name:
/// ```json
/// {"stream": "trade.BTCUSDT", "data": {...}}
/// ```
///
/// Private channels (`orders`, `fills`, `positions`, `funding`, `wallet`) require
/// authentication first. Market channels work without auth.
/// Maximum WebSocket connections allowed per IP address.
const MAX_WS_CONNECTIONS_PER_IP: u32 = 10;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(exchange): State<AppState>,
    Query(query): Query<WsQuery>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    // Extract client IP for connection limiting.
    let ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // C5: Enforce per-IP WebSocket connection limit.
    {
        let mut conns = exchange.ws_ip_connections.lock().unwrap_or_else(|e| e.into_inner());
        let count = conns.entry(ip.clone()).or_insert(0);
        if *count >= MAX_WS_CONNECTIONS_PER_IP {
            tracing::warn!(
                ip = %ip,
                connections = *count,
                limit = MAX_WS_CONNECTIONS_PER_IP,
                "WebSocket connection rejected: per-IP limit exceeded"
            );
            return (
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                axum::Json(serde_json::json!({
                    "error": "Too many WebSocket connections from this IP"
                })),
            ).into_response();
        }
        *count += 1;
    }

    let initial_streams: Vec<String> = query
        .streams
        .as_deref()
        .map(|s| {
            s.split('/')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let combined_stream_mode = !initial_streams.is_empty();
    let exchange_for_cleanup = Arc::clone(&exchange);
    let ip_for_cleanup = ip.clone();

    ws.on_upgrade(move |socket| async move {
        handle_ws_connection(socket, exchange, initial_streams, combined_stream_mode).await;

        // Decrement connection count when the WebSocket closes.
        let mut conns = exchange_for_cleanup.ws_ip_connections.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(count) = conns.get_mut(&ip_for_cleanup) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                conns.remove(&ip_for_cleanup);
            }
        }
    }).into_response()
}

async fn handle_ws_connection(
    socket: WebSocket,
    exchange: Arc<Exchange>,
    initial_streams: Vec<String>,
    combined_stream_mode: bool,
) {
    let (mut sender, mut receiver) = socket.split();

    // Subscribe to broadcast channels
    let mut market_rx = exchange.market_tx.subscribe();
    let mut private_rx = exchange.private_tx.subscribe();

    // Track subscribed topics + authenticated user.
    // Pre-populate from URL ?streams=... parameter.
    let subscriptions = Arc::new(tokio::sync::Mutex::new(initial_streams));
    let auth_user = Arc::new(tokio::sync::Mutex::new(Option::<String>::None));

    let subs_for_market = subscriptions.clone();
    let subs_for_private = subscriptions.clone();
    let auth_for_private = auth_user.clone();

    // Per-symbol previous book snapshot for delta computation (per-client).
    // Keyed by symbol string; holds both the snapshot and the last sent sequence.
    let book_cache: Arc<tokio::sync::Mutex<HashMap<String, BookSnapshot>>> =
        Arc::new(tokio::sync::Mutex::new(HashMap::new()));

    // Channel for outgoing messages (merge multiple sources → one sender)
    let (out_tx, mut out_rx) = tokio::sync::mpsc::channel::<String>(1000);
    let out_tx_market = out_tx.clone();
    let out_tx_private = out_tx.clone();
    let out_tx_control = out_tx.clone();

    // Task 1: Forward outgoing messages to WebSocket
    let send_task = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // Task 2: Forward market events to client (filtered by subscriptions)
    let market_task = tokio::spawn(async move {
        loop {
            match market_rx.recv().await {
                Ok(event) => {
                    // Derive the canonical event topic used for subscription matching.
                    let event_topic = match &event {
                        MarketEvent::Trade { symbol, .. } => format!("trade.{symbol}"),
                        MarketEvent::BookUpdate { symbol, .. } => format!("orderbook.{symbol}"),
                        MarketEvent::Liquidation { symbol, .. } => format!("liquidation.{symbol}"),
                        MarketEvent::FundingUpdate { symbol, .. } => format!("funding.{symbol}"),
                        MarketEvent::KlineUpdate { symbol, interval, .. } => {
                            format!("kline.{interval}.{symbol}")
                        }
                        MarketEvent::TickerUpdate { symbol, .. } => format!("ticker.{symbol}"),
                        MarketEvent::MiniTickerUpdate { symbol, .. } => {
                            format!("ticker.mini.{symbol}")
                        }
                        // funding.{symbol}: funding rate channel.
                        MarketEvent::FundingRateUpdate { symbol, .. } => format!("funding.{symbol}"),
                        // markPrice.{symbol}: real-time mark price channel.
                        MarketEvent::MarkPriceUpdate { symbol, .. } => format!("markPrice.{symbol}"),
                        // ticker.all: all-market mini ticker (Feature 7).
                        MarketEvent::AllMiniTicker { .. } => "ticker.all".to_string(),
                    };

                    // Find all subscriptions that match this event.
                    let matched_subs: Vec<String> = {
                        let subs = subs_for_market.lock().await;
                        subs.iter()
                            .filter(|s| topic_matches(s, &event_topic))
                            .cloned()
                            .collect()
                    };

                    if matched_subs.is_empty() {
                        continue;
                    }

                    // Helper: wrap a message in the combined-stream envelope when the
                    // connection was opened with ?streams=... URL parameter.
                    let wrap_stream = |stream_name: &str, inner: &str| -> String {
                        if combined_stream_mode {
                            // Parse the inner JSON and re-wrap with {"stream":..., "data":...}
                            // so clients can demultiplex without inspecting op/topic fields.
                            if let Ok(inner_val) = serde_json::from_str::<serde_json::Value>(inner) {
                                let data = inner_val.get("data").unwrap_or(&inner_val);
                                serde_json::json!({
                                    "stream": stream_name,
                                    "data": data,
                                }).to_string()
                            } else {
                                inner.to_string()
                            }
                        } else {
                            inner.to_string()
                        }
                    };

                    // Handle BookUpdate specially: build snapshot/delta/fixed-depth messages.
                    if let MarketEvent::BookUpdate { symbol, bids, asks, sequence } = &event {
                        let current_bids: HashMap<String, String> = bids
                            .iter()
                            .map(|(p, q)| (p.to_string(), q.to_string()))
                            .collect();
                        let current_asks: HashMap<String, String> = asks
                            .iter()
                            .map(|(p, q)| (p.to_string(), q.to_string()))
                            .collect();

                        let mut cache = book_cache.lock().await;

                        for sub in &matched_subs {
                            let msg = if is_diff_topic(sub) {
                                // orderbook.diff.{symbol}: incremental diff with seq/prevSeq
                                build_diff_message(
                                    symbol,
                                    &current_bids,
                                    &current_asks,
                                    *sequence,
                                    &mut cache,
                                    sub,
                                )
                            } else if let Some(depth) = fixed_depth_from_topic(sub) {
                                // orderbook.N.{symbol}: top-N snapshot
                                Some(build_depth_snapshot_message(
                                    symbol,
                                    bids,
                                    asks,
                                    *sequence,
                                    depth,
                                    sub,
                                ))
                            } else {
                                // orderbook.{symbol}: snapshot-then-delta (legacy behaviour)
                                Some(build_legacy_book_message(
                                    symbol,
                                    &current_bids,
                                    &current_asks,
                                    *sequence,
                                    &mut cache,
                                    sub,
                                ))
                            };

                            if let Some(m) = msg {
                                let out = wrap_stream(sub, &m);
                                if out_tx_market.send(out).await.is_err() {
                                    return;
                                }
                            }
                        }

                        // Update the cache once (keyed without a topic-specific prefix so
                        // all topics for the same symbol share one snapshot).
                        cache.insert(
                            symbol.clone(),
                            BookSnapshot {
                                bids: current_bids,
                                asks: current_asks,
                                sequence: *sequence,
                            },
                        );
                    } else {
                        // All other event types: forward as-is to each matched sub.
                        let inner = serde_json::json!({
                            "op": "data",
                            "topic": event_topic,
                            "data": event,
                        }).to_string();
                        // In combined-stream mode use the subscription topic as stream name;
                        // there may be multiple matched subs (e.g. ticker + ticker.mini).
                        for sub in &matched_subs {
                            let out = wrap_stream(sub, &inner);
                            if out_tx_market.send(out).await.is_err() {
                                break;
                            }
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("WS client lagged, skipped {n} market events");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Task 3: Forward private events (filtered by authenticated user_id)
    let private_task = tokio::spawn(async move {
        loop {
            match private_rx.recv().await {
                Ok(event) => {
                    // Extract user_id and topic from event.
                    // The topic is an owned String so that per-symbol channels (e.g.
                    // "adl.BTCUSDT") can be constructed without lifetime issues.
                    let (event_user_id, topic): (String, String) = match &event {
                        PrivateEvent::OrderAccepted { user_id, .. } => {
                            (user_id.clone(), "orders".into())
                        }
                        PrivateEvent::OrderFilled { user_id, .. } => {
                            (user_id.clone(), "fills".into())
                        }
                        PrivateEvent::OrderCancelled { user_id, .. } => {
                            (user_id.clone(), "orders".into())
                        }
                        PrivateEvent::OrderRejected { user_id, .. } => {
                            (user_id.clone(), "orders".into())
                        }
                        PrivateEvent::PositionUpdate { user_id, .. } => {
                            (user_id.clone(), "positions".into())
                        }
                        PrivateEvent::FundingPayment { user_id, .. } => {
                            (user_id.clone(), "funding".into())
                        }
                        PrivateEvent::DepositCredited { user_id, .. } => {
                            (user_id.clone(), "deposits".into())
                        }
                        PrivateEvent::AdlClose { user_id, .. } => {
                            (user_id.clone(), "adl".into())
                        }
                        PrivateEvent::WalletUpdate { user_id, .. } => {
                            (user_id.clone(), "wallet".into())
                        }
                        PrivateEvent::AdlWarning { user_id, symbol, .. } => {
                            // Per-symbol ADL warning channel: "adl.{symbol}".
                            // Subscribers use "adl.BTCUSDT" to receive warnings for a
                            // specific symbol without getting noise from all symbols.
                            (user_id.clone(), format!("adl.{symbol}"))
                        }
                    };

                    // Only forward if user is authenticated AND event belongs to them
                    let auth = auth_for_private.lock().await;
                    let subs = subs_for_private.lock().await;

                    if let Some(ref authed_uid) = *auth {
                        if *authed_uid == event_user_id && subs.iter().any(|s| s == &topic) {
                            let msg = serde_json::json!({
                                "op": "data",
                                "topic": topic,
                                "data": event,
                            });
                            if out_tx_private.send(msg.to_string()).await.is_err() {
                                break;
                            }
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("WS client lagged, skipped {n} private events");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Session timeout: disconnect clients after a fixed duration to force re-auth.
    const WS_SESSION_MAX_DURATION_SECS: u64 = 3600; // 1 hour
    let session_deadline =
        tokio::time::Instant::now() + tokio::time::Duration::from_secs(WS_SESSION_MAX_DURATION_SECS);

    // Task 4: Process incoming messages from client
    let mut failed_auth_attempts: u32 = 0;
    loop {
        let recv_fut = receiver.next();
        tokio::select! {
            msg_opt = recv_fut => {
                let msg = match msg_opt {
                    Some(Ok(Message::Text(t))) => t.to_string(),
                    Some(Ok(Message::Ping(_))) => continue,
                    Some(Ok(Message::Close(_))) => break,
                    Some(Err(_)) => break,
                    None => break,
                    Some(Ok(_)) => continue,
                };

                let req = match serde_json::from_str::<WsRequest>(&msg) {
                    Ok(r) => r,
                    Err(_) => continue,
                };

                match req.op.as_str() {
                    "auth" => {
                        // Track failed auth attempts to prevent brute force.
                        let prev_auth = auth_user.lock().await.clone();
                        handle_auth(&exchange, &req, &auth_user, &out_tx_control).await;
                        let new_auth = auth_user.lock().await.clone();
                        if new_auth.is_none() && prev_auth.is_none() {
                            // Auth was attempted but failed.
                            failed_auth_attempts += 1;
                            if failed_auth_attempts >= 5 {
                                let response = serde_json::json!({
                                    "op": "auth",
                                    "success": false,
                                    "error": "Too many failed authentication attempts — connection closed",
                                });
                                let _ = out_tx_control.send(response.to_string()).await;
                                break;
                            }
                        }
                    }
                    "subscribe" => {
                        // Check if private topics require auth.
                        // Private channels are: orders, fills, positions, funding, wallet, deposits,
                        // adl (bare), and adl.{symbol} per-symbol ADL warning channel.
                        let has_private = req.args.iter().any(|a| {
                            matches!(
                                a.as_str(),
                                "orders" | "fills" | "positions" | "funding" | "wallet" | "deposits" | "adl"
                            ) || a.starts_with("adl.")
                        });
                        if has_private && auth_user.lock().await.is_none() {
                            let response = serde_json::json!({
                                "op": "subscribe",
                                "success": false,
                                "message": "Private channels require authentication. Send {\"op\":\"auth\"} first.",
                            });
                            let _ = out_tx_control.send(response.to_string()).await;
                            continue;
                        }

                        let mut subs = subscriptions.lock().await;
                        for arg in &req.args {
                            if !subs.contains(arg) {
                                subs.push(arg.clone());
                            }
                        }
                        let response = serde_json::json!({
                            "op": "subscribe",
                            "success": true,
                            "args": req.args,
                        });
                        let _ = out_tx_control.send(response.to_string()).await;
                    }
                    "unsubscribe" => {
                        let mut subs = subscriptions.lock().await;
                        subs.retain(|s| !req.args.contains(s));
                        let response = serde_json::json!({
                            "op": "unsubscribe",
                            "success": true,
                            "args": req.args,
                        });
                        let _ = out_tx_control.send(response.to_string()).await;
                    }
                    "ping" => {
                        let response = serde_json::json!({"op": "pong"});
                        let _ = out_tx_control.send(response.to_string()).await;
                    }
                    _ => {
                        let response = serde_json::json!({
                            "op": "error",
                            "message": format!("Unknown op: {}", req.op),
                        });
                        let _ = out_tx_control.send(response.to_string()).await;
                    }
                }
            }
            _ = tokio::time::sleep_until(session_deadline) => {
                let response = serde_json::json!({"op": "session_expired"});
                let _ = out_tx_control.send(response.to_string()).await;
                break;
            }
        }
    }

    // Client disconnected — clean up
    send_task.abort();
    market_task.abort();
    private_task.abort();
}

// ---------------------------------------------------------------------------
// Auth helper
// ---------------------------------------------------------------------------

async fn handle_auth(
    exchange: &Arc<Exchange>,
    req: &WsRequest,
    auth_user: &Arc<tokio::sync::Mutex<Option<String>>>,
    out_tx: &tokio::sync::mpsc::Sender<String>,
) {
    // --- Method 1: ListenKey ---
    if let Some(ref key) = req.listen_key {
        let result = {
            let keys = exchange.listen_keys.lock().unwrap_or_else(|e| e.into_inner());
            keys.get(key.as_str()).and_then(|info| {
                if info.expires_at > chrono::Utc::now() {
                    Some(info.user_id.to_string())
                } else {
                    None // expired
                }
            })
        };

        match result {
            Some(user_id) => {
                *auth_user.lock().await = Some(user_id);
                let response = serde_json::json!({"op": "auth", "success": true});
                let _ = out_tx.send(response.to_string()).await;
            }
            None => {
                let response = serde_json::json!({
                    "op": "auth",
                    "success": false,
                    "message": "Invalid or expired listen key",
                });
                let _ = out_tx.send(response.to_string()).await;
            }
        }
        return;
    }

    // --- Method 2: API key HMAC: args = [api_key, timestamp, signature] ---
    if req.args.len() >= 3 {
        let api_key = &req.args[0];
        let timestamp = &req.args[1];
        let signature = &req.args[2];

        if let Some(ref key_store) = exchange.key_store {
            match key_store.verify_signature(api_key, timestamp, "5000", "", signature) {
                Ok(user_id) => {
                    *auth_user.lock().await = Some(user_id.to_string());
                    let response = serde_json::json!({"op": "auth", "success": true});
                    let _ = out_tx.send(response.to_string()).await;
                }
                Err(e) => {
                    let response = serde_json::json!({
                        "op": "auth",
                        "success": false,
                        "message": format!("{e}"),
                    });
                    let _ = out_tx.send(response.to_string()).await;
                }
            }
        } else {
            // No key store configured.
            // S19 D.1: cached field replaces env::var syscall on WS auth.
            if exchange.is_production {
                tracing::warn!("WS auth rejected: no key store configured in PRODUCTION mode");
                let response = serde_json::json!({
                    "op": "auth",
                    "success": false,
                    "error": "Authentication not configured",
                });
                let _ = out_tx.send(response.to_string()).await;
                return;
            }
            // Dev mode — fallback: auth with nil user with a warning.
            tracing::warn!("WS auth: no key store configured — allowing nil user (dev mode)");
            *auth_user.lock().await = Some(uuid::Uuid::nil().to_string());
            let response = serde_json::json!({"op": "auth", "success": true});
            let _ = out_tx.send(response.to_string()).await;
        }
        return;
    }

    // --- Phase 1 shortcut: auth without credentials → nil user ---
    *auth_user.lock().await = Some(uuid::Uuid::nil().to_string());
    let response = serde_json::json!({"op": "auth", "success": true});
    let _ = out_tx.send(response.to_string()).await;
}

// ---------------------------------------------------------------------------
// Book message builders
// ---------------------------------------------------------------------------

/// Build an incremental diff message for `orderbook.diff.{symbol}`.
///
/// Returns `None` when nothing changed (no message should be sent).
///
/// The `cache` map is used to compute changed levels; it is **not** updated
/// here — the caller updates it once after iterating all matched subs.
fn build_diff_message(
    symbol: &str,
    current_bids: &HashMap<String, String>,
    current_asks: &HashMap<String, String>,
    sequence: u64,
    cache: &mut HashMap<String, BookSnapshot>,
    sub: &str,
) -> Option<String> {
    let _ = sub; // topic only used by callers for routing; symbol already extracted

    let (bid_deltas, ask_deltas, prev_seq) = if let Some(prev) = cache.get(symbol) {
        let prev_seq = prev.sequence;
        let bid_deltas = compute_level_deltas(&prev.bids, current_bids);
        let ask_deltas = compute_level_deltas(&prev.asks, current_asks);

        if bid_deltas.is_empty() && ask_deltas.is_empty() {
            // Nothing changed — skip.
            return None;
        }
        (bid_deltas, ask_deltas, prev_seq)
    } else {
        // First update for this symbol — send all levels as the initial diff.
        let bid_deltas: Vec<[String; 2]> = current_bids
            .iter()
            .map(|(p, q)| [p.clone(), q.clone()])
            .collect();
        let ask_deltas: Vec<[String; 2]> = current_asks
            .iter()
            .map(|(p, q)| [p.clone(), q.clone()])
            .collect();
        (bid_deltas, ask_deltas, 0u64)
    };

    let msg = serde_json::json!({
        "op": "data",
        "topic": format!("orderbook.diff.{symbol}"),
        "type": "diff",
        "data": {
            "symbol": symbol,
            "bids": bid_deltas,
            "asks": ask_deltas,
            "seq": sequence,
            "prevSeq": prev_seq,
        },
    });
    Some(msg.to_string())
}

/// Build a fixed-depth snapshot message for `orderbook.N.{symbol}`.
fn build_depth_snapshot_message(
    symbol: &str,
    bids: &[(rust_decimal::Decimal, rust_decimal::Decimal)],
    asks: &[(rust_decimal::Decimal, rust_decimal::Decimal)],
    sequence: u64,
    depth: usize,
    sub: &str,
) -> String {
    let capped_bids: Vec<[String; 2]> = bids
        .iter()
        .take(depth)
        .map(|(p, q)| [p.to_string(), q.to_string()])
        .collect();
    let capped_asks: Vec<[String; 2]> = asks
        .iter()
        .take(depth)
        .map(|(p, q)| [p.to_string(), q.to_string()])
        .collect();

    serde_json::json!({
        "op": "data",
        "topic": sub,
        "type": "snapshot",
        "data": {
            "symbol": symbol,
            "bids": capped_bids,
            "asks": capped_asks,
            "seq": sequence,
        },
    })
    .to_string()
}

/// Build the legacy `orderbook.{symbol}` message: full snapshot first, then deltas.
///
/// The `cache` map is read here but **not** updated — the caller does one final update.
fn build_legacy_book_message(
    symbol: &str,
    current_bids: &HashMap<String, String>,
    current_asks: &HashMap<String, String>,
    sequence: u64,
    cache: &mut HashMap<String, BookSnapshot>,
    sub: &str,
) -> String {
    if let Some(prev) = cache.get(symbol) {
        let bid_deltas = compute_level_deltas(&prev.bids, current_bids);
        let ask_deltas = compute_level_deltas(&prev.asks, current_asks);

        if bid_deltas.is_empty() && ask_deltas.is_empty() {
            // Nothing changed: still send an empty delta so the client knows the
            // sequence has advanced (they can use `seq` for gap detection).
        }

        // Send deltas as [[price, qty]] array — same format as snapshot.
        // Frontend expects [string, string][], not a {price: qty} map.
        let bid_delta_arr: Vec<[&str; 2]> = bid_deltas.iter().map(|[p, q]| [p.as_str(), q.as_str()]).collect();
        let ask_delta_arr: Vec<[&str; 2]> = ask_deltas.iter().map(|[p, q]| [p.as_str(), q.as_str()]).collect();

        serde_json::json!({
            "op": "data",
            "topic": sub,
            "type": "delta",
            "data": {
                "symbol": symbol,
                "bids": bid_delta_arr,
                "asks": ask_delta_arr,
                "sequence": sequence,
            },
        })
        .to_string()
    } else {
        // First update — full snapshot.
        let bids_list: Vec<[String; 2]> = current_bids
            .iter()
            .map(|(p, q)| [p.clone(), q.clone()])
            .collect();
        let asks_list: Vec<[String; 2]> = current_asks
            .iter()
            .map(|(p, q)| [p.clone(), q.clone()])
            .collect();

        serde_json::json!({
            "op": "data",
            "topic": sub,
            "type": "snapshot",
            "data": {
                "symbol": symbol,
                "bids": bids_list,
                "asks": asks_list,
                "sequence": sequence,
            },
        })
        .to_string()
    }
}

/// Compute changed/added/removed price levels between `prev` and `current`.
///
/// Returns a list of `[price, qty]` pairs where:
/// - qty `"0"` means the level was removed.
/// - any other qty means the level was added or its quantity changed.
fn compute_level_deltas(
    prev: &HashMap<String, String>,
    current: &HashMap<String, String>,
) -> Vec<[String; 2]> {
    let mut deltas = Vec::new();

    // Changed or new levels
    for (price, qty) in current {
        if prev.get(price) != Some(qty) {
            deltas.push([price.clone(), qty.clone()]);
        }
    }

    // Removed levels (present in prev but not in current)
    for price in prev.keys() {
        if !current.contains_key(price) {
            deltas.push([price.clone(), "0".to_string()]);
        }
    }

    deltas
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_deltas_added_level() {
        let prev: HashMap<String, String> = [("100".to_string(), "1.0".to_string())].into();
        let current: HashMap<String, String> = [
            ("100".to_string(), "1.0".to_string()),
            ("101".to_string(), "2.0".to_string()),
        ]
        .into();
        let deltas = compute_level_deltas(&prev, &current);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0], ["101".to_string(), "2.0".to_string()]);
    }

    #[test]
    fn compute_deltas_removed_level() {
        let prev: HashMap<String, String> = [
            ("100".to_string(), "1.0".to_string()),
            ("99".to_string(), "0.5".to_string()),
        ]
        .into();
        let current: HashMap<String, String> =
            [("100".to_string(), "1.0".to_string())].into();
        let deltas = compute_level_deltas(&prev, &current);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0], ["99".to_string(), "0".to_string()]);
    }

    #[test]
    fn compute_deltas_changed_qty() {
        let prev: HashMap<String, String> = [("100".to_string(), "1.0".to_string())].into();
        let current: HashMap<String, String> = [("100".to_string(), "2.5".to_string())].into();
        let deltas = compute_level_deltas(&prev, &current);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0], ["100".to_string(), "2.5".to_string()]);
    }

    #[test]
    fn compute_deltas_no_changes() {
        let book: HashMap<String, String> = [("100".to_string(), "1.0".to_string())].into();
        let deltas = compute_level_deltas(&book, &book);
        assert!(deltas.is_empty(), "no deltas when book is unchanged");
    }

    #[test]
    fn topic_matches_exact() {
        assert!(topic_matches("trade.BTCUSDT", "trade.BTCUSDT"));
        assert!(!topic_matches("trade.BTCUSDT", "trade.ETHUSDT"));
    }

    #[test]
    fn topic_matches_orderbook_variants() {
        // Legacy plain orderbook
        assert!(topic_matches("orderbook.BTCUSDT", "orderbook.BTCUSDT"));
        // Diff topic matches the base event topic
        assert!(topic_matches("orderbook.diff.BTCUSDT", "orderbook.BTCUSDT"));
        // Fixed-depth topic matches
        assert!(topic_matches("orderbook.50.BTCUSDT", "orderbook.BTCUSDT"));
        // Different symbol must not match
        assert!(!topic_matches("orderbook.50.ETHUSDT", "orderbook.BTCUSDT"));
    }

    #[test]
    fn is_diff_topic_detection() {
        assert!(is_diff_topic("orderbook.diff.BTCUSDT"));
        assert!(!is_diff_topic("orderbook.BTCUSDT"));
        assert!(!is_diff_topic("orderbook.50.BTCUSDT"));
    }

    #[test]
    fn fixed_depth_parsing() {
        assert_eq!(fixed_depth_from_topic("orderbook.50.BTCUSDT"), Some(50));
        assert_eq!(fixed_depth_from_topic("orderbook.1.BTCUSDT"), Some(1));
        assert_eq!(fixed_depth_from_topic("orderbook.diff.BTCUSDT"), None);
        assert_eq!(fixed_depth_from_topic("orderbook.BTCUSDT"), None);
    }

    #[test]
    fn symbol_from_topic_extraction() {
        assert_eq!(symbol_from_orderbook_topic("orderbook.BTCUSDT"), Some("BTCUSDT"));
        assert_eq!(symbol_from_orderbook_topic("orderbook.50.BTCUSDT"), Some("BTCUSDT"));
        assert_eq!(symbol_from_orderbook_topic("orderbook.diff.BTCUSDT"), Some("BTCUSDT"));
    }

    #[test]
    fn build_diff_message_first_update() {
        let mut cache = HashMap::new();
        let bids: HashMap<String, String> = [("100".to_string(), "1.0".to_string())].into();
        let asks: HashMap<String, String> = [("101".to_string(), "0.5".to_string())].into();

        let msg = build_diff_message("BTCUSDT", &bids, &asks, 42, &mut cache, "orderbook.diff.BTCUSDT");
        assert!(msg.is_some());
        let parsed: serde_json::Value = serde_json::from_str(&msg.unwrap()).unwrap();
        assert_eq!(parsed["data"]["seq"], 42);
        assert_eq!(parsed["data"]["prevSeq"], 0);
        assert_eq!(parsed["type"], "diff");
    }

    #[test]
    fn build_diff_message_no_change_returns_none() {
        let mut cache = HashMap::new();
        let bids: HashMap<String, String> = [("100".to_string(), "1.0".to_string())].into();
        let asks: HashMap<String, String> = [("101".to_string(), "0.5".to_string())].into();
        // Prime cache
        cache.insert("BTCUSDT".to_string(), BookSnapshot {
            bids: bids.clone(),
            asks: asks.clone(),
            sequence: 41,
        });

        // Same data → should return None (nothing changed)
        let msg = build_diff_message("BTCUSDT", &bids, &asks, 42, &mut cache, "orderbook.diff.BTCUSDT");
        assert!(msg.is_none());
    }
}
