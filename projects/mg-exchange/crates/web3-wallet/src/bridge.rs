//! Cross-chain bridge abstraction.
//!
//! Phase 1: simulated bridge execution with realistic route and fee models.
//!
//! # Supported bridge protocols
//! - **Native bridge**: canonical Ethereum ↔ Arbitrum and Ethereum ↔ Base bridges.
//! - **Circle CCTP**: native USDC transfers between any two supported EVM chains.
//! - **LayerZero** / **Wormhole**: general-purpose cross-chain routes for other tokens.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::types::{Chain, TxStatus, TxType, Web3Transaction};

/// A bridge provider that can relay tokens between two chains.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeProvider {
    /// Canonical L1↔L2 native bridge (Ethereum ↔ Arbitrum / Base).
    NativeBridge,
    /// Circle's Cross-Chain Transfer Protocol — native USDC only.
    CircleCctp,
    /// LayerZero omnichain messaging protocol.
    LayerZero,
    /// Wormhole generic cross-chain bridge.
    Wormhole,
}

use serde::{Deserialize, Serialize};

impl BridgeProvider {
    pub fn label(&self) -> &'static str {
        match self {
            Self::NativeBridge => "native_bridge",
            Self::CircleCctp   => "circle_cctp",
            Self::LayerZero    => "layerzero",
            Self::Wormhole     => "wormhole",
        }
    }
}

impl std::fmt::Display for BridgeProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// A bridge route from one chain/token to another.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeRoute {
    pub from_chain: Chain,
    pub to_chain: Chain,
    pub token: String,
    pub provider: BridgeProvider,
    /// Approximate transfer time.
    pub estimated_time_secs: u64,
    /// Bridge fee in USD.
    pub fee_usd: Decimal,
}

/// A time-limited quote for a specific bridge transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeQuote {
    pub id: Uuid,
    pub route: BridgeRoute,
    /// Input amount (in `route.token`).
    pub amount_in: Decimal,
    /// Output amount after bridge fee.
    pub amount_out: Decimal,
    /// Bridge fee paid (USD equivalent).
    pub fee: Decimal,
    pub estimated_time_secs: u64,
    pub expires_at: DateTime<Utc>,
}

impl BridgeQuote {
    /// Returns `true` while the quote is within its validity window.
    pub fn is_valid(&self) -> bool {
        Utc::now() < self.expires_at
    }
}

/// Manages bridge routes and executes cross-chain transfers.
pub struct BridgeManager {
    routes: Vec<BridgeRoute>,
}

impl BridgeManager {
    /// Create a manager pre-loaded with all supported bridge routes.
    pub fn new() -> Self {
        Self { routes: default_routes() }
    }

    /// Return all routes that can move `token` from `from` to `to`.
    pub fn get_routes(&self, from: Chain, to: Chain, token: &str) -> Vec<&BridgeRoute> {
        self.routes
            .iter()
            .filter(|r| {
                r.from_chain == from
                    && r.to_chain == to
                    && r.token.eq_ignore_ascii_case(token)
            })
            .collect()
    }

    /// Fetch the best (lowest-fee) quote for a bridge transfer.
    pub fn get_quote(
        &self,
        from: Chain,
        to: Chain,
        token: &str,
        amount: Decimal,
    ) -> Result<BridgeQuote> {
        if amount <= Decimal::ZERO {
            return Err(anyhow!("amount must be positive (got {amount})"));
        }

        let route = self
            .get_routes(from, to, token)
            .into_iter()
            .min_by(|a, b| a.fee_usd.cmp(&b.fee_usd))
            .ok_or_else(|| {
                anyhow!(
                    "no bridge route for {token} {from} → {to}"
                )
            })?
            .clone();

        let fee = route.fee_usd;
        let amount_out = if amount > fee { amount - fee } else { Decimal::ZERO };

        Ok(BridgeQuote {
            id: Uuid::new_v4(),
            estimated_time_secs: route.estimated_time_secs,
            amount_in: amount,
            amount_out,
            fee,
            expires_at: Utc::now() + Duration::seconds(60),
            route,
        })
    }

