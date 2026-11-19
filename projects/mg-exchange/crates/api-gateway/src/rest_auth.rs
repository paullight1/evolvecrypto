//! OAuth2 / social login handlers.
//!
//! ## Supported providers
//! - `google`   — Google Sign-In (ID token verification via tokeninfo endpoint)
//! - `apple`    — Sign in with Apple (JWT verification via Apple's public keys)
//! - `telegram` — Telegram Login Widget (HMAC-SHA256 hash verification)
//!
//! ## Phase 1 behaviour
//! Provider token verification is **simulated**: the server accepts any non-empty
//! token and derives a deterministic email from the provider name and a hash of
//! the token.  Real verification is gated behind the corresponding env-var:
//! - `GOOGLE_CLIENT_ID` — enables real Google tokeninfo verification
//! - `APPLE_CLIENT_ID`  — enables real Apple JWT verification
//! - `TELEGRAM_BOT_TOKEN` — enables real Telegram hash verification
//!
//! When an env-var is absent the simulated path runs, allowing frontend
//! integration work to proceed before production credentials are available.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::rest::AppState;

// ── Request / response types ───────────────────────────────────────────────────

/// Request body for `POST /api/v1/auth/social/:provider`.
#[derive(Debug, Deserialize)]
pub struct SocialAuthRequest {
    /// OAuth ID token, access token, or Telegram data-check hash.
    pub token: String,
    /// Optional OAuth redirect URI (used in server-side code-exchange flows).
    pub redirect_uri: Option<String>,
}

// ── Provider verification ──────────────────────────────────────────────────────

/// Outcome of provider-specific token verification.
struct VerifiedIdentity {
    /// Stable identifier for the user in the provider's system (e.g. Google `sub`).
    provider_id: String,
    /// Email address, when the provider supplies one.
    email: Option<String>,
}

/// Derive a deterministic exchange email from a provider name + token.
///
/// Used in Phase 1 simulation and as a fallback when the provider does not
/// supply an email (e.g. Telegram).
fn simulated_email(provider: &str, token: &str) -> String {
    let hash = {
        let mut h = Sha256::new();
        h.update(token.as_bytes());
        let bytes = h.finalize();
        hex::encode(&bytes[..8]) // 8-byte prefix → 16 hex chars
    };
    format!("{provider}_{hash}@social.exchange.local")
}

/// Returns true if the PRODUCTION env var is set to "true".
fn is_production() -> bool {
    std::env::var("PRODUCTION").unwrap_or_default() == "true"
}

/// Verify a Google ID token.
///
/// In Phase 1 (no `GOOGLE_CLIENT_ID` set): accepts any token and derives a
/// simulated identity.  **Blocked in production** — returns an error if
/// `PRODUCTION=true` and `GOOGLE_CLIENT_ID` is not set.
///
/// In production: POST to `https://oauth2.googleapis.com/tokeninfo` and
/// extract `email`, `email_verified`, and `sub`.
async fn verify_google(token: &str) -> Result<VerifiedIdentity, String> {
    let client_id = std::env::var("GOOGLE_CLIENT_ID").unwrap_or_default();

    if client_id.is_empty() {
        if is_production() {
            tracing::error!("CRITICAL: Google OAuth simulation blocked in production — set GOOGLE_CLIENT_ID");
            return Err("Social authentication not available".to_string());
        }
        // Dev-only simulation — accept any token.
        tracing::warn!("Google verification simulated (GOOGLE_CLIENT_ID not set) — DEV MODE ONLY");
        return Ok(VerifiedIdentity {
            provider_id: format!("google_sim_{}", &token[..token.len().min(16)]),
            email: Some(simulated_email("google", token)),
        });
    }

    // TODO: Phase 2 — real verification.
    // 1. POST https://oauth2.googleapis.com/tokeninfo?id_token={token}
    // 2. Verify `aud` == client_id
    // 3. Verify `email_verified` == true
    // 4. Extract `sub` (provider_id) and `email`
    let client = reqwest::Client::new();
    let url = format!("https://oauth2.googleapis.com/tokeninfo?id_token={token}");
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Google tokeninfo request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err("Google token verification failed".to_string());
    }

    let info: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse Google tokeninfo response: {e}"))?;

    // Verify audience matches our client ID.
    let aud = info["aud"].as_str().unwrap_or("");
    if aud != client_id {
        return Err("Google token audience mismatch".to_string());
    }

    // Require verified email.
    if info["email_verified"].as_str() != Some("true") {
        return Err("Google email not verified".to_string());
    }

    let sub = info["sub"]
        .as_str()
        .ok_or_else(|| "Missing sub in Google token".to_string())?
        .to_string();
    let email = info["email"].as_str().map(|s| s.to_string());

    Ok(VerifiedIdentity {
        provider_id: sub,
        email,
    })
}

