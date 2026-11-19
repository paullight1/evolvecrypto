//! Liquidation engine for perpetual futures.
//!
//! Scans all positions against current mark prices and emits [`LiquidationOrder`]s
//! for any position whose effective margin has fallen below the maintenance threshold.
//!
//! # Insurance Fund
//! When a liquidation is executed:
//! - If the fill price is **better** than `bankruptcy_price` (surplus): the excess
//!   goes into the insurance fund.
//! - If the fill price is **worse** than `bankruptcy_price` (deficit): the insurance
//!   fund covers the shortfall. If the fund is exhausted, the loss is socialised
//!   (tracked as `socialised_loss`).
//!
//! # Cascade Prevention
//! Large-scale market moves can trigger many liquidations simultaneously, which
//! drives prices further down and triggers yet more liquidations. To slow a cascade,
//! this engine processes at most `max_batch_size` liquidations per call to
//! [`LiquidationEngine::check_all_positions`].
//!
//! # Partial Liquidation
//! Before fully closing a position the engine attempts a partial liquidation: it
//! reduces the position by only as much as is needed to bring the margin ratio back
//! above the maintenance threshold (with a 10% safety buffer).  Only if the
//! minimum reducible quantity is the full position size does it emit a full-close
//! order.

use std::collections::HashMap;

use chrono::Utc;
use rust_decimal::Decimal;
use tracing::{info, warn};
use uuid::Uuid;

use exchange_common::{
    futures::{LiquidationOrder, PerpetualConfig, Position, PositionSide},
    types::{Price, Qty, Symbol, UserId},
};

use crate::margin_calculator::is_liquidatable;

// ── Liquidation event record ─────────────────────────────────────────────────

/// A record of one liquidation event (partial or full).
///
/// Stored on [`Exchange`] for user history and the public liquidation feed.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LiquidationEvent {
    /// Unique event id.
    pub id: Uuid,
    /// The user whose position was (partially) liquidated.
    pub user_id: UserId,
    pub symbol: Symbol,
    pub side: PositionSide,
    /// Quantity that was reduced in this event.
    pub qty_liquidated: Qty,
    /// Execution price (mark price at the time of liquidation).
    pub price: Price,
    /// `true` if only part of the position was liquidated.
    pub is_partial: bool,
    /// Liquidation fee charged (sent to insurance fund).
    pub fee: Decimal,
    pub timestamp: chrono::DateTime<Utc>,
}

// ── Partial liquidation calculation ──────────────────────────────────────────

/// Fee rate charged on the liquidated notional (goes to insurance fund).
/// Used by the tasks layer to compute the fee amount recorded in LiquidationEvent.
pub const LIQUIDATION_FEE_RATE: &str = "0.0015"; // 0.15%

/// Safety buffer above maintenance margin after partial liquidation.
///
/// The engine targets `maintenance_margin * SAFETY_BUFFER` as the minimum
/// effective margin of the *remaining* position. This prevents the position
/// from immediately triggering liquidation again after the partial close.
const SAFETY_BUFFER: &str = "1.1"; // 10% above maintenance

