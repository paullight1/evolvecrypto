//! Compromised API key detection via unusual symbol activity.
//!
//! Tracks the set of symbols each API key normally trades. When a key suddenly
//! starts trading a symbol it has never touched before, this may indicate the
//! key has been stolen and is being used to pump an illiquid pair.
//!
//! # Reference
//! - Binance SYS pump (2018): stolen API keys used to market-buy illiquid SYS.
//! - C4 in threat catalogue.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use uuid::Uuid;

use exchange_common::types::{Symbol, UserId};

use crate::alert::{AlertSeverity, AlertType, AutoAction, SurveillanceAlert};

/// Tracks per-key symbol usage and detects anomalies.
pub struct ApiKeyAnomalyDetector {
    /// Per-user (or per-API-key) set of known trading symbols.
    known_symbols: HashMap<UserId, HashSet<Symbol>>,
    /// Minimum number of historical symbols before anomaly detection kicks in.
    /// New keys (< min_history) get a learning period.
    min_history: usize,
}

impl ApiKeyAnomalyDetector {
    pub fn new(min_history: usize) -> Self {
        Self {
            known_symbols: HashMap::new(),
            min_history,
        }
    }

    /// Default: require at least 5 distinct symbols in history.
    pub fn with_defaults() -> Self {
        Self::new(5)
    }

    /// Record a trade on a symbol for this user/key.
    /// Returns an alert if the symbol is new and the user has enough history.
    pub fn on_trade(
        &mut self,
        user_id: UserId,
        symbol: &Symbol,
        now: DateTime<Utc>,
    ) -> Option<SurveillanceAlert> {
        let symbols = self.known_symbols.entry(user_id).or_default();

        if symbols.contains(symbol) {
            return None; // Known symbol, no anomaly.
        }

        // New symbol — check if we have enough history to flag.
        let has_history = symbols.len() >= self.min_history;

        // Always add the symbol to history.
        symbols.insert(symbol.clone());

        if has_history {
            return Some(SurveillanceAlert {
                id: Uuid::new_v4(),
                alert_type: AlertType::Spoofing(crate::alert::SpoofingAlert {
                    user_id,
                    cancel_ratio: rust_decimal::Decimal::ZERO,
                    orders_placed: 0,
                    orders_cancelled: 0,
                    window_start: now,
                    window_end: now,
                }),
                severity: AlertSeverity::Medium,
                user_ids: vec![user_id],
                symbol: Some(symbol.clone()),
                description: format!(
                    "API key anomaly: first-time activity on {} (known symbols: {})",
                    symbol,
                    symbols.len() - 1, // exclude the new one
                ),
                auto_action: AutoAction::ReduceRateLimit,
                created_at: now,
                reviewed: false,
            });
        }

        None
    }

    /// Number of tracked keys.
    pub fn tracked_keys(&self) -> usize {
        self.known_symbols.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use exchange_common::Symbol;

    fn user() -> UserId { Uuid::new_v4() }

    #[test]
    fn no_alert_during_learning_period() {
        let mut det = ApiKeyAnomalyDetector::new(3);
        let u = user();
        let now = Utc::now();

        // First 3 symbols — learning period, no alerts.
        assert!(det.on_trade(u, &Symbol::new("BTCUSDT"), now).is_none());
        assert!(det.on_trade(u, &Symbol::new("ETHUSDT"), now).is_none());
        assert!(det.on_trade(u, &Symbol::new("SOLUSDT"), now).is_none());
    }

    #[test]
    fn alert_on_new_symbol_after_learning() {
        let mut det = ApiKeyAnomalyDetector::new(3);
        let u = user();
        let now = Utc::now();

        // Build history.
        det.on_trade(u, &Symbol::new("BTCUSDT"), now);
        det.on_trade(u, &Symbol::new("ETHUSDT"), now);
        det.on_trade(u, &Symbol::new("SOLUSDT"), now);

        // New symbol after learning period → alert.
        let alert = det.on_trade(u, &Symbol::new("SYSUSDT"), now);
        assert!(alert.is_some(), "new symbol after learning should alert");
    }

    #[test]
    fn known_symbol_no_alert() {
        let mut det = ApiKeyAnomalyDetector::new(3);
        let u = user();
        let now = Utc::now();

        det.on_trade(u, &Symbol::new("BTCUSDT"), now);
        det.on_trade(u, &Symbol::new("ETHUSDT"), now);
        det.on_trade(u, &Symbol::new("SOLUSDT"), now);

        // Trading a known symbol — no alert.
        assert!(det.on_trade(u, &Symbol::new("BTCUSDT"), now).is_none());
    }

    #[test]
    fn different_users_independent() {
        let mut det = ApiKeyAnomalyDetector::new(2);
        let u1 = user();
        let u2 = user();
        let now = Utc::now();

        det.on_trade(u1, &Symbol::new("BTCUSDT"), now);
        det.on_trade(u1, &Symbol::new("ETHUSDT"), now);

        // u1 gets alert on new symbol, u2 doesn't (no history).
        assert!(det.on_trade(u1, &Symbol::new("SYSUSDT"), now).is_some());
        assert!(det.on_trade(u2, &Symbol::new("SYSUSDT"), now).is_none());
    }
}
