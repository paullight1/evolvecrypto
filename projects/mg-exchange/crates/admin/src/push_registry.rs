//! Push notification device registry — stores per-user push tokens for APNS, FCM,
//! and Web Push delivery.
//!
//! Phase 1: in-memory. Phase 2: persisted to PostgreSQL so tokens survive restarts.

use chrono::{DateTime, Utc};
use exchange_common::UserId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// The mobile/web push delivery platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PushPlatform {
    /// Apple Push Notification service (iOS / macOS).
    Apns,
    /// Firebase Cloud Messaging (Android / cross-platform).
    Fcm,
    /// Web Push (VAPID, browser-based).
    WebPush,
}

/// A device push token submitted by the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceToken {
    /// The raw push token string (APNS device token, FCM registration id, or Web Push endpoint).
    pub token: String,
    /// Which platform this token belongs to.
    pub platform: PushPlatform,
    /// Optional human-readable device label (e.g. "iPhone 15 Pro").
    pub device_name: Option<String>,
}

/// A registered device record as stored in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredDevice {
    /// Unique identifier for this device registration.
    pub id: Uuid,
    /// User who registered this device.
    pub user_id: UserId,
    /// The push token and platform info.
    pub token: DeviceToken,
    /// When the device was first registered.
    pub registered_at: DateTime<Utc>,
    /// When the last push was sent to this device, if any.
    pub last_push_at: Option<DateTime<Utc>>,
}

/// In-memory registry of user push devices.
///
/// Each user may register multiple devices (phone + tablet, for example).
/// Duplicate tokens (same token string) for the same user are deduplicated on
/// registration — the existing record is refreshed rather than duplicated.
#[derive(Default)]
pub struct DeviceRegistry {
    /// user_id → list of registered devices
    devices: HashMap<UserId, Vec<RegisteredDevice>>,
}

impl DeviceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a push token for a user.
    ///
    /// If the same token is already registered for this user, the record is
    /// refreshed (registered_at updated) and its id is returned. Otherwise a
    /// new record is inserted.
    ///
    /// Returns the device id.
    pub fn register(&mut self, user_id: UserId, token: DeviceToken) -> Uuid {
        let entries = self.devices.entry(user_id).or_default();

        // Dedup: if the token string already exists, refresh the timestamp.
        if let Some(existing) = entries.iter_mut().find(|d| d.token.token == token.token) {
            existing.registered_at = Utc::now();
            existing.token = token;
            return existing.id;
        }

        let id = Uuid::new_v4();
        entries.push(RegisteredDevice {
            id,
            user_id,
            token,
            registered_at: Utc::now(),
            last_push_at: None,
        });

        tracing::debug!(%user_id, %id, "push device registered");
        id
    }

    /// Unregister a device by its id.
    ///
    /// Returns `true` if the device was found and removed, `false` otherwise.
    pub fn unregister(&mut self, user_id: UserId, device_id: Uuid) -> bool {
        let entries = self.devices.entry(user_id).or_default();
        let before = entries.len();
        entries.retain(|d| d.id != device_id);
        let removed = entries.len() < before;
        if removed {
            tracing::debug!(%user_id, %device_id, "push device unregistered");
        }
        removed
    }

    /// Return all registered devices for a user (may be empty).
    pub fn user_devices(&self, user_id: UserId) -> &[RegisteredDevice] {
        self.devices
            .get(&user_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Record that a push was successfully sent to a device.
    pub fn record_push_sent(&mut self, device_id: Uuid) {
        for devices in self.devices.values_mut() {
            if let Some(d) = devices.iter_mut().find(|d| d.id == device_id) {
                d.last_push_at = Some(Utc::now());
                return;
            }
        }
    }

    /// Total number of registered devices across all users.
    pub fn total_devices(&self) -> usize {
        self.devices.values().map(|v| v.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> UserId {
        Uuid::new_v4()
    }

    fn apns_token(s: &str) -> DeviceToken {
        DeviceToken {
            token: s.to_string(),
            platform: PushPlatform::Apns,
            device_name: None,
        }
    }

    #[test]
    fn register_new_device_returns_id() {
        let mut reg = DeviceRegistry::new();
        let uid = user();
        let id = reg.register(uid, apns_token("tok1"));
        assert_eq!(reg.user_devices(uid).len(), 1);
        assert_eq!(reg.user_devices(uid)[0].id, id);
    }

    #[test]
    fn duplicate_token_is_deduplicated() {
        let mut reg = DeviceRegistry::new();
        let uid = user();
        let id1 = reg.register(uid, apns_token("tok1"));
        let id2 = reg.register(uid, apns_token("tok1"));
        assert_eq!(id1, id2);
        assert_eq!(reg.user_devices(uid).len(), 1);
    }

    #[test]
    fn different_tokens_both_registered() {
        let mut reg = DeviceRegistry::new();
        let uid = user();
        reg.register(uid, apns_token("tok1"));
        reg.register(uid, apns_token("tok2"));
        assert_eq!(reg.user_devices(uid).len(), 2);
    }

    #[test]
    fn unregister_removes_device() {
        let mut reg = DeviceRegistry::new();
        let uid = user();
        let id = reg.register(uid, apns_token("tok1"));
        assert!(reg.unregister(uid, id));
        assert_eq!(reg.user_devices(uid).len(), 0);
    }

    #[test]
    fn unregister_unknown_id_returns_false() {
        let mut reg = DeviceRegistry::new();
        let uid = user();
        assert!(!reg.unregister(uid, Uuid::new_v4()));
    }

    #[test]
    fn user_devices_empty_for_unknown_user() {
        let reg = DeviceRegistry::new();
        assert!(reg.user_devices(user()).is_empty());
    }

    #[test]
    fn total_devices_counts_across_users() {
        let mut reg = DeviceRegistry::new();
        let u1 = user();
        let u2 = user();
        reg.register(u1, apns_token("t1"));
        reg.register(u1, apns_token("t2"));
        reg.register(u2, apns_token("t3"));
        assert_eq!(reg.total_devices(), 3);
    }
}
