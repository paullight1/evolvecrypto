//! Implied volatility surface — a (strike × expiry) → IV grid with
//! bilinear interpolation and flat extrapolation.
//!
//! # Overview
//!
//! The volatility surface is built from observed market quotes (bid/ask mid
//! IVs from live option order books).  Given a target strike and expiry we
//! interpolate across the surface to produce a "fair" IV for pricing or Greeks
//! calculation.
//!
//! # Interpolation scheme
//!
//! 1. **Tenor axis** — linear interpolation between the two nearest expiry
//!    slices (in calendar-days space).  Flat extrapolation beyond the extreme
//!    expiries.
//! 2. **Strike axis** — within each expiry slice, linear interpolation between
//!    the two surrounding strike points.  Flat extrapolation beyond the extreme
//!    strikes (use the nearest-boundary IV).
//!
//! This is intentionally simple.  A production surface would use
//! SVI/SABR parameterisation, but bilinear interpolation is sufficient for
//! Phase 1 margin calculation and risk monitoring.
//!
//! # ATM volatility
//!
//! `atm_vol` queries the surface at `strike = spot`, providing the ATM
//! (at-the-money forward) volatility for a given expiry.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::options_pricing::BlackScholes;

// ─────────────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────────────

/// A single observed IV data point on the surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolPoint {
    /// Strike price.
    pub strike: Decimal,
    /// Expiry timestamp.
    pub expiry: DateTime<Utc>,
    /// Implied volatility as a fraction (0.20 = 20%).
    pub iv: Decimal,
}

/// A market quote used to bootstrap the IV surface.
///
/// The IV is derived from the mid-price of the bid/ask spread.
#[derive(Debug, Clone)]
pub struct OptionQuote {
    /// Option symbol, e.g. `"BTC-20260401-70000-C"`.
    pub symbol: String,
    /// Strike.
    pub strike: Decimal,
    /// Expiry.
    pub expiry: DateTime<Utc>,
    /// `true` = call, `false` = put.
    pub is_call: bool,
    /// Mid-market premium (in quote currency).
    pub mid_price: Decimal,
    /// Current underlying spot price.
    pub spot: Decimal,
    /// Risk-free rate used for IV inversion.
    pub risk_free_rate: Decimal,
}

// ─────────────────────────────────────────────────────────────────────────────
// Volatility surface
// ─────────────────────────────────────────────────────────────────────────────

/// Implied volatility surface for one underlying asset.
///
/// The surface is a collection of observed (strike, expiry, IV) triples.
/// Queries are answered by bilinear interpolation.
#[derive(Debug, Clone)]
pub struct VolatilitySurface {
    /// Underlying asset code, e.g. `"BTC"`.
    pub underlying: String,
    /// All observed IV points, sorted by (expiry, strike) after construction.
    points: Vec<VolPoint>,
    /// Fallback IV used when the surface has no data.
    fallback_iv: Decimal,
}

impl VolatilitySurface {
    /// Construct a surface directly from known IV points.
    ///
    /// Points are sorted by (expiry asc, strike asc) internally.
    pub fn new(underlying: &str, mut points: Vec<VolPoint>) -> Self {
        points.sort_by(|a, b| {
            a.expiry
                .cmp(&b.expiry)
                .then(a.strike.partial_cmp(&b.strike).unwrap_or(std::cmp::Ordering::Equal))
        });
        Self {
            underlying: underlying.to_uppercase(),
            points,
            fallback_iv: Decimal::new(80, 2), // 80% — conservative default
        }
    }

