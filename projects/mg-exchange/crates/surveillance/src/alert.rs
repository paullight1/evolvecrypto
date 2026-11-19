//! Unified alert types for the surveillance system.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use exchange_common::types::{Symbol, UserId};

/// A detected spoofing / layering pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpoofingAlert {
    pub user_id: UserId,
    /// Fraction of orders cancelled within the analysis window.
    pub cancel_ratio: Decimal,
    pub orders_placed: usize,
    pub orders_cancelled: usize,
    /// Start of the window that triggered this alert.
    pub window_start: DateTime<Utc>,
    /// End of the window that triggered this alert.
    pub window_end: DateTime<Utc>,
}

/// A detected wash-trading pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WashTradingAlert {
    /// All user IDs involved (single user for self-trading, multiple for circular).
    pub user_ids: Vec<UserId>,
    pub symbol: Symbol,
    /// Cumulative buy volume in the window.
    pub buy_volume: Decimal,
    /// Cumulative sell volume in the window.
    pub sell_volume: Decimal,
    /// `min(buy, sell) / max(buy, sell)` — 1.0 = perfectly balanced.
    pub balance_ratio: Decimal,
}

/// A detected quote-stuffing pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteStuffingAlert {
    pub user_id: UserId,
    /// Measured rate in the last second.
    pub orders_per_second: u32,
    /// Configured threshold that was breached.
    pub threshold: u32,
    pub detected_at: DateTime<Utc>,
}

/// Discriminated union of all concrete alert payloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum AlertType {
    Spoofing(SpoofingAlert),
    WashTrading(WashTradingAlert),
    QuoteStuffing(QuoteStuffingAlert),
}

/// How serious the alert is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AlertSeverity {
    Low,
    Medium,
    High,
    Critical,
}

impl std::fmt::Display for AlertSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AlertSeverity::Low => write!(f, "Low"),
            AlertSeverity::Medium => write!(f, "Medium"),
            AlertSeverity::High => write!(f, "High"),
            AlertSeverity::Critical => write!(f, "Critical"),
        }
    }
}

/// Automated response to take when an alert fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutoAction {
    /// Record only, no automatic intervention.
    None,
    /// Cancel all open orders for the flagged user(s).
    CancelOrders,
    /// Freeze the account — no new orders accepted.
    FreezeAccount,
    /// Drop the user's order-rate limit to throttle throughput.
    ReduceRateLimit,
}

/// A single surveillance alert with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurveillanceAlert {
    /// Globally unique identifier for this alert.
    pub id: Uuid,
    pub alert_type: AlertType,
    pub severity: AlertSeverity,
    /// All user IDs implicated by this alert.
    pub user_ids: Vec<UserId>,
    /// Trading pair involved, if applicable.
    pub symbol: Option<Symbol>,
    /// Human-readable summary suitable for a compliance dashboard.
    pub description: String,
    /// Action the system took (or should take) automatically.
    pub auto_action: AutoAction,
    pub created_at: DateTime<Utc>,
    /// `true` once a compliance officer has reviewed and closed the alert.
    pub reviewed: bool,
}

impl SurveillanceAlert {
    /// Mark this alert as reviewed by a compliance officer.
    pub fn review(&mut self) {
        self.reviewed = true;
    }
}
