//! Error types for the risk engine.

use thiserror::Error;

use exchange_common::types::{Symbol, UserId};

/// Errors that can be returned by the risk engine.
#[derive(Debug, Error)]
pub enum RiskEngineError {
    #[error("Position not found for user {user_id} on {symbol}")]
    PositionNotFound { user_id: UserId, symbol: Symbol },

    #[error("Invalid quantity: {0}")]
    InvalidQuantity(String),

    #[error("Invalid price: {0}")]
    InvalidPrice(String),

    #[error("Invalid leverage: {0}")]
    InvalidLeverage(String),

    #[error("Insufficient margin: need {needed}, have {available}")]
    InsufficientMargin {
        needed: rust_decimal::Decimal,
        available: rust_decimal::Decimal,
    },

    #[error("Symbol config not found: {0}")]
    ConfigNotFound(Symbol),

    #[error("Cannot change position mode while positions are open")]
    CannotChangeMode,
}

/// Convenience alias.
pub type RiskResult<T> = Result<T, RiskEngineError>;
