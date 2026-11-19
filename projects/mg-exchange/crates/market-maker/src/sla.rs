//! SLA monitoring engine for registered market makers.
//!
//! Tracks periodic snapshots of each MM's quoting behaviour and evaluates
//! compliance against configurable thresholds. Non-compliant MMs receive a
//! [`DemotionAction`] that the caller is expected to enforce.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use tracing::{debug, info, warn};
use uuid::Uuid;

// ── errors ───────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum SlaError {
    #[error("market maker {0} is not registered")]
    UnknownMm(Uuid),
}

// ── config ────────────────────────────────────────────────────────────────────

/// SLA parameters applied uniformly to every registered MM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MmSlaConfig {
    /// Minimum fraction of time that the MM must be actively quoting (0–100).
    pub min_uptime_pct: Decimal,
    /// Maximum allowed average spread in basis points.
    pub max_spread_bps: Decimal,
    /// Minimum order-book depth (quote asset notional) that must be present on
    /// each side while the MM is quoting.
    pub min_depth_quote: Decimal,
    /// Minimum fraction of snapshots with two-sided quotes (0–100).
    pub min_two_sided_pct: Decimal,
    /// Maximum average quote-refill time in seconds.
    pub max_refill_secs: Decimal,
    /// Number of consecutive non-compliant evaluation periods before demotion
    /// escalates from `Warning` to `ReduceRebates`.
    pub warning_threshold: u32,
    /// Number of consecutive non-compliant periods before `SuspendMm`.
    pub suspend_threshold: u32,
    /// Evaluation window: only snapshots taken within the last `window_secs`
    /// seconds are considered in each [`MmSlaMonitor::evaluate`] call.
    pub window_secs: u64,
}

impl Default for MmSlaConfig {
    fn default() -> Self {
        Self {
            min_uptime_pct: Decimal::new(95, 0),
            max_spread_bps: Decimal::new(10, 0),
            min_depth_quote: Decimal::new(10_000, 0),
            min_two_sided_pct: Decimal::new(90, 0),
            max_refill_secs: Decimal::new(5, 0),
            warning_threshold: 2,
            suspend_threshold: 5,
            window_secs: 3600,
        }
    }
}

// ── snapshot ──────────────────────────────────────────────────────────────────

/// A single point-in-time observation of a market maker's quoting state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MmSnapshot {
    pub mm_id: Uuid,
    pub timestamp: DateTime<Utc>,
    /// Whether the MM was actively quoting at snapshot time.
    pub is_present: bool,
    /// Current spread in basis points (`None` when not quoting).
    pub spread_bps: Option<Decimal>,
    /// Bid-side depth in quote-asset notional (`None` when not quoting).
    pub bid_depth_quote: Option<Decimal>,
    /// Ask-side depth in quote-asset notional (`None` when not quoting).
    pub ask_depth_quote: Option<Decimal>,
    /// Time since last quote refresh in seconds (`None` when not quoting or
    /// for the very first quote).
    pub refill_secs: Option<Decimal>,
}

impl MmSnapshot {
    /// Returns `true` when the MM is quoting on both sides simultaneously.
    pub fn is_two_sided(&self) -> bool {
        self.is_present && self.bid_depth_quote.is_some() && self.ask_depth_quote.is_some()
    }

    /// Returns `true` when both bid and ask depths satisfy `min_depth`.
    pub fn min_depth_met(&self, min_depth: Decimal) -> bool {
        match (self.bid_depth_quote, self.ask_depth_quote) {
            (Some(bid), Some(ask)) => bid >= min_depth && ask >= min_depth,
            _ => false,
        }
    }
}

// ── report ────────────────────────────────────────────────────────────────────

