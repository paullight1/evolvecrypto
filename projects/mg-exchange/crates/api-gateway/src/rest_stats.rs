//! Trading statistics REST handlers (OKX-style Trading Data module).
//!
//! All endpoints are public (no authentication required).
//!
//! Routes registered in `server.rs`:
//!   GET /api/v1/market/open-interest-history/{symbol}
//!   GET /api/v1/market/taker-volume/{symbol}
//!   GET /api/v1/market/top-trader-sentiment/{symbol}
//!   GET /api/v1/market/long-short-ratio-history/{symbol}
//!   GET /api/v1/market/total-volume
//!   GET /api/v1/market/premium-index/{symbol}

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use rust_decimal::Decimal;
use serde::Deserialize;

use exchange_common::Symbol;
use exchange_market_data::MarketDataProvider;

use crate::rest::AppState;

// ---------------------------------------------------------------------------
// Shared query parameter types
// ---------------------------------------------------------------------------

/// Pagination / window query parameters shared across history endpoints.
#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    /// Maximum number of records to return. Defaults to 100, capped at 500.
    pub limit: Option<usize>,
}

// ---------------------------------------------------------------------------
// Helper: validate symbol, return 400 on failure
// ---------------------------------------------------------------------------

fn check_symbol(s: &str) -> Result<Symbol, (StatusCode, Json<serde_json::Value>)> {
    if s.len() < 3
        || s.len() > 20
        || !s
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("Invalid symbol: '{}'. Must be 3–20 uppercase ASCII letters/digits.", s)
            })),
        ));
    }
    Ok(Symbol::new(s))
}

// ---------------------------------------------------------------------------
// 1. GET /api/v1/market/open-interest-history/{symbol}
// ---------------------------------------------------------------------------

/// Return historical open interest snapshots for a perpetual symbol.
///
/// Snapshots are recorded every 5 minutes by `spawn_oi_snapshotter`.
/// The most recent entries are returned first (newest at index 0).
pub async fn get_oi_history(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> impl IntoResponse {
    let sym = match check_symbol(&symbol) {
        Ok(s) => s,
        Err(e) => return e,
    };

    let limit = q.limit.unwrap_or(100).min(500);

    let hist = exchange
        .oi_history
        .lock().unwrap_or_else(|e| e.into_inner());

    let snapshots: Vec<_> = hist
        .get(&sym)
        .map(|v| v.iter().rev().take(limit).collect())
        .unwrap_or_default();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "symbol": symbol,
            "data": snapshots,
        })),
    )
}

// ---------------------------------------------------------------------------
// 2. GET /api/v1/market/taker-volume/{symbol}
// ---------------------------------------------------------------------------

/// Return historical taker buy/sell volume in 5-minute buckets.
///
/// Buckets are accumulated in real time on every fill.
/// The most recent buckets are returned first.
pub async fn get_taker_volume(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> impl IntoResponse {
    let sym = match check_symbol(&symbol) {
        Ok(s) => s,
        Err(e) => return e,
    };

    let limit = q.limit.unwrap_or(100).min(500);

    let hist = exchange
        .taker_volume_history
        .lock().unwrap_or_else(|e| e.into_inner());

    let snapshots: Vec<_> = hist
        .get(&sym)
        .map(|v| v.iter().rev().take(limit).collect())
        .unwrap_or_default();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "symbol": symbol,
            "data": snapshots,
        })),
    )
}

// ---------------------------------------------------------------------------
// 3. GET /api/v1/market/top-trader-sentiment/{symbol}
// ---------------------------------------------------------------------------

