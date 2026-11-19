//! DEX aggregator REST handlers.
//!
//! Endpoints:
//!   GET  /api/v1/dex/chains  — list supported chains and their metadata.
//!   GET  /api/v1/dex/quote   — fetch best DEX quote for a swap.
//!   GET  /api/v1/dex/route   — smart order router decision (CEX vs DEX).
//!   POST /api/v1/dex/swap    — execute a swap (Phase 1: simulated).

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use rust_decimal::Decimal;
use serde::Deserialize;

use exchange_common::Side;
use exchange_dex_aggregator::{
    Chain, DexAggregator, GasOracle, RouteDecision, SmartOrderRouter, SwapExecutor, SwapRequest,
    validate_slippage_bps,
};

use crate::rest::AppState;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn bad_request(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": msg.into()})))
}

fn ok(body: serde_json::Value) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::OK, Json(body))
}

fn parse_chain(s: &str) -> Result<Chain, String> {
    match s.to_lowercase().as_str() {
        "ethereum" | "eth" => Ok(Chain::Ethereum),
        "arbitrum" | "arb" => Ok(Chain::Arbitrum),
        "base" => Ok(Chain::Base),
        "polygon" | "matic" => Ok(Chain::Polygon),
        "bsc" | "bnb" => Ok(Chain::Bsc),
        "solana" | "sol" => Ok(Chain::Solana),
        "avalanche" | "avax" => Ok(Chain::Avalanche),
        "optimism" | "op" => Ok(Chain::Optimism),
        other => Err(format!("unknown chain '{other}'; see GET /api/v1/dex/chains")),
    }
}

// ── GET /api/v1/dex/chains ────────────────────────────────────────────────────

/// GET /api/v1/dex/chains
///
/// Returns metadata for every chain supported by the DEX aggregator.
pub async fn dex_chains(_state: State<AppState>) -> impl IntoResponse {
    let chains = vec![
        chain_info(Chain::Ethereum, "Ethereum", "ETH", Some(1)),
        chain_info(Chain::Arbitrum, "Arbitrum One", "ETH", Some(42161)),
        chain_info(Chain::Base, "Base", "ETH", Some(8453)),
        chain_info(Chain::Polygon, "Polygon", "MATIC", Some(137)),
        chain_info(Chain::Bsc, "BNB Smart Chain", "BNB", Some(56)),
        chain_info(Chain::Solana, "Solana", "SOL", None),
        chain_info(Chain::Avalanche, "Avalanche C-Chain", "AVAX", Some(43114)),
        chain_info(Chain::Optimism, "Optimism", "ETH", Some(10)),
    ];

    ok(serde_json::json!({ "chains": chains }))
}

fn chain_info(
    chain: Chain,
    name: &str,
    native_token: &str,
    chain_id: Option<u64>,
) -> serde_json::Value {
    serde_json::json!({
        "id": chain.label(),
        "name": name,
        "native_token": native_token,
        "chain_id": chain_id,
        "gas_estimate_usd": GasOracle::estimate_gas_usd(chain),
        "providers": supported_providers(chain),
    })
}

fn supported_providers(chain: Chain) -> Vec<&'static str> {
    let mut v = vec![];
    if chain != Chain::Solana {
        v.push("1inch");
        v.push("0x");
    }
    if chain == Chain::Solana {
        v.push("jupiter");
    }
    v
}

// ── GET /api/v1/dex/quote ────────────────────────────────────────────────────

/// Query parameters for `GET /api/v1/dex/quote`.
#[derive(Debug, Deserialize)]
pub struct DexQuoteParams {
    /// Chain identifier (e.g. "ethereum", "arbitrum", "solana").
    pub chain: String,
    /// Input token address or symbol.
    pub token_in: String,
    /// Output token address or symbol.
    pub token_out: String,
    /// Amount of `token_in` to swap.
    pub amount: Decimal,
    /// Slippage tolerance in basis points (1–500). Defaults to 50 (0.5%).
    #[serde(default = "default_slippage")]
    pub slippage_bps: u16,
}

fn default_slippage() -> u16 {
    50
}

