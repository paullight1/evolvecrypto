//! Token-bucket rate limiter for the API gateway.
//!
//! [`RateLimiter`] tracks one bucket per client key (IP address or API key).
//! Each bucket refills at a steady `rate` tokens/second up to `max` tokens.
//! A request consumes one token; requests arriving when the bucket is empty
//! are rejected with HTTP 429.
//!
//! ## Weight-based tracking
//!
//! Each endpoint has an associated weight (market data = 1, order placement = 5,
//! batch orders = 10, cancel = 1). The weight is attached to the request as an
//! [`axum::Extension<EndpointWeight>`] by the router and consumed by the middleware,
//! which appends three response headers on every successful response:
//!
//! | Header                     | Meaning                                    |
//! |----------------------------|--------------------------------------------|
//! | `X-RateLimit-Used-Weight`  | Total weight consumed this minute          |
//! | `X-RateLimit-Order-Count`  | Order-placement requests this minute       |
//! | `X-RateLimit-Limit`        | Bucket refill rate (requests / second)     |

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

use axum::extract::Extension;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::auth::ApiKeyStore;

// ---------------------------------------------------------------------------
// Token bucket
// ---------------------------------------------------------------------------

/// A single token-bucket entry for one client key.
#[derive(Debug)]
struct TokenBucket {
    /// Current token count (fractional to support sub-second refill).
    tokens: f64,
    /// Token refill rate in tokens per second.
    rate: f64,
    /// Maximum token capacity (burst size).
    max: f64,
    /// Wall-clock time of the last refill calculation.
    last_refill: Instant,
}

impl TokenBucket {
    fn new(rate: f64, max: f64) -> Self {
        Self {
            tokens: max, // start full
            rate,
            max,
            last_refill: Instant::now(),
        }
    }

    /// Attempt to consume one token.
    ///
    /// Returns `true` if the request is allowed, `false` if the bucket is empty.
    fn try_consume(&mut self) -> bool {
        self.refill();
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Add tokens proportional to the elapsed time since the last refill.
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.rate).min(self.max);
        self.last_refill = now;
    }
}

// ---------------------------------------------------------------------------
// Per-client weight accumulator
// ---------------------------------------------------------------------------

/// Rolling one-minute weight and order-count accumulator for one client key.
#[derive(Debug)]
struct WeightBucket {
    /// Cumulative weight consumed in the current minute window.
    used_weight: u32,
    /// Order-placement request count in the current minute window.
    order_count: u32,
    /// Start of the current one-minute window.
    window_start: Instant,
}

impl WeightBucket {
    fn new() -> Self {
        Self {
            used_weight: 0,
            order_count: 0,
            window_start: Instant::now(),
        }
    }

    /// Reset the window if more than 60 seconds have elapsed.
    fn maybe_reset(&mut self) {
        if self.window_start.elapsed().as_secs() >= 60 {
            self.used_weight = 0;
            self.order_count = 0;
            self.window_start = Instant::now();
        }
    }

    /// Record `weight` units consumed; if `is_order` is true, also increment the order counter.
    fn record(&mut self, weight: u32, is_order: bool) {
        self.maybe_reset();
        self.used_weight = self.used_weight.saturating_add(weight);
        if is_order {
            self.order_count = self.order_count.saturating_add(1);
        }
    }
}

// ---------------------------------------------------------------------------
// RateLimiter
// ---------------------------------------------------------------------------

/// Shared token-bucket rate limiter.
///
/// Thread-safe: the inner map is guarded by a [`Mutex`].
#[derive(Debug, Clone)]
pub struct RateLimiter {
    inner: Arc<Mutex<RateLimiterInner>>,
}

#[derive(Debug)]
struct RateLimiterInner {
    buckets: HashMap<String, TokenBucket>,
    /// Per-client weight accumulators.
    weights: HashMap<String, WeightBucket>,
    rate: f64,
    max: f64,
}

impl RateLimiter {
    /// Create a rate limiter.
    ///
    /// - `requests_per_second` — steady-state refill rate.
    /// - `burst` — maximum tokens that can accumulate (allows short bursts above the rate).
    pub fn new(requests_per_second: u32, burst: u32) -> Self {
        Self {
            inner: Arc::new(Mutex::new(RateLimiterInner {
                buckets: HashMap::new(),
                weights: HashMap::new(),
                rate: f64::from(requests_per_second),
                max: f64::from(burst),
            })),
        }
    }

