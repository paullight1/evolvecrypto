//! Exchange settlement — funding rates, PnL, insurance fund, and scheduling.

pub mod error;
pub mod funding;
pub mod insurance_fund;
pub mod pnl;
pub mod scheduler;

pub use error::SettlementError;
pub use funding::{
    calculate_funding_rate, calculate_funding_rate_twap, FundingHistory, FundingPayment,
    FundingRateRecord, FundingSettlement, PremiumBuffer,
};
pub use insurance_fund::{Clawback, InsuranceFund, SocializedLoss};
pub use pnl::{calculate_effective_leverage, calculate_realized_pnl, calculate_roe, PnlSummary};
pub use scheduler::{is_funding_time, next_funding_time, FundingScheduler};
