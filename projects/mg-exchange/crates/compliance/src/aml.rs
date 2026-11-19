use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// AML alert type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmlAlertType {
    /// Transaction exceeds reporting threshold (400,000 UAH / ~$10,000).
    ReportingThreshold,
    /// Rapid sequence of deposits followed by withdrawal (structuring).
    Structuring,
    /// Deposit from sanctioned address.
    SanctionedAddress,
    /// Unusually large transaction for this user's history.
    UnusualAmount,
    /// Multiple small deposits just below threshold (smurfing).
    Smurfing,
}

/// AML alert record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmlAlert {
    pub id: Uuid,
    pub user_id: Uuid,
    pub alert_type: AmlAlertType,
    pub amount_usd: Decimal,
    pub description: String,
    pub created_at: DateTime<Utc>,
    /// Whether this alert requires STR (Suspicious Transaction Report) to SFMS.
    pub requires_str: bool,
    /// Whether compliance has reviewed this alert.
    pub reviewed: bool,
}

/// AML checker — screens transactions for suspicious activity.
///
/// Phase 1: rule-based checks with in-memory alert store.
/// Phase 2: integrate sanctions lists (OFAC, EU, NSDC) + ML scoring.
pub struct AmlChecker {
    /// Reporting threshold in USD (transactions above this are reported).
    reporting_threshold_usd: Decimal,
    alerts: Vec<AmlAlert>,
    /// Track recent transaction amounts per user for pattern detection.
    recent_tx: HashMap<Uuid, Vec<(DateTime<Utc>, Decimal)>>,
}

impl AmlChecker {
    pub fn new() -> Self {
        Self {
            // 400,000 UAH ≈ $10,000 USD
            reporting_threshold_usd: Decimal::new(10_000, 0),
            alerts: Vec::new(),
            recent_tx: HashMap::new(),
        }
    }

    /// Screen a deposit/withdrawal transaction.
    /// Returns alerts if any rules are triggered.
    pub fn screen_transaction(
        &mut self,
        user_id: Uuid,
        amount_usd: Decimal,
        is_withdrawal: bool,
    ) -> Vec<AmlAlert> {
        let mut new_alerts = Vec::new();
        let now = Utc::now();

        // Rule 1: Reporting threshold
        if amount_usd >= self.reporting_threshold_usd {
            let alert = AmlAlert {
                id: Uuid::new_v4(),
                user_id,
                alert_type: AmlAlertType::ReportingThreshold,
                amount_usd,
                description: format!(
                    "{} of ${} exceeds reporting threshold (${})",
                    if is_withdrawal { "Withdrawal" } else { "Deposit" },
                    amount_usd,
                    self.reporting_threshold_usd,
                ),
                created_at: now,
                requires_str: true, // Mandatory reporting to SFMS
                reviewed: false,
            };
            new_alerts.push(alert);
        }

        // Track transaction for pattern detection
        let user_txs = self.recent_tx.entry(user_id).or_default();
        user_txs.push((now, amount_usd));

        // Keep only last 24h of transactions
        let cutoff = now - chrono::Duration::hours(24);
        user_txs.retain(|(ts, _)| *ts > cutoff);

        // Rule 2: Smurfing — 5+ transactions just below threshold in 24h
        let near_threshold = user_txs
            .iter()
            .filter(|(_, amt)| {
                *amt >= self.reporting_threshold_usd * Decimal::new(8, 1) // 80% of threshold
                    && *amt < self.reporting_threshold_usd
            })
            .count();

        if near_threshold >= 5 {
            let alert = AmlAlert {
                id: Uuid::new_v4(),
                user_id,
                alert_type: AmlAlertType::Smurfing,
                amount_usd,
                description: format!(
                    "Potential smurfing: {} transactions near threshold in 24h",
                    near_threshold,
                ),
                created_at: now,
                requires_str: true,
                reviewed: false,
            };
            new_alerts.push(alert);
        }

        // Rule 3: Structuring — deposits followed by quick withdrawal
        if is_withdrawal {
            let deposit_sum: Decimal = user_txs.iter().map(|(_, amt)| *amt).sum();
            if deposit_sum > Decimal::ZERO && amount_usd > deposit_sum * Decimal::new(9, 1) {
                // Withdrawing > 90% of deposits within 24h
                let alert = AmlAlert {
                    id: Uuid::new_v4(),
                    user_id,
                    alert_type: AmlAlertType::Structuring,
                    amount_usd,
                    description: format!(
                        "Rapid deposit-withdrawal: deposited ${}, withdrawing ${}",
                        deposit_sum, amount_usd,
                    ),
                    created_at: now,
                    requires_str: false, // Flagged for manual review
                    reviewed: false,
                };
                new_alerts.push(alert);
            }
        }

        // Store alerts
        self.alerts.extend(new_alerts.clone());

        new_alerts
    }

