use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use exchange_common::{Symbol, UserId};

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// What kind of event triggered this notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotificationType {
    OrderFilled,
    /// Sent at 70%, 80%, and 90% margin usage thresholds.
    PositionLiquidationWarning,
    /// Position has been fully liquidated.
    PositionLiquidated,
    MarginCall,
    LoginNewDevice,
    WithdrawalConfirmed,
    /// Withdrawal has been initiated and is pending processing.
    WithdrawalRequested,
    /// Deposit is confirming on-chain (not yet credited).
    DepositConfirming,
    /// Deposit has been credited to the user's balance.
    DepositCompleted,
    FundingPayment,
    PriceAlert,
    /// Account has been frozen by compliance or admin action.
    AccountFrozen,
    /// Suspicious activity detected on the account (e.g. new IP, 2FA change).
    SecurityAlert,
}

/// How the notification is (or will be) delivered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotificationChannel {
    /// Real-time push to the user's open WebSocket connection.
    WebSocket,
    /// Email — reserved for critical events (liquidation, margin call, login).
    Email,
    /// Apple Push Notification Service (iOS).
    Apns,
    /// Firebase Cloud Messaging (Android).
    Fcm,
    /// Browser Web Push (RFC 8030).
    WebPush,
}

/// A single notification record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: Uuid,
    pub user_id: UserId,
    pub notification_type: NotificationType,
    pub channel: NotificationChannel,
    pub title: String,
    pub body: String,
    /// Structured payload for the client to act on (e.g., order id, symbol).
    pub data: serde_json::Value,
    pub read: bool,
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Price alert
// ---------------------------------------------------------------------------

/// Direction a price alert is watching.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertDirection {
    /// Alert when price rises above target.
    Above,
    /// Alert when price falls below target.
    Below,
}

/// A user-configured price alert for a symbol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceAlert {
    pub symbol: Symbol,
    pub target_price: Decimal,
    pub direction: AlertDirection,
    /// Set to true once triggered so it fires only once.
    pub triggered: bool,
}

// ---------------------------------------------------------------------------
// User preferences
// ---------------------------------------------------------------------------

/// Per-user notification opt-in preferences.
///
/// `liquidation_warning` cannot be disabled — it is always true regardless
/// of what the user sends.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationPreferences {
    pub order_filled: bool,
    /// Cannot be disabled. Enforced in `set_preferences`.
    pub liquidation_warning: bool,
    pub login_alert: bool,
    pub price_alerts: Vec<PriceAlert>,
}

