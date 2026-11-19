//! Translation layer between FIX 4.4 messages and exchange-common types.

use chrono::Utc;
use rust_decimal::Decimal;
use uuid::Uuid;

use exchange_common::{
    CancelOrderCommand, FillEvent, OrderId, OrderType, PlaceOrderCommand, Side, StpMode, Symbol,
    TimeInForce, UserId,
};

use crate::builder::{build_execution_report, FixMessageBuilder};
use crate::error::FixError;
use crate::parser::{msg_type, tag, FixMessage};

// ---------------------------------------------------------------------------
// FIX → Exchange types
// ---------------------------------------------------------------------------

/// Convert a NewOrderSingle (MsgType=D) into a [`PlaceOrderCommand`].
pub fn fix_to_place_order(
    msg: &FixMessage,
    user_id: UserId,
) -> Result<PlaceOrderCommand, FixError> {
    require_msg_type(msg, msg_type::NEW_ORDER_SINGLE)?;

    let cl_ord_id = require_str(msg, tag::CL_ORD_ID)?;
    let order_id = parse_uuid_or_new(cl_ord_id);

    let symbol = Symbol::new(require_str(msg, tag::SYMBOL)?);

    let side = parse_side(require_str(msg, tag::SIDE)?)?;

    let qty = require_decimal(msg, tag::ORDER_QTY)?;

    let ord_type_str = require_str(msg, tag::ORD_TYPE)?;
    let order_type = parse_order_type(ord_type_str)?;

    // Price is required for limit orders; for market orders we default to zero.
    let price = match order_type {
        OrderType::Limit => require_decimal(msg, tag::PRICE)?,
        OrderType::Market => Decimal::ZERO,
    };

    let time_in_force = msg
        .get_field(tag::TIME_IN_FORCE)
        .map(parse_time_in_force)
        .transpose()?
        .unwrap_or_default();

    Ok(PlaceOrderCommand {
        order_id,
        user_id,
        symbol,
        side,
        order_type,
        time_in_force,
        price,
        qty,
        timestamp: Utc::now(),
        reduce_only: false,
        expire_at: None,
        position_idx: 0,
        iceberg_qty: None,
        order_link_id: None,
        slippage_tolerance: None,
        close_position: false,
        stp_mode: StpMode::default(),
        request_expire_time: None,
    })
}

/// Convert an OrderCancelRequest (MsgType=F) into a [`CancelOrderCommand`].
pub fn fix_to_cancel_order(
    msg: &FixMessage,
    user_id: UserId,
) -> Result<CancelOrderCommand, FixError> {
    require_msg_type(msg, msg_type::ORDER_CANCEL_REQUEST)?;

    // Prefer OrigClOrdID (tag 41) which refers to the original order being cancelled,
    // fall back to ClOrdID (tag 11).
    let id_str = msg
        .get_field(tag::ORIG_CL_ORD_ID)
        .or_else(|| msg.get_field(tag::CL_ORD_ID))
        .ok_or(FixError::MissingField(tag::CL_ORD_ID))?;

    let order_id = parse_uuid_or_new(id_str);
    let symbol = Symbol::new(require_str(msg, tag::SYMBOL)?);

    Ok(CancelOrderCommand {
        order_id,
        user_id,
        symbol,
        timestamp: Utc::now(),
    })
}

// ---------------------------------------------------------------------------
// Exchange types → FIX
// ---------------------------------------------------------------------------

/// Build an ExecutionReport for a fill event.
///
/// The exec_type is `F` (Trade) and ord_status `2` (Filled).
pub fn fill_to_execution_report(
    fill: &FillEvent,
    sender: &str,
    target: &str,
) -> FixMessage {
    let side_str = side_to_fix(fill.taker_side);
    let exec_id = fill.trade_id.to_string();
    let order_id = fill.taker_order_id.to_string();

    build_execution_report(
        sender,
        target,
        &order_id,
        &exec_id,
        "F",   // ExecType: Trade
        side_str,
        fill.symbol.as_str(),
        fill.qty,
        fill.price,
        "2",   // OrdStatus: Filled
    )
}

