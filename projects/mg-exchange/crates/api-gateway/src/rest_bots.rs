//! Advanced trading bot REST handlers.
//!
//! Implements five bot types that complement the existing Grid and DCA bots:
//!
//! 1. **Infinity Grid** — unbounded-upper grid that extends upward as price rises.
//! 2. **Futures Grid** — perpetual-futures grid with long/short/neutral bias and leverage.
//! 3. **Spot-Futures Arbitrage** — basis arbitrage: buy spot + sell futures when spread widens.
//! 4. **Martingale** — scales into position on drawdowns and takes profit at average + pct.
//! 5. **Slicing** — executes a large order in randomised slices over a given duration.
//!
//! All handlers are registered in `server::build_router` under `/api/v1/bots/*`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use exchange_common::Symbol;
use exchange_trading_bots::BotStatus;

use crate::auth::AuthenticatedUser;
use crate::rest::AppState;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn bad_request(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"error": msg.into()})),
    )
}

fn not_found(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": msg.into()})),
    )
}

// ── Domain types ──────────────────────────────────────────────────────────────

/// Direction bias for a futures grid bot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FuturesGridSide {
    /// More buy levels below current price (bullish).
    Long,
    /// More sell levels above current price (bearish).
    Short,
    /// Symmetric buy and sell levels (market-neutral).
    Neutral,
}

/// State of the spot-futures arbitrage position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArbState {
    /// No open position — watching for entry.
    Watching,
    /// Entered: long spot / short futures.
    Entered,
}

/// A single Martingale layer: the price level at which it triggers and the
/// accumulated order ID once placed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MartingaleLayer {
    /// Layer index (0-based). Layer 0 is the entry order.
    pub layer: u32,
    /// Trigger price for this layer (entry price for layer 0; entry - drop * layer for later layers).
    pub trigger_price: Decimal,
    /// Quantity assigned to this layer: `initial_qty * multiplier^layer`.
    pub qty: Decimal,
    /// Exchange order ID once submitted; `None` if not yet placed.
    pub order_id: Option<Uuid>,
    /// Whether this layer has been filled.
    pub filled: bool,
}

/// An infinity grid bot — like a regular grid but without an upper bound.
///
/// New levels are added dynamically as price rises; the grid grows upward.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfinityGridBot {
    pub id: Uuid,
    pub user_id: Uuid,
    pub symbol: Symbol,
    /// Bottom of the grid range.
    pub lower_price: Decimal,
    /// Quantity per grid level (constant).
    pub qty_per_level: Decimal,
    /// Spacing between levels as a percentage (e.g. `0.01` = 1 %).
    pub grid_spacing_pct: Decimal,
    /// Highest grid level currently tracked.
    pub highest_level: Decimal,
    /// All grid lines placed so far (buy_price → optional order_id).
    pub grid_lines: Vec<InfinityGridLine>,
    pub status: BotStatus,
    pub realized_pnl: Decimal,
    pub total_trades: u64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfinityGridLine {
    pub price: Decimal,
    pub buy_order_id: Option<Uuid>,
    pub sell_order_id: Option<Uuid>,
}

/// A futures grid bot that operates on perpetual contracts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuturesGridBot {
    pub id: Uuid,
    pub user_id: Uuid,
    pub symbol: Symbol,
    pub lower_price: Decimal,
    pub upper_price: Decimal,
    /// Number of grid levels.
    pub grids: u32,
    /// Quantity per grid level (in contracts).
    pub qty_per_level: Decimal,
    /// Leverage applied to the position.
    pub leverage: u32,
    pub side: FuturesGridSide,
    pub status: BotStatus,
    pub realized_pnl: Decimal,
    pub total_trades: u64,
    pub created_at: DateTime<Utc>,
}

/// A spot-futures arbitrage bot that exploits the basis between spot and perp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbBot {
    pub id: Uuid,
    pub user_id: Uuid,
    /// The spot symbol (e.g. "BTCUSDT").
    pub spot_symbol: Symbol,
    /// The perpetual futures symbol (e.g. "BTCUSDT-PERP" or "BTCUSDT").
    pub futures_symbol: Symbol,
    /// Spread percentage above which the bot opens an arbitrage position.
    pub entry_spread_pct: Decimal,
    /// Spread percentage below which the bot closes the position.
    pub exit_spread_pct: Decimal,
    /// Notional quantity traded on each leg.
    pub qty: Decimal,
    pub arb_state: ArbState,
    /// Total realised profit across all completed arb cycles.
    pub total_profit: Decimal,
    pub entries: u64,
    pub exits: u64,
    pub status: BotStatus,
    pub created_at: DateTime<Utc>,
}

/// A martingale bot that scales into a losing position and closes at average + pct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MartingaleBot {
    pub id: Uuid,
    pub user_id: Uuid,
    pub symbol: Symbol,
    /// "buy" or "sell".
    pub side: String,
    /// Quantity for layer 0.
    pub initial_qty: Decimal,
    /// Quantity multiplier per additional layer.
    pub multiplier: Decimal,
    /// Maximum number of layers (limits total exposure).
    pub max_layers: u32,
    /// Percentage price drop from previous layer that triggers the next one.
    pub price_drop_pct: Decimal,
    /// Percentage above average entry price at which the bot takes profit.
    pub take_profit_pct: Decimal,
    /// Layer definitions computed at bot creation.
    pub layers: Vec<MartingaleLayer>,
    /// Average entry price (volume-weighted across filled layers).
    pub avg_entry: Decimal,
    /// Total quantity filled so far.
    pub total_qty_filled: Decimal,
    pub status: BotStatus,
    pub realized_pnl: Decimal,
    pub created_at: DateTime<Utc>,
}

/// A slicing bot that executes a large order in randomised slices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlicingBot {
    pub id: Uuid,
    pub user_id: Uuid,
    pub symbol: Symbol,
    /// "buy" or "sell".
    pub side: String,
    /// Total quantity to execute.
    pub total_qty: Decimal,
    /// Execution window in minutes.
    pub duration_minutes: u64,
    /// Minimum slice size.
    pub min_slice: Decimal,
    /// Maximum slice size.
    pub max_slice: Decimal,
    /// Whether to randomise slice sizes; always true in Phase 1.
    pub randomize: bool,
    /// Quantity already executed.
    pub executed_qty: Decimal,
    /// Timestamp of the next scheduled slice.
    pub next_slice_at: DateTime<Utc>,
    /// Deadline — bot stops after this point regardless of remaining qty.
    pub deadline: DateTime<Utc>,
    pub status: BotStatus,
    pub created_at: DateTime<Utc>,
}

/// Discriminated union stored in `Exchange::advanced_bots`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AdvancedBot {
    InfinityGrid(InfinityGridBot),
    FuturesGrid(FuturesGridBot),
    Arb(ArbBot),
    Martingale(MartingaleBot),
    Slicing(SlicingBot),
}

impl AdvancedBot {
    pub fn id(&self) -> Uuid {
        match self {
            Self::InfinityGrid(b) => b.id,
            Self::FuturesGrid(b) => b.id,
            Self::Arb(b) => b.id,
            Self::Martingale(b) => b.id,
            Self::Slicing(b) => b.id,
        }
    }

    pub fn user_id(&self) -> Uuid {
        match self {
            Self::InfinityGrid(b) => b.user_id,
            Self::FuturesGrid(b) => b.user_id,
            Self::Arb(b) => b.user_id,
            Self::Martingale(b) => b.user_id,
            Self::Slicing(b) => b.user_id,
        }
    }

    pub fn status(&self) -> BotStatus {
        match self {
            Self::InfinityGrid(b) => b.status,
            Self::FuturesGrid(b) => b.status,
            Self::Arb(b) => b.status,
            Self::Martingale(b) => b.status,
            Self::Slicing(b) => b.status,
        }
    }