    /// Execute a bridge transfer (Phase 1: simulated instant confirmation).
    pub async fn execute_bridge(
        &self,
        quote: &BridgeQuote,
        user_id: Uuid,
        from_address: &str,
        to_address: &str,
    ) -> Result<Web3Transaction> {
        if !quote.is_valid() {
            return Err(anyhow!("bridge quote {} has expired", quote.id));
        }
        if quote.amount_out <= Decimal::ZERO {
            return Err(anyhow!(
                "bridge amount after fee is non-positive (fee {} exceeds amount {})",
                quote.fee, quote.amount_in
            ));
        }

        tracing::info!(
            user_id = %user_id,
            from = %quote.route.from_chain,
            to = %quote.route.to_chain,
            token = %quote.route.token,
            amount_in = %quote.amount_in,
            amount_out = %quote.amount_out,
            provider = %quote.route.provider,
            "Executing bridge transfer (Phase 1: simulated)",
        );

        // Simulate a synthetic tx hash.
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(quote.id.as_bytes());
        hasher.update(user_id.as_bytes());
        hasher.update(Utc::now().timestamp_micros().to_le_bytes());
        let hash = format!("0x{}", hex::encode(hasher.finalize()));

        Ok(Web3Transaction {
            id: Uuid::new_v4(),
            chain: quote.route.from_chain,
            tx_type: TxType::Bridge,
            from: from_address.to_string(),
            to: to_address.to_string(),
            amount: quote.amount_in,
            token: quote.route.token.clone(),
            gas_estimate_usd: quote.fee,
            status: TxStatus::Confirmed,
            tx_hash: Some(hash),
            created_at: Utc::now(),
        })
    }
}

impl Default for BridgeManager {
    fn default() -> Self {
        Self::new()
    }
}

// ── Default route table ───────────────────────────────────────────────────────

