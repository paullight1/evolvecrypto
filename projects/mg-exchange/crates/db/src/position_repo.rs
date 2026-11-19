use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

/// Position row from the database.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PositionRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub symbol: String,
    pub side: String,
    pub size: Decimal,
    pub entry_price: Decimal,
    pub leverage: Decimal,
    pub margin_mode: String,
    pub margin: Decimal,
    pub realized_pnl: Decimal,
    pub funding_accumulated: Decimal,
    pub liquidation_price: Option<Decimal>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
}

pub struct PositionRepo;

impl PositionRepo {
    /// Insert a new open position.
    pub async fn create(pool: &PgPool, pos: &PositionRow) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO positions
             (id, user_id, symbol, side, size, entry_price, leverage,
              margin_mode, margin, realized_pnl, funding_accumulated, liquidation_price, status)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, 'Open')",
        )
        .bind(pos.id)
        .bind(pos.user_id)
        .bind(&pos.symbol)
        .bind(&pos.side)
        .bind(pos.size)
        .bind(pos.entry_price)
        .bind(pos.leverage)
        .bind(&pos.margin_mode)
        .bind(pos.margin)
        .bind(pos.realized_pnl)
        .bind(pos.funding_accumulated)
        .bind(pos.liquidation_price)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Update position size, entry_price, margin, and realized_pnl (on increase/reduce).
    pub async fn update(
        pool: &PgPool,
        id: Uuid,
        size: Decimal,
        entry_price: Decimal,
        margin: Decimal,
        realized_pnl: Decimal,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE positions
             SET size = $2, entry_price = $3, margin = $4, realized_pnl = $5, updated_at = NOW()
             WHERE id = $1",
        )
        .bind(id)
        .bind(size)
        .bind(entry_price)
        .bind(margin)
        .bind(realized_pnl)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Mark a position as closed (or liquidated) and record final realized PnL.
    pub async fn close(
        pool: &PgPool,
        id: Uuid,
        final_pnl: Decimal,
        status: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE positions
             SET size = 0, realized_pnl = $2, status = $3, closed_at = NOW(), updated_at = NOW()
             WHERE id = $1",
        )
        .bind(id)
        .bind(final_pnl)
        .bind(status)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Get all open positions for a user, ordered newest first.
    pub async fn open_for_user(
        pool: &PgPool,
        user_id: Uuid,
    ) -> Result<Vec<PositionRow>, sqlx::Error> {
        let rows = sqlx::query_as::<_, PositionRow>(
            "SELECT * FROM positions
             WHERE user_id = $1 AND status = 'Open'
             ORDER BY created_at DESC",
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// Get the single open position for a user+symbol combination, if any.
    pub async fn open_for_symbol(
        pool: &PgPool,
        user_id: Uuid,
        symbol: &str,
    ) -> Result<Option<PositionRow>, sqlx::Error> {
        let row = sqlx::query_as::<_, PositionRow>(
            "SELECT * FROM positions
             WHERE user_id = $1 AND symbol = $2 AND status = 'Open'",
        )
        .bind(user_id)
        .bind(symbol)
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    /// Get all open positions (used for startup state restore).
    pub async fn list_open(pool: &PgPool) -> Result<Vec<PositionRow>, sqlx::Error> {
        let rows = sqlx::query_as::<_, PositionRow>(
            "SELECT * FROM positions WHERE status = 'Open' ORDER BY created_at"
        )
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// Get all open positions across all users (used by the liquidation monitor).
    pub async fn all_open(pool: &PgPool) -> Result<Vec<PositionRow>, sqlx::Error> {
        let rows = sqlx::query_as::<_, PositionRow>(
            "SELECT * FROM positions WHERE status = 'Open'",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// Accumulate a funding payment into `funding_accumulated`.
    ///
    /// `amount` is signed: negative when the user pays, positive when received.
    pub async fn add_funding(
        pool: &PgPool,
        id: Uuid,
        amount: Decimal,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE positions
             SET funding_accumulated = funding_accumulated + $2, updated_at = NOW()
             WHERE id = $1",
        )
        .bind(id)
        .bind(amount)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Sum of open position sizes for a symbol (open interest proxy).
    pub async fn open_interest(pool: &PgPool, symbol: &str) -> Result<Decimal, sqlx::Error> {
        let oi: Option<Decimal> = sqlx::query_scalar(
            "SELECT SUM(size) FROM positions WHERE symbol = $1 AND status = 'Open'",
        )
        .bind(symbol)
        .fetch_one(pool)
        .await?;

        Ok(oi.unwrap_or(Decimal::ZERO))
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

    /// Insert a user row to satisfy the foreign key on `positions.user_id`.
    async fn insert_user(pool: &PgPool) -> Uuid {
        let user_id = Uuid::new_v4();
        sqlx::query("INSERT INTO users (id) VALUES ($1)")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("Failed to create user");
        user_id
    }

    fn sample_position(user_id: Uuid) -> PositionRow {
        let now = Utc::now();
        PositionRow {
            id: Uuid::new_v4(),
            user_id,
            symbol: "BTCUSDT".to_string(),
            side: "Long".to_string(),
            size: dec!(1.0),
            entry_price: dec!(50000),
            leverage: dec!(10),
            margin_mode: "Isolated".to_string(),
            margin: dec!(5000),
            realized_pnl: Decimal::ZERO,
            funding_accumulated: Decimal::ZERO,
            liquidation_price: Some(dec!(45500)),
            status: "Open".to_string(),
            created_at: now,
            updated_at: now,
            closed_at: None,
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_create_and_open_for_user() {
        let pool = test_pool().await;
        let user_id = insert_user(&pool).await;
        let pos = sample_position(user_id);

        PositionRepo::create(&pool, &pos)
            .await
            .expect("create failed");

        let open = PositionRepo::open_for_user(&pool, user_id)
            .await
            .expect("open_for_user failed");

        assert!(open.iter().any(|r| r.id == pos.id));
        let found = open.iter().find(|r| r.id == pos.id).unwrap();
        assert_eq!(found.symbol, "BTCUSDT");
        assert_eq!(found.side, "Long");
        assert_eq!(found.status, "Open");
    }

    #[tokio::test]
    #[ignore]
    async fn test_open_for_symbol_and_update() {
        let pool = test_pool().await;
        let user_id = insert_user(&pool).await;
        let pos = sample_position(user_id);

        PositionRepo::create(&pool, &pos)
            .await
            .expect("create failed");

        // Confirm open_for_symbol finds it
        let found = PositionRepo::open_for_symbol(&pool, user_id, "BTCUSDT")
            .await
            .expect("open_for_symbol failed")
            .expect("should be Some");
        assert_eq!(found.id, pos.id);

        // Update size, entry_price, margin, realized_pnl
        PositionRepo::update(&pool, pos.id, dec!(0.5), dec!(51000), dec!(2550), dec!(500))
            .await
            .expect("update failed");

        let updated = PositionRepo::open_for_symbol(&pool, user_id, "BTCUSDT")
            .await
            .expect("open_for_symbol after update failed")
            .expect("should still be open");
        assert_eq!(updated.size, dec!(0.5));
        assert_eq!(updated.entry_price, dec!(51000));
        assert_eq!(updated.realized_pnl, dec!(500));
    }

    #[tokio::test]
    #[ignore]
    async fn test_close_and_open_interest() {
        let pool = test_pool().await;
        let user_a = insert_user(&pool).await;
        let user_b = insert_user(&pool).await;

        let mut pos_a = sample_position(user_a);
        pos_a.size = dec!(2.0);
        let mut pos_b = sample_position(user_b);
        pos_b.id = Uuid::new_v4();
        pos_b.size = dec!(3.0);

        PositionRepo::create(&pool, &pos_a)
            .await
            .expect("create pos_a failed");
        PositionRepo::create(&pool, &pos_b)
            .await
            .expect("create pos_b failed");

        let oi_before = PositionRepo::open_interest(&pool, "BTCUSDT")
            .await
            .expect("open_interest failed");
        // At least 5 (may include rows from other test runs)
        assert!(oi_before >= dec!(5.0));

        // Close pos_a
        PositionRepo::close(&pool, pos_a.id, dec!(1000), "Closed")
            .await
            .expect("close failed");

        // Closed position should not appear in open_for_user
        let open = PositionRepo::open_for_user(&pool, user_a)
            .await
            .expect("open_for_user after close failed");
        assert!(!open.iter().any(|r| r.id == pos_a.id));

        // open_interest drops by 2
        let oi_after = PositionRepo::open_interest(&pool, "BTCUSDT")
            .await
            .expect("open_interest after close failed");
        assert_eq!(oi_after, oi_before - dec!(2.0));
    }
}
