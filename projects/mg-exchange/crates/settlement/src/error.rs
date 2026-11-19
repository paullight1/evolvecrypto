//! Settlement-specific error types.

use rust_decimal::Decimal;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SettlementError {
    #[error("Index price must not be zero")]
    ZeroIndexPrice,

    #[error("Margin must not be zero")]
    ZeroMargin,

    #[error("Equity must be positive, got {0}")]
    NonPositiveEquity(Decimal),

    #[error("Price must be positive, got {0}")]
    NonPositivePrice(Decimal),

    #[error("Quantity must not be negative, got {0}")]
    NegativeQuantity(Decimal),

    #[error("Amount must not be negative, got {0}")]
    NegativeAmount(Decimal),

    #[error(
        "Insurance fund insufficient: requested {requested}, available {available}"
    )]
    InsufficientInsuranceFund {
        requested: Decimal,
        available: Decimal,
    },

    #[error(
        "Invalid funding interval {0}h: must be a positive divisor of 24 (1, 2, 3, 4, 6, 8, 12, 24)"
    )]
    InvalidFundingInterval(u32),
}
