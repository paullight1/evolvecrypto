//! Market-maker support library for the exchange.
//!
//! Provides four independent modules:
//!
//! * [`sla`] — SLA monitoring: snapshot ingestion, uptime/spread/depth/refill
//!   tracking, and demotion actions for non-compliant market makers.
//!
//! * [`toxicity`] — Toxicity detection: VPIN, Kyle's Lambda, and adverse
//!   selection rate from a rolling window of fills.
//!
//! * [`rebates`] — Dynamic rebate engine: quality scoring and rebate
//!   calculation based on observed SLA metrics.
//!
//! * [`pnl_attribution`] — PnL attribution: decompose a market maker's PnL
//!   into spread capture, inventory, rebates, and adverse selection cost.

pub mod pnl_attribution;
pub mod rebates;
pub mod sla;
pub mod toxicity;

pub use pnl_attribution::{AttributionFill, MmPnlAttribution, MmPnlAttributor};
pub use rebates::{MmQualityScore, RebateConfig, RebateEngine};
pub use sla::{
    DemotionAction, MmSlaConfig, MmSlaMonitor, MmSlaReport, MmSnapshot, SlaError,
};
pub use toxicity::{MmFillRecord, ToxicityAnalyzer, ToxicityReport, UserToxicityScorer};
