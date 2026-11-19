//! Volume tier gaming detection.
//!
//! Detects when a user's 30-day volume approaches a VIP tier boundary with
//! suspicious patterns (circular trading, rapid cancel-replace, same-size orders).
//!
//! # Reference
//! - Fee tier gaming: wash trading to reach rebate tiers (maker rebate farming).
//! - B3 in threat catalogue: Maker Rebate Farming.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;

use exchange_common::types::UserId;

use crate::alert::{AlertSeverity, AlertType, AutoAction, SurveillanceAlert, VolumeTierGamingAlert};

/// VIP tier boundary definition.
#[derive(Debug, Clone)]
pub struct TierBoundary {
    pub name: String,
    pub volume_usd: Decimal,
}

/// Check if a user's volume is suspiciously close to a tier boundary.
///
/// Returns an alert if:
/// 1. Volume is within `proximity_pct` of a tier boundary (default 5%).
/// 2. The user has suspicious volume patterns (flagged externally).
///
/// `user_volume_30d` — user's 30-day trading volume in USD.
/// `tier_boundaries` — sorted list of VIP tier volume thresholds.
/// `has_suspicious_patterns` — true if wash-trading or circular patterns detected.
pub fn check_tier_gaming(
    user_id: UserId,
    user_volume_30d: Decimal,
    tier_boundaries: &[TierBoundary],
    has_suspicious_patterns: bool,
    proximity_pct: Decimal,
) -> Option<SurveillanceAlert> {
    if !has_suspicious_patterns {
        return None;
    }

    for boundary in tier_boundaries {
        if boundary.volume_usd.is_zero() {
            continue;
        }

        // Check if user is within proximity_pct of this boundary.
        let distance = boundary.volume_usd - user_volume_30d;
        if distance <= Decimal::ZERO {
            continue; // Already past this tier.
        }

        let proximity = Decimal::ONE - (distance / boundary.volume_usd);
        let threshold = Decimal::ONE - proximity_pct;

        if proximity >= threshold {
            let now = chrono::Utc::now();
            return Some(SurveillanceAlert {
                id: Uuid::new_v4(),
                alert_type: AlertType::VolumeTierGaming(VolumeTierGamingAlert {
                    user_id,
                    volume_30d: user_volume_30d,
                    tier_boundary: boundary.volume_usd,
                    proximity_pct: proximity,
                    detected_at: now,
                }),
                severity: AlertSeverity::Medium,
                user_ids: vec![user_id],
                symbol: None,
                description: format!(
                    "Volume tier gaming: user at {:.1}% of {} tier (${:.0}), suspicious patterns detected",
                    proximity * dec!(100),
                    boundary.name,
                    boundary.volume_usd,
                ),
                auto_action: AutoAction::ReduceRateLimit,
                created_at: now,
                reviewed: false,
            });
        }
    }

    None
}

/// Default VIP tier boundaries (Bybit-style).
pub fn default_tier_boundaries() -> Vec<TierBoundary> {
    vec![
        TierBoundary { name: "VIP1".into(), volume_usd: dec!(1_000_000) },
        TierBoundary { name: "VIP2".into(), volume_usd: dec!(5_000_000) },
        TierBoundary { name: "VIP3".into(), volume_usd: dec!(10_000_000) },
        TierBoundary { name: "VIP4".into(), volume_usd: dec!(25_000_000) },
        TierBoundary { name: "VIP5".into(), volume_usd: dec!(50_000_000) },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> UserId { Uuid::new_v4() }

    #[test]
    fn no_alert_without_suspicious_patterns() {
        let alert = check_tier_gaming(
            user(),
            dec!(990_000), // 99% of VIP1
            &default_tier_boundaries(),
            false, // not suspicious
            dec!(0.05),
        );
        assert!(alert.is_none());
    }

    #[test]
    fn alert_when_close_to_tier_with_suspicious_patterns() {
        let alert = check_tier_gaming(
            user(),
            dec!(960_000), // 96% of VIP1 ($1M)
            &default_tier_boundaries(),
            true,
            dec!(0.05), // 5% proximity
        );
        assert!(alert.is_some());
        let a = alert.unwrap();
        assert!(matches!(a.alert_type, AlertType::VolumeTierGaming(_)));
    }

    #[test]
    fn no_alert_when_far_from_tier() {
        let alert = check_tier_gaming(
            user(),
            dec!(500_000), // 50% of VIP1
            &default_tier_boundaries(),
            true,
            dec!(0.05),
        );
        assert!(alert.is_none());
    }

    #[test]
    fn already_past_tier_no_alert() {
        let alert = check_tier_gaming(
            user(),
            dec!(1_500_000), // past VIP1, 30% of VIP2
            &default_tier_boundaries(),
            true,
            dec!(0.05),
        );
        // Not close enough to VIP2 ($5M) — should not alert.
        assert!(alert.is_none());
    }
}
