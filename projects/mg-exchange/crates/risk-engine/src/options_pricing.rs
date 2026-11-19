//! Black-Scholes option pricing model with full Greeks and implied volatility.
//!
//! # Design
//!
//! All public APIs accept and return [`rust_decimal::Decimal`] to stay consistent
//! with the rest of the exchange codebase (no f64 for money values).  Internally
//! the transcendental functions (exp, ln, sqrt) require f64; we convert at the
//! boundary and convert back immediately.
//!
//! # Formulas
//!
//! ```text
//! d1 = (ln(S/K) + (r + σ²/2)·T) / (σ·√T)
//! d2 = d1 − σ·√T
//!
//! Call = S·N(d1) − K·e^(−rT)·N(d2)
//! Put  = K·e^(−rT)·N(−d2) − S·N(−d1)
//!
//! Put-call parity: C − P = S − K·e^(−rT)
//! ```
//!
//! # References
//!
//! - Black, F. & Scholes, M. (1973). "The Pricing of Options and Corporate Liabilities."
//! - Abramowitz & Stegun (1964), §26.2.17 — rational CDF approximation.
//! - Haug, E.G. (2007). "The Complete Guide to Option Pricing Formulas."

use rust_decimal::Decimal;
use tracing::trace;

// ─────────────────────────────────────────────────────────────────────────────
// Normal distribution helpers (f64, internal only)
// ─────────────────────────────────────────────────────────────────────────────

/// Standard normal CDF Φ(x), accurate to ±7.5 × 10⁻⁸.
///
/// Uses the Abramowitz & Stegun rational-polynomial approximation (§26.2.17).
/// All arithmetic is in f64; the result is unitless.
pub(crate) fn standard_normal_cdf(x: f64) -> f64 {
    // Horner-form coefficients from A&S Table 26.2, formula (26.2.17).
    const P: f64 = 0.231_641_9;
    const B1: f64 = 0.319_381_530;
    const B2: f64 = -0.356_563_782;
    const B3: f64 = 1.781_477_937;
    const B4: f64 = -1.821_255_978;
    const B5: f64 = 1.330_274_429;

    let t = 1.0 / (1.0 + P * x.abs());
    let poly = t * (B1 + t * (B2 + t * (B3 + t * (B4 + t * B5))));
    let pdf_x = (-0.5 * x * x).exp() / (2.0_f64 * std::f64::consts::PI).sqrt();
    let cdf = 1.0 - pdf_x * poly;
    if x >= 0.0 {
        cdf
    } else {
        1.0 - cdf
    }
}

/// Standard normal PDF φ(x) = e^(-x²/2) / √(2π).
#[inline]
fn standard_normal_pdf(x: f64) -> f64 {
    (-0.5 * x * x).exp() / (2.0_f64 * std::f64::consts::PI).sqrt()
}

/// Convert a finite f64 to Decimal, falling back to Decimal::ZERO for NaN/Inf.
#[inline]
fn to_dec(x: f64) -> Decimal {
    Decimal::try_from(x).unwrap_or(Decimal::ZERO)
}

// ─────────────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────────────

/// All five first-order Black-Scholes sensitivities for one option.
///
/// Conventions match the industry standard:
/// - `theta` is expressed **per calendar day** (negative for long positions).
/// - `vega` is expressed **per 1% move in implied volatility**.
/// - `rho` is expressed **per 1% move in the risk-free rate**.
#[derive(Debug, Clone, PartialEq)]
pub struct OptionGreeks {
    /// dV/dS — sensitivity of option price to underlying spot price.
    /// Call: [0, 1], Put: [-1, 0].
    pub delta: Decimal,
    /// d²V/dS² — rate of change of delta per unit change in spot.
    /// Always non-negative; peaks at-the-money.
    pub gamma: Decimal,
    /// dV/dt per calendar day — time decay (negative for long options).
    pub theta: Decimal,
    /// dV/dσ per 1% move in implied volatility.
    pub vega: Decimal,
    /// dV/dr per 1% move in risk-free rate.
    pub rho: Decimal,
}

// ─────────────────────────────────────────────────────────────────────────────
// BlackScholes model
// ─────────────────────────────────────────────────────────────────────────────

/// Black-Scholes European option pricing model.
///
/// All methods are pure functions with no state; the struct exists only to
/// provide a namespaced API consistent with the rest of the codebase.
pub struct BlackScholes;

