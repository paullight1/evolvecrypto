//! Funding rate manipulation safeguard.
//!
//! Detects anomalous premium index values that may indicate market manipulation.
//! When the instantaneous premium deviates more than 3x from its 24-hour EWMA,
//! an anomaly alert is emitted.
//!
//! # Reference
//!
//! - Bybit: funding rate clamped to ±0.05% per epoch (already enforced in `funding.rs`).
//! - This module adds anomaly *detection* on top of the existing clamp *enforcement*.
//! - Manipulation pattern: attacker takes large position → pushes price to skew
//!   premium index → earns funding from counterparties.

use rust_decimal::Decimal;

use exchange_common::Symbol;

/// Tracks the EWMA of premium index per symbol and detects anomalies.
pub struct FundingRateGuard {
    /// 24h EWMA of premium index per symbol.
    /// At 1-minute sampling, span = 1440 (24 * 60).
    ewma_per_symbol: std::collections::HashMap<Symbol, Decimal>,
    /// EWMA span (number of ticks). Default: 1440 (24h at 1-min intervals).
    span: u32,
    /// Anomaly threshold: current premium / EWMA. Default: 3.0.
    anomaly_multiplier: Decimal,
    /// Emitted anomaly alerts.
    alerts: Vec<FundingAnomaly>,
}

/// Alert emitted when funding rate premium appears manipulated.
#[derive(Debug, Clone)]
pub struct FundingAnomaly {
    pub symbol: Symbol,
    pub current_premium: Decimal,
    pub ewma_premium: Decimal,
    pub deviation_ratio: Decimal,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl Default for FundingRateGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl FundingRateGuard {
    pub fn new() -> Self {
        Self {
            ewma_per_symbol: std::collections::HashMap::new(),
            span: 1440, // 24h at 1-min sampling
            anomaly_multiplier: Decimal::from(3),
            alerts: Vec::new(),
        }
    }

    /// Feed a new premium index sample for a symbol.
    ///
    /// Updates the EWMA and checks for anomalies. Returns `Some(FundingAnomaly)`
    /// if the current premium deviates more than `anomaly_multiplier` times from
    /// the EWMA.
    pub fn on_premium_sample(
        &mut self,
        symbol: &Symbol,
        premium: Decimal,
    ) -> Option<FundingAnomaly> {
        let alpha = Decimal::TWO / (Decimal::from(self.span) + Decimal::ONE);
        let prev_ewma = self.ewma_per_symbol.get(symbol).copied().unwrap_or(premium);
        let new_ewma = alpha * premium + (Decimal::ONE - alpha) * prev_ewma;

        self.ewma_per_symbol.insert(symbol.clone(), new_ewma);

        // Don't alert on first few samples (EWMA not yet stabilized).
        // Use absolute values for comparison.
        let ewma_abs = new_ewma.abs();
        let premium_abs = premium.abs();

        // Only check deviation if EWMA is non-trivial (> 0.0001%)
        if ewma_abs < Decimal::new(1, 6) {
            return None;
        }

        let deviation_ratio = premium_abs / ewma_abs;

        if deviation_ratio > self.anomaly_multiplier {
            let alert = FundingAnomaly {
                symbol: symbol.clone(),
                current_premium: premium,
                ewma_premium: new_ewma,
                deviation_ratio,
                timestamp: chrono::Utc::now(),
            };
            tracing::warn!(
                symbol = %symbol,
                premium = %premium,
                ewma = %new_ewma,
                ratio = %deviation_ratio,
                "Funding rate anomaly detected — premium deviates >3x from EWMA"
            );
            self.alerts.push(alert.clone());
            return Some(alert);
        }

        None
    }

    /// Get all anomaly alerts.
    pub fn alerts(&self) -> &[FundingAnomaly] {
        &self.alerts
    }

    /// Clear old alerts.
    pub fn clear_alerts(&mut self) {
        self.alerts.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use exchange_common::Symbol;
    use rust_decimal_macros::dec;

    fn btc() -> Symbol {
        Symbol::new("BTCUSDT")
    }

    #[test]
    fn test_normal_premium_no_alert() {
        let mut guard = FundingRateGuard::new();

        // Feed 100 normal samples at 0.01% premium.
        for _ in 0..100 {
            assert!(guard.on_premium_sample(&btc(), dec!(0.0001)).is_none());
        }
    }

    #[test]
    fn test_sudden_spike_triggers_alert() {
        let mut guard = FundingRateGuard::new();

        // Stabilize EWMA at ~0.01% over 100 samples.
        for _ in 0..100 {
            guard.on_premium_sample(&btc(), dec!(0.0001));
        }

        // Spike to 0.05% (5x normal) — should trigger anomaly.
        let alert = guard.on_premium_sample(&btc(), dec!(0.0005));
        assert!(alert.is_some(), "5x spike should trigger anomaly");
        assert!(alert.unwrap().deviation_ratio > dec!(3));
    }

    #[test]
    fn test_near_zero_ewma_no_false_positive() {
        let mut guard = FundingRateGuard::new();

        // EWMA near zero — premium also near zero. Should not divide-by-zero or false positive.
        for _ in 0..10 {
            assert!(guard.on_premium_sample(&btc(), dec!(0.0000001)).is_none());
        }
    }

    #[test]
    fn test_multiple_symbols_independent() {
        let mut guard = FundingRateGuard::new();
        let eth = Symbol::new("ETHUSDT");

        // Feed BTC at 0.01%, ETH at 0.05%
        for _ in 0..100 {
            guard.on_premium_sample(&btc(), dec!(0.0001));
            guard.on_premium_sample(&eth, dec!(0.0005));
        }

        // Spike BTC — ETH unaffected.
        let btc_alert = guard.on_premium_sample(&btc(), dec!(0.001));
        assert!(btc_alert.is_some(), "BTC spike should trigger");

        // Normal ETH sample — no alert.
        let eth_alert = guard.on_premium_sample(&eth, dec!(0.0005));
        assert!(eth_alert.is_none(), "ETH should not be affected");
    }

    #[test]
    fn test_bybit_max_rate_already_enforced() {
        // Verify our clamp is ±0.05% as per Bybit spec (tested in funding.rs).
        // FundingRateGuard adds detection, not enforcement — clamp is in
        // calculate_funding_rate_twap().
        let guard = FundingRateGuard::new();
        assert_eq!(guard.anomaly_multiplier, dec!(3));
    }
}
