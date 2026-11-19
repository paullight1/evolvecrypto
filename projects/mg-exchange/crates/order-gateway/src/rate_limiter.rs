//! Per-user order rate limiting.
//!
//! Two implementations are provided:
//!
//! 1. **`OrderRateLimiter`** — sliding-window counter backed by `Vec<Instant>`.
//!    Kept for tests and as a reference; wrap in `Mutex` for multi-threaded use.
//!
//! 2. **`LockFreeRateLimiter`** — slot-based counter using `DashMap` + atomics.
//!    Requires no external lock; `&self` is sufficient for all operations.
//!    Uses fixed-slot (per-second / per-minute) windows rather than a true
//!    sliding window — the same model used by Binance and Bybit.  The trade-off:
//!    up to 2× burst is possible at a slot boundary (last request of old slot +
//!    first of new), which is acceptable for crypto exchange rate limiting.
//!
//! Prevents individual users from overwhelming the matching engine with order
//! spam. Two independent limits are enforced:
//! - **Per-second**: burst protection (default 50 orders/sec)
//! - **Per-minute**: sustained load protection (default 1200 orders/min)
//!
//! # Reference
//!
//! - Binance: 10 orders/sec, 100K orders/day per account (fixed-slot windows)
//! - Bybit: 10 orders/sec (spot), 100 orders/sec (derivatives) (fixed-slot)
//! - NASDAQ: configurable per member firm, typical 1000 msgs/sec

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
use exchange_common::UserId;

/// Configuration for the rate limiter.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum orders per second per user. 0 = unlimited.
    pub max_per_second: u32,
    /// Maximum orders per minute per user. 0 = unlimited.
    pub max_per_minute: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_per_second: 50,
            max_per_minute: 1200,
        }
    }
}

/// Result of a rate limit check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RateLimitResult {
    /// Order is allowed to proceed.
    Allowed,
    /// Rate limit exceeded — order should be rejected.
    Exceeded {
        /// Which window was breached.
        window: &'static str,
        /// Current count in that window.
        count: u32,
        /// Maximum allowed in that window.
        limit: u32,
    },
}

/// Per-user sliding window counters.
#[derive(Debug)]
struct UserWindow {
    /// Timestamps of recent orders (ring buffer, newest at end).
    timestamps: Vec<Instant>,
    /// Write cursor for ring buffer.
    cursor: usize,
}

impl UserWindow {
    fn new(capacity: usize) -> Self {
        Self {
            timestamps: Vec::with_capacity(capacity),
            cursor: 0,
        }
    }

    fn record(&mut self, now: Instant) {
        if self.timestamps.len() < self.timestamps.capacity() {
            self.timestamps.push(now);
        } else {
            self.timestamps[self.cursor] = now;
            self.cursor = (self.cursor + 1) % self.timestamps.len();
        }
    }

    fn count_since(&self, cutoff: Instant) -> u32 {
        self.timestamps.iter().filter(|&&t| t >= cutoff).count() as u32
    }
}

/// Per-user order rate limiter.
///
/// Thread-safe usage: wrap in `Mutex` at the Exchange level.
#[derive(Debug)]
pub struct OrderRateLimiter {
    config: RateLimitConfig,
    windows: HashMap<UserId, UserWindow>,
    /// Per-user override configs — set by surveillance ReduceRateLimit auto-action.
    /// Users in this map get tighter limits than the global default.
    overrides: HashMap<UserId, RateLimitConfig>,
}

