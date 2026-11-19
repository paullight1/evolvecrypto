use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Snapshot of the exchange's capital adequacy position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapitalReport {
    /// Total own funds held by the exchange (equity capital, retained earnings).
    pub own_funds_usd: Decimal,
    /// Regulatory minimum own-funds requirement.
    /// MiCA Art. 67: €125k ≈ $135k for Class 2 CASPs.
    pub regulatory_minimum_usd: Decimal,
    /// Ratio: own_funds / regulatory_minimum.  ≥ 1.0 = compliant.
    pub ratio: Decimal,
    /// `true` when own_funds ≥ regulatory_minimum.
    pub compliant: bool,
    /// Percentage buffer above minimum (negative if below).
    pub buffer_pct: Decimal,
    /// Total value in all insurance / default funds at report time.
    pub insurance_fund_total: Decimal,
    pub generated_at: DateTime<Utc>,
}

/// Tracks capital adequacy against regulatory requirements.
///
/// Phase 1: own_funds is set manually by the admin API.
/// Phase 2: auto-calculated from the exchange treasury wallet balance.
pub struct CapitalTracker {
    /// Regulatory minimum own-funds (MiCA Art. 67: ~$135k ≈ €125k).
    pub regulatory_minimum: Decimal,
    /// Current own funds held by the exchange.
    pub own_funds: Decimal,
}

impl CapitalTracker {
    /// Create a tracker with MiCA default minimum (~€125k ≈ $135k).
    pub fn new() -> Self {
        Self {
            regulatory_minimum: Decimal::from(135_000),
            own_funds: Decimal::ZERO,
        }
    }

    /// Update the reported own-funds figure.
    pub fn set_own_funds(&mut self, amount: Decimal) {
        self.own_funds = amount;
    }

    /// Produce a capital adequacy report, incorporating the current insurance fund total.
    pub fn report(&self, insurance_fund_total: Decimal) -> CapitalReport {
        let ratio = if self.regulatory_minimum > Decimal::ZERO {
            self.own_funds / self.regulatory_minimum
        } else {
            Decimal::from(999)
        };
        let buffer = ratio - Decimal::ONE;

        CapitalReport {
            own_funds_usd: self.own_funds,
            regulatory_minimum_usd: self.regulatory_minimum,
            ratio,
            compliant: self.own_funds >= self.regulatory_minimum,
            buffer_pct: buffer * Decimal::from(100),
            insurance_fund_total,
            generated_at: Utc::now(),
        }
    }

    /// Return `true` if the exchange currently meets the regulatory minimum.
    pub fn is_compliant(&self) -> bool {
        self.own_funds >= self.regulatory_minimum
    }

    /// Return `true` when own funds fall below 1.5× the minimum — an early
    /// warning that action should be taken before breaching the requirement.
    pub fn early_warning(&self) -> bool {
        self.own_funds < self.regulatory_minimum * Decimal::new(15, 1)
    }
}

impl Default for CapitalTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_compliant_above_minimum() {
        let mut tracker = CapitalTracker::new();
        // 135_000 * 2 = 270_000 — well above 1.5× threshold
        tracker.set_own_funds(dec!(270_000));
        assert!(tracker.is_compliant());
        assert!(!tracker.early_warning());
    }

    #[test]
    fn test_non_compliant_below_minimum() {
        let mut tracker = CapitalTracker::new();
        tracker.set_own_funds(dec!(100_000));
        assert!(!tracker.is_compliant());
        assert!(tracker.early_warning());
    }

    #[test]
    fn test_early_warning_at_1_4x() {
        let mut tracker = CapitalTracker::new();
        // 135_000 * 1.4 = 189_000 — below 1.5x threshold → early warning
        tracker.set_own_funds(dec!(189_000));
        assert!(tracker.is_compliant());   // above minimum
        assert!(tracker.early_warning());  // but below 1.5x
    }

    #[test]
    fn test_no_early_warning_at_1_6x() {
        let mut tracker = CapitalTracker::new();
        tracker.set_own_funds(dec!(216_000)); // 135_000 * 1.6
        assert!(!tracker.early_warning());
    }

    #[test]
    fn test_report_ratio() {
        let mut tracker = CapitalTracker::new();
        tracker.set_own_funds(dec!(270_000)); // exactly 2x
        let report = tracker.report(dec!(50_000));
        assert_eq!(report.ratio, dec!(2));
        assert!(report.compliant);
        assert_eq!(report.buffer_pct, dec!(100)); // 100% above minimum
        assert_eq!(report.insurance_fund_total, dec!(50_000));
    }

    #[test]
    fn test_zero_own_funds_is_non_compliant() {
        let tracker = CapitalTracker::new();
        assert!(!tracker.is_compliant());
    }
}
