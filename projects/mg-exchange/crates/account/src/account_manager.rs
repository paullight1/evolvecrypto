use std::collections::HashMap;

use exchange_common::*;
use rust_decimal::Decimal;

use crate::balance::Balance;
use crate::error::AccountError;

/// Asset identifier (e.g., "BTC", "USDT", "ETH").
pub type Asset = String;

/// Manages all user accounts and their balances.
///
/// Phase 1: in-memory storage. Phase 2: PostgreSQL backend.
/// The API is designed to be async-ready — swap the impl, keep the interface.
///
/// All balance mutations go through this manager to maintain audit trail.
pub struct AccountManager {
    /// user_id → asset → balance
    balances: HashMap<UserId, HashMap<Asset, Balance>>,
    /// Product registry for resolving base/quote assets
    products: HashMap<Symbol, Product>,
}

impl AccountManager {
    pub fn new() -> Self {
        Self {
            balances: HashMap::new(),
            products: HashMap::new(),
        }
    }

    /// Register a trading pair.
    pub fn add_product(&mut self, product: Product) {
        self.products.insert(product.symbol.clone(), product);
    }

    /// Return all registered products.
    pub fn products(&self) -> Vec<&Product> {
        self.products.values().collect()
    }

    /// Create a new user account (idempotent).
    pub fn create_account(&mut self, user_id: UserId) {
        self.balances.entry(user_id).or_default();
    }

    /// Deposit funds into a user's account.
    pub fn deposit(
        &mut self,
        user_id: UserId,
        asset: &str,
        amount: Decimal,
    ) -> Result<(), AccountError> {
        let balances = self
            .balances
            .get_mut(&user_id)
            .ok_or(AccountError::AccountNotFound(user_id))?;

        balances
            .entry(asset.to_string())
            .or_insert_with(Balance::zero)
            .deposit(amount)
    }

    /// Withdraw funds from a user's account.
    pub fn withdraw(
        &mut self,
        user_id: UserId,
        asset: &str,
        amount: Decimal,
    ) -> Result<(), AccountError> {
        let balance = self
            .balances
            .get_mut(&user_id)
            .ok_or(AccountError::AccountNotFound(user_id))?
            .get_mut(asset)
            .ok_or_else(|| AccountError::AssetNotFound(asset.to_string()))?;

        balance.withdraw(amount)
    }

    /// Lock funds for a new order.
    ///
    /// - Buy order: locks `price * qty` in quote asset (e.g., USDT)
    /// - Sell order: locks `qty` in base asset (e.g., BTC)
    pub fn lock_for_order(
        &mut self,
        user_id: UserId,
        symbol: &Symbol,
        side: Side,
        price: Price,
        qty: Qty,
    ) -> Result<LockInfo, AccountError> {
        let product = self
            .products
            .get(symbol)
            .ok_or_else(|| AccountError::AssetNotFound(symbol.to_string()))?
            .clone();

        let (asset, amount) = match side {
            Side::Buy => (product.quote_asset.clone(), price * qty),
            Side::Sell => (product.base_asset.clone(), qty),
        };

        let balance = self
            .balances
            .get_mut(&user_id)
            .ok_or(AccountError::AccountNotFound(user_id))?
            .get_mut(&asset)
            .ok_or_else(|| AccountError::AssetNotFound(asset.clone()))?;

        balance.try_lock(amount)?;

        Ok(LockInfo { asset, amount })
    }

    /// Unlock funds when an order is cancelled or rejected.
    pub fn unlock_for_order(
        &mut self,
        user_id: UserId,
        lock: &LockInfo,
    ) -> Result<(), AccountError> {
        let balance = self
            .balances
            .get_mut(&user_id)
            .ok_or(AccountError::AccountNotFound(user_id))?
            .get_mut(&lock.asset)
            .ok_or_else(|| AccountError::AssetNotFound(lock.asset.clone()))?;

        balance.unlock(lock.amount)
    }

    /// Settle a fill: transfer assets between maker and taker.
    ///
    /// For a fill where taker is buying:
    ///   - Taker: deduct locked quote, credit base
    ///   - Maker: deduct locked base, credit quote
    ///
    /// Fees are deducted from the received asset.
    pub fn settle_fill(&mut self, fill: &FillEvent, product: &Product) -> Result<FeeInfo, AccountError> {
        let plan = FillSettlement::compute(fill, product);
        self.apply_fill_buyer_side(&plan)?;
        self.apply_fill_seller_side(&plan)?;
        Ok(plan.fee_info())
    }

