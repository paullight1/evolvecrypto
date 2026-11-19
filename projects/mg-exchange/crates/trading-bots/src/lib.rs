pub mod dca;
pub mod grid;
pub mod rebalancer;

pub use dca::{DcaBot, DcaBotConfig};
pub use grid::{BotStatus, GridBot, GridBotConfig, GridLine, GridSide};
pub use rebalancer::{RebalanceAction, RebalanceDirection, Rebalancer, RebalancerConfig};

use exchange_common::{OrderType, Side, Symbol, UserId};
use rust_decimal::Decimal;
use uuid::Uuid;

/// An order that a bot wants to place on the exchange.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BotOrder {
    pub order_id: Uuid,
    pub user_id: UserId,
    pub symbol: Symbol,
    pub side: Side,
    pub price: Decimal,
    pub qty: Decimal,
    pub order_type: OrderType,
}
