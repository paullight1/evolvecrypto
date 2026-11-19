//! Yield strategy allocator.
//!
//! `YieldAllocator` holds a set of `DeFiProtocol` instances and answers two
//! questions:
//!
//! 1. **Where can I get the best yield for a given asset?** (`best_yield`)
//! 2. **How should I split a deposit across protocols for optimal risk-adjusted
//!    return?** (`allocate`)
//! 3. **Should any existing allocations be rebalanced?** (`rebalance`)
//!
//! # Allocation strategy
//! - Allocations are ranked by APY, highest first.
//! - A protocol is skipped if its `risk_score > max_risk_score`.
//! - No single protocol receives more than `max_per_protocol_pct` of the total
//!   deposit. The remainder flows to the next best protocol.
//! - Any residual that cannot be placed (all protocols at cap or exhausted) is
//!   left unallocated (returned as an empty-amount entry).
//!
//! # Rebalancing
//! A rebalance is recommended when a source allocation's APY is at least
//! `REBALANCE_THRESHOLD_BPS` basis points below the best available APY for the
//! same asset.

use anyhow::{bail, Result};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;
use chrono::Utc;

use crate::protocols::DeFiProtocol;
use crate::types::{AllocationStatus, RebalanceAction, YieldAllocation, YieldOpportunity};

/// Minimum APY improvement (in absolute fraction) that triggers a rebalance.
/// 0.003 = 30 bps.
const REBALANCE_THRESHOLD: Decimal = dec!(0.003);

/// Yield allocator: selects protocols and splits capital across them.
pub struct YieldAllocator {
    protocols: Vec<Box<dyn DeFiProtocol>>,
    /// Maximum fraction of a deposit that can go to any single protocol.
    /// E.g. `0.30` = 30%.
    max_per_protocol_pct: Decimal,
    /// Protocols with `risk_score > max_risk_score` are excluded.
    max_risk_score: u8,
}

impl YieldAllocator {
    /// Create a new allocator with the given protocols and constraints.
    ///
    /// `max_per_protocol_pct` must be in (0, 1].
    /// `max_risk_score` must be in [1, 10].
    pub fn new(
        protocols: Vec<Box<dyn DeFiProtocol>>,
        max_per_protocol_pct: Decimal,
        max_risk_score: u8,
    ) -> Self {
        debug_assert!(
            max_per_protocol_pct > Decimal::ZERO && max_per_protocol_pct <= dec!(1),
            "max_per_protocol_pct must be in (0, 1]"
        );
        Self {
            protocols,
            max_per_protocol_pct,
            max_risk_score,
        }
    }

    /// Default allocator: Aave v3, Lido, Compound v3, EigenLayer.
    /// Max 30% per protocol; excludes protocols with risk score > 5.
    pub fn with_defaults() -> Self {
        use crate::protocols::{aave::AaveProtocol, compound::CompoundProtocol, eigenlayer::EigenLayerProtocol, lido::LidoProtocol};
        Self::new(
            vec![
                Box::new(AaveProtocol::new()),
                Box::new(LidoProtocol::new()),
                Box::new(CompoundProtocol::new()),
                Box::new(EigenLayerProtocol::new()),
            ],
            dec!(0.30),
            5,
        )
    }

