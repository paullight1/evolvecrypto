//! Aave v3 integration.
//!
//! Phase 1: returns simulated APYs derived from realistic Aave v3 market rates.
//! No live API calls are made.
//!
//! Phase 2: replace simulated APY with a call to the Aave v3 subgraph or RPC:
//!   `POST https://api.thegraph.com/subgraphs/name/aave/protocol-v3`
//!   Query `reserves { liquidityRate symbol }` — liquidityRate is in RAY (1e27).

use anyhow::{bail, Result};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::protocols::{BoxFuture, DeFiProtocol};
use crate::types::{AuditStatus, DeFiProtocolInfo, YieldOpportunity, YieldStrategy};
use uuid::Uuid;

/// Assets Aave v3 supports and their simulated APYs.
///
/// Rates are chosen to reflect realistic Aave v3 supply rates as of early 2025.
const AAVE_RATES: &[(&str, Decimal)] = &[
    ("USDC", dec!(0.045)),  // 4.5%
    ("USDT", dec!(0.042)),  // 4.2%
    ("DAI",  dec!(0.038)),  // 3.8%
    ("ETH",  dec!(0.021)),  // 2.1%
];

/// Chains where Aave v3 is deployed.
const AAVE_CHAINS: &[&str] = &["ethereum", "arbitrum"];

pub struct AaveProtocol;

impl AaveProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AaveProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl DeFiProtocol for AaveProtocol {
    fn name(&self) -> &str {
        "Aave v3"
    }

    fn chain(&self) -> &str {
        "ethereum"
    }

    fn supported_assets(&self) -> Vec<String> {
        AAVE_RATES.iter().map(|(asset, _)| (*asset).to_string()).collect()
    }

    fn get_apy(&self, asset: &str) -> BoxFuture<Result<Decimal>> {
        let asset = asset.to_uppercase();
        Box::pin(async move {
            AAVE_RATES
                .iter()
                .find(|(a, _)| a.eq_ignore_ascii_case(&asset))
                .map(|(_, apy)| *apy)
                .ok_or_else(|| anyhow::anyhow!("Aave v3 does not support asset '{asset}'"))
        })
    }

    fn deposit(&self, asset: &str, amount: Decimal) -> BoxFuture<Result<String>> {
        let asset = asset.to_uppercase();
        let supports = self.supports_asset(&asset);
        Box::pin(async move {
            if !supports {
                bail!("Aave v3 does not support asset '{asset}'");
            }
            if amount <= Decimal::ZERO {
                bail!("deposit amount must be positive");
            }
            // Phase 1: synthetic tx hash.
            let tx_hash = format!("0xaave3_deposit_{}_{}", asset.to_lowercase(), Uuid::new_v4().simple());
            tracing::info!(protocol = "Aave v3", asset = %asset, amount = %amount, tx = %tx_hash, "simulated deposit");
            Ok(tx_hash)
        })
    }

    fn withdraw(&self, asset: &str, amount: Decimal) -> BoxFuture<Result<String>> {
        let asset = asset.to_uppercase();
        let supports = self.supports_asset(&asset);
        Box::pin(async move {
            if !supports {
                bail!("Aave v3 does not support asset '{asset}'");
            }
            if amount <= Decimal::ZERO {
                bail!("withdrawal amount must be positive");
            }
            let tx_hash = format!("0xaave3_withdraw_{}_{}", asset.to_lowercase(), Uuid::new_v4().simple());
            tracing::info!(protocol = "Aave v3", asset = %asset, amount = %amount, tx = %tx_hash, "simulated withdrawal");
            Ok(tx_hash)
        })
    }

    fn risk_score(&self) -> u8 {
        // Risk 2: battle-tested lending protocol audited by OpenZeppelin and Trail of Bits.
        2
    }

    fn info(&self) -> DeFiProtocolInfo {
        DeFiProtocolInfo {
            name: "Aave v3".into(),
            chain: "ethereum".into(),
            // TVL sourced from DeFiLlama; approximate as of 2025-Q1.
            tvl_usd: dec!(12_000_000_000),
            audit_status: AuditStatus::Audited {
                firms: vec!["OpenZeppelin".into(), "Trail of Bits".into(), "SigmaPrime".into()],
                latest_date: "2023-09".into(),
            },
            risk_score: self.risk_score(),
            insurance_coverage: true,
        }
    }

    fn opportunities(&self) -> BoxFuture<Result<Vec<YieldOpportunity>>> {
        Box::pin(async move {
            let opps = AAVE_RATES
                .iter()
                .flat_map(|(asset, apy)| {
                    // Expose the opportunity on every supported chain.
                    AAVE_CHAINS.iter().map(move |chain| YieldOpportunity {
                        protocol: "Aave v3".into(),
                        asset: (*asset).to_string(),
                        apy: *apy,
                        // Assume 7-day average is slightly below the spot rate.
                        apy_7d_avg: *apy * dec!(0.98),
                        risk_score: 2,
                        min_deposit: dec!(10),
                        lock_period_days: None,
                        chain: (*chain).to_string(),
                        strategy: YieldStrategy::Lending,
                    })
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
    async fn aave_apy_usdc() {
        let p = AaveProtocol::new();
        let apy = p.get_apy("USDC").await.unwrap();
        assert_eq!(apy, dec!(0.045));
    }

    #[tokio::test]
    async fn aave_apy_case_insensitive() {
        let p = AaveProtocol::new();
        let apy = p.get_apy("usdc").await.unwrap();
        assert_eq!(apy, dec!(0.045));
    }

    #[tokio::test]
    async fn aave_unsupported_asset_errors() {
        let p = AaveProtocol::new();
        assert!(p.get_apy("SOL").await.is_err());
    }

    #[tokio::test]
    async fn aave_deposit_returns_tx_hash() {
        let p = AaveProtocol::new();
        let tx = p.deposit("USDT", dec!(500)).await.unwrap();
        assert!(tx.starts_with("0xaave3_deposit_usdt_"));
    }

    #[tokio::test]
    async fn aave_deposit_zero_amount_errors() {
        let p = AaveProtocol::new();
        assert!(p.deposit("USDC", Decimal::ZERO).await.is_err());
    }

    #[tokio::test]
    async fn aave_withdraw_returns_tx_hash() {
        let p = AaveProtocol::new();
        let tx = p.withdraw("DAI", dec!(100)).await.unwrap();
        assert!(tx.starts_with("0xaave3_withdraw_dai_"));
    }

    #[tokio::test]
    async fn aave_opportunities_count() {
        let p = AaveProtocol::new();
        let opps = p.opportunities().await.unwrap();
        // 4 assets × 2 chains
        assert_eq!(opps.len(), 8);
        assert!(opps.iter().all(|o| o.risk_score == 2));
        assert!(opps.iter().all(|o| o.strategy == YieldStrategy::Lending));
    }

    #[test]
    fn aave_risk_score() {
        assert_eq!(AaveProtocol::new().risk_score(), 2);
    }

    #[test]
    fn aave_supports_asset() {
        let p = AaveProtocol::new();
        assert!(p.supports_asset("USDC"));
        assert!(p.supports_asset("usdt"));
        assert!(!p.supports_asset("SOL"));
    }
}
