//! Stop cluster exposure policy.
//!
//! Detects when stop/trigger orders cluster at a specific price level for a
//! symbol, creating a cascade risk if the price reaches that level. When too
//! many stops are concentrated at one price band, triggering them simultaneously
//! would cause a burst of market orders that could cascade the price further.
//!
//! # Policy
//!
//! - Track trigger prices of all active stop/TP/SL orders per symbol.
//! - Group them into price bands (configurable width, default 0.5%).
//! - If total notional in any band exceeds `max_cluster_notional`, reject
//!   new stop orders that would add to that band.
//!
//! # Reference
//! - BitMEX cascading liquidations (2018-2020): stop clusters amplified crashes.
//! - Bybit "Stop Loss Cluster Protection" whitepaper (2021).
//! - B4 in threat catalogue: Stop/Liquidation Hunting.

use std::collections::HashMap;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use exchange_common::types::Symbol;

/// Configuration for the stop cluster policy.
#[derive(Debug, Clone)]
pub struct StopClusterConfig {
    /// Width of each price band as a fraction (e.g. 0.005 = 0.5%).
    pub band_width_pct: Decimal,
    /// Maximum total notional (price × qty) allowed in any single band.
    pub max_cluster_notional: Decimal,
}

impl Default for StopClusterConfig {
    fn default() -> Self {
        Self {
            band_width_pct: dec!(0.005),       // 0.5% bands
            max_cluster_notional: dec!(500000), // $500K per band
        }
    }
}

/// A tracked stop order entry.
#[derive(Debug, Clone)]
struct StopEntry {
    trigger_price: Decimal,
    notional: Decimal, // trigger_price * qty
}

/// Monitors stop order concentration per symbol and rejects new stops
/// that would exceed the cluster threshold.
#[derive(Debug)]
pub struct StopClusterPolicy {
    config: StopClusterConfig,
    /// Per-symbol list of active stop entries.
    stops: HashMap<Symbol, Vec<StopEntry>>,
}

/// Result of checking whether a new stop can be added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopClusterResult {
    /// Stop can be placed.
    Allowed,
    /// Stop rejected — would exceed cluster threshold.
    Rejected {
        /// The price band center.
        band_center: Decimal,
        /// Current notional in the band.
        current_notional: Decimal,
        /// Maximum allowed.
        max_notional: Decimal,
    },
}

