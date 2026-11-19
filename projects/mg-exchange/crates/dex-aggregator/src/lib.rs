//! DEX aggregator crate.
//!
//! Queries on-chain DEX liquidity (1inch, Jupiter, 0x) and compares it against
//! the CEX order book to determine optimal trade routing.
//!
//! # Phase 1 (current)
//! - Simulated quotes using realistic spread and gas models.
//! - No live API calls, no private key management.
//! - Smart order router compares simulated DEX prices with CEX depth.
//!
//! # Phase 2 (future)
//! - Live API calls to 1inch, Jupiter, and 0x.
//! - On-chain swap submission via ethers-rs / alloy / Anchor SDK.
//! - Real-time gas oracle integration.

pub mod aggregator;
pub mod execution;
pub mod gas;
pub mod providers;
pub mod router;
pub mod slippage;
pub mod types;

pub use aggregator::DexAggregator;
pub use execution::SwapExecutor;
pub use gas::GasOracle;
pub use router::{RouteDecision, SmartOrderRouter};
pub use slippage::{check_absolute_cap, check_price_impact, min_amount_out, validate_slippage_bps};
pub use types::{Chain, DexQuote, QuoteRequest, SwapHop, SwapRequest, SwapResult, SwapStatus};
