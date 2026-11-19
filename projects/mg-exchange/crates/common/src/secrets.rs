//! Secrets management abstraction.
//!
//! Phase 1: `EnvSecretProvider` reads from environment variables (current behaviour).
//! Phase 2: `VaultSecretProvider` or `KmsSecretProvider` for HashiCorp Vault,
//! AWS Secrets Manager, or GCP Secret Manager — keys never leave the managed store.
//!
//! The trait is intentionally synchronous because secrets are read at startup
//! (not in the hot path) and most backends support blocking reads.

use std::collections::HashMap;

/// Secret names used across the exchange.
///
/// Centralised here so that secret providers and startup validation can
/// reference them without string duplication.
pub mod names {
    pub const JWT_SECRET: &str = "JWT_SECRET";
    pub const WEBHOOK_SECRET: &str = "WEBHOOK_SECRET";
    pub const API_SECRET_ENCRYPTION_KEY: &str = "API_SECRET_ENCRYPTION_KEY";
    pub const METRICS_API_KEY: &str = "METRICS_API_KEY";
    pub const CUSTODY_HMAC_SECRET: &str = "CUSTODY_HMAC_SECRET";
    pub const SUMSUB_WEBHOOK_SECRET: &str = "SUMSUB_WEBHOOK_SECRET";
    pub const SUMSUB_SECRET_KEY: &str = "SUMSUB_SECRET_KEY";
    pub const CHAINALYSIS_API_KEY: &str = "CHAINALYSIS_API_KEY";
    pub const CAPTCHA_SECRET_KEY: &str = "CAPTCHA_SECRET_KEY";
    pub const SMTP_PASSWORD: &str = "SMTP_PASSWORD";
}

/// A provider of secret values.
///
/// Implementations may read from environment variables, a local config file,
/// HashiCorp Vault, AWS Secrets Manager, GCP Secret Manager, or an HSM.
///
/// All methods are synchronous and infallible on the "not found" path — a
/// missing secret returns `None`, not an error. Startup code is responsible
/// for panicking on required-but-missing secrets.
pub trait SecretProvider: Send + Sync {
    /// Retrieve a secret by name. Returns `None` if not configured.
    fn get(&self, name: &str) -> Option<String>;

    /// Retrieve a secret, returning a default if not configured.
    fn get_or(&self, name: &str, default: &str) -> String {
        self.get(name).unwrap_or_else(|| default.to_string())
    }

    /// Retrieve a required secret. Panics with a descriptive message if missing.
    fn require(&self, name: &str) -> String {
        self.get(name)
            .unwrap_or_else(|| panic!("FATAL: required secret '{name}' not found in secret provider"))
    }
}

/// Phase 1 implementation: reads secrets from environment variables.
///
/// This is the current behaviour, now behind the `SecretProvider` trait so
/// it can be swapped for Vault/KMS without changing call sites.
#[derive(Debug, Default)]
pub struct EnvSecretProvider;

impl SecretProvider for EnvSecretProvider {
    fn get(&self, name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|s| !s.is_empty())
    }
}

/// Test / mock implementation: returns pre-configured values.
#[derive(Debug, Default)]
pub struct StaticSecretProvider {
    secrets: HashMap<String, String>,
}

impl StaticSecretProvider {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, name: &str, value: &str) -> Self {
        self.secrets.insert(name.to_string(), value.to_string());
        self
    }
}

impl SecretProvider for StaticSecretProvider {
    fn get(&self, name: &str) -> Option<String> {
        self.secrets.get(name).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_provider_reads_from_env() {
        // SAFETY: single-threaded test; no other thread reads this variable.
        unsafe { std::env::set_var("__TEST_SECRET_XYZ", "hello"); }
        let p = EnvSecretProvider;
        assert_eq!(p.get("__TEST_SECRET_XYZ"), Some("hello".to_string()));
        unsafe { std::env::remove_var("__TEST_SECRET_XYZ"); }
        assert_eq!(p.get("__TEST_SECRET_XYZ"), None);
    }

    #[test]
    fn env_provider_skips_empty_values() {
        unsafe { std::env::set_var("__TEST_SECRET_EMPTY", ""); }
        let p = EnvSecretProvider;
        assert_eq!(p.get("__TEST_SECRET_EMPTY"), None);
        unsafe { std::env::remove_var("__TEST_SECRET_EMPTY"); }
    }

    #[test]
    fn static_provider_returns_configured_values() {
        let p = StaticSecretProvider::new()
            .with("JWT_SECRET", "my-jwt")
            .with("DB_PASS", "hunter2");
        assert_eq!(p.get("JWT_SECRET"), Some("my-jwt".to_string()));
        assert_eq!(p.get("DB_PASS"), Some("hunter2".to_string()));
        assert_eq!(p.get("MISSING"), None);
    }

    #[test]
    fn get_or_returns_default_when_missing() {
        let p = StaticSecretProvider::new();
        assert_eq!(p.get_or("NOPE", "fallback"), "fallback");
    }

    #[test]
    #[should_panic(expected = "required secret 'MISSING' not found")]
    fn require_panics_when_missing() {
        let p = StaticSecretProvider::new();
        p.require("MISSING");
    }
}
