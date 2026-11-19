//! Dynamic rebate engine for market makers.
//!
//! Rebates are computed as:
//!
//! ```text
//! rebate = base_rebate + quality_bonus * composite_score
//! ```
//!
//! where `composite_score` is a weighted average of four normalised
//! quality dimensions, each in `[0, 1]`:
//!
//! | Dimension | Weight | Formula |
//! |-----------|--------|---------|
//! | Uptime    | 0.30   | `uptime_pct / 100` |
//! | Spread    | 0.30   | `1 - avg_spread_bps / max_spread_bps` (tighter = higher) |
//! | Depth     | 0.20   | `min(actual_depth / required_depth, 1)` |
//! | Refill    | 0.20   | `1 - avg_refill_secs / max_refill_secs` (faster = higher) |

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tracing::debug;

// ── config ────────────────────────────────────────────────────────────────────

/// Parameters for the rebate engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebateConfig {
    /// Flat rebate paid regardless of quality (in basis points of notional).
    pub base_rebate_bps: Decimal,
    /// Additional rebate at perfect quality (in basis points of notional).
    pub quality_bonus_bps: Decimal,
    /// Spread at which quality drops to zero (basis points).
    pub max_spread_bps: Decimal,
    /// Depth at which depth score reaches 1.
    pub required_depth_quote: Decimal,
    /// Refill time at which quality drops to zero (seconds).
    pub max_refill_secs: Decimal,
}

impl Default for RebateConfig {
    fn default() -> Self {
        Self {
            base_rebate_bps: Decimal::new(1, 0),  // 1 bps
            quality_bonus_bps: Decimal::new(4, 0), // up to 4 bps
            max_spread_bps: Decimal::new(20, 0),
            required_depth_quote: Decimal::new(10_000, 0),
            max_refill_secs: Decimal::new(10, 0),
        }
    }
}

// ── quality score ─────────────────────────────────────────────────────────────

/// Detailed breakdown of an MM's quality score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MmQualityScore {
    /// Normalised uptime score in `[0, 1]`.
    pub uptime_score: Decimal,
    /// Normalised spread score in `[0, 1]` (tighter = higher).
    pub spread_score: Decimal,
    /// Normalised depth score in `[0, 1]`.
    pub depth_score: Decimal,
    /// Normalised refill score in `[0, 1]` (faster = higher).
    pub refill_score: Decimal,
    /// Weighted composite of the four scores, in `[0, 1]`.
    pub composite: Decimal,
}

// ── engine ────────────────────────────────────────────────────────────────────

/// Computes quality scores and rebate rates for market makers.
#[derive(Debug, Clone)]
pub struct RebateEngine {
    config: RebateConfig,
}

impl RebateEngine {
    pub fn new(config: RebateConfig) -> Self {
        Self { config }
    }

    /// Compute the quality score from observed SLA metrics.
    ///
    /// All inputs are clamped so that scores remain in `[0, 1]`.
    ///
    /// # Parameters
    /// - `uptime_pct`: observed uptime in `[0, 100]`.
    /// - `avg_spread_bps`: observed average spread in basis points.
    /// - `avg_depth_quote`: observed average depth (quote-asset notional).
    /// - `avg_refill_secs`: observed average quote-refill latency in seconds.
    pub fn compute_quality(
        &self,
        uptime_pct: Decimal,
        avg_spread_bps: Decimal,
        avg_depth_quote: Decimal,
        avg_refill_secs: Decimal,
    ) -> MmQualityScore {
        let one = Decimal::ONE;
        let zero = Decimal::ZERO;
        let hundred = Decimal::from(100u32);

        // uptime_score = uptime_pct / 100, capped at 1
        let uptime_score = (uptime_pct / hundred).min(one).max(zero);

        // spread_score = 1 - (avg_spread_bps / max_spread_bps), floored at 0
        let spread_score = if self.config.max_spread_bps.is_zero() {
            zero
        } else {
            (one - avg_spread_bps / self.config.max_spread_bps).max(zero).min(one)
        };

        // depth_score = min(actual_depth / required_depth, 1)
        let depth_score = if self.config.required_depth_quote.is_zero() {
            one
        } else {
            (avg_depth_quote / self.config.required_depth_quote).min(one).max(zero)
        };

        // refill_score = 1 - (avg_refill_secs / max_refill_secs), floored at 0
        let refill_score = if self.config.max_refill_secs.is_zero() {
            zero
        } else {
            (one - avg_refill_secs / self.config.max_refill_secs).max(zero).min(one)
        };

        // composite = 0.30 * uptime + 0.30 * spread + 0.20 * depth + 0.20 * refill
        let w_uptime = Decimal::new(30, 2);
        let w_spread = Decimal::new(30, 2);
        let w_depth = Decimal::new(20, 2);
        let w_refill = Decimal::new(20, 2);

        let composite = uptime_score * w_uptime
            + spread_score * w_spread
            + depth_score * w_depth
            + refill_score * w_refill;

        debug!(
            %uptime_score, %spread_score, %depth_score, %refill_score, %composite,
            "MM quality score computed"
        );

        MmQualityScore {
            uptime_score,
            spread_score,
            depth_score,
            refill_score,
            composite,
        }
    }

