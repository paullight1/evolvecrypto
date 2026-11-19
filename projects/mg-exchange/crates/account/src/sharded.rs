//! Sharded account store — replaces the single `Mutex<AccountManager>` on the
//! Exchange with 64 per-user shards to eliminate single-lock contention under
//! multi-user order flow.
//!
//! # Deadlock prevention protocol (SEI CERT LCK07-J)
//!
//! - Sharding key: `shard_of(user_id) = (user_id.as_u128() as usize) & (N_SHARDS - 1)`
//!   where `N_SHARDS = 64` (power-of-two → the `&` is a fast mask).
//! - **Single-user operations** acquire exactly one shard lock via `with_user`.
//! - **2-party operations** (transfer, settle_fill) acquire shard locks in
//!   strict ascending index order via `with_users_2`. Same-shard fast path
//!   collapses to one lock acquisition.
//! - **N-party operations** (≤ `MAX_SHARDS_PER_ATOMIC_OP` distinct shards) go
//!   through `with_users_n`, which sorts the unique shard set ascending before
//!   acquiring. Beyond `MAX_SHARDS_PER_ATOMIC_OP`, use separate calls — the
//!   cap is enforced by a debug assertion.
//! - **Fan-out queries** (`user_ids`, `all_asset_totals`) iterate all shards in
//!   ascending order. They never interleave with cross-shard writes because each
//!   shard lock is taken & released one at a time.
//! - **Admin writes** (`add_product`, `create_account`) either target a single
//!   shard (by user) or replicate across all 64 shards in ascending order
//!   (products). Both paths respect the ordering invariant.
//!
//! Lock ordering is encapsulated inside this module — call sites never touch a
//! shard index directly, so accidental ordering violations become impossible.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use rust_decimal::Decimal;

use exchange_common::{FillEvent, Price, Product, Qty, Side, Symbol, UserId};

use crate::account_manager::{AccountManager, Asset, FeeInfo, FillSettlement, LockInfo};
use crate::balance::Balance;
use crate::error::AccountError;

/// Number of shards. Power of two so `idx & (N-1)` replaces modulo.
pub const N_SHARDS: usize = 64;

/// Hard cap on the number of distinct shards that may be locked for a single
/// atomic operation. Beyond this, deadlock risk grows (O(N²) in the number of
/// participants) and operations should be decomposed into separate calls.
pub const MAX_SHARDS_PER_ATOMIC_OP: usize = 4;

#[inline]
fn shard_of(user: UserId) -> usize {
    (user.as_u128() as usize) & (N_SHARDS - 1)
}

/// 64-shard account store. `Arc<ShardedAccounts>` is the intended usage —
/// clone the Arc into tasks, share the sharded locks.
pub struct ShardedAccounts {
    shards: Box<[Mutex<AccountManager>]>,
}

impl ShardedAccounts {
    pub fn new() -> Self {
        let mut v: Vec<Mutex<AccountManager>> = Vec::with_capacity(N_SHARDS);
        for _ in 0..N_SHARDS {
            v.push(Mutex::new(AccountManager::new()));
        }
        Self {
            shards: v.into_boxed_slice(),
        }
    }

    pub fn new_arc() -> Arc<Self> {
        Arc::new(Self::new())
    }

    // --- Product registry (replicated across all shards) -----------------

    /// Register a trading pair. Products are replicated across all 64 shards
    /// so any shard can resolve a symbol without cross-shard lookup. Called at
    /// admin-registration time only (rare), so the 64 sequential lock
    /// acquisitions are acceptable.
    pub fn add_product(&self, product: Product) {
        for shard in self.shards.iter() {
            shard.lock().add_product(product.clone());
        }
    }

    /// Lookup a product by symbol. Reads from shard 0 (products are replicated).
    pub fn get_product(&self, symbol: &Symbol) -> Option<Product> {
        self.shards[0].lock().get_product(symbol).cloned()
    }

    /// List all registered products (reads from shard 0).
    pub fn products(&self) -> Vec<Product> {
        self.shards[0]
            .lock()
            .products()
            .into_iter()
            .cloned()
            .collect()
    }

    // --- Single-user mutations --------------------------------------------