    pub fn status_mut(&mut self) -> &mut BotStatus {
        match self {
            Self::InfinityGrid(b) => &mut b.status,
            Self::FuturesGrid(b) => &mut b.status,
            Self::Arb(b) => &mut b.status,
            Self::Martingale(b) => &mut b.status,
            Self::Slicing(b) => &mut b.status,
        }
    }

    pub fn bot_type(&self) -> &'static str {
        match self {
            Self::InfinityGrid(_) => "infinity_grid",
            Self::FuturesGrid(_) => "futures_grid",
            Self::Arb(_) => "arb",
            Self::Martingale(_) => "martingale",
            Self::Slicing(_) => "slicing",
        }
    }

    /// Collect all live (unfilled, non-None) order IDs across the bot's internal state.
    ///
    /// Used when stopping a bot: every returned order ID must be cancelled on
    /// the matching engine to prevent orphaned resting orders.
    pub fn live_order_ids(&self) -> Vec<Uuid> {
        match self {
            Self::InfinityGrid(b) => {
                let mut ids = Vec::new();
                for line in &b.grid_lines {
                    if let Some(id) = line.buy_order_id {
                        ids.push(id);
                    }
                    if let Some(id) = line.sell_order_id {
                        ids.push(id);
                    }
                }
                ids
            }
            Self::FuturesGrid(_) => {
                // FuturesGrid uses the same grid_lines pattern as regular GridBot
                // which is managed by the matching engine directly. No internal order tracking.
                Vec::new()
            }
            Self::Arb(_) => {
                // Arb bot places orders on-demand per cycle, no persistent resting orders.
                Vec::new()
            }
            Self::Martingale(b) => {
                b.layers
                    .iter()
                    .filter(|l| !l.filled)
                    .filter_map(|l| l.order_id)
                    .collect()
            }
            Self::Slicing(_) => {
                // Slicing bot places one order at a time; current order is transient.
                Vec::new()
            }
        }
    }

    /// Serialize the bot into a summary JSON value for list/get responses.
    pub fn to_summary(&self) -> serde_json::Value {
        match self {
            Self::InfinityGrid(b) => serde_json::json!({
                "bot_id": b.id.to_string(),
                "type": "infinity_grid",
                "symbol": b.symbol.to_string(),
                "lower_price": b.lower_price,
                "qty_per_level": b.qty_per_level,
                "grid_spacing_pct": b.grid_spacing_pct,
                "highest_level": b.highest_level,
                "grid_lines": b.grid_lines.len(),
                "status": bot_status_str(b.status),
                "realized_pnl": b.realized_pnl,
                "total_trades": b.total_trades,
                "created_at": b.created_at.to_rfc3339(),
            }),
            Self::FuturesGrid(b) => serde_json::json!({
                "bot_id": b.id.to_string(),
                "type": "futures_grid",
                "symbol": b.symbol.to_string(),
                "lower_price": b.lower_price,
                "upper_price": b.upper_price,
                "grids": b.grids,
                "qty_per_level": b.qty_per_level,
                "leverage": b.leverage,
                "side": format!("{:?}", b.side).to_lowercase(),
                "status": bot_status_str(b.status),
                "realized_pnl": b.realized_pnl,
                "total_trades": b.total_trades,
                "created_at": b.created_at.to_rfc3339(),
            }),
            Self::Arb(b) => serde_json::json!({
                "bot_id": b.id.to_string(),
                "type": "arb",
                "spot_symbol": b.spot_symbol.to_string(),
                "futures_symbol": b.futures_symbol.to_string(),
                "entry_spread_pct": b.entry_spread_pct,
                "exit_spread_pct": b.exit_spread_pct,
                "qty": b.qty,
                "arb_state": format!("{:?}", b.arb_state).to_lowercase(),
                "total_profit": b.total_profit,
                "entries": b.entries,
                "exits": b.exits,
                "status": bot_status_str(b.status),
                "created_at": b.created_at.to_rfc3339(),
            }),
            Self::Martingale(b) => serde_json::json!({
                "bot_id": b.id.to_string(),
                "type": "martingale",
                "symbol": b.symbol.to_string(),
                "side": b.side,
                "initial_qty": b.initial_qty,
                "multiplier": b.multiplier,
                "max_layers": b.max_layers,
                "price_drop_pct": b.price_drop_pct,
                "take_profit_pct": b.take_profit_pct,
                "avg_entry": b.avg_entry,
                "total_qty_filled": b.total_qty_filled,
                "layers_count": b.layers.len(),
                "status": bot_status_str(b.status),
                "realized_pnl": b.realized_pnl,
                "created_at": b.created_at.to_rfc3339(),
            }),
            Self::Slicing(b) => serde_json::json!({
                "bot_id": b.id.to_string(),
                "type": "slicing",
                "symbol": b.symbol.to_string(),
                "side": b.side,
                "total_qty": b.total_qty,
                "executed_qty": b.executed_qty,
                "remaining_qty": b.total_qty - b.executed_qty,
                "duration_minutes": b.duration_minutes,
                "min_slice": b.min_slice,
                "max_slice": b.max_slice,
                "randomize": b.randomize,
                "deadline": b.deadline.to_rfc3339(),
                "next_slice_at": b.next_slice_at.to_rfc3339(),
                "status": bot_status_str(b.status),
                "created_at": b.created_at.to_rfc3339(),
            }),
        }
    }
}

fn bot_status_str(status: BotStatus) -> &'static str {
    match status {
        BotStatus::Active => "active",
        BotStatus::Paused => "paused",
        BotStatus::Stopped => "stopped",
    }
}

// ── Martingale helpers ────────────────────────────────────────────────────────

/// Compute the layer definitions for a martingale bot given an entry price.
///
/// Layer 0 is placed at `entry_price`. Each subsequent layer triggers when
/// price drops `price_drop_pct` percent below the **previous** layer's price.
/// Quantity grows geometrically: `initial_qty * multiplier^layer`.
fn build_martingale_layers(
    entry_price: Decimal,
    initial_qty: Decimal,
    multiplier: Decimal,
    max_layers: u32,
    price_drop_pct: Decimal,
) -> Vec<MartingaleLayer> {
    let mut layers = Vec::with_capacity(max_layers as usize);
    let drop_factor = Decimal::ONE - price_drop_pct / Decimal::ONE_HUNDRED;

    let mut current_price = entry_price;
    let mut current_qty = initial_qty;

    for layer in 0..max_layers {
        let trigger_price = if layer == 0 {
            entry_price
        } else {
            current_price * drop_factor
        };

        layers.push(MartingaleLayer {
            layer,
            trigger_price,
            qty: current_qty,
            order_id: None,
            filled: false,
        });

        current_price = trigger_price;
        current_qty = current_qty * multiplier;
    }

    layers
}

// ── 1. Infinity Grid ──────────────────────────────────────────────────────────

/// Request body for `POST /api/v1/bots/infinity-grid`.
#[derive(Debug, Deserialize)]
pub struct CreateInfinityGridRequest {
    pub symbol: String,
    pub lower_price: Decimal,
    pub qty_per_level: Decimal,
    /// Percentage spacing between adjacent grid levels (e.g. `1.0` = 1 %).
    pub grid_spacing_pct: Decimal,
}

