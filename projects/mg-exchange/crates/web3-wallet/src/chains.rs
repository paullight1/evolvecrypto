//! Multi-chain metadata and per-chain configuration.

use rust_decimal::Decimal;

use crate::types::Chain;

/// Static configuration for a single blockchain.
#[derive(Debug, Clone)]
pub struct ChainConfig {
    pub chain: Chain,
    pub name: &'static str,
    /// Native gas token symbol.
    pub native_token: &'static str,
    /// Average transaction confirmation time in seconds.
    pub avg_block_time_secs: u64,
    /// Estimated gas cost for a simple token transfer, in USD.
    pub gas_transfer_usd: Decimal,
}

/// Return configuration for all supported chains.
pub fn all_chain_configs() -> Vec<ChainConfig> {
    vec![
        ChainConfig {
            chain: Chain::Ethereum,
            name: "Ethereum",
            native_token: "ETH",
            avg_block_time_secs: 12,
            gas_transfer_usd: Decimal::new(300, 2), // $3.00
        },
        ChainConfig {
            chain: Chain::Arbitrum,
            name: "Arbitrum One",
            native_token: "ETH",
            avg_block_time_secs: 1,
            gas_transfer_usd: Decimal::new(10, 2), // $0.10
        },
        ChainConfig {
            chain: Chain::Base,
            name: "Base",
            native_token: "ETH",
            avg_block_time_secs: 2,
            gas_transfer_usd: Decimal::new(5, 2), // $0.05
        },
        ChainConfig {
            chain: Chain::Polygon,
            name: "Polygon",
            native_token: "MATIC",
            avg_block_time_secs: 2,
            gas_transfer_usd: Decimal::new(1, 2), // $0.01
        },
        ChainConfig {
            chain: Chain::Bsc,
            name: "BNB Smart Chain",
            native_token: "BNB",
            avg_block_time_secs: 3,
            gas_transfer_usd: Decimal::new(10, 2), // $0.10
        },
        ChainConfig {
            chain: Chain::Solana,
            name: "Solana",
            native_token: "SOL",
            avg_block_time_secs: 1,
            gas_transfer_usd: Decimal::new(1, 3), // $0.001
        },
        ChainConfig {
            chain: Chain::Avalanche,
            name: "Avalanche C-Chain",
            native_token: "AVAX",
            avg_block_time_secs: 2,
            gas_transfer_usd: Decimal::new(5, 2), // $0.05
        },
        ChainConfig {
            chain: Chain::Optimism,
            name: "Optimism",
            native_token: "ETH",
            avg_block_time_secs: 2,
            gas_transfer_usd: Decimal::new(5, 2), // $0.05
        },
    ]
}

/// Look up the configuration for a specific chain.
pub fn chain_config(chain: Chain) -> Option<ChainConfig> {
    all_chain_configs().into_iter().find(|c| c.chain == chain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_chains_have_config() {
        for &chain in Chain::all() {
            assert!(
                chain_config(chain).is_some(),
                "missing chain config for {chain}"
            );
        }
    }

    #[test]
    fn gas_costs_are_positive() {
        for cfg in all_chain_configs() {
            assert!(
                cfg.gas_transfer_usd > Decimal::ZERO,
                "gas cost for {} must be positive",
                cfg.chain
            );
        }
    }
}
