//! Core types for the DeFi yield integration crate.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Protocol metadata ─────────────────────────────────────────────────────────

/// Audit status for a DeFi protocol.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AuditStatus {
    /// Protocol has been audited by one or more security firms.
    Audited {
        /// Names of auditing firms (e.g. "OpenZeppelin", "Trail of Bits").
        firms: Vec<String>,
        /// Date of the most recent audit, ISO-8601 (e.g. "2024-11").
        latest_date: String,
    },
    /// Protocol has not undergone a formal third-party audit.
    Unaudited,
}

/// Static metadata describing a DeFi protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeFiProtocolInfo {
    /// Protocol name (e.g. "Aave v3").
    pub name: String,
    /// Primary chain of deployment (e.g. "ethereum").
    pub chain: String,
    /// Total value locked in USD at last update.
    pub tvl_usd: Decimal,
    /// Audit status.
    pub audit_status: AuditStatus,
    /// Composite risk score: 1 (safest) – 10 (highest risk).
    pub risk_score: u8,
    /// Whether the protocol is covered by on-chain insurance (e.g. Nexus Mutual).
    pub insurance_coverage: bool,
}

// ── Yield strategy ────────────────────────────────────────────────────────────

/// Category of yield-generating strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum YieldStrategy {
    /// Supply assets to a lending protocol (Aave, Compound).
    Lending,
    /// Stake ETH to receive a liquid staking token (Lido stETH).
    LiquidStaking,
    /// Restake liquid staking tokens for additional AVS rewards (EigenLayer).
    Restaking,
    /// Provide liquidity to an AMM and earn trading fees (Uniswap/Curve).
    LpFarming,
}

// ── Yield opportunity ─────────────────────────────────────────────────────────

/// A live yield opportunity on a specific protocol for a specific asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YieldOpportunity {
    /// Protocol name (e.g. "Aave v3").
    pub protocol: String,
    /// Asset symbol (e.g. "USDC", "ETH").
    pub asset: String,
    /// Current annualised percentage yield (e.g. `0.045` = 4.5%).
    pub apy: Decimal,
    /// 7-day average APY — used for stability comparison.
    pub apy_7d_avg: Decimal,
    /// Protocol risk score (1–10).
    pub risk_score: u8,
    /// Minimum deposit amount in asset units.
    pub min_deposit: Decimal,
    /// Lock-up period in days, or `None` for flexible/liquid positions.
    pub lock_period_days: Option<u32>,
    /// Chain where the opportunity exists (e.g. "ethereum").
    pub chain: String,
    /// Yield strategy category.
    pub strategy: YieldStrategy,
}

// ── Allocation ────────────────────────────────────────────────────────────────

/// Lifecycle status of a yield allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationStatus {
    /// Funds are actively deployed and earning yield.
    Active,
    /// Withdrawal requested; unbonding / settlement in progress.
    Withdrawing,
    /// Funds have been returned to the user's account.
    Withdrawn,
}

/// A single yield allocation for a user on a specific protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YieldAllocation {
    /// Unique allocation ID.
    pub id: Uuid,
    /// User account ID.
    pub user_id: Uuid,
    /// Protocol name.
    pub protocol: String,
    /// Asset deployed.
    pub asset: String,
    /// Principal amount deployed.
    pub amount: Decimal,
    /// Current APY at the time of last refresh.
    pub current_apy: Decimal,
    /// Accumulated yield earned since `deposited_at`.
    pub earned_yield: Decimal,
    /// When the deposit was made.
    pub deposited_at: DateTime<Utc>,
    /// Lifecycle status.
    pub status: AllocationStatus,
}

// ── Rebalancing ───────────────────────────────────────────────────────────────

/// An action produced by the allocator when rebalancing is recommended.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebalanceAction {
    /// Protocol to exit (withdraw from).
    pub from_protocol: String,
    /// Protocol to enter (deposit into).
    pub to_protocol: String,
    /// Asset to move.
    pub asset: String,
    /// Amount to move.
    pub amount: Decimal,
    /// Reason for the rebalance (e.g. "higher APY available").
    pub reason: String,
}

// ── Request / response helpers ────────────────────────────────────────────────

/// Request to deposit funds into a DeFi protocol via the allocator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepositRequest {
    /// User placing the deposit.
    pub user_id: Uuid,
    /// Asset to deposit.
    pub asset: String,
    /// Amount to deposit.
    pub amount: Decimal,
    /// Optional: preferred protocol. If `None`, allocator picks optimally.
    pub preferred_protocol: Option<String>,
}

/// Request to withdraw funds from a DeFi allocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithdrawRequest {
    /// User requesting withdrawal.
    pub user_id: Uuid,
    /// Allocation ID to withdraw from.
    pub allocation_id: Uuid,
    /// Amount to withdraw, or `None` for full withdrawal.
    pub amount: Option<Decimal>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn audit_status_audited_round_trips() {
        let status = AuditStatus::Audited {
            firms: vec!["OpenZeppelin".into(), "Trail of Bits".into()],
            latest_date: "2024-11".into(),
        };
        let json = serde_json::to_string(&status).unwrap();
        let back: AuditStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, back);
    }

    #[test]
    fn audit_status_unaudited_round_trips() {
        let status = AuditStatus::Unaudited;
        let json = serde_json::to_string(&status).unwrap();
        let back: AuditStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, back);
    }

    #[test]
    fn allocation_status_display() {
        let json = serde_json::to_string(&AllocationStatus::Active).unwrap();
        assert_eq!(json, "\"active\"");
        let json = serde_json::to_string(&AllocationStatus::Withdrawing).unwrap();
        assert_eq!(json, "\"withdrawing\"");
    }

    #[test]
    fn yield_opportunity_fields() {
        let opp = YieldOpportunity {
            protocol: "Aave v3".into(),
            asset: "USDC".into(),
            apy: dec!(0.045),
            apy_7d_avg: dec!(0.044),
            risk_score: 2,
            min_deposit: dec!(10),
            lock_period_days: None,
            chain: "ethereum".into(),
            strategy: YieldStrategy::Lending,
        };
        assert_eq!(opp.protocol, "Aave v3");
        assert!(opp.apy > dec!(0));
    }
}
