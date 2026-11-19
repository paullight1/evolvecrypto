mod deposit;
mod withdrawal;
mod provider;
pub mod address_whitelist;
pub mod document_storage;
pub mod fireblocks;
pub mod self_hosted;
pub mod wallet_management;
pub mod webhook;

pub use deposit::{DepositManager, DepositRecord, DepositStatus, required_confirmations};
pub use withdrawal::{
    DailyLimitConfig, UserTier, WithdrawalError, WithdrawalLimitError, WithdrawalManager,
    WithdrawalRecord, WithdrawalStatus,
};
pub use provider::{CustodyProvider, CustodyError, MockCustodyProvider, DepositAddress, ProviderTxStatus};
pub use wallet_management::{WalletInfo, WalletManager, WalletPolicy, WalletType};
pub use address_whitelist::{AddressWhitelist, WhitelistedAddress};
pub use fireblocks::{FireblocksCustodyProvider, FireblocksConfig, FireblocksWebhookEvent};
pub use self_hosted::SelfHostedCustodyProvider;
pub use webhook::{DepositEvent, DepositEventStatus, parse_deposit_event, fireblocks_id_to_asset};
pub use custody_event::CustodyEvent;

pub mod custody_event;