    /// Build a surface by inverting market quotes to implied volatilities via
    /// Black-Scholes (Newton-Raphson, up to 50 iterations).
    ///
    /// Quotes whose IV cannot be inverted are silently skipped (e.g. quotes
    /// that are below intrinsic value).
    pub fn from_quotes(underlying: &str, quotes: &[OptionQuote]) -> Self {
        let mut points = Vec::with_capacity(quotes.len());

        for q in quotes {
            if let Some(iv) = BlackScholes::implied_volatility(
                q.mid_price,
                q.spot,
                q.strike,
                time_to_expiry_years(q.expiry),
                q.risk_free_rate,
                q.is_call,
            ) {
                points.push(VolPoint {
                    strike: q.strike,
                    expiry: q.expiry,
                    iv,
                });

                debug!(
                    underlying,
                    symbol = %q.symbol,
                    %q.strike,
                    %iv,
                    "bootstrapped IV from quote"
                );
            }
        }

        Self::new(underlying, points)
    }

    /// Interpolate the implied volatility for any (strike, expiry) combination.
    ///
    /// Algorithm:
    /// 1. Find the two expiry slices that bracket `expiry`.
    /// 2. For each slice, interpolate IV across the strike dimension.
    /// 3. Interpolate the resulting two IVs across the time dimension.
    ///
    /// Falls back to `fallback_iv` when the surface has no data.
    pub fn get_iv(&self, strike: Decimal, expiry: DateTime<Utc>) -> Decimal {
        if self.points.is_empty() {
            return self.fallback_iv;
        }

        let expiry_ts = expiry.timestamp();

        // Collect unique expiry timestamps (sorted by construction).
        let expiry_ts_list: Vec<i64> = {
            let mut v: Vec<i64> = self.points.iter().map(|p| p.expiry.timestamp()).collect();
            v.dedup();
            v
        };

        if expiry_ts_list.is_empty() {
            return self.fallback_iv;
        }

        // Find the bracketing expiry indices.
        let (lo_exp_ts, hi_exp_ts, t_weight) = bracket(&expiry_ts_list, expiry_ts);

        let iv_lo = self.strike_interp(strike, lo_exp_ts);
        let iv_hi = self.strike_interp(strike, hi_exp_ts);

        // Linear interpolation in time.
        let iv = lerp(iv_lo, iv_hi, to_f64(t_weight));
        to_dec(iv.max(0.0001)) // floor IV at 1bp
    }

    /// At-the-money implied volatility — queries the surface at `strike = spot`.
    pub fn atm_vol(&self, spot: Decimal, expiry: DateTime<Utc>) -> Decimal {
        self.get_iv(spot, expiry)
    }

    /// Number of data points in the surface.
    pub fn point_count(&self) -> usize {
        self.points.len()
    }

    /// Returns `true` if the surface has at least one data point.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    // ── Private helpers ──────────────────────────────────────────────────────

