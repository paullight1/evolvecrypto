//! Telegram Bot API types (subset required for trading bot functionality).
//!
//! These mirror the Telegram API JSON schema for updates, messages, and
//! callback queries. Only the fields the bot actually reads are included —
//! unknown fields are silently ignored by serde's `deny_unknown_fields`-free
//! default.

use serde::{Deserialize, Serialize};

// ---- Inbound types (received from Telegram) ----

/// Top-level update object delivered by polling or webhook.
#[derive(Debug, Clone, Deserialize)]
pub struct TelegramUpdate {
    pub update_id: i64,
    pub message: Option<Message>,
    pub callback_query: Option<CallbackQuery>,
}

/// A message sent in a chat.
#[derive(Debug, Clone, Deserialize)]
pub struct Message {
    pub message_id: i64,
    pub chat: Chat,
    pub from: Option<User>,
    pub text: Option<String>,
}

/// The chat a message belongs to.
#[derive(Debug, Clone, Deserialize)]
pub struct Chat {
    pub id: i64,
}

/// The Telegram user who sent a message or triggered a callback.
#[derive(Debug, Clone, Deserialize)]
pub struct User {
    pub id: i64,
    pub username: Option<String>,
    pub first_name: Option<String>,
}

/// Fired when a user taps an inline keyboard button.
#[derive(Debug, Clone, Deserialize)]
pub struct CallbackQuery {
    pub id: String,
    pub from: User,
    pub data: Option<String>,
    /// The message the keyboard was attached to.
    pub message: Option<Message>,
}

// ---- Outbound types (sent to Telegram) ----

/// Inline keyboard markup sent with a message.
#[derive(Debug, Clone, Serialize)]
pub struct InlineKeyboard {
    pub inline_keyboard: Vec<Vec<InlineButton>>,
}

/// A single button in an inline keyboard row.
#[derive(Debug, Clone, Serialize)]
pub struct InlineButton {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callback_data: Option<String>,
    /// External URL — mutually exclusive with `callback_data`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl InlineButton {
    pub fn callback(text: impl Into<String>, data: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            callback_data: Some(data.into()),
            url: None,
        }
    }

    pub fn url(text: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            callback_data: None,
            url: Some(url.into()),
        }
    }
}

/// `sendMessage` / `editMessageText` body fields.
#[derive(Debug, Serialize)]
pub struct SendMessageBody<'a> {
    pub chat_id: i64,
    pub text: &'a str,
    pub parse_mode: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_markup: Option<&'a InlineKeyboard>,
}

/// `answerCallbackQuery` body fields.
#[derive(Debug, Serialize)]
pub struct AnswerCallbackBody<'a> {
    pub callback_query_id: &'a str,
    pub text: &'a str,
    /// If `true`, shows the text as an alert dialog instead of a notification.
    pub show_alert: bool,
}

/// Wrapper around the Telegram API boolean result.
#[derive(Debug, Deserialize)]
pub struct TelegramApiResponse<T> {
    pub ok: bool,
    pub result: Option<T>,
    pub description: Option<String>,
}

/// Result of `getUpdates`.
pub type GetUpdatesResponse = TelegramApiResponse<Vec<TelegramUpdate>>;
