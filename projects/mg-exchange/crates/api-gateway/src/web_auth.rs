//! Email/password authentication and JWT session management.
//!
//! ## Overview
//! - [`PasswordHasher`] wraps `argon2id` for secure password storage.
//! - [`JwtManager`] issues short-lived access tokens (15 min) and long-lived
//!   refresh tokens (30 days).  Tokens carry a `token_type` claim so they
//!   cannot be swapped for one another.
//! - [`WebAuthError`] is the error type for all operations in this module.

use argon2::password_hash::{PasswordHash, PasswordHasher as _, PasswordVerifier, SaltString};
use rand_core::OsRng;
use argon2::Argon2;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Error ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebAuthError {
    /// Argon2 hashing or verification failed.
    HashingFailed(String),
    /// JWT signing failed.
    TokenSignFailed(String),
    /// Token is invalid, expired, or has the wrong type.
    TokenInvalid(String),
    /// Caller presented a refresh token where an access token was expected, or vice-versa.
    WrongTokenType,
}

impl std::fmt::Display for WebAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WebAuthError::HashingFailed(e) => write!(f, "Password hashing failed: {e}"),
            WebAuthError::TokenSignFailed(e) => write!(f, "Token signing failed: {e}"),
            WebAuthError::TokenInvalid(e) => write!(f, "Token invalid: {e}"),
            WebAuthError::WrongTokenType => write!(f, "Wrong token type"),
        }
    }
}

// ── PasswordHasher ────────────────────────────────────────────────────────────

/// Argon2id password hasher.
///
/// A zero-size type — all methods are associated functions that rely on the
/// `argon2` crate defaults (Argon2id, 19 MiB memory, 2 iterations, 1 thread).
pub struct PasswordHasher;

impl PasswordHasher {
    /// Hash a plaintext password with a random salt.
    ///
    /// Returns the PHC-encoded hash string suitable for storage.
    pub fn hash(password: &str) -> Result<String, WebAuthError> {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        argon2
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| WebAuthError::HashingFailed(e.to_string()))
    }

    /// Verify a plaintext password against a stored PHC-encoded hash.
    pub fn verify(password: &str, hash: &str) -> Result<bool, WebAuthError> {
        let parsed = PasswordHash::new(hash)
            .map_err(|e| WebAuthError::HashingFailed(e.to_string()))?;
        match Argon2::default().verify_password(password.as_bytes(), &parsed) {
            Ok(()) => Ok(true),
            Err(argon2::password_hash::Error::Password) => Ok(false),
            Err(e) => Err(WebAuthError::HashingFailed(e.to_string())),
        }
    }
}

// ── JWT types ─────────────────────────────────────────────────────────────────

/// JWT claims embedded in both access and refresh tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Subject — the user's UUID as a string.
    pub sub: String,
    /// Unix timestamp (seconds) at which the token expires.
    pub exp: u64,
    /// Unix timestamp (seconds) at which the token was issued.
    pub iat: u64,
    /// `"access"` or `"refresh"`.
    pub token_type: String,
    /// JWT ID — a random UUID that makes every token unique even when issued
    /// within the same second (prevents token collision on rapid refresh).
    pub jti: String,
}

/// A freshly issued token pair returned on login or refresh.
#[derive(Debug, Clone)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    /// Seconds until the access token expires.
    pub expires_in: u64,
}

// ── JwtManager ────────────────────────────────────────────────────────────────

/// Signs and verifies JWTs for the web auth flow.
pub struct JwtManager {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    pub access_ttl_secs: u64,
    pub refresh_ttl_secs: u64,
}

impl JwtManager {
    /// Construct a `JwtManager` from an arbitrary secret string.
    ///
    /// Typical usage: read `JWT_SECRET` from the environment.
    pub fn new(secret: &str) -> Self {
        Self {
            encoding_key: EncodingKey::from_secret(secret.as_bytes()),
            decoding_key: DecodingKey::from_secret(secret.as_bytes()),
            access_ttl_secs: 900,         // 15 minutes
            refresh_ttl_secs: 30 * 24 * 3600, // 30 days
        }
    }

