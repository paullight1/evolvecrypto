//! Compound v3 (Comet) integration.
//!
//! Phase 1: returns simulated APYs. No live RPC calls.
//!
//! Phase 2: fetch supply APY from the Compound v3 API:
//!   `GET https://api.compound.finance/api/v3/market?network=mainnet`
//!   Parse `supplyApy` for each base asset.

use anyhow::{bail, Result};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;

use crate::protocols::{BoxFuture, DeFiProtocol};
use crate::types::{AuditStatus, DeFiProtocolInfo, YieldOpportunity, YieldStrategy};

/// Assets supported by Compound v3 and their simulated supply APYs.
const COMPOUND_RATES: &[(&str, Decimal)] = &[
    ("USDC", dec!(0.038)), // 3.8%
    ("ETH",  dec!(0.018)), // 1.8%
];

/// Chains where Compound v3 is deployed.
const COMPOUND_CHAINS: &[&str] = &["ethereum", "base"];

pub struct CompoundProtocol;

impl CompoundProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CompoundProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl DeFiProtocol for CompoundProtocol {
    fn name(&self) -> &str {
        "Compound v3"
    }

    fn chain(&self) -> &str {
        "ethereum"
    }

    fn supported_assets(&self) -> Vec<String> {
        COMPOUND_RATES.iter().map(|(a, _)| (*a).to_string()).collect()
    }

    fn get_apy(&self, asset: &str) -> BoxFuture<Result<Decimal>> {
        let asset = asset.to_uppercase();
        Box::pin(async move {
            COMPOUND_RATES
                .iter()
                .find(|(a, _)| a.eq_ignore_ascii_case(&asset))
                .map(|(_, apy)| *apy)
                .ok_or_else(|| anyhow::anyhow!("Compound v3 does not support asset '{asset}'"))
        })
    }

    fn deposit(&self, asset: &str, amount: Decimal) -> BoxFuture<Result<String>> {
        let asset = asset.to_uppercase();
        let supports = self.supports_asset(&asset);
        Box::pin(async move {
            if !supports {
                bail!("Compound v3 does not support asset '{asset}'");
            }
            if amount <= Decimal::ZERO {
                bail!("deposit amount must be positive");
            }
            let tx_hash = format!("0xcompv3_deposit_{}_{}", asset.to_lowercase(), Uuid::new_v4().simple());
            tracing::info!(protocol = "Compound v3", asset = %asset, amount = %amount, tx = %tx_hash, "simulated deposit");
            Ok(tx_hash)
        })
    }

    fn withdraw(&self, asset: &str, amount: Decimal) -> BoxFuture<Result<String>> {
        let asset = asset.to_uppercase();
        let supports = self.supports_asset(&asset);
        Box::pin(async move {
            if !supports {
                bail!("Compound v3 does not support asset '{asset}'");
            }
            if amount <= Decimal::ZERO {
                bail!("withdrawal amount must be positive");
            }
            let tx_hash = format!("0xcompv3_withdraw_{}_{}", asset.to_lowercase(), Uuid::new_v4().simple());
            tracing::info!(protocol = "Compound v3", asset = %asset, amount = %amount, tx = %tx_hash, "simulated withdrawal");
            Ok(tx_hash)
        })
    }

    fn risk_score(&self) -> u8 {
        // Risk 2: same bracket as Aave — long-running, fully audited lending protocol.
        2
    }

    fn info(&self) -> DeFiProtocolInfo {
        DeFiProtocolInfo {
            name: "Compound v3".into(),
            chain: "ethereum".into(),
            tvl_usd: dec!(3_000_000_000),
            audit_status: AuditStatus::Audited {
                firms: vec!["OpenZeppelin".into(), "ChainSecurity".into()],
                latest_date: "2023-08".into(),
            },
            risk_score: self.risk_score(),
            insurance_coverage: false,
        }
    }

    fn opportunities(&self) -> BoxFuture<Result<Vec<YieldOpportunity>>> {
        Box::pin(async move {
            let opps = COMPOUND_RATES
                .iter()
                .flat_map(|(asset, apy)| {
                    COMPOUND_CHAINS.iter().map(move |chain| YieldOpportunity {
                        protocol: "Compound v3".into(),
                        asset: (*asset).to_string(),
                        apy: *apy,
                        apy_7d_avg: *apy * dec!(0.97),
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
    async fn compound_apy_usdc() {
        let p = CompoundProtocol::new();
        let apy = p.get_apy("USDC").await.unwrap();
        assert_eq!(apy, dec!(0.038));
    }

    #[tokio::test]
    async fn compound_unsupported_asset_errors() {
        let p = CompoundProtocol::new();
        assert!(p.get_apy("BTC").await.is_err());
    }

    #[tokio::test]
    async fn compound_deposit_returns_tx_hash() {
        let p = CompoundProtocol::new();
        let tx = p.deposit("USDC", dec!(1000)).await.unwrap();
        assert!(tx.starts_with("0xcompv3_deposit_usdc_"));
    }

    #[tokio::test]
    async fn compound_opportunities_count() {
        let p = CompoundProtocol::new();
        let opps = p.opportunities().await.unwrap();
        // 2 assets × 2 chains
        assert_eq!(opps.len(), 4);
    }
}
