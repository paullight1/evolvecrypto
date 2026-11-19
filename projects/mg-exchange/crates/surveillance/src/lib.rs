//! Exchange surveillance — real-time market manipulation detection.
//!
//! # Modules
//! - [`spoofing`] — spoofing / layering detection (cancel-ratio analysis)
//! - [`wash_trading`] — wash-trading detection (volume balance + circular rings)
//! - [`quote_stuffing`] — quote-stuffing detection (order-rate limiting)
//! - [`alert`] — unified alert types and severity model
//! - [`engine`] — orchestrator that fans events out to all detectors

pub mod alert;
pub mod engine;
pub mod quote_stuffing;
pub mod spoofing;
pub mod wash_trading;

pub use alert::{
    AlertSeverity, AlertType, AutoAction, QuoteStuffingAlert, SpoofingAlert, SurveillanceAlert,
    WashTradingAlert,
};
pub use engine::SurveillanceEngine;
pub use quote_stuffing::QuoteStuffingDetector;
pub use spoofing::SpoofingDetector;
pub use wash_trading::WashTradingDetector;