impl Default for NotificationPreferences {
    fn default() -> Self {
        Self {
            order_filled: true,
            liquidation_warning: true,
            login_alert: true,
            price_alerts: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

/// Manages user notifications and price alerts.
///
/// Phase 1: in-memory. Phase 2: persist to PostgreSQL and fan out to
/// an email/push delivery service.
pub struct NotificationManager {
    /// All notifications ever created, newest last.
    notifications: Vec<Notification>,
    /// Per-user notification preferences.
    preferences: HashMap<UserId, NotificationPreferences>,
}

impl NotificationManager {
    pub fn new() -> Self {
        Self {
            notifications: Vec::new(),
            preferences: HashMap::new(),
        }
    }

    // --- Core API ---

    /// Create and store a notification.
    ///
    /// Returns the newly created notification's id for reference.
    pub fn notify(
        &mut self,
        user_id: UserId,
        notification_type: NotificationType,
        channel: NotificationChannel,
        title: impl Into<String>,
        body: impl Into<String>,
        data: serde_json::Value,
    ) -> Uuid {
        let id = Uuid::new_v4();
        let notif = Notification {
            id,
            user_id,
            notification_type,
            channel,
            title: title.into(),
            body: body.into(),
            data,
            read: false,
            created_at: Utc::now(),
        };

        tracing::debug!(
            notification_id = %id,
            user_id = %user_id,
            title = %notif.title,
            "notification created"
        );

        self.notifications.push(notif);
        id
    }

    /// Send a liquidation warning if `margin_usage_pct` crosses a monitored threshold.
    ///
    /// Thresholds: 70%, 80%, 90%. Only fires if the user has not opted out (they cannot).
    /// Returns `Some(notification_id)` if a notification was created, `None` otherwise.
    pub fn liquidation_warning(
        &mut self,
        user_id: UserId,
        symbol: &Symbol,
        margin_usage_pct: f64,
    ) -> Option<Uuid> {
        // liquidation_warning cannot be disabled, but we still check preferences
        // to get the canonical "always enabled" value.
        let _prefs = self.preferences(user_id);

        let level = if margin_usage_pct >= 90.0 {
            "CRITICAL"
        } else if margin_usage_pct >= 80.0 {
            "HIGH"
        } else if margin_usage_pct >= 70.0 {
            "WARNING"
        } else {
            return None;
        };

        let title = format!("[{}] Liquidation Risk — {}", level, symbol.as_str());
        let body = format!(
            "Your margin usage on {} is {:.1}%. Reduce your position or add margin to avoid liquidation.",
            symbol.as_str(),
            margin_usage_pct
        );

        let id = self.notify(
            user_id,
            NotificationType::PositionLiquidationWarning,
            NotificationChannel::WebSocket,
            title,
            body,
            serde_json::json!({
                "symbol": symbol.as_str(),
                "margin_usage_pct": margin_usage_pct,
                "level": level,
            }),
        );

        Some(id)
    }

    /// Return all unread notifications for a user, oldest first.
    pub fn get_unread(&self, user_id: UserId) -> Vec<&Notification> {
        self.notifications
            .iter()
            .filter(|n| n.user_id == user_id && !n.read)
            .collect()
    }

    /// Mark a notification as read. No-op if id is unknown.
    pub fn mark_read(&mut self, notification_id: Uuid) {
        if let Some(n) = self.notifications.iter_mut().find(|n| n.id == notification_id) {
            n.read = true;
        }
    }

    /// Check all untriggered price alerts against `price` for `symbol`.
    ///
    /// Marks matching alerts as triggered and creates notifications for the
    /// owning users. Returns the ids of all fired notifications.
    pub fn check_price_alerts(&mut self, symbol: &Symbol, price: Decimal) -> Vec<Uuid> {
        // Collect which (user_id, alert_index) pairs fire — borrow checker requires
        // we separate the read pass from the write pass.
        let mut to_fire: Vec<(UserId, usize)> = Vec::new();

        for (user_id, prefs) in &self.preferences {
            for (idx, alert) in prefs.price_alerts.iter().enumerate() {
                if alert.triggered || &alert.symbol != symbol {
                    continue;
                }
                let fires = match alert.direction {
                    AlertDirection::Above => price >= alert.target_price,
                    AlertDirection::Below => price <= alert.target_price,
                };
                if fires {
                    to_fire.push((*user_id, idx));
                }
            }
        }

        let mut notification_ids = Vec::with_capacity(to_fire.len());

        for (user_id, alert_idx) in to_fire {
            // Mark the alert as triggered
            let prefs = self.preferences.get_mut(&user_id).expect("user must exist");
            let alert = &mut prefs.price_alerts[alert_idx];
            alert.triggered = true;

            let direction_word = match alert.direction {
                AlertDirection::Above => "above",
                AlertDirection::Below => "below",
            };
            let target = alert.target_price;
            let sym = alert.symbol.clone();

            let id = self.notify(
                user_id,
                NotificationType::PriceAlert,
                NotificationChannel::WebSocket,
                format!("Price Alert — {}", sym.as_str()),
                format!(
                    "{} has reached {}, which is {} your target of {}.",
                    sym.as_str(),
                    price,
                    direction_word,
                    target
                ),
                serde_json::json!({
                    "symbol": sym.as_str(),
                    "price": price.to_string(),
                    "target": target.to_string(),
                    "direction": direction_word,
                }),
            );
            notification_ids.push(id);
        }

        notification_ids
    }

    /// Update (or create) notification preferences for a user.
    ///
    /// Enforces that `liquidation_warning` is always true regardless of input.
    pub fn set_preferences(&mut self, user_id: UserId, mut prefs: NotificationPreferences) {
        // This preference cannot be disabled — always enforce it.
        prefs.liquidation_warning = true;
        self.preferences.insert(user_id, prefs);
    }

    /// Get a reference to a user's preferences, creating defaults if absent.
    fn preferences(&mut self, user_id: UserId) -> &NotificationPreferences {
        self.preferences
            .entry(user_id)
            .or_insert_with(NotificationPreferences::default)
    }
}

impl Default for NotificationManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_user() -> UserId {
        Uuid::new_v4()
    }

    fn btcusdt() -> Symbol {
        Symbol::new("BTCUSDT")
    }

    // --- Basic notification creation ---

    #[test]
    fn test_notify_creates_notification() {
        let mut mgr = NotificationManager::new();
        let user = make_user();

        let id = mgr.notify(
            user,
            NotificationType::OrderFilled,
            NotificationChannel::WebSocket,
            "Order Filled",
            "Your limit buy was filled at 50000.",
            serde_json::json!({"order_id": "abc123"}),
        );

        let unread = mgr.get_unread(user);
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].id, id);
        assert!(!unread[0].read);
        assert_eq!(unread[0].notification_type, NotificationType::OrderFilled);
    }

