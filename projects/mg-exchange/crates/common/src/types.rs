use std::sync::Arc;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Price in decimal. All arithmetic uses Decimal — NEVER f64 for money.
pub type Price = Decimal;

/// Quantity in decimal (base asset units).
pub type Qty = Decimal;

/// Unique order identifier.
pub type OrderId = Uuid;

/// Unique user identifier.
pub type UserId = Uuid;

/// Unique trade identifier.
pub type TradeId = u64;

/// Monotonic sequence number for matching engine events.
pub type SeqNo = u64;

/// Trading pair symbol (e.g., "BTCUSDT").
///
/// Backed by `Arc<str>` rather than `String` so that `.clone()` is a cheap
/// atomic reference-count increment (~5 ns) instead of a heap allocation +
/// memcpy (~50–100 ns). Hash and Eq delegate to the underlying `str` slice,
/// so HashMap keys are identical to the `String` variant. Serde serialises to
/// and deserialises from a plain string — byte-identical to the previous
/// `String` representation under both `serde_json` and `bincode`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Symbol(pub Arc<str>);

impl Symbol {
    /// Create a new `Symbol` from any string-like value.
    ///
    /// Accepts `&str`, `String`, `&String`, `Arc<str>`, etc. — anything that
    /// implements `AsRef<str>`. The inner `Arc<str>` is allocated once and then
    /// cheaply reference-counted on every `.clone()`.
    pub fn new(s: impl AsRef<str>) -> Self {
        Self(Arc::from(s.as_ref()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Symbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.0)
    }
}

/// Order side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    pub fn opposite(&self) -> Self {
        match self {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        }
    }
}

/// Order type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderType {
    /// Limit order — rests on the book if not immediately filled.
    Limit,
    /// Market order — fills immediately, remainder cancelled.
    Market,
}

/// Self-trade prevention mode.
///
/// Default is `ExpireTaker` matching Bybit production default — when an
/// order would match against one of the user's own resting orders, the
/// incoming (taker) order is cancelled and the resting (maker) order is
/// preserved. This is the safest default for retail users: protects
/// against accidental self-crossing while minimizing disruption to
/// existing orders on the book. Institutional users who explicitly want
/// different semantics (`ExpireMaker`, `ExpireBoth`, or `None` for
/// algorithmic strategies that require self-crossing) must set `stp_mode`
/// explicitly on each order.
///
/// **Footgun warning:** any `PlaceOrderCommand` constructed with
/// `..Default::default()` or via struct-update syntax will silently
/// inherit `ExpireTaker` semantics. Internal system orders (liquidation,
/// funding settlement, algo execution) that need to match against ANY
/// resting order regardless of owner should set `stp_mode: StpMode::None`
/// explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum StpMode {
    /// No self-trade prevention. Taker matches against any resting order,
    /// including the user's own. Required for internal system orders and
    /// algorithmic strategies that intentionally self-cross.
    None,
    /// Cancel the incoming (taker) order on a self-trade (default).
    #[default]
    ExpireTaker,
    /// Cancel the resting (maker) order on a self-trade.
    ExpireMaker,
    /// Cancel both orders on a self-trade.
    ExpireBoth,
}

/// Time-in-force policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeInForce {
    /// Good till cancelled (default).
    Gtc,
    /// Immediate or cancel — fill what you can, cancel the rest.
    Ioc,
    /// Fill or kill — fill entirely or cancel entirely.
    Fok,
    /// Post only — rejected if would take liquidity (maker only).
    PostOnly,
    /// Good till date — automatically cancelled at `expire_at`.
    Gtd,
}

impl Default for TimeInForce {
    fn default() -> Self {
        Self::Gtc
    }
}

/// Order status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    /// Accepted but not yet in the book (in command queue).
    Pending,
    /// Resting on the order book.
    Open,
    /// Partially filled, still on the book.
    PartiallyFilled,
    /// Completely filled.
    Filled,
    /// Cancelled by user or system.
    Cancelled,
    /// Rejected (insufficient balance, post-only would take, etc.).
    Rejected,
}

