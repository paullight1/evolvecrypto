use sha2::{Digest, Sha256};
use std::path::PathBuf;
use uuid::Uuid;

/// Local filesystem storage for KYC documents.
///
/// Files are stored under `<base_path>/<user_id>/<doc_type>_<uuid>.<ext>`.
/// The storage path is configured via the `DOCUMENT_STORAGE_PATH` environment
/// variable (default: `./data/kyc-docs`).
pub struct LocalDocumentStorage {
    base_path: PathBuf,
}

impl LocalDocumentStorage {
    /// Create storage using the path from `DOCUMENT_STORAGE_PATH` env var,
    /// falling back to `./data/kyc-docs`.
    pub fn new() -> Self {
        let path = std::env::var("DOCUMENT_STORAGE_PATH")
            .unwrap_or_else(|_| "./data/kyc-docs".to_string());
        Self {
            base_path: PathBuf::from(path),
        }
    }

    /// Store a document file on disk.
    ///
    /// Creates the per-user directory if it does not exist. Returns
    /// `(storage_path, sha256_hex)` on success.
    pub async fn store(
        &self,
        user_id: Uuid,
        doc_type: &str,
        data: &[u8],
        ext: &str,
    ) -> Result<(String, String), std::io::Error> {
        let user_dir = self.base_path.join(user_id.to_string());
        tokio::fs::create_dir_all(&user_dir).await?;

        let file_name = format!("{}_{}.{}", doc_type, Uuid::new_v4().simple(), ext);
        let file_path = user_dir.join(&file_name);

        let hash = format!("{:x}", Sha256::digest(data));

        tokio::fs::write(&file_path, data).await?;

        Ok((file_path.to_string_lossy().into_owned(), hash))
    }
}

impl Default for LocalDocumentStorage {
    fn default() -> Self {
        Self::new()
    }
}
