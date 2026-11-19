//! REST handlers for the Unified Trading Account feature.
//!
//! ## Endpoints
//!
//! - `GET  /api/v1/account/unified`         — full unified account summary
//! - `POST /api/v1/account/upgrade-unified` — switch Classic → Unified
//! - `GET  /api/v1/account/collateral`      — per-asset collateral breakdown
//!
//! ## Account modes
//!
//! Users start in **Classic** mode (separate spot / futures / fund pools).
//! Calling `POST /account/upgrade-unified` merges all balance pools into a
//! single unified pool. The upgrade is one-way: once in Unified mode the user
//! cannot revert to Classic in the current implementation.
//!
//! ## Price data
//!
//! Collateral values require current spot prices. For Phase 1 we derive
//! prices from the exchange's own mark-price index or fall back to hard-coded
//! reference prices when no index price is available.

use std::collections::HashMap;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use rust_decimal::Decimal;
use serde::Serialize;

use exchange_account::{collateral_haircut, AccountMode, UnifiedAccount};
use exchange_market_data::MarketDataProvider;

use crate::auth::AuthenticatedUser;
use crate::rest::AppState;

// ─────────────────────────────────────────────────────────────────────────────
// Shared helpers
// ─────────────────────────────────────────────────────────────────────────────

fn ok_json(v: impl Serialize) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::OK,
        Json(serde_json::to_value(v).unwrap_or(serde_json::Value::Null)),
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Price resolution
// ─────────────────────────────────────────────────────────────────────────────

/// Build an asset → USD price map from the exchange's 24h tickers.
///
/// For each known USDT-quoted trading pair (e.g. "BTCUSDT") we use the last
/// trade price as the spot price for the base asset (e.g. "BTC"). Stablecoins
/// are always fixed at 1.00. EXC is included with price 0 so its entry appears
/// in the collateral table (with zero collateral value due to the haircut).
fn resolve_prices(exchange: &AppState) -> HashMap<String, Decimal> {
    let mut prices: HashMap<String, Decimal> = HashMap::new();

    // Stablecoins are always 1 USD.
    for stable in &["USDT", "USDC", "BUSD", "DAI", "FDUSD"] {
        prices.insert((*stable).to_string(), Decimal::ONE);
    }

    // Derive prices from 24h tickers for non-stablecoin assets.
    for ticker in exchange.market_data.all_tickers() {
        let sym = ticker.symbol.as_str();
        let base = strip_quote_suffix(sym);
        if base.is_empty() || base == sym {
            continue;
        }
        let price = ticker.last_price;
        if price > Decimal::ZERO {
            prices.entry(base.to_string()).or_insert(price);
        }
    }

    // EXC is included for display (haircut = 0, collateral value = 0).
    prices.entry("EXC".to_string()).or_insert(Decimal::ZERO);

    prices
}

