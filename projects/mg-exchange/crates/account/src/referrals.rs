use std::collections::HashMap;

use chrono::{DateTime, Utc};
use exchange_common::UserId;
use rust_decimal::Decimal;
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

/// Errors that can occur during referral operations.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReferralError {
    #[error("Referral code not found: {0}")]
    InvalidCode(String),

    #[error("User {0} already has a referrer")]
    AlreadyReferred(UserId),

    #[error("User cannot refer themselves")]
    SelfReferral,
}

/// A single recorded commission payout.
#[derive(Debug, Clone)]
pub struct CommissionRecord {
    pub id: Uuid,
    pub referrer_id: UserId,
    pub trader_id: UserId,
    pub trade_id: u64,
    pub fee_amount: Decimal,
    pub commission: Decimal,
    pub asset: String,
    /// 1 = direct referral, 2 = indirect (referrer's referrer)
    pub level: u8,
    pub timestamp: DateTime<Utc>,
}

/// Manages referral codes, referral relationships, and affiliate commissions.
///
/// Commission model:
/// - Level 1: direct referrer receives `commission_rate` fraction of the fee.
/// - Level 2: the referrer's own referrer receives `level2_rate` fraction of the fee.
///
/// Phase 1: in-memory. Phase 2: PostgreSQL backend.
pub struct ReferralManager {
    /// user_id → referral code
    user_codes: HashMap<UserId, String>,
    /// referral_code → owner user_id
    code_to_user: HashMap<String, UserId>,
    /// user_id → who referred them (direct referrer)
    referred_by: HashMap<UserId, UserId>,
    /// referrer → list of users they directly referred
    referrals: HashMap<UserId, Vec<UserId>>,
    /// Accumulated commissions: user_id → asset → total amount
    commissions: HashMap<UserId, HashMap<String, Decimal>>,
    /// Full commission history in insertion order
    history: Vec<CommissionRecord>,
    /// Fraction of the trade fee paid to the direct referrer (default 0.20 = 20%).
    commission_rate: Decimal,
    /// Fraction of the trade fee paid to the indirect referrer (default 0.05 = 5%).
    level2_rate: Decimal,
    /// Salt used when deriving deterministic referral codes.
    salt: String,
}

impl ReferralManager {
    /// Create a new manager with the default 20%/5% commission rates.
    pub fn new() -> Self {
        Self::with_rates(
            Decimal::new(20, 2), // 0.20
            Decimal::new(5, 2),  // 0.05
        )
    }

    /// Create a manager with custom commission rates and a random salt.
    pub fn with_rates(commission_rate: Decimal, level2_rate: Decimal) -> Self {
        Self {
            user_codes: HashMap::new(),
            code_to_user: HashMap::new(),
            referred_by: HashMap::new(),
            referrals: HashMap::new(),
            commissions: HashMap::new(),
            history: Vec::new(),
            commission_rate,
            level2_rate,
            salt: Uuid::new_v4().to_string(),
        }
    }

    /// Create a manager with a fixed salt — useful for deterministic tests.
    pub fn with_rates_and_salt(
        commission_rate: Decimal,
        level2_rate: Decimal,
        salt: impl Into<String>,
    ) -> Self {
        Self {
            user_codes: HashMap::new(),
            code_to_user: HashMap::new(),
            referred_by: HashMap::new(),
            referrals: HashMap::new(),
            commissions: HashMap::new(),
            history: Vec::new(),
            commission_rate,
            level2_rate,
            salt: salt.into(),
        }
    }

    // -------------------------------------------------------------------------
    // Code management
    // -------------------------------------------------------------------------

    /// Return the referral code for `user_id`, generating one on first call.
    ///
    /// The code is derived deterministically from the user id and the manager's
    /// salt so the same user always gets the same code within a given manager
    /// instance. Collisions are resolved by appending a counter suffix.
    pub fn generate_code(&mut self, user_id: UserId) -> String {
        if let Some(code) = self.user_codes.get(&user_id) {
            return code.clone();
        }

        let code = self.derive_unique_code(user_id);
        self.user_codes.insert(user_id, code.clone());
        self.code_to_user.insert(code.clone(), user_id);
        code
    }

