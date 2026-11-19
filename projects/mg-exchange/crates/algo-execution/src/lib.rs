//! Algorithmic execution strategies for large order placement.
//!
//! Provides TWAP and VWAP execution algorithms that split orders into
//! smaller slices to minimise market impact.

pub mod common;
pub mod twap;
pub mod vwap;

pub use common::{AlgoOrder, AlgoStatus, SliceOrderType, SliceStatus};
pub use twap::{TwapConfig, TwapExecution, TwapSlice};
pub use vwap::{VwapConfig, VwapExecution, VwapSlice};
