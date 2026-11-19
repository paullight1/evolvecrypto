//! M24-7: Adversarial test suite for infrastructure persistence.
//!
//! Validates:
//! 1. Account freeze persists across in-memory state (DB write-behind readiness)
//! 2. Audit log records security-critical operations
//! 3. Rate-limit store abstraction works
//! 4. IP reputation store abstraction works
//! 5. Notification preferences persist new push fields

use std::sync::Arc;

use exchange_api_gateway::exchange::Exchange;
use exchange_api_gateway::rate_limit::{create_rate_limit_store, InMemoryRateLimitStore, RateLimitStore};
use exchange_api_gateway::security::{create_reputation_store, InMemoryReputationStore, ReputationStore};
use exchange_common::*;
use rust_decimal_macros::dec;
use uuid::Uuid;

fn setup_exchange() -> Arc<Exchange> {
    let exchange = Exchange::new();
    let product = Product {
        symbol: Symbol::new("BTCUSDT"),
        base_asset: "BTC".to_string(),
        quote_asset: "USDT".to_string(),
        product_type: ProductType::Perpetual,
        tick_size: dec!(0.01),
        lot_size: dec!(0.001),
        min_notional: dec!(10),
        maker_fee: dec!(0.001),
        taker_fee: dec!(0.002),
        max_order_qty: None,
        max_order_notional: None,
        max_leverage: Some(dec!(100)),
    };
    exchange.add_product(product);
    exchange
}

// ── Freeze persistence readiness ──────────────────────────────────────────

#[test]
fn freeze_state_survives_in_memory() {
    let exchange = setup_exchange();
    let user = Uuid::new_v4();
    exchange.create_account(user);

    assert!(exchange.get_account_status(user).can_trade());

    exchange.freeze_account(user);
    assert!(!exchange.get_account_status(user).can_trade());

    // Verify the status is Frozen (not just "some non-Active status").
    assert_eq!(
        exchange.get_account_status(user),
        exchange_common::types::AccountStatus::Frozen
    );

    exchange.unfreeze_account(user);
    assert!(exchange.get_account_status(user).can_trade());
    assert_eq!(
        exchange.get_account_status(user),
        exchange_common::types::AccountStatus::Active
    );
}

// ── Audit log completeness ────────────────────────────────────────────────

#[test]
fn audit_log_records_freeze_and_unfreeze() {
    let exchange = setup_exchange();
    let user = Uuid::new_v4();
    exchange.create_account(user);

    exchange.freeze_account(user);
    exchange.unfreeze_account(user);

    let log = exchange.audit_log.lock().unwrap_or_else(|e| e.into_inner());
    let entries = log.entries_since_id(0);

    let freeze_entry = entries.iter().find(|e| {
        e.action == exchange_admin::AuditAction::AccountFrozen
            && e.user_id == Some(user)
    });
    assert!(freeze_entry.is_some(), "freeze should create an audit entry");

    let unfreeze_entry = entries.iter().find(|e| {
        e.action == exchange_admin::AuditAction::AccountUnfrozen
            && e.user_id == Some(user)
    });
    assert!(unfreeze_entry.is_some(), "unfreeze should create an audit entry");
}

#[test]
fn audit_helper_creates_tamper_evident_chain() {
    let exchange = setup_exchange();
    let user = Uuid::new_v4();

    exchange.audit(
        Some(user),
        exchange_admin::AuditAction::Login,
        serde_json::json!({"test": true}),
        Some("1.2.3.4".to_string()),
    );
    exchange.audit(
        Some(user),
        exchange_admin::AuditAction::ApiKeyCreated,
        serde_json::json!({"key": "abc"}),
        None,
    );

    let log = exchange.audit_log.lock().unwrap_or_else(|e| e.into_inner());
    let entries = log.entries_since_id(0);
    assert_eq!(entries.len(), 2);

    // Verify chain: second entry's prev_hash matches first entry's hash.
    assert_eq!(entries[1].prev_hash, entries[0].entry_hash);
    // Hashes must not be empty.
    assert!(!entries[0].entry_hash.is_empty());
    assert!(!entries[1].entry_hash.is_empty());
}

// ── Rate-limit store abstraction ──────────────────────────────────────────

#[test]
fn in_memory_rate_limit_store_basic() {
    let store = InMemoryRateLimitStore::new();

    // 3 tokens, should allow 3 requests.
    assert!(store.try_consume("client-a", 1.0, 3.0));
    assert!(store.try_consume("client-a", 1.0, 3.0));
    assert!(store.try_consume("client-a", 1.0, 3.0));
    // 4th should be blocked.
    assert!(!store.try_consume("client-a", 1.0, 3.0));

    // Different key is independent.
    assert!(store.try_consume("client-b", 1.0, 3.0));
}

#[test]
fn in_memory_rate_limit_store_weight_tracking() {
    let store = InMemoryRateLimitStore::new();
    let (w1, o1) = store.record_weight("user-1", 5, true);
    assert_eq!(w1, 5);
    assert_eq!(o1, 1);

    let (w2, o2) = store.record_weight("user-1", 3, false);
    assert_eq!(w2, 8);
    assert_eq!(o2, 1); // only first was an order
}

#[test]
fn create_rate_limit_store_returns_in_memory() {
    let store = create_rate_limit_store();
    // Should work as a functioning store.
    assert!(store.try_consume("test", 10.0, 10.0));
}

// ── IP reputation store abstraction ───────────────────────────────────────

#[test]
fn in_memory_reputation_store_tracks_failures() {
    let store = InMemoryReputationStore::new();

    // Should not be blocked initially.
    assert!(!store.is_blocked("1.2.3.4"));

    // Record failures up to threshold (10).
    for _ in 0..9 {
        assert!(!store.record_failure("1.2.3.4"));
    }
    // 10th should trigger block.
    assert!(store.record_failure("1.2.3.4"));
    assert!(store.is_blocked("1.2.3.4"));

    // Unblock.
    store.unblock("1.2.3.4");
    assert!(!store.is_blocked("1.2.3.4"));
}

#[test]
fn in_memory_reputation_store_success_resets() {
    let store = InMemoryReputationStore::new();

    for _ in 0..5 {
        store.record_failure("5.6.7.8");
    }
    store.record_success("5.6.7.8");

    // After success, counter resets — need full 10 again.
    for _ in 0..9 {
        assert!(!store.record_failure("5.6.7.8"));
    }
    assert!(store.record_failure("5.6.7.8"));
}

#[test]
fn create_reputation_store_returns_in_memory() {
    let store = create_reputation_store();
    assert!(!store.is_blocked("test-ip"));
    store.record_failure("test-ip");
    assert!(!store.is_blocked("test-ip")); // one failure not enough
}

// ── Notification preferences new push fields ──────────────────────────────

#[test]
fn notification_preferences_default_has_all_push_fields() {
    let prefs = exchange_api_gateway::exchange::NotificationPreferences::default();
    assert!(prefs.push_fills);
    assert!(prefs.push_deposits);
    assert!(prefs.push_withdrawals);
    assert!(prefs.push_margin_call);
    assert!(prefs.push_liquidation);
    assert!(prefs.push_price_alerts);
    assert!(prefs.push_funding);
}

#[test]
fn notification_preferences_toggles_work() {
    let prefs = exchange_api_gateway::exchange::NotificationPreferences {
        push_funding: false,
        push_deposits: false,
        ..Default::default()
    };
    assert!(!prefs.push_funding);
    assert!(!prefs.push_deposits);
    assert!(prefs.push_fills); // unchanged
}