/// Calculate the minimum quantity to liquidate so that the remaining position
/// is brought back above its maintenance margin (with safety buffer).
///
/// Returns `(qty_to_liquidate, is_partial)`.
///
/// - If a partial close would be sufficient, `qty_to_liquidate < pos.size` and
///   `is_partial = true`.
/// - If the position is so underwater that even a full close is required (or if
///   no partial size is recoverable), `qty_to_liquidate == pos.size` and
///   `is_partial = false`.
pub fn partial_liquidation_qty(
    pos: &Position,
    mark_price: Price,
    config: &PerpetualConfig,
) -> (Qty, bool) {
    let safety: Decimal = SAFETY_BUFFER.parse().unwrap_or_else(|_| {
        Decimal::new(11, 1) // 1.1
    });

    if pos.size.is_zero() || mark_price.is_zero() {
        return (pos.size, false);
    }

    // Per-unit effective margin of the position.
    let margin_per_unit = pos.margin / pos.size;
    let upnl_per_unit = match pos.side {
        PositionSide::Long => mark_price - pos.entry_price,
        PositionSide::Short => pos.entry_price - mark_price,
    };
    let effective_per_unit = margin_per_unit + upnl_per_unit;

    // Walk tiers from lowest MMR (index 0) upward.  For each tier that satisfies
    // the per-unit condition, determine the minimum remaining size that still fits
    // within that tier.
    let mut best_remaining: Option<Qty> = None;

    for tier in &config.tiers {
        let required_per_unit = mark_price * tier.maintenance_margin_rate * safety;
        if effective_per_unit < required_per_unit {
            // This tier's MMR is too high; the remaining position would still be underwater.
            continue;
        }

        // The per-unit condition holds for any size in this tier.
        let min_r = if tier.min_notional.is_zero() {
            Decimal::new(1, 8) // 0.00000001 — effectively zero
        } else {
            tier.min_notional / mark_price
        };

        let max_r = if tier.max_notional == Decimal::MAX {
            pos.size
        } else {
            let slightly_below = tier.max_notional - Decimal::ONE;
            (slightly_below / mark_price).min(pos.size)
        };

        if max_r < min_r {
            continue; // Tier range is empty at this mark price.
        }

        let target_r = max_r.min(pos.size);
        if target_r >= pos.size {
            continue;
        }

        // Keep the candidate with the largest remaining size (minimal reduction).
        best_remaining = Some(match best_remaining {
            Some(prev) => prev.max(target_r),
            None => target_r,
        });
    }

    match best_remaining {
        Some(r) if r > Decimal::ZERO && r < pos.size => {
            let qty = pos.size - r;
            (qty, true)
        }
        _ => {
            // No viable partial size found — fully liquidate.
            (pos.size, false)
        }
    }
}

/// Outcome of processing insurance fund surplus or deficit after a liquidation fill.
#[derive(Debug, Clone)]
pub struct InsuranceFundUpdate {
    /// Symbol this update applies to.
    pub symbol: Symbol,
    /// Positive = surplus added to the fund, negative = fund disbursement.
    pub delta: Decimal,
    /// Any remaining loss that the fund could not cover (socialised to traders).
    pub socialised_loss: Decimal,
}

/// Result of running a liquidation batch.
#[derive(Debug, Default)]
pub struct LiquidationBatch {
    /// Orders that should be sent to the matching engine for execution.
    pub orders: Vec<LiquidationOrder>,
    /// Insurance fund adjustments per symbol.
    pub insurance_updates: Vec<InsuranceFundUpdate>,
}

/// The liquidation engine.
///
/// Holds per-symbol configuration and processes liquidatable positions in batches.
pub struct LiquidationEngine {
    /// Maximum liquidations emitted per `check_all_positions` call.
    max_batch_size: usize,
}

impl LiquidationEngine {
    /// Create a new engine with the given cascade-prevention batch limit.
    pub fn new(max_batch_size: usize) -> Self {
        Self { max_batch_size }
    }

