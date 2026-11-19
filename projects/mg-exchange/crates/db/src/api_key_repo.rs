use chrono::DateTime;
use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

/// API key row from the database.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ApiKeyRow {
    pub key: String,
    /// AES-256-GCM encrypted API secret (base64-encoded `nonce || ciphertext`).
    /// Decrypted at runtime using the `API_SECRET_ENCRYPTION_KEY` env var.
    /// Never log this field.
    pub secret_encrypted: String,
    pub user_id: Uuid,
    pub label: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

pub struct ApiKeyRepo;

impl ApiKeyRepo {
    /// Create a new API key for a user.
    ///
    /// # Security note
    /// `secret_encrypted` must be the AES-256-GCM ciphertext produced by
    /// `auth::encrypt_secret`. The plaintext secret should be shown to the user
    /// exactly once at creation time and never stored or logged.
    pub async fn create(
        pool: &PgPool,
        key: &str,
        secret_encrypted: &str,
        user_id: Uuid,
        label: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO api_keys (key, secret_encrypted, user_id, label)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(key)
        .bind(secret_encrypted)
        .bind(user_id)
        .bind(label)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Look up an API key row by its public key string.
    pub async fn find_by_key(pool: &PgPool, key: &str) -> Result<Option<ApiKeyRow>, sqlx::Error> {
        let row = sqlx::query_as::<_, ApiKeyRow>(
            "SELECT key, secret_encrypted, user_id, label, is_active, created_at
             FROM api_keys WHERE key = $1",
        )
        .bind(key)
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    /// List all API keys for a user, ordered by creation time (newest first).
    pub async fn list_for_user(pool: &PgPool, user_id: Uuid) -> Result<Vec<ApiKeyRow>, sqlx::Error> {
        let rows = sqlx::query_as::<_, ApiKeyRow>(
            "SELECT key, secret_encrypted, user_id, label, is_active, created_at
             FROM api_keys WHERE user_id = $1
             ORDER BY created_at DESC",
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// Soft-delete a key by setting `is_active = false`. The row is retained for
    /// audit purposes.
    pub async fn revoke(pool: &PgPool, key: &str) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE api_keys SET is_active = false WHERE key = $1")
            .bind(key)
            .execute(pool)
            .await?;

        Ok(())
    }

    /// Return whether an API key exists and is currently active.
    pub async fn is_active(pool: &PgPool, key: &str) -> Result<bool, sqlx::Error> {
        let result = sqlx::query_scalar::<_, bool>(
            "SELECT COALESCE(
                (SELECT is_active FROM api_keys WHERE key = $1),
                false
             )",
        )
        .bind(key)
        .fetch_one(pool)
        .await?;

        Ok(result)
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
    async fn test_api_key_lifecycle() {
        let pool = test_pool().await;
        let user_id = Uuid::new_v4();

        // Create user (foreign key constraint)
        sqlx::query("INSERT INTO users (id) VALUES ($1)")
            .bind(user_id)
            .execute(&pool)
            .await
            .expect("Failed to create user");

        let key = format!("testkey-{}", Uuid::new_v4().simple());
        // In real usage this would be the output of auth::encrypt_secret().
        let secret_encrypted = "dGVzdC1lbmNyeXB0ZWQtc2VjcmV0LXBsYWNlaG9sZGVy";

        // Create key
        ApiKeyRepo::create(&pool, &key, secret_encrypted, user_id, "my trading bot")
            .await
            .expect("create failed");

        // find_by_key
        let found = ApiKeyRepo::find_by_key(&pool, &key)
            .await
            .expect("find_by_key failed")
            .expect("key should exist");
        assert_eq!(found.user_id, user_id);
        assert_eq!(found.label, "my trading bot");
        assert!(found.is_active);

        // is_active — should be true
        assert!(ApiKeyRepo::is_active(&pool, &key).await.expect("is_active failed"));

        // list_for_user
        let keys = ApiKeyRepo::list_for_user(&pool, user_id)
            .await
            .expect("list_for_user failed");
        assert!(!keys.is_empty());
        assert!(keys.iter().any(|k| k.key == key));

        // Revoke
        ApiKeyRepo::revoke(&pool, &key).await.expect("revoke failed");

        // is_active — should now be false
        assert!(!ApiKeyRepo::is_active(&pool, &key).await.expect("is_active after revoke failed"));

        // find_by_key still returns the row (soft delete)
        let after_revoke = ApiKeyRepo::find_by_key(&pool, &key)
            .await
            .expect("find after revoke failed")
            .expect("row should still exist");
        assert!(!after_revoke.is_active);

        // Non-existent key returns false from is_active
        let missing = ApiKeyRepo::is_active(&pool, "does-not-exist")
            .await
            .expect("is_active for missing key failed");
        assert!(!missing);
    }
}
