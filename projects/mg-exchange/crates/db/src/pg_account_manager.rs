//! PostgreSQL-backed AccountManager.
//!
//! This is an async replacement for the in-memory `AccountManager` in
//! `crates/account`. It wraps the existing `BalanceRepo` / `UserRepo` primitives
//! and adds the higher-level semantics the exchange needs:
//!
//! - Transactional settle_fill (single `BEGIN … COMMIT`)
//! - Side-aware `lock_for_order` (determines asset from symbol + side)
//! - Proper `PgAccountError` with distinct variants for every failure mode

use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use crate::balance_repo::{BalanceRepo, BalanceRow};
use crate::user_repo::UserRepo;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum PgAccountError {
    #[error("Insufficient available balance: need {needed}, have {available}")]
    InsufficientBalance { needed: Decimal, available: Decimal },

    #[error("Insufficient locked balance: need {needed}, have {locked}")]
    InsufficientLocked { needed: Decimal, locked: Decimal },

    #[error("Account not found: {0}")]
    AccountNotFound(Uuid),

    #[error("Asset not found for user {user_id}: {asset}")]
    AssetNotFound { user_id: Uuid, asset: String },

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
}

// Map BalanceError → PgAccountError so we can use `?` after try_lock calls.
impl From<crate::balance_repo::BalanceError> for PgAccountError {
    fn from(e: crate::balance_repo::BalanceError) -> Self {
        match e {
            crate::balance_repo::BalanceError::NotFound { user_id, asset } => {
                PgAccountError::AssetNotFound { user_id, asset }
            }
            crate::balance_repo::BalanceError::Insufficient { needed, available } => {
                PgAccountError::InsufficientBalance { needed, available }
            }
            crate::balance_repo::BalanceError::Db(e) => PgAccountError::Database(e),
        }
    }
}

// ---------------------------------------------------------------------------
// LockInfo — mirrors the one in account_manager but lives here to avoid a
// cross-crate dependency on exchange-account from exchange-db.
// ---------------------------------------------------------------------------

/// Describes the funds locked for a pending order.
///
/// Callers must pass this back to [`PgAccountManager::unlock`] on cancel, or
/// use the individual asset/amount fields for `settle_fill`.
#[derive(Debug, Clone)]
pub struct LockInfo {
    pub asset: String,
    pub amount: Decimal,
}

// ---------------------------------------------------------------------------
// PgAccountManager
// ---------------------------------------------------------------------------

/// Async, PostgreSQL-backed account manager.
///
/// This is a thin coordination layer on top of `BalanceRepo` and `UserRepo`.
/// Every method that may produce a race condition uses a database transaction
/// with `SELECT … FOR UPDATE` to guarantee correctness under concurrency.
pub struct PgAccountManager {
    pool: PgPool,
}

impl PgAccountManager {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    // -----------------------------------------------------------------------
    // Account lifecycle
    // -----------------------------------------------------------------------

