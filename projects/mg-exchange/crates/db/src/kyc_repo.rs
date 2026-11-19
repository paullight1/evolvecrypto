use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

/// KYC record row from the database.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct KycRow {
    pub user_id: Uuid,
    pub level: String,
    pub status: String,
    pub provider_ref: Option<String>,
    pub reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct KycRepo;

impl KycRepo {
    /// Insert or update a KYC record for a user.
    pub async fn upsert(
        pool: &PgPool,
        user_id: Uuid,
        level: &str,
        status: &str,
        provider_ref: Option<&str>,
        reason: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO kyc_records (user_id, level, status, provider_ref, reason, updated_at)
             VALUES ($1, $2, $3, $4, $5, NOW())
             ON CONFLICT (user_id) DO UPDATE SET
                level = EXCLUDED.level,
                status = EXCLUDED.status,
                provider_ref = EXCLUDED.provider_ref,
                reason = EXCLUDED.reason,
                updated_at = NOW()",
        )
        .bind(user_id)
        .bind(level)
        .bind(status)
        .bind(provider_ref)
        .bind(reason)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Get KYC record for a user.
    pub async fn get(pool: &PgPool, user_id: Uuid) -> Result<Option<KycRow>, sqlx::Error> {
        let row = sqlx::query_as::<_, KycRow>(
            "SELECT user_id, level, status, provider_ref, reason, created_at, updated_at
             FROM kyc_records WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    /// List all KYC records with status = 'Pending'.
    pub async fn list_pending(pool: &PgPool) -> Result<Vec<KycRow>, sqlx::Error> {
        let rows = sqlx::query_as::<_, KycRow>(
            "SELECT user_id, level, status, provider_ref, reason, created_at, updated_at
             FROM kyc_records WHERE status = 'Pending'
             ORDER BY created_at ASC",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// List ALL KYC records (for startup hydration).
    pub async fn list_all(pool: &PgPool) -> Result<Vec<KycRow>, sqlx::Error> {
        let rows = sqlx::query_as::<_, KycRow>(
            "SELECT user_id, level, status, provider_ref, reason, created_at, updated_at
             FROM kyc_records
             ORDER BY created_at ASC",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// Update the status (and optional reason) of a KYC record.
    pub async fn update_status(
        pool: &PgPool,
        user_id: Uuid,
        status: &str,
        reason: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE kyc_records SET status = $2, reason = $3, updated_at = NOW()
             WHERE user_id = $1",
        )
        .bind(user_id)
        .bind(status)
        .bind(reason)
        .execute(pool)
        .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> PgPool {
        let url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must be set for integration tests");
        PgPool::connect(&url).await.expect("Failed to connect to test database")
    }

    #[tokio::test]
    #[ignore]
    async fn test_kyc_lifecycle() {
        let pool = test_pool().await;
        let user_id = Uuid::new_v4();

        // Create the user first (foreign key constraint)
        sqlx::query("INSERT INTO users (id) VALUES ($1)")
            .bind(user_id)
            .execute(&pool)
            .await
            .expect("Failed to create user");

        // Upsert a KYC record
        KycRepo::upsert(&pool, user_id, "Basic", "Pending", Some("prov-001"), None)
            .await
            .expect("upsert failed");

        // Get it back
        let row = KycRepo::get(&pool, user_id)
            .await
            .expect("get failed")
            .expect("row should exist");
        assert_eq!(row.user_id, user_id);
        assert_eq!(row.level, "Basic");
        assert_eq!(row.status, "Pending");
        assert_eq!(row.provider_ref.as_deref(), Some("prov-001"));
        assert!(row.reason.is_none());

        // list_pending should include this user
        let pending = KycRepo::list_pending(&pool).await.expect("list_pending failed");
        assert!(pending.iter().any(|r| r.user_id == user_id));

        // Update status to Verified
        KycRepo::update_status(&pool, user_id, "Verified", None)
            .await
            .expect("update_status failed");

        let updated = KycRepo::get(&pool, user_id)
            .await
            .expect("get after update failed")
            .expect("row should still exist");
        assert_eq!(updated.status, "Verified");

        // Should no longer appear in pending list
        let pending_after = KycRepo::list_pending(&pool).await.expect("list_pending after failed");
        assert!(!pending_after.iter().any(|r| r.user_id == user_id));

        // Upsert again — idempotent update
        KycRepo::upsert(&pool, user_id, "Full", "Verified", Some("prov-002"), Some("docs uploaded"))
            .await
            .expect("second upsert failed");
        let row2 = KycRepo::get(&pool, user_id)
            .await
            .expect("get after second upsert failed")
            .expect("row should exist");
        assert_eq!(row2.level, "Full");
        assert_eq!(row2.provider_ref.as_deref(), Some("prov-002"));
    }
}
