//! Auto-Deleverage (ADL) system for perpetual futures.
//!
//! When a liquidation results in a loss that exceeds the insurance fund, the exchange
//! force-closes the most profitable opposing positions to cover the deficit. This is
//! the ADL mechanism used by Bybit and Binance.
//!
//! # Flow
//! 1. Liquidation engine detects a bankrupt position, insurance fund is insufficient.
//! 2. [`AdlRanking::rank_positions`] selects and ranks all opposing positions by a
//!    profitability-leverage score.
//! 3. [`calculate_adl`] walks the ranked list and force-closes enough size to cover
//!    the deficit, filling at `bankruptcy_price` (not mark price).
//! 4. Users can monitor their [`adl_quantile`] (1–5) to decide whether to reduce
//!    voluntarily before they are selected.

use std::collections::HashMap;

use rust_decimal::Decimal;
use tracing::{info, warn};

use exchange_common::{
    futures::{Position, PositionSide},
    types::{Price, Symbol, UserId},
};

// ─── ADL ranking ─────────────────────────────────────────────────────────────

/// A position that has been scored for ADL selection.
#[derive(Debug, Clone)]
pub struct AdlCandidate {
    pub user_id: UserId,
    pub symbol: Symbol,
    pub side: PositionSide,
    /// Position size in base asset.
    pub size: Decimal,
    /// ADL score = `pnl_ratio * effective_leverage`.
    /// Higher = first to be deleveraged.
    pub score: Decimal,
    /// Original entry price (copied for the ADL order record).
    pub entry_price: Price,
    /// Margin allocated (copied for convenience).
    pub margin: Decimal,
    /// Unrealized PnL at the time of ranking.
    pub unrealized_pnl: Decimal,
}

/// Ranks open positions on the **opposite** side of a bankrupt position by
/// their ADL score so the most profitable / most-leveraged are deleveraged first.
#[derive(Debug, Default)]
pub struct AdlRanking;

impl AdlRanking {
    /// Rank all positions on the **opposite** side of `bankrupt_side` for `symbol`.
    ///
    /// Positions with non-positive unrealized PnL (not profitable) are excluded —
    /// there is nothing to deleverage from an underwater position.
    ///
    /// Returns candidates sorted descending by score (highest score first).
    pub fn rank_positions(
        &self,
        positions: &[Position],
        mark_prices: &HashMap<Symbol, Price>,
        bankrupt_side: PositionSide,
        symbol: &Symbol,
    ) -> Vec<AdlCandidate> {
        let target_side = bankrupt_side.opposite();

        let mut candidates: Vec<AdlCandidate> = positions
            .iter()
            .filter(|p| &p.symbol == symbol && p.side == target_side && !p.size.is_zero())
            .filter_map(|p| {
                let &mark = mark_prices.get(&p.symbol)?;
                let upnl = p.unrealized_pnl(mark);
                // Only profitable positions are ADL targets.
                if upnl <= Decimal::ZERO {
                    return None;
                }
                let score = adl_score(p, upnl, mark);
                Some(AdlCandidate {
                    user_id: p.user_id,
                    symbol: p.symbol.clone(),
                    side: p.side,
                    size: p.size,
                    score,
                    entry_price: p.entry_price,
                    margin: p.margin,
                    unrealized_pnl: upnl,
                })
            })
            .collect();

        // Highest score first.
        candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        candidates
    }
}

/// ADL score for a single position.
///
/// `score = pnl_ratio * effective_leverage`
///
/// Where:
/// - `pnl_ratio = unrealized_pnl / margin` (ROE — return on equity)
/// - `effective_leverage = notional / effective_margin`
///   (effective_margin = margin + unrealized_pnl)
///
/// Returns `Decimal::ZERO` if the effective margin is zero or negative (to
/// avoid division by zero or nonsensical scores).
fn adl_score(position: &Position, unrealized_pnl: Decimal, mark_price: Price) -> Decimal {
    if position.margin.is_zero() {
        return Decimal::ZERO;
    }

    let effective_margin = position.margin + unrealized_pnl;
    if effective_margin <= Decimal::ZERO {
        return Decimal::ZERO;
    }

    let pnl_ratio = unrealized_pnl / position.margin;
    let notional = position.notional(mark_price);
    let effective_leverage = notional / effective_margin;

    pnl_ratio * effective_leverage
}

