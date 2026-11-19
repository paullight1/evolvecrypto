use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RiskFactor {
    HighRiskJurisdiction(String),
    LargeTransactions,
    PepMatch,
    NewAccount,
    FrequentCashOuts,
    UnusualPattern(String),
    SanctionsNearMatch,
    DormantReactivation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskScore {
    pub user_id: Uuid,
    pub level: RiskLevel,
    /// Numeric score in range 0–100.
    pub score: u32,
    pub factors: Vec<RiskFactor>,
    pub last_assessed: DateTime<Utc>,
    pub next_review: DateTime<Utc>,
}

/// In-memory customer risk scoring engine.
///
/// Computes a numeric risk score from a set of [`RiskFactor`] values and maps
/// it to a [`RiskLevel`].  Higher-risk users are scheduled for more frequent
/// review.
pub struct RiskScoringEngine {
    scores: HashMap<Uuid, RiskScore>,
}

impl RiskScoringEngine {
    pub fn new() -> Self {
        Self {
            scores: HashMap::new(),
        }
    }

    /// Assess (or re-assess) a user's risk profile from the given factors.
    ///
    /// The score is the sum of per-factor weights capped at 100.  Returns a
    /// reference to the stored [`RiskScore`].
    pub fn assess(&mut self, user_id: Uuid, factors: Vec<RiskFactor>) -> &RiskScore {
        let raw_score: u32 = factors
            .iter()
            .map(|f| match f {
                RiskFactor::PepMatch => 40,
                RiskFactor::SanctionsNearMatch => 50,
                RiskFactor::HighRiskJurisdiction(_) => 30,
                RiskFactor::LargeTransactions => 15,
                RiskFactor::FrequentCashOuts => 20,
                RiskFactor::UnusualPattern(_) => 25,
                RiskFactor::NewAccount => 10,
                RiskFactor::DormantReactivation => 20,
            })
            .sum();

        let score = raw_score.min(100);

        let level = match score {
            0..=20 => RiskLevel::Low,
            21..=50 => RiskLevel::Medium,
            51..=80 => RiskLevel::High,
            _ => RiskLevel::Critical,
        };

        let review_days = match level {
            RiskLevel::Low => 90,
            RiskLevel::Medium => 30,
            RiskLevel::High => 7,
            RiskLevel::Critical => 1,
        };

        self.scores.insert(
            user_id,
            RiskScore {
                user_id,
                level,
                score,
                factors,
                last_assessed: Utc::now(),
                next_review: Utc::now() + chrono::Duration::days(review_days),
            },
        );

        // SAFETY: we just inserted the entry above.
        self.scores.get(&user_id).expect("just inserted")
    }

    pub fn get_score(&self, user_id: Uuid) -> Option<&RiskScore> {
        self.scores.get(&user_id)
    }

    /// Returns all users rated High or Critical.
    pub fn high_risk_users(&self) -> Vec<&RiskScore> {
        self.scores
            .values()
            .filter(|s| matches!(s.level, RiskLevel::High | RiskLevel::Critical))
            .collect()
    }

    /// Returns all users whose scheduled review date has passed.
    pub fn due_for_review(&self) -> Vec<&RiskScore> {
        let now = Utc::now();
        self.scores
            .values()
            .filter(|s| s.next_review <= now)
            .collect()
    }
}

impl Default for RiskScoringEngine {
    fn default() -> Self {
        Self::new()
    }
}