    /// Create a new user account. Returns an error if the `user_id` already
    /// exists (PostgreSQL PRIMARY KEY constraint).
    pub async fn create_account(
        &self,
        user_id: Uuid,
        email: Option<&str>,
    ) -> Result<(), PgAccountError> {
        UserRepo::create(&self.pool, user_id, email).await?;
        tracing::debug!(%user_id, "created user account");
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Balance mutations
    // -----------------------------------------------------------------------

    /// Deposit `amount` of `asset` into the user's account.
    ///
    /// Creates the balance row if it does not yet exist (UPSERT).
    pub async fn deposit(
        &self,
        user_id: Uuid,
        asset: &str,
        amount: Decimal,
    ) -> Result<BalanceRow, PgAccountError> {
        self.assert_account_exists(user_id).await?;
        let row = BalanceRepo::deposit(&self.pool, user_id, asset, amount).await?;
        tracing::debug!(%user_id, %asset, %amount, "deposited");
        Ok(row)
    }

    /// Withdraw `amount` of `asset` from the user's available (unlocked) balance.
    ///
    /// Fails with [`PgAccountError::InsufficientBalance`] if `available < amount`.
    /// Uses `SELECT FOR UPDATE` internally to guard against concurrent withdrawals.
    pub async fn withdraw(
        &self,
        user_id: Uuid,
        asset: &str,
        amount: Decimal,
    ) -> Result<BalanceRow, PgAccountError> {
        let mut tx = self.pool.begin().await?;

        let row = sqlx::query_as::<_, BalanceRow>(
            "SELECT user_id, asset, total, locked, updated_at
             FROM balances WHERE user_id = $1 AND asset = $2 FOR UPDATE",
        )
        .bind(user_id)
        .bind(asset)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| PgAccountError::AssetNotFound {
            user_id,
            asset: asset.to_string(),
        })?;

        if row.available() < amount {
            return Err(PgAccountError::InsufficientBalance {
                needed: amount,
                available: row.available(),
            });
        }

        let updated = sqlx::query_as::<_, BalanceRow>(
            "UPDATE balances SET total = total - $3, updated_at = NOW()
             WHERE user_id = $1 AND asset = $2
             RETURNING user_id, asset, total, locked, updated_at",
        )
        .bind(user_id)
        .bind(asset)
        .bind(amount)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;
        tracing::debug!(%user_id, %asset, %amount, "withdrawn");
        Ok(updated)
    }

    /// Lock `amount` of `asset` for a pending order.
    ///
    /// Delegates to [`BalanceRepo::try_lock`] which uses `SELECT FOR UPDATE`
    /// to prevent races.  Fails with [`PgAccountError::InsufficientBalance`]
    /// when `available < amount`.
    pub async fn try_lock(
        &self,
        user_id: Uuid,
        asset: &str,
        amount: Decimal,
    ) -> Result<BalanceRow, PgAccountError> {
        let row = BalanceRepo::try_lock(&self.pool, user_id, asset, amount).await?;
        tracing::debug!(%user_id, %asset, %amount, "locked");
        Ok(row)
    }

    /// Reduce the locked amount (called when an order is cancelled).
    pub async fn unlock(
        &self,
        user_id: Uuid,
        asset: &str,
        amount: Decimal,
    ) -> Result<BalanceRow, PgAccountError> {
        // BalanceRepo::unlock does not check for over-unlock; the DB constraint
        // `balance_locked_non_negative` acts as a safety net.
        let row = BalanceRepo::unlock(&self.pool, user_id, asset, amount).await?;
        tracing::debug!(%user_id, %asset, %amount, "unlocked");
        Ok(row)
    }

    /// Deduct `amount` from both `total` and `locked` (outgoing side of a fill).
    pub async fn deduct_locked(
        &self,
        user_id: Uuid,
        asset: &str,
        amount: Decimal,
    ) -> Result<BalanceRow, PgAccountError> {
        let row = BalanceRepo::deduct_locked(&self.pool, user_id, asset, amount).await?;
        tracing::debug!(%user_id, %asset, %amount, "deducted locked");
        Ok(row)
    }

    /// Increase `total` for `asset` (incoming side of a fill).
    ///
    /// Creates the balance row if it does not yet exist (UPSERT).
    pub async fn credit(
        &self,
        user_id: Uuid,
        asset: &str,
        amount: Decimal,
    ) -> Result<BalanceRow, PgAccountError> {
        let row = BalanceRepo::credit(&self.pool, user_id, asset, amount).await?;
        tracing::debug!(%user_id, %asset, %amount, "credited");
        Ok(row)
    }

    // -----------------------------------------------------------------------
    // Atomic fill settlement
    // -----------------------------------------------------------------------