impl BlackScholes {
    // ── Internal f64 kernel ──────────────────────────────────────────────────

    /// Validate and convert inputs.  Returns `None` when any input is
    /// non-positive (expired, zero-vol, zero-price).
    fn inputs_f64(
        s: Decimal,
        k: Decimal,
        t: Decimal,
        r: Decimal,
        sigma: Decimal,
    ) -> Option<(f64, f64, f64, f64, f64)> {
        let s: f64 = s.try_into().ok()?;
        let k: f64 = k.try_into().ok()?;
        let t: f64 = t.try_into().ok()?;
        let r: f64 = r.try_into().ok()?;
        let sigma: f64 = sigma.try_into().ok()?;

        if s <= 0.0 || k <= 0.0 || t <= 0.0 || sigma <= 0.0 {
            return None;
        }

        Some((s, k, t, r, sigma))
    }

    /// Compute d1 and d2 from the validated f64 inputs.
    #[inline]
    fn d1_d2(s: f64, k: f64, t: f64, r: f64, sigma: f64) -> (f64, f64) {
        let sqrt_t = t.sqrt();
        let d1 = ((s / k).ln() + (r + 0.5 * sigma * sigma) * t) / (sigma * sqrt_t);
        let d2 = d1 - sigma * sqrt_t;
        (d1, d2)
    }

    // ── Public pricing methods ───────────────────────────────────────────────

    /// Black-Scholes European call price.
    ///
    /// Returns `Decimal::ZERO` if the option is expired or inputs are invalid.
    ///
    /// # Parameters
    /// - `s` — spot price of the underlying
    /// - `k` — option strike price
    /// - `t` — time to expiry in years (e.g. 30 days → 30/365 ≈ 0.0822)
    /// - `r` — continuously-compounded risk-free rate (e.g. 5% → 0.05)
    /// - `sigma` — implied volatility as a fraction (e.g. 20% → 0.20)
    pub fn call_price(s: Decimal, k: Decimal, t: Decimal, r: Decimal, sigma: Decimal) -> Decimal {
        let Some((s, k, t, r, sigma)) = Self::inputs_f64(s, k, t, r, sigma) else {
            return Decimal::ZERO;
        };
        let (d1, d2) = Self::d1_d2(s, k, t, r, sigma);
        let disc = (-r * t).exp();
        let price = s * standard_normal_cdf(d1) - k * disc * standard_normal_cdf(d2);

        trace!(s, k, t, r, sigma, d1, d2, price, "BS call_price");
        to_dec(price.max(0.0))
    }

    /// Black-Scholes European put price.
    ///
    /// Returns `Decimal::ZERO` if the option is expired or inputs are invalid.
    pub fn put_price(s: Decimal, k: Decimal, t: Decimal, r: Decimal, sigma: Decimal) -> Decimal {
        let Some((s, k, t, r, sigma)) = Self::inputs_f64(s, k, t, r, sigma) else {
            return Decimal::ZERO;
        };
        let (d1, d2) = Self::d1_d2(s, k, t, r, sigma);
        let disc = (-r * t).exp();
        let price =
            k * disc * standard_normal_cdf(-d2) - s * standard_normal_cdf(-d1);

        trace!(s, k, t, r, sigma, d1, d2, price, "BS put_price");
        to_dec(price.max(0.0))
    }

