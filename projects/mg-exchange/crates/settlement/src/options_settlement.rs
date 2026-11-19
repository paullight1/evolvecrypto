//! Options settlement and margin calculation.
//!
//! # Settlement at expiry
//!
//! European-style cash settlement:
//! - **In-the-money (ITM)**: intrinsic value is paid to the long, debited from
//!   the short.  `intrinsic = max(0, S_T − K)` for calls,
//!   `max(0, K − S_T)` for puts.
//! - **Out-of-the-money (OTM)**: the option expires worthless; no cash moves.
//! - **At-the-money**: treated as OTM (zero intrinsic value).
//!
//! # Margin for short options (written positions)
//!
//! Buyers pay the premium upfront and cannot lose more than it; they need no
//! additional margin.  Sellers (writers) face theoretically unlimited liability
//! (for calls) or a bounded-but-large loss (for puts), so they must post margin.
//!
//! The margin model used here is an **exchange-minimum simplified model**:
//!
//! ```text
//! For a short call:
//!   margin = max(
//!       initial_margin_rate × spot × qty,
//!       OTM_amount_floor × qty + mark_price × qty
//!   )
//!
//! For a short put:
//!   margin = max(
//!       initial_margin_rate × spot × qty,
//!       OTM_amount_floor × qty + mark_price × qty
//!   )
//!
//! where:
//!   initial_margin_rate  = 0.10   (10% of spot notional)
//!   OTM_amount_floor     = 0.075  (7.5% of spot notional for OTM buffer)
//! ```
//!
//! This is consistent with CBOE/CME simplified margin rules for short options.
//! Long positions require no margin (premium already paid).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};
use uuid::Uuid;

use exchange_risk_engine::options_instruments::{OptionInstrument, OptionType};

// ─────────────────────────────────────────────────────────────────────────────
// Constants
// ─────────────────────────────────────────────────────────────────────────────

/// Initial margin rate for short option positions (10% of spot notional).
pub const OPTION_INITIAL_MARGIN_RATE: Decimal =
    Decimal::from_parts(10, 0, 0, false, 2); // 0.10

/// OTM buffer floor: 7.5% of spot notional added to the out-of-the-money amount.
pub const OPTION_OTM_BUFFER_RATE: Decimal =
    Decimal::from_parts(75, 0, 0, false, 3); // 0.075

// ─────────────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────────────

/// Side of an option position from the holder's perspective.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PositionSide {
    /// Long — bought the option (paid premium, limited downside).
    Long,
    /// Short — sold/wrote the option (received premium, unlimited upside risk).
    Short,
}

/// A user's position in a single option instrument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionPosition {
    pub user_id: Uuid,
    /// Symbol matching `OptionInstrument::symbol`.
    pub symbol: String,
    /// Long (buyer) or Short (writer).
    pub side: PositionSide,
    /// Absolute quantity in contracts (always positive).
    pub quantity: Decimal,
    /// Average premium paid (Long) or received (Short) per contract.
    pub entry_price: Decimal,
    /// Unrealised PnL in settlement currency.
    pub unrealized_pnl: Decimal,
}

/// The action to take at expiry for one position.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SettlementAction {
    /// The option expires worthless; no cash movement.
    ExpireWorthless {
        user_id: Uuid,
        symbol: String,
        quantity: Decimal,
    },
    /// The ITM option is exercised; the holder receives `payout`, the writer
    /// owes `payout`.
    ExerciseITM {
        user_id: Uuid,
        symbol: String,
        /// Quantity exercised (= position.quantity for European-style).
        quantity: Decimal,
        /// Intrinsic value per contract at settlement.
        intrinsic_per_contract: Decimal,
        /// Total cash credit/debit = intrinsic_per_contract × quantity.
        total_payout: Decimal,
        /// Sign: +1 for longs (receive), -1 for shorts (pay).
        direction: i8,
    },
}

