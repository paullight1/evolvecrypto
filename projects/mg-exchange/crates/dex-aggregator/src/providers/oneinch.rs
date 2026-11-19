//! 1inch Fusion API client.
//!
//! Phase 1: returns simulated quotes modelled on realistic 1inch behaviour
//! (competitive pricing, multi-hop routes, higher gas on mainnet).
//!
//! Phase 2: uncomment the `reqwest` call to hit the live endpoint:
//!   `GET https://api.1inch.dev/swap/v6.0/{chainId}/quote`
//!
//! Required header: `Authorization: Bearer <ONEINCH_API_KEY>`.

use anyhow::bail;
use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::gas::GasOracle;
use crate::providers::{BoxQuoteFuture, DexProvider};
use crate::types::{Chain, DexQuote, QuoteRequest, SwapHop};

/// EVM-compatible chains that 1inch supports.
const SUPPORTED_CHAINS: &[Chain] = &[
    Chain::Ethereum,
    Chain::Arbitrum,
    Chain::Base,
    Chain::Polygon,
    Chain::Bsc,
    Chain::Avalanche,
    Chain::Optimism,
];

/// 1inch spread model: typically 0.2% better than raw AMM price due to
/// Pathfinder routing across many liquidity sources.
const ONEINCH_SPREAD_ADVANTAGE: Decimal = dec!(0.002);

pub struct OneInchProvider {
    // Phase 2: store reqwest::Client and api_key here.
}

impl OneInchProvider {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for OneInchProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl DexProvider for OneInchProvider {
    fn name(&self) -> &str {
        "1inch"
    }

    fn supported_chains(&self) -> &[Chain] {
        SUPPORTED_CHAINS
    }

    fn get_quote(&self, req: &QuoteRequest) -> BoxQuoteFuture {
        // Clone what we need before moving into the future.
        let chain = req.chain;
        let token_in = req.token_in.clone();
        let token_out = req.token_out.clone();
        let amount = req.amount;
        let slippage_bps = req.slippage_bps;
        let supports = self.supports_chain(chain);

        Box::pin(async move {
            if !supports {
                bail!(
                    "1inch does not support chain '{chain}' — use Jupiter for Solana"
                );
            }
            if amount <= Decimal::ZERO {
                bail!("amount_in must be positive");
            }

            // ── Phase 1: simulated quote ──────────────────────────────────────
            tracing::debug!(
                provider = "1inch",
                chain = %chain,
                token_in = %token_in,
                token_out = %token_out,
                amount = %amount,
                "fetching simulated quote"
            );

            let gas_usd = GasOracle::estimate_gas_usd(chain);
            let gas_native = GasOracle::estimate_gas_native(chain);

            let effective_price = dec!(1) + ONEINCH_SPREAD_ADVANTAGE;
            let amount_out = amount * effective_price;
            let price_impact = (amount / dec!(100_000)) * dec!(100);

            let pool_label = format!(
                "0x{}-pool-{}",
                &token_in[..token_in.len().min(8)],
                &token_out[..token_out.len().min(8)]
            );
            let hop = SwapHop {
                dex: "uniswap_v3".into(),
                pool: pool_label,
                token_in: token_in.clone(),
                token_out: token_out.clone(),
                portion: dec!(100),
            };

            Ok(DexQuote {
                provider: "1inch".into(),
                chain,
                token_in,
                token_out,
                amount_in: amount,
                amount_out,
                price: effective_price,
                price_impact,
                gas_estimate: gas_native,
                gas_usd,
                route: vec![hop],
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
    async fn oneinch_quote_evm_chain() {
        let provider = OneInchProvider::new();
        let req = QuoteRequest {
            chain: Chain::Arbitrum,
            token_in: "0xUSDC".into(),
            token_out: "0xWETH".into(),
            amount: dec!(1000),
            slippage_bps: 50,
        };
        let quote = provider.get_quote(&req).await.unwrap();
        assert_eq!(quote.provider, "1inch");
        assert!(quote.amount_out > Decimal::ZERO);
        assert!(quote.is_valid());
    }

    #[tokio::test]
    async fn oneinch_rejects_solana() {
        let provider = OneInchProvider::new();
        let req = QuoteRequest {
            chain: Chain::Solana,
            token_in: "USDC".into(),
            token_out: "SOL".into(),
            amount: dec!(100),
            slippage_bps: 50,
        };
        let result = provider.get_quote(&req).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Jupiter"));
    }

    #[tokio::test]
    async fn oneinch_rejects_zero_amount() {
        let provider = OneInchProvider::new();
        let req = QuoteRequest {
            chain: Chain::Ethereum,
            token_in: "0xUSDC".into(),
            token_out: "0xWETH".into(),
            amount: Decimal::ZERO,
            slippage_bps: 50,
        };
        assert!(provider.get_quote(&req).await.is_err());
    }
}
