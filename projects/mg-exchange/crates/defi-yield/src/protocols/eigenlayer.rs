//! EigenLayer restaking integration.
//!
//! EigenLayer allows ETH liquid staking tokens (stETH, rETH) to be restaked
//! in Actively Validated Services (AVS), earning additional rewards on top of
//! the base liquid staking yield.
//!
//! Phase 1: simulated APYs. No live RPC calls.
//!
//! Phase 2: query AVS reward rates from the EigenLayer operator registry.
//!
//! # Risk note
//! EigenLayer introduces additional slashing risk: an AVS can slash restaked
//! assets if it detects misbehaviour. This makes it materially riskier than
//! vanilla liquid staking, hence risk_score = 6.

use anyhow::{bail, Result};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;

use crate::protocols::{BoxFuture, DeFiProtocol};
use crate::types::{AuditStatus, DeFiProtocolInfo, YieldOpportunity, YieldStrategy};

/// Assets accepted for restaking and their additional (incremental) APYs.
///
/// These are *on top of* the base liquid staking yield (e.g. Lido's 3.5%).
/// The total APY for a restaker is: LST APY + EigenLayer incremental APY.
const EIGENLAYER_RATES: &[(&str, Decimal)] = &[
    ("stETH", dec!(0.030)), // +3.0% on top of Lido 3.5% → ~6.5% total
    ("rETH",  dec!(0.025)), // +2.5% on top of Rocket Pool ~3.6% → ~6.1% total
];

pub struct EigenLayerProtocol;

impl EigenLayerProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EigenLayerProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl DeFiProtocol for EigenLayerProtocol {
    fn name(&self) -> &str {
        "EigenLayer"
    }

    fn chain(&self) -> &str {
        "ethereum"
    }

    fn supported_assets(&self) -> Vec<String> {
        EIGENLAYER_RATES.iter().map(|(a, _)| (*a).to_string()).collect()
    }

    fn get_apy(&self, asset: &str) -> BoxFuture<Result<Decimal>> {
        let asset = asset.to_string();
        Box::pin(async move {
            EIGENLAYER_RATES
                .iter()
                .find(|(a, _)| a.eq_ignore_ascii_case(&asset))
                .map(|(_, apy)| *apy)
                .ok_or_else(|| anyhow::anyhow!(
                    "EigenLayer does not support asset '{asset}'; supported: stETH, rETH"
                ))
        })
    }

    fn deposit(&self, asset: &str, amount: Decimal) -> BoxFuture<Result<String>> {
        let asset = asset.to_string();
        let supports = self.supports_asset(&asset);
        Box::pin(async move {
            if !supports {
                bail!("EigenLayer does not support asset '{asset}'; supported: stETH, rETH");
            }
            if amount <= Decimal::ZERO {
                bail!("deposit amount must be positive");
            }
            let tx_hash = format!("0xeigen_restake_{}_{}", asset.to_lowercase(), Uuid::new_v4().simple());
            tracing::info!(
                protocol = "EigenLayer",
                asset = %asset,
                amount = %amount,
                tx = %tx_hash,
                "simulated restake deposit"
            );
            Ok(tx_hash)
        })
    }

    fn withdraw(&self, asset: &str, amount: Decimal) -> BoxFuture<Result<String>> {
        let asset = asset.to_string();
        let supports = self.supports_asset(&asset);
        Box::pin(async move {
            if !supports {
                bail!("EigenLayer does not support asset '{asset}'");
            }
            if amount <= Decimal::ZERO {
                bail!("withdrawal amount must be positive");
            }
            // EigenLayer has a 7-day unbonding period.
            let tx_hash = format!("0xeigen_undelegate_{}_{}", asset.to_lowercase(), Uuid::new_v4().simple());
            tracing::info!(
                protocol = "EigenLayer",
                asset = %asset,
                amount = %amount,
                tx = %tx_hash,
                "simulated restake withdrawal (7-day unbonding)"
            );
            Ok(tx_hash)
        })
    }

    fn risk_score(&self) -> u8 {
        // Risk 6: newer protocol, additional AVS slashing risk, complex
        // operator/AVS relationship, significant smart-contract surface area.
        6
    }

    fn info(&self) -> DeFiProtocolInfo {
        DeFiProtocolInfo {
            name: "EigenLayer".into(),
            chain: "ethereum".into(),
            tvl_usd: dec!(15_000_000_000),
            audit_status: AuditStatus::Audited {
                firms: vec!["Sigma Prime".into(), "Trail of Bits".into()],
                latest_date: "2024-04".into(),
            },
            risk_score: self.risk_score(),
            insurance_coverage: false,
        }
    }

    fn opportunities(&self) -> BoxFuture<Result<Vec<YieldOpportunity>>> {
        Box::pin(async move {
            let opps = EIGENLAYER_RATES
                .iter()
                .map(|(asset, apy)| YieldOpportunity {
                    protocol: "EigenLayer".into(),
                    asset: (*asset).to_string(),
                    apy: *apy,
                    apy_7d_avg: *apy * dec!(0.95),
                    risk_score: 6,
                    // Minimum restake: 0.001 stETH / rETH.
                    min_deposit: dec!(0.001),
                    // 7-day unbonding window.
                    lock_period_days: Some(7),
                    chain: "ethereum".into(),
                    strategy: YieldStrategy::Restaking,
                })
                .collect();
            Ok(opps)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn eigenlayer_apy_steth() {
        let p = EigenLayerProtocol::new();
        let apy = p.get_apy("stETH").await.unwrap();
        assert_eq!(apy, dec!(0.030));
    }

    #[tokio::test]
    async fn eigenlayer_apy_reth() {
        let p = EigenLayerProtocol::new();
        let apy = p.get_apy("rETH").await.unwrap();
        assert_eq!(apy, dec!(0.025));
    }

    #[tokio::test]
    async fn eigenlayer_unsupported_asset_errors() {
        let p = EigenLayerProtocol::new();
        assert!(p.get_apy("ETH").await.is_err());
        assert!(p.get_apy("USDC").await.is_err());
    }

    #[tokio::test]
    async fn eigenlayer_deposit_returns_tx() {
        let p = EigenLayerProtocol::new();
        let tx = p.deposit("stETH", dec!(2)).await.unwrap();
        assert!(tx.starts_with("0xeigen_restake_steth_"));
    }

    #[tokio::test]
    async fn eigenlayer_withdraw_returns_tx() {
        let p = EigenLayerProtocol::new();
        let tx = p.withdraw("rETH", dec!(1)).await.unwrap();
        assert!(tx.starts_with("0xeigen_undelegate_reth_"));
    }

    #[tokio::test]
    async fn eigenlayer_opportunities_have_lock_period() {
        let p = EigenLayerProtocol::new();
        let opps = p.opportunities().await.unwrap();
        assert_eq!(opps.len(), 2);
        assert!(opps.iter().all(|o| o.lock_period_days == Some(7)));
        assert!(opps.iter().all(|o| o.strategy == YieldStrategy::Restaking));
        assert!(opps.iter().all(|o| o.risk_score == 6));
    }

    #[test]
    fn eigenlayer_risk_score_higher_than_aave() {
        assert!(EigenLayerProtocol::new().risk_score() > 2);
    }
}
