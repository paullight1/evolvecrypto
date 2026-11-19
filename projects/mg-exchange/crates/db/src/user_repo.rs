use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

/// User row from the database.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UserRow {
    pub id: Uuid,
    pub email: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct UserRepo;

impl UserRepo {
    /// Create a new user.
    pub async fn create(pool: &PgPool, id: Uuid, email: Option<&str>) -> Result<UserRow, sqlx::Error> {
        let row = sqlx::query_as::<_, UserRow>(
            "INSERT INTO users (id, email) VALUES ($1, $2)
             RETURNING id, email, created_at, updated_at"
        )
        .bind(id)
        .bind(email)
        .fetch_one(pool)
        .await?;

        Ok(row)
    }

    /// Get user by ID.
    pub async fn get(pool: &PgPool, id: Uuid) -> Result<Option<UserRow>, sqlx::Error> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, email, created_at, updated_at FROM users WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    /// List all users.
    pub async fn list_all(pool: &PgPool) -> Result<Vec<UserRow>, sqlx::Error> {
        sqlx::query_as::<_, UserRow>(
            "SELECT id, email, created_at, updated_at FROM users ORDER BY created_at"
        )
        .fetch_all(pool)
        .await
    }

    /// Check if user exists.
    pub async fn exists(pool: &PgPool, id: Uuid) -> Result<bool, sqlx::Error> {
        let result = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)"
        )
        .bind(id)
        .fetch_one(pool)
        .await?;

        Ok(result)
    }
}