    /// Return the steady-state refill rate (tokens per second).
    pub fn rate(&self) -> f64 {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).rate
    }

    /// Check and consume one token for `key`.
    ///
    /// Returns `true` if the request is allowed.
    pub fn check(&self, key: &str) -> bool {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let rate = inner.rate;
        let max = inner.max;
        let bucket = inner
            .buckets
            .entry(key.to_string())
            .or_insert_with(|| TokenBucket::new(rate, max));
        bucket.try_consume()
    }

    /// Record `weight` units for `key` and return the current window totals.
    ///
    /// Returns `(used_weight, order_count)`.
    pub fn record_weight(&self, key: &str, weight: u32, is_order: bool) -> (u32, u32) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let wb = inner
            .weights
            .entry(key.to_string())
            .or_insert_with(WeightBucket::new);
        wb.record(weight, is_order);
        (wb.used_weight, wb.order_count)
    }
}

// ---------------------------------------------------------------------------
// Rate tiers
// ---------------------------------------------------------------------------

/// Rate tiers assigned to clients based on their authentication/VIP status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateTier {
    /// Unauthenticated client.
    Default,
    /// Regular authenticated user.
    Authenticated,
    /// VIP tier 3+ user.
    Vip,
    /// Designated market maker.
    MarketMaker,
}

/// A set of [`RateLimiter`] instances, one per [`RateTier`].
///
/// Limits:
/// - Default      — 30 rps, burst 60
/// - Authenticated — 60 rps, burst 120
/// - Vip          — 120 rps, burst 240
/// - MarketMaker  — 300 rps, burst 600
#[derive(Debug, Clone)]
pub struct TieredRateLimiter {
    default: RateLimiter,
    authenticated: RateLimiter,
    vip: RateLimiter,
    market_maker: RateLimiter,
}

impl TieredRateLimiter {
    pub fn new() -> Self {
        Self {
            default: RateLimiter::new(30, 60),
            authenticated: RateLimiter::new(60, 120),
            vip: RateLimiter::new(120, 240),
            market_maker: RateLimiter::new(300, 600),
        }
    }

    /// Create a limiter with a custom default-tier rate for testing.
    #[cfg(test)]
    pub fn with_default_tier(requests_per_second: u32, burst: u32) -> Self {
        Self {
            default: RateLimiter::new(requests_per_second, burst),
            authenticated: RateLimiter::new(60, 120),
            vip: RateLimiter::new(120, 240),
            market_maker: RateLimiter::new(300, 600),
        }
    }

    /// Create a limiter with unlimited rates for benchmarks and load tests.
    ///
    /// A burst of u32::MAX effectively disables the token-bucket cap so
    /// benchmarks measure order-processing latency without middleware throttling.
    #[cfg(any(test, feature = "bench"))]
    pub fn unlimited() -> Self {
        Self {
            default: RateLimiter::new(u32::MAX, u32::MAX),
            authenticated: RateLimiter::new(u32::MAX, u32::MAX),
            vip: RateLimiter::new(u32::MAX, u32::MAX),
            market_maker: RateLimiter::new(u32::MAX, u32::MAX),
        }
    }

    /// Check and consume one token for `key` under the given `tier`.
    ///
    /// Returns `true` if the request is allowed.
    pub fn check(&self, key: &str, tier: RateTier) -> bool {
        match tier {
            RateTier::Default => self.default.check(key),
            RateTier::Authenticated => self.authenticated.check(key),
            RateTier::Vip => self.vip.check(key),
            RateTier::MarketMaker => self.market_maker.check(key),
        }
    }

    /// Record endpoint weight for `key` under `tier` and return `(used_weight, order_count)`.
    pub fn record_weight(
        &self,
        key: &str,
        tier: RateTier,
        weight: u32,
        is_order: bool,
    ) -> (u32, u32) {
        match tier {
            RateTier::Default => self.default.record_weight(key, weight, is_order),
            RateTier::Authenticated => self.authenticated.record_weight(key, weight, is_order),
            RateTier::Vip => self.vip.record_weight(key, weight, is_order),
            RateTier::MarketMaker => self.market_maker.record_weight(key, weight, is_order),
        }
    }

    /// Return the rate limit (rps) for the given tier.
    pub fn limit_for(&self, tier: RateTier) -> f64 {
        match tier {
            RateTier::Default => self.default.rate(),
            RateTier::Authenticated => self.authenticated.rate(),
            RateTier::Vip => self.vip.rate(),
            RateTier::MarketMaker => self.market_maker.rate(),
        }
    }
}

impl Default for TieredRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Endpoint weight annotation
// ---------------------------------------------------------------------------

/// The request weight for one endpoint.
///
/// Injected into requests by the router via `axum::Extension<EndpointWeight>`.
/// The middleware reads this to update per-client weight counters and write
/// the `X-RateLimit-*` headers.
///
/// Standard weights:
/// - Market data (orderbook, trades, ticker): **1**
/// - Place order / amend: **5**
/// - Batch orders: **10**
/// - Cancel order: **1**
#[derive(Debug, Clone, Copy)]
pub struct EndpointWeight {
    /// Token weight consumed by this endpoint.
    pub weight: u32,
    /// Whether this request counts toward the order placement counter.
    pub is_order: bool,
}

