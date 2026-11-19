//! FIX 4.4 message parsing and serialisation.
//!
//! Wire format: tag=value pairs delimited by SOH (0x01).
//! The first three fields are always 8 (BeginString), 9 (BodyLength), then the
//! message body, and the last field is 10 (CheckSum).

use rust_decimal::Decimal;
use std::str::FromStr;

use crate::error::FixError;

/// SOH delimiter used between FIX fields on the wire.
pub const SOH: u8 = 0x01;

// ---------------------------------------------------------------------------
// Well-known FIX 4.4 tag constants
// ---------------------------------------------------------------------------

pub mod tag {
    pub const BEGIN_STRING: u32 = 8;
    pub const BODY_LENGTH: u32 = 9;
    pub const MSG_TYPE: u32 = 35;
    pub const SENDER_COMP_ID: u32 = 49;
    pub const TARGET_COMP_ID: u32 = 56;
    pub const MSG_SEQ_NUM: u32 = 34;
    pub const SENDING_TIME: u32 = 52;
    pub const CL_ORD_ID: u32 = 11;
    pub const ORIG_CL_ORD_ID: u32 = 41;
    pub const ORDER_ID: u32 = 37;
    pub const EXEC_ID: u32 = 17;
    pub const SIDE: u32 = 54;
    pub const SYMBOL: u32 = 55;
    pub const ORDER_QTY: u32 = 38;
    pub const PRICE: u32 = 44;
    pub const ORD_TYPE: u32 = 40;
    pub const TIME_IN_FORCE: u32 = 59;
    pub const EXEC_TYPE: u32 = 150;
    pub const ORD_STATUS: u32 = 39;
    pub const LAST_PX: u32 = 31;
    pub const LAST_QTY: u32 = 32;
    pub const LEAVES_QTY: u32 = 151;
    pub const CUM_QTY: u32 = 14;
    pub const TEXT: u32 = 58;
    pub const HEARTBEAT_INT: u32 = 108;
    pub const ENCRYPT_METHOD: u32 = 98;
    pub const TEST_REQ_ID: u32 = 112;
    pub const CHECK_SUM: u32 = 10;
    pub const TRANSACT_TIME: u32 = 60;
    pub const EXEC_TRANS_TYPE: u32 = 20;
    pub const ACCOUNT: u32 = 1;
    pub const USERNAME: u32 = 553;
    pub const PASSWORD: u32 = 554;
}

/// Well-known MsgType (tag 35) values.
pub mod msg_type {
    pub const HEARTBEAT: &str = "0";
    pub const LOGON: &str = "A";
    pub const LOGOUT: &str = "5";
    pub const NEW_ORDER_SINGLE: &str = "D";
    pub const ORDER_CANCEL_REQUEST: &str = "F";
    pub const EXECUTION_REPORT: &str = "8";
    pub const ORDER_CANCEL_REJECT: &str = "9";
    pub const TEST_REQUEST: &str = "1";
    pub const RESEND_REQUEST: &str = "2";
}

/// A parsed FIX message as an ordered list of (tag, value) pairs.
///
/// Ordering is preserved because some fields (e.g. repeating groups) are
/// position-sensitive in the FIX spec.
#[derive(Debug, Clone, PartialEq)]
pub struct FixMessage {
    pub fields: Vec<(u32, String)>,
}

impl FixMessage {
    // -----------------------------------------------------------------------
    // Parsing
    // -----------------------------------------------------------------------

    /// Parse a raw FIX frame from bytes.
    ///
    /// The frame must start with `8=FIX.4.4` and end with `10=XXX`.
    /// Checksum is validated.
    pub fn parse(raw: &[u8]) -> Result<Self, FixError> {
        if raw.is_empty() {
            return Err(FixError::EmptyMessage);
        }

        let fields = Self::split_fields(raw)?;

        // Locate checksum field — must be last
        let check_pos = fields
            .iter()
            .rposition(|(t, _)| *t == tag::CHECK_SUM)
            .ok_or(FixError::MissingChecksum)?;

        if check_pos != fields.len() - 1 {
            return Err(FixError::MalformedMessage(
                "CheckSum (tag 10) must be the last field".into(),
            ));
        }

        // Validate checksum: covers everything up to (not including) "10=..."
        // Find the byte offset of "10=" in the raw slice.
        let checksum_field_start = Self::find_last_tag10_offset(raw)?;
        let body_for_checksum = &raw[..checksum_field_start];
        let computed = Self::calculate_checksum(body_for_checksum);
        let claimed = &fields[check_pos].1;
        if &computed != claimed {
            return Err(FixError::ChecksumMismatch {
                expected: computed,
                actual: claimed.clone(),
            });
        }

        // Require BeginString as first field
        if fields.first().map(|(t, _)| *t) != Some(tag::BEGIN_STRING) {
            return Err(FixError::MissingBeginString);
        }

        // Require BodyLength as second field
        if fields.get(1).map(|(t, _)| *t) != Some(tag::BODY_LENGTH) {
            return Err(FixError::MissingBodyLength);
        }

        Ok(Self { fields })
    }

