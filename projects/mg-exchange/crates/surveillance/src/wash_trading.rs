//! Wash-trading detection.
//!
//! Wash trading inflates reported volume without genuine economic risk changing
//! hands. Two patterns are detected:
//!
//! 1. **Volume balance** — a single user's buy and sell volume on the same
//!    symbol are suspiciously close (≥ `volume_balance_threshold`), indicating
//!    they are both sides of the market artificially.
//!
//! 2. **Circular trading** — a ring of accounts trade with each other such that
//!    value cycles back to its origin: A→B→C→A.  We detect this by tracking
//!    directed trade edges and searching for cycles of length ≥ 3 within the
//!    analysis window.

use std::collections::{HashMap, HashSet, VecDeque};

use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tracing::debug;

use exchange_common::types::{Side, Symbol, UserId};

use crate::alert::{AlertSeverity, AutoAction, SurveillanceAlert, WashTradingAlert};

// ──────────────────────────────────────────────────────────────────────────────
// Internal types
// ──────────────────────────────────────────────────────────────────────────────

/// A single trade as recorded by the detector.
#[derive(Debug, Clone)]
pub struct WashTradeCandidate {
    pub maker_user_id: UserId,
    pub taker_user_id: UserId,
    pub symbol: Symbol,
    pub price: Decimal,
    pub qty: Decimal,
    pub taker_side: Side,
    pub timestamp: DateTime<Utc>,
}

// ──────────────────────────────────────────────────────────────────────────────
// Public detector
// ──────────────────────────────────────────────────────────────────────────────

pub struct WashTradingDetector {
    /// All trades received within the current window.
    recent_trades: Vec<WashTradeCandidate>,
    /// `min(buy_vol, sell_vol) / max(buy_vol, sell_vol)` threshold; 0.8 = flag.
    volume_balance_threshold: Decimal,
    /// Rolling window width in seconds.
    window_secs: u64,
}

impl WashTradingDetector {
    pub fn new(volume_balance_threshold: Decimal, window_secs: u64) -> Self {
        Self {
            recent_trades: Vec::new(),
            volume_balance_threshold,
            window_secs,
        }
    }

    /// Record a new trade.
    pub fn on_trade(
        &mut self,
        maker_user_id: UserId,
        taker_user_id: UserId,
        symbol: Symbol,
        price: Decimal,
        qty: Decimal,
        taker_side: Side,
        timestamp: DateTime<Utc>,
    ) {
        self.recent_trades.push(WashTradeCandidate {
            maker_user_id,
            taker_user_id,
            symbol,
            price,
            qty,
            taker_side,
            timestamp,
        });
    }

    /// Drop trades older than the rolling window.
    pub fn cleanup_old(&mut self, now: DateTime<Utc>) {
        let cutoff = now - Duration::seconds(self.window_secs as i64);
        self.recent_trades.retain(|t| t.timestamp >= cutoff);
    }

    /// Check whether `user_id` has suspiciously balanced buy/sell volume on
    /// `symbol` within the rolling window ending at `now`.
    pub fn check_volume_balance(
        &self,
        user_id: UserId,
        symbol: &Symbol,
        now: DateTime<Utc>,
    ) -> Option<SurveillanceAlert> {
        let cutoff = now - Duration::seconds(self.window_secs as i64);

        let mut buy_vol = Decimal::ZERO;
        let mut sell_vol = Decimal::ZERO;

        for t in &self.recent_trades {
            if t.timestamp < cutoff || &t.symbol != symbol {
                continue;
            }
            // Determine what each user did in this trade.
            // Taker side is from the taker's perspective.
            if t.taker_user_id == user_id {
                match t.taker_side {
                    Side::Buy => buy_vol += t.qty,
                    Side::Sell => sell_vol += t.qty,
                }
            }
            if t.maker_user_id == user_id {
                // Maker is on the opposite side to the taker.
                match t.taker_side {
                    Side::Buy => sell_vol += t.qty,
                    Side::Sell => buy_vol += t.qty,
                }
            }
        }

        if buy_vol.is_zero() || sell_vol.is_zero() {
            return None;
        }

        let ratio =
            buy_vol.min(sell_vol) / buy_vol.max(sell_vol);

        if ratio < self.volume_balance_threshold {
            return None;
        }

        debug!(
            %user_id,
            %symbol,
            %buy_vol,
            %sell_vol,
            %ratio,
            "wash trading: volume balance alert"
        );

        let raw = WashTradingAlert {
            user_ids: vec![user_id],
            symbol: symbol.clone(),
            buy_volume: buy_vol,
            sell_volume: sell_vol,
            balance_ratio: ratio,
        };

        let severity = severity_from_ratio(ratio);
        let description = format!(
            "User {user_id} has {ratio:.2} buy/sell volume balance on {symbol} \
             (threshold {:.2}), possible wash trading",
            self.volume_balance_threshold,
        );

        Some(SurveillanceAlert {
            id: uuid::Uuid::new_v4(),
            alert_type: crate::alert::AlertType::WashTrading(raw),
            severity,
            user_ids: vec![user_id],
            symbol: Some(symbol.clone()),
            description,
            auto_action: AutoAction::CancelOrders,
            created_at: now,
            reviewed: false,
        })
    }

