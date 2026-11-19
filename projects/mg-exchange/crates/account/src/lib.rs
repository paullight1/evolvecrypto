pub mod balance;
mod account_manager;
mod error;
mod fee_engine;
mod referrals;
pub mod sharded;
mod sub_accounts;
mod transfers;
pub mod unified_account;
mod wallet;

pub use balance::Balance;
pub use account_manager::{AccountManager, LockInfo, FeeInfo, FeeEntry};
pub use sharded::{ShardedAccounts, N_SHARDS};
pub use wallet::{CoinBalance, WalletSummary};
pub use error::AccountError;
pub use fee_engine::{FeeEngine, VipTier, UserFeeState};
pub use referrals::{ReferralManager, CommissionRecord, ReferralError};
pub use sub_accounts::{
    SubAccountManager, SubAccountGroup, SubAccount, SubAccountError,
    PortfolioSummary, AccountSummary,
};
pub use transfers::{TransferManager, TransferRecord, TransferType, TransferError};
pub use unified_account::{UnifiedAccount, AccountMode, collateral_haircut};
