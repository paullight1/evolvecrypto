//! Toxicity detection for market-maker fills.
//!
//! Implements three complementary measures:
//!
//! * **VPIN** — Volume-synchronized Probability of Informed Trading.
//!   High VPIN means an unusually large fraction of flow is one-directional,
//!   which typically indicates informed, toxic order flow.
//!
//! * **Kyle's Lambda** — simplified price-impact coefficient.
//!   Measures how much price moves per unit of signed volume.
//!
//! * **Adverse selection rate** — fraction of fills where price moved against
//!   the MM within 30 seconds of execution.

use chrono::{DateTime, Duration, Utc};
use exchange_common::Side;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tracing::debug;

// ── fill record ───────────────────────────────────────────────────────────────

/// A single fill received by the market maker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MmFillRecord {
    /// Unique fill identifier (monotonically increasing is fine).
    pub id: u64,
    pub timestamp: DateTime<Utc>,
    /// Side of the aggressor (taker side).
    pub taker_side: Side,
    /// Quantity traded.
    pub qty: Decimal,
    /// Fill price.
    pub price: Decimal,
    /// Price observed 30 seconds after this fill, used for adverse-selection
    /// calculation. `None` until updated via
    /// [`ToxicityAnalyzer::update_post_fill_price`].
    pub price_after_30s: Option<Decimal>,
}

// ── report ────────────────────────────────────────────────────────────────────

/// Toxicity summary computed over the current fill window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToxicityReport {
    /// VPIN score in `[0, 1]`. Values above ~0.5 are considered elevated.
    pub vpin: Decimal,
    /// Kyle's Lambda (price impact per unit signed volume). Always non-negative.
    pub kyle_lambda: Decimal,
    /// Fraction of fills with observed adverse price movement, in `[0, 1]`.
    pub adverse_selection_rate: Decimal,
    /// Number of fills used in this computation.
    pub fill_count: usize,
    /// Number of fills for which a post-fill price has been recorded.
    pub fills_with_post_price: usize,
}

// ── analyzer ─────────────────────────────────────────────────────────────────

/// Configurable toxicity analyzer for a single market.
#[derive(Debug)]
pub struct ToxicityAnalyzer {
    /// How far back fills are retained (seconds).
    window_secs: u64,
    fills: Vec<MmFillRecord>,
    next_id: u64,
}

impl ToxicityAnalyzer {
    pub fn new(window_secs: u64) -> Self {
        Self {
            window_secs,
            fills: Vec::new(),
            next_id: 0,
        }
    }

    /// Add a new fill to the analyzer.  Returns the assigned fill ID.
    pub fn record_fill(
        &mut self,
        timestamp: DateTime<Utc>,
        taker_side: Side,
        qty: Decimal,
        price: Decimal,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        debug!(fill_id = id, ?taker_side, %qty, %price, "recording MM fill");
        self.fills.push(MmFillRecord {
            id,
            timestamp,
            taker_side,
            qty,
            price,
            price_after_30s: None,
        });
        id
    }

    /// Record the market price observed ~30 s after fill `fill_id`.
    ///
    /// Call this once a 30-second timer fires for each fill. The price is used
    /// to determine adverse selection.
    pub fn update_post_fill_price(&mut self, fill_id: u64, price_after_30s: Decimal) {
        if let Some(fill) = self.fills.iter_mut().find(|f| f.id == fill_id) {
            fill.price_after_30s = Some(price_after_30s);
        }
    }

    /// Discard fills older than the configured window relative to `now`.
    pub fn cleanup(&mut self, now: DateTime<Utc>) {
        let cutoff = now - Duration::seconds(self.window_secs as i64);
        let before = self.fills.len();
        self.fills.retain(|f| f.timestamp >= cutoff);
        let removed = before - self.fills.len();
        if removed > 0 {
            debug!(removed, "pruned old MM fill records");
        }
    }

