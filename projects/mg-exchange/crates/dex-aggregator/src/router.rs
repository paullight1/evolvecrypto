//! Smart Order Router (SOR).
//!
//! Compares the CEX order book depth against DEX quotes and decides where to
//! route an incoming order for best execution.
//!
//! Routing rules (in priority order):
//!
//! 1. If CEX has sufficient depth AND spread is tight (< 0.5%):
//!    → route entirely to CEX (zero gas cost, instant settlement).
//!
//! 2. If the DEX net price (after gas) is strictly better than the CEX price:
//!    → route entirely to DEX.
//!
//! 3. If the order is large (> `SPLIT_THRESHOLD_USD`) and CEX can absorb only
//!    part of it without significant market impact:
//!    → split between CEX (liquid portion) and DEX (remainder).
//!
//! 4. Fallback: route to CEX (always available, no gas risk).

use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tracing::debug;

use exchange_common::Side;

use crate::aggregator::DexAggregator;
use crate::gas::GasOracle;
use crate::types::{Chain, DexQuote};

/// Maximum spread (as a fraction) at which we still prefer the CEX.
/// 0.005 = 0.5%.
const CEX_SPREAD_THRESHOLD: Decimal = dec!(0.005);

/// Minimum order size (in USD) before we consider splitting across venues.
/// Below this, gas costs would eat most of any DEX price advantage.
const SPLIT_THRESHOLD_USD: Decimal = dec!(50_000);

/// Minimum price improvement (as a fraction of CEX price) required to route to DEX.
/// We need at least 0.1% improvement to overcome execution risk.
const DEX_MIN_IMPROVEMENT: Decimal = dec!(0.001);

/// The routing decision produced by `SmartOrderRouter::route`.
#[derive(Debug)]
pub enum RouteDecision {
    /// Route the entire order through the CEX order book.
    Cex { reason: String },

    /// Route the entire order through a DEX.
    Dex { quote: DexQuote, reason: String },

    /// Split the order between the CEX and a DEX.
    Split {
        /// Percentage of the order (0–100) to route through the CEX.
        cex_pct: Decimal,
        /// DEX quote for the remaining portion.
        dex_quote: DexQuote,
        reason: String,
    },
}

impl RouteDecision {
    /// Returns a short label for logging and API responses.
    pub fn venue(&self) -> &'static str {
        match self {
            RouteDecision::Cex { .. } => "cex",
            RouteDecision::Dex { .. } => "dex",
            RouteDecision::Split { .. } => "split",
        }
    }
}

/// Routes orders between the CEX order book and on-chain DEX liquidity.
pub struct SmartOrderRouter {
    dex_aggregator: DexAggregator,
}

impl SmartOrderRouter {
    pub fn new(dex_aggregator: DexAggregator) -> Self {
        Self { dex_aggregator }
    }

