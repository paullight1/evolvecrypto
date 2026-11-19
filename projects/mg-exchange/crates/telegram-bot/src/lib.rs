//! Exchange Telegram Bot
//!
//! Provides a Telegram Bot API client for interacting with the exchange.
//! Supports slash commands for trading, balance enquiries, positions,
//! PnL summaries, and price alerts.
//!
//! # Integration
//!
//! ## Webhook (production)
//!
//! Register the bot with Telegram's webhook API pointing at:
//! `POST /api/v1/webhooks/telegram`
//!
//! The api-gateway parses the incoming JSON update, deserialises it into a
//! [`types::TelegramUpdate`], and calls [`bot::TelegramBot::process_update`].
//!
//! ## Long-polling (development)
//!
//! Call [`bot::TelegramBot::start_polling`] in a background task.
//!
//! # Feature flags
//!
//! No feature flags are currently defined. Disable HTTP calls in tests by not
//! calling `send_message` / `answer_callback` — the command routing logic is
//! covered by unit tests that do not require network access.

pub mod bot;
pub mod commands;
pub mod keyboard;
pub mod types;
