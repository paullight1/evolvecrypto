//! FIX 4.4 protocol support for the exchange.
//!
//! This crate provides:
//! - [`parser`]: wire-format parsing and serialisation of FIX messages.
//! - [`builder`]: ergonomic construction of FIX messages.
//! - [`translator`]: conversion between FIX messages and exchange-common types.
//! - [`session`]: FIX session state (sequence numbers, heartbeats, logon).
//! - [`error`]: unified error type.
//!
//! # Quick start
//!
//! ```
//! use exchange_fix_gateway::builder::build_logon;
//! use exchange_fix_gateway::parser::FixMessage;
//!
//! let msg = build_logon("CLIENT1", "EXCHANGE", 30);
//! let bytes = msg.to_bytes();
//! let parsed = FixMessage::parse(&bytes).unwrap();
//! assert_eq!(parsed.msg_type(), Some("A"));
//! ```

pub mod builder;
pub mod error;
pub mod parser;
pub mod session;
pub mod tcp_server;
pub mod translator;

pub use builder::{
    build_execution_report, build_heartbeat, build_logon, FixMessageBuilder,
};
pub use error::FixError;
pub use parser::{msg_type, tag, FixMessage};
pub use session::{FixSession, SessionAction};
pub use tcp_server::{ExchangeAdapter, FixTcpServer, FixTcpServerConfig, route_message};
pub use translator::{
    fill_to_execution_report, fix_to_cancel_order, fix_to_place_order,
    order_accepted_to_report, order_rejected_to_report,
};