/// Outcome of a single SLA evaluation for one market maker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MmSlaReport {
    pub mm_id: Uuid,
    pub evaluated_at: DateTime<Utc>,
    /// Fraction of evaluation-window snapshots where the MM was present (0–100).
    pub uptime_pct: Decimal,
    /// Average spread across present snapshots, in basis points.
    pub avg_spread_bps: Decimal,
    /// Fraction of snapshots where min-depth was satisfied (0–100).
    pub min_depth_met_pct: Decimal,
    /// Fraction of snapshots where the MM quoted two-sided (0–100).
    pub two_sided_pct: Decimal,
    /// Average quote-refill latency across snapshots that reported a refill
    /// time, in seconds. Zero when no refill data is available.
    pub avg_refill_time_secs: Decimal,
    /// Whether all SLA thresholds are met.
    pub is_compliant: bool,
    /// Recommended action when `is_compliant` is `false`.
    pub demotion: Option<DemotionAction>,
}

// ── demotion action ───────────────────────────────────────────────────────────

/// Action recommended when an MM breaches its SLA.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DemotionAction {
    /// First breach — send a warning notification.
    Warning,
    /// Repeated breach — apply rebate reduction.
    ReduceRebates,
    /// Sustained breach — suspend MM privileges entirely.
    SuspendMm,
}

// ── per-MM state ──────────────────────────────────────────────────────────────

#[derive(Debug)]
struct MmState {
    snapshots: Vec<MmSnapshot>,
    consecutive_violations: u32,
    is_suspended: bool,
}

impl MmState {
    fn new() -> Self {
        Self {
            snapshots: Vec::new(),
            consecutive_violations: 0,
            is_suspended: false,
        }
    }

    /// Purge snapshots older than `window_secs` relative to `now`.
    fn prune(&mut self, now: DateTime<Utc>, window_secs: u64) {
        let cutoff = now - chrono::Duration::seconds(window_secs as i64);
        self.snapshots.retain(|s| s.timestamp >= cutoff);
    }
}

// ── monitor ───────────────────────────────────────────────────────────────────

/// Central SLA monitor: register MMs, feed snapshots, evaluate compliance.
#[derive(Debug)]
pub struct MmSlaMonitor {
    config: MmSlaConfig,
    mms: HashMap<Uuid, MmState>,
}

impl MmSlaMonitor {
    pub fn new(config: MmSlaConfig) -> Self {
        Self {
            config,
            mms: HashMap::new(),
        }
    }

    /// Register a new market maker. Idempotent — re-registering resets state.
    pub fn register_mm(&mut self, mm_id: Uuid) {
        info!(mm_id = %mm_id, "registering market maker");
        self.mms.insert(mm_id, MmState::new());
    }

    /// Feed a new snapshot for the given MM.
    pub fn record_snapshot(&mut self, snapshot: MmSnapshot) -> Result<(), SlaError> {
        let state = self
            .mms
            .get_mut(&snapshot.mm_id)
            .ok_or(SlaError::UnknownMm(snapshot.mm_id))?;

        debug!(
            mm_id = %snapshot.mm_id,
            is_present = snapshot.is_present,
            spread_bps = ?snapshot.spread_bps,
            "recording MM snapshot"
        );
        state.snapshots.push(snapshot);
        Ok(())
    }

