//! Unified Trading Account — single margin pool across spot, futures, and options.
//!
//! # What is a Unified Account?
//!
//! In the **Classic** account model users hold separate balance pools:
//! - *Trading account* — locked balances for spot orders.
//! - *Futures account* — USDT margin for perpetual positions.
//! - *Fund account* — savings / earn / deposits.
//!
//! Transferring between pools adds friction and capital inefficiency.
//!
//! A **Unified** account merges all pools: spot holdings act as collateral for
//! futures positions, and unrealised PnL from one position can margin another.
//! This is how Bybit UTA and OKX Portfolio Margin work.
//!
//! # Collateral haircuts
//!
//! Not all assets are equally liquid. We apply a *haircut*: `1 USDT` of BTC
//! is worth `0.95 USD` of collateral. This protects the exchange against
//! adverse price moves before a liquidation can be executed.
//!
//! **Anti-FTT policy**: the exchange's own token (EXC) has a haircut of 0,
//! meaning it contributes **zero** collateral value. This prevents the
//! FTX-style circular collateralisation collapse where the exchange token was
//! used to back user positions.
//!
//! # Margin ratio
//!
//! ```text
//! margin_ratio = total_initial_margin / total_collateral_value_usd
//! ```
//!
//! - 0   → no margin used
//! - 1   → fully utilised, no room for new positions
//! - >1  → maintenance margin zone; liquidation may be imminent
//!
//! Liquidation is triggered when:
//! ```text
//! total_equity_usd <= total_maintenance_margin
//! ```

use std::collections::HashMap;

use rust_decimal::Decimal;
use uuid::Uuid;

// ─────────────────────────────────────────────────────────────────────────────
// Account mode
// ─────────────────────────────────────────────────────────────────────────────

/// Operating mode of a user account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountMode {
    /// Separate balance pools: trading / futures / fund (legacy behaviour).
    Classic,
    /// Single unified balance pool; spot holdings are futures collateral.
    Unified,
}

impl std::fmt::Display for AccountMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccountMode::Classic => write!(f, "classic"),
            AccountMode::Unified => write!(f, "unified"),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Collateral haircuts
// ─────────────────────────────────────────────────────────────────────────────