/// `POST /api/v1/bots/infinity-grid` — create an infinity grid bot.
///
/// The grid has no upper bound; new sell levels are added dynamically above
/// `highest_level` each time the bot executor tick detects upward price
/// movement beyond the current top level.
pub async fn create_infinity_grid(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<CreateInfinityGridRequest>,
) -> impl IntoResponse {
    if req.lower_price <= Decimal::ZERO {
        return bad_request("lower_price must be positive");
    }
    if req.qty_per_level <= Decimal::ZERO {
        return bad_request("qty_per_level must be positive");
    }
    if req.grid_spacing_pct <= Decimal::ZERO || req.grid_spacing_pct >= Decimal::ONE_HUNDRED {
        return bad_request("grid_spacing_pct must be in (0, 100)");
    }

    let user_id = auth.0;
    let now = Utc::now();
    let symbol = Symbol::new(&req.symbol);

    // Seed the first grid line at `lower_price`.
    let first_line = InfinityGridLine {
        price: req.lower_price,
        buy_order_id: None,
        sell_order_id: None,
    };

    let bot = InfinityGridBot {
        id: Uuid::new_v4(),
        user_id,
        symbol: symbol.clone(),
        lower_price: req.lower_price,
        qty_per_level: req.qty_per_level,
        grid_spacing_pct: req.grid_spacing_pct,
        highest_level: req.lower_price,
        grid_lines: vec![first_line],
        status: BotStatus::Active,
        realized_pnl: Decimal::ZERO,
        total_trades: 0,
        created_at: now,
    };

    let bot_id = bot.id;
    exchange
        .advanced_bots
        .lock().unwrap_or_else(|e| e.into_inner())
        .push(AdvancedBot::InfinityGrid(bot));

    tracing::info!(
        user_id = %user_id,
        bot_id = %bot_id,
        symbol = %req.symbol,
        lower_price = %req.lower_price,
        grid_spacing_pct = %req.grid_spacing_pct,
        "infinity grid bot created"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "bot_id": bot_id.to_string(),
            "type": "infinity_grid",
            "status": "active",
        })),
    )
}

// ── 2. Futures Grid ───────────────────────────────────────────────────────────

/// Request body for `POST /api/v1/bots/futures-grid`.
#[derive(Debug, Deserialize)]
pub struct CreateFuturesGridRequest {
    pub symbol: String,
    pub lower_price: Decimal,
    pub upper_price: Decimal,
    pub grids: u32,
    pub qty_per_level: Decimal,
    pub leverage: u32,
    /// "long", "short", or "neutral".
    pub side: String,
}

/// `POST /api/v1/bots/futures-grid` — create a futures grid bot.
///
/// Unlike a spot grid the bot trades perpetual futures contracts and applies
/// leverage.  Long-biased bots place more buy levels; short-biased bots place
/// more sell levels.
pub async fn create_futures_grid(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<CreateFuturesGridRequest>,
) -> impl IntoResponse {
    if req.upper_price <= req.lower_price {
        return bad_request("upper_price must be greater than lower_price");
    }
    if req.grids < 2 {
        return bad_request("grids must be at least 2");
    }
    if req.qty_per_level <= Decimal::ZERO {
        return bad_request("qty_per_level must be positive");
    }
    if req.leverage == 0 || req.leverage > 125 {
        return bad_request("leverage must be between 1 and 125");
    }

    let side = match req.side.to_lowercase().as_str() {
        "long" => FuturesGridSide::Long,
        "short" => FuturesGridSide::Short,
        "neutral" => FuturesGridSide::Neutral,
        _ => return bad_request("side must be 'long', 'short', or 'neutral'"),
    };

    let user_id = auth.0;
    let now = Utc::now();

    let bot = FuturesGridBot {
        id: Uuid::new_v4(),
        user_id,
        symbol: Symbol::new(&req.symbol),
        lower_price: req.lower_price,
        upper_price: req.upper_price,
        grids: req.grids,
        qty_per_level: req.qty_per_level,
        leverage: req.leverage,
        side,
        status: BotStatus::Active,
        realized_pnl: Decimal::ZERO,
        total_trades: 0,
        created_at: now,
    };

    let bot_id = bot.id;
    exchange
        .advanced_bots
        .lock().unwrap_or_else(|e| e.into_inner())
        .push(AdvancedBot::FuturesGrid(bot));

    tracing::info!(
        user_id = %user_id,
        bot_id = %bot_id,
        symbol = %req.symbol,
        leverage = %req.leverage,
        side = %req.side,
        "futures grid bot created"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "bot_id": bot_id.to_string(),
            "type": "futures_grid",
            "status": "active",
        })),
    )
}

// ── 3. Spot-Futures Arbitrage ─────────────────────────────────────────────────

/// Request body for `POST /api/v1/bots/arb`.
#[derive(Debug, Deserialize)]
pub struct CreateArbBotRequest {
    pub spot_symbol: String,
    pub futures_symbol: String,
    /// Spread % above which the bot opens: (futures_price - spot_price) / spot_price * 100.
    pub entry_spread_pct: Decimal,
    /// Spread % below which the bot closes (convergence target).
    pub exit_spread_pct: Decimal,
    pub qty: Decimal,
}

/// `POST /api/v1/bots/arb` — create a spot-futures arbitrage bot.
///
/// The bot watches the basis (spot vs. perp spread).  When
/// `spread > entry_spread_pct` it opens by buying spot and shorting the perp.
/// When `spread < exit_spread_pct` it closes both legs, capturing the
/// convergence as profit.
pub async fn create_arb_bot(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<CreateArbBotRequest>,
) -> impl IntoResponse {
    if req.entry_spread_pct <= Decimal::ZERO {
        return bad_request("entry_spread_pct must be positive");
    }
    if req.exit_spread_pct < Decimal::ZERO {
        return bad_request("exit_spread_pct must be non-negative");
    }
    if req.exit_spread_pct >= req.entry_spread_pct {
        return bad_request("exit_spread_pct must be less than entry_spread_pct");
    }
    if req.qty <= Decimal::ZERO {
        return bad_request("qty must be positive");
    }

    let user_id = auth.0;
    let now = Utc::now();

    let bot = ArbBot {
        id: Uuid::new_v4(),
        user_id,
        spot_symbol: Symbol::new(&req.spot_symbol),
        futures_symbol: Symbol::new(&req.futures_symbol),
        entry_spread_pct: req.entry_spread_pct,
        exit_spread_pct: req.exit_spread_pct,
        qty: req.qty,
        arb_state: ArbState::Watching,
        total_profit: Decimal::ZERO,
        entries: 0,
        exits: 0,
        status: BotStatus::Active,
        created_at: now,
    };

    let bot_id = bot.id;
    exchange
        .advanced_bots
        .lock().unwrap_or_else(|e| e.into_inner())
        .push(AdvancedBot::Arb(bot));

    tracing::info!(
        user_id = %user_id,
        bot_id = %bot_id,
        spot = %req.spot_symbol,
        futures = %req.futures_symbol,
        entry_spread_pct = %req.entry_spread_pct,
        exit_spread_pct = %req.exit_spread_pct,
        "arb bot created"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "bot_id": bot_id.to_string(),
            "type": "arb",
            "status": "active",
        })),
    )
}

// ── 4. Martingale ─────────────────────────────────────────────────────────────

/// Request body for `POST /api/v1/bots/martingale`.
#[derive(Debug, Deserialize)]
pub struct CreateMartingaleRequest {
    pub symbol: String,
    /// "buy" or "sell".
    pub side: String,
    pub initial_qty: Decimal,
    /// Quantity multiplier per additional layer (e.g. `2.0` = double each time).
    pub multiplier: Decimal,
    /// Maximum number of layers. Must be in `[1, 10]`.
    pub max_layers: u32,
    /// Percentage price drop from the previous layer that triggers the next.
    pub price_drop_pct: Decimal,
    /// Percentage above the weighted-average entry that closes all positions.
    pub take_profit_pct: Decimal,
}

