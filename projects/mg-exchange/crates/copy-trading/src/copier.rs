//! Trade copier — generates follower orders from a master's trade.
//!
//! When a master trader executes a trade, [`TradeCopier::generate_copy_orders`]
//! fans that trade out to every active follower, scaling quantity proportionally
//! to each follower's allocation relative to the master's equity.
//!
//! Proportional sizing formula:
//! ```text
//! follower_qty = master_qty × (follower_allocation / master_equity)
//! ```
//!
//! Orders with a computed quantity ≤ 0 are silently dropped.

use rust_decimal::Decimal;
use rust_decimal::prelude::Zero;
use tracing::debug;

use exchange_common::types::{OrderType, Side, Symbol, UserId};

use crate::follower::FollowRelation;

// ──────────────────────────────────────────────────────────────────────────────
// Input / output types
// ──────────────────────────────────────────────────────────────────────────────

/// A trade executed by a master trader that should be copied to followers.
#[derive(Debug, Clone)]
pub struct MasterTrade {
    pub master_id: UserId,
    pub symbol: Symbol,
    pub side: Side,
    pub qty: Decimal,
    pub price: Decimal,
    pub order_type: OrderType,
}

/// A copy order to be placed on behalf of a follower.
#[derive(Debug, Clone)]
pub struct CopyOrder {
    pub follower_id: UserId,
    pub symbol: Symbol,
    pub side: Side,
    pub qty: Decimal,
    pub order_type: OrderType,
    /// Limit price; `None` for market orders.
    pub price: Option<Decimal>,
}

// ──────────────────────────────────────────────────────────────────────────────
// Copier
// ──────────────────────────────────────────────────────────────────────────────

/// Stateless helper that converts a master trade into a set of follower orders.
pub struct TradeCopier;

