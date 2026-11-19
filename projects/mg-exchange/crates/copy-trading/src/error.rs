//! Error types for the copy-trading subsystem.

use thiserror::Error;
use uuid::Uuid;

use exchange_common::types::UserId;

#[derive(Debug, Error, PartialEq)]
pub enum CopyTradingError {
    // ── Master errors ────────────────────────────────────────────────────────

    /// Attempt to register a user who is already a master.
    #[error("User {0} is already registered as a master trader")]
    AlreadyMaster(UserId),

    /// Profit-share rate must be in [0, 1].
    #[error("Invalid profit-share rate {0}: must be between 0 and 1")]
    InvalidProfitShareRate(String),

    /// The requested master does not exist.
    #[error("Master trader not found: {0}")]
    MasterNotFound(UserId),

    /// Master has reached their follower capacity.
    #[error("Master {0} has reached the maximum number of followers")]
    MasterAtCapacity(UserId),

    /// The master is no longer accepting new followers.
    #[error("Master {0} is not active")]
    MasterInactive(UserId),

    // ── Follower / relation errors ───────────────────────────────────────────

    /// A follow relation with this ID does not exist.
    #[error("Follow relation not found: {0}")]
    RelationNotFound(Uuid),

    /// The follower already follows this master.
    #[error("User {follower} already follows master {master}")]
    AlreadyFollowing { follower: UserId, master: UserId },

    /// Following this master would exceed the per-follower limit.
    #[error("Follower {0} has reached the maximum number of masters")]
    TooManyMasters(UserId),

    /// Allocation must be positive.
    #[error("Allocation must be positive, got {0}")]
    InvalidAllocation(String),

    // ── Settlement errors ────────────────────────────────────────────────────

    /// Share rate supplied to settlement is out of [0, 1].
    #[error("Share rate must be between 0 and 1, got {0}")]
    InvalidShareRate(String),
}
