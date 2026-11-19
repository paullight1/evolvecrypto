//! Funding settlement scheduler.
//!
//! Standard Bybit-style funding times for 8h intervals: 00:00, 08:00, 16:00 UTC.
//! For other intervals the schedule is derived by dividing the UTC day at equal
//! increments starting from midnight.

use chrono::{DateTime, Duration, DurationRound, Utc};
use serde::{Deserialize, Serialize};

use crate::error::SettlementError;

/// Determines the next funding settlement time given the current time and interval.
///
/// The schedule is anchored to UTC midnight: slots are at
/// 00:00, 00:00+interval, 00:00+2*interval, … within each UTC day.
/// For a standard 8h interval that is 00:00, 08:00, 16:00 UTC.
///
/// If `now` is exactly on a slot boundary it is treated as just-settled and
/// the *next* slot is returned.
pub fn next_funding_time(now: DateTime<Utc>, interval_hours: u32) -> Result<DateTime<Utc>, SettlementError> {
    if interval_hours == 0 {
        return Err(SettlementError::InvalidFundingInterval(interval_hours));
    }
    if 24 % interval_hours != 0 {
        return Err(SettlementError::InvalidFundingInterval(interval_hours));
    }

    let interval_secs = i64::from(interval_hours) * 3600;
    let interval = Duration::seconds(interval_secs);

    // Truncate to the start of the current interval slot.
    let slot_start = now
        .duration_trunc(interval)
        .map_err(|_| SettlementError::InvalidFundingInterval(interval_hours))?;

    Ok(slot_start + interval)
}

/// Returns `true` if `now` falls within `tolerance_seconds` of a funding slot boundary.
pub fn is_funding_time(
    now: DateTime<Utc>,
    interval_hours: u32,
    tolerance_seconds: i64,
) -> Result<bool, SettlementError> {
    let next = next_funding_time(now, interval_hours)?;
    let secs_until_next = (next - now).num_seconds();

    // Also check whether we just passed a boundary (within tolerance in the past).
    let interval_secs = i64::from(interval_hours) * 3600;
    let secs_since_last = interval_secs - secs_until_next;

    Ok(secs_until_next <= tolerance_seconds || secs_since_last <= tolerance_seconds)
}

/// Scheduler state that tracks the last settled time and computes upcoming slots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundingScheduler {
    pub interval_hours: u32,
    pub last_settled_at: Option<DateTime<Utc>>,
}

impl FundingScheduler {
    pub fn new(interval_hours: u32) -> Self {
        Self {
            interval_hours,
            last_settled_at: None,
        }
    }

    /// Next funding time from the perspective of `now`.
    pub fn next_funding_time(&self, now: DateTime<Utc>) -> Result<DateTime<Utc>, SettlementError> {
        next_funding_time(now, self.interval_hours)
    }

    /// Check whether it is currently funding time (within ±`tolerance_seconds`).
    pub fn is_funding_time(
        &self,
        now: DateTime<Utc>,
        tolerance_seconds: i64,
    ) -> Result<bool, SettlementError> {
        is_funding_time(now, self.interval_hours, tolerance_seconds)
    }

    /// Mark that settlement has just occurred at `now`.
    pub fn record_settlement(&mut self, now: DateTime<Utc>) {
        self.last_settled_at = Some(now);
    }