    #[test]
    fn test_new_notification_is_unread() {
        let mut mgr = NotificationManager::new();
        let user = make_user();

        mgr.notify(
            user,
            NotificationType::FundingPayment,
            NotificationChannel::WebSocket,
            "Funding",
            "You received a funding payment.",
            serde_json::Value::Null,
        );

        assert_eq!(mgr.get_unread(user).len(), 1);
    }

    #[test]
    fn test_mark_read_removes_from_unread() {
        let mut mgr = NotificationManager::new();
        let user = make_user();

        let id = mgr.notify(
            user,
            NotificationType::OrderFilled,
            NotificationChannel::WebSocket,
            "Order Filled",
            "Done.",
            serde_json::Value::Null,
        );

        assert_eq!(mgr.get_unread(user).len(), 1);
        mgr.mark_read(id);
        assert_eq!(mgr.get_unread(user).len(), 0);
    }

    #[test]
    fn test_mark_read_unknown_id_is_noop() {
        let mut mgr = NotificationManager::new();
        // Should not panic
        mgr.mark_read(Uuid::new_v4());
    }

    #[test]
    fn test_get_unread_only_returns_own_notifications() {
        let mut mgr = NotificationManager::new();
        let alice = make_user();
        let bob = make_user();

        mgr.notify(alice, NotificationType::OrderFilled, NotificationChannel::WebSocket, "A", "a", serde_json::Value::Null);
        mgr.notify(bob, NotificationType::FundingPayment, NotificationChannel::WebSocket, "B", "b", serde_json::Value::Null);

        assert_eq!(mgr.get_unread(alice).len(), 1);
        assert_eq!(mgr.get_unread(bob).len(), 1);
    }

    #[test]
    fn test_multiple_notifications_ordered_oldest_first() {
        let mut mgr = NotificationManager::new();
        let user = make_user();

        let id1 = mgr.notify(user, NotificationType::OrderFilled, NotificationChannel::WebSocket, "First", "", serde_json::Value::Null);
        let id2 = mgr.notify(user, NotificationType::FundingPayment, NotificationChannel::WebSocket, "Second", "", serde_json::Value::Null);

        let unread = mgr.get_unread(user);
        assert_eq!(unread.len(), 2);
        assert_eq!(unread[0].id, id1);
        assert_eq!(unread[1].id, id2);
    }

    // --- Liquidation warning thresholds ---

    #[test]
    fn test_liquidation_warning_below_70_is_silent() {
        let mut mgr = NotificationManager::new();
        let user = make_user();

        let result = mgr.liquidation_warning(user, &btcusdt(), 69.9);
        assert!(result.is_none());
        assert_eq!(mgr.get_unread(user).len(), 0);
    }

