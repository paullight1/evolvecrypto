//! PnL attribution for market makers.
//!
//! Decomposes a market maker's total PnL into four components:
//!
//! | Component            | Description |
//! |----------------------|-------------|
//! | Spread capture       | Revenue from buying below and selling above mid |
//! | Inventory PnL        | Mark-to-market gain/loss on the current position |
//! | Rebate income        | Exchange rebates received for providing liquidity |
//! | Adverse selection    | Cost from fills that were immediately picked off |
//!
//! `net_pnl = spread_capture + inventory_pnl + rebate_income - adverse_selection_cost`

use exchange_common::Side;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tracing::debug;

// ── fill input ────────────────────────────────────────────────────────────────

/// A fill used as input to the PnL attribution calculation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributionFill {
    /// Side of the aggressor (taker).  The MM is always the maker/passive side.
    pub taker_side: Side,
    /// Fill price.
    pub price: Decimal,
    /// Quantity filled.
    pub qty: Decimal,
    /// Mid-price at the moment of the fill.
    pub mid_price: Decimal,
}

// ── report ────────────────────────────────────────────────────────────────────

/// PnL attribution breakdown for a single measurement period.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MmPnlAttribution {
    /// Revenue captured from bid-ask spread.
    ///
    /// For each fill where the MM is passive:
    /// - If taker is Buy, MM sold at `fill_price`; spread capture = `fill_price - mid`.
    /// - If taker is Sell, MM bought at `fill_price`; spread capture = `mid - fill_price`.
    ///
    /// Positive when the MM consistently quotes around mid.
    pub spread_capture: Decimal,
    /// Mark-to-market gain or loss on the accumulated inventory position.
    pub inventory_pnl: Decimal,
    /// Total rebates received from the exchange.
    pub rebate_income: Decimal,
    /// Total cost from adverse selection (fills immediately picked off).
    pub adverse_selection_cost: Decimal,
    /// Net PnL: `spread_capture + inventory_pnl + rebate_income - adverse_selection_cost`.
    pub net_pnl: Decimal,
    /// Number of fills included in this attribution.
    pub fill_count: usize,
}

// ── attribution engine ────────────────────────────────────────────────────────

/// Stateless PnL attribution calculator.
pub struct MmPnlAttributor;

