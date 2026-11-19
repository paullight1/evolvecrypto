//! On-chain balance provider for Proof of Reserves.
//!
//! Queries real blockchain balances so the PoR report can attest that
//! the exchange holds at least as much on-chain as it owes in liabilities.
//!
//! Phase 1: MockChainBalanceProvider (test/dev).
//! Phase 2: BTC via public API (blockstream.info), ETH via public JSON-RPC.
//! No API keys required for public endpoints.

use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// A single on-chain address and its balance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressBalance {
    pub address: String,
    pub asset: String,
    pub balance: Decimal,
}

/// Provider that fetches on-chain balances for a set of exchange-controlled addresses.
pub trait ChainBalanceProvider: Send + Sync {
    /// Fetch the balance for each address.
    ///
    /// Returns a map of asset → (total_balance, vec_of_addresses).
    /// This is the same format that `ReportGenerator::generate` expects for
    /// the `reserves` parameter.
    fn fetch_balances(
        &self,
        addresses: &[WalletAddress],
    ) -> Result<HashMap<String, (Decimal, Vec<String>)>, ChainBalanceError>;
}

/// Exchange-controlled wallet address with asset tag.
#[derive(Debug, Clone)]
pub struct WalletAddress {
    pub address: String,
    pub asset: String,
    /// Hot / Warm / Cold — informational, not used for balance fetch.
    pub wallet_type: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ChainBalanceError {
    #[error("RPC request failed: {0}")]
    RpcError(String),
    #[error("Balance parse error: {0}")]
    ParseError(String),
}

// ---------------------------------------------------------------------------
// Mock provider (dev/test)
// ---------------------------------------------------------------------------

/// Returns pre-configured balances. Used in dev mode and tests.
pub struct MockChainBalanceProvider {
    balances: HashMap<String, Decimal>,
}

impl MockChainBalanceProvider {
    pub fn new() -> Self {
        Self {
            balances: HashMap::new(),
        }
    }

    /// Set the balance for a specific address.
    pub fn set_balance(mut self, address: &str, balance: Decimal) -> Self {
        self.balances.insert(address.to_string(), balance);
        self
    }
}

impl Default for MockChainBalanceProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ChainBalanceProvider for MockChainBalanceProvider {
    fn fetch_balances(
        &self,
        addresses: &[WalletAddress],
    ) -> Result<HashMap<String, (Decimal, Vec<String>)>, ChainBalanceError> {
        let mut result: HashMap<String, (Decimal, Vec<String>)> = HashMap::new();

        for addr in addresses {
            let balance = self
                .balances
                .get(&addr.address)
                .copied()
                .unwrap_or(Decimal::ZERO);

            let entry = result
                .entry(addr.asset.clone())
                .or_insert_with(|| (Decimal::ZERO, Vec::new()));
            entry.0 += balance;
            entry.1.push(addr.address.clone());
        }

        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// Bitcoin provider (public API — no key required)
// ---------------------------------------------------------------------------

/// Fetches BTC balances from blockstream.info API.
///
/// Endpoint: `GET https://blockstream.info/api/address/{address}`
/// Returns JSON with `chain_stats.funded_txo_sum` and `chain_stats.spent_txo_sum`
/// in satoshis. Balance = funded - spent.
///
/// No API key required. Rate limit: ~10 req/sec (polite usage).
pub struct BtcBlockstreamProvider {
    base_url: String,
}

impl BtcBlockstreamProvider {
    pub fn new() -> Self {
        Self {
            base_url: "https://blockstream.info/api".to_string(),
        }
    }

    pub fn with_base_url(base_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
        }
    }

    /// Fetch balance for a single BTC address (in BTC, not satoshis).
    ///
    /// Calls `GET {base_url}/address/{address}` and parses `chain_stats`.
    /// Balance = funded_txo_sum - spent_txo_sum (in satoshis, converted to BTC).
    ///
    /// This is a blocking HTTP call — intended for background tasks, not hot path.
    fn fetch_btc_balance(&self, address: &str) -> Result<Decimal, ChainBalanceError> {
        let url = format!("{}/address/{}", self.base_url, address);

        let resp = reqwest::blocking::get(&url)
            .map_err(|e| ChainBalanceError::RpcError(format!("BTC HTTP request failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(ChainBalanceError::RpcError(format!(
                "BTC API returned status {} for address {address}",
                resp.status()
            )));
        }

        let body: serde_json::Value = resp
            .json()
            .map_err(|e| ChainBalanceError::ParseError(format!("BTC JSON parse failed: {e}")))?;

        // Blockstream response: { chain_stats: { funded_txo_sum: N, spent_txo_sum: N } }
        let funded = body["chain_stats"]["funded_txo_sum"]
            .as_u64()
            .unwrap_or(0);
        let spent = body["chain_stats"]["spent_txo_sum"]
            .as_u64()
            .unwrap_or(0);
        let satoshis = funded.saturating_sub(spent);

        // Convert satoshis to BTC (1 BTC = 100_000_000 sat).
        let btc = Decimal::new(satoshis as i64, 8);

        tracing::info!(
            address = %address,
            satoshis,
            btc = %btc,
            "BTC balance fetched from Blockstream"
        );

        Ok(btc)
    }
}

impl Default for BtcBlockstreamProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ChainBalanceProvider for BtcBlockstreamProvider {
    fn fetch_balances(
        &self,
        addresses: &[WalletAddress],
    ) -> Result<HashMap<String, (Decimal, Vec<String>)>, ChainBalanceError> {
        let mut result: HashMap<String, (Decimal, Vec<String>)> = HashMap::new();

        for addr in addresses {
            if addr.asset != "BTC" {
                continue;
            }
            let balance = self.fetch_btc_balance(&addr.address)?;
            let entry = result
                .entry("BTC".to_string())
                .or_insert_with(|| (Decimal::ZERO, Vec::new()));
            entry.0 += balance;
            entry.1.push(addr.address.clone());
        }

        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// Ethereum provider (public JSON-RPC — no key required for public nodes)
// ---------------------------------------------------------------------------

/// Fetches ETH balances via JSON-RPC `eth_getBalance`.
///
/// Default endpoint: Cloudflare's public Ethereum gateway.
/// No API key required. For production, use Alchemy/Infura with a key.
pub struct EthRpcProvider {
    rpc_url: String,
}

impl EthRpcProvider {
    pub fn new() -> Self {
        Self {
            rpc_url: "https://cloudflare-eth.com".to_string(),
        }
    }

    pub fn with_url(rpc_url: &str) -> Self {
        Self {
            rpc_url: rpc_url.to_string(),
        }
    }

    /// Call `eth_getBalance` via JSON-RPC and return balance in ETH.
    ///
    /// Blocking HTTP — intended for background PoR report generation.
    fn fetch_eth_balance(&self, address: &str) -> Result<Decimal, ChainBalanceError> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getBalance",
            "params": [address, "latest"],
            "id": 1,
        });

