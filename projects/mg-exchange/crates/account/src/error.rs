use rust_decimal::Decimal;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AccountError {
    #[error("Insufficient available balance: need {needed}, have {available}")]
    InsufficientBalance { needed: Decimal, available: Decimal },

    #[error("Insufficient locked balance: need {needed}, have {locked}")]
    InsufficientLocked { needed: Decimal, locked: Decimal },

    #[error("Account not found: {0}")]
    AccountNotFound(uuid::Uuid),

    #[error("Asset not found: {0}")]
    AssetNotFound(String),

    #[error("Negative amount: {0}")]
    NegativeAmount(Decimal),
}
