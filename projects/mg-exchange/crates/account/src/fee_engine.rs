use std::collections::HashMap;
use std::str::FromStr;

use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;

use exchange_common::UserId;

/// Parse a decimal from a string literal. Panics at startup if the literal is invalid,
/// which is a programming error equivalent to a malformed constant.
fn d(s: &str) -> Decimal {
    Decimal::from_str(s).expect("hard-coded decimal literal must be valid")
}

/// A VIP fee tier with volume/balance thresholds and fee rates.
///
/// Maker fee can be negative (rebate) — Supreme tier earns money for posting liquidity.
#[derive(Debug, Clone)]
pub struct VipTier {
    pub level: u8,
    pub name: String,
    /// 30-day trading volume threshold (USD notional).
    pub min_volume_30d: Decimal,
    /// OR: asset balance threshold (USD equivalent). Either criterion qualifies.
    pub min_balance: Decimal,
    /// Maker fee rate. Negative = rebate paid TO the maker.
    pub maker_fee: Decimal,
    /// Taker fee rate. Always >= 0.
    pub taker_fee: Decimal,
}

/// Tracked per-user fee state.
#[derive(Debug, Clone)]
pub struct UserFeeState {
    pub tier_level: u8,
    pub volume_30d: Decimal,
    pub balance_usd: Decimal,
    pub last_updated: DateTime<Utc>,
    /// Timestamp after which the current `volume_30d` window expires.
    /// Volume is reset to zero once this deadline is passed, preventing
    /// users from holding VIP tier forever on stale volume.
    pub volume_reset_at: DateTime<Utc>,
}

impl UserFeeState {
    fn new() -> Self {
        let now = Utc::now();
        Self {
            tier_level: 0,
            volume_30d: Decimal::ZERO,
            balance_usd: Decimal::ZERO,
            last_updated: now,
            volume_reset_at: now + Duration::days(30),
        }
    }
}

/// Manages VIP fee tiers and per-user fee state.
///
/// Tier qualification: a user reaches a tier if EITHER their 30-day volume
/// OR their USD balance meets the tier's minimum threshold.
///
/// Phase 1: in-memory. Phase 2: persist volume + tier snapshots in PostgreSQL.
pub struct FeeEngine {
    /// All tiers ordered ascending by level.
    tiers: Vec<VipTier>,
    /// user_id → current tier + 30-day volume.
    user_tiers: HashMap<UserId, UserFeeState>,
}

impl FeeEngine {
    /// Create a new FeeEngine with the default Bybit-inspired tier schedule.
    pub fn new() -> Self {
        let tiers = vec![
            VipTier {
                level: 0,
                name: "Regular".to_string(),
                min_volume_30d: Decimal::ZERO,
                min_balance: Decimal::ZERO,
                maker_fee: d("0.001"),   // 0.10%
                taker_fee: d("0.002"),   // 0.20%
            },
            VipTier {
                level: 1,
                name: "VIP1".to_string(),
                min_volume_30d: d("1000000"),
                min_balance: Decimal::ZERO,
                maker_fee: d("0.0008"),  // 0.08%
                taker_fee: d("0.0016"),  // 0.16%
            },
            VipTier {
                level: 2,
                name: "VIP2".to_string(),
                min_volume_30d: d("5000000"),
                min_balance: Decimal::ZERO,
                maker_fee: d("0.0006"),  // 0.06%
                taker_fee: d("0.0012"),  // 0.12%
            },
            VipTier {
                level: 3,
                name: "VIP3".to_string(),
                min_volume_30d: d("25000000"),
                min_balance: Decimal::ZERO,
                maker_fee: d("0.0004"),  // 0.04%
                taker_fee: d("0.0008"),  // 0.08%
            },
            VipTier {
                level: 4,
                name: "VIP4".to_string(),
                min_volume_30d: d("100000000"),
                min_balance: Decimal::ZERO,
                maker_fee: d("0.0002"),  // 0.02%
                taker_fee: d("0.0005"),  // 0.05%
            },
            VipTier {
                level: 5,
                name: "Supreme".to_string(),
                min_volume_30d: d("500000000"),
                min_balance: Decimal::ZERO,
                maker_fee: d("-0.0001"), // -0.01% rebate
                taker_fee: d("0.0003"),  // 0.03%
            },
        ];

        Self {
            tiers,
            user_tiers: HashMap::new(),
        }
    }

