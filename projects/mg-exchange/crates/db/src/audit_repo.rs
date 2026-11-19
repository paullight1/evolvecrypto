use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

/// Audit log row from the database.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AuditRow {
    pub id: i64,
    pub user_id: Option<Uuid>,
    pub action: String,
    pub detail: serde_json::Value,
    /// Stored as INET in Postgres; surfaced here as the text representation.
    pub ip_address: Option<String>,
    pub created_at: DateTime<Utc>,
    pub prev_hash: String,
    pub entry_hash: String,
}

pub struct AuditRepo;

impl AuditRepo {
    /// Append a new entry to the audit log. This is the ONLY write operation —
    /// the audit log is strictly append-only; no UPDATE or DELETE is ever issued.
    ///
    /// `ip` is an optional IP address string (IPv4 or IPv6). It is cast to the
    /// Postgres `INET` type inside the query; invalid addresses will be rejected
    /// by the database.
    pub async fn append(
        pool: &PgPool,
        user_id: Option<Uuid>,
        action: &str,
        detail: serde_json::Value,
        ip: Option<&str>,
        prev_hash: &str,
        entry_hash: &str,
    ) -> Result<i64, sqlx::Error> {
        let id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO audit_log (user_id, action, detail, ip_address, prev_hash, entry_hash)
             VALUES ($1, $2, $3, $4::INET, $5, $6)
             RETURNING id",
        )
        .bind(user_id)
        .bind(action)
        .bind(detail)
        .bind(ip)
        .bind(prev_hash)
        .bind(entry_hash)
        .fetch_one(pool)
        .await?;

        Ok(id)
    }

    /// Return the `entry_hash` of the most recent row, or `"genesis"` if the
    /// table is empty. Used at startup to continue the hash chain from where
    /// the database left off.
    pub async fn last_entry_hash(pool: &PgPool) -> Result<String, sqlx::Error> {
        let hash: Option<String> = sqlx::query_scalar(
            "SELECT entry_hash FROM audit_log ORDER BY id DESC LIMIT 1",
        )
        .fetch_optional(pool)
        .await?;

        Ok(hash.unwrap_or_else(|| "genesis".to_string()))
    }

    /// Query the audit log with optional filters.
    ///
    /// All filter parameters are optional; omitting them returns all rows (up to
    /// `limit`). Results are ordered oldest-first so callers see events in
    /// chronological order.
    pub async fn query(
        pool: &PgPool,
        user_id: Option<Uuid>,
        action: Option<&str>,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
        limit: i64,
    ) -> Result<Vec<AuditRow>, sqlx::Error> {
        // Build the WHERE clauses dynamically. The ip_address column is INET, but
        // we cast it to TEXT for the AuditRow so we don't need an inet crate on
        // the Rust side.
        let rows = sqlx::query_as::<_, AuditRow>(
            "SELECT id, user_id, action, detail,
                    ip_address::TEXT AS ip_address,
                    created_at, prev_hash, entry_hash
             FROM audit_log
             WHERE ($1::UUID IS NULL OR user_id = $1)
               AND ($2::VARCHAR IS NULL OR action = $2)
               AND ($3::TIMESTAMPTZ IS NULL OR created_at >= $3)
               AND ($4::TIMESTAMPTZ IS NULL OR created_at <= $4)
             ORDER BY created_at ASC
             LIMIT $5",
        )
        .bind(user_id)
        .bind(action)
        .bind(from)
        .bind(to)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// Return total number of audit log entries.
    pub async fn count(pool: &PgPool) -> Result<i64, sqlx::Error> {
        let n = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM audit_log")
            .fetch_one(pool)
            .await?;

        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn test_pool() -> PgPool {
        let url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must be set for integration tests");
        PgPool::connect(&url).await.expect("Failed to connect to test database")
    }

    #[tokio::test]
    #[ignore]
    async fn test_audit_append_and_query() {
        let pool = test_pool().await;
        let user_id = Uuid::new_v4();
        let other_id = Uuid::new_v4();

        // Create users (foreign key constraint on audit_log is nullable, but let's
        // use real users so the FK is satisfied when user_id is Some)
        for uid in [user_id, other_id] {
            sqlx::query("INSERT INTO users (id) VALUES ($1)")
                .bind(uid)
                .execute(&pool)
                .await
                .expect("Failed to create user");
        }

        let count_before = AuditRepo::count(&pool).await.expect("count failed");

        // Append 5 entries: 3 for user_id, 2 for other_id / system
        AuditRepo::append(&pool, Some(user_id), "login", json!({"method": "password"}), Some("1.2.3.4"), "genesis", "hash1")
            .await
            .expect("append 1 failed");
        AuditRepo::append(&pool, Some(user_id), "withdraw", json!({"asset": "BTC", "amount": "0.1"}), None, "hash1", "hash2")
            .await
            .expect("append 2 failed");
        AuditRepo::append(&pool, Some(user_id), "login", json!({"method": "totp"}), Some("5.6.7.8"), "hash2", "hash3")
            .await
            .expect("append 3 failed");
        AuditRepo::append(&pool, Some(other_id), "login", json!({"method": "password"}), None, "hash3", "hash4")
            .await
            .expect("append 4 failed");
        AuditRepo::append(&pool, None, "system_event", json!({"msg": "startup"}), None, "hash4", "hash5")
            .await
            .expect("append 5 failed");

        // count should have increased by 5
        let count_after = AuditRepo::count(&pool).await.expect("count after failed");
        assert_eq!(count_after - count_before, 5);

        // Query by user_id — should return exactly 3 rows for user_id
        let by_user = AuditRepo::query(&pool, Some(user_id), None, None, None, 100)
            .await
            .expect("query by user failed");
        assert_eq!(by_user.len(), 3);
        assert!(by_user.iter().all(|r| r.user_id == Some(user_id)));

        // Query by action "login" for user_id — should return 2 rows
        let logins = AuditRepo::query(&pool, Some(user_id), Some("login"), None, None, 100)
            .await
            .expect("query by user+action failed");
        assert_eq!(logins.len(), 2);

        // Query all logins (no user filter) — should include all 3 login events
        let all_logins = AuditRepo::query(&pool, None, Some("login"), None, None, 100)
            .await
            .expect("query all logins failed");
        assert!(all_logins.len() >= 3);

        // Limit respected
        let limited = AuditRepo::query(&pool, Some(user_id), None, None, None, 2)
            .await
            .expect("query with limit failed");
        assert_eq!(limited.len(), 2);

        // ip_address round-trip
        let with_ip = by_user
            .iter()
            .find(|r| r.ip_address.is_some())
            .expect("at least one row should have an IP");
        assert!(
            with_ip.ip_address.as_deref() == Some("1.2.3.4")
                || with_ip.ip_address.as_deref() == Some("5.6.7.8")
        );
    }
}
