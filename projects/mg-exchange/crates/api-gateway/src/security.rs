//! Security hardening: per-endpoint rate limiting, IP reputation tracking,
//! brute-force protection, security event logging, and session management.
//!
//! This module is concerned with *policy* — what limits apply to which groups
//! of endpoints — and with the in-memory bookkeeping needed to enforce them.
//! The actual token-bucket machinery lives in [`crate::rate_limit`].

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Per-endpoint rate limit config ──────────────────────────────────────────

/// A named rate limit configuration for one group of endpoints.
///
/// Limits are expressed as (requests, window_seconds) pairs so the same
/// config can be displayed to clients in the `Retry-After` header without
/// floating-point arithmetic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Human-readable name for this endpoint group, e.g. `"auth"`.
    pub name: String,
    /// Maximum requests allowed within `window_secs`.
    pub max_requests: u32,
    /// Rolling window in seconds.
    pub window_secs: u32,
    /// Burst allowance — how many tokens the bucket starts with (and caps at).
    pub burst: u32,
}

impl RateLimitConfig {
    /// Auth endpoints: 5 requests per minute.
    pub fn auth() -> Self {
        Self { name: "auth".into(), max_requests: 5, window_secs: 60, burst: 5 }
    }

    /// Order placement: 100 requests per second per user.
    pub fn order_placement() -> Self {
        Self { name: "order_placement".into(), max_requests: 100, window_secs: 1, burst: 100 }
    }

    /// Market data: 300 requests per second per IP.
    pub fn market_data() -> Self {
        Self { name: "market_data".into(), max_requests: 300, window_secs: 1, burst: 300 }
    }

    /// Admin endpoints: 30 requests per minute per key.
    pub fn admin() -> Self {
        Self { name: "admin".into(), max_requests: 30, window_secs: 60, burst: 30 }
    }

    /// Compute the equivalent refill rate (tokens / second).
    pub fn rate_per_second(&self) -> f64 {
        f64::from(self.max_requests) / f64::from(self.window_secs)
    }
}

/// Build the default endpoint-group → config map.
pub fn default_rate_limit_configs() -> HashMap<String, RateLimitConfig> {
    let mut map = HashMap::new();
    let auth = RateLimitConfig::auth();
    let orders = RateLimitConfig::order_placement();
    let market = RateLimitConfig::market_data();
    let admin = RateLimitConfig::admin();
    map.insert(auth.name.clone(), auth);
    map.insert(orders.name.clone(), orders);
    map.insert(market.name.clone(), market);
    map.insert(admin.name.clone(), admin);
    map
}

// ── IP reputation / brute-force block ───────────────────────────────────────

/// Policy constants for brute-force detection.
pub const FAILED_LOGIN_WINDOW_SECS: u64 = 300;   // 5 minutes
pub const FAILED_LOGIN_THRESHOLD: u32 = 10;       // block after N failures
pub const BLOCK_DURATION_SECS: u64 = 1800;        // 30 minutes

/// Policy constants for 2FA brute-force detection.
pub const TOTP_ATTEMPT_WINDOW_SECS: u64 = 900;    // 15 minutes
pub const TOTP_MAX_ATTEMPTS: u32 = 5;

/// Per-IP tracking state.
#[derive(Debug, Clone)]
pub struct IpRecord {
    /// Count of failed login attempts within the current window.
    pub failed_attempts: u32,
    /// Wall clock of the first failure in the current window.
    pub window_start: Instant,
    /// When the block expires, if the IP is currently blocked.
    pub blocked_until: Option<Instant>,
    /// Human-readable ISO timestamp for the block expiry (serialisable).
    pub blocked_until_ts: Option<DateTime<Utc>>,
}

impl IpRecord {
    fn new() -> Self {
        Self {
            failed_attempts: 0,
            window_start: Instant::now(),
            blocked_until: None,
            blocked_until_ts: None,
        }
    }

    /// Returns `true` if this IP is currently blocked.
    pub fn is_blocked(&self) -> bool {
        match self.blocked_until {
            Some(until) => Instant::now() < until,
            None => false,
        }
    }
}