/// An order as it lives in the matching engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: OrderId,
    pub user_id: UserId,
    pub symbol: Symbol,
    pub side: Side,
    pub order_type: OrderType,
    pub time_in_force: TimeInForce,
    pub price: Price,
    pub qty: Qty,
    pub filled_qty: Qty,
    pub status: OrderStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Expiration time for GTD orders. `None` for all other TIF types.
    #[serde(default)]
    pub expire_at: Option<chrono::DateTime<chrono::Utc>>,
    /// User-provided client order ID.
    #[serde(default)]
    pub order_link_id: Option<String>,
}

impl Order {
    pub fn remaining_qty(&self) -> Qty {
        self.qty - self.filled_qty
    }

    pub fn is_fully_filled(&self) -> bool {
        self.filled_qty >= self.qty
    }
}

/// A command entering the matching engine or a durable balance-critical event.
///
/// The WAL stores all variants so that balance mutations can be replayed on
/// crash recovery alongside order commands.  The matching engine only processes
/// `PlaceOrder` and `CancelOrder`; the remaining variants are balance events
/// that are replayed by the account manager layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    PlaceOrder(PlaceOrderCommand),
    CancelOrder(CancelOrderCommand),
    /// A confirmed on-chain deposit that has been credited to a user's balance.
    BalanceDeposit {
        user_id: UserId,
        asset: String,
        amount: Decimal,
        /// External reference — tx hash, deposit record ID, etc.
        reference: String,
    },
    /// A processed withdrawal that has been debited from a user's balance.
    BalanceWithdraw {
        user_id: UserId,
        asset: String,
        amount: Decimal,
        /// External reference — withdrawal ID, tx hash, etc.
        reference: String,
    },
    /// A periodic funding payment applied to a perpetual futures position.
    FundingPayment {
        user_id: UserId,
        asset: String,
        /// Signed amount: positive = received, negative = paid.
        amount: Decimal,
        /// Annualised funding rate at the time of this settlement.
        rate: Decimal,
        /// The symbol this payment relates to (e.g. "BTCUSDT").
        symbol: String,
    },
    /// A periodic snapshot of open perpetual positions, written to the WAL for
    /// crash recovery.  The matching engine ignores this variant.
    PositionSnapshot {
        positions: Vec<crate::futures::Position>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceOrderCommand {
    pub order_id: OrderId,
    pub user_id: UserId,
    pub symbol: Symbol,
    pub side: Side,
    pub order_type: OrderType,
    pub time_in_force: TimeInForce,
    /// Price for limit orders. Ignored for market orders.
    pub price: Price,
    /// Quantity in base asset.
    pub qty: Qty,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// When `true`, the order may only reduce an existing position.
    /// It is rejected if no opposite-side position exists or if `qty` exceeds position size.
    #[serde(default)]
    pub reduce_only: bool,
    /// Expiration time for GTD (Good-Till-Date) orders. `None` for all other TIF types.
    #[serde(default)]
    pub expire_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Position index for hedge mode (0=one-way, 1=long, 2=short).
    #[serde(default)]
    pub position_idx: u8,
    /// For iceberg orders: the visible portion submitted to the book at a time.
    /// Total quantity is in `qty`. Must be positive and less than `qty`.
    #[serde(default)]
    pub iceberg_qty: Option<Decimal>,
    /// User-provided client order ID for tracking and idempotency.
    #[serde(default)]
    pub order_link_id: Option<String>,
    /// Slippage tolerance for market orders (percentage, e.g., 1.0 = 1%).
    /// When set, market orders are converted to IOC limit orders with a price
    /// cap at BBO ± slippage_pct% before submission to the matching engine.
    #[serde(default)]
    pub slippage_tolerance: Option<Decimal>,
    /// When true, close the entire position regardless of the `qty` field.
    /// Sets `qty` to the full open position size and forces `reduce_only = true`.
    #[serde(default)]
    pub close_position: bool,
    /// Self-trade prevention mode.
    #[serde(default)]
    pub stp_mode: StpMode,
    /// Request expiration time — the order is rejected if the server processes
    /// it after this timestamp. Useful for stale-order protection.
    #[serde(default)]
    pub request_expire_time: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelOrderCommand {
    pub order_id: OrderId,
    pub user_id: UserId,
    pub symbol: Symbol,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Command to amend (modify) an existing resting order.
///
/// If `new_price` is `None`, the current price is preserved.
/// If `new_qty` is `None`, the current quantity is preserved.
///
/// When price changes, the order loses time priority (moved to back of queue at new price).
/// When only quantity decreases, time priority is preserved.
///
/// The exchange processes this as an atomic cancel-and-resubmit within a single critical
/// section so there is no balance-unlock gap visible externally.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmendOrderCommand {
    pub order_id: OrderId,
    pub user_id: UserId,
    pub symbol: Symbol,
    pub new_price: Option<Price>,
    pub new_qty: Option<Qty>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Events emitted by the matching engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MatchEvent {
    /// An order was accepted and placed on the book.
    OrderAccepted {
        order_id: OrderId,
        symbol: Symbol,
        seq: SeqNo,
    },
    /// A trade occurred between two orders.
    Fill(FillEvent),
    /// An order was cancelled.
    OrderCancelled {
        order_id: OrderId,
        symbol: Symbol,
        remaining_qty: Qty,
        seq: SeqNo,
    },
    /// An order was rejected.
    OrderRejected {
        order_id: OrderId,
        symbol: Symbol,
        reason: std::borrow::Cow<'static, str>,
        seq: SeqNo,
    },
    /// A cancel request was rejected (e.g. order already filled or not found).
    CancelRejected {
        order_id: OrderId,
        symbol: Symbol,
        reason: std::borrow::Cow<'static, str>,
        seq: SeqNo,
    },
}

/// A fill (trade) between maker and taker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillEvent {
    pub trade_id: TradeId,
    pub symbol: Symbol,
    pub price: Price,
    pub qty: Qty,
    pub maker_order_id: OrderId,
    pub taker_order_id: OrderId,
    pub maker_user_id: UserId,
    pub taker_user_id: UserId,
    /// Side of the TAKER (aggressor).
    pub taker_side: Side,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub seq: SeqNo,
}

/// Market category — distinguishes spot from perpetual futures products.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProductType {
    Spot,
    Perpetual,
}

/// Product (trading pair) configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Product {
    pub symbol: Symbol,
    pub base_asset: String,
    pub quote_asset: String,
    pub product_type: ProductType,
    /// Minimum price increment (e.g., 0.01 for BTC/USDT).
    pub tick_size: Decimal,
    /// Minimum quantity increment (e.g., 0.00001 for BTC).
    pub lot_size: Decimal,
    /// Minimum order value in quote asset.
    pub min_notional: Decimal,
    /// Maker fee rate (negative = rebate).
    pub maker_fee: Decimal,
    /// Taker fee rate.
    pub taker_fee: Decimal,
    /// Maximum single-order quantity in base asset. `None` = unlimited.
    #[serde(default)]
    pub max_order_qty: Option<Decimal>,
    /// Maximum single-order notional in quote asset. `None` = unlimited.
    #[serde(default)]
    pub max_order_notional: Option<Decimal>,
    /// Maximum leverage for perpetual positions. `None` = use global default.
    #[serde(default)]
    pub max_leverage: Option<Decimal>,
}

/// Account status — controls what operations a user account can perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AccountStatus {
    /// Normal operation.
    #[default]
    Active,
    /// Account is frozen — no trading or withdrawals.
    Frozen,
    /// Account is suspended pending compliance review.
    Suspended,
    /// Account has partial restrictions (e.g. withdraw-only).
    Restricted,
}

impl AccountStatus {
    /// Returns `true` if the account is allowed to place or cancel orders.
    pub fn can_trade(self) -> bool {
        matches!(self, Self::Active)
    }

    /// Returns `true` if the account is allowed to initiate withdrawals.
    pub fn can_withdraw(self) -> bool {
        matches!(self, Self::Active | Self::Restricted)
    }
}