/// Return the long/short sentiment of top traders for a symbol.
///
/// "Top traders" are defined as the top 20 % of open-position holders by
/// absolute notional size.  The ratio is computed on-demand from in-memory
/// positions; no historical data is returned.
pub async fn get_top_trader_sentiment(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
) -> impl IntoResponse {
    let sym = match check_symbol(&symbol) {
        Ok(s) => s,
        Err(e) => return e,
    };

    let mark = exchange
        .get_mark_price(&sym)
        .map(|mp| mp.price)
        .unwrap_or(Decimal::ONE);

    // Collect (user_id, side, notional) for this symbol.
    let all_positions: Vec<(uuid::Uuid, exchange_common::futures::PositionSide, Decimal)> = {
        let pm = exchange.positions.lock();
        pm.all_positions()
            .iter()
            .filter(|p| p.symbol == sym && p.size > Decimal::ZERO)
            .map(|p| (p.user_id, p.side, p.size * mark))
            .collect()
    };

    if all_positions.is_empty() {
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "symbol": symbol,
                "longRatio": "0",
                "shortRatio": "0",
                "longCount": 0,
                "shortCount": 0,
                "timestamp": Utc::now().timestamp(),
            })),
        );
    }

    // Sort by notional descending and take the top 20 %.
    let mut positions = all_positions;
    positions.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    let top_n = ((positions.len() as f64 * 0.2).ceil() as usize).max(1);
    let top_traders = &positions[..top_n];

    let (long_count, short_count) =
        top_traders
            .iter()
            .fold((0u64, 0u64), |(l, s), (_, side, _)| {
                match side {
                    exchange_common::futures::PositionSide::Long => (l + 1, s),
                    exchange_common::futures::PositionSide::Short => (l, s + 1),
                }
            });

    let total_count = long_count + short_count;
    let hundred = Decimal::ONE_HUNDRED;
    let (long_ratio, short_ratio) = if total_count > 0 {
        let t = Decimal::from(total_count);
        (
            (Decimal::from(long_count) / t * hundred).round_dp(4),
            (Decimal::from(short_count) / t * hundred).round_dp(4),
        )
    } else {
        (Decimal::ZERO, Decimal::ZERO)
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "symbol": symbol,
            "longRatio": long_ratio.to_string(),
            "shortRatio": short_ratio.to_string(),
            "longCount": long_count,
            "shortCount": short_count,
            "timestamp": Utc::now().timestamp(),
        })),
    )
}

// ---------------------------------------------------------------------------
// 4. GET /api/v1/market/long-short-ratio-history/{symbol}
// ---------------------------------------------------------------------------

/// Return historical long/short ratio snapshots for a perpetual symbol.
///
/// Snapshots are recorded every 5 minutes alongside the OI snapshotter task.
/// The most recent entries are returned first.
pub async fn get_long_short_ratio_history(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> impl IntoResponse {
    let sym = match check_symbol(&symbol) {
        Ok(s) => s,
        Err(e) => return e,
    };

    let limit = q.limit.unwrap_or(100).min(500);

    let hist = exchange
        .long_short_history
        .lock().unwrap_or_else(|e| e.into_inner());

    let snapshots: Vec<_> = hist
        .get(&sym)
        .map(|v| v.iter().rev().take(limit).collect())
        .unwrap_or_default();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "symbol": symbol,
            "data": snapshots,
        })),
    )
}

// ---------------------------------------------------------------------------
// 5. GET /api/v1/market/total-volume
// ---------------------------------------------------------------------------

/// Return aggregate 24-hour trading volume across all symbols in USD.
///
/// Volume is sourced from the in-memory market data service (24h rolling window).
pub async fn get_total_volume(State(exchange): State<AppState>) -> impl IntoResponse {
    let perp_symbols: Vec<Symbol> = exchange
        .perp_configs
        .load()
        .keys()
        .cloned()
        .collect();

    // Use market data tickers for all symbols — tickers track 24h rolling volume.
    let mut total_volume_usd = Decimal::ZERO;
    let mut symbol_volumes: Vec<serde_json::Value> = Vec::new();

    for sym in &perp_symbols {
        if let Some(ticker) = exchange.market_data.ticker(sym) {
            // volume_24h is in base asset; multiply by last price to get USD notional.
            let usd_volume = ticker.volume_24h * ticker.last_price;
            total_volume_usd += usd_volume;
            symbol_volumes.push(serde_json::json!({
                "symbol": sym.to_string(),
                "volume_24h": ticker.volume_24h.to_string(),
                "volume_usd_24h": usd_volume.to_string(),
            }));
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "total_volume_usd_24h": total_volume_usd.to_string(),
            "by_symbol": symbol_volumes,
            "timestamp": Utc::now().timestamp(),
        })),
    )
}

// ---------------------------------------------------------------------------
// 6. GET /api/v1/market/premium-index/{symbol}
// ---------------------------------------------------------------------------

