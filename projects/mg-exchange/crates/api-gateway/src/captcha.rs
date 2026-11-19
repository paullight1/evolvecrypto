//! CAPTCHA verification for login and registration endpoints.
//!
//! Supports hCaptcha and Cloudflare Turnstile (both use the same siteverify API shape).
//!
//! ## Configuration (environment variables)
//! - `CAPTCHA_SECRET_KEY` — secret key from hCaptcha or Turnstile dashboard.
//!   When absent, verification is skipped (dev/test mode).
//! - `CAPTCHA_VERIFY_URL` — override the default hCaptcha endpoint.
//!   Defaults to `https://api.hcaptcha.com/siteverify`.
//!   Set to `https://challenges.cloudflare.com/turnstile/v0/siteverify` for Turnstile.

use anyhow::{anyhow, Result};
use serde::Deserialize;

// ── Response shape ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct CaptchaResponse {
    success: bool,
    #[serde(rename = "error-codes")]
    error_codes: Option<Vec<String>>,
}

// ── Verifier ───────────────────────────────────────────────────────────────────

/// Verifies CAPTCHA tokens against the hCaptcha or Turnstile siteverify API.
///
/// When `CAPTCHA_SECRET_KEY` is not set the verifier is disabled and every
/// call to [`verify`] returns `Ok(true)` — appropriate for local development
/// where no widget is rendered.
pub struct CaptchaVerifier {
    secret_key: String,
    verify_url: String,
    enabled: bool,
}

impl CaptchaVerifier {
    /// Construct from environment variables.
    ///
    /// - `CAPTCHA_SECRET_KEY` — must be set for CAPTCHA to be enforced.
    /// - `CAPTCHA_VERIFY_URL` — optional; defaults to hCaptcha siteverify.
    pub fn from_env() -> Self {
        let secret = std::env::var("CAPTCHA_SECRET_KEY").unwrap_or_default();
        let enabled = !secret.is_empty();
        if enabled {
            tracing::info!("CAPTCHA verification enabled");
        } else {
            tracing::warn!("CAPTCHA_SECRET_KEY not set — CAPTCHA verification disabled (dev mode)");
        }
        Self {
            secret_key: secret,
            verify_url: std::env::var("CAPTCHA_VERIFY_URL")
                .unwrap_or_else(|_| "https://api.hcaptcha.com/siteverify".to_string()),
            enabled,
        }
    }

    /// Build a verifier explicitly — useful in tests.
    pub fn new(secret_key: impl Into<String>, verify_url: impl Into<String>) -> Self {
        let secret_key = secret_key.into();
        let enabled = !secret_key.is_empty();
        Self {
            secret_key,
            verify_url: verify_url.into(),
            enabled,
        }
    }

    /// Whether CAPTCHA enforcement is active (secret key is configured).
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Verify a CAPTCHA token submitted by the client.
    ///
    /// Returns `Ok(true)` when the token is valid (or CAPTCHA is disabled).
    /// Returns `Ok(false)` when the token is rejected by the provider.
    /// Returns `Err` only on network/deserialization failures.
    ///
    /// The `remote_ip` is optional; when provided it is forwarded to the
    /// provider for additional abuse detection.
    pub async fn verify(&self, token: &str, remote_ip: Option<&str>) -> Result<bool> {
        if !self.enabled {
            return Ok(true);
        }

        // An empty token is an immediate failure — no round-trip needed.
        if token.is_empty() {
            return Ok(false);
        }

        let client = reqwest::Client::new();
        let mut params = vec![
            ("secret", self.secret_key.as_str()),
            ("response", token),
        ];
        // remote_ip binding needs to outlive the params vec borrow
        let ip_val;
        if let Some(ip) = remote_ip {
            ip_val = ip.to_string();
            params.push(("remoteip", ip_val.as_str()));
        }

        let resp = client
            .post(&self.verify_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| anyhow!("CAPTCHA HTTP request failed: {e}"))?;

        let captcha_resp: CaptchaResponse = resp
            .json()
            .await
            .map_err(|e| anyhow!("Failed to deserialize CAPTCHA response: {e}"))?;

        if !captcha_resp.success {
            if let Some(codes) = &captcha_resp.error_codes {
                tracing::warn!(error_codes = ?codes, "CAPTCHA verification failed");
            }
        }

        Ok(captcha_resp.success)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_when_no_secret() {
        // Temporarily clear the env var for this test.
        // This is safe because tests run in-process and we construct from explicit args.
        let verifier = CaptchaVerifier::new("", "https://api.hcaptcha.com/siteverify");
        assert!(!verifier.is_enabled());
    }

    #[test]
    fn enabled_when_secret_set() {
        let verifier = CaptchaVerifier::new("test-secret-key", "https://api.hcaptcha.com/siteverify");
        assert!(verifier.is_enabled());
    }

    #[tokio::test]
    async fn disabled_verifier_always_passes() {
        let verifier = CaptchaVerifier::new("", "https://api.hcaptcha.com/siteverify");
        // Any token (even empty) passes when disabled.
        assert!(verifier.verify("", None).await.unwrap());
        assert!(verifier.verify("any-token", None).await.unwrap());
    }

    #[tokio::test]
    async fn enabled_verifier_rejects_empty_token() {
        let verifier = CaptchaVerifier::new("secret", "https://api.hcaptcha.com/siteverify");
        // Empty token is rejected immediately without a network call.
        let result = verifier.verify("", None).await.unwrap();
        assert!(!result, "empty token must be rejected");
    }
}
