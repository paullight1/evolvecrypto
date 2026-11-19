//! Exchange risk engine — perpetual futures margin, liquidation, and position management.
//!
//! # Modules
//! - [`margin_calculator`] — pure margin math (initial, maintenance, available, max size)
//! - [`mark_price`] — fair mark price derived from index and last price
//! - [`position_manager`] — position lifecycle (open, increase, reduce, close, flip)
//! - [`liquidation`] — liquidation detection, bankruptcy price, insurance fund
//! - [`adl`] — auto-deleverage ranking, execution, and quantile indicator
//! - [`error`] — error types

pub mod adl;
pub mod error;
pub mod liquidation;
pub mod margin_calculator;
pub mod mark_price;
pub mod options_instruments;
pub mod options_margin;
pub mod options_pricing;
pub mod portfolio_margin;
pub mod position_manager;
pub mod vol_surface;

pub use adl::{adl_quantile, calculate_adl, AdlCandidate, AdlOrder, AdlRanking};
pub use error::{RiskEngineError, RiskResult};
pub use liquidation::{
    bankruptcy_price, partial_liquidation_qty, InsuranceFundUpdate, LiquidationBatch,
    LiquidationEngine, LiquidationEvent, LIQUIDATION_FEE_RATE,
};
pub use margin_calculator::{
    available_margin, is_liquidatable, max_position_size, required_initial_margin,
    required_initial_margin_for_config, required_initial_margin_inverse,
    required_maintenance_margin, required_maintenance_margin_inverse,
};
pub use mark_price::{basis_rate, calculate_mark_price, calculate_mark_price_ema, DEFAULT_MARK_EMA_SPAN};
pub use portfolio_margin::{
    analyse as portfolio_margin_analyse, brackets_for_config, compute_net_delta,
    required_portfolio_margin, NotionalBracket, PortfolioMarginAnalysis, ScenarioResult,
    PORTFOLIO_MARGIN_SAFETY_FACTOR,
};
pub use position_manager::{CloseResult, PositionManager};
