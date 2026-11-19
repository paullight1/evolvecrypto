//! Sanctions screening — checks names and crypto addresses against known lists.
//!
//! Phase 1: in-memory lists loaded at startup (simplified OFAC SDN simulation).
//! Phase 2: integrate live OFAC SDN XML feed + Chainalysis/Elliptic address API.
//!
//! **Security note:** `SanctionsResult::Match` deliberately omits the specific list
//! name in user-facing responses — revealing which list matched would help bad actors
//! understand the screening coverage. Only internal logs should contain that detail.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Bulk-load entry type
// ---------------------------------------------------------------------------

/// A single sanctioned entity with an optional list of associated addresses.
///
/// Used by the admin bulk-load endpoint (POST /admin/sanctions/load).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SanctionsEntry {
    /// Full legal name of the sanctioned person or entity.
    pub name: String,
    /// Associated crypto addresses (may be empty).
    #[serde(default)]
    pub addresses: Vec<String>,
}

// ---------------------------------------------------------------------------
// Supported list identifiers
// ---------------------------------------------------------------------------

/// Canonical identifiers for sanctions lists supported by this screener.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SanctionsList {
    OfacSdn,
    OfacCrypto,
    EuConsolidated,
    UnSecurityCouncil,
    UkHmTreasury,
    /// Ukraine NSDC (National Security and Defence Council).
    UaNsdc,
}

/// Metadata about a loaded sanctions list.
#[derive(Debug, Clone, Serialize)]
pub struct ListMetadata {
    pub list: SanctionsList,
    pub entry_count: usize,
    pub last_updated: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Result type
// ---------------------------------------------------------------------------

/// Outcome of a sanctions screening check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SanctionsResult {
    /// No match found — the name or address is not on any loaded list.
    Clear,
    /// A match was found. `list` identifies the source; `matched_entry` is the
    /// exact entry that matched (used for internal audit logging only — do not
    /// expose to the user).
    Match {
        /// Name of the list that produced the match (e.g. "OFAC_SDN").
        list: String,
        /// The exact string from the list that matched the input.
        matched_entry: String,
    },
}

// ---------------------------------------------------------------------------
// Screener
// ---------------------------------------------------------------------------

/// In-memory sanctions screener.
///
/// Pre-loaded with known sanctioned names (exact, case-insensitive) and
/// sanctioned crypto addresses (exact, case-insensitive). A real implementation
/// would pull from the OFAC SDN XML feed and on-chain address clustering APIs.
pub struct SanctionsScreener {
    /// Normalised (lowercase) sanctioned names from OFAC SDN and equivalent lists.
    names: HashSet<String>,
    /// Normalised (lowercase) sanctioned crypto addresses.
    addresses: HashSet<String>,
    /// Rolling count of positive matches (name or address) since process start.
    /// Used by the regulatory reporter to populate the sanctions_matches field.
    match_count: std::sync::atomic::AtomicU64,
}

