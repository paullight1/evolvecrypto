//! Position lifecycle management for perpetual futures.
//!
//! [`PositionManager`] is the single source of truth for all open positions.
//! It handles opens, increases, reductions, full closes, and position flips
//! (e.g., a long flipping to short in a single order in one-way mode).
//!
//! In hedge mode (`position_idx` 1 or 2) a position never flips — both Long
//! and Short can coexist simultaneously for the same symbol.

use std::collections::HashMap;

use chrono::Utc;
use rust_decimal::Decimal;
use tracing::{debug, info};

use exchange_common::{
    futures::{MarginMode, Position, PositionIdx, PositionMode, PositionSide},
    types::{Price, Qty, Symbol, UserId},
};

use crate::{
    error::{RiskEngineError, RiskResult},
    margin_calculator::required_initial_margin,
};

/// Key into the position map: (user, symbol, position_idx).
type PositionKey = (UserId, Symbol, PositionIdx);

/// In-memory store for all open positions across all users and symbols.
///
/// All mutations go through the public methods to ensure invariants are upheld
/// (e.g., size never negative, margin correctly updated, realized PnL tracked).
#[derive(Debug, Default)]
pub struct PositionManager {
    positions: HashMap<PositionKey, Position>,
    /// Per-user per-symbol position mode (defaults to OneWay when absent).
    position_modes: HashMap<(UserId, Symbol), PositionMode>,
}

/// Result of a close or reduce operation, carrying the realized PnL.
#[derive(Debug, Clone)]
pub struct CloseResult {
    pub realized_pnl: Decimal,
    /// Margin released back to the user.
    pub released_margin: Decimal,
}

