//! Liquidation cascade circuit breaker — halts forced liquidations when
//! too many fire in a short window to prevent cascade amplification.
//!
//! # Purpose
//!
//! Forced liquidations generate large market orders. When many liquidations
//! fire simultaneously on the same symbol, each pushes the price further,
//! triggering more liquidations in a destructive cascade. This breaker
//! pauses forced liquidations while still allowing voluntary position closes.
//!
//! # Reference
//!
//! - BitMEX (2018-2020): repeated cascade liquidation events causing flash crashes.
//! - Default threshold: 10 liquidations within 60 seconds triggers 30s pause.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};

use exchange_common::Symbol;

/// Configuration for the liquidation cascade breaker.
#[derive(Debug, Clone)]
pub struct LiquidationBreakerConfig {
    /// Max liquidations per symbol within the window before halting.
    pub max_liquidations: usize,
    /// Window in seconds to count liquidations.
    pub window_secs: u64,
    /// Duration in seconds to halt forced liquidations after breaker trips.
    pub halt_duration_secs: u64,
}

impl Default for LiquidationBreakerConfig {
    fn default() -> Self {
        Self {
            max_liquidations: 10,
            window_secs: 60,
            halt_duration_secs: 30,
        }
    }
}

/// State for a single symbol's breaker.
#[derive(Debug)]
struct SymbolBreakerState {
    /// Timestamps of recent liquidations.
    timestamps: Vec<DateTime<Utc>>,
    /// If Some, liquidations are halted until this time.
    halted_until: Option<DateTime<Utc>>,
}

impl SymbolBreakerState {
    fn new() -> Self {
        Self {
            timestamps: Vec::new(),
            halted_until: None,
        }
    }
}

/// Result of checking whether a liquidation is allowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiquidationCheckResult {
    /// Liquidation may proceed.
    Allowed,
    /// Liquidations are paused — cascade breaker active.
    Halted { resume_at: DateTime<Utc> },
}

/// Monitors liquidation frequency per symbol and halts forced liquidations
/// when the rate exceeds the configured threshold.
pub struct LiquidationCascadeBreaker {
    config: LiquidationBreakerConfig,
    symbols: HashMap<Symbol, SymbolBreakerState>,
}

impl LiquidationCascadeBreaker {
    pub fn new(config: LiquidationBreakerConfig) -> Self {
        Self {
            config,
            symbols: HashMap::new(),
        }
    }

    /// Record a liquidation event and check if the breaker should trip.
    ///
    /// Returns `Allowed` if the liquidation can proceed, `Halted` if
    /// the cascade breaker has been triggered.
    pub fn on_liquidation(&mut self, symbol: &Symbol, now: DateTime<Utc>) -> LiquidationCheckResult {
        let state = self.symbols
            .entry(symbol.clone())
            .or_insert_with(SymbolBreakerState::new);

        // Check if currently halted.
        if let Some(resume_at) = state.halted_until {
            if now < resume_at {
                return LiquidationCheckResult::Halted { resume_at };
            }
            // Halt expired — reset.
            state.halted_until = None;
            state.timestamps.clear();
        }

        // Record this liquidation.
        state.timestamps.push(now);

        // Prune outside window.
        let cutoff = now - Duration::seconds(self.config.window_secs as i64);
        state.timestamps.retain(|t| *t >= cutoff);

        // Check threshold.
        if state.timestamps.len() > self.config.max_liquidations {
            let resume_at = now + Duration::seconds(self.config.halt_duration_secs as i64);
            state.halted_until = Some(resume_at);

            tracing::error!(
                symbol = %symbol,
                count = state.timestamps.len(),
                threshold = self.config.max_liquidations,
                halt_secs = self.config.halt_duration_secs,
                "LIQUIDATION CASCADE BREAKER triggered — forced liquidations halted"
            );

            return LiquidationCheckResult::Halted { resume_at };
        }

        LiquidationCheckResult::Allowed
    }

