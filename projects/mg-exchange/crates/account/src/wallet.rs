use rust_decimal::Decimal;
use serde::Serialize;

/// Per-coin breakdown within a wallet summary.
#[derive(Debug, Clone, Serialize)]
pub struct CoinBalance {
    pub coin: String,
    /// Total wallet balance (does not include unrealized PnL).
    pub wallet_balance: Decimal,
    /// Available balance (wallet_balance − locked).
    pub available_balance: Decimal,
    /// Amount locked for open orders.
    pub locked: Decimal,
    /// Unrealized PnL from open perpetual positions margined in this coin.
    pub unrealized_pnl: Decimal,
    /// Equity = wallet_balance + unrealized_pnl.
    pub equity: Decimal,
    /// Portion of `locked` held as order margin (same as `locked` in Phase 1).
    pub order_margin: Decimal,
    /// Initial margin consumed by open positions denominated in this coin.
    pub position_initial_margin: Decimal,
    /// Maintenance margin required for positions denominated in this coin.
    pub position_maintenance_margin: Decimal,
    /// Accumulated realized PnL from closed positions (Phase 1: always zero).
    pub cumulative_realized_pnl: Decimal,
    /// Maximum amount that can be withdrawn right now.
    pub available_to_withdraw: Decimal,
}

/// Aggregated wallet summary across all coins.
#[derive(Debug, Clone, Serialize)]
pub struct WalletSummary {
    /// Total account equity expressed in USD.
    pub total_equity_usd: Decimal,
    /// Total wallet balance (without uPnL) expressed in USD.
    pub total_wallet_balance_usd: Decimal,
    /// Total available (unlocked) balance in USD.
    pub total_available_balance_usd: Decimal,
    /// Sum of all position initial margins in USD.
    pub total_initial_margin_usd: Decimal,
    /// Sum of all position maintenance margins in USD.
    pub total_maintenance_margin_usd: Decimal,
    /// Sum of all unrealized PnL values in USD.
    pub total_unrealized_pnl_usd: Decimal,
    /// Account initial-margin rate: total_im / total_equity.
    pub account_im_rate: Decimal,
    /// Account maintenance-margin rate: total_mm / total_equity.
    pub account_mm_rate: Decimal,
    /// Per-coin breakdown.
    pub coins: Vec<CoinBalance>,
}
