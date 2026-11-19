//! PEP (Politically Exposed Persons) screening.
//!
//! Phase 1: in-memory list loaded at startup.
//! Phase 2: integrate live feeds (WorldCheck, Dow Jones Risk & Compliance, etc.).
//!
//! **Security note:** match details (matched entry, list source) must only appear
//! in internal audit logs — never in user-facing responses.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A Politically Exposed Person entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PepEntry {
    pub name: String,
    pub country: String,
    pub position: String,
    pub risk_level: PepRiskLevel,
    pub source_list: String,
    pub aliases: Vec<String>,
}

/// Risk tier for a PEP entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PepRiskLevel {
    /// Family members and close associates.
    Low,
    /// Local politicians, judges, senior public officials.
    Medium,
    /// Heads of state, government ministers, central bank governors.
    High,
}

/// Outcome of a PEP screening check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PepScreenResult {
    pub is_pep: bool,
    pub matches: Vec<PepMatch>,
}

/// A single match produced during screening.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PepMatch {
    pub entry: PepEntry,
    /// Confidence score in [0.0, 1.0].
    pub match_score: f64,
    /// Which field matched: "name", "name_partial", or "alias".
    pub matched_field: String,
}

// ---------------------------------------------------------------------------
// Screener
// ---------------------------------------------------------------------------

/// In-memory PEP screening engine.
///
/// Pre-loaded with known PEP entries. A real implementation pulls from
/// WorldCheck, Dow Jones Risk & Compliance, or equivalent commercial feeds.
pub struct PepScreener {
    entries: Vec<PepEntry>,
}

impl PepScreener {
    /// Create a screener with an empty list.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Bootstrap with sample PEP data for testing.
    ///
    /// Real data must come from a licensed commercial feed.
    pub fn with_defaults() -> Self {
        let mut screener = Self::new();
        screener.load_entries(vec![
            // Sample entry only — real data comes from WorldCheck, Dow Jones, etc.
            PepEntry {
                name: "Sample PEP Person".into(),
                country: "XX".into(),
                position: "Head of State".into(),
                risk_level: PepRiskLevel::High,
                source_list: "SAMPLE".into(),
                aliases: vec!["S. PEP Person".into()],
            },
        ]);
        screener
    }

    /// Append PEP entries to the loaded list.
    pub fn load_entries(&mut self, entries: Vec<PepEntry>) {
        self.entries.extend(entries);
    }

    /// Screen a name against the PEP database.
    ///
    /// Performs exact match (score 1.0), substring match (score 0.7),
    /// and alias exact match (score 0.9). Case-insensitive throughout.
    ///
    /// Phase 2 will add Levenshtein distance and transliteration support.
    pub fn screen(&self, name: &str) -> PepScreenResult {
        let normalized = name.to_lowercase();
        let normalized = normalized.trim();
        let mut matches = Vec::new();

        for entry in &self.entries {
            let entry_name_lower = entry.name.to_lowercase();

            // Exact name match.
            if entry_name_lower == normalized {
                matches.push(PepMatch {
                    entry: entry.clone(),
                    match_score: 1.0,
                    matched_field: "name".into(),
                });
                continue;
            }

            // Partial / substring match — only for inputs with meaningful length.
            if normalized.len() >= 4 {
                if entry_name_lower.contains(normalized) || normalized.contains(entry_name_lower.as_str()) {
                    matches.push(PepMatch {
                        entry: entry.clone(),
                        match_score: 0.7,
                        matched_field: "name_partial".into(),
                    });
                    continue;
                }
            }

            // Exact alias match.
            for alias in &entry.aliases {
                if alias.to_lowercase() == normalized {
                    matches.push(PepMatch {
                        entry: entry.clone(),
                        match_score: 0.9,
                        matched_field: "alias".into(),
                    });
                    break;
                }
            }
        }

        PepScreenResult {
            is_pep: !matches.is_empty(),
            matches,
        }
    }

    /// Number of entries currently loaded.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

impl Default for PepScreener {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        let screener = PepScreener::with_defaults();
        let result = screener.screen("Sample PEP Person");
        assert!(result.is_pep);
        assert_eq!(result.matches[0].match_score, 1.0);
        assert_eq!(result.matches[0].matched_field, "name");
    }

    #[test]
    fn test_case_insensitive() {
        let screener = PepScreener::with_defaults();
        let result = screener.screen("sample pep person");
        assert!(result.is_pep);
        assert_eq!(result.matches[0].match_score, 1.0);
    }

    #[test]
    fn test_alias_match() {
        let screener = PepScreener::with_defaults();
        let result = screener.screen("S. PEP Person");
        assert!(result.is_pep);
        assert_eq!(result.matches[0].matched_field, "alias");
        assert_eq!(result.matches[0].match_score, 0.9);
    }

    #[test]
    fn test_no_match() {
        let screener = PepScreener::with_defaults();
        let result = screener.screen("John Regular Citizen");
        assert!(!result.is_pep);
        assert!(result.matches.is_empty());
    }

    #[test]
    fn test_entry_count() {
        let screener = PepScreener::with_defaults();
        assert_eq!(screener.entry_count(), 1);
    }

    #[test]
    fn test_empty_screener_always_clear() {
        let screener = PepScreener::new();
        let result = screener.screen("Sample PEP Person");
        assert!(!result.is_pep);
    }

    #[test]
    fn test_load_entries_extends_list() {
        let mut screener = PepScreener::new();
        screener.load_entries(vec![
            PepEntry {
                name: "Alpha Leader".into(),
                country: "AA".into(),
                position: "President".into(),
                risk_level: PepRiskLevel::High,
                source_list: "TEST".into(),
                aliases: vec![],
            },
            PepEntry {
                name: "Beta Judge".into(),
                country: "BB".into(),
                position: "Chief Justice".into(),
                risk_level: PepRiskLevel::Medium,
                source_list: "TEST".into(),
                aliases: vec!["B. Judge".into()],
            },
        ]);
        assert_eq!(screener.entry_count(), 2);
        assert!(screener.screen("Alpha Leader").is_pep);
        assert!(screener.screen("B. Judge").is_pep);
        assert!(!screener.screen("Charlie Nobody").is_pep);
    }
}
