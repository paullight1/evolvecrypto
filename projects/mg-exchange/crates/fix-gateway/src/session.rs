//! FIX 4.4 session management.
//!
//! Tracks sequence numbers, logon state, and heartbeat cadence for a single
//! FIX session (one pair of SenderCompID / TargetCompID).

use chrono::{DateTime, Duration, Utc};

use crate::builder::{build_heartbeat, build_logon};
use crate::error::FixError;
use crate::parser::{msg_type, tag, FixMessage};

// ---------------------------------------------------------------------------
// Session action
// ---------------------------------------------------------------------------

/// What the session layer should do in response to an inbound message.
#[derive(Debug, PartialEq)]
pub enum SessionAction {
    /// Respond with a Logon message.
    SendLogon,
    /// Respond with a Heartbeat message.
    SendHeartbeat,
    /// Respond with a Reject (admin-level session problem).
    SendReject { reason: String },
    /// The message is a valid application message; pass it up the stack.
    Process(FixMessage),
    /// Request resend of missed messages.
    SendResendRequest { begin_seq: u64, end_seq: u64 },
    /// Respond with a Logout message.
    SendLogout { reason: String },
    /// Fatal session error; close the connection.
    Disconnect,
}

// ---------------------------------------------------------------------------
// FixSession
// ---------------------------------------------------------------------------

/// State for a single FIX 4.4 session.
#[derive(Debug, Clone)]
pub struct FixSession {
    pub sender_comp_id: String,
    pub target_comp_id: String,
    /// Next sequence number to use on outgoing messages.
    pub outgoing_seq: u64,
    /// The sequence number we expect on the *next* inbound message.
    pub incoming_seq: u64,
    /// Heartbeat interval in seconds agreed during logon.
    pub heartbeat_interval: u32,
    /// Timestamp of the last inbound message (for heartbeat timeout detection).
    pub last_received: DateTime<Utc>,
    pub is_logged_on: bool,
}

impl FixSession {
    /// Create a new, not-yet-logged-on session.
    pub fn new(sender: &str, target: &str, heartbeat_interval: u32) -> Self {
        Self {
            sender_comp_id: sender.to_owned(),
            target_comp_id: target.to_owned(),
            outgoing_seq: 1,
            incoming_seq: 1,
            heartbeat_interval,
            last_received: Utc::now(),
            is_logged_on: false,
        }
    }

    // -----------------------------------------------------------------------
    // Sequence numbers
    // -----------------------------------------------------------------------

    /// Consume and return the next outgoing sequence number, incrementing the counter.
    pub fn next_outgoing_seq(&mut self) -> u64 {
        let seq = self.outgoing_seq;
        self.outgoing_seq += 1;
        seq
    }

