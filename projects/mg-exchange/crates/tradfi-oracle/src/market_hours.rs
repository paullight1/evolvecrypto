//! Market session definitions and trading-hours logic for TradFi instruments.
//!
//! TradFi instruments trade on regulated schedules unlike crypto which is 24/7.
//! This module determines whether a market is currently open and when it will
//! next open or close — enabling the exchange to gate order acceptance for
//! out-of-hours symbols.
//!
//! # Sessions defined
//! | Asset class   | Session name  | Hours (UTC)           | Days        |
//! |---------------|---------------|-----------------------|-------------|
//! | Forex         | Forex         | Sun 22:00 – Fri 22:00 | 24/5        |
//! | Metals        | Metals        | Sun 22:00 – Fri 22:00 | 24/5 (LBMA) |
//! | Equities      | NYSE          | 14:30 – 21:00         | Mon–Fri     |
//! | Oil (WTI)     | NYMEX         | Sun 23:00 – Fri 22:00 | ~24/5       |
//!
//! All times are UTC.

use std::collections::HashMap;

use chrono::{DateTime, Datelike, Duration, NaiveTime, TimeZone, Timelike, Utc, Weekday};

// ── Asset class ───────────────────────────────────────────────────────────────

/// Broad asset class for a TradFi perpetual.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssetClass {
    /// Major and minor forex pairs (EUR/USD, GBP/USD, …).
    Forex,
    /// Precious metals (XAU, XAG). Follows electronic forex hours.
    Metals,
    /// Equity index futures (SPX, NDX). Follows NYSE/CME hours.
    Equities,
    /// Energy commodities (WTI crude). Follows NYMEX hours.
    Energy,
}

// ── Market session ────────────────────────────────────────────────────────────

/// Defines the trading schedule for one market session.
#[derive(Debug, Clone)]
pub struct MarketSession {
    pub name: String,
    /// Asset class this session applies to.
    pub asset_class: AssetClass,
    /// UTC time of day when the session opens (hour, minute).
    pub open_utc: (u32, u32),
    /// UTC time of day when the session closes (hour, minute).
    pub close_utc: (u32, u32),
    /// ISO weekdays on which the session runs (Mon = 1 … Sun = 7).
    pub trading_days: Vec<Weekday>,
    /// True for sessions that run 24 hours across their trading days
    /// (open time == close time convention).
    pub is_continuous: bool,
}

/// Map from exchange-style symbol (e.g. `"XAUUSD"`) to [`AssetClass`].
pub fn asset_class_for_symbol(symbol: &str) -> Option<AssetClass> {
    match symbol {
        "EURUSD" | "GBPUSD" | "JPYUSD" | "AUDUSD" | "CHFUSD" => Some(AssetClass::Forex),
        "XAUUSD" | "XAGUSD" => Some(AssetClass::Metals),
        "SPXUSD" | "NDXUSD" => Some(AssetClass::Equities),
        "WTIUSD" => Some(AssetClass::Energy),
        _ => None,
    }
}

// ── Trading hours registry ────────────────────────────────────────────────────

/// Registry of market sessions indexed by [`AssetClass`].
///
/// Create once at startup via [`TradingHours::default`] and share via `Arc`.
pub struct TradingHours {
    sessions: HashMap<AssetClass, MarketSession>,
}

