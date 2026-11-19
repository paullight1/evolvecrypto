//! Loss-rate kill switch — halts an account's trading when losses accumulate
//! too fast or position notional grows too rapidly.
//!
//! # Purpose
//!
//! Defends against runaway algorithms (Knight Capital, 2012: $440M in 45 min)
//! and compromised API keys executing loss-generating trades at high speed.
//!
//! # Reference
//!
//! - Knight Capital (2012): Old code reactivated by deployment sent millions of
//!   unintended orders in 45 minutes, causing $440M loss. A kill switch
//!   monitoring loss rate would have stopped it in ~2 minutes.
//! - Standard institutional risk control: "loss circuit breaker" per desk/account.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;

pub type UserId = Uuid;

/// Configuration for the kill switch thresholds.
#[derive(Debug, Clone)]
pub struct KillSwitchConfig {
    /// Maximum realized loss in USD within the monitoring window before halt.
    /// Default: $50,000 (configurable per account).
    pub max_loss_usd: Decimal,
    /// Maximum position notional accumulation in USD within the window.
    /// Default: $5,000,000 (prevents runaway order spam).
    pub max_notional_usd: Decimal,
    /// Monitoring window duration. Default: 60 seconds.
    pub window_secs: u64,
}

impl Default for KillSwitchConfig {
    fn default() -> Self {
        Self {
            max_loss_usd: dec!(50000),
            max_notional_usd: dec!(5000000),
            window_secs: 60,
        }
    }
}

/// Event recorded for loss-rate tracking.
#[derive(Debug, Clone)]
struct LossEvent {
    timestamp: DateTime<Utc>,
    realized_pnl: Decimal,
    notional: Decimal,
}

/// Kill switch state for a single user.
#[derive(Debug)]
struct UserKillSwitchState {
    events: Vec<LossEvent>,
    halted: bool,
    halted_at: Option<DateTime<Utc>>,
}

impl UserKillSwitchState {
    fn new() -> Self {
        Self {
            events: Vec::new(),
            halted: false,
            halted_at: None,
        }
    }
}

/// Result of a kill switch check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KillSwitchResult {
    /// Trading allowed.
    Ok,
    /// Account halted due to rapid loss accumulation.
    HaltedLossRate {
        window_loss: Decimal,
        threshold: Decimal,
    },
    /// Account halted due to rapid position notional growth.
    HaltedNotionalRate {
        window_notional: Decimal,
        threshold: Decimal,
    },
}

/// Monitors per-user loss rate and position accumulation to detect
/// runaway trading that should be halted.
pub struct KillSwitchMonitor {
    config: KillSwitchConfig,
    users: HashMap<UserId, UserKillSwitchState>,
}

impl KillSwitchMonitor {
    pub fn new(config: KillSwitchConfig) -> Self {
        Self {
            config,
            users: HashMap::new(),
        }
    }

    /// Record a trade event for a user.
    ///
    /// `realized_pnl` is the PnL from this fill (negative = loss).
    /// `notional` is the notional value of the order that filled.
    ///
    /// Returns `KillSwitchResult::Ok` if within limits, or a halt variant
    /// if the user should be immediately blocked from further trading.
    pub fn on_fill(
        &mut self,
        user_id: UserId,
        realized_pnl: Decimal,
        notional: Decimal,
        now: DateTime<Utc>,
    ) -> KillSwitchResult {
        let state = self.users.entry(user_id).or_insert_with(UserKillSwitchState::new);

        if state.halted {
            return KillSwitchResult::HaltedLossRate {
                window_loss: Decimal::ZERO,
                threshold: self.config.max_loss_usd,
            };
        }

        state.events.push(LossEvent {
            timestamp: now,
            realized_pnl,
            notional,
        });

        // Prune events outside window.
        let cutoff = now - Duration::seconds(self.config.window_secs as i64);
        state.events.retain(|e| e.timestamp >= cutoff);

        // Sum losses (only negative PnL) and notional in window.
        let window_loss: Decimal = state.events.iter()
            .map(|e| e.realized_pnl)
            .filter(|pnl| pnl.is_sign_negative())
            .map(|pnl| pnl.abs())
            .sum();

        let window_notional: Decimal = state.events.iter()
            .map(|e| e.notional)
            .sum();

        if window_loss > self.config.max_loss_usd {
            state.halted = true;
            state.halted_at = Some(now);
            tracing::error!(
                user_id = %user_id,
                window_loss = %window_loss,
                threshold = %self.config.max_loss_usd,
                "KILL SWITCH TRIGGERED: loss rate exceeded — account halted"
            );
            return KillSwitchResult::HaltedLossRate {
                window_loss,
                threshold: self.config.max_loss_usd,
            };
        }

        if window_notional > self.config.max_notional_usd {
            state.halted = true;
            state.halted_at = Some(now);
            tracing::error!(
                user_id = %user_id,
                window_notional = %window_notional,
                threshold = %self.config.max_notional_usd,
                "KILL SWITCH TRIGGERED: notional rate exceeded — account halted"
            );
            return KillSwitchResult::HaltedNotionalRate {
                window_notional,
                threshold: self.config.max_notional_usd,
            };
        }

        KillSwitchResult::Ok
    }