impl TradeCopier {
    /// Generate copy orders for all active, proportional followers of the master.
    ///
    /// # Arguments
    /// * `master_trade`  — the trade to replicate.
    /// * `followers`     — slice of *all* follow relations for this master
    ///                     (active and inactive; inactive ones are skipped).
    /// * `master_equity` — master's total account equity in USDT, used as the
    ///                     denominator for proportional sizing.
    ///
    /// # Returns
    /// A `Vec<CopyOrder>` — one per eligible follower. Relations that are
    /// inactive, non-proportional, or result in a zero/negative quantity are
    /// omitted.
    pub fn generate_copy_orders(
        master_trade: &MasterTrade,
        followers: &[FollowRelation],
        master_equity: Decimal,
    ) -> Vec<CopyOrder> {
        if master_equity <= Decimal::zero() {
            debug!(
                master_id = %master_trade.master_id,
                "generate_copy_orders: master_equity is non-positive, skipping"
            );
            return vec![];
        }

        followers
            .iter()
            .filter(|r| r.is_active && r.proportional)
            .filter_map(|r| {
                let follower_qty =
                    master_trade.qty * (r.allocation / master_equity);

                if follower_qty <= Decimal::zero() {
                    debug!(
                        follower_id = %r.follower_id,
                        %follower_qty,
                        "copy order qty non-positive, skipping"
                    );
                    return None;
                }

                let price = match master_trade.order_type {
                    OrderType::Limit => Some(master_trade.price),
                    OrderType::Market => None,
                };

                Some(CopyOrder {
                    follower_id: r.follower_id,
                    symbol: master_trade.symbol.clone(),
                    side: master_trade.side,
                    qty: follower_qty,
                    order_type: master_trade.order_type,
                    price,
                })
            })
            .collect()
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    use exchange_common::types::{OrderType, Side, Symbol};

    use crate::follower::FollowRelation;

    use super::*;

    fn uid() -> UserId {
        Uuid::new_v4()
    }

    fn btc() -> Symbol {
        Symbol::new("BTCUSDT")
    }

    fn make_relation(
        follower_id: UserId,
        master_id: UserId,
        allocation: Decimal,
        proportional: bool,
        is_active: bool,
    ) -> FollowRelation {
        FollowRelation {
            id: Uuid::new_v4(),
            follower_id,
            master_id,
            allocation,
            used_allocation: dec!(0),
            proportional,
            stop_loss_pct: None,
            total_pnl: dec!(0),
            is_active,
            created_at: chrono::Utc::now(),
        }
    }

    fn master_trade(master_id: UserId, qty: Decimal, order_type: OrderType) -> MasterTrade {
        MasterTrade {
            master_id,
            symbol: btc(),
            side: Side::Buy,
            qty,
            price: dec!(50000),
            order_type,
        }
    }

    // ── proportional sizing ───────────────────────────────────────────────────

    #[test]
    fn proportional_sizing_correct() {
        let m = uid();
        let f = uid();
        // master equity: 10 000 USDT; follower allocation: 1 000 USDT → ratio 0.1
        // master qty: 2 BTC → follower qty: 0.2 BTC
        let rel = make_relation(f, m, dec!(1000), true, true);
        let trade = master_trade(m, dec!(2), OrderType::Limit);
        let orders = TradeCopier::generate_copy_orders(&trade, &[rel], dec!(10000));

        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].follower_id, f);
        assert_eq!(orders[0].qty, dec!(0.2));
    }

    #[test]
    fn multiple_followers_all_get_orders() {
        let m = uid();
        let f1 = uid();
        let f2 = uid();
        let f3 = uid();
        let followers = vec![
            make_relation(f1, m, dec!(500), true, true),
            make_relation(f2, m, dec!(1000), true, true),
            make_relation(f3, m, dec!(250), true, true),
        ];
        // master equity 5 000; master qty 1
        let trade = master_trade(m, dec!(1), OrderType::Market);
        let orders = TradeCopier::generate_copy_orders(&trade, &followers, dec!(5000));

        assert_eq!(orders.len(), 3);
        // Verify ratios.
        let qty_for = |fid: UserId| orders.iter().find(|o| o.follower_id == fid).unwrap().qty;
        assert_eq!(qty_for(f1), dec!(0.1));   // 500/5000
        assert_eq!(qty_for(f2), dec!(0.2));   // 1000/5000
        assert_eq!(qty_for(f3), dec!(0.05));  // 250/5000
    }

    // ── inactive relations skipped ────────────────────────────────────────────

    #[test]
    fn inactive_relation_is_skipped() {
        let m = uid();
        let f = uid();
        let rel = make_relation(f, m, dec!(1000), true, false); // inactive
        let trade = master_trade(m, dec!(1), OrderType::Market);
        let orders = TradeCopier::generate_copy_orders(&trade, &[rel], dec!(10000));
        assert!(orders.is_empty());
    }

    // ── non-proportional relations skipped ───────────────────────────────────

    #[test]
    fn non_proportional_relation_is_skipped() {
        let m = uid();
        let f = uid();
        let rel = make_relation(f, m, dec!(1000), false, true); // proportional=false
        let trade = master_trade(m, dec!(1), OrderType::Market);
        let orders = TradeCopier::generate_copy_orders(&trade, &[rel], dec!(10000));
        assert!(orders.is_empty());
    }

    // ── zero / non-positive equity ────────────────────────────────────────────

    #[test]
    fn zero_master_equity_returns_no_orders() {
        let m = uid();
        let rel = make_relation(uid(), m, dec!(1000), true, true);
        let trade = master_trade(m, dec!(1), OrderType::Market);
        let orders = TradeCopier::generate_copy_orders(&trade, &[rel], dec!(0));
        assert!(orders.is_empty());
    }

    // ── order type preservation ───────────────────────────────────────────────

    #[test]
    fn limit_order_preserves_price() {
        let m = uid();
        let f = uid();
        let rel = make_relation(f, m, dec!(1000), true, true);
        let trade = MasterTrade {
            master_id: m,
            symbol: btc(),
            side: Side::Sell,
            qty: dec!(1),
            price: dec!(48000),
            order_type: OrderType::Limit,
        };
        let orders = TradeCopier::generate_copy_orders(&trade, &[rel], dec!(10000));
        assert_eq!(orders[0].price, Some(dec!(48000)));
        assert_eq!(orders[0].order_type, OrderType::Limit);
        assert_eq!(orders[0].side, Side::Sell);
    }

    #[test]
    fn market_order_has_no_price() {
        let m = uid();
        let f = uid();
        let rel = make_relation(f, m, dec!(1000), true, true);
        let trade = master_trade(m, dec!(1), OrderType::Market);
        let orders = TradeCopier::generate_copy_orders(&trade, &[rel], dec!(10000));
        assert_eq!(orders[0].price, None);
        assert_eq!(orders[0].order_type, OrderType::Market);
    }

    // ── mixed active/inactive ─────────────────────────────────────────────────

    #[test]
    fn only_active_proportional_followers_get_orders() {
        let m = uid();
        let active = make_relation(uid(), m, dec!(1000), true, true);
        let inactive = make_relation(uid(), m, dec!(1000), true, false);
        let fixed = make_relation(uid(), m, dec!(1000), false, true);

        let followers = vec![active.clone(), inactive, fixed];
        let trade = master_trade(m, dec!(1), OrderType::Market);
        let orders = TradeCopier::generate_copy_orders(&trade, &followers, dec!(10000));

        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].follower_id, active.follower_id);
    }
}