    /// Compute all five Greeks for a single option in one pass.
    ///
    /// Returns `None` when the option has expired (`t ≤ 0`) or any input is
    /// degenerate.
    pub fn greeks(
        s: Decimal,
        k: Decimal,
        t: Decimal,
        r: Decimal,
        sigma: Decimal,
        is_call: bool,
    ) -> Option<OptionGreeks> {
        let (s, k, t, r, sigma) = Self::inputs_f64(s, k, t, r, sigma)?;
        let sqrt_t = t.sqrt();
        let (d1, d2) = Self::d1_d2(s, k, t, r, sigma);
        let disc = (-r * t).exp();

        let nd1 = standard_normal_cdf(d1);
        let nd2 = standard_normal_cdf(d2);
        let pdf_d1 = standard_normal_pdf(d1);

        // Delta
        let delta = if is_call { nd1 } else { nd1 - 1.0 };

        // Gamma — identical for calls and puts (same formula)
        let gamma = pdf_d1 / (s * sigma * sqrt_t);

        // Theta (per year, then converted to per day below)
        let common_theta = -(s * pdf_d1 * sigma) / (2.0 * sqrt_t);
        let theta_annual = if is_call {
            common_theta - r * k * disc * nd2
        } else {
            common_theta + r * k * disc * standard_normal_cdf(-d2)
        };
        // Conventional sign: long position loses time value each day.
        let theta_daily = theta_annual / 365.0;

        // Vega — per 1% change in volatility (divide by 100)
        let vega = s * sqrt_t * pdf_d1 / 100.0;

        // Rho — per 1% change in risk-free rate (divide by 100)
        let rho = if is_call {
            k * t * disc * nd2 / 100.0
        } else {
            -k * t * disc * standard_normal_cdf(-d2) / 100.0
        };

        Some(OptionGreeks {
            delta: to_dec(delta),
            gamma: to_dec(gamma),
            theta: to_dec(theta_daily),
            vega: to_dec(vega),
            rho: to_dec(rho),
        })
    }