    /// Check if a user is currently halted.
    pub fn is_halted(&self, user_id: &UserId) -> bool {
        self.users.get(user_id).map(|s| s.halted).unwrap_or(false)
    }

    /// Admin: manually reset a halted account (after investigation).
    pub fn reset(&mut self, user_id: &UserId) -> bool {
        if let Some(state) = self.users.get_mut(user_id) {
            state.halted = false;
            state.halted_at = None;
            state.events.clear();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_monitor() -> KillSwitchMonitor {
        KillSwitchMonitor::new(KillSwitchConfig::default())
    }

    #[test]
    fn test_normal_trading_not_halted() {
        let mut monitor = default_monitor();
        let user = Uuid::new_v4();
        let now = Utc::now();

        // Small profitable trades.
        let result = monitor.on_fill(user, dec!(100), dec!(10000), now);
        assert_eq!(result, KillSwitchResult::Ok);
        assert!(!monitor.is_halted(&user));
    }

    #[test]
    fn test_loss_rate_triggers_halt() {
        let mut monitor = default_monitor(); // max_loss = $50K in 60s
        let user = Uuid::new_v4();
        let now = Utc::now();

        // Accumulate $51K in losses within 60s.
        for i in 0..51 {
            let t = now + Duration::seconds(i);
            let result = monitor.on_fill(user, dec!(-1000), dec!(50000), t);
            if i >= 50 {
                assert!(matches!(result, KillSwitchResult::HaltedLossRate { .. }));
            }
        }
        assert!(monitor.is_halted(&user));
    }

    #[test]
    fn test_notional_rate_triggers_halt() {
        let config = KillSwitchConfig {
            max_loss_usd: dec!(1000000), // High enough not to trigger
            max_notional_usd: dec!(100000), // Low threshold for test
            window_secs: 60,
        };
        let mut monitor = KillSwitchMonitor::new(config);
        let user = Uuid::new_v4();
        let now = Utc::now();

        // $50K per fill × 3 = $150K > $100K threshold.
        monitor.on_fill(user, dec!(0), dec!(50000), now);
        monitor.on_fill(user, dec!(0), dec!(50000), now + Duration::seconds(1));
        let result = monitor.on_fill(user, dec!(0), dec!(50000), now + Duration::seconds(2));
        assert!(matches!(result, KillSwitchResult::HaltedNotionalRate { .. }));
    }

    #[test]
    fn test_losses_outside_window_not_counted() {
        let mut monitor = default_monitor(); // 60s window
        let user = Uuid::new_v4();
        let now = Utc::now();

        // Old losses (>60s ago) — should NOT count.
        let old = now - Duration::seconds(120);
        monitor.on_fill(user, dec!(-40000), dec!(100000), old);

        // New small loss — should be ok since old ones expired.
        let result = monitor.on_fill(user, dec!(-1000), dec!(10000), now);
        assert_eq!(result, KillSwitchResult::Ok);
    }

    #[test]
    fn test_admin_reset() {
        let mut monitor = default_monitor();
        let user = Uuid::new_v4();
        let now = Utc::now();

        // Trigger halt.
        for i in 0..60 {
            monitor.on_fill(user, dec!(-1000), dec!(50000), now + Duration::seconds(i));
        }
        assert!(monitor.is_halted(&user));

        // Admin reset.
        assert!(monitor.reset(&user));
        assert!(!monitor.is_halted(&user));
    }

    #[test]
    fn test_profits_dont_trigger_loss_halt() {
        let mut monitor = default_monitor();
        let user = Uuid::new_v4();
        let now = Utc::now();

        // Big profits with small notional — should never trigger loss halt.
        for i in 0..10 {
            let result = monitor.on_fill(user, dec!(10000), dec!(50000), now + Duration::seconds(i));
            assert_eq!(result, KillSwitchResult::Ok);
        }
        assert!(!monitor.is_halted(&user));
    }

    #[test]
    fn test_halted_stays_halted() {
        let mut monitor = default_monitor();
        let user = Uuid::new_v4();
        let now = Utc::now();

        // Trigger halt.
        for i in 0..60 {
            monitor.on_fill(user, dec!(-1000), dec!(50000), now + Duration::seconds(i));
        }

        // Further fills should also return halted.
        let result = monitor.on_fill(user, dec!(100), dec!(1000), now + Duration::seconds(70));
        assert!(matches!(result, KillSwitchResult::HaltedLossRate { .. }));
    }
}