    #[test]
    fn test_liquidation_warning_at_70_fires() {
        let mut mgr = NotificationManager::new();
        let user = make_user();

        let result = mgr.liquidation_warning(user, &btcusdt(), 70.0);
        assert!(result.is_some());
        let unread = mgr.get_unread(user);
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].notification_type, NotificationType::PositionLiquidationWarning);
        assert!(unread[0].title.contains("WARNING"));
    }

    #[test]
    fn test_liquidation_warning_at_80_fires_high() {
        let mut mgr = NotificationManager::new();
        let user = make_user();

        mgr.liquidation_warning(user, &btcusdt(), 80.0);
        let unread = mgr.get_unread(user);
        assert_eq!(unread.len(), 1);
        assert!(unread[0].title.contains("HIGH"));
    }

    #[test]
    fn test_liquidation_warning_at_90_fires_critical() {
        let mut mgr = NotificationManager::new();
        let user = make_user();

        mgr.liquidation_warning(user, &btcusdt(), 90.0);
        let unread = mgr.get_unread(user);
        assert_eq!(unread.len(), 1);
        assert!(unread[0].title.contains("CRITICAL"));
    }

    #[test]
    fn test_liquidation_warning_carries_symbol_in_data() {
        let mut mgr = NotificationManager::new();
        let user = make_user();

        mgr.liquidation_warning(user, &btcusdt(), 75.0);
        let unread = mgr.get_unread(user);
        assert_eq!(unread[0].data["symbol"], "BTCUSDT");
    }

    // --- Price alerts ---

    #[test]
    fn test_price_alert_above_triggers_when_price_reaches_target() {
        let mut mgr = NotificationManager::new();
        let user = make_user();

        let prefs = NotificationPreferences {
            order_filled: true,
            liquidation_warning: true,
            login_alert: true,
            price_alerts: vec![PriceAlert {
                symbol: btcusdt(),
                target_price: dec!(50000),
                direction: AlertDirection::Above,
                triggered: false,
            }],
        };
        mgr.set_preferences(user, prefs);

        // Below target — should not fire
        let fired = mgr.check_price_alerts(&btcusdt(), dec!(49999));
        assert!(fired.is_empty());

        // At target — should fire
        let fired = mgr.check_price_alerts(&btcusdt(), dec!(50000));
        assert_eq!(fired.len(), 1);

        let unread = mgr.get_unread(user);
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].notification_type, NotificationType::PriceAlert);
    }

    #[test]
    fn test_price_alert_below_triggers_when_price_drops() {
        let mut mgr = NotificationManager::new();
        let user = make_user();

        let prefs = NotificationPreferences {
            order_filled: true,
            liquidation_warning: true,
            login_alert: true,
            price_alerts: vec![PriceAlert {
                symbol: btcusdt(),
                target_price: dec!(40000),
                direction: AlertDirection::Below,
                triggered: false,
            }],
        };
        mgr.set_preferences(user, prefs);

        // Above target — should not fire
        mgr.check_price_alerts(&btcusdt(), dec!(40001));
        assert_eq!(mgr.get_unread(user).len(), 0);

        // At target — should fire
        mgr.check_price_alerts(&btcusdt(), dec!(40000));
        assert_eq!(mgr.get_unread(user).len(), 1);
    }

    #[test]
    fn test_price_alert_fires_only_once() {
        let mut mgr = NotificationManager::new();
        let user = make_user();

        let prefs = NotificationPreferences {
            order_filled: true,
            liquidation_warning: true,
            login_alert: true,
            price_alerts: vec![PriceAlert {
                symbol: btcusdt(),
                target_price: dec!(50000),
                direction: AlertDirection::Above,
                triggered: false,
            }],
        };
        mgr.set_preferences(user, prefs);

        mgr.check_price_alerts(&btcusdt(), dec!(50000));
        mgr.check_price_alerts(&btcusdt(), dec!(50000));
        mgr.check_price_alerts(&btcusdt(), dec!(55000));

        // Only one notification despite multiple triggers
        assert_eq!(mgr.get_unread(user).len(), 1);
    }

    #[test]
    fn test_price_alert_wrong_symbol_does_not_fire() {
        let mut mgr = NotificationManager::new();
        let user = make_user();

        let prefs = NotificationPreferences {
            order_filled: true,
            liquidation_warning: true,
            login_alert: true,
            price_alerts: vec![PriceAlert {
                symbol: btcusdt(),
                target_price: dec!(50000),
                direction: AlertDirection::Above,
                triggered: false,
            }],
        };
        mgr.set_preferences(user, prefs);

        // Price event for ETHUSDT, not BTCUSDT
        mgr.check_price_alerts(&Symbol::new("ETHUSDT"), dec!(50000));
        assert_eq!(mgr.get_unread(user).len(), 0);
    }

    // --- User preferences ---

    #[test]
    fn test_liquidation_warning_cannot_be_disabled() {
        let mut mgr = NotificationManager::new();
        let user = make_user();

        let prefs = NotificationPreferences {
            order_filled: false,
            liquidation_warning: false, // user tries to disable
            login_alert: false,
            price_alerts: vec![],
        };
        mgr.set_preferences(user, prefs);

        // Despite user's attempt, the preference is forced to true
        let stored = &mgr.preferences[&user];
        assert!(stored.liquidation_warning, "liquidation_warning must always be true");
    }

    #[test]
    fn test_default_preferences_are_all_enabled() {
        let prefs = NotificationPreferences::default();
        assert!(prefs.order_filled);
        assert!(prefs.liquidation_warning);
        assert!(prefs.login_alert);
        assert!(prefs.price_alerts.is_empty());
    }

    #[test]
    fn test_set_preferences_updates_existing() {
        let mut mgr = NotificationManager::new();
        let user = make_user();

        let prefs1 = NotificationPreferences::default();
        mgr.set_preferences(user, prefs1);

        let prefs2 = NotificationPreferences {
            order_filled: false,
            liquidation_warning: true,
            login_alert: false,
            price_alerts: vec![],
        };
        mgr.set_preferences(user, prefs2);

        let stored = &mgr.preferences[&user];
        assert!(!stored.order_filled);
        assert!(!stored.login_alert);
    }
}
