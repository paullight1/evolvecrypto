use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

/// Balance row from the database.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct BalanceRow {
    pub user_id: Uuid,
    pub asset: String,
    pub total: Decimal,
    pub locked: Decimal,
    pub updated_at: DateTime<Utc>,
}

impl BalanceRow {
    pub fn available(&self) -> Decimal {
        self.total - self.locked
    }
}

pub struct BalanceRepo;

impl BalanceRepo {
    /// Get balance for a user + asset. Returns None if no balance exists.
    pub async fn get(pool: &PgPool, user_id: Uuid, asset: &str) -> Result<Option<BalanceRow>, sqlx::Error> {
        let row = sqlx::query_as::<_, BalanceRow>(
            "SELECT user_id, asset, total, locked, updated_at
             FROM balances WHERE user_id = $1 AND asset = $2"
        )
        .bind(user_id)
        .bind(asset)
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    /// Get all balances for a user.
    pub async fn get_all(pool: &PgPool, user_id: Uuid) -> Result<Vec<BalanceRow>, sqlx::Error> {
        let rows = sqlx::query_as::<_, BalanceRow>(
            "SELECT user_id, asset, total, locked, updated_at
             FROM balances WHERE user_id = $1 ORDER BY asset"
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// Get all non-zero balances across all users.
    pub async fn all_balances(pool: &PgPool) -> Result<Vec<BalanceRow>, sqlx::Error> {
        sqlx::query_as::<_, BalanceRow>(
            "SELECT user_id, asset, total, locked, updated_at
             FROM balances WHERE total > 0 ORDER BY user_id, asset"
        )
        .fetch_all(pool)
        .await
    }

    /// Deposit: increase total balance. Creates row if not exists.
    pub async fn deposit(pool: &PgPool, user_id: Uuid, asset: &str, amount: Decimal) -> Result<BalanceRow, sqlx::Error> {
        let row = sqlx::query_as::<_, BalanceRow>(
            "INSERT INTO balances (user_id, asset, total, locked, updated_at)
             VALUES ($1, $2, $3, 0, NOW())
             ON CONFLICT (user_id, asset) DO UPDATE SET
                total = balances.total + $3,
                updated_at = NOW()
             RETURNING user_id, asset, total, locked, updated_at"
        )
        .bind(user_id)
        .bind(asset)
        .bind(amount)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    /// Lock funds for an order: increase locked, must not exceed available.
    /// Uses SELECT FOR UPDATE to prevent race conditions.
    pub async fn try_lock(pool: &PgPool, user_id: Uuid, asset: &str, amount: Decimal) -> Result<BalanceRow, BalanceError> {
        let mut tx = pool.begin().await.map_err(BalanceError::Db)?;

        // Lock the row
        let row = sqlx::query_as::<_, BalanceRow>(
            "SELECT user_id, asset, total, locked, updated_at
             FROM balances WHERE user_id = $1 AND asset = $2 FOR UPDATE"
        )
        .bind(user_id)
        .bind(asset)
        .fetch_optional(&mut *tx)
        .await
        .map_err(BalanceError::Db)?;

        let row = row.ok_or(BalanceError::NotFound { user_id, asset: asset.to_string() })?;

        if row.available() < amount {
            return Err(BalanceError::Insufficient {
                needed: amount,
                available: row.available(),
            });
        }

        // Update locked
        let updated = sqlx::query_as::<_, BalanceRow>(
            "UPDATE balances SET locked = locked + $3, updated_at = NOW()
             WHERE user_id = $1 AND asset = $2
             RETURNING user_id, asset, total, locked, updated_at"
        )
        .bind(user_id)
        .bind(asset)
        .bind(amount)
        .fetch_one(&mut *tx)
        .await
        .map_err(BalanceError::Db)?;

        tx.commit().await.map_err(BalanceError::Db)?;
        Ok(updated)
    }

    /// Unlock funds (on cancel/reject).
    pub async fn unlock(pool: &PgPool, user_id: Uuid, asset: &str, amount: Decimal) -> Result<BalanceRow, sqlx::Error> {
        let row = sqlx::query_as::<_, BalanceRow>(
            "UPDATE balances SET locked = locked - $3, updated_at = NOW()
             WHERE user_id = $1 AND asset = $2
             RETURNING user_id, asset, total, locked, updated_at"
        )
        .bind(user_id)
        .bind(asset)
        .bind(amount)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    /// Deduct locked funds (after fill — outgoing side).
    pub async fn deduct_locked(pool: &PgPool, user_id: Uuid, asset: &str, amount: Decimal) -> Result<BalanceRow, sqlx::Error> {
        let row = sqlx::query_as::<_, BalanceRow>(
            "UPDATE balances SET total = total - $3, locked = locked - $3, updated_at = NOW()
             WHERE user_id = $1 AND asset = $2
             RETURNING user_id, asset, total, locked, updated_at"
        )
        .bind(user_id)
        .bind(asset)
        .bind(amount)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    /// Credit funds (after fill — incoming side).
    pub async fn credit(pool: &PgPool, user_id: Uuid, asset: &str, amount: Decimal) -> Result<BalanceRow, sqlx::Error> {
        let row = sqlx::query_as::<_, BalanceRow>(
            "INSERT INTO balances (user_id, asset, total, locked, updated_at)
             VALUES ($1, $2, $3, 0, NOW())
             ON CONFLICT (user_id, asset) DO UPDATE SET
                total = balances.total + $3,
                updated_at = NOW()
             RETURNING user_id, asset, total, locked, updated_at"
        )
        .bind(user_id)
        .bind(asset)
        .bind(amount)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    /// Atomic fill settlement: deduct from one side, credit to other.
    /// Runs in a single transaction.
    pub async fn settle_fill(
        pool: &PgPool,
        buyer_id: Uuid,
        seller_id: Uuid,
        base_asset: &str,
        quote_asset: &str,
        base_qty: Decimal,
        quote_cost: Decimal,
        buyer_base_credit: Decimal,  // base_qty - buyer_fee
        seller_quote_credit: Decimal, // quote_cost - seller_fee
    ) -> Result<(), sqlx::Error> {
        let mut tx = pool.begin().await?;

        // Buyer: deduct locked quote
        sqlx::query(
            "UPDATE balances SET total = total - $3, locked = locked - $3, updated_at = NOW()
             WHERE user_id = $1 AND asset = $2"
        )
        .bind(buyer_id)
        .bind(quote_asset)
        .bind(quote_cost)
        .execute(&mut *tx)
        .await?;

        // Buyer: credit base (minus fee)
        sqlx::query(
            "INSERT INTO balances (user_id, asset, total, locked, updated_at)
             VALUES ($1, $2, $3, 0, NOW())
             ON CONFLICT (user_id, asset) DO UPDATE SET
                total = balances.total + $3, updated_at = NOW()"
        )
        .bind(buyer_id)
        .bind(base_asset)
        .bind(buyer_base_credit)
        .execute(&mut *tx)
        .await?;

        // Seller: deduct locked base
        sqlx::query(
            "UPDATE balances SET total = total - $3, locked = locked - $3, updated_at = NOW()
             WHERE user_id = $1 AND asset = $2"
        )
        .bind(seller_id)
        .bind(base_asset)
        .bind(base_qty)
        .execute(&mut *tx)
        .await?;

        // Seller: credit quote (minus fee)
        sqlx::query(
            "INSERT INTO balances (user_id, asset, total, locked, updated_at)
             VALUES ($1, $2, $3, 0, NOW())
             ON CONFLICT (user_id, asset) DO UPDATE SET
                total = balances.total + $3, updated_at = NOW()"
        )
        .bind(seller_id)
        .bind(quote_asset)
        .bind(seller_quote_credit)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BalanceError {
    #[error("Balance not found for user {user_id} asset {asset}")]
    NotFound { user_id: Uuid, asset: String },

    #[error("Insufficient balance: need {needed}, available {available}")]
    Insufficient { needed: Decimal, available: Decimal },

    #[error("Database error: {0}")]
    Db(sqlx::Error),
}