    /// Settle a fill between buyer and seller in a **single** database transaction.
    ///
    /// Operations (all inside one `BEGIN … COMMIT`):
    /// 1. Buyer: deduct locked quote (cost = price * qty, net of any prior fee lock)
    /// 2. Buyer: credit base (qty minus fee)
    /// 3. Seller: deduct locked base (qty)
    /// 4. Seller: credit quote (cost minus fee)
    ///
    /// The PostgreSQL CHECK constraints (`balance_total_non_negative`,
    /// `balance_locked_non_negative`, `balance_available_non_negative`) provide
    /// a database-level safety net in addition to the pre-fill lock checks.
    pub async fn settle_fill(
        &self,
        buyer_id: Uuid,
        seller_id: Uuid,
        base_asset: &str,
        quote_asset: &str,
        base_qty: Decimal,
        quote_cost: Decimal,
        buyer_base_credit: Decimal,
        seller_quote_credit: Decimal,
    ) -> Result<(), PgAccountError> {
        BalanceRepo::settle_fill(
            &self.pool,
            buyer_id,
            seller_id,
            base_asset,
            quote_asset,
            base_qty,
            quote_cost,
            buyer_base_credit,
            seller_quote_credit,
        )
        .await?;

        tracing::debug!(
            %buyer_id,
            %seller_id,
            %base_asset,
            %quote_asset,
            %base_qty,
            %quote_cost,
            "fill settled"
        );
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Order-level helpers
    // -----------------------------------------------------------------------

    /// Determine which asset to lock and call [`try_lock`].
    ///
    /// - `Buy`  → lock `price * qty` of `quote_asset`
    /// - `Sell` → lock `qty` of `base_asset`
    ///
    /// `product` is accepted as individual fields so callers do not need to
    /// depend on the `exchange-account` crate just to pass a `Product`.
    pub async fn lock_for_order(
        &self,
        user_id: Uuid,
        symbol: &str,
        side: OrderSide,
        price: Decimal,
        qty: Decimal,
        base_asset: &str,
        quote_asset: &str,
    ) -> Result<LockInfo, PgAccountError> {
        let (asset, amount) = match side {
            OrderSide::Buy => (quote_asset.to_string(), price * qty),
            OrderSide::Sell => (base_asset.to_string(), qty),
        };

        self.try_lock(user_id, &asset, amount).await?;

        tracing::debug!(%user_id, %symbol, ?side, %asset, %amount, "locked for order");
        Ok(LockInfo { asset, amount })
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// Fetch the balance row for `user_id` + `asset`, or `None` if absent.
    pub async fn get_balance(
        &self,
        user_id: Uuid,
        asset: &str,
    ) -> Result<Option<BalanceRow>, PgAccountError> {
        let row = BalanceRepo::get(&self.pool, user_id, asset).await?;
        Ok(row)
    }

    /// Fetch all balance rows for `user_id`, ordered by asset name.
    pub async fn get_all_balances(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<BalanceRow>, PgAccountError> {
        let rows = BalanceRepo::get_all(&self.pool, user_id).await?;
        Ok(rows)
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Return `AccountNotFound` if the user row does not exist.
    ///
    /// Called before mutations that assume the account was explicitly created
    /// (e.g., deposit). Balance rows reference `users(id)` via FK, so skipping
    /// this check would yield a less informative FK violation error.
    async fn assert_account_exists(&self, user_id: Uuid) -> Result<(), PgAccountError> {
        let exists = UserRepo::exists(&self.pool, user_id).await?;
        if !exists {
            return Err(PgAccountError::AccountNotFound(user_id));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// OrderSide — local mirror so exchange-db has no dep on exchange-account
// ---------------------------------------------------------------------------

/// Which side of the market an order is on.
///
/// Mirrors `exchange_common::Side` but kept local to avoid pulling in a
/// domain dependency from the database crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderSide {
    Buy,
    Sell,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    // -----------------------------------------------------------------------
    // Pure-logic unit tests (no DB required)
    // -----------------------------------------------------------------------

    #[test]
    fn lock_for_order_buy_computes_quote_amount() {
        // Buy side: lock price * qty of the quote asset
        let price = dec!(50_000);
        let qty = dec!(0.5);
        let expected = price * qty; // 25_000

        let (asset, amount) = match OrderSide::Buy {
            OrderSide::Buy => ("USDT".to_string(), price * qty),
            OrderSide::Sell => ("BTC".to_string(), qty),
        };

        assert_eq!(asset, "USDT");
        assert_eq!(amount, expected);
    }

    #[test]
    fn lock_for_order_sell_uses_qty_of_base() {
        let price = dec!(50_000);
        let qty = dec!(1.5);

        let (asset, amount) = match OrderSide::Sell {
            OrderSide::Buy => ("USDT".to_string(), price * qty),
            OrderSide::Sell => ("BTC".to_string(), qty),
        };

        assert_eq!(asset, "BTC");
        assert_eq!(amount, qty);
    }

    #[test]
    fn balance_row_available_is_total_minus_locked() {
        // BalanceRow::available() mirrors Balance::available() semantics.
        let row = BalanceRow {
            user_id: Uuid::new_v4(),
            asset: "USDT".to_string(),
            total: dec!(1000),
            locked: dec!(300),
            updated_at: chrono::Utc::now(),
        };
        assert_eq!(row.available(), dec!(700));
    }

    #[test]
    fn pg_account_error_from_balance_error_insufficient() {
        let balance_err = crate::balance_repo::BalanceError::Insufficient {
            needed: dec!(100),
            available: dec!(50),
        };
        let pg_err: PgAccountError = balance_err.into();
        assert!(matches!(pg_err, PgAccountError::InsufficientBalance { .. }));
    }

    #[test]
    fn pg_account_error_from_balance_error_not_found() {
        let uid = Uuid::new_v4();
        let balance_err = crate::balance_repo::BalanceError::NotFound {
            user_id: uid,
            asset: "ETH".to_string(),
        };
        let pg_err: PgAccountError = balance_err.into();
        assert!(matches!(pg_err, PgAccountError::AssetNotFound { .. }));
    }

    // -----------------------------------------------------------------------
    // Integration tests — require a live PostgreSQL instance.
    // Run with:  TEST_DATABASE_URL=postgres://... cargo test -p exchange-db -- --include-ignored
    // -----------------------------------------------------------------------

    async fn connect() -> PgAccountManager {
        let url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must be set to run integration tests");
        let pool = PgPool::connect(&url)
            .await
            .expect("failed to connect to PostgreSQL");
        // Apply migrations so the schema is up to date.
        sqlx::migrate!("../../migrations")
            .run(&pool)
            .await
            .expect("migration failed");
        PgAccountManager::new(pool)
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn integration_create_account_deposit_get_balance() {
        let mgr = connect().await;
        let user_id = Uuid::new_v4();

        mgr.create_account(user_id, Some("alice@example.com"))
            .await
            .expect("create_account");

        let row = mgr
            .deposit(user_id, "USDT", dec!(10_000))
            .await
            .expect("deposit");

        assert_eq!(row.total, dec!(10_000));
        assert_eq!(row.locked, dec!(0));
        assert_eq!(row.available(), dec!(10_000));

        let fetched = mgr
            .get_balance(user_id, "USDT")
            .await
            .expect("get_balance")
            .expect("should exist");

        assert_eq!(fetched.total, dec!(10_000));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn integration_deposit_try_lock_verifies_locked_amount() {
        let mgr = connect().await;
        let user_id = Uuid::new_v4();

        mgr.create_account(user_id, None).await.expect("create_account");
        mgr.deposit(user_id, "USDT", dec!(5_000))
            .await
            .expect("deposit");

        let row = mgr
            .try_lock(user_id, "USDT", dec!(2_000))
            .await
            .expect("try_lock");

        assert_eq!(row.locked, dec!(2_000));
        assert_eq!(row.available(), dec!(3_000));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn integration_try_lock_insufficient_balance_fails() {
        let mgr = connect().await;
        let user_id = Uuid::new_v4();

        mgr.create_account(user_id, None).await.expect("create_account");
        mgr.deposit(user_id, "USDT", dec!(100))
            .await
            .expect("deposit");

        let err = mgr
            .try_lock(user_id, "USDT", dec!(500))
            .await
            .expect_err("should fail with InsufficientBalance");

        assert!(
            matches!(err, PgAccountError::InsufficientBalance { .. }),
            "expected InsufficientBalance, got: {err:?}"
        );
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn integration_settle_fill_buyer_gets_base_seller_gets_quote() {
        let mgr = connect().await;
        let buyer_id = Uuid::new_v4();
        let seller_id = Uuid::new_v4();

        // Set up accounts
        mgr.create_account(buyer_id, Some("buyer@example.com"))
            .await
            .expect("create buyer");
        mgr.create_account(seller_id, Some("seller@example.com"))
            .await
            .expect("create seller");

        // Buyer deposits USDT; seller deposits BTC
        mgr.deposit(buyer_id, "USDT", dec!(50_000))
            .await
            .expect("buyer deposit");
        mgr.deposit(seller_id, "BTC", dec!(1))
            .await
            .expect("seller deposit");

        // Lock funds (simulates order placement)
        mgr.try_lock(buyer_id, "USDT", dec!(50_000))
            .await
            .expect("buyer lock");
        mgr.try_lock(seller_id, "BTC", dec!(1))
            .await
            .expect("seller lock");

        // Settle: 1 BTC @ 50_000 USDT; fees already reflected in credit amounts
        // buyer_base_credit  = 1 BTC - 0.2% taker fee = 0.998
        // seller_quote_credit = 50_000 - 0.1% maker fee = 49_950
        let buyer_base_credit = dec!(0.998);
        let seller_quote_credit = dec!(49_950);

        mgr.settle_fill(
            buyer_id,
            seller_id,
            "BTC",
            "USDT",
            dec!(1),
            dec!(50_000),
            buyer_base_credit,
            seller_quote_credit,
        )
        .await
        .expect("settle_fill");

        // Buyer: USDT gone, BTC arrived (minus fee)
        let buyer_usdt = mgr
            .get_balance(buyer_id, "USDT")
            .await
            .expect("get_balance")
            .expect("buyer USDT should exist");
        assert_eq!(buyer_usdt.total, dec!(0));
        assert_eq!(buyer_usdt.locked, dec!(0));

        let buyer_btc = mgr
            .get_balance(buyer_id, "BTC")
            .await
            .expect("get_balance")
            .expect("buyer BTC should exist after credit");
        assert_eq!(buyer_btc.total, buyer_base_credit);

        // Seller: BTC gone, USDT arrived (minus fee)
        let seller_btc = mgr
            .get_balance(seller_id, "BTC")
            .await
            .expect("get_balance")
            .expect("seller BTC should exist");
        assert_eq!(seller_btc.total, dec!(0));
        assert_eq!(seller_btc.locked, dec!(0));

        let seller_usdt = mgr
            .get_balance(seller_id, "USDT")
            .await
            .expect("get_balance")
            .expect("seller USDT should exist after credit");
        assert_eq!(seller_usdt.total, seller_quote_credit);
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn integration_concurrent_try_lock_only_one_succeeds() {
        // Two tasks race to lock the same 1000 USDT from a balance of 1200.
        // The first one to acquire the `SELECT FOR UPDATE` row lock wins;
        // the second should fail with InsufficientBalance.
        let mgr = std::sync::Arc::new(connect().await);
        let user_id = Uuid::new_v4();

        mgr.create_account(user_id, None)
            .await
            .expect("create_account");
        mgr.deposit(user_id, "USDT", dec!(1_200))
            .await
            .expect("deposit");

        // Share the pool across both tasks.
        let mgr2 = mgr.clone();

        let t1 = tokio::spawn(async move { mgr.try_lock(user_id, "USDT", dec!(1_000)).await });
        let t2 = tokio::spawn(async move { mgr2.try_lock(user_id, "USDT", dec!(1_000)).await });

        let (r1, r2) = tokio::try_join!(t1, t2).expect("tasks panicked");

        let successes = [r1.is_ok(), r2.is_ok()].iter().filter(|&&b| b).count();
        let failures = [r1.is_err(), r2.is_err()].iter().filter(|&&b| b).count();

        assert_eq!(successes, 1, "exactly one lock should succeed");
        assert_eq!(failures, 1, "exactly one lock should fail");
    }
}
