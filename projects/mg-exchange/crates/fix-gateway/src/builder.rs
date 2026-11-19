//! FIX 4.4 message builder.
//!
//! Uses the builder pattern for ergonomic construction. `build()` automatically
//! prepends BeginString + BodyLength and appends the CheckSum.

use chrono::Utc;
use rust_decimal::Decimal;

use crate::parser::{tag, FixMessage, SOH};

/// Builder for FIX 4.4 messages.
///
/// Fields are added in declaration order. `build()` wraps them with the
/// standard envelope (tags 8, 9) and trailer (tag 10).
pub struct FixMessageBuilder {
    fields: Vec<(u32, String)>,
}

impl FixMessageBuilder {
    /// Create a builder for a message of the given `msg_type` (tag 35).
    ///
    /// `sender` and `target` are the SenderCompID / TargetCompID values.
    pub fn new(msg_type: &str, sender: &str, target: &str) -> Self {
        let mut builder = Self {
            fields: Vec::new(),
        };
        builder.fields.push((tag::MSG_TYPE, msg_type.to_owned()));
        builder
            .fields
            .push((tag::SENDER_COMP_ID, sender.to_owned()));
        builder
            .fields
            .push((tag::TARGET_COMP_ID, target.to_owned()));
        // Add a sending timestamp for every message
        builder.fields.push((
            tag::SENDING_TIME,
            Utc::now().format("%Y%m%d-%H:%M:%S%.3f").to_string(),
        ));
        builder
    }

    /// Append a field to the message body.
    pub fn field(mut self, tag: u32, value: impl ToString) -> Self {
        self.fields.push((tag, value.to_string()));
        self
    }

    /// Finalise and return the [`FixMessage`].
    ///
    /// This method:
    /// 1. Serialises the body fields (everything after tag 9) to bytes.
    /// 2. Calculates `BodyLength` (byte length of the body).
    /// 3. Prepends `8=FIX.4.4` and `9=<body_len>`.
    /// 4. Appends `10=<checksum>`.
    pub fn build(self) -> FixMessage {
        // Serialise body fields
        let mut body_bytes = Vec::<u8>::new();
        for (t, v) in &self.fields {
            body_bytes.extend_from_slice(t.to_string().as_bytes());
            body_bytes.push(b'=');
            body_bytes.extend_from_slice(v.as_bytes());
            body_bytes.push(SOH);
        }

        // Assemble full frame for checksum calculation
        let mut frame = Vec::<u8>::new();
        // tag 8
        frame.extend_from_slice(
            format!("{}=FIX.4.4", tag::BEGIN_STRING).as_bytes(),
        );
        frame.push(SOH);
        // tag 9
        frame.extend_from_slice(
            format!("{}={}", tag::BODY_LENGTH, body_bytes.len()).as_bytes(),
        );
        frame.push(SOH);
        // body
        frame.extend_from_slice(&body_bytes);

        let checksum = FixMessage::calculate_checksum(&frame);

        // Build final ordered field list
        let mut all_fields: Vec<(u32, String)> = Vec::new();
        all_fields.push((tag::BEGIN_STRING, "FIX.4.4".to_owned()));
        all_fields.push((tag::BODY_LENGTH, body_bytes.len().to_string()));
        all_fields.extend(self.fields);
        all_fields.push((tag::CHECK_SUM, checksum));

        FixMessage { fields: all_fields }
    }
}

// ---------------------------------------------------------------------------
// Convenience constructors
// ---------------------------------------------------------------------------

/// Build a Logon message (MsgType=A).
pub fn build_logon(sender: &str, target: &str, heartbeat_interval: u32) -> FixMessage {
    FixMessageBuilder::new(crate::parser::msg_type::LOGON, sender, target)
        .field(tag::ENCRYPT_METHOD, "0") // No encryption
        .field(tag::HEARTBEAT_INT, heartbeat_interval)
        .build()
}

/// Build a Logout message (MsgType=5) with an optional reason text.
pub fn build_logout(sender: &str, target: &str, reason: &str) -> FixMessage {
    let mut b = FixMessageBuilder::new(crate::parser::msg_type::LOGOUT, sender, target);
    if !reason.is_empty() {
        b = b.field(tag::TEXT, reason);
    }
    b.build()
}

/// Build a Heartbeat message (MsgType=0).
pub fn build_heartbeat(sender: &str, target: &str) -> FixMessage {
    FixMessageBuilder::new(crate::parser::msg_type::HEARTBEAT, sender, target).build()
}