        let client = reqwest::blocking::Client::new();
        let resp = client
            .post(&self.rpc_url)
            .json(&payload)
            .send()
            .map_err(|e| ChainBalanceError::RpcError(format!("ETH RPC request failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(ChainBalanceError::RpcError(format!(
                "ETH RPC returned status {} for address {address}",
                resp.status()
            )));
        }

        let body: serde_json::Value = resp
            .json()
            .map_err(|e| ChainBalanceError::ParseError(format!("ETH JSON parse failed: {e}")))?;

        // Response: { "result": "0x..." } — hex-encoded wei.
        let hex_str = body["result"]
            .as_str()
            .unwrap_or("0x0")
            .trim_start_matches("0x");

        let wei = u128::from_str_radix(hex_str, 16)
            .map_err(|e| ChainBalanceError::ParseError(format!("ETH hex parse failed: {e}")))?;

        // Convert wei to ETH (1 ETH = 10^18 wei).
        // Use integer division to avoid floating point: split into whole + fractional.
        let eth_whole = wei / 1_000_000_000_000_000_000;
        let eth_frac = wei % 1_000_000_000_000_000_000;
        let eth = Decimal::new(eth_whole as i64, 0)
            + Decimal::new(eth_frac as i64, 18);

        tracing::info!(
            address = %address,
            wei,
            eth = %eth,
            "ETH balance fetched via JSON-RPC"
        );

        Ok(eth)
    }
}

