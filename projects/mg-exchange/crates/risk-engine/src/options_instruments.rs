//! Option instrument generation — creates standard calls and puts across
//! a grid of strikes and expiries for a given underlying.
//!
//! # Strike grid
//!
//! Strike spacing and the ±20% around-spot range are calibrated per asset:
//!
//! | Underlying | Near-term (≤ 30 d) | Far-term (> 30 d) |
//! |------------|-------------------|------------------|
//! | BTC        | $1,000             | $5,000            |
//! | ETH        | $50                | $250              |
//! | *default*  | 1% of spot (rounded) | 5% of spot     |
//!
//! # Expiry conventions
//!
//! - **Weekly**: next N Fridays (up to 4 weeks out)
//! - **Monthly**: last Friday of each calendar month (up to 3 months out)
//! - **Quarterly**: last Friday of March, June, September, December
//!
//! Duplicates are de-duplicated and expiries are sorted ascending.

use chrono::{DateTime, Datelike, Duration, TimeZone, Utc, Weekday};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tracing::debug;

// ─────────────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────────────

/// Whether an option contract conveys the right to buy (Call) or sell (Put).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OptionType {
    Call,
    Put,
}

impl std::fmt::Display for OptionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Call => write!(f, "C"),
            Self::Put => write!(f, "P"),
        }
    }
}

/// How the option settles at expiry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettlementType {
    /// Intrinsic value paid in the quote currency (e.g., USDT).
    Cash,
    /// Underlying asset delivered (physical exercise).
    Physical,
}

/// A single listed option instrument.
///
/// The symbol format is `{UNDERLYING}-{YYYYMMDD}-{STRIKE}-{C|P}`,
/// e.g. `BTC-20260401-70000-C`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionInstrument {
    /// Exchange-unique symbol, e.g. `"BTC-20260401-70000-C"`.
    pub symbol: String,
    /// Base asset, e.g. `"BTC"`.
    pub underlying: String,
    /// Strike price in quote currency (USD/USDT).
    pub strike: Decimal,
    /// Expiry timestamp (UTC, 08:00 UTC by convention — Deribit/Bybit standard).
    pub expiry: DateTime<Utc>,
    /// Call or Put.
    pub option_type: OptionType,
    /// Cash or Physical settlement.
    pub settlement: SettlementType,
    /// Minimum price increment for the premium quote.
    pub tick_size: Decimal,
    /// Minimum tradeable quantity (in contracts).
    pub lot_size: Decimal,
    /// Minimum order notional value in quote currency.
    pub min_notional: Decimal,
}

impl OptionInstrument {
    /// Returns `true` if the option is in-the-money at the given spot price.
    pub fn is_itm(&self, spot: Decimal) -> bool {
        match self.option_type {
            OptionType::Call => spot > self.strike,
            OptionType::Put => spot < self.strike,
        }
    }