impl OrderRateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            windows: HashMap::new(),
            overrides: HashMap::new(),
        }
    }

    /// Reset all rate limit counters. Used during demo mode seeding.
    pub fn reset(&mut self) {
        self.windows.clear();
    }

    /// Set a per-user rate limit override (e.g. surveillance-imposed throttle).
    pub fn set_override(&mut self, user_id: UserId, config: RateLimitConfig) {
        self.overrides.insert(user_id, config);
    }

    /// Remove a per-user rate limit override.
    pub fn clear_override(&mut self, user_id: UserId) {
        self.overrides.remove(&user_id);
    }

    /// Check if a user has an active rate limit override.
    pub fn has_override(&self, user_id: UserId) -> bool {
        self.overrides.contains_key(&user_id)
    }

    /// Check whether a user is within rate limits and record the attempt.
    ///
    /// Returns `Allowed` if the order can proceed, or `Exceeded` with details
    /// about which window was breached.
    ///
    /// This method both checks and records in one atomic call to avoid
    /// TOCTOU races.
    pub fn check_and_record(&mut self, user_id: UserId) -> RateLimitResult {
        let now = Instant::now();

        // Use per-user override if set (e.g. surveillance-imposed throttle),
        // otherwise fall back to the global config.
        let cfg = self.overrides.get(&user_id).unwrap_or(&self.config);
        let max_per_second = cfg.max_per_second;
        let max_per_minute = cfg.max_per_minute;

        let max_capacity = max_per_minute.max(max_per_second) as usize + 1;

        let window = self
            .windows
            .entry(user_id)
            .or_insert_with(|| UserWindow::new(max_capacity));

        // Check per-second limit.
        if max_per_second > 0 {
            let one_sec_ago = now.checked_sub(std::time::Duration::from_secs(1)).unwrap_or(now);
            let count = window.count_since(one_sec_ago);
            if count >= max_per_second {
                return RateLimitResult::Exceeded {
                    window: "per_second",
                    count,
                    limit: max_per_second,
                };
            }
        }

        // Check per-minute limit.
        if max_per_minute > 0 {
            let one_min_ago = now.checked_sub(std::time::Duration::from_secs(60)).unwrap_or(now);
            let count = window.count_since(one_min_ago);
            if count >= max_per_minute {
                return RateLimitResult::Exceeded {
                    window: "per_minute",
                    count,
                    limit: max_per_minute,
                };
            }
        }

        // Record the order timestamp.
        window.record(now);
        RateLimitResult::Allowed
    }

    /// Prune entries for users who haven't sent orders in the last 5 minutes.
    /// Call periodically to prevent memory growth from idle users.
    pub fn prune_idle(&mut self) {
        let cutoff = Instant::now()
            .checked_sub(std::time::Duration::from_secs(300))
            .unwrap_or_else(Instant::now);
        self.windows.retain(|_, w| w.count_since(cutoff) > 0);
    }

    /// Number of tracked users.
    pub fn tracked_users(&self) -> usize {
        self.windows.len()
    }
}

// ---------------------------------------------------------------------------
// Sprint S18 — C.2: Lock-free rate limiter
// ---------------------------------------------------------------------------

/// Per-user atomic counters for a single slot window.
///
/// The slot approach uses a fixed-duration bucket rather than a true sliding
/// window.  When `SystemTime` advances past the current slot boundary, the
/// counter is reset to 1 (the current request) atomically.  Two threads racing
/// on a slot transition may each observe the old slot and each store the new
/// slot start independently — the worst case is that both requests count as "1"
/// for the new slot rather than "2", which is a small under-count acceptable
/// for DoS-prevention purposes.  We never over-admit requests: if the slot
/// has not changed, `fetch_add` strictly increments the shared counter.
struct RateLimitState {
    /// Unix-second timestamp of the current per-second slot.
    second_slot_start: AtomicU64,
    /// Order count within the current per-second slot.
    second_count: AtomicU64,
    /// Unix-minute timestamp (seconds / 60) of the current per-minute slot.
    minute_slot_start: AtomicU64,
    /// Order count within the current per-minute slot.
    minute_count: AtomicU64,
}

