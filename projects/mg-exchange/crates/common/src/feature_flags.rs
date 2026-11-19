use std::collections::HashMap;
use std::sync::RwLock;

/// Known feature flag names.
pub const FLAG_FUTURES: &str = "ENABLE_FUTURES";
pub const FLAG_WITHDRAWALS: &str = "ENABLE_WITHDRAWALS";
pub const FLAG_COPY_TRADING: &str = "ENABLE_COPY_TRADING";
pub const FLAG_TRADING_BOTS: &str = "ENABLE_TRADING_BOTS";
pub const FLAG_REGISTRATION: &str = "ENABLE_REGISTRATION";
pub const FLAG_DEPOSITS: &str = "ENABLE_DEPOSITS";

/// All known flags, used to populate defaults.
const ALL_FLAGS: &[&str] = &[
    FLAG_FUTURES,
    FLAG_WITHDRAWALS,
    FLAG_COPY_TRADING,
    FLAG_TRADING_BOTS,
    FLAG_REGISTRATION,
    FLAG_DEPOSITS,
];

/// Runtime feature flag system.
///
/// Flags can be loaded from environment variables or set at runtime via the
/// admin API. Each flag defaults to `true` (enabled) unless explicitly disabled.
///
/// The inner `RwLock` allows concurrent reads with exclusive writes, which is
/// the right trade-off for flags that are read on every request but toggled
/// rarely.
pub struct FeatureFlags {
    flags: RwLock<HashMap<String, bool>>,
}

impl FeatureFlags {
    /// Create flags with all known flags enabled.
    pub fn new() -> Self {
        let flags = ALL_FLAGS
            .iter()
            .map(|&name| (name.to_string(), true))
            .collect();
        Self {
            flags: RwLock::new(flags),
        }
    }

    /// Load flags from environment variables.
    ///
    /// Each `ENABLE_*` env var is read; a missing variable defaults to `true`.
    /// The value `"0"`, `"false"`, or `"no"` (case-insensitive) disables the flag;
    /// any other value (including `"1"`, `"true"`, `"yes"`) enables it.
    pub fn from_env() -> Self {
        let flags = ALL_FLAGS
            .iter()
            .map(|&name| {
                let enabled = std::env::var(name)
                    .map(|v| !matches!(v.to_lowercase().as_str(), "0" | "false" | "no"))
                    .unwrap_or(true);
                (name.to_string(), enabled)
            })
            .collect();
        Self {
            flags: RwLock::new(flags),
        }
    }

    /// Conservative defaults for a soft launch.
    ///
    /// Disables riskier features (futures, copy trading, trading bots) while
    /// keeping deposits, withdrawals, and registration active.
    pub fn launch_safe() -> Self {
        let instance = Self::new();
        instance.set(FLAG_FUTURES, false);
        instance.set(FLAG_COPY_TRADING, false);
        instance.set(FLAG_TRADING_BOTS, false);
        instance
    }

    /// Return `true` if the named flag is enabled.
    ///
    /// Unknown flags (not in `ALL_FLAGS`) are considered disabled.
    pub fn is_enabled(&self, flag: &str) -> bool {
        self.flags
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(flag)
            .copied()
            .unwrap_or(false)
    }

    /// Set a flag at runtime.
    pub fn set(&self, flag: &str, enabled: bool) {
        self.flags
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(flag.to_string(), enabled);
    }

    /// Return a snapshot of all flag states.
    pub fn all(&self) -> HashMap<String, bool> {
        self.flags
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_flag_default_enabled() {
        let flags = FeatureFlags::new();
        assert!(flags.is_enabled(FLAG_FUTURES));
        assert!(flags.is_enabled(FLAG_WITHDRAWALS));
        assert!(flags.is_enabled(FLAG_DEPOSITS));
        assert!(flags.is_enabled(FLAG_REGISTRATION));
        assert!(flags.is_enabled(FLAG_COPY_TRADING));
        assert!(flags.is_enabled(FLAG_TRADING_BOTS));
    }

    #[test]
    fn test_feature_flag_toggle() {
        let flags = FeatureFlags::new();
        assert!(flags.is_enabled(FLAG_FUTURES));

        flags.set(FLAG_FUTURES, false);
        assert!(!flags.is_enabled(FLAG_FUTURES));

        flags.set(FLAG_FUTURES, true);
        assert!(flags.is_enabled(FLAG_FUTURES));
    }

    #[test]
    fn test_launch_safe_disables_futures() {
        let flags = FeatureFlags::launch_safe();
        assert!(!flags.is_enabled(FLAG_FUTURES));
        assert!(!flags.is_enabled(FLAG_COPY_TRADING));
        assert!(!flags.is_enabled(FLAG_TRADING_BOTS));
        // Safe features remain enabled
        assert!(flags.is_enabled(FLAG_WITHDRAWALS));
        assert!(flags.is_enabled(FLAG_DEPOSITS));
        assert!(flags.is_enabled(FLAG_REGISTRATION));
    }

    #[test]
    fn test_all_returns_snapshot() {
        let flags = FeatureFlags::new();
        flags.set(FLAG_FUTURES, false);
        let snapshot = flags.all();
        assert_eq!(snapshot.get(FLAG_FUTURES), Some(&false));
        assert_eq!(snapshot.get(FLAG_WITHDRAWALS), Some(&true));
    }

    #[test]
    fn test_unknown_flag_is_disabled() {
        let flags = FeatureFlags::new();
        assert!(!flags.is_enabled("ENABLE_UNKNOWN_FEATURE"));
    }
}