    /// Apply the buyer-side half of a pre-computed fill settlement.
    ///
    /// Used by `ShardedAccounts::settle_fill` for the cross-shard path where
    /// buyer and seller live in different shard locks — this method only touches
    /// the buyer's balances. For the same-shard fast path, use `settle_fill`
    /// directly.
    pub(crate) fn apply_fill_buyer_side(&mut self, plan: &FillSettlement) -> Result<(), AccountError> {
        let buyer_balances = self
            .balances
            .get_mut(&plan.buyer_id)
            .ok_or(AccountError::AccountNotFound(plan.buyer_id))?;

        buyer_balances
            .get_mut(&plan.quote)
            .ok_or_else(|| AccountError::AssetNotFound(plan.quote.clone()))?
            .deduct_locked(plan.cost)?;

        let base_credit = plan.qty - plan.buyer_fee;
        buyer_balances
            .entry(plan.base.clone())
            .or_insert_with(Balance::zero)
            .credit(base_credit)?;

        Ok(())
    }

    /// Apply the seller-side half of a pre-computed fill settlement.
    pub(crate) fn apply_fill_seller_side(&mut self, plan: &FillSettlement) -> Result<(), AccountError> {
        let seller_balances = self
            .balances
            .get_mut(&plan.seller_id)
            .ok_or(AccountError::AccountNotFound(plan.seller_id))?;

        seller_balances
            .get_mut(&plan.base)
            .ok_or_else(|| AccountError::AssetNotFound(plan.base.clone()))?
            .deduct_locked(plan.qty)?;

        let quote_credit = plan.cost - plan.seller_fee;
        seller_balances
            .entry(plan.quote.clone())
            .or_insert_with(Balance::zero)
            .credit(quote_credit)?;

        Ok(())
    }

    /// Unlock remaining locked amount after partial fill + cancel.
    ///
    /// When an order is partially filled then cancelled, unlock:
    /// - Buy: (original_locked - total_cost_of_fills)
    /// - Sell: remaining_qty
    pub fn unlock_remainder(
        &mut self,
        user_id: UserId,
        asset: &str,
        amount: Decimal,
    ) -> Result<(), AccountError> {
        let balance = self
            .balances
            .get_mut(&user_id)
            .ok_or(AccountError::AccountNotFound(user_id))?
            .get_mut(asset)
            .ok_or_else(|| AccountError::AssetNotFound(asset.to_string()))?;

        balance.unlock(amount)
    }

    /// Atomically check and reserve margin for a futures order.
    ///
    /// Moves `amount` from available → locked in the user's `asset` balance.
    /// Both the check (`available >= amount`) and the deduction happen under the
    /// caller's existing `accounts` lock, which eliminates the TOCTOU race that
    /// would otherwise allow two concurrent orders to both pass a read-only check.
    ///
    /// Returns `Err` if the account or asset is not found, or if available balance
    /// is insufficient.
    pub fn try_lock_margin(
        &mut self,
        user_id: UserId,
        asset: &str,
        amount: Decimal,
    ) -> Result<(), AccountError> {
        let balance = self
            .balances
            .get_mut(&user_id)
            .ok_or(AccountError::AccountNotFound(user_id))?
            .get_mut(asset)
            .ok_or_else(|| AccountError::AssetNotFound(asset.to_string()))?;

        balance.try_lock(amount)
    }

    // --- Queries ---

    pub fn get_balance(&self, user_id: &UserId, asset: &str) -> Option<&Balance> {
        self.balances.get(user_id)?.get(asset)
    }

    pub fn get_all_balances(&self, user_id: &UserId) -> Option<&HashMap<Asset, Balance>> {
        self.balances.get(user_id)
    }

    pub fn get_product(&self, symbol: &Symbol) -> Option<&Product> {
        self.products.get(symbol)
    }

    /// Sum all user balances per asset — used for reconciliation and PoR.
    ///
    /// Returns a map of asset → total of all user `available + locked` balances.
    pub fn all_asset_totals(&self) -> HashMap<String, Decimal> {
        let mut totals: HashMap<String, Decimal> = HashMap::new();
        for user_balances in self.balances.values() {
            for (asset, balance) in user_balances {
                *totals.entry(asset.clone()).or_default() += balance.total();
            }
        }
        totals
    }

    /// Return all registered user IDs.
    pub fn user_ids(&self) -> Vec<UserId> {
        self.balances.keys().copied().collect()
    }

