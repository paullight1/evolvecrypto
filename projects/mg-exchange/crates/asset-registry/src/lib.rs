use std::collections::HashMap;
use std::str::FromStr;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Network identifier: "ERC20", "TRC20", "BEP20", "SOL", "BTC", "ARB", etc.
    pub network: String,
    /// Human-readable network name
    pub chain_type: String,
    /// Token contract address (empty for native coins like BTC/ETH)
    pub contract_address: Option<String>,
    /// Whether deposits are enabled on this network
    pub deposit_enabled: bool,
    /// Whether withdrawals are enabled on this network
    pub withdraw_enabled: bool,
    /// Fixed withdrawal fee in coin units
    pub withdraw_fee: Decimal,
    /// Minimum withdrawal amount
    pub withdraw_min: Decimal,
    /// Maximum withdrawal amount per transaction
    pub withdraw_max: Decimal,
    /// Minimum deposit amount
    pub deposit_min: Decimal,
    /// Number of confirmations needed before deposit is credited
    pub confirmations_required: u32,
    /// Number of confirmations for "safe" (unlocked for withdrawal)
    pub safe_confirmations: u32,
    /// Decimal precision for amounts on this network
    pub min_accuracy: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoinInfo {
    /// Coin ticker: "BTC", "ETH", "USDT", etc.
    pub coin: String,
    /// Full name
    pub name: String,
    /// Whether this coin can be traded on the exchange
    pub is_tradeable: bool,
    /// Available networks for deposit/withdrawal
    pub networks: Vec<NetworkConfig>,
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("Unknown coin: {0}")]
    UnknownCoin(String),
    #[error("Unknown network '{network}' for coin '{coin}'")]
    UnknownNetwork { coin: String, network: String },
    #[error("Withdrawals disabled for {coin} on {network}")]
    WithdrawDisabled { coin: String, network: String },
    #[error("Deposits disabled for {coin} on {network}")]
    DepositDisabled { coin: String, network: String },
    #[error("Amount {amount} below minimum {min} for {coin} on {network}")]
    BelowMinimum {
        coin: String,
        network: String,
        amount: Decimal,
        min: Decimal,
    },
    #[error("Amount {amount} exceeds maximum {max} for {coin} on {network}")]
    ExceedsMaximum {
        coin: String,
        network: String,
        amount: Decimal,
        max: Decimal,
    },
}

pub struct AssetRegistry {
    coins: HashMap<String, CoinInfo>,
}

impl AssetRegistry {
    pub fn new() -> Self {
        Self {
            coins: HashMap::new(),
        }
    }

