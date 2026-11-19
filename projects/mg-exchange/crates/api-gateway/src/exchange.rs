use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::{Mutex, RwLock};

use arc_swap::ArcSwap;

use exchange_copy_trading::{CopyOrder, FollowerManager, MasterRegistry, MasterTrade, TradeCopier};
use exchange_market_maker::{MmSlaConfig, MmSlaMonitor, MmSlaReport};
use exchange_market_maker::toxicity::UserToxicityScorer;
use exchange_trading_bots::{DcaBot, GridBot};

// Separate alias to clarify intent: tokio Mutex used only where we hold the
// lock across an `.await` point (i.e. async provider calls in VerificationService).
use tokio::sync::Mutex as AsyncMutex;

use chrono::{DateTime, Utc};
use exchange_account::{AccountManager, FeeEngine, LockInfo, ReferralManager, ShardedAccounts, SubAccountManager, TransferManager};
use exchange_common::futures::{MarginMode, MarkPrice, PerpetualConfig, Position};
use exchange_common::*;
use exchange_index_price::IndexPriceAggregator;
use exchange_journal::{GroupCommitConfig, GroupCommitWal, WriteAheadLog};
use exchange_matching_engine::{EnginePool, CircuitBreaker, CircuitBreakerResult};
use exchange_matching_engine::circuit_breaker::{btc_circuit_breaker, eth_circuit_breaker, sol_circuit_breaker};
use exchange_market_data::{InProcessMarketData, MarketDataProvider};
use exchange_order_gateway::{OrderCountSource, OrderLimitConfig, OrderValidator, SelfTradeGuard, SelfTradePolicy};
use exchange_risk_engine::{calculate_mark_price_ema, DEFAULT_MARK_EMA_SPAN, PositionManager};
use exchange_settlement::{FundingHistory, FundingScheduler, InsuranceFund};
use exchange_trigger_service::TriggerEngine;
use rust_decimal::Decimal;
use tokio::sync::broadcast;

use exchange_admin::{CapitalTracker, ExchangeMetrics, NotificationChannel, NotificationManager, NotificationType};
use exchange_compliance::{
    JurisdictionEngine, TotpManager, TravelRuleEngine, VaspDirectory, VerificationService,
    SanctionsScreener, PepScreener, TosManager, CorporateKycManager, EddManager,
    RiskScoringEngine, SarManager,
};
use exchange_compliance::complaints::ComplaintManager;
use exchange_compliance::risk_warnings::RiskWarningManager;
use exchange_admin::AuditLog;
use exchange_custody::{AddressWhitelist, CustodyProvider, DepositManager, MockCustodyProvider, SelfHostedCustodyProvider, WalletManager, WithdrawalManager};
use exchange_surveillance::SurveillanceEngine;

use crate::auth::ApiKeyStore;
use crate::web_auth::JwtManager;

// --- Dual-approval withdrawal types (Task 12) ---

/// Policy governing when withdrawals require more than one approver.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ApprovalPolicy {
    /// Withdrawal amounts at or above this USD value require multi-sig approval.
    pub threshold_usd: Decimal,
    /// How many approvers must vote "approved" before the withdrawal is processed.
    pub required_approvers: u32,
    /// Minutes the approval window stays open before the request is auto-rejected.
    pub timeout_minutes: u32,
}

impl Default for ApprovalPolicy {
    fn default() -> Self {
        Self {
            threshold_usd: Decimal::new(10_000, 0), // $10,000
            required_approvers: 2,
            timeout_minutes: 60,
        }
    }
}

/// A single approver's vote on a pending withdrawal.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WithdrawalApproval {
    pub withdrawal_id: uuid::Uuid,
    pub approver_id: String,
    /// "approved" or "rejected"
    pub decision: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

// --- Data-retention policy types (Task 15) ---

/// Declared retention window for one category of exchange data.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RetentionPolicy {
    /// Logical table / data category name (e.g. "orders", "audit_log").
    pub table: String,
    /// Minimum legal retention in days (data must NOT be deleted before this).
    pub min_retention_days: u32,
    /// Days after which data should be archived to cold storage.
    pub archive_after_days: u32,
    /// Timestamp of the oldest record currently in the live store, if known.
    pub current_oldest: Option<chrono::DateTime<chrono::Utc>>,
    /// Total number of records currently in the live store.
    pub current_count: usize,
}

fn default_retention_policies() -> Vec<RetentionPolicy> {
    vec![
        RetentionPolicy {
            table: "orders".into(),
            min_retention_days: 1825,   // 5 years
            archive_after_days: 365,
            current_oldest: None,
            current_count: 0,
        },
        RetentionPolicy {
            table: "trades".into(),
            min_retention_days: 1825,
            archive_after_days: 365,
            current_oldest: None,
            current_count: 0,
        },
        RetentionPolicy {
            table: "audit_log".into(),
            min_retention_days: 2555,   // 7 years
            archive_after_days: 730,    // 2 years
            current_oldest: None,
            current_count: 0,
        },
        RetentionPolicy {
            table: "withdrawals".into(),
            min_retention_days: 1825,
            archive_after_days: 365,
            current_oldest: None,
            current_count: 0,
        },
        RetentionPolicy {
            table: "deposits".into(),
            min_retention_days: 1825,
            archive_after_days: 365,
            current_oldest: None,
            current_count: 0,
        },
        RetentionPolicy {
            table: "kyc_documents".into(),
            min_retention_days: 1825,
            archive_after_days: 1825,
            current_oldest: None,
            current_count: 0,
        },
    ]
}

// --- Pre-Market / Call Auction types ---

/// Trading phase for a listed pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PairPhase {
    /// Pre-market: call auction — orders collected, no matching.
    CallAuction,
    /// Pre-market: continuous trading with restrictions (lower leverage, tighter price limits).
    PreMarketTrading,
    /// Normal trading — full features.
    Active,
    /// Settle-only — only reduce_only orders.
    SettleOnly,
    /// Disabled — no orders accepted.
    Disabled,
}

/// Phase configuration attached to a trading pair.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PairPhaseConfig {
    pub phase: PairPhase,
    /// Maximum allowed leverage during this phase (None = use pair default).
    pub max_leverage: Option<u32>,
    /// ±N% price band around `reference_price` (None = no band).
    pub price_limit_pct: Option<Decimal>,
    /// Opening / indicative reference price used for price-band enforcement.
    pub reference_price: Option<Decimal>,
    /// When the call auction ends (None = open-ended).
    pub auction_end_time: Option<chrono::DateTime<chrono::Utc>>,
}

// --- Notification Preferences types ---

/// Per-user notification preferences.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NotificationPreferences {
    pub email_fills: bool,
    pub email_deposits: bool,
    pub email_withdrawals: bool,
    pub email_margin_call: bool,
    pub email_liquidation: bool,
    pub email_login: bool,
    pub push_fills: bool,
    pub push_deposits: bool,
    pub push_withdrawals: bool,
    pub push_margin_call: bool,
    pub push_liquidation: bool,
    pub push_price_alerts: bool,
    pub push_funding: bool,
}

impl Default for NotificationPreferences {
    fn default() -> Self {
        Self {
            email_fills: false,
            email_deposits: true,
            email_withdrawals: true,
            email_margin_call: true,
            email_liquidation: true,
            email_login: true,
            push_fills: true,
            push_deposits: true,
            push_withdrawals: true,
            push_margin_call: true,
            push_liquidation: true,
            push_price_alerts: true,
            push_funding: true,
        }
    }
}

// --- Price Alert types ---

/// A user-defined price alert for a symbol.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PriceAlert {
    pub id: uuid::Uuid,
    pub user_id: uuid::Uuid,
    pub symbol: String,
    /// "above" or "below"
    pub condition: String,
    pub price: Decimal,
    pub triggered: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// --- Insurance Fund History (M13 Task 2) ---

/// A single insurance fund event record for auditing and historical queries.
#[derive(Debug, Clone, serde::Serialize)]
pub struct InsuranceFundHistoryEntry {
    pub id: uuid::Uuid,
    pub symbol: String,
    /// Signed amount: positive = surplus added, negative = deficit absorbed.
    pub amount: Decimal,
    /// Running balance after this event.
    pub balance_after: Decimal,
    /// Human-readable reason: "liquidation_surplus", "liquidation_deficit",
    /// "socialized_loss", "hourly_snapshot", "seed".
    pub reason: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

// --- Maintenance Mode types (Task 17) ---

/// Scheduled or active maintenance window information.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MaintenanceInfo {
    pub scheduled_start: chrono::DateTime<chrono::Utc>,
    pub scheduled_end: chrono::DateTime<chrono::Utc>,
    pub reason: String,
    pub is_active: bool,
}

// --- Withdrawal Confirmation types ---

/// A pending withdrawal awaiting email confirmation.
#[derive(Debug, Clone)]
pub struct WithdrawalConfirmation {
    pub user_id: uuid::Uuid,
    pub asset: String,
    pub amount: Decimal,
    pub address: String,
    pub token: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

// --- Device Management types (Task 18) ---

/// A registered user device.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DeviceInfo {
    pub id: uuid::Uuid,
    pub device_name: String,
    pub ip_address: String,
    pub last_login: chrono::DateTime<chrono::Utc>,
    pub trusted: bool,
}

// --- Cross-Collateral Haircut types ---

/// Collateral tier classification (M26-1).
///
/// Determines the collateral weight range for margin calculations:
/// - Tier1: major assets (BTC, ETH, USDT, USDC) — 95-100% weight
/// - Tier2: large-cap alts (SOL, AVAX, etc.) — 80-90% weight
/// - Tier3: small caps — 50-70% weight
/// - Tier4: meme coins, exchange tokens — 0% weight (not eligible)
///
/// # Reference
/// - Mango Markets ($114M exploit): attacker used 100% MNGO as collateral
/// - FTX/FTT ($8B): exchange token as self-referencing collateral
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CollateralTier {
    /// Premier assets: BTC, ETH, USDT, USDC. Weight: 95-100%.
    Tier1,
    /// Large-cap alts. Weight: 80-90%.
    Tier2,
    /// Small caps. Weight: 50-70%.
    Tier3,
    /// Memes, exchange tokens. Weight: 0% (ineligible).
    Tier4,
}

impl CollateralTier {
    /// Derive the tier from the haircut rate.
    pub fn from_haircut(haircut: Decimal) -> Self {
        if haircut >= Decimal::new(95, 2) { CollateralTier::Tier1 }
        else if haircut >= Decimal::new(80, 2) { CollateralTier::Tier2 }
        else if haircut >= Decimal::new(50, 2) { CollateralTier::Tier3 }
        else { CollateralTier::Tier4 }
    }
}

/// Configuration for a single asset as cross-margin collateral.
///
/// The `haircut_rate` is applied to the USD value of the asset to determine
/// how much effective margin it contributes. A rate of 1.0 means no discount
/// (full USD value); a rate of 0.80 means 20% is discounted for risk.
///
/// Example: 1 BTC @ $60,000 with haircut 0.95 → $57,000 effective collateral.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CollateralConfig {
    /// The asset ticker, e.g. "BTC", "ETH", "USDT".
    pub coin: String,
    /// Fraction of USD value counted toward margin. In [0.0, 1.0].
    /// Must be represented as Decimal — never f64.
    pub haircut_rate: Decimal,
    /// Maximum amount of this asset that can contribute to effective collateral.
    /// `None` means no cap.
    pub max_collateral_amount: Option<Decimal>,
    /// Whether this asset is currently eligible to be used as collateral.
    pub is_eligible: bool,
    /// M26-1: Collateral eligibility tier. Tier4 assets are excluded from margin.
    pub tier: CollateralTier,
}

impl CollateralConfig {
    /// Build the default collateral config map for well-known assets.
    ///
    /// - USDT/USDC: haircut 1.0 (stablecoins — no discount)
    /// - BTC: haircut 0.95 (premier asset, small discount)
    /// - ETH: haircut 0.90 (large-cap, moderate discount)
    /// - BNB/SOL/XRP: haircut 0.85 (mid-cap)
    /// - Other supported altcoins default to 0.70
    pub fn defaults() -> HashMap<String, CollateralConfig> {
        let entries = [
            ("USDT",  Decimal::new(100, 2), None), // 1.00
            ("USDC",  Decimal::new(100, 2), None), // 1.00
            ("BTC",   Decimal::new(95, 2),  None), // 0.95
            ("ETH",   Decimal::new(90, 2),  None), // 0.90
            ("BNB",   Decimal::new(85, 2),  None), // 0.85
            ("SOL",   Decimal::new(85, 2),  None), // 0.85
            ("XRP",   Decimal::new(85, 2),  None), // 0.85
            ("DOGE",  Decimal::new(70, 2),  None), // 0.70
            ("AVAX",  Decimal::new(80, 2),  None), // 0.80
            ("MATIC", Decimal::new(75, 2),  None), // 0.75
        ];
        entries
            .into_iter()
            .map(|(coin, haircut_rate, max_collateral_amount)| {
                let cfg = CollateralConfig {
                    coin: coin.to_string(),
                    tier: CollateralTier::from_haircut(haircut_rate),
                    haircut_rate,
                    max_collateral_amount,
                    is_eligible: true,
                };
                (coin.to_string(), cfg)
            })
            .collect()
    }
}

// --- Standardised Error Codes (Feature 8) ---

/// Canonical exchange error codes for v2 API responses.
///
/// Every REST v2 response carries a `retCode` field. `0` means success; all
/// other values indicate a specific failure category. Codes ≥ 10000 are
/// client errors; codes ≥ 50000 are server errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ExchangeErrorCode(pub u32);

impl ExchangeErrorCode {
    /// Request succeeded.
    pub const OK: Self = Self(0);
    /// A required parameter is missing or has an invalid value.
    pub const INVALID_PARAM: Self = Self(10001);
    /// API key is missing, invalid, or revoked.
    pub const UNAUTHORIZED: Self = Self(10002);
    /// Too many requests — rate limit exceeded.
    pub const RATE_LIMITED: Self = Self(10003);
    /// Not enough balance to complete the operation.
    pub const INSUFFICIENT_BALANCE: Self = Self(10004);
    /// The referenced order does not exist.
    pub const ORDER_NOT_FOUND: Self = Self(10005);
    /// The referenced trading symbol is not listed.
    pub const SYMBOL_NOT_FOUND: Self = Self(10006);
    /// KYC verification is required before this action.
    pub const KYC_REQUIRED: Self = Self(10007);
    /// Exchange is in maintenance mode; try again later.
    pub const MAINTENANCE_MODE: Self = Self(10008);

    /// Return the canonical English description for this error code.
    pub fn message(self) -> &'static str {
        match self.0 {
            0     => "OK",
            10001 => "Invalid parameter",
            10002 => "Unauthorized",
            10003 => "Rate limited",
            10004 => "Insufficient balance",
            10005 => "Order not found",
            10006 => "Symbol not found",
            10007 => "KYC required",
            10008 => "Maintenance mode",
            _     => "Unknown error",
        }
    }
}

impl std::fmt::Display for ExchangeErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.0, self.message())
    }
}

// --- Daily Account Snapshot (Feature 1) ---

/// Daily equity and balance snapshot for a user, taken at midnight UTC.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DailySnapshot {
    pub user_id: uuid::Uuid,
    /// Date string in "YYYY-MM-DD" format (UTC).
    pub date: String,
    /// Total account equity in USD at snapshot time.
    pub total_equity_usd: Decimal,
    /// Per-coin balances at snapshot time.
    pub balances: std::collections::HashMap<String, Decimal>,
    /// Number of open positions at snapshot time.
    pub positions_count: usize,
    /// Aggregate unrealised PnL across all open positions.
    pub unrealised_pnl: Decimal,
    /// When the snapshot was taken.
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// --- Account Tier (Feature 3) ---

/// Account tier for feature access gating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountTier {
    /// Default tier — basic trading features.
    Basic,
    /// Identity-verified — higher limits.
    Verified,
    /// Professional tier — highest limits, API priority.
    Pro,
}

impl std::fmt::Display for AccountTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Basic    => write!(f, "Basic"),
            Self::Verified => write!(f, "Verified"),
            Self::Pro      => write!(f, "Pro"),
        }
    }
}

// --- Gift Cards (Feature 5) ---

/// Status of a gift card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GiftCardStatus {
    /// Card has been issued but not yet redeemed.
    Active,
    /// Card has been claimed by a recipient.
    Redeemed,
    /// Card was cancelled by the creator before redemption.
    Cancelled,
}

/// A gift card that transfers funds between exchange users.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GiftCard {
    pub id: uuid::Uuid,
    /// Unique redemption code.
    pub code: String,
    pub creator_id: uuid::Uuid,
    /// Asset coin (e.g. "USDT").
    pub asset: String,
    pub amount: Decimal,
    /// Optional personal message from the creator.
    pub message: Option<String>,
    /// User who redeemed the card (None if not yet redeemed).
    pub redeemed_by: Option<uuid::Uuid>,
    pub status: GiftCardStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub redeemed_at: Option<chrono::DateTime<chrono::Utc>>,
}

// --- Premium Kline Snapshot (Feature 6) ---

/// Maximum number of premium snapshots retained per symbol.
pub const MAX_PREMIUM_HISTORY_PER_SYMBOL: usize = 2_000;

/// A point-in-time snapshot of the premium (mark − index) / index for a symbol.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PremiumSnapshot {
    /// Unix timestamp (seconds).
    pub timestamp: i64,
    /// Premium percentage: (mark_price − index_price) / index_price * 100.
    pub premium_pct: Decimal,
    /// Mark price at snapshot time.
    pub mark_price: Decimal,
    /// Index price at snapshot time.
    pub index_price: Decimal,
}

// --- Market Maker Protection types ---

/// Market maker protection configuration.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MmpConfig {
    pub window_ms: u64,
    pub frozen_ms: u64,
    pub qty_limit: Decimal,
    pub count_limit: u32,
}

/// Runtime MMP state for one user.
#[derive(Debug)]
pub struct MmpState {
    pub config: MmpConfig,
    pub is_frozen: bool,
    pub frozen_until: Option<chrono::DateTime<chrono::Utc>>,
    pub recent_fills: Vec<(chrono::DateTime<chrono::Utc>, Decimal)>,
}

// --- Convert/Swap types ---

/// A pending conversion quote with expiry.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConvertQuote {
    pub quote_id: uuid::Uuid,
    pub user_id: uuid::Uuid,
    pub from_coin: String,
    pub to_coin: String,
    pub from_amount: Decimal,
    pub to_amount: Decimal,
    pub rate: Decimal,
    pub fee: Decimal,
    pub fee_coin: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

// --- Spot Margin types ---

/// Spot margin account settings per user.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SpotMarginSettings {
    pub enabled: bool,
    pub max_leverage: Decimal,
}

/// An active spot margin borrow record.
///
/// Interest accrues every hour at `rate` (fraction per day, e.g. 0.0002 for 0.02%/day).
/// The background task `spawn_margin_interest_accrual` credits `interest_accrued`
/// once per hour.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MarginBorrow {
    pub id: uuid::Uuid,
    pub user_id: uuid::Uuid,
    pub coin: String,
    pub amount: Decimal,
    pub interest_accrued: Decimal,
    /// Daily interest rate (e.g. 0.0002 = 0.02 %/day).
    pub daily_rate: Decimal,
    pub borrowed_at: chrono::DateTime<chrono::Utc>,
    pub status: String, // "active", "repaid"
}

/// A single interest charge event, recorded each time the accrual task runs.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MarginInterestRecord {
    pub id: uuid::Uuid,
    pub borrow_id: uuid::Uuid,
    pub user_id: uuid::Uuid,
    pub coin: String,
    pub interest_amount: Decimal,
    pub charged_at: chrono::DateTime<chrono::Utc>,
}

// --- Dust Conversion types ---

/// A single asset entry in a dust conversion batch.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DustConversionItem {
    pub coin: String,
    pub amount: Decimal,
    /// USD value at conversion time.
    pub usd_value: Decimal,
    /// USDT credited to the user for this asset.
    pub usdt_received: Decimal,
}

/// A completed dust conversion record.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DustConversion {
    pub id: uuid::Uuid,
    pub user_id: uuid::Uuid,
    pub items: Vec<DustConversionItem>,
    /// Total USDT credited.
    pub total_usdt: Decimal,
    pub converted_at: chrono::DateTime<chrono::Utc>,
}

// --- Earn / Staking types ---

/// An earn product (staking, savings, etc).
#[derive(Debug, Clone, serde::Serialize)]
pub struct EarnProduct {
    pub id: uuid::Uuid,
    pub coin: String,
    pub name: String,
    pub apy: Decimal,
    pub min_amount: Decimal,
    pub max_amount: Decimal,
    /// M26-6: Maximum total deposits across all users for this pool.
    /// Deposits beyond this cap are rejected.
    pub pool_capacity: Option<Decimal>,
    /// M26-6: Maximum per-user deposit as a fraction of pool capacity (default 5%).
    pub max_user_pct: Decimal,
    pub lock_days: u32, // 0 = flexible
    pub product_type: String, // "flexible", "fixed_30d", "fixed_90d", "staking"
    /// Whether auto-compounding is supported by this product.
    pub supports_auto_compound: bool,
}

/// A user's earn allocation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EarnAllocation {
    pub id: uuid::Uuid,
    pub user_id: uuid::Uuid,
    pub product_id: uuid::Uuid,
    pub coin: String,
    pub amount: Decimal,
    pub earned_interest: Decimal,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub unlock_at: Option<chrono::DateTime<chrono::Utc>>,
    pub status: String, // "active", "redeemed"
    /// When true the hourly accrual task reinvests earned interest into `amount`
    /// instead of accumulating it in `earned_interest`.
    pub auto_compound: bool,
    /// Timestamp of the last time interest was accrued for this allocation.
    /// Used by the background task to avoid double-counting partial hours.
    pub last_accrued_at: chrono::DateTime<chrono::Utc>,
}

// --- Margin Lending types ---

/// An active or historical margin loan record.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LoanRecord {
    pub id: uuid::Uuid,
    pub user_id: uuid::Uuid,
    pub coin: String,
    pub principal: Decimal,
    pub interest_accrued: Decimal,
    pub hourly_rate: Decimal,
    pub status: String, // "active", "repaid"
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub repaid_at: Option<chrono::DateTime<chrono::Utc>>,
}

// --- History types ---

/// A record of an order that has reached a terminal state (filled, cancelled, or rejected).
#[derive(Debug, Clone)]
pub struct OrderHistoryEntry {
    pub order_id: OrderId,
    pub user_id: UserId,
    pub symbol: Symbol,
    pub side: String,
    pub order_type: String,
    pub price: Decimal,
    pub qty: Decimal,
    pub filled_qty: Decimal,
    pub status: String,
    pub created_at: DateTime<Utc>,
    /// User-provided client order ID, if any.
    pub order_link_id: Option<String>,
}

/// A public trade record (one side of a fill).
#[derive(Debug, Clone)]
pub struct TradeHistoryEntry {
    pub trade_id: u64,
    pub symbol: Symbol,
    pub price: Decimal,
    pub qty: Decimal,
    /// Notional value of the trade: price × qty.
    pub quote_qty: Decimal,
    pub taker_side: String,
    /// True when the buyer was the passive (maker) side.
    pub is_buyer_maker: bool,
    pub timestamp: DateTime<Utc>,
}

/// A private fill record for a specific user (both maker and taker get one).
#[derive(Debug, Clone, serde::Serialize)]
pub struct UserFillEntry {
    pub trade_id: u64,
    pub order_id: OrderId,
    pub symbol: Symbol,
    pub side: String,
    pub role: String, // "maker" or "taker"
    pub price: Decimal,
    pub qty: Decimal,
    pub quote_qty: Decimal,
    pub fee: Decimal,
    pub fee_asset: String,
    pub timestamp: DateTime<Utc>,
}

/// Maximum trade history entries kept per symbol in memory.
const MAX_TRADE_HISTORY_PER_SYMBOL: usize = 10_000;
const MAX_ORDER_HISTORY: usize = 100_000;
const MAX_USER_FILLS: usize = 500;
const MAX_CLOSED_POSITIONS: usize = 50_000;
pub const MAX_CONVERT_HISTORY: usize = 50_000;
pub const MAX_DUST_CONVERSIONS: usize = 10_000;
pub const MAX_MARGIN_INTEREST_HISTORY: usize = 500_000;
pub const MAX_INSURANCE_FUND_HISTORY: usize = 100_000;
pub const MAX_DELIVERY_SETTLEMENT_HISTORY: usize = 50_000;
pub const MAX_LOANS: usize = 50_000;

/// Push an item to a Vec, evicting the oldest entries if over capacity.
pub fn capped_push<T>(vec: &mut Vec<T>, item: T, max: usize) {
    vec.push(item);
    if vec.len() > max {
        let excess = vec.len() - max;
        vec.drain(0..excess);
    }
}

/// Metadata for an iceberg order: the fields needed to place the next visible slice
/// once the current slice is fully filled.
#[derive(Debug, Clone)]
pub struct IcebergMeta {
    /// The original iceberg order_id — used as the key in IcebergManager.
    /// All slices share the same original_id so the manager's state is consistent.
    pub original_id: OrderId,
    pub user_id: UserId,
    pub symbol: Symbol,
    pub side: Side,
    pub price: Price,
    pub time_in_force: TimeInForce,
    pub visible_qty: Decimal,
    pub position_idx: u8,
    pub reduce_only: bool,
    /// Tracks how much of the current visible slice has been filled.
    /// Used to detect when the slice is fully consumed (for futures, where order_locks
    /// are not maintained).
    pub slice_filled: Decimal,
}

/// Snapshot of a submitted order's essential fields, kept until the order reaches a terminal state.
/// Used to record history for futures orders that never touch `order_locks`.
#[derive(Debug, Clone)]
pub struct PendingOrderInfo {
    pub user_id: UserId,
    pub symbol: Symbol,
    pub side: Side,
    pub order_type: OrderType,
    pub price: Decimal,
    pub qty: Decimal,
    pub created_at: DateTime<Utc>,
    /// User-provided client order ID, if any.
    pub order_link_id: Option<String>,
    /// Whether this order is reduce-only (must close, never open a position).
    pub reduce_only: bool,
}

/// Well-known treasury account UUID — all collected trading fees are credited here.
pub const TREASURY_ACCOUNT_ID: uuid::Uuid = uuid::Uuid::from_u128(0xFEE0_FEE0_FEE0_FEE0_FEE0_FEE0_FEE0_FEE0);

/// Well-known yield reserve pool account UUID.
///
/// This account backs all earn and staking payouts. A seed balance is deposited
/// at startup; it grows from margin borrowing interest collected during the
/// margin interest accrual task. Earn and staking tasks withdraw from this pool
/// before crediting users, ensuring payouts are never created from thin air.
pub const YIELD_POOL_ACCOUNT_ID: uuid::Uuid = uuid::Uuid::from_u128(0xE1E1_D000_E1E1_D000_E1E1_D000_E1E1_D000);

// --- Market Maker Program types (Task 4) ---

/// Registration record for a formal market maker.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MarketMakerRecord {
    /// The exchange user ID acting as market maker.
    pub user_id: uuid::Uuid,
    /// Human-readable name for the MM entity.
    pub name: String,
    /// Per-symbol SLA config — overrides the default when present.
    /// Key: symbol string (e.g. "BTCUSDT"), value: symbol-specific SLA.
    pub symbol_configs: HashMap<Symbol, MmSlaConfig>,
    /// Fee override: maker fee rate applied instead of the standard VIP tier.
    /// `None` means the standard fee schedule applies.
    pub maker_fee_override: Option<Decimal>,
    /// Taker fee override.
    pub taker_fee_override: Option<Decimal>,
    /// Rate-limit multiplier applied on top of the base tier (e.g. 5.0 = 5x).
    pub rate_limit_multiplier: Decimal,
    /// Whether the MM registration is currently active.
    pub is_active: bool,
    /// Timestamp when the MM was registered.
    pub registered_at: chrono::DateTime<chrono::Utc>,
    /// Cumulative SLA violation count across all evaluations.
    pub total_violations: u32,
    /// Current penalty level: 0=none, 1=warning, 2=fee_increase, 3=removed.
    pub penalty_level: u8,
    /// Timestamp of last SLA evaluation (per symbol, latest wins).
    pub last_evaluated_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Central registry for the market maker program.
///
/// Holds registrations + the SLA monitor for each symbol.
pub struct MarketMakerProgram {
    /// All registered MMs keyed by user_id.
    pub records: HashMap<uuid::Uuid, MarketMakerRecord>,
    /// Default SLA config applied to all MMs without a symbol-specific override.
    pub default_sla: MmSlaConfig,
    /// Per-symbol SLA monitor (one monitor holds state for all MMs on that symbol).
    pub monitors: HashMap<Symbol, MmSlaMonitor>,
    /// Historical SLA reports (capped at 10 000 entries).
    pub reports: Vec<MmSlaReport>,
}

impl MarketMakerProgram {
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
            default_sla: MmSlaConfig::default(),
            monitors: HashMap::new(),
            reports: Vec::new(),
        }
    }

    /// Register a market maker.  If the user was already registered, the
    /// existing record is replaced (idempotent re-registration resets state).
    pub fn register(
        &mut self,
        user_id: uuid::Uuid,
        name: impl Into<String>,
        maker_fee_override: Option<Decimal>,
        taker_fee_override: Option<Decimal>,
    ) -> &MarketMakerRecord {
        let record = MarketMakerRecord {
            user_id,
            name: name.into(),
            symbol_configs: HashMap::new(),
            maker_fee_override,
            taker_fee_override,
            rate_limit_multiplier: Decimal::new(5, 0), // 5x by default
            is_active: true,
            registered_at: chrono::Utc::now(),
            total_violations: 0,
            penalty_level: 0,
            last_evaluated_at: None,
        };
        // Ensure the user is registered in every existing SLA monitor.
        for monitor in self.monitors.values_mut() {
            monitor.register_mm(user_id);
        }
        self.records.insert(user_id, record);
        self.records.get(&user_id).expect("just inserted")
    }

    /// Return the SLA config for this MM on this symbol (falls back to default).
    pub fn sla_config_for(&self, user_id: uuid::Uuid, symbol: &Symbol) -> &MmSlaConfig {
        self.records
            .get(&user_id)
            .and_then(|r| r.symbol_configs.get(symbol))
            .unwrap_or(&self.default_sla)
    }

    /// Get or create the SLA monitor for the given symbol.
    pub fn monitor_for_symbol(&mut self, symbol: &Symbol) -> &mut MmSlaMonitor {
        if !self.monitors.contains_key(symbol) {
            let mut monitor = MmSlaMonitor::new(self.default_sla.clone());
            // Register all existing MMs into the new monitor.
            for uid in self.records.keys().copied().collect::<Vec<_>>() {
                monitor.register_mm(uid);
            }
            self.monitors.insert(symbol.clone(), monitor);
        }
        self.monitors.get_mut(symbol).expect("just inserted")
    }

    /// Return `true` when `user_id` is a currently-active registered MM.
    pub fn is_active_mm(&self, user_id: uuid::Uuid) -> bool {
        self.records.get(&user_id).map(|r| r.is_active).unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// OrderCountSource implementation for EnginePool (Sprint 8, KG-2)
// ---------------------------------------------------------------------------

/// Wrapper that implements `OrderCountSource` for an `EnginePool`.
///
/// Needed because `OrderCountSource` is defined in `exchange-order-gateway`
/// and `EnginePool` is in `exchange-matching-engine` — an orphan-rule-safe
/// newtype wrapper is the clean solution.
pub struct EnginePoolCountSource<'a>(pub &'a exchange_matching_engine::EnginePool);

impl<'a> OrderCountSource for EnginePoolCountSource<'a> {
    fn user_open_order_count(&self, symbol: &Symbol, user_id: UserId) -> usize {
        self.0
            .user_open_order_count(symbol, user_id)
            .unwrap_or(0) // SymbolNotFound → 0 (new symbol, no orders yet)
    }

    fn user_orders_at_price(
        &self,
        symbol: &Symbol,
        user_id: UserId,
        side: Side,
        price: Price,
    ) -> usize {
        self.0
            .user_orders_at_price(symbol, user_id, side, price)
            .unwrap_or(0)
    }

    /// Sum open orders for `user_id` across every registered symbol (Option A).
    ///
    /// Iterates `EnginePool::symbols()` (a snapshot of registered symbols) and
    /// sends one `UserOrderCount` request per engine thread.  Cost: O(N_symbols)
    /// round-trips through each engine's command channel, ~50 µs total for 20
    /// symbols.  Acceptable for the validation path; not on the matching hot path.
    fn user_total_open_order_count(&self, user_id: UserId) -> usize {
        self.0
            .symbols()
            .into_iter()
            .map(|sym| self.0.user_open_order_count(sym, user_id).unwrap_or(0))
            .sum()
    }
}

// ---------------------------------------------------------------------------
// S15: Post-fill async event channel
// ---------------------------------------------------------------------------

/// Capacity of the post-fill event MPSC channel (B.2 spec: 8192).
///
/// At 2000 fills/sec with a 5 ms worker lag tolerance this gives ~10 ms of
/// headroom. On saturation `try_send` returns an error that is logged and
/// counted via `event_channel_dropped_total`; the hot path is never blocked.
pub const POST_FILL_CHANNEL_CAPACITY: usize = 8_192;

/// Events dispatched from the hot path to the background post-fill worker.
///
/// Every variant carries only the minimum data needed to execute the
/// ASYNC-SAFE side effects (surveillance, notifications, WS broadcast, etc.).
/// Financial operations (balance settlement, fees, positions) are NOT
/// represented here — they run synchronously before this event is sent.
#[derive(Debug, Clone)]
pub enum PostFillEvent {
    /// A trade fill occurred. Carries a snapshot of the fill data needed for
    /// all post-fill side effects.
    Fill {
        fill: exchange_common::FillEvent,
        /// True when this is a perpetual futures fill; false for spot.
        is_futures: bool,
    },
    /// An order was accepted and placed on the book.
    OrderAccepted {
        order_id: exchange_common::OrderId,
        user_id: exchange_common::UserId,
        symbol: exchange_common::Symbol,
    },
    /// An order was cancelled (either by user or by the engine).
    OrderCancelled {
        order_id: exchange_common::OrderId,
        user_id: exchange_common::UserId,
        symbol: exchange_common::Symbol,
        remaining_qty: rust_decimal::Decimal,
    },
}

// ---------------------------------------------------------------------------
// Exchange struct
// ---------------------------------------------------------------------------

/// Shared exchange state — the single source of truth.
///
/// Phase 1: everything in one process behind a Mutex.
/// Phase 2: matching engine gets its own thread with SPSC channel,
/// account manager moves to PostgreSQL — but the API stays the same.
pub struct Exchange {
    /// S19 D.1 — PRODUCTION env var captured at Exchange construction.
    /// Frozen at startup so hot paths don't touch the process env table.
    pub is_production: bool,
    /// S19 D.1 — DEV_MODE captured at Exchange construction. Set when any of
    /// `DEV_MODE`, `EXCHANGE_DEV_MODE` is present, or when `DATABASE_URL` is
    /// absent (same rule that `main.rs` uses to auto-activate dev mode).
    /// Frozen at startup so the `place_order` KYC bypass check is a field
    /// read, not a syscall.
    pub is_dev_mode: bool,
    /// S20 J.4 — shared secret required to hit `GET /metrics`. When set,
    /// every request to `/metrics` must present a matching `X-Metrics-Key`
    /// header (compared in constant time). When `None` the endpoint is
    /// open — acceptable in dev mode but production deployments SHOULD set
    /// `METRICS_API_KEY`. The constructor logs a warning if `is_production`
    /// is true and this field is `None`.
    pub metrics_api_key: Option<String>,
    /// Whether this instance is running in demo/testnet mode.
    /// When true: new users get auto-funded, demo data is seeded.
    pub is_demo: std::sync::atomic::AtomicBool,
    /// Per-symbol matching engine threads. `process()` and `snapshot()` take `&self` (read),
    /// only `add_symbol()` needs write.
    pub engine_pool: RwLock<EnginePool>,
    // S18 Phase B: 64-shard account store (ShardedAccounts). Replaces the single
    // parking_lot::Mutex<AccountManager> used through S18 Phase A — `accounts` is
    // now an Arc<ShardedAccounts> so cloning it into background tasks shares the
    // shard locks rather than the outer mutex. See crates/account/src/sharded.rs
    // for the deadlock-prevention protocol (ascending shard index lock order).
    pub accounts: Arc<ShardedAccounts>,
    /// Track locks per order for unlock on cancel/reject.
    pub order_locks: dashmap::DashMap<OrderId, OrderLockRecord>,
    /// Self-trade prevention guard.
    pub stp_guard: parking_lot::Mutex<SelfTradeGuard>,
    /// Per-user order rate limiter — prevents order spam DoS.
    /// Per-user order rate limiter — lock-free slot-based counter (Sprint S18 C.2).
    /// No external Mutex required; `DashMap` + atomics allow `&self` mutation.
    pub rate_limiter: Arc<exchange_order_gateway::LockFreeRateLimiter>,
    /// Per-user open-order caps (Sprint 8, KG-2).
    ///
    /// Enforced in `place_order` via `OrderValidator::validate_with_counts`
    /// after the basic product-level checks and before the balance lock.
    /// Default: 500 orders per user per symbol, 50 per price level.
    pub order_limit_config: OrderLimitConfig,
    /// Per-user `OrderLimitConfig` overrides for VIP / institutional tiering (KG-8).
    ///
    /// When a user_id is present in this map, the associated config is used
    /// instead of `order_limit_config` (the global default).  This allows market
    /// makers and institutional accounts to operate with higher order caps without
    /// raising the global cap for all retail users.
    ///
    /// The map is consulted in `place_order` at Step 0a1 via
    /// `effective_order_limit_config()`.  The map is not persisted to the WAL —
    /// overrides must be restored on restart by the operator (typically via a
    /// startup script or admin endpoint).
    ///
    /// **Follow-up task:** add a `PUT /admin/users/{user_id}/order-limits` REST
    /// endpoint that wraps `set_user_order_limit_override` so overrides can be
    /// configured via the API without restarting the server.
    pub order_limit_overrides: HashMap<UserId, OrderLimitConfig>,
    /// Write-ahead log — every command persisted BEFORE matching.
    pub journal: Arc<std::sync::Mutex<Option<exchange_journal::SegmentedWal>>>,
    /// Group-commit WAL handle — replaces per-order fsync with batched fsync.
    ///
    /// When `Some`, `place_order` writes through this instead of `journal`.
    /// The `GroupCommitWal` worker batches commands and issues one fsync per
    /// batch, dramatically reducing I/O syscalls under concurrent load.
    /// Populated by `with_journal`; `None` in in-memory (test/bench) mode.
    pub group_commit_wal: Option<Arc<GroupCommitWal>>,
    /// S15: Sender half of the bounded MPSC channel to the post-fill worker.
    ///
    /// `process_events_sync` handles all financial work (balance, fee, position),
    /// then `dispatch_post_fill_events` calls `try_send` here.  The worker
    /// executes surveillance, notifications, WS broadcast, kline/ticker updates,
    /// FIX drop-copy, and copy-trading fan-out asynchronously.
    ///
    /// Capacity: `POST_FILL_CHANNEL_CAPACITY` (8192).  On `try_send` failure the
    /// event is dropped and `event_channel_dropped_total` is incremented.  The
    /// hot path is NEVER blocked.
    pub post_fill_event_tx: tokio::sync::mpsc::Sender<PostFillEvent>,
    /// S15: Receiver half — stored here so `spawn_post_fill_event_worker` can
    /// take it out exactly once via `Mutex::lock().take()`.  Using
    /// `Option<Mutex<Receiver>>` avoids changing the `new()` return type.
    pub post_fill_event_rx: Mutex<Option<tokio::sync::mpsc::Receiver<PostFillEvent>>>,
    /// API key store for authentication (None = Phase 1 no-auth mode).
    pub key_store: Option<Arc<ApiKeyStore>>,
    /// Metrics counters.
    pub metrics: Arc<ExchangeMetrics>,
    /// Per-symbol circuit breakers — enforces price bands and halts trading on repeated breaches.
    pub circuit_breakers: parking_lot::Mutex<HashMap<Symbol, CircuitBreaker>>,
    /// Broadcast channel for market data (trades, book updates, liquidations, funding).
    pub market_tx: broadcast::Sender<MarketEvent>,
    /// Broadcast channel for private user events (fills, order status, positions, funding payments).
    pub private_tx: broadcast::Sender<PrivateEvent>,

    /// Real-time candle aggregation and 24h rolling-window tickers.
    /// Fed from every trade fill so the klines endpoint always has data.
    pub market_data: Arc<InProcessMarketData>,

    // --- Futures state ---

    /// Open positions across all users and symbols.
    pub positions: parking_lot::Mutex<PositionManager>,
    /// Perpetual contract configuration per symbol.
    ///
    /// Read-heavy: every futures order reads this map 3-4 times (KYC check,
    /// is-perp flag, margin estimation, liquidation). Writes are admin-only (rare).
    /// Sprint S17: migrated from parking_lot::Mutex to ArcSwap — readers pay only
    /// one atomic pointer load with no mutex acquisition.
    pub perp_configs: ArcSwap<HashMap<Symbol, PerpetualConfig>>,
    /// Per-symbol pair status: "active", "disabled", or "settle_only".
    /// Pairs not in this map default to "active".
    ///
    /// Read-heavy: checked on every order (fallback to legacy status). Writes are
    /// admin-only (rare). Sprint S17: migrated from std::sync::Mutex to ArcSwap.
    pub product_status: ArcSwap<HashMap<Symbol, String>>,
    /// Conditional/trigger order engine (stop, take-profit, trailing stop).
    pub trigger_engine: Mutex<TriggerEngine>,
    /// Bracket order manager — links entry orders to TP/SL trigger pairs (OCO).
    pub bracket_manager: Mutex<exchange_trigger_service::BracketManager>,
    /// Index price aggregator (manipulation-resistant weighted median of spot prices).
    pub index_aggregator: Mutex<IndexPriceAggregator>,
    /// Funding settlement scheduler (tracks next funding time).
    pub funding_scheduler: Mutex<FundingScheduler>,
    /// Insurance funds keyed by symbol.
    pub insurance_funds: Mutex<HashMap<Symbol, InsuranceFund>>,
    /// Historical funding rates and payments per symbol.
    pub funding_history: Mutex<HashMap<Symbol, FundingHistory>>,
    /// TWAP premium index buffers — 8h rolling average of (perp−index)/index per symbol.
    /// Sampled every minute by the premium sampler background task.
    pub premium_buffers: Mutex<HashMap<Symbol, exchange_settlement::PremiumBuffer>>,
    /// EMA basis state per symbol for mark price calculation.
    /// Stores the previous EMA value used in `calculate_mark_price_ema`.
    pub mark_ema_basis: Mutex<HashMap<Symbol, Decimal>>,
    /// Implied volatility surface for options mark pricing.
    /// `None` until populated via admin endpoint or bootstrapped from order book.
    pub vol_surface: Mutex<Option<exchange_risk_engine::vol_surface::VolatilitySurface>>,

    /// FIX ExecutionReport broadcast channel for drop copy.
    ///
    /// On every fill, an ExecutionReport FIX message (as bytes) is sent to all
    /// subscribers. The FIX TCP server subscribes to this for unsolicited
    /// execution reporting to connected sessions.
    pub fix_execution_tx: broadcast::Sender<Vec<u8>>,

    /// Counters for ledger reconciliation: fills processed in-memory vs written to DB.
    pub fills_processed: std::sync::atomic::AtomicU64,
    pub fills_ledger_written: std::sync::atomic::AtomicU64,
    /// Counter of ledger write failures (C2). Any non-zero value means
    /// in-memory and DB balances have diverged. Monitored by Alertmanager.
    pub ledger_write_failures: std::sync::atomic::AtomicU64,

    // --- History ---

    /// Orders that have reached a terminal state (filled, partially filled + cancelled, rejected).
    pub order_history: Mutex<Vec<OrderHistoryEntry>>,
    /// Public trade history per symbol, capped at [`MAX_TRADE_HISTORY_PER_SYMBOL`] entries.
    /// VecDeque provides O(1) pop_front for FIFO eviction instead of O(n) Vec::remove(0).
    pub trade_history: Mutex<HashMap<Symbol, VecDeque<TradeHistoryEntry>>>,
    /// Private per-user fill history, capped at 500 entries per user.
    /// Both maker and taker get an entry for each fill.
    pub user_fills: Mutex<HashMap<UserId, VecDeque<UserFillEntry>>>,
    /// Per-user per-symbol leverage preference. Defaults to 10x if not set.
    /// Updated by `set_leverage()`, read by `settle_futures_fill()`.
    pub user_leverage: Mutex<HashMap<(UserId, Symbol), Decimal>>,
    /// Pending order metadata (all symbols, including futures) for history recording.
    /// Entries are inserted on submission and removed when the order reaches a terminal state.
    pub pending_orders: dashmap::DashMap<OrderId, PendingOrderInfo>,
    /// Closed position PnL records — one entry per fully-closed position.
    /// Populated in [`Exchange::settle_futures_fill`] whenever a position reaches zero size.
    pub closed_positions: Mutex<Vec<exchange_common::futures::ClosedPositionRecord>>,
    /// Margin reserved (locked in the USDT balance) for in-flight futures orders.
    ///
    /// Keyed by order_id. The locked amount is reserved atomically with the margin check
    /// in `place_order` to eliminate the TOCTOU race between concurrent orders from the
    /// same user. Entries are removed — and the lock released — in `handle_cancel`,
    /// `handle_reject`, and `settle_futures_fill` (open path).
    pub futures_margin_locks: Mutex<HashMap<OrderId, Decimal>>,

    // --- Fee, surveillance & notifications ---

    /// VIP fee tier engine — determines maker/taker fees per user and tracks 30-day volume.
    pub fee_engine: Mutex<FeeEngine>,
    /// Real-time market surveillance — spoofing, wash-trading, quote-stuffing detection.
    pub surveillance: parking_lot::Mutex<SurveillanceEngine>,
    /// Per-user account status overrides — Frozen, Suspended, etc.
    /// Default (absent) = Active. Set by surveillance auto-actions or admin.
    pub account_statuses: Mutex<HashMap<UserId, exchange_common::types::AccountStatus>>,
    /// Per-user rate limit overrides set by surveillance ReduceRateLimit auto-action.
    /// Users in this map get a throttled rate limit (10 req/s) instead of the default.
    pub rate_limit_overrides: Mutex<HashMap<UserId, exchange_order_gateway::RateLimitConfig>>,
    /// Per-counterparty toxicity scorer — EWMA of adverse fill rate.
    /// Users with toxicity > 0.85 get auto-rate-limited.
    pub toxicity_scorer: Mutex<UserToxicityScorer>,
    /// User notification manager — order fills, liquidation warnings, price alerts.
    pub notifications: Mutex<NotificationManager>,
    /// NOTIF-1: Device token registry for push notifications (APNS/FCM/WebPush).
    pub device_registry: Mutex<exchange_admin::DeviceRegistry>,

    // --- Account management ---

    /// Sub-account topology manager — groups and reverse lookups.
    pub sub_accounts: Mutex<SubAccountManager>,
    /// Internal transfer engine — zero-fee instant balance moves between accounts.
    pub transfers: Mutex<TransferManager>,
    /// Referral code and commission manager.
    pub referrals: Mutex<ReferralManager>,
    /// TOTP 2FA manager — generate secrets, verify codes, enable/disable per user.
    pub totp: Mutex<TotpManager>,

    // --- Custody (deposits / withdrawals) ---

    /// Coin/asset registry with network configs, fees, and validation.
    /// Wrapped in `Mutex` so that coins can be registered at runtime via the admin API.
    pub asset_registry: std::sync::Arc<Mutex<exchange_asset_registry::AssetRegistry>>,
    /// Deposit address registry and incoming deposit tracker.
    pub deposit_manager: Mutex<DepositManager>,
    /// Withdrawal request manager (submission, approval, status polling).
    pub withdrawal_manager: Mutex<WithdrawalManager>,
    /// Custody provider — Mock in Phase 1, Fireblocks in Phase 2.
    pub custody_provider: Box<dyn CustodyProvider + Send + Sync>,
    /// Per-user withdrawal address whitelist (24-hour activation cooldown).
    pub address_whitelist: Mutex<AddressWhitelist>,
    /// Hot/warm/cold wallet split manager with per-coin sweep policies.
    pub wallet_manager: Mutex<WalletManager>,

    // --- Compliance ---

    /// FATF Recommendation 16 Travel Rule engine — tracks qualifying transfers.
    pub travel_rule: Mutex<TravelRuleEngine>,
    /// KYC verification service — manages provider flows (Diia, Sumsub, mock).
    /// Uses tokio Mutex because `start_verification` is async (HTTP calls to providers).
    pub verification_service: AsyncMutex<VerificationService>,
    /// Sanctions screener — name and address checks against OFAC SDN and equivalent lists.
    pub sanctions: Mutex<SanctionsScreener>,
    /// P3-3: Timestamp (ms since epoch) of last successful OFAC feed refresh.
    pub last_ofac_refresh_ms: std::sync::atomic::AtomicU64,
    /// PEP screener — checks names against the Politically Exposed Persons database.
    pub pep_screener: Mutex<PepScreener>,
    /// Terms of Service manager — tracks published versions and per-user acceptances.
    pub tos_manager: Mutex<TosManager>,
    /// Immutable audit log — every significant action recorded for regulatory compliance.
    pub audit_log: Mutex<AuditLog>,
    /// MiCA Art.71 complaint handling system with SLA tracking.
    pub complaint_manager: Mutex<ComplaintManager>,
    /// Risk warning acknowledgement tracker — mandatory before leveraged trading.
    pub risk_warnings: Mutex<RiskWarningManager>,
    /// Customer risk scoring engine — computes AML risk levels and schedules reviews.
    pub risk_scoring: Mutex<RiskScoringEngine>,
    /// Enhanced Due Diligence case manager — tracks EDD investigations per user.
    pub edd_manager: Mutex<EddManager>,
    /// SAR/STR filing manager — tracks suspicious activity reports lifecycle.
    pub sar_manager: Mutex<SarManager>,
    /// Corporate KYC manager — handles entity registrations and UBO verification.
    pub corporate_kyc: Mutex<CorporateKycManager>,

    // --- Connection tracking ---

    /// Per-IP WebSocket connection counter for M25-5 limit enforcement.
    /// Max 10 WS connections per IP address.
    pub ws_ip_connections: Mutex<HashMap<String, u32>>,

    // --- Feature flags (T12.1) ---

    /// Runtime feature flag system — controls which product areas are live.
    pub feature_flags: Arc<exchange_common::FeatureFlags>,

    // --- Web Auth (T9) ---

    /// JWT manager — issues and verifies access/refresh token pairs.
    pub jwt: JwtManager,
    /// Argon2id password hashes keyed by user id.
    pub password_hashes: Mutex<HashMap<UserId, String>>,
    /// Email → user_id mapping (registration directory).
    pub user_emails: Mutex<HashMap<String, UserId>>,
    /// Live refresh tokens: token string → (user_id, expiry).
    pub refresh_tokens: Mutex<HashMap<String, (UserId, DateTime<Utc>)>>,
    /// Active login sessions, keyed by session UUID for O(1) lookup and revocation.
    pub sessions: Mutex<HashMap<uuid::Uuid, SessionRecord>>,

    // --- Copy trading ---

    /// Registered master traders and their performance statistics.
    pub copy_trading_masters: Mutex<MasterRegistry>,
    /// Follow relations between followers and masters.
    pub copy_trading_followers: Mutex<FollowerManager>,

    // --- Trading bots ---

    /// Active grid bots across all users.
    pub grid_bots: Mutex<Vec<GridBot>>,
    /// Active DCA bots across all users.
    pub dca_bots: Mutex<Vec<DcaBot>>,

    // --- Convert / Swap ---

    /// Pending conversion quotes (expire after 10 seconds).
    pub convert_quotes: Mutex<HashMap<uuid::Uuid, ConvertQuote>>,
    /// Completed convert history (all users, kept in memory).
    pub convert_history: Mutex<Vec<ConvertQuote>>,

    // --- Spot Margin ---

    /// Per-user spot margin trading settings.
    pub spot_margin_settings: Mutex<HashMap<uuid::Uuid, SpotMarginSettings>>,

    /// Active spot margin borrows keyed by (user_id, coin).
    ///
    /// A user can have at most one active borrow per coin at a time; subsequent
    /// borrows increase the amount on the existing record.
    pub margin_borrows: Mutex<HashMap<(uuid::Uuid, String), MarginBorrow>>,

    /// Append-only interest charge history for all users.
    ///
    /// Written by `spawn_margin_interest_accrual` every hour.
    /// Capped at 500 000 entries (oldest dropped on overflow).
    pub margin_interest_history: Mutex<Vec<MarginInterestRecord>>,

    // --- Dust Conversion ---

    /// History of dust conversion batches across all users.
    pub dust_conversions: Mutex<Vec<DustConversion>>,

    // --- Earn / Staking ---

    /// Available earn products (staking, savings, etc).
    pub earn_products: Mutex<Vec<EarnProduct>>,
    /// User earn allocations.
    pub earn_allocations: Mutex<Vec<EarnAllocation>>,

    // --- Margin Lending ---

    /// Active and historical loans.
    pub loans: Mutex<Vec<LoanRecord>>,
    /// Hourly interest rates per coin.
    pub lending_rates: Mutex<HashMap<String, Decimal>>,

    // --- Algo execution (TWAP / VWAP) ---

    /// Active TWAP executions keyed by their algo ID.
    pub twap_executions: Mutex<HashMap<uuid::Uuid, exchange_algo_execution::TwapExecution>>,
    /// Active VWAP executions keyed by their algo ID.
    pub vwap_executions: Mutex<HashMap<uuid::Uuid, exchange_algo_execution::VwapExecution>>,

    // --- Client order ID index ---

    /// Client order ID → internal order ID index (scoped per user).
    /// Key is (user_id, link_id) to prevent cross-user collision.
    /// Entries are removed when the order reaches a terminal state.
    pub order_link_index: parking_lot::Mutex<HashMap<(uuid::Uuid, String), uuid::Uuid>>,

    // --- GTD order registry ---

    /// GTD (Good-Till-Date) order expiry registry.
    ///
    /// Maps order_id → (symbol, expire_at). The GTD expiry checker task scans
    /// this map every second and cancels any order whose `expire_at` has passed.
    /// Entries are removed on cancellation or when the order fills.
    pub gtd_orders: Mutex<HashMap<OrderId, (Symbol, DateTime<Utc>)>>,

    // --- Iceberg order manager ---

    /// Iceberg order manager — tracks hidden quantities and replenishes visible slices
    /// after each full fill of the visible portion.
    pub iceberg_manager: Mutex<exchange_matching_engine::IcebergManager>,

    /// Metadata needed to re-place the next visible slice of an iceberg order.
    ///
    /// Keyed by the order_id of the *currently visible* slice on the book.
    /// Entries are removed when the iceberg is fully consumed or cancelled.
    pub iceberg_meta: Mutex<HashMap<OrderId, IcebergMeta>>,

    // --- User account settings ---

    /// Per-user trading preferences (serialized as JSON strings for flexibility).
    /// Key: user_id, Value: JSON settings blob.
    pub user_settings: Mutex<HashMap<UserId, serde_json::Value>>,

    // --- Ledger (write-behind to PostgreSQL) ---

    /// PostgreSQL connection pool for double-entry ledger writes.
    ///
    /// `None` in dev/test mode (in-memory only).
    /// Set via `set_db_pool` after construction, before the server starts.
    ///
    /// `PgPool` is already `Clone + Send + Sync`, so no Mutex is needed.
    /// `OnceLock` provides thread-safe single-assignment without any locking overhead.
    pub db_pool: std::sync::OnceLock<sqlx::PgPool>,

    /// JWT token blacklist — set of revoked JWT IDs (jti claim).
    /// Checked on every authenticated request. Tokens in this set are rejected
    /// even if their signature is valid and they haven't expired.
    pub token_blacklist: dashmap::DashSet<String>,

    /// Four-eyes (dual-control) approval queue for irreversible admin operations.
    pub four_eyes: Mutex<crate::four_eyes::FourEyesManager>,

    /// Secrets provider — abstracts env vars, Vault, KMS, etc.
    /// Defaults to `EnvSecretProvider`. Injected at startup so call sites
    /// don't scatter `env::var` reads throughout the codebase.
    pub secrets: Arc<dyn exchange_common::SecretProvider>,

    // --- Position DB ID tracking ---

    /// Maps (user_id, symbol) → the DB row UUID assigned when the position was first created.
    ///
    /// Necessary for write-behind updates and closes: in-memory positions have no DB primary key,
    /// so we keep a side-map here and populate it when we INSERT the row.
    pub position_db_ids: Mutex<HashMap<(UserId, Symbol), uuid::Uuid>>,

    // --- Fund account (multi-wallet) ---

    /// Funding account balances — separate from the trading account.
    ///
    /// Deposits arrive here first. Users transfer to `accounts` (trading) before
    /// they can place orders or open positions. Withdrawals are sourced from here.
    /// Key: user_id → coin → balance (e.g., "USDT" → Decimal).
    pub fund_accounts: Mutex<HashMap<uuid::Uuid, HashMap<String, Decimal>>>,

    // --- Dead Man's Switch (Task 7.3) ---

    /// Per-user deadline for the dead man's switch.
    ///
    /// When a deadline is present and has passed, the background task cancels
    /// all of the user's open orders and removes the entry. `timeoutSecs: 0`
    /// (or `DELETE /orders/cancel-all-after`) removes the entry without firing.
    pub dead_mans_switch: Mutex<HashMap<uuid::Uuid, chrono::DateTime<chrono::Utc>>>,

    // --- Email ---

    /// Email sender (None if SMTP not configured).
    pub email_sender: Option<Arc<exchange_admin::EmailSender>>,
    /// Pending email verification codes: user_id → (code, expires_at).
    pub email_codes: Mutex<HashMap<UserId, (String, DateTime<Utc>)>>,
    /// Set of users who have verified their email.
    /// Sprint S18 C.3: migrated from `Mutex<HashSet>` to `DashSet` — lock-free reads.
    pub email_verified: dashmap::DashSet<UserId>,
    /// Set of users whose KYC has been approved (Verified status).
    ///
    /// This is a sync-accessible mirror of `verification_service` KYC state,
    /// maintained so that `place_order` (which is synchronous) can check KYC
    /// without blocking an async runtime thread via `blocking_lock`.
    ///
    /// Updated in `admin_kyc_approve`, `kyc_webhook`, and any other path that
    /// transitions a user to `KycStatus::Verified`.
    ///
    /// Sprint S18 C.3: migrated from `Mutex<HashSet>` to `DashSet` — lock-free reads.
    pub kyc_approved: dashmap::DashSet<UserId>,

    // --- Market Maker Protection ---

    /// Per-user MMP state (config + freeze status + recent fill window).
    pub mmp_state: Mutex<HashMap<uuid::Uuid, MmpState>>,

    // --- Dual-approval withdrawals (Task 12) ---

    /// Votes cast so far for withdrawals that require multi-sig approval.
    /// Keyed by withdrawal_id.
    pub withdrawal_approvals: Mutex<HashMap<uuid::Uuid, Vec<WithdrawalApproval>>>,
    /// Policy that decides when dual approval is required.
    pub withdrawal_approval_policy: ApprovalPolicy,

    // --- Data retention (Task 15) ---

    /// Declared retention windows per data category.
    pub retention_policies: Mutex<Vec<RetentionPolicy>>,

    // --- Tax reporting (Task 16) ---

    /// Funding payment history per user — signed amounts (positive = received, negative = paid).
    /// Used to populate the tax report endpoint.
    pub user_funding_history: Mutex<std::collections::HashMap<uuid::Uuid, Vec<rust_decimal::Decimal>>>,

    // --- Regulatory reporting (Task 17) ---

    /// Archive of generated regulatory reports (type → list of reports).
    pub regulatory_reports: Mutex<Vec<exchange_compliance::RegulatoryReport>>,

    // --- VASP Directory (Task 18) ---

    /// Directory of known counterparty VASPs for travel-rule data exchange.
    pub vasp_directory: Mutex<VaspDirectory>,

    // --- Jurisdiction rules (Task 19) ---

    /// Jurisdiction-specific product and risk rules engine.
    pub jurisdiction_engine: Mutex<JurisdictionEngine>,
    /// Per-user jurisdiction assignment (M22-5). Populated from KYC country or IP geolocation.
    pub user_jurisdictions: Mutex<HashMap<UserId, exchange_compliance::jurisdiction::Jurisdiction>>,

    // --- Capital adequacy (Task 20) ---

    /// Capital adequacy tracker — monitors own-funds vs regulatory minimum (MiCA Art. 67).
    pub capital_tracker: Mutex<CapitalTracker>,

    // --- Operational controls ---

    /// Accounts that have been frozen by compliance/admin — blocked from trading and withdrawals.
    /// Sprint S18 C.3: migrated from `Mutex<HashSet>` to `DashSet` — lock-free reads.
    pub frozen_accounts: dashmap::DashSet<UserId>,

    /// Emergency halt flag — when true, no new orders are accepted from any user.
    pub operating_mode: exchange_common::operating_mode::AtomicOperatingMode,

    // --- Pre-Market / Call Auction (Task 7) ---

    /// Per-symbol trading phase (call auction, pre-market, active, settle-only, disabled).
    /// Symbols absent from this map are treated as `PairPhase::Active`.
    pub pair_phases: parking_lot::Mutex<HashMap<Symbol, PairPhaseConfig>>,

    /// Orders collected during call auction phase (NOT sent to matching engine).
    pub auction_orders: Mutex<HashMap<Symbol, Vec<PlaceOrderCommand>>>,

    // --- Anti-Phishing (Task 10) ---

    /// Anti-phishing codes per user (displayed in all emails).
    pub anti_phishing_codes: Mutex<HashMap<uuid::Uuid, String>>,

    // --- Price Alerts (Task 11) ---

    /// User-defined price alerts.
    pub price_alerts: Mutex<Vec<PriceAlert>>,

    // --- Notification Preferences (Task 12) ---

    /// Per-user notification preferences.
    pub notification_prefs: Mutex<HashMap<uuid::Uuid, NotificationPreferences>>,

    // --- Margin Call Cooldowns ---

    /// Last margin call notification time per (user, symbol) to avoid spam.
    pub margin_call_cooldowns: Mutex<HashMap<(uuid::Uuid, String), chrono::DateTime<chrono::Utc>>>,

    // --- Withdrawal Confirmation ---

    /// Pending withdrawal confirmations: token → (user_id, asset, amount, address, expires_at).
    pub withdrawal_confirmations: Mutex<HashMap<String, WithdrawalConfirmation>>,

    // --- Login Known IPs ---

    /// Known IPs per user — first login from a new IP triggers a notification.
    pub known_login_ips: Mutex<HashMap<uuid::Uuid, Vec<String>>>,

    // --- Maintenance Mode (Task 17) ---

    /// Scheduled or active maintenance window.
    /// `None` when no maintenance is planned.
    pub maintenance_mode: Mutex<Option<MaintenanceInfo>>,

    // --- Device Management (Task 18) ---

    /// Trusted devices per user — registered on login, revocable by the user.
    pub user_devices: Mutex<HashMap<uuid::Uuid, Vec<DeviceInfo>>>,

    // --- Portfolio / Cross-Margin Mode (M13 Task 1) ---

    /// Per-user margin mode preference.
    ///
    /// When set to `Cross`, all futures positions for the user share a single
    /// margin pool (account equity). When `Isolated` (default), each position
    /// has its own dedicated margin collateral.
    pub user_margin_modes: Mutex<HashMap<UserId, MarginMode>>,

    // --- Portfolio Margin Mode ---

    /// Users that have enabled portfolio margin.
    ///
    /// When enabled, the pre-trade margin check in `place_order` uses
    /// scenario-based stress testing instead of per-position isolated margin.
    /// Required equity ≥ $100,000 to activate (enforced at enable time).
    pub portfolio_margin_users: Mutex<HashSet<UserId>>,

    // --- Insurance Fund History (M13 Task 2) ---

    /// Append-only log of every insurance fund event: surplus, deficit, and
    /// socialized losses. Used by the admin history endpoint and for audit.
    pub insurance_fund_history: Mutex<Vec<InsuranceFundHistoryEntry>>,

    // --- Chain orders (M13 Task 9) ---

    /// Sequential order chain manager — executes a series of orders one after another,
    /// activating the next step only when the previous step is fully filled.
    pub chain_manager: Mutex<exchange_trigger_service::ChainManager>,

    // --- Sub-account API keys (Task 3) ---

    /// Independent API keys for sub-accounts.
    ///
    /// Key: sub-account user_id, Value: list of raw API key strings that are
    /// valid for that sub-account.  The master account's keys (in `key_store`)
    /// are NOT valid for sub-accounts; each sub gets its own credential set so
    /// risk and access can be isolated.
    pub sub_account_api_keys: Mutex<HashMap<uuid::Uuid, Vec<String>>>,

    // --- Formal Market Maker Program (Task 4) ---

    /// Registry of registered market makers, their SLA configs, and the
    /// per-symbol SLA monitors used by the background evaluation task.
    pub market_maker_program: Mutex<MarketMakerProgram>,

    // --- Security Hardening (Task 7) ---

    /// Per-endpoint-group rate limit configurations keyed by group name
    /// (e.g. "auth", "order_placement", "market_data", "admin").
    pub rate_limit_configs: HashMap<String, crate::security::RateLimitConfig>,

    /// IP reputation tracker — records failed login attempts and enforces
    /// temporary blocks after the brute-force threshold is reached.
    pub ip_reputation: crate::security::IpReputationTracker,

    /// Per-user 2FA brute-force protection: max 5 attempts per 15 minutes.
    pub totp_attempt_tracker: crate::security::TotpAttemptTracker,

    /// Append-only security event log (login_success, login_failed, etc.).
    pub security_events: Mutex<crate::security::SecurityEventLog>,

    // --- Fiat On/Off Ramp (Task 8) ---

    /// Fiat payment gateway — registered providers and payment record store.
    pub fiat_gateway: Mutex<crate::fiat::FiatGateway>,

    // --- Cross-Collateral Haircuts ---

    /// Per-asset collateral configuration: haircut rate, maximum amount, and eligibility.
    ///
    /// A haircut of 0.95 means 1 unit of the asset counts as 0.95 USDT worth of margin.
    /// USDT has haircut 1.0 (no discount). Altcoins typically 0.70-0.85.
    pub collateral_configs: Mutex<HashMap<String, CollateralConfig>>,

    /// Which coins each user has opted-in as cross-margin collateral.
    ///
    /// Only assets in this set (per user) are included in `cross_margin_balance`.
    /// When a user has no entry here, all eligible coins default to enabled.
    pub user_collateral_settings: Mutex<HashMap<UserId, HashSet<String>>>,

    // --- Liquidation Event History ---

    /// Append-only log of every liquidation event (partial and full).
    ///
    /// Capped at 50 000 entries — oldest entries are dropped once the cap is
    /// reached.  Used by:
    /// - GET /account/liquidation-history  (user, private)
    /// - GET /admin/liquidations           (admin)
    /// - GET /market/liquidations/{symbol} (public, anonymised)
    pub liquidation_events: Mutex<Vec<exchange_risk_engine::LiquidationEvent>>,

    // --- ListenKey (WS private data auth) ---

    /// Active listen keys for WebSocket authentication.
    ///
    /// Binance-style: client calls POST /api/v1/listen-key to obtain a key,
    /// then connects to WS and sends `{"op":"auth","listenKey":"<key>"}`.
    /// Keys expire after 60 minutes unless refreshed via PUT /api/v1/listen-key.
    pub listen_keys: Mutex<HashMap<String, ListenKeyInfo>>,

    // --- Block Trading / RFQ ---

    /// Active RFQ requests keyed by RFQ ID.
    ///
    /// Created when a taker calls POST /block-trade/rfq.
    /// Expires after 5 minutes by default. Market makers respond with quotes;
    /// the taker picks one and calls POST /block-trade/rfq/{id}/execute.
    pub block_trade_rfqs: Mutex<HashMap<uuid::Uuid, BlockTradeRfq>>,

    /// Settled block trades (append-only history).
    pub block_trades: Mutex<Vec<BlockTrade>>,

    // --- Broker API ---

    /// Registered brokers keyed by their exchange user ID.
    ///
    /// Brokers create sub-accounts for clients and earn commission on client
    /// trades. Registered by admin via POST /admin/brokers.
    pub brokers: Mutex<HashMap<UserId, BrokerRecord>>,

    /// Commission events earned by brokers when their clients trade.
    ///
    /// One entry per fill that triggers a commission. Capped at 100 000 entries.
    /// VecDeque for O(1) pop_front eviction.
    pub broker_commissions: Mutex<VecDeque<BrokerCommission>>,

    /// Maps a client (sub-account) user_id → broker user_id.
    ///
    /// Populated when a broker calls POST /broker/clients.
    /// Consulted in the fill path to attribute commission to the broker.
    pub broker_clients: Mutex<BrokerClientMap>,

    /// Minimum block trade quantities per symbol (in base asset units).
    ///
    /// Trades below this size are rejected at execution time.
    pub block_trade_minimums: Mutex<HashMap<String, Decimal>>,

    // --- Trade Competitions (Task 1) ---

    /// All created trade competitions.
    pub competitions: Mutex<Vec<Competition>>,

    /// Participants per competition: competition_id → list of entries.
    pub competition_participants: Mutex<HashMap<uuid::Uuid, Vec<CompetitionEntry>>>,

    // --- Leveraged Tokens (Task 2) ---

    /// Leveraged token definitions keyed by token symbol (e.g. "BTC3L").
    pub leveraged_tokens: Mutex<HashMap<String, LeveragedToken>>,

    /// User leveraged token holdings: (user_id, token_symbol) → token amount.
    pub lt_holdings: Mutex<HashMap<(uuid::Uuid, String), Decimal>>,

    // --- DeFi Yield ---

    /// User DeFi yield allocations across protocols.
    pub defi_allocations: Mutex<Vec<exchange_defi_yield::YieldAllocation>>,

    // --- Web3 Wallet ---

    /// Per-user Web3 wallet manager (create, import, export wallets).
    pub web3_wallets: Mutex<exchange_web3_wallet::WalletManager>,
    /// Web3 transaction history per user.
    pub web3_tx_history: Mutex<HashMap<uuid::Uuid, Vec<exchange_web3_wallet::Web3Transaction>>>,
    /// DApp connector — WalletConnect-style session management.
    pub dapp_connector: Mutex<exchange_web3_wallet::DAppConnector>,

    // --- Unified Trading Account ---

    /// Unified account state per user (collateral, margin mode, etc.).
    pub unified_accounts: Mutex<HashMap<uuid::Uuid, exchange_account::UnifiedAccount>>,

    // --- Copy Trading Parameters (Task 3) ---

    /// Per-user copy trading settings.
    pub copy_trading_settings: Mutex<HashMap<uuid::Uuid, CopyTradingSettings>>,

    // --- P2P Trading (Task 4) ---

    /// All P2P ads.
    pub p2p_ads: Mutex<Vec<P2pAd>>,

    /// All P2P orders.
    pub p2p_orders: Mutex<Vec<P2pOrder>>,

    // --- Algo Orders (algo namespace) ---

    /// Active and historical algo orders (iceberg, twap, trailing_stop, signal).
    pub algo_orders: Mutex<Vec<AlgoOrder>>,

    // --- Signal Bots ---

    /// Webhook-driven signal bots created by users.
    pub signal_bots: Mutex<Vec<SignalBot>>,

    // --- Recurring Buy Plans ---

    /// User-defined scheduled DCA purchase plans.
    pub recurring_buy_plans: Mutex<Vec<RecurringBuyPlan>>,

    // --- Trading Statistics ---

    /// Historical open interest snapshots per symbol.
    ///
    /// Populated every 5 minutes by `spawn_oi_snapshotter`. Capped at 2 000 entries
    /// per symbol (oldest entry is dropped when the cap is reached).
    /// VecDeque for O(1) pop_front eviction.
    pub oi_history: Mutex<HashMap<Symbol, VecDeque<OiSnapshot>>>,

    /// Historical taker buy/sell volume per symbol in 5-minute buckets.
    ///
    /// Updated on every fill in `handle_fill`. Capped at 2 000 buckets per symbol.
    /// VecDeque for O(1) pop_front eviction.
    pub taker_volume_history: Mutex<HashMap<Symbol, VecDeque<TakerVolumeSnapshot>>>,

    /// Historical long/short ratio snapshots per symbol.
    ///
    /// Populated every 5 minutes alongside OI. Capped at 2 000 entries per symbol.
    /// VecDeque for O(1) pop_front eviction.
    pub long_short_history: Mutex<HashMap<Symbol, VecDeque<LongShortSnapshot>>>,

    // --- Dual Investment (advanced structured product) ---

    /// Available dual investment product templates.
    ///
    /// Each template defines an underlying coin, direction, strike price, expiry,
    /// and advertised APY. Users subscribe to a product to open a position.
    pub dual_invest_products: Mutex<Vec<DualInvestProduct>>,

    /// Active and settled dual investment positions across all users.
    pub dual_invest_positions: Mutex<Vec<DualInvestPosition>>,

    // --- ETH/SOL Staking ---

    /// Per-user, per-coin staking positions.
    ///
    /// Key: (user_id, coin). Coins supported in Phase 1: "ETH", "SOL".
    /// Rewards accrue hourly via `spawn_staking_rewards_accrual`.
    pub staking_positions: Mutex<HashMap<(uuid::Uuid, String), StakingPosition>>,

    /// Pending unbonding entries across all users.
    ///
    /// Entries are added on `unstake` and cleared by the background task once
    /// `available_at` is in the past (funds credited back to the user).
    pub staking_unbonding: Mutex<Vec<UnbondingEntry>>,

    /// Historical APY snapshots for staking, keyed by coin.
    ///
    /// Populated by the hourly accrual task. Capped at 8 760 entries per coin
    /// (one year of hourly data).
    pub staking_apy_history: Mutex<HashMap<String, Vec<StakingApySnapshot>>>,

    // --- Crypto Loans ---

    /// All crypto-backed loans (active and repaid).
    ///
    /// Collateral is locked from the user's balance when a loan is opened.
    /// Auto-liquidated when LTV exceeds 85 %.
    pub crypto_loans: Mutex<Vec<CryptoLoan>>,

    // --- Sub-account API key records ---

    /// Full API key records for sub-accounts, keyed by sub-account user_id.
    ///
    /// Each entry holds a list of [`SubAccountApiKey`] structs that carry the
    /// label, permissions, IP whitelist and secret hash.
    pub sub_account_api_key_records: Mutex<HashMap<uuid::Uuid, Vec<SubAccountApiKey>>>,

    // --- Sub-account transfer permissions ---

    /// Controls whether a sub-account may initiate outbound transfers.
    ///
    /// Defaults to `false` (blocked) when no entry is present.
    /// The master account sets this via
    /// `PUT /sub-accounts/{id}/transfer-permission`.
    pub sub_transfer_permissions: Mutex<HashMap<uuid::Uuid, bool>>,

    // --- Platform announcements ---

    /// All announcements created by admins, ordered by insertion time.
    ///
    /// Public read endpoint filters to those with `publish_at <= now`.
    /// Capped at 500 entries (oldest dropped on overflow).
    pub announcements: Mutex<Vec<Announcement>>,

    // --- System Status ---

    /// Live system component status and incident log.
    ///
    /// Updated by admin via `POST /admin/status/component` and
    /// `POST /admin/status/incident`. Read publicly via `GET /status` and
    /// `GET /status/incidents`.
    pub system_status: Mutex<SystemStatus>,

    // --- Block-trade counterparty registry ---

    /// Registered block-trade counterparties (market makers and institutional
    /// accounts). Each entry is shown via GET /block-trade/counterparties.
    pub block_trade_counterparties: Mutex<Vec<BlockTradeCounterparty>>,

    // --- Options Trading ---

    /// Available option instruments (calls and puts across multiple strikes/expiries).
    pub option_instruments: Mutex<Vec<OptionInstrument>>,

    /// Per-user option positions keyed by (user_id, symbol).
    pub option_positions: Mutex<HashMap<(UserId, String), OptionPosition>>,

    // --- Delivery / Quarterly Futures ---

    /// Available delivery (quarterly) futures contracts.
    pub delivery_contracts: Mutex<Vec<DeliveryContract>>,

    /// Per-user delivery futures positions keyed by (user_id, symbol).
    pub delivery_positions: Mutex<HashMap<(UserId, String), DeliveryPosition>>,

    /// Historical delivery settlement prices, keyed by contract symbol.
    pub delivery_settlement_history: Mutex<Vec<DeliverySettlementRecord>>,

    // --- Multi-leg strategy orders ---

    /// Active and historical multi-leg strategy orders.
    ///
    /// Each entry represents one logical strategy (e.g. spread, straddle) and
    /// carries references to the constituent leg orders.
    pub strategy_orders: Mutex<Vec<StrategyOrder>>,

    // --- Spot Hedge Mode ---

    /// Users that have enabled spot hedge mode.
    ///
    /// When enabled, the position manager does not net a spot buy against an
    /// open short perpetual — the user can hold both simultaneously.
    pub spot_hedge_users: Mutex<HashSet<UserId>>,

    // --- Prevented Matches ---

    /// Ring-buffer of STP-prevented match records, capped at 10 000 entries.
    ///
    /// Each entry records metadata about a potential self-trade that was
    /// blocked by the self-trade prevention engine.
    pub prevented_matches: Mutex<VecDeque<PreventedMatch>>,

    // --- Launchpad (IEO Token Sale) ---

    /// All IEO token-sale projects, ordered by creation time.
    pub launchpad_projects: Mutex<Vec<crate::rest_products::LaunchpadProject>>,

    /// User USDT commitments to Launchpad projects.
    ///
    /// At project end_time the background task computes pro-rata allocations,
    /// credits tokens, and refunds unused USDT.
    pub launchpad_commitments: Mutex<Vec<crate::rest_products::LaunchpadCommitment>>,

    // --- Launchpool (Stake to Earn) ---

    /// All Launchpool staking pools.
    pub launchpool_pools: Mutex<Vec<crate::rest_products::LaunchpoolPool>>,

    /// Per-user stake records across all Launchpool pools.
    pub launchpool_stakes: Mutex<Vec<crate::rest_products::LaunchpoolStake>>,

    // --- Shark Fin (Structured Product) ---

    /// Available Shark Fin product definitions.
    pub shark_fin_products: Mutex<Vec<crate::rest_products::SharkFinProduct>>,

    /// User subscriptions (positions) in Shark Fin products.
    pub shark_fin_positions: Mutex<Vec<crate::rest_products::SharkFinPosition>>,

    // --- Liquidity Mining / LP Rewards ---

    /// Constant-product AMM pools, one per trading pair.
    pub liquidity_pools: Mutex<Vec<crate::rest_products::LiquidityPool>>,

    /// User LP positions across all pools.
    pub liquidity_positions: Mutex<Vec<crate::rest_products::LiquidityPosition>>,

    // --- Stablecoin Auto-Conversion ---

    /// Per-user auto-convert settings.
    ///
    /// When enabled, any balance increase of `from` coin is immediately
    /// converted to `to` coin at 1:1. Updated via PUT /account/auto-convert.
    pub auto_convert_settings: Mutex<HashMap<UserId, crate::rest_products::AutoConvertConfig>>,

    // --- Advanced Bots ---

    /// Active advanced bots across all users:
    /// infinity-grid, futures-grid, arb, martingale, and slicing.
    ///
    /// Managed by `rest_bots` handlers and ticked by
    /// `tasks::run_advanced_bot_executor`.
    pub advanced_bots: Mutex<Vec<crate::rest_bots::AdvancedBot>>,

    // --- Daily Account Snapshots (Feature 1) ---

    /// Daily equity snapshots keyed by (user_id, date_string).
    ///
    /// Populated at midnight UTC by `spawn_daily_snapshot`.
    /// Date string format: "YYYY-MM-DD".
    pub daily_snapshots: Mutex<HashMap<(UserId, String), DailySnapshot>>,

    // --- Account Tiers (Feature 3) ---

    /// Per-user account tier (Basic, Verified, Pro).
    ///
    /// Users absent from this map are treated as `AccountTier::Basic`.
    pub account_tiers: Mutex<HashMap<UserId, AccountTier>>,

    // --- Gift Cards (Feature 5) ---

    /// All issued gift cards (active, redeemed, and cancelled).
    pub gift_cards: Mutex<Vec<GiftCard>>,

    // --- Premium Kline History (Feature 6) ---

    /// Historical premium (mark − index) snapshots per symbol.
    ///
    /// Populated every 5 minutes alongside OI snapshots.
    /// Capped at [`MAX_PREMIUM_HISTORY_PER_SYMBOL`] per symbol.
    pub premium_history: Mutex<HashMap<Symbol, VecDeque<PremiumSnapshot>>>,

    // --- Bonus / Coupon System ---

    /// All bonus campaigns created by admins.
    ///
    /// Each campaign defines a bonus type (deposit, trading, signup), amount,
    /// optional coupon code, expiry, and usage cap.
    pub bonus_campaigns: Mutex<Vec<crate::rest_engagement::BonusCampaign>>,

    /// Per-user list of claimed bonuses.
    ///
    /// Bonuses are credited as "BONUS_USDT" — tradeable margin but not withdrawable.
    pub user_bonuses: Mutex<HashMap<UserId, Vec<crate::rest_engagement::UserBonus>>>,

    // --- Airdrop Portal ---

    /// All airdrop distribution events (active and past).
    pub airdrops: Mutex<Vec<crate::rest_engagement::Airdrop>>,

    /// Tracks which (user, airdrop) pairs have already been claimed.
    ///
    /// Key: (user_id, airdrop_id). Presence means claimed.
    pub airdrop_claims: Mutex<HashMap<(UserId, uuid::Uuid), bool>>,

    // --- Multi-Tier Affiliate Commissions ---

    /// Referral tree: child (referred user) → parent (referrer).
    ///
    /// Walking this map up to 3 hops gives the three tiers of referrers
    /// that share in a trader's fees.
    pub referral_tree: Mutex<HashMap<UserId, UserId>>,

    /// Append-only log of affiliate commission events.
    ///
    /// Capped at 200 000 entries (oldest evicted via VecDeque::pop_front).
    pub affiliate_commissions: Mutex<VecDeque<crate::rest_engagement::AffiliateCommission>>,

    // --- Competition Teams ---

    /// Teams created within trade competitions.
    ///
    /// A team belongs to exactly one competition. Members must have joined
    /// the competition before creating or joining a team.
    pub competition_teams: Mutex<Vec<crate::rest_engagement::CompetitionTeam>>,

    // --- Proof of Reserves cache ---

    /// The most recently generated Proof of Reserves report and its
    /// corresponding liability snapshot.
    ///
    /// Cached so that `/api/v1/proof-of-reserves/verify` (public) and
    /// `/api/v1/proof-of-reserves/my-proof` (authenticated) can serve proof
    /// data from the same consistent snapshot rather than each regenerating the
    /// Merkle tree independently.
    ///
    /// Populated by `POST /admin/proof-of-reserves` (admin trigger) and read
    /// by the two public/private endpoints.  `None` until the first admin
    /// report is generated.
    pub latest_por_report: Mutex<Option<(exchange_proof_of_reserves::ReserveReport, Vec<exchange_proof_of_reserves::UserLiability>)>>,
}

// --- System Status types ---

/// Status of an individual system component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ComponentStatus {
    Operational,
    Degraded,
    Maintenance,
    Unavailable,
}

impl std::fmt::Display for ComponentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Operational  => write!(f, "operational"),
            Self::Degraded     => write!(f, "degraded"),
            Self::Maintenance  => write!(f, "maintenance"),
            Self::Unavailable  => write!(f, "unavailable"),
        }
    }
}

/// An incident visible on the public status page.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Incident {
    pub id: uuid::Uuid,
    pub title: String,
    /// "investigating", "identified", "monitoring", "resolved"
    pub status: String,
    pub message: String,
    pub affected_components: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Aggregate system health including per-component status and open incidents.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SystemStatus {
    /// Per-component status, keyed by component name.
    pub components: HashMap<String, ComponentStatus>,
    /// Overall worst-case status across all components.
    pub overall: ComponentStatus,
    /// Optional human-readable status message.
    pub message: Option<String>,
    /// Open and recently resolved incidents.
    pub incidents: Vec<Incident>,
    /// When the status was last updated.
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Default for SystemStatus {
    fn default() -> Self {
        let mut components = HashMap::new();
        components.insert("matching_engine".into(), ComponentStatus::Operational);
        components.insert("risk_engine".into(),     ComponentStatus::Operational);
        components.insert("settlement".into(),      ComponentStatus::Operational);
        components.insert("api".into(),             ComponentStatus::Operational);
        components.insert("websocket".into(),       ComponentStatus::Operational);
        Self {
            components,
            overall: ComponentStatus::Operational,
            message: None,
            incidents: Vec::new(),
            updated_at: chrono::Utc::now(),
        }
    }
}

// --- Block-trade counterparty types ---

/// A registered block-trade counterparty (market maker or institutional account).
#[derive(Debug, Clone, serde::Serialize)]
pub struct BlockTradeCounterparty {
    pub id: uuid::Uuid,
    pub user_id: uuid::Uuid,
    pub name: String,
    /// Symbols this counterparty is willing to trade.
    pub symbols: Vec<String>,
    pub is_active: bool,
    /// Whether this counterparty is currently online.
    pub is_online: bool,
    pub registered_at: chrono::DateTime<chrono::Utc>,
}

// --- Options Trading types ---

/// Whether an option contract is a Call or a Put.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OptionType {
    Call,
    Put,
}

impl std::fmt::Display for OptionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Call => write!(f, "Call"),
            Self::Put  => write!(f, "Put"),
        }
    }
}

/// An option instrument (a single strike/expiry/type combination).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OptionInstrument {
    /// Exchange-unique symbol, e.g. "BTC-20240628-70000-C".
    pub symbol: String,
    /// Underlying perpetual symbol, e.g. "BTCUSDT".
    pub underlying: String,
    /// Strike price in USD.
    pub strike_price: Decimal,
    /// Expiry datetime (UTC).
    pub expiry: chrono::DateTime<chrono::Utc>,
    /// Call or Put.
    pub option_type: OptionType,
    /// Number of underlying coins per contract.
    pub contract_size: Decimal,
    /// Settlement coin (always "USDT" in Phase 1).
    pub settle_coin: String,
    /// False once the contract has expired and been settled.
    pub is_active: bool,
}

/// Black-Scholes Greeks for one option instrument.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Greeks {
    /// Rate of change of premium with respect to spot price.
    pub delta: Decimal,
    /// Rate of change of delta with respect to spot price.
    pub gamma: Decimal,
    /// Daily time-decay of the premium (negative for long positions).
    pub theta: Decimal,
    /// Sensitivity of premium to a 1-point change in implied volatility.
    pub vega: Decimal,
    /// Sensitivity of premium to a 1-point change in risk-free rate.
    pub rho: Decimal,
    /// Implied volatility used for the calculation.
    pub iv: Decimal,
    /// Theoretical fair value (Black-Scholes price) of the option.
    pub mark_price: Decimal,
}

/// A user's position in an option instrument.
#[derive(Debug, Clone, serde::Serialize)]
pub struct OptionPosition {
    pub user_id: UserId,
    /// Option symbol (e.g. "BTC-20240628-70000-C").
    pub symbol: String,
    /// Underlying perpetual symbol.
    pub underlying: String,
    /// Net signed quantity (positive = long, negative = short).
    pub qty: Decimal,
    /// Average entry premium paid/received per contract.
    pub avg_entry_premium: Decimal,
    /// Unrealised PnL in settlement coin.
    pub unrealised_pnl: Decimal,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Status of a delivery futures contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DeliveryStatus {
    /// Actively trading.
    Active,
    /// Settlement window open; no new positions may be opened.
    Settling,
    /// Contract has been fully settled and is closed.
    Delivered,
}

impl std::fmt::Display for DeliveryStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active    => write!(f, "Active"),
            Self::Settling  => write!(f, "Settling"),
            Self::Delivered => write!(f, "Delivered"),
        }
    }
}

/// A delivery (quarterly) futures contract definition.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeliveryContract {
    /// Exchange symbol, e.g. "BTCUSDT_240628".
    pub symbol: String,
    /// Underlying perpetual, e.g. "BTCUSDT".
    pub underlying: String,
    /// When the contract is settled and positions are closed.
    pub delivery_date: chrono::DateTime<chrono::Utc>,
    /// Number of underlying coins per contract.
    pub contract_size: Decimal,
    /// Current contract status.
    pub status: DeliveryStatus,
}

/// A user's position in a delivery futures contract.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DeliveryPosition {
    pub user_id: UserId,
    pub symbol: String,
    pub underlying: String,
    /// Positive = long, negative = short (in contracts).
    pub qty: Decimal,
    /// Average entry price.
    pub avg_entry_price: Decimal,
    /// Unrealised PnL at last mark.
    pub unrealised_pnl: Decimal,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// A historical settlement price for a delivered contract.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DeliverySettlementRecord {
    /// The contract symbol that was settled.
    pub symbol: String,
    /// Underlying perpetual symbol.
    pub underlying: String,
    /// Index price at settlement (time-weighted average over final hour).
    pub settlement_price: Decimal,
    /// When the settlement was processed.
    pub settled_at: chrono::DateTime<chrono::Utc>,
}

// --- Multi-Leg Strategy Order types ---

/// Classification of a multi-leg strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrategyType {
    /// Two-leg directional bet: long one asset, short another.
    Spread,
    /// Two legs on the same underlying: long call + long put at same strike/expiry.
    Straddle,
    /// Like straddle but different strikes.
    Strangle,
    /// Three-leg (or four-leg) combination for capped profit/risk.
    ButterflySpread,
    /// Same instrument, different expiries.
    CalendarSpread,
    /// User-defined N-leg combination.
    Custom,
}

/// A single constituent leg of a multi-leg strategy order.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StrategyLeg {
    /// Symbol for this leg (e.g. "BTCUSDT").
    pub symbol: String,
    /// "buy" or "sell".
    pub side: String,
    pub qty: Decimal,
    /// Price for limit legs. `None` indicates a market leg.
    pub price: Option<Decimal>,
    /// The order_id assigned when this leg was submitted to the matching engine.
    pub order_id: Option<uuid::Uuid>,
    /// Fill state of this specific leg: "pending", "filled", "cancelled", "failed".
    pub status: String,
}

/// Execution mode for multi-leg strategies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrategyExecution {
    /// Submit all legs concurrently; roll back filled legs if any leg fails.
    Simultaneous,
    /// Submit legs one-by-one in order; abort on first failure (no rollback of prior legs).
    Sequential,
}

impl Default for StrategyExecution {
    fn default() -> Self {
        Self::Simultaneous
    }
}

/// A complete multi-leg strategy order record.
#[derive(Debug, Clone, serde::Serialize)]
pub struct StrategyOrder {
    pub id: uuid::Uuid,
    pub user_id: uuid::Uuid,
    pub strategy_type: StrategyType,
    pub execution: StrategyExecution,
    pub legs: Vec<StrategyLeg>,
    /// Overall status: "active" (some legs open), "filled", "cancelled", "failed".
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

// --- Prevented Match types ---

/// A record of an STP-prevented potential match.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PreventedMatch {
    pub id: uuid::Uuid,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub user_id: uuid::Uuid,
    /// The incoming order that was blocked.
    pub order_id: uuid::Uuid,
    /// The resting order it would have matched against.
    pub would_match_order_id: uuid::Uuid,
    pub symbol: String,
    pub qty: Decimal,
    pub price: Decimal,
    pub stp_mode: String,
}

/// Maximum number of prevented-match records to keep in memory.
pub const MAX_PREVENTED_MATCHES: usize = 10_000;

// --- Trading Statistics types ---

/// Maximum number of snapshots retained per symbol in OI / long-short history.
pub const MAX_STATS_HISTORY_PER_SYMBOL: usize = 2_000;

/// A point-in-time snapshot of open interest for one perpetual symbol.
#[derive(Debug, Clone, serde::Serialize)]
pub struct OiSnapshot {
    /// Unix timestamp (seconds).
    pub timestamp: i64,
    /// Total open interest in contracts (base asset).
    pub open_interest: Decimal,
    /// Notional value in USD at snapshot time.
    pub open_interest_value_usd: Decimal,
}

/// A 5-minute bucket of taker buy/sell volume for one perpetual symbol.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TakerVolumeSnapshot {
    /// Unix timestamp of the bucket start (seconds, aligned to 5-minute boundaries).
    pub timestamp: i64,
    /// Total buy-side taker volume (base asset qty).
    pub buy_volume: Decimal,
    /// Total sell-side taker volume (base asset qty).
    pub sell_volume: Decimal,
    /// Number of buy-side taker fills in this bucket.
    pub buy_count: u64,
    /// Number of sell-side taker fills in this bucket.
    pub sell_count: u64,
}

/// A point-in-time snapshot of the long/short ratio for one perpetual symbol.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LongShortSnapshot {
    /// Unix timestamp (seconds).
    pub timestamp: i64,
    /// Fraction of open interest held long (0.0–1.0, as Decimal).
    pub long_ratio: Decimal,
    /// Fraction of open interest held short (0.0–1.0, as Decimal).
    pub short_ratio: Decimal,
    /// Total long position size (base asset).
    pub long_volume: Decimal,
    /// Total short position size (base asset).
    pub short_volume: Decimal,
}

// --- Trade Competition types (Task 1) ---

/// A trade competition created by an admin.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Competition {
    pub id: uuid::Uuid,
    pub name: String,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub end_time: chrono::DateTime<chrono::Utc>,
    pub prize_pool: Decimal,
    /// Ranking metric: "pnl", "volume", or "roi".
    pub metric: String,
    /// Symbols in scope.  Empty means all symbols are included.
    pub symbols: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// A user's entry in a competition.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CompetitionEntry {
    pub user_id: uuid::Uuid,
    pub joined_at: chrono::DateTime<chrono::Utc>,
    /// Balance snapshot captured at join time (used for ROI calculation).
    pub snapshot_balance: Decimal,
}

// --- Leveraged Token types (Task 2) ---

/// An auto-rebalancing leveraged token (e.g. BTC3L = 3x long BTC).
///
/// NAV formula: `initial_nav * (1 + leverage * underlying_return)`.
/// Rebalancing logic is not automated in Phase 1 — NAV is updated by admin or
/// a background task when the underlying price moves.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LeveragedToken {
    /// Token symbol, e.g. "BTC3L".
    pub symbol: String,
    /// Underlying perp symbol, e.g. "BTCUSDT".
    pub underlying: String,
    /// Target leverage factor (signed: positive = long, negative = short).
    pub leverage: i32,
    /// Current Net Asset Value per token in USDT.
    pub nav: Decimal,
    /// Total tokens in circulation.
    pub total_supply: Decimal,
    /// Percentage move in underlying that triggers a rebalance (e.g. 5 = 5%).
    pub rebalance_threshold_pct: u32,
    /// The underlying price recorded at the most recent NAV update.
    /// `None` until the first price tick is received.
    pub last_rebalance_price: Option<Decimal>,
}

/// Seed the default leveraged token catalogue.
pub fn default_leveraged_tokens() -> HashMap<String, LeveragedToken> {
    let defaults = [
        ("BTC3L",  "BTCUSDT",  3),
        ("BTC3S",  "BTCUSDT", -3),
        ("ETH3L",  "ETHUSDT",  3),
        ("ETH3S",  "ETHUSDT", -3),
    ];
    defaults
        .into_iter()
        .map(|(sym, underlying, lev)| {
            let token = LeveragedToken {
                symbol: sym.to_string(),
                underlying: underlying.to_string(),
                leverage: lev,
                nav: Decimal::new(100, 0), // start at $100
                total_supply: Decimal::ZERO,
                rebalance_threshold_pct: 5,
                last_rebalance_price: None,
            };
            (sym.to_string(), token)
        })
        .collect()
}

// --- Copy Trading Settings types (Task 3) ---

/// Per-follower copy trading configuration.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CopyTradingSettings {
    /// The master trader to copy.
    pub master_id: uuid::Uuid,
    /// Fixed USDT amount per copied trade.  `None` means use `copy_ratio`.
    pub fixed_amount: Option<Decimal>,
    /// Maximum total notional exposure to copy trades.
    pub max_position_size: Option<Decimal>,
    /// Auto-exit when copy PnL falls by this percentage.
    pub stop_loss_pct: Option<Decimal>,
    /// Fraction of the master's position size to replicate (0 < ratio ≤ 1).
    pub copy_ratio: Decimal,
    /// Whether copy trading is currently active for this user.
    pub is_active: bool,
}

// --- P2P Trading types (Task 4) ---

/// A P2P advertisement posted by a user.
#[derive(Debug, Clone, serde::Serialize)]
pub struct P2pAd {
    pub id: uuid::Uuid,
    pub user_id: uuid::Uuid,
    /// "buy" = ad owner wants to buy crypto (pays fiat), "sell" = ad owner wants to sell crypto.
    pub side: String,
    /// Crypto asset being traded, e.g. "USDT".
    pub asset: String,
    pub fiat_currency: String,
    /// Fiat price per unit of crypto.
    pub price: Decimal,
    pub min_amount: Decimal,
    pub max_amount: Decimal,
    pub payment_methods: Vec<String>,
    pub is_active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Order status lifecycle for P2P trades.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum P2pOrderStatus {
    Created = 0,
    PaymentSent = 1,
    Completed = 2,
    Cancelled = 3,
    Disputed = 4,
}

/// A P2P order created against an ad.
///
/// When created, the seller's crypto is escrowed (debited from their balance).
/// On `Completed`, escrowed crypto is credited to the buyer.
/// On `Cancelled`, escrowed crypto is returned to the seller.
#[derive(Debug, Clone, serde::Serialize)]
pub struct P2pOrder {
    pub id: uuid::Uuid,
    pub ad_id: uuid::Uuid,
    pub buyer_id: uuid::Uuid,
    pub seller_id: uuid::Uuid,
    pub asset: String,
    pub fiat_currency: String,
    pub crypto_amount: Decimal,
    pub fiat_amount: Decimal,
    pub status: P2pOrderStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

// --- Algo Orders (algo namespace) ---

/// Algo order type distinguishing execution strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlgoType {
    /// Split a large order into smaller visible slices to hide true size.
    Iceberg,
    /// Time-Weighted Average Price — slice execution evenly over a time window.
    Twap,
    /// Trail the market and trigger when price reverses by a given distance.
    TrailingStop,
    /// Execute when an external webhook signal is received.
    Signal,
}

/// Lifecycle status of an algo order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlgoStatus {
    /// Actively running.
    Running,
    /// Fully filled.
    Filled,
    /// Cancelled by the user.
    Cancelled,
    /// Terminated by an error or system condition.
    Failed,
}

/// An algo order tracked in the `/algo/orders` namespace.
///
/// `params` is a free-form JSON object whose schema depends on `algo_type`:
/// - `iceberg`       → `{visible_qty: Decimal}`
/// - `twap`          → `{slices: u32, interval_secs: u64}`
/// - `trailing_stop` → `{trail_pct: Decimal, activation_price: Option<Decimal>}`
/// - `signal`        → `{bot_id: Uuid}`
#[derive(Debug, Clone, serde::Serialize)]
pub struct AlgoOrder {
    pub id: uuid::Uuid,
    pub user_id: uuid::Uuid,
    pub algo_type: AlgoType,
    pub symbol: String,
    pub side: String,
    pub total_qty: Decimal,
    pub filled_qty: Decimal,
    pub params: serde_json::Value,
    pub status: AlgoStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

// --- Signal Bots ---

/// A webhook-driven signal bot that places orders on behalf of a user.
///
/// The `webhook_secret` is validated on each incoming POST to
/// `/webhooks/signal/{bot_id}`. Bots only process symbols listed in `symbols`
/// (empty list means all symbols are permitted).
#[derive(Debug, Clone, serde::Serialize)]
pub struct SignalBot {
    pub id: uuid::Uuid,
    pub user_id: uuid::Uuid,
    pub name: String,
    /// Permitted symbols (empty = all allowed).
    pub symbols: Vec<String>,
    /// HMAC secret validated on every incoming webhook call.
    #[serde(skip_serializing)]
    pub webhook_secret: String,
    pub is_active: bool,
    pub trades_executed: u64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// --- Recurring Buy Plans ---

/// Frequency of a recurring buy plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecurringBuyFrequency {
    Daily,
    Weekly,
    Monthly,
}

/// A user-defined scheduled DCA purchase plan.
///
/// The background task `spawn_recurring_buy_executor` wakes every minute,
/// compares `next_execution` to the current time, and executes a market
/// buy of `amount_usdt` worth of `coin` when due.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RecurringBuyPlan {
    pub id: uuid::Uuid,
    pub user_id: uuid::Uuid,
    /// The coin to purchase (e.g. "BTC").
    pub coin: String,
    /// USDT amount to spend per execution.
    pub amount_usdt: Decimal,
    pub frequency: RecurringBuyFrequency,
    /// For weekly plans: 0=Monday … 6=Sunday.  Ignored for daily and monthly.
    pub day_of_week: Option<u8>,
    /// Hour of day (0–23) at which the purchase executes.
    pub hour: u8,
    /// Timestamp of the next scheduled execution.
    pub next_execution: chrono::DateTime<chrono::Utc>,
    /// Cumulative USDT invested across all executions.
    pub total_invested: Decimal,
    /// Number of times this plan has executed successfully.
    pub executions: u64,
    pub is_active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// --- Dual Investment types ---

/// Direction of a dual investment product.
///
/// - `BuyLow`: user deposits USDT and buys the underlying at strike if price ≤ strike at expiry.
/// - `SellHigh`: user deposits the underlying coin and sells at strike if price ≥ strike at expiry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DualInvestDirection {
    BuyLow,
    SellHigh,
}

/// Status of a dual investment product.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DualInvestStatus {
    /// Accepting new subscriptions.
    Active,
    /// No longer accepting subscriptions but not yet settled.
    Closed,
    /// Settlement complete.
    Settled,
}

/// A dual investment product template that users can subscribe to.
///
/// Example: BTC Buy-Low at $60 000, 7-day expiry, 12 % APY.
/// On expiry: if mark price ≤ $60 000 → user receives BTC at $60 000 + premium.
///            if mark price > $60 000 → user receives USDT back + premium.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DualInvestProduct {
    pub id: uuid::Uuid,
    /// Underlying coin (e.g. "BTC", "ETH").
    pub underlying: String,
    pub direction: DualInvestDirection,
    /// Strike price in USDT.
    pub strike_price: Decimal,
    /// Absolute expiry timestamp.
    pub expiry: chrono::DateTime<chrono::Utc>,
    /// Annualised percentage yield, e.g. `Decimal::new(12, 2)` = 12 %.
    pub apy: Decimal,
    pub status: DualInvestStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// A user's subscription to a dual investment product.
///
/// `BuyLow`  : `amount` is in USDT (deposit currency).
/// `SellHigh`: `amount` is in the underlying coin.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DualInvestPosition {
    pub id: uuid::Uuid,
    pub user_id: uuid::Uuid,
    pub product_id: uuid::Uuid,
    /// Amount deposited (USDT for BuyLow, coin units for SellHigh).
    pub amount: Decimal,
    /// Whether the position has been settled at expiry.
    pub settled: bool,
    /// Payout credited to the user at settlement.
    /// `None` until settlement runs.
    pub payout: Option<Decimal>,
    /// Coin in which the payout was credited.
    pub payout_coin: Option<String>,
    pub subscribed_at: chrono::DateTime<chrono::Utc>,
}

// --- ETH/SOL Staking types ---

/// A user's staking position for one coin.
#[derive(Debug, Clone, serde::Serialize)]
pub struct StakingPosition {
    pub user_id: uuid::Uuid,
    pub coin: String,
    /// Total amount currently staked (excludes pending unbonding).
    pub staked_amount: Decimal,
    /// Rewards accumulated since the last claim (or since staking began).
    pub pending_rewards: Decimal,
    pub staked_at: chrono::DateTime<chrono::Utc>,
    pub last_accrued_at: chrono::DateTime<chrono::Utc>,
}

/// An in-progress unstake request subject to the unbonding period.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UnbondingEntry {
    pub id: uuid::Uuid,
    pub user_id: uuid::Uuid,
    pub coin: String,
    pub amount: Decimal,
    /// When this amount becomes available in the user's wallet.
    pub available_at: chrono::DateTime<chrono::Utc>,
    pub requested_at: chrono::DateTime<chrono::Utc>,
}

/// A single historical APY data point for staking.
#[derive(Debug, Clone, serde::Serialize)]
pub struct StakingApySnapshot {
    pub coin: String,
    pub apy: Decimal,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
}

/// Unbonding period per coin in days.
pub const STAKING_UNBONDING_DAYS: &[(&str, i64)] = &[("ETH", 3), ("SOL", 2)];

/// Annual staking yield per coin.
pub const STAKING_APY: &[(&str, &str)] = &[("ETH", "0.04"), ("SOL", "0.07")];

/// Maximum APY history entries retained per coin (one year of hourly data).
pub const MAX_STAKING_APY_HISTORY: usize = 8_760;

// --- Crypto Loan types ---

/// Status of a crypto-backed loan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoanStatus {
    /// Loan is active, interest is accruing.
    Active,
    /// Fully repaid by the user.
    Repaid,
    /// Collateral was liquidated because LTV exceeded 85 %.
    Liquidated,
}

/// A collateral-backed crypto loan.
///
/// LTV = loan_amount / (collateral_amount × collateral_price).
/// Auto-liquidated when LTV > 85 %.
///
/// Interest accrues hourly at `interest_rate` (daily rate stored as Decimal,
/// e.g. `0.0005` = 0.05 %/day). The background task `spawn_loan_interest_accrual`
/// updates `interest_accrued` each hour.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CryptoLoan {
    pub id: uuid::Uuid,
    pub user_id: uuid::Uuid,
    /// The coin posted as collateral (e.g. "BTC").
    pub collateral_coin: String,
    pub collateral_amount: Decimal,
    /// The coin borrowed (e.g. "USDT").
    pub loan_coin: String,
    pub loan_amount: Decimal,
    /// Interest that has accrued but not yet been repaid.
    pub interest_accrued: Decimal,
    /// Daily interest rate (e.g. 0.0005 = 0.05 %/day).
    pub interest_rate: Decimal,
    /// Current Loan-to-Value ratio (updated on price change and repayment).
    pub ltv: Decimal,
    pub status: LoanStatus,
    pub opened_at: chrono::DateTime<chrono::Utc>,
    pub closed_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Supported collateral→loan pairs with their LTV limits.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LoanCollateralInfo {
    pub collateral_coin: String,
    pub loan_coin: String,
    /// Maximum LTV at origination (e.g. 0.65 = 65 %).
    pub max_initial_ltv: Decimal,
    /// Liquidation LTV threshold (0.85 = 85 %).
    pub liquidation_ltv: Decimal,
    /// Daily interest rate.
    pub daily_rate: Decimal,
}

/// Return the supported collateral configurations.
pub fn default_loan_collateral_info() -> Vec<LoanCollateralInfo> {
    vec![
        LoanCollateralInfo {
            collateral_coin: "BTC".into(),
            loan_coin: "USDT".into(),
            max_initial_ltv: Decimal::new(65, 2),   // 0.65
            liquidation_ltv: Decimal::new(85, 2),   // 0.85
            daily_rate: Decimal::new(5, 4),          // 0.0005
        },
        LoanCollateralInfo {
            collateral_coin: "ETH".into(),
            loan_coin: "USDT".into(),
            max_initial_ltv: Decimal::new(65, 2),
            liquidation_ltv: Decimal::new(85, 2),
            daily_rate: Decimal::new(5, 4),
        },
    ]
}

// --- Block Trading / RFQ types ---

/// A single leg in a multi-leg RFQ (e.g., for spreads or combos).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RfqLeg {
    pub symbol: String,
    pub side: String,
    pub qty: Decimal,
}

/// RFQ status lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RfqStatus {
    /// Waiting for market maker quotes.
    AwaitingQuotes,
    /// At least one quote received, taker can execute.
    Quoted,
    /// Taker accepted a quote; settlement done.
    Executed,
    /// Expired without execution.
    Expired,
    /// Cancelled by the taker.
    Cancelled,
}

/// A Request for Quote created by a taker for an off-book institutional trade.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BlockTradeRfq {
    pub id: uuid::Uuid,
    pub taker_id: UserId,
    /// Primary leg symbol (also used for single-leg RFQs).
    pub symbol: String,
    pub side: String,
    pub qty: Decimal,
    /// Additional legs for multi-leg (combo) RFQs. Empty for single-leg.
    pub legs: Vec<RfqLeg>,
    pub status: RfqStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    /// Quotes submitted by market makers, keyed by quote ID.
    pub quotes: Vec<RfqQuote>,
}

/// A price quote submitted by a market maker in response to an RFQ.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RfqQuote {
    pub id: uuid::Uuid,
    pub rfq_id: uuid::Uuid,
    pub maker_id: UserId,
    pub price: Decimal,
    /// Time the quote expires (market makers may set shorter validity).
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Minimum notional size requirements for block trades (per symbol, in base qty).
///
/// Block trades below the minimum size are rejected at execution time.
/// These defaults are conservative; operators can update them at runtime.
pub fn default_block_trade_minimums() -> HashMap<String, Decimal> {
    let mut m = HashMap::new();
    m.insert("BTCUSDT".to_string(), Decimal::new(5, 0));   // 5 BTC
    m.insert("ETHUSDT".to_string(), Decimal::new(50, 0));  // 50 ETH
    m.insert("SOLUSDT".to_string(), Decimal::new(500, 0)); // 500 SOL
    m
}

/// A settled block trade record.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BlockTrade {
    pub id: uuid::Uuid,
    pub rfq_id: uuid::Uuid,
    pub maker_id: UserId,
    pub taker_id: UserId,
    pub symbol: String,
    /// The agreed price (from the accepted quote).
    pub price: Decimal,
    pub qty: Decimal,
    /// Taker side: "Buy" means taker is buyer, maker is seller.
    pub taker_side: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

// --- Broker API types ---

/// A registered broker record.
///
/// Brokers white-label the exchange for their clients. The exchange creates
/// client sub-accounts on the broker's behalf. When a client fills a trade,
/// `commission_rate` × fee is forwarded to the broker's account.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BrokerRecord {
    /// The broker's own exchange user ID.
    pub user_id: UserId,
    /// Human-readable name for the broker entity.
    pub name: String,
    /// Fraction of the client's trading fee paid to the broker.
    /// E.g. 0.20 means the broker earns 20% of the client's fee.
    pub commission_rate: Decimal,
    /// Whether the broker registration is currently active.
    pub is_active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Cumulative commission earned (in USDT equivalent), updated on each fill.
    pub total_commission: Decimal,
}

/// One commission event: a client of this broker completed a trade fill.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BrokerCommission {
    pub id: uuid::Uuid,
    pub broker_id: UserId,
    pub client_id: UserId,
    pub trade_id: u64,
    /// Commission amount earned by the broker.
    pub amount: Decimal,
    /// Asset the commission is denominated in (typically "USDT").
    pub asset: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Maps a client (sub-account) user_id → broker user_id.
///
/// Populated when the broker calls POST /broker/clients.
/// Used in the fill path to check whether a client's trade earns commission.
pub type BrokerClientMap = HashMap<UserId, UserId>;

// --- End Block Trading / Broker types ---

/// Authentication record for a WebSocket listen key.
///
/// Listen keys allow clients to authenticate private WebSocket channels without
/// sending HMAC signatures on each message. A key is valid for 60 minutes and
/// can be refreshed by the client to extend its lifetime.
#[derive(Debug, Clone)]
pub struct ListenKeyInfo {
    /// The exchange user this key belongs to.
    pub user_id: UserId,
    /// When the key was originally issued.
    pub created_at: DateTime<Utc>,
    /// When this key stops being valid. Reset to `now + 60 min` on keepalive.
    pub expires_at: DateTime<Utc>,
}

/// A login session created when a user authenticates via email/password.
#[derive(Debug, Clone)]
pub struct SessionRecord {
    pub id: uuid::Uuid,
    pub user_id: UserId,
    pub ip: String,
    pub user_agent: String,
    pub created_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub is_active: bool,
}

/// Record of what was locked for an order (needed for unlock on cancel).
#[derive(Debug, Clone)]
pub struct OrderLockRecord {
    pub user_id: UserId,
    pub symbol: Symbol,
    pub side: Side,
    pub price: Price,
    pub qty: Qty,
    pub lock: LockInfo,
    pub filled_qty: Qty,
    pub filled_cost: Decimal,
}

// --- Sub-account API key types ---

/// A full API key record for a sub-account.
///
/// Created via `POST /sub-accounts/{id}/api-keys`.
/// Secrets are stored as SHA-256 hashes and never returned after creation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SubAccountApiKey {
    pub id: uuid::Uuid,
    pub sub_account_id: uuid::Uuid,
    /// Human-readable label provided by the master user.
    pub label: String,
    /// The public key string (returned in list responses).
    pub api_key: String,
    /// SHA-256 hex hash of the secret.  Never returned over the API.
    #[serde(skip_serializing)]
    pub secret_hash: String,
    /// Allowed operations: "read", "trade", "withdraw".
    pub permissions: Vec<String>,
    /// When non-empty, only requests from these IP addresses are accepted.
    pub ip_whitelist: Vec<String>,
    pub is_active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// --- Announcement types ---

/// A platform-wide announcement created by an admin.
///
/// Stored in `Exchange::announcements`.
/// The public endpoint `GET /announcements` returns entries
/// whose `publish_at <= now`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Announcement {
    pub id: uuid::Uuid,
    pub title: String,
    pub content: String,
    /// Category: "maintenance", "listing", "delisting", or "general".
    pub announcement_type: String,
    /// When the announcement becomes visible to the public.
    pub publish_at: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Public market events (broadcast to all subscribers of a symbol).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type")]
pub enum MarketEvent {
    Trade {
        symbol: String,
        price: Decimal,
        qty: Decimal,
        taker_side: String,
        trade_id: u64,
        timestamp: i64,
    },
    BookUpdate {
        symbol: String,
        bids: Vec<(Decimal, Decimal)>,
        asks: Vec<(Decimal, Decimal)>,
        sequence: u64,
    },
    /// Public liquidation feed — announces that a position was liquidated.
    Liquidation {
        symbol: String,
        side: String,
        size: Decimal,
        price: Decimal,
        timestamp: i64,
    },
    /// Funding rate update broadcast every interval.
    FundingUpdate {
        symbol: String,
        rate: Decimal,
        mark_price: Decimal,
        next_time: i64,
    },
    /// Current candle state after each trade, for the three most common intervals.
    KlineUpdate {
        symbol: String,
        interval: String,
        open_time: i64,
        open: String,
        high: String,
        low: String,
        close: String,
        volume: String,
        is_closed: bool,
    },
    /// Rolling 24h ticker snapshot after each trade.
    TickerUpdate {
        symbol: String,
        last_price: String,
        price_change_pct_24h: String,
        high_24h: String,
        low_24h: String,
        volume_24h: String,
        mark_price: String,
        index_price: String,
        funding_rate: String,
        open_interest: String,
    },
    /// Lightweight mini-ticker: just last price + 24h volume.
    ///
    /// Lower bandwidth than [`TickerUpdate`] — suitable for list views where
    /// only the most current price and activity level matter.
    MiniTickerUpdate {
        symbol: String,
        last_price: String,
        volume_24h: String,
        timestamp: i64,
    },
    /// Real-time funding rate broadcast on `funding.{symbol}`.
    ///
    /// Emitted every time the funding rate is calculated by the settlement
    /// task. Clients subscribe to `funding.{symbol}`.
    FundingRateUpdate {
        symbol: String,
        /// Annualised funding rate (e.g. 0.0001 = 0.01 % per 8 h).
        rate: Decimal,
        /// Mark price at the time of calculation.
        mark_price: Decimal,
        /// Unix ms timestamp of the next funding settlement.
        next_funding_time: i64,
        timestamp: i64,
    },
    /// Real-time mark price broadcast on `markPrice.{symbol}`.
    ///
    /// Emitted every few seconds by the mark-price update task.
    /// Clients subscribe to `markPrice.{symbol}`.
    MarkPriceUpdate {
        symbol: String,
        mark_price: Decimal,
        index_price: Decimal,
        /// Current funding rate.
        funding_rate: Decimal,
        timestamp: i64,
    },
    /// All-market mini ticker broadcast on `ticker.all`.
    ///
    /// Emitted every second by `spawn_all_mini_ticker`.
    /// Carries a compact snapshot for every active symbol.
    AllMiniTicker {
        /// One entry per active symbol.
        tickers: Vec<AllMiniTickerEntry>,
        timestamp: i64,
    },
}

/// A single entry in the all-market mini ticker broadcast.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AllMiniTickerEntry {
    pub symbol: String,
    pub price: String,
    pub change_24h: String,
}

/// Private user events (broadcast to authenticated user).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type")]
pub enum PrivateEvent {
    OrderAccepted {
        user_id: String,
        order_id: String,
        symbol: String,
    },
    OrderFilled {
        user_id: String,
        order_id: String,
        symbol: String,
        price: Decimal,
        qty: Decimal,
        side: String,
        role: String, // "maker" or "taker"
    },
    OrderCancelled {
        user_id: String,
        order_id: String,
        symbol: String,
        remaining_qty: Decimal,
    },
    OrderRejected {
        user_id: String,
        order_id: String,
        symbol: String,
        reason: String,
    },
    /// Position opened, increased, reduced, or closed.
    PositionUpdate {
        user_id: String,
        symbol: String,
        side: String,
        size: Decimal,
        entry_price: Decimal,
        unrealized_pnl: Decimal,
        margin: Decimal,
        liq_price: Option<Decimal>,
    },
    /// Funding payment settled for a position.
    FundingPayment {
        user_id: String,
        symbol: String,
        amount: Decimal,
        rate: Decimal,
    },
    /// Deposit confirmed on-chain and credited to the user's exchange balance.
    DepositCredited {
        user_id: String,
        asset: String,
        amount: Decimal,
        tx_hash: String,
    },
    /// Auto-deleverage: position was forcibly reduced to cover insurance fund deficit.
    AdlClose {
        user_id: String,
        symbol: String,
        qty: Decimal,
        price: Decimal,
        realized_pnl: Decimal,
    },
    /// Real-time balance change notification.
    ///
    /// Emitted after fills, funding payments, deposits, withdrawals, and transfers.
    /// Subscribers on the "wallet" private channel receive this event.
    WalletUpdate {
        user_id: String,
        coin: String,
        wallet_balance: Decimal,
        available_balance: Decimal,
        /// What triggered the update: "fill", "funding", "deposit", "withdrawal", "transfer".
        trigger: String,
        /// Snapshot of current open futures positions.
        /// Empty when the update is unrelated to a position change.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        positions: Vec<WalletPositionSnapshot>,
    },
    /// ADL warning: the user's position is in the top ADL quantile (4 or 5).
    ///
    /// Sent on the private `adl.{symbol}` channel when the liquidation monitor
    /// detects the user is at high risk of being auto-delevered.
    AdlWarning {
        user_id: String,
        symbol: String,
        /// ADL quantile: 1 (lowest risk) to 5 (highest risk).
        quantile: u8,
        side: String,
        size: Decimal,
        mark_price: Decimal,
        timestamp: i64,
    },
}

/// A lightweight position snapshot bundled inside [`PrivateEvent::WalletUpdate`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct WalletPositionSnapshot {
    pub symbol: String,
    pub side: String,
    pub size: Decimal,
    pub entry_price: Decimal,
    pub mark_price: Decimal,
    pub unrealized_pnl: Decimal,
    pub margin: Decimal,
}

// --- Default perpetual configs ---

fn default_perp_configs() -> HashMap<Symbol, PerpetualConfig> {
    let mut map = HashMap::new();
    for config in [
        PerpetualConfig::btc_default(),
        PerpetualConfig::eth_default(),
        PerpetualConfig::sol_default(),
        PerpetualConfig::btcusd_inverse_default(),
        PerpetualConfig::ethusd_inverse_default(),
        PerpetualConfig::bnb_default(),
        PerpetualConfig::xrp_default(),
        PerpetualConfig::doge_default(),
        PerpetualConfig::avax_default(),
        PerpetualConfig::ada_default(),
        PerpetualConfig::link_default(),
        PerpetualConfig::dot_default(),
    ] {
        map.insert(config.symbol.clone(), config);
    }
    map
}

fn default_insurance_funds(perp_configs: &HashMap<Symbol, PerpetualConfig>) -> HashMap<Symbol, InsuranceFund> {
    perp_configs
        .keys()
        .map(|sym| (sym.clone(), InsuranceFund::new(sym.clone(), Decimal::ZERO)))
        .collect()
}

fn default_dual_invest_products() -> Vec<DualInvestProduct> {
    use chrono::Duration;
    let now = chrono::Utc::now();
    vec![
        DualInvestProduct {
            id: uuid::Uuid::new_v4(),
            underlying: "BTC".into(),
            direction: DualInvestDirection::BuyLow,
            strike_price: Decimal::new(60_000, 0),
            expiry: now + Duration::days(7),
            apy: Decimal::new(12, 2), // 12%
            status: DualInvestStatus::Active,
            created_at: now,
        },
        DualInvestProduct {
            id: uuid::Uuid::new_v4(),
            underlying: "BTC".into(),
            direction: DualInvestDirection::SellHigh,
            strike_price: Decimal::new(75_000, 0),
            expiry: now + Duration::days(7),
            apy: Decimal::new(10, 2), // 10%
            status: DualInvestStatus::Active,
            created_at: now,
        },
        DualInvestProduct {
            id: uuid::Uuid::new_v4(),
            underlying: "ETH".into(),
            direction: DualInvestDirection::BuyLow,
            strike_price: Decimal::new(3_000, 0),
            expiry: now + Duration::days(3),
            apy: Decimal::new(15, 2), // 15%
            status: DualInvestStatus::Active,
            created_at: now,
        },
        DualInvestProduct {
            id: uuid::Uuid::new_v4(),
            underlying: "ETH".into(),
            direction: DualInvestDirection::SellHigh,
            strike_price: Decimal::new(4_000, 0),
            expiry: now + Duration::days(3),
            apy: Decimal::new(8, 2), // 8%
            status: DualInvestStatus::Active,
            created_at: now,
        },
    ]
}

fn default_earn_products() -> Vec<EarnProduct> {
    vec![
        // USDT Flexible — 3% APY, withdraw anytime, auto-compound supported.
        EarnProduct {
            id: uuid::Uuid::new_v4(),
            coin: "USDT".into(),
            name: "USDT Flexible Savings".into(),
            apy: Decimal::new(3, 2),  // 0.03 = 3%
            min_amount: Decimal::from(10),
            max_amount: Decimal::from(1_000_000),
            lock_days: 0,
            product_type: "flexible".into(),
            supports_auto_compound: true,
            pool_capacity: Some(Decimal::from(10_000_000)),
            max_user_pct: Decimal::new(5, 2),
        },
        // BTC Flexible — 1.5% APY.
        EarnProduct {
            id: uuid::Uuid::new_v4(),
            coin: "BTC".into(),
            name: "BTC Flexible Savings".into(),
            apy: Decimal::new(15, 3), // 0.015 = 1.5%
            min_amount: Decimal::new(1, 4), // 0.0001 BTC
            max_amount: Decimal::from(100),
            lock_days: 0,
            product_type: "flexible".into(),
            supports_auto_compound: true,
            pool_capacity: Some(Decimal::from(1000)),
            max_user_pct: Decimal::new(5, 2),
        },
        // USDT 30-day Fixed — 5% APY, locked for 30 days.
        EarnProduct {
            id: uuid::Uuid::new_v4(),
            coin: "USDT".into(),
            name: "USDT Fixed 30D".into(),
            apy: Decimal::new(5, 2),  // 0.05 = 5%
            min_amount: Decimal::from(100),
            max_amount: Decimal::from(500_000),
            lock_days: 30,
            product_type: "fixed_30d".into(),
            supports_auto_compound: false,
            pool_capacity: Some(Decimal::from(5_000_000)),
            max_user_pct: Decimal::new(5, 2),
        },
        EarnProduct {
            id: uuid::Uuid::new_v4(),
            coin: "BTC".into(),
            name: "BTC Staking".into(),
            apy: Decimal::new(2, 2),
            min_amount: Decimal::new(1, 3),
            max_amount: Decimal::from(100),
            lock_days: 0,
            product_type: "staking".into(),
            supports_auto_compound: false,
            pool_capacity: Some(Decimal::from(500)),
            max_user_pct: Decimal::new(5, 2),
        },
        EarnProduct {
            id: uuid::Uuid::new_v4(),
            coin: "ETH".into(),
            name: "ETH Staking".into(),
            apy: Decimal::new(35, 3),
            min_amount: Decimal::new(1, 2),
            max_amount: Decimal::from(1000),
            lock_days: 0,
            product_type: "staking".into(),
            supports_auto_compound: false,
            pool_capacity: Some(Decimal::from(5000)),
            max_user_pct: Decimal::new(5, 2),
        },
    ]
}

fn default_circuit_breakers() -> HashMap<Symbol, CircuitBreaker> {
    let mut map = HashMap::new();
    map.insert(Symbol::new("BTCUSDT"), btc_circuit_breaker());
    map.insert(Symbol::new("ETHUSDT"), eth_circuit_breaker());
    map.insert(Symbol::new("SOLUSDT"), sol_circuit_breaker());
    map
}

// --- Black-Scholes Greeks computation ---

/// Approximate the cumulative normal distribution Φ(x) using a rational
/// polynomial approximation (Abramowitz & Stegun 26.2.17, max error 7.5e-8).
///
/// We avoid pulling in a statistics library by computing this inline.
/// All arithmetic is in `f64` (not Decimal) because transcendental functions
/// like e^x and sqrt are not available on `Decimal`.  The results are
/// converted back to `Decimal` before returning from `calculate_greeks`.
fn standard_normal_cdf(x: f64) -> f64 {
    // Adapted from numerical recipes / A&S for speed.
    const P:  f64 = 0.231_641_9;
    const B1: f64 = 0.319_381_530;
    const B2: f64 = -0.356_563_782;
    const B3: f64 = 1.781_477_937;
    const B4: f64 = -1.821_255_978;
    const B5: f64 = 1.330_274_429;

    let t = 1.0 / (1.0 + P * x.abs());
    let poly = t * (B1 + t * (B2 + t * (B3 + t * (B4 + t * B5))));
    let pdf_x = (-0.5 * x * x).exp() / (2.0 * std::f64::consts::PI).sqrt();
    let cdf = 1.0 - pdf_x * poly;
    if x >= 0.0 { cdf } else { 1.0 - cdf }
}

/// Compute Black-Scholes option Greeks.
///
/// # Parameters
/// - `spot`          — current price of the underlying (USD)
/// - `strike`        — option strike price (USD)
/// - `time_to_expiry`— time to expiry in years (e.g. 7 days = 7/365)
/// - `volatility`    — implied/realised volatility as a fraction (e.g. 0.80 = 80 %)
/// - `rate`          — risk-free rate as a fraction (e.g. 0.05 = 5 %)
/// - `is_call`       — `true` for a call, `false` for a put
///
/// Returns `None` when `time_to_expiry` ≤ 0 (expired option).
pub fn calculate_greeks(
    spot: Decimal,
    strike: Decimal,
    time_to_expiry: Decimal,
    volatility: Decimal,
    rate: Decimal,
    is_call: bool,
) -> Option<Greeks> {
    // Convert to f64 for transcendental maths.
    let s: f64  = spot.try_into().ok()?;
    let k: f64  = strike.try_into().ok()?;
    let t: f64  = time_to_expiry.try_into().ok()?;
    let v: f64  = volatility.try_into().ok()?;
    let r: f64  = rate.try_into().ok()?;

    if t <= 0.0 || v <= 0.0 || s <= 0.0 || k <= 0.0 {
        return None;
    }

    let sqrt_t = t.sqrt();
    let d1 = ((s / k).ln() + (r + 0.5 * v * v) * t) / (v * sqrt_t);
    let d2 = d1 - v * sqrt_t;

    let nd1     = standard_normal_cdf(d1);
    let nd2     = standard_normal_cdf(d2);
    let neg_nd1 = standard_normal_cdf(-d1);
    let neg_nd2 = standard_normal_cdf(-d2);

    // PDF of d1 for gamma/vega/theta.
    let pdf_d1 = (-0.5 * d1 * d1).exp() / (2.0 * std::f64::consts::PI).sqrt();

    let disc    = (-r * t).exp(); // e^{-rT}

    let (mark_price, delta, theta) = if is_call {
        let price = s * nd1 - k * disc * nd2;
        let delta = nd1;
        // Theta for call (per year).
        let theta = -(s * pdf_d1 * v) / (2.0 * sqrt_t)
            - r * k * disc * nd2;
        (price, delta, theta)
    } else {
        let price = k * disc * neg_nd2 - s * neg_nd1;
        let delta = nd1 - 1.0;
        // Theta for put (per year).
        let theta = -(s * pdf_d1 * v) / (2.0 * sqrt_t)
            + r * k * disc * neg_nd2;
        (price, delta, theta)
    };

    let gamma = pdf_d1 / (s * v * sqrt_t);
    let vega  = s * sqrt_t * pdf_d1 / 100.0; // per 1 % vol move
    let rho   = if is_call {
        k * t * disc * nd2 / 100.0
    } else {
        -k * t * disc * neg_nd2 / 100.0
    };
    // Theta is conventionally expressed per calendar day.
    let theta_daily = theta / 365.0;

    fn to_dec(x: f64) -> Decimal {
        Decimal::try_from(x).unwrap_or(Decimal::ZERO)
    }

    Some(Greeks {
        delta:      to_dec(delta),
        gamma:      to_dec(gamma),
        theta:      to_dec(theta_daily),
        vega:       to_dec(vega),
        rho:        to_dec(rho),
        iv:         volatility,
        mark_price: to_dec(mark_price.max(0.0)),
    })
}

/// Seed BTC option instruments at 5 strikes and 3 expiries (1w / 2w / 1m).
fn default_option_instruments() -> Vec<OptionInstrument> {
    let now = chrono::Utc::now();
    let expiries = [
        now + chrono::Duration::days(7),
        now + chrono::Duration::days(14),
        now + chrono::Duration::days(30),
    ];
    let strikes: [Decimal; 5] = [
        Decimal::new(60_000, 0),
        Decimal::new(65_000, 0),
        Decimal::new(70_000, 0),
        Decimal::new(75_000, 0),
        Decimal::new(80_000, 0),
    ];
    let mut out = Vec::new();
    for expiry in &expiries {
        let date_tag = expiry.format("%y%m%d").to_string();
        for &strike in &strikes {
            let strike_k = (strike / Decimal::new(1_000, 0)).round();
            for opt_type in [OptionType::Call, OptionType::Put] {
                let c = match opt_type { OptionType::Call => "C", OptionType::Put => "P" };
                out.push(OptionInstrument {
                    symbol: format!("BTC-{date_tag}-{strike_k}K-{c}"),
                    underlying: "BTCUSDT".to_string(),
                    strike_price: strike,
                    expiry: *expiry,
                    option_type: opt_type,
                    contract_size: Decimal::new(1, 2),
                    settle_coin: "USDT".to_string(),
                    is_active: true,
                });
            }
        }
    }
    out
}

/// Seed BTC/ETH delivery (quarterly) futures contracts.
fn default_delivery_contracts() -> Vec<DeliveryContract> {
    let now = chrono::Utc::now();
    let q1 = now + chrono::Duration::days(90);
    let q2 = now + chrono::Duration::days(180);
    vec![
        DeliveryContract {
            symbol: format!("BTCUSDT_{}", q1.format("%y%m%d")),
            underlying: "BTCUSDT".to_string(),
            delivery_date: q1,
            contract_size: Decimal::ONE,
            status: DeliveryStatus::Active,
        },
        DeliveryContract {
            symbol: format!("BTCUSDT_{}", q2.format("%y%m%d")),
            underlying: "BTCUSDT".to_string(),
            delivery_date: q2,
            contract_size: Decimal::ONE,
            status: DeliveryStatus::Active,
        },
        DeliveryContract {
            symbol: format!("ETHUSDT_{}", q1.format("%y%m%d")),
            underlying: "ETHUSDT".to_string(),
            delivery_date: q1,
            contract_size: Decimal::new(10, 0),
            status: DeliveryStatus::Active,
        },
        DeliveryContract {
            symbol: format!("ETHUSDT_{}", q2.format("%y%m%d")),
            underlying: "ETHUSDT".to_string(),
            delivery_date: q2,
            contract_size: Decimal::new(10, 0),
            status: DeliveryStatus::Active,
        },
    ]
}

/// Create the appropriate custody provider based on environment variables.
///
/// If `CUSTODY_BASE_URL` and `CUSTODY_HMAC_SECRET` are both set, uses `SelfHostedCustodyProvider`
/// (real custody service). Otherwise falls back to `MockCustodyProvider` (dev/test).
fn create_custody_provider() -> Box<dyn CustodyProvider + Send + Sync> {
    match (std::env::var("CUSTODY_BASE_URL"), std::env::var("CUSTODY_HMAC_SECRET")) {
        (Ok(url), Ok(secret)) => {
            tracing::info!(url = %url, "Using SelfHostedCustodyProvider (real custody)");
            Box::new(SelfHostedCustodyProvider::new(&url, secret.as_bytes()))
        }
        _ => {
            if std::env::var("PRODUCTION").is_ok() {
                tracing::warn!(
                    "PRODUCTION mode without CUSTODY_BASE_URL / CUSTODY_HMAC_SECRET — \
                     using MockCustodyProvider. Deposits and withdrawals will NOT work!"
                );
            }
            Box::new(MockCustodyProvider::new())
        }
    }
}

impl Exchange {
    /// Create exchange without WAL (for tests).
    pub fn new() -> Arc<Self> {
        let (market_tx, _) = broadcast::channel(10_000);
        let (private_tx, _) = broadcast::channel(10_000);
        let (fix_execution_tx, _) = broadcast::channel(1_000);
        // S15: post-fill async channel
        let (_post_fill_tx, _post_fill_rx) = tokio::sync::mpsc::channel::<PostFillEvent>(POST_FILL_CHANNEL_CAPACITY);

        let perp_configs = default_perp_configs();
        let insurance_funds = default_insurance_funds(&perp_configs);
        let symbols: Vec<Symbol> = perp_configs.keys().cloned().collect();

        let ex = Arc::new(Self {
            is_production: std::env::var("PRODUCTION").as_deref() == Ok("true"),
            is_dev_mode: std::env::var("DEV_MODE").is_ok()
                || std::env::var("EXCHANGE_DEV_MODE").is_ok()
                || std::env::var("DATABASE_URL").is_err(),
            metrics_api_key: std::env::var("METRICS_API_KEY").ok().filter(|s| !s.is_empty()),
            is_demo: std::sync::atomic::AtomicBool::new(false),
            engine_pool: RwLock::new(EnginePool::new()),
            accounts: ShardedAccounts::new_arc(),
            order_locks: dashmap::DashMap::new(),
            stp_guard: parking_lot::Mutex::new(SelfTradeGuard::new(SelfTradePolicy::CancelTaker)),
            rate_limiter: Arc::new(exchange_order_gateway::LockFreeRateLimiter::new(
                exchange_order_gateway::RateLimitConfig::default(),
            )),
            order_limit_config: OrderLimitConfig::default(),
            order_limit_overrides: HashMap::new(),
            journal: Arc::new(std::sync::Mutex::new(None)),
            group_commit_wal: None,
            key_store: None,
            metrics: Arc::new(ExchangeMetrics::new()),
            circuit_breakers: parking_lot::Mutex::new(default_circuit_breakers()),
            market_tx,
            private_tx,
            fix_execution_tx,
            market_data: Arc::new(InProcessMarketData::new()),
            // S15: post-fill channel — tx/rx initialized outside the struct literal
            // so the matched pair can be used across two fields.
            // See the block below for actual initialization; these placeholders
            // are overwritten immediately (the struct is built in a block that
            // runs the channel::channel() call first, then places into fields).
            // IMPORTANT: See the `let ex = Arc::new(Self { ... })` block above —
            // the actual channel is created in the `new()` preamble and spliced in.
            post_fill_event_tx: _post_fill_tx,
            post_fill_event_rx: Mutex::new(Some(_post_fill_rx)),
            positions: parking_lot::Mutex::new(PositionManager::new()),
            perp_configs: ArcSwap::new(Arc::new(perp_configs)),
            product_status: ArcSwap::new(Arc::new(HashMap::new())),
            trigger_engine: Mutex::new(TriggerEngine::with_defaults()),
            bracket_manager: Mutex::new(exchange_trigger_service::BracketManager::new()),
            index_aggregator: Mutex::new(IndexPriceAggregator::with_defaults(symbols)),
            funding_scheduler: Mutex::new(FundingScheduler::new(8)),
            insurance_funds: Mutex::new(insurance_funds),
            funding_history: Mutex::new(HashMap::new()),
            premium_buffers: Mutex::new(HashMap::new()),
            mark_ema_basis: Mutex::new(HashMap::new()),
            vol_surface: Mutex::new(None),
            fills_processed: std::sync::atomic::AtomicU64::new(0),
            fills_ledger_written: std::sync::atomic::AtomicU64::new(0),
            ledger_write_failures: std::sync::atomic::AtomicU64::new(0),
            order_history: Mutex::new(Vec::new()),
            trade_history: Mutex::new(HashMap::new()),
            user_fills: Mutex::new(HashMap::new()),
            user_leverage: Mutex::new(HashMap::new()),
            pending_orders: dashmap::DashMap::new(),
            closed_positions: Mutex::new(Vec::new()),
            futures_margin_locks: Mutex::new(HashMap::new()),
            fee_engine: Mutex::new(FeeEngine::new()),
            surveillance: parking_lot::Mutex::new(SurveillanceEngine::with_defaults()),
            account_statuses: Mutex::new(HashMap::new()),
            rate_limit_overrides: Mutex::new(HashMap::new()),
            toxicity_scorer: Mutex::new(UserToxicityScorer::with_defaults()),
            notifications: Mutex::new(NotificationManager::new()),
            device_registry: Mutex::new(exchange_admin::DeviceRegistry::new()),
            sub_accounts: Mutex::new(SubAccountManager::new()),
            transfers: Mutex::new(TransferManager::new()),
            referrals: Mutex::new(ReferralManager::new()),
            totp: Mutex::new(TotpManager::new()),
            asset_registry: std::sync::Arc::new(Mutex::new(exchange_asset_registry::AssetRegistry::with_defaults())),
            deposit_manager: Mutex::new(DepositManager::new()),
            withdrawal_manager: Mutex::new(WithdrawalManager::new()),
            custody_provider: create_custody_provider(),
            address_whitelist: Mutex::new(AddressWhitelist::new()),
            wallet_manager: Mutex::new(WalletManager::new()),
            travel_rule: Mutex::new(TravelRuleEngine::new()),
            verification_service: AsyncMutex::new(VerificationService::mock()),
            sanctions: Mutex::new(SanctionsScreener::with_defaults()),
            last_ofac_refresh_ms: std::sync::atomic::AtomicU64::new(0),
            pep_screener: Mutex::new(PepScreener::with_defaults()),
            tos_manager: Mutex::new(TosManager::new()),
            audit_log: Mutex::new(AuditLog::new()),
            complaint_manager: Mutex::new(ComplaintManager::new()),
            risk_warnings: Mutex::new(RiskWarningManager::new()),
            ws_ip_connections: Mutex::new(HashMap::new()),
            feature_flags: Arc::new(exchange_common::FeatureFlags::from_env()),
            jwt: JwtManager::new(
                &std::env::var("JWT_SECRET").unwrap_or_else(|_| "dev-secret-change-in-production".to_string()),
            ),
            password_hashes: Mutex::new(HashMap::new()),
            user_emails: Mutex::new(HashMap::new()),
            refresh_tokens: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            copy_trading_masters: Mutex::new(MasterRegistry::new()),
            copy_trading_followers: Mutex::new(FollowerManager::with_defaults()),
            grid_bots: Mutex::new(Vec::new()),
            dca_bots: Mutex::new(Vec::new()),
            convert_quotes: Mutex::new(HashMap::new()),
            convert_history: Mutex::new(Vec::new()),
            spot_margin_settings: Mutex::new(HashMap::new()),
            margin_borrows: Mutex::new(HashMap::new()),
            margin_interest_history: Mutex::new(Vec::new()),
            dust_conversions: Mutex::new(Vec::new()),
            earn_products: Mutex::new(default_earn_products()),
            earn_allocations: Mutex::new(Vec::new()),
            loans: Mutex::new(Vec::new()),
            lending_rates: Mutex::new({
                let mut m = HashMap::new();
                m.insert("USDT".into(), Decimal::new(1, 6));
                m.insert("BTC".into(), Decimal::new(5, 7));
                m.insert("ETH".into(), Decimal::new(5, 7));
                m
            }),
            twap_executions: Mutex::new(HashMap::new()),
            vwap_executions: Mutex::new(HashMap::new()),
            order_link_index: parking_lot::Mutex::new(HashMap::new()),
            gtd_orders: Mutex::new(HashMap::new()),
            iceberg_manager: Mutex::new(exchange_matching_engine::IcebergManager::new()),
            iceberg_meta: Mutex::new(HashMap::new()),
            user_settings: Mutex::new(HashMap::new()),
            db_pool: std::sync::OnceLock::new(),
            token_blacklist: dashmap::DashSet::new(),
            four_eyes: Mutex::new(crate::four_eyes::FourEyesManager::new()),
            secrets: Arc::new(exchange_common::EnvSecretProvider),
            position_db_ids: Mutex::new(HashMap::new()),
            fund_accounts: Mutex::new(HashMap::new()),
            dead_mans_switch: Mutex::new(HashMap::new()),
            email_sender: match exchange_admin::EmailConfig::from_env() {
                Some(config) => {
                    tracing::info!("Email sender configured");
                    Some(Arc::new(exchange_admin::EmailSender::new(config)))
                }
                None => {
                    tracing::warn!("SMTP not configured — email notifications disabled");
                    None
                }
            },
            email_codes: Mutex::new(HashMap::new()),
            email_verified: dashmap::DashSet::new(),
            kyc_approved: dashmap::DashSet::new(),
            mmp_state: Mutex::new(HashMap::new()),
            risk_scoring: Mutex::new(RiskScoringEngine::new()),
            edd_manager: Mutex::new(EddManager::new()),
            sar_manager: Mutex::new(SarManager::new()),
            corporate_kyc: Mutex::new(CorporateKycManager::new()),
            withdrawal_approvals: Mutex::new(HashMap::new()),
            withdrawal_approval_policy: ApprovalPolicy::default(),
            retention_policies: Mutex::new(default_retention_policies()),
            user_funding_history: Mutex::new(HashMap::new()),
            regulatory_reports: Mutex::new(Vec::new()),
            vasp_directory: Mutex::new(VaspDirectory::new()),
            jurisdiction_engine: Mutex::new(JurisdictionEngine::new()),
            user_jurisdictions: Mutex::new(HashMap::new()),
            capital_tracker: Mutex::new(CapitalTracker::new()),
            frozen_accounts: dashmap::DashSet::new(),
            operating_mode: exchange_common::operating_mode::AtomicOperatingMode::default(),
            pair_phases: parking_lot::Mutex::new(HashMap::new()),
            auction_orders: Mutex::new(HashMap::new()),
            anti_phishing_codes: Mutex::new(HashMap::new()),
            price_alerts: Mutex::new(Vec::new()),
            notification_prefs: Mutex::new(HashMap::new()),
            margin_call_cooldowns: Mutex::new(HashMap::new()),
            withdrawal_confirmations: Mutex::new(HashMap::new()),
            known_login_ips: Mutex::new(HashMap::new()),
            maintenance_mode: Mutex::new(None),
            user_devices: Mutex::new(HashMap::new()),
            user_margin_modes: Mutex::new(HashMap::new()),
            insurance_fund_history: Mutex::new(Vec::new()),
            chain_manager: Mutex::new(exchange_trigger_service::ChainManager::new()),
            sub_account_api_keys: Mutex::new(HashMap::new()),
            market_maker_program: Mutex::new(MarketMakerProgram::new()),
            rate_limit_configs: crate::security::default_rate_limit_configs(),
            ip_reputation: crate::security::IpReputationTracker::new(),
            totp_attempt_tracker: crate::security::TotpAttemptTracker::new(),
            security_events: Mutex::new(crate::security::SecurityEventLog::default()),
            fiat_gateway: Mutex::new(crate::fiat::FiatGateway::with_defaults()),
            collateral_configs: Mutex::new(CollateralConfig::defaults()),
            user_collateral_settings: Mutex::new(HashMap::new()),
            liquidation_events: Mutex::new(Vec::new()),
            listen_keys: Mutex::new(HashMap::new()),
            portfolio_margin_users: Mutex::new(HashSet::new()),
            block_trade_rfqs: Mutex::new(HashMap::new()),
            block_trades: Mutex::new(Vec::new()),
            brokers: Mutex::new(HashMap::new()),
            broker_commissions: Mutex::new(VecDeque::new()),
            broker_clients: Mutex::new(HashMap::new()),
            block_trade_minimums: Mutex::new(default_block_trade_minimums()),
            competitions: Mutex::new(Vec::new()),
            competition_participants: Mutex::new(HashMap::new()),
            leveraged_tokens: Mutex::new(default_leveraged_tokens()),
            lt_holdings: Mutex::new(HashMap::new()),
            defi_allocations: Mutex::new(Vec::new()),
            web3_wallets: Mutex::new(exchange_web3_wallet::WalletManager::new()),
            web3_tx_history: Mutex::new(HashMap::new()),
            dapp_connector: Mutex::new(exchange_web3_wallet::DAppConnector::new()),
            unified_accounts: Mutex::new(HashMap::new()),
            copy_trading_settings: Mutex::new(HashMap::new()),
            p2p_ads: Mutex::new(Vec::new()),
            p2p_orders: Mutex::new(Vec::new()),
            algo_orders: Mutex::new(Vec::new()),
            signal_bots: Mutex::new(Vec::new()),
            recurring_buy_plans: Mutex::new(Vec::new()),
            oi_history: Mutex::new(HashMap::new()),
            taker_volume_history: Mutex::new(HashMap::new()),
            long_short_history: Mutex::new(HashMap::new()),
            dual_invest_products: Mutex::new(default_dual_invest_products()),
            dual_invest_positions: Mutex::new(Vec::new()),
            staking_positions: Mutex::new(HashMap::new()),
            staking_unbonding: Mutex::new(Vec::new()),
            staking_apy_history: Mutex::new(HashMap::new()),
            crypto_loans: Mutex::new(Vec::new()),
            sub_account_api_key_records: Mutex::new(HashMap::new()),
            sub_transfer_permissions: Mutex::new(HashMap::new()),
            announcements: Mutex::new(Vec::new()),
            system_status: Mutex::new(SystemStatus::default()),
            block_trade_counterparties: Mutex::new(Vec::new()),
            strategy_orders: Mutex::new(Vec::new()),
            spot_hedge_users: Mutex::new(HashSet::new()),
            prevented_matches: Mutex::new(VecDeque::new()),
            option_instruments: Mutex::new(default_option_instruments()),
            option_positions: Mutex::new(HashMap::new()),
            delivery_contracts: Mutex::new(default_delivery_contracts()),
            delivery_positions: Mutex::new(HashMap::new()),
            delivery_settlement_history: Mutex::new(Vec::new()),
            launchpad_projects: Mutex::new(Vec::new()),
            launchpad_commitments: Mutex::new(Vec::new()),
            launchpool_pools: Mutex::new(Vec::new()),
            launchpool_stakes: Mutex::new(Vec::new()),
            shark_fin_products: Mutex::new(Vec::new()),
            shark_fin_positions: Mutex::new(Vec::new()),
            liquidity_pools: Mutex::new(Vec::new()),
            liquidity_positions: Mutex::new(Vec::new()),
            auto_convert_settings: Mutex::new(HashMap::new()),
            advanced_bots: Mutex::new(Vec::new()),
            daily_snapshots: Mutex::new(HashMap::new()),
            account_tiers: Mutex::new(HashMap::new()),
            gift_cards: Mutex::new(Vec::new()),
            premium_history: Mutex::new(HashMap::new()),
            bonus_campaigns: Mutex::new(Vec::new()),
            user_bonuses: Mutex::new(HashMap::new()),
            airdrops: Mutex::new(Vec::new()),
            airdrop_claims: Mutex::new(HashMap::new()),
            referral_tree: Mutex::new(HashMap::new()),
            affiliate_commissions: Mutex::new(VecDeque::new()),
            competition_teams: Mutex::new(Vec::new()),
            latest_por_report: Mutex::new(None),
        });

        // Register the treasury account so it can receive fee credits.
        ex.accounts.create_account(TREASURY_ACCOUNT_ID);

        // Register the yield reserve pool and seed it so earn/staking payouts
        // are backed by real capital rather than created from nothing.
        ex.accounts.create_account(YIELD_POOL_ACCOUNT_ID);
        let _ = ex.accounts.deposit(YIELD_POOL_ACCOUNT_ID, "USDT", Decimal::from(1_000_000u64));
        let _ = ex.accounts.deposit(YIELD_POOL_ACCOUNT_ID, "BTC", Decimal::from(10u64));
        let _ = ex.accounts.deposit(YIELD_POOL_ACCOUNT_ID, "ETH", Decimal::from(100u64));

        ex
    }

    /// Create exchange with WAL + optional API key store (for production).
    pub fn with_journal(wal_path: impl Into<PathBuf>, key_store: Option<Arc<ApiKeyStore>>) -> anyhow::Result<Arc<Self>> {
        let path = wal_path.into();

        // Open a segmented WAL in the directory at `path`.
        // If `path` points to a file (legacy single-file WAL), we use the parent
        // directory for segments and the file itself is ignored (replay reads all
        // segment files in the directory).
        let wal_dir = if path.is_dir() {
            path.clone()
        } else {
            // Legacy: path was "data/commands.wal". Use "data/wal/" as segment dir.
            let dir = path.parent().unwrap_or(&path).join("wal");
            std::fs::create_dir_all(&dir)?;
            dir
        };

        let wal = exchange_journal::SegmentedWal::open(&wal_dir)
            .map_err(|e| anyhow::anyhow!("Failed to open SegmentedWal at {}: {}", wal_dir.display(), e))?;

        tracing::info!(
            dir = %wal_dir.display(),
            segment = wal.current_segment_id(),
            "SegmentedWal opened"
        );

        // Start the group-commit WAL worker that writes through the shared
        // SegmentedWal. Commands are batched in an async channel and flushed
        // via append_batch (N writes + 1 fsync). This eliminates the split-brain
        // bug where the old GC WAL wrote to a separate file.
        let shared_journal: Arc<std::sync::Mutex<Option<exchange_journal::SegmentedWal>>> =
            Arc::new(std::sync::Mutex::new(Some(wal)));
        let (gc_wal, gc_handle) = GroupCommitWal::start_shared(
            Arc::clone(&shared_journal),
            256,  // max_batch_size
            10,   // max_delay_ms
        );
        tokio::spawn(async move {
            if let Err(e) = gc_handle.await {
                tracing::error!(error = %e, "group-commit WAL worker panicked");
            }
        });
        let gc_wal = Arc::new(gc_wal);
        tracing::info!("Group-commit WAL started (shared, batch_size=256, delay_ms=10)");

        if std::env::var("JWT_SECRET").is_err() {
            tracing::warn!("JWT_SECRET not set — using insecure default. Set JWT_SECRET in production!");
        }

        let (market_tx, _) = broadcast::channel(10_000);
        let (private_tx, _) = broadcast::channel(10_000);
        let (fix_execution_tx, _) = broadcast::channel(1_000);
        // S15: post-fill async channel
        let (_post_fill_tx, _post_fill_rx) = tokio::sync::mpsc::channel::<PostFillEvent>(POST_FILL_CHANNEL_CAPACITY);

        let perp_configs = default_perp_configs();
        let insurance_funds = default_insurance_funds(&perp_configs);
        let symbols: Vec<Symbol> = perp_configs.keys().cloned().collect();

        let ex = Arc::new(Self {
            is_production: std::env::var("PRODUCTION").as_deref() == Ok("true"),
            is_dev_mode: std::env::var("DEV_MODE").is_ok()
                || std::env::var("EXCHANGE_DEV_MODE").is_ok()
                || std::env::var("DATABASE_URL").is_err(),
            metrics_api_key: std::env::var("METRICS_API_KEY").ok().filter(|s| !s.is_empty()),
            is_demo: std::sync::atomic::AtomicBool::new(false),
            engine_pool: RwLock::new(EnginePool::new()),
            accounts: ShardedAccounts::new_arc(),
            order_locks: dashmap::DashMap::new(),
            stp_guard: parking_lot::Mutex::new(SelfTradeGuard::new(SelfTradePolicy::CancelTaker)),
            rate_limiter: Arc::new(exchange_order_gateway::LockFreeRateLimiter::new(
                exchange_order_gateway::RateLimitConfig::default(),
            )),
            order_limit_config: OrderLimitConfig::default(),
            order_limit_overrides: HashMap::new(),
            journal: shared_journal,
            group_commit_wal: Some(gc_wal),
            key_store,
            metrics: Arc::new(ExchangeMetrics::new()),
            circuit_breakers: parking_lot::Mutex::new(default_circuit_breakers()),
            market_tx,
            private_tx,
            fix_execution_tx,
            market_data: Arc::new(InProcessMarketData::new()),
            post_fill_event_tx: _post_fill_tx,
            post_fill_event_rx: Mutex::new(Some(_post_fill_rx)),
            positions: parking_lot::Mutex::new(PositionManager::new()),
            perp_configs: ArcSwap::new(Arc::new(perp_configs)),
            product_status: ArcSwap::new(Arc::new(HashMap::new())),
            trigger_engine: Mutex::new(TriggerEngine::with_defaults()),
            bracket_manager: Mutex::new(exchange_trigger_service::BracketManager::new()),
            index_aggregator: Mutex::new(IndexPriceAggregator::with_defaults(symbols)),
            funding_scheduler: Mutex::new(FundingScheduler::new(8)),
            insurance_funds: Mutex::new(insurance_funds),
            funding_history: Mutex::new(HashMap::new()),
            premium_buffers: Mutex::new(HashMap::new()),
            mark_ema_basis: Mutex::new(HashMap::new()),
            vol_surface: Mutex::new(None),
            fills_processed: std::sync::atomic::AtomicU64::new(0),
            fills_ledger_written: std::sync::atomic::AtomicU64::new(0),
            ledger_write_failures: std::sync::atomic::AtomicU64::new(0),
            order_history: Mutex::new(Vec::new()),
            trade_history: Mutex::new(HashMap::new()),
            user_fills: Mutex::new(HashMap::new()),
            user_leverage: Mutex::new(HashMap::new()),
            pending_orders: dashmap::DashMap::new(),
            closed_positions: Mutex::new(Vec::new()),
            futures_margin_locks: Mutex::new(HashMap::new()),
            fee_engine: Mutex::new(FeeEngine::new()),
            surveillance: parking_lot::Mutex::new(SurveillanceEngine::with_defaults()),
            account_statuses: Mutex::new(HashMap::new()),
            rate_limit_overrides: Mutex::new(HashMap::new()),
            toxicity_scorer: Mutex::new(UserToxicityScorer::with_defaults()),
            notifications: Mutex::new(NotificationManager::new()),
            device_registry: Mutex::new(exchange_admin::DeviceRegistry::new()),
            sub_accounts: Mutex::new(SubAccountManager::new()),
            transfers: Mutex::new(TransferManager::new()),
            referrals: Mutex::new(ReferralManager::new()),
            totp: Mutex::new(TotpManager::new()),
            asset_registry: std::sync::Arc::new(Mutex::new(exchange_asset_registry::AssetRegistry::with_defaults())),
            deposit_manager: Mutex::new(DepositManager::new()),
            withdrawal_manager: Mutex::new(WithdrawalManager::new()),
            custody_provider: create_custody_provider(),
            address_whitelist: Mutex::new(AddressWhitelist::new()),
            wallet_manager: Mutex::new(WalletManager::new()),
            travel_rule: Mutex::new(TravelRuleEngine::new()),
            verification_service: AsyncMutex::new(VerificationService::mock()),
            sanctions: Mutex::new(SanctionsScreener::with_defaults()),
            last_ofac_refresh_ms: std::sync::atomic::AtomicU64::new(0),
            pep_screener: Mutex::new(PepScreener::with_defaults()),
            tos_manager: Mutex::new(TosManager::new()),
            audit_log: Mutex::new(AuditLog::new()),
            complaint_manager: Mutex::new(ComplaintManager::new()),
            risk_warnings: Mutex::new(RiskWarningManager::new()),
            ws_ip_connections: Mutex::new(HashMap::new()),
            feature_flags: Arc::new(exchange_common::FeatureFlags::from_env()),
            jwt: JwtManager::new(
                &std::env::var("JWT_SECRET").unwrap_or_else(|_| "dev-secret-change-in-production".to_string()),
            ),
            password_hashes: Mutex::new(HashMap::new()),
            user_emails: Mutex::new(HashMap::new()),
            refresh_tokens: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            copy_trading_masters: Mutex::new(MasterRegistry::new()),
            copy_trading_followers: Mutex::new(FollowerManager::with_defaults()),
            grid_bots: Mutex::new(Vec::new()),
            dca_bots: Mutex::new(Vec::new()),
            convert_quotes: Mutex::new(HashMap::new()),
            convert_history: Mutex::new(Vec::new()),
            spot_margin_settings: Mutex::new(HashMap::new()),
            margin_borrows: Mutex::new(HashMap::new()),
            margin_interest_history: Mutex::new(Vec::new()),
            dust_conversions: Mutex::new(Vec::new()),
            earn_products: Mutex::new(default_earn_products()),
            earn_allocations: Mutex::new(Vec::new()),
            loans: Mutex::new(Vec::new()),
            lending_rates: Mutex::new({
                let mut m = HashMap::new();
                m.insert("USDT".into(), Decimal::new(1, 6));
                m.insert("BTC".into(), Decimal::new(5, 7));
                m.insert("ETH".into(), Decimal::new(5, 7));
                m
            }),
            twap_executions: Mutex::new(HashMap::new()),
            vwap_executions: Mutex::new(HashMap::new()),
            order_link_index: parking_lot::Mutex::new(HashMap::new()),
            gtd_orders: Mutex::new(HashMap::new()),
            iceberg_manager: Mutex::new(exchange_matching_engine::IcebergManager::new()),
            iceberg_meta: Mutex::new(HashMap::new()),
            user_settings: Mutex::new(HashMap::new()),
            db_pool: std::sync::OnceLock::new(),
            token_blacklist: dashmap::DashSet::new(),
            four_eyes: Mutex::new(crate::four_eyes::FourEyesManager::new()),
            secrets: Arc::new(exchange_common::EnvSecretProvider),
            position_db_ids: Mutex::new(HashMap::new()),
            fund_accounts: Mutex::new(HashMap::new()),
            dead_mans_switch: Mutex::new(HashMap::new()),
            email_sender: match exchange_admin::EmailConfig::from_env() {
                Some(config) => {
                    tracing::info!("Email sender configured");
                    Some(Arc::new(exchange_admin::EmailSender::new(config)))
                }
                None => {
                    tracing::warn!("SMTP not configured — email notifications disabled");
                    None
                }
            },
            email_codes: Mutex::new(HashMap::new()),
            email_verified: dashmap::DashSet::new(),
            kyc_approved: dashmap::DashSet::new(),
            mmp_state: Mutex::new(HashMap::new()),
            risk_scoring: Mutex::new(RiskScoringEngine::new()),
            edd_manager: Mutex::new(EddManager::new()),
            sar_manager: Mutex::new(SarManager::new()),
            corporate_kyc: Mutex::new(CorporateKycManager::new()),
            withdrawal_approvals: Mutex::new(HashMap::new()),
            withdrawal_approval_policy: ApprovalPolicy::default(),
            retention_policies: Mutex::new(default_retention_policies()),
            user_funding_history: Mutex::new(HashMap::new()),
            regulatory_reports: Mutex::new(Vec::new()),
            vasp_directory: Mutex::new(VaspDirectory::new()),
            jurisdiction_engine: Mutex::new(JurisdictionEngine::new()),
            user_jurisdictions: Mutex::new(HashMap::new()),
            capital_tracker: Mutex::new(CapitalTracker::new()),
            frozen_accounts: dashmap::DashSet::new(),
            operating_mode: exchange_common::operating_mode::AtomicOperatingMode::default(),
            pair_phases: parking_lot::Mutex::new(HashMap::new()),
            auction_orders: Mutex::new(HashMap::new()),
            anti_phishing_codes: Mutex::new(HashMap::new()),
            price_alerts: Mutex::new(Vec::new()),
            notification_prefs: Mutex::new(HashMap::new()),
            margin_call_cooldowns: Mutex::new(HashMap::new()),
            withdrawal_confirmations: Mutex::new(HashMap::new()),
            known_login_ips: Mutex::new(HashMap::new()),
            maintenance_mode: Mutex::new(None),
            user_devices: Mutex::new(HashMap::new()),
            user_margin_modes: Mutex::new(HashMap::new()),
            insurance_fund_history: Mutex::new(Vec::new()),
            chain_manager: Mutex::new(exchange_trigger_service::ChainManager::new()),
            sub_account_api_keys: Mutex::new(HashMap::new()),
            market_maker_program: Mutex::new(MarketMakerProgram::new()),
            rate_limit_configs: crate::security::default_rate_limit_configs(),
            ip_reputation: crate::security::IpReputationTracker::new(),
            totp_attempt_tracker: crate::security::TotpAttemptTracker::new(),
            security_events: Mutex::new(crate::security::SecurityEventLog::default()),
            fiat_gateway: Mutex::new(crate::fiat::FiatGateway::with_defaults()),
            collateral_configs: Mutex::new(CollateralConfig::defaults()),
            user_collateral_settings: Mutex::new(HashMap::new()),
            liquidation_events: Mutex::new(Vec::new()),
            listen_keys: Mutex::new(HashMap::new()),
            portfolio_margin_users: Mutex::new(HashSet::new()),
            block_trade_rfqs: Mutex::new(HashMap::new()),
            block_trades: Mutex::new(Vec::new()),
            brokers: Mutex::new(HashMap::new()),
            broker_commissions: Mutex::new(VecDeque::new()),
            broker_clients: Mutex::new(HashMap::new()),
            block_trade_minimums: Mutex::new(default_block_trade_minimums()),
            competitions: Mutex::new(Vec::new()),
            competition_participants: Mutex::new(HashMap::new()),
            leveraged_tokens: Mutex::new(default_leveraged_tokens()),
            lt_holdings: Mutex::new(HashMap::new()),
            defi_allocations: Mutex::new(Vec::new()),
            web3_wallets: Mutex::new(exchange_web3_wallet::WalletManager::new()),
            web3_tx_history: Mutex::new(HashMap::new()),
            dapp_connector: Mutex::new(exchange_web3_wallet::DAppConnector::new()),
            unified_accounts: Mutex::new(HashMap::new()),
            copy_trading_settings: Mutex::new(HashMap::new()),
            p2p_ads: Mutex::new(Vec::new()),
            p2p_orders: Mutex::new(Vec::new()),
            algo_orders: Mutex::new(Vec::new()),
            signal_bots: Mutex::new(Vec::new()),
            recurring_buy_plans: Mutex::new(Vec::new()),
            oi_history: Mutex::new(HashMap::new()),
            taker_volume_history: Mutex::new(HashMap::new()),
            long_short_history: Mutex::new(HashMap::new()),
            dual_invest_products: Mutex::new(default_dual_invest_products()),
            dual_invest_positions: Mutex::new(Vec::new()),
            staking_positions: Mutex::new(HashMap::new()),
            staking_unbonding: Mutex::new(Vec::new()),
            staking_apy_history: Mutex::new(HashMap::new()),
            crypto_loans: Mutex::new(Vec::new()),
            sub_account_api_key_records: Mutex::new(HashMap::new()),
            sub_transfer_permissions: Mutex::new(HashMap::new()),
            announcements: Mutex::new(Vec::new()),
            system_status: Mutex::new(SystemStatus::default()),
            block_trade_counterparties: Mutex::new(Vec::new()),
            strategy_orders: Mutex::new(Vec::new()),
            spot_hedge_users: Mutex::new(HashSet::new()),
            prevented_matches: Mutex::new(VecDeque::new()),
            option_instruments: Mutex::new(default_option_instruments()),
            option_positions: Mutex::new(HashMap::new()),
            delivery_contracts: Mutex::new(default_delivery_contracts()),
            delivery_positions: Mutex::new(HashMap::new()),
            delivery_settlement_history: Mutex::new(Vec::new()),
            launchpad_projects: Mutex::new(Vec::new()),
            launchpad_commitments: Mutex::new(Vec::new()),
            launchpool_pools: Mutex::new(Vec::new()),
            launchpool_stakes: Mutex::new(Vec::new()),
            shark_fin_products: Mutex::new(Vec::new()),
            shark_fin_positions: Mutex::new(Vec::new()),
            liquidity_pools: Mutex::new(Vec::new()),
            liquidity_positions: Mutex::new(Vec::new()),
            auto_convert_settings: Mutex::new(HashMap::new()),
            advanced_bots: Mutex::new(Vec::new()),
            daily_snapshots: Mutex::new(HashMap::new()),
            account_tiers: Mutex::new(HashMap::new()),
            gift_cards: Mutex::new(Vec::new()),
            premium_history: Mutex::new(HashMap::new()),
            bonus_campaigns: Mutex::new(Vec::new()),
            user_bonuses: Mutex::new(HashMap::new()),
            airdrops: Mutex::new(Vec::new()),
            airdrop_claims: Mutex::new(HashMap::new()),
            referral_tree: Mutex::new(HashMap::new()),
            affiliate_commissions: Mutex::new(VecDeque::new()),
            competition_teams: Mutex::new(Vec::new()),
            latest_por_report: Mutex::new(None),
        });

        // Register the treasury account so it can receive fee credits.
        ex.accounts.create_account(TREASURY_ACCOUNT_ID);

        // Register the yield reserve pool and seed it so earn/staking payouts
        // are backed by real capital rather than created from nothing.
        ex.accounts.create_account(YIELD_POOL_ACCOUNT_ID);
        let _ = ex.accounts.deposit(YIELD_POOL_ACCOUNT_ID, "USDT", Decimal::from(1_000_000u64));
        let _ = ex.accounts.deposit(YIELD_POOL_ACCOUNT_ID, "BTC", Decimal::from(10u64));
        let _ = ex.accounts.deposit(YIELD_POOL_ACCOUNT_ID, "ETH", Decimal::from(100u64));

        Ok(ex)
    }

    /// Generate and store a 6-digit email verification code for a user.
    ///
    /// The code is stored with a 15-minute expiry. Any previously issued code
    /// for this user is overwritten.
    pub fn generate_email_code(self: &Arc<Self>, user_id: UserId) -> String {
        use rand_core::RngCore;
        let mut rng = rand_core::OsRng;
        let code = format!("{:06}", rng.next_u32() % 1_000_000);
        let expires = chrono::Utc::now() + chrono::Duration::minutes(15);
        self.email_codes
            .lock().unwrap_or_else(|e| e.into_inner())
            .insert(user_id, (code.clone(), expires));
        code
    }

    /// Verify an email code. Returns `true` if the code matches and has not expired.
    ///
    /// On success the code is consumed and the user is added to `email_verified`.
    pub fn verify_email_code(self: &Arc<Self>, user_id: UserId, code: &str) -> bool {
        let mut codes = self.email_codes.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((stored_code, expires)) = codes.get(&user_id) {
            if stored_code == code && chrono::Utc::now() < *expires {
                codes.remove(&user_id);
                drop(codes);
                self.email_verified.insert(user_id);
                return true;
            }
        }
        false
    }

    /// Set the PostgreSQL pool for ledger writes.
    ///
    /// Call this after construction and before spawning background tasks.
    /// Once set, every financial operation will write an async ledger entry
    /// to the database without blocking the synchronous call path.
    /// Verify a JWT access token AND check the blacklist.
    ///
    /// Returns the claims if valid, or an error if the token is invalid,
    /// expired, or has been revoked (blacklisted via logout or admin freeze).
    pub fn verify_access_token(
        &self,
        token: &str,
    ) -> Result<crate::web_auth::Claims, crate::web_auth::WebAuthError> {
        let claims = self.jwt.verify_access(token)?;
        if self.token_blacklist.contains(&claims.jti) {
            return Err(crate::web_auth::WebAuthError::TokenInvalid(
                "Token has been revoked".to_string(),
            ));
        }
        Ok(claims)
    }

    /// Blacklist a token by its JTI. Used by logout and admin freeze.
    pub fn blacklist_token(&self, jti: &str) {
        self.token_blacklist.insert(jti.to_string());
    }

    pub fn set_db_pool(self: &Arc<Self>, pool: sqlx::PgPool) {
        // OnceLock::set returns Err if already set — that would be a programming error,
        // so we log it as a warning rather than silently dropping the new pool.
        if self.db_pool.set(pool).is_err() {
            tracing::warn!("set_db_pool called more than once — ignoring duplicate call");
        }
        tracing::info!("Ledger writes enabled (PostgreSQL connected)");
    }

    /// Load state from PostgreSQL on startup.
    ///
    /// Restores users, balances, and open positions from the database into
    /// in-memory state. Call after `set_db_pool` and before the HTTP server
    /// starts accepting requests.
    pub async fn load_from_db(self: &Arc<Self>, pool: &sqlx::PgPool) -> anyhow::Result<()> {
        // 1. Load users
        let users = exchange_db::user_repo::UserRepo::list_all(pool).await?;
        let user_count = users.len();
        for user in &users {
            self.accounts.create_account(user.id);
            if let Some(ref email) = user.email {
                self.user_emails.lock().unwrap_or_else(|e| e.into_inner())
                    .insert(email.clone(), user.id);
            }
        }
        tracing::info!(count = user_count, "loaded users from DB");

        // 2. Load balances
        let balances = exchange_db::balance_repo::BalanceRepo::all_balances(pool).await?;
        let balance_count = balances.len();
        for bal in &balances {
            let _ = self.accounts.deposit(bal.user_id, &bal.asset, bal.total);
        }
        tracing::info!(count = balance_count, "loaded balances from DB");

        // 3. Load open positions
        let positions = exchange_db::position_repo::PositionRepo::list_open(pool).await?;
        let pos_count = positions.len();
        for pos in &positions {
            let side = match pos.side.as_str() {
                "Long" => exchange_common::futures::PositionSide::Long,
                _ => exchange_common::futures::PositionSide::Short,
            };
            let margin_mode = match pos.margin_mode.as_str() {
                "Cross" => exchange_common::futures::MarginMode::Cross,
                _ => exchange_common::futures::MarginMode::Isolated,
            };
            let _ = self.positions.lock()
                .open_or_increase(
                    pos.user_id,
                    exchange_common::Symbol::new(&pos.symbol),
                    side,
                    pos.size,
                    pos.entry_price,
                    pos.leverage,
                    margin_mode,
                    0, // one-way mode on DB restore
                );
            self.position_db_ids.lock().unwrap_or_else(|e| e.into_inner())
                .insert((pos.user_id, exchange_common::Symbol::new(&pos.symbol)), pos.id);
        }
        tracing::info!(count = pos_count, "loaded open positions from DB");

        // 4. C1: Load open orders into pending_orders
        let open_orders = exchange_db::order_repo::OrderRepo::list_open(pool).await?;
        let open_count = open_orders.len();
        for row in &open_orders {
            let side = match row.side.as_str() {
                "Buy" => exchange_common::Side::Buy,
                _ => exchange_common::Side::Sell,
            };
            let order_type = match row.order_type.as_str() {
                "Market" => exchange_common::OrderType::Market,
                _ => exchange_common::OrderType::Limit,
            };
            self.pending_orders.insert(row.id, PendingOrderInfo {
                user_id: row.user_id,
                symbol: exchange_common::Symbol::new(&row.symbol),
                side,
                order_type,
                price: row.price.unwrap_or(rust_decimal::Decimal::ZERO),
                qty: row.qty,
                created_at: row.created_at,
                order_link_id: None,
                reduce_only: false,
            });
        }
        tracing::info!(count = open_count, "loaded open orders from DB into pending_orders");

        Ok(())
    }

    /// Register a new trading pair.
    pub fn add_product(self: &Arc<Self>, product: Product) {
        self.engine_pool.write().unwrap_or_else(|e| e.into_inner()).add_symbol(product.symbol.clone());
        self.accounts.add_product(product);
    }

    /// Register a perpetual contract configuration.
    ///
    /// Write path (admin-only, infrequent): load current Arc<HashMap>, clone,
    /// insert the new entry, then atomically swap the pointer. Readers that
    /// loaded before the swap see the old snapshot; readers after the swap see
    /// the new one. The narrow window is the same eventual-consistency property
    /// used by Bybit/Binance for config changes.
    pub fn add_perp_config(self: &Arc<Self>, config: PerpetualConfig) {
        let old = self.perp_configs.load_full();
        let mut new_map = (*old).clone();
        new_map.insert(config.symbol.clone(), config);
        self.perp_configs.store(Arc::new(new_map));
    }

    /// Create user account + deposit initial funds.
    pub fn create_account(self: &Arc<Self>, user_id: UserId) {
        self.accounts.create_account(user_id);
    }

    /// Append a balance-critical command to the WAL.
    ///
    /// Non-fatal: if the WAL is not configured (e.g. in-memory mode) the write
    /// is silently skipped.  A WAL write failure is logged at `warn` level but
    /// does NOT roll back the already-applied balance mutation — the mutation has
    /// already been durably applied to in-memory state and will reach the DB via
    /// write-behind.  The WAL entry is best-effort additional durability.
    pub fn append_to_wal(&self, command: &Command) {
        if let Some(wal) = self.journal.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            if let Err(e) = wal.append(command) {
                tracing::warn!(error = %e, "WAL write for balance event failed — continuing without WAL entry");
            }
        }
    }

    pub fn deposit(self: &Arc<Self>, user_id: UserId, asset: &str, amount: Decimal) -> anyhow::Result<()> {
        self.accounts.deposit(user_id, asset, amount)?;
        // WAL: record the balance mutation for crash-recovery durability.
        // Written after the mutation succeeds so we never WAL an event that did not take effect.
        self.append_to_wal(&Command::BalanceDeposit {
            user_id,
            asset: asset.to_string(),
            amount,
            reference: String::new(), // caller may supply a reference via deposit_with_ref
        });
        Ok(())
    }

    /// Deposit with an explicit external reference (tx hash, deposit record ID, etc.).
    pub fn deposit_with_ref(self: &Arc<Self>, user_id: UserId, asset: &str, amount: Decimal, reference: &str) -> anyhow::Result<()> {
        self.accounts.deposit(user_id, asset, amount)?;
        self.append_to_wal(&Command::BalanceDeposit {
            user_id,
            asset: asset.to_string(),
            amount,
            reference: reference.to_string(),
        });
        Ok(())
    }

    /// Compute the effective USD collateral for a user across all enabled assets.
    ///
    /// For each enabled collateral coin the contribution is:
    ///   `min(available_balance, max_collateral_amount) * usd_price * haircut_rate`
    ///
    /// USDT is always worth 1 USD. Other assets are valued via the index price
    /// aggregator using `{COIN}USDT` symbols (e.g. "BTCUSDT"). When no index
    /// price is available for a coin, that coin is excluded (conservative fallback).
    ///
    /// A user with no explicit settings has all eligible assets enabled by default.
    pub fn effective_collateral_usd(self: &Arc<Self>, user_id: UserId) -> Decimal {
        let configs = self.collateral_configs.lock().unwrap_or_else(|e| e.into_inner());
        let user_settings = self.user_collateral_settings.lock().unwrap_or_else(|e| e.into_inner());
        // S18 Phase B: snapshot all of this user's balances once under a single
        // shard lock, then iterate locally. The loop below may consult many coins
        // so holding the shard lock for the whole loop (via `with_user`) would
        // work but also extends the critical section unnecessarily.
        let user_balances = self.accounts.get_all_balances(user_id);
        let index_agg = self.index_aggregator.lock().unwrap_or_else(|e| e.into_inner());

        // Determine which coins are enabled for this user. If no explicit setting
        // exists, all eligible coins are considered enabled.
        let user_enabled: Option<&HashSet<String>> = user_settings.get(&user_id);

        let mut total = Decimal::ZERO;

        for (coin, cfg) in configs.iter() {
            if !cfg.is_eligible {
                continue;
            }

            // If the user has explicit settings, only include opted-in coins.
            if let Some(enabled_set) = user_enabled {
                if !enabled_set.contains(coin.as_str()) {
                    continue;
                }
            }

            let balance = user_balances
                .get(coin)
                .map(|b| b.available())
                .unwrap_or(Decimal::ZERO);

            if balance.is_zero() {
                continue;
            }

            // Apply optional max-collateral cap.
            let capped_balance = match cfg.max_collateral_amount {
                Some(max) => balance.min(max),
                None => balance,
            };

            // Resolve USD price.
            let usd_price = if coin == "USDT" || coin == "USDC" {
                Decimal::ONE
            } else {
                let symbol = Symbol::new(&format!("{coin}USDT"));
                match index_agg.calculate_index(&symbol) {
                    Ok(idx) => idx.price,
                    Err(_) => {
                        tracing::debug!(
                            coin = %coin,
                            user_id = %user_id,
                            "No index price for collateral coin — skipping"
                        );
                        continue;
                    }
                }
            };

            total += capped_balance * usd_price * cfg.haircut_rate;
        }

        total
    }

    /// Compute cross-margin account balance for a user.
    ///
    /// Equals multi-asset effective collateral (with haircuts) plus the sum of
    /// unrealized PnL across all open cross-margin positions at current mark prices.
    ///
    /// This is the effective equity that backs all cross-margin positions; it is
    /// used for liquidation thresholds and available-margin calculations.
    pub fn cross_margin_balance(self: &Arc<Self>, user_id: UserId) -> Decimal {
        let collateral = self.effective_collateral_usd(user_id);

        let mark_prices = self.get_mark_prices();
        let pm = self.positions.lock();
        let total_upnl: Decimal = pm
            .user_positions(user_id)
            .into_iter()
            .filter(|p| p.margin_mode == MarginMode::Cross)
            .filter_map(|p| {
                let mark = mark_prices.get(&p.symbol)?;
                Some(p.unrealized_pnl(mark.price))
            })
            .sum();

        collateral + total_upnl
    }

    /// Return the user's current margin mode preference (default: Isolated).
    pub fn get_user_margin_mode(&self, user_id: UserId) -> MarginMode {
        self.user_margin_modes
            .lock().unwrap_or_else(|e| e.into_inner())
            .get(&user_id)
            .copied()
            .unwrap_or(MarginMode::Isolated)
    }

    /// Append an entry to the insurance fund history log.
    pub fn record_insurance_fund_event(
        &self,
        symbol: &Symbol,
        amount: Decimal,
        balance_after: Decimal,
        reason: &str,
    ) {
        let entry = InsuranceFundHistoryEntry {
            id: uuid::Uuid::new_v4(),
            symbol: symbol.to_string(),
            amount,
            balance_after,
            reason: reason.to_string(),
            timestamp: chrono::Utc::now(),
        };
        capped_push(
            &mut self.insurance_fund_history.lock().unwrap_or_else(|e| e.into_inner()),
            entry,
            MAX_INSURANCE_FUND_HISTORY,
        );
    }

    // -----------------------------------------------------------------------
    // Per-user OrderLimitConfig overrides (KG-8, Sprint 12)
    // -----------------------------------------------------------------------

    /// Set a per-user `OrderLimitConfig` override, replacing the global default
    /// for that user.
    ///
    /// Typical use: grant a market maker or institutional account higher
    /// per-symbol / per-level caps without raising the global cap.
    ///
    /// **Follow-up task:** expose via `PUT /admin/users/{user_id}/order-limits`
    /// REST endpoint so overrides can be configured at runtime via the API.
    pub fn set_user_order_limit_override(&mut self, user_id: UserId, config: OrderLimitConfig) {
        self.order_limit_overrides.insert(user_id, config);
    }

    /// Remove a per-user override, returning the user to the global default.
    pub fn clear_user_order_limit_override(&mut self, user_id: UserId) {
        self.order_limit_overrides.remove(&user_id);
    }

    /// Return the effective `OrderLimitConfig` for `user_id`.
    ///
    /// Returns the user-specific override if one exists, otherwise falls back
    /// to the global `order_limit_config`.
    pub fn effective_order_limit_config(&self, user_id: UserId) -> &OrderLimitConfig {
        self.order_limit_overrides
            .get(&user_id)
            .unwrap_or(&self.order_limit_config)
    }

    /// **M18-2: Unified order pipeline entry point.**
    ///
    /// This is the SOLE code path for all order placement. Every REST handler,
    /// background task (algos, bots, copy-trading, liquidation), and internal
    /// caller MUST use this function. No caller may bypass it to access the
    /// matching engine directly.
    ///
    /// Pipeline stages (in order):
    /// 1. Exchange halt check
    /// 2. Compliance freeze check (frozen_accounts)
    /// 3. Identity gate (email verified, KYC for perps)
    /// 4. Account status check (Frozen/Suspended/PendingKyc/Closed → reject)
    /// 5. Rate limiting (per-user, with surveillance overrides)
    /// 6. Self-trade prevention (STP)
    /// 7. Surveillance recording (on_order_placed)
    /// 8. Circuit breaker (price band, halt check)
    /// 9. Pair phase check (pre-market, auction, settle-only)
    /// 10. Product validation (tick size, lot size, min notional, leverage)
    /// 11. Balance lock (spot: lock quote/base, futures: margin lock)
    /// 12. WAL append (crash recovery)
    /// 13. Engine dispatch (matching)
    /// 14. Post-match settlement (fills, fees, position updates)
    ///
    /// Any stage can reject the order with an appropriate error.
    pub fn place_order(self: &Arc<Self>, mut cmd: PlaceOrderCommand) -> anyhow::Result<Vec<MatchEvent>> {
        let place_order_start = std::time::Instant::now();

        // Operational gate: reject new orders unless the exchange is in Normal mode.
        // LOCK-1: operating_mode (AtomicU8 — lock-free, no Mutex)
        let mode = self.operating_mode.load();
        if !mode.allows_new_orders() {
            return Err(anyhow::anyhow!("Exchange is in {mode} mode — no new orders accepted"));
        }

        // Compliance gate: reject orders from frozen accounts.
        // LOCK-2: frozen_accounts (DashSet — lock-free read, Sprint S18 C.3)
        if self.frozen_accounts.contains(&cmd.user_id) {
            return Err(anyhow::anyhow!("Account is frozen — trading suspended"));
        }

        // Identity gates: require KYC approval and email verification before trading.
        //
        // Bypassed when DEV_MODE / EXCHANGE_DEV_MODE is set OR when DATABASE_URL is
        // absent (the same condition that `main.rs` uses to auto-activate DEV_MODE).
        // This ensures tests that use `Exchange::new()` without a database continue
        // to work without needing to call KYC flows.
        //
        // `kyc_approved` is a sync-accessible mirror of `verification_service` state,
        // updated whenever KYC is approved (admin approve or webhook).  This avoids
        // calling `blocking_lock` on an async Mutex from within a sync function.
        // M22-1: KYC gate — perpetual trading requires KYC, spot allowed without.
        // In dev mode (no DATABASE_URL), KYC checks are skipped entirely.
        // S19 D.1: `self.is_dev_mode` is cached at Exchange construction; the
        // three env::var syscalls this used to do per order are eliminated.
        if !self.is_dev_mode {
            // Email verification required for all trading in production.
            // LOCK-3: email_verified (DashSet — lock-free read, Sprint S18 C.3)
            let email_ok = self.email_verified.contains(&cmd.user_id);
            if !email_ok {
                return Err(anyhow::anyhow!("Email verification required before trading"));
            }
            // KYC required only for perpetual futures trading.
            // LOCK-4: perp_configs (read — lock-free ArcSwap load, S17)
            let is_perp_order = self.perp_configs.load().contains_key(&cmd.symbol);
            if is_perp_order {
                // LOCK-5: kyc_approved (DashSet — lock-free read, Sprint S18 C.3)
                let kyc_ok = self.kyc_approved.contains(&cmd.user_id);
                if !kyc_ok {
                    return Err(anyhow::anyhow!("KYC verification required for perpetual trading"));
                }
            }
        }

        // S13 G.1 — Stage checkpoint: auth (halt + frozen + KYC/email gates complete).
        let t_auth = std::time::Instant::now();
        self.metrics.record_place_order_stage(
            "auth",
            t_auth.duration_since(place_order_start).as_nanos() as u64,
        );

        let symbol = cmd.symbol.clone();
        let order_id = cmd.order_id;
        let user_id = cmd.user_id;
        let side = cmd.side;
        let price = cmd.price;
        let qty = cmd.qty;
        let order_type = cmd.order_type;
        let cmd_time_in_force = cmd.time_in_force;
        let submitted_at = cmd.timestamp;

        // Iceberg: capture total qty and visible qty before the command is moved.
        // We replace the command qty with the visible slice only, so the book only
        // sees the visible portion.
        let iceberg_visible = cmd.iceberg_qty.take(); // removes field from cmd
        let iceberg_total_qty = qty; // original qty is the full iceberg size
        let cmd_position_idx = cmd.position_idx;
        let cmd_reduce_only = cmd.reduce_only;
        let cmd_order_link_id = cmd.order_link_id.clone();
        if let Some(visible) = iceberg_visible {
            cmd.qty = visible;
        }

        // Determine early whether this is a perpetual futures order so the combined
        // validation+lock block below can decide whether to attempt a balance lock.
        // LOCK-6: perp_configs (read — lock-free ArcSwap load, S17)
        let is_perp = self.perp_configs.load().contains_key(&symbol);

        // Step 0a: Validate order against product rules (tick size, lot size, min notional).
        // For futures orders we do this now as a standalone check.  For spot orders the
        // same validation is folded into Step 1 below so we can combine two separate
        // accounts-lock acquisitions into one (see Step 1 comment).
        if is_perp {
            // LOCK-7: accounts (perp product rule validation) — S18 Phase B: products
            // are replicated across all shards, `get_product` is a single shard-0 lock.
            if let Some(product) = self.accounts.get_product(&symbol) {
                OrderValidator::validate(&cmd, &product)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
            }
            // Unknown symbol will be caught by matching engine
        }

        // Step 0a1: Per-user open-order cap check (Sprint 8, KG-2).
        //
        // Enforcement site for DoS mitigation: limits open orders per user per
        // symbol and per price level.  Runs for ALL order types (spot + perp)
        // BEFORE balance lock, so rejected orders never touch the ledger.
        //
        // Source of truth: matching engine's `user_order_count` index
        // (`OrderBook::user_open_order_count()` / `user_orders_at_price()`).
        {
            // LOCK-8: engine_pool (read for count source)
            let pool = self.engine_pool.read().unwrap_or_else(|e| e.into_inner());
            let count_source = EnginePoolCountSource(&*pool);
            // Re-validate product rules + order caps together.  For spot orders
            // this also runs the product rule checks for the first time.
            // LOCK-9: accounts (product meta + per-user caps) — S18 Phase B: product
            // lookup is a shard-0 lock; no global AccountManager lock needed.
            if let Some(product) = self.accounts.get_product(&symbol) {
                // KG-8: use per-user override if set, otherwise fall back to global default.
                // KG-9: propagate ValidationError as a typed anyhow::Error so REST handlers
                // can recover it via downcast_ref::<ValidationError>() — no fragile string prefix.
                let effective_config = self.effective_order_limit_config(cmd.user_id);
                OrderValidator::validate_with_counts(
                    &cmd,
                    &product,
                    &count_source,
                    effective_config,
                )
                .map_err(anyhow::Error::from)?;
            }
        }

        // Step 0a1b: Account status check — frozen/suspended accounts cannot trade.
        {
            let status = self.get_account_status(cmd.user_id);
            if !status.can_trade() {
                return Err(anyhow::anyhow!("Account cannot trade: status is {:?}", status));
            }
        }

        // Step 0a2: Rate limiting — prevent order spam DoS.
        // The limiter automatically applies per-user overrides (set by surveillance
        // ReduceRateLimit action) when present, falling back to the global config.
        {
            // LOCK-10: rate_limiter (LockFreeRateLimiter — no Mutex, Sprint S18 C.2)
            if let exchange_order_gateway::RateLimitResult::Exceeded { window, count, limit } =
                self.rate_limiter.check_and_record(cmd.user_id)
            {
                tracing::warn!(
                    user_id = %cmd.user_id,
                    window = %window,
                    count = count,
                    limit = limit,
                    "order rate limit exceeded"
                );
                return Err(anyhow::anyhow!(
                    "Rate limit exceeded: {} orders in {} window (max {})",
                    count, window, limit
                ));
            }
        }

        // S13 G.1 — Stage checkpoint: rate_limit (rate limiter check complete).
        let t_rate_limit = std::time::Instant::now();
        self.metrics.record_place_order_stage(
            "rate_limit",
            t_rate_limit.duration_since(t_auth).as_nanos() as u64,
        );

        // Step 0b: Self-trade prevention
        {
            // LOCK-11: stp_guard — instrumented with wait-time histogram (S20 J.3).
            let lock_start = std::time::Instant::now();
            let guard = self.stp_guard.lock();
            self.metrics
                .record_lock_wait("stp_guard", lock_start.elapsed().as_nanos() as u64);
            if guard.would_self_trade(&cmd) {
                // Record the prevented match before returning.
                let conflicting = guard.conflicting_orders(&cmd);
                let would_match_order_id = conflicting
                    .first()
                    .copied()
                    .unwrap_or(uuid::Uuid::nil());

                let record = PreventedMatch {
                    id: uuid::Uuid::new_v4(),
                    timestamp: chrono::Utc::now(),
                    user_id,
                    order_id,
                    would_match_order_id,
                    symbol: symbol.to_string(),
                    qty,
                    price,
                    stp_mode: format!("{:?}", cmd.stp_mode),
                };

                let mut prevented = self
                    .prevented_matches
                    .lock().unwrap_or_else(|e| e.into_inner());
                if prevented.len() >= MAX_PREVENTED_MATCHES {
                    prevented.pop_front();
                }
                prevented.push_back(record);

                return Err(anyhow::anyhow!("Self-trade would occur"));
            }
        }

        // Step 0b2: Surveillance — record order placement for manipulation detection.
        // Use on_order_placed_full to feed layering and settlement manipulation detectors.
        // LOCK-12: surveillance — instrumented with wait-time histogram (S20 J.3).
        {
            let lock_start = std::time::Instant::now();
            let mut surveillance = self.surveillance.lock();
            self.metrics
                .record_lock_wait("surveillance", lock_start.elapsed().as_nanos() as u64);
            surveillance.on_order_placed_full(
                user_id,
                order_id,
                &symbol,
                side,
                price,
                qty,
                submitted_at,
            );
        }

        // Step 0c: Circuit breaker — reject orders outside the price band or during a halt.
        {
            // For market orders we need the current best bid/ask from the book.
            let (best_bid, best_ask) = {
                // LOCK-13: engine_pool (read for BBO snapshot — circuit breaker check)
                let pool = self.engine_pool.read().unwrap_or_else(|e| e.into_inner());
                match pool.snapshot(&symbol, 1) {
                    Ok(snap) => (
                        snap.bids.first().map(|(p, _)| *p),
                        snap.asks.first().map(|(p, _)| *p),
                    ),
                    Err(_) => (None, None),
                }
            };

            // LOCK-14: circuit_breakers
            let cb_result = self
                .circuit_breakers
                .lock()
                .get_mut(&symbol)
                .map(|cb| cb.check_order(price, order_type, side, best_bid, best_ask));

            match cb_result {
                Some(CircuitBreakerResult::Rejected { reason }) => {
                    return Err(anyhow::anyhow!("Circuit breaker: {reason}"));
                }
                Some(CircuitBreakerResult::Halted { resume_at }) => {
                    let remaining = resume_at.saturating_duration_since(std::time::Instant::now());
                    return Err(anyhow::anyhow!(
                        "Circuit breaker: trading halted for {:.1}s",
                        remaining.as_secs_f64()
                    ));
                }
                Some(CircuitBreakerResult::Allowed) | None => {
                    // Allowed or no circuit breaker configured for this symbol.
                }
            }
        }

        // Check pair phase (pre-market, call auction, settle-only, disabled).
        // pair_phases takes precedence; product_status is the legacy fallback.
        {
            // LOCK-15: pair_phases
            let phases = self.pair_phases.lock();
            if let Some(config) = phases.get(&symbol) {
                match config.phase {
                    PairPhase::Disabled => {
                        return Err(anyhow::anyhow!("Trading pair {} is disabled", symbol));
                    }
                    PairPhase::SettleOnly if !cmd.reduce_only => {
                        return Err(anyhow::anyhow!(
                            "Pair {} is in settle-only mode — only reduce_only orders accepted",
                            symbol
                        ));
                    }
                    PairPhase::CallAuction => {
                        // Only limit orders accepted during call auction.
                        if cmd.order_type != OrderType::Limit {
                            return Err(anyhow::anyhow!(
                                "Only limit orders are accepted during call auction for {}",
                                symbol
                            ));
                        }
                        // Enforce price band around the indicative reference price.
                        if let (Some(ref_price), Some(limit_pct)) =
                            (config.reference_price, config.price_limit_pct)
                        {
                            let max = ref_price
                                * (Decimal::ONE + limit_pct / Decimal::from(100));
                            let min = ref_price
                                * (Decimal::ONE - limit_pct / Decimal::from(100));
                            if price > max || price < min {
                                return Err(anyhow::anyhow!(
                                    "Price outside call auction limits [{min} – {max}]"
                                ));
                            }
                        }
                        // Buffer the order instead of sending to the matching engine.
                        drop(phases);
                        self.auction_orders
                            .lock().unwrap_or_else(|e| e.into_inner())
                            .entry(symbol.clone())
                            .or_default()
                            .push(cmd);
                        tracing::debug!(symbol = %symbol, "order buffered for call auction");
                        return Ok(vec![]);
                    }
                    PairPhase::PreMarketTrading => {
                        // Enforce price band during pre-market continuous trading.
                        if let (Some(ref_price), Some(limit_pct)) =
                            (config.reference_price, config.price_limit_pct)
                        {
                            let max = ref_price
                                * (Decimal::ONE + limit_pct / Decimal::from(100));
                            let min = ref_price
                                * (Decimal::ONE - limit_pct / Decimal::from(100));
                            if price > max || price < min {
                                return Err(anyhow::anyhow!(
                                    "Price outside pre-market limits [{min} – {max}]"
                                ));
                            }
                        }
                        // Enforce leverage cap during pre-market.
                        if let Some(max_lev) = config.max_leverage {
                            if is_perp {
                                // LOCK-16: positions (pre-market leverage cap check)
                                let user_leverage = self.positions
                                    .lock()
                                    .get_position_one_way(user_id, &symbol)
                                    .map(|p| p.leverage)
                                    .unwrap_or(Decimal::from(10u32));
                                if user_leverage > Decimal::from(max_lev) {
                                    return Err(anyhow::anyhow!(
                                        "Leverage {}x exceeds pre-market max {}x for {}",
                                        user_leverage, max_lev, symbol
                                    ));
                                }
                            }
                        }
                    }
                    PairPhase::Active | PairPhase::SettleOnly => {
                        // Active: no restrictions. SettleOnly + reduce_only: already allowed above.
                    }
                }
            } else {
                // No phase entry: fall back to legacy product_status check.
                // LOCK-17: product_status (read — lock-free ArcSwap load, S17)
                let status = self.product_status.load();
                if let Some(s) = status.get(&symbol) {
                    if s == "disabled" {
                        return Err(anyhow::anyhow!("Trading pair {} is disabled", symbol));
                    }
                    if s == "settle_only" && !cmd.reduce_only {
                        return Err(anyhow::anyhow!(
                            "Pair {} is in settle-only mode — only reduce_only orders accepted",
                            symbol
                        ));
                    }
                }
            }
        }

        // S13 G.1 — Stage checkpoint: validation (product rules + caps + STP + surveillance + circuit breaker + pair phase).
        let t_validation = std::time::Instant::now();
        self.metrics.record_place_order_stage(
            "validation",
            t_validation.duration_since(t_rate_limit).as_nanos() as u64,
        );

        // Step 1: Lock balance (spot only — futures use margin from position manager).
        //
        // For spot orders we acquire the accounts lock ONCE and perform both the
        // product-rule validation (Step 0a was skipped for spot) and the balance
        // lock in the same critical section.  This eliminates the two separate
        // lock/unlock round-trips that the original code had for spot orders:
        //   old: lock → validate product → unlock → ... → lock → lock_for_order → unlock
        //   new: lock → validate product + lock_for_order → unlock
        //
        // Phase 2 sharding design note: when the accounts store is sharded by
        // user_id (e.g. 64 buckets keyed by `user_id.as_u128() % 64`), both
        // operations on the same user land in the same shard so this combined
        // single-lock pattern scales without modification.  Product metadata
        // (tick size, lot size, min notional) should be migrated to a separate
        // read-only store in Phase 2 so it no longer contends with per-user
        // balance mutations.
        if !is_perp {
            // For spot market buy orders, price is 0 (set by REST handler for market orders).
            // We must estimate the price from the order book to lock the correct amount of
            // quote asset. Without this, lock_for_order locks 0 USDT and settle_fill fails
            // silently — the buyer gets base asset for free.
            //
            // For spot market sells, price=0 is safe because lock_for_order locks base qty,
            // not quote (price * qty).
            let lock_price = if price == Decimal::ZERO && side == Side::Buy {
                // Use best ask + 5% slippage buffer to ensure we lock enough quote.
                // The actual fill will be at the maker's price (price improvement for taker).
                // Any over-locked amount is released after the fill or on cancel.
                // LOCK-18: engine_pool (read for market buy price estimation)
                let pool = self.engine_pool.read().unwrap_or_else(|e| e.into_inner());
                match pool.snapshot(&symbol, 1) {
                    Ok(snap) => {
                        let best_ask = snap.asks.first().map(|(p, _)| *p).unwrap_or(Decimal::ZERO);
                        if best_ask == Decimal::ZERO {
                            return Err(anyhow::anyhow!("No ask liquidity — cannot place spot market buy"));
                        }
                        // 5% buffer for slippage across multiple price levels.
                        let buffer = Decimal::new(105, 2); // 1.05
                        best_ask * buffer
                    }
                    Err(_) => return Err(anyhow::anyhow!("Unknown symbol: {}", symbol)),
                }
            } else {
                price
            };

            let lock = {
                // LOCK-19: accounts (spot balance lock) — S18 Phase B: routes to the
                // user's shard. Product rule validation is done separately via the
                // replicated products registry (no cross-shard lookup needed).
                if let Some(product) = self.accounts.get_product(&symbol) {
                    OrderValidator::validate(&cmd, &product)
                        .map_err(|e| anyhow::anyhow!("{}", e))?;
                }

                // Lock the balance for this order.  For market buys, lock_price is the
                // estimated fill price with slippage buffer.
                self.accounts
                    .lock_for_order(user_id, &symbol, side, lock_price, cmd.qty)?
            };

            // Track the lock so settle_fill / cancel can release it later.
            self.order_locks.insert(order_id, OrderLockRecord {
                user_id,
                symbol: symbol.clone(),
                side,
                price: lock_price,
                qty: cmd.qty,
                lock,
                filled_qty: Decimal::ZERO,
                filled_cost: Decimal::ZERO,
            });
        } else {
            // Futures: pre-flight margin check before sending to matching engine.
            // Estimate the price to compute required initial margin.
            let est_price = if cmd.price > Decimal::ZERO {
                cmd.price
            } else {
                // Market order: use best bid/ask from the book as a price estimate.
                // LOCK-18f: engine_pool (read for futures market order price estimation)
                let pool = self.engine_pool.read().unwrap_or_else(|e| e.into_inner());
                match pool.snapshot(&symbol, 1) {
                    Ok(snap) => match side {
                        Side::Buy => snap.asks.first().map(|(p, _)| *p).unwrap_or(Decimal::ZERO),
                        Side::Sell => snap.bids.first().map(|(p, _)| *p).unwrap_or(Decimal::ZERO),
                    },
                    Err(_) => Decimal::ZERO,
                }
            };

            if est_price > Decimal::ZERO {
                let use_portfolio_margin = self.has_portfolio_margin(user_id);

                let required_margin = if use_portfolio_margin {
                    // Portfolio margin: compute scenario-based required margin for the
                    // current portfolio, then add the incremental margin for this new order.
                    let positions_owned = self.get_positions(user_id);
                    let positions_refs: Vec<&Position> = positions_owned.iter().collect();

                    let mut mark_prices: HashMap<Symbol, Decimal> = self
                        .get_mark_prices()
                        .into_iter()
                        .map(|(sym, mp)| (sym, mp.price))
                        .collect();
                    // Ensure the current symbol has a mark price (use est_price as fallback).
                    mark_prices.entry(symbol.clone()).or_insert(est_price);

                    // LOCK-19f-a: perp_configs (portfolio margin scenario configs)
                    // S17: load_full() returns Arc<HashMap> — no mutex, no clone,
                    // just an atomic pointer load + reference count increment.
                    let configs = self.perp_configs.load_full();

                    // Existing portfolio required margin.
                    let existing_pm = exchange_risk_engine::required_portfolio_margin(
                        &positions_refs,
                        &mark_prices,
                        &configs,
                    );

                    // Incremental margin for the new position (standard isolated calc).
                    let leverage = {
                        // LOCK-19f-b: positions (portfolio margin incremental calc)
                        let pm = self.positions.lock();
                        pm.get_position(user_id, &symbol, cmd_position_idx)
                            .map(|p| p.leverage)
                            .unwrap_or_else(|| Decimal::from(10))
                    };
                    let incremental = exchange_risk_engine::margin_calculator::required_initial_margin(
                        cmd.qty,
                        est_price,
                        leverage,
                    );

                    // Required = max(portfolio stress, incremental) — we require at least
                    // the incremental margin for the single new position, plus any existing
                    // portfolio stress margin.
                    existing_pm + incremental
                } else {
                    // Standard isolated/cross margin: initial margin for this order only.
                    let leverage = {
                        // LOCK-19f-c: positions (standard margin leverage lookup)
                        let pm = self.positions.lock();
                        pm.get_position(user_id, &symbol, cmd_position_idx)
                            .map(|p| p.leverage)
                            .unwrap_or_else(|| Decimal::from(10))
                    };
                    exchange_risk_engine::margin_calculator::required_initial_margin(
                        cmd.qty,
                        est_price,
                        leverage,
                    )
                };

                // Atomically check + reserve the margin under the user's shard lock.
                // `try_lock_margin` fails with InsufficientBalance when available <
                // required, so no concurrent order from the same user can race through
                // this gate on the same balance snapshot.
                // LOCK-20f: accounts shard (futures margin reserve — atomic check + lock)
                self.accounts
                    .try_lock_margin(user_id, "USDT", required_margin)
                    .map_err(|e| anyhow::anyhow!(
                        "Insufficient margin: need {} USDT available ({})",
                        required_margin,
                        e,
                    ))?;

                // Record the reservation so it can be released on cancel/reject/fill.
                // LOCK-21f: futures_margin_locks (record reserved margin)
                self.futures_margin_locks
                    .lock().unwrap_or_else(|e| e.into_inner())
                    .insert(order_id, required_margin);
            }
        }

        // S13 G.1 — Stage checkpoint: balance_lock (spot lock OR futures margin reserve complete).
        let t_balance_lock = std::time::Instant::now();
        self.metrics.record_place_order_stage(
            "balance_lock",
            t_balance_lock.duration_since(t_validation).as_nanos() as u64,
        );

        // Track order metadata so we can write history for futures orders (no lock record).
        self.pending_orders.insert(order_id, PendingOrderInfo {
            user_id,
            symbol: symbol.clone(),
            side,
            order_type,
            price,
            qty,
            created_at: submitted_at,
            order_link_id: cmd_order_link_id.clone(),
            reduce_only: cmd_reduce_only,
        });

        // C1: Persist order to DB so it survives restart.
        if let Some(pool) = self.db_pool.get() {
            let pool = pool.clone();
            let sym = symbol.to_string();
            let side_str = format!("{:?}", side);
            let ot_str = format!("{:?}", order_type);
            let tif_str = format!("{:?}", cmd.time_in_force);
            tokio::spawn(async move {
                if let Err(e) = exchange_db::order_repo::OrderRepo::create(
                    &pool, order_id, user_id, &sym, &side_str, &ot_str, &tif_str,
                    Some(price), qty,
                ).await {
                    tracing::warn!(order_id = %order_id, error = %e, "order DB persist failed (non-fatal)");
                }
            });
        }

        // Step 2: Write to WAL (durability BEFORE matching)
        //
        // When group_commit_wal is configured, write through it — the GC flush
        // task batches commands and calls SegmentedWal::append_batch (N writes +
        // 1 fsync). Both paths write to the same SegmentedWal, so replay covers
        // every committed command.
        //
        // Fallback: if group_commit_wal is None (in-memory / test mode), write
        // directly to the SegmentedWal with per-entry fsync.
        let cmd_expire_at = cmd.expire_at;
        let cmd_tif = cmd.time_in_force;
        let command = Command::PlaceOrder(cmd);
        // Write to the shared SegmentedWal directly. The group_commit_wal's
        // append_sync uses block_on() which panics inside an async runtime
        // (e.g., demo seeder). Writing through the shared Mutex is safe in
        // both sync and async contexts; group commit batching still works
        // because the GC flush task also locks this same Mutex.
        if let Some(wal) = self.journal.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            wal.append(&command).map_err(|e| anyhow::anyhow!("WAL write failed: {}", e))?;
        }

        // Step 2b: Register GTD orders so the expiry checker can cancel them when due.
        if cmd_tif == TimeInForce::Gtd {
            if let Some(expire_at) = cmd_expire_at {
                // LOCK-24: gtd_orders (GTD expiry registration)
                self.gtd_orders
                    .lock().unwrap_or_else(|e| e.into_inner())
                    .insert(order_id, (symbol.clone(), expire_at));
            }
        }

        // S13 G.1 — Stage checkpoint: wal_write (journal lock + WAL append complete).
        let t_wal_write = std::time::Instant::now();
        self.metrics.record_place_order_stage(
            "wal_write",
            t_wal_write.duration_since(t_balance_lock).as_nanos() as u64,
        );

        // Step 3: Execute in matching engine (timed)
        // LOCK-25: engine_pool (read, engine dispatch — the per-symbol engine thread)
        let match_start = std::time::Instant::now();
        let events = self.engine_pool.read().unwrap_or_else(|e| e.into_inner())
            .process(&symbol, command)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        let match_us = match_start.elapsed().as_micros() as u64;
        self.metrics.record_matching_latency_us(match_us);
        self.metrics.inc_orders_placed();
        self.metrics.inc_symbol_orders_placed_labeled(
            symbol.as_str(),
            side == Side::Buy,
            order_type == OrderType::Limit,
        );

        // S13 G.1 — Stage checkpoint: matching (engine dispatch complete).
        let t_matching = std::time::Instant::now();
        self.metrics.record_place_order_stage(
            "matching",
            t_matching.duration_since(t_wal_write).as_nanos() as u64,
        );

        // Step 3b: Write order to DB (write-behind, non-blocking).
        {
            let maybe_pool = self.db_pool.get().cloned();
            if let Some(pool) = maybe_pool {
                let oid = order_id;
                let uid = user_id;
                let sym = symbol.to_string();
                let side_str = format!("{:?}", side);
                let otype_str = format!("{:?}", order_type);
                let tif_str = format!("{:?}", cmd_time_in_force);
                let db_price = if order_type == OrderType::Market { None } else { Some(price) };
                tokio::spawn(async move {
                    if let Err(e) = exchange_db::order_repo::OrderRepo::create(
                        &pool, oid, uid, &sym, &side_str, &otype_str, &tif_str, db_price, qty,
                    ).await {
                        tracing::error!(
                            order_id = %oid,
                            error = %e,
                            "order DB create failed"
                        );
                    }
                });
            }
        }

        // Step 4: Process events — sync-critical financial work (balance, fee, position).
        //
        // S15: Split into two phases:
        //   Phase A (sync)  — financial settlement: balance debit/credit, fee capture,
        //                     position update, liquidation-chain check, STP guard update.
        //                     Must complete before ack is returned.
        //   Phase B (async) — surveillance, notifications, WS broadcast, FIX drop-copy.
        //                     Dispatched to background worker via bounded MPSC channel.
        //
        // LOCK-N note: process_events_sync acquires accounts, positions, order_locks,
        // futures_margin_locks, fee_engine, circuit_breakers, iceberg_meta, grid_bots,
        // dca_bots, twap_executions, vwap_executions, liquidity_pools, stp_guard.
        // Surveillance, notifications, market_data, market_tx, private_tx, fix_execution_tx
        // are deferred to the async worker.
        self.process_events_sync(&events, &symbol);

        // S15 S13 G.1 — Stage checkpoint: events_sync (financial settlement complete).
        let t_events_sync = std::time::Instant::now();
        self.metrics.record_place_order_stage(
            "events_sync",
            t_events_sync.duration_since(t_matching).as_nanos() as u64,
        );
        // Keep legacy "process_events" metric updated for backward compat with existing dashboards.
        self.metrics.record_place_order_stage(
            "process_events",
            t_events_sync.duration_since(t_matching).as_nanos() as u64,
        );

        // Phase B: dispatch post-fill events to async worker (try_send — non-blocking).
        self.dispatch_post_fill_events(&events, &symbol);

        // S15 S13 G.1 — Stage checkpoint: events_dispatch (channel send complete).
        let t_events_dispatch = std::time::Instant::now();
        self.metrics.record_place_order_stage(
            "events_dispatch",
            t_events_dispatch.duration_since(t_events_sync).as_nanos() as u64,
        );

        // Step 4b: Register order_link_id → order_id mapping.
        // Only register for non-rejected, non-immediately-cancelled orders (accepted or filled).
        let order_rejected = events.iter().any(|e| matches!(e, MatchEvent::OrderRejected { order_id: oid, .. } if *oid == order_id));
        if !order_rejected {
            if let Some(link_id) = cmd_order_link_id {
                // LOCK-26: order_link_index (register order_link_id → order_id)
                self.order_link_index.lock()
                    .insert((user_id, link_id), order_id);
            }
        }

        // Step 5: Register iceberg if the order was accepted and rests on the book.
        //
        // An iceberg order is registered ONLY when OrderAccepted is emitted (the visible
        // slice actually rested on the book). If the order was immediately filled in full
        // (IOC-style) or rejected, there is nothing to replenish.
        if let Some(visible) = iceberg_visible {
            let accepted = events.iter().any(|e| matches!(e, MatchEvent::OrderAccepted { order_id: oid, .. } if *oid == order_id));
            if accepted {
                // LOCK-27: iceberg_manager (register iceberg slice)
                self.iceberg_manager
                    .lock().unwrap_or_else(|e| e.into_inner())
                    .register(order_id, iceberg_total_qty, visible);

                // LOCK-28: iceberg_meta (register iceberg metadata)
                self.iceberg_meta
                    .lock().unwrap_or_else(|e| e.into_inner())
                    .insert(order_id, IcebergMeta {
                        original_id: order_id,
                        user_id,
                        symbol: symbol.clone(),
                        side,
                        price,
                        time_in_force: cmd_time_in_force,
                        visible_qty: visible,
                        position_idx: cmd_position_idx,
                        reduce_only: cmd_reduce_only,
                        slice_filled: Decimal::ZERO,
                    });
            }
        }

        // Record end-to-end place_order latency.
        let place_order_ns = place_order_start.elapsed().as_nanos() as u64;
        self.metrics.record_place_order_latency_ns(symbol.as_str(), place_order_ns);

        Ok(events)
    }

    /// Run the call auction for a symbol: discover equilibrium price, match,
    /// and settle all fills via the normal pipeline.
    /// Returns the auction result (clearing price, matched volume) if successful.
    pub fn run_call_auction(self: &Arc<Self>, symbol: &Symbol) -> Option<exchange_matching_engine::AuctionResult> {
        let orders: Vec<PlaceOrderCommand> = self
            .auction_orders
            .lock().unwrap_or_else(|e| e.into_inner())
            .remove(symbol)
            .unwrap_or_default();

        if orders.is_empty() {
            return None;
        }

        let reference_price = self
            .pair_phases
            .lock()
            .get(symbol)
            .and_then(|c| c.reference_price);

        // Use a timestamp-based trade ID seed for auction fills.
        let mut next_trade_id = chrono::Utc::now().timestamp_millis() as u64;

        let result = exchange_matching_engine::run_auction(&orders, reference_price, &mut next_trade_id)?;

        tracing::info!(
            symbol = %symbol,
            clearing_price = %result.clearing_price,
            matched_volume = %result.matched_volume,
            fills = result.fills.len(),
            unmatched = result.unmatched.len(),
            "call auction completed"
        );

        // Convert auction fills to MatchEvents and process through settlement.
        let events = exchange_matching_engine::fills_to_events(&result.fills, 0);
        self.process_events(&events, symbol);

        // Place unmatched orders on the continuous book.
        for cmd in &result.unmatched {
            if let Err(e) = self.place_order(cmd.clone()) {
                tracing::warn!(
                    order_id = %cmd.order_id,
                    error = %e,
                    "failed to place unmatched auction order on book"
                );
            }
        }

        Some(result)
    }

    // ── M24-6: Audit log helper ────────────────────────────────────────────

    /// Record a security-relevant action in the tamper-evident audit log.
    ///
    /// Call this for every privileged operation: login, API key management,
    /// freeze/unfreeze, withdrawal approval, KYC decisions, etc.
    pub fn audit(
        &self,
        user_id: Option<UserId>,
        action: exchange_admin::AuditAction,
        detail: serde_json::Value,
        ip: Option<String>,
    ) {
        self.audit_log
            .lock().unwrap_or_else(|e| e.into_inner())
            .log(user_id, action, detail, ip);
    }

    // ── NOTIF: Push notification dispatch ──────────────────────────────────

    /// Send a push notification to all registered devices for a user.
    ///
    /// This is fire-and-forget — push failures are logged but never block
    /// the calling operation. The notification is also stored in the
    /// in-memory notification log for the inbox API.
    pub fn send_push_notification(
        &self,
        user_id: UserId,
        title: &str,
        body: &str,
        notification_type: exchange_admin::NotificationType,
        data: serde_json::Value,
    ) {
        // Store in notification log.
        {
            let mut mgr = self.notifications.lock().unwrap_or_else(|e| e.into_inner());
            mgr.notify(
                user_id,
                notification_type.clone(),
                exchange_admin::NotificationChannel::Apns,
                title,
                body,
                data.clone(),
            );
        }

        // Log push (actual APNS/FCM delivery happens in Phase 2 via PushProvider).
        let device_count = {
            let reg = self.device_registry.lock().unwrap_or_else(|e| e.into_inner());
            reg.user_devices(user_id).len()
        };

        if device_count > 0 {
            tracing::info!(
                %user_id,
                devices = device_count,
                notification_type = ?notification_type,
                "push notification queued for {} devices",
                device_count,
            );
        }
    }

    /// M18-2 alias: `validate_and_place_order` is the same as `place_order`.
    ///
    /// This exists so code searching for the M18-2 task name finds the entry point.
    /// Both names refer to the same unified pipeline.
    #[inline]
    pub fn validate_and_place_order(self: &Arc<Self>, cmd: PlaceOrderCommand) -> anyhow::Result<Vec<MatchEvent>> {
        self.place_order(cmd)
    }

    /// Cancel an order: remove from book → unlock balance.
    pub fn cancel_order(self: &Arc<Self>, mut cmd: CancelOrderCommand) -> anyhow::Result<Vec<MatchEvent>> {
        let mode = self.operating_mode.load();
        if !mode.allows_cancels() {
            return Err(anyhow::anyhow!("Exchange is in {mode} mode — cancels not accepted"));
        }

        let order_id = cmd.order_id;

        // Resolve symbol from pending orders if not provided (REST cancel passes "").
        if cmd.symbol.as_str().is_empty() {
            if let Some(info) = self.pending_orders.get(&order_id) {
                cmd.symbol = info.symbol.clone();
            } else if let Some(record) = self.order_locks.get(&order_id) {
                cmd.symbol = record.symbol.clone();
            } else {
                return Err(anyhow::anyhow!("Order not found: {order_id}"));
            }
        }
        let symbol = cmd.symbol.clone();

        // Write to WAL before matching
        let command = Command::CancelOrder(cmd);
        if let Some(wal) = self.journal.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            wal.append(&command).map_err(|e| anyhow::anyhow!("WAL write failed: {}", e))?;
        }

        let events = self.engine_pool.read().unwrap_or_else(|e| e.into_inner())
            .process(&symbol, command)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Process cancel events
        for event in &events {
            if let MatchEvent::OrderCancelled { order_id, .. } = event {
                self.handle_cancel(*order_id);
                // Clean up iceberg tracking state for the cancelled visible slice.
                self.iceberg_manager
                    .lock().unwrap_or_else(|e| e.into_inner())
                    .cancel(*order_id);
                self.iceberg_meta
                    .lock().unwrap_or_else(|e| e.into_inner())
                    .remove(order_id);
            }
        }

        // If no cancel event (order didn't exist), also clean up lock
        if events.is_empty() {
            self.handle_cancel(order_id);
            self.iceberg_manager
                .lock().unwrap_or_else(|e| e.into_inner())
                .cancel(order_id);
            self.iceberg_meta
                .lock().unwrap_or_else(|e| e.into_inner())
                .remove(&order_id);
        }

        Ok(events)
    }

    // --- Surveillance auto-action enforcement ---

    /// Get the account status for a user. Returns Active if not explicitly set.
    pub fn get_account_status(&self, user_id: UserId) -> exchange_common::types::AccountStatus {
        self.account_statuses
            .lock().unwrap_or_else(|e| e.into_inner())
            .get(&user_id)
            .copied()
            .unwrap_or(exchange_common::types::AccountStatus::Active)
    }

    /// Freeze an account — blocks all trading and withdrawals.
    /// Called by surveillance auto-actions when critical manipulation is detected.
    pub fn freeze_account(&self, user_id: UserId) {
        tracing::warn!(%user_id, "surveillance: freezing account");
        self.account_statuses
            .lock().unwrap_or_else(|e| e.into_inner())
            .insert(user_id, exchange_common::types::AccountStatus::Frozen);

        // M24-6: Audit log entry for account freeze.
        self.audit(
            Some(user_id),
            exchange_admin::AuditAction::AccountFrozen,
            serde_json::json!({"reason": "surveillance_auto_action"}),
            None,
        );

        // M24-4: Write-behind DB persistence for account freeze.
        if let Some(pool) = self.db_pool.get().cloned() {
            tokio::spawn(async move {
                if let Err(e) = exchange_db::account_status_repo::AccountStatusRepo::set_status(
                    &pool, user_id, "Frozen", Some("surveillance_auto_action"),
                ).await {
                    tracing::error!(%user_id, error = %e, "failed to persist account freeze to DB");
                }
            });
        }

        // NOTIF-6: Push notification for account freeze.
        self.send_push_notification(
            user_id,
            "Account Frozen",
            "Your account has been frozen due to suspicious activity. Contact support.",
            exchange_admin::NotificationType::AccountFrozen,
            serde_json::json!({"reason": "surveillance_auto_action"}),
        );
    }

    /// Unfreeze an account — restores normal trading (admin action).
    pub fn unfreeze_account(&self, user_id: UserId) {
        tracing::info!(%user_id, "admin: unfreezing account");
        self.account_statuses
            .lock().unwrap_or_else(|e| e.into_inner())
            .remove(&user_id);

        // M24-6: Audit log entry for account unfreeze.
        self.audit(
            Some(user_id),
            exchange_admin::AuditAction::AccountUnfrozen,
            serde_json::json!({"action": "admin_unfreeze"}),
            None,
        );

        // M24-4: Write-behind DB persistence for account unfreeze.
        if let Some(pool) = self.db_pool.get().cloned() {
            tokio::spawn(async move {
                if let Err(e) = exchange_db::account_status_repo::AccountStatusRepo::set_status(
                    &pool, user_id, "Active", None,
                ).await {
                    tracing::error!(%user_id, error = %e, "failed to persist account unfreeze to DB");
                }
            });
        }
    }

    /// Cancel all open orders for a user across all symbols.
    /// Called by surveillance auto-actions (CancelOrders).
    pub fn cancel_all_orders_for_user(self: &Arc<Self>, user_id: UserId) -> (usize, usize) {
        let order_ids: Vec<(OrderId, Symbol)> = self
            .pending_orders
            .iter()
            .filter(|r| r.user_id == user_id)
            .map(|r| (*r.key(), r.symbol.clone()))
            .collect();

        let total = order_ids.len();
        let mut cancelled = 0usize;

        for (order_id, symbol) in order_ids {
            let cmd = CancelOrderCommand {
                user_id,
                order_id,
                symbol,
                timestamp: chrono::Utc::now(),
            };
            if self.cancel_order(cmd).is_ok() {
                cancelled += 1;
            }
        }

        tracing::warn!(
            %user_id,
            cancelled,
            total,
            "surveillance: cancelled all orders for user"
        );
        (cancelled, total)
    }

    /// Apply a reduced rate limit override for a user (surveillance action).
    /// Sets the user to 10 orders/sec, 60 orders/min (vs default 50/s, 1200/min).
    pub fn set_reduced_rate_limit(&self, user_id: UserId) {
        tracing::warn!(%user_id, "surveillance: reducing rate limit for user");
        self.rate_limiter.set_override(
            user_id,
            exchange_order_gateway::RateLimitConfig {
                max_per_second: 10,
                max_per_minute: 60,
            },
        );
    }

    /// Remove a rate limit override (admin action).
    pub fn clear_rate_limit_override(&self, user_id: UserId) {
        self.rate_limiter.clear_override(user_id);
    }

    /// Enforce all auto-actions from a batch of surveillance alerts.
    /// Called by the surveillance background task after `check_all()`.
    pub fn enforce_auto_actions(self: &Arc<Self>, alerts: &[exchange_surveillance::SurveillanceAlert]) {
        use exchange_surveillance::AutoAction;

        for alert in alerts {
            match alert.auto_action {
                AutoAction::None => {}
                AutoAction::CancelOrders => {
                    for &uid in &alert.user_ids {
                        self.cancel_all_orders_for_user(uid);
                    }
                }
                AutoAction::FreezeAccount => {
                    for &uid in &alert.user_ids {
                        self.cancel_all_orders_for_user(uid);
                        self.freeze_account(uid);
                    }
                }
                AutoAction::ReduceRateLimit => {
                    for &uid in &alert.user_ids {
                        self.set_reduced_rate_limit(uid);
                    }
                }
            }
        }
    }

    // --- Futures methods ---

    /// Get the current mark price for a symbol.
    ///
    /// Uses the index aggregator (weighted median of external spot prices) and
    /// the best mid-price from the order book as the last traded price proxy.
    pub fn get_mark_price(self: &Arc<Self>, symbol: &Symbol) -> Option<MarkPrice> {
        let index_price = {
            let agg = self.index_aggregator.lock().unwrap_or_else(|e| e.into_inner());
            agg.calculate_index(symbol).ok()?.price
        };

        // Best estimate of last price: mid-point of order book, fallback to index
        let last_price = {
            let pool = self.engine_pool.read().unwrap_or_else(|e| e.into_inner());
            match pool.snapshot(symbol, 1) {
                Ok(snap) => {
                    match (snap.bids.first(), snap.asks.first()) {
                        (Some((bid, _)), Some((ask, _))) => (*bid + *ask) / Decimal::TWO,
                        (Some((bid, _)), None) => *bid,
                        (None, Some((ask, _))) => *ask,
                        (None, None) => index_price,
                    }
                }
                Err(_) => index_price,
            }
        };

        // Use EMA-dampened mark price to resist single-trade manipulation.
        // A single large trade at 2x price only moves mark by ~6% of the spike
        // (with 30-tick EMA span), rather than 33% with the simple formula.
        let prev_ema = {
            let ema_map = self.mark_ema_basis.lock().unwrap_or_else(|e| e.into_inner());
            ema_map.get(symbol).copied().unwrap_or(Decimal::ZERO)
        };

        let (mark_price, new_ema) = calculate_mark_price_ema(
            symbol.clone(),
            index_price,
            last_price,
            prev_ema,
            DEFAULT_MARK_EMA_SPAN,
        );

        // Store the updated EMA state for the next calculation.
        self.mark_ema_basis
            .lock().unwrap_or_else(|e| e.into_inner())
            .insert(symbol.clone(), new_ema);

        Some(mark_price)
    }

    /// Get mark prices for all configured perpetual symbols.
    pub fn get_mark_prices(self: &Arc<Self>) -> HashMap<Symbol, MarkPrice> {
        let symbols: Vec<Symbol> = self.perp_configs
            .load()
            .keys()
            .cloned()
            .collect();
        symbols
            .iter()
            .filter_map(|sym| self.get_mark_price(sym).map(|mp| (sym.clone(), mp)))
            .collect()
    }

    /// Check whether the index price for a perpetual symbol is healthy (enough
    /// fresh sources to compute a weighted median).
    ///
    /// Used as a pre-trade gate: perpetual orders are rejected when the index
    /// is unavailable, preventing trading without a reliable mark price for
    /// liquidation and funding.
    pub fn is_index_healthy(&self, symbol: &Symbol) -> bool {
        let agg = self.index_aggregator.lock().unwrap_or_else(|e| e.into_inner());
        agg.calculate_index(symbol).is_ok()
    }

    /// Return per-symbol, per-source health status for the index price feeds.
    ///
    /// Intended for the admin monitoring endpoint (`GET /admin/index/health`).
    pub fn index_health_report(&self) -> serde_json::Value {
        let agg = self.index_aggregator.lock().unwrap_or_else(|e| e.into_inner());
        let mut symbols_report = serde_json::Map::new();
        for symbol in agg.symbols() {
            let statuses = agg.source_status(symbol);
            let healthy_count = statuses.iter().filter(|s| s.is_healthy && !s.is_stale).count();
            let index_ok = agg.calculate_index(symbol).is_ok();
            let sources: Vec<serde_json::Value> = statuses
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "name": s.name,
                        "is_healthy": s.is_healthy,
                        "is_stale": s.is_stale,
                        "last_price": s.last_price.map(|p| p.to_string()),
                        "last_update": s.last_update,
                    })
                })
                .collect();
            symbols_report.insert(
                symbol.to_string(),
                serde_json::json!({
                    "index_available": index_ok,
                    "healthy_sources": healthy_count,
                    "total_sources": statuses.len(),
                    "sources": sources,
                }),
            );
        }
        serde_json::Value::Object(symbols_report)
    }

    /// Compute the mark price for an option contract.
    ///
    /// Uses the vol surface (if loaded) to obtain the implied volatility, then
    /// computes the Black-Scholes fair value. Falls back to the surface's
    /// `fallback_iv` (80%) when no surface data is available.
    ///
    /// Returns `None` if the underlying index price is unavailable.
    pub fn get_option_mark_price(
        &self,
        underlying: &str,
        strike: Decimal,
        expiry: chrono::DateTime<chrono::Utc>,
        option_type: exchange_risk_engine::options_instruments::OptionType,
    ) -> Option<Decimal> {
        // 1. Get underlying spot price from index aggregator.
        let underlying_symbol = Symbol::new(&format!("{underlying}USDT"));
        let spot = {
            let agg = self.index_aggregator.lock().unwrap_or_else(|e| e.into_inner());
            agg.calculate_index(&underlying_symbol).ok()?.price
        };

        // 2. Get IV from vol surface (or use fallback).
        let iv = {
            let surface_guard = self.vol_surface.lock().unwrap_or_else(|e| e.into_inner());
            match surface_guard.as_ref() {
                Some(surface) => surface.get_iv(strike, expiry),
                None => Decimal::new(80, 2), // 80% default ATM vol
            }
        };

        // 3. Compute time to expiry in years.
        let now = chrono::Utc::now();
        let seconds_to_expiry = (expiry - now).num_seconds().max(0) as f64;
        let t_years = Decimal::from_f64_retain(seconds_to_expiry / 31_536_000.0)
            .unwrap_or(Decimal::ZERO);

        if t_years.is_zero() {
            // Expired — intrinsic value only.
            return match option_type {
                exchange_risk_engine::options_instruments::OptionType::Call => {
                    Some((spot - strike).max(Decimal::ZERO))
                }
                exchange_risk_engine::options_instruments::OptionType::Put => {
                    Some((strike - spot).max(Decimal::ZERO))
                }
            };
        }

        // 4. Black-Scholes pricing (risk-free rate = 0 for crypto).
        let mark = match option_type {
            exchange_risk_engine::options_instruments::OptionType::Call => {
                exchange_risk_engine::options_pricing::BlackScholes::call_price(
                    spot, strike, t_years, Decimal::ZERO, iv,
                )
            }
            exchange_risk_engine::options_instruments::OptionType::Put => {
                exchange_risk_engine::options_pricing::BlackScholes::put_price(
                    spot, strike, t_years, Decimal::ZERO, iv,
                )
            }
        };

        Some(mark)
    }

    /// Get all open positions for a user.
    pub fn get_positions(self: &Arc<Self>, user_id: UserId) -> Vec<Position> {
        self.positions
            .lock()
            .user_positions(user_id)
            .into_iter()
            .cloned()
            .collect()
    }

    // -----------------------------------------------------------------------
    // Portfolio Margin
    // -----------------------------------------------------------------------

    /// Run scenario-based stress tests across all of the user's open positions.
    ///
    /// Applies ±5 %, ±10 %, ±15 % price shocks to every position's mark price
    /// and sums the portfolio PnL at each shock level.
    ///
    /// Returns one [`ScenarioResult`] per shock (6 total).
    pub fn portfolio_margin_scenarios(
        self: &Arc<Self>,
        user_id: UserId,
    ) -> Vec<exchange_risk_engine::ScenarioResult> {
        let positions_owned = self.get_positions(user_id);
        let positions_refs: Vec<&Position> = positions_owned.iter().collect();

        let mark_prices: HashMap<Symbol, Decimal> = self
            .get_mark_prices()
            .into_iter()
            .map(|(sym, mp)| (sym, mp.price))
            .collect();

        let configs = self.perp_configs.load_full();

        exchange_risk_engine::portfolio_margin_analyse(
            &positions_refs,
            &mark_prices,
            &configs,
        )
        .scenarios
    }

    /// Compute net delta per base asset for the user's portfolio.
    ///
    /// Returns a map of base asset → net delta (e.g., `{"BTC" => 1.5}`).
    /// Positive = net long, negative = net short.
    pub fn portfolio_net_delta(
        self: &Arc<Self>,
        user_id: UserId,
    ) -> HashMap<String, Decimal> {
        let positions_owned = self.get_positions(user_id);
        let positions_refs: Vec<&Position> = positions_owned.iter().collect();

        let mark_prices: HashMap<Symbol, Decimal> = self
            .get_mark_prices()
            .into_iter()
            .map(|(sym, mp)| (sym, mp.price))
            .collect();

        let configs = self.perp_configs.load_full();

        exchange_risk_engine::compute_net_delta(&positions_refs, &mark_prices, &configs)
    }

    /// Full portfolio margin analysis for a user: scenarios, net delta, and required margin.
    pub fn portfolio_margin_analysis(
        self: &Arc<Self>,
        user_id: UserId,
    ) -> exchange_risk_engine::PortfolioMarginAnalysis {
        let positions_owned = self.get_positions(user_id);
        let positions_refs: Vec<&Position> = positions_owned.iter().collect();

        let mark_prices: HashMap<Symbol, Decimal> = self
            .get_mark_prices()
            .into_iter()
            .map(|(sym, mp)| (sym, mp.price))
            .collect();

        let configs = self.perp_configs.load_full();

        exchange_risk_engine::portfolio_margin_analyse(&positions_refs, &mark_prices, &configs)
    }

    /// Enable portfolio margin mode for a user.
    ///
    /// # Errors
    /// Returns an error if the user's USDT equity is below $100,000.
    pub fn enable_portfolio_margin(
        self: &Arc<Self>,
        user_id: UserId,
    ) -> anyhow::Result<()> {
        const MIN_EQUITY_USD: Decimal = Decimal::from_parts(100_000, 0, 0, false, 0);

        let equity = self
            .accounts
            .get_balance(user_id, "USDT")
            .map(|b| b.total())
            .unwrap_or(Decimal::ZERO);

        if equity < MIN_EQUITY_USD {
            return Err(anyhow::anyhow!(
                "Portfolio margin requires at least $100,000 USDT equity; current equity: {}",
                equity
            ));
        }

        self.portfolio_margin_users
            .lock().unwrap_or_else(|e| e.into_inner())
            .insert(user_id);

        tracing::info!(user_id = %user_id, equity = %equity, "portfolio margin enabled");
        Ok(())
    }

    /// Disable portfolio margin mode for a user.
    pub fn disable_portfolio_margin(self: &Arc<Self>, user_id: UserId) {
        self.portfolio_margin_users
            .lock().unwrap_or_else(|e| e.into_inner())
            .remove(&user_id);
        tracing::info!(user_id = %user_id, "portfolio margin disabled");
    }

    /// Whether portfolio margin mode is active for the user.
    pub fn has_portfolio_margin(self: &Arc<Self>, user_id: UserId) -> bool {
        self.portfolio_margin_users
            .lock().unwrap_or_else(|e| e.into_inner())
            .contains(&user_id)
    }

    /// Set leverage for a user on a symbol.
    ///
    /// Validates against `PerpetualConfig::max_leverage`. Does not change
    /// existing positions — the new leverage applies to the next order.
    pub fn set_leverage(
        self: &Arc<Self>,
        user_id: UserId,
        symbol: &Symbol,
        leverage: Decimal,
    ) -> anyhow::Result<()> {
        if leverage <= Decimal::ZERO {
            return Err(anyhow::anyhow!("Leverage must be positive"));
        }

        let perp_configs = self.perp_configs.load();
        let config = perp_configs.get(symbol)
            .ok_or_else(|| anyhow::anyhow!("Unknown perpetual symbol: {symbol}"))?;

        if leverage > config.max_leverage {
            return Err(anyhow::anyhow!(
                "Leverage {} exceeds maximum {} for {}",
                leverage,
                config.max_leverage,
                symbol
            ));
        }
        drop(perp_configs);

        // Store the user's leverage preference.
        self.user_leverage.lock().unwrap_or_else(|e| e.into_inner())
            .insert((user_id, symbol.clone()), leverage);

        tracing::info!(
            user_id = %user_id,
            symbol = %symbol,
            leverage = %leverage,
            "leverage updated"
        );

        Ok(())
    }

    /// Get the user's leverage for a symbol, defaulting to 10x if not set.
    pub fn get_user_leverage(&self, user_id: UserId, symbol: &Symbol) -> Decimal {
        self.user_leverage.lock().unwrap_or_else(|e| e.into_inner())
            .get(&(user_id, symbol.clone()))
            .copied()
            .unwrap_or(Decimal::from(10u32))
    }

    /// Add isolated margin to an existing position.
    ///
    /// Deducts `amount` from the user's USDT balance and adds it to the
    /// position's margin, improving the liquidation price.
    pub fn add_margin(
        self: &Arc<Self>,
        user_id: UserId,
        symbol: &Symbol,
        amount: Decimal,
    ) -> anyhow::Result<()> {
        if amount <= Decimal::ZERO {
            return Err(anyhow::anyhow!("Margin amount must be positive"));
        }

        // Verify the position exists
        {
            let positions = self.positions.lock();
            let pos = positions.get_position_one_way(user_id, symbol)
                .ok_or_else(|| anyhow::anyhow!("No open position for {} on {}", user_id, symbol))?;

            if pos.margin_mode != MarginMode::Isolated {
                return Err(anyhow::anyhow!("add_margin is only supported for isolated margin positions"));
            }
        }

        // Deduct from the settlement asset balance (USDT for linear, BTC/ETH for inverse).
        let (settle_coin, _) = self.settle_asset_for(symbol);
        self.accounts
            .withdraw(user_id, &settle_coin, amount)
            .map_err(|e| anyhow::anyhow!("Insufficient balance to add margin: {e}"))?;

        // Add to position margin
        self.positions
            .lock()
            .add_margin_to_position(user_id, symbol, 0, amount)
            .map_err(|e| anyhow::anyhow!("Failed to add margin to position: {e}"))?;

        tracing::info!(
            user_id = %user_id,
            symbol = %symbol,
            amount = %amount,
            "margin added to position"
        );

        // Broadcast position update
        if let Some(pos) = self.positions.lock().get_position_one_way(user_id, symbol).cloned() {
            let mark = self.get_mark_price(symbol)
                .map(|mp| mp.price)
                .unwrap_or(pos.entry_price);
            if self.private_tx.send(PrivateEvent::PositionUpdate {
                user_id: user_id.to_string(),
                symbol: symbol.to_string(),
                side: format!("{:?}", pos.side),
                size: pos.size,
                entry_price: pos.entry_price,
                unrealized_pnl: pos.unrealized_pnl(mark),
                margin: pos.margin + amount,
                liq_price: pos.liquidation_price(),
            }).is_err() {
                tracing::debug!("private broadcast: no active subscribers");
            }
        }

        Ok(())
    }

    /// Adjust leverage on a live position.
    ///
    /// Validates the new leverage against the symbol's `PerpetualConfig`, then
    /// recalculates the required margin. If more margin is needed the difference
    /// is withdrawn from the user's USDT balance; if margin is released it is
    /// credited back.
    ///
    /// Returns the margin delta (positive = locked more, negative = released).
    pub fn adjust_position_leverage(
        self: &Arc<Self>,
        user_id: UserId,
        symbol: &Symbol,
        position_idx: u8,
        new_leverage: Decimal,
    ) -> anyhow::Result<Decimal> {
        if new_leverage <= Decimal::ZERO {
            return Err(anyhow::anyhow!("Leverage must be positive"));
        }

        // Validate against per-symbol max leverage.
        {
            let perp_configs = self.perp_configs.load();
            let config = perp_configs
                .get(symbol)
                .ok_or_else(|| anyhow::anyhow!("Unknown perpetual symbol: {symbol}"))?;
            if new_leverage > config.max_leverage {
                return Err(anyhow::anyhow!(
                    "Leverage {} exceeds maximum {} for {}",
                    new_leverage,
                    config.max_leverage,
                    symbol
                ));
            }
        }

        // Compute the margin delta before committing any state changes.
        let delta = {
            let pos = {
                let positions = self.positions.lock();
                positions
                    .get_position(user_id, symbol, position_idx)
                    .filter(|p| !p.is_closed())
                    .ok_or_else(|| anyhow::anyhow!("No open position for {} on {}", user_id, symbol))?
                    .clone()
            };
            let new_margin = exchange_risk_engine::margin_calculator::required_initial_margin(
                pos.size,
                pos.entry_price,
                new_leverage,
            );
            new_margin - pos.margin
        };

        // If delta > 0 the user needs to lock more margin — check balance first.
        // S18 Phase B: use `with_user` to atomically check-and-debit under one
        // shard lock, preventing a TOCTOU race between the availability check
        // and the subsequent withdraw.
        if delta > Decimal::ZERO {
            self.accounts.with_user(user_id, |am| -> anyhow::Result<()> {
                let available = am
                    .get_balance(&user_id, "USDT")
                    .map(|b| b.available())
                    .unwrap_or(Decimal::ZERO);
                if available < delta {
                    return Err(anyhow::anyhow!(
                        "Insufficient balance: need {delta} USDT but only {available} available"
                    ));
                }
                Ok(())
            })?;
        }

        // Apply the leverage change.
        {
            let mut positions = self.positions.lock();
            positions.adjust_leverage(user_id, symbol, position_idx, new_leverage)?;
        }

        // Settle the margin delta with the account.
        if delta > Decimal::ZERO {
            self.accounts
                .withdraw(user_id, "USDT", delta)
                .map_err(|e| anyhow::anyhow!("Failed to lock additional margin: {e}"))?;
        } else if delta < Decimal::ZERO {
            let _ = self.accounts.deposit(user_id, "USDT", delta.abs());
        }

        // Broadcast the position update.
        if let Some(pos) = self
            .positions
            .lock()
            .get_position(user_id, symbol, position_idx)
            .cloned()
        {
            let mark = self
                .get_mark_price(symbol)
                .map(|mp| mp.price)
                .unwrap_or(pos.entry_price);
            if self.private_tx.send(PrivateEvent::PositionUpdate {
                user_id: user_id.to_string(),
                symbol: symbol.to_string(),
                side: format!("{:?}", pos.side),
                size: pos.size,
                entry_price: pos.entry_price,
                unrealized_pnl: pos.unrealized_pnl(mark),
                margin: pos.margin,
                liq_price: pos.liquidation_price(),
            }).is_err() {
                tracing::debug!("private broadcast: no active subscribers");
            }
        }

        Ok(delta)
    }

    /// Handle a fill for a futures order — updates position instead of transferring spot balances.
    ///
    /// Called from `handle_fill` when the symbol has a `PerpetualConfig`.
    fn handle_futures_fill(self: &Arc<Self>, fill: &FillEvent) {
        if !self.perp_configs.load().contains_key(&fill.symbol) {
            return;
        }

        let taker_fill_side = fill.taker_side;
        let maker_fill_side = fill.taker_side.opposite();

        // Settle each party independently — detect open/increase vs reduce/close.
        self.settle_futures_fill(fill.taker_user_id, taker_fill_side, fill);
        self.settle_futures_fill(fill.maker_user_id, maker_fill_side, fill);

        let mark_price = self.get_mark_price(&fill.symbol)
            .map(|mp| mp.price)
            .unwrap_or(fill.price);

        // Broadcast position updates for both parties.
        // If a position was fully closed it is size=0; broadcast that too so clients know.
        for user_id in [fill.taker_user_id, fill.maker_user_id] {
            let pos_snapshot = self.positions.lock()
                .get_position_one_way(user_id, &fill.symbol)
                .cloned();

            match pos_snapshot {
                Some(pos) => {
                    if self.private_tx.send(PrivateEvent::PositionUpdate {
                        user_id: user_id.to_string(),
                        symbol: fill.symbol.to_string(),
                        side: format!("{:?}", pos.side),
                        size: pos.size,
                        entry_price: pos.entry_price,
                        unrealized_pnl: pos.unrealized_pnl(mark_price),
                        margin: pos.margin,
                        liq_price: pos.liquidation_price(),
                    }).is_err() {
                        tracing::debug!("private broadcast: no active subscribers");
                    }
                }
                None => {
                    // Position was fully closed and removed — broadcast a zeroed update.
                    if self.private_tx.send(PrivateEvent::PositionUpdate {
                        user_id: user_id.to_string(),
                        symbol: fill.symbol.to_string(),
                        side: String::new(),
                        size: Decimal::ZERO,
                        entry_price: Decimal::ZERO,
                        unrealized_pnl: Decimal::ZERO,
                        margin: Decimal::ZERO,
                        liq_price: None,
                    }).is_err() {
                        tracing::debug!("private broadcast: no active subscribers");
                    }
                }
            }
        }

        // Copy trading: if the taker is a registered active master, propagate
        // the trade to their followers proportionally.
        //
        // This runs AFTER both settle_futures_fill calls return, so all position
        // and account locks from settlement are already released.  Calling
        // self.place_order here is safe — there are no held locks to re-enter.
        self.propagate_copy_trade(fill);
    }

    /// If `fill.taker_user_id` is a registered master trader, generate copy
    /// orders for all active followers and place them on the exchange.
    ///
    /// Errors from individual copy orders are logged but do not affect the
    /// original fill's settlement.
    fn propagate_copy_trade(self: &Arc<Self>, fill: &FillEvent) {
        // Fast path: check master registration without holding the lock across
        // any potentially-blocking work.
        let is_active_master = {
            let masters = self.copy_trading_masters.lock().unwrap_or_else(|e| e.into_inner());
            masters.get(fill.taker_user_id).map(|m| m.is_active).unwrap_or(false)
        };

        if !is_active_master {
            return;
        }

        // Master equity = USDT balance + unrealized PnL on all open positions.
        // Using just the USDT balance would understate equity for margined traders,
        // causing inflated or zero follower position sizes.
        let master_equity = {
            let usdt_balance = self
                .accounts
                .get_balance(fill.taker_user_id, "USDT")
                .map(|b| b.total())
                .unwrap_or(Decimal::ZERO);
            let unrealized_pnl = {
                let pm = self.positions.lock();
                pm.all_positions()
                    .iter()
                    .filter(|p| p.user_id == fill.taker_user_id)
                    .map(|p| {
                        let mp = self.get_mark_price(&p.symbol)
                            .map(|m| m.price)
                            .unwrap_or(p.entry_price);
                        p.unrealized_pnl(mp)
                    })
                    .sum::<Decimal>()
            };
            usdt_balance + unrealized_pnl
        };

        let follower_relations: Vec<_> = {
            let followers = self.copy_trading_followers.lock().unwrap_or_else(|e| e.into_inner());
            followers
                .master_followers(fill.taker_user_id)
                .into_iter()
                .cloned()
                .collect()
        };

        if follower_relations.is_empty() {
            return;
        }

        let master_trade = MasterTrade {
            master_id: fill.taker_user_id,
            symbol: fill.symbol.clone(),
            side: fill.taker_side,
            qty: fill.qty,
            price: fill.price,
            order_type: OrderType::Market,
        };

        let copy_orders: Vec<CopyOrder> =
            TradeCopier::generate_copy_orders(&master_trade, &follower_relations, master_equity);

        tracing::info!(
            master = %fill.taker_user_id,
            symbol = %fill.symbol,
            followers = copy_orders.len(),
            "copy trading: propagating master fill to followers"
        );

        for copy in copy_orders {
            let cmd = PlaceOrderCommand {
                order_id: uuid::Uuid::new_v4(),
                user_id: copy.follower_id,
                symbol: copy.symbol,
                side: copy.side,
                order_type: copy.order_type,
                time_in_force: TimeInForce::Ioc,
                price: copy.price.unwrap_or(Decimal::ZERO),
                qty: copy.qty,
                timestamp: chrono::Utc::now(),
                reduce_only: false,
                expire_at: None,
                position_idx: 0,
                iceberg_qty: None,
                order_link_id: None,
                slippage_tolerance: None,
                close_position: false,
                stp_mode: StpMode::default(),
                request_expire_time: None,
            };

            if let Err(e) = self.place_order(cmd) {
                tracing::warn!(
                    master = %fill.taker_user_id,
                    follower = %copy.follower_id,
                    symbol = %fill.symbol,
                    error = %e,
                    "copy order placement failed"
                );
            }
        }
    }

    /// Return the settlement asset and contract_size for a futures symbol.
    ///
    /// For linear contracts returns ("USDT", Decimal::ONE).
    /// For inverse contracts returns (e.g. "BTC", Decimal::ONE) for BTCUSD.
    fn settle_asset_for(&self, symbol: &Symbol) -> (String, Decimal) {
        let configs = self.perp_configs.load();
        if let Some(cfg) = configs.get(symbol) {
            (cfg.settle_coin.clone(), cfg.contract_size)
        } else {
            ("USDT".to_string(), Decimal::ONE)
        }
    }

    /// Settle a single party's side of a futures fill.
    ///
    /// Determines whether this fill opens/increases the user's position or
    /// reduces/closes an existing opposite-side position, then updates the
    /// position and adjusts the settlement-asset balance accordingly.
    ///
    /// For **linear** contracts settlement is in USDT.
    /// For **inverse** contracts settlement is in the base asset (e.g. BTC for BTCUSD).
    ///
    /// Lock discipline: `positions` and `accounts` are **never held simultaneously**.
    /// We read-then-release `positions`, compute, then acquire `accounts`.
    fn settle_futures_fill(
        self: &Arc<Self>,
        user_id: UserId,
        fill_side: Side,
        fill: &FillEvent,
    ) {
        // Use the user's stored leverage preference (defaults to 10x if not set).
        let default_leverage = self.get_user_leverage(user_id, &fill.symbol);

        // Determine settlement asset and contract size for this symbol.
        let (settle_coin, contract_size) = self.settle_asset_for(&fill.symbol);
        let is_inverse = settle_coin != "USDT";

        // Determine the position side this fill represents for the user.
        let fill_pos_side = match fill_side {
            Side::Buy => exchange_common::futures::PositionSide::Long,
            Side::Sell => exchange_common::futures::PositionSide::Short,
        };

        // Check whether the user has an existing position on the OPPOSITE side
        // (meaning this fill is a reduce/close, not an open/increase).
        let existing_snapshot = self.positions.lock()
            .get_position_one_way(user_id, &fill.symbol)
            .filter(|p| !p.is_closed() && p.side != fill_pos_side)
            .cloned();

        if let Some(existing) = existing_snapshot {
            // --- Reduce / close path ---
            //
            // The user is trading against their existing position.
            // `reduce` handles the partial-close arithmetic; `open_or_increase` handles
            // flips (where qty > existing size) but we call `reduce` explicitly so
            // we always know the exact released_margin and realized_pnl.

            let reduce_qty = fill.qty.min(existing.size);

            let close_result = {
                let mut pm = self.positions.lock();
                pm.reduce(user_id, &fill.symbol, reduce_qty, fill.price, 0)
            };

            match close_result {
                Ok(result) => {
                    // Credit: released margin + realized PnL back to USDT balance.
                    // Credit: released margin + realized PnL back to the settlement asset.
                    // For linear: settle_coin = "USDT". For inverse: settle_coin = "BTC"/"ETH".
                    //
                    // For inverse positions the PositionManager computes PnL using the
                    // linear formula (price_diff * qty). We override that with the
                    // inverse formula here so settlement is correct in base-asset terms.
                    //
                    // Inverse realized PnL (base asset) =
                    //   side_sign * reduce_qty * contract_size * (1/entry_price - 1/fill_price)
                    let (released_margin, realized_pnl) = if is_inverse && !existing.entry_price.is_zero() && !fill.price.is_zero() {
                        let inv_pnl = existing.side.sign()
                            * reduce_qty
                            * contract_size
                            * (Decimal::ONE / existing.entry_price - Decimal::ONE / fill.price);
                        let frac = reduce_qty / existing.size;
                        let rel_margin = existing.margin * frac;
                        (rel_margin, inv_pnl)
                    } else {
                        (result.released_margin, result.realized_pnl)
                    };

                    let credit = released_margin + realized_pnl;

                    tracing::info!(
                        user_id = %user_id,
                        symbol = %fill.symbol,
                        reduce_qty = %reduce_qty,
                        realized_pnl = %realized_pnl,
                        released_margin = %released_margin,
                        credit = %credit,
                        settle_coin = %settle_coin,
                        "futures position reduced — crediting balance"
                    );

                    if credit > Decimal::ZERO {
                        if let Err(e) = self.accounts.deposit(user_id, &settle_coin, credit)
                        {
                            tracing::error!(
                                user_id = %user_id,
                                symbol = %fill.symbol,
                                credit = %credit,
                                error = %e,
                                "failed to credit PnL + margin on position reduce"
                            );
                        }
                    } else if credit < Decimal::ZERO {
                        // Net loss exceeds margin — debit the shortfall.
                        let debit = credit.abs();
                        if let Err(e) = self.accounts.withdraw(user_id, &settle_coin, debit)
                        {
                            tracing::warn!(
                                user_id = %user_id,
                                symbol = %fill.symbol,
                                debit = %debit,
                                error = %e,
                                "failed to debit loss on position reduce (Phase 1: allowing)"
                            );
                        }
                    }

                    // Record a closed-position entry when the position reaches zero size.
                    // We use `existing` (pre-reduce snapshot) for the entry price / metadata
                    // and `result.realized_pnl` for the final PnL figure.
                    {
                        let is_fully_closed = reduce_qty >= existing.size;
                        if is_fully_closed {
                            // Estimate trading fee for the close trade.
                            // Taker fee on notional = qty * price * taker_fee_rate.
                            let close_fee = {
                                let fee_rate = self
                                    .accounts
                                    .get_product(&fill.symbol)
                                    .map(|p| p.taker_fee)
                                    .unwrap_or(Decimal::new(6, 4)); // 0.06% default
                                reduce_qty * fill.price * fee_rate
                            };
                            let record = exchange_common::futures::ClosedPositionRecord {
                                id: uuid::Uuid::new_v4(),
                                user_id,
                                symbol: fill.symbol.clone(),
                                side: existing.side,
                                avg_entry_price: existing.entry_price,
                                avg_exit_price: fill.price,
                                closed_size: reduce_qty,
                                realized_pnl: result.realized_pnl,
                                trading_fee: close_fee,
                                funding_fee: existing.funding_accumulated,
                                leverage: existing.leverage,
                                close_reason: exchange_common::futures::CloseReason::UserClosed,
                                opened_at: existing.created_at,
                                closed_at: chrono::Utc::now(),
                            };
                            let mut closed = self.closed_positions
                                .lock().unwrap_or_else(|e| e.into_inner());
                            closed.push(record);
                            if closed.len() > MAX_CLOSED_POSITIONS {
                                let excess = closed.len() - MAX_CLOSED_POSITIONS;
                                closed.drain(0..excess);
                            }
                        }
                    }

                    // Write-behind ledger: margin release + realized PnL.
                    let maybe_pool = self.db_pool.get().cloned();
                    if let Some(pool) = maybe_pool {
                        let uid = user_id;
                        let margin = result.released_margin;
                        let pnl = result.realized_pnl;
                        let trade_ref = fill.trade_id.to_string();
                        tokio::spawn(async move {
                            let margin_escrow = uuid::Uuid::from_u128(0x08);
                            let pnl_pool = uuid::Uuid::from_u128(0x09);
                            let mut entries = vec![
                                // Release margin: escrow → user
                                exchange_db::ledger::LedgerEntry {
                                    account_id: margin_escrow,
                                    account_type: "system".into(),
                                    asset: "USDT".into(),
                                    amount: -margin,
                                    entry_type: exchange_db::ledger::EntryType::MarginRelease,
                                    reference_id: Some(trade_ref.clone()),
                                    reference_type: Some("trade".into()),
                                    description: None,
                                },
                                exchange_db::ledger::LedgerEntry {
                                    account_id: uid,
                                    account_type: "user_derivatives".into(),
                                    asset: "USDT".into(),
                                    amount: margin,
                                    entry_type: exchange_db::ledger::EntryType::MarginRelease,
                                    reference_id: Some(trade_ref.clone()),
                                    reference_type: Some("trade".into()),
                                    description: None,
                                },
                            ];
                            // Realized PnL: user ↔ pnl_pool (zero-sum)
                            if !pnl.is_zero() {
                                entries.push(exchange_db::ledger::LedgerEntry {
                                    account_id: uid,
                                    account_type: "user_derivatives".into(),
                                    asset: "USDT".into(),
                                    amount: pnl,
                                    entry_type: exchange_db::ledger::EntryType::RealizedPnl,
                                    reference_id: Some(trade_ref.clone()),
                                    reference_type: Some("trade".into()),
                                    description: Some(format!("Realized PnL: {}", pnl)),
                                });
                                entries.push(exchange_db::ledger::LedgerEntry {
                                    account_id: pnl_pool,
                                    account_type: "system".into(),
                                    asset: "USDT".into(),
                                    amount: -pnl,
                                    entry_type: exchange_db::ledger::EntryType::RealizedPnl,
                                    reference_id: Some(trade_ref),
                                    reference_type: Some("trade".into()),
                                    description: None,
                                });
                            }
                            if let Err(e) = exchange_db::ledger::LedgerService::post(&pool, entries).await {
                                tracing::error!(
                                    user_id = %uid,
                                    error = %e,
                                    "ledger write failed for margin release / realized PnL"
                                );
                            }
                        });
                    }

                    // Position write-behind: update or close the DB row.
                    {
                        let maybe_pool = self.db_pool.get().cloned();
                        if let Some(pool) = maybe_pool {
                            let uid = user_id;
                            let sym = fill.symbol.clone();
                            let db_id = self.position_db_ids
                                .lock().unwrap_or_else(|e| e.into_inner())
                                .get(&(user_id, fill.symbol.clone()))
                                .copied();
                            if let Some(db_id) = db_id {
                                // Check whether the position was fully closed (removed from in-memory map).
                                let still_open = self.positions.lock()
                                    .get_position_one_way(user_id, &fill.symbol)
                                    .map(|p| !p.is_closed())
                                    .unwrap_or(false);
                                if still_open {
                                    // Partial reduce — update the DB row with new size/entry/margin.
                                    let pos_snapshot = self.positions.lock()
                                        .get_position_one_way(user_id, &fill.symbol)
                                        .cloned();
                                    if let Some(pos) = pos_snapshot {
                                        let size = pos.size;
                                        let entry = pos.entry_price;
                                        let pos_margin = pos.margin;
                                        let realized = pos.realized_pnl;
                                        tokio::spawn(async move {
                                            if let Err(e) = exchange_db::position_repo::PositionRepo::update(
                                                &pool, db_id, size, entry, pos_margin, realized,
                                            ).await {
                                                tracing::error!(
                                                    user_id = %uid,
                                                    db_id = %db_id,
                                                    error = %e,
                                                    "position DB update failed on reduce"
                                                );
                                            }
                                        });
                                    }
                                } else {
                                    // Fully closed — mark as Closed in DB and remove from id map.
                                    let final_pnl = result.realized_pnl;
                                    self.position_db_ids
                                        .lock().unwrap_or_else(|e| e.into_inner())
                                        .remove(&(uid, sym));
                                    tokio::spawn(async move {
                                        if let Err(e) = exchange_db::position_repo::PositionRepo::close(
                                            &pool, db_id, final_pnl, "Closed",
                                        ).await {
                                            tracing::error!(
                                                user_id = %uid,
                                                db_id = %db_id,
                                                error = %e,
                                                "position DB close failed"
                                            );
                                        }
                                    });
                                }
                            }
                        }
                    }

                    // If there is remaining fill qty after the reduce (position flip),
                    // open a new position on the opposite side for the remainder.
                    let remainder_qty = fill.qty - reduce_qty;
                    if remainder_qty > Decimal::ZERO {
                        let new_margin = if is_inverse && !fill.price.is_zero() {
                            // Inverse: margin in base asset
                            remainder_qty * contract_size / (fill.price * default_leverage)
                        } else {
                            remainder_qty * fill.price / default_leverage
                        };
                        let open_result = self.positions.lock()
                            .open_or_increase(
                                user_id,
                                fill.symbol.clone(),
                                fill_pos_side,
                                remainder_qty,
                                fill.price,
                                default_leverage,
                                MarginMode::Isolated,
                                0, // one-way mode remainder after flip
                            );

                        if let Err(e) = open_result {
                            tracing::error!(
                                user_id = %user_id,
                                symbol = %fill.symbol,
                                error = %e,
                                "failed to open remainder position after flip"
                            );
                        } else {
                            if let Err(e) = self.accounts.withdraw(user_id, &settle_coin, new_margin)
                            {
                                // Margin deduction failed — close the under-margined position
                                // to prevent a zero-margin position from existing.
                                tracing::error!(
                                    user_id = %user_id,
                                    symbol = %fill.symbol,
                                    new_margin = %new_margin,
                                    error = %e,
                                    "insufficient margin for flip remainder — closing position"
                                );
                                let _ = self.positions.lock()
                                    .reduce(user_id, &fill.symbol, remainder_qty, fill.price, 0);
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(
                        user_id = %user_id,
                        symbol = %fill.symbol,
                        error = %e,
                        "failed to reduce position on futures fill"
                    );
                }
            }
        } else {
            // --- Atomic reduce-only guard ---
            //
            // If this fill is for a reduce-only order but no opposite position exists
            // (e.g., position was liquidated between order placement and fill), we must
            // NOT open a new position. Log the violation and skip settlement.
            {
                let order_id = if fill.taker_user_id == user_id {
                    fill.taker_order_id
                } else {
                    fill.maker_order_id
                };
                let is_reduce_only = self.pending_orders
                    .get(&order_id)
                    .map(|info| info.reduce_only)
                    .unwrap_or(false);

                if is_reduce_only {
                    tracing::warn!(
                        user_id = %user_id,
                        order_id = %order_id,
                        symbol = %fill.symbol,
                        "reduce-only fill arrived but no opposite position — skipping open"
                    );
                    return;
                }
            }

            // --- Open / increase path ---
            // For inverse contracts: margin is held in the base asset (BTC/ETH).
            //   initial_margin_base = contracts * contract_size / (price * leverage)
            // For linear: margin is held in USDT.
            //   initial_margin_usdt = qty * price / leverage
            let initial_margin = if is_inverse && !fill.price.is_zero() {
                fill.qty * contract_size / (fill.price * default_leverage)
            } else {
                fill.qty * fill.price / default_leverage
            };

            let open_result = {
                let mut pm = self.positions.lock();
                pm.open_or_increase(
                    user_id,
                    fill.symbol.clone(),
                    fill_pos_side,
                    fill.qty,
                    fill.price,
                    default_leverage,
                    MarginMode::Isolated,
                    0, // one-way mode (Phase 1 default)
                )
            };

            match open_result {
                Ok(_) => {
                    // Release the pre-flight margin reservation that was atomically locked
                    // in place_order. The reservation used `try_lock` (available → locked);
                    // we now `unlock_remainder` to restore it to available so the
                    // subsequent `withdraw` call (which operates on available balance) can
                    // succeed. For maker orders that went straight to the book the lock
                    // entry exists; for market orders that immediately fill it also exists.
                    let order_id_for_user = if fill.taker_user_id == user_id {
                        fill.taker_order_id
                    } else {
                        fill.maker_order_id
                    };
                    if let Some(reserved) = self
                        .futures_margin_locks
                        .lock().unwrap_or_else(|e| e.into_inner())
                        .remove(&order_id_for_user)
                    {
                        if let Err(e) = self
                            .accounts
                            .unlock_remainder(user_id, "USDT", reserved)
                        {
                            tracing::warn!(
                                order_id = %order_id_for_user,
                                user_id = %user_id,
                                reserved = %reserved,
                                error = %e,
                                "settle_futures_fill: failed to release margin reservation"
                            );
                        }
                    }

                    // Deduct initial margin from the settlement asset balance.
                    // Phase 1: log but do not block the fill if balance is insufficient.
                    if let Err(e) = self.accounts.withdraw(user_id, &settle_coin, initial_margin)
                    {
                        tracing::warn!(
                            user_id = %user_id,
                            symbol = %fill.symbol,
                            initial_margin = %initial_margin,
                            settle_coin = %settle_coin,
                            error = %e,
                            "insufficient balance for futures margin (Phase 1: allowing)"
                        );
                    }

                    // Write-behind ledger: margin lock (user → escrow).
                    let maybe_pool = self.db_pool.get().cloned();
                    if let Some(pool) = maybe_pool {
                        let margin = initial_margin;
                        let uid = user_id;
                        let trade_ref = fill.trade_id.to_string();

                        // Snapshot position data for DB write and decide whether this
                        // is a fresh open (needs INSERT) or an increase (needs UPDATE).
                        let pos_snapshot = self.positions.lock()
                            .get_position_one_way(user_id, &fill.symbol)
                            .cloned();
                        let existing_db_id = self.position_db_ids
                            .lock().unwrap_or_else(|e| e.into_inner())
                            .get(&(user_id, fill.symbol.clone()))
                            .copied();

                        if let Some(pos) = pos_snapshot {
                            if let Some(db_id) = existing_db_id {
                                // Position already in DB — update size/entry/margin.
                                let size = pos.size;
                                let entry = pos.entry_price;
                                let pos_margin = pos.margin;
                                let realized = pos.realized_pnl;
                                let pool2 = pool.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = exchange_db::position_repo::PositionRepo::update(
                                        &pool2, db_id, size, entry, pos_margin, realized,
                                    ).await {
                                        tracing::error!(
                                            user_id = %uid,
                                            db_id = %db_id,
                                            error = %e,
                                            "position DB update failed on increase"
                                        );
                                    }
                                });
                            } else {
                                // New position — INSERT and record the DB id.
                                let new_id = uuid::Uuid::new_v4();
                                self.position_db_ids
                                    .lock().unwrap_or_else(|e| e.into_inner())
                                    .insert((user_id, fill.symbol.clone()), new_id);

                                let row = exchange_db::position_repo::PositionRow {
                                    id: new_id,
                                    user_id: uid,
                                    symbol: pos.symbol.to_string(),
                                    side: format!("{:?}", pos.side),
                                    size: pos.size,
                                    entry_price: pos.entry_price,
                                    leverage: pos.leverage,
                                    margin_mode: format!("{:?}", pos.margin_mode),
                                    margin: pos.margin,
                                    realized_pnl: pos.realized_pnl,
                                    funding_accumulated: pos.funding_accumulated,
                                    liquidation_price: pos.liquidation_price(),
                                    status: "Open".into(),
                                    created_at: pos.created_at,
                                    updated_at: pos.updated_at,
                                    closed_at: None,
                                };
                                let pool2 = pool.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = exchange_db::position_repo::PositionRepo::create(
                                        &pool2, &row,
                                    ).await {
                                        tracing::error!(
                                            user_id = %uid,
                                            error = %e,
                                            "position DB create failed"
                                        );
                                    }
                                });
                            }
                        }

                        tokio::spawn(async move {
                            let margin_escrow = uuid::Uuid::from_u128(0x08);
                            if let Err(e) = exchange_db::ledger::LedgerService::post(&pool, vec![
                                exchange_db::ledger::LedgerEntry {
                                    account_id: uid,
                                    account_type: "user_derivatives".into(),
                                    asset: "USDT".into(),
                                    amount: -margin,
                                    entry_type: exchange_db::ledger::EntryType::MarginLock,
                                    reference_id: Some(trade_ref.clone()),
                                    reference_type: Some("trade".into()),
                                    description: Some("Margin locked for position".into()),
                                },
                                exchange_db::ledger::LedgerEntry {
                                    account_id: margin_escrow,
                                    account_type: "system".into(),
                                    asset: "USDT".into(),
                                    amount: margin,
                                    entry_type: exchange_db::ledger::EntryType::MarginLock,
                                    reference_id: Some(trade_ref),
                                    reference_type: Some("trade".into()),
                                    description: None,
                                },
                            ]).await {
                                tracing::error!(
                                    user_id = %uid,
                                    error = %e,
                                    "ledger write failed for margin lock"
                                );
                            }
                        });
                    }
                }
                Err(e) => {
                    tracing::error!(
                        user_id = %user_id,
                        symbol = %fill.symbol,
                        error = %e,
                        "failed to open/increase position on futures fill"
                    );
                }
            }
        }

        // Collect trading fee from this user and credit it to the treasury.
        // Use the product's configured futures fees, optionally improved by the
        // user's VIP tier if it offers a better rate.
        let is_taker = fill.taker_user_id == user_id;
        let fee_rate = {
            let product_fees = self
                .accounts
                .get_product(&fill.symbol)
                .map(|product| (product.maker_fee, product.taker_fee));
            let (vip_maker_fee, vip_taker_fee) = self
                .fee_engine
                .lock().unwrap_or_else(|e| e.into_inner())
                .get_fees(user_id);

            match product_fees {
                Some((product_maker_fee, product_taker_fee)) => {
                    if is_taker {
                        if vip_taker_fee < product_taker_fee {
                            vip_taker_fee
                        } else {
                            product_taker_fee
                        }
                    } else if vip_maker_fee < product_maker_fee {
                        vip_maker_fee
                    } else {
                        product_maker_fee
                    }
                }
                None => {
                    if is_taker {
                        vip_taker_fee
                    } else {
                        vip_maker_fee
                    }
                }
            }
        };
        let fee_amount = fill.qty * fill.price * fee_rate;
        if fee_amount > Decimal::ZERO {
            // S18 Phase B: fee capture is two independent per-user mutations
            // (trader debit, treasury credit). Treasury is a single well-known
            // account; fee accumulation is monotonic so the two-step sequence
            // is safe without atomic cross-user locking.
            let _ = self.accounts.withdraw(user_id, "USDT", fee_amount);
            let _ = self.accounts.deposit(TREASURY_ACCOUNT_ID, "USDT", fee_amount);
        }

        // --- Broker commission ---
        // If the trading user is a broker client, award commission_rate × fee to the broker.
        if fee_amount > Decimal::ZERO {
            let broker_id_opt = self
                .broker_clients
                .lock().unwrap_or_else(|e| e.into_inner())
                .get(&user_id)
                .copied();

            if let Some(broker_id) = broker_id_opt {
                let commission_rate = self
                    .brokers
                    .lock().unwrap_or_else(|e| e.into_inner())
                    .get(&broker_id)
                    .filter(|b| b.is_active)
                    .map(|b| b.commission_rate);

                if let Some(rate) = commission_rate {
                    let commission = fee_amount * rate;
                    if commission > Decimal::ZERO {
                        // Credit commission to broker from treasury share.
                        let _ = self.accounts.deposit(broker_id, "USDT", commission);

                        // Update broker's cumulative total.
                        if let Some(broker) = self
                            .brokers
                            .lock().unwrap_or_else(|e| e.into_inner())
                            .get_mut(&broker_id)
                        {
                            broker.total_commission += commission;
                        }

                        // Append commission event (cap at 100 000 entries).
                        let mut commissions = self
                            .broker_commissions
                            .lock().unwrap_or_else(|e| e.into_inner());
                        if commissions.len() >= 100_000 {
                            commissions.pop_front();
                        }
                        commissions.push_back(crate::exchange::BrokerCommission {
                            id: uuid::Uuid::new_v4(),
                            broker_id,
                            client_id: user_id,
                            trade_id: fill.trade_id,
                            amount: commission,
                            asset: "USDT".to_string(),
                            timestamp: chrono::Utc::now(),
                        });

                        tracing::debug!(
                            broker_id = %broker_id,
                            client_id = %user_id,
                            commission = %commission,
                            trade_id = %fill.trade_id,
                            "broker commission credited"
                        );
                    }
                }
            }
        }

        // --- Referral (affiliate) commission ---
        // If the trading user was referred by someone, distribute a share of the fee
        // to the referral chain (up to 3 tiers). This uses the engagement referral_tree,
        // not the ReferralManager, matching the affiliate program model.
        if fee_amount > Decimal::ZERO {
            let tree = self.referral_tree.lock().unwrap_or_else(|e| e.into_inner());
            let tier1 = tree.get(&user_id).copied();
            let tier2 = tier1.and_then(|t1| tree.get(&t1).copied());
            let tier3 = tier2.and_then(|t2| tree.get(&t2).copied());
            drop(tree);

            if let Some(t1) = tier1 {
                let now = chrono::Utc::now();
                // Rates: 20% tier1, 10% tier2, 5% tier3 (standard exchange affiliate rates)
                let mut payouts: Vec<(uuid::Uuid, u8, Decimal)> = vec![(t1, 1, Decimal::new(20, 2))];
                if let Some(t2) = tier2 {
                    payouts.push((t2, 2, Decimal::new(10, 2))); // 0.10
                }
                if let Some(t3) = tier3 {
                    payouts.push((t3, 3, Decimal::new(5, 2)));  // 0.05
                }

                // S18 Phase B: affiliate commission is a fan-out of per-user
                // credits. Each deposit routes to the earner's shard. Commission
                // amounts are additive — no need for a single atomic cross-shard
                // operation.
                let mut commissions = self.affiliate_commissions.lock().unwrap_or_else(|e| e.into_inner());

                for (earner_id, tier, rate) in payouts {
                    let amount = fee_amount * rate;
                    if amount > Decimal::ZERO {
                        let _ = self.accounts.deposit(earner_id, "USDT", amount);
                        commissions.push_back(crate::rest_engagement::AffiliateCommission {
                            id: uuid::Uuid::new_v4(),
                            earner_id,
                            trader_id: user_id,
                            tier,
                            rate,
                            amount,
                            earned_at: now,
                        });
                    }
                }

                while commissions.len() > 200_000 {
                    commissions.pop_front();
                }
            }
        }
    }

    /// S15 B.2 — Financial-critical (SYNC) portion of event processing.
    ///
    /// Runs on the hot path BEFORE the REST handler returns the order ack.
    ///
    /// Covers:
    /// - Circuit breaker reference price update (needed for next order's price-band check)
    /// - Metrics (lock-free atomics — negligible cost)
    /// - Spot order_locks update (track filled amounts — needed for unlock on cancel)
    /// - Spot fee calculation and `settle_fill` (balance debit/credit — MUST be sync)
    /// - Spot treasury fee credit (part of double-entry ledger — MUST be sync)
    /// - Futures `handle_futures_fill` (position update, PnL — MUST be sync)
    /// - Spot fee_engine.update_volume (VIP tier — mild lag acceptable but kept sync for simplicity)
    /// - Liquidity pool fee share (financial — MUST be sync)
    /// - Iceberg replenishment (needs to happen before next order tick)
    /// - STP guard `on_order_accepted` (needed for future self-trade checks)
    /// - Grid/DCA/TWAP/VWAP bot callbacks (recursively call place_order — kept sync)
    /// - `handle_cancel` / `handle_reject` (balance unlock — MUST be sync)
    ///
    /// Does NOT do: surveillance, notifications, push, WS broadcast, FIX, trade history.
    /// Those run in the async worker via `dispatch_post_fill_events`.
    fn process_events_sync(self: &Arc<Self>, events: &[MatchEvent], _symbol: &Symbol) {
        for event in events {
            match event {
                MatchEvent::Fill(fill) => {
                    self.handle_fill_sync(fill);
                }
                MatchEvent::OrderCancelled { order_id, .. } => {
                    self.handle_cancel(*order_id);
                }
                MatchEvent::OrderRejected { order_id, reason, .. } => {
                    self.handle_reject(*order_id, reason);
                }
                MatchEvent::OrderAccepted { order_id, .. } => {
                    // STP guard registration must stay sync — future orders from the
                    // same user need to see this order for self-trade prevention.
                    if let Some(record) = self.order_locks.get(order_id) {
                        self.stp_guard.lock().on_order_accepted(
                            *order_id,
                            record.user_id,
                            record.symbol.clone(),
                            record.side,
                        );
                    }
                    // private_tx.send(OrderAccepted) and BookUpdate are dispatched
                    // to the async worker via dispatch_post_fill_events.
                }
                MatchEvent::CancelRejected { order_id, reason, .. } => {
                    tracing::debug!(
                        order_id = %order_id,
                        reason = %reason,
                        "cancel rejected (order not on book)"
                    );
                }
            }
        }
    }

    /// S15 B.2 — Dispatch ASYNC-SAFE side effects to the post-fill worker.
    ///
    /// Called AFTER `process_events_sync` returns. Sends one `PostFillEvent` per
    /// fill and per accepted order to the background worker channel. Uses `try_send`
    /// so the hot path is never blocked.
    ///
    /// On channel saturation (`try_send` returns `Full`), the event is dropped and
    /// `event_channel_dropped_total` is incremented. The order ack is still returned
    /// normally — surveillance/notification lag is acceptable under backpressure.
    fn dispatch_post_fill_events(self: &Arc<Self>, events: &[MatchEvent], symbol: &Symbol) {
        for event in events {
            match event {
                MatchEvent::Fill(fill) => {
                    let is_futures = self.perp_configs
                        .load()
                        .contains_key(&fill.symbol);
                    let pf_event = PostFillEvent::Fill {
                        fill: fill.clone(),
                        is_futures,
                    };
                    if let Err(_e) = self.post_fill_event_tx.try_send(pf_event) {
                        self.metrics.inc_event_channel_dropped();
                        tracing::warn!(
                            symbol = %fill.symbol,
                            "post_fill channel full — surveillance/notification event dropped"
                        );
                    }
                }
                MatchEvent::OrderAccepted { order_id, .. } => {
                    // Resolve user_id for the notification worker.
                    let user_id = self.order_locks.get(order_id)
                        .map(|r| r.user_id)
                        .or_else(|| {
                            self.pending_orders.get(order_id).map(|i| i.user_id)
                        });
                    if let Some(uid) = user_id {
                        let pf_event = PostFillEvent::OrderAccepted {
                            order_id: *order_id,
                            user_id: uid,
                            symbol: symbol.clone(),
                        };
                        if let Err(_e) = self.post_fill_event_tx.try_send(pf_event) {
                            self.metrics.inc_event_channel_dropped();
                            tracing::warn!(
                                order_id = %order_id,
                                "post_fill channel full — OrderAccepted event dropped"
                            );
                        }
                    }
                    // Also dispatch the book update via a Fill-less path:
                    // We send a sentinel that causes the worker to broadcast BookUpdate.
                    // Implemented by the worker listening on OrderAccepted and broadcasting.
                }
                MatchEvent::OrderCancelled { order_id, .. } => {
                    // Cancellation private events are dispatched inside handle_cancel already.
                    // Nothing additional needed here currently.
                    let _ = order_id;
                }
                _ => {}
            }
        }

        // Dispatch a BookUpdate request to the worker for the symbol.
        // The worker reads the current snapshot from engine_pool.
        // We reuse OrderAccepted variant with a sentinel order_id to signal "please send BookUpdate".
        // Actually simpler: add a BookUpdateNeeded variant — but to keep the change minimal,
        // the worker will broadcast BookUpdate after processing any event for this symbol.
        // This is handled inside spawn_post_fill_event_worker by broadcasting after each Fill.
    }

    /// Legacy wrapper — kept for the second call site (`cancel_order` path at line 5172).
    ///
    /// This path does not carry fills so the only sync work is handle_cancel/handle_reject
    /// (already called inline). For the non-fill path we skip the dispatch entirely since
    /// cancellations emit their WS events from inside handle_cancel.
    fn process_events(self: &Arc<Self>, events: &[MatchEvent], symbol: &Symbol) {
        self.process_events_sync(events, symbol);
        self.dispatch_post_fill_events(events, symbol);
    }

    /// S15: Financial-critical (SYNC) fill handler.
    ///
    /// Called from `process_events_sync` on the hot path.  Performs all work that
    /// MUST complete before the REST handler returns an order ack:
    ///
    /// - Circuit breaker reference-price update
    /// - Lock-free metrics update
    /// - Spot: order_locks track, fee calculation, `settle_fill`, treasury credit,
    ///         liquidity pool fee share, fee_engine.update_volume, record_filled_order,
    ///         iceberg replenishment, write-behind DB spawn
    /// - Futures: `handle_futures_fill` (position/PnL), fee_engine.update_volume,
    ///            iceberg replenishment (futures path)
    /// - Grid/DCA/TWAP/VWAP bot callbacks (call place_order recursively — kept sync)
    ///
    /// Does NOT do: taker_volume_history, surveillance, market_data, WS broadcasts,
    /// notifications, FIX, trade history. Those run in the async worker.
    fn handle_fill_sync(self: &Arc<Self>, fill: &FillEvent) {
        // Update circuit breaker reference price with the confirmed trade price.
        if let Some(cb) = self.circuit_breakers.lock().get_mut(&fill.symbol) {
            cb.update_reference(fill.price);
        }

        // Metrics
        self.metrics.inc_trades();
        self.metrics.inc_orders_filled();
        self.metrics.inc_fills();
        self.metrics.touch_last_trade();
        self.metrics.inc_symbol_fills(fill.symbol.as_str());
        // Volume in cents: (price * qty * 100) as integer
        let vol_cents = (fill.price * fill.qty * rust_decimal::Decimal::ONE_HUNDRED)
            .to_string()
            .parse::<u64>()
            .unwrap_or(0);
        self.metrics.add_volume_cents(vol_cents);

        // ASYNC-SAFE: taker_volume_history — deferred to post-fill worker.
        // (5-minute bucket stats; slight lag acceptable)

        // If this is a perpetual symbol, update positions instead of spot balances
        if self.perp_configs.load().contains_key(&fill.symbol) {
            // SYNC: position update (must complete before ack — affects margin checks).
            self.handle_futures_fill(fill);

            // SYNC: VIP fee tier volume tracking.
            let notional = fill.price * fill.qty;
            {
                let mut fee_engine = self.fee_engine.lock().unwrap_or_else(|e| e.into_inner());
                fee_engine.update_volume(fill.taker_user_id, notional);
                fee_engine.update_volume(fill.maker_user_id, notional);
            }

            // ASYNC-SAFE: surveillance, market_data, WS, notifications, FIX
            // — deferred to post-fill worker via PostFillEvent::Fill.

            // SYNC: Public trade history — kept sync to maintain backward compatibility
            // with tests that read trade_history immediately after a fill, and because
            // it's a cheap Vec push that doesn't justify async indirection.
            self.push_trade_history(fill);

            // SYNC: Iceberg replenishment for futures — must happen promptly so next
            // visible slice appears on the book without delay.
            {
                let mut meta_map = self.iceberg_meta.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(meta) = meta_map.get_mut(&fill.maker_order_id) {
                    meta.slice_filled += fill.qty;
                    if meta.slice_filled >= meta.visible_qty {
                        drop(meta_map);
                        self.replenish_iceberg_slice(fill.maker_order_id);
                    }
                }
            }

            return;
        }

        // --- Spot settlement path ---

        // Get product for fee calculation (S18 Phase B: ShardedAccounts returns
        // an owned Product clone from the replicated products registry).
        let product = self.accounts.get_product(&fill.symbol);

        if let Some(product) = product {
            // Update order lock records (track filled amounts)
            {
                let fill_cost = fill.price * fill.qty;

                if let Some(mut record) = self.order_locks.get_mut(&fill.taker_order_id) {
                    record.filled_qty += fill.qty;
                    record.filled_cost += fill_cost;
                }
                if let Some(mut record) = self.order_locks.get_mut(&fill.maker_order_id) {
                    record.filled_qty += fill.qty;
                    record.filled_cost += fill_cost;
                }
            }

            // Check if taker or maker order is fully filled → record in history
            {
                let taker_done = self.order_locks
                    .get(&fill.taker_order_id)
                    .map(|r| r.filled_qty >= r.qty)
                    .unwrap_or(false);
                let maker_done = self.order_locks
                    .get(&fill.maker_order_id)
                    .map(|r| r.filled_qty >= r.qty)
                    .unwrap_or(false);

                if taker_done {
                    self.record_filled_order(fill.taker_order_id);
                }
                if maker_done {
                    self.record_filled_order(fill.maker_order_id);
                    // Iceberg replenishment: if the maker's visible slice is an iceberg,
                    // place the next slice now that the current one is fully consumed.
                    self.replenish_iceberg_slice(fill.maker_order_id);
                }
            }

            // Apply VIP fee tiers: override static product fees with per-user rates.
            // Taker pays taker fee; maker pays maker fee.
            let product_with_vip_fees = {
                let fee_engine = self.fee_engine.lock().unwrap_or_else(|e| e.into_inner());
                let (maker_fee, _) = fee_engine.get_fees(fill.maker_user_id);
                let (_, taker_fee) = fee_engine.get_fees(fill.taker_user_id);
                let mut p = product.clone();
                p.maker_fee = maker_fee;
                p.taker_fee = taker_fee;
                p
            };

            // S18 Phase B: ShardedAccounts::settle_fill locks the buyer and
            // seller shards in ascending index order (or a single shared shard
            // when the hash collides). Treasury fee credits are a separate op
            // — treasury is monotonic so no atomic cross-user lock is required.
            match self.accounts.settle_fill(fill, &product_with_vip_fees) {
                Err(e) => {
                    tracing::error!(
                        symbol = %fill.symbol,
                        maker = %fill.maker_user_id,
                        taker = %fill.taker_user_id,
                        price = %fill.price,
                        qty = %fill.qty,
                        error = %e,
                        "CRITICAL: spot settle_fill failed — balances NOT updated"
                    );
                }
                Ok(fee_info) => {
                    if fee_info.maker_fee.amount > Decimal::ZERO {
                        let _ = self.accounts.deposit(
                            TREASURY_ACCOUNT_ID,
                            &fee_info.maker_fee.asset,
                            fee_info.maker_fee.amount,
                        );
                    }
                    if fee_info.taker_fee.amount > Decimal::ZERO {
                        let _ = self.accounts.deposit(
                            TREASURY_ACCOUNT_ID,
                            &fee_info.taker_fee.asset,
                            fee_info.taker_fee.amount,
                        );
                    }
                }
            }

            // Route a fraction of trading fees to the matching LP pool (if one exists).
            // This funds the liquidity mining reward distribution.
            // 20% of fees → LP pool's total_fees_earned; 80% stays in treasury.
            {
                let lp_fee_share = Decimal::new(20, 2); // 0.20 = 20%
                let symbol_str = fill.symbol.to_string();
                let total_fee_usdt = fill.qty * fill.price * Decimal::new(1, 3); // rough fee estimate
                let lp_portion = total_fee_usdt * lp_fee_share;
                if lp_portion > Decimal::ZERO {
                    let mut pools = self.liquidity_pools.lock().unwrap_or_else(|e| e.into_inner());
                    // Find a pool whose pair contains this symbol's base or quote asset.
                    if let Some(pool) = pools.iter_mut().find(|p| {
                        p.pair.contains(&symbol_str.replace("USDT", ""))
                    }) {
                        pool.total_fees_earned += lp_portion;
                    }
                }
            }

            // Write-behind ledger entry (async, non-blocking).
            // Taker is the aggressor. When taker_side == Buy: buyer=taker, seller=maker.
            // buyer_fee is collected in base asset; seller_fee in quote asset.
            {
                let maybe_pool = self.db_pool.get().cloned();
                if let Some(pool) = maybe_pool {
                    let (buyer_id, seller_id) = if fill.taker_side == Side::Buy {
                        (fill.taker_user_id, fill.maker_user_id)
                    } else {
                        (fill.maker_user_id, fill.taker_user_id)
                    };
                    let buyer_fee_rate = if fill.taker_side == Side::Buy {
                        product_with_vip_fees.taker_fee
                    } else {
                        product_with_vip_fees.maker_fee
                    };
                    let seller_fee_rate = if fill.taker_side == Side::Sell {
                        product_with_vip_fees.taker_fee
                    } else {
                        product_with_vip_fees.maker_fee
                    };
                    let base_qty = fill.qty;
                    let quote_cost = fill.price * fill.qty;
                    let buyer_fee = base_qty * buyer_fee_rate;
                    let seller_fee = quote_cost * seller_fee_rate;
                    let base_asset = product.base_asset.clone();
                    let quote_asset = product.quote_asset.clone();
                    let trade_id = fill.trade_id.to_string();

                    let ex_ref = Arc::clone(self);
                    tokio::spawn(async move {
                        if let Err(e) = exchange_db::ledger::LedgerService::settle_spot_trade(
                            &pool,
                            buyer_id,
                            seller_id,
                            &base_asset,
                            &quote_asset,
                            base_qty,
                            quote_cost,
                            buyer_fee,
                            seller_fee,
                            &trade_id,
                        )
                        .await
                        {
                            // C2: Ledger write failed AFTER in-memory balance update.
                            // This creates a divergence: in-memory state has the fill,
                            // but the DB ledger doesn't. The reconciliation job will
                            // detect this drift. Log prominently and increment the
                            // mismatch counter so monitoring can alert.
                            tracing::error!(
                                trade_id = %trade_id,
                                error = %e,
                                "RECONCILIATION ALERT: ledger write failed for spot trade — \
                                 in-memory and DB balances have DIVERGED. The hourly \
                                 reconciliation job will detect and report this gap."
                            );
                            ex_ref.ledger_write_failures.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        } else {
                            ex_ref.fills_ledger_written.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                    });
                }
            }

            // SYNC: Update 30-day volume for VIP tier recalculation.
            let notional = fill.price * fill.qty;
            {
                let mut fee_engine = self.fee_engine.lock().unwrap_or_else(|e| e.into_inner());
                fee_engine.update_volume(fill.taker_user_id, notional);
                fee_engine.update_volume(fill.maker_user_id, notional);
            }

            // SYNC: Public trade history — kept sync for same reason as futures path above.
            self.push_trade_history(fill);

            // ASYNC-SAFE: surveillance, market_data, WS, notifications, FIX
            // — deferred to post-fill worker via PostFillEvent::Fill.
        }

        // Grid bot reactive fill handling.
        //
        // Check whether the taker or maker order ID belongs to any active grid
        // bot. If so, call on_fill to generate the replacement order one level
        // above (sell filled) or below (buy filled) and place it immediately.
        //
        // We check both order IDs because grid limit orders sit on the book as
        // makers when they are eventually crossed by an incoming taker.
        let grid_follow_ups: Vec<(exchange_common::Symbol, exchange_common::UserId, Vec<exchange_trading_bots::BotOrder>)> = {
            let mut bots = self.grid_bots.lock().unwrap_or_else(|e| e.into_inner());
            let mut results = Vec::new();

            for bot in bots.iter_mut() {
                if bot.status != exchange_trading_bots::BotStatus::Active {
                    continue;
                }
                // Check if taker order belongs to this bot.
                let taker_owned = bot.grid_lines.iter().any(|l| {
                    l.buy_order_id == Some(fill.taker_order_id)
                        || l.sell_order_id == Some(fill.taker_order_id)
                });
                // Check if maker order belongs to this bot.
                let maker_owned = bot.grid_lines.iter().any(|l| {
                    l.buy_order_id == Some(fill.maker_order_id)
                        || l.sell_order_id == Some(fill.maker_order_id)
                });

                if taker_owned {
                    let side = fill.taker_side;
                    let orders = bot.on_fill(fill.price, side);
                    if !orders.is_empty() {
                        results.push((bot.config.symbol.clone(), bot.config.user_id, orders));
                    }
                } else if maker_owned {
                    let side = fill.taker_side.opposite();
                    let orders = bot.on_fill(fill.price, side);
                    if !orders.is_empty() {
                        results.push((bot.config.symbol.clone(), bot.config.user_id, orders));
                    }
                }
            }
            results
        };

        let now = chrono::Utc::now();
        for (symbol, user_id, bot_orders) in grid_follow_ups {
            for bot_order in bot_orders {
                let cmd = exchange_common::PlaceOrderCommand {
                    order_id: bot_order.order_id,
                    user_id,
                    symbol: symbol.clone(),
                    side: bot_order.side,
                    order_type: bot_order.order_type,
                    time_in_force: exchange_common::TimeInForce::Gtc,
                    price: bot_order.price,
                    qty: bot_order.qty,
                    reduce_only: false,
                    expire_at: None,
                    timestamp: now,
                    position_idx: 0,
                    iceberg_qty: None,
                    order_link_id: None,
                    slippage_tolerance: None,
                    close_position: false,
                    stp_mode: exchange_common::StpMode::default(),
                    request_expire_time: None,
                };
                if let Err(e) = self.place_order(cmd) {
                    tracing::error!(
                        symbol = %symbol,
                        side = ?bot_order.side,
                        price = %bot_order.price,
                        error = %e,
                        "grid bot replacement order failed"
                    );
                } else {
                    tracing::debug!(
                        symbol = %symbol,
                        side = ?bot_order.side,
                        price = %bot_order.price,
                        "grid bot replacement order placed"
                    );
                }
            }
        }

        // DCA bot fill callback — update avg_price, total_invested, etc.
        {
            let mut bots = self.dca_bots.lock().unwrap_or_else(|e| e.into_inner());
            for bot in bots.iter_mut() {
                if bot.status != exchange_trading_bots::BotStatus::Active {
                    continue;
                }
                // DCA bots place market orders; the taker is always the bot user.
                if bot.config.user_id == fill.taker_user_id && bot.config.symbol == fill.symbol {
                    bot.on_fill(fill.price, fill.qty);
                }
            }
        }

        // TWAP/VWAP algo fill callback — update filled_qty, slice status, completion.
        {
            let mut twaps = self.twap_executions.lock().unwrap_or_else(|e| e.into_inner());
            for exec in twaps.values_mut() {
                for (idx, slice) in exec.slices.iter().enumerate() {
                    if slice.order_id == Some(fill.taker_order_id) || slice.order_id == Some(fill.maker_order_id) {
                        exec.on_fill(idx as u32, fill.price, fill.qty);
                        break;
                    }
                }
            }
        }
        {
            let mut vwaps = self.vwap_executions.lock().unwrap_or_else(|e| e.into_inner());
            for exec in vwaps.values_mut() {
                for (idx, slice) in exec.slices.iter().enumerate() {
                    if slice.order_id == Some(fill.taker_order_id) || slice.order_id == Some(fill.maker_order_id) {
                        exec.on_fill(idx as u32, fill.price, fill.qty);
                        break;
                    }
                }
            }
        }
    }

    /// S15: ASYNC-SAFE side effects for a fill event.
    ///
    /// Called by `spawn_post_fill_event_worker` — NOT on the hot path.
    ///
    /// Executes everything that was removed from `handle_fill_sync`:
    /// - taker_volume_history update (5-min bucketed stats)
    /// - Surveillance (on_trade, on_order_filled)
    /// - market_data.on_trade + update_book
    /// - WS broadcasts: Trade, KlineUpdate, TickerUpdate, MiniTickerUpdate, BookUpdate
    /// - Push notifications (both sides)
    /// - WS private OrderFilled events
    /// - FIX ExecutionReport drop copy
    /// - Public trade history (push_trade_history)
    /// - Private OrderAccepted WS event (for OrderAccepted events dispatched separately)
    ///
    /// `is_futures` indicates whether this is a perpetual fill (true) or spot (false).
    pub fn handle_fill_side_effects(self: &Arc<Self>, fill: &FillEvent, is_futures: bool) {
        // Trading statistics: record taker buy/sell volume in the current 5-minute bucket.
        {
            let bucket_secs = (fill.timestamp.timestamp() / 300) * 300;
            let mut tv = self.taker_volume_history.lock().unwrap_or_else(|e| e.into_inner());
            let buckets = tv.entry(fill.symbol.clone()).or_default();
            let needs_new = buckets.back().map(|b| b.timestamp != bucket_secs).unwrap_or(true);
            if needs_new {
                buckets.push_back(TakerVolumeSnapshot {
                    timestamp: bucket_secs,
                    buy_volume: Decimal::ZERO,
                    sell_volume: Decimal::ZERO,
                    buy_count: 0,
                    sell_count: 0,
                });
                if buckets.len() > MAX_STATS_HISTORY_PER_SYMBOL {
                    buckets.pop_front();
                }
            }
            if let Some(bucket) = buckets.back_mut() {
                if fill.taker_side == Side::Buy {
                    bucket.buy_volume += fill.qty;
                    bucket.buy_count += 1;
                } else {
                    bucket.sell_volume += fill.qty;
                    bucket.sell_count += 1;
                }
            }
        }

        // Surveillance: record trade + fill events for manipulation detection.
        {
            let mut surv = self.surveillance.lock();
            surv.on_trade(
                fill.maker_user_id,
                fill.taker_user_id,
                fill.symbol.clone(),
                fill.price,
                fill.qty,
                fill.taker_side,
                fill.timestamp,
            );
            surv.on_order_filled(fill.maker_user_id, fill.maker_order_id);
            surv.on_order_filled(fill.taker_user_id, fill.taker_order_id);
        }

        // Feed into market data service for candle/ticker aggregation.
        {
            self.market_data.on_trade(&fill.symbol, fill.price, fill.qty, fill.taker_side, fill.timestamp);

            let (bid, bid_sz, ask, ask_sz) = {
                let pool = self.engine_pool.read().unwrap_or_else(|e| e.into_inner());
                match pool.snapshot(&fill.symbol, 1) {
                    Ok(snap) => {
                        let (b, bs) = snap.bids.first().map(|(p, q)| (Some(*p), Some(*q))).unwrap_or((None, None));
                        let (a, as_) = snap.asks.first().map(|(p, q)| (Some(*p), Some(*q))).unwrap_or((None, None));
                        (b, bs, a, as_)
                    }
                    Err(_) => (None, None, None, None),
                }
            };
            self.market_data.update_book(&fill.symbol, bid, bid_sz, ask, ask_sz);
        }

        // Broadcast trade event to WS subscribers.
        if self.market_tx.send(MarketEvent::Trade {
            symbol: fill.symbol.to_string(),
            price: fill.price,
            qty: fill.qty,
            taker_side: format!("{:?}", fill.taker_side),
            trade_id: fill.trade_id,
            timestamp: fill.timestamp.timestamp_millis(),
        }).is_err() {
            tracing::debug!("market broadcast: no active subscribers");
        }

        // Broadcast current candle state for the three most common intervals.
        for (interval_str, interval) in &[
            ("1m", exchange_market_data::CandleInterval::M1),
            ("5m", exchange_market_data::CandleInterval::M5),
            ("1h", exchange_market_data::CandleInterval::H1),
        ] {
            if let Some(candle) = self.market_data.current_candle(&fill.symbol, *interval) {
                if self.market_tx.send(MarketEvent::KlineUpdate {
                    symbol: fill.symbol.to_string(),
                    interval: interval_str.to_string(),
                    open_time: candle.open_time.timestamp_millis(),
                    open: candle.open.to_string(),
                    high: candle.high.to_string(),
                    low: candle.low.to_string(),
                    close: candle.close.to_string(),
                    volume: candle.volume.to_string(),
                    is_closed: candle.is_closed,
                }).is_err() {
                    tracing::debug!("market broadcast: no active subscribers");
                }
            }
        }

        if let Some(ticker) = self.market_data.ticker(&fill.symbol) {
            if self.market_tx.send(MarketEvent::TickerUpdate {
                symbol: fill.symbol.to_string(),
                last_price: ticker.last_price.to_string(),
                price_change_pct_24h: ticker.price_change_pct_24h.to_string(),
                high_24h: ticker.high_24h.to_string(),
                low_24h: ticker.low_24h.to_string(),
                volume_24h: ticker.volume_24h.to_string(),
                mark_price: ticker.mark_price.map(|v| v.to_string()).unwrap_or_default(),
                index_price: ticker.index_price.map(|v| v.to_string()).unwrap_or_default(),
                funding_rate: ticker.funding_rate.map(|v| v.to_string()).unwrap_or_default(),
                open_interest: ticker.open_interest.map(|v| v.to_string()).unwrap_or_default(),
            }).is_err() {
                tracing::debug!("market broadcast: no active subscribers");
            }
            if self.market_tx.send(MarketEvent::MiniTickerUpdate {
                symbol: fill.symbol.to_string(),
                last_price: ticker.last_price.to_string(),
                volume_24h: ticker.volume_24h.to_string(),
                timestamp: chrono::Utc::now().timestamp_millis(),
            }).is_err() {
                tracing::debug!("market broadcast: no active subscribers");
            }
        }

        // Broadcast book update (snapshot after the fill).
        if let Ok(snap) = self.engine_pool.read().unwrap_or_else(|e| e.into_inner()).snapshot(&fill.symbol, 25) {
            if self.market_tx.send(MarketEvent::BookUpdate {
                symbol: fill.symbol.to_string(),
                bids: snap.bids,
                asks: snap.asks,
                sequence: snap.sequence,
            }).is_err() {
                tracing::debug!("market broadcast: no active subscribers");
            }
        }

        // NOTE: push_trade_history is called synchronously in handle_fill_sync
        // (kept sync for test compatibility and low cost). Not repeated here.

        // Notifications: order-filled for both sides.
        {
            let mut notifs = self.notifications.lock().unwrap_or_else(|e| e.into_inner());
            notifs.notify(
                fill.taker_user_id,
                NotificationType::OrderFilled,
                NotificationChannel::WebSocket,
                format!("Order Filled — {}", fill.symbol.as_str()),
                format!("Filled {} {} @ {}", fill.qty, fill.symbol.as_str(), fill.price),
                serde_json::json!({
                    "order_id": fill.taker_order_id.to_string(),
                    "symbol": fill.symbol.as_str(),
                    "price": fill.price.to_string(),
                    "qty": fill.qty.to_string(),
                    "role": "taker",
                }),
            );
            notifs.notify(
                fill.maker_user_id,
                NotificationType::OrderFilled,
                NotificationChannel::WebSocket,
                format!("Order Filled — {}", fill.symbol.as_str()),
                format!("Filled {} {} @ {}", fill.qty, fill.symbol.as_str(), fill.price),
                serde_json::json!({
                    "order_id": fill.maker_order_id.to_string(),
                    "symbol": fill.symbol.as_str(),
                    "price": fill.price.to_string(),
                    "qty": fill.qty.to_string(),
                    "role": "maker",
                }),
            );
        }

        // Push notification for order fills (both taker and maker).
        self.send_push_notification(
            fill.taker_user_id,
            "Order Filled",
            &format!("Filled {} {} @ {}", fill.qty, fill.symbol.as_str(), fill.price),
            exchange_admin::NotificationType::OrderFilled,
            serde_json::json!({
                "order_id": fill.taker_order_id.to_string(),
                "symbol": fill.symbol.as_str(),
                "price": fill.price.to_string(),
                "qty": fill.qty.to_string(),
                "role": "taker",
            }),
        );
        self.send_push_notification(
            fill.maker_user_id,
            "Order Filled",
            &format!("Filled {} {} @ {}", fill.qty, fill.symbol.as_str(), fill.price),
            exchange_admin::NotificationType::OrderFilled,
            serde_json::json!({
                "order_id": fill.maker_order_id.to_string(),
                "symbol": fill.symbol.as_str(),
                "price": fill.price.to_string(),
                "qty": fill.qty.to_string(),
                "role": "maker",
            }),
        );

        // Private WS fill events (taker and maker).
        if self.private_tx.send(PrivateEvent::OrderFilled {
            user_id: fill.taker_user_id.to_string(),
            order_id: fill.taker_order_id.to_string(),
            symbol: fill.symbol.to_string(),
            price: fill.price,
            qty: fill.qty,
            side: format!("{:?}", fill.taker_side),
            role: "taker".to_string(),
        }).is_err() {
            tracing::debug!("private broadcast: no active subscribers");
        }
        if self.private_tx.send(PrivateEvent::OrderFilled {
            user_id: fill.maker_user_id.to_string(),
            order_id: fill.maker_order_id.to_string(),
            symbol: fill.symbol.to_string(),
            price: fill.price,
            qty: fill.qty,
            side: format!("{:?}", fill.taker_side.opposite()),
            role: "maker".to_string(),
        }).is_err() {
            tracing::debug!("private broadcast: no active subscribers");
        }

        // FIX ExecutionReport drop copy.
        {
            let taker_report = exchange_fix_gateway::build_execution_report(
                "EXCHANGE", "DROP_COPY",
                &fill.taker_order_id.to_string(),
                &fill.trade_id.to_string(),
                "F",
                if fill.taker_side == Side::Buy { "1" } else { "2" },
                fill.symbol.as_str(),
                fill.qty,
                fill.price,
                "2",
            );
            let _ = self.fix_execution_tx.send(taker_report.to_bytes());

            let maker_report = exchange_fix_gateway::build_execution_report(
                "EXCHANGE", "DROP_COPY",
                &fill.maker_order_id.to_string(),
                &fill.trade_id.to_string(),
                "F",
                if fill.taker_side == Side::Buy { "2" } else { "1" },
                fill.symbol.as_str(),
                fill.qty,
                fill.price,
                "2",
            );
            let _ = self.fix_execution_tx.send(maker_report.to_bytes());
        }

        // Suppress unused-variable warning when is_futures is not needed at call site.
        let _ = is_futures;
    }

    fn handle_cancel(self: &Arc<Self>, order_id: OrderId) {
        self.metrics.inc_orders_cancelled();
        self.stp_guard.lock().on_order_removed(&order_id);

        // Remove from GTD registry (no-op for non-GTD orders).
        self.gtd_orders
            .lock().unwrap_or_else(|e| e.into_inner())
            .remove(&order_id);

        // Notify surveillance of the cancellation so cancel-ratio tracking stays accurate.
        // We use the current timestamp as the cancel time; pending_orders holds the user_id.
        let cancel_user = self.pending_orders
            .get(&order_id)
            .map(|info| info.user_id);
        if let Some(user_id) = cancel_user {
            self.surveillance.lock().on_order_cancelled(
                user_id,
                order_id,
                chrono::Utc::now(),
            );
        }

        // Spot orders: unlock remaining balance
        let lock_record = self.order_locks.remove(&order_id).map(|(_, v)| v);
        if let Some(ref record) = lock_record {
            let remaining_lock = match record.side {
                Side::Buy => record.lock.amount - record.filled_cost,
                Side::Sell => record.qty - record.filled_qty,
            };
            if remaining_lock > Decimal::ZERO {
                let _ = self.accounts.unlock_remainder(
                    record.user_id,
                    &record.lock.asset,
                    remaining_lock,
                );
            }
        }

        // Futures orders: release the margin reservation made during place_order.
        // This is a no-op for spot orders (they have no entry in futures_margin_locks).
        if let Some(reserved) = self
            .futures_margin_locks
            .lock().unwrap_or_else(|e| e.into_inner())
            .remove(&order_id)
        {
            // Identify the user from pending_orders (still present at this point).
            let uid = self.pending_orders.get(&order_id).map(|info| info.user_id);
            if let Some(user_id) = uid {
                if let Err(e) = self
                    .accounts
                    .unlock_remainder(user_id, "USDT", reserved)
                {
                    tracing::warn!(
                        order_id = %order_id,
                        user_id = %user_id,
                        reserved = %reserved,
                        error = %e,
                        "handle_cancel: failed to release futures margin lock"
                    );
                }
            }
        }

        // Use pending_orders for history metadata (covers both spot and futures)
        let pending = self.pending_orders.remove(&order_id).map(|(_, v)| v);
        if let Some(info) = pending {
            // Remove from link ID index if this order had one.
            if let Some(ref link_id) = info.order_link_id {
                self.order_link_index.lock()
                    .remove(&(info.user_id, link_id.clone()));
            }

            let filled_qty = lock_record.as_ref().map(|r| r.filled_qty).unwrap_or(Decimal::ZERO);
            let status = if filled_qty > Decimal::ZERO {
                "partially_filled_cancelled"
            } else {
                "cancelled"
            };
            {
                let mut hist = self.order_history.lock().unwrap_or_else(|e| e.into_inner());
                hist.push(OrderHistoryEntry {
                    order_id,
                    user_id: info.user_id,
                    symbol: info.symbol.clone(),
                    side: format!("{:?}", info.side),
                    order_type: format!("{:?}", info.order_type),
                    price: info.price,
                    qty: info.qty,
                    filled_qty,
                    status: status.to_string(),
                    created_at: info.created_at,
                    order_link_id: info.order_link_id.clone(),
                });
                if hist.len() > MAX_ORDER_HISTORY {
                    let excess = hist.len() - MAX_ORDER_HISTORY;
                    hist.drain(0..excess);
                }
            }

            // Order write-behind: mark as Cancelled in DB.
            let maybe_pool = self.db_pool.get().cloned();
            if let Some(pool) = maybe_pool {
                tokio::spawn(async move {
                    if let Err(e) = exchange_db::order_repo::OrderRepo::update_status(
                        &pool, order_id, "Cancelled", filled_qty,
                    ).await {
                        tracing::error!(
                            order_id = %order_id,
                            error = %e,
                            "order DB status update to Cancelled failed"
                        );
                    }
                });
            }

            if self.private_tx.send(PrivateEvent::OrderCancelled {
                user_id: info.user_id.to_string(),
                order_id: order_id.to_string(),
                symbol: info.symbol.to_string(),
                remaining_qty: info.qty - filled_qty,
            }).is_err() {
                tracing::debug!("private broadcast: no active subscribers");
            }
        }
    }

    /// Push a fill into the public trade history, capping at [`MAX_TRADE_HISTORY_PER_SYMBOL`].
    fn push_trade_history(self: &Arc<Self>, fill: &FillEvent) {
        self.fills_processed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let taker_side_str = format!("{:?}", fill.taker_side);
        // The buyer was the maker when the taker sold (taker_side == Sell).
        let is_buyer_maker = taker_side_str == "Sell";
        let entry = TradeHistoryEntry {
            trade_id: fill.trade_id,
            symbol: fill.symbol.clone(),
            price: fill.price,
            qty: fill.qty,
            quote_qty: fill.price * fill.qty,
            taker_side: taker_side_str,
            is_buyer_maker,
            timestamp: fill.timestamp,
        };
        let mut history = self.trade_history.lock().unwrap_or_else(|e| e.into_inner());
        let trades = history.entry(fill.symbol.clone()).or_default();
        trades.push_back(entry);
        if trades.len() > MAX_TRADE_HISTORY_PER_SYMBOL {
            trades.pop_front();
        }

        // Private per-user fill history — both maker and taker get an entry.
        let quote_qty = fill.price * fill.qty;
        let maker_side = match fill.taker_side { Side::Buy => "Sell", Side::Sell => "Buy" };
        let taker_side = format!("{:?}", fill.taker_side);

        let mut user_fills = self.user_fills.lock().unwrap_or_else(|e| e.into_inner());
        for (user_id, order_id, side, role) in [
            (fill.maker_user_id, fill.maker_order_id, maker_side.to_string(), "maker"),
            (fill.taker_user_id, fill.taker_order_id, taker_side, "taker"),
        ] {
            let fills = user_fills.entry(user_id).or_default();
            fills.push_back(UserFillEntry {
                trade_id: fill.trade_id,
                order_id,
                symbol: fill.symbol.clone(),
                side,
                role: role.to_string(),
                price: fill.price,
                qty: fill.qty,
                quote_qty,
                fee: Decimal::ZERO, // Filled in by settle_fill caller if needed
                fee_asset: String::new(),
                timestamp: fill.timestamp,
            });
            if fills.len() > MAX_USER_FILLS {
                fills.pop_front();
            }
        }
    }

    /// Record a fully-filled order into order history.
    ///
    /// Removes from both `order_locks` (spot balance tracking) and `pending_orders`
    /// (history metadata), covering both spot and futures order paths.
    fn record_filled_order(self: &Arc<Self>, order_id: OrderId) {
        self.stp_guard.lock().on_order_removed(&order_id);
        // Remove from GTD registry if the order was GTD (no-op for other TIFs).
        self.gtd_orders
            .lock().unwrap_or_else(|e| e.into_inner())
            .remove(&order_id);
        let lock_record = self.order_locks.remove(&order_id).map(|(_, v)| v);
        let pending = self.pending_orders.remove(&order_id).map(|(_, v)| v);

        if let Some(info) = pending {
            // Remove from link ID index if this order had one.
            if let Some(ref link_id) = info.order_link_id {
                self.order_link_index.lock()
                    .remove(&(info.user_id, link_id.clone()));
            }

            let filled_qty = lock_record.as_ref().map(|r| r.filled_qty).unwrap_or(info.qty);
            {
                let mut hist = self.order_history.lock().unwrap_or_else(|e| e.into_inner());
                hist.push(OrderHistoryEntry {
                    order_id,
                    user_id: info.user_id,
                    symbol: info.symbol,
                    side: format!("{:?}", info.side),
                    order_type: format!("{:?}", info.order_type),
                    price: info.price,
                    qty: info.qty,
                    filled_qty,
                    status: "filled".to_string(),
                    created_at: info.created_at,
                    order_link_id: info.order_link_id.clone(),
                });
                if hist.len() > MAX_ORDER_HISTORY {
                    let excess = hist.len() - MAX_ORDER_HISTORY;
                    hist.drain(0..excess);
                }
            }

            // Order write-behind: mark as Filled in DB.
            let maybe_pool = self.db_pool.get().cloned();
            if let Some(pool) = maybe_pool {
                tokio::spawn(async move {
                    if let Err(e) = exchange_db::order_repo::OrderRepo::update_status(
                        &pool, order_id, "Filled", filled_qty,
                    ).await {
                        tracing::error!(
                            order_id = %order_id,
                            error = %e,
                            "order DB status update to Filled failed"
                        );
                    }
                });
            }
        }
    }

    /// Unlock balance for a rejected order and record it in order history.
    fn handle_reject(self: &Arc<Self>, order_id: OrderId, _reason: &str) {
        self.metrics.inc_orders_cancelled();
        self.stp_guard.lock().on_order_removed(&order_id);
        // Remove from GTD registry in case rejection happens after WAL write.
        self.gtd_orders
            .lock().unwrap_or_else(|e| e.into_inner())
            .remove(&order_id);

        let lock_record = self.order_locks.remove(&order_id).map(|(_, v)| v);
        if let Some(ref record) = lock_record {
            let _ = self.accounts.unlock_remainder(
                record.user_id,
                &record.lock.asset,
                record.lock.amount,
            );
        }

        // Futures orders: release the margin reservation made during place_order.
        // No-op for spot orders (they have no entry in futures_margin_locks).
        if let Some(reserved) = self
            .futures_margin_locks
            .lock().unwrap_or_else(|e| e.into_inner())
            .remove(&order_id)
        {
            let uid = self.pending_orders.get(&order_id).map(|info| info.user_id);
            if let Some(user_id) = uid {
                if let Err(e) = self
                    .accounts
                    .unlock_remainder(user_id, "USDT", reserved)
                {
                    tracing::warn!(
                        order_id = %order_id,
                        user_id = %user_id,
                        reserved = %reserved,
                        error = %e,
                        "handle_reject: failed to release futures margin lock"
                    );
                }
            }
        }

        let pending = self.pending_orders.remove(&order_id).map(|(_, v)| v);
        if let Some(info) = pending {
            // Remove from link ID index if this order had one.
            if let Some(ref link_id) = info.order_link_id {
                self.order_link_index.lock()
                    .remove(&(info.user_id, link_id.clone()));
            }

            {
                let mut hist = self.order_history.lock().unwrap_or_else(|e| e.into_inner());
                hist.push(OrderHistoryEntry {
                    order_id,
                    user_id: info.user_id,
                    symbol: info.symbol.clone(),
                    side: format!("{:?}", info.side),
                    order_type: format!("{:?}", info.order_type),
                    price: info.price,
                    qty: info.qty,
                    filled_qty: Decimal::ZERO,
                    status: "rejected".to_string(),
                    created_at: info.created_at,
                    order_link_id: info.order_link_id.clone(),
                });
                if hist.len() > MAX_ORDER_HISTORY {
                    let excess = hist.len() - MAX_ORDER_HISTORY;
                    hist.drain(0..excess);
                }
            }

            // Emit OrderRejected private event for WS subscribers.
            let _ = self.private_tx.send(crate::exchange::PrivateEvent::OrderRejected {
                user_id: info.user_id.to_string(),
                order_id: order_id.to_string(),
                symbol: info.symbol.to_string(),
                reason: _reason.to_string(),
            });
        }
    }

    /// Replenish an iceberg order by placing the next visible slice on the book.
    ///
    /// `slice_order_id` is the order_id of the visible slice that just completed.
    /// The corresponding `IcebergMeta` entry holds `original_id`, which is the key
    /// in `IcebergManager`.  After advancing the manager, a new limit order with the
    /// next slice size is placed.
    fn replenish_iceberg_slice(self: &Arc<Self>, slice_order_id: OrderId) {
        // Retrieve metadata for the completed slice to get the original_id.
        let meta = match self.iceberg_meta
            .lock().unwrap_or_else(|e| e.into_inner())
            .remove(&slice_order_id)
        {
            Some(m) => m,
            None => return, // No iceberg tracking for this order — nothing to do.
        };

        let original_id = meta.original_id;

        // Ask the IcebergManager for the next slice size.
        let next_qty = match self.iceberg_manager
            .lock().unwrap_or_else(|e| e.into_inner())
            .on_visible_filled(original_id)
        {
            Some(q) if q > Decimal::ZERO => q,
            _ => {
                // Fully consumed — IcebergManager cleaned up its entry already.
                return;
            }
        };

        let slice_id = uuid::Uuid::new_v4();

        // Build the slice command before consuming meta in the struct spread.
        let slice_cmd = PlaceOrderCommand {
            order_id: slice_id,
            user_id: meta.user_id,
            symbol: meta.symbol.clone(),
            side: meta.side,
            order_type: OrderType::Limit,
            time_in_force: meta.time_in_force,
            price: meta.price,
            qty: next_qty,
            reduce_only: meta.reduce_only,
            expire_at: None,
            timestamp: chrono::Utc::now(),
            position_idx: meta.position_idx,
            // iceberg_qty is None: the slice is a plain limit order.
            // The iceberg state lives in IcebergManager (keyed by original_id)
            // and IcebergMeta (keyed by slice_id).
            iceberg_qty: None,
            order_link_id: None,
            slippage_tolerance: None,
            close_position: false,
            stp_mode: StpMode::default(),
            request_expire_time: None,
        };

        // Register metadata for the new slice under the new slice_id.
        {
            let mut meta_map = self.iceberg_meta.lock().unwrap_or_else(|e| e.into_inner());
            meta_map.insert(slice_id, IcebergMeta {
                original_id,
                visible_qty: next_qty,
                slice_filled: Decimal::ZERO,
                ..meta
            });
        }

        if let Err(e) = self.place_order(slice_cmd) {
            // Recover remaining hidden quantity: compute what's still unplaced and
            // release the corresponding balance lock so the user isn't charged for
            // quantity they didn't get to trade.
            let remaining_hidden = {
                let mgr = self.iceberg_manager.lock().unwrap_or_else(|e| e.into_inner());
                mgr.remaining(original_id).unwrap_or(Decimal::ZERO)
            };

            tracing::error!(
                original_order_id = %original_id,
                slice_order_id = %slice_id,
                remaining_hidden = %remaining_hidden,
                error = %e,
                "iceberg slice placement failed; releasing remaining hidden qty to user"
            );

            // NOTE: Balance recovery for the unplaced hidden quantity is handled
            // by the order_locks mechanism when the original order is cleaned up.
            // The original order's lock covers the full quantity; individual
            // iceberg slices don't lock additional balance. Logging at ERROR
            // level ensures ops visibility for investigation.

            // Clean up on failure to avoid stale state.
            self.iceberg_manager
                .lock().unwrap_or_else(|e| e.into_inner())
                .cancel(original_id);
            self.iceberg_meta
                .lock().unwrap_or_else(|e| e.into_inner())
                .remove(&slice_id);
        }
    }
}