/// Build an ExecutionReport (MsgType=8).
///
/// * `exec_type` — tag 150 value (e.g. "0"=New, "4"=Canceled, "8"=Rejected, "F"=Trade)
/// * `status`    — tag 39 value (e.g. "0"=New, "4"=Canceled, "8"=Rejected, "2"=Filled)
#[allow(clippy::too_many_arguments)]
pub fn build_execution_report(
    sender: &str,
    target: &str,
    order_id: &str,
    exec_id: &str,
    exec_type: &str,
    side: &str,
    symbol: &str,
    qty: Decimal,
    price: Decimal,
    status: &str,
) -> FixMessage {
    FixMessageBuilder::new(
        crate::parser::msg_type::EXECUTION_REPORT,
        sender,
        target,
    )
    .field(tag::ORDER_ID, order_id)
    .field(tag::EXEC_ID, exec_id)
    .field(tag::EXEC_TYPE, exec_type)
    .field(tag::ORD_STATUS, status)
    .field(tag::SIDE, side)
    .field(tag::SYMBOL, symbol)
    .field(tag::ORDER_QTY, qty)
    .field(tag::PRICE, price)
    .field(tag::LEAVES_QTY, Decimal::ZERO)
    .field(tag::CUM_QTY, qty)
    .field(
        tag::TRANSACT_TIME,
        Utc::now().format("%Y%m%d-%H:%M:%S%.3f").to_string(),
    )
    .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{msg_type, tag, FixMessage};

    #[test]
    fn build_logon_is_parseable() {
        let msg = build_logon("CLIENT1", "EXCHANGE", 30);
        // Re-encode then re-parse to verify the envelope is valid
        let bytes = msg.to_bytes();
        let parsed = FixMessage::parse(&bytes).expect("should parse built logon");
        assert_eq!(parsed.msg_type(), Some(msg_type::LOGON));
        assert_eq!(parsed.get_field(tag::SENDER_COMP_ID), Some("CLIENT1"));
        assert_eq!(parsed.get_field(tag::TARGET_COMP_ID), Some("EXCHANGE"));
        assert_eq!(parsed.get_field_int(tag::HEARTBEAT_INT), Some(30));
        assert_eq!(parsed.get_field(tag::ENCRYPT_METHOD), Some("0"));
    }

    #[test]
    fn build_heartbeat_is_parseable() {
        let msg = build_heartbeat("EXCHANGE", "CLIENT1");
        let bytes = msg.to_bytes();
        let parsed = FixMessage::parse(&bytes).expect("should parse heartbeat");
        assert_eq!(parsed.msg_type(), Some(msg_type::HEARTBEAT));
    }

    #[test]
    fn build_execution_report_is_parseable() {
        use rust_decimal_macros::dec;
        let msg = build_execution_report(
            "EXCHANGE",
            "CLIENT1",
            "ord-999",
            "exec-1",
            "0", // New
            "1", // Buy
            "BTCUSDT",
            dec!(0.5),
            dec!(30000),
            "0", // New
        );
        let bytes = msg.to_bytes();
        let parsed = FixMessage::parse(&bytes).expect("should parse exec report");
        assert_eq!(parsed.msg_type(), Some(msg_type::EXECUTION_REPORT));
        assert_eq!(parsed.get_field(tag::SYMBOL), Some("BTCUSDT"));
        assert_eq!(parsed.get_field(tag::SIDE), Some("1"));
        assert_eq!(parsed.get_field(tag::EXEC_TYPE), Some("0"));
        assert_eq!(parsed.get_field(tag::ORD_STATUS), Some("0"));
        let price = parsed.get_field_decimal(tag::PRICE).expect("price");
        assert_eq!(price, dec!(30000));
    }

    #[test]
    fn builder_custom_fields() {
        let msg = FixMessageBuilder::new("D", "CLI", "EXG")
            .field(tag::CL_ORD_ID, "order-42")
            .field(tag::SYMBOL, "ETHUSDT")
            .field(tag::SIDE, "2")
            .field(tag::ORDER_QTY, "1.0")
            .field(tag::ORD_TYPE, "2")
            .field(tag::PRICE, "2000.00")
            .field(tag::TIME_IN_FORCE, "1")
            .build();
        let bytes = msg.to_bytes();
        let parsed = FixMessage::parse(&bytes).expect("should parse");
        assert_eq!(parsed.msg_type(), Some("D"));
        assert_eq!(parsed.get_field(tag::SYMBOL), Some("ETHUSDT"));
    }

    #[test]
    fn auto_checksum_is_correct() {
        let msg = build_heartbeat("A", "B");
        // The last field must be tag 10 with a valid 3-digit checksum
        let last = msg.fields.last().expect("has fields");
        assert_eq!(last.0, tag::CHECK_SUM);
        assert_eq!(last.1.len(), 3);
        // Re-parse verifies checksum internally
        FixMessage::parse(&msg.to_bytes()).expect("checksum valid");
    }

    #[test]
    fn body_length_matches_actual_body() {
        let msg = build_logon("S", "T", 60);
        let bl: usize = msg
            .get_field(tag::BODY_LENGTH)
            .and_then(|v| v.parse().ok())
            .expect("body length present");

        // Body = all bytes from after tag-9 field up to (not including) tag-10 field
        let bytes = msg.to_bytes();
        // Find end of "9=<len>\x01"
        let tag9_prefix = format!("9={bl}");
        let after_tag9 = bytes
            .windows(tag9_prefix.len() + 1)
            .position(|w| &w[..tag9_prefix.len()] == tag9_prefix.as_bytes() && w[tag9_prefix.len()] == SOH)
            .map(|p| p + tag9_prefix.len() + 1)
            .expect("tag 9 present");

        // Find start of "10=" field
        let tag10_prefix = b"\x0110=";
        let tag10_start = bytes
            .windows(tag10_prefix.len())
            .rposition(|w| w == tag10_prefix)
            .map(|p| p + 1)
            .expect("tag 10 present");

        let actual_body_len = tag10_start - after_tag9;
        assert_eq!(bl, actual_body_len, "BodyLength must equal actual body byte count");
    }
}