    /// Bootstrap with default coin configurations.
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register_defaults();
        registry
    }

    pub fn register(&mut self, info: CoinInfo) {
        self.coins.insert(info.coin.clone(), info);
    }

    pub fn get(&self, coin: &str) -> Option<&CoinInfo> {
        self.coins.get(coin)
    }

    pub fn get_network(&self, coin: &str, network: &str) -> Option<&NetworkConfig> {
        self.coins
            .get(coin)?
            .networks
            .iter()
            .find(|n| n.network == network)
    }

    pub fn all_coins(&self) -> Vec<&CoinInfo> {
        self.coins.values().collect()
    }

    /// Validate a withdrawal request against registry rules.
    pub fn validate_withdrawal(
        &self,
        coin: &str,
        network: &str,
        amount: Decimal,
    ) -> Result<&NetworkConfig, RegistryError> {
        let coin_info = self
            .coins
            .get(coin)
            .ok_or_else(|| RegistryError::UnknownCoin(coin.to_string()))?;
        let net = coin_info
            .networks
            .iter()
            .find(|n| n.network == network)
            .ok_or_else(|| RegistryError::UnknownNetwork {
                coin: coin.to_string(),
                network: network.to_string(),
            })?;
        if !net.withdraw_enabled {
            return Err(RegistryError::WithdrawDisabled {
                coin: coin.to_string(),
                network: network.to_string(),
            });
        }
        if amount < net.withdraw_min {
            return Err(RegistryError::BelowMinimum {
                coin: coin.to_string(),
                network: network.to_string(),
                amount,
                min: net.withdraw_min,
            });
        }
        if amount > net.withdraw_max {
            return Err(RegistryError::ExceedsMaximum {
                coin: coin.to_string(),
                network: network.to_string(),
                amount,
                max: net.withdraw_max,
            });
        }
        Ok(net)
    }

    /// Validate a deposit (check network exists and deposits are enabled).
    pub fn validate_deposit(
        &self,
        coin: &str,
        network: &str,
    ) -> Result<&NetworkConfig, RegistryError> {
        let coin_info = self
            .coins
            .get(coin)
            .ok_or_else(|| RegistryError::UnknownCoin(coin.to_string()))?;
        let net = coin_info
            .networks
            .iter()
            .find(|n| n.network == network)
            .ok_or_else(|| RegistryError::UnknownNetwork {
                coin: coin.to_string(),
                network: network.to_string(),
            })?;
        if !net.deposit_enabled {
            return Err(RegistryError::DepositDisabled {
                coin: coin.to_string(),
                network: network.to_string(),
            });
        }
        Ok(net)
    }

    fn register_defaults(&mut self) {
        // BTC
        self.register(CoinInfo {
            coin: "BTC".into(),
            name: "Bitcoin".into(),
            is_tradeable: true,
            networks: vec![NetworkConfig {
                network: "BTC".into(),
                chain_type: "Bitcoin".into(),
                contract_address: None,
                deposit_enabled: true,
                withdraw_enabled: true,
                withdraw_fee: Decimal::from_str("0.0002").expect("valid decimal"),
                withdraw_min: Decimal::from_str("0.001").expect("valid decimal"),
                withdraw_max: Decimal::from_str("100").expect("valid decimal"),
                deposit_min: Decimal::from_str("0.0001").expect("valid decimal"),
                confirmations_required: 2,
                safe_confirmations: 6,
                min_accuracy: 8,
            }],
        });

        // ETH
        self.register(CoinInfo {
            coin: "ETH".into(),
            name: "Ethereum".into(),
            is_tradeable: true,
            networks: vec![
                NetworkConfig {
                    network: "ERC20".into(),
                    chain_type: "Ethereum".into(),
                    contract_address: None,
                    deposit_enabled: true,
                    withdraw_enabled: true,
                    withdraw_fee: Decimal::from_str("0.001").expect("valid decimal"),
                    withdraw_min: Decimal::from_str("0.01").expect("valid decimal"),
                    withdraw_max: Decimal::from_str("5000").expect("valid decimal"),
                    deposit_min: Decimal::from_str("0.001").expect("valid decimal"),
                    confirmations_required: 12,
                    safe_confirmations: 64,
                    min_accuracy: 8,
                },
                NetworkConfig {
                    network: "ARB".into(),
                    chain_type: "Arbitrum One".into(),
                    contract_address: None,
                    deposit_enabled: true,
                    withdraw_enabled: true,
                    withdraw_fee: Decimal::from_str("0.0001").expect("valid decimal"),
                    withdraw_min: Decimal::from_str("0.001").expect("valid decimal"),
                    withdraw_max: Decimal::from_str("5000").expect("valid decimal"),
                    deposit_min: Decimal::from_str("0.0001").expect("valid decimal"),
                    confirmations_required: 12,
                    safe_confirmations: 20,
                    min_accuracy: 8,
                },
            ],
        });

        // USDT
        self.register(CoinInfo {
            coin: "USDT".into(),
            name: "Tether USD".into(),
            is_tradeable: true,
            networks: vec![
                NetworkConfig {
                    network: "ERC20".into(),
                    chain_type: "Ethereum".into(),
                    contract_address: Some(
                        "0xdac17f958d2ee523a2206206994597c13d831ec7".into(),
                    ),
                    deposit_enabled: true,
                    withdraw_enabled: true,
                    withdraw_fee: Decimal::from_str("3.0").expect("valid decimal"),
                    withdraw_min: Decimal::from_str("10").expect("valid decimal"),
                    withdraw_max: Decimal::from_str("1000000").expect("valid decimal"),
                    deposit_min: Decimal::ONE,
                    confirmations_required: 12,
                    safe_confirmations: 64,
                    min_accuracy: 6,
                },
                NetworkConfig {
                    network: "TRC20".into(),
                    chain_type: "Tron".into(),
                    contract_address: Some("TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t".into()),
                    deposit_enabled: true,
                    withdraw_enabled: true,
                    withdraw_fee: Decimal::ONE,
                    withdraw_min: Decimal::from_str("5").expect("valid decimal"),
                    withdraw_max: Decimal::from_str("1000000").expect("valid decimal"),
                    deposit_min: Decimal::ONE,
                    confirmations_required: 20,
                    safe_confirmations: 41,
                    min_accuracy: 6,
                },
                NetworkConfig {
                    network: "SOL".into(),
                    chain_type: "Solana".into(),
                    contract_address: Some(
                        "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB".into(),
                    ),
                    deposit_enabled: true,
                    withdraw_enabled: true,
                    withdraw_fee: Decimal::ONE,
                    withdraw_min: Decimal::ONE,
                    withdraw_max: Decimal::from_str("1000000").expect("valid decimal"),
                    deposit_min: Decimal::from_str("0.1").expect("valid decimal"),
                    confirmations_required: 1,
                    safe_confirmations: 1,
                    min_accuracy: 6,
                },
            ],
        });

        // USDC
        self.register(CoinInfo {
            coin: "USDC".into(),
            name: "USD Coin".into(),
            is_tradeable: true,
            networks: vec![
                NetworkConfig {
                    network: "ERC20".into(),
                    chain_type: "Ethereum".into(),
                    contract_address: Some(
                        "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".into(),
                    ),
                    deposit_enabled: true,
                    withdraw_enabled: true,
                    withdraw_fee: Decimal::from_str("3.0").expect("valid decimal"),
                    withdraw_min: Decimal::from_str("10").expect("valid decimal"),
                    withdraw_max: Decimal::from_str("1000000").expect("valid decimal"),
                    deposit_min: Decimal::ONE,
                    confirmations_required: 12,
                    safe_confirmations: 64,
                    min_accuracy: 6,
                },
                NetworkConfig {
                    network: "SOL".into(),
                    chain_type: "Solana".into(),
                    contract_address: Some(
                        "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
                    ),
                    deposit_enabled: true,
                    withdraw_enabled: true,
                    withdraw_fee: Decimal::ONE,
                    withdraw_min: Decimal::ONE,
                    withdraw_max: Decimal::from_str("1000000").expect("valid decimal"),
                    deposit_min: Decimal::from_str("0.1").expect("valid decimal"),
                    confirmations_required: 1,
                    safe_confirmations: 1,
                    min_accuracy: 6,
                },
            ],
        });

        // SOL
        self.register(CoinInfo {
            coin: "SOL".into(),
            name: "Solana".into(),
            is_tradeable: true,
            networks: vec![NetworkConfig {
                network: "SOL".into(),
                chain_type: "Solana".into(),
                contract_address: None,
                deposit_enabled: true,
                withdraw_enabled: true,
                withdraw_fee: Decimal::from_str("0.01").expect("valid decimal"),
                withdraw_min: Decimal::from_str("0.1").expect("valid decimal"),
                withdraw_max: Decimal::from_str("50000").expect("valid decimal"),
                deposit_min: Decimal::from_str("0.01").expect("valid decimal"),
                confirmations_required: 1,
                safe_confirmations: 1,
                min_accuracy: 9,
            }],
        });
    }
}