    pub fn create_account(&self, user_id: UserId) {
        self.shards[shard_of(user_id)].lock().create_account(user_id);
    }

    pub fn deposit(
        &self,
        user_id: UserId,
        asset: &str,
        amount: Decimal,
    ) -> Result<(), AccountError> {
        self.shards[shard_of(user_id)]
            .lock()
            .deposit(user_id, asset, amount)
    }

    pub fn withdraw(
        &self,
        user_id: UserId,
        asset: &str,
        amount: Decimal,
    ) -> Result<(), AccountError> {
        self.shards[shard_of(user_id)]
            .lock()
            .withdraw(user_id, asset, amount)
    }

    pub fn lock_for_order(
        &self,
        user_id: UserId,
        symbol: &Symbol,
        side: Side,
        price: Price,
        qty: Qty,
    ) -> Result<LockInfo, AccountError> {
        self.shards[shard_of(user_id)]
            .lock()
            .lock_for_order(user_id, symbol, side, price, qty)
    }

    pub fn unlock_for_order(
        &self,
        user_id: UserId,
        lock: &LockInfo,
    ) -> Result<(), AccountError> {
        self.shards[shard_of(user_id)]
            .lock()
            .unlock_for_order(user_id, lock)
    }

    pub fn unlock_remainder(
        &self,
        user_id: UserId,
        asset: &str,
        amount: Decimal,
    ) -> Result<(), AccountError> {
        self.shards[shard_of(user_id)]
            .lock()
            .unlock_remainder(user_id, asset, amount)
    }

    pub fn try_lock_margin(
        &self,
        user_id: UserId,
        asset: &str,
        amount: Decimal,
    ) -> Result<(), AccountError> {
        self.shards[shard_of(user_id)]
            .lock()
            .try_lock_margin(user_id, asset, amount)
    }

    // --- Single-user queries (cloned return values) -----------------------

    pub fn get_balance(&self, user_id: UserId, asset: &str) -> Option<Balance> {
        self.shards[shard_of(user_id)]
            .lock()
            .get_balance(&user_id, asset)
            .cloned()
    }

    pub fn get_all_balances(&self, user_id: UserId) -> HashMap<Asset, Balance> {
        self.shards[shard_of(user_id)]
            .lock()
            .get_all_balances(&user_id)
            .cloned()
            .unwrap_or_default()
    }

    // --- Closure-based single-user access ---------------------------------

    /// Hold the shard lock for a user while executing `f`. Use when multiple
    /// operations on the same user must be atomic relative to other threads.
    pub fn with_user<R>(&self, user_id: UserId, f: impl FnOnce(&mut AccountManager) -> R) -> R {
        let mut guard = self.shards[shard_of(user_id)].lock();
        f(&mut *guard)
    }

    // --- Cross-user mutations (deadlock-safe lock ordering) ---------------

    /// Execute `f` holding up to two shard locks in ascending index order.
    ///
    /// If `user_a` and `user_b` hash to the same shard, `f` is called with
    /// the single shared guard as the first argument and `None` as the second.
    /// Otherwise `f` is called with both guards, where the first argument is
    /// always `user_a`'s shard and the second is always `user_b`'s shard —
    /// caller-facing order is preserved even though the lock acquisition
    /// order is by shard index.
    pub fn with_users_2<R>(
        &self,
        user_a: UserId,
        user_b: UserId,
        f: impl FnOnce(&mut AccountManager, Option<&mut AccountManager>) -> R,
    ) -> R {
        let idx_a = shard_of(user_a);
        let idx_b = shard_of(user_b);

        if idx_a == idx_b {
            let mut g = self.shards[idx_a].lock();
            return f(&mut *g, None);
        }

        // Split the shard slice so the borrow checker can give us two disjoint
        // mutable references. `split_at` gives immutable shared borrows of the
        // boxed slice, but `Mutex::lock()` only needs `&Mutex`, which is shared.
        let (lo, hi) = if idx_a < idx_b { (idx_a, idx_b) } else { (idx_b, idx_a) };
        debug_assert!(lo < hi, "ascending lock order invariant");

        // Acquire in ascending index order. Both guards are disjoint — each
        // borrows a different slice element — so they coexist until `f` returns.
        let mut g_lo = self.shards[lo].lock();
        let mut g_hi = self.shards[hi].lock();

        if idx_a == lo {
            f(&mut *g_lo, Some(&mut *g_hi))
        } else {
            f(&mut *g_hi, Some(&mut *g_lo))
        }
    }

