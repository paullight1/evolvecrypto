//! Gas estimation per chain.
//!
//! Phase 1 uses static estimates based on historical averages.
//! Phase 2 should query live gas oracles (e.g. Blocknative, ETH gas station,
//! or chain-native `eth_gasPrice` / `eth_feeHistory` RPC methods).

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::types::Chain;

/// Provides gas cost estimates for swaps on each supported chain.
///
/// All estimates are in USD and represent the typical cost of a single
/// swap transaction (approve + swap, two transactions on first use).
pub struct GasOracle;

impl GasOracle {
    /// Estimated gas cost in USD for a single swap on the given chain.
    ///
    /// Values are Phase 1 static estimates derived from typical gas prices
    /// and token prices as of early 2025.
    pub fn estimate_gas_usd(chain: Chain) -> Decimal {
        match chain {
            Chain::Ethereum => dec!(5.00),  // ~200k gas @ 25 gwei, ETH ~$2500
            Chain::Arbitrum => dec!(0.10),  // L2 rollup, ~10k gas, much cheaper
            Chain::Base => dec!(0.05),      // Optimistic rollup on Coinbase infra
            Chain::Polygon => dec!(0.02),   // MATIC is cheap; fast finality
            Chain::Bsc => dec!(0.20),       // BNB gas, moderate cost
            Chain::Solana => dec!(0.01),    // Solana compute units are minimal
            Chain::Avalanche => dec!(0.15), // AVAX gas, C-Chain fees
            Chain::Optimism => dec!(0.08),  // Similar to Base, slightly higher
        }
    }

    /// Estimated gas in native token units for a swap.
    ///
    /// Returns a representative gas unit count for the chain. These are
    /// used to populate `gas_estimate` in `DexQuote`.
    pub fn estimate_gas_native(chain: Chain) -> Decimal {
        match chain {
            Chain::Ethereum => dec!(0.002),   // ~200k gas * 25 gwei ≈ 0.005 ETH, simplified
            Chain::Arbitrum => dec!(0.00004), // L2 compressed calldata cost
            Chain::Base => dec!(0.00002),
            Chain::Polygon => dec!(0.5),      // MATIC units are fractional relative to price
            Chain::Bsc => dec!(0.001),        // BNB
            Chain::Solana => dec!(0.000025),  // SOL compute units
            Chain::Avalanche => dec!(0.005),  // AVAX
            Chain::Optimism => dec!(0.00003),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ethereum_gas_most_expensive() {
        let eth_gas = GasOracle::estimate_gas_usd(Chain::Ethereum);
        let sol_gas = GasOracle::estimate_gas_usd(Chain::Solana);
        assert!(eth_gas > sol_gas, "Ethereum gas should exceed Solana gas");
    }

    #[test]
    fn all_chains_return_positive_gas() {
        let chains = [
            Chain::Ethereum,
            Chain::Arbitrum,
            Chain::Base,
            Chain::Polygon,
            Chain::Bsc,
            Chain::Solana,
            Chain::Avalanche,
            Chain::Optimism,
        ];
        for chain in chains {
            let gas = GasOracle::estimate_gas_usd(chain);
            assert!(
                gas > Decimal::ZERO,
                "Gas estimate for {chain} must be positive"
            );
        }
    }

    #[test]
    fn l2s_cheaper_than_ethereum() {
        let eth = GasOracle::estimate_gas_usd(Chain::Ethereum);
        for l2 in [Chain::Arbitrum, Chain::Base, Chain::Optimism] {
            let l2_gas = GasOracle::estimate_gas_usd(l2);
            assert!(
                l2_gas < eth,
                "{l2} gas ({l2_gas}) should be cheaper than Ethereum ({eth})"
            );
        }
    }
}