    /// Decides the optimal routing venue for an order.
    ///
    /// # Arguments
    ///
    /// * `symbol` — trading pair (e.g. "ETHUSDT"), used to derive token addresses.
    /// * `side` — `Buy` or `Sell`.
    /// * `amount` — order size in base asset units.
    /// * `cex_best_price` — best available price on the CEX (top-of-book).
    /// * `cex_available_depth` — total CEX depth available within the spread
    ///   threshold, in USD-equivalent units.
    /// * `chain` — preferred chain for DEX execution.
    ///
    /// Returns `Err` only on aggregator-level failures (e.g. no providers
    /// configured). The happy path always returns a `RouteDecision`.
    pub async fn route(
        &self,
        symbol: &str,
        side: Side,
        amount: Decimal,
        cex_best_price: Decimal,
        cex_available_depth: Decimal,
        chain: Chain,
    ) -> Result<RouteDecision> {
        debug!(
            symbol,
            side = ?side,
            amount = %amount,
            cex_best_price = %cex_best_price,
            cex_depth = %cex_available_depth,
            chain = %chain,
            "smart order router evaluating"
        );

        // Derive approximate token addresses/symbols from the symbol string.
        // Phase 2: look these up in the asset registry.
        let (token_in, token_out) = derive_tokens(symbol, side);

        // Estimate order size in USD (approximate: amount * cex_best_price).
        let order_usd = amount * cex_best_price;

        // ── Rule 1: CEX has enough depth with tight spread ────────────────────
        //
        // If the CEX can absorb the full order within the spread threshold,
        // there is no reason to pay DEX gas costs.
        if cex_available_depth >= order_usd {
            let spread = spread_fraction(cex_best_price);
            if spread <= CEX_SPREAD_THRESHOLD {
                return Ok(RouteDecision::Cex {
                    reason: format!(
                        "CEX depth ${:.0} covers order ${:.0}; spread {:.3}% ≤ threshold {:.1}%",
                        cex_available_depth,
                        order_usd,
                        spread * dec!(100),
                        CEX_SPREAD_THRESHOLD * dec!(100)
                    ),
                });
            }
        }

        // ── Fetch best DEX quote ───────────────────────────────────────────────
        let dex_quote = match self
            .dex_aggregator
            .get_best_quote(chain, token_in, token_out, amount, 50)
            .await
        {
            Ok(q) => q,
            Err(e) => {
                // No DEX quote available — fall back to CEX.
                return Ok(RouteDecision::Cex {
                    reason: format!("DEX quote unavailable ({e}); routing to CEX"),
                });
            }
        };

        // ── Rule 2: DEX offers better net price (after gas) ───────────────────
        let gas_usd = GasOracle::estimate_gas_usd(chain);
        let dex_net_amount_out = dex_quote.amount_out - (gas_usd / cex_best_price);
        let cex_amount_out = amount; // at CEX price, 1:1 in base asset terms

        // Price improvement fraction: how much more of the output asset we get.
        let improvement = if cex_amount_out > Decimal::ZERO {
            (dex_net_amount_out - cex_amount_out) / cex_amount_out
        } else {
            Decimal::ZERO
        };

        if improvement > DEX_MIN_IMPROVEMENT {
            return Ok(RouteDecision::Dex {
                reason: format!(
                    "DEX net output {:.6} vs CEX {:.6} (+{:.3}% after gas ${:.2})",
                    dex_net_amount_out,
                    cex_amount_out,
                    improvement * dec!(100),
                    gas_usd,
                ),
                quote: dex_quote,
            });
        }

        // ── Rule 3: Large order — split if CEX depth is insufficient ──────────
        if order_usd >= SPLIT_THRESHOLD_USD && cex_available_depth < order_usd {
            let cex_pct = if order_usd > Decimal::ZERO {
                (cex_available_depth / order_usd * dec!(100)).min(dec!(100))
            } else {
                dec!(50)
            };

            return Ok(RouteDecision::Split {
                cex_pct,
                dex_quote,
                reason: format!(
                    "large order ${order_usd:.0}; CEX can absorb {cex_pct:.0}% (${cex_available_depth:.0}), DEX handles remainder"
                ),
            });
        }

        // ── Fallback: CEX ─────────────────────────────────────────────────────
        Ok(RouteDecision::Cex {
            reason: format!(
                "DEX improvement {:.4}% below threshold {:.1}%; routing to CEX",
                improvement * dec!(100),
                DEX_MIN_IMPROVEMENT * dec!(100)
            ),
        })
    }
}