/// Build an ExecutionReport acknowledging a new order (ExecType=0, OrdStatus=0).
pub fn order_accepted_to_report(
    order_id: OrderId,
    symbol: &str,
    sender: &str,
    target: &str,
) -> FixMessage {
    FixMessageBuilder::new(msg_type::EXECUTION_REPORT, sender, target)
        .field(tag::ORDER_ID, order_id)
        .field(tag::EXEC_ID, Uuid::new_v4())
        .field(tag::EXEC_TYPE, "0")  // New
        .field(tag::ORD_STATUS, "0") // New
        .field(tag::SYMBOL, symbol)
        .field(tag::LEAVES_QTY, Decimal::ZERO)
        .field(tag::CUM_QTY, Decimal::ZERO)
        .field(
            tag::TRANSACT_TIME,
            Utc::now().format("%Y%m%d-%H:%M:%S%.3f").to_string(),
        )
        .build()
}

/// Build an ExecutionReport for a rejected order (ExecType=8, OrdStatus=8).
pub fn order_rejected_to_report(
    order_id: OrderId,
    reason: &str,
    sender: &str,
    target: &str,
) -> FixMessage {
    FixMessageBuilder::new(msg_type::EXECUTION_REPORT, sender, target)
        .field(tag::ORDER_ID, order_id)
        .field(tag::EXEC_ID, Uuid::new_v4())
        .field(tag::EXEC_TYPE, "8")  // Rejected
        .field(tag::ORD_STATUS, "8") // Rejected
        .field(tag::TEXT, reason)
        .field(tag::LEAVES_QTY, Decimal::ZERO)
        .field(tag::CUM_QTY, Decimal::ZERO)
        .field(
            tag::TRANSACT_TIME,
            Utc::now().format("%Y%m%d-%H:%M:%S%.3f").to_string(),
        )
        .build()
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn require_msg_type<'a>(msg: &'a FixMessage, expected: &str) -> Result<&'a str, FixError> {
    let actual = msg
        .msg_type()
        .ok_or(FixError::MissingField(tag::MSG_TYPE))?;
    if actual != expected {
        return Err(FixError::UnexpectedMsgType(actual.to_owned()));
    }
    Ok(actual)
}

fn require_str<'a>(msg: &'a FixMessage, t: u32) -> Result<&'a str, FixError> {
    msg.get_field(t).ok_or(FixError::MissingField(t))
}

fn require_decimal(msg: &FixMessage, t: u32) -> Result<Decimal, FixError> {
    let raw = require_str(msg, t)?;
    raw.parse::<Decimal>().map_err(|_| FixError::InvalidFieldValue {
        tag: t,
        reason: format!("cannot parse '{raw}' as decimal"),
    })
}

fn parse_uuid_or_new(s: &str) -> Uuid {
    s.parse::<Uuid>().unwrap_or_else(|_| Uuid::new_v4())
}

fn parse_side(s: &str) -> Result<Side, FixError> {
    match s {
        "1" => Ok(Side::Buy),
        "2" => Ok(Side::Sell),
        _ => Err(FixError::InvalidFieldValue {
            tag: tag::SIDE,
            reason: format!("unknown side '{s}'"),
        }),
    }
}

fn side_to_fix(side: Side) -> &'static str {
    match side {
        Side::Buy => "1",
        Side::Sell => "2",
    }
}

fn parse_order_type(s: &str) -> Result<OrderType, FixError> {
    match s {
        "1" => Ok(OrderType::Market),
        "2" => Ok(OrderType::Limit),
        _ => Err(FixError::InvalidFieldValue {
            tag: tag::ORD_TYPE,
            reason: format!("unknown order type '{s}'"),
        }),
    }
}