#[allow(clippy::vec_init_then_push)]
fn default_routes() -> Vec<BridgeRoute> {
    let mut routes = Vec::new();

    // ── Ethereum ↔ Arbitrum (native bridge) ─────────────────────────────────
    // L1→L2: ~1–3 minutes (sequencer picks up instantly).
    // L2→L1: up to 7 days due to Optimistic rollup challenge period.
    routes.push(BridgeRoute {
        from_chain: Chain::Ethereum,
        to_chain: Chain::Arbitrum,
        token: "ETH".into(),
        provider: BridgeProvider::NativeBridge,
        estimated_time_secs: 180,    // ~3 min
        fee_usd: Decimal::new(200, 2), // $2.00
    });
    routes.push(BridgeRoute {
        from_chain: Chain::Arbitrum,
        to_chain: Chain::Ethereum,
        token: "ETH".into(),
        provider: BridgeProvider::NativeBridge,
        estimated_time_secs: 604_800, // 7 days
        fee_usd: Decimal::new(150, 2), // $1.50
    });

    // ── Ethereum ↔ Base (native bridge) ─────────────────────────────────────
    routes.push(BridgeRoute {
        from_chain: Chain::Ethereum,
        to_chain: Chain::Base,
        token: "ETH".into(),
        provider: BridgeProvider::NativeBridge,
        estimated_time_secs: 180,
        fee_usd: Decimal::new(200, 2),
    });
    routes.push(BridgeRoute {
        from_chain: Chain::Base,
        to_chain: Chain::Ethereum,
        token: "ETH".into(),
        provider: BridgeProvider::NativeBridge,
        estimated_time_secs: 604_800,
        fee_usd: Decimal::new(150, 2),
    });

    // ── USDC: any EVM ↔ any EVM via Circle CCTP ─────────────────────────────
    // CCTP burns on source and mints on destination; typical finality ~20 min.
    let cctp_chains = [
        Chain::Ethereum,
        Chain::Arbitrum,
        Chain::Base,
        Chain::Polygon,
        Chain::Avalanche,
        Chain::Optimism,
    ];
    for &from in &cctp_chains {
        for &to in &cctp_chains {
            if from != to {
                routes.push(BridgeRoute {
                    from_chain: from,
                    to_chain: to,
                    token: "USDC".into(),
                    provider: BridgeProvider::CircleCctp,
                    estimated_time_secs: 1_200, // ~20 min
                    fee_usd: Decimal::new(50, 2), // $0.50
                });
            }
        }
    }

    // ── General tokens: LayerZero routes ─────────────────────────────────────
    // Connect all EVM chains bidirectionally for USDT.
    let lz_chains = [
        Chain::Ethereum,
        Chain::Arbitrum,
        Chain::Base,
        Chain::Polygon,
        Chain::Bsc,
        Chain::Avalanche,
        Chain::Optimism,
    ];
    for &from in &lz_chains {
        for &to in &lz_chains {
            if from != to {
                routes.push(BridgeRoute {
                    from_chain: from,
                    to_chain: to,
                    token: "USDT".into(),
                    provider: BridgeProvider::LayerZero,
                    estimated_time_secs: 300, // ~5 min
                    fee_usd: Decimal::new(75, 2), // $0.75
                });
            }
        }
    }

    // ── Solana ↔ EVM: Wormhole ───────────────────────────────────────────────
    for &evm in &[Chain::Ethereum, Chain::Arbitrum, Chain::Base] {
        for &token in &["SOL", "USDC", "USDT"] {
            routes.push(BridgeRoute {
                from_chain: Chain::Solana,
                to_chain: evm,
                token: token.into(),
                provider: BridgeProvider::Wormhole,
                estimated_time_secs: 900, // ~15 min
                fee_usd: Decimal::new(100, 2), // $1.00
            });
            routes.push(BridgeRoute {
                from_chain: evm,
                to_chain: Chain::Solana,
                token: token.into(),
                provider: BridgeProvider::Wormhole,
                estimated_time_secs: 900,
                fee_usd: Decimal::new(100, 2),
            });
        }
    }

    routes
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn native_bridge_eth_to_arbitrum() {
        let mgr = BridgeManager::new();
        let routes = mgr.get_routes(Chain::Ethereum, Chain::Arbitrum, "ETH");
        assert!(!routes.is_empty(), "must have ETH Ethereum→Arbitrum route");
    }

    #[test]
    fn cctp_usdc_any_to_any() {
        let mgr = BridgeManager::new();
        let routes = mgr.get_routes(Chain::Polygon, Chain::Avalanche, "USDC");
        assert!(!routes.is_empty(), "USDC CCTP Polygon→Avalanche must exist");
        assert!(routes.iter().any(|r| r.provider == BridgeProvider::CircleCctp));
    }

    #[test]
    fn layerzero_usdt_all_evm_pairs() {
        let mgr = BridgeManager::new();
        let routes = mgr.get_routes(Chain::Bsc, Chain::Optimism, "USDT");
        assert!(!routes.is_empty(), "USDT LayerZero BSC→Optimism must exist");
    }

    #[test]
    fn wormhole_sol_to_eth() {
        let mgr = BridgeManager::new();
        let routes = mgr.get_routes(Chain::Solana, Chain::Ethereum, "SOL");
        assert!(!routes.is_empty(), "SOL Solana→Ethereum Wormhole must exist");
    }

    #[test]
    fn get_quote_picks_best_fee() {
        let mgr = BridgeManager::new();
        // USDC Ethereum→Arbitrum: both CCTP and LayerZero routes exist.
        // Quote should return the lower-fee one.
        let quote = mgr.get_quote(Chain::Ethereum, Chain::Arbitrum, "USDC", dec!(1000)).unwrap();
        assert!(quote.amount_out > Decimal::ZERO);
        assert!(quote.fee > Decimal::ZERO);
        assert_eq!(quote.amount_in, dec!(1000));
    }

    #[test]
    fn get_quote_rejects_zero_amount() {
        let mgr = BridgeManager::new();
        let err = mgr.get_quote(Chain::Ethereum, Chain::Arbitrum, "ETH", Decimal::ZERO)
            .unwrap_err();
        assert!(err.to_string().contains("amount must be positive"));
    }

    #[test]
    fn get_quote_fails_for_unknown_route() {
        let mgr = BridgeManager::new();
        let err = mgr.get_quote(Chain::Solana, Chain::Bsc, "UNKNOWN_TOKEN", dec!(1))
            .unwrap_err();
        assert!(err.to_string().contains("no bridge route"));
    }

    #[tokio::test]
    async fn execute_bridge_returns_confirmed_tx() {
        let mgr = BridgeManager::new();
        // Use 1000 USDC so the bridge fee ($0.50) is well below the transfer amount.
        let quote = mgr.get_quote(Chain::Ethereum, Chain::Arbitrum, "USDC", dec!(1000)).unwrap();
        let user_id = Uuid::new_v4();
        let tx = mgr.execute_bridge(&quote, user_id, "0xfrom", "0xto").await.unwrap();

        assert_eq!(tx.tx_type, TxType::Bridge);
        assert!(matches!(tx.status, TxStatus::Confirmed));
        assert!(tx.tx_hash.is_some());
    }

    #[tokio::test]
    async fn execute_expired_quote_fails() {
        let mgr = BridgeManager::new();
        let mut quote = mgr.get_quote(Chain::Ethereum, Chain::Arbitrum, "ETH", dec!(1)).unwrap();
        // Force expiry.
        quote.expires_at = Utc::now() - Duration::seconds(1);
        let err = mgr.execute_bridge(&quote, Uuid::new_v4(), "0xfrom", "0xto").await.unwrap_err();
        assert!(err.to_string().contains("expired"));
    }
}