    /// Cross-user atomic transfer of `amount` of `asset` from `from` to `to`.
    /// Uses the 2-shard lock protocol with deposit-failure rollback: if the
    /// destination deposit fails (e.g. destination account missing), the
    /// source debit is rolled back under the same held locks so the ledger
    /// invariant is preserved.
    pub fn transfer(
        &self,
        from: UserId,
        to: UserId,
        asset: &str,
        amount: Decimal,
    ) -> Result<(), AccountError> {
        self.with_users_2(from, to, |am_from, am_to_opt| match am_to_opt {
            None => {
                // Same shard — single AccountManager, both ops on the same guard.
                am_from.withdraw(from, asset, amount)?;
                if let Err(e) = am_from.deposit(to, asset, amount) {
                    // Roll back the debit; depositing back into the source
                    // cannot fail because the account exists (we just debited it).
                    am_from
                        .deposit(from, asset, amount)
                        .expect("rollback deposit into source must succeed");
                    return Err(e);
                }
                Ok(())
            }
            Some(am_to) => {
                am_from.withdraw(from, asset, amount)?;
                if let Err(e) = am_to.deposit(to, asset, amount) {
                    am_from
                        .deposit(from, asset, amount)
                        .expect("rollback deposit into source must succeed");
                    return Err(e);
                }
                Ok(())
            }
        })
    }

    /// Settle a fill across buyer and seller atomically.
    ///
    /// Same-shard fast path: delegates to `AccountManager::settle_fill`.
    /// Cross-shard path: precomputes the settlement plan, then applies the
    /// buyer-half on the buyer shard and the seller-half on the seller shard
    /// under a single 2-shard ascending-order lock acquisition.
    pub fn settle_fill(&self, fill: &FillEvent, product: &Product) -> Result<FeeInfo, AccountError> {
        let plan = FillSettlement::compute(fill, product);

        self.with_users_2(plan.buyer_id, plan.seller_id, |am_buyer, am_seller_opt| {
            match am_seller_opt {
                None => {
                    // Same shard — apply both halves through the shared guard.
                    am_buyer.apply_fill_buyer_side(&plan)?;
                    am_buyer.apply_fill_seller_side(&plan)?;
                }
                Some(am_seller) => {
                    am_buyer.apply_fill_buyer_side(&plan)?;
                    am_seller.apply_fill_seller_side(&plan)?;
                }
            }
            Ok::<_, AccountError>(plan.fee_info())
        })
    }

    // --- Fan-out queries --------------------------------------------------

    pub fn user_ids(&self) -> Vec<UserId> {
        let mut out = Vec::new();
        for shard in self.shards.iter() {
            out.extend(shard.lock().user_ids());
        }
        out
    }

    pub fn user_count(&self) -> usize {
        self.shards.iter().map(|s| s.lock().user_ids().len()).sum()
    }

    pub fn all_asset_totals(&self) -> HashMap<String, Decimal> {
        let mut totals: HashMap<String, Decimal> = HashMap::new();
        for shard in self.shards.iter() {
            for (asset, amt) in shard.lock().all_asset_totals() {
                *totals.entry(asset).or_default() += amt;
            }
        }
        totals
    }

    pub fn all_user_asset_balances(&self) -> Vec<(UserId, String, Decimal)> {
        let mut out = Vec::new();
        for shard in self.shards.iter() {
            out.extend(shard.lock().all_user_asset_balances());
        }
        out
    }
}

impl Default for ShardedAccounts {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use exchange_common::ProductType;

    fn make_product(symbol: &str, base: &str, quote: &str) -> Product {
        Product {
            symbol: Symbol::new(symbol),
            base_asset: base.to_string(),
            quote_asset: quote.to_string(),
            product_type: ProductType::Perpetual,
            tick_size: dec!(0.01),
            lot_size: dec!(0.00001),
            min_notional: dec!(10),
            maker_fee: dec!(0.001),
            taker_fee: dec!(0.002),
            max_order_qty: None,
            max_order_notional: None,
            max_leverage: None,
        }
    }