    fn split_fields(raw: &[u8]) -> Result<Vec<(u32, String)>, FixError> {
        let mut fields = Vec::new();
        let mut start = 0usize;

        while start < raw.len() {
            // Find the next SOH
            let end = raw[start..]
                .iter()
                .position(|&b| b == SOH)
                .map(|p| start + p)
                .unwrap_or(raw.len());

            let chunk = &raw[start..end];
            if !chunk.is_empty() {
                let pair = std::str::from_utf8(chunk).map_err(|_| {
                    FixError::MalformedMessage("Non-UTF8 bytes in field".into())
                })?;

                let eq = pair.find('=').ok_or_else(|| {
                    FixError::MalformedMessage(format!("No '=' in field: {pair}"))
                })?;

                let tag_str = &pair[..eq];
                let value = &pair[eq + 1..];

                let tag: u32 = tag_str.parse().map_err(|_| {
                    FixError::MalformedMessage(format!("Non-numeric tag: {tag_str}"))
                })?;

                fields.push((tag, value.to_owned()));
            }

            start = end + 1; // skip the SOH
        }

        Ok(fields)
    }

    /// Find the byte offset of the final `10=` field in the raw frame.
    fn find_last_tag10_offset(raw: &[u8]) -> Result<usize, FixError> {
        // Walk backwards, looking for SOH followed by "10="
        // The checksum field always appears as "\x0110=NNN\x01" at the end.
        let needle = b"\x0110=";
        raw.windows(needle.len())
            .rposition(|w| w == needle)
            .map(|pos| pos + 1) // skip the leading SOH
            .ok_or(FixError::MissingChecksum)
    }

    // -----------------------------------------------------------------------
    // Field accessors
    // -----------------------------------------------------------------------

    /// Return the first value for `tag`, if present.
    pub fn get_field(&self, tag: u32) -> Option<&str> {
        self.fields
            .iter()
            .find(|(t, _)| *t == tag)
            .map(|(_, v)| v.as_str())
    }

    /// Parse the first value for `tag` as an `i64`.
    pub fn get_field_int(&self, tag: u32) -> Option<i64> {
        self.get_field(tag)?.parse().ok()
    }

    /// Parse the first value for `tag` as a `Decimal`.
    pub fn get_field_decimal(&self, tag: u32) -> Option<Decimal> {
        Decimal::from_str(self.get_field(tag)?).ok()
    }

    /// Convenience: return the MsgType value (tag 35).
    pub fn msg_type(&self) -> Option<&str> {
        self.get_field(tag::MSG_TYPE)
    }

    // -----------------------------------------------------------------------
    // Serialisation
    // -----------------------------------------------------------------------