// ─── ADL execution ────────────────────────────────────────────────────────────

/// A force-close order emitted by the ADL engine.
#[derive(Debug, Clone, PartialEq)]
pub struct AdlOrder {
    pub user_id: UserId,
    pub symbol: Symbol,
    /// Quantity to close (≤ candidate's full size).
    pub close_qty: Decimal,
    /// Execution price — the bankrupt position's bankruptcy price, not mark price.
    pub close_price: Price,
    /// The candidate's original entry price (for audit / PnL attribution).
    pub original_entry_price: Price,
}

/// Walk the ranked candidates in order and issue close orders until `deficit` is
/// covered or all candidates are exhausted.
///
/// Each candidate is closed at `bankruptcy_price`. The deficit is measured in
/// quote-asset notional. The engine closes the minimum size needed to cover the
/// remainder of the deficit from each candidate.
///
/// # Arguments
/// - `deficit` — remaining loss not covered by the insurance fund (quote asset).
/// - `candidates` — pre-ranked list from [`AdlRanking::rank_positions`], highest
///   score first.
/// - `bankruptcy_price` — the bankrupt position's bankruptcy price; ADL orders
///   execute at this price.
pub fn calculate_adl(
    deficit: Decimal,
    candidates: &[AdlCandidate],
    bankruptcy_price: Price,
) -> Vec<AdlOrder> {
    if deficit <= Decimal::ZERO || bankruptcy_price <= Decimal::ZERO {
        return Vec::new();
    }

    let mut orders = Vec::new();
    let mut remaining = deficit;

    for candidate in candidates {
        if remaining <= Decimal::ZERO {
            break;
        }
        if candidate.size.is_zero() || bankruptcy_price.is_zero() {
            continue;
        }

        // Notional value of the full candidate position at bankruptcy price.
        let full_notional = candidate.size * bankruptcy_price;

        let close_qty = if full_notional <= remaining {
            // Full close covers less than (or exactly) what remains.
            remaining -= full_notional;
            candidate.size
        } else {
            // Partial close: close only what we need.
            let qty = remaining / bankruptcy_price;
            remaining = Decimal::ZERO;
            qty
        };

        info!(
            user_id = %candidate.user_id,
            symbol = %candidate.symbol,
            side = ?candidate.side,
            close_qty = %close_qty,
            close_price = %bankruptcy_price,
            score = %candidate.score,
            "ADL order emitted",
        );

        orders.push(AdlOrder {
            user_id: candidate.user_id,
            symbol: candidate.symbol.clone(),
            close_qty,
            close_price: bankruptcy_price,
            original_entry_price: candidate.entry_price,
        });
    }

    if remaining > Decimal::ZERO {
        warn!(
            remaining_deficit = %remaining,
            candidates_exhausted = candidates.len(),
            "ADL could not fully cover deficit — no more candidates",
        );
    }

    orders
}

// ─── ADL quantile (indicator) ────────────────────────────────────────────────