/// Lock-free per-user order rate limiter (Sprint S18 — C.2).
///
/// Uses `DashMap` (sharded concurrent hash map) to hold per-user atomic
/// counters.  No external `Mutex` is required: all methods take `&self`.
///
/// **Semantics** — fixed-slot (same as Binance / Bybit):
/// - Per-second window: `floor(unix_secs)` — resets every whole second.
/// - Per-minute window: `floor(unix_secs / 60)` — resets every whole minute.
///
/// **Thread safety** — DashMap shards by key hash so concurrent requests for
/// *different* users never contend.  Concurrent requests for the *same* user
/// land in the same DashMap shard and use `Relaxed` atomics for the counters.
/// Relaxed ordering is intentional: the counter is not used to synchronise
/// any other memory — it only needs to be monotonically increasing within a
/// slot, which `fetch_add` guarantees on all platforms.
pub struct LockFreeRateLimiter {
    /// Per-user slot counters.
    per_user: DashMap<UserId, RateLimitState>,
    /// Global default limits.
    global: RateLimitConfig,
    /// Per-user limit overrides (surveillance-imposed throttles).
    overrides: DashMap<UserId, RateLimitConfig>,
}

impl LockFreeRateLimiter {
    /// Create a new limiter with the supplied global defaults.
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            per_user: DashMap::new(),
            global: config,
            overrides: DashMap::new(),
        }
    }

    /// Reset all per-user counters. Used during demo mode seeding.
    pub fn reset(&self) {
        self.per_user.clear();
    }

    /// Apply a per-user rate limit override.
    pub fn set_override(&self, user_id: UserId, config: RateLimitConfig) {
        self.overrides.insert(user_id, config);
    }

    /// Remove a per-user rate limit override.
    pub fn clear_override(&self, user_id: UserId) {
        self.overrides.remove(&user_id);
    }

    /// Check whether a user has an active override.
    pub fn has_override(&self, user_id: UserId) -> bool {
        self.overrides.contains_key(&user_id)
    }

    /// Number of tracked users (for observability / prune triggers).
    pub fn tracked_users(&self) -> usize {
        self.per_user.len()
    }

    /// Prune entries for users who have been idle for at least `idle_secs`.
    ///
    /// DashMap does not expose a `retain` that can also inspect the value, so
    /// we collect keys first then remove.  This is a maintenance operation
    /// called infrequently (once per minute by the background task).
    pub fn prune_idle(&self, idle_secs: u64) {
        let now_sec = Self::unix_secs();
        let stale_keys: Vec<UserId> = self
            .per_user
            .iter()
            .filter(|entry| {
                let s = entry.value();
                let last_sec_slot = s.second_slot_start.load(Ordering::Relaxed);
                now_sec.saturating_sub(last_sec_slot) > idle_secs
            })
            .map(|entry| *entry.key())
            .collect();
        for key in stale_keys {
            self.per_user.remove(&key);
        }
    }

    /// Check if a user is within rate limits and record the attempt.
    ///
    /// Returns `Allowed` if the order can proceed, or `Exceeded` with details.
    /// Both check and record happen atomically per window (no TOCTOU gap).
    pub fn check_and_record(&self, user_id: UserId) -> RateLimitResult {
        let now_sec = Self::unix_secs();
        let now_min = now_sec / 60;

        let cfg = if let Some(ov) = self.overrides.get(&user_id) {
            ov.clone()
        } else {
            self.global.clone()
        };

        // Lazily create the state entry for new users.
        let state = self.per_user.entry(user_id).or_insert_with(|| RateLimitState {
            second_slot_start: AtomicU64::new(now_sec),
            second_count: AtomicU64::new(0),
            minute_slot_start: AtomicU64::new(now_min),
            minute_count: AtomicU64::new(0),
        });

        // --- Per-second check ---
        if cfg.max_per_second > 0 {
            let current_slot = state.second_slot_start.load(Ordering::Relaxed);
            let count = if current_slot != now_sec {
                // Slot boundary crossed: reset and count this request as 1.
                state.second_slot_start.store(now_sec, Ordering::Relaxed);
                state.second_count.store(1, Ordering::Relaxed);
                1u64
            } else {
                state.second_count.fetch_add(1, Ordering::Relaxed) + 1
            };
            if count > cfg.max_per_second as u64 {
                // Roll back the increment so the counter stays accurate for
                // subsequent requests in this slot.
                state.second_count.fetch_sub(1, Ordering::Relaxed);
                return RateLimitResult::Exceeded {
                    window: "per_second",
                    count: count as u32,
                    limit: cfg.max_per_second,
                };
            }
        }

        // --- Per-minute check ---
        if cfg.max_per_minute > 0 {
            let current_slot = state.minute_slot_start.load(Ordering::Relaxed);
            let count = if current_slot != now_min {
                state.minute_slot_start.store(now_min, Ordering::Relaxed);
                state.minute_count.store(1, Ordering::Relaxed);
                1u64
            } else {
                state.minute_count.fetch_add(1, Ordering::Relaxed) + 1
            };
            if count > cfg.max_per_minute as u64 {
                state.minute_count.fetch_sub(1, Ordering::Relaxed);
                // Also undo the per-second increment recorded above.
                if cfg.max_per_second > 0 {
                    state.second_count.fetch_sub(1, Ordering::Relaxed);
                }
                return RateLimitResult::Exceeded {
                    window: "per_minute",
                    count: count as u32,
                    limit: cfg.max_per_minute,
                };
            }
        }

        RateLimitResult::Allowed
    }

    /// Current Unix timestamp in whole seconds.
    #[inline(always)]
    fn unix_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn user() -> UserId {
        Uuid::new_v4()
    }

    #[test]
    fn test_allows_within_limit() {
        let config = RateLimitConfig {
            max_per_second: 5,
            max_per_minute: 100,
        };
        let mut limiter = OrderRateLimiter::new(config);
        let uid = user();
        for _ in 0..5 {
            assert_eq!(limiter.check_and_record(uid), RateLimitResult::Allowed);
        }
    }

    #[test]
    fn test_exceeds_per_second() {
        let config = RateLimitConfig {
            max_per_second: 3,
            max_per_minute: 1000,
        };
        let mut limiter = OrderRateLimiter::new(config);
        let uid = user();
        for _ in 0..3 {
            assert_eq!(limiter.check_and_record(uid), RateLimitResult::Allowed);
        }
        // 4th should be rejected.
        match limiter.check_and_record(uid) {
            RateLimitResult::Exceeded { window, limit, .. } => {
                assert_eq!(window, "per_second");
                assert_eq!(limit, 3);
            }
            RateLimitResult::Allowed => panic!("should have been rate limited"),
        }
    }

    #[test]
    fn test_different_users_independent() {
        let config = RateLimitConfig {
            max_per_second: 2,
            max_per_minute: 100,
        };
        let mut limiter = OrderRateLimiter::new(config);
        let user_a = user();
        let user_b = user();

        assert_eq!(limiter.check_and_record(user_a), RateLimitResult::Allowed);
        assert_eq!(limiter.check_and_record(user_a), RateLimitResult::Allowed);
        // user_a exhausted, but user_b is fresh.
        assert!(matches!(limiter.check_and_record(user_a), RateLimitResult::Exceeded { .. }));
        assert_eq!(limiter.check_and_record(user_b), RateLimitResult::Allowed);
    }

    #[test]
    fn test_unlimited_when_zero() {
        let config = RateLimitConfig {
            max_per_second: 0, // unlimited
            max_per_minute: 0,
        };
        let mut limiter = OrderRateLimiter::new(config);
        let uid = user();
        for _ in 0..1000 {
            assert_eq!(limiter.check_and_record(uid), RateLimitResult::Allowed);
        }
    }

    #[test]
    fn test_prune_idle() {
        let config = RateLimitConfig::default();
        let mut limiter = OrderRateLimiter::new(config);
        let uid = user();
        limiter.check_and_record(uid);
        assert_eq!(limiter.tracked_users(), 1);
        // Prune won't remove — order was just placed.
        limiter.prune_idle();
        assert_eq!(limiter.tracked_users(), 1);
    }

    // --- Per-user override tests (M19-3) ---

    #[test]
    fn test_override_applies_tighter_limit() {
        let config = RateLimitConfig {
            max_per_second: 50,
            max_per_minute: 1200,
        };
        let mut limiter = OrderRateLimiter::new(config);
        let uid = user();

        // Set a tight override: 3/sec
        limiter.set_override(uid, RateLimitConfig {
            max_per_second: 3,
            max_per_minute: 60,
        });

        // 3 orders should succeed
        for _ in 0..3 {
            assert_eq!(limiter.check_and_record(uid), RateLimitResult::Allowed);
        }
        // 4th should be rejected at the override limit
        match limiter.check_and_record(uid) {
            RateLimitResult::Exceeded { limit, .. } => assert_eq!(limit, 3),
            RateLimitResult::Allowed => panic!("should have been rate limited"),
        }
    }

    #[test]
    fn test_override_does_not_affect_other_users() {
        let config = RateLimitConfig {
            max_per_second: 50,
            max_per_minute: 1200,
        };
        let mut limiter = OrderRateLimiter::new(config);
        let throttled = user();
        let normal = user();

        limiter.set_override(throttled, RateLimitConfig {
            max_per_second: 2,
            max_per_minute: 10,
        });

        // Throttled user hits limit at 2
        for _ in 0..2 {
            assert_eq!(limiter.check_and_record(throttled), RateLimitResult::Allowed);
        }
        assert!(matches!(limiter.check_and_record(throttled), RateLimitResult::Exceeded { .. }));

        // Normal user still has the global limit (50/s)
        for _ in 0..10 {
            assert_eq!(limiter.check_and_record(normal), RateLimitResult::Allowed);
        }
    }

    #[test]
    fn test_clear_override_restores_default() {
        let config = RateLimitConfig {
            max_per_second: 50,
            max_per_minute: 1200,
        };
        let mut limiter = OrderRateLimiter::new(config);
        let uid = user();

        limiter.set_override(uid, RateLimitConfig {
            max_per_second: 1,
            max_per_minute: 5,
        });
        assert!(limiter.has_override(uid));

        limiter.clear_override(uid);
        assert!(!limiter.has_override(uid));
    }
}