/// Derives approximate token identifiers from a trading pair symbol.
///
/// For example "ETHUSDT" → base "ETH", quote "USDT"; the `side` then
/// determines which is `token_in` and which is `token_out`.
///
/// Quotes are tried longest-first so that "USDT" is preferred over "USD"
/// and "USDC" is not accidentally matched by "USD".
///
/// Phase 2 would look up canonical contract addresses from the asset registry.
fn derive_tokens(symbol: &str, side: Side) -> (&'static str, &'static str) {
    // Known quote currencies, ordered longest-first to avoid prefix collisions
    // (e.g. "USDT" before "USD").
    const KNOWN_QUOTES: &[&str] = &["USDT", "USDC", "BUSD", "BTC", "ETH", "USD"];

    let (base, quote) = 'parse: {
        for &q in KNOWN_QUOTES {
            if let Some(b) = symbol.strip_suffix(q) {
                if !b.is_empty() {
                    // We found a valid split — but we can't return `b` directly
                    // because it's a slice of `symbol` (not `'static`). For
                    // Phase 1 we resolve to a known static string; unknown bases
                    // fall through to the fallback below.
                    //
                    // Map the quote slice → &'static str so the caller gets a
                    // 'static lifetime without allocation.
                    let static_quote: &'static str = match q {
                        "USDT" => "USDT",
                        "USDC" => "USDC",
                        "BUSD" => "BUSD",
                        "BTC"  => "BTC",
                        "ETH"  => "ETH",
                        "USD"  => "USD",
                        _      => break 'parse ("ETH", "USDC"),
                    };
                    let static_base: &'static str = match b {
                        "BTC"  => "BTC",
                        "ETH"  => "ETH",
                        "SOL"  => "SOL",
                        "BNB"  => "BNB",
                        "XRP"  => "XRP",
                        "ADA"  => "ADA",
                        "DOGE" => "DOGE",
                        "AVAX" => "AVAX",
                        "MATIC"=> "MATIC",
                        "DOT"  => "DOT",
                        "LINK" => "LINK",
                        "UNI"  => "UNI",
                        "ATOM" => "ATOM",
                        "NEAR" => "NEAR",
                        "LTC"  => "LTC",
                        "TRX"  => "TRX",
                        "OP"   => "OP",
                        "ARB"  => "ARB",
                        "SUI"  => "SUI",
                        _      => break 'parse ("ETH", "USDC"),
                    };
                    break 'parse (static_base, static_quote);
                }
            }
        }
        // Fallback for unrecognised symbols.
        ("ETH", "USDC")
    };

    match side {
        Side::Buy  => (quote, base), // buying base with quote → spend quote, receive base
        Side::Sell => (base, quote), // selling base for quote → spend base, receive quote
    }
}