impl EndpointWeight {
    /// Weight 1, not an order placement (default for most endpoints).
    pub const fn market() -> Self {
        Self { weight: 1, is_order: false }
    }

    /// Weight 5, counts as an order placement.
    pub const fn place_order() -> Self {
        Self { weight: 5, is_order: true }
    }

    /// Weight 10, counts as an order placement (batch).
    pub const fn batch_order() -> Self {
        Self { weight: 10, is_order: true }
    }

    /// Weight 1, not an order placement (cancel).
    pub const fn cancel_order() -> Self {
        Self { weight: 1, is_order: false }
    }
}

impl Default for EndpointWeight {
    fn default() -> Self {
        Self::market()
    }
}

// ---------------------------------------------------------------------------
// Middleware
// ---------------------------------------------------------------------------

/// Axum middleware that rate-limits requests using tiered buckets and injects
/// `X-RateLimit-*` headers into every allowed response.
///
/// For authenticated requests (with `X-API-Key` header), the key is validated
/// against the `ApiKeyStore` extension before assigning the `Authenticated`
/// tier.  An `x-api-key` header whose value is not present or is inactive in
/// the store is treated as unauthenticated (falls back to `Default` tier).
/// This prevents revoked or fabricated API keys from receiving elevated limits.
pub async fn rate_limit_middleware(
    Extension(limiter): Extension<TieredRateLimiter>,
    Extension(key_store): Extension<Option<Arc<ApiKeyStore>>>,
    request: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Response {
    // Determine tier + key from API key header (if present)
    let (key, tier) = if let Some(api_key) = request
        .headers()
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
    {
        // Only grant Authenticated tier when the key exists and is active in
        // the store.  Unknown or revoked keys fall back to the Default tier so
        // they cannot exploit the higher burst allowance.
        let is_valid = key_store
            .as_ref()
            .and_then(|store| store.get(api_key))
            .map(|k| k.is_active)
            .unwrap_or(false);

        if is_valid {
            (api_key.to_string(), RateTier::Authenticated)
        } else {
            // Invalid/revoked key: rate-limit by key string (not IP) so a
            // flood of bad-key requests does not penalise other clients that
            // share the same IP.
            (api_key.to_string(), RateTier::Default)
        }
    } else {
        // Unauthenticated: use IP as key
        let ip = request
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(',').next())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        (ip, RateTier::Default)
    };

    // Consume one rate-limit token.
    if !limiter.check(&key, tier) {
        tracing::warn!(key = %key, tier = ?tier, "rate limit exceeded");
        let body = serde_json::json!({
            "error": "Rate limit exceeded",
            "retry_after": 1,
        });
        return (StatusCode::TOO_MANY_REQUESTS, axum::Json(body)).into_response();
    }

    // Read endpoint weight (defaults to 1 if not set by the router).
    let endpoint_weight = request
        .extensions()
        .get::<EndpointWeight>()
        .copied()
        .unwrap_or_default();

    // Record weight and obtain window totals for the response headers.
    let (used_weight, order_count) =
        limiter.record_weight(&key, tier, endpoint_weight.weight, endpoint_weight.is_order);

    let rate_limit = limiter.limit_for(tier) as u32;

    // Run the actual handler.
    let mut response = next.run(request).await;

    // Inject rate-limit usage headers.
    let headers = response.headers_mut();
    if let Ok(v) = used_weight.to_string().parse() {
        headers.insert(
            axum::http::HeaderName::from_static("x-ratelimit-used-weight"),
            v,
        );
    }
    if let Ok(v) = order_count.to_string().parse() {
        headers.insert(
            axum::http::HeaderName::from_static("x-ratelimit-order-count"),
            v,
        );
    }
    if let Ok(v) = rate_limit.to_string().parse() {
        headers.insert(
            axum::http::HeaderName::from_static("x-ratelimit-limit"),
            v,
        );
    }

    response
}

// ── RateLimitStore abstraction ────────────────────────────────────────────────

/// Trait abstracting the rate-limit backing store.
///
/// Phase 1: in-memory. Phase 2: Redis-backed for multi-node deployments.
pub trait RateLimitStore: Send + Sync {
    /// Try to consume one token for `key` with the given `rate` (tokens/sec) and
    /// `max` (burst capacity).  Returns `true` if the request is allowed.
    fn try_consume(&self, key: &str, rate: f64, max: f64) -> bool;

    /// Record `weight` units for `key` and return `(used_weight, order_count)`.
    fn record_weight(&self, key: &str, weight: u32, is_order: bool) -> (u32, u32);
}

