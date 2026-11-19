//! Insurance fund management for perpetual futures.
//!
//! The insurance fund absorbs the difference between the bankruptcy price and
//! the actual liquidation fill price. When the fund is depleted, losses are
//! socialised across all profitable positions for that symbol.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use exchange_common::{Symbol, UserId};

use crate::error::SettlementError;

/// Insurance fund for a single symbol.
///
/// Invariant: balance >= 0 at all times.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsuranceFund {
    pub symbol: Symbol,
    balance: Decimal,
}

impl InsuranceFund {
    /// Create a new insurance fund with an optional seed balance.
    pub fn new(symbol: Symbol, initial_balance: Decimal) -> Self {
        debug_assert!(
            initial_balance >= Decimal::ZERO,
            "insurance fund initial balance must be non-negative"
        );
        Self {
            symbol,
            balance: initial_balance.max(Decimal::ZERO),
        }
    }

    /// Current fund balance (quote asset).
    pub fn balance(&self) -> Decimal {
        self.balance
    }

    /// Add surplus from a profitable liquidation.
    ///
    /// This occurs when the liquidation fill price is better than the
    /// bankruptcy price, leaving leftover margin that is contributed to the fund.
    pub fn add_surplus(&mut self, amount: Decimal) -> Result<(), SettlementError> {
        if amount < Decimal::ZERO {
            return Err(SettlementError::NegativeAmount(amount));
        }
        self.balance += amount;
        info!(
            symbol = %self.symbol,
            amount = %amount,
            balance = %self.balance,
            "insurance fund surplus added"
        );
        Ok(())
    }

    /// Attempt to cover a deficit from a loss-making liquidation.
    ///
    /// Returns `Ok(())` if the fund had enough to cover the full amount.
    /// Returns `Err(SettlementError::InsufficientInsuranceFund)` otherwise;
    /// in that case the fund balance is left unchanged so the caller can
    /// decide how to handle the shortfall (e.g., socialised loss).
    pub fn cover_deficit(&mut self, amount: Decimal) -> Result<(), SettlementError> {
        if amount < Decimal::ZERO {
            return Err(SettlementError::NegativeAmount(amount));
        }
        if amount > self.balance {
            warn!(
                symbol = %self.symbol,
                requested = %amount,
                available = %self.balance,
                "insurance fund insufficient — socialised loss may follow"
            );
            return Err(SettlementError::InsufficientInsuranceFund {
                requested: amount,
                available: self.balance,
            });
        }
        self.balance -= amount;
        info!(
            symbol = %self.symbol,
            amount = %amount,
            balance = %self.balance,
            "insurance fund covered deficit"
        );
        Ok(())
    }
}

/// A socialised loss event triggered when the insurance fund is depleted.
///
/// The uncovered shortfall is distributed proportionally across all
/// profitable positions (winners absorb the loss of the defaulted position).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocializedLoss {
    pub symbol: Symbol,
    /// Total shortfall that could not be covered by the insurance fund.
    pub total_shortfall: Decimal,
    /// Per-user clawback amounts (always non-negative; actual deduction from profit).
    pub clawbacks: Vec<Clawback>,
}

/// A single user's share of a socialised loss.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clawback {
    pub user_id: UserId,
    /// Amount deducted from the user's unrealised/realised profit.
    pub amount: Decimal,
}

impl SocializedLoss {
    /// Distribute `shortfall` proportionally across `profitable_positions`.
    ///
    /// Each entry is `(user_id, profit)` where `profit > 0`.
    /// Returns `None` if there are no profitable positions (nothing to claw back from).
    pub fn distribute(
        symbol: Symbol,
        shortfall: Decimal,
        profitable_positions: &[(UserId, Decimal)],
    ) -> Option<Self> {
        if shortfall <= Decimal::ZERO {
            return None;
        }

        let total_profit: Decimal = profitable_positions
            .iter()
            .filter(|(_, p)| *p > Decimal::ZERO)
            .map(|(_, p)| *p)
            .sum();

        if total_profit.is_zero() {
            return None;
        }

        let clawbacks = profitable_positions
            .iter()
            .filter(|(_, profit)| *profit > Decimal::ZERO)
            .map(|(user_id, profit)| {
                let share = profit / total_profit;
                let amount = (shortfall * share).round_dp(8);
                Clawback {
                    user_id: *user_id,
                    amount,
                }
            })
            .collect();

        Some(SocializedLoss {
            symbol,
            total_shortfall: shortfall,
            clawbacks,
        })
    }

