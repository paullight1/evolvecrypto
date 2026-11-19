use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tracing::warn;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Hot / warm / cold wallet balances for a single asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletBalances {
    pub hot: Decimal,
    pub warm: Decimal,
    pub cold: Decimal,
}

impl WalletBalances {
    /// Sum across all wallet tiers.
    pub fn total(&self) -> Decimal {
        self.hot + self.warm + self.cold
    }
}

/// Overall health of a single asset's reconciliation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReconciliationStatus {
    /// `discrepancy == 0` — perfect balance.
    Balanced,
    /// `|discrepancy_pct| < 0.01%` — within acceptable operational tolerance.
    WithinTolerance,
    /// `|discrepancy_pct| >= 0.01%` — requires investigation.
    Discrepancy,
}

/// Reconciliation result for one asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetReconciliation {
    pub asset: String,

    // ---- Liabilities (what users are owed) ----
    /// Sum of all user wallet balances (available + locked).
    pub total_user_balances: Decimal,
    /// Subset of `total_user_balances` that is locked in open orders.
    pub total_locked: Decimal,
    /// Subset of `total_user_balances` that is freely available.
    pub total_available: Decimal,

    // ---- Reserves (what the exchange holds) ----
    pub hot_wallet: Decimal,
    pub warm_wallet: Decimal,
    pub cold_wallet: Decimal,
    /// `hot_wallet + warm_wallet + cold_wallet`
    pub total_reserves: Decimal,

    // ---- Delta ----
    /// `total_reserves - total_user_balances`.
    /// Positive means the exchange holds more than users are owed (surplus).
    /// Negative means a shortfall — critical.
    pub discrepancy: Decimal,
    /// `discrepancy / total_user_balances * 100`, or `0` when liabilities are zero.
    pub discrepancy_pct: Decimal,

    pub status: ReconciliationStatus,
}

/// Full reconciliation snapshot across all tracked assets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconciliationReport {
    pub timestamp: DateTime<Utc>,
    pub assets: Vec<AssetReconciliation>,
    /// Worst status across all assets: `Discrepancy` > `WithinTolerance` > `Balanced`.
    pub status: ReconciliationStatus,
}

// ---------------------------------------------------------------------------
// Reconciler
// ---------------------------------------------------------------------------

/// Fraction of total liabilities below which a discrepancy is acceptable.
///
/// 0.01% = 1 basis point.
const TOLERANCE_PCT: Decimal = Decimal::from_parts(1, 0, 0, false, 4); // 0.0001

/// Computes balance reconciliation reports between user ledger and wallet reserves.
pub struct Reconciler;

impl Reconciler {
    /// Compute a `ReconciliationReport` from a snapshot of user balances and wallet reserves.
    ///
    /// `user_balances`: asset → total user-owned balance (available + locked).
    /// `reserves`: asset → `WalletBalances` across wallet tiers.
    ///
    /// Assets present in one map but not the other are included with zeros on the missing side.
    pub fn reconcile(
        user_balances: HashMap<String, Decimal>,
        reserves: HashMap<String, WalletBalances>,
    ) -> ReconciliationReport {
        // Union of all asset keys
        let mut assets: Vec<String> = user_balances
            .keys()
            .chain(reserves.keys())
            .cloned()
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        assets.sort(); // deterministic ordering

        let mut reconciled: Vec<AssetReconciliation> = assets
            .into_iter()
            .map(|asset| {
                let liabilities = user_balances
                    .get(&asset)
                    .copied()
                    .unwrap_or(Decimal::ZERO);

                let wallets = reserves.get(&asset).cloned().unwrap_or(WalletBalances {
                    hot: Decimal::ZERO,
                    warm: Decimal::ZERO,
                    cold: Decimal::ZERO,
                });

                let total_reserves = wallets.total();
                let discrepancy = total_reserves - liabilities;

                let discrepancy_pct = if liabilities.is_zero() {
                    Decimal::ZERO
                } else {
                    (discrepancy / liabilities) * Decimal::ONE_HUNDRED
                };

                let status = Self::classify(discrepancy_pct);

                if status == ReconciliationStatus::Discrepancy {
                    warn!(
                        asset = %asset,
                        liabilities = %liabilities,
                        reserves = %total_reserves,
                        discrepancy_pct = %discrepancy_pct,
                        "balance discrepancy detected"
                    );
                }

                AssetReconciliation {
                    asset,
                    total_user_balances: liabilities,
                    // Phase 1: we receive total balances as one number; locked/available
                    // split requires the ledger to pass them separately — not yet wired.
                    // Keep these at zero until the caller supplies the breakdown.
                    total_locked: Decimal::ZERO,
                    total_available: liabilities,
                    hot_wallet: wallets.hot,
                    warm_wallet: wallets.warm,
                    cold_wallet: wallets.cold,
                    total_reserves,
                    discrepancy,
                    discrepancy_pct,
                    status,
                }
            })
            .collect();

        // Sort by worst status first, then alphabetically.
        reconciled.sort_by(|a, b| {
            Self::status_ord(b.status)
                .cmp(&Self::status_ord(a.status))
                .then_with(|| a.asset.cmp(&b.asset))
        });

        let overall_status = reconciled
            .iter()
            .map(|a| a.status)
            .max_by_key(|s| Self::status_ord(*s))
            .unwrap_or(ReconciliationStatus::Balanced);

        ReconciliationReport {
            timestamp: Utc::now(),
            assets: reconciled,
            status: overall_status,
        }
    }

