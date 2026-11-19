use rust_decimal::Decimal;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum TriggerError {
    #[error("Trigger order not found: {0}")]
    OrderNotFound(Uuid),

    #[error("Trigger order {0} is not in Active status")]
    OrderNotActive(Uuid),

    #[error("Duplicate trigger order id: {0}")]
    DuplicateOrderId(Uuid),

    #[error("TrailingStop order {0} is missing trailing_delta")]
    MissingTrailingDelta(Uuid),

    #[error("TrailingStop order {0} has invalid trailing_delta {1}: must be > 0")]
    InvalidTrailingDelta(Uuid, Decimal),
}