    /// Convert a quality score into a rebate rate in basis points.
    ///
    /// `rebate = base_rebate + quality_bonus * composite`
    pub fn rebate_for_quality(&self, score: &MmQualityScore) -> Decimal {
        self.config.base_rebate_bps + self.config.quality_bonus_bps * score.composite
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn engine() -> RebateEngine {
        RebateEngine::new(RebateConfig {
            base_rebate_bps: dec!(1),
            quality_bonus_bps: dec!(4),
            max_spread_bps: dec!(20),
            required_depth_quote: dec!(10_000),
            max_refill_secs: dec!(10),
        })
    }

    // T1: perfect metrics → composite = 1 → max rebate = base + bonus
    #[test]
    fn perfect_quality_gives_max_rebate() {
        let e = engine();
        let score = e.compute_quality(dec!(100), dec!(0), dec!(10_000), dec!(0));
        assert_eq!(score.composite, Decimal::ONE);
        let rebate = e.rebate_for_quality(&score);
        assert_eq!(rebate, dec!(5)); // 1 + 4 * 1
    }

    // T2: zero quality → composite = 0 → only base rebate
    #[test]
    fn zero_quality_gives_base_rebate_only() {
        let e = engine();
        // uptime=0, spread=max, depth=0, refill=max → all sub-scores = 0
        let score = e.compute_quality(dec!(0), dec!(20), dec!(0), dec!(10));
        assert_eq!(score.composite, Decimal::ZERO);
        let rebate = e.rebate_for_quality(&score);
        assert_eq!(rebate, dec!(1)); // base only
    }

    // T3: uptime score calculation
    #[test]
    fn uptime_score_is_pct_over_100() {
        let e = engine();
        let score = e.compute_quality(dec!(80), dec!(20), dec!(0), dec!(10));
        assert_eq!(score.uptime_score, dec!(0.80));
    }

    // T4: spread score — tighter spread → higher score
    #[test]
    fn spread_score_tighter_is_higher() {
        let e = engine();
        let tight = e.compute_quality(dec!(0), dec!(5), dec!(0), dec!(10));
        let wide = e.compute_quality(dec!(0), dec!(15), dec!(0), dec!(10));
        assert!(tight.spread_score > wide.spread_score);
        // At spread=5 with max_spread=20: score = 1 - 5/20 = 0.75
        assert_eq!(tight.spread_score, dec!(0.75));
    }

    // T5: depth score capped at 1 when depth exceeds required
    #[test]
    fn depth_score_capped_at_one() {
        let e = engine();
        let score = e.compute_quality(dec!(0), dec!(20), dec!(50_000), dec!(10));
        assert_eq!(score.depth_score, Decimal::ONE);
    }

    // T6: refill score — faster refill → higher score
    #[test]
    fn refill_score_faster_is_higher() {
        let e = engine();
        let fast = e.compute_quality(dec!(0), dec!(20), dec!(0), dec!(2));
        let slow = e.compute_quality(dec!(0), dec!(20), dec!(0), dec!(8));
        assert!(fast.refill_score > slow.refill_score);
        // At refill=2 with max=10: score = 1 - 2/10 = 0.8
        assert_eq!(fast.refill_score, dec!(0.8));
    }

    // T7: composite weight sum (uptime=100, spread=0, depth=10000, refill=0)
    #[test]
    fn composite_is_correct_weighted_average() {
        let e = engine();
        // uptime=50% → 0.5, spread=10 → 0.5, depth=5000 → 0.5, refill=5 → 0.5
        let score = e.compute_quality(dec!(50), dec!(10), dec!(5_000), dec!(5));
        assert_eq!(score.composite, dec!(0.5));
    }

    // T8: scores are clamped — spread above max yields zero spread score
    #[test]
    fn spread_above_max_clamps_to_zero() {
        let e = engine();
        let score = e.compute_quality(dec!(100), dec!(30), dec!(10_000), dec!(0));
        assert_eq!(score.spread_score, Decimal::ZERO);
    }
}