    /// Detect circular trading (A→B→C→A) among all trades in the current
    /// window. Returns one alert per detected cycle.
    pub fn check_circular_trading(
        &self,
        now: DateTime<Utc>,
    ) -> Vec<SurveillanceAlert> {
        let cutoff = now - Duration::seconds(self.window_secs as i64);

        // Build an adjacency list: who traded to whom (taker bought from maker).
        let mut graph: HashMap<UserId, HashSet<UserId>> = HashMap::new();

        for t in &self.recent_trades {
            if t.timestamp < cutoff {
                continue;
            }
            // The aggressor (taker) directed value toward the maker.
            graph
                .entry(t.taker_user_id)
                .or_default()
                .insert(t.maker_user_id);
        }

        let mut alerts = Vec::new();
        let mut reported_cycles: HashSet<Vec<UserId>> = HashSet::new();

        for &start in graph.keys() {
            if let Some(cycle) = find_cycle_bfs(&graph, start) {
                // Canonicalise the cycle so we don't report duplicates.
                let mut canonical = cycle.clone();
                canonical.sort();
                if reported_cycles.insert(canonical) {
                    let description = format!(
                        "Circular trading detected: {} users form a trading ring",
                        cycle.len()
                    );
                    let raw = WashTradingAlert {
                        user_ids: cycle.clone(),
                        symbol: Symbol::new("(multiple)"),
                        buy_volume: Decimal::ZERO,
                        sell_volume: Decimal::ZERO,
                        balance_ratio: dec!(1),
                    };
                    alerts.push(SurveillanceAlert {
                        id: uuid::Uuid::new_v4(),
                        alert_type: crate::alert::AlertType::WashTrading(raw),
                        severity: AlertSeverity::High,
                        user_ids: cycle,
                        symbol: None,
                        description,
                        auto_action: AutoAction::FreezeAccount,
                        created_at: now,
                        reviewed: false,
                    });
                }
            }
        }

        alerts
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Graph helpers
// ──────────────────────────────────────────────────────────────────────────────

/// BFS from `start`; returns the cycle as an ordered list of nodes if one is
/// found, or `None` if `start` is not part of any cycle.
fn find_cycle_bfs(
    graph: &HashMap<UserId, HashSet<UserId>>,
    start: UserId,
) -> Option<Vec<UserId>> {
    // Queue entries: (current node, path taken to reach it).
    let mut queue: VecDeque<(UserId, Vec<UserId>)> = VecDeque::new();
    queue.push_back((start, vec![start]));

    while let Some((node, path)) = queue.pop_front() {
        let Some(neighbours) = graph.get(&node) else {
            continue;
        };

        for &next in neighbours {
            if next == start && path.len() >= 3 {
                // Found a cycle of length ≥ 3 back to start.
                return Some(path);
            }
            // Avoid revisiting nodes within the same path to prevent infinite loops.
            if !path.contains(&next) {
                let mut new_path = path.clone();
                new_path.push(next);
                queue.push_back((next, new_path));
            }
        }
    }

    None
}

// ──────────────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────────────

fn severity_from_ratio(ratio: Decimal) -> AlertSeverity {
    if ratio >= dec!(0.99) {
        AlertSeverity::Critical
    } else if ratio >= dec!(0.95) {
        AlertSeverity::High
    } else if ratio >= dec!(0.90) {
        AlertSeverity::Medium
    } else {
        AlertSeverity::Low
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    use exchange_common::types::Side;

    use super::*;

    fn detector() -> WashTradingDetector {
        WashTradingDetector::new(dec!(0.80), 3600)
    }

    fn symbol() -> Symbol {
        Symbol::new("BTCUSDT")
    }

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    #[test]
    fn no_alert_when_only_buys() {
        let mut det = detector();
        let u = Uuid::new_v4();
        let t = now();

        det.on_trade(Uuid::new_v4(), u, symbol(), dec!(50000), dec!(1), Side::Buy, t);

        assert!(det.check_volume_balance(u, &symbol(), t).is_none());
    }

    #[test]
    fn no_alert_when_imbalanced() {
        let mut det = detector();
        let u = Uuid::new_v4();
        let t = now();

        // 10 buy, 1 sell → ratio = 0.1, below 0.80 threshold.
        det.on_trade(Uuid::new_v4(), u, symbol(), dec!(50000), dec!(10), Side::Buy, t);
        det.on_trade(u, Uuid::new_v4(), symbol(), dec!(50000), dec!(1), Side::Buy, t);

        assert!(det.check_volume_balance(u, &symbol(), t).is_none());
    }

    #[test]
    fn alert_fires_on_balanced_volume() {
        let mut det = detector();
        let u = Uuid::new_v4();
        let counterparty = Uuid::new_v4();
        let t = now();

        // u buys 5 BTC as taker.
        det.on_trade(counterparty, u, symbol(), dec!(50000), dec!(5), Side::Buy, t);
        // u sells 5 BTC as taker.
        det.on_trade(counterparty, u, symbol(), dec!(50000), dec!(5), Side::Sell, t);

        let alert = det.check_volume_balance(u, &symbol(), t);
        assert!(alert.is_some());
    }

    #[test]
    fn trades_outside_window_are_ignored() {
        let mut det = WashTradingDetector::new(dec!(0.80), 60);
        let u = Uuid::new_v4();
        let t = now();

        // Trades placed 2 hours ago — outside 60-second window.
        let old = t - Duration::seconds(7200);
        det.on_trade(Uuid::new_v4(), u, symbol(), dec!(50000), dec!(5), Side::Buy, old);
        det.on_trade(Uuid::new_v4(), u, symbol(), dec!(50000), dec!(5), Side::Sell, old);

        assert!(det.check_volume_balance(u, &symbol(), t).is_none());
    }

    #[test]
    fn circular_trading_detected() {
        let mut det = detector();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let t = now();

        // A→B: A buys from B (A is taker, B is maker).
        det.on_trade(b, a, symbol(), dec!(50000), dec!(1), Side::Buy, t);
        // B→C: B buys from C.
        det.on_trade(c, b, symbol(), dec!(50000), dec!(1), Side::Buy, t);
        // C→A: C buys from A — completes the ring.
        det.on_trade(a, c, symbol(), dec!(50000), dec!(1), Side::Buy, t);

        let alerts = det.check_circular_trading(t);
        assert!(!alerts.is_empty(), "expected at least one circular trading alert");
        assert!(alerts[0].user_ids.len() >= 3);
    }

    #[test]
    fn no_circular_alert_for_pair_trade() {
        let mut det = detector();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let t = now();

        // Only A→B and B→A — cycle of length 2 does not count.
        det.on_trade(b, a, symbol(), dec!(50000), dec!(1), Side::Buy, t);
        det.on_trade(a, b, symbol(), dec!(50000), dec!(1), Side::Buy, t);

        let alerts = det.check_circular_trading(t);
        assert!(alerts.is_empty(), "pair back-and-forth should not trigger circular alert");
    }

    #[test]
    fn cleanup_removes_old_trades() {
        let mut det = WashTradingDetector::new(dec!(0.80), 60);
        let t = now();

        det.on_trade(
            Uuid::new_v4(), Uuid::new_v4(), symbol(),
            dec!(50000), dec!(1), Side::Buy,
            t - Duration::seconds(120),
        );

        assert_eq!(det.recent_trades.len(), 1);
        det.cleanup_old(t);
        assert_eq!(det.recent_trades.len(), 0);
    }

    #[test]
    fn maker_side_volume_counted_correctly() {
        let mut det = detector();
        let u = Uuid::new_v4();
        let counterparty = Uuid::new_v4();
        let t = now();

        // u is maker; taker buys → u is selling.
        // u is maker; taker sells → u is buying.
        det.on_trade(u, counterparty, symbol(), dec!(50000), dec!(3), Side::Buy, t);
        det.on_trade(u, counterparty, symbol(), dec!(50000), dec!(3), Side::Sell, t);

        let alert = det.check_volume_balance(u, &symbol(), t);
        assert!(alert.is_some());
    }
}
