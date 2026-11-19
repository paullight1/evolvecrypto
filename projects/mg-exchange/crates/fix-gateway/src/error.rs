//! Error type for the FIX gateway.

use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum FixError {
    #[error("Empty message")]
    EmptyMessage,

    #[error("Message is missing BeginString (tag 8)")]
    MissingBeginString,

    #[error("Message is missing BodyLength (tag 9)")]
    MissingBodyLength,

    #[error("Message is missing CheckSum (tag 10)")]
    MissingChecksum,

    #[error("Checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("Malformed message: {0}")]
    MalformedMessage(String),

    #[error("Missing required field tag {0}")]
    MissingField(u32),

    #[error("Invalid field value for tag {tag}: {reason}")]
    InvalidFieldValue { tag: u32, reason: String },

    #[error("Unexpected message type: {0}")]
    UnexpectedMsgType(String),

    #[error("Sequence gap: expected {expected}, got {actual}")]
    SequenceGap { expected: u64, actual: u64 },

    #[error("Session not logged on")]
    NotLoggedOn,
}
