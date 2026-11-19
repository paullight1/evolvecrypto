use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use exchange_common::{Price, Qty, Side, Symbol, UserId};

/// Which oracle price triggers this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerBy {
    /// Last traded price on this exchange.
    LastPrice,
    /// Fair-value mark price (used for PnL / liquidation).
    MarkPrice,
    /// External spot index price.
    IndexPrice,
}

/// The flavor of conditional order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerOrderType {
    /// Stop: triggers and places a limit order.
    StopLimit,
    /// Stop: triggers and places a market order.
    StopMarket,
    /// Take-profit: triggers and places a limit order.
    TakeProfit,
    /// Take-profit: triggers and places a market order.
    TakeProfitMarket,
    /// Trailing stop: triggers when price retraces by `trailing_delta` from peak/trough.
    TrailingStop,
}

/// Execution type when a TP or SL trigger fires.
///
/// - `Market` (default): the child order is a market order (immediate fill at best price).
/// - `Limit`: the child order is a limit order at `tp_limit_price` / `sl_limit_price`.
///   The limit price must be provided when `Limit` is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TpSlOrderType {
    /// Execute as a market order when the trigger fires (default).
    #[default]
    Market,
    /// Execute as a limit order when the trigger fires; requires a limit price.
    Limit,
}

/// Specifies whether a trailing stop uses a fixed-amount or percentage-based delta.
///
/// - `Fixed`: `trailing_delta` is an absolute price distance (e.g., $500).
/// - `Percentage`: `trailing_delta` is a percentage of the extreme price (e.g., 1.0 = 1%).
///
/// Both modes track the same high-water mark (sell) / low-water mark (buy) but differ
/// in how the trigger threshold is computed from the extreme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TrailType {
    /// Trigger when price retraces by a fixed amount from the extreme.
    #[default]
    Fixed,
    /// Trigger when price retraces by a percentage of the extreme.
    Percentage,
}

/// Lifecycle state of a trigger order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerStatus {
    /// Waiting for the trigger condition.
    Active,
    /// Condition met — a matching-engine command has been emitted.
    Triggered,
    /// Cancelled by user or system before triggering.
    Cancelled,
    /// Past its `expires_at` timestamp without triggering.
    Expired,
}

/// A conditional (stop / take-profit / trailing) order that lives outside the
/// matching engine and converts to a regular command once its price condition
/// is met.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerOrder {
    pub id: Uuid,
    pub user_id: UserId,
    pub symbol: Symbol,
    pub trigger_type: TriggerOrderType,
    pub side: Side,

    /// The reference price that activates this order.
    pub trigger_price: Price,

    /// Limit price to use after triggering.
    /// `None` means the converted order is a market order.
    pub order_price: Option<Price>,

    pub qty: Qty,

    /// Which price feed drives the trigger check.
    pub trigger_by: TriggerBy,

    /// Futures: if `true` the converted order may only reduce an existing
    /// position, never open or enlarge one.
    pub reduce_only: bool,

    /// Futures: close the entire position when this order triggers.
    pub close_on_trigger: bool,

    /// For `TrailingStop` only: how far price must retrace from the
    /// peak (sell) or trough (buy) before triggering.
    pub trailing_delta: Option<Decimal>,

    /// For `TrailingStop` only: whether `trailing_delta` is a fixed amount or a percentage.
    #[serde(default)]
    pub trail_type: TrailType,

    // --- TP/SL limit order fields ---

    /// Limit price used when the TP fires as a limit order.
    /// Only meaningful when `tp_order_type == TpSlOrderType::Limit`.
    pub tp_limit_price: Option<Price>,

    /// Limit price used when the SL fires as a limit order.
    /// Only meaningful when `sl_order_type == TpSlOrderType::Limit`.
    pub sl_limit_price: Option<Price>,

    /// Execution type for the take-profit leg (default: Market).
    #[serde(default)]
    pub tp_order_type: TpSlOrderType,

    /// Execution type for the stop-loss leg (default: Market).
    #[serde(default)]
    pub sl_order_type: TpSlOrderType,

    pub status: TriggerStatus,

    pub created_at: DateTime<Utc>,
    pub triggered_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
}

impl TriggerOrder {
    /// Returns `true` if this order is still waiting for its condition.
    pub fn is_active(&self) -> bool {
        self.status == TriggerStatus::Active
    }
}