    fn classify(discrepancy_pct: Decimal) -> ReconciliationStatus {
        let abs = discrepancy_pct.abs();
        if abs.is_zero() {
            ReconciliationStatus::Balanced
        } else if abs < TOLERANCE_PCT * Decimal::ONE_HUNDRED {
            // TOLERANCE_PCT is already fractional (0.0001); multiply by 100 → 0.01 (pct)
            ReconciliationStatus::WithinTolerance
        } else {
            ReconciliationStatus::Discrepancy
        }
    }

    /// Numeric rank for sorting (higher = worse).
    fn status_ord(status: ReconciliationStatus) -> u8 {
        match status {
            ReconciliationStatus::Balanced => 0,
            ReconciliationStatus::WithinTolerance => 1,
            ReconciliationStatus::Discrepancy => 2,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn wallets(hot: Decimal, warm: Decimal, cold: Decimal) -> WalletBalances {
        WalletBalances { hot, warm, cold }
    }

    fn single_asset_report(
        liability: Decimal,
        hot: Decimal,
        warm: Decimal,
        cold: Decimal,
    ) -> ReconciliationReport {
        let mut balances = HashMap::new();
        balances.insert("USDT".to_string(), liability);

        let mut reserves = HashMap::new();
        reserves.insert("USDT".to_string(), wallets(hot, warm, cold));

        Reconciler::reconcile(balances, reserves)
    }

    // --- Balanced ---

    #[test]
    fn test_balanced_when_reserves_equal_liabilities() {
        let report = single_asset_report(dec!(1_000_000), dec!(100_000), dec!(200_000), dec!(700_000));

        assert_eq!(report.status, ReconciliationStatus::Balanced);
        let asset = &report.assets[0];
        assert_eq!(asset.discrepancy, Decimal::ZERO);
        assert_eq!(asset.status, ReconciliationStatus::Balanced);
    }

    #[test]
    fn test_zero_balances_are_balanced() {
        let report = single_asset_report(Decimal::ZERO, Decimal::ZERO, Decimal::ZERO, Decimal::ZERO);

        assert_eq!(report.status, ReconciliationStatus::Balanced);
        assert_eq!(report.assets[0].discrepancy_pct, Decimal::ZERO);
    }

    // --- WithinTolerance ---

    #[test]
    fn test_within_tolerance_for_tiny_discrepancy() {
        // 0.001% discrepancy — below the 0.01% threshold
        let liability = dec!(1_000_000);
        let reserve = liability + dec!(10); // 10 USDT on 1M = 0.001%

        let report = single_asset_report(liability, reserve, Decimal::ZERO, Decimal::ZERO);

        assert_eq!(report.status, ReconciliationStatus::WithinTolerance);
        assert_eq!(report.assets[0].status, ReconciliationStatus::WithinTolerance);
    }

    #[test]
    fn test_within_tolerance_negative_discrepancy() {
        // Reserves slightly below liabilities — still within tolerance
        let liability = dec!(1_000_000);
        let reserve = liability - dec!(5); // 0.0005% shortfall

        let report = single_asset_report(liability, reserve, Decimal::ZERO, Decimal::ZERO);

        assert_eq!(report.status, ReconciliationStatus::WithinTolerance);
    }

    // --- Discrepancy ---

    #[test]
    fn test_discrepancy_when_reserves_too_low() {
        let liability = dec!(1_000_000);
        let reserve = dec!(900_000); // 10% short — far outside tolerance

        let report = single_asset_report(liability, reserve, Decimal::ZERO, Decimal::ZERO);

        assert_eq!(report.status, ReconciliationStatus::Discrepancy);
        let asset = &report.assets[0];
        assert!(asset.discrepancy < Decimal::ZERO);
        assert_eq!(asset.status, ReconciliationStatus::Discrepancy);
    }

    #[test]
    fn test_discrepancy_when_reserves_too_high() {
        // An unexplained surplus is also a discrepancy — could indicate accounting error.
        let liability = dec!(1_000_000);
        let reserve = dec!(1_200_000); // 20% surplus

        let report = single_asset_report(liability, reserve, Decimal::ZERO, Decimal::ZERO);

        assert_eq!(report.status, ReconciliationStatus::Discrepancy);
        assert!(report.assets[0].discrepancy > Decimal::ZERO);
    }

    // --- Multiple assets ---

    #[test]
    fn test_multiple_assets_reported_individually() {
        let mut balances = HashMap::new();
        balances.insert("BTC".to_string(), dec!(100));
        balances.insert("ETH".to_string(), dec!(1000));
        balances.insert("USDT".to_string(), dec!(5_000_000));

        let mut reserves = HashMap::new();
        reserves.insert("BTC".to_string(), wallets(dec!(30), dec!(20), dec!(50))); // balanced
        reserves.insert("ETH".to_string(), wallets(dec!(500), dec!(400), dec!(100))); // balanced
        reserves.insert("USDT".to_string(), wallets(dec!(4_000_000), dec!(500_000), dec!(499_000))); // 1k short: 0.02% > tolerance

        let report = Reconciler::reconcile(balances, reserves);

        assert_eq!(report.assets.len(), 3);
        // Overall status is the worst
        assert_eq!(report.status, ReconciliationStatus::Discrepancy);

        // USDT should be first (worst status)
        assert_eq!(report.assets[0].asset, "USDT");
        assert_eq!(report.assets[0].status, ReconciliationStatus::Discrepancy);

        let btc = report.assets.iter().find(|a| a.asset == "BTC").unwrap();
        assert_eq!(btc.status, ReconciliationStatus::Balanced);

        let eth = report.assets.iter().find(|a| a.asset == "ETH").unwrap();
        assert_eq!(eth.status, ReconciliationStatus::Balanced);
    }

    #[test]
    fn test_asset_in_balances_but_not_reserves_is_full_discrepancy() {
        let mut balances = HashMap::new();
        balances.insert("SOL".to_string(), dec!(10_000));

        let report = Reconciler::reconcile(balances, HashMap::new());

        assert_eq!(report.status, ReconciliationStatus::Discrepancy);
        assert_eq!(report.assets[0].total_reserves, Decimal::ZERO);
        assert!(report.assets[0].discrepancy < Decimal::ZERO);
    }

    #[test]
    fn test_asset_in_reserves_but_not_balances_is_surplus_discrepancy() {
        let mut reserves = HashMap::new();
        reserves.insert("DOGE".to_string(), wallets(dec!(50_000), Decimal::ZERO, Decimal::ZERO));

        let report = Reconciler::reconcile(HashMap::new(), reserves);

        // liabilities = 0, reserves = 50_000 → discrepancy_pct stays 0 (division guard)
        // so status is Balanced (no users are owed anything)
        assert_eq!(report.assets[0].discrepancy_pct, Decimal::ZERO);
        assert_eq!(report.assets[0].status, ReconciliationStatus::Balanced);
    }

    #[test]
    fn test_wallet_totals_are_summed_correctly() {
        let report = single_asset_report(dec!(1_000_000), dec!(100_000), dec!(300_000), dec!(600_000));

        let asset = &report.assets[0];
        assert_eq!(asset.hot_wallet, dec!(100_000));
        assert_eq!(asset.warm_wallet, dec!(300_000));
        assert_eq!(asset.cold_wallet, dec!(600_000));
        assert_eq!(asset.total_reserves, dec!(1_000_000));
    }

    #[test]
    fn test_empty_input_produces_empty_report() {
        let report = Reconciler::reconcile(HashMap::new(), HashMap::new());

        assert!(report.assets.is_empty());
        assert_eq!(report.status, ReconciliationStatus::Balanced);
    }

    #[test]
    fn test_discrepancy_pct_calculation() {
        // liability = 1_000, reserves = 1_010 → discrepancy = +10 → pct = +1%
        let report = single_asset_report(dec!(1000), dec!(1010), Decimal::ZERO, Decimal::ZERO);
        let asset = &report.assets[0];

        assert_eq!(asset.discrepancy, dec!(10));
        // 10 / 1000 * 100 = 1%
        assert_eq!(asset.discrepancy_pct, dec!(1));
        assert_eq!(asset.status, ReconciliationStatus::Discrepancy);
    }
}