    /// Compute a toxicity report over all fills currently in the window.
    pub fn report(&self) -> ToxicityReport {
        let fills = &self.fills;
        let fill_count = fills.len();

        if fill_count == 0 {
            return ToxicityReport {
                vpin: Decimal::ZERO,
                kyle_lambda: Decimal::ZERO,
                adverse_selection_rate: Decimal::ZERO,
                fill_count: 0,
                fills_with_post_price: 0,
            };
        }

        // ── VPIN ─────────────────────────────────────────────────────────────
        // VPIN = |buy_vol - sell_vol| / total_vol
        let total_vol: Decimal = fills.iter().map(|f| f.qty).sum();
        let buy_vol: Decimal = fills
            .iter()
            .filter(|f| f.taker_side == Side::Buy)
            .map(|f| f.qty)
            .sum();
        let sell_vol = total_vol - buy_vol;
        let vpin = if total_vol.is_zero() {
            Decimal::ZERO
        } else {
            let imbalance = (buy_vol - sell_vol).abs();
            imbalance / total_vol
        };

        // ── Kyle's Lambda ─────────────────────────────────────────────────────
        // Simplified: sum(|price_move| * sign_of_volume) / sum(|volume|)
        // We interpret "price_move" as the change from the previous fill price.
        // sign_of_volume: Buy = +1, Sell = -1.
        let kyle_lambda = {
            let mut numerator = Decimal::ZERO;
            let mut denominator = Decimal::ZERO;
            for (i, fill) in fills.iter().enumerate() {
                let price_move = if i == 0 {
                    Decimal::ZERO
                } else {
                    (fill.price - fills[i - 1].price).abs()
                };
                let sign = match fill.taker_side {
                    Side::Buy => Decimal::ONE,
                    Side::Sell => -Decimal::ONE,
                };
                numerator += price_move * sign;
                denominator += fill.qty;
            }
            if denominator.is_zero() {
                Decimal::ZERO
            } else {
                (numerator / denominator).abs()
            }
        };

        // ── Adverse selection ─────────────────────────────────────────────────
        // For each fill with a post-fill price, check if price moved against
        // the MM's passive side.
        // When taker is Buy, MM sold (ask side). Adverse = price went up after.
        // When taker is Sell, MM bought (bid side). Adverse = price went down after.
        let fills_with_post_price = fills.iter().filter(|f| f.price_after_30s.is_some()).count();
        let adverse_count = fills
            .iter()
            .filter(|f| {
                let Some(post_price) = f.price_after_30s else {
                    return false;
                };
                match f.taker_side {
                    Side::Buy => post_price > f.price,   // MM sold, price rose → adverse
                    Side::Sell => post_price < f.price,  // MM bought, price fell → adverse
                }
            })
            .count();

        let adverse_selection_rate = if fills_with_post_price == 0 {
            Decimal::ZERO
        } else {
            Decimal::from(adverse_count) / Decimal::from(fills_with_post_price)
        };

        ToxicityReport {
            vpin,
            kyle_lambda,
            adverse_selection_rate,
            fill_count,
            fills_with_post_price,
        }
    }
}

// ── per-user scorer ───────────────────────────────────────────────────────────

/// Maintains a separate [`ToxicityAnalyzer`] for each taker user ID.
///
/// The market-maker service records every fill it receives against maker
/// quotes.  This scorer fans fills out to per-user analyzers so the MM can
/// identify which counterparties are sending toxic (informed) flow.
#[derive(Debug)]
pub struct UserToxicityScorer {
    window_secs: u64,
    analyzers: HashMap<UserId, ToxicityAnalyzer>,
}

use std::collections::HashMap;
use exchange_common::UserId;

impl UserToxicityScorer {
    /// Create a scorer with the given look-back window (seconds per user).
    pub fn new(window_secs: u64) -> Self {
        Self {
            window_secs,
            analyzers: HashMap::new(),
        }
    }

    /// Create a scorer with a default 5-minute look-back window.
    pub fn with_defaults() -> Self {
        Self::new(300)
    }

    /// Record a fill attributed to `taker_user_id`.
    pub fn record_fill(
        &mut self,
        taker_user_id: UserId,
        timestamp: DateTime<Utc>,
        taker_side: Side,
        qty: Decimal,
        price: Decimal,
    ) -> u64 {
        let analyzer = self
            .analyzers
            .entry(taker_user_id)
            .or_insert_with(|| ToxicityAnalyzer::new(self.window_secs));
        analyzer.record_fill(timestamp, taker_side, qty, price)
    }

    /// Get the toxicity report for a specific taker.  Returns `None` if no
    /// fills have been recorded for that user.
    pub fn report_for(&self, taker_user_id: UserId) -> Option<ToxicityReport> {
        self.analyzers.get(&taker_user_id).map(|a| a.report())
    }