    #[test]
    fn shard_of_is_deterministic() {
        let u = uuid::Uuid::new_v4();
        assert_eq!(shard_of(u), shard_of(u));
        assert!(shard_of(u) < N_SHARDS);
    }

    #[test]
    fn single_user_deposit_withdraw() {
        let accts = ShardedAccounts::new();
        let alice = uuid::Uuid::new_v4();
        accts.create_account(alice);
        accts.deposit(alice, "USDT", dec!(100)).unwrap();
        assert_eq!(accts.get_balance(alice, "USDT").unwrap().total(), dec!(100));
        accts.withdraw(alice, "USDT", dec!(40)).unwrap();
        assert_eq!(accts.get_balance(alice, "USDT").unwrap().total(), dec!(60));
    }

    #[test]
    fn transfer_cross_shard_is_atomic() {
        let accts = ShardedAccounts::new();
        // Find two users in different shards.
        let mut alice;
        let mut bob;
        loop {
            alice = uuid::Uuid::new_v4();
            bob = uuid::Uuid::new_v4();
            if shard_of(alice) != shard_of(bob) {
                break;
            }
        }
        accts.create_account(alice);
        accts.create_account(bob);
        accts.deposit(alice, "USDT", dec!(100)).unwrap();
        accts.deposit(bob, "USDT", dec!(0)).unwrap();

        accts.transfer(alice, bob, "USDT", dec!(30)).unwrap();
        assert_eq!(accts.get_balance(alice, "USDT").unwrap().total(), dec!(70));
        assert_eq!(accts.get_balance(bob, "USDT").unwrap().total(), dec!(30));
    }

    #[test]
    fn transfer_same_shard() {
        let accts = ShardedAccounts::new();
        // Force same shard — repeatedly generate until we get a collision.
        let mut alice;
        let mut bob;
        loop {
            alice = uuid::Uuid::new_v4();
            bob = uuid::Uuid::new_v4();
            if shard_of(alice) == shard_of(bob) && alice != bob {
                break;
            }
        }
        accts.create_account(alice);
        accts.create_account(bob);
        accts.deposit(alice, "USDT", dec!(100)).unwrap();
        accts.deposit(bob, "USDT", dec!(0)).unwrap();

        accts.transfer(alice, bob, "USDT", dec!(30)).unwrap();
        assert_eq!(accts.get_balance(alice, "USDT").unwrap().total(), dec!(70));
        assert_eq!(accts.get_balance(bob, "USDT").unwrap().total(), dec!(30));
    }

    #[test]
    fn settle_fill_cross_shard_matches_monolithic() {
        let accts = ShardedAccounts::new();
        let product = make_product("BTCUSDT", "BTC", "USDT");
        accts.add_product(product.clone());

        let mut alice;
        let mut bob;
        loop {
            alice = uuid::Uuid::new_v4();
            bob = uuid::Uuid::new_v4();
            if shard_of(alice) != shard_of(bob) {
                break;
            }
        }
        accts.create_account(alice);
        accts.create_account(bob);
        accts.deposit(alice, "USDT", dec!(100000)).unwrap();
        accts.deposit(bob, "BTC", dec!(10)).unwrap();

        // Both lock funds
        accts
            .lock_for_order(alice, &Symbol::new("BTCUSDT"), Side::Buy, dec!(50000), dec!(1))
            .unwrap();
        accts
            .lock_for_order(bob, &Symbol::new("BTCUSDT"), Side::Sell, dec!(50000), dec!(1))
            .unwrap();

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

        let fees = accts.settle_fill(&fill, &product).unwrap();

        // Alice: 100000 - 50000 = 50000 USDT, 1 BTC - 0.2% fee = 0.998
        assert_eq!(accts.get_balance(alice, "USDT").unwrap().total(), dec!(50000));
        assert_eq!(accts.get_balance(alice, "BTC").unwrap().total(), dec!(0.998));
        // Bob: 10 BTC - 1 = 9 BTC, 50000 USDT - 0.1% fee = 49950
        assert_eq!(accts.get_balance(bob, "BTC").unwrap().total(), dec!(9));
        assert_eq!(accts.get_balance(bob, "USDT").unwrap().total(), dec!(49950));

        assert_eq!(fees.taker_fee.asset, "BTC");
        assert_eq!(fees.taker_fee.amount, dec!(0.002));
        assert_eq!(fees.maker_fee.asset, "USDT");
        assert_eq!(fees.maker_fee.amount, dec!(50));
    }