    /// Scan all positions and return liquidation orders for those that breach maintenance margin.
    ///
    /// `positions` — all currently open positions (across all users).
    /// `mark_prices` — current mark price per symbol.
    /// `configs` — perpetual config per symbol (contains maintenance margin rate).
    pub fn check_all_positions<'a>(
        &self,
        positions: impl Iterator<Item = &'a Position>,
        mark_prices: &HashMap<Symbol, Price>,
        configs: &HashMap<Symbol, PerpetualConfig>,
    ) -> LiquidationBatch {
        let mut batch = LiquidationBatch::default();

        for pos in positions {
            if batch.orders.len() >= self.max_batch_size {
                break;
            }

            let Some(&mark) = mark_prices.get(&pos.symbol) else {
                continue;
            };
            let Some(config) = configs.get(&pos.symbol) else {
                continue;
            };

            if !is_liquidatable(pos, mark, config) {
                continue;
            }

            let bk_price = match config.contract_type {
                exchange_common::futures::ContractType::Inverse => {
                    bankruptcy_price_inverse(pos, config.contract_size)
                }
                exchange_common::futures::ContractType::Linear => bankruptcy_price(pos),
            };
            let (liq_qty, is_partial) = partial_liquidation_qty(pos, mark, config);

            if liq_qty.is_zero() {
                continue;
            }

            info!(
                user_id = %pos.user_id,
                symbol = %pos.symbol,
                side = ?pos.side,
                position_size = %pos.size,
                liq_qty = %liq_qty,
                is_partial = %is_partial,
                mark = %mark,
                bankruptcy_price = %bk_price,
                "liquidating position",
            );

            batch.orders.push(LiquidationOrder {
                user_id: pos.user_id,
                symbol: pos.symbol.clone(),
                side: pos.side,
                size: liq_qty,
                bankruptcy_price: bk_price,
                mark_price: mark,
                timestamp: Utc::now(),
                position_idx: pos.position_idx,
            });
        }

        batch
    }

    /// Check if a cross-margin user's total account is liquidatable.
    ///
    /// For cross-margin: all positions share the margin pool, so liquidation is
    /// triggered when total equity falls below the sum of maintenance margins.
    /// All cross positions are liquidated together.
    pub fn check_cross_margin_liquidation(
        &self,
        user_positions: &[&Position],
        mark_prices: &HashMap<Symbol, Price>,
        configs: &HashMap<Symbol, PerpetualConfig>,
        total_equity: Decimal,
    ) -> Vec<LiquidationOrder> {
        if user_positions.is_empty() {
            return Vec::new();
        }

        // Sum of maintenance margins for all cross positions.
        let total_maintenance: Decimal = user_positions
            .iter()
            .filter_map(|pos| {
                let mark = mark_prices.get(&pos.symbol)?;
                let config = configs.get(&pos.symbol)?;
                Some(pos.notional(*mark) * config.maintenance_margin_rate)
            })
            .sum();

        // If total equity < total maintenance margin, liquidate all cross positions.
        if total_equity < total_maintenance {
            user_positions
                .iter()
                .filter_map(|pos| {
                    let mark = mark_prices.get(&pos.symbol)?;
                    let config = configs.get(&pos.symbol)?;
                    let bk = match config.contract_type {
                        exchange_common::futures::ContractType::Inverse => {
                            bankruptcy_price_inverse(pos, config.contract_size)
                        }
                        exchange_common::futures::ContractType::Linear => bankruptcy_price(pos),
                    };
                    Some(LiquidationOrder {
                        user_id: pos.user_id,
                        symbol: pos.symbol.clone(),
                        side: pos.side,
                        size: pos.size,
                        mark_price: *mark,
                        bankruptcy_price: bk,
                        timestamp: Utc::now(),
                        position_idx: pos.position_idx,
                    })
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Process insurance fund impact after a liquidation fills at `fill_price`.
    ///
    /// Returns an [`InsuranceFundUpdate`] that the caller must apply to persistent state.
    pub fn process_insurance_fund(
        &self,
        order: &LiquidationOrder,
        fill_price: Price,
        current_fund_balance: Decimal,
    ) -> InsuranceFundUpdate {
        // For a Long: we close at fill_price. Bankruptcy is the price where margin = 0.
        // Surplus if fill > bankruptcy (we got a better price than the worst case).
        // For a Short: Bankruptcy price is higher than entry. Fill below bankruptcy = surplus.
        let surplus = match order.side {
            PositionSide::Long => {
                // Close long: fill high is good; bankruptcy is the floor.
                (fill_price - order.bankruptcy_price) * order.size
            }
            PositionSide::Short => {
                // Close short: fill low is good; bankruptcy is the ceiling.
                (order.bankruptcy_price - fill_price) * order.size
            }
        };

        if surplus >= Decimal::ZERO {
            // Surplus: add to fund.
            InsuranceFundUpdate {
                symbol: order.symbol.clone(),
                delta: surplus,
                socialised_loss: Decimal::ZERO,
            }
        } else {
            // Deficit: fund absorbs the shortfall.
            let shortfall = surplus.abs();
            let covered = shortfall.min(current_fund_balance);
            let socialised = shortfall - covered;

            if socialised > Decimal::ZERO {
                warn!(
                    symbol = %order.symbol,
                    shortfall = %shortfall,
                    covered = %covered,
                    socialised = %socialised,
                    "insurance fund exhausted — socialising loss",
                );
            }

            InsuranceFundUpdate {
                symbol: order.symbol.clone(),
                delta: -covered,
                socialised_loss: socialised,
            }
        }
    }
}

/// Price at which the position's effective margin reaches zero (i.e., total loss of margin).
///
/// For **linear** contracts:
/// - Long:  `bankruptcy = entry - margin / size`
/// - Short: `bankruptcy = entry + margin / size`
///
/// For **inverse** contracts (coin-margined), pass `contract_size` via `bankruptcy_price_inverse`.
/// This function uses the linear formula as a safe default for backward compatibility.
pub fn bankruptcy_price(position: &Position) -> Price {
    debug_assert!(!position.size.is_zero(), "cannot compute bankruptcy price for empty position");
    let margin_per_unit = position.margin / position.size;
    match position.side {
        PositionSide::Long => position.entry_price - margin_per_unit,
        PositionSide::Short => position.entry_price + margin_per_unit,
    }
}

/// Bankruptcy price for an **inverse** (coin-margined) position.
///
/// The effective margin in base asset becomes zero at:
///
/// Long:  `bk = entry * N*cs / (N*cs + margin*entry)`
/// Short: `bk = entry * N*cs / (N*cs - margin*entry)`
///
/// Falls back to the linear formula if any denominator is non-positive.
pub fn bankruptcy_price_inverse(position: &Position, contract_size: Decimal) -> Price {
    debug_assert!(!position.size.is_zero(), "cannot compute bankruptcy price for empty position");
    let n_cs = position.size * contract_size;
    match position.side {
        PositionSide::Long => {
            let denom = n_cs + position.margin * position.entry_price;
            if denom.is_zero() {
                return position.entry_price;
            }
            position.entry_price * n_cs / denom
        }
        PositionSide::Short => {
            let denom = n_cs - position.margin * position.entry_price;
            if denom <= Decimal::ZERO {
                // Extremely well-margined short; use linear approximation as fallback
                return position.entry_price + position.margin / position.size;
            }
            position.entry_price * n_cs / denom
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use exchange_common::{
        futures::{MarginMode, PerpetualConfig, Position, PositionSide},
        types::Symbol,
    };
    use rust_decimal_macros::dec;
    use uuid::Uuid;
    use chrono::Utc;

    fn btc() -> Symbol {
        Symbol::new("BTCUSDT")
    }

    fn make_position(side: PositionSide, size: Decimal, entry: Decimal, margin: Decimal) -> Position {
        Position {
            user_id: Uuid::nil(),
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

    fn btc_config() -> PerpetualConfig {
        PerpetualConfig::btc_default()
    }

    // --- bankruptcy_price ---

    #[test]
    fn test_bankruptcy_price_long() {
        // Entry 50_000, margin 5_000, size 1 → bankruptcy = 45_000
        let pos = make_position(PositionSide::Long, dec!(1), dec!(50000), dec!(5000));
        assert_eq!(bankruptcy_price(&pos), dec!(45000));
    }

    #[test]
    fn test_bankruptcy_price_short() {
        // Entry 50_000, margin 5_000, size 1 → bankruptcy = 55_000
        let pos = make_position(PositionSide::Short, dec!(1), dec!(50000), dec!(5000));
        assert_eq!(bankruptcy_price(&pos), dec!(55000));
    }

    #[test]
    fn test_bankruptcy_price_fractional_size() {
        // Entry 50_000, margin 500, size 0.1 → margin_per_unit = 5_000
        // long: bankruptcy = 50_000 - 5_000 = 45_000
        let pos = make_position(PositionSide::Long, dec!(0.1), dec!(50000), dec!(500));
        assert_eq!(bankruptcy_price(&pos), dec!(45000));
    }

    // --- check_all_positions (liquidation detection) ---

    #[test]
    fn test_healthy_position_not_liquidated() {
        let engine = LiquidationEngine::new(100);
        let pos = make_position(PositionSide::Long, dec!(1), dec!(50000), dec!(5000));

        let mut prices = HashMap::new();
        prices.insert(btc(), dec!(50000)); // price at entry — fine

        let mut configs = HashMap::new();
        configs.insert(btc(), btc_config());

        let batch = engine.check_all_positions(std::iter::once(&pos), &prices, &configs);
        assert!(batch.orders.is_empty());
    }

    #[test]
    fn test_underwater_position_liquidated() {
        let engine = LiquidationEngine::new(100);
        let pos = make_position(PositionSide::Long, dec!(1), dec!(50000), dec!(5000));

        let mut prices = HashMap::new();
        prices.insert(btc(), dec!(44000)); // well below bankruptcy price (45_000)

        let mut configs = HashMap::new();
        configs.insert(btc(), btc_config());

        let batch = engine.check_all_positions(std::iter::once(&pos), &prices, &configs);
        assert_eq!(batch.orders.len(), 1);
        assert_eq!(batch.orders[0].size, dec!(1));
        assert_eq!(batch.orders[0].side, PositionSide::Long);
    }

    #[test]
    fn test_batch_size_limits_liquidations() {
        let engine = LiquidationEngine::new(2); // max 2 per batch

        // Create 5 underwater positions for different users.
        let positions: Vec<Position> = (0..5u128)
            .map(|i| {
                let mut pos = make_position(PositionSide::Long, dec!(1), dec!(50000), dec!(5000));
                pos.user_id = Uuid::from_u128(i + 1);
                pos
            })
            .collect();

        let mut prices = HashMap::new();
        prices.insert(btc(), dec!(40000)); // all liquidatable

        let mut configs = HashMap::new();
        configs.insert(btc(), btc_config());

        let batch = engine.check_all_positions(positions.iter(), &prices, &configs);
        assert_eq!(batch.orders.len(), 2); // capped at max_batch_size
    }

    #[test]
    fn test_missing_mark_price_skipped() {
        let engine = LiquidationEngine::new(100);
        let pos = make_position(PositionSide::Long, dec!(1), dec!(50000), dec!(5000));

        // No mark price for BTCUSDT → position skipped, no liquidation.
        let prices: HashMap<Symbol, Price> = HashMap::new();
        let mut configs = HashMap::new();
        configs.insert(btc(), btc_config());

        let batch = engine.check_all_positions(std::iter::once(&pos), &prices, &configs);
        assert!(batch.orders.is_empty());
    }

    // --- insurance fund ---

    #[test]
    fn test_insurance_fund_surplus_long() {
        let engine = LiquidationEngine::new(100);
        let pos = make_position(PositionSide::Long, dec!(1), dec!(50000), dec!(5000));
        let order = LiquidationOrder {
            user_id: Uuid::nil(),
            symbol: btc(),
            side: PositionSide::Long,
            size: dec!(1),
            bankruptcy_price: dec!(45000),
            mark_price: dec!(44000),
            timestamp: Utc::now(),
            position_idx: 0,
        };

        // Fill at 46_000 (better than bankruptcy 45_000) → surplus = (46_000 - 45_000) * 1 = 1_000
        let update = engine.process_insurance_fund(&order, dec!(46000), dec!(10000));
        assert_eq!(update.delta, dec!(1000));
        assert_eq!(update.socialised_loss, Decimal::ZERO);
        let _ = pos; // pos used for context
    }

    #[test]
    fn test_insurance_fund_deficit_covered_by_fund() {
        let engine = LiquidationEngine::new(100);
        let order = LiquidationOrder {
            user_id: Uuid::nil(),
            symbol: btc(),
            side: PositionSide::Long,
            size: dec!(1),
            bankruptcy_price: dec!(45000),
            mark_price: dec!(44000),
            timestamp: Utc::now(),
            position_idx: 0,
        };

        // Fill at 43_000 (worse than bankruptcy 45_000) → deficit = (45_000 - 43_000) * 1 = 2_000
        // Fund balance 5_000 — covers it fully.
        let update = engine.process_insurance_fund(&order, dec!(43000), dec!(5000));
        assert_eq!(update.delta, dec!(-2000));
        assert_eq!(update.socialised_loss, Decimal::ZERO);
    }

    #[test]
    fn test_insurance_fund_deficit_socialised_when_exhausted() {
        let engine = LiquidationEngine::new(100);
        let order = LiquidationOrder {
            user_id: Uuid::nil(),
            symbol: btc(),
            side: PositionSide::Long,
            size: dec!(2),
            bankruptcy_price: dec!(45000),
            mark_price: dec!(44000),
            timestamp: Utc::now(),
            position_idx: 0,
        };

        // Fill at 43_000 → deficit = (45_000 - 43_000) * 2 = 4_000
        // Fund balance 1_000 → covers 1_000, socialises 3_000.
        let update = engine.process_insurance_fund(&order, dec!(43000), dec!(1000));
        assert_eq!(update.delta, dec!(-1000));
        assert_eq!(update.socialised_loss, dec!(3000));
    }

    #[test]
    fn test_insurance_fund_surplus_short() {
        let engine = LiquidationEngine::new(100);
        let order = LiquidationOrder {
            user_id: Uuid::nil(),
            symbol: btc(),
            side: PositionSide::Short,
            size: dec!(1),
            bankruptcy_price: dec!(55000),
            mark_price: dec!(56000),
            timestamp: Utc::now(),
            position_idx: 0,
        };

        // Fill at 54_000 (below bankruptcy 55_000 → surplus for short)
        // surplus = (55_000 - 54_000) * 1 = 1_000
        let update = engine.process_insurance_fund(&order, dec!(54000), dec!(5000));
        assert_eq!(update.delta, dec!(1000));
        assert_eq!(update.socialised_loss, Decimal::ZERO);
    }

    // --- tiered MMR in liquidation detection ---

    #[test]
    fn test_large_btc_position_uses_higher_mmr() {
        // A position with notional ≥ 25M uses 5% MMR.
        // 500 BTC at mark 50_000 → notional = 25_000_000 (tier 4 boundary, inclusive).
        // margin = 1_000_000. effective = 1_000_000, maintenance = 25_000_000 * 0.05 = 1_250_000.
        // 1_000_000 < 1_250_000 → liquidatable.
        let engine = LiquidationEngine::new(100);
        let mut pos = make_position(PositionSide::Long, dec!(500), dec!(50000), dec!(1000000));

        let mut prices = HashMap::new();
        prices.insert(btc(), dec!(50000));

        let mut configs = HashMap::new();
        configs.insert(btc(), btc_config());

        let batch = engine.check_all_positions(std::iter::once(&pos), &prices, &configs);
        // At this size/margin the tiered MMR causes liquidation even at entry price.
        assert_eq!(batch.orders.len(), 1);

        // Now give enough margin: 1_250_001 > 1_250_000 → not liquidatable.
        pos.margin = dec!(1250001);
        let batch2 = engine.check_all_positions(std::iter::once(&pos), &prices, &configs);
        assert!(batch2.orders.is_empty());
    }

    // --- cross-margin liquidation ---

    #[test]
    fn test_cross_margin_liquidation_triggered_when_equity_insufficient() {
        let engine = LiquidationEngine::new(100);

        // User has two cross-margin positions.
        let user_id = Uuid::new_v4();
        let pos1 = {
            let mut p = make_position(PositionSide::Long, dec!(1), dec!(50000), dec!(5000));
            p.user_id = user_id;
            p.margin_mode = MarginMode::Cross;
            p
        };
        let pos2 = {
            let mut p = make_position(PositionSide::Short, dec!(2), dec!(30000), dec!(6000));
            p.user_id = user_id;
            p.symbol = Symbol::new("ETHUSDT");
            p.margin_mode = MarginMode::Cross;
            p
        };

        let mut prices = HashMap::new();
        prices.insert(btc(), dec!(50000));
        prices.insert(Symbol::new("ETHUSDT"), dec!(30000));

        let mut configs = HashMap::new();
        configs.insert(btc(), btc_config());
        configs.insert(Symbol::new("ETHUSDT"), PerpetualConfig::eth_default());

        // Low equity: total_equity = 100
        // maintenance for pos1 = 50000 * 0.005 = 250
        // maintenance for pos2 = 60000 * 0.01 = 600
        // total maintenance = 850 > 100 → should liquidate
        let total_equity = dec!(100);
        let user_positions = vec![&pos1, &pos2];
        let liq_orders = engine.check_cross_margin_liquidation(
            &user_positions,
            &prices,
            &configs,
            total_equity,
        );

        assert!(!liq_orders.is_empty(), "should trigger cross-margin liquidation");
        assert_eq!(liq_orders.len(), 2, "both positions should be marked for liquidation");
    }

    #[test]
    fn test_cross_margin_no_liquidation_when_equity_sufficient() {
        let engine = LiquidationEngine::new(100);

        let user_id = Uuid::new_v4();
        let pos = {
            let mut p = make_position(PositionSide::Long, dec!(1), dec!(50000), dec!(5000));
            p.user_id = user_id;
            p.margin_mode = MarginMode::Cross;
            p
        };

        let mut prices = HashMap::new();
        prices.insert(btc(), dec!(50000));

        let mut configs = HashMap::new();
        configs.insert(btc(), btc_config());

        // Large equity: well above maintenance margin.
        let total_equity = dec!(100000);
        let user_positions = vec![&pos];
        let liq_orders = engine.check_cross_margin_liquidation(
            &user_positions,
            &prices,
            &configs,
            total_equity,
        );

        assert!(liq_orders.is_empty(), "sufficient equity — should not liquidate");
    }

    #[test]
    fn test_cross_margin_liquidation_empty_positions() {
        let engine = LiquidationEngine::new(100);
        let prices = HashMap::new();
        let configs = HashMap::new();

        let orders = engine.check_cross_margin_liquidation(&[], &prices, &configs, dec!(10000));
        assert!(orders.is_empty());
    }

    #[test]
    fn test_small_position_at_tier0_mmr_survives_deeper_loss() {
        // Small BTC position: notional = 50_000 (tier 0: MMR 0.5%).
        // 1 BTC, entry 50_000, margin 5_000. Price drops to 45_200.
        //   effective = 5_000 + (45_200 - 50_000) = 5_000 - 4_800 = 200
        //   maintenance = 45_200 * 0.005 = 226
        //   200 < 226 → liquidatable
        let engine = LiquidationEngine::new(100);
        let pos = make_position(PositionSide::Long, dec!(1), dec!(50000), dec!(5000));

        let mut prices = HashMap::new();
        prices.insert(btc(), dec!(45200));

        let mut configs = HashMap::new();
        configs.insert(btc(), btc_config());

        let batch = engine.check_all_positions(std::iter::once(&pos), &prices, &configs);
        assert_eq!(batch.orders.len(), 1);
    }

    // ─── P0-5: Funding debt liquidation tests ─────────────────────────────

    #[test]
    fn test_unpaid_funding_debt_triggers_liquidation_at_higher_price() {
        // Without debt: pos is healthy at mark=48_000
        //   effective = 5000 + (48000 - 50000) = 5000 - 2000 = 3000
        //   maintenance = 48000 * 0.005 = 240 → NOT liquidatable
        //
        // With 3000 unpaid debt:
        //   effective = 3000 - 3000 = 0 < 240 → liquidatable
        let engine = LiquidationEngine::new(100);

        let mut pos_no_debt = make_position(PositionSide::Long, dec!(1), dec!(50000), dec!(5000));
        let mut pos_with_debt = pos_no_debt.clone();
        pos_with_debt.unpaid_funding_debt = dec!(3000);

        let mut prices = HashMap::new();
        prices.insert(btc(), dec!(48000));
        let mut configs = HashMap::new();
        configs.insert(btc(), btc_config());

        // No debt: healthy
        let batch_no_debt = engine.check_all_positions(std::iter::once(&pos_no_debt), &prices, &configs);
        assert!(batch_no_debt.orders.is_empty(), "should be healthy without debt");

        // With debt: liquidatable even at same (higher) price
        let batch_with_debt = engine.check_all_positions(std::iter::once(&pos_with_debt), &prices, &configs);
        assert_eq!(batch_with_debt.orders.len(), 1, "debt should push position into liquidation");
    }

    #[test]
    fn test_small_funding_debt_does_not_trigger_premature_liquidation() {
        // 1 USDT of debt on a 5000 USDT margin position at entry price is negligible.
        let engine = LiquidationEngine::new(100);
        let mut pos = make_position(PositionSide::Long, dec!(1), dec!(50000), dec!(5000));
        pos.unpaid_funding_debt = dec!(1);

        let mut prices = HashMap::new();
        prices.insert(btc(), dec!(50000)); // at entry, very healthy
        let mut configs = HashMap::new();
        configs.insert(btc(), btc_config());

        let batch = engine.check_all_positions(std::iter::once(&pos), &prices, &configs);
        assert!(batch.orders.is_empty(), "tiny debt should not liquidate a healthy position");
    }
}
