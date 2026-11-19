//! 0x API client.
//!
//! 0x (Matcha) aggregates liquidity across Uniswap, Curve, Balancer, and other
//! EVM AMMs. It also supports gasless swaps via meta-transactions (Permit2).
//!
//! Phase 1: simulated quotes.
//!
//! Phase 2 endpoint:
//!   `GET https://api.0x.org/swap/v1/quote?sellToken=…&buyToken=…&sellAmount=…`
//!   Required header: `0x-api-key: <ZEROX_API_KEY>`

use anyhow::bail;
use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::gas::GasOracle;
use crate::providers::{BoxQuoteFuture, DexProvider};
use crate::types::{Chain, DexQuote, QuoteRequest, SwapHop};

/// 0x supports the major EVM chains.
const SUPPORTED_CHAINS: &[Chain] = &[
    Chain::Ethereum,
    Chain::Arbitrum,
    Chain::Base,
    Chain::Polygon,
    Chain::Bsc,
    Chain::Avalanche,
    Chain::Optimism,
];

/// 0x spread model: slightly more conservative than 1inch.
const ZEROX_SPREAD_ADVANTAGE: Decimal = dec!(0.0015);

pub struct ZeroXProvider {
    // Phase 2: store reqwest::Client and api_key here.
}

impl ZeroXProvider {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for ZeroXProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl DexProvider for ZeroXProvider {
    fn name(&self) -> &str {
        "0x"
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
                bail!("0x does not support chain '{chain}'");
            }
            if amount <= Decimal::ZERO {
                bail!("sellAmount must be positive");
            }

            // ── Phase 1: simulated quote ──────────────────────────────────────
            tracing::debug!(
                provider = "0x",
                chain = %chain,
                token_in = %token_in,
                token_out = %token_out,
                amount = %amount,
                "fetching simulated quote"
            );

            let gas_usd = GasOracle::estimate_gas_usd(chain);
            let gas_native = GasOracle::estimate_gas_native(chain);

            let effective_price = dec!(1) + ZEROX_SPREAD_ADVANTAGE;
            let amount_out = amount * effective_price;
            let price_impact = (amount / dec!(200_000)) * dec!(100);

            let ti_prefix = &token_in[..token_in.len().min(6)];
            let to_prefix = &token_out[..token_out.len().min(6)];

            // 0x typically routes through a Curve/Balancer split.
            let hops = vec![
                SwapHop {
                    dex: "curve_3pool".into(),
                    pool: format!("curve-{ti_prefix}-{to_prefix}"),
                    token_in: token_in.clone(),
                    token_out: token_out.clone(),
                    portion: dec!(70),
                },
                SwapHop {
                    dex: "balancer_v2".into(),
                    pool: format!("balancer-{ti_prefix}-{to_prefix}"),
                    token_in: token_in.clone(),
                    token_out: token_out.clone(),
                    portion: dec!(30),
                },
            ];

            Ok(DexQuote {
                provider: "0x".into(),
                chain,
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
    async fn zerox_quote_polygon() {
        let provider = ZeroXProvider::new();
        let req = QuoteRequest {
            chain: Chain::Polygon,
            token_in: "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174".into(), // USDC on Polygon
            token_out: "0x7ceB23fD6bC0adD59E62ac25578270cFf1b9f619".into(), // WETH on Polygon
            amount: dec!(500),
            slippage_bps: 50,
        };
        let quote = provider.get_quote(&req).await.unwrap();
        assert_eq!(quote.provider, "0x");
        assert!(quote.amount_out > Decimal::ZERO);
    }

    #[tokio::test]
    async fn zerox_rejects_solana() {
        let provider = ZeroXProvider::new();
        let req = QuoteRequest {
            chain: Chain::Solana,
            token_in: "USDC".into(),
            token_out: "SOL".into(),
            amount: dec!(100),
            slippage_bps: 50,
        };
        assert!(provider.get_quote(&req).await.is_err());
    }
}
