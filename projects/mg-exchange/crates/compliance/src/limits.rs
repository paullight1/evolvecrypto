use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::kyc::KycLevel;

/// Trading limits based on KYC verification level.
///
/// Ukrainian Virtual Assets Law thresholds:
/// - > 30,000 UAH (~$750): enhanced due diligence required
/// - > 400,000 UAH (~$10,000): mandatory reporting to SFMS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingLimits {
    pub can_trade: bool,
    pub can_deposit: bool,
    pub can_withdraw: bool,
    /// Daily trading volume limit in USD equivalent.
    pub daily_trade_usd: Decimal,
    /// Daily withdrawal limit in USD equivalent.
    pub daily_withdraw_usd: Decimal,
    /// Single transaction limit in USD equivalent.
    pub single_tx_usd: Decimal,
}

impl TradingLimits {
    /// Get limits for a KYC level.
    pub fn for_level(level: KycLevel) -> Self {
        match level {
            KycLevel::None => Self {
                can_trade: false,
                can_deposit: false,
                can_withdraw: false,
                daily_trade_usd: Decimal::ZERO,
                daily_withdraw_usd: Decimal::ZERO,
                single_tx_usd: Decimal::ZERO,
            },
            KycLevel::Basic => Self {
                can_trade: true,
                can_deposit: true,
                can_withdraw: true,
                daily_trade_usd: Decimal::new(750, 0),      // $750 (≈30,000 UAH)
                daily_withdraw_usd: Decimal::new(500, 0),    // $500
                single_tx_usd: Decimal::new(750, 0),         // $750
            },
            KycLevel::Full => Self {
                can_trade: true,
                can_deposit: true,
                can_withdraw: true,
                daily_trade_usd: Decimal::new(100_000, 0),   // $100,000
                daily_withdraw_usd: Decimal::new(50_000, 0), // $50,000
                single_tx_usd: Decimal::new(100_000, 0),     // $100,000
            },
        }
    }

    /// Suspended account — everything blocked.
    pub fn suspended() -> Self {
        Self {
            can_trade: false,
            can_deposit: false,
            can_withdraw: false,
            daily_trade_usd: Decimal::ZERO,
            daily_withdraw_usd: Decimal::ZERO,
            single_tx_usd: Decimal::ZERO,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_none_level_blocks_all() {
        let limits = TradingLimits::for_level(KycLevel::None);
        assert!(!limits.can_trade);
        assert!(!limits.can_deposit);
        assert!(!limits.can_withdraw);
        assert_eq!(limits.daily_trade_usd, dec!(0));
    }

    #[test]
    fn test_basic_level_limited() {
        let limits = TradingLimits::for_level(KycLevel::Basic);
        assert!(limits.can_trade);
        assert_eq!(limits.daily_trade_usd, dec!(750));
        assert_eq!(limits.daily_withdraw_usd, dec!(500));
    }

    #[test]
    fn test_full_level_high_limits() {
        let limits = TradingLimits::for_level(KycLevel::Full);
        assert!(limits.can_trade);
        assert_eq!(limits.daily_trade_usd, dec!(100000));
        assert_eq!(limits.daily_withdraw_usd, dec!(50000));
    }

    #[test]
    fn test_suspended_blocks_all() {
        let limits = TradingLimits::suspended();
        assert!(!limits.can_trade);
        assert_eq!(limits.daily_trade_usd, dec!(0));
    }
}
