//! DEX aggregator — fans out quote requests to all capable providers and
//! selects the best available quote.

use anyhow::{anyhow, Result};
use rust_decimal::Decimal;
use tracing::warn;

use crate::gas::GasOracle;
use crate::providers::DexProvider;
use crate::types::{Chain, DexQuote, QuoteRequest};

/// Queries multiple DEX providers in parallel and returns the best quote.
///
/// "Best" is defined as highest `amount_out` (most tokens received for a given
/// `amount_in`), which already accounts for protocol fees because providers
/// include their fee in `amount_out`.
///
/// Gas cost is NOT subtracted from `amount_out` at this layer — the smart
/// order router (`router.rs`) applies gas-adjusted comparison when deciding
/// whether to route to a DEX or the CEX order book.
pub struct DexAggregator {
    providers: Vec<Box<dyn DexProvider>>,
    gas_oracle: GasOracle,
}

impl DexAggregator {
    /// Constructs an aggregator with the default provider set:
    /// 1inch, Jupiter, and 0x.
    pub fn new() -> Self {
        use crate::providers::{
            jupiter::JupiterProvider, oneinch::OneInchProvider, zerox::ZeroXProvider,
        };

        Self {
            providers: vec![
                Box::new(OneInchProvider::new()),
                Box::new(JupiterProvider::new()),
                Box::new(ZeroXProvider::new()),
            ],
            gas_oracle: GasOracle,
        }
    }

    /// Constructs an aggregator with a custom provider list.
    ///
    /// Useful in tests where you want to inject a subset of providers.
    pub fn with_providers(providers: Vec<Box<dyn DexProvider>>) -> Self {
        Self {
            providers,
            gas_oracle: GasOracle,
        }
    }

    /// Returns quotes from every provider that supports `chain`.
    ///
    /// Requests are driven sequentially in Phase 1 (providers return instantly
    /// with simulated data). Phase 2 should spawn concurrent tokio tasks —
    /// the `BoxQuoteFuture` return type makes this straightforward with
    /// `tokio::task::JoinSet` once providers are wrapped in `Arc`.
    ///
    /// Each element in the returned `Vec` corresponds to one capable provider.
    /// Failures are preserved as `Err` so the caller can inspect partial results.
    pub async fn get_all_quotes(
        &self,
        chain: Chain,
        token_in: &str,
        token_out: &str,
        amount: Decimal,
        slippage_bps: u16,
    ) -> Vec<Result<DexQuote>> {
        let mut results = Vec::new();

        for provider in &self.providers {
            if !provider.supports_chain(chain) {
                continue;
            }

            let req = QuoteRequest {
                chain,
                token_in: token_in.to_string(),
                token_out: token_out.to_string(),
                amount,
                slippage_bps,
            };

            let result = provider.get_quote(&req).await;
            if let Err(ref e) = result {
                warn!(
                    provider = provider.name(),
                    chain = %chain,
                    error = %e,
                    "provider quote failed"
                );
            }
            results.push(result);
        }

        results
    }

    /// Returns the single best quote across all capable providers.
    ///
    /// "Best" = highest `amount_out`. Returns `Err` if no provider supports
    /// the chain or all providers return errors.
    pub async fn get_best_quote(
        &self,
        chain: Chain,
        token_in: &str,
        token_out: &str,
        amount: Decimal,
        slippage_bps: u16,
    ) -> Result<DexQuote> {
        let all = self
            .get_all_quotes(chain, token_in, token_out, amount, slippage_bps)
            .await;

        let best = all
            .into_iter()
            .filter_map(|r| r.ok())
            .max_by(|a, b| a.amount_out.cmp(&b.amount_out));

        best.ok_or_else(|| {
            anyhow!(
                "no DEX provider returned a valid quote for '{token_in}' on chain {chain}"
            )
        })
    }

    /// Returns the gas oracle for use by the smart order router.
    pub fn gas_oracle(&self) -> &GasOracle {
        &self.gas_oracle
    }

    /// Lists the names of all providers registered with this aggregator.
    pub fn provider_names(&self) -> Vec<&str> {
        self.providers.iter().map(|p| p.name()).collect()
    }
}

impl Default for DexAggregator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[tokio::test]
    async fn best_quote_ethereum_returns_winner() {
        let agg = DexAggregator::new();
        let quote = agg
            .get_best_quote(Chain::Ethereum, "0xUSDC", "0xWETH", dec!(10_000), 50)
            .await
            .unwrap();
        assert!(quote.amount_out > Decimal::ZERO);
        // 1inch and 0x both support Ethereum; best quote comes from one of them.
        assert!(quote.provider == "1inch" || quote.provider == "0x");
    }

    #[tokio::test]
    async fn best_quote_solana_uses_jupiter() {
        let agg = DexAggregator::new();
        let quote = agg
            .get_best_quote(
                Chain::Solana,
                "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                "So11111111111111111111111111111111111111112",
                dec!(1000),
                30,
            )
            .await
            .unwrap();
        assert_eq!(quote.provider, "jupiter");
    }

    #[tokio::test]
    async fn all_quotes_ethereum_returns_two_providers() {
        let agg = DexAggregator::new();
        let quotes = agg
            .get_all_quotes(Chain::Ethereum, "0xA", "0xB", dec!(500), 50)
            .await;

        // Ethereum is supported by 1inch and 0x (not Jupiter).
        let successes: Vec<_> = quotes.into_iter().filter_map(|r| r.ok()).collect();
        assert_eq!(successes.len(), 2);
    }

    #[tokio::test]
    async fn no_provider_for_chain_returns_err() {
        // Only EVM providers; query Solana → all skip → no results → Err.
        use crate::providers::{oneinch::OneInchProvider, zerox::ZeroXProvider};
        let agg = DexAggregator::with_providers(vec![
            Box::new(OneInchProvider::new()),
            Box::new(ZeroXProvider::new()),
        ]);
        let result = agg
            .get_best_quote(Chain::Solana, "USDC", "SOL", dec!(100), 50)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn provider_names_contains_all_defaults() {
        let agg = DexAggregator::new();
        let names = agg.provider_names();
        assert!(names.contains(&"1inch"));
        assert!(names.contains(&"jupiter"));
        assert!(names.contains(&"0x"));
    }

    #[tokio::test]
    async fn oneinch_beats_zerox_on_ethereum() {
        // 1inch has a 0.2% advantage vs 0x's 0.15% on the same amount.
        // For equal amounts, 1inch amount_out > 0x amount_out.
        let agg = DexAggregator::new();
        let all = agg
            .get_all_quotes(Chain::Ethereum, "USDC", "WETH", dec!(1000), 50)
            .await;
        let quotes: Vec<_> = all.into_iter().filter_map(|r| r.ok()).collect();
        assert_eq!(quotes.len(), 2);
        let oneinch = quotes.iter().find(|q| q.provider == "1inch").unwrap();
        let zerox = quotes.iter().find(|q| q.provider == "0x").unwrap();
        assert!(oneinch.amount_out > zerox.amount_out);
    }
}
