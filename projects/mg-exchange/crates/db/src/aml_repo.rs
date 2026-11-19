use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

/// AML alert row from the database.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AmlAlertRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub alert_type: String,
    pub amount_usd: Decimal,
    pub description: String,
    pub requires_str: bool,
    pub reviewed: bool,
    pub reviewed_by: Option<Uuid>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

pub struct AmlRepo;

impl AmlRepo {
    /// Insert a new AML alert.
    pub async fn insert_alert(
        pool: &PgPool,
        alert_id: Uuid,
        user_id: Uuid,
        alert_type: &str,
        amount_usd: Decimal,
        description: &str,
        requires_str: bool,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO aml_alerts (id, user_id, alert_type, amount_usd, description, requires_str)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(alert_id)
        .bind(user_id)
        .bind(alert_type)
        .bind(amount_usd)
        .bind(description)
        .bind(requires_str)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Mark an alert as reviewed by a compliance officer.
    pub async fn mark_reviewed(
        pool: &PgPool,
        alert_id: Uuid,
        reviewed_by: Uuid,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE aml_alerts
             SET reviewed = true, reviewed_by = $2, reviewed_at = NOW()
             WHERE id = $1",
        )
        .bind(alert_id)
        .bind(reviewed_by)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// List all alerts that have not yet been reviewed.
    pub async fn unreviewed(pool: &PgPool) -> Result<Vec<AmlAlertRow>, sqlx::Error> {
        let rows = sqlx::query_as::<_, AmlAlertRow>(
            "SELECT id, user_id, alert_type, amount_usd, description, requires_str,
                    reviewed, reviewed_by, reviewed_at, created_at
             FROM aml_alerts
             WHERE NOT reviewed
             ORDER BY created_at ASC",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// List all alerts for a specific user.
    pub async fn user_alerts(pool: &PgPool, user_id: Uuid) -> Result<Vec<AmlAlertRow>, sqlx::Error> {
        let rows = sqlx::query_as::<_, AmlAlertRow>(
            "SELECT id, user_id, alert_type, amount_usd, description, requires_str,
                    reviewed, reviewed_by, reviewed_at, created_at
             FROM aml_alerts
             WHERE user_id = $1
             ORDER BY created_at DESC",
        )
        .bind(user_id)
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
        let url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must be set for integration tests");
        PgPool::connect(&url).await.expect("Failed to connect to test database")
    }

    #[tokio::test]
    #[ignore]
    async fn test_aml_lifecycle() {
        let pool = test_pool().await;
        let user_id = Uuid::new_v4();
        let reviewer_id = Uuid::new_v4();
        let alert_id = Uuid::new_v4();

        // Create users (foreign key constraint)
        sqlx::query("INSERT INTO users (id) VALUES ($1)")
            .bind(user_id)
            .execute(&pool)
            .await
            .expect("Failed to create user");
        sqlx::query("INSERT INTO users (id) VALUES ($1)")
            .bind(reviewer_id)
            .execute(&pool)
            .await
            .expect("Failed to create reviewer");

        // Insert an alert
        AmlRepo::insert_alert(
            &pool,
            alert_id,
            user_id,
            "large_withdrawal",
            dec!(50000.00),
            "Withdrawal exceeds daily threshold",
            true,
        )
        .await
        .expect("insert_alert failed");

        // Should appear in unreviewed list
        let unreviewed = AmlRepo::unreviewed(&pool).await.expect("unreviewed failed");
        assert!(unreviewed.iter().any(|a| a.id == alert_id));
        let alert = unreviewed.iter().find(|a| a.id == alert_id).unwrap();
        assert!(!alert.reviewed);
        assert!(alert.requires_str);

        // Should appear in user_alerts
        let user_alerts = AmlRepo::user_alerts(&pool, user_id)
            .await
            .expect("user_alerts failed");
        assert!(user_alerts.iter().any(|a| a.id == alert_id));

        // Mark as reviewed
        AmlRepo::mark_reviewed(&pool, alert_id, reviewer_id)
            .await
            .expect("mark_reviewed failed");

        // Should no longer appear in unreviewed list
        let unreviewed_after = AmlRepo::unreviewed(&pool).await.expect("unreviewed after failed");
        assert!(!unreviewed_after.iter().any(|a| a.id == alert_id));

        // Reviewed fields should be set in user_alerts
        let user_alerts_after = AmlRepo::user_alerts(&pool, user_id)
            .await
            .expect("user_alerts after failed");
        let reviewed_alert = user_alerts_after
            .iter()
            .find(|a| a.id == alert_id)
            .expect("alert should still be in user_alerts");
        assert!(reviewed_alert.reviewed);
        assert_eq!(reviewed_alert.reviewed_by, Some(reviewer_id));
        assert!(reviewed_alert.reviewed_at.is_some());
    }
}
