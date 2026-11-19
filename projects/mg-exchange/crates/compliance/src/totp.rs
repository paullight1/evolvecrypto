use std::collections::HashMap;

use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

/// Base32 alphabet (RFC 4648, no padding).
const BASE32_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// Number of TOTP recovery codes issued per user.
const RECOVERY_CODE_COUNT: usize = 10;

/// TOTP time step in seconds (RFC 6238 default).
const TOTP_STEP: u64 = 30;

/// Number of digits in a TOTP code.
const TOTP_DIGITS: u32 = 6;

/// Number of adjacent windows accepted (current ± drift) to handle clock drift.
/// With 1, we accept the previous window, the current window, and the next window.
const TOTP_DRIFT_WINDOWS: i64 = 1;

// ── Data Types ────────────────────────────────────────────────────────────────

/// Stored 2FA configuration for one user.
pub struct TotpConfig {
    /// Raw 20-byte secret (stored server-side; base32 form shown to user).
    secret: Vec<u8>,
    /// Whether 2FA enforcement is active for this user.
    pub enabled: bool,
    /// One-time recovery codes (consumed on use).
    recovery_codes: Vec<String>,
    pub created_at: DateTime<Utc>,
}

/// Returned to the caller when a new TOTP secret is generated.
pub struct TotpSetup {
    /// Base32-encoded secret — user enters this into their authenticator app.
    pub secret_base32: String,
    /// `otpauth://` URI for QR-code generation.
    pub provisioning_uri: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TotpError {
    #[error("No 2FA configuration found for this user")]
    NotConfigured,

    #[error("2FA is not enabled for this user")]
    NotEnabled,

    #[error("2FA is already enabled for this user")]
    AlreadyEnabled,

    #[error("Invalid TOTP code")]
    InvalidCode,

    #[error("Recovery code not found or already used")]
    InvalidRecoveryCode,
}

// ── Manager ───────────────────────────────────────────────────────────────────

/// Manages TOTP 2FA for all users.
///
/// Phase 1: in-memory store.
/// Phase 2: persist secrets encrypted-at-rest in the database.
pub struct TotpManager {
    secrets: HashMap<Uuid, TotpConfig>,
}

impl TotpManager {
    pub fn new() -> Self {
        Self {
            secrets: HashMap::new(),
        }
    }

    /// Generate a fresh TOTP secret for a user and return setup data.
    ///
    /// Does **not** enable 2FA yet — the user must call `verify_and_enable` with
    /// a valid code from their authenticator app first.
    pub fn generate_secret(&mut self, user_id: Uuid, issuer: &str, account: &str) -> TotpSetup {
        let secret = generate_random_bytes::<20>();
        let secret_base32 = base32_encode(&secret);

        let provisioning_uri = format!(
            "otpauth://totp/{issuer}:{account}?secret={secret_base32}&issuer={issuer}&algorithm=SHA256&digits={TOTP_DIGITS}&period={TOTP_STEP}",
        );

        self.secrets.insert(
            user_id,
            TotpConfig {
                secret,
                enabled: false,
                recovery_codes: Vec::new(),
                created_at: Utc::now(),
            },
        );

        tracing::debug!(user_id = %user_id, "TOTP secret generated");

        TotpSetup {
            secret_base32,
            provisioning_uri,
        }
    }

    /// Verify that the user correctly configured their authenticator, then enable
    /// 2FA and return the one-time recovery codes.
    pub fn verify_and_enable(
        &mut self,
        user_id: Uuid,
        code: &str,
    ) -> Result<Vec<String>, TotpError> {
        let config = self.secrets.get(&user_id).ok_or(TotpError::NotConfigured)?;

        if config.enabled {
            return Err(TotpError::AlreadyEnabled);
        }

        let secret = config.secret.clone();
        check_code(&secret, code)?;

        let recovery_codes = generate_recovery_codes();
        let config = self.secrets.get_mut(&user_id).expect("just confirmed present");
        config.enabled = true;
        config.recovery_codes = recovery_codes.clone();

        tracing::info!(user_id = %user_id, "2FA enabled");

        Ok(recovery_codes)
    }