    /// Return all yield opportunities for `asset` across eligible protocols,
    /// sorted by APY descending.
    ///
    /// Protocols with `risk_score > max_risk_score` are excluded.
    pub async fn best_yield(&self, asset: &str) -> Result<Vec<YieldOpportunity>> {
        let asset_upper = asset.to_uppercase();
        let mut opportunities: Vec<YieldOpportunity> = Vec::new();

        for protocol in &self.protocols {
            if protocol.risk_score() > self.max_risk_score {
                tracing::debug!(
                    protocol = protocol.name(),
                    risk_score = protocol.risk_score(),
                    max = self.max_risk_score,
                    "skipping protocol: risk score exceeds threshold"
                );
                continue;
            }

            if !protocol.supports_asset(&asset_upper) {
                continue;
            }

            match protocol.opportunities().await {
                Ok(opps) => {
                    for opp in opps {
                        if opp.asset.eq_ignore_ascii_case(&asset_upper) {
                            opportunities.push(opp);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        protocol = protocol.name(),
                        error = %e,
                        "failed to fetch opportunities; skipping"
                    );
                }
            }
        }

        // Sort by APY descending.
        opportunities.sort_by(|a, b| b.apy.partial_cmp(&a.apy).unwrap_or(std::cmp::Ordering::Equal));
        Ok(opportunities)
    }

    /// Allocate `amount` of `asset` across the best eligible protocols.
    ///
    /// Returns a list of `YieldAllocation` records (one per protocol used).
    /// The `tx_hash` from each simulated deposit is stored in a log message.
    pub async fn allocate(
        &self,
        user_id: Uuid,
        asset: &str,
        amount: Decimal,
    ) -> Result<Vec<YieldAllocation>> {
        if amount <= Decimal::ZERO {
            bail!("allocation amount must be positive");
        }

        let opportunities = self.best_yield(asset).await?;

        if opportunities.is_empty() {
            bail!(
                "no eligible yield protocols found for '{}' (max_risk_score={})",
                asset,
                self.max_risk_score
            );
        }

        let max_per_protocol = amount * self.max_per_protocol_pct;
        let mut remaining = amount;
        let mut allocations = Vec::new();
        let now = Utc::now();

        for opp in &opportunities {
            if remaining <= Decimal::ZERO {
                break;
            }

            // Find the protocol instance.
            let protocol = match self.find_protocol(&opp.protocol) {
                Some(p) => p,
                None => continue,
            };

            let chunk = remaining.min(max_per_protocol);

            // Check min_deposit constraint.
            if chunk < opp.min_deposit {
                tracing::debug!(
                    protocol = %opp.protocol,
                    chunk = %chunk,
                    min_deposit = %opp.min_deposit,
                    "skipping protocol: chunk below min_deposit"
                );
                continue;
            }

            // Simulate the deposit.
            match protocol.deposit(asset, chunk).await {
                Ok(tx_hash) => {
                    tracing::info!(
                        user_id = %user_id,
                        protocol = %opp.protocol,
                        asset = %asset,
                        amount = %chunk,
                        apy = %opp.apy,
                        tx = %tx_hash,
                        "yield allocation created"
                    );

                    allocations.push(YieldAllocation {
                        id: Uuid::new_v4(),
                        user_id,
                        protocol: opp.protocol.clone(),
                        asset: asset.to_uppercase(),
                        amount: chunk,
                        current_apy: opp.apy,
                        earned_yield: Decimal::ZERO,
                        deposited_at: now,
                        status: AllocationStatus::Active,
                    });

                    remaining -= chunk;
                }
                Err(e) => {
                    tracing::warn!(
                        protocol = %opp.protocol,
                        error = %e,
                        "deposit failed; skipping protocol"
                    );
                }
            }
        }

        if allocations.is_empty() {
            bail!("all protocols rejected the deposit for asset '{asset}'");
        }

        Ok(allocations)
    }

    /// Analyse existing allocations and return recommended rebalance actions.
    ///
    /// A rebalance is suggested when the best available APY for an allocation's
    /// asset exceeds the allocation's current APY by at least `REBALANCE_THRESHOLD`.
    pub async fn rebalance(&self, current: &[YieldAllocation]) -> Result<Vec<RebalanceAction>> {
        let mut actions = Vec::new();

        for alloc in current {
            if alloc.status != AllocationStatus::Active {
                continue;
            }

            let best = self.best_yield(&alloc.asset).await?;
            let best_opp = match best.first() {
                Some(o) => o,
                None => continue,
            };

            // Skip if already in the best protocol.
            if best_opp.protocol == alloc.protocol {
                continue;
            }

            let improvement = best_opp.apy - alloc.current_apy;
            if improvement >= REBALANCE_THRESHOLD {
                tracing::debug!(
                    from = %alloc.protocol,
                    to = %best_opp.protocol,
                    asset = %alloc.asset,
                    current_apy = %alloc.current_apy,
                    best_apy = %best_opp.apy,
                    improvement_bps = %(improvement * dec!(10000)),
                    "rebalance recommended"
                );

                actions.push(RebalanceAction {
                    from_protocol: alloc.protocol.clone(),
                    to_protocol: best_opp.protocol.clone(),
                    asset: alloc.asset.clone(),
                    amount: alloc.amount,
                    reason: format!(
                        "move to {} for {:.2}% APY (currently {:.2}% at {}; improvement: {:.0} bps)",
                        best_opp.protocol,
                        best_opp.apy * dec!(100),
                        alloc.current_apy * dec!(100),
                        alloc.protocol,
                        improvement * dec!(10000),
                    ),
                });
            }
        }

        Ok(actions)
    }

    // ── private helpers ───────────────────────────────────────────────────────

    fn find_protocol(&self, name: &str) -> Option<&dyn DeFiProtocol> {
        self.protocols
            .iter()
            .find(|p| p.name() == name)
            .map(|p| p.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn test_allocator() -> YieldAllocator {
        YieldAllocator::with_defaults()
    }

    #[tokio::test]
    async fn best_yield_usdc_returns_sorted_desc() {
        let alloc = test_allocator();
        let opps = alloc.best_yield("USDC").await.unwrap();
        assert!(!opps.is_empty());
        // Verify descending APY order.
        for w in opps.windows(2) {
            assert!(w[0].apy >= w[1].apy);
        }
    }

    #[tokio::test]
    async fn best_yield_eth_excludes_high_risk() {
        // Default max_risk_score = 5; EigenLayer risk_score = 6 → excluded.
        let alloc = test_allocator();
        let opps = alloc.best_yield("ETH").await.unwrap();
        assert!(opps.iter().all(|o| o.risk_score <= 5));
        assert!(!opps.iter().any(|o| o.protocol == "EigenLayer"));
    }

    #[tokio::test]
    async fn best_yield_unknown_asset_empty() {
        let alloc = test_allocator();
        let opps = alloc.best_yield("DOGE").await.unwrap();
        assert!(opps.is_empty());
    }

    #[tokio::test]
    async fn allocate_usdc_produces_allocations() {
        let alloc = test_allocator();
        let user_id = Uuid::new_v4();
        let result = alloc.allocate(user_id, "USDC", dec!(1000)).await.unwrap();
        assert!(!result.is_empty());
        // Total allocated must equal deposit.
        let total: Decimal = result.iter().map(|a| a.amount).sum();
        assert_eq!(total, dec!(1000));
    }

    #[tokio::test]
    async fn allocate_respects_max_per_protocol_cap() {
        // max_per_protocol_pct = 30% → each protocol gets at most 300 USDC.
        let alloc = test_allocator();
        let user_id = Uuid::new_v4();
        let result = alloc.allocate(user_id, "USDC", dec!(1000)).await.unwrap();
        for a in &result {
            assert!(
                a.amount <= dec!(300) + dec!(0.0001), // small epsilon for rounding
                "protocol {} received {}, expected <= 300",
                a.protocol,
                a.amount
            );
        }
    }

    #[tokio::test]
    async fn allocate_zero_amount_errors() {
        let alloc = test_allocator();
        assert!(alloc.allocate(Uuid::new_v4(), "USDC", Decimal::ZERO).await.is_err());
    }

    #[tokio::test]
    async fn allocate_unknown_asset_errors() {
        let alloc = test_allocator();
        assert!(alloc.allocate(Uuid::new_v4(), "DOGE", dec!(100)).await.is_err());
    }

    #[tokio::test]
    async fn rebalance_suggests_action_when_better_apy_available() {
        let alloc = test_allocator();
        // Compound USDC APY = 3.8%. Aave USDC APY = 4.5%.
        // An allocation stuck in Compound should trigger a rebalance toward Aave.
        let stale = YieldAllocation {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            protocol: "Compound v3".into(),
            asset: "USDC".into(),
            amount: dec!(1000),
            current_apy: dec!(0.038),
            earned_yield: Decimal::ZERO,
            deposited_at: Utc::now(),
            status: AllocationStatus::Active,
        };
        let actions = alloc.rebalance(&[stale]).await.unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].from_protocol, "Compound v3");
        assert_eq!(actions[0].to_protocol, "Aave v3");
    }

    #[tokio::test]
    async fn rebalance_no_action_when_already_best() {
        let alloc = test_allocator();
        // Already in Aave (highest APY for USDC) — no rebalance needed.
        let best = YieldAllocation {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            protocol: "Aave v3".into(),
            asset: "USDC".into(),
            amount: dec!(1000),
            current_apy: dec!(0.045),
            earned_yield: Decimal::ZERO,
            deposited_at: Utc::now(),
            status: AllocationStatus::Active,
        };
        let actions = alloc.rebalance(&[best]).await.unwrap();
        assert!(actions.is_empty());
    }

    #[tokio::test]
    async fn rebalance_skips_inactive_allocations() {
        let alloc = test_allocator();
        let withdrawn = YieldAllocation {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            protocol: "Compound v3".into(),
            asset: "USDC".into(),
            amount: dec!(500),
            current_apy: dec!(0.01), // very low — would normally trigger rebalance
            earned_yield: Decimal::ZERO,
            deposited_at: Utc::now(),
            status: AllocationStatus::Withdrawn,
        };
        let actions = alloc.rebalance(&[withdrawn]).await.unwrap();
        assert!(actions.is_empty());
    }
}