impl Default for TradingHours {
    /// Construct the standard TradFi trading hours registry.
    fn default() -> Self {
        let mut sessions = HashMap::new();

        // ── Forex — 24/5 (Sun 22:00 UTC open, Fri 22:00 UTC close) ──────────
        // Electronically the market opens as Auckland/Wellington come online
        // Sunday evening UTC and closes when New York closes Friday evening.
        sessions.insert(
            AssetClass::Forex,
            MarketSession {
                name: "Forex".to_string(),
                asset_class: AssetClass::Forex,
                open_utc: (22, 0),
                close_utc: (22, 0),
                trading_days: vec![
                    Weekday::Sun,
                    Weekday::Mon,
                    Weekday::Tue,
                    Weekday::Wed,
                    Weekday::Thu,
                    Weekday::Fri,
                ],
                is_continuous: true,
            },
        );

        // ── Metals — same hours as forex (LBMA + electronic) ─────────────────
        sessions.insert(
            AssetClass::Metals,
            MarketSession {
                name: "Metals".to_string(),
                asset_class: AssetClass::Metals,
                open_utc: (22, 0),
                close_utc: (22, 0),
                trading_days: vec![
                    Weekday::Sun,
                    Weekday::Mon,
                    Weekday::Tue,
                    Weekday::Wed,
                    Weekday::Thu,
                    Weekday::Fri,
                ],
                is_continuous: true,
            },
        );

        // ── NYSE / Equities — 14:30–21:00 UTC Mon–Fri ────────────────────────
        // Core NYSE hours. Extended hours (pre-market 09:00, after-hours 01:00)
        // can be added as a separate session in Phase 2.
        sessions.insert(
            AssetClass::Equities,
            MarketSession {
                name: "NYSE".to_string(),
                asset_class: AssetClass::Equities,
                open_utc: (14, 30),
                close_utc: (21, 0),
                trading_days: vec![
                    Weekday::Mon,
                    Weekday::Tue,
                    Weekday::Wed,
                    Weekday::Thu,
                    Weekday::Fri,
                ],
                is_continuous: false,
            },
        );

        // ── NYMEX / Energy — Sun 23:00 – Fri 22:00 UTC (~24/5) ───────────────
        // WTI crude oil electronic trading on NYMEX (CME Globex).
        sessions.insert(
            AssetClass::Energy,
            MarketSession {
                name: "NYMEX".to_string(),
                asset_class: AssetClass::Energy,
                open_utc: (23, 0),
                close_utc: (22, 0),
                trading_days: vec![
                    Weekday::Sun,
                    Weekday::Mon,
                    Weekday::Tue,
                    Weekday::Wed,
                    Weekday::Thu,
                    Weekday::Fri,
                ],
                is_continuous: true,
            },
        );

        Self { sessions }
    }
}

impl TradingHours {
    /// Returns `true` when the market for `symbol` is currently open at `now`.
    ///
    /// Returns `false` for unknown symbols (conservative: block trading).
    pub fn is_market_open(&self, symbol: &str, now: DateTime<Utc>) -> bool {
        let Some(class) = asset_class_for_symbol(symbol) else {
            return false;
        };
        let Some(session) = self.sessions.get(&class) else {
            return false;
        };

        self.session_open(session, now)
    }

    /// Returns the session definition for a symbol's asset class.
    pub fn session_for_symbol(&self, symbol: &str) -> Option<&MarketSession> {
        let class = asset_class_for_symbol(symbol)?;
        self.sessions.get(&class)
    }

    /// Compute the next time the market for `symbol` will open after `now`.
    ///
    /// Returns `None` for unknown symbols.
    pub fn next_open(&self, symbol: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let session = self.session_for_symbol(symbol)?;
        // If currently open, the "next" open is the next time it reopens after
        // closing. Search up to 8 days forward.
        let mut candidate = now;
        for _ in 0..=(8 * 24) {
            candidate += Duration::hours(1);
            if self.session_open(session, candidate)
                && !self.session_open(session, candidate - Duration::minutes(1))
            {
                // This hour is the first open minute after a closed period.
                return Some(snap_to_open(session, candidate));
            }
        }
        None
    }

