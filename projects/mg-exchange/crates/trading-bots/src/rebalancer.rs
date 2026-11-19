use std::collections::HashMap;

use chrono::{DateTime, Utc};
use exchange_common::UserId;
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::BotStatus;

/// Configuration for the Smart Rebalancer.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RebalancerConfig {
    pub user_id: UserId,
    /// Target allocation: asset name → fraction of total portfolio (must sum to 1.0).
    pub target_allocation: HashMap<String, Decimal>,
    /// Trigger rebalancing when any asset's allocation deviates by more than this percentage.
    pub rebalance_threshold_pct: Decimal,
}

/// A running rebalancer instance.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Rebalancer {
    pub id: Uuid,
    pub config: RebalancerConfig,
    pub status: BotStatus,
    pub last_rebalance: Option<DateTime<Utc>>,
}

/// Direction of a rebalancing trade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RebalanceDirection {
    Buy,
    Sell,
}

/// A single rebalancing action: buy or sell `amount` of `asset`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RebalanceAction {
    pub asset: String,
    pub action: RebalanceDirection,
    /// Amount in quote-asset (USDT) to buy or sell.
    pub amount: Decimal,
}

impl Rebalancer {
    pub fn new(config: RebalancerConfig) -> Self {
        Self {
            id: Uuid::new_v4(),
            config,
            status: BotStatus::Active,
            last_rebalance: None,
        }
    }

    /// Compute the total portfolio value in quote-asset.
    fn total_value(
        balances: &HashMap<String, Decimal>,
        prices: &HashMap<String, Decimal>,
    ) -> Decimal {
        balances
            .iter()
            .map(|(asset, &qty)| {
                let price = prices.get(asset).copied().unwrap_or(Decimal::ONE);
                qty * price
            })
            .sum()
    }

    /// Return `true` when at least one asset deviates from its target by more
    /// than `rebalance_threshold_pct`.
    pub fn needs_rebalance(
        &self,
        current_balances: &HashMap<String, Decimal>,
        prices: &HashMap<String, Decimal>,
    ) -> bool {
        if self.status != BotStatus::Active {
            return false;
        }

        let total = Self::total_value(current_balances, prices);
        if total == Decimal::ZERO {
            return false;
        }

        self.config.target_allocation.iter().any(|(asset, &target)| {
            let qty = current_balances.get(asset).copied().unwrap_or(Decimal::ZERO);
            let price = prices.get(asset).copied().unwrap_or(Decimal::ONE);
            let current_pct = qty * price / total * dec_100();
            let target_pct = target * dec_100();
            (current_pct - target_pct).abs() > self.config.rebalance_threshold_pct
        })
    }

    /// Calculate the set of buy/sell actions needed to bring the portfolio back
    /// to its target allocation.
    ///
    /// For each asset:
    /// - `current_pct > target_pct` → sell the excess value.
    /// - `current_pct < target_pct` → buy the deficit value.
    ///
    /// Actions with a zero amount are omitted.
    pub fn calculate_rebalance(
        &self,
        current_balances: &HashMap<String, Decimal>,
        prices: &HashMap<String, Decimal>,
    ) -> Vec<RebalanceAction> {
        let total = Self::total_value(current_balances, prices);
        if total == Decimal::ZERO {
            return vec![];
        }

        let mut actions = Vec::new();

        for (asset, &target_fraction) in &self.config.target_allocation {
            let qty = current_balances.get(asset).copied().unwrap_or(Decimal::ZERO);
            let price = prices.get(asset).copied().unwrap_or(Decimal::ONE);
            let current_value = qty * price;
            let target_value = total * target_fraction;
            let diff = target_value - current_value; // positive → need to buy, negative → need to sell

            if diff == Decimal::ZERO {
                continue;
            }

            let (action, amount) = if diff > Decimal::ZERO {
                (RebalanceDirection::Buy, diff)
            } else {
                (RebalanceDirection::Sell, diff.abs())
            };

            actions.push(RebalanceAction {
                asset: asset.clone(),
                action,
                amount,
            });
        }

        // Stable ordering by asset name for deterministic output.
        actions.sort_by(|a, b| a.asset.cmp(&b.asset));
        actions
    }
}

