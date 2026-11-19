//! Core types for the DEX aggregator.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// EVM-compatible and non-EVM chains supported by the aggregator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Chain {
    Ethereum,
    Arbitrum,
    Base,
    Polygon,
    Bsc,
    Solana,
    Avalanche,
    Optimism,
}

impl Chain {
    /// Returns the EVM chain ID, or `None` for non-EVM chains (e.g. Solana).
    pub fn chain_id(self) -> Option<u64> {
        match self {
            Chain::Ethereum => Some(1),
            Chain::Arbitrum => Some(42161),
            Chain::Base => Some(8453),
            Chain::Polygon => Some(137),
            Chain::Bsc => Some(56),
            Chain::Solana => None,
            Chain::Avalanche => Some(43114),
            Chain::Optimism => Some(10),
        }
    }

    /// Human-readable label used in logs and API responses.
    pub fn label(self) -> &'static str {
        match self {
            Chain::Ethereum => "ethereum",
            Chain::Arbitrum => "arbitrum",
            Chain::Base => "base",
            Chain::Polygon => "polygon",
            Chain::Bsc => "bsc",
            Chain::Solana => "solana",
            Chain::Avalanche => "avalanche",
            Chain::Optimism => "optimism",
        }
    }
}

impl std::fmt::Display for Chain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// A quote returned by a single DEX provider for a given swap.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DexQuote {
    /// Provider name: "1inch", "jupiter", "0x".
    pub provider: String,
    pub chain: Chain,
    /// Input token — address (EVM) or mint address (Solana).
    pub token_in: String,
    /// Output token.
    pub token_out: String,
    /// Exact input amount.
    pub amount_in: Decimal,
    /// Expected output amount after fees.
    pub amount_out: Decimal,
    /// Effective price: `amount_out / amount_in`.
    pub price: Decimal,
    /// Price impact as a percentage (e.g. `0.12` = 0.12%).
    pub price_impact: Decimal,
    /// Gas cost denominated in the chain's native token.
    pub gas_estimate: Decimal,
    /// Gas cost converted to USD.
    pub gas_usd: Decimal,
    /// Ordered hops the swap takes through on-chain pools.
    pub route: Vec<SwapHop>,
    /// Quote validity deadline — provider quotes expire after a short window.
    pub expires_at: DateTime<Utc>,
    /// Slippage tolerance in basis points (e.g. `50` = 0.5%).
    pub slippage_bps: u16,
}

impl DexQuote {
    /// Net output after subtracting gas cost from the received amount value.
    ///
    /// `amount_out` is denominated in the output token; `gas_usd` is in USD.
    /// This helper returns the raw `amount_out` so callers can compare quotes
    /// on the same basis. Gas-adjusted comparison should use `net_value_usd`.
    pub fn net_amount_out(&self) -> Decimal {
        self.amount_out
    }

    /// Returns `true` when the quote is still within its validity window.
    pub fn is_valid(&self) -> bool {
        Utc::now() < self.expires_at
    }
}

/// A single hop in a multi-hop swap route.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapHop {
    /// DEX protocol name: "uniswap_v3", "curve", "sushiswap", etc.
    pub dex: String,
    /// Pool address or identifier.
    pub pool: String,
    pub token_in: String,
    pub token_out: String,
    /// Fraction of the total input amount routed through this hop (0–100%).
    pub portion: Decimal,
}

/// Result of attempting to execute a swap.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapResult {
    /// On-chain transaction hash, if the swap was submitted.
    pub tx_hash: Option<String>,
    pub amount_in: Decimal,
    pub amount_out: Decimal,
    /// Actual gas consumed (in native token units).
    pub gas_used: Decimal,
    pub status: SwapStatus,
}

/// Lifecycle status of a swap execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SwapStatus {
    /// Phase 1: swap was validated and priced but not broadcast to a chain.
    Simulated,
    /// Transaction has been submitted; awaiting inclusion.
    Pending,
    /// Transaction included in a finalised block.
    Confirmed,
    /// Transaction failed or was rejected.
    Failed(String),
}

/// Request parameters for querying a DEX quote.
#[derive(Debug, Clone)]
pub struct QuoteRequest {
    pub chain: Chain,
    pub token_in: String,
    pub token_out: String,
    pub amount: Decimal,
    pub slippage_bps: u16,
}

/// Request parameters for executing a swap.
#[derive(Debug, Clone)]
pub struct SwapRequest {
    pub quote: DexQuote,
    /// Wallet address that will send and receive tokens.
    pub sender: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn chain_id_evm_chains() {
        assert_eq!(Chain::Ethereum.chain_id(), Some(1));
        assert_eq!(Chain::Arbitrum.chain_id(), Some(42161));
        assert_eq!(Chain::Base.chain_id(), Some(8453));
    }

    #[test]
    fn chain_id_solana_is_none() {
        assert_eq!(Chain::Solana.chain_id(), None);
    }

    #[test]
    fn chain_display() {
        assert_eq!(Chain::Ethereum.to_string(), "ethereum");
        assert_eq!(Chain::Solana.to_string(), "solana");
    }

    #[test]
    fn dex_quote_validity() {
        let quote = DexQuote {
            provider: "1inch".into(),
            chain: Chain::Ethereum,
            token_in: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".into(), // USDC
            token_out: "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".into(), // WETH
            amount_in: dec!(1000),
            amount_out: dec!(0.45),
            price: dec!(0.00045),
            price_impact: dec!(0.05),
            gas_estimate: dec!(0.003),
            gas_usd: dec!(5.00),
            route: vec![],
            expires_at: Utc::now() + chrono::Duration::seconds(30),
            slippage_bps: 50,
        };

        assert!(quote.is_valid());
    }

    #[test]
    fn dex_quote_expired() {
        let quote = DexQuote {
            provider: "0x".into(),
            chain: Chain::Polygon,
            token_in: "USDC".into(),
            token_out: "WMATIC".into(),
            amount_in: dec!(100),
            amount_out: dec!(110),
            price: dec!(1.1),
            price_impact: dec!(0.01),
            gas_estimate: dec!(0.01),
            gas_usd: dec!(0.02),
            route: vec![],
            expires_at: Utc::now() - chrono::Duration::seconds(1),
            slippage_bps: 30,
        };

        assert!(!quote.is_valid());
    }
}