    /// Prune stale fills for all tracked users.  Call periodically to bound
    /// memory usage.
    pub fn cleanup_all(&mut self, now: DateTime<Utc>) {
        for analyzer in self.analyzers.values_mut() {
            analyzer.cleanup(now);
        }
        // Drop users whose analyzer is now empty.
        self.analyzers.retain(|_, a| a.report().fill_count > 0);
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use rust_decimal_macros::dec;

    fn fixed_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 6, 1, 12, 0, 0).unwrap()
    }

    fn analyzer() -> ToxicityAnalyzer {
        ToxicityAnalyzer::new(3600)
    }

    // T1: no fills → all zeros
    #[test]
    fn no_fills_returns_zero_toxicity() {
        let a = analyzer();
        let r = a.report();
        assert_eq!(r.vpin, Decimal::ZERO);
        assert_eq!(r.kyle_lambda, Decimal::ZERO);
        assert_eq!(r.adverse_selection_rate, Decimal::ZERO);
        assert_eq!(r.fill_count, 0);
    }

    // T2: perfectly balanced buy/sell → VPIN = 0
    #[test]
    fn balanced_flow_gives_low_vpin() {
        let mut a = analyzer();
        let now = fixed_now();
        a.record_fill(now, Side::Buy, dec!(10), dec!(100));
        a.record_fill(now, Side::Sell, dec!(10), dec!(100));
        let r = a.report();
        assert_eq!(r.vpin, Decimal::ZERO);
    }

    // T3: all buys → VPIN = 1
    #[test]
    fn one_sided_buy_flow_gives_high_vpin() {
        let mut a = analyzer();
        let now = fixed_now();
        for _ in 0..5 {
            a.record_fill(now, Side::Buy, dec!(10), dec!(100));
        }
        let r = a.report();
        assert_eq!(r.vpin, Decimal::ONE);
    }

    // T4: all sells → VPIN = 1
    #[test]
    fn one_sided_sell_flow_gives_high_vpin() {
        let mut a = analyzer();
        let now = fixed_now();
        for _ in 0..4 {
            a.record_fill(now, Side::Sell, dec!(5), dec!(200));
        }
        let r = a.report();
        assert_eq!(r.vpin, Decimal::ONE);
    }

    // T5: adverse selection counting
    #[test]
    fn adverse_selection_counted_correctly() {
        let mut a = analyzer();
        let now = fixed_now();

        // Fill 1: taker Buy @ 100 → MM sold; post price 110 → adverse
        let id1 = a.record_fill(now, Side::Buy, dec!(1), dec!(100));
        // Fill 2: taker Buy @ 100 → MM sold; post price 90 → not adverse
        let id2 = a.record_fill(now, Side::Buy, dec!(1), dec!(100));
        // Fill 3: taker Sell @ 100 → MM bought; post price 90 → adverse
        let id3 = a.record_fill(now, Side::Sell, dec!(1), dec!(100));

        a.update_post_fill_price(id1, dec!(110));
        a.update_post_fill_price(id2, dec!(90));
        a.update_post_fill_price(id3, dec!(90));

        let r = a.report();
        assert_eq!(r.fills_with_post_price, 3);
        // 2 out of 3 fills are adverse
        assert_eq!(r.adverse_selection_rate, dec!(2) / dec!(3));
    }

    // T6: post-fill price update only affects the targeted fill
    #[test]
    fn post_fill_price_targets_correct_fill() {
        let mut a = analyzer();
        let now = fixed_now();
        let id1 = a.record_fill(now, Side::Buy, dec!(1), dec!(100));
        let _id2 = a.record_fill(now, Side::Sell, dec!(1), dec!(100));

        a.update_post_fill_price(id1, dec!(120)); // only id1 gets a post price

        let r = a.report();
        assert_eq!(r.fills_with_post_price, 1);
        // id1 is adverse (taker Buy, price rose); id2 has no post price → not counted
        assert_eq!(r.adverse_selection_rate, Decimal::ONE);
    }

    // T7: cleanup removes fills outside the window
    #[test]
    fn cleanup_removes_old_records() {
        let mut a = ToxicityAnalyzer::new(60); // 60-second window
        let now = fixed_now();
        let old = now - Duration::seconds(120);

        a.record_fill(old, Side::Buy, dec!(10), dec!(100));
        a.record_fill(now, Side::Sell, dec!(10), dec!(100));

        a.cleanup(now);

        let r = a.report();
        assert_eq!(r.fill_count, 1); // only the recent fill remains
    }

    // T8: adverse selection rate is 0 when no post prices recorded
    #[test]
    fn adverse_selection_zero_without_post_prices() {
        let mut a = analyzer();
        let now = fixed_now();
        a.record_fill(now, Side::Buy, dec!(5), dec!(100));
        a.record_fill(now, Side::Sell, dec!(5), dec!(100));

        let r = a.report();
        assert_eq!(r.adverse_selection_rate, Decimal::ZERO);
        assert_eq!(r.fills_with_post_price, 0);
    }
}
