//! Pre-built inline keyboards for the trading bot.
//!
//! Each function returns an [`InlineKeyboard`] whose `callback_data` strings
//! are handled in `commands::handle_callback`.

use crate::types::{InlineButton, InlineKeyboard};

/// Main menu — shown on /start and after most operations.
pub fn main_menu() -> InlineKeyboard {
    InlineKeyboard {
        inline_keyboard: vec![
            vec![
                InlineButton::callback("Balance", "cmd:balance"),
                InlineButton::callback("Positions", "cmd:positions"),
            ],
            vec![
                InlineButton::callback("Price", "cmd:price:BTCUSDT"),
                InlineButton::callback("Alerts", "cmd:alerts"),
            ],
            vec![
                InlineButton::callback("Orders", "cmd:orders"),
                InlineButton::callback("PnL", "cmd:pnl"),
            ],
            vec![
                InlineButton::callback("Help", "cmd:help"),
            ],
        ],
    }
}

/// Quick-trade keyboard for BTC and ETH market orders.
pub fn quick_trade() -> InlineKeyboard {
    InlineKeyboard {
        inline_keyboard: vec![
            vec![
                InlineButton::callback("Buy BTC", "trade:buy:BTCUSDT"),
                InlineButton::callback("Sell BTC", "trade:sell:BTCUSDT"),
            ],
            vec![
                InlineButton::callback("Buy ETH", "trade:buy:ETHUSDT"),
                InlineButton::callback("Sell ETH", "trade:sell:ETHUSDT"),
            ],
            vec![
                InlineButton::callback("Back", "cmd:menu"),
            ],
        ],
    }
}

/// Actions available for a specific open position.
///
/// `symbol` is embedded in callback data so the handler knows which position
/// to act on when the user taps a button.
pub fn position_actions(symbol: &str) -> InlineKeyboard {
    InlineKeyboard {
        inline_keyboard: vec![
            vec![
                InlineButton::callback(
                    "Close Position",
                    format!("pos:close:{symbol}"),
                ),
                InlineButton::callback(
                    "Set TP/SL",
                    format!("pos:tpsl:{symbol}"),
                ),
            ],
            vec![
                InlineButton::callback(
                    "Add Margin",
                    format!("pos:margin:{symbol}"),
                ),
                InlineButton::callback("Back", "cmd:positions"),
            ],
        ],
    }
}

/// Confirmation keyboard used for destructive actions (close position, cancel
/// order) to prevent accidental taps.
pub fn confirm_cancel(action_data: &str) -> InlineKeyboard {
    InlineKeyboard {
        inline_keyboard: vec![
            vec![
                InlineButton::callback("Confirm", action_data.to_owned()),
                InlineButton::callback("Cancel", "cmd:menu"),
            ],
        ],
    }
}