/// Thread-safe IP reputation tracker.
///
/// Records failed authentication attempts and auto-blocks IPs that exceed the
/// threshold within the observation window.
#[derive(Debug, Clone)]
pub struct IpReputationTracker {
    inner: Arc<Mutex<HashMap<String, IpRecord>>>,
}

impl IpReputationTracker {
    pub fn new() -> Self {
        Self { inner: Arc::new(Mutex::new(HashMap::new())) }
    }

    /// Record a failed login from `ip`. Returns `true` if the IP should now be blocked.
    pub fn record_failed_login(&self, ip: &str) -> bool {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let record = map.entry(ip.to_string()).or_insert_with(IpRecord::new);

        let now = Instant::now();
        let elapsed = now.duration_since(record.window_start).as_secs();

        // Reset window if it has expired.
        if elapsed > FAILED_LOGIN_WINDOW_SECS {
            record.failed_attempts = 0;
            record.window_start = now;
        }

        record.failed_attempts += 1;

        if record.failed_attempts >= FAILED_LOGIN_THRESHOLD {
            let block_expiry = now + std::time::Duration::from_secs(BLOCK_DURATION_SECS);
            record.blocked_until = Some(block_expiry);
            record.blocked_until_ts = Some(Utc::now() + chrono::Duration::seconds(BLOCK_DURATION_SECS as i64));
            tracing::warn!(
                ip = %ip,
                attempts = record.failed_attempts,
                "IP blocked after repeated failed logins"
            );
            true
        } else {
            false
        }
    }

    /// Returns `true` if the IP is currently blocked.
    pub fn is_blocked(&self, ip: &str) -> bool {
        let map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.get(ip).map(|r| r.is_blocked()).unwrap_or(false)
    }

    /// Clear the failed-attempt counter and block for `ip` (admin unblock).
    pub fn unblock(&self, ip: &str) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.remove(ip);
    }

    /// Admin push: manually block `ip` for `duration_secs`, bypassing the
    /// failed-login threshold.
    ///
    /// Used by the high-latency runbook to immediately cut off an abusive
    /// client while the incident is being investigated. Any existing record
    /// for this IP is replaced — this is an authoritative override.
    pub fn block_manually(&self, ip: &str, duration_secs: u64) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut record = IpRecord::new();
        record.blocked_until =
            Some(Instant::now() + std::time::Duration::from_secs(duration_secs));
        record.blocked_until_ts =
            Some(Utc::now() + chrono::Duration::seconds(duration_secs as i64));
        // Mark it as "maxed out" so that is_blocked() and anyone reading the
        // record can see this is an enforced block, not just a warning level.
        record.failed_attempts = FAILED_LOGIN_THRESHOLD;
        map.insert(ip.to_string(), record);
    }

    /// Snapshot of all currently-blocked IPs (for the admin endpoint).
    pub fn blocked_ips(&self) -> Vec<BlockedIpInfo> {
        let map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.iter()
            .filter(|(_, r)| r.is_blocked())
            .map(|(ip, r)| BlockedIpInfo {
                ip: ip.clone(),
                blocked_until: r.blocked_until_ts,
                failed_attempts: r.failed_attempts,
            })
            .collect()
    }

    /// Record a successful login — resets the failure counter for `ip`.
    pub fn record_success(&self, ip: &str) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(record) = map.get_mut(ip) {
            record.failed_attempts = 0;
            record.blocked_until = None;
            record.blocked_until_ts = None;
        }
    }
}

impl Default for IpReputationTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Summary returned by `GET /admin/security/blocked-ips`.
#[derive(Debug, Clone, Serialize)]
pub struct BlockedIpInfo {
    pub ip: String,
    pub blocked_until: Option<DateTime<Utc>>,
    pub failed_attempts: u32,
}

// ── 2FA brute-force protection ───────────────────────────────────────────────

/// Per-user 2FA attempt state.
#[derive(Debug, Clone)]
struct TotpRecord {
    attempts: u32,
    window_start: Instant,
}

/// Tracks 2FA verification attempts per user to prevent brute-force.
#[derive(Debug, Clone)]
pub struct TotpAttemptTracker {
    inner: Arc<Mutex<HashMap<Uuid, TotpRecord>>>,
}