    /// Return all individual (user_id, asset, balance_total) tuples — used by PoR to build
    /// the per-user liability list for the Merkle tree.
    pub fn all_user_asset_balances(&self) -> Vec<(UserId, String, Decimal)> {
        let mut result = Vec::new();
        for (user_id, user_balances) in &self.balances {
            for (asset, balance) in user_balances {
                if !balance.total().is_zero() {
                    result.push((*user_id, asset.clone(), balance.total()));
                }
            }
        }
        result
    }
}

impl Default for AccountManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Info about locked funds for an order (needed for unlock on cancel).
#[derive(Debug, Clone)]
pub struct LockInfo {
    pub asset: Asset,
    pub amount: Decimal,
}

/// Fee collected from a single party.
#[derive(Debug, Clone)]
pub struct FeeEntry {
    pub asset: String,
    pub amount: Decimal,
}

/// Fees collected from a fill.
#[derive(Debug, Clone)]
pub struct FeeInfo {
    pub maker_fee: FeeEntry,
    pub taker_fee: FeeEntry,
}

/// Pre-computed settlement plan for a fill — extracted so cross-shard fill
/// settlement (in `ShardedAccounts::settle_fill`) can compute it once and
/// apply the buyer and seller halves independently on different shard locks.
///
/// Fields are intentionally `pub(crate)` — only the account crate touches this.
#[derive(Debug, Clone)]
pub(crate) struct FillSettlement {
    pub(crate) buyer_id: UserId,
    pub(crate) seller_id: UserId,
    pub(crate) maker_id: UserId,
    pub(crate) taker_id: UserId,
    pub(crate) base: Asset,
    pub(crate) quote: Asset,
    pub(crate) qty: Decimal,
    pub(crate) cost: Decimal,
    pub(crate) buyer_fee: Decimal,
    pub(crate) seller_fee: Decimal,
}

impl FillSettlement {
    pub(crate) fn compute(fill: &FillEvent, product: &Product) -> Self {
        let (buyer_id, seller_id) = match fill.taker_side {
            Side::Buy => (fill.taker_user_id, fill.maker_user_id),
            Side::Sell => (fill.maker_user_id, fill.taker_user_id),
        };

        let (maker_id, taker_id) = (fill.maker_user_id, fill.taker_user_id);
        let cost = fill.price * fill.qty;

        // Maker fee applied to what maker receives.
        // Taker fee applied to what taker receives.
        let (buyer_fee, seller_fee) = match fill.taker_side {
            Side::Buy => {
                // Taker is buyer → receives base, pays taker fee on base.
                // Maker is seller → receives quote, pays maker fee on quote.
                let taker_fee = fill.qty * product.taker_fee;
                let maker_fee = cost * product.maker_fee;
                (taker_fee, maker_fee)
            }
            Side::Sell => {
                // Taker is seller → receives quote, pays taker fee on quote.
                // Maker is buyer → receives base, pays maker fee on base.
                let taker_fee = cost * product.taker_fee;
                let maker_fee = fill.qty * product.maker_fee;
                (maker_fee, taker_fee)
            }
        };

        Self {
            buyer_id,
            seller_id,
            maker_id,
            taker_id,
            base: product.base_asset.clone(),
            quote: product.quote_asset.clone(),
            qty: fill.qty,
            cost,
            buyer_fee,
            seller_fee,
        }
    }