/// Return the collateral haircut factor for an asset.
///
/// A haircut of `0.95` means 1 unit of the asset counts as `0.95 USD` worth
/// of margin capacity. Stablecoins have no discount (haircut = 1.00).
///
/// # Anti-FTT policy
///
/// `EXC` (the exchange's own token) always returns `0`. This ensures the
/// exchange can never back user positions with its own circulating supply —
/// the structural flaw that contributed to the FTX collapse.
pub fn collateral_haircut(asset: &str) -> Decimal {
    // Decimal::from_parts(mantissa_lo, mantissa_mid, mantissa_hi, negative, scale)
    // 1.00 = 100 * 10^-2
    // 0.95 = 95 * 10^-2
    // 0.90 = 90 * 10^-2  etc.
    match asset {
        // Fiat stablecoins — treated as cash equivalents (100%).
        "USDT" | "USDC" | "BUSD" | "DAI" | "FDUSD" => Decimal::ONE,
        // Tier-1 crypto assets.
        "BTC" => Decimal::from_parts(95, 0, 0, false, 2),  // 0.95
        "ETH" => Decimal::from_parts(90, 0, 0, false, 2),  // 0.90
        // Major layer-1 / large-cap alts.
        "SOL" | "BNB" | "XRP" | "ADA" | "AVAX" | "DOT" => {
            Decimal::from_parts(85, 0, 0, false, 2)  // 0.85
        }
        // Exchange token: ZERO haircut (anti-FTT policy).
        "EXC" => Decimal::ZERO,
        // Everything else: conservative discount.
        _ => Decimal::from_parts(80, 0, 0, false, 2),  // 0.80
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Per-asset balance
// ─────────────────────────────────────────────────────────────────────────────

/// Balance and collateral information for a single asset in a unified account.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UnifiedAssetBalance {
    /// Asset ticker (e.g. "BTC", "USDT").
    pub asset: String,
    /// Total wallet balance: deposited funds plus settled PnL, minus withdrawals.
    /// Excludes unrealised PnL.
    pub wallet_balance: Decimal,
    /// Amount locked in open spot orders (cannot be withdrawn or used as margin).
    pub locked_in_orders: Decimal,
    /// Sum of unrealised PnL from all derivative positions denominated in this
    /// asset (e.g. BTC for inverse perps, USDT for linear perps).
    pub unrealized_pnl: Decimal,
    /// `wallet_balance + unrealized_pnl` — the mark-to-market value of the asset.
    pub equity: Decimal,
    /// `equity * spot_price_usd * haircut` — USD collateral contribution.
    pub collateral_value_usd: Decimal,
    /// Haircut applied (informational; see [`collateral_haircut`]).
    pub haircut: Decimal,
}

impl UnifiedAssetBalance {
    /// Compute equity and collateral value given the current spot price in USD.
    ///
    /// If `price_usd` is zero the asset contributes no collateral (safe default
    /// for assets with no market price).
    pub fn revalue(&mut self, price_usd: Decimal) {
        self.equity = self.wallet_balance + self.unrealized_pnl;
        let hc = collateral_haircut(&self.asset);
        self.haircut = hc;
        self.collateral_value_usd = if price_usd.is_zero() || self.equity <= Decimal::ZERO {
            Decimal::ZERO
        } else {
            self.equity * price_usd * hc
        };
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Unified account
// ─────────────────────────────────────────────────────────────────────────────

/// Full unified account state for one user.
///
/// In Classic mode the fields are still computed but reflect only the
/// trading-account balance (futures collateral semantics do not apply).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UnifiedAccount {
    /// Owner of this account.
    pub user_id: Uuid,
    /// Whether the user is in Classic or Unified mode.
    pub mode: AccountMode,
    /// Per-asset breakdown. Keyed by asset ticker.
    pub assets: HashMap<String, UnifiedAssetBalance>,
    /// Sum of all `collateral_value_usd` across assets (USD).
    pub total_collateral_usd: Decimal,
    /// Sum of all `equity * price` without haircuts (USD).
    pub total_equity_usd: Decimal,
    /// Sum of initial margin requirements across all open positions (USD).
    pub total_initial_margin: Decimal,
    /// Sum of maintenance margin requirements across all open positions (USD).
    pub total_maintenance_margin: Decimal,
    /// `total_collateral_usd - total_initial_margin - locked_in_orders_usd`
    pub available_balance_usd: Decimal,
}

impl UnifiedAccount {
    /// Create a new account in the given mode with no assets.
    pub fn new(user_id: Uuid, mode: AccountMode) -> Self {
        Self {
            user_id,
            mode,
            assets: HashMap::new(),
            total_collateral_usd: Decimal::ZERO,
            total_equity_usd: Decimal::ZERO,
            total_initial_margin: Decimal::ZERO,
            total_maintenance_margin: Decimal::ZERO,
            available_balance_usd: Decimal::ZERO,
        }
    }

    /// Upsert a wallet balance for an asset.
    ///
    /// Call this whenever the underlying `AccountManager` balance changes.
    /// The collateral value is not recalculated here — call [`calculate_equity`]
    /// after all asset updates to refresh the aggregate totals.
    pub fn set_wallet_balance(
        &mut self,
        asset: &str,
        wallet_balance: Decimal,
        locked_in_orders: Decimal,
        unrealized_pnl: Decimal,
    ) {
        let entry = self.assets.entry(asset.to_string()).or_insert_with(|| {
            UnifiedAssetBalance {
                asset: asset.to_string(),
                wallet_balance: Decimal::ZERO,
                locked_in_orders: Decimal::ZERO,
                unrealized_pnl: Decimal::ZERO,
                equity: Decimal::ZERO,
                collateral_value_usd: Decimal::ZERO,
                haircut: collateral_haircut(asset),
            }
        });
        entry.wallet_balance = wallet_balance;
        entry.locked_in_orders = locked_in_orders;
        entry.unrealized_pnl = unrealized_pnl;
    }

    /// Recalculate all per-asset collateral values and refresh aggregate totals.
    ///
    /// `prices` maps asset ticker → current spot price in USD. Assets not
    /// present in `prices` are treated as price-zero (no collateral contribution).
    ///
    /// `initial_margin` and `maintenance_margin` are the sum of open-position
    /// margin requirements, expressed in USD.
    pub fn calculate_equity(
        &mut self,
        prices: &HashMap<String, Decimal>,
        initial_margin: Decimal,
        maintenance_margin: Decimal,
    ) {
        let mut total_collateral = Decimal::ZERO;
        let mut total_equity = Decimal::ZERO;
        let mut total_locked_usd = Decimal::ZERO;

        for asset_bal in self.assets.values_mut() {
            let price = prices
                .get(&asset_bal.asset)
                .copied()
                .unwrap_or(Decimal::ZERO);

            asset_bal.revalue(price);
            total_collateral += asset_bal.collateral_value_usd;

            // Gross equity (before haircuts) for informational display.
            let gross_equity = asset_bal.equity * price;
            if gross_equity > Decimal::ZERO {
                total_equity += gross_equity;
            }

            // Locked orders reduce available capacity.
            let locked_usd = asset_bal.locked_in_orders * price;
            if locked_usd > Decimal::ZERO {
                total_locked_usd += locked_usd;
            }
        }

        self.total_collateral_usd = total_collateral;
        self.total_equity_usd = total_equity;
        self.total_initial_margin = initial_margin;
        self.total_maintenance_margin = maintenance_margin;

        // available = collateral − initial_margin − locked_orders
        let raw_available = total_collateral - initial_margin - total_locked_usd;
        self.available_balance_usd = raw_available.max(Decimal::ZERO);
    }

    /// Return the USD available for opening new positions or placing new orders.
    ///
    /// Always non-negative.
    pub fn available_for_trading(&self) -> Decimal {
        self.available_balance_usd
    }

    /// Return whether the account has enough room to absorb `additional_margin`.
    pub fn can_open_position(&self, additional_margin: Decimal) -> bool {
        self.available_balance_usd >= additional_margin
    }

    /// Return whether this account should be liquidated.
    ///
    /// Liquidation is triggered when total equity falls to or below the
    /// total maintenance margin requirement:
    /// ```text
    /// total_equity_usd <= total_maintenance_margin
    /// ```
    pub fn is_liquidatable(&self) -> bool {
        // Guard: no positions means nothing to liquidate.
        if self.total_maintenance_margin.is_zero() {
            return false;
        }
        self.total_equity_usd <= self.total_maintenance_margin
    }

    /// Return the margin utilisation ratio.
    ///
    /// ```text
    /// margin_ratio = total_initial_margin / total_collateral_usd
    /// ```
    ///
    /// - `0.0` — no margin in use
    /// - `1.0` — fully utilised
    /// - `>1.0` — over-margined / liquidation zone
    ///
    /// Returns `Decimal::ZERO` when there is no collateral.
    pub fn margin_ratio(&self) -> Decimal {
        if self.total_collateral_usd.is_zero() {
            return Decimal::ZERO;
        }
        self.total_initial_margin / self.total_collateral_usd
    }

    /// Build a unified account by merging spot, futures, and fund balances.
    ///
    /// This is called when a user upgrades from Classic to Unified mode.
    /// All three legacy balance maps are merged into a single asset pool.
    /// The account mode is set to `Unified` and all margins start at zero
    /// (they are recalculated on the next call to [`calculate_equity`]).
    ///
    /// # Arguments
    ///
    /// - `user_id` — owner of the new account.
    /// - `spot_balances` — trading account balances (asset → total).
    /// - `futures_balances` — futures margin account (asset → total).
    /// - `fund_balances` — fund / savings account (asset → total).
    pub fn upgrade_to_unified(
        user_id: Uuid,
        spot_balances: &HashMap<String, Decimal>,
        futures_balances: &HashMap<String, Decimal>,
        fund_balances: &HashMap<String, Decimal>,
    ) -> Self {
        let mut account = Self::new(user_id, AccountMode::Unified);

        // Merge all three balance sources into a single asset map.
        // We use a temporary accumulator to avoid multiple mutable borrow issues.
        let mut merged: HashMap<String, Decimal> = HashMap::new();

        for (asset, &amount) in spot_balances {
            *merged.entry(asset.clone()).or_default() += amount;
        }
        for (asset, &amount) in futures_balances {
            *merged.entry(asset.clone()).or_default() += amount;
        }
        for (asset, &amount) in fund_balances {
            *merged.entry(asset.clone()).or_default() += amount;
        }

        for (asset, total) in merged {
            account.set_wallet_balance(&asset, total, Decimal::ZERO, Decimal::ZERO);
        }

        account
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    use rust_decimal_macros::dec;

    fn usdt_prices() -> HashMap<String, Decimal> {
        let mut p = HashMap::new();
        p.insert("USDT".to_string(), dec!(1));
        p.insert("BTC".to_string(), dec!(50000));
        p.insert("ETH".to_string(), dec!(3000));
        p.insert("SOL".to_string(), dec!(200));
        p.insert("EXC".to_string(), dec!(5));
        p
    }

    // ── collateral_haircut ───────────────────────────────────────────────────

    #[test]
    fn test_stablecoins_have_full_haircut() {
        for stable in &["USDT", "USDC", "BUSD", "DAI", "FDUSD"] {
            assert_eq!(
                collateral_haircut(stable),
                dec!(1.00),
                "{stable} should have 1.00 haircut"
            );
        }
    }

    #[test]
    fn test_btc_haircut() {
        assert_eq!(collateral_haircut("BTC"), dec!(0.95));
    }

    #[test]
    fn test_eth_haircut() {
        assert_eq!(collateral_haircut("ETH"), dec!(0.90));
    }

    #[test]
    fn test_major_alts_haircut() {
        for alt in &["SOL", "BNB", "XRP", "ADA", "AVAX", "DOT"] {
            assert_eq!(
                collateral_haircut(alt),
                dec!(0.85),
                "{alt} should have 0.85 haircut"
            );
        }
    }

    #[test]
    fn test_exc_haircut_is_zero_anti_ftt() {
        assert_eq!(
            collateral_haircut("EXC"),
            dec!(0.00),
            "EXC must have zero haircut (anti-FTT policy)"
        );
    }

    #[test]
    fn test_unknown_asset_gets_conservative_haircut() {
        assert_eq!(collateral_haircut("SHIB"), dec!(0.80));
        assert_eq!(collateral_haircut("DOGE"), dec!(0.80));
    }

    // ── equity calculation ───────────────────────────────────────────────────

    #[test]
    fn test_usdt_only_equity() {
        let user_id = Uuid::new_v4();
        let mut account = UnifiedAccount::new(user_id, AccountMode::Unified);
        account.set_wallet_balance("USDT", dec!(10000), Decimal::ZERO, Decimal::ZERO);
        account.calculate_equity(&usdt_prices(), Decimal::ZERO, Decimal::ZERO);

        // 10000 USDT * 1.00 price * 1.00 haircut = 10000 collateral
        assert_eq!(account.total_collateral_usd, dec!(10000));
        assert_eq!(account.total_equity_usd, dec!(10000));
        assert_eq!(account.available_balance_usd, dec!(10000));
    }

    #[test]
    fn test_btc_collateral_with_haircut() {
        let user_id = Uuid::new_v4();
        let mut account = UnifiedAccount::new(user_id, AccountMode::Unified);
        // 1 BTC at $50,000 with 0.95 haircut → $47,500 collateral
        account.set_wallet_balance("BTC", dec!(1), Decimal::ZERO, Decimal::ZERO);
        account.calculate_equity(&usdt_prices(), Decimal::ZERO, Decimal::ZERO);

        assert_eq!(account.total_collateral_usd, dec!(47500));
    }

    #[test]
    fn test_eth_collateral_with_haircut() {
        let user_id = Uuid::new_v4();
        let mut account = UnifiedAccount::new(user_id, AccountMode::Unified);
        // 10 ETH at $3,000 with 0.90 haircut → $27,000 collateral
        account.set_wallet_balance("ETH", dec!(10), Decimal::ZERO, Decimal::ZERO);
        account.calculate_equity(&usdt_prices(), Decimal::ZERO, Decimal::ZERO);

        assert_eq!(account.total_collateral_usd, dec!(27000));
    }

    #[test]
    fn test_exc_excluded_from_collateral() {
        let user_id = Uuid::new_v4();
        let mut account = UnifiedAccount::new(user_id, AccountMode::Unified);
        // 1000 EXC at $5 each, but haircut = 0 → $0 collateral
        account.set_wallet_balance("EXC", dec!(1000), Decimal::ZERO, Decimal::ZERO);
        account.calculate_equity(&usdt_prices(), Decimal::ZERO, Decimal::ZERO);

        assert_eq!(
            account.total_collateral_usd,
            dec!(0),
            "EXC must contribute zero collateral (anti-FTT)"
        );
    }

    #[test]
    fn test_multi_asset_equity_sums_correctly() {
        let user_id = Uuid::new_v4();
        let mut account = UnifiedAccount::new(user_id, AccountMode::Unified);
        // 5000 USDT → 5000 collateral
        // 1 BTC at 50000 * 0.95 → 47500 collateral
        // Total = 52500
        account.set_wallet_balance("USDT", dec!(5000), Decimal::ZERO, Decimal::ZERO);
        account.set_wallet_balance("BTC", dec!(1), Decimal::ZERO, Decimal::ZERO);
        account.calculate_equity(&usdt_prices(), Decimal::ZERO, Decimal::ZERO);

        assert_eq!(account.total_collateral_usd, dec!(52500));
    }

    // ── available balance ────────────────────────────────────────────────────

    #[test]
    fn test_available_balance_reduces_with_margin() {
        let user_id = Uuid::new_v4();
        let mut account = UnifiedAccount::new(user_id, AccountMode::Unified);
        account.set_wallet_balance("USDT", dec!(10000), Decimal::ZERO, Decimal::ZERO);
        // $3,000 initial margin locked in positions
        account.calculate_equity(&usdt_prices(), dec!(3000), dec!(1500));

        assert_eq!(account.total_collateral_usd, dec!(10000));
        assert_eq!(account.available_balance_usd, dec!(7000)); // 10000 - 3000
    }

    #[test]
    fn test_available_balance_reduces_with_locked_orders() {
        let user_id = Uuid::new_v4();
        let mut account = UnifiedAccount::new(user_id, AccountMode::Unified);
        // 10000 USDT, 2000 locked in open spot orders
        account.set_wallet_balance("USDT", dec!(10000), dec!(2000), Decimal::ZERO);
        account.calculate_equity(&usdt_prices(), Decimal::ZERO, Decimal::ZERO);

        // collateral = 10000 (wallet_balance+unrealized)*haircut is based on equity
        // locked orders reduce available: 10000 - 0 (margin) - 2000 = 8000
        assert_eq!(account.available_balance_usd, dec!(8000));
    }

    #[test]
    fn test_available_balance_never_negative() {
        let user_id = Uuid::new_v4();
        let mut account = UnifiedAccount::new(user_id, AccountMode::Unified);
        account.set_wallet_balance("USDT", dec!(1000), Decimal::ZERO, Decimal::ZERO);
        // $5000 initial margin (more than available)
        account.calculate_equity(&usdt_prices(), dec!(5000), dec!(2500));

        assert_eq!(account.available_balance_usd, Decimal::ZERO);
    }

    // ── position checks ──────────────────────────────────────────────────────

    #[test]
    fn test_can_open_position_within_available() {
        let user_id = Uuid::new_v4();
        let mut account = UnifiedAccount::new(user_id, AccountMode::Unified);
        account.set_wallet_balance("USDT", dec!(10000), Decimal::ZERO, Decimal::ZERO);
        account.calculate_equity(&usdt_prices(), Decimal::ZERO, Decimal::ZERO);

        assert!(account.can_open_position(dec!(5000)));
        assert!(account.can_open_position(dec!(10000)));
        assert!(!account.can_open_position(dec!(10001)));
    }

    // ── liquidation ──────────────────────────────────────────────────────────

    #[test]
    fn test_not_liquidatable_when_no_positions() {
        let user_id = Uuid::new_v4();
        let mut account = UnifiedAccount::new(user_id, AccountMode::Unified);
        account.set_wallet_balance("USDT", dec!(10000), Decimal::ZERO, Decimal::ZERO);
        account.calculate_equity(&usdt_prices(), Decimal::ZERO, Decimal::ZERO);

        assert!(!account.is_liquidatable());
    }

    #[test]
    fn test_not_liquidatable_when_equity_exceeds_maintenance() {
        let user_id = Uuid::new_v4();
        let mut account = UnifiedAccount::new(user_id, AccountMode::Unified);
        account.set_wallet_balance("USDT", dec!(10000), Decimal::ZERO, Decimal::ZERO);
        // maintenance margin = $500, equity = $10,000 → safe
        account.calculate_equity(&usdt_prices(), dec!(5000), dec!(500));

        assert!(!account.is_liquidatable());
    }

    #[test]
    fn test_liquidatable_when_equity_at_maintenance() {
        let user_id = Uuid::new_v4();
        let mut account = UnifiedAccount::new(user_id, AccountMode::Unified);
        // 1 BTC at 50000 with large unrealized loss
        account.set_wallet_balance("USDT", dec!(500), Decimal::ZERO, Decimal::ZERO);
        // equity ($500) <= maintenance ($500) → liquidatable
        account.calculate_equity(&usdt_prices(), dec!(1000), dec!(500));

        assert!(account.is_liquidatable());
    }

    #[test]
    fn test_liquidatable_when_equity_below_maintenance() {
        let user_id = Uuid::new_v4();
        let mut account = UnifiedAccount::new(user_id, AccountMode::Unified);
        account.set_wallet_balance("USDT", dec!(200), Decimal::ZERO, dec!(-300));
        // equity = 200 + (-300 * 1 price) ... negative net
        account.calculate_equity(&usdt_prices(), dec!(1000), dec!(300));

        // total_equity_usd = 200 (wallet_balance * price, since equity+pnl=-100 which is negative, collateral=0)
        // total_maintenance = 300 → liquidatable when equity <= maintenance
        // The key is that if equity <= maintenance, liquidation fires.
        assert!(account.is_liquidatable());
    }

    // ── margin ratio ─────────────────────────────────────────────────────────

    #[test]
    fn test_margin_ratio_zero_when_no_positions() {
        let user_id = Uuid::new_v4();
        let mut account = UnifiedAccount::new(user_id, AccountMode::Unified);
        account.set_wallet_balance("USDT", dec!(10000), Decimal::ZERO, Decimal::ZERO);
        account.calculate_equity(&usdt_prices(), Decimal::ZERO, Decimal::ZERO);

        assert_eq!(account.margin_ratio(), Decimal::ZERO);
    }

    #[test]
    fn test_margin_ratio_half_utilized() {
        let user_id = Uuid::new_v4();
        let mut account = UnifiedAccount::new(user_id, AccountMode::Unified);
        account.set_wallet_balance("USDT", dec!(10000), Decimal::ZERO, Decimal::ZERO);
        // 5000 initial margin out of 10000 collateral → 50% utilisation
        account.calculate_equity(&usdt_prices(), dec!(5000), dec!(2500));

        assert_eq!(account.margin_ratio(), dec!(0.5));
    }

    #[test]
    fn test_margin_ratio_zero_when_no_collateral() {
        let user_id = Uuid::new_v4();
        let account = UnifiedAccount::new(user_id, AccountMode::Unified);

        // No assets added — collateral is zero.
        assert_eq!(account.margin_ratio(), Decimal::ZERO);
    }

    // ── upgrade to unified ───────────────────────────────────────────────────

    #[test]
    fn test_upgrade_merges_all_balances() {
        let user_id = Uuid::new_v4();

        let mut spot = HashMap::new();
        spot.insert("USDT".to_string(), dec!(5000));
        spot.insert("BTC".to_string(), dec!(0.1));

        let mut futures = HashMap::new();
        futures.insert("USDT".to_string(), dec!(3000));

        let mut fund = HashMap::new();
        fund.insert("ETH".to_string(), dec!(2));

        let account = UnifiedAccount::upgrade_to_unified(user_id, &spot, &futures, &fund);

        assert_eq!(account.mode, AccountMode::Unified);
        assert_eq!(account.user_id, user_id);

        // USDT: 5000 (spot) + 3000 (futures) = 8000
        let usdt = account.assets.get("USDT").expect("USDT present");
        assert_eq!(usdt.wallet_balance, dec!(8000));

        // BTC: 0.1 from spot only
        let btc = account.assets.get("BTC").expect("BTC present");
        assert_eq!(btc.wallet_balance, dec!(0.1));

        // ETH: 2 from fund only
        let eth = account.assets.get("ETH").expect("ETH present");
        assert_eq!(eth.wallet_balance, dec!(2));
    }

    #[test]
    fn test_upgrade_with_empty_sources() {
        let user_id = Uuid::new_v4();

        let mut spot = HashMap::new();
        spot.insert("USDT".to_string(), dec!(1000));

        let account = UnifiedAccount::upgrade_to_unified(
            user_id,
            &spot,
            &HashMap::new(),
            &HashMap::new(),
        );

        assert_eq!(account.mode, AccountMode::Unified);
        assert_eq!(account.assets.len(), 1);
        let usdt = account.assets.get("USDT").expect("USDT present");
        assert_eq!(usdt.wallet_balance, dec!(1000));
    }

    #[test]
    fn test_upgrade_exc_present_but_zero_collateral() {
        let user_id = Uuid::new_v4();

        let mut spot = HashMap::new();
        spot.insert("EXC".to_string(), dec!(10000));
        spot.insert("USDT".to_string(), dec!(1000));

        let mut account = UnifiedAccount::upgrade_to_unified(
            user_id,
            &spot,
            &HashMap::new(),
            &HashMap::new(),
        );

        account.calculate_equity(&usdt_prices(), Decimal::ZERO, Decimal::ZERO);

        // EXC contributes nothing due to zero haircut.
        // Only USDT contributes: 1000 * 1 * 1.0 = 1000.
        assert_eq!(account.total_collateral_usd, dec!(1000));
    }
}
