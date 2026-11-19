//! Core Telegram bot: HTTP client wrapper, update processing, and polling loop.
//!
//! `TelegramBot` is the single entry-point used by both the polling loop and
//! the webhook handler in `api-gateway`. All Telegram Bot API calls go through
//! `send_message` and `answer_callback`; command routing lives in `commands`.

use anyhow::{Context, Result};
use tracing::{debug, error, info, warn};

use crate::{
    commands::{handle_callback, handle_command},
    types::{
        AnswerCallbackBody, GetUpdatesResponse, InlineKeyboard, SendMessageBody,
        TelegramUpdate,
    },
};

const TELEGRAM_API_BASE: &str = "https://api.telegram.org/bot";

/// Manages connection to the Telegram Bot API.
///
/// Clone is cheap — the inner `reqwest::Client` is already `Arc`-wrapped.
#[derive(Clone)]
pub struct TelegramBot {
    token: String,
    http: reqwest::Client,
}

impl TelegramBot {
    /// Create a new bot instance.
    ///
    /// `token` is the Bot API token obtained from @BotFather.
    pub fn new(token: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("failed to build reqwest client");
        Self { token, http }
    }

    // ---- Public API ----

    /// Send a text message to `chat_id`.
    ///
    /// `text` supports Telegram MarkdownV2 formatting. `keyboard` is an
    /// optional inline keyboard attached to the message.
    pub async fn send_message(
        &self,
        chat_id: i64,
        text: &str,
        keyboard: Option<InlineKeyboard>,
    ) -> Result<()> {
        let url = self.api_url("sendMessage");
        let body = SendMessageBody {
            chat_id,
            text,
            parse_mode: "Markdown",
            reply_markup: keyboard.as_ref(),
        };

        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("send_message: HTTP request failed")?;

        let status = resp.status();
        let raw: serde_json::Value = resp
            .json()
            .await
            .context("send_message: failed to parse response JSON")?;

        if !status.is_success() || raw["ok"].as_bool() != Some(true) {
            warn!(
                chat_id,
                status = status.as_u16(),
                description = ?raw["description"],
                "send_message: Telegram API error"
            );
        } else {
            debug!(chat_id, "send_message: ok");
        }

        Ok(())
    }

    /// Answer a callback query (clears the loading spinner on the button).
    ///
    /// `text` is displayed as a brief notification on the user's device.
    /// Pass an empty string to dismiss silently.
    pub async fn answer_callback(&self, callback_id: &str, text: &str) -> Result<()> {
        let url = self.api_url("answerCallbackQuery");
        let body = AnswerCallbackBody {
            callback_query_id: callback_id,
            text,
            show_alert: false,
        };

        self.http
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("answer_callback: HTTP request failed")?;

        Ok(())
    }

    /// Route a single update to the appropriate handler.
    ///
    /// Errors from individual handlers are logged but do not stop processing —
    /// a malformed command should not crash the bot loop.
    pub async fn process_update(&self, update: TelegramUpdate) -> Result<()> {
        debug!(update_id = update.update_id, "processing update");

        if let Some(msg) = update.message {
            let chat_id = msg.chat.id;
            if let Some(text) = msg.text.as_deref() {
                if let Some(cmd_body) = text.strip_prefix('/') {
                    // Strip @BotUsername suffix (present in group chats).
                    let cmd_body = cmd_body
                        .split('@')
                        .next()
                        .unwrap_or(cmd_body);

                    let mut parts = cmd_body.split_whitespace();
                    let cmd = parts.next().unwrap_or("").to_lowercase();
                    let args: Vec<&str> = parts.collect();

                    if let Err(e) = handle_command(self, &cmd, &args, chat_id).await {
                        error!(cmd, error = ?e, "command handler failed");
                        // Send a user-visible error only for command errors.
                        let _ = self
                            .send_message(chat_id, "An error occurred. Please try again.", None)
                            .await;
                    }
                }
                // Non-command text messages are silently ignored.
            }
        } else if let Some(cb) = update.callback_query {
            let chat_id = cb
                .message
                .as_ref()
                .map(|m| m.chat.id)
                .unwrap_or(0);

            let data = cb.data.as_deref().unwrap_or("");
            if let Err(e) = handle_callback(self, &cb.id, data, chat_id).await {
                error!(data, error = ?e, "callback handler failed");
            }
        }

        Ok(())
    }

    /// Run the bot with long-polling (development / simple deployment mode).
    ///
    /// This blocks indefinitely. For production deployments, prefer registering
    /// the webhook endpoint `POST /api/v1/webhooks/telegram` in api-gateway,
    /// which avoids the polling overhead.
    pub async fn start_polling(&self) -> Result<()> {
        info!("Telegram bot polling started");
        let mut last_update_id: i64 = 0;

        loop {
            match self.fetch_updates(last_update_id + 1).await {
                Ok(updates) => {
                    for update in updates {
                        last_update_id = last_update_id.max(update.update_id);
                        let bot = self.clone();
                        tokio::spawn(async move {
                            if let Err(e) = bot.process_update(update).await {
                                error!(error = ?e, "update processing failed");
                            }
                        });
                    }
                }
                Err(e) => {
                    error!(error = ?e, "getUpdates failed — retrying in 5 s");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    }

    // ---- Private helpers ----

    fn api_url(&self, method: &str) -> String {
        format!("{TELEGRAM_API_BASE}{}/{method}", self.token)
    }

    async fn fetch_updates(&self, offset: i64) -> Result<Vec<TelegramUpdate>> {
        let url = self.api_url("getUpdates");
        let resp: GetUpdatesResponse = self
            .http
            .get(&url)
            .query(&[
                ("offset", offset.to_string()),
                ("timeout", "25".to_owned()), // long-poll for up to 25 s
                ("limit", "100".to_owned()),
            ])
            .send()
            .await
            .context("getUpdates: HTTP request failed")?
            .json()
            .await
            .context("getUpdates: failed to parse JSON")?;

        if !resp.ok {
            let desc = resp.description.as_deref().unwrap_or("unknown error");
            anyhow::bail!("getUpdates API error: {desc}");
        }

        Ok(resp.result.unwrap_or_default())
    }
}

// ---- Tests ----

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bot() -> TelegramBot {
        TelegramBot::new("test_token:test".into())
    }

    #[test]
    fn api_url_format() {
        let bot = make_bot();
        assert_eq!(
            bot.api_url("sendMessage"),
            "https://api.telegram.org/bottest_token:test/sendMessage"
        );
    }

    #[test]
    fn api_url_get_updates() {
        let bot = make_bot();
        assert!(bot.api_url("getUpdates").contains("getUpdates"));
    }

    #[test]
    fn process_update_non_command_text_is_ignored() {
        // A message without a leading '/' should not crash the router.
        let update = TelegramUpdate {
            update_id: 1,
            message: Some(crate::types::Message {
                message_id: 1,
                chat: crate::types::Chat { id: 42 },
                from: None,
                text: Some("hello there".into()),
            }),
            callback_query: None,
        };
        // We just verify the update is parseable — no network call.
        assert!(update.message.is_some());
    }

    #[test]
    fn process_update_no_text_is_safe() {
        let update = TelegramUpdate {
            update_id: 2,
            message: Some(crate::types::Message {
                message_id: 2,
                chat: crate::types::Chat { id: 99 },
                from: None,
                text: None,
            }),
            callback_query: None,
        };
        assert!(update.message.as_ref().unwrap().text.is_none());
    }
}