    /// Create a FeeEngine with a custom tier schedule.
    ///
    /// Used for testing and for exchanges that want to override the default
    /// Bybit-inspired tiers with their own fee structure.
    pub fn with_custom_tiers(tiers: Vec<VipTier>) -> Self {
        Self {
            tiers,
            user_tiers: HashMap::new(),
        }
    }

    /// Return all configured VIP tiers (ordered ascending by level).
    pub fn tiers(&self) -> &[VipTier] {
        &self.tiers
    }

    /// Read-only access to per-user fee state.
    ///
    /// Used by the VIP tier recalculator background task to iterate all users
    /// and sync balance-based tier qualification.
    pub fn user_tiers(&self) -> &HashMap<UserId, UserFeeState> {
        &self.user_tiers
    }

    /// Get the maker and taker fee rates for a user.
    ///
    /// Returns Regular (level 0) fees if the user has no recorded state.
    pub fn get_fees(&self, user_id: UserId) -> (Decimal, Decimal) {
        let tier = self.get_tier(user_id);
        (tier.maker_fee, tier.taker_fee)
    }

    /// Get the current VIP tier for a user.
    ///
    /// Returns the Regular tier if the user has no recorded state.
    pub fn get_tier(&self, user_id: UserId) -> &VipTier {
        let level = self
            .user_tiers
            .get(&user_id)
            .map(|s| s.tier_level)
            .unwrap_or(0) as usize;

        // tiers[0] always exists — guaranteed by constructor
        &self.tiers[level.min(self.tiers.len() - 1)]
    }

    /// Record a trade's notional value toward the user's 30-day volume.
    ///
    /// Call this every time a fill is settled. Automatically recalculates
    /// the tier after updating volume.
    ///
    /// Also resets the 30-day window on every new trade so that the window
    /// is always "last 30 days of activity".
    pub fn update_volume(&mut self, user_id: UserId, trade_notional: Decimal) {
        let now = Utc::now();
        let state = self.user_tiers.entry(user_id).or_insert_with(UserFeeState::new);
        state.volume_30d += trade_notional;
        state.last_updated = now;
        // Extend the decay deadline: 30 days from the most recent trade.
        state.volume_reset_at = now + Duration::days(30);

        // Eagerly recalculate so callers see the new tier immediately
        self.recalculate_tier(user_id);
    }

    /// Reset `volume_30d` to zero for any user whose 30-day window has expired.
    ///
    /// Call this periodically (e.g. hourly) from a background task.  Users who
    /// have not traded in the last 30 days will have their VIP tier demoted.
    pub fn decay_stale_volumes(&mut self) {
        let now = Utc::now();
        let stale: Vec<UserId> = self
            .user_tiers
            .iter()
            .filter(|(_, s)| s.volume_30d > Decimal::ZERO && now >= s.volume_reset_at)
            .map(|(uid, _)| *uid)
            .collect();

        for user_id in stale {
            if let Some(state) = self.user_tiers.get_mut(&user_id) {
                tracing::info!(
                    %user_id,
                    old_volume = %state.volume_30d,
                    old_tier = state.tier_level,
                    "VIP volume window expired — resetting 30d volume"
                );
                state.volume_30d = Decimal::ZERO;
                state.volume_reset_at = now + Duration::days(30);
            }
            self.recalculate_tier(user_id);
        }
    }

    /// Return the user's current 30-day trading volume (USD notional).
    ///
    /// Returns zero if the user has no recorded fee state.
    pub fn get_volume(&self, user_id: UserId) -> Decimal {
        self.user_tiers
            .get(&user_id)
            .map(|s| s.volume_30d)
            .unwrap_or(Decimal::ZERO)
    }

    /// Update the USD balance used for tier qualification.
    ///
    /// Call this when the user's portfolio value changes (e.g., after deposit
    /// or periodic mark-to-market). Automatically recalculates the tier.
    pub fn set_balance(&mut self, user_id: UserId, balance_usd: Decimal) {
        let state = self.user_tiers.entry(user_id).or_insert_with(UserFeeState::new);
        state.balance_usd = balance_usd;
        state.last_updated = Utc::now();

        self.recalculate_tier(user_id);
    }