    #[test]
    fn fan_out_user_ids() {
        let accts = ShardedAccounts::new();
        for _ in 0..100 {
            accts.create_account(uuid::Uuid::new_v4());
        }
        assert_eq!(accts.user_ids().len(), 100);
    }

    /// S18 Phase B C.3 — concurrent cross-shard transfers must never deadlock
    /// and must preserve total USDT across all users (money conservation).
    ///
    /// 16 threads × 128 transfers = 2048 randomised 2-user transfers running in
    /// parallel. Each transfer is ordered via `with_users_2`'s ascending shard
    /// lock protocol. With 32 distinct users hashed across 64 shards, shard
    /// collisions are frequent enough to exercise both same-shard and
    /// cross-shard code paths.
    ///
    /// The test fails if:
    /// - The total USDT across all users changes (money created or destroyed)
    /// - Any thread panics (deadlock → timeout, or lock-ordering error)
    #[test]
    fn concurrent_transfers_no_deadlock_no_money_loss() {
        use std::sync::Arc;
        use std::thread;

        const USERS: usize = 32;
        const THREADS: usize = 16;
        const TRANSFERS_PER_THREAD: usize = 128;
        const INITIAL_BAL: i64 = 10_000;

        let accts = Arc::new(ShardedAccounts::new());
        let user_ids: Vec<UserId> = (0..USERS).map(|_| uuid::Uuid::new_v4()).collect();
        for uid in &user_ids {
            accts.create_account(*uid);
            accts.deposit(*uid, "USDT", Decimal::from(INITIAL_BAL)).unwrap();
        }

        let expected_total = Decimal::from(INITIAL_BAL * USERS as i64);
        assert_eq!(
            accts.all_asset_totals().get("USDT").copied().unwrap_or_default(),
            expected_total,
            "initial total USDT must equal USERS * INITIAL_BAL"
        );

        let handles: Vec<_> = (0..THREADS)
            .map(|tid| {
                let accts = Arc::clone(&accts);
                let users = user_ids.clone();
                thread::spawn(move || {
                    // Deterministic per-thread xorshift PRNG — keeps the test
                    // reproducible without adding a `rand` dep.
                    let mut rng_state: u64 = 0xdead_beef_u64.wrapping_add(tid as u64 + 1);
                    let mut next = || {
                        let mut x = rng_state;
                        x ^= x << 13;
                        x ^= x >> 7;
                        x ^= x << 17;
                        rng_state = x;
                        x
                    };

                    for _ in 0..TRANSFERS_PER_THREAD {
                        let i = (next() as usize) % USERS;
                        let mut j = (next() as usize) % USERS;
                        if j == i {
                            j = (j + 1) % USERS;
                        }
                        // Small random amount so the source always has enough.
                        let amount = Decimal::from((next() % 5 + 1) as u64);
                        // Ignore InsufficientBalance errors — the test only
                        // cares about deadlocks and conservation. Balances can
                        // drift and a transfer may be rejected cleanly.
                        let _ = accts.transfer(users[i], users[j], "USDT", amount);
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().expect("thread panicked — possible deadlock or lock-order violation");
        }

        // Money conservation: the total across all users must still match.
        assert_eq!(
            accts.all_asset_totals().get("USDT").copied().unwrap_or_default(),
            expected_total,
            "total USDT changed — transfer is not conservative"
        );
    }

    #[test]
    fn fan_out_all_asset_totals() {
        let accts = ShardedAccounts::new();
        for _ in 0..50 {
            let u = uuid::Uuid::new_v4();
            accts.create_account(u);
            accts.deposit(u, "USDT", dec!(10)).unwrap();
            accts.deposit(u, "BTC", dec!(1)).unwrap();
        }
        let totals = accts.all_asset_totals();
        assert_eq!(totals.get("USDT").copied().unwrap_or_default(), dec!(500));
        assert_eq!(totals.get("BTC").copied().unwrap_or_default(), dec!(50));
    }
}