    /// Issue a fresh access + refresh token pair for the given user.
    pub fn issue_tokens(&self, user_id: &str) -> Result<TokenPair, WebAuthError> {
        let access_token = self.sign_token(user_id, "access", self.access_ttl_secs)?;
        let refresh_token = self.sign_token(user_id, "refresh", self.refresh_ttl_secs)?;
        Ok(TokenPair {
            access_token,
            refresh_token,
            expires_in: self.access_ttl_secs,
        })
    }

    /// Verify an access token and return its claims.
    ///
    /// Returns `Err(WebAuthError::WrongTokenType)` if the token is a refresh token.
    pub fn verify_access(&self, token: &str) -> Result<Claims, WebAuthError> {
        let claims = self.decode(token)?;
        if claims.token_type != "access" {
            return Err(WebAuthError::WrongTokenType);
        }
        Ok(claims)
    }

    /// Verify a refresh token and return its claims.
    ///
    /// Returns `Err(WebAuthError::WrongTokenType)` if the token is an access token.
    pub fn verify_refresh(&self, token: &str) -> Result<Claims, WebAuthError> {
        let claims = self.decode(token)?;
        if claims.token_type != "refresh" {
            return Err(WebAuthError::WrongTokenType);
        }
        Ok(claims)
    }

    // ── internals ─────────────────────────────────────────────────────────────

    fn sign_token(&self, user_id: &str, token_type: &str, ttl_secs: u64) -> Result<String, WebAuthError> {
        let now = chrono::Utc::now().timestamp() as u64;
        let claims = Claims {
            sub: user_id.to_string(),
            exp: now + ttl_secs,
            iat: now,
            token_type: token_type.to_string(),
            jti: Uuid::new_v4().to_string(),
        };
        encode(&Header::default(), &claims, &self.encoding_key)
            .map_err(|e| WebAuthError::TokenSignFailed(e.to_string()))
    }

    fn decode(&self, token: &str) -> Result<Claims, WebAuthError> {
        let mut validation = Validation::default();
        validation.validate_exp = true;
        decode::<Claims>(token, &self.decoding_key, &validation)
            .map(|td| td.claims)
            .map_err(|e| WebAuthError::TokenInvalid(e.to_string()))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_password_hash_and_verify_correct() {
        let hash = PasswordHasher::hash("StrongPass123!").unwrap();
        assert!(PasswordHasher::verify("StrongPass123!", &hash).unwrap());
    }

    #[test]
    fn test_password_verify_wrong_password() {
        let hash = PasswordHasher::hash("correct_password").unwrap();
        assert!(!PasswordHasher::verify("wrong_password", &hash).unwrap());
    }

    #[test]
    fn test_jwt_access_token_roundtrip() {
        let mgr = JwtManager::new("test_secret_key_for_jwt");
        let pair = mgr.issue_tokens("user-123").unwrap();
        let claims = mgr.verify_access(&pair.access_token).unwrap();
        assert_eq!(claims.sub, "user-123");
        assert_eq!(claims.token_type, "access");
    }

    #[test]
    fn test_jwt_refresh_token_roundtrip() {
        let mgr = JwtManager::new("test_secret_key_for_jwt");
        let pair = mgr.issue_tokens("user-456").unwrap();
        let claims = mgr.verify_refresh(&pair.refresh_token).unwrap();
        assert_eq!(claims.sub, "user-456");
        assert_eq!(claims.token_type, "refresh");
    }

    #[test]
    fn test_access_token_rejected_as_refresh() {
        let mgr = JwtManager::new("test_secret_key_for_jwt");
        let pair = mgr.issue_tokens("user-abc").unwrap();
        assert_eq!(
            mgr.verify_refresh(&pair.access_token).unwrap_err(),
            WebAuthError::WrongTokenType
        );
    }

    #[test]
    fn test_refresh_token_rejected_as_access() {
        let mgr = JwtManager::new("test_secret_key_for_jwt");
        let pair = mgr.issue_tokens("user-abc").unwrap();
        assert_eq!(
            mgr.verify_access(&pair.refresh_token).unwrap_err(),
            WebAuthError::WrongTokenType
        );
    }

    #[test]
    fn test_invalid_token_rejected() {
        let mgr = JwtManager::new("test_secret_key_for_jwt");
        assert!(matches!(
            mgr.verify_access("not.a.real.token"),
            Err(WebAuthError::TokenInvalid(_))
        ));
    }
}
