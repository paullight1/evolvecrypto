//! DEX provider implementations.
//!
//! Each provider module implements the `DexProvider` trait, which exposes a
//! single `get_quote` async method. Phase 1 providers return simulated quotes
//! derived from realistic spread and gas models rather than making live API
//! calls, so the aggregator works without API keys in development.

pub mod jupiter;
pub mod oneinch;
pub mod zerox;

use std::future::Future;
use std::pin::Pin;

use anyhow::Result;

use crate::types::{Chain, DexQuote, QuoteRequest};

/// A boxed, heap-allocated future used as the return type of `DexProvider::get_quote`.
///
/// `Pin<Box<dyn Future<Output = Result<DexQuote>> + Send>>` is the standard
/// idiom for returning an async value from a dyn-compatible trait. This alias
/// keeps the signature readable.
pub type BoxQuoteFuture = Pin<Box<dyn Future<Output = Result<DexQuote>> + Send>>;

/// Trait implemented by every DEX liquidity provider.
///
/// `get_quote` returns a `BoxQuoteFuture` (a pinned, boxed future) rather than
/// an `impl Future` so that the trait remains dyn-compatible and providers can
/// be stored as `Vec<Box<dyn DexProvider>>`.
///
/// Provider implementations use `Box::pin(async move { … })` to create the future.
pub trait DexProvider: Send + Sync {
    /// Human-readable name used in logs and `DexQuote::provider`.
    fn name(&self) -> &str;

    /// Returns the set of chains this provider supports.
    fn supported_chains(&self) -> &[Chain];

    /// Fetch a swap quote for the given request.
    ///
    /// Returns `Err` if the chain is unsupported, the provider API is
    /// unreachable, or the response cannot be parsed.
    fn get_quote(&self, req: &QuoteRequest) -> BoxQuoteFuture;

    /// Returns `true` if this provider supports the given chain.
    fn supports_chain(&self, chain: Chain) -> bool {
        self.supported_chains().contains(&chain)
    }
}