    /// Return the referral code for `user_id` without creating one.
    pub fn get_referral_code(&self, user_id: UserId) -> Option<&str> {
        self.user_codes.get(&user_id).map(String::as_str)
    }

    // -------------------------------------------------------------------------
    // Referral registration
    // -------------------------------------------------------------------------

    /// Register that `user_id` was referred via `referral_code`.
    ///
    /// # Errors
    /// - [`ReferralError::InvalidCode`] — the code does not exist.
    /// - [`ReferralError::AlreadyReferred`] — the user already has a referrer.
    /// - [`ReferralError::SelfReferral`] — the code belongs to `user_id` itself.
    pub fn apply_code(
        &mut self,
        user_id: UserId,
        referral_code: &str,
    ) -> Result<(), ReferralError> {
        let referrer_id = *self
            .code_to_user
            .get(referral_code)
            .ok_or_else(|| ReferralError::InvalidCode(referral_code.to_string()))?;

        if self.detect_self_referral(user_id, referral_code) {
            return Err(ReferralError::SelfReferral);
        }

        if self.referred_by.contains_key(&user_id) {
            return Err(ReferralError::AlreadyReferred(user_id));
        }

        self.referred_by.insert(user_id, referrer_id);
        self.referrals
            .entry(referrer_id)
            .or_default()
            .push(user_id);

        tracing::info!(
            trader = %user_id,
            referrer = %referrer_id,
            code = referral_code,
            "referral applied"
        );

        Ok(())
    }

    /// Returns `true` if `referral_code` belongs to `user_id` (self-referral attempt).
    pub fn detect_self_referral(&self, user_id: UserId, referral_code: &str) -> bool {
        self.code_to_user
            .get(referral_code)
            .map(|&owner| owner == user_id)
            .unwrap_or(false)
    }

    // -------------------------------------------------------------------------
    // Commission recording
    // -------------------------------------------------------------------------

    /// Record a trade fee and distribute referral commissions.
    ///
    /// If `trader_id` has a direct referrer, that referrer earns
    /// `commission_rate * fee_amount`.  If the referrer was themselves referred,
    /// their referrer earns `level2_rate * fee_amount`.
    pub fn on_trade_fee(
        &mut self,
        trader_id: UserId,
        trade_id: u64,
        fee_amount: Decimal,
        asset: &str,
        timestamp: DateTime<Utc>,
    ) {
        let Some(&level1_referrer) = self.referred_by.get(&trader_id) else {
            return;
        };

        // Level 1 commission
        let level1_commission = fee_amount * self.commission_rate;
        self.credit_commission(level1_referrer, asset, level1_commission);
        self.history.push(CommissionRecord {
            id: Uuid::new_v4(),
            referrer_id: level1_referrer,
            trader_id,
            trade_id,
            fee_amount,
            commission: level1_commission,
            asset: asset.to_string(),
            level: 1,
            timestamp,
        });

        tracing::debug!(
            referrer = %level1_referrer,
            trader = %trader_id,
            commission = %level1_commission,
            asset,
            level = 1,
            "referral commission recorded"
        );

        // Level 2 commission — referrer's own referrer
        if let Some(&level2_referrer) = self.referred_by.get(&level1_referrer) {
            let level2_commission = fee_amount * self.level2_rate;
            self.credit_commission(level2_referrer, asset, level2_commission);
            self.history.push(CommissionRecord {
                id: Uuid::new_v4(),
                referrer_id: level2_referrer,
                trader_id,
                trade_id,
                fee_amount,
                commission: level2_commission,
                asset: asset.to_string(),
                level: 2,
                timestamp,
            });

            tracing::debug!(
                referrer = %level2_referrer,
                trader = %trader_id,
                commission = %level2_commission,
                asset,
                level = 2,
                "referral commission recorded"
            );
        }
    }

    // -------------------------------------------------------------------------
    // Queries
    // -------------------------------------------------------------------------

