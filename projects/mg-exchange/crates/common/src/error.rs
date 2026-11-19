use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExchangeError {
    #[error("Insufficient balance: need {needed}, have {available}")]
    InsufficientBalance {
        needed: rust_decimal::Decimal,
        available: rust_decimal::Decimal,
    },

    #[error("Order not found: {0}")]
    OrderNotFound(uuid::Uuid),

    #[error("Symbol not found: {0}")]
    SymbolNotFound(String),

    #[error("Invalid price: {0}")]
    InvalidPrice(String),

    #[error("Invalid quantity: {0}")]
    InvalidQuantity(String),

    #[error("Order would self-trade")]
    SelfTrade,

    #[error("Post-only order would take liquidity")]
    PostOnlyWouldTake,

    #[error("FOK order cannot be fully filled")]
    FokCannotFill,

    #[error("Account not found: {0}")]
    AccountNotFound(uuid::Uuid),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Internal error: {0}")]
    Internal(String),
}
