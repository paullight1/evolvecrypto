//! Jupiter (Solana) API client.
//!
//! Jupiter is the dominant aggregator on Solana and uses mint addresses
//! (base-58 encoded public keys) instead of EVM contract addresses.
//!
//! Phase 1: simulated quotes.
//!
//! Phase 2 endpoint:
//!   `GET https://quote-api.jup.ag/v6/quote?inputMint=…&outputMint=…&amount=…&slippageBps=…`

use anyhow::bail;
use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::gas::GasOracle;
use crate::providers::{BoxQuoteFuture, DexProvider};
use crate::types::{Chain, DexQuote, QuoteRequest, SwapHop};

/// Jupiter only operates on Solana.
const SUPPORTED_CHAINS: &[Chain] = &[Chain::Solana];

/// Jupiter's routing typically yields very tight spreads on Solana AMMs.
const JUPITER_SPREAD_ADVANTAGE: Decimal = dec!(0.001);

pub struct JupiterProvider {
    // Phase 2: store reqwest::Client here.
}

impl JupiterProvider {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for JupiterProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl DexProvider for JupiterProvider {
    fn name(&self) -> &str {
        "jupiter"
    }

    fn supported_chains(&self) -> &[Chain] {
        SUPPORTED_CHAINS
    }

    fn get_quote(&self, req: &QuoteRequest) -> BoxQuoteFuture {
        let chain = req.chain;
        let token_in = req.token_in.clone();
        let token_out = req.token_out.clone();
        let amount = req.amount;
        let slippage_bps = req.slippage_bps;
        let supports = self.supports_chain(chain);

        Box::pin(async move {
            if !supports {
                bail!("Jupiter only supports Solana; received chain '{chain}'");
            }
            if amount <= Decimal::ZERO {
                bail!("amount must be positive");
            }

            // ── Phase 1: simulated quote ──────────────────────────────────────
            tracing::debug!(
                provider = "jupiter",
                token_in = %token_in,
                token_out = %token_out,
                amount = %amount,
                "fetching simulated Solana quote"
            );

            let gas_usd = GasOracle::estimate_gas_usd(Chain::Solana);
            let gas_native = GasOracle::estimate_gas_native(Chain::Solana);

            let effective_price = dec!(1) + JUPITER_SPREAD_ADVANTAGE;
            let amount_out = amount * effective_price;
            let price_impact = (amount / dec!(1_000_000)) * dec!(100);

            let ti_prefix = &token_in[..token_in.len().min(8)];
            let to_prefix = &token_out[..token_out.len().min(8)];

            // Jupiter often routes through Orca, Raydium, or Meteora.
            let hops = vec![
                SwapHop {
                    dex: "orca_whirlpool".into(),
                    pool: format!("whirlpool-{ti_prefix}-{to_prefix}"),
                    token_in: token_in.clone(),
                    token_out: token_out.clone(),
                    portion: dec!(60),
                },
                SwapHop {
                    dex: "raydium_clmm".into(),
                    pool: format!("raydium-{ti_prefix}-{to_prefix}"),
                    token_in: token_in.clone(),
                    token_out: token_out.clone(),
                    portion: dec!(40),
                },
            ];

            Ok(DexQuote {
                provider: "jupiter".into(),
                chain: Chain::Solana,
                token_in,
                token_out,
                amount_in: amount,
                amount_out,
                price: effective_price,
                price_impact,
                gas_estimate: gas_native,
                gas_usd,
                route: hops,
                expires_at: Utc::now() + chrono::Duration::seconds(30),
                slippage_bps,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn jupiter_quote_solana() {
        let provider = JupiterProvider::new();
        let req = QuoteRequest {
            chain: Chain::Solana,
            token_in: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(), // USDC mint
            token_out: "So11111111111111111111111111111111111111112".into(),   // SOL mint
            amount: dec!(500),
            slippage_bps: 30,
        };
        let quote = provider.get_quote(&req).await.unwrap();
        assert_eq!(quote.provider, "jupiter");
        assert_eq!(quote.chain, Chain::Solana);
        assert!(quote.amount_out > Decimal::ZERO);
        // Jupiter splits across multiple pools
        assert!(quote.route.len() > 1);
    }

    #[tokio::test]
    async fn jupiter_rejects_evm_chain() {
        let provider = JupiterProvider::new();
        let req = QuoteRequest {
            chain: Chain::Ethereum,
            token_in: "0xUSDC".into(),
            token_out: "0xWETH".into(),
            amount: dec!(100),
            slippage_bps: 50,
        };
        assert!(provider.get_quote(&req).await.is_err());
    }
}
