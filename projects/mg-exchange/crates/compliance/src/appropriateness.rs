//! MiCA Article 71(2) Appropriateness Assessment.
//!
//! Before offering complex or leveraged products to non-professional clients,
//! the CASP must assess whether the product is appropriate for the client
//! based on their knowledge and experience.
//!
//! The assessment is a questionnaire. Each answer carries a score. A total
//! score >= `PASS_THRESHOLD` gates access to: futures, margin, leveraged tokens,
//! options. Users below the threshold can still trade spot.
//!
//! Reference: MiCA Regulation (EU) 2023/1114, Article 71(2); ESMA draft RTS
//! on appropriateness assessment for CASPs (2024 consultation).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Minimum score required to access complex/leveraged products.
pub const PASS_THRESHOLD: u32 = 60;

/// A single question in the appropriateness questionnaire.
#[derive(Debug, Clone, Serialize)]
pub struct Question {
    pub id: &'static str,
    pub text: &'static str,
    pub options: Vec<(&'static str, u32)>, // (label, score)
}

/// The fixed questionnaire. Based on ESMA's MiFID II suitability guidance
/// adapted for crypto CASPs under MiCA.
pub fn questionnaire() -> Vec<Question> {
    vec![
        Question {
            id: "experience_years",
            text: "How many years of experience do you have trading financial instruments (stocks, futures, crypto)?",
            options: vec![
                ("None", 0),
                ("Less than 1 year", 10),
                ("1-3 years", 20),
                ("More than 3 years", 30),
            ],
        },
        Question {
            id: "leverage_understanding",
            text: "Which statement best describes leverage in trading?",
            options: vec![
                ("I don't know what leverage is", 0),
                ("Leverage increases potential profit", 5),
                ("Leverage amplifies both profits and losses, and I may lose more than my deposit", 20),
            ],
        },
        Question {
            id: "loss_tolerance",
            text: "How much of your investment capital can you afford to lose without impacting your financial stability?",
            options: vec![
                ("None — I cannot afford any losses", 0),
                ("Up to 10%", 5),
                ("Up to 50%", 10),
                ("Up to 100% — I only invest what I can afford to lose", 15),
            ],
        },
        Question {
            id: "liquidation_understanding",
            text: "What happens when a leveraged position is liquidated?",
            options: vec![
                ("I don't know", 0),
                ("The exchange closes my position and I lose my margin deposit", 20),
            ],
        },
        Question {
            id: "funding_rate_understanding",
            text: "In a perpetual futures contract, what is a funding rate?",
            options: vec![
                ("I don't know", 0),
                ("A periodic payment exchanged between long and short holders to keep the contract price anchored to the spot price", 15),
            ],
        },
    ]
}

/// A user's submitted assessment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssessmentResult {
    pub user_id: Uuid,
    /// answer_id → selected option index
    pub answers: HashMap<String, usize>,
    pub score: u32,
    pub passed: bool,
    pub submitted_at: DateTime<Utc>,
}

/// In-memory store of assessment results.
///
/// Phase 1: HashMap. Phase 2: Postgres-backed (same pattern as KYC).
pub struct AppropriatenessManager {
    results: HashMap<Uuid, AssessmentResult>,
}

impl AppropriatenessManager {
    pub fn new() -> Self {
        Self {
            results: HashMap::new(),
        }
    }

    /// Submit an assessment. `answers` maps question_id → selected option index.
    ///
    /// Returns the computed score and whether the user passed.
    pub fn submit(&mut self, user_id: Uuid, answers: HashMap<String, usize>) -> AssessmentResult {
        let questions = questionnaire();
        let mut score = 0u32;

        for q in &questions {
            if let Some(&idx) = answers.get(q.id) {
                if let Some(&(_, pts)) = q.options.get(idx) {
                    score += pts;
                }
            }
        }

        let passed = score >= PASS_THRESHOLD;

        let result = AssessmentResult {
            user_id,
            answers,
            score,
            passed,
            submitted_at: Utc::now(),
        };

        if passed {
            tracing::info!(user_id = %user_id, score, "Appropriateness assessment PASSED");
        } else {
            tracing::info!(user_id = %user_id, score, threshold = PASS_THRESHOLD, "Appropriateness assessment FAILED — leverage access denied");
        }

        self.results.insert(user_id, result.clone());
        result
    }