impl TotpAttemptTracker {
    pub fn new() -> Self {
        Self { inner: Arc::new(Mutex::new(HashMap::new())) }
    }

    /// Record a failed 2FA attempt. Returns `true` if the user is now rate-limited.
    pub fn record_failed_attempt(&self, user_id: Uuid) -> bool {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let record = map.entry(user_id).or_insert(TotpRecord {
            attempts: 0,
            window_start: Instant::now(),
        });

        let now = Instant::now();
        let elapsed = now.duration_since(record.window_start).as_secs();

        if elapsed > TOTP_ATTEMPT_WINDOW_SECS {
            record.attempts = 0;
            record.window_start = now;
        }

        record.attempts += 1;

        if record.attempts > TOTP_MAX_ATTEMPTS {
            tracing::warn!(
                user_id = %user_id,
                attempts = record.attempts,
                "2FA brute-force threshold exceeded"
            );
            true
        } else {
            false
        }
    }

    /// Returns `true` if the user has exceeded the 2FA attempt limit.
    pub fn is_limited(&self, user_id: Uuid) -> bool {
        let map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match map.get(&user_id) {
            None => false,
            Some(r) => {
                let elapsed = Instant::now().duration_since(r.window_start).as_secs();
                if elapsed > TOTP_ATTEMPT_WINDOW_SECS {
                    false
                } else {
                    r.attempts > TOTP_MAX_ATTEMPTS
                }
            }
        }
    }

    /// Reset the counter after a successful verification.
    pub fn record_success(&self, user_id: Uuid) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.remove(&user_id);
    }
}

impl Default for TotpAttemptTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ── Security event log ───────────────────────────────────────────────────────

/// The type of security event that was observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityEventType {
    LoginSuccess,
    LoginFailed,
    ApiKeyCreated,
    WithdrawalRequested,
    TwoFaFailed,
    IpBlocked,
    SessionExpired,
    SessionRevoked,
}

/// A single security-relevant event.
#[derive(Debug, Clone, Serialize)]
pub struct SecurityEvent {
    pub id: Uuid,
    pub event_type: SecurityEventType,
    /// The affected user, if known (anonymous if login failed before user lookup).
    pub user_id: Option<Uuid>,
    /// Source IP address.
    pub ip: String,
    pub timestamp: DateTime<Utc>,
    /// Extra context (e.g. API key label, withdrawal amount).
    pub details: String,
}

impl SecurityEvent {
    pub fn new(
        event_type: SecurityEventType,
        user_id: Option<Uuid>,
        ip: impl Into<String>,
        details: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            event_type,
            user_id,
            ip: ip.into(),
            timestamp: Utc::now(),
            details: details.into(),
        }
    }
}

/// Filters for `GET /admin/security/events`.
#[derive(Debug, Clone, Deserialize)]
pub struct SecurityEventFilter {
    pub event_type: Option<SecurityEventType>,
    pub user_id: Option<Uuid>,
    pub ip: Option<String>,
    /// Return events at or after this Unix millisecond timestamp.
    pub since_ms: Option<i64>,
    /// Maximum number of results to return (default 100).
    pub limit: Option<usize>,
}

/// In-memory security event store with a configurable capacity cap.
#[derive(Debug)]
pub struct SecurityEventLog {
    events: Vec<SecurityEvent>,
    capacity: usize,
}

impl SecurityEventLog {
    pub fn new(capacity: usize) -> Self {
        Self { events: Vec::new(), capacity }
    }

    /// Append a new event. Oldest events are evicted when capacity is exceeded.
    pub fn append(&mut self, event: SecurityEvent) {
        if self.events.len() >= self.capacity {
            self.events.remove(0);
        }
        self.events.push(event);
    }

