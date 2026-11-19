use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

/// Funding rate snapshot row from the database.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct FundingRateRow {
    pub id: i64,
    pub symbol: String,
    pub rate: Decimal,
    pub mark_price: Decimal,
    pub index_price: Decimal,
    pub settled_at: DateTime<Utc>,
}

/// Funding payment row — one record per position per settlement interval.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct FundingPaymentRow {
    pub id: i64,
    pub user_id: Uuid,
    pub symbol: String,
    pub position_id: Uuid,
    pub rate: Decimal,
    pub amount: Decimal,
    pub mark_price: Decimal,
    pub ledger_transaction_id: Option<Uuid>,
    pub settled_at: DateTime<Utc>,
}

pub struct FundingRepo;

impl FundingRepo {
    /// Persist a funding rate snapshot at a settlement timestamp.
    pub async fn insert_rate(
        pool: &PgPool,
        symbol: &str,
        rate: Decimal,
        mark_price: Decimal,
        index_price: Decimal,
        settled_at: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO funding_rates (symbol, rate, mark_price, index_price, settled_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(symbol)
        .bind(rate)
        .bind(mark_price)
        .bind(index_price)
        .bind(settled_at)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Get the most recent funding rates for a symbol, newest first.
    pub async fn recent_rates(
        pool: &PgPool,
        symbol: &str,
        limit: i64,
    ) -> Result<Vec<FundingRateRow>, sqlx::Error> {
        let rows = sqlx::query_as::<_, FundingRateRow>(
            "SELECT * FROM funding_rates
             WHERE symbol = $1
             ORDER BY settled_at DESC
             LIMIT $2",
        )
        .bind(symbol)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// Record a funding payment for a specific position.
    ///
    /// `amount` is signed: negative when the user pays, positive when received.
    /// `ledger_tx_id` links to the corresponding double-entry ledger transaction,
    /// or `None` when the ledger write is deferred / not applicable.
    pub async fn insert_payment(
        pool: &PgPool,
        user_id: Uuid,
        symbol: &str,
        position_id: Uuid,
        rate: Decimal,
        amount: Decimal,
        mark_price: Decimal,
        ledger_tx_id: Option<Uuid>,
        settled_at: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO funding_payments
             (user_id, symbol, position_id, rate, amount, mark_price, ledger_transaction_id, settled_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(user_id)
        .bind(symbol)
        .bind(position_id)
        .bind(rate)
        .bind(amount)
        .bind(mark_price)
        .bind(ledger_tx_id)
        .bind(settled_at)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Get recent funding payments for a user, newest first.
    pub async fn user_payments(
        pool: &PgPool,
        user_id: Uuid,
        limit: i64,
    ) -> Result<Vec<FundingPaymentRow>, sqlx::Error> {
        let rows = sqlx::query_as::<_, FundingPaymentRow>(
            "SELECT * FROM funding_payments
             WHERE user_id = $1
             ORDER BY settled_at DESC
             LIMIT $2",
        )
        .bind(user_id)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    async fn test_pool() -> PgPool {
        PgPool::connect(
            &std::env::var("TEST_DATABASE_URL")
                .expect("TEST_DATABASE_URL must be set for integration tests"),
        )
        .await
        .expect("Failed to connect to test database")
    }

    /// Insert a user row to satisfy the foreign key on `funding_payments.user_id`.
    async fn insert_user(pool: &PgPool) -> Uuid {
        let user_id = Uuid::new_v4();
        sqlx::query("INSERT INTO users (id) VALUES ($1)")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("Failed to create user");
        user_id
    }

    #[tokio::test]
    #[ignore]
    async fn test_insert_rate_and_recent_rates() {
        let pool = test_pool().await;
        let settled_at = Utc::now();

        FundingRepo::insert_rate(
            &pool,
            "BTCUSDT",
            dec!(0.0001),
            dec!(50000),
            dec!(49990),
            settled_at,
        )
        .await
        .expect("insert_rate failed");

        let rates = FundingRepo::recent_rates(&pool, "BTCUSDT", 10)
            .await
            .expect("recent_rates failed");

        // The row we just inserted must appear.
        assert!(rates.iter().any(|r| {
            r.symbol == "BTCUSDT"
                && r.rate == dec!(0.0001)
                && r.mark_price == dec!(50000)
        }));
    }

    #[tokio::test]
    #[ignore]
    async fn test_recent_rates_limit_respected() {
        let pool = test_pool().await;

        // Insert 3 rates so the limit test is meaningful regardless of existing data.
        for i in 0..3u32 {
            FundingRepo::insert_rate(
                &pool,
                "ETHUSDT",
                dec!(0.0002),
                Decimal::from(3000 + i),
                Decimal::from(2999 + i),
                Utc::now(),
            )
            .await
            .expect("insert_rate failed");
        }

        let rates = FundingRepo::recent_rates(&pool, "ETHUSDT", 2)
            .await
            .expect("recent_rates with limit failed");

        assert!(rates.len() <= 2);
    }

    #[tokio::test]
    #[ignore]
    async fn test_insert_payment_and_user_payments() {
        let pool = test_pool().await;
        let user_id = insert_user(&pool).await;
        let position_id = Uuid::new_v4();
        let settled_at = Utc::now();

        // Negative amount — user pays funding.
        FundingRepo::insert_payment(
            &pool,
            user_id,
            "BTCUSDT",
            position_id,
            dec!(0.0001),
            dec!(-5.00),
            dec!(50000),
            None,
            settled_at,
        )
        .await
        .expect("insert_payment failed");

        let payments = FundingRepo::user_payments(&pool, user_id, 10)
            .await
            .expect("user_payments failed");

        assert!(payments.iter().any(|p| {
            p.user_id == user_id
                && p.position_id == position_id
                && p.amount == dec!(-5.00)
                && p.symbol == "BTCUSDT"
        }));
    }
}
