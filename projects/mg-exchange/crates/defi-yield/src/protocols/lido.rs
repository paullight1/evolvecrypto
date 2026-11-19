//! Lido stETH integration.
//!
//! Phase 1: returns a simulated staking APY. No live RPC calls are made.
//!
//! Phase 2: replace simulated APY with the Lido APR API:
//!   `GET https://eth-api.lido.fi/v1/protocol/steth/apr/last`
//!
//! Lido-specific detail: depositing ETH mints stETH at approximately 1:1.
//! The stETH/ETH ratio (rebasing mechanism) is tracked here for Phase 2 accuracy.

use anyhow::{bail, Result};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;

use crate::protocols::{BoxFuture, DeFiProtocol};
use crate::types::{AuditStatus, DeFiProtocolInfo, YieldOpportunity, YieldStrategy};

/// Simulated Lido stETH APY (annualised).
/// Sourced from Lido historical data (early 2025: ~3.5%).
const LIDO_ETH_APY: Decimal = dec!(0.035);

/// Phase 1: simulated stETH/ETH exchange ratio (slightly below 1 due to fees).
const STETH_ETH_RATIO: Decimal = dec!(0.9998);

pub struct LidoProtocol;

impl LidoProtocol {
    pub fn new() -> Self {
        Self
    }

    /// Returns the simulated stETH/ETH ratio.
    ///
    /// Phase 2: fetch from `eth_call` on the stETH contract
    /// (`getPooledEthByShares(1e18)` / 1e18).
    pub fn steth_eth_ratio(&self) -> Decimal {
        STETH_ETH_RATIO
    }
}

impl Default for LidoProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl DeFiProtocol for LidoProtocol {
    fn name(&self) -> &str {
        "Lido"
    }

    fn chain(&self) -> &str {
        "ethereum"
    }

    fn supported_assets(&self) -> Vec<String> {
        // Lido accepts ETH and returns stETH.
        vec!["ETH".into()]
    }

    fn get_apy(&self, asset: &str) -> BoxFuture<Result<Decimal>> {
        let asset = asset.to_uppercase();
        Box::pin(async move {
            if asset != "ETH" {
                bail!("Lido only supports ETH staking; received '{asset}'");
            }
            Ok(LIDO_ETH_APY)
        })
    }

    fn deposit(&self, asset: &str, amount: Decimal) -> BoxFuture<Result<String>> {
        let asset = asset.to_uppercase();
        Box::pin(async move {
            if asset != "ETH" {
                bail!("Lido only supports ETH staking; received '{asset}'");
            }
            if amount <= Decimal::ZERO {
                bail!("deposit amount must be positive");
            }
            let steth_received = amount * STETH_ETH_RATIO;
            let tx_hash = format!("0xlido_stake_eth_{}", Uuid::new_v4().simple());
            tracing::info!(
                protocol = "Lido",
                eth_deposited = %amount,
                steth_received = %steth_received,
                tx = %tx_hash,
                "simulated ETH stake"
            );
            Ok(tx_hash)
        })
    }

    fn withdraw(&self, asset: &str, amount: Decimal) -> BoxFuture<Result<String>> {
        let asset = asset.to_uppercase();
        Box::pin(async move {
            if asset != "ETH" {
                bail!("Lido only supports ETH; received '{asset}'");
            }
            if amount <= Decimal::ZERO {
                bail!("withdrawal amount must be positive");
            }
            // Lido withdrawals go through the withdrawal queue (days).
            let tx_hash = format!("0xlido_withdraw_steth_{}", Uuid::new_v4().simple());
            tracing::info!(
                protocol = "Lido",
                amount = %amount,
                tx = %tx_hash,
                "simulated stETH withdrawal queued"
            );
            Ok(tx_hash)
        })
    }

    fn risk_score(&self) -> u8 {
        // Risk 3: liquid staking has stETH/ETH depeg risk and smart-contract risk.
        // Lower than EigenLayer since Lido is the most battle-tested LST protocol.
        3
    }

    fn info(&self) -> DeFiProtocolInfo {
        DeFiProtocolInfo {
            name: "Lido".into(),
            chain: "ethereum".into(),
            tvl_usd: dec!(22_000_000_000),
            audit_status: AuditStatus::Audited {
                firms: vec!["MixBytes".into(), "Sigma Prime".into(), "Quantstamp".into()],
                latest_date: "2024-02".into(),
            },
            risk_score: self.risk_score(),
            insurance_coverage: false,
        }
    }

    fn opportunities(&self) -> BoxFuture<Result<Vec<YieldOpportunity>>> {
        Box::pin(async move {
            Ok(vec![YieldOpportunity {
                protocol: "Lido".into(),
                asset: "ETH".into(),
                apy: LIDO_ETH_APY,
                apy_7d_avg: LIDO_ETH_APY * dec!(0.99),
                risk_score: 3,
                min_deposit: dec!(0.01),
                lock_period_days: None, // stETH is liquid; withdrawal queue ~1-7 days
                chain: "ethereum".into(),
                strategy: YieldStrategy::LiquidStaking,
            }])
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lido_apy_eth() {
        let p = LidoProtocol::new();
        let apy = p.get_apy("ETH").await.unwrap();
        assert_eq!(apy, dec!(0.035));
    }

    #[tokio::test]
    async fn lido_rejects_non_eth() {
        let p = LidoProtocol::new();
        let err = p.get_apy("USDC").await.unwrap_err();
        assert!(err.to_string().contains("only supports ETH"));
    }

    #[tokio::test]
    async fn lido_deposit_returns_tx_hash() {
        let p = LidoProtocol::new();
        let tx = p.deposit("ETH", dec!(1)).await.unwrap();
        assert!(tx.starts_with("0xlido_stake_eth_"));
    }

    #[tokio::test]
    async fn lido_deposit_zero_errors() {
        let p = LidoProtocol::new();
        assert!(p.deposit("ETH", Decimal::ZERO).await.is_err());
    }

    #[tokio::test]
    async fn lido_withdraw_returns_tx_hash() {
        let p = LidoProtocol::new();
        let tx = p.withdraw("ETH", dec!(0.5)).await.unwrap();
        assert!(tx.starts_with("0xlido_withdraw_steth_"));
    }

    #[tokio::test]
    async fn lido_opportunities_single_entry() {
        let p = LidoProtocol::new();
        let opps = p.opportunities().await.unwrap();
        assert_eq!(opps.len(), 1);
        assert_eq!(opps[0].asset, "ETH");
        assert_eq!(opps[0].strategy, YieldStrategy::LiquidStaking);
    }

    #[test]
    fn lido_steth_ratio_below_one() {
        let p = LidoProtocol::new();
        assert!(p.steth_eth_ratio() < dec!(1));
        assert!(p.steth_eth_ratio() > dec!(0.99));
    }

    #[test]
    fn lido_risk_score() {
        assert_eq!(LidoProtocol::new().risk_score(), 3);
    }
}