/// Compute the Bybit-style ADL indicator (1–5) for a single position.
///
/// The indicator shows where `position` sits in the ranked distribution of all
/// positions on the same side and symbol:
/// - **5** — top 20% by score (highest ADL risk)
/// - **4** — 20–40%
/// - **3** — 40–60%
/// - **2** — 60–80%
/// - **1** — bottom 20% (lowest ADL risk)
///
/// Returns `None` if the position is not profitable (score = 0) or there are no
/// comparable positions.
pub fn adl_quantile(
    position: &Position,
    all_positions: &[Position],
    mark_price: Price,
) -> Option<u8> {
    let own_upnl = position.unrealized_pnl(mark_price);
    if own_upnl <= Decimal::ZERO {
        return None;
    }

    let own_score = adl_score(position, own_upnl, mark_price);
    if own_score <= Decimal::ZERO {
        return None;
    }

    // Collect scores of all profitable positions on the same side and symbol,
    // including `position` itself so the quantile is computed over the full peer group.
    let mut peer_scores: Vec<Decimal> = all_positions
        .iter()
        .filter(|p| {
            p.symbol == position.symbol
                && p.side == position.side
                && !p.size.is_zero()
        })
        .filter_map(|p| {
            let upnl = p.unrealized_pnl(mark_price);
            if upnl <= Decimal::ZERO {
                return None;
            }
            Some(adl_score(p, upnl, mark_price))
        })
        .collect();

    if peer_scores.is_empty() {
        // Only this position — it sits at quantile 5 (only profitable one = ADL target).
        return Some(5);
    }

    peer_scores.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let total = peer_scores.len();
    let rank = peer_scores
        .iter()
        .position(|&s| s <= own_score)
        .unwrap_or(total);

    // rank is 0-indexed from the top (0 = highest score).
    // Convert to quantile 1–5 (1 = safest, 5 = most at risk).
    let quantile = match rank * 5 / total {
        0 => 5,
        1 => 4,
        2 => 3,
        3 => 2,
        _ => 1,
    };

    Some(quantile)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use exchange_common::futures::{MarginMode, Position, PositionSide};
    use rust_decimal_macros::dec;
    use uuid::Uuid;
    use chrono::Utc;

    fn btc() -> Symbol {
        Symbol::new("BTCUSDT")
    }

    fn make_position(
        user_id: u128,
        side: PositionSide,
        size: Decimal,
        entry: Decimal,
        margin: Decimal,
    ) -> Position {
        Position {
            user_id: Uuid::from_u128(user_id),
            symbol: btc(),
            side,
            size,
            entry_price: entry,
            leverage: dec!(10),
            margin_mode: MarginMode::Isolated,
            margin,
            realized_pnl: Decimal::ZERO,
            funding_accumulated: Decimal::ZERO,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            position_idx: 0,
            tp_price: None,
            sl_price: None,
            trailing_stop: None,
            tp_trigger_id: None,
            sl_trigger_id: None,
            tp_size: None,
            sl_size: None,
            auto_add_margin: false,
            unpaid_funding_debt: Decimal::ZERO,
        }
    }

    fn mark_prices(price: Decimal) -> HashMap<Symbol, Price> {
        let mut m = HashMap::new();
        m.insert(btc(), price);
        m
    }

    // ─── Ranking tests ───────────────────────────────────────────────────────

    #[test]
    fn test_rank_most_profitable_first() {
        // Two shorts, both profitable at mark=40_000 (entry 50_000).
        // User 1: size=1, margin=1000  → higher ROE
        // User 2: size=1, margin=5000  → lower ROE
        let positions = vec![
            make_position(1, PositionSide::Short, dec!(1), dec!(50000), dec!(1000)),
            make_position(2, PositionSide::Short, dec!(1), dec!(50000), dec!(5000)),
        ];
        let ranking = AdlRanking::default();
        let candidates = ranking.rank_positions(&positions, &mark_prices(dec!(40000)), PositionSide::Long, &btc());

        assert_eq!(candidates.len(), 2);
        // User 1 has smaller margin → higher pnl_ratio → higher score → ranked first.
        assert_eq!(candidates[0].user_id, Uuid::from_u128(1));
        assert!(candidates[0].score > candidates[1].score);
    }

    #[test]
    fn test_rank_leverage_amplifies_score() {
        // Same ROE (pnl_ratio = 1.0), but different effective leverage.
        // Higher notional relative to effective margin → higher score.
        // User A: size=2, entry=50_000, margin=10_000, mark=55_000
        //   upnl = 2*(55_000-50_000) = 10_000, pnl_ratio = 10_000/10_000 = 1.0
        //   effective_margin = 20_000, notional = 110_000, eff_lev = 5.5, score = 5.5
        // User B: size=1, entry=50_000, margin=5_000, mark=55_000
        //   upnl = 1*(55_000-50_000) = 5_000, pnl_ratio = 5_000/5_000 = 1.0
        //   effective_margin = 10_000, notional = 55_000, eff_lev = 5.5, score = 5.5
        // Both same score — just checking the formula works consistently.
        let positions = vec![
            make_position(1, PositionSide::Long, dec!(2), dec!(50000), dec!(10000)),
            make_position(2, PositionSide::Long, dec!(1), dec!(50000), dec!(5000)),
        ];
        let ranking = AdlRanking::default();
        let candidates = ranking.rank_positions(
            &positions,
            &mark_prices(dec!(55000)),
            PositionSide::Short,
            &btc(),
        );
        assert_eq!(candidates.len(), 2);
        // Both score identically (same pnl_ratio, same effective_leverage).
        assert_eq!(candidates[0].score, candidates[1].score);
    }

    #[test]
    fn test_rank_only_opposite_side() {
        // One long (profitable), one short (profitable) — bankrupt side is Long.
        // Only shorts should be ranked.
        let positions = vec![
            make_position(1, PositionSide::Long, dec!(1), dec!(50000), dec!(5000)),
            make_position(2, PositionSide::Short, dec!(1), dec!(50000), dec!(5000)),
        ];
        let mark = dec!(45000); // Long is losing, Short is winning.
        let ranking = AdlRanking::default();
        let candidates =
            ranking.rank_positions(&positions, &mark_prices(mark), PositionSide::Long, &btc());

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].user_id, Uuid::from_u128(2));
        assert_eq!(candidates[0].side, PositionSide::Short);
    }

    #[test]
    fn test_rank_excludes_zero_profit_positions() {
        // Short position at mark = entry_price → upnl = 0 → excluded.
        let positions = vec![
            make_position(1, PositionSide::Short, dec!(1), dec!(50000), dec!(5000)),
        ];
        let ranking = AdlRanking::default();
        // mark = entry → short neither wins nor loses
        let candidates =
            ranking.rank_positions(&positions, &mark_prices(dec!(50000)), PositionSide::Long, &btc());
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_rank_excludes_losing_positions() {
        // Short at entry 50_000, mark 55_000 → upnl = -5_000 → losing → excluded.
        let positions = vec![
            make_position(1, PositionSide::Short, dec!(1), dec!(50000), dec!(5000)),
        ];
        let ranking = AdlRanking::default();
        let candidates =
            ranking.rank_positions(&positions, &mark_prices(dec!(55000)), PositionSide::Long, &btc());
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_rank_no_candidates_when_none_available() {
        let ranking = AdlRanking::default();
        let candidates = ranking.rank_positions(
            &[],
            &mark_prices(dec!(50000)),
            PositionSide::Long,
            &btc(),
        );
        assert!(candidates.is_empty());
    }

    // ─── ADL calculation tests ───────────────────────────────────────────────

    fn make_candidate(user_id: u128, size: Decimal, score: Decimal) -> AdlCandidate {
        AdlCandidate {
            user_id: Uuid::from_u128(user_id),
            symbol: btc(),
            side: PositionSide::Short,
            size,
            score,
            entry_price: dec!(50000),
            margin: dec!(5000),
            unrealized_pnl: dec!(5000),
        }
    }

    #[test]
    fn test_adl_single_candidate_covers_deficit() {
        // Deficit = 10_000, candidate size = 1, close_price = 45_000.
        // Full close notional = 45_000 ≥ deficit → partial close.
        // close_qty = 10_000 / 45_000 ≈ 0.2222...
        let candidates = vec![make_candidate(1, dec!(1), dec!(10))];
        let orders = calculate_adl(dec!(10000), &candidates, dec!(45000));

        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].close_price, dec!(45000));
        // close_qty = 10_000 / 45_000
        let expected_qty = dec!(10000) / dec!(45000);
        assert_eq!(orders[0].close_qty, expected_qty);
    }

    #[test]
    fn test_adl_full_close_when_notional_less_than_deficit() {
        // Deficit = 50_000, candidate size = 1, close_price = 45_000.
        // Full close notional = 45_000 < deficit → full close, remainder = 5_000.
        let candidates = vec![make_candidate(1, dec!(1), dec!(10))];
        let orders = calculate_adl(dec!(50000), &candidates, dec!(45000));

        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].close_qty, dec!(1));
    }

    #[test]
    fn test_adl_multiple_candidates_needed() {
        // Deficit = 90_000, each candidate: size=1, close_price=50_000.
        // Candidate 1: full close 50_000 → remaining = 40_000.
        // Candidate 2: partial close 40_000 / 50_000 = 0.8 → remaining = 0.
        // Candidate 3: not touched.
        let candidates = vec![
            make_candidate(1, dec!(1), dec!(30)),
            make_candidate(2, dec!(1), dec!(20)),
            make_candidate(3, dec!(1), dec!(10)),
        ];
        let orders = calculate_adl(dec!(90000), &candidates, dec!(50000));

        assert_eq!(orders.len(), 2);
        assert_eq!(orders[0].close_qty, dec!(1)); // full close
        assert_eq!(orders[0].user_id, Uuid::from_u128(1));
        assert_eq!(orders[1].user_id, Uuid::from_u128(2));
        let expected_qty = dec!(40000) / dec!(50000);
        assert_eq!(orders[1].close_qty, expected_qty);
    }

    #[test]
    fn test_adl_partial_deleverage_stops_early() {
        // Deficit = 10_000 < first candidate's full notional (1 * 50_000 = 50_000).
        // Only a partial close of the first candidate is needed.
        let candidates = vec![
            make_candidate(1, dec!(1), dec!(30)),
            make_candidate(2, dec!(1), dec!(20)),
        ];
        let orders = calculate_adl(dec!(10000), &candidates, dec!(50000));

        assert_eq!(orders.len(), 1, "second candidate should not be touched");
        assert_eq!(orders[0].user_id, Uuid::from_u128(1));
        assert!(orders[0].close_qty < dec!(1));
    }

    #[test]
    fn test_adl_zero_deficit_returns_empty() {
        let candidates = vec![make_candidate(1, dec!(1), dec!(10))];
        let orders = calculate_adl(Decimal::ZERO, &candidates, dec!(45000));
        assert!(orders.is_empty());
    }

    #[test]
    fn test_adl_no_candidates_returns_empty() {
        let orders = calculate_adl(dec!(10000), &[], dec!(45000));
        assert!(orders.is_empty());
    }

    #[test]
    fn test_adl_preserves_original_entry_price() {
        let mut c = make_candidate(1, dec!(1), dec!(10));
        c.entry_price = dec!(48000);
        let orders = calculate_adl(dec!(5000), &[c], dec!(45000));
        assert_eq!(orders[0].original_entry_price, dec!(48000));
        assert_eq!(orders[0].close_price, dec!(45000));
    }

    // ─── ADL quantile tests ───────────────────────────────────────────────────

    #[test]
    fn test_adl_quantile_single_profitable_position() {
        // Only one profitable position → it is the sole ADL target → quantile 5.
        let pos = make_position(1, PositionSide::Short, dec!(1), dec!(50000), dec!(5000));
        let all = vec![pos.clone()];
        // mark=45_000 → short profits.
        let q = adl_quantile(&pos, &all, dec!(45000));
        assert_eq!(q, Some(5));
    }

    #[test]
    fn test_adl_quantile_unprofitable_returns_none() {
        // Long at entry, no PnL.
        let pos = make_position(1, PositionSide::Long, dec!(1), dec!(50000), dec!(5000));
        let all = vec![pos.clone()];
        let q = adl_quantile(&pos, &all, dec!(50000));
        assert_eq!(q, None);
    }

    #[test]
    fn test_adl_quantile_top_scorer_is_five() {
        // Five positions with different margins (= different scores). Highest scorer → 5.
        let positions: Vec<Position> = (1..=5u128)
            .map(|i| {
                // User i has margin = i * 1000. Lower margin = higher pnl_ratio.
                // User 1 → highest score, user 5 → lowest.
                make_position(i, PositionSide::Short, dec!(1), dec!(50000), Decimal::from(i * 1000))
            })
            .collect();

        // mark=45_000: short upnl = 5_000 for all.
        let top = positions[0].clone(); // user 1, margin=1000, highest score
        let q = adl_quantile(&top, &positions, dec!(45000));
        assert_eq!(q, Some(5));
    }

    #[test]
    fn test_adl_quantile_losing_position_excluded() {
        // Short entered at 40_000, mark = 55_000 → short is losing (price went up).
        // upnl = (40_000 - 55_000) * 1 = -15_000 → quantile should be None.
        let winner = make_position(1, PositionSide::Short, dec!(1), dec!(50000), dec!(5000));
        let losing = make_position(2, PositionSide::Short, dec!(1), dec!(40000), dec!(5000));
        let all = vec![winner.clone(), losing.clone()];
        let q = adl_quantile(&losing, &all, dec!(55000));
        assert_eq!(q, None);
    }
}
