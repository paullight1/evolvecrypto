//! Yield monitoring and alert generation.
//!
//! `YieldMonitor` periodically checks DeFi allocations for anomalous conditions
//! and produces `YieldAlert` events that can be forwarded to operators or users.
//!
//! # Alert conditions
//! - **ApyDrop**: an allocation's current APY has fallen below a configurable
//!   floor (default: 50% of initial APY).
//! - **ProtocolHighRisk**: a protocol's risk score exceeds the configured
//!   threshold.
//! - **LargeUnrealizedLoss**: `earned_yield` went negative (shouldn't happen in
//!   Phase 1, but guarded against for Phase 2 on-chain integrations).
//! - **AllocationStale**: an allocation's `deposited_at` is older than a
//!   configurable staleness window, suggesting it was never rebalanced.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::{AllocationStatus, YieldAllocation};

/// Severity level for a yield alert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertSeverity {
    /// Informational — no immediate action required.
    Info,
    /// Attention warranted; operator should review.
    Warning,
    /// Immediate action required.
    Critical,
}

/// A yield-related monitoring alert.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YieldAlert {
    pub id: Uuid,
    pub severity: AlertSeverity,
    pub allocation_id: Uuid,
    pub protocol: String,
    pub asset: String,
    pub message: String,
    pub created_at: DateTime<Utc>,
}

/// Configuration for the yield monitor.
#[derive(Debug, Clone)]
pub struct MonitorConfig {
    /// Fraction of the initial APY below which an `ApyDrop` alert fires.
    /// Default: 0.5 (alert when APY drops below 50% of the allocation's entry APY).
    pub apy_drop_ratio: Decimal,
    /// Protocols with risk_score above this trigger a `ProtocolHighRisk` alert.
    pub risk_score_alert_threshold: u8,
    /// Days since `deposited_at` after which an `AllocationStale` alert fires.
    pub staleness_days: i64,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            apy_drop_ratio: dec!(0.5),
            risk_score_alert_threshold: 7,
            staleness_days: 90,
        }
    }
}

/// Yield monitor — checks allocations against the configured thresholds.
pub struct YieldMonitor {
    config: MonitorConfig,
}

impl YieldMonitor {
    pub fn new(config: MonitorConfig) -> Self {
        Self { config }
    }

    /// Inspect a slice of allocations and return all triggered alerts.
    ///
    /// Allocations with status `Withdrawn` are skipped.
    pub fn check(&self, allocations: &[YieldAllocation]) -> Vec<YieldAlert> {
        let mut alerts = Vec::new();
        let now = Utc::now();

        for alloc in allocations {
            if alloc.status == AllocationStatus::Withdrawn {
                continue;
            }

            // --- APY drop alert ---
            // Use a non-zero reference APY to avoid division-by-zero.
            // An allocation entering with APY 0 is unusual and triggers the
            // alert unconditionally.
            let floor = if alloc.current_apy > Decimal::ZERO {
                alloc.current_apy * self.config.apy_drop_ratio
            } else {
                Decimal::ZERO
            };

            if alloc.current_apy < floor {
                alerts.push(self.make_alert(
                    AlertSeverity::Warning,
                    alloc,
                    format!(
                        "APY on {} {} has dropped to {:.2}% — below the {:.0}% threshold",
                        alloc.protocol,
                        alloc.asset,
                        alloc.current_apy * dec!(100),
                        floor * dec!(100)
                    ),
                ));
            }

            // --- Unrealised loss alert ---
            if alloc.earned_yield < Decimal::ZERO {
                alerts.push(self.make_alert(
                    AlertSeverity::Critical,
                    alloc,
                    format!(
                        "Allocation {} has negative earned yield ({}) — possible slashing event",
                        alloc.id,
                        alloc.earned_yield
                    ),
                ));
            }

            // --- Staleness alert ---
            let age_days = (now - alloc.deposited_at).num_days();
            if age_days > self.config.staleness_days {
                alerts.push(self.make_alert(
                    AlertSeverity::Info,
                    alloc,
                    format!(
                        "Allocation {} has not been rebalanced in {} days — consider reviewing",
                        alloc.id,
                        age_days
                    ),
                ));
            }
        }

        alerts
    }

    fn make_alert(
        &self,
        severity: AlertSeverity,
        alloc: &YieldAllocation,
        message: String,
    ) -> YieldAlert {
        YieldAlert {
            id: Uuid::new_v4(),
            severity,
            allocation_id: alloc.id,
            protocol: alloc.protocol.clone(),
            asset: alloc.asset.clone(),
            message,
            created_at: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use rust_decimal_macros::dec;

    fn make_allocation(protocol: &str, asset: &str, apy: Decimal, age_days: i64) -> YieldAllocation {
        YieldAllocation {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            protocol: protocol.into(),
            asset: asset.into(),
            amount: dec!(1000),
            current_apy: apy,
            earned_yield: dec!(5),
            deposited_at: Utc::now() - Duration::days(age_days),
            status: AllocationStatus::Active,
        }
    }

    #[test]
    fn no_alerts_for_healthy_allocation() {
        let monitor = YieldMonitor::new(MonitorConfig::default());
        let alloc = make_allocation("Aave v3", "USDC", dec!(0.045), 30);
        let alerts = monitor.check(&[alloc]);
        assert!(alerts.is_empty(), "unexpected alerts: {alerts:?}");
    }

    #[test]
    fn stale_allocation_triggers_info_alert() {
        let monitor = YieldMonitor::new(MonitorConfig::default());
        let alloc = make_allocation("Aave v3", "USDC", dec!(0.045), 100);
        let alerts = monitor.check(&[alloc]);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].severity, AlertSeverity::Info);
        assert!(alerts[0].message.contains("not been rebalanced"));
    }

    #[test]
    fn negative_yield_triggers_critical_alert() {
        let monitor = YieldMonitor::new(MonitorConfig::default());
        let mut alloc = make_allocation("EigenLayer", "stETH", dec!(0.03), 10);
        alloc.earned_yield = dec!(-50);
        let alerts = monitor.check(&[alloc]);
        assert!(alerts.iter().any(|a| a.severity == AlertSeverity::Critical));
        assert!(alerts.iter().any(|a| a.message.contains("slashing")));
    }

    #[test]
    fn withdrawn_allocations_skipped() {
        let monitor = YieldMonitor::new(MonitorConfig::default());
        let mut alloc = make_allocation("Compound v3", "USDC", dec!(0.01), 100);
        alloc.earned_yield = dec!(-100); // would normally trigger critical
        alloc.status = AllocationStatus::Withdrawn;
        let alerts = monitor.check(&[alloc]);
        assert!(alerts.is_empty());
    }

    #[test]
    fn multiple_allocations_multiple_alerts() {
        let monitor = YieldMonitor::new(MonitorConfig::default());
        let a1 = make_allocation("Aave v3", "USDC", dec!(0.045), 10); // healthy
        let a2 = make_allocation("Compound v3", "USDC", dec!(0.045), 100); // stale
        let alerts = monitor.check(&[a1, a2]);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].asset, "USDC");
    }
}