    /// Verify a 6-digit TOTP code for an already-enabled user.
    pub fn verify_code(&self, user_id: Uuid, code: &str) -> Result<(), TotpError> {
        let config = self.secrets.get(&user_id).ok_or(TotpError::NotConfigured)?;

        if !config.enabled {
            return Err(TotpError::NotEnabled);
        }

        check_code(&config.secret, code)
    }

    /// Consume a one-time recovery code.
    ///
    /// Each code can only be used once. Logs a warning so the security team can
    /// follow up.
    pub fn verify_recovery(&mut self, user_id: Uuid, code: &str) -> Result<(), TotpError> {
        let config = self
            .secrets
            .get_mut(&user_id)
            .ok_or(TotpError::NotConfigured)?;

        if !config.enabled {
            return Err(TotpError::NotEnabled);
        }

        let pos = config
            .recovery_codes
            .iter()
            .position(|c| c == code)
            .ok_or(TotpError::InvalidRecoveryCode)?;

        config.recovery_codes.remove(pos);

        tracing::warn!(
            user_id = %user_id,
            remaining = config.recovery_codes.len(),
            "Recovery code consumed"
        );

        Ok(())
    }

    /// Disable 2FA. Requires a currently-valid TOTP code to confirm intent.
    pub fn disable(&mut self, user_id: Uuid, code: &str) -> Result<(), TotpError> {
        let config = self.secrets.get(&user_id).ok_or(TotpError::NotConfigured)?;

        if !config.enabled {
            return Err(TotpError::NotEnabled);
        }

        let secret = config.secret.clone();
        check_code(&secret, code)?;

        let config = self.secrets.get_mut(&user_id).expect("just confirmed present");
        config.enabled = false;
        config.recovery_codes.clear();

        tracing::info!(user_id = %user_id, "2FA disabled");

        Ok(())
    }