/// Returns a spread fraction estimate for a given price level.
///
/// Phase 1: uses a fixed 0.1% model. Phase 2 would use the actual bid-ask spread.
fn spread_fraction(_mid_price: Decimal) -> Decimal {
    dec!(0.001) // 0.1% typical CEX spread
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_router() -> SmartOrderRouter {
        SmartOrderRouter::new(DexAggregator::new())
    }

    // ── derive_tokens unit tests ──────────────────────────────────────────

    #[test]
    fn derive_tokens_btcusdt_buy() {
        // Buying BTCUSDT: spend USDT, receive BTC.
        let (token_in, token_out) = derive_tokens("BTCUSDT", Side::Buy);
        assert_eq!(token_in, "USDT");
        assert_eq!(token_out, "BTC");
    }

    #[test]
    fn derive_tokens_btcusdt_sell() {
        // Selling BTCUSDT: spend BTC, receive USDT.
        let (token_in, token_out) = derive_tokens("BTCUSDT", Side::Sell);
        assert_eq!(token_in, "BTC");
        assert_eq!(token_out, "USDT");
    }

    #[test]
    fn derive_tokens_ethusdc_sell() {
        let (token_in, token_out) = derive_tokens("ETHUSDC", Side::Sell);
        assert_eq!(token_in, "ETH");
        assert_eq!(token_out, "USDC");
    }

    #[test]
    fn derive_tokens_solusdt_buy() {
        let (token_in, token_out) = derive_tokens("SOLUSDT", Side::Buy);
        assert_eq!(token_in, "USDT");
        assert_eq!(token_out, "SOL");
    }

    #[test]
    fn derive_tokens_ethbtc_sell() {
        // ETH/BTC pair: quote is BTC.
        let (token_in, token_out) = derive_tokens("ETHBTC", Side::Sell);
        assert_eq!(token_in, "ETH");
        assert_eq!(token_out, "BTC");
    }

    #[test]
    fn derive_tokens_unknown_symbol_falls_back() {
        // An unrecognised symbol should fall back to ETH/USDC rather than panic.
        let (token_in, token_out) = derive_tokens("UNKNOWNXYZ", Side::Buy);
        // Fallback: token_in is USDC (quote), token_out is ETH (base) for Buy.
        assert_eq!(token_in, "USDC");
        assert_eq!(token_out, "ETH");
    }

    #[test]
    fn derive_tokens_empty_base_falls_back() {
        // Symbol that IS a known quote with no base ("USDT") should fall back.
        let (token_in, token_out) = derive_tokens("USDT", Side::Sell);
        assert_eq!(token_in, "ETH");
        assert_eq!(token_out, "USDC");
    }

    // ── routing integration tests ─────────────────────────────────────────

    #[tokio::test]
    async fn routes_to_cex_when_depth_sufficient() {
        let router = make_router();
        // 1 ETH order at $3000; CEX has $5000 depth — should route to CEX.
        let decision = router
            .route("ETHUSDT", Side::Buy, dec!(1), dec!(3000), dec!(5000), Chain::Arbitrum)
            .await
            .unwrap();
        assert_eq!(decision.venue(), "cex");
    }

    #[tokio::test]
    async fn routes_to_cex_when_no_dex_providers_available() {
        use crate::providers::oneinch::OneInchProvider;
        // Only EVM providers; ask for Solana quote → all fail → fallback to CEX.
        let agg = DexAggregator::with_providers(vec![Box::new(OneInchProvider::new())]);
        let router = SmartOrderRouter::new(agg);

        let decision = router
            .route("SOLUSDT", Side::Buy, dec!(100), dec!(150), dec!(5000), Chain::Solana)
            .await
            .unwrap();
        assert_eq!(decision.venue(), "cex");
    }

    #[tokio::test]
    async fn split_for_large_order_with_insufficient_depth() {
        let router = make_router();
        // $100k order on Arbitrum; CEX only has $30k depth.
        let decision = router
            .route(
                "ETHUSDT",
                Side::Sell,
                dec!(33),       // 33 ETH ≈ $100k at $3000
                dec!(3000),
                dec!(30_000),   // only $30k CEX depth
                Chain::Arbitrum,
            )
            .await
            .unwrap();

        // Should be split or DEX, not pure CEX.
        assert_ne!(decision.venue(), "cex");
    }

    #[tokio::test]
    async fn route_decision_venue_labels() {
        let cex = RouteDecision::Cex { reason: "test".into() };
        let dex = RouteDecision::Dex {
            reason: "test".into(),
            quote: DexQuote {
                provider: "1inch".into(),
                chain: Chain::Arbitrum,
                token_in: "A".into(),
                token_out: "B".into(),
                amount_in: dec!(100),
                amount_out: dec!(100),
                price: dec!(1),
                price_impact: dec!(0.01),
                gas_estimate: dec!(0.00004),
                gas_usd: dec!(0.10),
                route: vec![],
                expires_at: chrono::Utc::now() + chrono::Duration::seconds(30),
                slippage_bps: 50,
            },
        };
        let split = RouteDecision::Split {
            cex_pct: dec!(60),
            reason: "test".into(),
            dex_quote: DexQuote {
                provider: "0x".into(),
                chain: Chain::Polygon,
                token_in: "A".into(),
                token_out: "B".into(),
                amount_in: dec!(100),
                amount_out: dec!(100),
                price: dec!(1),
                price_impact: dec!(0.01),
                gas_estimate: dec!(0.5),
                gas_usd: dec!(0.02),
                route: vec![],
                expires_at: chrono::Utc::now() + chrono::Duration::seconds(30),
                slippage_bps: 50,
            },
        };

        assert_eq!(cex.venue(), "cex");
        assert_eq!(dex.venue(), "dex");
        assert_eq!(split.venue(), "split");
    }
}