    /// Screen a crypto address against sanctions lists.
    ///
    /// Phase 1: stub — always returns clean.
    /// Phase 2: integrate Chainalysis/Elliptic API.
    pub fn screen_address(&self, _address: &str) -> AddressScreenResult {
        // TODO: integrate with sanctions screening provider
        AddressScreenResult::Clean
    }

    /// Get all unreviewed alerts.
    pub fn unreviewed_alerts(&self) -> Vec<&AmlAlert> {
        self.alerts.iter().filter(|a| !a.reviewed).collect()
    }

    /// Get alerts for a specific user.
    pub fn user_alerts(&self, user_id: &Uuid) -> Vec<&AmlAlert> {
        self.alerts.iter().filter(|a| a.user_id == *user_id).collect()
    }

    /// Mark an alert as reviewed.
    pub fn review_alert(&mut self, alert_id: &Uuid) -> bool {
        if let Some(alert) = self.alerts.iter_mut().find(|a| a.id == *alert_id) {
            alert.reviewed = true;
            true
        } else {
            false
        }
    }

    /// Get alerts requiring STR filing.
    pub fn str_required(&self) -> Vec<&AmlAlert> {
        self.alerts.iter().filter(|a| a.requires_str && !a.reviewed).collect()
    }

    pub fn total_alerts(&self) -> usize {
        self.alerts.len()
    }
}

impl Default for AmlChecker {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of address screening.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressScreenResult {
    Clean,
    Sanctioned { list: String },
    HighRisk { reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_small_transaction_no_alert() {
        let mut checker = AmlChecker::new();
        let user = Uuid::new_v4();

        let alerts = checker.screen_transaction(user, dec!(100), false);
        assert!(alerts.is_empty());
    }

    #[test]
    fn test_above_threshold_triggers_alert() {
        let mut checker = AmlChecker::new();
        let user = Uuid::new_v4();

        let alerts = checker.screen_transaction(user, dec!(15000), false);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].alert_type, AmlAlertType::ReportingThreshold);
        assert!(alerts[0].requires_str);
    }

    #[test]
    fn test_exact_threshold_triggers() {
        let mut checker = AmlChecker::new();
        let user = Uuid::new_v4();

        let alerts = checker.screen_transaction(user, dec!(10000), false);
        assert_eq!(alerts.len(), 1);
    }

    #[test]
    fn test_smurfing_detection() {
        let mut checker = AmlChecker::new();
        let user = Uuid::new_v4();

        // 5 transactions at $9,000 each (just below $10,000 threshold)
        for _ in 0..5 {
            checker.screen_transaction(user, dec!(9000), false);
        }

        // The 5th transaction should trigger smurfing alert
        let user_alerts = checker.user_alerts(&user);
        assert!(user_alerts.iter().any(|a| a.alert_type == AmlAlertType::Smurfing));
    }

    #[test]
    fn test_review_alert() {
        let mut checker = AmlChecker::new();
        let user = Uuid::new_v4();

        checker.screen_transaction(user, dec!(15000), false);
        assert_eq!(checker.unreviewed_alerts().len(), 1);

        let alert_id = checker.unreviewed_alerts()[0].id;
        checker.review_alert(&alert_id);
        assert_eq!(checker.unreviewed_alerts().len(), 0);
    }

    #[test]
    fn test_str_required() {
        let mut checker = AmlChecker::new();
        let user = Uuid::new_v4();

        checker.screen_transaction(user, dec!(15000), false);
        assert_eq!(checker.str_required().len(), 1);
    }

    #[test]
    fn test_address_screening_stub() {
        let checker = AmlChecker::new();
        assert_eq!(
            checker.screen_address("0xabc123"),
            AddressScreenResult::Clean
        );
    }
}