    /// Re-evaluate and update the tier for a user based on current volume + balance.
    ///
    /// Scans tiers from highest to lowest and assigns the first one the user
    /// qualifies for. A user qualifies for a tier when EITHER volume OR balance
    /// meets the tier's minimum.
    pub fn recalculate_tier(&mut self, user_id: UserId) {
        let state = match self.user_tiers.get_mut(&user_id) {
            Some(s) => s,
            None => return,
        };

        let volume = state.volume_30d;
        let balance = state.balance_usd;

        // Walk from highest to lowest tier, assign first match.
        // A tier's balance criterion only applies when min_balance > 0 (zero means
        // "no balance requirement for this tier").
        let new_level = self
            .tiers
            .iter()
            .rev()
            .find(|tier| {
                let qualifies_by_volume = volume >= tier.min_volume_30d;
                let qualifies_by_balance =
                    tier.min_balance > Decimal::ZERO && balance >= tier.min_balance;
                qualifies_by_volume || qualifies_by_balance
            })
            .map(|tier| tier.level)
            .unwrap_or(0);

        if new_level != state.tier_level {
            tracing::info!(
                user_id = %user_id,
                old_level = state.tier_level,
                new_level,
                volume_30d = %volume,
                balance_usd = %balance,
                "VIP tier changed"
            );
            state.tier_level = new_level;
        }
    }
}