/// In-memory [`RateLimitStore`] with per-call rate/max configuration.
pub struct InMemoryRateLimitStore {
    buckets: Mutex<HashMap<String, TokenBucket>>,
    weights: Mutex<HashMap<String, WeightBucket>>,
}

impl InMemoryRateLimitStore {
    pub fn new() -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            weights: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryRateLimitStore {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimitStore for InMemoryRateLimitStore {
    fn try_consume(&self, key: &str, rate: f64, max: f64) -> bool {
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        let bucket = buckets
            .entry(key.to_string())
            .or_insert_with(|| TokenBucket::new(rate, max));
        bucket.try_consume()
    }

    fn record_weight(&self, key: &str, weight: u32, is_order: bool) -> (u32, u32) {
        let mut weights = self.weights.lock().unwrap_or_else(|e| e.into_inner());
        let wb = weights
            .entry(key.to_string())
            .or_insert_with(WeightBucket::new);
        wb.record(weight, is_order);
        (wb.used_weight, wb.order_count)
    }
}

/// Create a default [`RateLimitStore`] suitable for production use.
pub fn create_rate_limit_store() -> InMemoryRateLimitStore {
    InMemoryRateLimitStore::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn allows_requests_within_limit() {
        // 10 rps, burst 10 — bucket starts full, so all 10 should pass.
        let limiter = RateLimiter::new(10, 10);
        for i in 0..10 {
            assert!(limiter.check("client-a"), "request {i} should be allowed");
        }
    }

    #[test]
    fn blocks_excess_requests() {
        // burst 3, rate 1 rps — after 3 requests the 4th must be blocked.
        let limiter = RateLimiter::new(1, 3);
        assert!(limiter.check("client-b"));
        assert!(limiter.check("client-b"));
        assert!(limiter.check("client-b"));
        assert!(!limiter.check("client-b"), "4th request should be blocked");
    }

    #[test]
    fn burst_allows_above_steady_rate() {
        // rate = 1 rps but burst = 5 — bucket starts full so 5 pass immediately.
        let limiter = RateLimiter::new(1, 5);
        let allowed: usize = (0..5).filter(|_| limiter.check("burst-client")).count();
        assert_eq!(allowed, 5, "should allow full burst");
        // 6th immediately after must be blocked
        assert!(!limiter.check("burst-client"));
    }

    #[test]
    fn tokens_refill_over_time() {
        // rate 10 rps, burst 1 — start with 1 token, consume it.
        let limiter = RateLimiter::new(10, 1);
        assert!(limiter.check("refill-client")); // consumes the only token
        assert!(!limiter.check("refill-client")); // empty

        // Wait 200ms → ~2 tokens added but bucket caps at max=1
        thread::sleep(Duration::from_millis(200));
        assert!(limiter.check("refill-client"), "should be allowed after refill");
    }

    #[test]
    fn different_keys_have_independent_buckets() {
        // burst 1 — exhaust key-x without affecting key-y.
        let limiter = RateLimiter::new(1, 1);
        assert!(limiter.check("key-x"));
        assert!(!limiter.check("key-x"), "key-x exhausted");
        // key-y starts fresh
        assert!(limiter.check("key-y"), "key-y should still be allowed");
    }

    #[test]
    fn zero_rate_blocks_after_burst() {
        // Unusual edge: even rate = 1 with burst = 0 means no tokens ever.
        // We test rate = 1 burst = 2 → only 2 requests before blocking.
        let limiter = RateLimiter::new(1, 2);
        assert!(limiter.check("c"));
        assert!(limiter.check("c"));
        assert!(!limiter.check("c"));
    }

    #[test]
    fn weight_accumulates_within_window() {
        let limiter = RateLimiter::new(100, 100);
        let (w1, _) = limiter.record_weight("user-1", 5, true);
        let (w2, o2) = limiter.record_weight("user-1", 5, false);
        assert_eq!(w1, 5);
        assert_eq!(w2, 10);
        assert_eq!(o2, 1, "only first call was an order");
    }

    #[test]
    fn weight_independent_per_key() {
        let limiter = RateLimiter::new(100, 100);
        limiter.record_weight("alice", 5, true);
        let (bob_weight, _) = limiter.record_weight("bob", 1, false);
        assert_eq!(bob_weight, 1, "bob's window is independent");
    }

    #[test]
    fn endpoint_weight_defaults() {
        let w = EndpointWeight::default();
        assert_eq!(w.weight, 1);
        assert!(!w.is_order);

        let w = EndpointWeight::place_order();
        assert_eq!(w.weight, 5);
        assert!(w.is_order);

        let w = EndpointWeight::batch_order();
        assert_eq!(w.weight, 10);
        assert!(w.is_order);
    }
}