fn parse_time_in_force(s: &str) -> Result<TimeInForce, FixError> {
    match s {
        "0" => Ok(TimeInForce::Gtc), // Day — we map to GTC (exchange is 24/7)
        "1" => Ok(TimeInForce::Gtc),
        "3" => Ok(TimeInForce::Ioc),
        "4" => Ok(TimeInForce::Fok),
        _ => Err(FixError::InvalidFieldValue {
            tag: tag::TIME_IN_FORCE,
            reason: format!("unknown TIF '{s}'"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::build_logon;
    use crate::parser::FixMessage;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn make_new_order(extra: &[(u32, &str)]) -> FixMessage {
        let mut b = FixMessageBuilder::new("D", "CLI", "EXG")
            .field(tag::CL_ORD_ID, "order-001")
            .field(tag::SYMBOL, "BTCUSDT")
            .field(tag::SIDE, "1")
            .field(tag::ORDER_QTY, "0.5")
            .field(tag::ORD_TYPE, "2")
            .field(tag::PRICE, "30000.00");

        for (t, v) in extra {
            b = b.field(*t, *v);
        }
        let msg = b.build();
        // Re-parse through the full parser to ensure validity
        FixMessage::parse(&msg.to_bytes()).expect("valid new order")
    }

    fn make_cancel(orig_cl_ord_id: &str) -> FixMessage {
        let msg = FixMessageBuilder::new("F", "CLI", "EXG")
            .field(tag::CL_ORD_ID, "cancel-001")
            .field(tag::ORIG_CL_ORD_ID, orig_cl_ord_id)
            .field(tag::SYMBOL, "BTCUSDT")
            .field(tag::SIDE, "1")
            .build();
        FixMessage::parse(&msg.to_bytes()).expect("valid cancel")
    }

    fn dummy_user() -> UserId {
        Uuid::new_v4()
    }

    // -----------------------------------------------------------------------
    // fix_to_place_order
    // -----------------------------------------------------------------------

    #[test]
    fn place_order_buy_limit() {
        let msg = make_new_order(&[(tag::TIME_IN_FORCE, "1")]);
        let user = dummy_user();
        let cmd = fix_to_place_order(&msg, user).expect("should translate");
        assert_eq!(cmd.user_id, user);
        assert_eq!(cmd.symbol.as_str(), "BTCUSDT");
        assert_eq!(cmd.side, Side::Buy);
        assert_eq!(cmd.order_type, OrderType::Limit);
        assert_eq!(cmd.time_in_force, TimeInForce::Gtc);
        assert_eq!(cmd.qty, dec!(0.5));
        assert_eq!(cmd.price, dec!(30000.00));
    }

    #[test]
    fn place_order_sell_market() {
        let msg = FixMessageBuilder::new("D", "CLI", "EXG")
            .field(tag::CL_ORD_ID, "mkt-001")
            .field(tag::SYMBOL, "ETHUSDT")
            .field(tag::SIDE, "2")
            .field(tag::ORDER_QTY, "1.0")
            .field(tag::ORD_TYPE, "1") // Market
            .build();
        let parsed = FixMessage::parse(&msg.to_bytes()).unwrap();
        let cmd = fix_to_place_order(&parsed, dummy_user()).expect("market order");
        assert_eq!(cmd.side, Side::Sell);
        assert_eq!(cmd.order_type, OrderType::Market);
        assert_eq!(cmd.price, Decimal::ZERO);
    }

    #[test]
    fn place_order_ioc() {
        let msg = make_new_order(&[(tag::TIME_IN_FORCE, "3")]);
        let cmd = fix_to_place_order(&msg, dummy_user()).expect("ioc order");
        assert_eq!(cmd.time_in_force, TimeInForce::Ioc);
    }

    #[test]
    fn place_order_fok() {
        let msg = make_new_order(&[(tag::TIME_IN_FORCE, "4")]);
        let cmd = fix_to_place_order(&msg, dummy_user()).expect("fok order");
        assert_eq!(cmd.time_in_force, TimeInForce::Fok);
    }

    #[test]
    fn place_order_missing_symbol_returns_error() {
        let msg = FixMessageBuilder::new("D", "CLI", "EXG")
            .field(tag::CL_ORD_ID, "x")
            .field(tag::SIDE, "1")
            .field(tag::ORDER_QTY, "1")
            .field(tag::ORD_TYPE, "2")
            .field(tag::PRICE, "100")
            .build();
        let parsed = FixMessage::parse(&msg.to_bytes()).unwrap();
        assert!(matches!(
            fix_to_place_order(&parsed, dummy_user()),
            Err(FixError::MissingField(55))
        ));
    }

    #[test]
    fn place_order_wrong_msg_type_returns_error() {
        let msg = build_logon("A", "B", 30);
        assert!(matches!(
            fix_to_place_order(&msg, dummy_user()),
            Err(FixError::UnexpectedMsgType(_))
        ));
    }

    #[test]
    fn place_order_invalid_side_returns_error() {
        let msg = FixMessageBuilder::new("D", "CLI", "EXG")
            .field(tag::CL_ORD_ID, "x")
            .field(tag::SYMBOL, "BTCUSDT")
            .field(tag::SIDE, "9") // invalid
            .field(tag::ORDER_QTY, "1")
            .field(tag::ORD_TYPE, "2")
            .field(tag::PRICE, "100")
            .build();
        let parsed = FixMessage::parse(&msg.to_bytes()).unwrap();
        assert!(matches!(
            fix_to_place_order(&parsed, dummy_user()),
            Err(FixError::InvalidFieldValue { tag: 54, .. })
        ));
    }

    // -----------------------------------------------------------------------
    // fix_to_cancel_order
    // -----------------------------------------------------------------------

    #[test]
    fn cancel_order_uses_orig_cl_ord_id() {
        let target_id = Uuid::new_v4().to_string();
        let msg = make_cancel(&target_id);
        let cmd = fix_to_cancel_order(&msg, dummy_user()).expect("cancel");
        assert_eq!(cmd.symbol.as_str(), "BTCUSDT");
        // order_id is parsed from the UUID string
        assert_eq!(cmd.order_id.to_string(), target_id);
    }

    #[test]
    fn cancel_order_wrong_msg_type_returns_error() {
        let msg = build_logon("A", "B", 30);
        assert!(matches!(
            fix_to_cancel_order(&msg, dummy_user()),
            Err(FixError::UnexpectedMsgType(_))
        ));
    }

    // -----------------------------------------------------------------------
    // fill_to_execution_report
    // -----------------------------------------------------------------------

    #[test]
    fn fill_produces_valid_exec_report() {
        use chrono::Utc;
        let fill = FillEvent {
            trade_id: 42,
            symbol: Symbol::new("BTCUSDT"),
            price: dec!(30000),
            qty: dec!(0.1),
            maker_order_id: Uuid::new_v4(),
            taker_order_id: Uuid::new_v4(),
            maker_user_id: Uuid::new_v4(),
            taker_user_id: Uuid::new_v4(),
            taker_side: Side::Buy,
            timestamp: Utc::now(),
            seq: 1,
        };
        let msg = fill_to_execution_report(&fill, "EXCHANGE", "CLIENT1");
        let bytes = msg.to_bytes();
        let parsed = FixMessage::parse(&bytes).expect("parseable");
        assert_eq!(parsed.msg_type(), Some("8"));
        assert_eq!(parsed.get_field(tag::EXEC_TYPE), Some("F"));
        assert_eq!(parsed.get_field(tag::ORD_STATUS), Some("2"));
        assert_eq!(parsed.get_field(tag::SIDE), Some("1")); // Buy
        assert_eq!(parsed.get_field(tag::SYMBOL), Some("BTCUSDT"));
        let price = parsed.get_field_decimal(tag::PRICE).unwrap();
        assert_eq!(price, dec!(30000));
    }

    // -----------------------------------------------------------------------
    // order_accepted_to_report / order_rejected_to_report
    // -----------------------------------------------------------------------

    #[test]
    fn order_accepted_report_is_valid() {
        let id = Uuid::new_v4();
        let msg = order_accepted_to_report(id, "BTCUSDT", "EXG", "CLI");
        let parsed = FixMessage::parse(&msg.to_bytes()).expect("parseable");
        assert_eq!(parsed.msg_type(), Some("8"));
        assert_eq!(parsed.get_field(tag::EXEC_TYPE), Some("0"));
        assert_eq!(parsed.get_field(tag::ORD_STATUS), Some("0"));
        assert_eq!(parsed.get_field(tag::ORDER_ID), Some(id.to_string().as_str()));
    }

    #[test]
    fn order_rejected_report_is_valid() {
        let id = Uuid::new_v4();
        let msg = order_rejected_to_report(id, "Insufficient balance", "EXG", "CLI");
        let parsed = FixMessage::parse(&msg.to_bytes()).expect("parseable");
        assert_eq!(parsed.msg_type(), Some("8"));
        assert_eq!(parsed.get_field(tag::EXEC_TYPE), Some("8"));
        assert_eq!(parsed.get_field(tag::ORD_STATUS), Some("8"));
        assert_eq!(parsed.get_field(tag::TEXT), Some("Insufficient balance"));
    }
}