impl Default for FeeEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_user() -> UserId {
        uuid::Uuid::new_v4()
    }

    // --- Tier lookup ---

    #[test]
    fn test_new_user_gets_regular_tier() {
        let engine = FeeEngine::new();
        let user = make_user();
        let tier = engine.get_tier(user);
        assert_eq!(tier.level, 0);
        assert_eq!(tier.name, "Regular");
    }

    #[test]
    fn test_default_fees_are_regular() {
        let engine = FeeEngine::new();
        let user = make_user();
        let (maker, taker) = engine.get_fees(user);
        assert_eq!(maker, dec!(0.001));
        assert_eq!(taker, dec!(0.002));
    }

    // --- Volume-driven tier upgrades ---

    #[test]
    fn test_volume_upgrade_to_vip1() {
        let mut engine = FeeEngine::new();
        let user = make_user();
        // Exactly at the VIP1 threshold
        engine.update_volume(user, dec!(1_000_000));
        assert_eq!(engine.get_tier(user).level, 1);
        assert_eq!(engine.get_tier(user).name, "VIP1");
    }

    #[test]
    fn test_volume_below_threshold_stays_regular() {
        let mut engine = FeeEngine::new();
        let user = make_user();
        engine.update_volume(user, dec!(999_999));
        assert_eq!(engine.get_tier(user).level, 0);
    }

    #[test]
    fn test_volume_upgrade_to_vip3() {
        let mut engine = FeeEngine::new();
        let user = make_user();
        engine.update_volume(user, dec!(25_000_000));
        assert_eq!(engine.get_tier(user).level, 3);
        let (maker, taker) = engine.get_fees(user);
        assert_eq!(maker, dec!(0.0004));
        assert_eq!(taker, dec!(0.0008));
    }

    #[test]
    fn test_cumulative_volume_triggers_upgrade() {
        let mut engine = FeeEngine::new();
        let user = make_user();
        // Many small trades accumulate
        for _ in 0..10 {
            engine.update_volume(user, dec!(200_000));
        }
        // 10 * 200_000 = 2_000_000, qualifies for VIP2? No, VIP2 is 5M. Should be VIP1.
        assert_eq!(engine.get_tier(user).level, 1);
    }

    #[test]
    fn test_volume_upgrade_to_supreme() {
        let mut engine = FeeEngine::new();
        let user = make_user();
        engine.update_volume(user, dec!(500_000_000));
        let tier = engine.get_tier(user);
        assert_eq!(tier.level, 5);
        assert_eq!(tier.name, "Supreme");
    }

    // --- Supreme maker rebate ---

    #[test]
    fn test_supreme_maker_fee_is_negative_rebate() {
        let mut engine = FeeEngine::new();
        let user = make_user();
        engine.update_volume(user, dec!(500_000_000));
        let (maker, taker) = engine.get_fees(user);
        assert!(maker < Decimal::ZERO, "Supreme maker fee should be a rebate (negative)");
        assert_eq!(maker, dec!(-0.0001));
        assert_eq!(taker, dec!(0.0003));
    }

    // --- Balance-driven tier qualification ---

    #[test]
    fn test_balance_zero_min_balance_does_not_auto_qualify() {
        // In the default tier schedule all tiers have min_balance = 0, which means
        // the balance criterion is inactive (zero means "no balance requirement").
        // A user with high balance but zero volume should stay at Regular.
        let mut engine = FeeEngine::new();
        let user = make_user();
        engine.set_balance(user, dec!(10_000_000));
        // Volume is 0 → only tier 0 (min_volume_30d = 0) qualifies by volume.
        // Higher tiers require min_volume_30d > 0, and min_balance is 0 (inactive).
        assert_eq!(engine.get_tier(user).level, 0);
    }

    #[test]
    fn test_balance_qualifies_when_min_balance_is_set() {
        // Build a custom engine with a tier that has a real min_balance threshold.
        let tiers = vec![
            VipTier {
                level: 0,
                name: "Regular".to_string(),
                min_volume_30d: Decimal::ZERO,
                min_balance: Decimal::ZERO,
                maker_fee: d("0.001"),
                taker_fee: d("0.002"),
            },
            VipTier {
                level: 1,
                name: "BalanceVIP".to_string(),
                min_volume_30d: d("1000000"),
                min_balance: d("50000"), // $50k balance OR $1M volume
                maker_fee: d("0.0008"),
                taker_fee: d("0.0016"),
            },
        ];
        let mut engine = FeeEngine { tiers, user_tiers: HashMap::new() };

        let user = make_user();
        // User has $50k balance but zero volume → qualifies via balance
        engine.set_balance(user, dec!(50_000));
        assert_eq!(engine.get_tier(user).level, 1, "should qualify via min_balance");

        let vol_user = make_user();
        // Different user qualifies via volume alone
        engine.update_volume(vol_user, dec!(1_000_000));
        assert_eq!(engine.get_tier(vol_user).level, 1, "should qualify via min_volume_30d");
    }

    #[test]
    fn test_both_volume_and_balance_are_independent_criteria() {
        let mut engine = FeeEngine::new();
        let vol_user = make_user();

        // Volume path — qualifies for VIP2 via volume
        engine.update_volume(vol_user, dec!(5_000_000));
        assert_eq!(engine.get_tier(vol_user).level, 2);

        // Setting balance additionally doesn't break anything
        engine.set_balance(vol_user, dec!(100_000));
        assert_eq!(engine.get_tier(vol_user).level, 2);
    }

    #[test]
    fn test_recalculate_tier_is_idempotent() {
        let mut engine = FeeEngine::new();
        let user = make_user();
        engine.update_volume(user, dec!(1_000_000));

        let level_before = engine.get_tier(user).level;
        engine.recalculate_tier(user);
        let level_after = engine.get_tier(user).level;

        assert_eq!(level_before, level_after);
    }

    #[test]
    fn test_recalculate_unknown_user_is_noop() {
        let mut engine = FeeEngine::new();
        let unknown = make_user();
        // Should not panic
        engine.recalculate_tier(unknown);
    }

    #[test]
    fn test_vip4_fees() {
        let mut engine = FeeEngine::new();
        let user = make_user();
        engine.update_volume(user, dec!(100_000_000));
        let (maker, taker) = engine.get_fees(user);
        assert_eq!(maker, dec!(0.0002));
        assert_eq!(taker, dec!(0.0005));
    }

    #[test]
    fn test_multiple_users_independent() {
        let mut engine = FeeEngine::new();
        let whale = make_user();
        let retail = make_user();

        engine.update_volume(whale, dec!(500_000_000));
        engine.update_volume(retail, dec!(100));

        assert_eq!(engine.get_tier(whale).level, 5);
        assert_eq!(engine.get_tier(retail).level, 0);
    }
}