impl Default for AssetRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults_have_btc_eth_usdt() {
        let reg = AssetRegistry::with_defaults();
        assert!(reg.get("BTC").is_some());
        assert!(reg.get("ETH").is_some());
        assert!(reg.get("USDT").is_some());
        assert!(reg.get("USDC").is_some());
        assert!(reg.get("SOL").is_some());
    }

    #[test]
    fn test_unknown_coin_error() {
        let reg = AssetRegistry::with_defaults();
        assert!(matches!(
            reg.validate_withdrawal("DOGE", "BTC", Decimal::ONE),
            Err(RegistryError::UnknownCoin(_))
        ));
    }

    #[test]
    fn test_unknown_network_error() {
        let reg = AssetRegistry::with_defaults();
        assert!(matches!(
            reg.validate_withdrawal("BTC", "TRC20", Decimal::ONE),
            Err(RegistryError::UnknownNetwork { .. })
        ));
    }

    #[test]
    fn test_below_minimum_error() {
        let reg = AssetRegistry::with_defaults();
        let result = reg.validate_withdrawal(
            "BTC",
            "BTC",
            Decimal::from_str("0.0001").unwrap(),
        );
        assert!(matches!(result, Err(RegistryError::BelowMinimum { .. })));
    }

    #[test]
    fn test_valid_withdrawal() {
        let reg = AssetRegistry::with_defaults();
        let result = reg.validate_withdrawal("BTC", "BTC", Decimal::ONE);
        assert!(result.is_ok());
    }

    #[test]
    fn test_usdt_has_multiple_networks() {
        let reg = AssetRegistry::with_defaults();
        let usdt = reg.get("USDT").unwrap();
        assert!(usdt.networks.len() >= 3, "USDT should have ERC20, TRC20, SOL");
    }

    #[test]
    fn test_get_network() {
        let reg = AssetRegistry::with_defaults();
        let net = reg.get_network("ETH", "ARB");
        assert!(net.is_some());
        assert_eq!(net.unwrap().chain_type, "Arbitrum One");
    }

    #[test]
    fn test_validate_deposit_ok() {
        let reg = AssetRegistry::with_defaults();
        assert!(reg.validate_deposit("USDT", "TRC20").is_ok());
    }

    #[test]
    fn test_validate_deposit_unknown_coin() {
        let reg = AssetRegistry::with_defaults();
        assert!(matches!(
            reg.validate_deposit("XYZ", "ERC20"),
            Err(RegistryError::UnknownCoin(_))
        ));
    }
}
