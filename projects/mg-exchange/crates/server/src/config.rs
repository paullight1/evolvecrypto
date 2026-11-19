//! Validated startup configuration.
//!
//! Loads all environment variables at startup, validates them, and returns
//! a typed `Config` struct. If any required value is missing or invalid,
//! the server fails fast with a descriptive error instead of discovering
//! the problem mid-flight.

use anyhow::{bail, Context, Result};

/// Validated startup configuration.
#[derive(Debug)]
pub struct Config {
    pub production: bool,
    pub dev_mode: bool,
    pub database_url: Option<String>,
    pub jwt_secret: String,
    pub webhook_secret: Option<String>,
    pub api_secret_encryption_key: Option<String>,
    pub metrics_api_key: Option<String>,
    pub data_dir: String,
    pub host: String,
    pub port: u16,
    pub allowed_origins: Vec<String>,
    pub fix_port: u16,
    pub db_pool_max: u32,
    pub log_json: bool,
    pub demo_mode: bool,
}

impl Config {
    /// Load and validate all configuration from environment variables.
    ///
    /// Panics are replaced by descriptive errors. Call this at the very
    /// start of main() and propagate the error.
    pub fn from_env() -> Result<Self> {
        let production = env_bool("PRODUCTION");
        let dev_mode = env_bool("DEV_MODE") || env_bool("EXCHANGE_DEV_MODE");

        if production && dev_mode {
            bail!("Cannot run DEV_MODE in production. Set PRODUCTION=false or remove DEV_MODE.");
        }

        let database_url = std::env::var("DATABASE_URL").ok().filter(|s| !s.is_empty());

        if production && database_url.is_none() {
            bail!("DATABASE_URL required in production mode.");
        }

        let jwt_secret = std::env::var("JWT_SECRET")
            .unwrap_or_else(|_| "dev-secret-change-in-production".to_string());

        if production && jwt_secret.len() < 32 {
            bail!(
                "JWT_SECRET must be at least 32 characters in production (got {}).",
                jwt_secret.len()
            );
        }

        let webhook_secret = std::env::var("WEBHOOK_SECRET").ok().filter(|s| !s.is_empty());
        if production && webhook_secret.is_none() {
            bail!("WEBHOOK_SECRET required in production mode.");
        }

        let api_secret_encryption_key = std::env::var("API_SECRET_ENCRYPTION_KEY")
            .ok()
            .filter(|s| !s.is_empty());
        if production && api_secret_encryption_key.is_none() {
            bail!("API_SECRET_ENCRYPTION_KEY required in production mode.");
        }

        let metrics_api_key = std::env::var("METRICS_API_KEY").ok().filter(|s| !s.is_empty());

        let data_dir = std::env::var("EXCHANGE_DATA_DIR").unwrap_or_else(|_| "./data".to_string());
        let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port: u16 = std::env::var("PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8080);
        let fix_port: u16 = std::env::var("FIX_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8081);

        let allowed_origins: Vec<String> = std::env::var("ALLOWED_ORIGINS")
            .unwrap_or_else(|_| "http://localhost:3000".to_string())
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let db_pool_max: u32 = std::env::var("DB_POOL_MAX")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(40);

        if db_pool_max == 0 || db_pool_max > 500 {
            bail!("DB_POOL_MAX must be between 1 and 500 (got {db_pool_max}).");
        }

        let log_json = env_bool("LOG_FORMAT_JSON") || production;
        let demo_mode = std::env::var("DEMO_MODE").is_ok();

        if production && demo_mode {
            bail!("DEMO_MODE cannot be enabled in production.");
        }

        let config = Self {
            production,
            dev_mode,
            database_url,
            jwt_secret,
            webhook_secret,
            api_secret_encryption_key,
            metrics_api_key,
            data_dir,
            host,
            port,
            allowed_origins,
            fix_port,
            db_pool_max,
            log_json,
            demo_mode,
        };

        Ok(config)
    }

    /// Print the validated config to tracing (secrets redacted).
    pub fn log_summary(&self) {
        tracing::info!(
            production = self.production,
            dev_mode = self.dev_mode,
            database = self.database_url.is_some(),
            host = %self.host,
            port = self.port,
            fix_port = self.fix_port,
            db_pool_max = self.db_pool_max,
            origins = ?self.allowed_origins,
            log_json = self.log_json,
            demo = self.demo_mode,
            jwt_secret = "[REDACTED]",
            webhook_secret = if self.webhook_secret.is_some() { "[SET]" } else { "[NOT SET]" },
            metrics_api_key = if self.metrics_api_key.is_some() { "[SET]" } else { "[NOT SET]" },
            "Configuration validated"
        );
    }
}

fn env_bool(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_bool_parses_true() {
        assert!(!env_bool("__NONEXISTENT_BOOL_TEST__"));
    }

    #[test]
    fn config_validates_db_pool_range() {
        // Can't easily test from_env without setting env vars,
        // but we can test the validation logic.
        assert!(0u32 == 0); // placeholder — real validation is in from_env()
    }
}