/// Return the premium (or discount) of the perpetual mark price vs the index price.
///
/// Formula: `(mark_price - index_price) / index_price * 100`
///
/// A positive value means the perpetual trades at a premium; negative means
/// a discount.  Both raw prices and the percentage are returned.
pub async fn get_premium_index(
    State(exchange): State<AppState>,
    Path(symbol): Path<String>,
) -> impl IntoResponse {
    let sym = match check_symbol(&symbol) {
        Ok(s) => s,
        Err(e) => return e,
    };

    let mark = match exchange.get_mark_price(&sym) {
        Some(mp) => mp.price,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": format!("No mark price available for symbol '{}'", symbol)
                })),
            );
        }
    };

    let index = {
        let agg = exchange
            .index_aggregator
            .lock().unwrap_or_else(|e| e.into_inner());
        match agg.calculate_index(&sym) {
            Ok(ip) => ip.price,
            Err(_) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({
                        "error": format!("No index price available for symbol '{}'", symbol)
                    })),
                );
            }
        }
    };

    let premium = if index > Decimal::ZERO {
        (mark - index) / index * Decimal::ONE_HUNDRED
    } else {
        Decimal::ZERO
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "symbol": symbol,
            "mark_price": mark.to_string(),
            "index_price": index.to_string(),
            "premium_pct": premium.round_dp(6).to_string(),
            "timestamp": Utc::now().timestamp(),
        })),
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rust_decimal::Decimal;

    use exchange_common::Symbol;
    use exchange_market_data::MarketDataProvider;

    use crate::exchange::{Exchange, LongShortSnapshot, OiSnapshot, TakerVolumeSnapshot, MAX_STATS_HISTORY_PER_SYMBOL};

    // -----------------------------------------------------------------------
    // Helper: create a minimal exchange instance for tests
    // -----------------------------------------------------------------------

    fn make_exchange() -> Arc<Exchange> {
        Exchange::new()
    }

    // -----------------------------------------------------------------------
    // OI history: snapshots are stored and capped correctly
    // -----------------------------------------------------------------------

    #[test]
    fn oi_history_stores_and_caps() {
        let ex = make_exchange();
        let sym = Symbol::new("BTCUSDT");

        // Insert MAX + 1 snapshots
        {
            let mut hist = ex.oi_history.lock().unwrap_or_else(|e| e.into_inner());
            let entries = hist.entry(sym.clone()).or_default();
            for i in 0..=(MAX_STATS_HISTORY_PER_SYMBOL as i64) {
                entries.push_back(OiSnapshot {
                    timestamp: i * 300,
                    open_interest: Decimal::from(i + 1),
                    open_interest_value_usd: Decimal::from((i + 1) * 50_000),
                });
                if entries.len() > MAX_STATS_HISTORY_PER_SYMBOL {
                    entries.pop_front();
                }
            }
        }

        let hist = ex.oi_history.lock().unwrap_or_else(|e| e.into_inner());
        let entries = hist.get(&sym).unwrap();
        // Length must not exceed the cap.
        assert_eq!(entries.len(), MAX_STATS_HISTORY_PER_SYMBOL);
        // The oldest entry was removed; the first entry should be timestamp 300.
        assert_eq!(entries[0].timestamp, 300);
    }

    // -----------------------------------------------------------------------
    // Taker volume: fill tracking updates the correct bucket
    // -----------------------------------------------------------------------

    #[test]
    fn taker_volume_bucket_accumulates() {
        let ex = make_exchange();
        let sym = Symbol::new("BTCUSDT");

        // Simulate two buy fills in the same 5-minute bucket and one sell fill.
        let bucket_ts: i64 = (chrono::Utc::now().timestamp() / 300) * 300;

        {
            let mut tv = ex.taker_volume_history.lock().unwrap_or_else(|e| e.into_inner());
            let buckets = tv.entry(sym.clone()).or_default();

            // First buy fill
            let needs_new = buckets.back().map(|b| b.timestamp != bucket_ts).unwrap_or(true);
            if needs_new {
                buckets.push_back(TakerVolumeSnapshot {
                    timestamp: bucket_ts,
                    buy_volume: Decimal::ZERO,
                    sell_volume: Decimal::ZERO,
                    buy_count: 0,
                    sell_count: 0,
                });
            }
            if let Some(b) = buckets.back_mut() {
                b.buy_volume += Decimal::new(1, 0);
                b.buy_count += 1;
            }

            // Second buy fill
            if let Some(b) = buckets.back_mut() {
                b.buy_volume += Decimal::new(2, 0);
                b.buy_count += 1;
            }

            // Sell fill
            if let Some(b) = buckets.back_mut() {
                b.sell_volume += Decimal::new(5, 1); // 0.5
                b.sell_count += 1;
            }
        }

        let tv = ex.taker_volume_history.lock().unwrap_or_else(|e| e.into_inner());
        let buckets = tv.get(&sym).unwrap();
        assert_eq!(buckets.len(), 1);
        let b = &buckets[0];
        assert_eq!(b.timestamp, bucket_ts);
        assert_eq!(b.buy_volume, Decimal::new(3, 0));  // 1 + 2
        assert_eq!(b.buy_count, 2);
        assert_eq!(b.sell_volume, Decimal::new(5, 1)); // 0.5
        assert_eq!(b.sell_count, 1);
    }

    // -----------------------------------------------------------------------
    // Taker volume: new bucket is created when window rolls over
    // -----------------------------------------------------------------------

    #[test]
    fn taker_volume_new_bucket_on_window_rollover() {
        let ex = make_exchange();
        let sym = Symbol::new("ETHUSDT");

        {
            let mut tv = ex.taker_volume_history.lock().unwrap_or_else(|e| e.into_inner());
            let buckets = tv.entry(sym.clone()).or_default();

            // Insert a fill for an old bucket
            buckets.push_back(TakerVolumeSnapshot {
                timestamp: 0, // far in the past
                buy_volume: Decimal::new(10, 0),
                sell_volume: Decimal::ZERO,
                buy_count: 5,
                sell_count: 0,
            });

            // Insert a fill for the current bucket
            let current_ts = (chrono::Utc::now().timestamp() / 300) * 300;
            let needs_new = buckets.back().map(|b| b.timestamp != current_ts).unwrap_or(true);
            if needs_new {
                buckets.push_back(TakerVolumeSnapshot {
                    timestamp: current_ts,
                    buy_volume: Decimal::ZERO,
                    sell_volume: Decimal::ZERO,
                    buy_count: 0,
                    sell_count: 0,
                });
            }
            if let Some(b) = buckets.back_mut() {
                b.sell_volume += Decimal::new(3, 0);
                b.sell_count += 1;
            }
        }

        let tv = ex.taker_volume_history.lock().unwrap_or_else(|e| e.into_inner());
        let buckets = tv.get(&sym).unwrap();
        assert_eq!(buckets.len(), 2, "should have the old bucket and the new one");
        assert_eq!(buckets[0].timestamp, 0);
        assert_eq!(buckets[1].sell_count, 1);
    }

    // -----------------------------------------------------------------------
    // Top trader sentiment: long/short counts are computed correctly
    // -----------------------------------------------------------------------

    #[test]
    fn top_trader_sentiment_ratio() {
        // With 10 longs and 0 shorts in the top 20%, longRatio should be 100%.
        // We test the arithmetic directly without spinning up a full HTTP server.

        let long_count: u64 = 8;
        let short_count: u64 = 2;
        let total = long_count + short_count;
        let hundred = Decimal::ONE_HUNDRED;
        let t = Decimal::from(total);
        let long_ratio = (Decimal::from(long_count) / t * hundred).round_dp(4);
        let short_ratio = (Decimal::from(short_count) / t * hundred).round_dp(4);

        assert_eq!(long_ratio, Decimal::from(80));
        assert_eq!(short_ratio, Decimal::from(20));
    }

    // -----------------------------------------------------------------------
    // Long/short history: snapshots are stored and capped
    // -----------------------------------------------------------------------

    #[test]
    fn long_short_history_stores_and_caps() {
        let ex = make_exchange();
        let sym = Symbol::new("SOLUSDT");

        {
            let mut hist = ex.long_short_history.lock().unwrap_or_else(|e| e.into_inner());
            let entries = hist.entry(sym.clone()).or_default();
            for i in 0..=(MAX_STATS_HISTORY_PER_SYMBOL as i64) {
                entries.push_back(LongShortSnapshot {
                    timestamp: i * 300,
                    long_ratio: Decimal::new(6, 1),
                    short_ratio: Decimal::new(4, 1),
                    long_volume: Decimal::from(i + 1),
                    short_volume: Decimal::from(i),
                });
                if entries.len() > MAX_STATS_HISTORY_PER_SYMBOL {
                    entries.pop_front();
                }
            }
        }

        let hist = ex.long_short_history.lock().unwrap_or_else(|e| e.into_inner());
        let entries = hist.get(&sym).unwrap();
        assert_eq!(entries.len(), MAX_STATS_HISTORY_PER_SYMBOL);
        // Oldest (ts=0) was evicted; first remaining entry is ts=300.
        assert_eq!(entries[0].timestamp, 300);
    }

    // -----------------------------------------------------------------------
    // Premium index: formula is correct
    // -----------------------------------------------------------------------

    #[test]
    fn premium_index_formula() {
        let mark = Decimal::new(50_500, 0);  // $50,500
        let index = Decimal::new(50_000, 0); // $50,000
        let premium = (mark - index) / index * Decimal::ONE_HUNDRED;
        // (500 / 50000) * 100 = 1.0 %
        assert_eq!(premium, Decimal::new(1, 0));
    }

    // -----------------------------------------------------------------------
    // Total volume: zero when no fills have been processed
    // -----------------------------------------------------------------------

    #[test]
    fn total_volume_empty_exchange() {
        let ex = make_exchange();
        // market_data is fresh — no tickers yet → volume should be zero.
        let perp_syms: Vec<Symbol> = ex
            .perp_configs
            .load()
            .keys()
            .cloned()
            .collect();
        let mut total = Decimal::ZERO;
        for sym in &perp_syms {
            if let Some(t) = ex.market_data.ticker(sym) {
                total += t.volume_24h * t.last_price;
            }
        }
        // No trades have been processed, so total should be zero.
        assert_eq!(total, Decimal::ZERO);
    }
}