/// Strip common quote-currency suffixes from a symbol to get the base asset.
///
/// "BTCUSDT" → "BTC", "ETHUSDT" → "ETH", "BTCUSD" → "BTC".
fn strip_quote_suffix(symbol: &str) -> &str {
    const SUFFIXES: &[&str] = &["USDT", "USDC", "BUSD", "USD"];
    for suffix in SUFFIXES {
        if let Some(base) = symbol.strip_suffix(suffix) {
            if !base.is_empty() {
                return base;
            }
        }
    }
    symbol
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /api/v1/account/unified
// ─────────────────────────────────────────────────────────────────────────────

/// Response body for the unified account summary endpoint.
#[derive(Debug, Serialize)]
pub struct UnifiedAccountResponse {
    /// User's account mode ("classic" or "unified").
    pub mode: String,
    /// Total collateral value in USD (after haircuts).
    pub total_collateral_usd: Decimal,
    /// Total equity in USD (before haircuts — raw mark-to-market).
    pub total_equity_usd: Decimal,
    /// Sum of initial margin requirements across all open positions.
    pub total_initial_margin: Decimal,
    /// Sum of maintenance margin requirements across all open positions.
    pub total_maintenance_margin: Decimal,
    /// USD available for opening new positions or placing new orders.
    pub available_balance_usd: Decimal,
    /// Current margin utilisation ratio (0 = none, 1 = fully used, >1 = over-margin).
    pub margin_ratio: Decimal,
    /// Whether this account is at risk of liquidation.
    pub is_liquidatable: bool,
    /// Per-asset breakdown.
    pub assets: Vec<AssetBalanceItem>,
}

/// One row in the per-asset collateral table.
#[derive(Debug, Serialize)]
pub struct AssetBalanceItem {
    pub asset: String,
    pub wallet_balance: Decimal,
    pub locked_in_orders: Decimal,
    pub unrealized_pnl: Decimal,
    pub equity: Decimal,
    pub price_usd: Decimal,
    pub collateral_value_usd: Decimal,
    pub haircut: Decimal,
}

/// `GET /api/v1/account/unified`
///
/// Returns the unified account summary for the authenticated user.
/// In Classic mode the response reflects the trading account balance only;
/// in Unified mode it covers all merged pools with full collateral computation.
pub async fn get_unified_account(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;

    let prices = resolve_prices(&exchange);

    // --- Get or construct the account snapshot ---

    let unified_accounts = exchange.unified_accounts.lock().unwrap_or_else(|e| e.into_inner());

    let (mode_str, account) = if let Some(ua) = unified_accounts.get(&user_id) {
        // Unified mode: return the stored (periodically refreshed) account.
        // Clone it so we can release the lock before serialising.
        ("unified".to_string(), ua.clone())
    } else {
        // Classic mode: synthesise a read-only snapshot from trading balances.
        drop(unified_accounts);
        let user_balances = exchange.accounts.get_all_balances(user_id);
        let mut ua = UnifiedAccount::new(user_id, AccountMode::Classic);

        for (asset, bal) in &user_balances {
            ua.set_wallet_balance(
                asset.as_str(),
                bal.total(),
                bal.locked(),
                Decimal::ZERO, // unrealised PnL not tracked per-asset in Classic
            );
        }

        // Compute position margins for this user.
        let (initial_margin, maintenance_margin) = compute_user_margins(&exchange, user_id, &prices);
        ua.calculate_equity(&prices, initial_margin, maintenance_margin);

        ("classic".to_string(), ua)
    };

    // --- Build response ---

    let assets: Vec<AssetBalanceItem> = account
        .assets
        .values()
        .map(|ab| AssetBalanceItem {
            asset: ab.asset.clone(),
            wallet_balance: ab.wallet_balance,
            locked_in_orders: ab.locked_in_orders,
            unrealized_pnl: ab.unrealized_pnl,
            equity: ab.equity,
            price_usd: prices.get(&ab.asset).copied().unwrap_or(Decimal::ZERO),
            collateral_value_usd: ab.collateral_value_usd,
            haircut: ab.haircut,
        })
        .collect();

    ok_json(UnifiedAccountResponse {
        mode: mode_str,
        total_collateral_usd: account.total_collateral_usd,
        total_equity_usd: account.total_equity_usd,
        total_initial_margin: account.total_initial_margin,
        total_maintenance_margin: account.total_maintenance_margin,
        available_balance_usd: account.available_balance_usd,
        margin_ratio: account.margin_ratio(),
        is_liquidatable: account.is_liquidatable(),
        assets,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /api/v1/account/upgrade-unified
// ─────────────────────────────────────────────────────────────────────────────

/// `POST /api/v1/account/upgrade-unified`
///
/// Switch the authenticated user's account from Classic to Unified mode.
///
/// On upgrade:
/// - The user's trading account, futures margin, and fund balances are merged
///   into a single pool.
/// - All future margin checks use the unified collateral model.
/// - This action is one-way: downgrading is not supported in Phase 1.
///
/// Returns `409 Conflict` if the user is already in Unified mode.
pub async fn upgrade_to_unified(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;

    // Guard: already upgraded?
    {
        let unified_accounts = exchange.unified_accounts.lock().unwrap_or_else(|e| e.into_inner());
        if unified_accounts.contains_key(&user_id) {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "account is already in unified mode"
                })),
            );
        }
    }

    // --- Snapshot the three legacy balance pools ---

    let spot_balances: HashMap<String, Decimal> = exchange
        .accounts
        .get_all_balances(user_id)
        .iter()
        .map(|(k, v)| (k.clone(), v.total()))
        .collect();

    let fund_balances: HashMap<String, Decimal> = {
        let fund = exchange.fund_accounts.lock().unwrap_or_else(|e| e.into_inner());
        fund.get(&user_id).cloned().unwrap_or_default()
    };

    // In Phase 1 the "futures balance" is part of the main trading account
    // (the same AccountManager) — there is no separate futures wallet.
    // We pass an empty map here; when Phase 2 separates the pools, pass the
    // actual futures wallet contents instead.
    let futures_balances: HashMap<String, Decimal> = HashMap::new();

    // --- Build unified account ---

    let prices = resolve_prices(&exchange);
    let (initial_margin, maintenance_margin) =
        compute_user_margins(&exchange, user_id, &prices);

    let mut ua = UnifiedAccount::upgrade_to_unified(
        user_id,
        &spot_balances,
        &futures_balances,
        &fund_balances,
    );
    ua.calculate_equity(&prices, initial_margin, maintenance_margin);

    // --- Persist ---

    {
        let mut unified_accounts = exchange
            .unified_accounts
            .lock().unwrap_or_else(|e| e.into_inner());
        unified_accounts.insert(user_id, ua.clone());
    }

    tracing::info!(
        user_id = %user_id,
        total_collateral_usd = %ua.total_collateral_usd,
        "account upgraded to unified mode"
    );

    ok_json(serde_json::json!({
        "message": "account upgraded to unified mode",
        "mode": "unified",
        "total_collateral_usd": ua.total_collateral_usd,
        "available_balance_usd": ua.available_balance_usd,
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /api/v1/account/collateral
// ─────────────────────────────────────────────────────────────────────────────

/// One entry in the collateral table response.
#[derive(Debug, Serialize)]
pub struct CollateralEntry {
    /// Asset ticker (e.g. "BTC", "USDT").
    pub asset: String,
    /// Current spot price in USD used for this calculation.
    pub price_usd: Decimal,
    /// Balance available (wallet balance minus locked orders, before PnL).
    pub balance: Decimal,
    /// Haircut factor (0.00 – 1.00). EXC is always 0.
    pub haircut: Decimal,
    /// Collateral value in USD = balance * price * haircut.
    pub collateral_value_usd: Decimal,
    /// Whether this asset is eligible as collateral (haircut > 0).
    pub is_eligible: bool,
}

/// Summary row appended to the collateral response.
#[derive(Debug, Serialize)]
pub struct CollateralSummary {
    /// Sum of all collateral_value_usd entries.
    pub total_collateral_usd: Decimal,
    /// Current account mode.
    pub account_mode: String,
}

/// Response body for `GET /api/v1/account/collateral`.
#[derive(Debug, Serialize)]
pub struct CollateralResponse {
    pub entries: Vec<CollateralEntry>,
    pub summary: CollateralSummary,
}

/// `GET /api/v1/account/collateral`
///
/// Returns per-asset collateral values and applicable haircut rates for the
/// authenticated user. Useful for understanding how much margin capacity each
/// holding contributes.
pub async fn get_collateral(
    State(exchange): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let user_id = auth.0;

    let prices = resolve_prices(&exchange);

    // Determine account mode and get balances.
    let is_unified = {
        let ua = exchange.unified_accounts.lock().unwrap_or_else(|e| e.into_inner());
        ua.contains_key(&user_id)
    };

    let mode_str = if is_unified { "unified" } else { "classic" };

    // Build collateral entries from the trading account balances.
    let user_balances = exchange.accounts.get_all_balances(user_id);
    let mut entries: Vec<CollateralEntry> = Vec::new();
    let mut total_collateral = Decimal::ZERO;

    for (asset, bal) in &user_balances {
        let available_balance = bal.available(); // excludes locked orders
        let price = prices.get(asset).copied().unwrap_or(Decimal::ZERO);
        let haircut = collateral_haircut(asset);
        let collateral_value = if available_balance > Decimal::ZERO && price > Decimal::ZERO {
            available_balance * price * haircut
        } else {
            Decimal::ZERO
        };

        total_collateral += collateral_value;

        entries.push(CollateralEntry {
            asset: asset.clone(),
            price_usd: price,
            balance: available_balance,
            haircut,
            collateral_value_usd: collateral_value,
            is_eligible: !haircut.is_zero(),
        });
    }

    // Sort entries: eligible first (by collateral value descending), then
    // ineligible (EXC and zero-balance) at the end.
    entries.sort_by(|a, b| {
        b.is_eligible
            .cmp(&a.is_eligible)
            .then(b.collateral_value_usd.cmp(&a.collateral_value_usd))
    });

    ok_json(CollateralResponse {
        entries,
        summary: CollateralSummary {
            total_collateral_usd: total_collateral,
            account_mode: mode_str.to_string(),
        },
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Compute the total initial and maintenance margin for all open positions of
/// `user_id`, expressed in USD.
///
/// For linear perps the margin is already in USDT (≈ USD).
/// For inverse perps the margin is in the base asset and needs a price
/// conversion — we use the mark prices from the prices map.
///
/// Returns `(total_initial_margin_usd, total_maintenance_margin_usd)`.
fn compute_user_margins(
    exchange: &AppState,
    user_id: uuid::Uuid,
    prices: &HashMap<String, Decimal>,
) -> (Decimal, Decimal) {
    use exchange_common::futures::ContractType;
    use exchange_risk_engine::margin_calculator::{
        required_initial_margin, required_maintenance_margin,
        required_initial_margin_inverse, required_maintenance_margin_inverse,
    };

    // Clone the positions to avoid holding the lock while accessing perp_cfgs.
    let user_positions: Vec<exchange_common::futures::Position> = {
        let positions = exchange.positions.lock();
        positions.user_positions(user_id).into_iter().cloned().collect()
    };
    let perp_cfgs = exchange.perp_configs.load();

    let mut total_initial = Decimal::ZERO;
    let mut total_maintenance = Decimal::ZERO;

    for pos in &user_positions {
        let config = match perp_cfgs.get(&pos.symbol) {
            Some(c) => c,
            None => continue,
        };

        let mark_price = prices
            .get(strip_quote_suffix(pos.symbol.as_str()))
            .copied()
            .unwrap_or(pos.entry_price);

        if mark_price.is_zero() || pos.size.is_zero() {
            continue;
        }

        let mmr = config.effective_mmr(pos.notional(mark_price));

        match config.contract_type {
            ContractType::Linear => {
                total_initial += required_initial_margin(pos.size, mark_price, pos.leverage);
                total_maintenance +=
                    required_maintenance_margin(pos.size, mark_price, mmr);
            }
            ContractType::Inverse => {
                let base_asset = strip_quote_suffix(pos.symbol.as_str());
                let base_price = prices
                    .get(base_asset)
                    .copied()
                    .unwrap_or(mark_price);

                // Initial margin is in base asset; convert to USD.
                let im_base = required_initial_margin_inverse(
                    pos.size,
                    mark_price,
                    pos.leverage,
                    config.contract_size,
                );
                total_initial += im_base * base_price;

                // Maintenance margin is in base asset; convert to USD.
                let mm_base = required_maintenance_margin_inverse(
                    pos.size,
                    mark_price,
                    mmr,
                    config.contract_size,
                );
                total_maintenance += mm_base * base_price;
            }
        }
    }

    (total_initial, total_maintenance)
}