/// GET /api/v1/dex/quote?chain=arbitrum&token_in=USDC&token_out=WETH&amount=1000&slippage_bps=50
///
/// Returns the best DEX quote across all capable providers for the given swap.
pub async fn dex_quote(
    _state: State<AppState>,
    Query(params): Query<DexQuoteParams>,
) -> impl IntoResponse {
    if params.amount <= Decimal::ZERO {
        return bad_request("amount must be positive");
    }

    if let Err(e) = validate_slippage_bps(params.slippage_bps) {
        return bad_request(e.to_string());
    }

    let chain = match parse_chain(&params.chain) {
        Ok(c) => c,
        Err(e) => return bad_request(e),
    };

    let agg = DexAggregator::new();
    match agg
        .get_best_quote(
            chain,
            &params.token_in,
            &params.token_out,
            params.amount,
            params.slippage_bps,
        )
        .await
    {
        Ok(quote) => ok(serde_json::json!({
            "provider": quote.provider,
            "chain": quote.chain.label(),
            "token_in": quote.token_in,
            "token_out": quote.token_out,
            "amount_in": quote.amount_in,
            "amount_out": quote.amount_out,
            "price": quote.price,
            "price_impact_pct": quote.price_impact,
            "gas_usd": quote.gas_usd,
            "route": quote.route,
            "expires_at": quote.expires_at,
            "slippage_bps": quote.slippage_bps,
        })),
        Err(e) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

// ── GET /api/v1/dex/route ────────────────────────────────────────────────────

/// Query parameters for `GET /api/v1/dex/route`.
#[derive(Debug, Deserialize)]
pub struct DexRouteParams {
    /// Trading pair symbol (e.g. "ETHUSDT").
    pub symbol: String,
    /// Order side: "buy" or "sell".
    pub side: String,
    /// Order size in base asset units.
    pub amount: Decimal,
    /// Current best CEX price.
    pub cex_price: Decimal,
    /// Total CEX order-book depth available within the spread threshold (USD).
    pub cex_depth_usd: Decimal,
    /// Preferred chain for DEX execution.
    pub chain: String,
}

/// GET /api/v1/dex/route
///
/// Returns the smart order router's venue decision for an order.
pub async fn dex_route(
    _state: State<AppState>,
    Query(params): Query<DexRouteParams>,
) -> impl IntoResponse {
    if params.amount <= Decimal::ZERO {
        return bad_request("amount must be positive");
    }
    if params.cex_price <= Decimal::ZERO {
        return bad_request("cex_price must be positive");
    }

    let side = match params.side.to_lowercase().as_str() {
        "buy" => Side::Buy,
        "sell" => Side::Sell,
        other => return bad_request(format!("unknown side '{other}'; use 'buy' or 'sell'")),
    };

    let chain = match parse_chain(&params.chain) {
        Ok(c) => c,
        Err(e) => return bad_request(e),
    };

    let router = SmartOrderRouter::new(DexAggregator::new());
    match router
        .route(
            &params.symbol,
            side,
            params.amount,
            params.cex_price,
            params.cex_depth_usd,
            chain,
        )
        .await
    {
        Ok(decision) => {
            let body = match &decision {
                RouteDecision::Cex { reason } => serde_json::json!({
                    "venue": "cex",
                    "reason": reason,
                }),
                RouteDecision::Dex { quote, reason } => serde_json::json!({
                    "venue": "dex",
                    "reason": reason,
                    "provider": quote.provider,
                    "chain": quote.chain.label(),
                    "amount_out": quote.amount_out,
                    "gas_usd": quote.gas_usd,
                    "price_impact_pct": quote.price_impact,
                }),
                RouteDecision::Split { cex_pct, dex_quote, reason } => serde_json::json!({
                    "venue": "split",
                    "reason": reason,
                    "cex_pct": cex_pct,
                    "dex_pct": Decimal::from(100u32) - cex_pct,
                    "dex_provider": dex_quote.provider,
                    "dex_chain": dex_quote.chain.label(),
                }),
            };
            ok(body)
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

// ── POST /api/v1/dex/swap ─────────────────────────────────────────────────────

/// Request body for `POST /api/v1/dex/swap`.
#[derive(Debug, serde::Deserialize)]
pub struct DexSwapRequest {
    pub chain: String,
    pub token_in: String,
    pub token_out: String,
    pub amount: Decimal,
    #[serde(default = "default_slippage")]
    pub slippage_bps: u16,
    /// Sender wallet address (required for Phase 2 on-chain submission).
    pub sender: String,
}

/// POST /api/v1/dex/swap
///
/// Fetches the best quote and executes the swap.
/// Phase 1: returns a simulated result with a synthetic transaction hash.
pub async fn dex_swap(
    _state: State<AppState>,
    Json(req): Json<DexSwapRequest>,
) -> impl IntoResponse {
    if req.amount <= Decimal::ZERO {
        return bad_request("amount must be positive");
    }
    if req.sender.is_empty() {
        return bad_request("sender address is required");
    }

    if let Err(e) = validate_slippage_bps(req.slippage_bps) {
        return bad_request(e.to_string());
    }

    let chain = match parse_chain(&req.chain) {
        Ok(c) => c,
        Err(e) => return bad_request(e),
    };

    // Fetch best quote.
    let agg = DexAggregator::new();
    let quote = match agg
        .get_best_quote(chain, &req.token_in, &req.token_out, req.amount, req.slippage_bps)
        .await
    {
        Ok(q) => q,
        Err(e) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        }
    };

    // Execute (simulated in Phase 1).
    let executor = SwapExecutor::new();
    let swap_req = SwapRequest {
        quote,
        sender: req.sender.clone(),
    };

    match executor.execute(swap_req).await {
        Ok(result) => ok(exchange_dex_aggregator::execution::format_swap_result(&result)),
        Err(e) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}