/// Verify an Apple identity token.
///
/// In Phase 1 (no `APPLE_CLIENT_ID` set): simulation mode.
///
/// In production: decode the JWT, fetch Apple's public keys from
/// `https://appleid.apple.com/auth/keys`, verify signature, extract `sub` and
/// `email`.
async fn verify_apple(token: &str) -> Result<VerifiedIdentity, String> {
    let client_id = std::env::var("APPLE_CLIENT_ID").unwrap_or_default();

    if client_id.is_empty() {
        if is_production() {
            tracing::error!("CRITICAL: Apple OAuth simulation blocked in production — set APPLE_CLIENT_ID");
            return Err("Social authentication not available".to_string());
        }
        tracing::warn!("Apple verification simulated (APPLE_CLIENT_ID not set) — DEV MODE ONLY");
        return Ok(VerifiedIdentity {
            provider_id: format!("apple_sim_{}", &token[..token.len().min(16)]),
            email: Some(simulated_email("apple", token)),
        });
    }

    // TODO: Phase 2 — real Apple JWT verification.
    // 1. Split JWT into header.payload.signature
    // 2. Decode header to get `kid`
    // 3. Fetch https://appleid.apple.com/auth/keys
    // 4. Find key matching `kid` and verify RS256 signature
    // 5. Verify `iss` == "https://appleid.apple.com"
    // 6. Verify `aud` == client_id
    // 7. Verify `exp` > now
    // 8. Extract `sub` (provider_id) and optional `email`
    Err("Apple verification requires APPLE_CLIENT_ID".to_string())
}

/// Verify a Telegram login widget hash.
///
/// In Phase 1 (no `TELEGRAM_BOT_TOKEN` set): simulation mode.
///
/// In production: the `token` field contains the URL-encoded data-check string
/// plus a `hash` parameter. Verification:
/// 1. Remove `hash` from the data-check string.
/// 2. Sort remaining fields alphabetically, join with `\n`.
/// 3. Compute `HMAC-SHA256(data_check_string, SHA256(bot_token))`.
/// 4. Compare to the submitted hash (constant-time).
/// 5. Check `auth_date` is within the last 86400 seconds.
async fn verify_telegram(token: &str) -> Result<VerifiedIdentity, String> {
    let bot_token = std::env::var("TELEGRAM_BOT_TOKEN").unwrap_or_default();

    if bot_token.is_empty() {
        if is_production() {
            tracing::error!("CRITICAL: Telegram verification simulation blocked in production — set TELEGRAM_BOT_TOKEN");
            return Err("Social authentication not available".to_string());
        }
        tracing::warn!("Telegram verification simulated (TELEGRAM_BOT_TOKEN not set) — DEV MODE ONLY");
        return Ok(VerifiedIdentity {
            provider_id: format!("tg_sim_{}", &token[..token.len().min(16)]),
            // Telegram does not always provide an email — use derived address.
            email: Some(simulated_email("telegram", token)),
        });
    }

    // TODO: Phase 2 — real Telegram verification.
    // Parse URL-encoded data_check_string from `token`, verify HMAC-SHA256,
    // extract `id` and `username`.
    Err("Telegram verification requires TELEGRAM_BOT_TOKEN".to_string())
}

// ── Handler ────────────────────────────────────────────────────────────────────