    /// Total amount actually clawed back (may differ from `total_shortfall` by rounding).
    pub fn total_clawed_back(&self) -> Decimal {
        self.clawbacks.iter().map(|c| c.amount).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    // --- InsuranceFund ---

    #[test]
    fn test_new_fund_zero_balance() {
        let fund = InsuranceFund::new(Symbol::new("BTCUSDT"), Decimal::ZERO);
        assert_eq!(fund.balance(), Decimal::ZERO);
    }

    #[test]
    fn test_add_surplus_increases_balance() {
        let mut fund = InsuranceFund::new(Symbol::new("BTCUSDT"), dec!(1000));
        fund.add_surplus(dec!(500)).unwrap();
        assert_eq!(fund.balance(), dec!(1500));
    }

    #[test]
    fn test_add_surplus_negative_errors() {
        let mut fund = InsuranceFund::new(Symbol::new("BTCUSDT"), dec!(1000));
        assert!(matches!(
            fund.add_surplus(dec!(-100)),
            Err(SettlementError::NegativeAmount(_))
        ));
    }

    #[test]
    fn test_cover_deficit_success() {
        let mut fund = InsuranceFund::new(Symbol::new("BTCUSDT"), dec!(1000));
        fund.cover_deficit(dec!(400)).unwrap();
        assert_eq!(fund.balance(), dec!(600));
    }

    #[test]
    fn test_cover_deficit_exact_balance() {
        let mut fund = InsuranceFund::new(Symbol::new("BTCUSDT"), dec!(1000));
        fund.cover_deficit(dec!(1000)).unwrap();
        assert_eq!(fund.balance(), Decimal::ZERO);
    }

    #[test]
    fn test_cover_deficit_insufficient_errors() {
        let mut fund = InsuranceFund::new(Symbol::new("BTCUSDT"), dec!(100));
        let err = fund.cover_deficit(dec!(500)).unwrap_err();
        assert!(matches!(
            err,
            SettlementError::InsufficientInsuranceFund { .. }
        ));
        // Balance must be unchanged after failure
        assert_eq!(fund.balance(), dec!(100));
    }

    #[test]
    fn test_cover_deficit_negative_errors() {
        let mut fund = InsuranceFund::new(Symbol::new("BTCUSDT"), dec!(1000));
        assert!(matches!(
            fund.cover_deficit(dec!(-50)),
            Err(SettlementError::NegativeAmount(_))
        ));
    }

    #[test]
    fn test_surplus_then_deficit_cycle() {
        let mut fund = InsuranceFund::new(Symbol::new("BTCUSDT"), Decimal::ZERO);
        fund.add_surplus(dec!(200)).unwrap();
        fund.add_surplus(dec!(300)).unwrap();
        assert_eq!(fund.balance(), dec!(500));
        fund.cover_deficit(dec!(150)).unwrap();
        assert_eq!(fund.balance(), dec!(350));
    }

    // --- SocializedLoss ---

    #[test]
    fn test_distribute_proportional() {
        let user_a = Uuid::new_v4();
        let user_b = Uuid::new_v4();

        // A has 3000 profit, B has 1000 → A pays 75%, B pays 25%
        let loss = SocializedLoss::distribute(
            Symbol::new("BTCUSDT"),
            dec!(1000),
            &[(user_a, dec!(3000)), (user_b, dec!(1000))],
        )
        .unwrap();

        assert_eq!(loss.total_shortfall, dec!(1000));
        assert_eq!(loss.clawbacks.len(), 2);

        let cb_a = loss.clawbacks.iter().find(|c| c.user_id == user_a).unwrap();
        let cb_b = loss.clawbacks.iter().find(|c| c.user_id == user_b).unwrap();

        assert_eq!(cb_a.amount, dec!(750));
        assert_eq!(cb_b.amount, dec!(250));
    }

    #[test]
    fn test_distribute_zero_shortfall_returns_none() {
        let result = SocializedLoss::distribute(
            Symbol::new("BTCUSDT"),
            Decimal::ZERO,
            &[(Uuid::new_v4(), dec!(1000))],
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_distribute_no_profitable_positions_returns_none() {
        let result = SocializedLoss::distribute(
            Symbol::new("BTCUSDT"),
            dec!(500),
            &[(Uuid::new_v4(), Decimal::ZERO)],
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_distribute_skips_non_profitable() {
        let user_a = Uuid::new_v4();
        let user_b = Uuid::new_v4();

        // user_b has zero profit — only user_a should absorb
        let loss = SocializedLoss::distribute(
            Symbol::new("BTCUSDT"),
            dec!(500),
            &[(user_a, dec!(1000)), (user_b, Decimal::ZERO)],
        )
        .unwrap();

        assert_eq!(loss.clawbacks.len(), 1);
        assert_eq!(loss.clawbacks[0].user_id, user_a);
        assert_eq!(loss.clawbacks[0].amount, dec!(500));
    }

    #[test]
    fn test_total_clawed_back() {
        let user_a = Uuid::new_v4();
        let user_b = Uuid::new_v4();
        let loss = SocializedLoss::distribute(
            Symbol::new("BTCUSDT"),
            dec!(1000),
            &[(user_a, dec!(1000)), (user_b, dec!(1000))],
        )
        .unwrap();
        // Each gets exactly 500
        assert_eq!(loss.total_clawed_back(), dec!(1000));
    }
}