    /// Implied volatility via Newton-Raphson iteration.
    ///
    /// Starting from an initial vol guess we iterate:
    /// ```text
    /// σ_{n+1} = σ_n − (BS(σ_n) − market_price) / vega(σ_n)
    /// ```
    /// until `|BS(σ) − market_price| < tolerance` or we exhaust `max_iters`.
    ///
    /// Returns `None` when:
    /// - The market price is below intrinsic value (arbitrage violation).
    /// - Vega collapses to near zero (deep ITM/OTM, very short tenor).
    /// - No convergence within `MAX_ITERS` iterations.
    pub fn implied_volatility(
        price: Decimal,
        s: Decimal,
        k: Decimal,
        t: Decimal,
        r: Decimal,
        is_call: bool,
    ) -> Option<Decimal> {
        const MAX_ITERS: u32 = 50;
        const TOLERANCE: f64 = 1e-8;
        const MIN_VEGA: f64 = 1e-10;

        let price_f: f64 = price.try_into().ok()?;
        let s_f: f64 = s.try_into().ok()?;
        let k_f: f64 = k.try_into().ok()?;
        let t_f: f64 = t.try_into().ok()?;
        let r_f: f64 = r.try_into().ok()?;

        if price_f <= 0.0 || s_f <= 0.0 || k_f <= 0.0 || t_f <= 0.0 {
            return None;
        }

        // Brenner-Subrahmanyam ATM approximation as the initial guess:
        // σ ≈ price / (S · √(T/2π))
        let sqrt_t = t_f.sqrt();
        let mut sigma = price_f / (s_f * (t_f / (2.0 * std::f64::consts::PI)).sqrt());
        // Clamp to a sensible range.
        sigma = sigma.clamp(1e-4, 10.0);

        for _ in 0..MAX_ITERS {
            let (d1, d2) = Self::d1_d2(s_f, k_f, t_f, r_f, sigma);
            let disc = (-r_f * t_f).exp();
            let pdf_d1 = standard_normal_pdf(d1);

            let bs_price = if is_call {
                s_f * standard_normal_cdf(d1) - k_f * disc * standard_normal_cdf(d2)
            } else {
                k_f * disc * standard_normal_cdf(-d2) - s_f * standard_normal_cdf(-d1)
            };

            let diff = bs_price - price_f;
            if diff.abs() < TOLERANCE {
                return Some(to_dec(sigma));
            }

            // Vega = S · √T · φ(d1)  (raw, not per-1% here)
            let vega = s_f * sqrt_t * pdf_d1;
            if vega.abs() < MIN_VEGA {
                return None;
            }

            sigma -= diff / vega;
            if sigma <= 0.0 {
                sigma = 1e-4; // recover from over-shoot
            }
        }

        // Did not converge within MAX_ITERS
        None
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    // Helper: assert two Decimals are within `tol` of each other.
    fn assert_close(actual: Decimal, expected: Decimal, tol: Decimal, label: &str) {
        let diff = (actual - expected).abs();
        assert!(
            diff <= tol,
            "{label}: actual={actual}, expected={expected}, diff={diff}, tol={tol}"
        );
    }

    // Standard ATM test inputs: S=100, K=100, T=1yr, r=5%, σ=20%.
    // Reference: Hull, "Options, Futures, and Other Derivatives" (any edition).
    // Call ≈ 10.4506, Put ≈ 5.5735.
    fn atm_params() -> (Decimal, Decimal, Decimal, Decimal, Decimal) {
        (dec!(100), dec!(100), dec!(1), dec!(0.05), dec!(0.20))
    }

    // ── Call and Put prices ──────────────────────────────────────────────────

    #[test]
    fn test_call_price_atm_known_value() {
        let (s, k, t, r, sigma) = atm_params();
        let price = BlackScholes::call_price(s, k, t, r, sigma);
        // Known reference value: 10.4506 (Hull Table 15.1).
        assert_close(price, dec!(10.4506), dec!(0.01), "ATM call price");
    }

    #[test]
    fn test_put_price_atm_known_value() {
        let (s, k, t, r, sigma) = atm_params();
        let price = BlackScholes::put_price(s, k, t, r, sigma);
        // Known reference: 5.5735.
        assert_close(price, dec!(5.5735), dec!(0.01), "ATM put price");
    }

    #[test]
    fn test_call_price_itm() {
        // Deep ITM: S=110, K=100 — call should be close to intrinsic + time value.
        let price = BlackScholes::call_price(dec!(110), dec!(100), dec!(1), dec!(0.05), dec!(0.20));
        // Must be > intrinsic (110 - 100*e^-0.05 ≈ 15.12) and positive.
        assert!(price > dec!(15), "ITM call below intrinsic-like bound: {price}");
    }

    #[test]
    fn test_put_price_itm() {
        // Deep ITM put: S=90, K=100.
        let price = BlackScholes::put_price(dec!(90), dec!(100), dec!(1), dec!(0.05), dec!(0.20));
        assert!(price > dec!(5), "ITM put suspiciously cheap: {price}");
    }

    #[test]
    fn test_prices_non_negative() {
        // Option prices must never be negative.
        for (s, k) in [(dec!(50), dec!(100)), (dec!(200), dec!(100))] {
            let c = BlackScholes::call_price(s, k, dec!(0.1), dec!(0.05), dec!(0.50));
            let p = BlackScholes::put_price(s, k, dec!(0.1), dec!(0.05), dec!(0.50));
            assert!(c >= Decimal::ZERO, "call price negative: {c}");
            assert!(p >= Decimal::ZERO, "put price negative: {p}");
        }
    }

    // ── Put-call parity ──────────────────────────────────────────────────────

    #[test]
    fn test_put_call_parity_atm() {
        let (s, k, t, r, sigma) = atm_params();
        let call = BlackScholes::call_price(s, k, t, r, sigma);
        let put = BlackScholes::put_price(s, k, t, r, sigma);
        // C − P = S − K·e^{−rT}
        let lhs = call - put;
        let rhs = s - k * to_dec((-0.05_f64).exp());
        assert_close(lhs, rhs, dec!(0.001), "put-call parity ATM");
    }

    #[test]
    fn test_put_call_parity_itm_call() {
        let (s, k, t, r, sigma) = (dec!(120), dec!(100), dec!(0.5), dec!(0.03), dec!(0.30));
        let call = BlackScholes::call_price(s, k, t, r, sigma);
        let put = BlackScholes::put_price(s, k, t, r, sigma);
        let lhs = call - put;
        let rhs = s - k * to_dec((-0.03_f64 * 0.5_f64).exp());
        assert_close(lhs, rhs, dec!(0.001), "put-call parity ITM call");
    }

    #[test]
    fn test_put_call_parity_high_vol() {
        let (s, k, t, r, sigma) = (dec!(100), dec!(95), dec!(2), dec!(0.02), dec!(0.80));
        let call = BlackScholes::call_price(s, k, t, r, sigma);
        let put = BlackScholes::put_price(s, k, t, r, sigma);
        let lhs = call - put;
        let rhs = s - k * to_dec((-0.02_f64 * 2.0_f64).exp());
        assert_close(lhs, rhs, dec!(0.001), "put-call parity high vol");
    }

    // ── Greeks ───────────────────────────────────────────────────────────────

    #[test]
    fn test_delta_atm_call_approx_half() {
        let (s, k, t, r, sigma) = atm_params();
        let g = BlackScholes::greeks(s, k, t, r, sigma, true).unwrap();
        // ATM call delta ≈ 0.50–0.60 (not exactly 0.5 because of r adjustment).
        assert!(
            g.delta > dec!(0.50) && g.delta < dec!(0.65),
            "ATM call delta out of range: {}",
            g.delta
        );
    }

    #[test]
    fn test_delta_atm_put_approx_negative_half() {
        // ATM put delta = N(d1) - 1.  With S=K=100, T=1, r=5%, σ=20%:
        // d1 = (0 + 0.07*1) / 0.2 = 0.35, N(0.35) ≈ 0.637
        // put delta ≈ -0.363.
        // The range [-0.50, -0.30] is correct for these parameters.
        let (s, k, t, r, sigma) = atm_params();
        let g = BlackScholes::greeks(s, k, t, r, sigma, false).unwrap();
        assert!(
            g.delta > dec!(-0.50) && g.delta < dec!(-0.30),
            "ATM put delta out of range: {}",
            g.delta
        );
    }

    #[test]
    fn test_delta_call_bounds() {
        // Call delta is always in [0, 1].
        for s in [dec!(50), dec!(100), dec!(200)] {
            let g = BlackScholes::greeks(s, dec!(100), dec!(1), dec!(0.05), dec!(0.30), true)
                .unwrap();
            assert!(
                g.delta >= Decimal::ZERO && g.delta <= Decimal::ONE,
                "call delta out of [0,1]: {}",
                g.delta
            );
        }
    }

    #[test]
    fn test_delta_put_bounds() {
        // Put delta is always in [-1, 0].
        for s in [dec!(50), dec!(100), dec!(200)] {
            let g = BlackScholes::greeks(s, dec!(100), dec!(1), dec!(0.05), dec!(0.30), false)
                .unwrap();
            assert!(
                g.delta <= Decimal::ZERO && g.delta >= dec!(-1),
                "put delta out of [-1,0]: {}",
                g.delta
            );
        }
    }

    #[test]
    fn test_gamma_atm_peaks() {
        // Gamma is maximised at ATM. Check ATM > deep OTM.
        let g_atm =
            BlackScholes::greeks(dec!(100), dec!(100), dec!(1), dec!(0.05), dec!(0.20), true)
                .unwrap();
        let g_otm =
            BlackScholes::greeks(dec!(100), dec!(200), dec!(1), dec!(0.05), dec!(0.20), true)
                .unwrap();
        let g_itm =
            BlackScholes::greeks(dec!(100), dec!(30), dec!(1), dec!(0.05), dec!(0.20), true)
                .unwrap();
        assert!(g_atm.gamma > g_otm.gamma, "ATM gamma should exceed OTM gamma");
        assert!(g_atm.gamma > g_itm.gamma, "ATM gamma should exceed ITM gamma");
    }

    #[test]
    fn test_gamma_non_negative() {
        for s in [dec!(70), dec!(100), dec!(140)] {
            let g = BlackScholes::greeks(s, dec!(100), dec!(0.5), dec!(0.05), dec!(0.25), true)
                .unwrap();
            assert!(g.gamma >= Decimal::ZERO, "gamma negative: {}", g.gamma);
        }
    }

    #[test]
    fn test_theta_negative_for_long() {
        // Long options lose value over time → theta < 0.
        let (s, k, t, r, sigma) = atm_params();
        let gc = BlackScholes::greeks(s, k, t, r, sigma, true).unwrap();
        let gp = BlackScholes::greeks(s, k, t, r, sigma, false).unwrap();
        assert!(gc.theta < Decimal::ZERO, "call theta should be negative: {}", gc.theta);
        assert!(gp.theta < Decimal::ZERO, "put theta should be negative: {}", gp.theta);
    }

    #[test]
    fn test_vega_non_negative() {
        let (s, k, t, r, sigma) = atm_params();
        let gc = BlackScholes::greeks(s, k, t, r, sigma, true).unwrap();
        let gp = BlackScholes::greeks(s, k, t, r, sigma, false).unwrap();
        assert!(gc.vega >= Decimal::ZERO, "call vega negative");
        assert!(gp.vega >= Decimal::ZERO, "put vega negative");
    }

    #[test]
    fn test_vega_call_equals_put() {
        // Vega is identical for calls and puts (same d1 formula).
        let (s, k, t, r, sigma) = atm_params();
        let gc = BlackScholes::greeks(s, k, t, r, sigma, true).unwrap();
        let gp = BlackScholes::greeks(s, k, t, r, sigma, false).unwrap();
        assert_close(gc.vega, gp.vega, dec!(0.0001), "call/put vega equality");
    }

    #[test]
    fn test_gamma_call_equals_put() {
        // Gamma is also identical for calls and puts.
        let (s, k, t, r, sigma) = atm_params();
        let gc = BlackScholes::greeks(s, k, t, r, sigma, true).unwrap();
        let gp = BlackScholes::greeks(s, k, t, r, sigma, false).unwrap();
        assert_close(gc.gamma, gp.gamma, dec!(0.0001), "call/put gamma equality");
    }

    #[test]
    fn test_greeks_expired_returns_none() {
        // t = 0 → expired → None.
        assert!(
            BlackScholes::greeks(dec!(100), dec!(100), Decimal::ZERO, dec!(0.05), dec!(0.20), true)
                .is_none()
        );
    }

    #[test]
    fn test_greeks_zero_vol_returns_none() {
        assert!(
            BlackScholes::greeks(dec!(100), dec!(100), dec!(1), dec!(0.05), Decimal::ZERO, true)
                .is_none()
        );
    }

    // ── Implied Volatility ───────────────────────────────────────────────────

    #[test]
    fn test_implied_vol_round_trip_call() {
        // Start with known sigma → compute call price → recover sigma.
        let sigma = dec!(0.30);
        let price = BlackScholes::call_price(dec!(100), dec!(100), dec!(1), dec!(0.05), sigma);
        let iv =
            BlackScholes::implied_volatility(price, dec!(100), dec!(100), dec!(1), dec!(0.05), true)
                .expect("IV should converge");
        assert_close(iv, sigma, dec!(0.0001), "call IV round-trip");
    }

    #[test]
    fn test_implied_vol_round_trip_put() {
        let sigma = dec!(0.45);
        let price = BlackScholes::put_price(dec!(100), dec!(110), dec!(0.5), dec!(0.03), sigma);
        let iv = BlackScholes::implied_volatility(
            price,
            dec!(100),
            dec!(110),
            dec!(0.5),
            dec!(0.03),
            false,
        )
        .expect("put IV should converge");
        assert_close(iv, sigma, dec!(0.0001), "put IV round-trip");
    }

    #[test]
    fn test_implied_vol_known_atm() {
        // ATM call price ≈ 10.4506 at σ=0.20 (Hull).
        let iv = BlackScholes::implied_volatility(
            dec!(10.4506),
            dec!(100),
            dec!(100),
            dec!(1),
            dec!(0.05),
            true,
        )
        .expect("IV should converge for Hull's ATM call");
        assert_close(iv, dec!(0.20), dec!(0.001), "Hull ATM call IV recovery");
    }

    #[test]
    fn test_implied_vol_various_sigmas() {
        // Round-trip test for a range of volatilities and moneyness.
        let test_cases = [
            (dec!(100), dec!(100), dec!(1), dec!(0.05), dec!(0.15), true),
            (dec!(100), dec!(90), dec!(0.5), dec!(0.02), dec!(0.40), true),
            (dec!(100), dec!(110), dec!(0.25), dec!(0.04), dec!(0.60), false),
            (dec!(100), dec!(100), dec!(2), dec!(0.01), dec!(0.80), false),
        ];

        for (s, k, t, r, sigma, is_call) in test_cases {
            let price = if is_call {
                BlackScholes::call_price(s, k, t, r, sigma)
            } else {
                BlackScholes::put_price(s, k, t, r, sigma)
            };
            let iv = BlackScholes::implied_volatility(price, s, k, t, r, is_call)
                .unwrap_or_else(|| panic!("IV did not converge for sigma={sigma}"));
            assert_close(iv, sigma, dec!(0.001), &format!("IV round-trip sigma={sigma}"));
        }
    }

    #[test]
    fn test_implied_vol_zero_price_returns_none() {
        assert!(
            BlackScholes::implied_volatility(
                Decimal::ZERO,
                dec!(100),
                dec!(100),
                dec!(1),
                dec!(0.05),
                true
            )
            .is_none()
        );
    }

    #[test]
    fn test_call_price_expired_returns_zero() {
        let price = BlackScholes::call_price(dec!(100), dec!(100), Decimal::ZERO, dec!(0.05), dec!(0.20));
        assert_eq!(price, Decimal::ZERO);
    }
}