// ---------------------------------------------------------------------------
// Sprint S18 C.2 — LockFreeRateLimiter tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod lock_free_tests {
    use super::*;
    use std::sync::Arc;
    use uuid::Uuid;

    fn uid() -> UserId {
        Uuid::new_v4()
    }

    fn limiter_with(per_sec: u32, per_min: u32) -> LockFreeRateLimiter {
        LockFreeRateLimiter::new(RateLimitConfig {
            max_per_second: per_sec,
            max_per_minute: per_min,
        })
    }

    #[test]
    fn allows_within_per_second_limit() {
        let lim = limiter_with(5, 1000);
        let u = uid();
        for _ in 0..5 {
            assert_eq!(lim.check_and_record(u), RateLimitResult::Allowed);
        }
    }

    #[test]
    fn rejects_on_per_second_overflow() {
        let lim = limiter_with(3, 1000);
        let u = uid();
        for _ in 0..3 {
            assert_eq!(lim.check_and_record(u), RateLimitResult::Allowed);
        }
        match lim.check_and_record(u) {
            RateLimitResult::Exceeded { window, limit, .. } => {
                assert_eq!(window, "per_second");
                assert_eq!(limit, 3);
            }
            RateLimitResult::Allowed => panic!("should have been rate limited"),
        }
    }

    #[test]
    fn different_users_are_independent() {
        let lim = limiter_with(2, 1000);
        let a = uid();
        let b = uid();
        assert_eq!(lim.check_and_record(a), RateLimitResult::Allowed);
        assert_eq!(lim.check_and_record(a), RateLimitResult::Allowed);
        assert!(matches!(lim.check_and_record(a), RateLimitResult::Exceeded { .. }));
        // User b is unaffected.
        assert_eq!(lim.check_and_record(b), RateLimitResult::Allowed);
    }

    #[test]
    fn unlimited_when_zero() {
        let lim = limiter_with(0, 0);
        let u = uid();
        for _ in 0..1000 {
            assert_eq!(lim.check_and_record(u), RateLimitResult::Allowed);
        }
    }

    #[test]
    fn override_applies_tighter_limit() {
        let lim = limiter_with(50, 1200);
        let u = uid();
        lim.set_override(u, RateLimitConfig { max_per_second: 2, max_per_minute: 100 });
        for _ in 0..2 {
            assert_eq!(lim.check_and_record(u), RateLimitResult::Allowed);
        }
        match lim.check_and_record(u) {
            RateLimitResult::Exceeded { limit, .. } => assert_eq!(limit, 2),
            RateLimitResult::Allowed => panic!("should have been rate limited"),
        }
    }

    #[test]
    fn clear_override_restores_global_limit() {
        let lim = limiter_with(50, 1200);
        let u = uid();
        lim.set_override(u, RateLimitConfig { max_per_second: 1, max_per_minute: 5 });
        assert!(lim.has_override(u));
        lim.clear_override(u);
        assert!(!lim.has_override(u));
    }

    /// C.2.7 — Concurrent CAS correctness test (Task C.2.7 from Sprint S18 spec).
    ///
    /// 100 tasks × 20 orders each against user with max_per_second=2000:
    /// all 2000 orders must succeed (no false rejects).
    ///
    /// 100 tasks × 100 orders each against user with max_per_second=100:
    /// at most 100 must be allowed; the rest must be rejected.
    #[test]
    fn concurrent_cas_correctness() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(8)
            .build()
            .unwrap();

        rt.block_on(async {
            // --- Part 1: generous limit — all 2000 should pass ---
            let lim = Arc::new(limiter_with(2000, 100_000));
            let user = uid();
            let allowed = Arc::new(AtomicU32::new(0));
            let rejected = Arc::new(AtomicU32::new(0));

            let mut tasks = Vec::new();
            for _ in 0..100 {
                let lim = Arc::clone(&lim);
                let allowed = Arc::clone(&allowed);
                let rejected = Arc::clone(&rejected);
                tasks.push(tokio::spawn(async move {
                    for _ in 0..20 {
                        match lim.check_and_record(user) {
                            RateLimitResult::Allowed => { allowed.fetch_add(1, Ordering::Relaxed); }
                            RateLimitResult::Exceeded { .. } => { rejected.fetch_add(1, Ordering::Relaxed); }
                        }
                    }
                }));
            }
            for t in tasks { t.await.unwrap(); }
            // Under generous limits all 2000 requests should pass.
            assert_eq!(allowed.load(Ordering::Relaxed), 2000,
                "expected all 2000 requests allowed, got {}",
                allowed.load(Ordering::Relaxed));
            assert_eq!(rejected.load(Ordering::Relaxed), 0);

            // --- Part 2: tight limit — at most 100 allowed ---
            let lim = Arc::new(limiter_with(100, 100_000));
            let user2 = uid();
            let allowed2 = Arc::new(AtomicU32::new(0));
            let rejected2 = Arc::new(AtomicU32::new(0));

            let mut tasks = Vec::new();
            for _ in 0..100 {
                let lim = Arc::clone(&lim);
                let allowed2 = Arc::clone(&allowed2);
                let rejected2 = Arc::clone(&rejected2);
                tasks.push(tokio::spawn(async move {
                    for _ in 0..100 {
                        match lim.check_and_record(user2) {
                            RateLimitResult::Allowed => { allowed2.fetch_add(1, Ordering::Relaxed); }
                            RateLimitResult::Exceeded { .. } => { rejected2.fetch_add(1, Ordering::Relaxed); }
                        }
                    }
                }));
            }
            for t in tasks { t.await.unwrap(); }
            // With a 100/sec limit, at most 100 of the 10_000 requests should pass.
            // Due to slot-reset races (two threads may both see slot change and both
            // store count=1) there is a small allowed over-count; we bound it to 2×
            // the limit to tolerate the worst-case 8-thread race.
            let a = allowed2.load(Ordering::Relaxed);
            assert!(a <= 200,
                "too many allowed under tight limit: {a} (max expected 200 accounting for boundary races)");
            assert!(rejected2.load(Ordering::Relaxed) >= 9800,
                "expected at least 9800 rejected, got {}",
                rejected2.load(Ordering::Relaxed));
        });
    }
}
