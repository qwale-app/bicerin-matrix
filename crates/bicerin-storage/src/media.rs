use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct MediaRecord {
    pub media_id: String,
    pub server_name: String,
    pub uploader: Option<String>,
    pub mime_type: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub storage_key: String,
    pub upload_name: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

use crate::db::StorageResult;
use crate::store::Store;

pub async fn create_media(store: &Store, media: &MediaRecord) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::create_media(pool, media).await,
        Store::Mongo(backend) => mongo::create_media(&backend.database, media).await,
    }
}

pub async fn get_media(
    store: &Store,
    server_name: &str,
    media_id: &str,
) -> StorageResult<MediaRecord> {
    match store {
        Store::Postgres(pool) => pg::get_media(pool, server_name, media_id).await,
        Store::Mongo(backend) => mongo::get_media(&backend.database, server_name, media_id).await,
    }
}

mod pg {
    use super::MediaRecord;
    use crate::db::{StorageError, StorageResult};

    pub async fn create_media(pool: &sqlx::PgPool, media: &MediaRecord) -> StorageResult<()> {
        sqlx::query(
            "INSERT INTO media (media_id, server_name, uploader, mime_type, size_bytes, sha256, storage_key, upload_name, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"
        )
        .bind(&media.media_id)
        .bind(&media.server_name)
        .bind(&media.uploader)
        .bind(&media.mime_type)
        .bind(media.size_bytes)
        .bind(&media.sha256)
        .bind(&media.storage_key)
        .bind(&media.upload_name)
        .bind(media.created_at)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_media(
        pool: &sqlx::PgPool,
        server_name: &str,
        media_id: &str,
    ) -> StorageResult<MediaRecord> {
        sqlx::query_as::<_, MediaRecord>(
            "SELECT media_id, server_name, uploader, mime_type, size_bytes, sha256, storage_key, upload_name, created_at FROM media WHERE server_name = $1 AND media_id = $2"
        )
        .bind(server_name)
        .bind(media_id)
        .fetch_optional(pool)
        .await?
        .ok_or(StorageError::NotFound)
    }
}

mod mongo {
    use super::MediaRecord;
    use crate::db::{StorageError, StorageResult};
    use mongodb::bson::doc;
    use mongodb::Database;

    fn media(db: &Database) -> mongodb::Collection<MediaRecord> {
        db.collection("media")
    }

    pub async fn create_media(db: &Database, record: &MediaRecord) -> StorageResult<()> {
        media(db).insert_one(record).await?;
        Ok(())
    }

    pub async fn get_media(
        db: &Database,
        server_name: &str,
        media_id: &str,
    ) -> StorageResult<MediaRecord> {
        media(db)
            .find_one(doc! { "server_name": server_name, "media_id": media_id })
            .await?
            .ok_or(StorageError::NotFound)
    }
}