/// `POST /api/v1/bots/martingale` — create a martingale bot.
///
/// Layer 0 is placed immediately at the current market price (via the bot
/// executor).  Each subsequent layer adds `initial_qty * multiplier^layer`
/// when price falls `price_drop_pct` below the previous layer.  All positions
/// are closed when price reaches `avg_entry * (1 + take_profit_pct / 100)`.
pub async fn create_martingale_bot(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<CreateMartingaleRequest>,
) -> impl IntoResponse {
    if req.initial_qty <= Decimal::ZERO {
        return bad_request("initial_qty must be positive");
    }
    if req.multiplier <= Decimal::ONE {
        return bad_request("multiplier must be greater than 1");
    }
    if req.max_layers == 0 || req.max_layers > 10 {
        return bad_request("max_layers must be between 1 and 10");
    }
    if req.price_drop_pct <= Decimal::ZERO || req.price_drop_pct >= Decimal::ONE_HUNDRED {
        return bad_request("price_drop_pct must be in (0, 100)");
    }
    if req.take_profit_pct <= Decimal::ZERO {
        return bad_request("take_profit_pct must be positive");
    }
    match req.side.to_lowercase().as_str() {
        "buy" | "sell" => {}
        _ => return bad_request("side must be 'buy' or 'sell'"),
    }

    let user_id = auth.0;
    let now = Utc::now();

    // Layers are computed relative to a placeholder entry price of ZERO.
    // The executor will re-compute actual trigger prices once the current
    // market price is known and layer 0 fires.
    let layers = build_martingale_layers(
        Decimal::ZERO, // entry_price resolved at first execution tick
        req.initial_qty,
        req.multiplier,
        req.max_layers,
        req.price_drop_pct,
    );

    let bot = MartingaleBot {
        id: Uuid::new_v4(),
        user_id,
        symbol: Symbol::new(&req.symbol),
        side: req.side.to_lowercase(),
        initial_qty: req.initial_qty,
        multiplier: req.multiplier,
        max_layers: req.max_layers,
        price_drop_pct: req.price_drop_pct,
        take_profit_pct: req.take_profit_pct,
        layers,
        avg_entry: Decimal::ZERO,
        total_qty_filled: Decimal::ZERO,
        status: BotStatus::Active,
        realized_pnl: Decimal::ZERO,
        created_at: now,
    };

    let bot_id = bot.id;
    exchange
        .advanced_bots
        .lock().unwrap_or_else(|e| e.into_inner())
        .push(AdvancedBot::Martingale(bot));

    tracing::info!(
        user_id = %user_id,
        bot_id = %bot_id,
        symbol = %req.symbol,
        max_layers = %req.max_layers,
        multiplier = %req.multiplier,
        "martingale bot created"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "bot_id": bot_id.to_string(),
            "type": "martingale",
            "status": "active",
        })),
    )
}

// ── 5. Slicing Bot ────────────────────────────────────────────────────────────

/// Request body for `POST /api/v1/bots/slicing`.
#[derive(Debug, Deserialize)]
pub struct CreateSlicingRequest {
    pub symbol: String,
    /// "buy" or "sell".
    pub side: String,
    pub total_qty: Decimal,
    /// Total window over which slices are spread.
    pub duration_minutes: u64,
    pub min_slice: Decimal,
    pub max_slice: Decimal,
    /// Whether to randomise sizes and intervals. Defaults to `true`.
    #[serde(default = "default_true")]
    pub randomize: bool,
}

fn default_true() -> bool {
    true
}

/// `POST /api/v1/bots/slicing` — create a slicing bot.
///
/// The bot breaks `total_qty` into random-sized slices between `min_slice`
/// and `max_slice` and places them as market orders at random intervals within
/// `duration_minutes`.  This deliberately avoids the fixed-interval pattern
/// of TWAP, making the execution less predictable to the market.
pub async fn create_slicing_bot(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Json(req): Json<CreateSlicingRequest>,
) -> impl IntoResponse {
    if req.total_qty <= Decimal::ZERO {
        return bad_request("total_qty must be positive");
    }
    if req.duration_minutes == 0 {
        return bad_request("duration_minutes must be at least 1");
    }
    if req.min_slice <= Decimal::ZERO {
        return bad_request("min_slice must be positive");
    }
    if req.max_slice < req.min_slice {
        return bad_request("max_slice must be >= min_slice");
    }
    if req.max_slice > req.total_qty {
        return bad_request("max_slice must not exceed total_qty");
    }
    match req.side.to_lowercase().as_str() {
        "buy" | "sell" => {}
        _ => return bad_request("side must be 'buy' or 'sell'"),
    }

    let user_id = auth.0;
    let now = Utc::now();
    let deadline = now + chrono::Duration::minutes(req.duration_minutes as i64);

    // Schedule the first slice for a few seconds from now so the bot executor
    // has time to read the bot state on its next tick.
    let next_slice_at = now + chrono::Duration::seconds(5);

    let bot = SlicingBot {
        id: Uuid::new_v4(),
        user_id,
        symbol: Symbol::new(&req.symbol),
        side: req.side.to_lowercase(),
        total_qty: req.total_qty,
        duration_minutes: req.duration_minutes,
        min_slice: req.min_slice,
        max_slice: req.max_slice,
        randomize: req.randomize,
        executed_qty: Decimal::ZERO,
        next_slice_at,
        deadline,
        status: BotStatus::Active,
        created_at: now,
    };

    let bot_id = bot.id;
    exchange
        .advanced_bots
        .lock().unwrap_or_else(|e| e.into_inner())
        .push(AdvancedBot::Slicing(bot));

    tracing::info!(
        user_id = %user_id,
        bot_id = %bot_id,
        symbol = %req.symbol,
        total_qty = %req.total_qty,
        duration_minutes = %req.duration_minutes,
        "slicing bot created"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "bot_id": bot_id.to_string(),
            "type": "slicing",
            "status": "active",
            "deadline": deadline.to_rfc3339(),
        })),
    )
}

// ── Common: list all advanced bots ───────────────────────────────────────────

/// `GET /api/v1/bots/advanced` — list all advanced bots for the authenticated user.
pub async fn list_advanced_bots(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;

    let bots = exchange
        .advanced_bots
        .lock().unwrap_or_else(|e| e.into_inner());

    let result: Vec<serde_json::Value> = bots
        .iter()
        .filter(|b| b.user_id() == user_id)
        .map(|b| b.to_summary())
        .collect();

    (StatusCode::OK, Json(serde_json::json!(result)))
}

// ── Common: stop an advanced bot ─────────────────────────────────────────────

/// `DELETE /api/v1/bots/advanced/{id}` — stop an advanced bot.
///
/// Sets the bot status to `Stopped`.  The bot executor will no longer act on
/// it, and it will be pruned from memory on the next executor tick.
pub async fn stop_advanced_bot(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let bot_id = match Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => return bad_request("invalid bot id"),
    };

    let user_id = auth.0;

    let mut bots = exchange
        .advanced_bots
        .lock().unwrap_or_else(|e| e.into_inner());

    let Some(bot) = bots
        .iter_mut()
        .find(|b| b.id() == bot_id && b.user_id() == user_id)
    else {
        return not_found("bot not found");
    };

    if bot.status() == BotStatus::Stopped {
        return bad_request("bot is already stopped");
    }

    *bot.status_mut() = BotStatus::Stopped;

    tracing::info!(
        bot_id = %bot_id,
        user_id = %user_id,
        bot_type = %bot.bot_type(),
        "advanced bot stopped"
    );

    (
        StatusCode::OK,
        Json(serde_json::json!({"status": "stopped"})),
    )
}

// ── Background executor ───────────────────────────────────────────────────────

