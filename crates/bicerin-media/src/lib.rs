//! Minimal local-disk media store.
//!
//! Bicerin's design keeps media metadata in PostgreSQL and the encrypted/opaque
//! bytes in object storage. This crate currently implements only the local
//! filesystem backend; an S3-compatible backend is a follow-up milestone.

use bicerin_error::{BicerinError, BicerinResult};
use bicerin_storage::media::MediaRecord;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

#[derive(Clone)]
pub struct MediaService {
    store: bicerin_storage::Store,
    root: PathBuf,
    server_name: String,
    max_upload_size: u64,
}

impl MediaService {
    pub fn new(store: bicerin_storage::Store, root: impl AsRef<Path>, server_name: String, max_upload_size: u64) -> Self {
        Self {
            store,
            root: root.as_ref().to_path_buf(),
            server_name,
            max_upload_size,
        }
    }

    fn storage_path(&self, storage_key: &str) -> PathBuf {
        // Shard by the first 4 hex characters to avoid huge flat directories.
        let (prefix, _) = storage_key.split_at(storage_key.len().min(4));
        self.root.join(prefix).join(storage_key)
    }

    pub async fn upload(
        &self,
        uploader: &str,
        mime_type: &str,
        upload_name: Option<String>,
        bytes: Vec<u8>,
    ) -> BicerinResult<String> {
        if bytes.len() as u64 > self.max_upload_size {
            return Err(BicerinError::MatrixError {
                errcode: "M_TOO_LARGE".to_string(),
                error: "Upload exceeds maximum allowed size".to_string(),
            });
        }

        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let sha256 = hex::encode(hasher.finalize());

        let media_id = generate_media_id();
        let storage_key = media_id.clone();
        let path = self.storage_path(&storage_key);

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| BicerinError::Internal(format!("failed to create media dir: {e}")))?;
        }

        let mut file = tokio::fs::File::create(&path)
            .await
            .map_err(|e| BicerinError::Internal(format!("failed to create media file: {e}")))?;
        file.write_all(&bytes)
            .await
            .map_err(|e| BicerinError::Internal(format!("failed to write media file: {e}")))?;

        let record = MediaRecord {
            media_id: media_id.clone(),
            server_name: self.server_name.clone(),
            uploader: Some(uploader.to_string()),
            mime_type: mime_type.to_string(),
            size_bytes: bytes.len() as i64,
            sha256,
            storage_key,
            upload_name,
            created_at: chrono::Utc::now(),
        };

        bicerin_storage::media::create_media(&self.store, &record)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;

        Ok(media_id)
    }

    pub async fn download(&self, server_name: &str, media_id: &str) -> BicerinResult<(MediaRecord, Vec<u8>)> {
        let record = bicerin_storage::media::get_media(&self.store, server_name, media_id)
            .await
            .map_err(|_| BicerinError::NotFound)?;

        let path = self.storage_path(&record.storage_key);
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|_| BicerinError::NotFound)?;

        Ok((record, bytes))
    }
}

fn generate_media_id() -> String {
    use rand::Rng;
    let bytes: Vec<u8> = (0..24).map(|_| rand::thread_rng().gen()).collect();
    hex::encode(bytes)
}
