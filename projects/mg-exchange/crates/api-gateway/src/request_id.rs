//! Request-ID middleware.
//!
//! Every request gets a unique identifier that is:
//!
//! - Propagated from the incoming `x-request-id` header if one is present.
//! - Generated as a UUID v4 if none is present.
//!
//! The ID is:
//! 1. Stored in request extensions as [`RequestId`] so downstream handlers
//!    can read it via `axum::Extension<RequestId>`.
//! 2. Reflected in every response header under the same `x-request-id` name.
//! 3. **S20 J.1** — attached as a `request_id` field on a `tracing::Span`
//!    that wraps the rest of the request chain. Every `tracing::info!` /
//!    `error!` / `debug!` line emitted during that request automatically
//!    carries the `request_id=<uuid>` field when the subscriber is JSON
//!    (or pretty-printed in dev). This makes it possible to grep all logs
//!    for a single request's full execution path.

use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;
use tracing::Instrument;
use uuid::Uuid;

/// Newtype wrapper around the request-id string.
///
/// Stored in request extensions by [`request_id_middleware`].
#[derive(Debug, Clone)]
pub struct RequestId(pub String);

/// Axum middleware layer that assigns/propagates `x-request-id`.
///
/// Usage: `.layer(axum::middleware::from_fn(request_id_middleware))`
///
/// Runs everything downstream inside a `tracing::info_span!("http_request",
/// request_id = ..., method = ..., uri = ...)` so log lines emitted by any
/// handler — including ones dispatched via `spawn_blocking` once the J.2
/// span propagation work lands — carry the request_id field.
pub async fn request_id_middleware(mut req: Request<axum::body::Body>, next: Next) -> Response {
    // Re-use any caller-supplied ID; generate a fresh UUID otherwise.
    let id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    // Store in extensions for handler access.
    req.extensions_mut().insert(RequestId(id.clone()));

    // S20 J.1 — create a span carrying request_id + method + path. `Instrument`
    // enters the span at every `.await` point inside `next.run(req)` so log
    // lines from axum, tower, and the REST handlers all include request_id.
    let method = req.method().to_string();
    let uri = req.uri().path().to_string();
    let span = tracing::info_span!(
        "http_request",
        request_id = %id,
        http.method = %method,
        http.path = %uri,
    );

    let mut resp = next.run(req).instrument(span).await;

    // Reflect ID in response header. The value is always valid ASCII so
    // `parse().unwrap()` is sound — we control the content.
    if let Ok(header_val) = id.parse() {
        resp.headers_mut().insert("x-request-id", header_val);
    }

    resp
}

/// S20 J.2 — spawn a blocking task with the current tracing span propagated.
///
/// `tokio::task::spawn_blocking` runs its closure on a separate OS thread
/// from the async runtime's blocking pool. `tracing::Span::current()` is
/// thread-local, so the `http_request` span created by
/// [`request_id_middleware`] is lost when the closure starts executing on
/// the blocking thread — log lines emitted inside `place_order` carry no
/// `request_id` field.
///
/// This helper captures `Span::current()` before the hand-off, moves it
/// into the closure, and re-enters the span for the closure's duration.
/// Every `tracing::info!` / `warn!` / `error!` inside the closure is then
/// attributed to the original request span.
///
/// Usage:
/// ```no_run
/// # use exchange_api_gateway::request_id::spawn_blocking_traced;
/// # async fn example() -> anyhow::Result<()> {
/// let result = spawn_blocking_traced(move || -> anyhow::Result<()> {
///     // exchange.place_order(cmd) — stand-in for the real blocking call
///     Ok(())
/// }).await?;
/// # let _ = result;
/// # Ok(())
/// # }
/// ```
pub fn spawn_blocking_traced<F, T>(f: F) -> tokio::task::JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let span = tracing::Span::current();
    tokio::task::spawn_blocking(move || {
        let _enter = span.enter();
        f()
    })
}