impl SanctionsScreener {
    /// Create a screener with empty lists.
    ///
    /// Call [`load_ofac_names`] and [`load_sanctioned_addresses`] to populate
    /// before production use.
    pub fn new() -> Self {
        Self {
            names: HashSet::new(),
            addresses: HashSet::new(),
            match_count: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Bulk-load sanctioned names (e.g. from OFAC SDN XML feed).
    ///
    /// Names are normalised to lowercase for case-insensitive matching.
    pub fn load_ofac_names(&mut self, names: Vec<String>) {
        for name in names {
            self.names.insert(name.to_lowercase());
        }
    }

    /// Bulk-load sanctioned crypto addresses.
    ///
    /// Addresses are normalised to lowercase to handle mixed-case hex.
    pub fn load_sanctioned_addresses(&mut self, addresses: Vec<String>) {
        for addr in addresses {
            self.addresses.insert(addr.to_lowercase());
        }
    }

    /// Screen a name against loaded sanctions lists.
    ///
    /// Matching is exact after case-normalisation. A production implementation
    /// would also apply fuzzy / transliteration matching.
    pub fn screen_name(&self, name: &str) -> SanctionsResult {
        let normalised = name.to_lowercase();
        if let Some(entry) = self.names.get(&normalised) {
            self.match_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return SanctionsResult::Match {
                list: "OFAC_SDN".to_string(),
                matched_entry: entry.clone(),
            };
        }
        SanctionsResult::Clear
    }

    /// Screen a crypto address against loaded sanctions lists.
    ///
    /// Matching is exact after case-normalisation (handles both `0xABC` and `0xabc`).
    pub fn screen_address(&self, address: &str) -> SanctionsResult {
        let normalised = address.to_lowercase();
        if let Some(entry) = self.addresses.get(&normalised) {
            self.match_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return SanctionsResult::Match {
                list: "OFAC_CRYPTO".to_string(),
                matched_entry: entry.clone(),
            };
        }
        SanctionsResult::Clear
    }

    /// Total number of positive sanctions matches since process start.
    pub fn match_count(&self) -> u64 {
        self.match_count.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Return `true` when no names and no addresses have been loaded.
    ///
    /// A production deployment should always have at least one entry — call this
    /// at startup and log a prominent warning if `true` is returned.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty() && self.addresses.is_empty()
    }

    /// Number of sanctioned names currently loaded.
    pub fn name_count(&self) -> usize {
        self.names.len()
    }

    /// Number of sanctioned addresses currently loaded.
    pub fn address_count(&self) -> usize {
        self.addresses.len()
    }

    /// Create a screener pre-loaded with a minimal set of well-known OFAC SDN
    /// entries for immediate use.  These are illustrative/representative entries
    /// from publicly available OFAC data.
    ///
    /// A production deployment must supplement this with the full live feed from
    /// `https://sanctionslist.ofac.treas.gov/Home/SdnList` or a compliance API
    /// such as Chainalysis or Elliptic.
    pub fn with_defaults() -> Self {
        let mut screener = Self::new();

        // Representative OFAC SDN names (publicly available).
        // Source: OFAC Specially Designated Nationals list (illustrative subset).
        screener.load_ofac_names(vec![
            "Lazarus Group".to_string(),
            "BlueNorOff".to_string(),
            "Tornado Cash".to_string(),
            "SUEX OTC".to_string(),
            "Garantex".to_string(),
            "Chatex".to_string(),
            "BITZLATO".to_string(),
        ]);

        // Representative sanctioned crypto addresses from OFAC and equivalents.
        // These are real entries that appeared in public OFAC actions.
        screener.load_sanctioned_addresses(vec![
            // Tornado Cash router (ETH) — OFAC action Aug 2022
            "0x7f268357a8c2552623316e2562d90e642bb538e5".to_string(),
            // Lazarus Group BTC deposit address (OFAC 2018 — illustrative)
            "149w62rY42aZBox8fGcmqNsXUzSStKeq8C".to_string(),
            // Guaranteed — OFAC Apr 2022
            "0x3ad9db589d201a710ed237c829c7a00d9a58ec7b".to_string(),
        ]);

        screener
    }

    /// Bulk-load entries from an admin-supplied list (name + optional addresses).
    ///
    /// Designed for the POST /admin/sanctions/load endpoint.  Each entry has a
    /// `name` and zero-or-more `addresses`.
    pub fn bulk_load(&mut self, entries: Vec<SanctionsEntry>) {
        let names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
        let addresses: Vec<String> = entries
            .iter()
            .flat_map(|e| e.addresses.iter().cloned())
            .collect();
        self.load_ofac_names(names);
        self.load_sanctioned_addresses(addresses);
    }

    /// Return metadata about each loaded list.
    ///
    /// Phase 1: returns a single synthetic record reflecting the in-memory state.
    /// Phase 2: each live feed will have its own metadata entry with a real timestamp.
    pub fn list_metadata(&self) -> Vec<ListMetadata> {
        vec![
            ListMetadata {
                list: SanctionsList::OfacSdn,
                entry_count: self.names.len(),
                last_updated: Utc::now(),
            },
            ListMetadata {
                list: SanctionsList::OfacCrypto,
                entry_count: self.addresses.len(),
                last_updated: Utc::now(),
            },
        ]
    }

    /// Screen both a name and a set of addresses, returning only the hits.
    ///
    /// Matches against names with `screen_name` and each address with
    /// `screen_address`. Only `SanctionsResult::Match` variants are included
    /// in the returned list.
    pub fn full_screen(&self, name: &str, addresses: &[String]) -> Vec<SanctionsResult> {
        let mut results = Vec::new();

        let name_result = self.screen_name(name);
        if matches!(name_result, SanctionsResult::Match { .. }) {
            results.push(name_result);
        }

        for addr in addresses {
            let addr_result = self.screen_address(addr);
            if matches!(addr_result, SanctionsResult::Match { .. }) {
                results.push(addr_result);
            }
        }

        results
    }
}

impl Default for SanctionsScreener {
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

    fn screener_with_data() -> SanctionsScreener {
        let mut s = SanctionsScreener::new();
        s.load_ofac_names(vec![
            "Ivan Badguy".to_string(),
            "Evil Corp LLC".to_string(),
        ]);
        s.load_sanctioned_addresses(vec![
            "0xDeadBeef1234567890deadbeef1234567890ABCD".to_string(),
        ]);
        s
    }

    #[test]
    fn test_sanctions_clear_name() {
        let s = screener_with_data();
        assert_eq!(s.screen_name("Alice Smith"), SanctionsResult::Clear);
    }

    #[test]
    fn test_sanctions_match_name_exact() {
        let s = screener_with_data();
        let result = s.screen_name("Ivan Badguy");
        assert!(matches!(result, SanctionsResult::Match { list, .. } if list == "OFAC_SDN"));
    }

    #[test]
    fn test_sanctions_match_name_case_insensitive() {
        let s = screener_with_data();
        // Screening is case-insensitive.
        assert!(matches!(s.screen_name("IVAN BADGUY"), SanctionsResult::Match { .. }));
        assert!(matches!(s.screen_name("ivan badguy"), SanctionsResult::Match { .. }));
    }

    #[test]
    fn test_sanctions_clear_address() {
        let s = screener_with_data();
        assert_eq!(
            s.screen_address("0x1111111111111111111111111111111111111111"),
            SanctionsResult::Clear
        );
    }

    #[test]
    fn test_sanctions_match_address() {
        let s = screener_with_data();
        let result = s.screen_address("0xDeadBeef1234567890deadbeef1234567890ABCD");
        assert!(matches!(result, SanctionsResult::Match { list, .. } if list == "OFAC_CRYPTO"));
    }

    #[test]
    fn test_sanctions_match_address_case_insensitive() {
        let s = screener_with_data();
        // Mixed-case address normalised before matching.
        let result = s.screen_address("0xdeadbeef1234567890deadbeef1234567890abcd");
        assert!(matches!(result, SanctionsResult::Match { .. }));
    }

    #[test]
    fn test_empty_screener_always_clear() {
        let s = SanctionsScreener::new();
        assert_eq!(s.screen_name("Ivan Badguy"), SanctionsResult::Clear);
        assert_eq!(
            s.screen_address("0xDeadBeef1234567890deadbeef1234567890ABCD"),
            SanctionsResult::Clear
        );
    }

    #[test]
    fn test_load_names_multiple_entries() {
        let mut s = SanctionsScreener::new();
        s.load_ofac_names(vec!["Alpha".to_string(), "Beta".to_string(), "Gamma".to_string()]);
        assert!(matches!(s.screen_name("Alpha"), SanctionsResult::Match { .. }));
        assert!(matches!(s.screen_name("Beta"), SanctionsResult::Match { .. }));
        assert!(matches!(s.screen_name("Gamma"), SanctionsResult::Match { .. }));
        assert_eq!(s.screen_name("Delta"), SanctionsResult::Clear);
    }

    #[test]
    fn test_matched_entry_preserved() {
        let mut s = SanctionsScreener::new();
        s.load_ofac_names(vec!["John Doe".to_string()]);
        if let SanctionsResult::Match { matched_entry, .. } = s.screen_name("john doe") {
            assert_eq!(matched_entry, "john doe");
        } else {
            panic!("Expected a match");
        }
    }

    #[test]
    fn test_match_count_increments_on_hits() {
        let s = screener_with_data();
        assert_eq!(s.match_count(), 0);

        // Name match → +1
        s.screen_name("Ivan Badguy");
        assert_eq!(s.match_count(), 1);

        // Address match → +1
        s.screen_address("0xDeadBeef1234567890deadbeef1234567890ABCD");
        assert_eq!(s.match_count(), 2);

        // Clear result → no increment
        s.screen_name("Legit Person");
        assert_eq!(s.match_count(), 2);
    }
}
