use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// The security tier of a wallet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WalletType {
    /// Online, immediately accessible for withdrawals. Max ~5% of AUM.
    Hot,
    /// Semi-offline (hardware wallet in secure facility). Max ~15% of AUM.
    Warm,
    /// Air-gapped cold storage. Holds the majority of reserves.
    Cold,
}

/// A registered wallet with its current balance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletInfo {
    pub wallet_type: WalletType,
    pub coin: String,
    pub address: String,
    pub balance: Decimal,
    pub last_updated: DateTime<Utc>,
}

/// Per-coin policy governing the maximum fraction each tier may hold.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletPolicy {
    pub coin: String,
    /// Maximum fraction of total coin AUM allowed in the hot wallet (e.g. 0.05 = 5%).
    pub hot_max_pct: Decimal,
    /// Maximum fraction allowed in the warm wallet (e.g. 0.15 = 15%).
    pub warm_max_pct: Decimal,
    /// Absolute amount that triggers an automatic sweep recommendation from hot→cold.
    /// Zero means disabled (threshold-based sweep not in use for this coin).
    pub sweep_threshold: Decimal,
}

/// Manages registered wallets and enforces hot/warm/cold split policies.
pub struct WalletManager {
    wallets: Vec<WalletInfo>,
    policies: HashMap<String, WalletPolicy>,
}

impl WalletManager {
    /// Create a new `WalletManager` pre-loaded with default policies for common coins.
    pub fn new() -> Self {
        let mut mgr = Self {
            wallets: Vec::new(),
            policies: HashMap::new(),
        };

        for coin in &["BTC", "ETH", "USDT", "USDC", "SOL"] {
            mgr.policies.insert(
                coin.to_string(),
                WalletPolicy {
                    coin: coin.to_string(),
                    hot_max_pct: Decimal::new(5, 2),   // 5%
                    warm_max_pct: Decimal::new(15, 2),  // 15%
                    sweep_threshold: Decimal::ZERO,      // configured per deployment
                },
            );
        }

        mgr
    }

    /// Register a wallet. Subsequent balance updates should be applied by replacing
    /// the entry (remove + re-register) or by mutating in the caller.
    pub fn register_wallet(&mut self, info: WalletInfo) {
        self.wallets.push(info);
    }

    /// Group all wallets by their type. The returned map borrows from `self`.
    pub fn balances_by_type(&self) -> HashMap<WalletType, Vec<&WalletInfo>> {
        let mut map: HashMap<WalletType, Vec<&WalletInfo>> = HashMap::new();
        for w in &self.wallets {
            map.entry(w.wallet_type).or_default().push(w);
        }
        map
    }

    /// Aggregate the total balance held across all wallet tiers for each coin.
    pub fn total_per_coin(&self) -> HashMap<String, Decimal> {
        let mut totals: HashMap<String, Decimal> = HashMap::new();
        for w in &self.wallets {
            *totals.entry(w.coin.clone()).or_default() += w.balance;
        }
        totals
    }

    /// Sum of balances in hot wallets for the given coin.
    pub fn hot_balance(&self, coin: &str) -> Decimal {
        self.wallets
            .iter()
            .filter(|w| w.coin == coin && w.wallet_type == WalletType::Hot)
            .map(|w| w.balance)
            .sum()
    }

    /// Return the amount that should be swept from hot→cold to bring the hot wallet
    /// back within policy limits, or `None` if no sweep is needed.
    pub fn needs_sweep(&self, coin: &str) -> Option<Decimal> {
        let policy = self.policies.get(coin)?;

        let total: Decimal = self.wallets
            .iter()
            .filter(|w| w.coin == coin)
            .map(|w| w.balance)
            .sum();

        if total.is_zero() {
            return None;
        }

        let hot = self.hot_balance(coin);
        let hot_pct = hot / total;

        if hot_pct > policy.hot_max_pct {
            // Sweep enough so that hot balance equals exactly hot_max_pct of total.
            Some(hot - total * policy.hot_max_pct)
        } else {
            None
        }
    }

    /// All registered wallets.
    pub fn all_wallets(&self) -> &[WalletInfo] {
        &self.wallets
    }

    /// All configured policies, keyed by coin symbol.
    pub fn policies(&self) -> &HashMap<String, WalletPolicy> {
        &self.policies
    }
}

impl Default for WalletManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn btc_hot(balance: Decimal) -> WalletInfo {
        WalletInfo {
            wallet_type: WalletType::Hot,
            coin: "BTC".into(),
            address: "bc1q_hot".into(),
            balance,
            last_updated: Utc::now(),
        }
    }

    fn btc_cold(balance: Decimal) -> WalletInfo {
        WalletInfo {
            wallet_type: WalletType::Cold,
            coin: "BTC".into(),
            address: "bc1q_cold".into(),
            balance,
            last_updated: Utc::now(),
        }
    }

    #[test]
    fn test_needs_sweep_when_hot_exceeds_policy() {
        let mut mgr = WalletManager::new();
        // 20 BTC hot, 80 BTC cold → hot = 20% > 5% policy.
        mgr.register_wallet(btc_hot(dec!(20)));
        mgr.register_wallet(btc_cold(dec!(80)));

        let sweep = mgr.needs_sweep("BTC").expect("sweep required");
        // Target: 5% of 100 = 5 BTC. Currently 20. Sweep 15.
        assert_eq!(sweep, dec!(15));
    }

    #[test]
    fn test_no_sweep_when_within_policy() {
        let mut mgr = WalletManager::new();
        // 5 BTC hot, 95 BTC cold → hot = 5% = policy limit, no sweep.
        mgr.register_wallet(btc_hot(dec!(5)));
        mgr.register_wallet(btc_cold(dec!(95)));

        assert!(mgr.needs_sweep("BTC").is_none());
    }

    #[test]
    fn test_needs_sweep_zero_balance_returns_none() {
        let mgr = WalletManager::new();
        assert!(mgr.needs_sweep("BTC").is_none());
    }

    #[test]
    fn test_total_per_coin() {
        let mut mgr = WalletManager::new();
        mgr.register_wallet(btc_hot(dec!(10)));
        mgr.register_wallet(btc_cold(dec!(90)));

        let totals = mgr.total_per_coin();
        assert_eq!(totals["BTC"], dec!(100));
    }

    #[test]
    fn test_balances_by_type_groups_correctly() {
        let mut mgr = WalletManager::new();
        mgr.register_wallet(btc_hot(dec!(5)));
        mgr.register_wallet(btc_cold(dec!(95)));

        let by_type = mgr.balances_by_type();
        assert_eq!(by_type[&WalletType::Hot].len(), 1);
        assert_eq!(by_type[&WalletType::Cold].len(), 1);
    }
}