    /// Query events with optional filters. Returns events in reverse-chronological order.
    pub fn query(&self, filter: &SecurityEventFilter) -> Vec<SecurityEvent> {
        let limit = filter.limit.unwrap_or(100).min(1000);
        let since = filter.since_ms.map(|ms| {
            DateTime::<Utc>::from_timestamp_millis(ms).unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
        });

        self.events
            .iter()
            .rev()
            .filter(|e| {
                if let Some(et) = filter.event_type {
                    if e.event_type != et {
                        return false;
                    }
                }
                if let Some(uid) = filter.user_id {
                    if e.user_id != Some(uid) {
                        return false;
                    }
                }
                if let Some(ref ip) = filter.ip {
                    if &e.ip != ip {
                        return false;
                    }
                }
                if let Some(since_dt) = since {
                    if e.timestamp < since_dt {
                        return false;
                    }
                }
                true
            })
            .take(limit)
            .cloned()
            .collect()
    }
}

impl Default for SecurityEventLog {
    fn default() -> Self {
        Self::new(10_000)
    }
}

// ── Session expiry constants ─────────────────────────────────────────────────

/// Maximum number of concurrent active sessions per user.
pub const MAX_SESSIONS_PER_USER: usize = 10;
/// Sessions older than this (in days) are treated as expired.
pub const SESSION_EXPIRY_DAYS: i64 = 30;

// ── ReputationStore abstraction ───────────────────────────────────────────────

/// Trait abstracting IP reputation storage.
///
/// Phase 1: in-memory. Phase 2: Redis-backed for multi-node deployments.
pub trait ReputationStore: Send + Sync {
    /// Record an authentication failure from `ip`.  Returns `true` if the IP
    /// should now be blocked.
    fn record_failure(&self, ip: &str) -> bool;
    /// Record a successful authentication from `ip`, resetting the failure counter.
    fn record_success(&self, ip: &str);
    /// Returns `true` if `ip` is currently blocked.
    fn is_blocked(&self, ip: &str) -> bool;
    /// Unblock `ip`.
    fn unblock(&self, ip: &str);
}

/// In-memory implementation of [`ReputationStore`] backed by [`IpReputationTracker`].
pub struct InMemoryReputationStore {
    inner: IpReputationTracker,
}

impl InMemoryReputationStore {
    pub fn new() -> Self {
        Self {
            inner: IpReputationTracker::new(),
        }
    }
}

impl Default for InMemoryReputationStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ReputationStore for InMemoryReputationStore {
    fn record_failure(&self, ip: &str) -> bool {
        self.inner.record_failed_login(ip)
    }

    fn record_success(&self, ip: &str) {
        self.inner.record_success(ip);
    }

    fn is_blocked(&self, ip: &str) -> bool {
        self.inner.is_blocked(ip)
    }

    fn unblock(&self, ip: &str) {
        self.inner.unblock(ip);
    }
}

