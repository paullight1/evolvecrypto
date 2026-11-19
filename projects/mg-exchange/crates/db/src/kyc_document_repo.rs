use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct KycDocumentRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub document_type: String,
    pub file_name: String,
    pub file_size: i64,
    pub content_type: String,
    pub storage_path: String,
    pub file_hash: String,
    pub status: String,
    pub reviewer_notes: Option<String>,
    pub uploaded_at: DateTime<Utc>,
    pub reviewed_at: Option<DateTime<Utc>>,
}

pub struct KycDocumentRepo;

impl KycDocumentRepo {
    pub async fn create(pool: &PgPool, doc: &KycDocumentRow) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO kyc_documents \
             (id, user_id, document_type, file_name, file_size, content_type, storage_path, file_hash, status) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'pending')",
        )
        .bind(doc.id)
        .bind(doc.user_id)
        .bind(&doc.document_type)
        .bind(&doc.file_name)
        .bind(doc.file_size)
        .bind(&doc.content_type)
        .bind(&doc.storage_path)
        .bind(&doc.file_hash)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn list_for_user(
        pool: &PgPool,
        user_id: Uuid,
    ) -> Result<Vec<KycDocumentRow>, sqlx::Error> {
        sqlx::query_as::<_, KycDocumentRow>(
            "SELECT * FROM kyc_documents WHERE user_id = $1 ORDER BY uploaded_at DESC",
        )
        .bind(user_id)
        .fetch_all(pool)
        .await
    }

    pub async fn list_pending(
        pool: &PgPool,
        limit: i64,
    ) -> Result<Vec<KycDocumentRow>, sqlx::Error> {
        sqlx::query_as::<_, KycDocumentRow>(
            "SELECT * FROM kyc_documents WHERE status = 'pending' ORDER BY uploaded_at ASC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(pool)
        .await
    }

    pub async fn update_status(
        pool: &PgPool,
        id: Uuid,
        status: &str,
        notes: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE kyc_documents \
             SET status = $2, reviewer_notes = $3, reviewed_at = NOW() \
             WHERE id = $1",
        )
        .bind(id)
        .bind(status)
        .bind(notes)
        .execute(pool)
        .await?;
        Ok(())
    }
}