/// Tick the advanced bot executor every `interval_ms` milliseconds.
///
/// Each bot type has its own execution logic:
///
/// - **InfinityGrid**: extends grid lines upward when price rises above
///   `highest_level`.
/// - **FuturesGrid**: places initial grid orders on first tick.
/// - **Arb**: compares spot vs. futures mid-price and opens/closes the arb.
/// - **Martingale**: places layer 0 on first tick; triggers subsequent layers
///   when price drops; closes all when price reaches the TP target.
/// - **Slicing**: fires the next randomised market-order slice when
///   `next_slice_at` is in the past.
///
/// Stopped bots are pruned at the end of each tick.
pub async fn run_advanced_bot_executor(
    exchange: std::sync::Arc<crate::exchange::Exchange>,
    interval_ms: u64,
) {
    use exchange_common::{OrderType, PlaceOrderCommand, Side, StpMode, TimeInForce};
    use rand_core::RngCore;
    use tokio::time::{interval, Duration};

    let mut ticker = interval(Duration::from_millis(interval_ms));

    loop {
        ticker.tick().await;

        let now = Utc::now();

        // Snapshot the IDs to avoid holding the lock across the entire tick.
        let bot_ids: Vec<Uuid> = {
            let bots = exchange
                .advanced_bots
                .lock().unwrap_or_else(|e| e.into_inner());
            bots.iter()
                .filter(|b| b.status() == BotStatus::Active)
                .map(|b| b.id())
                .collect()
        };

        for bot_id in bot_ids {
            // ── helper: get mid-price for a symbol ─────────────────────────
            let mid_price_for = |sym: &Symbol| -> Option<Decimal> {
                let pool = exchange.engine_pool.read().unwrap_or_else(|e| e.into_inner());
                match pool.snapshot(sym, 1) {
                    Ok(snap) => match (snap.bids.first(), snap.asks.first()) {
                        (Some((bid, _)), Some((ask, _))) => {
                            Some((*bid + *ask) / Decimal::TWO)
                        }
                        (Some((bid, _)), None) => Some(*bid),
                        (None, Some((ask, _))) => Some(*ask),
                        (None, None) => None,
                    },
                    Err(_) => None,
                }
            };

            // ── helper: place a market order ───────────────────────────────
            let place_market = |user_id: Uuid, sym: Symbol, side: Side, qty: Decimal| {
                let cmd = PlaceOrderCommand {
                    order_id: Uuid::new_v4(),
                    user_id,
                    symbol: sym,
                    side,
                    order_type: OrderType::Market,
                    time_in_force: TimeInForce::Ioc,
                    price: Decimal::ZERO,
                    qty,
                    reduce_only: false,
                    expire_at: None,
                    timestamp: now,
                    position_idx: 0,
                    iceberg_qty: None,
                    order_link_id: None,
                    slippage_tolerance: None,
                    close_position: false,
                    stp_mode: StpMode::default(),
                    request_expire_time: None,
                };
                exchange.place_order(cmd)
            };

            // ── helper: place a limit order ────────────────────────────────
            let place_limit = |user_id: Uuid, sym: Symbol, side: Side, price: Decimal, qty: Decimal| {
                let cmd = PlaceOrderCommand {
                    order_id: Uuid::new_v4(),
                    user_id,
                    symbol: sym,
                    side,
                    order_type: OrderType::Limit,
                    time_in_force: TimeInForce::Gtc,
                    price,
                    qty,
                    reduce_only: false,
                    expire_at: None,
                    timestamp: now,
                    position_idx: 0,
                    iceberg_qty: None,
                    order_link_id: None,
                    slippage_tolerance: None,
                    close_position: false,
                    stp_mode: StpMode::default(),
                    request_expire_time: None,
                };
                exchange.place_order(cmd)
            };

            // Dispatch to per-type logic.
            let bot_type = {
                let bots = exchange
                    .advanced_bots
                    .lock().unwrap_or_else(|e| e.into_inner());
                bots.iter()
                    .find(|b| b.id() == bot_id)
                    .map(|b| b.bot_type())
            };

            match bot_type {
                None => continue, // Bot removed between snapshot and now.
                Some("infinity_grid") => {
                    // ── Infinity Grid ──────────────────────────────────────
                    let (symbol, user_id, highest, spacing_pct, qty_per_level, lower) = {
                        let bots = exchange
                            .advanced_bots
                            .lock().unwrap_or_else(|e| e.into_inner());
                        let Some(AdvancedBot::InfinityGrid(b)) =
                            bots.iter().find(|b| b.id() == bot_id)
                        else {
                            continue;
                        };
                        (
                            b.symbol.clone(),
                            b.user_id,
                            b.highest_level,
                            b.grid_spacing_pct,
                            b.qty_per_level,
                            b.lower_price,
                        )
                    };

                    let Some(mid) = mid_price_for(&symbol) else {
                        continue;
                    };

                    // Determine if we need more grid lines above the current highest.
                    let multiplier = Decimal::ONE + spacing_pct / Decimal::ONE_HUNDRED;

                    // Collect prices of new levels to add, then add them.
                    let mut new_levels: Vec<Decimal> = Vec::new();
                    let mut next_price = highest * multiplier;
                    while next_price <= mid * (Decimal::ONE + spacing_pct / Decimal::ONE_HUNDRED * Decimal::TWO)
                    {
                        new_levels.push(next_price);
                        next_price = next_price * multiplier;
                    }

                    for level_price in &new_levels {
                        // Place a sell limit at this new level.
                        match place_limit(user_id, symbol.clone(), Side::Sell, *level_price, qty_per_level) {
                            Ok(_) => {
                                tracing::debug!(
                                    bot_id = %bot_id,
                                    price = %level_price,
                                    "infinity grid: added sell level"
                                );
                            }
                            Err(e) => {
                                tracing::warn!(
                                    bot_id = %bot_id,
                                    price = %level_price,
                                    error = %e,
                                    "infinity grid: failed to place sell level"
                                );
                            }
                        }
                    }

                    // Place initial buy orders for the first tick (lines below mid-price).
                    // Only do this if the bot has just one grid line (the seeded lower_price).
                    let needs_buy_init = {
                        let bots = exchange
                            .advanced_bots
                            .lock().unwrap_or_else(|e| e.into_inner());
                        bots.iter()
                            .find(|b| b.id() == bot_id)
                            .and_then(|b| {
                                if let AdvancedBot::InfinityGrid(ig) = b {
                                    Some(ig.grid_lines.len() == 1)
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(false)
                    };

                    if needs_buy_init {
                        // Place one buy level at lower_price.
                        if lower < mid {
                            if let Err(e) =
                                place_limit(user_id, symbol.clone(), Side::Buy, lower, qty_per_level)
                            {
                                tracing::warn!(
                                    bot_id = %bot_id,
                                    price = %lower,
                                    error = %e,
                                    "infinity grid: failed to place initial buy"
                                );
                            }
                        }
                    }

                    // Update highest_level and record new grid lines.
                    if let Some(&new_highest) = new_levels.last() {
                        let mut bots = exchange
                            .advanced_bots
                            .lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(AdvancedBot::InfinityGrid(b)) =
                            bots.iter_mut().find(|b| b.id() == bot_id)
                        {
                            b.highest_level = new_highest;
                            for p in &new_levels {
                                b.grid_lines.push(InfinityGridLine {
                                    price: *p,
                                    buy_order_id: None,
                                    sell_order_id: None,
                                });
                            }
                        }
                    }
                }

                Some("futures_grid") => {
                    // ── Futures Grid ───────────────────────────────────────
                    // On first tick: place initial grid orders similar to spot grid.
                    let needs_init = {
                        let bots = exchange
                            .advanced_bots
                            .lock().unwrap_or_else(|e| e.into_inner());
                        bots.iter()
                            .find(|b| b.id() == bot_id)
                            .map(|b| {
                                if let AdvancedBot::FuturesGrid(fg) = b {
                                    // We use total_trades as a proxy for "already initialised".
                                    fg.total_trades == 0
                                } else {
                                    false
                                }
                            })
                            .unwrap_or(false)
                    };

                    if !needs_init {
                        continue;
                    }

                    let (symbol, user_id, lower, upper, grids, qty, leverage, side) = {
                        let bots = exchange
                            .advanced_bots
                            .lock().unwrap_or_else(|e| e.into_inner());
                        let Some(AdvancedBot::FuturesGrid(b)) =
                            bots.iter().find(|b| b.id() == bot_id)
                        else {
                            continue;
                        };
                        (
                            b.symbol.clone(),
                            b.user_id,
                            b.lower_price,
                            b.upper_price,
                            b.grids,
                            b.qty_per_level,
                            b.leverage,
                            b.side,
                        )
                    };

                    let Some(mid) = mid_price_for(&symbol) else {
                        continue;
                    };

                    let spacing = (upper - lower)
                        / Decimal::from(grids.saturating_sub(1).max(1));

                    // Compute how many buy vs sell levels depending on bias.
                    // For neutral: all levels below mid → buy, above mid → sell.
                    // For long: 2/3 buy, 1/3 sell. For short: 1/3 buy, 2/3 sell.
                    let (buy_weight, sell_weight): (u32, u32) = match side {
                        FuturesGridSide::Long => (2, 1),
                        FuturesGridSide::Short => (1, 2),
                        FuturesGridSide::Neutral => (1, 1),
                    };
                    let _ = buy_weight; // used conceptually; implementation uses mid split
                    let _ = sell_weight;

                    // Set leverage via exchange (fire-and-forget for Phase 1).
                    // We skip the actual leverage API call here and log intent.
                    tracing::info!(
                        bot_id = %bot_id,
                        symbol = %symbol,
                        leverage = %leverage,
                        "futures grid: would set leverage (Phase 1: leverage logged, not applied)"
                    );

                    for i in 0..grids {
                        let price = lower + spacing * Decimal::from(i);
                        let order_side = if price < mid { Side::Buy } else { Side::Sell };

                        // Long-biased: place buy on both sides of mid below a threshold.
                        let final_side = match side {
                            FuturesGridSide::Long => {
                                if price < upper * Decimal::new(2, 0) / Decimal::from(3u32) + lower / Decimal::from(3u32) {
                                    Side::Buy
                                } else {
                                    order_side
                                }
                            }
                            FuturesGridSide::Short => {
                                if price > lower + (upper - lower) / Decimal::from(3u32) {
                                    Side::Sell
                                } else {
                                    order_side
                                }
                            }
                            FuturesGridSide::Neutral => order_side,
                        };

                        if let Err(e) = place_limit(user_id, symbol.clone(), final_side, price, qty) {
                            tracing::warn!(
                                bot_id = %bot_id,
                                price = %price,
                                side = ?final_side,
                                error = %e,
                                "futures grid: failed to place grid order"
                            );
                        }
                    }

                    // Mark as initialised by bumping total_trades.
                    {
                        let mut bots = exchange
                            .advanced_bots
                            .lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(AdvancedBot::FuturesGrid(b)) =
                            bots.iter_mut().find(|b| b.id() == bot_id)
                        {
                            b.total_trades = 1; // sentinel: grid initialised
                        }
                    }

                    tracing::info!(
                        bot_id = %bot_id,
                        symbol = %symbol,
                        grids = %grids,
                        "futures grid bot initial orders placed"
                    );
                }

                Some("arb") => {
                    // ── Spot-Futures Arbitrage ─────────────────────────────
                    let (spot_sym, futures_sym, user_id, entry_pct, exit_pct, qty, arb_state) = {
                        let bots = exchange
                            .advanced_bots
                            .lock().unwrap_or_else(|e| e.into_inner());
                        let Some(AdvancedBot::Arb(b)) =
                            bots.iter().find(|b| b.id() == bot_id)
                        else {
                            continue;
                        };
                        (
                            b.spot_symbol.clone(),
                            b.futures_symbol.clone(),
                            b.user_id,
                            b.entry_spread_pct,
                            b.exit_spread_pct,
                            b.qty,
                            b.arb_state,
                        )
                    };

                    let Some(spot_mid) = mid_price_for(&spot_sym) else {
                        continue;
                    };
                    let Some(futures_mid) = mid_price_for(&futures_sym) else {
                        continue;
                    };

                    // spread_pct = (futures - spot) / spot * 100
                    let spread_pct = if spot_mid > Decimal::ZERO {
                        (futures_mid - spot_mid) / spot_mid * Decimal::ONE_HUNDRED
                    } else {
                        Decimal::ZERO
                    };

                    match arb_state {
                        ArbState::Watching => {
                            if spread_pct >= entry_pct {
                                // Open: buy spot, sell futures.
                                let spot_result =
                                    place_market(user_id, spot_sym.clone(), Side::Buy, qty);
                                let fut_result =
                                    place_market(user_id, futures_sym.clone(), Side::Sell, qty);

                                match (spot_result, fut_result) {
                                    (Ok(_), Ok(_)) => {
                                        let mut bots = exchange
                                            .advanced_bots
                                            .lock().unwrap_or_else(|e| e.into_inner());
                                        if let Some(AdvancedBot::Arb(b)) =
                                            bots.iter_mut().find(|b| b.id() == bot_id)
                                        {
                                            b.arb_state = ArbState::Entered;
                                            b.entries += 1;
                                        }
                                        tracing::info!(
                                            bot_id = %bot_id,
                                            spread_pct = %spread_pct,
                                            "arb bot: opened position"
                                        );
                                    }
                                    (Err(e), _) | (_, Err(e)) => {
                                        tracing::error!(
                                            bot_id = %bot_id,
                                            error = %e,
                                            "arb bot: failed to open position"
                                        );
                                    }
                                }
                            }
                        }
                        ArbState::Entered => {
                            if spread_pct <= exit_pct {
                                // Close: sell spot, buy futures.
                                let spot_result =
                                    place_market(user_id, spot_sym.clone(), Side::Sell, qty);
                                let fut_result =
                                    place_market(user_id, futures_sym.clone(), Side::Buy, qty);

                                match (spot_result, fut_result) {
                                    (Ok(_), Ok(_)) => {
                                        let entry_value = spot_mid * qty;
                                        let profit = (entry_pct - spread_pct)
                                            / Decimal::ONE_HUNDRED
                                            * entry_value;

                                        let mut bots = exchange
                                            .advanced_bots
                                            .lock().unwrap_or_else(|e| e.into_inner());
                                        if let Some(AdvancedBot::Arb(b)) =
                                            bots.iter_mut().find(|b| b.id() == bot_id)
                                        {
                                            b.arb_state = ArbState::Watching;
                                            b.exits += 1;
                                            b.total_profit += profit;
                                        }
                                        tracing::info!(
                                            bot_id = %bot_id,
                                            spread_pct = %spread_pct,
                                            estimated_profit = %profit,
                                            "arb bot: closed position"
                                        );
                                    }
                                    (Err(e), _) | (_, Err(e)) => {
                                        tracing::error!(
                                            bot_id = %bot_id,
                                            error = %e,
                                            "arb bot: failed to close position"
                                        );
                                    }
                                }
                            }
                        }
                    }
                }

                Some("martingale") => {
                    // ── Martingale ─────────────────────────────────────────
                    let (symbol, user_id, side_str, initial_qty, multiplier, price_drop_pct, take_profit_pct, max_layers) = {
                        let bots = exchange
                            .advanced_bots
                            .lock().unwrap_or_else(|e| e.into_inner());
                        let Some(AdvancedBot::Martingale(b)) =
                            bots.iter().find(|b| b.id() == bot_id)
                        else {
                            continue;
                        };
                        (
                            b.symbol.clone(),
                            b.user_id,
                            b.side.clone(),
                            b.initial_qty,
                            b.multiplier,
                            b.price_drop_pct,
                            b.take_profit_pct,
                            b.max_layers,
                        )
                    };

                    let Some(mid) = mid_price_for(&symbol) else {
                        continue;
                    };

                    let order_side = if side_str == "buy" { Side::Buy } else { Side::Sell };

                    // --- Layer 0: place the entry order the first time ------
                    let layer0_unfilled = {
                        let bots = exchange
                            .advanced_bots
                            .lock().unwrap_or_else(|e| e.into_inner());
                        bots.iter().find(|b| b.id() == bot_id).map(|b| {
                            if let AdvancedBot::Martingale(m) = b {
                                m.layers
                                    .first()
                                    .map(|l| !l.filled && l.order_id.is_none())
                                    .unwrap_or(false)
                            } else {
                                false
                            }
                        }).unwrap_or(false)
                    };

                    if layer0_unfilled {
                        match place_market(user_id, symbol.clone(), order_side, initial_qty) {
                            Ok(_) => {
                                let mut bots = exchange
                                    .advanced_bots
                                    .lock().unwrap_or_else(|e| e.into_inner());
                                if let Some(AdvancedBot::Martingale(b)) =
                                    bots.iter_mut().find(|b| b.id() == bot_id)
                                {
                                    if let Some(layer) = b.layers.first_mut() {
                                        layer.filled = true;
                                        layer.trigger_price = mid;
                                    }
                                    // Update avg_entry and total_qty.
                                    b.avg_entry = mid;
                                    b.total_qty_filled = initial_qty;
                                    // Rebuild layer trigger prices from the real entry.
                                    let drop_factor = Decimal::ONE - price_drop_pct / Decimal::ONE_HUNDRED;
                                    let mut trigger = mid;
                                    let mut layer_qty = initial_qty * multiplier;
                                    for layer in b.layers.iter_mut().skip(1) {
                                        trigger = trigger * drop_factor;
                                        layer.trigger_price = trigger;
                                        layer.qty = layer_qty;
                                        layer_qty = layer_qty * multiplier;
                                    }
                                }
                                tracing::info!(
                                    bot_id = %bot_id,
                                    entry_price = %mid,
                                    "martingale: layer 0 filled"
                                );
                            }
                            Err(e) => {
                                tracing::error!(
                                    bot_id = %bot_id,
                                    error = %e,
                                    "martingale: layer 0 order failed"
                                );
                            }
                        }
                        continue; // Process TP check on the next tick.
                    }

                    // --- Subsequent layers: trigger on price drop -----------
                    let layers_snapshot: Vec<MartingaleLayer> = {
                        let bots = exchange
                            .advanced_bots
                            .lock().unwrap_or_else(|e| e.into_inner());
                        bots.iter().find(|b| b.id() == bot_id).map(|b| {
                            if let AdvancedBot::Martingale(m) = b {
                                m.layers.clone()
                            } else {
                                vec![]
                            }
                        }).unwrap_or_default()
                    };

                    for (idx, layer) in layers_snapshot.iter().enumerate().skip(1) {
                        if layer.filled || layer.order_id.is_some() {
                            continue;
                        }
                        // Trigger if mid <= layer trigger price (for buy side)
                        // or mid >= layer trigger price (for sell side).
                        let triggered = if order_side == Side::Buy {
                            mid <= layer.trigger_price
                        } else {
                            mid >= layer.trigger_price
                        };

                        if !triggered {
                            continue;
                        }

                        match place_market(user_id, symbol.clone(), order_side, layer.qty) {
                            Ok(_) => {
                                let mut bots = exchange
                                    .advanced_bots
                                    .lock().unwrap_or_else(|e| e.into_inner());
                                if let Some(AdvancedBot::Martingale(b)) =
                                    bots.iter_mut().find(|b| b.id() == bot_id)
                                {
                                    if let Some(l) = b.layers.get_mut(idx) {
                                        l.filled = true;
                                    }
                                    // Update weighted-average entry.
                                    let prev_notional = b.avg_entry * b.total_qty_filled;
                                    let new_notional = mid * layer.qty;
                                    b.total_qty_filled += layer.qty;
                                    if b.total_qty_filled > Decimal::ZERO {
                                        b.avg_entry = (prev_notional + new_notional)
                                            / b.total_qty_filled;
                                    }
                                }
                                tracing::info!(
                                    bot_id = %bot_id,
                                    layer = %idx,
                                    price = %mid,
                                    "martingale: layer triggered"
                                );
                            }
                            Err(e) => {
                                tracing::error!(
                                    bot_id = %bot_id,
                                    layer = %idx,
                                    error = %e,
                                    "martingale: layer order failed"
                                );
                            }
                        }
                    }

                    // --- Take profit check -----------------------------------
                    let (avg_entry, total_qty_filled) = {
                        let bots = exchange
                            .advanced_bots
                            .lock().unwrap_or_else(|e| e.into_inner());
                        bots.iter().find(|b| b.id() == bot_id).map(|b| {
                            if let AdvancedBot::Martingale(m) = b {
                                (m.avg_entry, m.total_qty_filled)
                            } else {
                                (Decimal::ZERO, Decimal::ZERO)
                            }
                        }).unwrap_or((Decimal::ZERO, Decimal::ZERO))
                    };

                    if total_qty_filled <= Decimal::ZERO || avg_entry <= Decimal::ZERO {
                        continue;
                    }

                    let tp_price = avg_entry
                        * (Decimal::ONE + take_profit_pct / Decimal::ONE_HUNDRED);
                    let tp_triggered = if order_side == Side::Buy {
                        mid >= tp_price
                    } else {
                        mid <= avg_entry * (Decimal::ONE - take_profit_pct / Decimal::ONE_HUNDRED)
                    };

                    if tp_triggered {
                        let close_side = if order_side == Side::Buy { Side::Sell } else { Side::Buy };
                        match place_market(user_id, symbol.clone(), close_side, total_qty_filled) {
                            Ok(_) => {
                                let pnl = if order_side == Side::Buy {
                                    (mid - avg_entry) * total_qty_filled
                                } else {
                                    (avg_entry - mid) * total_qty_filled
                                };

                                let mut bots = exchange
                                    .advanced_bots
                                    .lock().unwrap_or_else(|e| e.into_inner());
                                if let Some(AdvancedBot::Martingale(b)) =
                                    bots.iter_mut().find(|b| b.id() == bot_id)
                                {
                                    b.realized_pnl += pnl;
                                    b.status = BotStatus::Stopped;
                                }
                                tracing::info!(
                                    bot_id = %bot_id,
                                    avg_entry = %avg_entry,
                                    exit_price = %mid,
                                    pnl = %pnl,
                                    "martingale: take profit reached — bot stopped"
                                );
                            }
                            Err(e) => {
                                tracing::error!(
                                    bot_id = %bot_id,
                                    error = %e,
                                    "martingale: take profit close order failed"
                                );
                            }
                        }
                    }
                    let _ = max_layers; // used in layer construction only
                }

                Some("slicing") => {
                    // ── Slicing Bot ────────────────────────────────────────
                    let (symbol, user_id, side_str, remaining_qty, min_slice, max_slice, next_slice_at, deadline) = {
                        let bots = exchange
                            .advanced_bots
                            .lock().unwrap_or_else(|e| e.into_inner());
                        let Some(AdvancedBot::Slicing(b)) =
                            bots.iter().find(|b| b.id() == bot_id)
                        else {
                            continue;
                        };
                        (
                            b.symbol.clone(),
                            b.user_id,
                            b.side.clone(),
                            b.total_qty - b.executed_qty,
                            b.min_slice,
                            b.max_slice,
                            b.next_slice_at,
                            b.deadline,
                        )
                    };

                    // Stop if past deadline or fully executed.
                    if now >= deadline || remaining_qty <= Decimal::ZERO {
                        let mut bots = exchange
                            .advanced_bots
                            .lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(AdvancedBot::Slicing(b)) =
                            bots.iter_mut().find(|b| b.id() == bot_id)
                        {
                            b.status = BotStatus::Stopped;
                        }
                        tracing::info!(
                            bot_id = %bot_id,
                            remaining_qty = %remaining_qty,
                            "slicing bot: completed or expired"
                        );
                        continue;
                    }

                    // Only execute when it is time.
                    if now < next_slice_at {
                        continue;
                    }

                    // Determine slice size: random between min and max, capped at remaining.
                    let mut rng = rand_core::OsRng;
                    let range = max_slice - min_slice;
                    let slice_qty = if range > Decimal::ZERO {
                        // Map a u64 into [0, 1) and scale to the range.
                        let rand_fraction = Decimal::from(rng.next_u64())
                            / Decimal::from(u64::MAX);
                        let raw = min_slice + rand_fraction * range;
                        raw.min(remaining_qty)
                    } else {
                        min_slice.min(remaining_qty)
                    };

                    if slice_qty <= Decimal::ZERO {
                        continue;
                    }

                    let order_side = if side_str == "buy" { Side::Buy } else { Side::Sell };

                    match place_market(user_id, symbol.clone(), order_side, slice_qty) {
                        Ok(_) => {
                            // Schedule the next slice: random interval within the
                            // remaining time budget divided by remaining slices.
                            let remaining_secs = (deadline - now).num_seconds().max(1) as u64;
                            let approx_slices_left = {
                                let ratio = remaining_qty / slice_qty;
                                // Convert Decimal to u64 via string to avoid num-traits dependency.
                                ratio
                                    .to_string()
                                    .split('.')
                                    .next()
                                    .and_then(|s| s.parse::<u64>().ok())
                                    .unwrap_or(1)
                                    .max(1)
                            };
                            let avg_interval = (remaining_secs / approx_slices_left).max(5);
                            // Randomise ±50 % around the average interval.
                            let jitter_secs = rng.next_u64() % avg_interval.max(1);
                            let next_secs = avg_interval / 2 + jitter_secs;

                            let mut bots = exchange
                                .advanced_bots
                                .lock().unwrap_or_else(|e| e.into_inner());
                            if let Some(AdvancedBot::Slicing(b)) =
                                bots.iter_mut().find(|b| b.id() == bot_id)
                            {
                                b.executed_qty += slice_qty;
                                b.next_slice_at = now
                                    + chrono::Duration::seconds(next_secs as i64);
                            }

                            tracing::info!(
                                bot_id = %bot_id,
                                slice_qty = %slice_qty,
                                next_secs = %next_secs,
                                "slicing bot: slice placed"
                            );
                        }
                        Err(e) => {
                            tracing::error!(
                                bot_id = %bot_id,
                                error = %e,
                                "slicing bot: slice order failed"
                            );
                        }
                    }
                }

                _ => continue,
            }
        }

        // Prune stopped bots.
        {
            let mut bots = exchange
                .advanced_bots
                .lock().unwrap_or_else(|e| e.into_inner());
            bots.retain(|b| b.status() != BotStatus::Stopped);
        }
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn build_martingale_layers_count_matches_max_layers() {
        let layers = build_martingale_layers(dec!(100), dec!(1), dec!(2), 5, dec!(5));
        assert_eq!(layers.len(), 5);
    }

    #[test]
    fn build_martingale_layer0_at_entry_price() {
        let entry = dec!(50_000);
        let layers = build_martingale_layers(entry, dec!(0.1), dec!(2), 3, dec!(10));
        assert_eq!(layers[0].trigger_price, entry);
    }

    #[test]
    fn build_martingale_layers_trigger_prices_decrease() {
        let layers = build_martingale_layers(dec!(100), dec!(1), dec!(2), 4, dec!(10));
        // Each layer trigger price should be less than the previous.
        for i in 1..layers.len() {
            assert!(
                layers[i].trigger_price < layers[i - 1].trigger_price,
                "layer {} trigger {} should be < layer {} trigger {}",
                i,
                layers[i].trigger_price,
                i - 1,
                layers[i - 1].trigger_price
            );
        }
    }

    #[test]
    fn build_martingale_layers_qty_grows_geometrically() {
        let layers = build_martingale_layers(dec!(100), dec!(1), dec!(2), 4, dec!(5));
        assert_eq!(layers[0].qty, dec!(1));
        assert_eq!(layers[1].qty, dec!(2));
        assert_eq!(layers[2].qty, dec!(4));
        assert_eq!(layers[3].qty, dec!(8));
    }

    #[test]
    fn advanced_bot_id_and_user_id_accessors() {
        let uid = Uuid::new_v4();
        let bot = ArbBot {
            id: Uuid::new_v4(),
            user_id: uid,
            spot_symbol: Symbol::new("BTCUSDT"),
            futures_symbol: Symbol::new("BTCUSDT"),
            entry_spread_pct: dec!(0.5),
            exit_spread_pct: dec!(0.1),
            qty: dec!(1),
            arb_state: ArbState::Watching,
            total_profit: Decimal::ZERO,
            entries: 0,
            exits: 0,
            status: BotStatus::Active,
            created_at: Utc::now(),
        };
        let wrapped = AdvancedBot::Arb(bot.clone());
        assert_eq!(wrapped.id(), bot.id);
        assert_eq!(wrapped.user_id(), uid);
        assert_eq!(wrapped.bot_type(), "arb");
    }

    #[test]
    fn advanced_bot_status_mut_changes_status() {
        let mut bot = AdvancedBot::Slicing(SlicingBot {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            symbol: Symbol::new("ETHUSDT"),
            side: "buy".into(),
            total_qty: dec!(10),
            duration_minutes: 30,
            min_slice: dec!(0.5),
            max_slice: dec!(2),
            randomize: true,
            executed_qty: Decimal::ZERO,
            next_slice_at: Utc::now(),
            deadline: Utc::now() + chrono::Duration::minutes(30),
            status: BotStatus::Active,
            created_at: Utc::now(),
        });

        assert_eq!(bot.status(), BotStatus::Active);
        *bot.status_mut() = BotStatus::Stopped;
        assert_eq!(bot.status(), BotStatus::Stopped);
    }

    #[test]
    fn infinity_grid_to_summary_has_correct_type() {
        let bot = InfinityGridBot {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            symbol: Symbol::new("BTCUSDT"),
            lower_price: dec!(90_000),
            qty_per_level: dec!(0.01),
            grid_spacing_pct: dec!(1),
            highest_level: dec!(90_000),
            grid_lines: vec![],
            status: BotStatus::Active,
            realized_pnl: Decimal::ZERO,
            total_trades: 0,
            created_at: Utc::now(),
        };
        let wrapped = AdvancedBot::InfinityGrid(bot);
        let summary = wrapped.to_summary();
        assert_eq!(summary["type"], "infinity_grid");
    }

    #[test]
    fn futures_grid_bot_summary_includes_leverage() {
        let bot = FuturesGridBot {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            symbol: Symbol::new("BTCUSDT"),
            lower_price: dec!(90_000),
            upper_price: dec!(100_000),
            grids: 10,
            qty_per_level: dec!(0.01),
            leverage: 5,
            side: FuturesGridSide::Long,
            status: BotStatus::Active,
            realized_pnl: Decimal::ZERO,
            total_trades: 0,
            created_at: Utc::now(),
        };
        let wrapped = AdvancedBot::FuturesGrid(bot);
        let summary = wrapped.to_summary();
        assert_eq!(summary["leverage"], 5);
        assert_eq!(summary["type"], "futures_grid");
    }

    #[test]
    fn slicing_bot_summary_computes_remaining_qty() {
        let now = Utc::now();
        let bot = SlicingBot {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            symbol: Symbol::new("SOLUSDT"),
            side: "sell".into(),
            total_qty: dec!(100),
            duration_minutes: 60,
            min_slice: dec!(1),
            max_slice: dec!(5),
            randomize: true,
            executed_qty: dec!(30),
            next_slice_at: now,
            deadline: now + chrono::Duration::minutes(60),
            status: BotStatus::Active,
            created_at: now,
        };
        let wrapped = AdvancedBot::Slicing(bot);
        let summary = wrapped.to_summary();
        assert_eq!(summary["remaining_qty"], serde_json::json!(dec!(70)));
    }
}