/// Create a default [`ReputationStore`] suitable for production use.
pub fn create_reputation_store() -> InMemoryReputationStore {
    InMemoryReputationStore::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ip_blocked_after_threshold() {
        let tracker = IpReputationTracker::new();
        let ip = "1.2.3.4";

        // Under threshold — should not be blocked.
        for _ in 0..(FAILED_LOGIN_THRESHOLD - 1) {
            tracker.record_failed_login(ip);
        }
        assert!(!tracker.is_blocked(ip));

        // Reaching the threshold — now blocked.
        let blocked = tracker.record_failed_login(ip);
        assert!(blocked);
        assert!(tracker.is_blocked(ip));
    }

    #[test]
    fn ip_unblock_clears_record() {
        let tracker = IpReputationTracker::new();
        let ip = "5.6.7.8";

        for _ in 0..FAILED_LOGIN_THRESHOLD {
            tracker.record_failed_login(ip);
        }
        assert!(tracker.is_blocked(ip));

        tracker.unblock(ip);
        assert!(!tracker.is_blocked(ip));
    }

    #[test]
    fn success_resets_failure_count() {
        let tracker = IpReputationTracker::new();
        let ip = "9.10.11.12";

        for _ in 0..(FAILED_LOGIN_THRESHOLD - 1) {
            tracker.record_failed_login(ip);
        }
        // Not yet blocked.
        assert!(!tracker.is_blocked(ip));

        // Successful login resets counter.
        tracker.record_success(ip);

        // Further failures start from zero again.
        for _ in 0..(FAILED_LOGIN_THRESHOLD - 1) {
            tracker.record_failed_login(ip);
        }
        assert!(!tracker.is_blocked(ip));
    }

    #[test]
    fn totp_rate_limit_enforced() {
        let tracker = TotpAttemptTracker::new();
        let uid = Uuid::new_v4();

        for _ in 0..TOTP_MAX_ATTEMPTS {
            tracker.record_failed_attempt(uid);
        }
        assert!(!tracker.is_limited(uid)); // exactly at threshold — not yet over

        let over = tracker.record_failed_attempt(uid);
        assert!(over);
        assert!(tracker.is_limited(uid));
    }

    #[test]
    fn totp_success_resets_limiter() {
        let tracker = TotpAttemptTracker::new();
        let uid = Uuid::new_v4();

        for _ in 0..=TOTP_MAX_ATTEMPTS {
            tracker.record_failed_attempt(uid);
        }
        assert!(tracker.is_limited(uid));

        tracker.record_success(uid);
        assert!(!tracker.is_limited(uid));
    }

    #[test]
    fn security_event_log_query_filters() {
        let mut log = SecurityEventLog::new(100);

        log.append(SecurityEvent::new(SecurityEventType::LoginFailed, None, "1.1.1.1", ""));
        log.append(SecurityEvent::new(SecurityEventType::LoginSuccess, Some(Uuid::new_v4()), "2.2.2.2", ""));
        log.append(SecurityEvent::new(SecurityEventType::ApiKeyCreated, Some(Uuid::new_v4()), "3.3.3.3", "key: test"));

        let filter = SecurityEventFilter {
            event_type: Some(SecurityEventType::LoginFailed),
            user_id: None,
            ip: None,
            since_ms: None,
            limit: None,
        };
        let results = log.query(&filter);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].event_type, SecurityEventType::LoginFailed);
    }

    #[test]
    fn security_event_log_evicts_oldest() {
        let mut log = SecurityEventLog::new(3);
        for i in 0..4u32 {
            log.append(SecurityEvent::new(
                SecurityEventType::LoginSuccess,
                None,
                format!("{i}.0.0.0"),
                "",
            ));
        }
        // Should hold 3 entries; the first one was evicted.
        assert_eq!(log.events.len(), 3);
    }

    #[test]
    fn default_rate_limit_configs_present() {
        let configs = default_rate_limit_configs();
        assert!(configs.contains_key("auth"));
        assert!(configs.contains_key("order_placement"));
        assert!(configs.contains_key("market_data"));
        assert!(configs.contains_key("admin"));
    }

    #[test]
    fn blocked_ips_snapshot() {
        let tracker = IpReputationTracker::new();
        let ip = "10.0.0.1";

        for _ in 0..FAILED_LOGIN_THRESHOLD {
            tracker.record_failed_login(ip);
        }

        let blocked = tracker.blocked_ips();
        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].ip, ip);
    }

    #[test]
    fn block_manually_flags_the_ip_as_blocked() {
        let tracker = IpReputationTracker::new();
        let ip = "1.2.3.4";

        assert!(!tracker.is_blocked(ip), "fresh tracker must not report blocked");

        tracker.block_manually(ip, 60);

        assert!(tracker.is_blocked(ip), "after block_manually the ip must be blocked");
        let blocked = tracker.blocked_ips();
        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].ip, ip);
        assert!(
            blocked[0].blocked_until.is_some(),
            "manual block must set an expiry timestamp",
        );
    }

    #[test]
    fn block_manually_replaces_existing_record() {
        let tracker = IpReputationTracker::new();
        let ip = "1.2.3.4";

        // Accumulate some failed logins without triggering the auto-block.
        for _ in 0..3 {
            tracker.record_failed_login(ip);
        }
        assert!(!tracker.is_blocked(ip));

        // Admin block short-circuits — the IP is blocked immediately.
        tracker.block_manually(ip, 60);
        assert!(tracker.is_blocked(ip));

        // Admin unblock reverses it.
        tracker.unblock(ip);
        assert!(!tracker.is_blocked(ip));
    }
}