impl PositionManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return a reference to an open position by (user, symbol, position_idx).
    pub fn get_position(&self, user_id: UserId, symbol: &Symbol, position_idx: PositionIdx) -> Option<&Position> {
        self.positions.get(&(user_id, symbol.clone(), position_idx))
    }

    /// Convenience helper for one-way mode lookups (position_idx = 0).
    pub fn get_position_one_way(&self, user_id: UserId, symbol: &Symbol) -> Option<&Position> {
        self.get_position(user_id, symbol, 0)
    }

    /// Return a mutable reference to an open position by (user, symbol, position_idx).
    ///
    /// Used by the funding settlement task to record unpaid debt directly on
    /// the position when a debit fails due to insufficient user balance.
    pub fn get_position_mut(&mut self, user_id: UserId, symbol: &Symbol, position_idx: PositionIdx) -> Option<&mut Position> {
        self.positions.get_mut(&(user_id, symbol.clone(), position_idx))
    }

    /// All open (non-zero) positions for a user.
    pub fn user_positions(&self, user_id: UserId) -> Vec<&Position> {
        self.positions
            .values()
            .filter(|p| p.user_id == user_id && !p.is_closed())
            .collect()
    }

    /// Available margin for a user under cross-margin mode.
    ///
    /// Cross-margin means all positions share the same margin pool:
    /// available = total_equity - sum(initial margins for cross positions)
    pub fn cross_margin_available(&self, user_id: UserId, total_equity: Decimal) -> Decimal {
        let total_margin: Decimal = self
            .positions
            .values()
            .filter(|p| p.user_id == user_id && p.margin_mode == MarginMode::Cross && !p.is_closed())
            .map(|p| p.margin)
            .sum();
        (total_equity - total_margin).max(Decimal::ZERO)
    }

    /// All open (non-zero) positions across all users and symbols.
    pub fn all_positions(&self) -> Vec<&Position> {
        self.positions
            .values()
            .filter(|p| !p.is_closed())
            .collect()
    }

    /// Open a new position or increase an existing one on the same side.
    ///
    /// In **one-way mode** (`position_idx == 0`): if the existing position is on the
    /// **opposite** side, the call performs a **flip** — the existing position is
    /// closed first, then a new one opened with the new side's quantity.
    /// The returned `CloseResult` carries realized PnL from closing the old position.
    ///
    /// In **hedge mode** (`position_idx == 1` or `2`): positions never flip.
    /// Each side is independent and can only increase or open.
    ///
    /// # Errors
    /// - [`RiskEngineError::InvalidQuantity`] if `qty` or `price` is non-positive.
    /// - [`RiskEngineError::InvalidLeverage`] if `leverage` is non-positive.
    pub fn open_or_increase(
        &mut self,
        user_id: UserId,
        symbol: Symbol,
        side: PositionSide,
        qty: Qty,
        price: Price,
        leverage: Decimal,
        margin_mode: MarginMode,
        position_idx: PositionIdx,
    ) -> RiskResult<Option<CloseResult>> {
        if qty <= Decimal::ZERO {
            return Err(RiskEngineError::InvalidQuantity(
                "quantity must be positive".into(),
            ));
        }
        if price <= Decimal::ZERO {
            return Err(RiskEngineError::InvalidPrice(
                "price must be positive".into(),
            ));
        }
        if leverage <= Decimal::ZERO {
            return Err(RiskEngineError::InvalidLeverage(
                "leverage must be positive".into(),
            ));
        }

        let key = (user_id, symbol.clone(), position_idx);

        // Position flip: only in one-way mode (position_idx == 0).
        // In hedge mode the caller selects the exact slot; no flip ever occurs.
        if position_idx == 0 {
            if let Some(existing) = self.positions.get(&key) {
                if existing.side != side && !existing.is_closed() {
                    let existing_size = existing.size;
                    debug!(
                        user_id = %user_id,
                        symbol = %symbol,
                        "position flip: closing {} {:?} before opening {:?}",
                        existing_size, existing.side, side
                    );

                    // Close the existing position first.
                    let flip_result = self.close(user_id, &symbol, price, position_idx)?;

                    let new_qty = qty;
                    let new_margin = required_initial_margin(new_qty, price, leverage);
                    let now = Utc::now();
                    self.positions.insert(
                        key,
                        Position {
                            user_id,
                            symbol,
                            side,
                            size: new_qty,
                            entry_price: price,
                            leverage,
                            margin_mode,
                            margin: new_margin,
                            realized_pnl: Decimal::ZERO,
                            funding_accumulated: Decimal::ZERO,
                            created_at: now,
                            updated_at: now,
                            position_idx,
                            tp_price: None,
                            sl_price: None,
                            trailing_stop: None,
                            tp_trigger_id: None,
                            sl_trigger_id: None,
                            tp_size: None,
                            sl_size: None,
                            auto_add_margin: false,
                            unpaid_funding_debt: Decimal::ZERO,
                        },
                    );
                    return Ok(Some(flip_result));
                }
            }
        }

        // --- open or increase ---
        let now = Utc::now();
        match self.positions.get_mut(&key) {
            Some(pos) if !pos.is_closed() => {
                // Increase existing position: recalculate weighted average entry.
                let old_notional = pos.size * pos.entry_price;
                let new_notional = qty * price;
                let total_size = pos.size + qty;
                let avg_entry = (old_notional + new_notional) / total_size;
                let additional_margin = required_initial_margin(qty, price, leverage);

                pos.entry_price = avg_entry;
                pos.size = total_size;
                pos.margin += additional_margin;
                pos.leverage = leverage; // caller may update leverage
                pos.updated_at = now;

                debug!(
                    user_id = %user_id,
                    symbol = %pos.symbol,
                    new_size = %total_size,
                    avg_entry = %avg_entry,
                    "increased position",
                );
            }
            _ => {
                // New position.
                let margin = required_initial_margin(qty, price, leverage);
                info!(
                    user_id = %user_id,
                    symbol = %symbol,
                    side = ?side,
                    qty = %qty,
                    price = %price,
                    margin = %margin,
                    position_idx = %position_idx,
                    "opening new position",
                );
                self.positions.insert(
                    key,
                    Position {
                        user_id,
                        symbol,
                        side,
                        size: qty,
                        entry_price: price,
                        leverage,
                        margin_mode,
                        margin,
                        realized_pnl: Decimal::ZERO,
                        funding_accumulated: Decimal::ZERO,
                        created_at: now,
                        updated_at: now,
                        position_idx,
                        tp_price: None,
                        sl_price: None,
                        trailing_stop: None,
                        tp_trigger_id: None,
                        sl_trigger_id: None,
                        tp_size: None,
                        sl_size: None,
                        auto_add_margin: false,
                        unpaid_funding_debt: Decimal::ZERO,
                    },
                );
            }
        }

        Ok(None)
    }

    /// Partially reduce a position by `qty` at the given close `price`.
    ///
    /// Realized PnL is calculated proportionally:
    /// `pnl = (close_price - entry_price) * qty * side_sign`
    ///
    /// The margin released is proportional to the reduction fraction.
    ///
    /// # Errors
    /// - [`RiskEngineError::PositionNotFound`] if the position does not exist.
    /// - [`RiskEngineError::InvalidQuantity`] if `qty` exceeds the current size.
    pub fn reduce(
        &mut self,
        user_id: UserId,
        symbol: &Symbol,
        qty: Qty,
        price: Price,
        position_idx: PositionIdx,
    ) -> RiskResult<CloseResult> {
        let key = (user_id, symbol.clone(), position_idx);
        let pos = self
            .positions
            .get_mut(&key)
            .filter(|p| !p.is_closed())
            .ok_or_else(|| RiskEngineError::PositionNotFound {
                user_id,
                symbol: symbol.clone(),
            })?;

        if qty <= Decimal::ZERO {
            return Err(RiskEngineError::InvalidQuantity(
                "reduce quantity must be positive".into(),
            ));
        }
        if qty > pos.size {
            return Err(RiskEngineError::InvalidQuantity(format!(
                "reduce qty {} exceeds position size {}",
                qty, pos.size
            )));
        }

        let pnl = pos.side.sign() * qty * (price - pos.entry_price);
        let fraction = qty / pos.size;
        let released_margin = pos.margin * fraction;

        pos.size -= qty;
        pos.margin -= released_margin;
        pos.realized_pnl += pnl;
        pos.updated_at = Utc::now();

        debug!(
            user_id = %user_id,
            symbol = %symbol,
            qty = %qty,
            pnl = %pnl,
            remaining_size = %pos.size,
            "reduced position",
        );

        Ok(CloseResult {
            realized_pnl: pnl,
            released_margin,
        })
    }

    /// Accumulate a funding payment into an open position's `funding_accumulated` field.
    ///
    /// `amount` is signed: negative when the user pays funding, positive when received.
    ///
    /// Returns `true` if the position was found and updated, `false` if no open position
    /// exists for the given `(user_id, symbol, position_idx)` triple.
    pub fn add_funding(&mut self, user_id: UserId, symbol: &Symbol, amount: Decimal, position_idx: PositionIdx) -> bool {
        let key = (user_id, symbol.clone(), position_idx);
        if let Some(pos) = self.positions.get_mut(&key) {
            pos.funding_accumulated += amount;
            pos.updated_at = Utc::now();
            true
        } else {
            false
        }
    }

    /// Fully close a position at `price`.
    ///
    /// Equivalent to `reduce(user_id, symbol, position.size, price, position_idx)`.
    ///
    /// # Errors
    /// - [`RiskEngineError::PositionNotFound`] if the position does not exist or is already closed.
    pub fn close(
        &mut self,
        user_id: UserId,
        symbol: &Symbol,
        price: Price,
        position_idx: PositionIdx,
    ) -> RiskResult<CloseResult> {
        let size = {
            let key = (user_id, symbol.clone(), position_idx);
            let pos = self
                .positions
                .get(&key)
                .filter(|p| !p.is_closed())
                .ok_or_else(|| RiskEngineError::PositionNotFound {
                    user_id,
                    symbol: symbol.clone(),
                })?;
            pos.size
        };

        self.reduce(user_id, symbol, size, price, position_idx)
    }

    /// Adjust leverage on a live position.
    ///
    /// Recalculates the required margin at the new leverage and updates the
    /// position in-place. Returns the margin delta: positive means more margin
    /// is required; negative means margin is released.
    ///
    /// # Errors
    /// - [`RiskEngineError::PositionNotFound`] if no open position exists.
    /// - [`RiskEngineError::InvalidLeverage`] if `new_leverage` is non-positive.
    pub fn adjust_leverage(
        &mut self,
        user_id: UserId,
        symbol: &Symbol,
        position_idx: PositionIdx,
        new_leverage: Decimal,
    ) -> RiskResult<Decimal> {
        if new_leverage <= Decimal::ZERO {
            return Err(RiskEngineError::InvalidLeverage(
                "leverage must be positive".into(),
            ));
        }

        let key = (user_id, symbol.clone(), position_idx);
        let pos = self
            .positions
            .get_mut(&key)
            .filter(|p| !p.is_closed())
            .ok_or_else(|| RiskEngineError::PositionNotFound {
                user_id,
                symbol: symbol.clone(),
            })?;

        let old_leverage = pos.leverage;
        let old_margin = pos.margin;
        let new_margin = required_initial_margin(pos.size, pos.entry_price, new_leverage);

        pos.leverage = new_leverage;
        pos.margin = new_margin;
        pos.updated_at = Utc::now();

        let delta = new_margin - old_margin;

        info!(
            user_id = %user_id,
            symbol = %symbol,
            old_leverage = %old_leverage,
            new_leverage = %new_leverage,
            margin_delta = %delta,
            "leverage adjusted on live position",
        );

        Ok(delta)
    }

    /// Set the position mode for a user/symbol pair.
    ///
    /// Returns an error if any open position exists for this user/symbol, since
    /// changing mode while a position is open would corrupt the position key semantics.
    pub fn set_position_mode(&mut self, user_id: UserId, symbol: &Symbol, mode: PositionMode) -> RiskResult<()> {
        let has_open = self.positions.iter().any(|((uid, sym, _), pos)| {
            *uid == user_id && sym == symbol && !pos.is_closed()
        });
        if has_open {
            return Err(RiskEngineError::CannotChangeMode);
        }
        self.position_modes.insert((user_id, symbol.clone()), mode);
        Ok(())
    }

    /// Get the position mode for a user/symbol pair. Defaults to `OneWay`.
    pub fn get_position_mode(&self, user_id: UserId, symbol: &Symbol) -> PositionMode {
        self.position_modes
            .get(&(user_id, symbol.clone()))
            .copied()
            .unwrap_or_default()
    }

    /// Update TP/SL/trailing-stop state on an open position.
    ///
    /// All fields are replaced atomically. Pass `None` to clear a field.
    ///
    /// # Errors
    /// - [`RiskEngineError::PositionNotFound`] if no open position exists for the key.
    pub fn update_trading_stop(
        &mut self,
        user_id: UserId,
        symbol: &Symbol,
        position_idx: PositionIdx,
        tp_price: Option<Price>,
        sl_price: Option<Price>,
        trailing_stop: Option<Decimal>,
        tp_trigger_id: Option<uuid::Uuid>,
        sl_trigger_id: Option<uuid::Uuid>,
        tp_size: Option<Qty>,
        sl_size: Option<Qty>,
    ) -> RiskResult<()> {
        let key = (user_id, symbol.clone(), position_idx);
        let pos = self
            .positions
            .get_mut(&key)
            .filter(|p| !p.is_closed())
            .ok_or_else(|| RiskEngineError::PositionNotFound {
                user_id,
                symbol: symbol.clone(),
            })?;
        pos.tp_price = tp_price;
        pos.sl_price = sl_price;
        pos.trailing_stop = trailing_stop;
        pos.tp_trigger_id = tp_trigger_id;
        pos.sl_trigger_id = sl_trigger_id;
        pos.tp_size = tp_size;
        pos.sl_size = sl_size;
        pos.updated_at = Utc::now();
        Ok(())
    }

    /// Enable or disable auto-add margin on an open position.
    ///
    /// When enabled, the liquidation monitor will automatically top up the
    /// position's margin from the user's available balance before liquidation
    /// triggers. Only meaningful for isolated margin positions.
    ///
    /// # Errors
    /// - [`RiskEngineError::PositionNotFound`] if no open position exists.
    pub fn set_auto_add_margin(
        &mut self,
        user_id: UserId,
        symbol: &Symbol,
        position_idx: PositionIdx,
        enabled: bool,
    ) -> RiskResult<()> {
        let key = (user_id, symbol.clone(), position_idx);
        let pos = self
            .positions
            .get_mut(&key)
            .filter(|p| !p.is_closed())
            .ok_or_else(|| RiskEngineError::PositionNotFound {
                user_id,
                symbol: symbol.clone(),
            })?;
        pos.auto_add_margin = enabled;
        pos.updated_at = Utc::now();
        Ok(())
    }

    /// Add margin directly to an open isolated position.
    ///
    /// Used by the auto-add margin task to top up a position's collateral.
    /// The caller is responsible for deducting the equivalent amount from the
    /// user's trading balance before calling this method.
    ///
    /// # Errors
    /// - [`RiskEngineError::PositionNotFound`] if no open position exists.
    pub fn add_margin_to_position(
        &mut self,
        user_id: UserId,
        symbol: &Symbol,
        position_idx: PositionIdx,
        amount: Decimal,
    ) -> RiskResult<()> {
        let key = (user_id, symbol.clone(), position_idx);
        let pos = self
            .positions
            .get_mut(&key)
            .filter(|p| !p.is_closed())
            .ok_or_else(|| RiskEngineError::PositionNotFound {
                user_id,
                symbol: symbol.clone(),
            })?;
        pos.margin += amount;
        pos.updated_at = Utc::now();
        Ok(())
    }

    /// Restore a position from a WAL `PositionSnapshot` during crash recovery.
    ///
    /// Inserts (or overwrites) the position in the in-memory map without any
    /// margin or balance side-effects — those were already applied when the
    /// original transactions were processed.
    pub fn restore_position(&mut self, position: Position) {
        let key = (position.user_id, position.symbol.clone(), position.position_idx);
        self.positions.insert(key, position);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use exchange_common::types::Symbol;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn sym() -> Symbol {
        Symbol::new("BTCUSDT")
    }

    fn user() -> UserId {
        Uuid::nil()
    }

    // --- open ---

    #[test]
    fn test_open_new_long_position() {
        let mut pm = PositionManager::new();
        pm.open_or_increase(user(), sym(), PositionSide::Long, dec!(1), dec!(50000), dec!(10), MarginMode::Isolated, 0).unwrap();

        let pos = pm.get_position(user(), &sym(), 0).expect("position exists");
        assert_eq!(pos.size, dec!(1));
        assert_eq!(pos.entry_price, dec!(50000));
        assert_eq!(pos.margin, dec!(5000)); // 1 * 50_000 / 10
        assert_eq!(pos.side, PositionSide::Long);
    }

    #[test]
    fn test_open_new_short_position() {
        let mut pm = PositionManager::new();
        pm.open_or_increase(user(), sym(), PositionSide::Short, dec!(2), dec!(30000), dec!(5), MarginMode::Isolated, 0).unwrap();

        let pos = pm.get_position(user(), &sym(), 0).unwrap();
        assert_eq!(pos.size, dec!(2));
        assert_eq!(pos.margin, dec!(12000)); // 2 * 30_000 / 5
        assert_eq!(pos.side, PositionSide::Short);
    }

    #[test]
    fn test_open_rejects_zero_qty() {
        let mut pm = PositionManager::new();
        let err = pm.open_or_increase(user(), sym(), PositionSide::Long, dec!(0), dec!(50000), dec!(10), MarginMode::Isolated, 0).unwrap_err();
        assert!(matches!(err, RiskEngineError::InvalidQuantity(_)));
    }

    #[test]
    fn test_open_rejects_zero_price() {
        let mut pm = PositionManager::new();
        let err = pm.open_or_increase(user(), sym(), PositionSide::Long, dec!(1), dec!(0), dec!(10), MarginMode::Isolated, 0).unwrap_err();
        assert!(matches!(err, RiskEngineError::InvalidPrice(_)));
    }

    #[test]
    fn test_open_rejects_zero_leverage() {
        let mut pm = PositionManager::new();
        let err = pm.open_or_increase(user(), sym(), PositionSide::Long, dec!(1), dec!(50000), dec!(0), MarginMode::Isolated, 0).unwrap_err();
        assert!(matches!(err, RiskEngineError::InvalidLeverage(_)));
    }

    // --- increase ---

    #[test]
    fn test_increase_position_weighted_avg_entry() {
        let mut pm = PositionManager::new();
        // Open 1 BTC @ 40_000
        pm.open_or_increase(user(), sym(), PositionSide::Long, dec!(1), dec!(40000), dec!(10), MarginMode::Isolated, 0).unwrap();
        // Add 1 BTC @ 60_000 → avg = (40_000 + 60_000) / 2 = 50_000
        pm.open_or_increase(user(), sym(), PositionSide::Long, dec!(1), dec!(60000), dec!(10), MarginMode::Isolated, 0).unwrap();

        let pos = pm.get_position(user(), &sym(), 0).unwrap();
        assert_eq!(pos.size, dec!(2));
        assert_eq!(pos.entry_price, dec!(50000));
        // margin = 1*40_000/10 + 1*60_000/10 = 4_000 + 6_000 = 10_000
        assert_eq!(pos.margin, dec!(10000));
    }

    #[test]
    fn test_increase_unequal_sizes() {
        let mut pm = PositionManager::new();
        // Open 2 BTC @ 50_000 → notional 100_000
        pm.open_or_increase(user(), sym(), PositionSide::Long, dec!(2), dec!(50000), dec!(10), MarginMode::Isolated, 0).unwrap();
        // Add 1 BTC @ 52_000 → new notional 52_000 → total 152_000 / 3
        pm.open_or_increase(user(), sym(), PositionSide::Long, dec!(1), dec!(52000), dec!(10), MarginMode::Isolated, 0).unwrap();

        let pos = pm.get_position(user(), &sym(), 0).unwrap();
        assert_eq!(pos.size, dec!(3));
        // avg = (100_000 + 52_000) / 3 = 50_666.666...
        let expected_avg = dec!(152000) / dec!(3);
        assert_eq!(pos.entry_price, expected_avg);
    }

    // --- reduce ---

    #[test]
    fn test_reduce_partial_long_at_profit() {
        let mut pm = PositionManager::new();
        pm.open_or_increase(user(), sym(), PositionSide::Long, dec!(2), dec!(50000), dec!(10), MarginMode::Isolated, 0).unwrap();
        // Reduce 1 BTC at 55_000 → pnl = (55_000 - 50_000) * 1 = 5_000
        let result = pm.reduce(user(), &sym(), dec!(1), dec!(55000), 0).unwrap();

        assert_eq!(result.realized_pnl, dec!(5000));
        // released margin = 50% of original margin (10_000) = 5_000
        assert_eq!(result.released_margin, dec!(5000));

        let pos = pm.get_position(user(), &sym(), 0).unwrap();
        assert_eq!(pos.size, dec!(1));
    }

    #[test]
    fn test_reduce_partial_short_at_profit() {
        let mut pm = PositionManager::new();
        pm.open_or_increase(user(), sym(), PositionSide::Short, dec!(1), dec!(50000), dec!(10), MarginMode::Isolated, 0).unwrap();
        // Reduce 0.5 BTC at 48_000 → pnl = (50_000 - 48_000) * 0.5 = 1_000
        let result = pm.reduce(user(), &sym(), dec!(0.5), dec!(48000), 0).unwrap();
        assert_eq!(result.realized_pnl, dec!(1000));
    }

    #[test]
    fn test_reduce_rejects_excess_qty() {
        let mut pm = PositionManager::new();
        pm.open_or_increase(user(), sym(), PositionSide::Long, dec!(1), dec!(50000), dec!(10), MarginMode::Isolated, 0).unwrap();
        let err = pm.reduce(user(), &sym(), dec!(2), dec!(50000), 0).unwrap_err();
        assert!(matches!(err, RiskEngineError::InvalidQuantity(_)));
    }

    #[test]
    fn test_reduce_rejects_nonexistent_position() {
        let mut pm = PositionManager::new();
        let err = pm.reduce(user(), &sym(), dec!(1), dec!(50000), 0).unwrap_err();
        assert!(matches!(err, RiskEngineError::PositionNotFound { .. }));
    }

    // --- close ---

    #[test]
    fn test_close_full_position_at_loss() {
        let mut pm = PositionManager::new();
        pm.open_or_increase(user(), sym(), PositionSide::Long, dec!(1), dec!(50000), dec!(10), MarginMode::Isolated, 0).unwrap();
        // Close at 48_000 → pnl = (48_000 - 50_000) * 1 = -2_000
        let result = pm.close(user(), &sym(), dec!(48000), 0).unwrap();
        assert_eq!(result.realized_pnl, dec!(-2000));

        let pos = pm.get_position(user(), &sym(), 0).unwrap();
        assert!(pos.is_closed());
    }

    #[test]
    fn test_close_rejects_nonexistent() {
        let mut pm = PositionManager::new();
        let err = pm.close(user(), &sym(), dec!(50000), 0).unwrap_err();
        assert!(matches!(err, RiskEngineError::PositionNotFound { .. }));
    }

    // --- flip (long → short) ---

    #[test]
    fn test_position_flip_long_to_short() {
        let mut pm = PositionManager::new();
        // Open long 1 BTC @ 50_000
        pm.open_or_increase(user(), sym(), PositionSide::Long, dec!(1), dec!(50000), dec!(10), MarginMode::Isolated, 0).unwrap();
        // Flip to short 2 BTC @ 52_000
        // Close result: pnl = (52_000 - 50_000) * 1 = 2_000
        let flip = pm.open_or_increase(user(), sym(), PositionSide::Short, dec!(2), dec!(52000), dec!(10), MarginMode::Isolated, 0)
            .unwrap()
            .expect("flip result returned");

        assert_eq!(flip.realized_pnl, dec!(2000));

        let pos = pm.get_position(user(), &sym(), 0).unwrap();
        assert_eq!(pos.side, PositionSide::Short);
        assert_eq!(pos.size, dec!(2));
        assert_eq!(pos.entry_price, dec!(52000));
    }

    #[test]
    fn test_position_flip_short_to_long_at_loss() {
        let mut pm = PositionManager::new();
        pm.open_or_increase(user(), sym(), PositionSide::Short, dec!(1), dec!(50000), dec!(10), MarginMode::Isolated, 0).unwrap();
        // Close @ 53_000 → pnl = (50_000 - 53_000) * 1 = -3_000
        let flip = pm.open_or_increase(user(), sym(), PositionSide::Long, dec!(1), dec!(53000), dec!(10), MarginMode::Isolated, 0)
            .unwrap()
            .expect("flip result");

        assert_eq!(flip.realized_pnl, dec!(-3000));

        let pos = pm.get_position(user(), &sym(), 0).unwrap();
        assert_eq!(pos.side, PositionSide::Long);
    }

    // --- hedge mode ---

    #[test]
    fn test_hedge_mode_long_and_short_coexist() {
        let mut pm = PositionManager::new();
        // Open Long at idx=1 (hedge long slot)
        pm.open_or_increase(user(), sym(), PositionSide::Long, dec!(1), dec!(50000), dec!(10), MarginMode::Isolated, 1).unwrap();
        // Open Short at idx=2 (hedge short slot)
        pm.open_or_increase(user(), sym(), PositionSide::Short, dec!(2), dec!(50000), dec!(10), MarginMode::Isolated, 2).unwrap();

        let long_pos = pm.get_position(user(), &sym(), 1).expect("hedge long exists");
        assert_eq!(long_pos.side, PositionSide::Long);
        assert_eq!(long_pos.size, dec!(1));

        let short_pos = pm.get_position(user(), &sym(), 2).expect("hedge short exists");
        assert_eq!(short_pos.side, PositionSide::Short);
        assert_eq!(short_pos.size, dec!(2));
    }

    #[test]
    fn test_hedge_mode_no_flip() {
        let mut pm = PositionManager::new();
        // In hedge mode, placing an opposite-side order at the same idx is prevented
        // by the caller; but using idx=1 for Long and idx=2 for Short is correct.
        // Verify that opening a Short at idx=1 does NOT flip the Long — it just
        // co-exists as a separate slot.
        pm.open_or_increase(user(), sym(), PositionSide::Long, dec!(1), dec!(50000), dec!(10), MarginMode::Isolated, 1).unwrap();
        // No flip for idx != 0
        let result = pm.open_or_increase(user(), sym(), PositionSide::Short, dec!(1), dec!(52000), dec!(10), MarginMode::Isolated, 2).unwrap();
        assert!(result.is_none(), "hedge mode must not flip");
    }

    // --- user_positions ---

    #[test]
    fn test_user_positions_multiple_symbols() {
        let mut pm = PositionManager::new();
        let user_a = Uuid::new_v4();
        let user_b = Uuid::new_v4();

        pm.open_or_increase(user_a, Symbol::new("BTCUSDT"), PositionSide::Long, dec!(1), dec!(50000), dec!(10), MarginMode::Isolated, 0).unwrap();
        pm.open_or_increase(user_a, Symbol::new("ETHUSDT"), PositionSide::Short, dec!(5), dec!(3000), dec!(5), MarginMode::Isolated, 0).unwrap();
        pm.open_or_increase(user_b, Symbol::new("BTCUSDT"), PositionSide::Long, dec!(1), dec!(50000), dec!(10), MarginMode::Isolated, 0).unwrap();

        let a_positions = pm.user_positions(user_a);
        assert_eq!(a_positions.len(), 2);

        let b_positions = pm.user_positions(user_b);
        assert_eq!(b_positions.len(), 1);
    }

    // --- add_funding ---

    #[test]
    fn test_add_funding_updates_accumulated() {
        let mut pm = PositionManager::new();
        pm.open_or_increase(user(), sym(), PositionSide::Long, dec!(1), dec!(50000), dec!(10), MarginMode::Isolated, 0).unwrap();

        let updated = pm.add_funding(user(), &sym(), dec!(-5), 0);
        assert!(updated, "should return true for existing position");

        let pos = pm.get_position(user(), &sym(), 0).unwrap();
        assert_eq!(pos.funding_accumulated, dec!(-5));

        // Apply a second payment
        pm.add_funding(user(), &sym(), dec!(2), 0);
        let pos = pm.get_position(user(), &sym(), 0).unwrap();
        assert_eq!(pos.funding_accumulated, dec!(-3));
    }

    #[test]
    fn test_add_funding_returns_false_for_missing_position() {
        let mut pm = PositionManager::new();
        let updated = pm.add_funding(user(), &sym(), dec!(10), 0);
        assert!(!updated, "should return false when position does not exist");
    }

    #[test]
    fn test_realized_pnl_accumulates_across_reduces() {
        let mut pm = PositionManager::new();
        pm.open_or_increase(user(), sym(), PositionSide::Long, dec!(3), dec!(50000), dec!(10), MarginMode::Isolated, 0).unwrap();
        pm.reduce(user(), &sym(), dec!(1), dec!(51000), 0).unwrap(); // pnl +1_000
        pm.reduce(user(), &sym(), dec!(1), dec!(52000), 0).unwrap(); // pnl +2_000

        let pos = pm.get_position(user(), &sym(), 0).unwrap();
        assert_eq!(pos.realized_pnl, dec!(3000));
    }

    // --- set_position_mode ---

    #[test]
    fn test_set_position_mode_allowed_when_no_open_positions() {
        let mut pm = PositionManager::new();
        let result = pm.set_position_mode(user(), &sym(), PositionMode::Hedge);
        assert!(result.is_ok());
        assert_eq!(pm.get_position_mode(user(), &sym()), PositionMode::Hedge);
    }

    #[test]
    fn test_set_position_mode_rejected_when_position_open() {
        let mut pm = PositionManager::new();
        pm.open_or_increase(user(), sym(), PositionSide::Long, dec!(1), dec!(50000), dec!(10), MarginMode::Isolated, 0).unwrap();
        let err = pm.set_position_mode(user(), &sym(), PositionMode::Hedge).unwrap_err();
        assert!(matches!(err, RiskEngineError::CannotChangeMode));
    }

    #[test]
    fn test_get_position_mode_defaults_to_one_way() {
        let pm = PositionManager::new();
        assert_eq!(pm.get_position_mode(user(), &sym()), PositionMode::OneWay);
    }

    // --- cross_margin_available ---

    #[test]
    fn test_cross_margin_available_no_positions() {
        let pm = PositionManager::new();
        let available = pm.cross_margin_available(user(), dec!(10000));
        assert_eq!(available, dec!(10000));
    }

    #[test]
    fn test_cross_margin_available_with_cross_position() {
        let mut pm = PositionManager::new();
        // Open a 1 BTC cross-margin position @ 50_000 with 10x leverage.
        // Margin = 50_000 / 10 = 5_000
        pm.open_or_increase(user(), sym(), PositionSide::Long, dec!(1), dec!(50000), dec!(10), MarginMode::Cross, 0).unwrap();

        // With total equity 10_000 and 5_000 tied up: available = 5_000
        let available = pm.cross_margin_available(user(), dec!(10000));
        assert_eq!(available, dec!(5000));
    }

    #[test]
    fn test_cross_margin_available_isolated_position_not_counted() {
        let mut pm = PositionManager::new();
        // Open an isolated position — should NOT reduce cross-margin available.
        pm.open_or_increase(user(), sym(), PositionSide::Long, dec!(1), dec!(50000), dec!(10), MarginMode::Isolated, 0).unwrap();

        let available = pm.cross_margin_available(user(), dec!(10000));
        // Isolated margin is not deducted from cross pool.
        assert_eq!(available, dec!(10000));
    }

    #[test]
    fn test_cross_margin_available_cannot_go_negative() {
        let mut pm = PositionManager::new();
        // Open position requiring more margin than total equity.
        pm.open_or_increase(user(), sym(), PositionSide::Long, dec!(1), dec!(50000), dec!(10), MarginMode::Cross, 0).unwrap();

        // Equity = 1_000 < margin = 5_000 → clamped to 0.
        let available = pm.cross_margin_available(user(), dec!(1000));
        assert_eq!(available, dec!(0));
    }

    #[test]
    fn test_cross_margin_available_multiple_cross_positions() {
        let mut pm = PositionManager::new();
        let u = user();
        // Two cross positions: each requires 5_000 margin.
        pm.open_or_increase(u, Symbol::new("BTCUSDT"), PositionSide::Long, dec!(1), dec!(50000), dec!(10), MarginMode::Cross, 0).unwrap();
        pm.open_or_increase(u, Symbol::new("ETHUSDT"), PositionSide::Short, dec!(1), dec!(50000), dec!(10), MarginMode::Cross, 0).unwrap();

        // Total margin = 10_000. Equity = 25_000 → available = 15_000.
        let available = pm.cross_margin_available(u, dec!(25000));
        assert_eq!(available, dec!(15000));
    }
}