impl MmPnlAttributor {
    /// Compute the full attribution from the provided inputs.
    ///
    /// # Parameters
    /// - `fills`: slice of fills for the measurement period.
    /// - `current_inventory_pnl`: externally provided mark-to-market PnL on
    ///   the current open position (can be negative).
    /// - `total_rebates`: cumulative rebates received in the period.
    /// - `total_adverse_cost`: cumulative adverse-selection cost in the period.
    pub fn attribute(
        fills: &[AttributionFill],
        current_inventory_pnl: Decimal,
        total_rebates: Decimal,
        total_adverse_cost: Decimal,
    ) -> MmPnlAttribution {
        let fill_count = fills.len();

        // Spread capture: sum over fills of (fill_price - mid) * qty for MM perspective.
        // MM perspective: taker Buy → MM sold above mid (positive); taker Sell → MM bought below mid (positive).
        let spread_capture: Decimal = fills
            .iter()
            .map(|f| {
                let edge = match f.taker_side {
                    Side::Buy => f.price - f.mid_price,   // MM sold; profit = price - mid
                    Side::Sell => f.mid_price - f.price,  // MM bought; profit = mid - price
                };
                edge * f.qty
            })
            .sum();

        let net_pnl = spread_capture + current_inventory_pnl + total_rebates - total_adverse_cost;

        debug!(
            %spread_capture,
            %current_inventory_pnl,
            %total_rebates,
            %total_adverse_cost,
            %net_pnl,
            fill_count,
            "PnL attribution computed"
        );

        MmPnlAttribution {
            spread_capture,
            inventory_pnl: current_inventory_pnl,
            rebate_income: total_rebates,
            adverse_selection_cost: total_adverse_cost,
            net_pnl,
            fill_count,
        }
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    // T1: pure spread capture — MM buys below mid and sells above mid
    #[test]
    fn pure_spread_capture_buy_and_sell() {
        // Mid = 100. MM sells to a Buy aggressor at 101 → edge = 1.
        // MM buys from a Sell aggressor at 99 → edge = 1.
        let fills = vec![
            AttributionFill {
                taker_side: Side::Buy,
                price: dec!(101),
                qty: dec!(1),
                mid_price: dec!(100),
            },
            AttributionFill {
                taker_side: Side::Sell,
                price: dec!(99),
                qty: dec!(1),
                mid_price: dec!(100),
            },
        ];
        let attr = MmPnlAttributor::attribute(&fills, Decimal::ZERO, Decimal::ZERO, Decimal::ZERO);
        assert_eq!(attr.spread_capture, dec!(2));
        assert_eq!(attr.net_pnl, dec!(2));
    }

    // T2: inventory PnL positive flows through correctly
    #[test]
    fn inventory_pnl_positive_adds_to_net() {
        let attr =
            MmPnlAttributor::attribute(&[], dec!(500), Decimal::ZERO, Decimal::ZERO);
        assert_eq!(attr.inventory_pnl, dec!(500));
        assert_eq!(attr.net_pnl, dec!(500));
    }

    // T3: inventory PnL negative reduces net
    #[test]
    fn inventory_pnl_negative_reduces_net() {
        let attr =
            MmPnlAttributor::attribute(&[], dec!(-200), Decimal::ZERO, Decimal::ZERO);
        assert_eq!(attr.inventory_pnl, dec!(-200));
        assert_eq!(attr.net_pnl, dec!(-200));
    }

    // T4: rebate income adds to net PnL
    #[test]
    fn rebate_income_adds_to_net() {
        let attr =
            MmPnlAttributor::attribute(&[], Decimal::ZERO, dec!(50), Decimal::ZERO);
        assert_eq!(attr.rebate_income, dec!(50));
        assert_eq!(attr.net_pnl, dec!(50));
    }

    // T5: adverse selection cost reduces net PnL
    #[test]
    fn adverse_selection_cost_reduces_net() {
        let attr =
            MmPnlAttributor::attribute(&[], Decimal::ZERO, Decimal::ZERO, dec!(75));
        assert_eq!(attr.adverse_selection_cost, dec!(75));
        assert_eq!(attr.net_pnl, dec!(-75));
    }

    // T6: full combined scenario
    #[test]
    fn net_pnl_is_sum_of_all_components() {
        // spread_capture=200, inventory_pnl=100, rebates=30, adverse=50
        // net = 200 + 100 + 30 - 50 = 280
        let fills = vec![
            AttributionFill {
                taker_side: Side::Buy,
                price: dec!(1100),
                qty: dec!(2),
                mid_price: dec!(1000),
            },
        ];
        // spread_capture = (1100 - 1000) * 2 = 200
        let attr =
            MmPnlAttributor::attribute(&fills, dec!(100), dec!(30), dec!(50));
        assert_eq!(attr.spread_capture, dec!(200));
        assert_eq!(attr.net_pnl, dec!(280));
    }

    // T7: no fills → zero spread capture, fill_count = 0
    #[test]
    fn no_fills_gives_zero_spread_capture() {
        let attr =
            MmPnlAttributor::attribute(&[], Decimal::ZERO, Decimal::ZERO, Decimal::ZERO);
        assert_eq!(attr.spread_capture, Decimal::ZERO);
        assert_eq!(attr.fill_count, 0);
        assert_eq!(attr.net_pnl, Decimal::ZERO);
    }

    // T8: fill at exactly mid → zero spread edge
    #[test]
    fn fill_at_mid_gives_zero_spread_capture() {
        let fills = vec![AttributionFill {
            taker_side: Side::Buy,
            price: dec!(100),
            qty: dec!(5),
            mid_price: dec!(100),
        }];
        let attr =
            MmPnlAttributor::attribute(&fills, Decimal::ZERO, Decimal::ZERO, Decimal::ZERO);
        assert_eq!(attr.spread_capture, Decimal::ZERO);
    }
}