    /// Verify that `incoming_seq` matches the expected next sequence.
    ///
    /// On success, the expected counter is incremented.
    pub fn validate_sequence(&mut self, incoming_seq: u64) -> Result<(), FixError> {
        if incoming_seq != self.incoming_seq {
            return Err(FixError::SequenceGap {
                expected: self.incoming_seq,
                actual: incoming_seq,
            });
        }
        self.incoming_seq += 1;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Heartbeat
    // -----------------------------------------------------------------------

    /// Returns `true` if we have not received anything for longer than the
    /// heartbeat interval (indicating the peer may be dead).
    pub fn is_heartbeat_overdue(&self, now: DateTime<Utc>) -> bool {
        let elapsed = now - self.last_received;
        elapsed > Duration::seconds(self.heartbeat_interval as i64)
    }

    // -----------------------------------------------------------------------
    // Message dispatch
    // -----------------------------------------------------------------------

    /// Process an inbound message and return the action to take.
    ///
    /// This handles session-level messages (Logon, Heartbeat, Logout) and
    /// passes application messages through as [`SessionAction::Process`].
    pub fn on_message_received(&mut self, msg: FixMessage) -> SessionAction {
        self.last_received = Utc::now();

        // Validate sequence number if present
        if let Some(seq) = msg.get_field_int(tag::MSG_SEQ_NUM) {
            if seq < 1 {
                return SessionAction::SendReject {
                    reason: "Invalid sequence number".into(),
                };
            }
            if let Err(FixError::SequenceGap { expected, actual }) =
                self.validate_sequence(seq as u64)
            {
                tracing::warn!(
                    expected,
                    actual,
                    "FIX sequence gap — requesting resend"
                );
                return SessionAction::SendReject {
                    reason: format!("Sequence gap: expected {expected}, got {actual}"),
                };
            }
        }

        match msg.msg_type() {
            Some(msg_type::LOGON) => {
                tracing::info!(
                    sender = %self.target_comp_id,
                    "FIX Logon received"
                );
                self.is_logged_on = true;
                // Update heartbeat interval if peer specifies one
                if let Some(hb) = msg.get_field_int(tag::HEARTBEAT_INT) {
                    if hb > 0 {
                        self.heartbeat_interval = hb as u32;
                    }
                }
                SessionAction::SendLogon
            }
            Some(msg_type::LOGOUT) => {
                tracing::info!("FIX Logout received");
                self.is_logged_on = false;
                SessionAction::Disconnect
            }
            Some(msg_type::HEARTBEAT) => {
                tracing::trace!("FIX Heartbeat received");
                SessionAction::SendHeartbeat
            }
            Some(msg_type::TEST_REQUEST) => {
                // TestRequest must be echoed back as a Heartbeat
                tracing::debug!("FIX TestRequest received");
                SessionAction::SendHeartbeat
            }
            None => SessionAction::SendReject {
                reason: "Missing MsgType (tag 35)".into(),
            },
            _ => {
                if !self.is_logged_on {
                    return SessionAction::SendReject {
                        reason: "Not logged on".into(),
                    };
                }
                SessionAction::Process(msg)
            }
        }
    }

    // -----------------------------------------------------------------------
    // Convenience: build session messages with the correct comp IDs
    // -----------------------------------------------------------------------

    /// Build a Logon response message.
    pub fn make_logon(&self) -> FixMessage {
        build_logon(&self.sender_comp_id, &self.target_comp_id, self.heartbeat_interval)
    }

    /// Build a Heartbeat message.
    pub fn make_heartbeat(&self) -> FixMessage {
        build_heartbeat(&self.sender_comp_id, &self.target_comp_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::FixMessageBuilder;
    use crate::parser::FixMessage;

    fn session() -> FixSession {
        FixSession::new("EXCHANGE", "CLIENT1", 30)
    }

    fn logon_msg(sender: &str, target: &str, seq: u64) -> FixMessage {
        let msg = FixMessageBuilder::new("A", sender, target)
            .field(tag::MSG_SEQ_NUM, seq)
            .field(tag::ENCRYPT_METHOD, "0")
            .field(tag::HEARTBEAT_INT, "30")
            .build();
        FixMessage::parse(&msg.to_bytes()).unwrap()
    }

    fn heartbeat_msg(sender: &str, target: &str, seq: u64) -> FixMessage {
        let msg = FixMessageBuilder::new("0", sender, target)
            .field(tag::MSG_SEQ_NUM, seq)
            .build();
        FixMessage::parse(&msg.to_bytes()).unwrap()
    }

    fn new_order_msg(seq: u64) -> FixMessage {
        let msg = FixMessageBuilder::new("D", "CLIENT1", "EXCHANGE")
            .field(tag::MSG_SEQ_NUM, seq)
            .field(tag::CL_ORD_ID, "ord-1")
            .field(tag::SYMBOL, "BTCUSDT")
            .field(tag::SIDE, "1")
            .field(tag::ORDER_QTY, "1")
            .field(tag::ORD_TYPE, "2")
            .field(tag::PRICE, "100")
            .build();
        FixMessage::parse(&msg.to_bytes()).unwrap()
    }

    // -----------------------------------------------------------------------
    // Logon flow
    // -----------------------------------------------------------------------

    #[test]
    fn logon_sets_logged_on_state() {
        let mut s = session();
        assert!(!s.is_logged_on);
        let action = s.on_message_received(logon_msg("CLIENT1", "EXCHANGE", 1));
        assert_eq!(action, SessionAction::SendLogon);
        assert!(s.is_logged_on);
    }

    #[test]
    fn logout_disconnects() {
        let mut s = session();
        s.is_logged_on = true;
        s.incoming_seq = 1;
        let msg = FixMessageBuilder::new("5", "CLIENT1", "EXCHANGE")
            .field(tag::MSG_SEQ_NUM, 1u64)
            .build();
        let parsed = FixMessage::parse(&msg.to_bytes()).unwrap();
        let action = s.on_message_received(parsed);
        assert_eq!(action, SessionAction::Disconnect);
        assert!(!s.is_logged_on);
    }

    #[test]
    fn app_message_rejected_when_not_logged_on() {
        let mut s = session();
        // seq=1 passes validation but session is not logged on
        let action = s.on_message_received(new_order_msg(1));
        assert!(matches!(action, SessionAction::SendReject { .. }));
    }

    #[test]
    fn app_message_processed_when_logged_on() {
        let mut s = session();
        // First message: logon with seq=1
        s.on_message_received(logon_msg("CLIENT1", "EXCHANGE", 1));
        // Second message: new order with seq=2
        let action = s.on_message_received(new_order_msg(2));
        assert!(matches!(action, SessionAction::Process(_)));
    }

    // -----------------------------------------------------------------------
    // Sequence numbers
    // -----------------------------------------------------------------------

    #[test]
    fn next_outgoing_seq_increments() {
        let mut s = session();
        assert_eq!(s.next_outgoing_seq(), 1);
        assert_eq!(s.next_outgoing_seq(), 2);
        assert_eq!(s.next_outgoing_seq(), 3);
        assert_eq!(s.outgoing_seq, 4);
    }

    #[test]
    fn validate_sequence_accepts_in_order() {
        let mut s = session();
        assert!(s.validate_sequence(1).is_ok());
        assert!(s.validate_sequence(2).is_ok());
        assert_eq!(s.incoming_seq, 3);
    }

    #[test]
    fn validate_sequence_detects_gap() {
        let mut s = session();
        let err = s.validate_sequence(5).unwrap_err();
        assert!(matches!(
            err,
            FixError::SequenceGap { expected: 1, actual: 5 }
        ));
        // Counter must NOT advance on gap
        assert_eq!(s.incoming_seq, 1);
    }

    #[test]
    fn validate_sequence_detects_replay() {
        let mut s = session();
        s.incoming_seq = 10;
        let err = s.validate_sequence(9).unwrap_err();
        assert!(matches!(
            err,
            FixError::SequenceGap { expected: 10, actual: 9 }
        ));
    }

    #[test]
    fn sequence_gap_in_message_produces_reject() {
        let mut s = session();
        s.is_logged_on = true;
        s.incoming_seq = 5;
        // Send seq=10 — gap of 5
        let action = s.on_message_received(new_order_msg(10));
        assert!(matches!(action, SessionAction::SendReject { .. }));
    }

    // -----------------------------------------------------------------------
    // Heartbeat timeout
    // -----------------------------------------------------------------------

    #[test]
    fn heartbeat_not_overdue_when_recent() {
        let s = session();
        let now = Utc::now();
        assert!(!s.is_heartbeat_overdue(now));
    }

    #[test]
    fn heartbeat_overdue_after_interval() {
        let mut s = session();
        // Simulate last_received being 60 seconds ago (interval is 30)
        s.last_received = Utc::now() - Duration::seconds(60);
        assert!(s.is_heartbeat_overdue(Utc::now()));
    }

    #[test]
    fn heartbeat_not_overdue_just_before_interval() {
        let mut s = session();
        s.last_received = Utc::now() - Duration::seconds(29);
        assert!(!s.is_heartbeat_overdue(Utc::now()));
    }

    #[test]
    fn receiving_message_resets_last_received() {
        let mut s = session();
        s.last_received = Utc::now() - Duration::seconds(60);
        // Receiving any message updates the timestamp
        s.on_message_received(logon_msg("CLIENT1", "EXCHANGE", 1));
        assert!(!s.is_heartbeat_overdue(Utc::now()));
    }

    // -----------------------------------------------------------------------
    // Heartbeat message handling
    // -----------------------------------------------------------------------

    #[test]
    fn heartbeat_message_returns_send_heartbeat() {
        let mut s = session();
        s.is_logged_on = true;
        s.incoming_seq = 1;
        let action = s.on_message_received(heartbeat_msg("CLIENT1", "EXCHANGE", 1));
        assert_eq!(action, SessionAction::SendHeartbeat);
    }

    // -----------------------------------------------------------------------
    // make_logon / make_heartbeat helpers
    // -----------------------------------------------------------------------

    #[test]
    fn make_logon_produces_valid_message() {
        let s = session();
        let msg = s.make_logon();
        let bytes = msg.to_bytes();
        let parsed = FixMessage::parse(&bytes).expect("valid logon");
        assert_eq!(parsed.msg_type(), Some("A"));
        assert_eq!(parsed.get_field(tag::SENDER_COMP_ID), Some("EXCHANGE"));
        assert_eq!(parsed.get_field(tag::TARGET_COMP_ID), Some("CLIENT1"));
    }

    #[test]
    fn make_heartbeat_produces_valid_message() {
        let s = session();
        let msg = s.make_heartbeat();
        let bytes = msg.to_bytes();
        let parsed = FixMessage::parse(&bytes).expect("valid heartbeat");
        assert_eq!(parsed.msg_type(), Some("0"));
    }
}
