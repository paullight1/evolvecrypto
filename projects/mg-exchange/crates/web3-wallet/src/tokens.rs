//! Token balance management across chains.
//!
//! Phase 1: simulated balances seeded deterministically from user_id.
//! Phase 2: query real on-chain balances via RPC endpoints.

use rust_decimal::Decimal;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::types::{Chain, TokenBalance};

/// Simulated USD prices per token (Phase 1 — static mock prices).
fn mock_price_usd(token: &str) -> Decimal {
    match token.to_uppercase().as_str() {
        "ETH"  => Decimal::new(3_500, 0),
        "BTC"  => Decimal::new(65_000, 0),
        "SOL"  => Decimal::new(150, 0),
        "BNB"  => Decimal::new(400, 0),
        "MATIC"=> Decimal::new(1, 0),
        "AVAX" => Decimal::new(35, 0),
        "USDC" => Decimal::new(1, 0),
        "USDT" => Decimal::new(1, 0),
        _      => Decimal::new(1, 0),
    }
}

/// Return simulated token balances for a wallet address on a given chain.
///
/// Phase 1: derives small deterministic balances from a hash of the address.
/// This gives each user a realistic-looking but fake portfolio.
pub fn simulated_balances(address: &str, chain: Chain) -> Vec<TokenBalance> {
    let tokens: &[&str] = match chain {
        Chain::Ethereum | Chain::Arbitrum | Chain::Base | Chain::Optimism => {
            &["ETH", "USDC", "USDT"]
        }
        Chain::Polygon => &["MATIC", "USDC", "USDT"],
        Chain::Bsc     => &["BNB", "USDC", "USDT"],
        Chain::Solana  => &["SOL", "USDC"],
        Chain::Avalanche => &["AVAX", "USDC", "USDT"],
    };

    tokens
        .iter()
        .enumerate()
        .map(|(i, &token)| {
            let balance = derive_balance(address, chain, token, i);
            let price = mock_price_usd(token);
            let value_usd = balance * price;
            TokenBalance {
                chain,
                token: token.to_string(),
                balance,
                value_usd,
            }
        })
        .collect()
}

/// Aggregate all balances across all chains for a user's wallet.
pub fn all_balances(_user_id: Uuid, addresses: &std::collections::HashMap<Chain, String>) -> Vec<TokenBalance> {
    addresses
        .iter()
        .flat_map(|(&chain, address)| simulated_balances(address, chain))
        .filter(|b| b.balance > Decimal::ZERO)
        .collect()
}

/// Compute total USD portfolio value across all token balances.
pub fn total_value_usd(balances: &[TokenBalance]) -> Decimal {
    balances.iter().map(|b| b.value_usd).sum()
}

fn derive_balance(address: &str, chain: Chain, token: &str, slot: usize) -> Decimal {
    let mut hasher = Sha256::new();
    hasher.update(address.as_bytes());
    hasher.update(chain.label().as_bytes());
    hasher.update(token.as_bytes());
    hasher.update([slot as u8]);
    let digest = hasher.finalize();

    // Use the first 2 bytes as a small balance 0..655 with 2 decimal places.
    let raw = u16::from_le_bytes([digest[0], digest[1]]);
    Decimal::new(raw as i64, 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulated_balances_returns_non_empty() {
        let balances = simulated_balances("0xtest_address", Chain::Ethereum);
        assert!(!balances.is_empty(), "must return some balances");
    }

    #[test]
    fn balances_are_on_correct_chain() {
        let balances = simulated_balances("0xtest", Chain::Polygon);
        for b in &balances {
            assert_eq!(b.chain, Chain::Polygon);
        }
    }

    #[test]
    fn total_value_usd_sums_correctly() {
        let balances = vec![
            TokenBalance {
                chain: Chain::Ethereum,
                token: "ETH".into(),
                balance: Decimal::new(100, 2), // 1.00
                value_usd: Decimal::new(3500, 0),
            },
            TokenBalance {
                chain: Chain::Ethereum,
                token: "USDC".into(),
                balance: Decimal::new(50000, 2), // 500.00
                value_usd: Decimal::new(500, 0),
            },
        ];
        assert_eq!(total_value_usd(&balances), Decimal::new(4000, 0));
    }

    #[test]
    fn all_balances_covers_all_chains() {
        use crate::wallet::WalletManager;

        let mut mgr = WalletManager::new();
        let user_id = Uuid::new_v4();
        let wallet = mgr.create_wallet(user_id).unwrap();

        let balances = all_balances(user_id, &wallet.addresses);
        // At minimum we expect entries for some chains.
        assert!(!balances.is_empty(), "must have at least one balance entry");

        // All returned chains must be present in the wallet's address map.
        for b in &balances {
            assert!(
                wallet.addresses.contains_key(&b.chain),
                "balance chain {} not in wallet",
                b.chain
            );
        }
    }
}