    /// Encode the message back to wire bytes (fields joined by SOH, trailing SOH included).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for (t, v) in &self.fields {
            out.extend_from_slice(t.to_string().as_bytes());
            out.push(b'=');
            out.extend_from_slice(v.as_bytes());
            out.push(SOH);
        }
        out
    }

    // -----------------------------------------------------------------------
    // Checksum
    // -----------------------------------------------------------------------

    /// Compute the FIX checksum: sum all bytes mod 256, zero-padded to 3 digits.
    pub fn calculate_checksum(data: &[u8]) -> String {
        let sum: u32 = data.iter().map(|&b| b as u32).sum();
        format!("{:03}", sum % 256)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal but valid FIX 4.4 frame for tests.
    fn make_raw(fields: &[(u32, &str)]) -> Vec<u8> {
        // Build body (everything after tag 9)
        let mut body = Vec::<u8>::new();
        for (t, v) in fields {
            body.extend_from_slice(t.to_string().as_bytes());
            body.push(b'=');
            body.extend_from_slice(v.as_bytes());
            body.push(SOH);
        }

        let body_len = body.len();

        // Assemble: 8=FIX.4.4 SOH 9=<len> SOH <body> 10=<cs> SOH
        let mut frame = Vec::<u8>::new();
        frame.extend_from_slice(b"8=FIX.4.4");
        frame.push(SOH);

        let bl_field = format!("9={body_len}");
        frame.extend_from_slice(bl_field.as_bytes());
        frame.push(SOH);

        frame.extend_from_slice(&body);

        // Checksum covers everything so far
        let cs = FixMessage::calculate_checksum(&frame);
        frame.extend_from_slice(format!("10={cs}").as_bytes());
        frame.push(SOH);

        frame
    }

    #[test]
    fn parse_valid_logon() {
        let raw = make_raw(&[
            (35, "A"),
            (49, "CLIENT1"),
            (56, "EXCHANGE"),
            (34, "1"),
            (52, "20240101-12:00:00"),
            (98, "0"),
            (108, "30"),
        ]);
        let msg = FixMessage::parse(&raw).expect("should parse");
        assert_eq!(msg.msg_type(), Some("A"));
        assert_eq!(msg.get_field(49), Some("CLIENT1"));
        assert_eq!(msg.get_field(56), Some("EXCHANGE"));
        assert_eq!(msg.get_field_int(108), Some(30));
    }

    #[test]
    fn parse_new_order_single() {
        let raw = make_raw(&[
            (35, "D"),
            (49, "CLIENT1"),
            (56, "EXCHANGE"),
            (34, "2"),
            (52, "20240101-12:00:00"),
            (11, "order-001"),
            (54, "1"),
            (55, "BTCUSDT"),
            (38, "0.5"),
            (44, "30000.00"),
            (40, "2"),
            (59, "1"),
        ]);
        let msg = FixMessage::parse(&raw).expect("should parse");
        assert_eq!(msg.msg_type(), Some("D"));
        assert_eq!(msg.get_field(55), Some("BTCUSDT"));
        assert_eq!(msg.get_field(54), Some("1"));
        let qty = msg.get_field_decimal(38).expect("qty");
        assert_eq!(qty.to_string(), "0.5");
        let price = msg.get_field_decimal(44).expect("price");
        assert_eq!(price.to_string(), "30000.00");
    }

    #[test]
    fn parse_empty_returns_error() {
        assert!(matches!(FixMessage::parse(&[]), Err(FixError::EmptyMessage)));
    }

    #[test]
    fn parse_missing_checksum_returns_error() {
        // Manually craft a frame with no tag-10
        let mut frame = Vec::<u8>::new();
        frame.extend_from_slice(b"8=FIX.4.4\x019=5\x0135=0\x01");
        assert!(matches!(
            FixMessage::parse(&frame),
            Err(FixError::MissingChecksum)
        ));
    }

    #[test]
    fn parse_wrong_checksum_returns_error() {
        let mut raw = make_raw(&[(35, "0"), (49, "A"), (56, "B"), (34, "1")]);
        // Corrupt the checksum digit
        let len = raw.len();
        // Last bytes are "10=NNN\x01" — flip a digit
        raw[len - 3] ^= 1;
        assert!(matches!(
            FixMessage::parse(&raw),
            Err(FixError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn parse_missing_begin_string_returns_error() {
        // Start with body-length, not begin-string
        let mut frame = Vec::<u8>::new();
        frame.extend_from_slice(b"9=5\x0135=0\x01");
        let cs = FixMessage::calculate_checksum(&frame);
        frame.extend_from_slice(format!("10={cs}").as_bytes());
        frame.push(SOH);
        assert!(matches!(
            FixMessage::parse(&frame),
            Err(FixError::MissingBeginString)
        ));
    }

    #[test]
    fn to_bytes_roundtrip() {
        let raw = make_raw(&[
            (35, "0"),
            (49, "S"),
            (56, "T"),
            (34, "5"),
            (52, "20240101-00:00:00"),
        ]);
        let msg = FixMessage::parse(&raw).expect("parse");
        let re_encoded = msg.to_bytes();
        let msg2 = FixMessage::parse(&re_encoded).expect("re-parse");
        assert_eq!(msg, msg2);
    }

    #[test]
    fn checksum_known_value() {
        // "8=FIX.4.4\x01" — each byte summed mod 256
        let data = b"8=FIX.4.4\x01";
        let cs = FixMessage::calculate_checksum(data);
        // '8'=56,'='=61,'F'=70,'I'=73,'X'=88,'.'=46,'4'=52,'.'=46,'4'=52,'\x01'=1 → 545 % 256 = 33
        assert_eq!(cs, "033");
    }

    #[test]
    fn get_field_missing_returns_none() {
        let raw = make_raw(&[(35, "0"), (49, "S"), (56, "T"), (34, "1")]);
        let msg = FixMessage::parse(&raw).expect("parse");
        assert_eq!(msg.get_field(9999), None);
        assert_eq!(msg.get_field_int(9999), None);
        assert_eq!(msg.get_field_decimal(9999), None);
    }

    #[test]
    fn soh_within_value_is_not_allowed() {
        // A value containing a raw SOH would split the field incorrectly —
        // verify the parser handles it without panicking (it will see extra fields).
        let mut frame = Vec::<u8>::new();
        frame.extend_from_slice(b"8=FIX.4.4\x019=3\x0135=0\x01");
        // Intentionally corrupt: insert SOH inside what would be the checksum
        frame.extend_from_slice(b"10=\x0199\x01");
        // The parser should return an error, not panic
        let result = FixMessage::parse(&frame);
        assert!(result.is_err());
    }
}