    /// Interpolate IV across the strike dimension for a fixed expiry timestamp.
    fn strike_interp(&self, strike: Decimal, expiry_ts: i64) -> f64 {
        // Collect all points for this expiry slice.
        let slice: Vec<&VolPoint> = self
            .points
            .iter()
            .filter(|p| p.expiry.timestamp() == expiry_ts)
            .collect();

        if slice.is_empty() {
            return to_f64(self.fallback_iv);
        }
        if slice.len() == 1 {
            return to_f64(slice[0].iv);
        }

        let strikes: Vec<f64> = slice.iter().map(|p| to_f64(p.strike)).collect();
        let s = to_f64(strike);

        let (lo_idx, hi_idx, w) = bracket_f64(&strikes, s);
        lerp(to_f64(slice[lo_idx].iv), to_f64(slice[hi_idx].iv), w)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Compute time-to-expiry in years from now.
fn time_to_expiry_years(expiry: DateTime<Utc>) -> Decimal {
    let seconds = (expiry - Utc::now()).num_seconds().max(0) as f64;
    to_dec(seconds / (365.25 * 24.0 * 3600.0))
}

/// Find the bracketing pair (lo_val, hi_val, weight) for `target` in a sorted
/// slice.  `weight` interpolates from lo to hi (0.0 = use lo, 1.0 = use hi).
/// Clamps to the endpoints for extrapolation.
fn bracket(sorted: &[i64], target: i64) -> (i64, i64, Decimal) {
    if target <= sorted[0] {
        return (sorted[0], sorted[0], Decimal::ZERO);
    }
    let last = *sorted.last().unwrap();
    if target >= last {
        return (last, last, Decimal::ZERO);
    }

    let pos = sorted.partition_point(|&v| v < target);
    let lo = sorted[pos - 1];
    let hi = sorted[pos];
    let w = if hi == lo {
        Decimal::ZERO
    } else {
        to_dec((target - lo) as f64 / (hi - lo) as f64)
    };
    (lo, hi, w)
}

/// Same as `bracket` but for f64 slices.
fn bracket_f64(sorted: &[f64], target: f64) -> (usize, usize, f64) {
    if sorted.is_empty() {
        return (0, 0, 0.0);
    }
    if target <= sorted[0] {
        return (0, 0, 0.0);
    }
    let last_idx = sorted.len() - 1;
    if target >= sorted[last_idx] {
        return (last_idx, last_idx, 0.0);
    }

    let pos = sorted.partition_point(|&v| v < target);
    let lo = pos - 1;
    let hi = pos;
    let w = if (sorted[hi] - sorted[lo]).abs() < f64::EPSILON {
        0.0
    } else {
        (target - sorted[lo]) / (sorted[hi] - sorted[lo])
    };
    (lo, hi, w)
}

#[inline]
fn lerp(lo: f64, hi: f64, w: f64) -> f64 {
    lo + (hi - lo) * w
}

#[inline]
fn to_f64(d: Decimal) -> f64 {
    d.try_into().unwrap_or(0.0)
}

#[inline]
fn to_dec(x: f64) -> Decimal {
    Decimal::try_from(x).unwrap_or(Decimal::ZERO)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use rust_decimal_macros::dec;

    fn make_surface() -> VolatilitySurface {
        let exp1 = Utc.with_ymd_and_hms(2026, 4, 3, 8, 0, 0).unwrap();
        let exp2 = Utc.with_ymd_and_hms(2026, 5, 1, 8, 0, 0).unwrap();

        let points = vec![
            VolPoint { strike: dec!(60000), expiry: exp1, iv: dec!(0.70) },
            VolPoint { strike: dec!(70000), expiry: exp1, iv: dec!(0.60) },
            VolPoint { strike: dec!(80000), expiry: exp1, iv: dec!(0.65) },
            VolPoint { strike: dec!(60000), expiry: exp2, iv: dec!(0.65) },
            VolPoint { strike: dec!(70000), expiry: exp2, iv: dec!(0.55) },
            VolPoint { strike: dec!(80000), expiry: exp2, iv: dec!(0.60) },
        ];
        VolatilitySurface::new("BTC", points)
    }

    #[test]
    fn test_exact_point_lookup() {
        let surf = make_surface();
        let exp1 = Utc.with_ymd_and_hms(2026, 4, 3, 8, 0, 0).unwrap();
        let iv = surf.get_iv(dec!(70000), exp1);
        assert_eq!(iv, dec!(0.60), "exact point should return its IV");
    }

    #[test]
    fn test_strike_interpolation() {
        let surf = make_surface();
        let exp1 = Utc.with_ymd_and_hms(2026, 4, 3, 8, 0, 0).unwrap();
        // Strike midpoint between 60k (0.70) and 70k (0.60) → expect ~0.65.
        let iv = surf.get_iv(dec!(65000), exp1);
        let diff = (iv - dec!(0.65)).abs();
        assert!(diff < dec!(0.005), "strike interpolation off: {iv}");
    }

    #[test]
    fn test_time_interpolation() {
        let surf = make_surface();
        // An expiry exactly halfway between exp1 and exp2 should give
        // roughly the average of their IVs at the same strike.
        let exp1 = Utc.with_ymd_and_hms(2026, 4, 3, 8, 0, 0).unwrap();
        let exp2 = Utc.with_ymd_and_hms(2026, 5, 1, 8, 0, 0).unwrap();
        let mid_ts = (exp1.timestamp() + exp2.timestamp()) / 2;
        let mid_exp = DateTime::from_timestamp(mid_ts, 0).unwrap();

        // At strike=70000: exp1→0.60, exp2→0.55 → expect ~0.575.
        let iv = surf.get_iv(dec!(70000), mid_exp);
        let diff = (iv - dec!(0.575)).abs();
        assert!(diff < dec!(0.005), "time interpolation off: {iv}");
    }

    #[test]
    fn test_extrapolation_below_min_strike() {
        let surf = make_surface();
        let exp1 = Utc.with_ymd_and_hms(2026, 4, 3, 8, 0, 0).unwrap();
        // Below minimum strike → flat extrapolation using lowest strike IV.
        let iv = surf.get_iv(dec!(10000), exp1);
        assert_eq!(iv, dec!(0.70));
    }

    #[test]
    fn test_extrapolation_above_max_strike() {
        let surf = make_surface();
        let exp1 = Utc.with_ymd_and_hms(2026, 4, 3, 8, 0, 0).unwrap();
        let iv = surf.get_iv(dec!(200000), exp1);
        assert_eq!(iv, dec!(0.65));
    }

    #[test]
    fn test_empty_surface_returns_fallback() {
        let surf = VolatilitySurface::new("BTC", vec![]);
        let exp = Utc.with_ymd_and_hms(2026, 4, 3, 8, 0, 0).unwrap();
        let iv = surf.get_iv(dec!(70000), exp);
        assert_eq!(iv, dec!(0.80)); // fallback
    }

    #[test]
    fn test_atm_vol_delegates_to_get_iv() {
        let surf = make_surface();
        let exp1 = Utc.with_ymd_and_hms(2026, 4, 3, 8, 0, 0).unwrap();
        assert_eq!(
            surf.atm_vol(dec!(70000), exp1),
            surf.get_iv(dec!(70000), exp1)
        );
    }

    #[test]
    fn test_from_quotes_bootstraps_iv() {
        let spot = dec!(70000);
        // Use a relative expiry (14 days out from *now*) instead of a
        // hardcoded date. A past date would make `to_dec((exp - now))`
        // collapse to ~1 second, and the Black-Scholes ATM call price at
        // tte=1s is ~0, which `from_quotes` cannot bootstrap IV from.
        let exp = Utc::now() + chrono::Duration::days(14);
        // Use a price we know corresponds to σ≈0.60 ATM (BTC $70k, ~14-day expiry, r=5%).
        // We generate the price from BS and then verify the surface recovers σ.
        let sigma = dec!(0.60);
        let t = to_dec((exp - Utc::now()).num_seconds().max(1) as f64 / (365.25 * 86400.0));
        let price = BlackScholes::call_price(spot, spot, t, dec!(0.05), sigma);

        let quotes = vec![OptionQuote {
            symbol: "BTC-20260403-70000-C".into(),
            strike: spot,
            expiry: exp,
            is_call: true,
            mid_price: price,
            spot,
            risk_free_rate: dec!(0.05),
        }];

        let surf = VolatilitySurface::from_quotes("BTC", &quotes);
        assert!(!surf.is_empty(), "surface should have one point");
        let iv = surf.get_iv(spot, exp);
        let diff = (iv - sigma).abs();
        assert!(diff < dec!(0.01), "bootstrapped IV too far from truth: {iv}");
    }

    #[test]
    fn test_iv_floor_at_minimum() {
        // IV must never be zero (floor applied).
        let surf = VolatilitySurface::new("BTC", vec![]);
        let exp = Utc.with_ymd_and_hms(2026, 4, 3, 8, 0, 0).unwrap();
        let iv = surf.get_iv(dec!(70000), exp);
        assert!(iv > Decimal::ZERO, "IV should be floored above zero");
    }
}