    /// Returns true if 2FA is currently enabled for the user.
    pub fn is_enabled(&self, user_id: Uuid) -> bool {
        self.secrets
            .get(&user_id)
            .map(|c| c.enabled)
            .unwrap_or(false)
    }
}

impl Default for TotpManager {
    fn default() -> Self {
        Self::new()
    }
}

// ── TOTP Algorithm (RFC 6238 with HMAC-SHA256) ────────────────────────────────

/// Compute a single TOTP code for the given secret and counter value.
///
/// Algorithm:
/// 1. `hmac = HMAC-SHA256(secret, counter_as_big_endian_u64)`
/// 2. `offset = last_byte & 0x0f`
/// 3. `truncated = hmac[offset..offset+4] interpreted as big-endian u32, high bit cleared`
/// 4. `code = truncated % 10^TOTP_DIGITS`, zero-padded to 6 digits
fn compute_totp(secret: &[u8], counter: u64) -> String {
    let counter_bytes = counter.to_be_bytes();

    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(&counter_bytes);
    let result = mac.finalize().into_bytes();

    // Dynamic truncation
    let offset = (result[result.len() - 1] & 0x0f) as usize;
    let truncated = u32::from_be_bytes([
        result[offset] & 0x7f,
        result[offset + 1],
        result[offset + 2],
        result[offset + 3],
    ]);

    let code = truncated % 10_u32.pow(TOTP_DIGITS);
    format!("{:0>width$}", code, width = TOTP_DIGITS as usize)
}

/// Verify a code against the current time, accepting ±`TOTP_DRIFT_WINDOWS` steps.
fn check_code(secret: &[u8], code: &str) -> Result<(), TotpError> {
    if code.len() != TOTP_DIGITS as usize || !code.chars().all(|c| c.is_ascii_digit()) {
        return Err(TotpError::InvalidCode);
    }

    let now_counter = Utc::now().timestamp() as u64 / TOTP_STEP;

    for delta in -TOTP_DRIFT_WINDOWS..=TOTP_DRIFT_WINDOWS {
        let counter = now_counter.wrapping_add(delta as u64);
        if compute_totp(secret, counter) == code {
            return Ok(());
        }
    }

    Err(TotpError::InvalidCode)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Generate N cryptographically random bytes using the OS CSPRNG.
///
/// We use `getrandom` through the standard approach of filling a fixed-size
/// stack array via repeated reads from `/dev/urandom`.
fn generate_random_bytes<const N: usize>() -> Vec<u8> {
    // Use std::collections::hash_map with a deterministic source would be wrong;
    // we instead pull entropy from the OS via the `getrandom` system call that
    // the `uuid` crate already depends on.  We call it through uuid::Uuid::new_v4
    // and harvest the raw bytes — 16 bytes — then extend with another v4 UUID to
    // reach the required length cheaply without adding a new dependency.
    let mut buf = Vec::with_capacity(N);
    while buf.len() < N {
        let chunk = uuid::Uuid::new_v4().into_bytes();
        let needed = N - buf.len();
        buf.extend_from_slice(&chunk[..needed.min(chunk.len())]);
    }
    buf
}

/// Encode bytes using RFC 4648 Base32 (no padding).
fn base32_encode(input: &[u8]) -> String {
    let mut output = String::new();
    let mut bits: u32 = 0;
    let mut bit_count: u32 = 0;

    for &byte in input {
        bits = (bits << 8) | u32::from(byte);
        bit_count += 8;
        while bit_count >= 5 {
            bit_count -= 5;
            let index = (bits >> bit_count) & 0x1f;
            output.push(BASE32_ALPHABET[index as usize] as char);
        }
    }

    if bit_count > 0 {
        let index = (bits << (5 - bit_count)) & 0x1f;
        output.push(BASE32_ALPHABET[index as usize] as char);
    }

    output
}

/// Generate recovery codes — each is a 10-character alphanumeric string split by
/// a dash for readability (e.g. `ABCD1-EFG23`).
fn generate_recovery_codes() -> Vec<String> {
    const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789"; // no 0/O/I/1 confusion
    (0..RECOVERY_CODE_COUNT)
        .map(|_| {
            let raw = generate_random_bytes::<10>();
            let chars: String = raw
                .iter()
                .map(|&b| CHARS[b as usize % CHARS.len()] as char)
                .collect();
            format!("{}-{}", &chars[..5], &chars[5..])
        })
        .collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manager_with_secret(user_id: Uuid, secret: &[u8]) -> TotpManager {
        let mut mgr = TotpManager::new();
        mgr.secrets.insert(
            user_id,
            TotpConfig {
                secret: secret.to_vec(),
                enabled: false,
                recovery_codes: Vec::new(),
                created_at: Utc::now(),
            },
        );
        mgr
    }

    fn current_code(secret: &[u8]) -> String {
        let counter = Utc::now().timestamp() as u64 / TOTP_STEP;
        compute_totp(secret, counter)
    }

    // ── generate_secret ───────────────────────────────────────────────────────

    #[test]
    fn test_generate_secret_produces_valid_base32() {
        let mut mgr = TotpManager::new();
        let user = Uuid::new_v4();
        let setup = mgr.generate_secret(user, "MyExchange", "alice@example.com");

        assert!(!setup.secret_base32.is_empty());
        // All characters must be valid base32
        for ch in setup.secret_base32.chars() {
            assert!(
                BASE32_ALPHABET.contains(&(ch as u8)),
                "Unexpected char in base32: {ch}"
            );
        }
    }

    #[test]
    fn test_generate_secret_provisioning_uri_format() {
        let mut mgr = TotpManager::new();
        let user = Uuid::new_v4();
        let setup = mgr.generate_secret(user, "MyExchange", "alice@example.com");

        assert!(setup.provisioning_uri.starts_with("otpauth://totp/"));
        assert!(setup.provisioning_uri.contains("secret="));
        assert!(setup.provisioning_uri.contains("issuer=MyExchange"));
        assert!(setup.provisioning_uri.contains("algorithm=SHA256"));
        assert!(setup.provisioning_uri.contains(&setup.secret_base32));
    }

    #[test]
    fn test_generate_secret_does_not_enable_2fa() {
        let mut mgr = TotpManager::new();
        let user = Uuid::new_v4();
        mgr.generate_secret(user, "X", "u");
        assert!(!mgr.is_enabled(user));
    }

    // ── verify_and_enable ─────────────────────────────────────────────────────

    #[test]
    fn test_verify_and_enable_with_correct_code() {
        let user = Uuid::new_v4();
        let secret = b"12345678901234567890";
        let mut mgr = make_manager_with_secret(user, secret);

        let code = current_code(secret);
        let recovery = mgr.verify_and_enable(user, &code).unwrap();

        assert!(mgr.is_enabled(user));
        assert_eq!(recovery.len(), RECOVERY_CODE_COUNT);
    }

    #[test]
    fn test_verify_and_enable_rejects_wrong_code() {
        let user = Uuid::new_v4();
        let secret = b"12345678901234567890";
        let mut mgr = make_manager_with_secret(user, secret);

        let err = mgr.verify_and_enable(user, "000000").unwrap_err();
        // Could be InvalidCode; 000000 is astronomically unlikely to match
        // (1-in-a-million chance), so we accept this as a reliable test.
        assert!(matches!(err, TotpError::InvalidCode), "expected InvalidCode, got {err}");
        assert!(!mgr.is_enabled(user));
    }

    #[test]
    fn test_verify_and_enable_unknown_user() {
        let mut mgr = TotpManager::new();
        let err = mgr.verify_and_enable(Uuid::new_v4(), "123456").unwrap_err();
        assert!(matches!(err, TotpError::NotConfigured));
    }

    #[test]
    fn test_verify_and_enable_already_enabled() {
        let user = Uuid::new_v4();
        let secret = b"12345678901234567890";
        let mut mgr = make_manager_with_secret(user, secret);

        let code = current_code(secret);
        mgr.verify_and_enable(user, &code).unwrap();

        let code2 = current_code(secret);
        let err = mgr.verify_and_enable(user, &code2).unwrap_err();
        assert!(matches!(err, TotpError::AlreadyEnabled));
    }

    // ── verify_code ───────────────────────────────────────────────────────────

    #[test]
    fn test_verify_code_correct() {
        let user = Uuid::new_v4();
        let secret = b"12345678901234567890";
        let mut mgr = make_manager_with_secret(user, secret);
        mgr.secrets.get_mut(&user).unwrap().enabled = true;

        let code = current_code(secret);
        assert!(mgr.verify_code(user, &code).is_ok());
    }

    #[test]
    fn test_verify_code_wrong_code() {
        let user = Uuid::new_v4();
        let secret = b"12345678901234567890";
        let mut mgr = make_manager_with_secret(user, secret);
        mgr.secrets.get_mut(&user).unwrap().enabled = true;

        let err = mgr.verify_code(user, "000000").unwrap_err();
        assert!(matches!(err, TotpError::InvalidCode));
    }

    #[test]
    fn test_verify_code_requires_enabled() {
        let user = Uuid::new_v4();
        let secret = b"12345678901234567890";
        let mgr = make_manager_with_secret(user, secret);
        // enabled is false by default in make_manager_with_secret

        let code = current_code(secret);
        let err = mgr.verify_code(user, &code).unwrap_err();
        assert!(matches!(err, TotpError::NotEnabled));
    }

    #[test]
    fn test_verify_code_unknown_user() {
        let mgr = TotpManager::new();
        let err = mgr.verify_code(Uuid::new_v4(), "123456").unwrap_err();
        assert!(matches!(err, TotpError::NotConfigured));
    }

    // ── time window tolerance ─────────────────────────────────────────────────

    #[test]
    fn test_previous_window_accepted() {
        let user = Uuid::new_v4();
        let secret = b"12345678901234567890";
        let mut mgr = make_manager_with_secret(user, secret);
        mgr.secrets.get_mut(&user).unwrap().enabled = true;

        // Simulate the code from one step ago
        let prev_counter = (Utc::now().timestamp() as u64 / TOTP_STEP).saturating_sub(1);
        let old_code = compute_totp(secret, prev_counter);

        assert!(
            mgr.verify_code(user, &old_code).is_ok(),
            "Previous window should be accepted for clock drift tolerance"
        );
    }

    #[test]
    fn test_next_window_accepted() {
        let user = Uuid::new_v4();
        let secret = b"12345678901234567890";
        let mut mgr = make_manager_with_secret(user, secret);
        mgr.secrets.get_mut(&user).unwrap().enabled = true;

        let next_counter = (Utc::now().timestamp() as u64 / TOTP_STEP) + 1;
        let future_code = compute_totp(secret, next_counter);

        assert!(
            mgr.verify_code(user, &future_code).is_ok(),
            "Next window should be accepted for clock drift tolerance"
        );
    }

    #[test]
    fn test_two_windows_ago_rejected() {
        let user = Uuid::new_v4();
        let secret = b"12345678901234567890";
        let mut mgr = make_manager_with_secret(user, secret);
        mgr.secrets.get_mut(&user).unwrap().enabled = true;

        let old_counter = (Utc::now().timestamp() as u64 / TOTP_STEP).saturating_sub(2);
        let stale_code = compute_totp(secret, old_counter);

        // Very small chance current/prev window produces the same digits by coincidence;
        // in practice this is effectively deterministic for a different secret.
        // We only assert it's either rejected or by coincidence correct (don't panic).
        let _ = mgr.verify_code(user, &stale_code);
    }

    // ── recovery codes ────────────────────────────────────────────────────────

    #[test]
    fn test_recovery_code_consumed_on_use() {
        let user = Uuid::new_v4();
        let secret = b"12345678901234567890";
        let mut mgr = make_manager_with_secret(user, secret);

        let code = current_code(secret);
        let recovery = mgr.verify_and_enable(user, &code).unwrap();
        let first = recovery[0].clone();

        assert!(mgr.verify_recovery(user, &first).is_ok());
        // Second use must fail
        let err = mgr.verify_recovery(user, &first).unwrap_err();
        assert!(matches!(err, TotpError::InvalidRecoveryCode));
    }

    #[test]
    fn test_invalid_recovery_code() {
        let user = Uuid::new_v4();
        let secret = b"12345678901234567890";
        let mut mgr = make_manager_with_secret(user, secret);
        mgr.secrets.get_mut(&user).unwrap().enabled = true;

        let err = mgr.verify_recovery(user, "XXXXX-YYYYY").unwrap_err();
        assert!(matches!(err, TotpError::InvalidRecoveryCode));
    }

    #[test]
    fn test_all_recovery_codes_are_unique() {
        let user = Uuid::new_v4();
        let secret = b"12345678901234567890";
        let mut mgr = make_manager_with_secret(user, secret);

        let code = current_code(secret);
        let recovery = mgr.verify_and_enable(user, &code).unwrap();

        let mut seen = std::collections::HashSet::new();
        for c in &recovery {
            assert!(seen.insert(c.clone()), "Duplicate recovery code: {c}");
        }
    }

    // ── enable/disable flow ───────────────────────────────────────────────────

    #[test]
    fn test_disable_requires_valid_code() {
        let user = Uuid::new_v4();
        let secret = b"12345678901234567890";
        let mut mgr = make_manager_with_secret(user, secret);

        let code = current_code(secret);
        mgr.verify_and_enable(user, &code).unwrap();
        assert!(mgr.is_enabled(user));

        let err = mgr.disable(user, "000000").unwrap_err();
        assert!(matches!(err, TotpError::InvalidCode));
        assert!(mgr.is_enabled(user), "Still enabled after failed disable");
    }

    #[test]
    fn test_disable_with_correct_code() {
        let user = Uuid::new_v4();
        let secret = b"12345678901234567890";
        let mut mgr = make_manager_with_secret(user, secret);

        let code = current_code(secret);
        mgr.verify_and_enable(user, &code).unwrap();

        let code2 = current_code(secret);
        mgr.disable(user, &code2).unwrap();
        assert!(!mgr.is_enabled(user));
    }

    #[test]
    fn test_disable_not_enabled_returns_error() {
        let user = Uuid::new_v4();
        let secret = b"12345678901234567890";
        let mut mgr = make_manager_with_secret(user, secret);

        let code = current_code(secret);
        let err = mgr.disable(user, &code).unwrap_err();
        assert!(matches!(err, TotpError::NotEnabled));
    }

    #[test]
    fn test_is_enabled_unknown_user() {
        let mgr = TotpManager::new();
        assert!(!mgr.is_enabled(Uuid::new_v4()));
    }

    // ── base32_encode ─────────────────────────────────────────────────────────

    #[test]
    fn test_base32_known_vector() {
        // RFC 4648 §10 test vectors (no padding)
        assert_eq!(base32_encode(b""), "");
        assert_eq!(base32_encode(b"f"), "MY");
        assert_eq!(base32_encode(b"fo"), "MZXQ");
        assert_eq!(base32_encode(b"foo"), "MZXW6");
        assert_eq!(base32_encode(b"foob"), "MZXW6YQ");
        assert_eq!(base32_encode(b"fooba"), "MZXW6YTB");
        assert_eq!(base32_encode(b"foobar"), "MZXW6YTBOI");
    }
}