impl StopClusterPolicy {
    pub fn new(config: StopClusterConfig) -> Self {
        Self {
            config,
            stops: HashMap::new(),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(StopClusterConfig::default())
    }

    /// Compute the band index for a given trigger price.
    /// Band boundaries are at `reference_price * (1 ± k * band_width)`.
    fn band_key(&self, trigger_price: Decimal, reference_price: Decimal) -> i64 {
        if reference_price.is_zero() || self.config.band_width_pct.is_zero() {
            return 0;
        }
        let ratio = trigger_price / reference_price;
        let band_width = self.config.band_width_pct;
        // Convert ratio to band index: band 0 is [1-bw/2, 1+bw/2)
        let offset = (ratio - Decimal::ONE) / band_width;
        // Use floor to get a stable integer band index.
        offset.floor().to_string().parse::<i64>().unwrap_or(0)
    }

    /// Check if adding a stop order at `trigger_price` with `qty` would exceed
    /// the cluster threshold for the given `symbol`.
    ///
    /// `reference_price` is the current mark/last price used to compute bands.
    pub fn check_new_stop(
        &self,
        symbol: &Symbol,
        trigger_price: Decimal,
        qty: Decimal,
        reference_price: Decimal,
    ) -> StopClusterResult {
        let new_notional = trigger_price * qty;
        let band = self.band_key(trigger_price, reference_price);

        let current_notional = self.band_notional(symbol, band, reference_price);

        if current_notional + new_notional > self.config.max_cluster_notional {
            let band_center = reference_price
                * (Decimal::ONE + self.config.band_width_pct * Decimal::from(band));
            return StopClusterResult::Rejected {
                band_center,
                current_notional,
                max_notional: self.config.max_cluster_notional,
            };
        }

        StopClusterResult::Allowed
    }

    /// Compute total notional in a specific band for a symbol.
    fn band_notional(&self, symbol: &Symbol, band: i64, reference_price: Decimal) -> Decimal {
        let Some(entries) = self.stops.get(symbol) else {
            return Decimal::ZERO;
        };
        entries
            .iter()
            .filter(|e| self.band_key(e.trigger_price, reference_price) == band)
            .map(|e| e.notional)
            .sum()
    }

    /// Register a new stop order (after it passes the check).
    pub fn register_stop(
        &mut self,
        symbol: &Symbol,
        trigger_price: Decimal,
        qty: Decimal,
    ) {
        let notional = trigger_price * qty;
        self.stops
            .entry(symbol.clone())
            .or_default()
            .push(StopEntry {
                trigger_price,
                notional,
            });
    }

    /// Remove a stop order (cancelled or triggered).
    /// Removes the first matching entry by trigger_price and notional.
    pub fn remove_stop(
        &mut self,
        symbol: &Symbol,
        trigger_price: Decimal,
        qty: Decimal,
    ) {
        let notional = trigger_price * qty;
        if let Some(entries) = self.stops.get_mut(symbol) {
            if let Some(idx) = entries.iter().position(|e| {
                e.trigger_price == trigger_price && e.notional == notional
            }) {
                entries.swap_remove(idx);
            }
        }
    }

    /// Number of tracked stops for a symbol.
    pub fn stop_count(&self, symbol: &Symbol) -> usize {
        self.stops.get(symbol).map_or(0, |v| v.len())
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn sym() -> Symbol {
        Symbol::new("BTCUSDT")
    }

    #[test]
    fn allows_stops_below_threshold() {
        let policy = StopClusterPolicy::new(StopClusterConfig {
            band_width_pct: dec!(0.01), // 1% bands
            max_cluster_notional: dec!(100000),
        });

        let result = policy.check_new_stop(
            &sym(),
            dec!(49500), // trigger price
            dec!(1.0),   // 1 BTC = $49,500 notional
            dec!(50000), // reference
        );
        assert_eq!(result, StopClusterResult::Allowed);
    }

    #[test]
    fn rejects_stop_exceeding_cluster_threshold() {
        let mut policy = StopClusterPolicy::new(StopClusterConfig {
            band_width_pct: dec!(0.01),
            max_cluster_notional: dec!(100000),
        });
        let s = sym();
        let ref_price = dec!(50000);

        // Fill a band with $90,000 of stops.
        policy.register_stop(&s, dec!(49600), dec!(1.0)); // $49,600
        policy.register_stop(&s, dec!(49700), dec!(0.8)); // $39,760
        // Total ~$89,360

        // This should still fit.
        let result = policy.check_new_stop(&s, dec!(49650), dec!(0.2), ref_price);
        assert_eq!(result, StopClusterResult::Allowed);

        // Register it.
        policy.register_stop(&s, dec!(49650), dec!(0.2));

        // Now try to add a large one that pushes over $100K.
        let result = policy.check_new_stop(&s, dec!(49650), dec!(1.0), ref_price);
        assert!(matches!(result, StopClusterResult::Rejected { .. }));
    }

    #[test]
    fn different_bands_are_independent() {
        let mut policy = StopClusterPolicy::new(StopClusterConfig {
            band_width_pct: dec!(0.01),
            max_cluster_notional: dec!(50000),
        });
        let s = sym();
        let ref_price = dec!(50000);

        // Fill one band near $49,500.
        policy.register_stop(&s, dec!(49500), dec!(1.0)); // $49,500

        // A stop in a different band ($48,000) should be fine.
        let result = policy.check_new_stop(&s, dec!(48000), dec!(1.0), ref_price);
        assert_eq!(result, StopClusterResult::Allowed);
    }

    #[test]
    fn remove_stop_frees_capacity() {
        let mut policy = StopClusterPolicy::new(StopClusterConfig {
            band_width_pct: dec!(0.01),
            max_cluster_notional: dec!(50000),
        });
        let s = sym();
        let ref_price = dec!(50000);

        // Fill the band.
        policy.register_stop(&s, dec!(49500), dec!(1.0));

        // Should be rejected (over $50K with another BTC).
        let result = policy.check_new_stop(&s, dec!(49500), dec!(1.0), ref_price);
        assert!(matches!(result, StopClusterResult::Rejected { .. }));

        // Remove the first stop.
        policy.remove_stop(&s, dec!(49500), dec!(1.0));

        // Now it should fit.
        let result = policy.check_new_stop(&s, dec!(49500), dec!(1.0), ref_price);
        assert_eq!(result, StopClusterResult::Allowed);
    }

    #[test]
    fn stop_count_tracks_entries() {
        let mut policy = StopClusterPolicy::with_defaults();
        let s = sym();

        assert_eq!(policy.stop_count(&s), 0);
        policy.register_stop(&s, dec!(49000), dec!(0.5));
        policy.register_stop(&s, dec!(48000), dec!(0.3));
        assert_eq!(policy.stop_count(&s), 2);

        policy.remove_stop(&s, dec!(49000), dec!(0.5));
        assert_eq!(policy.stop_count(&s), 1);
    }

    #[test]
    fn empty_reference_price_does_not_panic() {
        let policy = StopClusterPolicy::with_defaults();
        let result = policy.check_new_stop(&sym(), dec!(49500), dec!(1.0), Decimal::ZERO);
        // With zero reference, should still work (degenerate band).
        assert_eq!(result, StopClusterResult::Allowed);
    }
}