    /// Compute the next time the market for `symbol` will close after `now`.
    ///
    /// Returns `None` for unknown symbols or when the market is currently closed.
    pub fn next_close(&self, symbol: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let session = self.session_for_symbol(symbol)?;
        let mut candidate = now;
        for _ in 0..=(8 * 24) {
            candidate += Duration::hours(1);
            if !self.session_open(session, candidate)
                && self.session_open(session, candidate - Duration::minutes(1))
            {
                return Some(snap_to_close(session, candidate));
            }
        }
        None
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    /// Core session-open predicate.
    ///
    /// For non-continuous sessions (e.g. NYSE): open iff the weekday is a
    /// trading day AND current UTC time is within [open_utc, close_utc).
    ///
    /// For continuous sessions (forex, metals, NYMEX): open iff the weekday is
    /// a trading day AND we are not in the weekly shutdown window. The shutdown
    /// is: Fri close_utc through Sun open_utc (values from the session config).
    fn session_open(&self, session: &MarketSession, now: DateTime<Utc>) -> bool {
        let weekday = now.weekday();
        let current_time = NaiveTime::from_hms_opt(now.hour(), now.minute(), 0)
            .expect("valid h/m");
        let open_time = NaiveTime::from_hms_opt(session.open_utc.0, session.open_utc.1, 0)
            .expect("valid open_utc");
        let close_time = NaiveTime::from_hms_opt(session.close_utc.0, session.close_utc.1, 0)
            .expect("valid close_utc");

        if !session.is_continuous {
            // Discrete session: must be a trading day and within the time window.
            session.trading_days.contains(&weekday)
                && current_time >= open_time
                && current_time < close_time
        } else {
            // Continuous 24/5 session.
            // Closed window: Friday at close_utc through Sunday at open_utc.
            let is_friday_after_close =
                weekday == Weekday::Fri && current_time >= close_time;
            let is_saturday = weekday == Weekday::Sat;
            let is_sunday_before_open =
                weekday == Weekday::Sun && current_time < open_time;

            !(is_friday_after_close || is_saturday || is_sunday_before_open)
        }
    }
}

/// Snap a candidate datetime to the session's open time on the same day.
fn snap_to_open(session: &MarketSession, candidate: DateTime<Utc>) -> DateTime<Utc> {
    let naive = candidate.date_naive().and_hms_opt(
        session.open_utc.0,
        session.open_utc.1,
        0,
    ).expect("valid hms");
    Utc.from_utc_datetime(&naive)
}

/// Snap a candidate datetime to the session's close time on the same day.
fn snap_to_close(session: &MarketSession, candidate: DateTime<Utc>) -> DateTime<Utc> {
    let naive = candidate.date_naive().and_hms_opt(
        session.close_utc.0,
        session.close_utc.1,
        0,
    ).expect("valid hms");
    Utc.from_utc_datetime(&naive)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn hours() -> TradingHours {
        TradingHours::default()
    }

    /// Build a UTC datetime from components.
    fn dt(year: i32, month: u32, day: u32, hour: u32, min: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, min, 0)
            .single()
            .expect("valid datetime")
    }

    // 2024-01-15 is a Monday.
    // 2024-01-19 is a Friday.
    // 2024-01-20 is a Saturday.
    // 2024-01-21 is a Sunday.

    // ── Forex / Metals (24/5) ────────────────────────────────────────────────

    #[test]
    fn forex_open_on_monday_midday() {
        let h = hours();
        // Monday 12:00 UTC — well within the trading week.
        assert!(h.is_market_open("EURUSD", dt(2024, 1, 15, 12, 0)));
    }

    #[test]
    fn forex_open_on_friday_before_close() {
        let h = hours();
        // Friday 21:59 UTC — still open.
        assert!(h.is_market_open("EURUSD", dt(2024, 1, 19, 21, 59)));
    }

    #[test]
    fn forex_closed_on_friday_after_close() {
        let h = hours();
        // Friday 22:01 UTC — market closed for the weekend.
        assert!(!h.is_market_open("EURUSD", dt(2024, 1, 19, 22, 1)));
    }