    /// Accumulated commissions for `user_id`: asset → total amount.
    pub fn get_commissions(&self, user_id: UserId) -> HashMap<String, Decimal> {
        self.commissions
            .get(&user_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Users directly referred by `user_id`.
    pub fn get_referrals(&self, user_id: UserId) -> Vec<UserId> {
        self.referrals
            .get(&user_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Most-recent-first commission history entries for `user_id`, capped at `limit`.
    pub fn commission_history(
        &self,
        user_id: UserId,
        limit: usize,
    ) -> Vec<&CommissionRecord> {
        self.history
            .iter()
            .rev()
            .filter(|r| r.referrer_id == user_id)
            .take(limit)
            .collect()
    }

    /// Total number of users directly referred by `user_id`.
    pub fn total_referred_count(&self, user_id: UserId) -> usize {
        self.referrals
            .get(&user_id)
            .map(Vec::len)
            .unwrap_or(0)
    }

    // -------------------------------------------------------------------------
    // Internal helpers
    // -------------------------------------------------------------------------

    fn credit_commission(&mut self, user_id: UserId, asset: &str, amount: Decimal) {
        *self
            .commissions
            .entry(user_id)
            .or_default()
            .entry(asset.to_string())
            .or_insert(Decimal::ZERO) += amount;
    }

    /// Derive a unique 8-character alphanumeric code for `user_id`.
    ///
    /// The first attempt uses the first 8 hex chars of SHA256(user_id||salt).
    /// On the rare collision an incrementing counter is appended to the input
    /// until a free code is found.
    fn derive_unique_code(&self, user_id: UserId) -> String {
        let mut attempt = 0u64;
        loop {
            let input = format!("{}{}{}", user_id, self.salt, attempt);
            let hash = Sha256::digest(input.as_bytes());
            // Take 5 bytes (40 bits) → encode as 8 base-32 chars (uppercase alphanumeric)
            let code = base32_encode(&hash[..5]);
            if !self.code_to_user.contains_key(&code) {
                return code;
            }
            attempt += 1;
        }
    }
}

impl Default for ReferralManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Encode bytes as uppercase alphanumeric using the Crockford base-32 alphabet
/// (digits 0-9 and letters A-Z excluding I, L, O, U — easy to read, no ambiguous chars).
fn base32_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut bits: u64 = 0;
    let mut bit_count = 0u32;
    let mut out = String::with_capacity(8);

    for &byte in bytes {
        bits = (bits << 8) | u64::from(byte);
        bit_count += 8;
        while bit_count >= 5 {
            bit_count -= 5;
            let idx = ((bits >> bit_count) & 0x1F) as usize;
            out.push(ALPHABET[idx] as char);
        }
    }

    // Pad to 8 characters if necessary
    while out.len() < 8 {
        let idx = ((bits << (5 - bit_count)) & 0x1F) as usize;
        out.push(ALPHABET[idx] as char);
        break;
    }

    out.truncate(8);
    out
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_manager() -> ReferralManager {
        // Fixed salt for deterministic code generation in tests
        ReferralManager::with_rates_and_salt(dec!(0.20), dec!(0.05), "test-salt-fixed")
    }

    fn make_user() -> UserId {
        Uuid::new_v4()
    }

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    // -------------------------------------------------------------------------
    // Code generation
    // -------------------------------------------------------------------------

    #[test]
    fn test_generate_code_returns_8_chars() {
        let mut mgr = make_manager();
        let user = make_user();
        let code = mgr.generate_code(user);
        assert_eq!(code.len(), 8, "code must be exactly 8 characters");
    }

    #[test]
    fn test_generate_code_is_alphanumeric() {
        let mut mgr = make_manager();
        let user = make_user();
        let code = mgr.generate_code(user);
        assert!(
            code.chars().all(|c| c.is_ascii_alphanumeric()),
            "code must be alphanumeric, got: {code}"
        );
    }

    #[test]
    fn test_generate_code_is_idempotent() {
        let mut mgr = make_manager();
        let user = make_user();
        let code1 = mgr.generate_code(user);
        let code2 = mgr.generate_code(user);
        assert_eq!(code1, code2, "same user must always get the same code");
    }

    #[test]
    fn test_generate_codes_are_unique_across_users() {
        let mut mgr = make_manager();
        let codes: Vec<String> = (0..50).map(|_| mgr.generate_code(make_user())).collect();
        let unique: std::collections::HashSet<&String> = codes.iter().collect();
        assert_eq!(codes.len(), unique.len(), "every user must receive a unique code");
    }

    #[test]
    fn test_get_referral_code_returns_none_before_generation() {
        let mgr = make_manager();
        let user = make_user();
        assert!(mgr.get_referral_code(user).is_none());
    }

    #[test]
    fn test_get_referral_code_returns_some_after_generation() {
        let mut mgr = make_manager();
        let user = make_user();
        let code = mgr.generate_code(user);
        assert_eq!(mgr.get_referral_code(user), Some(code.as_str()));
    }

    // -------------------------------------------------------------------------
    // Applying referral codes
    // -------------------------------------------------------------------------

    #[test]
    fn test_apply_valid_code_succeeds() {
        let mut mgr = make_manager();
        let referrer = make_user();
        let new_user = make_user();

        let code = mgr.generate_code(referrer);
        assert!(mgr.apply_code(new_user, &code).is_ok());
    }

    #[test]
    fn test_apply_invalid_code_returns_error() {
        let mut mgr = make_manager();
        let user = make_user();

        let err = mgr.apply_code(user, "DEADBEEF").unwrap_err();
        assert!(matches!(err, ReferralError::InvalidCode(_)));
    }

    #[test]
    fn test_self_referral_is_blocked() {
        let mut mgr = make_manager();
        let user = make_user();
        let code = mgr.generate_code(user);

        let err = mgr.apply_code(user, &code).unwrap_err();
        assert_eq!(err, ReferralError::SelfReferral);
    }

    #[test]
    fn test_already_referred_is_blocked() {
        let mut mgr = make_manager();
        let referrer1 = make_user();
        let referrer2 = make_user();
        let new_user = make_user();

        let code1 = mgr.generate_code(referrer1);
        let code2 = mgr.generate_code(referrer2);

        mgr.apply_code(new_user, &code1).unwrap();
        let err = mgr.apply_code(new_user, &code2).unwrap_err();
        assert_eq!(err, ReferralError::AlreadyReferred(new_user));
    }

    #[test]
    fn test_detect_self_referral_true_for_own_code() {
        let mut mgr = make_manager();
        let user = make_user();
        let code = mgr.generate_code(user);
        assert!(mgr.detect_self_referral(user, &code));
    }

    #[test]
    fn test_detect_self_referral_false_for_other_code() {
        let mut mgr = make_manager();
        let user = make_user();
        let other = make_user();
        let code = mgr.generate_code(other);
        assert!(!mgr.detect_self_referral(user, &code));
    }

    // -------------------------------------------------------------------------
    // Commission calculations
    // -------------------------------------------------------------------------

    #[test]
    fn test_level1_commission_on_trade_fee() {
        let mut mgr = make_manager();
        let referrer = make_user();
        let trader = make_user();

        let code = mgr.generate_code(referrer);
        mgr.apply_code(trader, &code).unwrap();

        // Fee = 100 USDT → referrer earns 20 USDT (20%)
        mgr.on_trade_fee(trader, 1, dec!(100), "USDT", now());

        let commissions = mgr.get_commissions(referrer);
        assert_eq!(commissions.get("USDT"), Some(&dec!(20)));
    }

    #[test]
    fn test_level2_commission_on_trade_fee() {
        let mut mgr = make_manager();
        let grandparent = make_user();
        let parent = make_user();
        let trader = make_user();

        let gp_code = mgr.generate_code(grandparent);
        let p_code = mgr.generate_code(parent);

        mgr.apply_code(parent, &gp_code).unwrap();
        mgr.apply_code(trader, &p_code).unwrap();

        // Fee = 100 USDT
        // parent earns 20 USDT (level 1, 20%)
        // grandparent earns 5 USDT (level 2, 5%)
        mgr.on_trade_fee(trader, 1, dec!(100), "USDT", now());

        let parent_commissions = mgr.get_commissions(parent);
        assert_eq!(parent_commissions.get("USDT"), Some(&dec!(20)));

        let gp_commissions = mgr.get_commissions(grandparent);
        assert_eq!(gp_commissions.get("USDT"), Some(&dec!(5)));
    }

    #[test]
    fn test_no_commission_without_referrer() {
        let mut mgr = make_manager();
        let trader = make_user();

        // trader has no referrer — on_trade_fee must be a no-op
        mgr.on_trade_fee(trader, 1, dec!(100), "USDT", now());

        assert!(mgr.history.is_empty());
        assert!(mgr.commissions.is_empty());
    }

    #[test]
    fn test_commissions_accumulate_over_multiple_trades() {
        let mut mgr = make_manager();
        let referrer = make_user();
        let trader = make_user();

        let code = mgr.generate_code(referrer);
        mgr.apply_code(trader, &code).unwrap();

        for _ in 0..5 {
            mgr.on_trade_fee(trader, 1, dec!(100), "USDT", now());
        }

        // 5 * 20 = 100
        let commissions = mgr.get_commissions(referrer);
        assert_eq!(commissions.get("USDT"), Some(&dec!(100)));
    }

    #[test]
    fn test_commissions_across_multiple_assets() {
        let mut mgr = make_manager();
        let referrer = make_user();
        let trader = make_user();

        let code = mgr.generate_code(referrer);
        mgr.apply_code(trader, &code).unwrap();

        mgr.on_trade_fee(trader, 1, dec!(100), "USDT", now());
        mgr.on_trade_fee(trader, 2, dec!(50), "BTC", now());

        let commissions = mgr.get_commissions(referrer);
        assert_eq!(commissions.get("USDT"), Some(&dec!(20)));
        assert_eq!(commissions.get("BTC"), Some(&dec!(10)));
    }

    // -------------------------------------------------------------------------
    // Commission history
    // -------------------------------------------------------------------------

    #[test]
    fn test_commission_history_entries_match_trades() {
        let mut mgr = make_manager();
        let referrer = make_user();
        let trader = make_user();

        let code = mgr.generate_code(referrer);
        mgr.apply_code(trader, &code).unwrap();

        mgr.on_trade_fee(trader, 42, dec!(200), "USDT", now());

        let history = mgr.commission_history(referrer, 10);
        assert_eq!(history.len(), 1);
        let record = history[0];
        assert_eq!(record.referrer_id, referrer);
        assert_eq!(record.trader_id, trader);
        assert_eq!(record.trade_id, 42);
        assert_eq!(record.fee_amount, dec!(200));
        assert_eq!(record.commission, dec!(40)); // 20% of 200
        assert_eq!(record.asset, "USDT");
        assert_eq!(record.level, 1);
    }

    #[test]
    fn test_commission_history_limit_respected() {
        let mut mgr = make_manager();
        let referrer = make_user();
        let trader = make_user();

        let code = mgr.generate_code(referrer);
        mgr.apply_code(trader, &code).unwrap();

        for i in 0..10 {
            mgr.on_trade_fee(trader, i, dec!(100), "USDT", now());
        }

        assert_eq!(mgr.commission_history(referrer, 3).len(), 3);
        assert_eq!(mgr.commission_history(referrer, 10).len(), 10);
        assert_eq!(mgr.commission_history(referrer, 100).len(), 10);
    }

    #[test]
    fn test_commission_history_most_recent_first() {
        let mut mgr = make_manager();
        let referrer = make_user();
        let trader = make_user();

        let code = mgr.generate_code(referrer);
        mgr.apply_code(trader, &code).unwrap();

        mgr.on_trade_fee(trader, 1, dec!(100), "USDT", now());
        mgr.on_trade_fee(trader, 2, dec!(200), "USDT", now());

        let history = mgr.commission_history(referrer, 10);
        // Most recent first: trade_id 2 should appear before trade_id 1
        assert_eq!(history[0].trade_id, 2);
        assert_eq!(history[1].trade_id, 1);
    }

    #[test]
    fn test_history_only_returns_entries_for_requested_user() {
        let mut mgr = make_manager();
        let referrer_a = make_user();
        let referrer_b = make_user();
        let trader_a = make_user();
        let trader_b = make_user();

        let code_a = mgr.generate_code(referrer_a);
        let code_b = mgr.generate_code(referrer_b);
        mgr.apply_code(trader_a, &code_a).unwrap();
        mgr.apply_code(trader_b, &code_b).unwrap();

        mgr.on_trade_fee(trader_a, 1, dec!(100), "USDT", now());
        mgr.on_trade_fee(trader_b, 2, dec!(100), "USDT", now());

        assert_eq!(mgr.commission_history(referrer_a, 10).len(), 1);
        assert_eq!(mgr.commission_history(referrer_b, 10).len(), 1);
    }

    // -------------------------------------------------------------------------
    // Referral list queries
    // -------------------------------------------------------------------------

    #[test]
    fn test_get_referrals_returns_correct_list() {
        let mut mgr = make_manager();
        let referrer = make_user();
        let user_a = make_user();
        let user_b = make_user();
        let user_c = make_user();

        let code = mgr.generate_code(referrer);
        mgr.apply_code(user_a, &code).unwrap();
        mgr.apply_code(user_b, &code).unwrap();
        mgr.apply_code(user_c, &code).unwrap();

        let mut referrals = mgr.get_referrals(referrer);
        referrals.sort();
        let mut expected = vec![user_a, user_b, user_c];
        expected.sort();
        assert_eq!(referrals, expected);
    }

    #[test]
    fn test_get_referrals_empty_for_user_with_no_referrals() {
        let mgr = make_manager();
        let user = make_user();
        assert!(mgr.get_referrals(user).is_empty());
    }

    #[test]
    fn test_total_referred_count() {
        let mut mgr = make_manager();
        let referrer = make_user();
        let code = mgr.generate_code(referrer);

        assert_eq!(mgr.total_referred_count(referrer), 0);

        for _ in 0..7 {
            mgr.apply_code(make_user(), &code).unwrap();
        }

        assert_eq!(mgr.total_referred_count(referrer), 7);
    }

    #[test]
    fn test_level2_history_records_correct_level() {
        let mut mgr = make_manager();
        let grandparent = make_user();
        let parent = make_user();
        let trader = make_user();

        let gp_code = mgr.generate_code(grandparent);
        let p_code = mgr.generate_code(parent);
        mgr.apply_code(parent, &gp_code).unwrap();
        mgr.apply_code(trader, &p_code).unwrap();

        mgr.on_trade_fee(trader, 1, dec!(100), "USDT", now());

        // grandparent has exactly one history entry at level 2
        let gp_history = mgr.commission_history(grandparent, 10);
        assert_eq!(gp_history.len(), 1);
        assert_eq!(gp_history[0].level, 2);

        // parent has exactly one history entry at level 1
        let p_history = mgr.commission_history(parent, 10);
        assert_eq!(p_history.len(), 1);
        assert_eq!(p_history[0].level, 1);
    }

    #[test]
    fn test_multiple_referrals_from_same_referrer_independent_commissions() {
        let mut mgr = make_manager();
        let referrer = make_user();
        let trader_a = make_user();
        let trader_b = make_user();

        let code = mgr.generate_code(referrer);
        mgr.apply_code(trader_a, &code).unwrap();
        mgr.apply_code(trader_b, &code).unwrap();

        mgr.on_trade_fee(trader_a, 1, dec!(100), "USDT", now());
        mgr.on_trade_fee(trader_b, 2, dec!(300), "USDT", now());

        // 20% of 100 + 20% of 300 = 20 + 60 = 80
        let commissions = mgr.get_commissions(referrer);
        assert_eq!(commissions.get("USDT"), Some(&dec!(80)));
        assert_eq!(mgr.total_referred_count(referrer), 2);
    }

    #[test]
    fn test_get_commissions_returns_empty_map_for_unknown_user() {
        let mgr = make_manager();
        let user = make_user();
        assert!(mgr.get_commissions(user).is_empty());
    }
}