    /// Evaluate the SLA for `mm_id` as of `now`.
    ///
    /// Snapshots outside the configured `window_secs` are pruned before
    /// evaluation. If no snapshots exist within the window the MM is treated
    /// as fully absent (uptime = 0 %).
    pub fn evaluate(&mut self, mm_id: Uuid, now: DateTime<Utc>) -> Result<MmSlaReport, SlaError> {
        let state = self
            .mms
            .get_mut(&mm_id)
            .ok_or(SlaError::UnknownMm(mm_id))?;

        state.prune(now, self.config.window_secs);

        let snapshots = &state.snapshots;
        let total = snapshots.len();

        // ── uptime ──
        let uptime_pct = if total == 0 {
            Decimal::ZERO
        } else {
            let present = snapshots.iter().filter(|s| s.is_present).count();
            Decimal::from(present * 100) / Decimal::from(total)
        };

        // ── avg spread (only present snapshots that reported a spread) ──
        let spread_samples: Vec<Decimal> = snapshots
            .iter()
            .filter_map(|s| s.spread_bps)
            .collect();
        let avg_spread_bps = mean(&spread_samples);

        // ── min-depth met pct ──
        let min_depth_met_pct = if total == 0 {
            Decimal::ZERO
        } else {
            let met = snapshots
                .iter()
                .filter(|s| s.min_depth_met(self.config.min_depth_quote))
                .count();
            Decimal::from(met * 100) / Decimal::from(total)
        };

        // ── two-sided pct ──
        let two_sided_pct = if total == 0 {
            Decimal::ZERO
        } else {
            let two_sided = snapshots.iter().filter(|s| s.is_two_sided()).count();
            Decimal::from(two_sided * 100) / Decimal::from(total)
        };

        // ── avg refill ──
        let refill_samples: Vec<Decimal> = snapshots
            .iter()
            .filter_map(|s| s.refill_secs)
            .collect();
        let avg_refill_time_secs = mean(&refill_samples);

        // ── compliance check ──
        let is_compliant = uptime_pct >= self.config.min_uptime_pct
            && (spread_samples.is_empty() || avg_spread_bps <= self.config.max_spread_bps)
            && min_depth_met_pct >= self.config.min_two_sided_pct
            && two_sided_pct >= self.config.min_two_sided_pct
            && (refill_samples.is_empty() || avg_refill_time_secs <= self.config.max_refill_secs);

        // ── violation counter & demotion ──
        let demotion = if is_compliant {
            state.consecutive_violations = 0;
            None
        } else {
            state.consecutive_violations += 1;
            let v = state.consecutive_violations;
            let action = if v >= self.config.suspend_threshold {
                DemotionAction::SuspendMm
            } else if v >= self.config.warning_threshold {
                DemotionAction::ReduceRebates
            } else {
                DemotionAction::Warning
            };
            warn!(
                mm_id = %mm_id,
                consecutive_violations = v,
                ?action,
                "MM SLA breach"
            );
            Some(action)
        };

        Ok(MmSlaReport {
            mm_id,
            evaluated_at: now,
            uptime_pct,
            avg_spread_bps,
            min_depth_met_pct,
            two_sided_pct,
            avg_refill_time_secs,
            is_compliant,
            demotion,
        })
    }

