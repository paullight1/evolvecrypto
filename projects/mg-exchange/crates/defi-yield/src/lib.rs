//! DeFi yield integration crate.
//!
//! Provides yield sources backed by real DeFi protocols (Aave v3, Lido, Compound v3,
//! EigenLayer) for the exchange Earn product.
//!
//! # Phase 1 (current)
//! All protocol interactions are simulated. APYs are hard-coded to realistic values,
//! deposits/withdrawals return synthetic transaction hashes, and no on-chain or
//! external API calls are made. The exchange can run fully offline.
//!
//! # Phase 2 (future)
//! Replace simulated logic in each `protocols/` module with:
//! - Aave v3 / Compound v3: The Graph subgraph queries for live supply APY.
//! - Lido: `eth-api.lido.fi` REST endpoint for staking APR.
//! - EigenLayer: operator registry RPC queries for AVS reward rates.
//! - Actual on-chain transaction submission via alloy / ethers-rs.
//!
//! # Structure
//! - [`types`] — core data types shared across modules.
//! - [`protocols`] — `DeFiProtocol` trait + per-protocol implementations.
//! - [`allocator`] — `YieldAllocator`: best-yield selection and capital splitting.
//! - [`risk`] — `ProtocolRiskAssessor`: composite risk scoring.
//! - [`monitor`] — `YieldMonitor`: alert generation for anomalous conditions.

pub mod allocator;
pub mod monitor;
pub mod protocols;
pub mod risk;
pub mod types;

pub use allocator::YieldAllocator;
pub use monitor::{AlertSeverity, MonitorConfig, YieldAlert, YieldMonitor};
pub use protocols::DeFiProtocol;
pub use risk::{ProtocolRiskAssessor, RiskAssessment};
pub use types::{
    AllocationStatus, AuditStatus, DeFiProtocolInfo, DepositRequest, RebalanceAction,
    WithdrawRequest, YieldAllocation, YieldOpportunity, YieldStrategy,
};