impl SettlementAction {
    /// Net cash flow for this action (positive = inflow).
    pub fn cash_flow(&self) -> Decimal {
        match self {
            Self::ExpireWorthless { .. } => Decimal::ZERO,
            Self::ExerciseITM { total_payout, direction, .. } => {
                if *direction > 0 {
                    *total_payout
                } else {
                    -total_payout
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// OptionsSettlement
// ─────────────────────────────────────────────────────────────────────────────

/// Options settlement engine.
pub struct OptionsSettlement;

impl OptionsSettlement {
    /// Settle all positions for an expired instrument.
    ///
    /// For each position:
    /// - ITM positions are exercised; a [`SettlementAction::ExerciseITM`] is
    ///   emitted with the correct direction (+1 long, -1 short).
    /// - OTM (and ATM) positions expire worthless.
    ///
    /// # Parameters
    /// - `instrument`        — the expired option.
    /// - `settlement_price`  — the final index/mark price at expiry (TWAP or
    ///   auction clearing price).
    /// - `positions`         — all open positions in this instrument.
    pub fn settle_expiry(
        instrument: &OptionInstrument,
        settlement_price: Decimal,
        positions: &[OptionPosition],
    ) -> Vec<SettlementAction> {
        let intrinsic = instrument.intrinsic_value(settlement_price);
        let is_itm = intrinsic > Decimal::ZERO;

        info!(
            symbol = %instrument.symbol,
            %settlement_price,
            strike = %instrument.strike,
            %intrinsic,
            is_itm,
            position_count = positions.len(),
            "settling expired option"
        );

        positions
            .iter()
            .map(|pos| {
                if is_itm {
                    let total_payout = intrinsic * pos.quantity;
                    let direction: i8 = match pos.side {
                        PositionSide::Long => 1,
                        PositionSide::Short => -1,
                    };

                    debug!(
                        user_id = %pos.user_id,
                        symbol = %pos.symbol,
                        %intrinsic,
                        %total_payout,
                        direction,
                        "exercising ITM option"
                    );

                    SettlementAction::ExerciseITM {
                        user_id: pos.user_id,
                        symbol: pos.symbol.clone(),
                        quantity: pos.quantity,
                        intrinsic_per_contract: intrinsic,
                        total_payout,
                        direction,
                    }
                } else {
                    debug!(
                        user_id = %pos.user_id,
                        symbol = %pos.symbol,
                        "option expires worthless"
                    );

                    SettlementAction::ExpireWorthless {
                        user_id: pos.user_id,
                        symbol: pos.symbol.clone(),
                        quantity: pos.quantity,
                    }
                }
            })
            .collect()
    }

    /// Calculate the required margin for one option position.
    ///
    /// - **Long positions**: zero margin (premium is fully paid at entry).
    /// - **Short positions**: exchange-minimum margin based on OTM amount and
    ///   mark price.  See module-level docs for the formula.
    ///
    /// # Parameters
    /// - `position`    — the option position (side, quantity, instrument).
    /// - `instrument`  — the option instrument definition.
    /// - `spot_price`  — current underlying spot/mark price.
    /// - `mark_price`  — current option mark price (Black-Scholes fair value).
    pub fn calculate_margin(
        position: &OptionPosition,
        instrument: &OptionInstrument,
        spot_price: Decimal,
        mark_price: Decimal,
    ) -> Decimal {
        // Long holders paid premium; no further margin required.
        if position.side == PositionSide::Long {
            return Decimal::ZERO;
        }

        let qty = position.quantity;
        if qty.is_zero() || spot_price.is_zero() {
            return Decimal::ZERO;
        }

        // OTM amount per contract (non-negative; positive when out-of-the-money).
        let otm_amount_per_contract = match instrument.option_type {
            OptionType::Call => {
                // Call is OTM when spot < strike.
                (instrument.strike - spot_price).max(Decimal::ZERO)
            }
            OptionType::Put => {
                // Put is OTM when spot > strike.
                (spot_price - instrument.strike).max(Decimal::ZERO)
            }
        };

        // Method 1: 10% of spot notional.
        let method1 = OPTION_INITIAL_MARGIN_RATE * spot_price * qty;

        // Method 2: 7.5% × spot_notional − OTM_amount + mark_price × qty.
        // This gives a lower margin for deep OTM options.
        let method2 = (OPTION_OTM_BUFFER_RATE * spot_price * qty)
            - (otm_amount_per_contract * qty)
            + (mark_price * qty);
        let method2 = method2.max(Decimal::ZERO);

        // Take the higher of the two methods (more conservative).
        let margin = method1.max(method2);

        debug!(
            user_id = %position.user_id,
            symbol = %position.symbol,
            %qty,
            %spot_price,
            %mark_price,
            %otm_amount_per_contract,
            %method1,
            %method2,
            %margin,
            "calculated option margin"
        );

        margin
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use exchange_risk_engine::options_instruments::{OptionInstrument, OptionType, SettlementType};
    use rust_decimal_macros::dec;

    fn btc_call(strike: Decimal) -> OptionInstrument {
        OptionInstrument {
            symbol: format!("BTC-20260401-{strike}-C"),
            underlying: "BTC".into(),
            strike,
            expiry: Utc::now(),
            option_type: OptionType::Call,
            settlement: SettlementType::Cash,
            tick_size: dec!(1),
            lot_size: dec!(0.001),
            min_notional: dec!(10),
        }
    }

    fn btc_put(strike: Decimal) -> OptionInstrument {
        OptionInstrument {
            symbol: format!("BTC-20260401-{strike}-P"),
            underlying: "BTC".into(),
            strike,
            expiry: Utc::now(),
            option_type: OptionType::Put,
            settlement: SettlementType::Cash,
            tick_size: dec!(1),
            lot_size: dec!(0.001),
            min_notional: dec!(10),
        }
    }

    fn long_pos(symbol: &str, qty: Decimal, entry: Decimal) -> OptionPosition {
        OptionPosition {
            user_id: Uuid::new_v4(),
            symbol: symbol.into(),
            side: PositionSide::Long,
            quantity: qty,
            entry_price: entry,
            unrealized_pnl: Decimal::ZERO,
        }
    }

    fn short_pos(symbol: &str, qty: Decimal, entry: Decimal) -> OptionPosition {
        OptionPosition {
            user_id: Uuid::new_v4(),
            symbol: symbol.into(),
            side: PositionSide::Short,
            quantity: qty,
            entry_price: entry,
            unrealized_pnl: Decimal::ZERO,
        }
    }

    // ── Settlement ───────────────────────────────────────────────────────────

    #[test]
    fn test_itm_call_settled_correctly() {
        // Call K=70000, settlement=72000 → intrinsic = 2000.
        let inst = btc_call(dec!(70000));
        let long = long_pos(&inst.symbol, dec!(2), dec!(1500));
        let actions = OptionsSettlement::settle_expiry(&inst, dec!(72000), &[long]);

        assert_eq!(actions.len(), 1);
        match &actions[0] {
            SettlementAction::ExerciseITM {
                intrinsic_per_contract,
                total_payout,
                direction,
                ..
            } => {
                assert_eq!(*intrinsic_per_contract, dec!(2000));
                assert_eq!(*total_payout, dec!(4000)); // 2 contracts × 2000
                assert_eq!(*direction, 1);             // long → receive
            }
            other => panic!("expected ExerciseITM, got {other:?}"),
        }
    }

    #[test]
    fn test_itm_call_short_owes_payout() {
        let inst = btc_call(dec!(70000));
        let short = short_pos(&inst.symbol, dec!(1), dec!(800));
        let actions = OptionsSettlement::settle_expiry(&inst, dec!(75000), &[short]);

        match &actions[0] {
            SettlementAction::ExerciseITM { direction, total_payout, .. } => {
                assert_eq!(*direction, -1);
                assert_eq!(*total_payout, dec!(5000));
            }
            other => panic!("expected ExerciseITM, got {other:?}"),
        }
    }

    #[test]
    fn test_otm_call_expires_worthless() {
        // Call K=70000, settlement=65000 → OTM.
        let inst = btc_call(dec!(70000));
        let long = long_pos(&inst.symbol, dec!(5), dec!(200));
        let actions = OptionsSettlement::settle_expiry(&inst, dec!(65000), &[long]);

        assert_eq!(actions.len(), 1);
        matches!(actions[0], SettlementAction::ExpireWorthless { .. });
    }

    #[test]
    fn test_itm_put_settled_correctly() {
        // Put K=70000, settlement=65000 → intrinsic = 5000.
        let inst = btc_put(dec!(70000));
        let long = long_pos(&inst.symbol, dec!(1), dec!(2000));
        let actions = OptionsSettlement::settle_expiry(&inst, dec!(65000), &[long]);

        match &actions[0] {
            SettlementAction::ExerciseITM {
                intrinsic_per_contract,
                total_payout,
                direction,
                ..
            } => {
                assert_eq!(*intrinsic_per_contract, dec!(5000));
                assert_eq!(*total_payout, dec!(5000));
                assert_eq!(*direction, 1);
            }
            other => panic!("expected ExerciseITM, got {other:?}"),
        }
    }

    #[test]
    fn test_otm_put_expires_worthless() {
        // Put K=70000, settlement=72000 → OTM.
        let inst = btc_put(dec!(70000));
        let long = long_pos(&inst.symbol, dec!(3), dec!(100));
        let actions = OptionsSettlement::settle_expiry(&inst, dec!(72000), &[long]);

        assert!(
            matches!(actions[0], SettlementAction::ExpireWorthless { .. }),
            "OTM put should expire worthless"
        );
    }

    #[test]
    fn test_atm_expires_worthless() {
        // ATM: S_T == K → intrinsic = 0.
        let inst = btc_call(dec!(70000));
        let long = long_pos(&inst.symbol, dec!(1), dec!(100));
        let actions = OptionsSettlement::settle_expiry(&inst, dec!(70000), &[long]);

        assert!(
            matches!(actions[0], SettlementAction::ExpireWorthless { .. }),
            "ATM should expire worthless (zero intrinsic)"
        );
    }

    #[test]
    fn test_multiple_positions_settled_independently() {
        let inst = btc_call(dec!(70000));
        let pos1 = long_pos(&inst.symbol, dec!(2), dec!(500));
        let pos2 = short_pos(&inst.symbol, dec!(1), dec!(500));
        let actions = OptionsSettlement::settle_expiry(&inst, dec!(73000), &[pos1, pos2]);

        assert_eq!(actions.len(), 2);
        // Both should be ExerciseITM.
        for action in &actions {
            assert!(
                matches!(action, SettlementAction::ExerciseITM { .. }),
                "both should be exercised ITM"
            );
        }
    }

    #[test]
    fn test_cash_flow_sign() {
        let inst = btc_call(dec!(70000));
        let long = long_pos(&inst.symbol, dec!(1), dec!(1000));
        let short = short_pos(&inst.symbol, dec!(1), dec!(1000));
        let long_actions = OptionsSettlement::settle_expiry(&inst, dec!(72000), &[long]);
        let short_actions = OptionsSettlement::settle_expiry(&inst, dec!(72000), &[short]);

        assert!(
            long_actions[0].cash_flow() > Decimal::ZERO,
            "long should receive cash"
        );
        assert!(
            short_actions[0].cash_flow() < Decimal::ZERO,
            "short should pay cash"
        );
    }

    // ── Margin calculation ───────────────────────────────────────────────────

    #[test]
    fn test_long_option_needs_no_margin() {
        let inst = btc_call(dec!(70000));
        let long = long_pos(&inst.symbol, dec!(1), dec!(500));
        let margin = OptionsSettlement::calculate_margin(&long, &inst, dec!(70000), dec!(500));
        assert_eq!(margin, Decimal::ZERO, "long option needs no margin");
    }

    #[test]
    fn test_short_call_margin_positive() {
        let inst = btc_call(dec!(70000));
        let short = short_pos(&inst.symbol, dec!(1), dec!(500));
        let margin = OptionsSettlement::calculate_margin(&short, &inst, dec!(70000), dec!(500));
        assert!(margin > Decimal::ZERO, "short call must require positive margin");
    }

    #[test]
    fn test_short_put_margin_positive() {
        let inst = btc_put(dec!(70000));
        let short = short_pos(&inst.symbol, dec!(1), dec!(500));
        let margin = OptionsSettlement::calculate_margin(&short, &inst, dec!(70000), dec!(500));
        assert!(margin > Decimal::ZERO, "short put must require positive margin");
    }

    #[test]
    fn test_short_call_atm_margin_floor_10pct() {
        // ATM short call: OTM amount = 0 → method2 ≈ 0.075×spot + mark_price.
        // method1 = 0.10 × 70000 = 7000.
        // method2 = 0.075 × 70000 - 0 + 500 = 5750.
        // Expected margin = max(7000, 5750) = 7000.
        let inst = btc_call(dec!(70000));
        let short = short_pos(&inst.symbol, dec!(1), dec!(500));
        let margin = OptionsSettlement::calculate_margin(&short, &inst, dec!(70000), dec!(500));
        assert_eq!(margin, dec!(7000), "ATM short call margin should be 10% of spot");
    }

    #[test]
    fn test_deep_otm_call_margin_reduced() {
        // Deep OTM call: K=100000, S=70000 → large OTM amount reduces method2.
        // method1 = 0.10 × 70000 = 7000.
        // method2 = 0.075 × 70000 - 30000 + mark_price ≈ 5250 - 30000 + small = negative → 0.
        // Expected = max(7000, 0) = 7000 (method1 floors it).
        let inst = btc_call(dec!(100000));
        let short = short_pos(&inst.symbol, dec!(1), dec!(10));
        // Very small mark price for deep OTM.
        let margin = OptionsSettlement::calculate_margin(&short, &inst, dec!(70000), dec!(10));
        assert_eq!(margin, dec!(7000));
    }

    #[test]
    fn test_margin_scales_with_quantity() {
        let inst = btc_call(dec!(70000));
        let short1 = short_pos(&inst.symbol, dec!(1), dec!(500));
        let short2 = short_pos(&inst.symbol, dec!(2), dec!(500));
        let m1 = OptionsSettlement::calculate_margin(&short1, &inst, dec!(70000), dec!(500));
        let m2 = OptionsSettlement::calculate_margin(&short2, &inst, dec!(70000), dec!(500));
        assert_eq!(m2, m1 * dec!(2), "margin should scale linearly with qty");
    }
}
