//! Exchange copy-trading subsystem.
//!
//! # Overview
//! This crate implements the four core building blocks of a copy-trading system:
//!
//! | Module | Responsibility |
//! |---|---|
//! | [`master`] | Master trader registry — registration, stats, leaderboard |
//! | [`follower`] | Follow relation management — follow/unfollow, stop-loss |
//! | [`copier`] | Proportional order generation when a master trades |
//! | [`settlement`] | Profit-sharing settlement between follower and master |
//!
//! # Quick-start
//! ```
//! # use rust_decimal_macros::dec;
//! # use uuid::Uuid;
//! # use exchange_copy_trading::master::MasterRegistry;
//! # use exchange_copy_trading::follower::{FollowerManager, FollowRelation};
//! # use exchange_copy_trading::copier::{TradeCopier, MasterTrade};
//! # use exchange_copy_trading::settlement::calculate_profit_share;
//! # use exchange_common::types::{OrderType, Side, Symbol};
//! #
//! let mut masters = MasterRegistry::new();
//! let mut followers = FollowerManager::with_defaults();
//!
//! let master_id = Uuid::new_v4();
//! let follower_id = Uuid::new_v4();
//!
//! masters.register(master_id, "Alice", dec!(0.10)).unwrap();
//!
//! let rel = followers
//!     .follow(follower_id, master_id, dec!(1000), true, Some(dec!(0.15)), true)
//!     .unwrap();
//!
//! let trade = MasterTrade {
//!     master_id,
//!     symbol: Symbol::new("BTCUSDT"),
//!     side: Side::Buy,
//!     qty: dec!(1),
//!     price: dec!(50000),
//!     order_type: OrderType::Market,
//! };
//!
//! let all_relations: Vec<FollowRelation> = followers
//!     .master_followers(master_id)
//!     .into_iter()
//!     .cloned()
//!     .collect();
//! let copy_orders =
//!     TradeCopier::generate_copy_orders(&trade, &all_relations, dec!(10000));
//!
//! assert_eq!(copy_orders.len(), 1);
//! assert_eq!(copy_orders[0].qty, dec!(0.1)); // 1000/10000 * 1 BTC
//!
//! let (master_cut, follower_net) = calculate_profit_share(dec!(100), dec!(0.10)).unwrap();
//! assert_eq!(master_cut, dec!(10));
//! assert_eq!(follower_net, dec!(90));
//! ```

pub mod copier;
pub mod error;
pub mod follower;
pub mod master;
pub mod settlement;

pub use copier::{CopyOrder, MasterTrade, TradeCopier};
pub use error::CopyTradingError;
pub use follower::{FollowRelation, FollowerManager};
pub use master::{MasterRegistry, MasterTrader};
pub use settlement::{ProfitSettlement, calculate_profit_share, settle};