impl Default for EthRpcProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ChainBalanceProvider for EthRpcProvider {
    fn fetch_balances(
        &self,
        addresses: &[WalletAddress],
    ) -> Result<HashMap<String, (Decimal, Vec<String>)>, ChainBalanceError> {
        let mut result: HashMap<String, (Decimal, Vec<String>)> = HashMap::new();

        for addr in addresses {
            if addr.asset != "ETH" {
                continue;
            }
            let balance = self.fetch_eth_balance(&addr.address)?;
            let entry = result
                .entry("ETH".to_string())
                .or_insert_with(|| (Decimal::ZERO, Vec::new()));
            entry.0 += balance;
            entry.1.push(addr.address.clone());
        }

        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// Composite provider: delegates to BTC + ETH providers
// ---------------------------------------------------------------------------

/// Combines multiple chain-specific providers into one.
pub struct CompositeChainBalanceProvider {
    providers: Vec<Box<dyn ChainBalanceProvider>>,
}

impl CompositeChainBalanceProvider {
    pub fn new(providers: Vec<Box<dyn ChainBalanceProvider>>) -> Self {
        Self { providers }
    }

    /// Default production setup: BTC (Blockstream) + ETH (Cloudflare).
    pub fn default_production() -> Self {
        Self::new(vec![
            Box::new(BtcBlockstreamProvider::new()),
            Box::new(EthRpcProvider::new()),
        ])
    }
}

impl ChainBalanceProvider for CompositeChainBalanceProvider {
    fn fetch_balances(
        &self,
        addresses: &[WalletAddress],
    ) -> Result<HashMap<String, (Decimal, Vec<String>)>, ChainBalanceError> {
        let mut combined: HashMap<String, (Decimal, Vec<String>)> = HashMap::new();

        for provider in &self.providers {
            let balances = provider.fetch_balances(addresses)?;
            for (asset, (balance, addrs)) in balances {
                let entry = combined
                    .entry(asset)
                    .or_insert_with(|| (Decimal::ZERO, Vec::new()));
                entry.0 += balance;
                entry.1.extend(addrs);
            }
        }

        Ok(combined)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn test_addresses() -> Vec<WalletAddress> {
        vec![
            WalletAddress {
                address: "bc1q_cold".to_string(),
                asset: "BTC".to_string(),
                wallet_type: "cold".to_string(),
            },
            WalletAddress {
                address: "bc1q_hot".to_string(),
                asset: "BTC".to_string(),
                wallet_type: "hot".to_string(),
            },
            WalletAddress {
                address: "0x_cold".to_string(),
                asset: "ETH".to_string(),
                wallet_type: "cold".to_string(),
            },
        ]
    }

    #[test]
    fn mock_provider_returns_configured_balances() {
        let provider = MockChainBalanceProvider::new()
            .set_balance("bc1q_cold", dec!(10))
            .set_balance("bc1q_hot", dec!(0.5))
            .set_balance("0x_cold", dec!(100));

        let result = provider.fetch_balances(&test_addresses()).unwrap();

        let btc = result.get("BTC").unwrap();
        assert_eq!(btc.0, dec!(10.5), "BTC total should be 10 + 0.5");
        assert_eq!(btc.1.len(), 2, "two BTC addresses");

        let eth = result.get("ETH").unwrap();
        assert_eq!(eth.0, dec!(100));
        assert_eq!(eth.1.len(), 1);
    }

    #[test]
    fn mock_provider_zero_for_unknown_addresses() {
        let provider = MockChainBalanceProvider::new();
        let result = provider.fetch_balances(&test_addresses()).unwrap();

        let btc = result.get("BTC").unwrap();
        assert_eq!(btc.0, Decimal::ZERO);
    }

    #[test]
    fn composite_provider_merges_results() {
        let btc = MockChainBalanceProvider::new()
            .set_balance("bc1q_cold", dec!(5));
        let eth = MockChainBalanceProvider::new()
            .set_balance("0x_cold", dec!(50));

        let composite = CompositeChainBalanceProvider::new(vec![
            Box::new(btc),
            Box::new(eth),
        ]);

        let result = composite.fetch_balances(&test_addresses()).unwrap();
        assert_eq!(result.get("BTC").unwrap().0, dec!(5));
        assert_eq!(result.get("ETH").unwrap().0, dec!(50));
    }

    #[test]
    fn mock_integrates_with_report_generator() {
        use crate::report::{ReportGenerator, ReserveStatus, UserLiability};

        let liabilities = vec![
            UserLiability { user_id: uuid::Uuid::from_u128(1), asset: "BTC".into(), balance: dec!(2) },
            UserLiability { user_id: uuid::Uuid::from_u128(2), asset: "ETH".into(), balance: dec!(10) },
        ];

        let provider = MockChainBalanceProvider::new()
            .set_balance("bc1q_cold", dec!(5))
            .set_balance("0x_cold", dec!(100));

        let reserves = provider.fetch_balances(&test_addresses()).unwrap();
        let (report, _tree) = ReportGenerator::generate(&liabilities, &reserves, None);

        assert_eq!(report.overall_status, ReserveStatus::FullyBacked);
        assert_eq!(report.assets.iter().find(|a| a.asset == "BTC").unwrap().total_reserve, dec!(5));
    }
}