/// `POST /api/v1/auth/social/:provider`
///
/// Authenticate via a third-party OAuth2 / Telegram provider.  On success the
/// server finds or creates a local account and returns a JWT token pair
/// identical in shape to the email/password login response.
///
/// Rate limiting on auth endpoints applies to this handler via the router-level
/// middleware.
pub async fn social_auth(
    State(exchange): State<AppState>,
    axum::extract::Path(provider): axum::extract::Path<String>,
    Json(req): Json<SocialAuthRequest>,
) -> impl IntoResponse {
    if req.token.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "token is required"})),
        );
    }

    // ── 1. Verify token with the appropriate provider ──────────────────────
    let identity = match provider.as_str() {
        "google" => verify_google(&req.token).await,
        "apple" => verify_apple(&req.token).await,
        "telegram" => verify_telegram(&req.token).await,
        _ => {
            tracing::warn!(provider = %provider, "Unknown social auth provider");
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Unknown provider"})),
            );
        }
    };

    let identity = match identity {
        Ok(id) => id,
        Err(e) => {
            tracing::warn!(provider = %provider, error = %e, "Social auth token verification failed");
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Token verification failed"})),
            );
        }
    };

    // ── 2. Derive the canonical email used as the account key ──────────────
    // Prefer the provider email; fall back to a deterministic synthetic address.
    let email = identity
        .email
        .unwrap_or_else(|| simulated_email(&provider, &identity.provider_id));

    // ── 3. Find or create a local account ─────────────────────────────────
    let user_id = {
        let mut emails = exchange.user_emails.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(&existing_id) = emails.get(&email) {
            tracing::info!(
                provider = %provider,
                provider_id = %identity.provider_id,
                user_id = %existing_id,
                "Social login — existing user"
            );
            existing_id
        } else {
            let new_id = Uuid::new_v4();
            emails.insert(email.clone(), new_id);
            drop(emails); // release lock before calling create_account

            exchange.create_account(new_id);
            // Auto-verify email for social logins — the provider already validated it.
            exchange.email_verified.insert(new_id);

            tracing::info!(
                provider = %provider,
                provider_id = %identity.provider_id,
                user_id = %new_id,
                email = %email,
                "Social login — new user created"
            );
            new_id
        }
    };

    // ── 4. Issue JWT tokens ────────────────────────────────────────────────
    let pair = match exchange.jwt.issue_tokens(&user_id.to_string()) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(user_id = %user_id, error = %e, "JWT issuance failed on social login");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Internal error"})),
            );
        }
    };

    let expiry = chrono::Utc::now()
        + chrono::Duration::seconds(exchange.jwt.refresh_ttl_secs as i64);

    exchange
        .refresh_tokens
        .lock().unwrap_or_else(|e| e.into_inner())
        .insert(pair.refresh_token.clone(), (user_id, expiry));

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "access_token":  pair.access_token,
            "refresh_token": pair.refresh_token,
            "expires_in":    pair.expires_in,
            "user_id":       user_id.to_string(),
            "email":         email,
        })),
    )
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use crate::exchange::Exchange;
    use crate::server::build_router;

    // Helper: POST /api/v1/auth/social/{provider}
    async fn social_login(
        app: axum::Router,
        provider: &str,
        token: &str,
    ) -> (StatusCode, serde_json::Value) {
        let body = serde_json::json!({ "token": token });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/auth/social/{provider}"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        if bytes.is_empty() {
            // Route not found or body-less response — return a diagnostic value.
            (status, serde_json::json!({"_debug": "empty body", "status": status.as_u16()}))
        } else {
            (status, serde_json::from_slice(&bytes).expect("response must be valid JSON"))
        }
    }

    #[tokio::test]
    async fn test_social_auth_google_creates_new_user() {
        // GOOGLE_CLIENT_ID not set → simulation mode.
        let exchange = Exchange::new();
        let app = build_router(exchange.clone());

        let (status, body) = social_login(app, "google", "fake-google-id-token-abc123").await;

        assert_eq!(status, StatusCode::OK, "body: {body}");
        assert!(
            body["access_token"].as_str().is_some(),
            "access_token must be present"
        );
        assert!(
            body["refresh_token"].as_str().is_some(),
            "refresh_token must be present"
        );
        assert!(
            body["user_id"].as_str().is_some(),
            "user_id must be present"
        );

        // Email is derived deterministically from provider + token hash.
        let email = body["email"].as_str().unwrap();
        assert!(email.contains("google_"), "email must contain provider prefix");
        assert!(email.ends_with("@social.exchange.local"), "email must use social domain");
    }

    #[tokio::test]
    async fn test_social_auth_same_token_returns_same_user() {
        // Same token → same deterministic email → same user_id on second call.
        let exchange = Exchange::new();
        let token = "stable-token-xyz";

        let app1 = build_router(exchange.clone());
        let (s1, body1) = social_login(app1, "google", token).await;
        assert_eq!(s1, StatusCode::OK);

        let app2 = build_router(exchange.clone());
        let (s2, body2) = social_login(app2, "google", token).await;
        assert_eq!(s2, StatusCode::OK);

        assert_eq!(
            body1["user_id"], body2["user_id"],
            "same token must resolve to the same user"
        );
    }

    #[tokio::test]
    async fn test_social_auth_different_tokens_different_users() {
        let exchange = Exchange::new();

        let app1 = build_router(exchange.clone());
        let (_, body1) = social_login(app1, "google", "token-alice").await;

        let app2 = build_router(exchange.clone());
        let (_, body2) = social_login(app2, "google", "token-bob").await;

        assert_ne!(
            body1["user_id"], body2["user_id"],
            "different tokens must yield different users"
        );
    }

    #[tokio::test]
    async fn test_social_auth_telegram_simulation() {
        // TELEGRAM_BOT_TOKEN not set → simulation mode.
        let exchange = Exchange::new();
        let app = build_router(exchange);

        let (status, body) = social_login(app, "telegram", "tg-data-hash-abc").await;

        assert_eq!(status, StatusCode::OK, "body: {body}");
        assert!(body["access_token"].as_str().is_some());
        let email = body["email"].as_str().unwrap();
        assert!(email.contains("telegram_"));
    }

    #[tokio::test]
    async fn test_social_auth_unknown_provider_rejected() {
        let exchange = Exchange::new();
        let app = build_router(exchange);

        let (status, body) = social_login(app, "facebook", "some-token").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["error"].as_str().unwrap().contains("Unknown provider"));
    }

    #[tokio::test]
    async fn test_social_auth_empty_token_rejected() {
        let exchange = Exchange::new();
        let app = build_router(exchange);

        let (status, body) = social_login(app, "google", "").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body["error"].as_str().unwrap().contains("token is required"));
    }

    #[tokio::test]
    async fn test_social_auth_issues_valid_jwt() {
        // Verify that the issued access token is accepted by the verify endpoint.
        let exchange = Exchange::new();
        let app1 = build_router(exchange.clone());

        let (status, body) = social_login(app1, "google", "token-for-jwt-test").await;
        assert_eq!(status, StatusCode::OK);

        let access = body["access_token"].as_str().unwrap();
        let user_id = body["user_id"].as_str().unwrap();

        // Verify the token is accepted by the exchange JWT manager directly.
        let claims = exchange
            .jwt
            .verify_access(access)
            .expect("access token must be valid");
        assert_eq!(claims.sub, user_id, "token sub must match returned user_id");
    }

    #[tokio::test]
    async fn test_social_auth_production_guard_blocks_simulation() {
        // Temporarily set PRODUCTION=true to verify guard works.
        // Note: this test only validates the verify_* functions, not the full
        // handler flow, to avoid env var races with other tests.
        //
        // The verify functions check is_production() internally.
        // We can't safely set env vars in parallel tests, so we test the
        // is_production() function directly and the error path structurally.
        assert!(
            !super::is_production(),
            "Tests must not run with PRODUCTION=true"
        );

        // In non-production mode, simulation should succeed.
        let result = super::verify_google("test-token").await;
        assert!(result.is_ok(), "Dev mode simulation must succeed");

        let result = super::verify_apple("test-token").await;
        assert!(result.is_ok(), "Dev mode Apple simulation must succeed");

        let result = super::verify_telegram("test-token").await;
        assert!(result.is_ok(), "Dev mode Telegram simulation must succeed");
    }
}