    /// Returns `true` if settlement is due: funding time is now AND it hasn't
    /// been settled since the last slot boundary.
    pub fn is_settlement_due(
        &self,
        now: DateTime<Utc>,
        tolerance_seconds: i64,
    ) -> Result<bool, SettlementError> {
        if !self.is_funding_time(now, tolerance_seconds)? {
            return Ok(false);
        }

        // If we've never settled, it's definitely due.
        let Some(last) = self.last_settled_at else {
            return Ok(true);
        };

        // Compute the current slot boundary.
        let interval_secs = i64::from(self.interval_hours) * 3600;
        let interval = Duration::seconds(interval_secs);
        let current_slot = now
            .duration_trunc(interval)
            .map_err(|_| SettlementError::InvalidFundingInterval(self.interval_hours))?;

        // Settlement is due if last settlement was before the current slot boundary.
        Ok(last < current_slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn utc(h: u32, m: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 1, 15, h, m, s).unwrap()
    }

    // --- next_funding_time ---

    #[test]
    fn test_next_funding_from_midnight() {
        // At 00:00:00, the current slot is 00:00, next is 08:00
        let t = utc(0, 0, 0);
        let next = next_funding_time(t, 8).unwrap();
        assert_eq!(next, utc(8, 0, 0));
    }

    #[test]
    fn test_next_funding_mid_interval() {
        // At 04:00, next slot is 08:00
        let t = utc(4, 0, 0);
        let next = next_funding_time(t, 8).unwrap();
        assert_eq!(next, utc(8, 0, 0));
    }

    #[test]
    fn test_next_funding_just_before_slot() {
        // At 07:59:59, still within current slot → next is 08:00
        let t = utc(7, 59, 59);
        let next = next_funding_time(t, 8).unwrap();
        assert_eq!(next, utc(8, 0, 0));
    }

    #[test]
    fn test_next_funding_at_slot_boundary() {
        // At exactly 08:00:00, current slot truncates to 08:00 → next is 16:00
        let t = utc(8, 0, 0);
        let next = next_funding_time(t, 8).unwrap();
        assert_eq!(next, utc(16, 0, 0));
    }

    #[test]
    fn test_next_funding_last_slot_wraps_to_next_day() {
        // At 17:00, next is 00:00 on the following day
        let t = utc(17, 0, 0);
        let next = next_funding_time(t, 8).unwrap();
        let expected = Utc.with_ymd_and_hms(2024, 1, 16, 0, 0, 0).unwrap();
        assert_eq!(next, expected);
    }

    #[test]
    fn test_next_funding_4h_interval() {
        // At 05:00 with 4h interval: current slot is 04:00, next is 08:00
        let t = utc(5, 0, 0);
        let next = next_funding_time(t, 4).unwrap();
        assert_eq!(next, utc(8, 0, 0));
    }

    #[test]
    fn test_next_funding_invalid_interval_zero() {
        let result = next_funding_time(utc(0, 0, 0), 0);
        assert!(matches!(result, Err(SettlementError::InvalidFundingInterval(0))));
    }

    #[test]
    fn test_next_funding_invalid_interval_non_divisor() {
        // 7 does not divide 24
        let result = next_funding_time(utc(0, 0, 0), 7);
        assert!(matches!(result, Err(SettlementError::InvalidFundingInterval(7))));
    }

    // --- is_funding_time ---

    #[test]
    fn test_is_funding_time_at_boundary() {
        // At exactly 08:00:00 with 30s tolerance
        let t = utc(8, 0, 0);
        // The boundary is at 16:00 next (since 08:00 truncates to slot start).
        // 08:00:00 itself: secs_until_next = 8*3600, secs_since_last = 0 → within 30s
        assert!(is_funding_time(t, 8, 30).unwrap());
    }

    #[test]
    fn test_is_not_funding_time_midpoint() {
        let t = utc(4, 0, 0);
        assert!(!is_funding_time(t, 8, 30).unwrap());
    }

    #[test]
    fn test_is_funding_time_just_before() {
        // 29 seconds before the boundary
        let t = Utc.with_ymd_and_hms(2024, 1, 15, 7, 59, 31).unwrap();
        assert!(is_funding_time(t, 8, 30).unwrap());
    }

    #[test]
    fn test_is_funding_time_just_after() {
        // 10 seconds after 08:00 boundary
        let t = Utc.with_ymd_and_hms(2024, 1, 15, 8, 0, 10).unwrap();
        assert!(is_funding_time(t, 8, 30).unwrap());
    }

    // --- FundingScheduler ---

    #[test]
    fn test_scheduler_settlement_due_never_settled() {
        let scheduler = FundingScheduler::new(8);
        // At exactly 08:00 (boundary), settlement is due when never settled before
        let t = utc(8, 0, 0);
        assert!(scheduler.is_settlement_due(t, 30).unwrap());
    }

    #[test]
    fn test_scheduler_not_due_after_settlement() {
        let mut scheduler = FundingScheduler::new(8);
        let t = utc(8, 0, 0);
        scheduler.record_settlement(t);
        // 5 seconds later: same slot, already settled
        let t2 = Utc.with_ymd_and_hms(2024, 1, 15, 8, 0, 5).unwrap();
        assert!(!scheduler.is_settlement_due(t2, 30).unwrap());
    }

    #[test]
    fn test_scheduler_due_after_full_interval() {
        let mut scheduler = FundingScheduler::new(8);
        scheduler.record_settlement(utc(8, 0, 0));
        // At 16:00 — new slot, not yet settled
        let t = utc(16, 0, 0);
        assert!(scheduler.is_settlement_due(t, 30).unwrap());
    }
}