    /// Convenience: evaluate and apply suspension flag when
    /// [`DemotionAction::SuspendMm`] is returned.
    pub fn demote_if_non_compliant(
        &mut self,
        mm_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<MmSlaReport, SlaError> {
        let report = self.evaluate(mm_id, now)?;
        if matches!(report.demotion, Some(DemotionAction::SuspendMm)) {
            if let Some(state) = self.mms.get_mut(&mm_id) {
                state.is_suspended = true;
                warn!(mm_id = %mm_id, "market maker suspended");
            }
        }
        Ok(report)
    }

    /// Returns `true` when the MM is registered and not suspended.
    pub fn is_compliant(&self, mm_id: Uuid) -> bool {
        self.mms
            .get(&mm_id)
            .map(|s| !s.is_suspended)
            .unwrap_or(false)
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn mean(values: &[Decimal]) -> Decimal {
    if values.is_empty() {
        return Decimal::ZERO;
    }
    let sum: Decimal = values.iter().sum();
    sum / Decimal::from(values.len())
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

    fn default_monitor() -> MmSlaMonitor {
        MmSlaMonitor::new(MmSlaConfig {
            min_uptime_pct: dec!(80),
            max_spread_bps: dec!(10),
            min_depth_quote: dec!(1000),
            min_two_sided_pct: dec!(80),
            max_refill_secs: dec!(5),
            warning_threshold: 2,
            suspend_threshold: 4,
            window_secs: 3600,
        })
    }

    fn present_snapshot(mm_id: Uuid, now: DateTime<Utc>) -> MmSnapshot {
        MmSnapshot {
            mm_id,
            timestamp: now,
            is_present: true,
            spread_bps: Some(dec!(5)),
            bid_depth_quote: Some(dec!(5000)),
            ask_depth_quote: Some(dec!(5000)),
            refill_secs: Some(dec!(1)),
        }
    }

    fn absent_snapshot(mm_id: Uuid, now: DateTime<Utc>) -> MmSnapshot {
        MmSnapshot {
            mm_id,
            timestamp: now,
            is_present: false,
            spread_bps: None,
            bid_depth_quote: None,
            ask_depth_quote: None,
            refill_secs: None,
        }
    }

    // T1: register then evaluate with no snapshots → zero uptime, non-compliant
    #[test]
    fn evaluate_with_no_snapshots_returns_zero_uptime() {
        let mut monitor = default_monitor();
        let mm_id = Uuid::new_v4();
        monitor.register_mm(mm_id);

        let report = monitor.evaluate(mm_id, fixed_now()).unwrap();
        assert_eq!(report.uptime_pct, Decimal::ZERO);
        assert!(!report.is_compliant);
    }

    // T2: unknown MM returns error
    #[test]
    fn evaluate_unknown_mm_returns_error() {
        let mut monitor = default_monitor();
        let result = monitor.evaluate(Uuid::new_v4(), fixed_now());
        assert!(matches!(result, Err(SlaError::UnknownMm(_))));
    }

    // T3: uptime calculation — 3 present, 1 absent → 75 %
    #[test]
    fn uptime_calculated_correctly() {
        let mut monitor = default_monitor();
        let mm_id = Uuid::new_v4();
        monitor.register_mm(mm_id);
        let now = fixed_now();

        for _ in 0..3 {
            monitor.record_snapshot(present_snapshot(mm_id, now)).unwrap();
        }
        monitor.record_snapshot(absent_snapshot(mm_id, now)).unwrap();

        let report = monitor.evaluate(mm_id, now).unwrap();
        assert_eq!(report.uptime_pct, dec!(75));
    }

    // T4: uptime 100 % when all snapshots are present
    #[test]
    fn uptime_100_when_all_present() {
        let mut monitor = default_monitor();
        let mm_id = Uuid::new_v4();
        monitor.register_mm(mm_id);
        let now = fixed_now();

        for _ in 0..5 {
            monitor.record_snapshot(present_snapshot(mm_id, now)).unwrap();
        }

        let report = monitor.evaluate(mm_id, now).unwrap();
        assert_eq!(report.uptime_pct, dec!(100));
    }

    // T5: spread averaging across multiple snapshots
    #[test]
    fn spread_averaged_correctly() {
        let mut monitor = default_monitor();
        let mm_id = Uuid::new_v4();
        monitor.register_mm(mm_id);
        let now = fixed_now();

        let spreads = [dec!(4), dec!(6), dec!(8)];
        for &s in &spreads {
            let mut snap = present_snapshot(mm_id, now);
            snap.spread_bps = Some(s);
            monitor.record_snapshot(snap).unwrap();
        }

        let report = monitor.evaluate(mm_id, now).unwrap();
        // mean of 4, 6, 8 = 6
        assert_eq!(report.avg_spread_bps, dec!(6));
    }

    // T6: two-sided detection — only snapshots with both sides count
    #[test]
    fn two_sided_pct_only_counts_both_sides() {
        let mut monitor = default_monitor();
        let mm_id = Uuid::new_v4();
        monitor.register_mm(mm_id);
        let now = fixed_now();

        // 2 two-sided, 1 bid-only, 1 absent → 50 %
        monitor.record_snapshot(present_snapshot(mm_id, now)).unwrap();
        monitor.record_snapshot(present_snapshot(mm_id, now)).unwrap();

        let mut bid_only = present_snapshot(mm_id, now);
        bid_only.ask_depth_quote = None;
        monitor.record_snapshot(bid_only).unwrap();

        monitor.record_snapshot(absent_snapshot(mm_id, now)).unwrap();

        let report = monitor.evaluate(mm_id, now).unwrap();
        assert_eq!(report.two_sided_pct, dec!(50));
    }

    // T7: min-depth compliance detection
    #[test]
    fn min_depth_met_pct_reflects_depth_failures() {
        let mut monitor = default_monitor();
        let mm_id = Uuid::new_v4();
        monitor.register_mm(mm_id);
        let now = fixed_now();

        // 1 above min_depth=1000, 1 below
        monitor.record_snapshot(present_snapshot(mm_id, now)).unwrap(); // depth = 5000 ✓

        let mut shallow = present_snapshot(mm_id, now);
        shallow.bid_depth_quote = Some(dec!(100)); // below 1000
        monitor.record_snapshot(shallow).unwrap();

        let report = monitor.evaluate(mm_id, now).unwrap();
        assert_eq!(report.min_depth_met_pct, dec!(50));
    }

    // T8: demotion escalates from Warning → ReduceRebates → SuspendMm
    #[test]
    fn demotion_escalates_across_evaluations() {
        let mut monitor = default_monitor(); // warning_threshold=2, suspend_threshold=4
        let mm_id = Uuid::new_v4();
        monitor.register_mm(mm_id);
        let now = fixed_now();

        // No snapshots → always non-compliant.
        let r1 = monitor.evaluate(mm_id, now).unwrap();
        assert_eq!(r1.demotion, Some(DemotionAction::Warning)); // violation 1

        let r2 = monitor.evaluate(mm_id, now).unwrap();
        assert_eq!(r2.demotion, Some(DemotionAction::ReduceRebates)); // violation 2

        let r3 = monitor.evaluate(mm_id, now).unwrap();
        assert_eq!(r3.demotion, Some(DemotionAction::ReduceRebates)); // violation 3

        let r4 = monitor.evaluate(mm_id, now).unwrap();
        assert_eq!(r4.demotion, Some(DemotionAction::SuspendMm)); // violation 4
    }

    // T9: compliant MM stays active — violations reset on compliance
    #[test]
    fn compliant_mm_resets_violations() {
        let mut monitor = MmSlaMonitor::new(MmSlaConfig {
            min_uptime_pct: dec!(50),
            max_spread_bps: dec!(20),
            min_depth_quote: dec!(100),
            min_two_sided_pct: dec!(50),
            max_refill_secs: dec!(10),
            warning_threshold: 2,
            suspend_threshold: 5,
            window_secs: 3600,
        });
        let mm_id = Uuid::new_v4();
        monitor.register_mm(mm_id);
        let now = fixed_now();

        // Trigger one violation.
        monitor.evaluate(mm_id, now).unwrap();

        // Now feed good snapshots so the next evaluation is compliant.
        for _ in 0..5 {
            monitor.record_snapshot(present_snapshot(mm_id, now)).unwrap();
        }

        let report = monitor.evaluate(mm_id, now).unwrap();
        assert!(report.is_compliant);
        assert!(report.demotion.is_none());

        // Violation counter must be reset — next non-compliant eval goes back to Warning.
        // Drain snapshots by using a far-future `now` outside the window.
        let future = now + chrono::Duration::seconds(7200);
        let r = monitor.evaluate(mm_id, future).unwrap();
        assert_eq!(r.demotion, Some(DemotionAction::Warning));
    }

    // T10: demote_if_non_compliant sets suspended flag
    #[test]
    fn demote_sets_suspended_flag() {
        let mut monitor = MmSlaMonitor::new(MmSlaConfig {
            suspend_threshold: 1,
            warning_threshold: 1,
            ..MmSlaConfig::default()
        });
        let mm_id = Uuid::new_v4();
        monitor.register_mm(mm_id);

        assert!(monitor.is_compliant(mm_id));

        let now = fixed_now();
        monitor.demote_if_non_compliant(mm_id, now).unwrap();

        assert!(!monitor.is_compliant(mm_id));
    }
}