    /// Check whether a user has passed the assessment.
    ///
    /// Returns `false` if the user has never submitted or failed.
    pub fn has_passed(&self, user_id: &Uuid) -> bool {
        self.results.get(user_id).map(|r| r.passed).unwrap_or(false)
    }

    /// Get the most recent assessment result for a user.
    pub fn get(&self, user_id: &Uuid) -> Option<&AssessmentResult> {
        self.results.get(user_id)
    }

    /// Number of assessments stored.
    pub fn count(&self) -> usize {
        self.results.len()
    }
}

impl Default for AppropriatenessManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn questionnaire_has_5_questions() {
        assert_eq!(questionnaire().len(), 5);
    }

    #[test]
    fn max_possible_score() {
        let qs = questionnaire();
        let max: u32 = qs.iter().map(|q| q.options.iter().map(|(_, s)| *s).max().unwrap_or(0)).sum();
        assert_eq!(max, 100, "max score should be 100");
    }

    #[test]
    fn experienced_user_passes() {
        let mut mgr = AppropriatenessManager::new();
        let user = Uuid::new_v4();

        let answers = HashMap::from([
            ("experience_years".to_string(), 3usize),      // 30
            ("leverage_understanding".to_string(), 2usize), // 20
            ("loss_tolerance".to_string(), 3usize),         // 15
            ("liquidation_understanding".to_string(), 1usize), // 20
            ("funding_rate_understanding".to_string(), 1usize), // 15
        ]);

        let result = mgr.submit(user, answers);
        assert_eq!(result.score, 100);
        assert!(result.passed);
        assert!(mgr.has_passed(&user));
    }

    #[test]
    fn inexperienced_user_fails() {
        let mut mgr = AppropriatenessManager::new();
        let user = Uuid::new_v4();

        let answers = HashMap::from([
            ("experience_years".to_string(), 0usize),      // 0
            ("leverage_understanding".to_string(), 0usize), // 0
            ("loss_tolerance".to_string(), 0usize),         // 0
            ("liquidation_understanding".to_string(), 0usize), // 0
            ("funding_rate_understanding".to_string(), 0usize), // 0
        ]);

        let result = mgr.submit(user, answers);
        assert_eq!(result.score, 0);
        assert!(!result.passed);
        assert!(!mgr.has_passed(&user));
    }

    #[test]
    fn borderline_user_at_threshold() {
        let mut mgr = AppropriatenessManager::new();
        let user = Uuid::new_v4();

        // Score exactly 60: experience_years=3 (30) + leverage=2 (20) + loss=1 (5) + liquidation=0 (0) + funding=0 (0) = 55 → not enough
        // Adjust: experience_years=3 (30) + leverage=2 (20) + loss=2 (10) + liquidation=0 (0) + funding=0 (0) = 60
        let answers = HashMap::from([
            ("experience_years".to_string(), 3usize),      // 30
            ("leverage_understanding".to_string(), 2usize), // 20
            ("loss_tolerance".to_string(), 2usize),         // 10
            ("liquidation_understanding".to_string(), 0usize), // 0
            ("funding_rate_understanding".to_string(), 0usize), // 0
        ]);

        let result = mgr.submit(user, answers);
        assert_eq!(result.score, 60);
        assert!(result.passed, "score exactly at threshold should pass");
    }

    #[test]
    fn unknown_user_has_not_passed() {
        let mgr = AppropriatenessManager::new();
        assert!(!mgr.has_passed(&Uuid::new_v4()));
    }

    #[test]
    fn resubmit_overwrites_previous() {
        let mut mgr = AppropriatenessManager::new();
        let user = Uuid::new_v4();

        // First: fail
        mgr.submit(user, HashMap::new());
        assert!(!mgr.has_passed(&user));

        // Second: pass with all correct
        let answers = HashMap::from([
            ("experience_years".to_string(), 3usize),
            ("leverage_understanding".to_string(), 2usize),
            ("loss_tolerance".to_string(), 3usize),
            ("liquidation_understanding".to_string(), 1usize),
            ("funding_rate_understanding".to_string(), 1usize),
        ]);
        mgr.submit(user, answers);
        assert!(mgr.has_passed(&user));
    }
}
