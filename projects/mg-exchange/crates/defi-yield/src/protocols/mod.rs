//! DeFi protocol trait and protocol implementations.
//!
//! Each module implements `DeFiProtocol` for a specific yield source.
//! Phase 1: all implementations are simulated — no live on-chain calls are made.
//! Phase 2: replace simulated logic with real API/RPC calls inside each module.

pub mod aave;
pub mod compound;
pub mod eigenlayer;
pub mod lido;

use std::future::Future;
use std::pin::Pin;

use anyhow::Result;
use rust_decimal::Decimal;

use crate::types::{DeFiProtocolInfo, YieldOpportunity};

/// Boxed async future used as the return type for `DeFiProtocol` methods.
///
/// The `Pin<Box<dyn Future<...> + Send>>` idiom keeps the trait dyn-compatible
/// so protocols can be stored as `Vec<Box<dyn DeFiProtocol>>`.
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Trait implemented by every supported DeFi yield protocol.
///
/// All `async` methods return `BoxFuture` to remain object-safe.
/// Implementors use `Box::pin(async move { … })` to wrap the body.
pub trait DeFiProtocol: Send + Sync {
    /// Human-readable name (e.g. "Aave v3", "Lido").
    fn name(&self) -> &str;

    /// Primary deployment chain (e.g. "ethereum").
    fn chain(&self) -> &str;

    /// Assets this protocol accepts (e.g. `["USDC", "USDT", "DAI"]`).
    fn supported_assets(&self) -> Vec<String>;

    /// Current annualised yield for `asset` expressed as a fraction
    /// (e.g. `0.045` represents 4.5%).
    ///
    /// Returns `Err` if `asset` is not supported by this protocol.
    fn get_apy(&self, asset: &str) -> BoxFuture<Result<Decimal>>;

    /// Simulate depositing `amount` of `asset` into the protocol.
    ///
    /// Returns a synthetic transaction hash.
    /// Phase 2: broadcasts a real on-chain transaction.
    fn deposit(&self, asset: &str, amount: Decimal) -> BoxFuture<Result<String>>;

    /// Simulate withdrawing `amount` of `asset` from the protocol.
    ///
    /// Returns a synthetic transaction hash.
    /// Phase 2: broadcasts a real on-chain withdrawal.
    fn withdraw(&self, asset: &str, amount: Decimal) -> BoxFuture<Result<String>>;

    /// Protocol risk score: 1 (safest) – 10 (highest risk).
    fn risk_score(&self) -> u8;

    /// Static protocol metadata (TVL, audit status, insurance coverage, etc.).
    fn info(&self) -> DeFiProtocolInfo;

    /// All current yield opportunities offered by this protocol.
    fn opportunities(&self) -> BoxFuture<Result<Vec<YieldOpportunity>>>;

    /// Returns `true` if this protocol accepts the given `asset`.
    fn supports_asset(&self, asset: &str) -> bool {
        self.supported_assets().iter().any(|a| a.eq_ignore_ascii_case(asset))
    }
}
