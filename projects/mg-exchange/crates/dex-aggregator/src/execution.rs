//! Swap execution layer.
//!
//! Phase 1: validates the quote and returns a `SwapResult` with status
//! `Simulated` — no on-chain transaction is broadcast.
//!
//! Phase 2: this module would:
//!   1. Build the calldata from the provider's quote response.
//!   2. Sign and submit the transaction via ethers-rs / alloy / Anchor SDK.
//!   3. Poll for receipt and update the status to `Confirmed` or `Failed`.

use anyhow::{bail, Result};
use chrono::Utc;
use tracing::info;
use uuid::Uuid;

use crate::slippage::{check_absolute_cap, check_price_impact, min_amount_out};
use crate::types::{SwapRequest, SwapResult, SwapStatus};

/// Executes (or simulates) a swap against a previously fetched `DexQuote`.
pub struct SwapExecutor;

impl SwapExecutor {
    pub fn new() -> Self {
        Self
    }

    /// Execute a swap.
    ///
    /// Phase 1 behaviour:
    /// - Validates the quote is still within its validity window.
    /// - Checks slippage constraints via `slippage.rs`.
    /// - Returns `SwapStatus::Simulated` with a synthetic transaction hash.
    ///
    /// Returns `Err` if the quote has expired or slippage checks fail.
    pub async fn execute(&self, req: SwapRequest) -> Result<SwapResult> {
        let quote = &req.quote;

        // 1. Quote expiry check.
        if !quote.is_valid() {
            bail!(
                "quote from {} expired at {} — please fetch a fresh quote",
                quote.provider,
                quote.expires_at
            );
        }

        // 2. Slippage protection — absolute cap first, then tolerance check.
        check_absolute_cap(quote)?;
        check_price_impact(quote)?;

        // 3. Compute minimum acceptable output (for on-chain tx in Phase 2).
        let min_out = min_amount_out(quote.amount_out, quote.slippage_bps);

        info!(
            provider = %quote.provider,
            chain = %quote.chain,
            amount_in = %quote.amount_in,
            amount_out = %quote.amount_out,
            min_out = %min_out,
            sender = %req.sender,
            "executing swap (Phase 1 — simulated)"
        );

        // 4. Phase 1: synthesise a plausible transaction hash.
        let synthetic_hash = format!("0xsim-{}", Uuid::new_v4().as_simple());

        Ok(SwapResult {
            tx_hash: Some(synthetic_hash),
            amount_in: quote.amount_in,
            amount_out: quote.amount_out,
            gas_used: quote.gas_estimate,
            status: SwapStatus::Simulated,
        })
    }
}

impl Default for SwapExecutor {
    fn default() -> Self {
        Self::new()
    }
}

/// Formats a `SwapResult` for display in API responses.
pub fn format_swap_result(result: &SwapResult) -> serde_json::Value {
    serde_json::json!({
        "tx_hash": result.tx_hash,
        "amount_in": result.amount_in,
        "amount_out": result.amount_out,
        "gas_used": result.gas_used,
        "status": match &result.status {
            SwapStatus::Simulated => "simulated",
            SwapStatus::Pending => "pending",
            SwapStatus::Confirmed => "confirmed",
            SwapStatus::Failed(_) => "failed",
        },
        "timestamp": Utc::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use rust_decimal_macros::dec;

    use crate::types::{Chain, DexQuote, SwapHop};

    fn valid_quote(slippage_bps: u16, price_impact: rust_decimal::Decimal) -> DexQuote {
        DexQuote {
            provider: "1inch".into(),
            chain: Chain::Arbitrum,
            token_in: "USDC".into(),
            token_out: "WETH".into(),
            amount_in: dec!(1000),
            amount_out: dec!(0.45),
            price: dec!(0.00045),
            price_impact,
            gas_estimate: dec!(0.00004),
            gas_usd: dec!(0.10),
            route: vec![SwapHop {
                dex: "uniswap_v3".into(),
                pool: "0xpool".into(),
                token_in: "USDC".into(),
                token_out: "WETH".into(),
                portion: dec!(100),
            }],
            expires_at: chrono::Utc::now() + Duration::seconds(30),
            slippage_bps,
        }
    }

    #[tokio::test]
    async fn execute_valid_swap_returns_simulated() {
        let executor = SwapExecutor::new();
        let req = SwapRequest {
            quote: valid_quote(50, dec!(0.1)),
            sender: "0xAliceWallet".into(),
        };
        let result = executor.execute(req).await.unwrap();
        assert!(matches!(result.status, SwapStatus::Simulated));
        assert!(result.tx_hash.is_some());
        assert!(result.tx_hash.as_ref().unwrap().starts_with("0xsim-"));
    }

    #[tokio::test]
    async fn execute_expired_quote_returns_err() {
        let executor = SwapExecutor::new();
        let mut quote = valid_quote(50, dec!(0.1));
        quote.expires_at = chrono::Utc::now() - Duration::seconds(1);

        let req = SwapRequest { quote, sender: "0xAlice".into() };
        let err = executor.execute(req).await.unwrap_err();
        assert!(err.to_string().contains("expired"));
    }

    #[tokio::test]
    async fn execute_high_price_impact_rejected() {
        let executor = SwapExecutor::new();
        // slippage_bps = 50 → tolerance = 0.5%; price_impact = 1.0% → should fail
        let req = SwapRequest {
            quote: valid_quote(50, dec!(1.0)),
            sender: "0xBob".into(),
        };
        assert!(executor.execute(req).await.is_err());
    }
}
