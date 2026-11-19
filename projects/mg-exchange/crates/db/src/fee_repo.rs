use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

pub struct FeeRepo;

impl FeeRepo {
    /// Record a fee or rebate entry.
    ///
    /// `amount` is signed: positive = revenue collected from the user,
    /// negative = rebate paid to the user.
    /// `ledger_tx_id` links to the double-entry ledger transaction, if available.
    pub async fn record_fee(
        pool: &PgPool,
        trade_id: Option<i64>,
        user_id: Uuid,
        asset: &str,
        amount: Decimal,
        fee_type: &str,
        ledger_tx_id: Option<Uuid>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO fee_revenue (trade_id, user_id, asset, amount, fee_type, ledger_transaction_id)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(trade_id)
        .bind(user_id)
        .bind(asset)
        .bind(amount)
        .bind(fee_type)
        .bind(ledger_tx_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Total positive fee revenue collected for an asset.
    pub async fn total_revenue(pool: &PgPool, asset: &str) -> Result<Decimal, sqlx::Error> {
        let total: Option<Decimal> = sqlx::query_scalar(
            "SELECT SUM(amount) FROM fee_revenue WHERE asset = $1 AND amount > 0",
        )
        .bind(asset)
        .fetch_one(pool)
        .await?;

        Ok(total.unwrap_or(Decimal::ZERO))
    }

    /// Total rebates paid out for an asset (returned as a positive absolute value).
    pub async fn total_rebates(pool: &PgPool, asset: &str) -> Result<Decimal, sqlx::Error> {
        let total: Option<Decimal> = sqlx::query_scalar(
            "SELECT SUM(ABS(amount)) FROM fee_revenue WHERE asset = $1 AND amount < 0",
        )
        .bind(asset)
        .fetch_one(pool)
        .await?;

        Ok(total.unwrap_or(Decimal::ZERO))
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

    /// Insert a user row; `fee_revenue.user_id` has no FK in the schema but we
    /// keep this consistent with other test helpers.
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
    async fn test_record_fee_and_total_revenue() {
        let pool = test_pool().await;
        let user_id = insert_user(&pool).await;

        // Use a unique asset name so aggregation is isolated to this test run.
        let asset = format!("TST{}", &user_id.to_string()[..8]);

        FeeRepo::record_fee(&pool, Some(1), user_id, &asset, dec!(10.00), "taker", None)
            .await
            .expect("record_fee failed");

        FeeRepo::record_fee(&pool, Some(2), user_id, &asset, dec!(5.00), "maker", None)
            .await
            .expect("record_fee maker failed");

        let revenue = FeeRepo::total_revenue(&pool, &asset)
            .await
            .expect("total_revenue failed");

        assert_eq!(revenue, dec!(15.00));
    }

    #[tokio::test]
    #[ignore]
    async fn test_record_rebate_and_total_rebates() {
        let pool = test_pool().await;
        let user_id = insert_user(&pool).await;

        let asset = format!("RBT{}", &user_id.to_string()[..8]);

        // Negative amount = rebate paid to user.
        FeeRepo::record_fee(&pool, None, user_id, &asset, dec!(-3.00), "rebate", None)
            .await
            .expect("record_fee rebate failed");

        let rebates = FeeRepo::total_rebates(&pool, &asset)
            .await
            .expect("total_rebates failed");

        // total_rebates returns the absolute value.
        assert_eq!(rebates, dec!(3.00));

        // Revenue should be zero for this asset (no positive entries).
        let revenue = FeeRepo::total_revenue(&pool, &asset)
            .await
            .expect("total_revenue failed");
        assert_eq!(revenue, Decimal::ZERO);
    }

    #[tokio::test]
    #[ignore]
    async fn test_revenue_and_rebates_are_independent() {
        let pool = test_pool().await;
        let user_id = insert_user(&pool).await;

        let asset = format!("MIX{}", &user_id.to_string()[..8]);

        FeeRepo::record_fee(&pool, Some(10), user_id, &asset, dec!(20.00), "taker", None)
            .await
            .expect("revenue entry failed");

        FeeRepo::record_fee(&pool, None, user_id, &asset, dec!(-4.00), "rebate", None)
            .await
            .expect("rebate entry failed");

        let revenue = FeeRepo::total_revenue(&pool, &asset)
            .await
            .expect("total_revenue failed");
        let rebates = FeeRepo::total_rebates(&pool, &asset)
            .await
            .expect("total_rebates failed");

        // Only positive rows count toward revenue, only negative toward rebates.
        assert_eq!(revenue, dec!(20.00));
        assert_eq!(rebates, dec!(4.00));
    }
}