    /// Check if forced liquidations are currently halted for a symbol.
    pub fn is_halted(&self, symbol: &Symbol, now: DateTime<Utc>) -> bool {
        self.symbols
            .get(symbol)
            .and_then(|s| s.halted_until)
            .map(|resume| now < resume)
            .unwrap_or(false)
    }

    /// Admin reset — lift the halt for a symbol.
    pub fn reset(&mut self, symbol: &Symbol) {
        if let Some(state) = self.symbols.get_mut(symbol) {
            state.halted_until = None;
            state.timestamps.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use exchange_common::Symbol;

    fn btc() -> Symbol { Symbol::new("BTCUSDT") }
    fn eth() -> Symbol { Symbol::new("ETHUSDT") }

    #[test]
    fn test_below_threshold_allowed() {
        let mut breaker = LiquidationCascadeBreaker::new(LiquidationBreakerConfig::default());
        let now = Utc::now();

        for i in 0..10 {
            let result = breaker.on_liquidation(&btc(), now + Duration::seconds(i));
            assert_eq!(result, LiquidationCheckResult::Allowed);
        }
    }

    #[test]
    fn test_exceeds_threshold_halts() {
        let mut breaker = LiquidationCascadeBreaker::new(LiquidationBreakerConfig::default());
        let now = Utc::now();

        // 11th liquidation within 60s → halted.
        for i in 0..11 {
            breaker.on_liquidation(&btc(), now + Duration::seconds(i));
        }

        assert!(breaker.is_halted(&btc(), now + Duration::seconds(15)));
    }

    #[test]
    fn test_halt_expires_after_duration() {
        let config = LiquidationBreakerConfig {
            max_liquidations: 5,
            window_secs: 60,
            halt_duration_secs: 10,
        };
        let mut breaker = LiquidationCascadeBreaker::new(config);
        let now = Utc::now();

        // Trip the breaker.
        for i in 0..6 {
            breaker.on_liquidation(&btc(), now + Duration::seconds(i));
        }
        assert!(breaker.is_halted(&btc(), now + Duration::seconds(8)));

        // After 10s halt expires.
        assert!(!breaker.is_halted(&btc(), now + Duration::seconds(20)));
    }

    #[test]
    fn test_different_symbols_independent() {
        let mut breaker = LiquidationCascadeBreaker::new(LiquidationBreakerConfig {
            max_liquidations: 3, window_secs: 60, halt_duration_secs: 30,
        });
        let now = Utc::now();

        // Trip BTC.
        for i in 0..4 {
            breaker.on_liquidation(&btc(), now + Duration::seconds(i));
        }
        assert!(breaker.is_halted(&btc(), now + Duration::seconds(5)));
        assert!(!breaker.is_halted(&eth(), now + Duration::seconds(5)));
    }

    #[test]
    fn test_admin_reset() {
        let mut breaker = LiquidationCascadeBreaker::new(LiquidationBreakerConfig {
            max_liquidations: 3, window_secs: 60, halt_duration_secs: 300,
        });
        let now = Utc::now();

        for i in 0..4 {
            breaker.on_liquidation(&btc(), now + Duration::seconds(i));
        }
        assert!(breaker.is_halted(&btc(), now + Duration::seconds(5)));

        breaker.reset(&btc());
        assert!(!breaker.is_halted(&btc(), now + Duration::seconds(5)));
    }

    #[test]
    fn test_old_liquidations_pruned() {
        let config = LiquidationBreakerConfig {
            max_liquidations: 5, window_secs: 10, halt_duration_secs: 30,
        };
        let mut breaker = LiquidationCascadeBreaker::new(config);
        let now = Utc::now();

        // 4 liquidations at t=0.
        for i in 0..4 {
            breaker.on_liquidation(&btc(), now + Duration::seconds(i));
        }

        // 2 more at t=15 — old ones pruned, total in window = 2.
        let later = now + Duration::seconds(15);
        let result = breaker.on_liquidation(&btc(), later);
        assert_eq!(result, LiquidationCheckResult::Allowed);
    }
}