    /// Intrinsic value at expiry (non-negative).
    pub fn intrinsic_value(&self, settlement_price: Decimal) -> Decimal {
        let diff = match self.option_type {
            OptionType::Call => settlement_price - self.strike,
            OptionType::Put => self.strike - settlement_price,
        };
        diff.max(Decimal::ZERO)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Per-asset configuration
// ─────────────────────────────────────────────────────────────────────────────

struct AssetConfig {
    near_term_spacing: Decimal, // strike spacing for tenors ≤ 30 days
    far_term_spacing: Decimal,  // strike spacing for tenors > 30 days
    tick_size: Decimal,
    lot_size: Decimal,
    min_notional: Decimal,
}

impl AssetConfig {
    fn for_underlying(underlying: &str) -> Self {
        match underlying.to_uppercase().as_str() {
            "BTC" => Self {
                near_term_spacing: Decimal::new(1_000, 0),
                far_term_spacing: Decimal::new(5_000, 0),
                tick_size: Decimal::new(1, 0),    // $1
                lot_size: Decimal::new(1, 3),     // 0.001 BTC
                min_notional: Decimal::new(10, 0), // $10
            },
            "ETH" => Self {
                near_term_spacing: Decimal::new(50, 0),
                far_term_spacing: Decimal::new(250, 0),
                tick_size: Decimal::new(1, 2),     // $0.01
                lot_size: Decimal::new(1, 2),      // 0.01 ETH
                min_notional: Decimal::new(5, 0),  // $5
            },
            _ => {
                // Generic: 1% near-term, 5% far-term (will be computed per-expiry).
                Self {
                    near_term_spacing: Decimal::ZERO, // computed dynamically
                    far_term_spacing: Decimal::ZERO,
                    tick_size: Decimal::new(1, 2),
                    lot_size: Decimal::new(1, 2),
                    min_notional: Decimal::new(1, 0),
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Expiry helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Find the next Friday on or after `from`.
fn next_friday_on_or_after(from: DateTime<Utc>) -> DateTime<Utc> {
    let days_until_friday = (Weekday::Fri.num_days_from_monday() as i64)
        - (from.weekday().num_days_from_monday() as i64);
    let offset = if days_until_friday <= 0 {
        days_until_friday + 7
    } else {
        days_until_friday
    };
    let date = from + Duration::days(offset);
    // Standardise to 08:00 UTC (Deribit / Bybit expiry convention).
    Utc.with_ymd_and_hms(date.year(), date.month(), date.day(), 8, 0, 0)
        .single()
        .unwrap_or(date)
}

/// Last Friday of a given (year, month).
fn last_friday_of_month(year: i32, month: u32) -> Option<DateTime<Utc>> {
    // Start from the last day of the month and walk backward to Friday.
    let last_day = last_day_of_month(year, month)?;
    let mut candidate = Utc
        .with_ymd_and_hms(year, month, last_day, 8, 0, 0)
        .single()?;
    while candidate.weekday() != Weekday::Fri {
        candidate -= Duration::days(1);
    }
    Some(candidate)
}

/// Number of days in a month.
fn last_day_of_month(year: i32, month: u32) -> Option<u32> {
    let next_month = if month == 12 { 1 } else { month + 1 };
    let next_year = if month == 12 { year + 1 } else { year };
    let first_of_next = Utc.with_ymd_and_hms(next_year, next_month, 1, 0, 0, 0).single()?;
    Some((first_of_next - Duration::days(1)).day())
}

/// Generate standard expiry dates:
/// - 4 weekly Fridays
/// - 3 monthly last Fridays
/// - 4 quarterly last Fridays (Mar, Jun, Sep, Dec)
///
/// Expiries must be at least 1 day out; duplicates are removed.
pub fn standard_expiries(now: DateTime<Utc>) -> Vec<DateTime<Utc>> {
    let min_expiry = now + Duration::hours(1);
    let mut expiries = Vec::new();

    // Weekly: next 4 Fridays.
    let mut friday = next_friday_on_or_after(now + Duration::days(1));
    for _ in 0..4 {
        if friday > min_expiry {
            expiries.push(friday);
        }
        friday += Duration::weeks(1);
    }

    // Monthly: last Friday of the next 3 calendar months.
    let mut year = now.year();
    let mut month = now.month();
    for _ in 0..3 {
        month += 1;
        if month > 12 {
            month = 1;
            year += 1;
        }
        if let Some(exp) = last_friday_of_month(year, month) {
            if exp > min_expiry {
                expiries.push(exp);
            }
        }
    }

    // Quarterly: last Friday of Mar, Jun, Sep, Dec for this year and next.
    for y in [year, year + 1] {
        for m in [3u32, 6, 9, 12] {
            if let Some(exp) = last_friday_of_month(y, m) {
                if exp > min_expiry {
                    expiries.push(exp);
                }
            }
        }
    }

    // Sort and de-duplicate (compare at minute resolution to avoid
    // floating-point differences if two paths produce the same Friday).
    expiries.sort();
    expiries.dedup_by(|a, b| {
        a.year() == b.year() && a.month() == b.month() && a.day() == b.day()
    });
    expiries
}

// ─────────────────────────────────────────────────────────────────────────────
// Strike grid
// ─────────────────────────────────────────────────────────────────────────────

/// Round `value` down to the nearest multiple of `spacing`.
fn round_down_to(value: Decimal, spacing: Decimal) -> Decimal {
    if spacing.is_zero() {
        return value;
    }
    (value / spacing).floor() * spacing
}

/// Generate the strike ladder for a single expiry.
///
/// The grid spans ±20% around `spot_price`, rounded to the nearest
/// `spacing` multiple.
fn generate_strikes(spot_price: Decimal, spacing: Decimal) -> Vec<Decimal> {
    if spacing.is_zero() || spot_price.is_zero() {
        return vec![];
    }

    let range_pct = Decimal::new(20, 2); // 20%
    let low_raw = spot_price * (Decimal::ONE - range_pct);
    let high_raw = spot_price * (Decimal::ONE + range_pct);

    let low = round_down_to(low_raw, spacing);
    let high = round_down_to(high_raw, spacing) + spacing; // inclusive

    let mut strikes = Vec::new();
    let mut current = low;
    while current <= high {
        if current > Decimal::ZERO {
            strikes.push(current);
        }
        current += spacing;
    }
    strikes
}

/// Compute the strike spacing for an unrecognised asset based on spot price.
/// Near-term: 1% of spot (rounded to a nice number).
/// Far-term:  5% of spot.
fn dynamic_spacing(spot_price: Decimal, near_term: bool) -> Decimal {
    let pct = if near_term {
        Decimal::new(1, 2)
    } else {
        Decimal::new(5, 2)
    };
    let raw = spot_price * pct;
    // Round to 1 significant figure for readability.
    if raw >= Decimal::new(1000, 0) {
        round_down_to(raw, Decimal::new(1000, 0)).max(Decimal::new(1000, 0))
    } else if raw >= Decimal::new(100, 0) {
        round_down_to(raw, Decimal::new(100, 0)).max(Decimal::new(100, 0))
    } else if raw >= Decimal::new(10, 0) {
        round_down_to(raw, Decimal::new(10, 0)).max(Decimal::new(10, 0))
    } else {
        round_down_to(raw, Decimal::ONE).max(Decimal::ONE)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// OptionsInstrumentGenerator
// ─────────────────────────────────────────────────────────────────────────────

/// Generates the full option chain for a given underlying asset.
pub struct OptionsInstrumentGenerator;

impl OptionsInstrumentGenerator {
    /// Generate option instruments (calls + puts) for `underlying` across all
    /// provided `expiries`.
    ///
    /// # Parameters
    /// - `underlying` — asset code, e.g. `"BTC"` or `"ETH"`.
    /// - `spot_price` — current index/mark price used to centre the strike grid.
    /// - `expiries` — list of expiry datetimes; pass `standard_expiries(now)` for
    ///   the default ladder.
    ///
    /// Returns an empty vec if `spot_price` is zero or negative.
    pub fn generate_instruments(
        underlying: &str,
        spot_price: Decimal,
        expiries: &[DateTime<Utc>],
        now: DateTime<Utc>,
    ) -> Vec<OptionInstrument> {
        if spot_price <= Decimal::ZERO || expiries.is_empty() {
            return vec![];
        }

        let cfg = AssetConfig::for_underlying(underlying);
        let mut instruments = Vec::new();

        for &expiry in expiries {
            let days_to_expiry = (expiry - now).num_days();
            let is_near_term = days_to_expiry <= 30;

            let spacing = if cfg.near_term_spacing.is_zero() {
                dynamic_spacing(spot_price, is_near_term)
            } else if is_near_term {
                cfg.near_term_spacing
            } else {
                cfg.far_term_spacing
            };

            let strikes = generate_strikes(spot_price, spacing);
            let expiry_str = expiry.format("%Y%m%d").to_string();

            debug!(
                underlying,
                %expiry,
                days_to_expiry,
                %spacing,
                strike_count = strikes.len(),
                "generating option chain"
            );

            for strike in &strikes {
                for opt_type in [OptionType::Call, OptionType::Put] {
                    let strike_str = format!("{}", strike.round());
                    let symbol = format!(
                        "{underlying}-{expiry_str}-{strike_str}-{opt_type}"
                    );

                    instruments.push(OptionInstrument {
                        symbol,
                        underlying: underlying.to_uppercase(),
                        strike: *strike,
                        expiry,
                        option_type: opt_type,
                        settlement: SettlementType::Cash,
                        tick_size: cfg.tick_size,
                        lot_size: cfg.lot_size,
                        min_notional: cfg.min_notional,
                    });
                }
            }
        }

        instruments
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use rust_decimal_macros::dec;

    fn fixed_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, 22, 12, 0, 0).unwrap()
    }

    #[test]
    fn test_next_friday() {
        // 2026-03-22 is a Sunday → next Friday is 2026-03-27.
        let now = fixed_now();
        let friday = next_friday_on_or_after(now + Duration::days(1));
        assert_eq!(friday.weekday(), Weekday::Fri);
        assert!(friday > now);
    }

    #[test]
    fn test_standard_expiries_are_fridays() {
        let expiries = standard_expiries(fixed_now());
        assert!(!expiries.is_empty(), "should produce at least one expiry");
        for exp in &expiries {
            assert_eq!(
                exp.weekday(),
                Weekday::Fri,
                "expiry {exp} is not a Friday"
            );
        }
    }

    #[test]
    fn test_standard_expiries_sorted_no_duplicates() {
        let expiries = standard_expiries(fixed_now());
        for window in expiries.windows(2) {
            assert!(window[0] < window[1], "expiries not strictly sorted");
        }
    }

    #[test]
    fn test_btc_instrument_symbol_format() {
        let now = fixed_now();
        let expiry = Utc.with_ymd_and_hms(2026, 4, 3, 8, 0, 0).unwrap(); // a Friday
        let instruments =
            OptionsInstrumentGenerator::generate_instruments("BTC", dec!(70000), &[expiry], now);

        assert!(!instruments.is_empty());
        for inst in &instruments {
            // Symbol must be BTC-YYYYMMDD-STRIKE-{C|P}.
            assert!(
                inst.symbol.starts_with("BTC-"),
                "symbol prefix wrong: {}",
                inst.symbol
            );
            let parts: Vec<&str> = inst.symbol.split('-').collect();
            assert_eq!(parts.len(), 4, "symbol part count wrong: {}", inst.symbol);
            assert!(
                parts[3] == "C" || parts[3] == "P",
                "option type suffix wrong: {}",
                inst.symbol
            );
        }
    }

    #[test]
    fn test_btc_near_term_strike_spacing() {
        // Near-term expiry (7 days out): strikes should be spaced $1,000 apart.
        let now = fixed_now();
        let expiry = now + Duration::days(7);
        let expiry = next_friday_on_or_after(expiry);
        let instruments =
            OptionsInstrumentGenerator::generate_instruments("BTC", dec!(70000), &[expiry], now);

        let calls: Vec<_> = instruments
            .iter()
            .filter(|i| i.option_type == OptionType::Call)
            .collect();
        assert!(calls.len() >= 2, "should have multiple calls");

        let mut strikes: Vec<Decimal> = calls.iter().map(|i| i.strike).collect();
        strikes.sort();
        strikes.dedup();
        for w in strikes.windows(2) {
            assert_eq!(
                w[1] - w[0],
                dec!(1000),
                "BTC near-term spacing should be $1000"
            );
        }
    }

    #[test]
    fn test_btc_far_term_strike_spacing() {
        // Far-term expiry (45 days out): strikes should be spaced $5,000 apart.
        let now = fixed_now();
        let expiry = now + Duration::days(45);
        let expiry = next_friday_on_or_after(expiry);
        let instruments =
            OptionsInstrumentGenerator::generate_instruments("BTC", dec!(70000), &[expiry], now);

        let calls: Vec<_> = instruments
            .iter()
            .filter(|i| i.option_type == OptionType::Call)
            .collect();

        let mut strikes: Vec<Decimal> = calls.iter().map(|i| i.strike).collect();
        strikes.sort();
        strikes.dedup();
        for w in strikes.windows(2) {
            assert_eq!(
                w[1] - w[0],
                dec!(5000),
                "BTC far-term spacing should be $5000"
            );
        }
    }

    #[test]
    fn test_strikes_within_20pct_of_spot() {
        let now = fixed_now();
        let expiry = next_friday_on_or_after(now + Duration::days(1));
        let spot = dec!(70000);
        let instruments =
            OptionsInstrumentGenerator::generate_instruments("BTC", spot, &[expiry], now);

        let low = spot * dec!(0.80);
        let high = spot * dec!(1.20);

        // Allow one spacing unit of over-hang from the grid rounding.
        let margin = dec!(1000);
        for inst in &instruments {
            assert!(
                inst.strike >= low - margin && inst.strike <= high + margin,
                "strike {} outside ±20%+margin range [{}, {}]",
                inst.strike,
                low - margin,
                high + margin
            );
        }
    }

    #[test]
    fn test_calls_and_puts_generated_for_each_strike() {
        let now = fixed_now();
        let expiry = next_friday_on_or_after(now + Duration::days(7));
        let instruments =
            OptionsInstrumentGenerator::generate_instruments("ETH", dec!(2000), &[expiry], now);

        let calls: Vec<_> = instruments
            .iter()
            .filter(|i| i.option_type == OptionType::Call)
            .collect();
        let puts: Vec<_> = instruments
            .iter()
            .filter(|i| i.option_type == OptionType::Put)
            .collect();

        assert_eq!(calls.len(), puts.len(), "equal calls and puts per expiry");
    }

    #[test]
    fn test_eth_near_term_spacing_is_50() {
        let now = fixed_now();
        let expiry = next_friday_on_or_after(now + Duration::days(7));
        let instruments =
            OptionsInstrumentGenerator::generate_instruments("ETH", dec!(2000), &[expiry], now);

        let mut strikes: Vec<Decimal> = instruments
            .iter()
            .filter(|i| i.option_type == OptionType::Call)
            .map(|i| i.strike)
            .collect();
        strikes.sort();
        strikes.dedup();

        for w in strikes.windows(2) {
            assert_eq!(w[1] - w[0], dec!(50), "ETH near-term spacing should be $50");
        }
    }

    #[test]
    fn test_intrinsic_value_call_itm() {
        let inst = OptionInstrument {
            symbol: "BTC-20260401-70000-C".into(),
            underlying: "BTC".into(),
            strike: dec!(70000),
            expiry: Utc::now(),
            option_type: OptionType::Call,
            settlement: SettlementType::Cash,
            tick_size: dec!(1),
            lot_size: dec!(0.001),
            min_notional: dec!(10),
        };
        assert_eq!(inst.intrinsic_value(dec!(72000)), dec!(2000));
    }

    #[test]
    fn test_intrinsic_value_put_otm() {
        let inst = OptionInstrument {
            symbol: "BTC-20260401-70000-P".into(),
            underlying: "BTC".into(),
            strike: dec!(70000),
            expiry: Utc::now(),
            option_type: OptionType::Put,
            settlement: SettlementType::Cash,
            tick_size: dec!(1),
            lot_size: dec!(0.001),
            min_notional: dec!(10),
        };
        // S=72000 > K=70000 → OTM put → intrinsic = 0.
        assert_eq!(inst.intrinsic_value(dec!(72000)), Decimal::ZERO);
    }

    #[test]
    fn test_empty_result_for_zero_spot() {
        let now = fixed_now();
        let expiry = next_friday_on_or_after(now + Duration::days(7));
        let instruments =
            OptionsInstrumentGenerator::generate_instruments("BTC", Decimal::ZERO, &[expiry], now);
        assert!(instruments.is_empty());
    }
}