    pub(crate) fn fee_info(&self) -> FeeInfo {
        let maker_fee_collected = if self.maker_id == self.buyer_id {
            FeeEntry { asset: self.base.clone(), amount: self.buyer_fee }
        } else {
            FeeEntry { asset: self.quote.clone(), amount: self.seller_fee }
        };
        let taker_fee_collected = if self.taker_id == self.buyer_id {
            FeeEntry { asset: self.base.clone(), amount: self.buyer_fee }
        } else {
            FeeEntry { asset: self.quote.clone(), amount: self.seller_fee }
        };
        FeeInfo {
            maker_fee: maker_fee_collected,
            taker_fee: taker_fee_collected,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn setup() -> (AccountManager, UserId, UserId, Product) {
        let mut mgr = AccountManager::new();

        let product = Product {
            symbol: Symbol::new("BTCUSDT"),
            base_asset: "BTC".to_string(),
            quote_asset: "USDT".to_string(),
            product_type: ProductType::Perpetual,
            tick_size: dec!(0.01),
            lot_size: dec!(0.00001),
            min_notional: dec!(10),
            maker_fee: dec!(0.001),
            taker_fee: dec!(0.002),
            max_order_qty: None,
            max_order_notional: None,
            max_leverage: None,
        };
        mgr.add_product(product.clone());

        let alice = uuid::Uuid::new_v4();
        let bob = uuid::Uuid::new_v4();

        mgr.create_account(alice);
        mgr.create_account(bob);

        // Alice has USDT (buyer)
        mgr.deposit(alice, "USDT", dec!(100000)).unwrap();
        // Bob has BTC (seller)
        mgr.deposit(bob, "BTC", dec!(10)).unwrap();

        (mgr, alice, bob, product)
    }

    #[test]
    fn test_deposit_and_query() {
        let (mgr, alice, _, _) = setup();
        let bal = mgr.get_balance(&alice, "USDT").unwrap();
        assert_eq!(bal.total(), dec!(100000));
        assert_eq!(bal.available(), dec!(100000));
    }

    #[test]
    fn test_lock_for_buy_order() {
        let (mut mgr, alice, _, _) = setup();

        // Alice places buy 1 BTC @ 50000
        let lock = mgr
            .lock_for_order(alice, &Symbol::new("BTCUSDT"), Side::Buy, dec!(50000), dec!(1))
            .unwrap();

        assert_eq!(lock.asset, "USDT");
        assert_eq!(lock.amount, dec!(50000));

        let bal = mgr.get_balance(&alice, "USDT").unwrap();
        assert_eq!(bal.locked(), dec!(50000));
        assert_eq!(bal.available(), dec!(50000));
    }

    #[test]
    fn test_lock_for_sell_order() {
        let (mut mgr, _, bob, _) = setup();

        // Bob places sell 0.5 BTC
        let lock = mgr
            .lock_for_order(bob, &Symbol::new("BTCUSDT"), Side::Sell, dec!(50000), dec!(0.5))
            .unwrap();

        assert_eq!(lock.asset, "BTC");
        assert_eq!(lock.amount, dec!(0.5));

        let bal = mgr.get_balance(&bob, "BTC").unwrap();
        assert_eq!(bal.locked(), dec!(0.5));
        assert_eq!(bal.available(), dec!(9.5));
    }

    #[test]
    fn test_lock_insufficient_balance() {
        let (mut mgr, alice, _, _) = setup();

        // Alice only has 100000 USDT, tries to buy 3 BTC @ 50000 = 150000
        let err = mgr
            .lock_for_order(alice, &Symbol::new("BTCUSDT"), Side::Buy, dec!(50000), dec!(3))
            .unwrap_err();

        assert!(matches!(err, AccountError::InsufficientBalance { .. }));
    }

    #[test]
    fn test_unlock_on_cancel() {
        let (mut mgr, alice, _, _) = setup();

        let lock = mgr
            .lock_for_order(alice, &Symbol::new("BTCUSDT"), Side::Buy, dec!(50000), dec!(1))
            .unwrap();

        mgr.unlock_for_order(alice, &lock).unwrap();

        let bal = mgr.get_balance(&alice, "USDT").unwrap();
        assert_eq!(bal.locked(), dec!(0));
        assert_eq!(bal.available(), dec!(100000));
    }

    #[test]
    fn test_settle_fill() {
        let (mut mgr, alice, bob, product) = setup();

        // Lock funds
        mgr.lock_for_order(alice, &Symbol::new("BTCUSDT"), Side::Buy, dec!(50000), dec!(1))
            .unwrap();
        mgr.lock_for_order(bob, &Symbol::new("BTCUSDT"), Side::Sell, dec!(50000), dec!(1))
            .unwrap();

        // Fill: Alice buys 1 BTC from Bob at 50000
        let fill = FillEvent {
            trade_id: 1,
            symbol: Symbol::new("BTCUSDT"),
            price: dec!(50000),
            qty: dec!(1),
            maker_order_id: uuid::Uuid::new_v4(),
            taker_order_id: uuid::Uuid::new_v4(),
            maker_user_id: bob,    // Bob is maker (resting sell)
            taker_user_id: alice,  // Alice is taker (incoming buy)
            taker_side: Side::Buy,
            timestamp: chrono::Utc::now(),
            seq: 1,
        };

        let fees = mgr.settle_fill(&fill, &product).unwrap();

        // Alice (taker buyer):
        //   USDT: 100000 - 50000 = 50000 (deducted locked)
        //   BTC: 1.0 - 0.002 (taker fee 0.2%) = 0.998
        let alice_usdt = mgr.get_balance(&alice, "USDT").unwrap();
        assert_eq!(alice_usdt.total(), dec!(50000));
        assert_eq!(alice_usdt.locked(), dec!(0));

        let alice_btc = mgr.get_balance(&alice, "BTC").unwrap();
        assert_eq!(alice_btc.total(), dec!(0.998)); // 1.0 - 0.2% fee

        // Bob (maker seller):
        //   BTC: 10 - 1 = 9 (deducted locked)
        //   USDT: 50000 - 50 (maker fee 0.1%) = 49950
        let bob_btc = mgr.get_balance(&bob, "BTC").unwrap();
        assert_eq!(bob_btc.total(), dec!(9));
        assert_eq!(bob_btc.locked(), dec!(0));

        let bob_usdt = mgr.get_balance(&bob, "USDT").unwrap();
        assert_eq!(bob_usdt.total(), dec!(49950)); // 50000 - 0.1% fee

        // Fees collected
        assert_eq!(fees.taker_fee.asset, "BTC");
        assert_eq!(fees.taker_fee.amount, dec!(0.002));
        assert_eq!(fees.maker_fee.asset, "USDT");
        assert_eq!(fees.maker_fee.amount, dec!(50));
    }

    #[test]
    fn test_partial_fill_then_cancel_remainder() {
        let (mut mgr, alice, bob, product) = setup();

        // Alice buys 2 BTC @ 50000, lock 100000 USDT
        let alice_lock = mgr
            .lock_for_order(alice, &Symbol::new("BTCUSDT"), Side::Buy, dec!(50000), dec!(2))
            .unwrap();
        assert_eq!(alice_lock.amount, dec!(100000));

        // Bob sells 1 BTC @ 50000
        mgr.lock_for_order(bob, &Symbol::new("BTCUSDT"), Side::Sell, dec!(50000), dec!(1))
            .unwrap();

        // Partial fill: 1 BTC
        let fill = FillEvent {
            trade_id: 1,
            symbol: Symbol::new("BTCUSDT"),
            price: dec!(50000),
            qty: dec!(1),
            maker_order_id: uuid::Uuid::new_v4(),
            taker_order_id: uuid::Uuid::new_v4(),
            maker_user_id: bob,
            taker_user_id: alice,
            taker_side: Side::Buy,
            timestamp: chrono::Utc::now(),
            seq: 1,
        };
        mgr.settle_fill(&fill, &product).unwrap();

        // Alice: 100000 - 50000 = 50000 total USDT, 50000 still locked (for remaining 1 BTC)
        let alice_usdt = mgr.get_balance(&alice, "USDT").unwrap();
        assert_eq!(alice_usdt.total(), dec!(50000));
        assert_eq!(alice_usdt.locked(), dec!(50000));
        assert_eq!(alice_usdt.available(), dec!(0));

        // Cancel remainder: unlock 50000 USDT (1 BTC * 50000)
        mgr.unlock_remainder(alice, "USDT", dec!(50000)).unwrap();

        let alice_usdt = mgr.get_balance(&alice, "USDT").unwrap();
        assert_eq!(alice_usdt.total(), dec!(50000));
        assert_eq!(alice_usdt.locked(), dec!(0));
        assert_eq!(alice_usdt.available(), dec!(50000));
    }

    #[test]
    fn test_withdraw() {
        let (mut mgr, alice, _, _) = setup();

        mgr.withdraw(alice, "USDT", dec!(30000)).unwrap();
        let bal = mgr.get_balance(&alice, "USDT").unwrap();
        assert_eq!(bal.total(), dec!(70000));
    }

    #[test]
    fn test_withdraw_respects_locked() {
        let (mut mgr, alice, _, _) = setup();

        mgr.lock_for_order(alice, &Symbol::new("BTCUSDT"), Side::Buy, dec!(50000), dec!(1.5))
            .unwrap();

        // 100000 total, 75000 locked, 25000 available
        let err = mgr.withdraw(alice, "USDT", dec!(30000)).unwrap_err();
        assert!(matches!(err, AccountError::InsufficientBalance { .. }));

        mgr.withdraw(alice, "USDT", dec!(25000)).unwrap();
    }

    #[test]
    fn test_nonexistent_account() {
        let mgr = AccountManager::new();
        let fake_id = uuid::Uuid::new_v4();
        assert!(mgr.get_balance(&fake_id, "BTC").is_none());
    }
}
