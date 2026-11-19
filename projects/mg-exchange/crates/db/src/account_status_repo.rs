use sqlx::PgPool;
use uuid::Uuid;

/// Row representing a non-Active account status in the database.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AccountStatusRow {
    pub user_id: Uuid,
    pub status: String,
    pub reason: Option<String>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

pub struct AccountStatusRepo;

impl AccountStatusRepo {
    /// Upsert account status. Only stores non-Active statuses.
    /// When status is "Active", the row is deleted (account is back to normal).
    pub async fn set_status(
        pool: &PgPool,
        user_id: Uuid,
        status: &str,
        reason: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        if status == "Active" {
            sqlx::query("DELETE FROM account_statuses WHERE user_id = $1")
                .bind(user_id)
                .execute(pool)
                .await?;
        } else {
            sqlx::query(
                "INSERT INTO account_statuses (user_id, status, reason, updated_at)
                 VALUES ($1, $2, $3, NOW())
                 ON CONFLICT (user_id)
                 DO UPDATE SET status = $2, reason = $3, updated_at = NOW()",
            )
            .bind(user_id)
            .bind(status)
            .bind(reason)
            .execute(pool)
            .await?;
        }
        Ok(())
    }

    /// Load all non-Active account statuses (for startup recovery).
    pub async fn load_all(pool: &PgPool) -> Result<Vec<AccountStatusRow>, sqlx::Error> {
        sqlx::query_as::<_, AccountStatusRow>(
            "SELECT user_id, status, reason, updated_at FROM account_statuses",
        )
        .fetch_all(pool)
        .await
    }
}