fn dec_100() -> Decimal {
    Decimal::from(100u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_config() -> RebalancerConfig {
        let mut target = HashMap::new();
        target.insert("BTC".to_string(), dec!(0.50));
        target.insert("ETH".to_string(), dec!(0.30));
        target.insert("USDT".to_string(), dec!(0.20));

        RebalancerConfig {
            user_id: Uuid::new_v4(),
            target_allocation: target,
            rebalance_threshold_pct: dec!(5),
        }
    }

    fn balanced_state() -> (HashMap<String, Decimal>, HashMap<String, Decimal>) {
        // Portfolio worth 10 000 USDT exactly at target weights.
        // BTC: 50% → 5000 USDT → 0.1 BTC at 50 000
        // ETH: 30% → 3000 USDT → 1.0 ETH at 3 000
        // USDT: 20% → 2000 USDT → 2000 at price 1
        let mut balances = HashMap::new();
        balances.insert("BTC".to_string(), dec!(0.1));
        balances.insert("ETH".to_string(), dec!(1.0));
        balances.insert("USDT".to_string(), dec!(2000));

        let mut prices = HashMap::new();
        prices.insert("BTC".to_string(), dec!(50000));
        prices.insert("ETH".to_string(), dec!(3000));
        prices.insert("USDT".to_string(), dec!(1));

        (balances, prices)
    }

    #[test]
    fn new_rebalancer_is_active() {
        let r = Rebalancer::new(make_config());
        assert_eq!(r.status, BotStatus::Active);
    }

    #[test]
    fn needs_rebalance_false_when_balanced() {
        let r = Rebalancer::new(make_config());
        let (balances, prices) = balanced_state();
        assert!(!r.needs_rebalance(&balances, &prices));
    }

    #[test]
    fn needs_rebalance_true_when_btc_over_weight() {
        let r = Rebalancer::new(make_config());
        let (mut balances, prices) = balanced_state();
        // Double BTC position → heavily over-weight.
        *balances.get_mut("BTC").unwrap() = dec!(0.2);
        assert!(r.needs_rebalance(&balances, &prices));
    }

    #[test]
    fn needs_rebalance_true_when_eth_under_weight() {
        let r = Rebalancer::new(make_config());
        let (mut balances, prices) = balanced_state();
        // Remove most ETH.
        *balances.get_mut("ETH").unwrap() = dec!(0.01);
        assert!(r.needs_rebalance(&balances, &prices));
    }

    #[test]
    fn needs_rebalance_false_when_stopped() {
        let mut r = Rebalancer::new(make_config());
        r.status = BotStatus::Stopped;
        let (mut balances, prices) = balanced_state();
        *balances.get_mut("BTC").unwrap() = dec!(0.5); // would normally trigger
        assert!(!r.needs_rebalance(&balances, &prices));
    }

    #[test]
    fn calculate_rebalance_empty_when_balanced() {
        let r = Rebalancer::new(make_config());
        let (balances, prices) = balanced_state();
        let actions = r.calculate_rebalance(&balances, &prices);
        // Every action amount should be effectively zero (or no actions).
        assert!(
            actions.iter().all(|a| a.amount < dec!(0.01)),
            "Expected no significant actions for balanced portfolio"
        );
    }

    #[test]
    fn calculate_rebalance_sells_over_weight_asset() {
        let r = Rebalancer::new(make_config());
        let (mut balances, prices) = balanced_state();
        // BTC now 0.2 BTC = 10 000 USDT, total = 15 000, BTC weight = 66% (target 50%).
        *balances.get_mut("BTC").unwrap() = dec!(0.2);

        let actions = r.calculate_rebalance(&balances, &prices);
        let btc_action = actions.iter().find(|a| a.asset == "BTC").unwrap();
        assert_eq!(btc_action.action, RebalanceDirection::Sell);
        assert!(btc_action.amount > Decimal::ZERO);
    }

    #[test]
    fn calculate_rebalance_buys_under_weight_asset() {
        let r = Rebalancer::new(make_config());
        let (mut balances, prices) = balanced_state();
        // Remove most ETH so it's under-weight.
        *balances.get_mut("ETH").unwrap() = dec!(0.01);

        let actions = r.calculate_rebalance(&balances, &prices);
        let eth_action = actions.iter().find(|a| a.asset == "ETH").unwrap();
        assert_eq!(eth_action.action, RebalanceDirection::Buy);
        assert!(eth_action.amount > Decimal::ZERO);
    }

    #[test]
    fn calculate_rebalance_buy_and_sell_sum_to_roughly_zero() {
        // Total portfolio value is conserved, so total buys ≈ total sells.
        let r = Rebalancer::new(make_config());
        let (mut balances, prices) = balanced_state();
        *balances.get_mut("BTC").unwrap() = dec!(0.2); // over-weight BTC

        let actions = r.calculate_rebalance(&balances, &prices);
        let net: Decimal = actions
            .iter()
            .map(|a| match a.action {
                RebalanceDirection::Buy => a.amount,
                RebalanceDirection::Sell => -a.amount,
            })
            .sum();

        // Net should be near zero (rounding may cause tiny differences).
        assert!(net.abs() < dec!(0.01), "net rebalance amount should be ~0, got {}", net);
    }

    #[test]
    fn calculate_rebalance_returns_empty_when_no_balances() {
        let r = Rebalancer::new(make_config());
        let actions = r.calculate_rebalance(&HashMap::new(), &HashMap::new());
        assert!(actions.is_empty());
    }

    #[test]
    fn threshold_prevents_rebalance_on_small_deviation() {
        let r = Rebalancer::new(make_config()); // threshold = 5%
        let (mut balances, prices) = balanced_state();
        // Shift BTC slightly (2%) — should not trigger.
        // BTC: 0.102 → 5100 USDT, total 10100, BTC% = 50.5% (delta = 0.5% < 5%)
        *balances.get_mut("BTC").unwrap() = dec!(0.102);
        assert!(!r.needs_rebalance(&balances, &prices));
    }
}