    #[test]
    fn forex_closed_on_saturday() {
        let h = hours();
        assert!(!h.is_market_open("EURUSD", dt(2024, 1, 20, 12, 0)));
    }

    #[test]
    fn forex_closed_sunday_before_open() {
        let h = hours();
        // Sunday 21:59 UTC — not yet open.
        assert!(!h.is_market_open("EURUSD", dt(2024, 1, 21, 21, 59)));
    }

    #[test]
    fn forex_open_sunday_after_open() {
        let h = hours();
        // Sunday 22:01 UTC — just opened.
        assert!(h.is_market_open("EURUSD", dt(2024, 1, 21, 22, 1)));
    }

    #[test]
    fn gold_follows_metals_session() {
        let h = hours();
        // Monday open.
        assert!(h.is_market_open("XAUUSD", dt(2024, 1, 15, 10, 0)));
        // Saturday closed.
        assert!(!h.is_market_open("XAUUSD", dt(2024, 1, 20, 10, 0)));
    }

    // ── NYSE / Equities ──────────────────────────────────────────────────────

    #[test]
    fn spx_closed_before_open() {
        let h = hours();
        // Monday 14:29 UTC — just before NYSE opens.
        assert!(!h.is_market_open("SPXUSD", dt(2024, 1, 15, 14, 29)));
    }

    #[test]
    fn spx_open_during_session() {
        let h = hours();
        // Monday 15:00 UTC — during NYSE session.
        assert!(h.is_market_open("SPXUSD", dt(2024, 1, 15, 15, 0)));
    }

    #[test]
    fn spx_closed_after_close() {
        let h = hours();
        // Monday 21:01 UTC — after NYSE close.
        assert!(!h.is_market_open("SPXUSD", dt(2024, 1, 15, 21, 1)));
    }

    #[test]
    fn spx_closed_on_weekend() {
        let h = hours();
        assert!(!h.is_market_open("SPXUSD", dt(2024, 1, 20, 15, 0))); // Saturday
        assert!(!h.is_market_open("SPXUSD", dt(2024, 1, 21, 15, 0))); // Sunday
    }

    // ── NYMEX / Energy ───────────────────────────────────────────────────────

    #[test]
    fn wti_open_on_monday() {
        let h = hours();
        assert!(h.is_market_open("WTIUSD", dt(2024, 1, 15, 12, 0)));
    }

    #[test]
    fn wti_closed_on_saturday() {
        let h = hours();
        assert!(!h.is_market_open("WTIUSD", dt(2024, 1, 20, 12, 0)));
    }

    // ── Unknown symbol ───────────────────────────────────────────────────────

    #[test]
    fn unknown_symbol_is_closed() {
        let h = hours();
        // Conservative: unknown symbols are treated as closed to prevent erroneous trades.
        assert!(!h.is_market_open("UNKNOWNSYM", dt(2024, 1, 15, 12, 0)));
    }

    // ── session_for_symbol ───────────────────────────────────────────────────

    #[test]
    fn session_for_eurusd_is_forex() {
        let h = hours();
        let s = h.session_for_symbol("EURUSD").unwrap();
        assert_eq!(s.name, "Forex");
    }

    #[test]
    fn session_for_xauusd_is_metals() {
        let h = hours();
        let s = h.session_for_symbol("XAUUSD").unwrap();
        assert_eq!(s.name, "Metals");
    }

    #[test]
    fn session_for_spxusd_is_nyse() {
        let h = hours();
        let s = h.session_for_symbol("SPXUSD").unwrap();
        assert_eq!(s.name, "NYSE");
    }

    #[test]
    fn session_for_wtiusd_is_nymex() {
        let h = hours();
        let s = h.session_for_symbol("WTIUSD").unwrap();
        assert_eq!(s.name, "NYMEX");
    }

    #[test]
    fn session_for_unknown_symbol_is_none() {
        let h = hours();
        assert!(h.session_for_symbol("XXXYYY").is_none());
    }
}
