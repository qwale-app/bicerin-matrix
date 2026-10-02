use crate::{db::StorageResult, store::Store};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PresenceRecord {
    pub user_id: String,
    pub presence: String,
    pub status_msg: Option<String>,
    pub last_active_ts: i64,
    pub currently_active: bool,
    pub stream_id: i64,
    pub updated_at: DateTime<Utc>,
}

pub async fn set_presence(store: &Store, record: &PresenceRecord) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::set_presence(pool, record).await,
        Store::Mongo(backend) => mongo::set_presence(&backend.database, record).await,
    }
}

pub async fn get_presence(store: &Store, user_id: &str) -> StorageResult<Option<PresenceRecord>> {
    match store {
        Store::Postgres(pool) => pg::get_presence(pool, user_id).await,
        Store::Mongo(backend) => mongo::get_presence(&backend.database, user_id).await,
    }
}

pub async fn get_presence_updates_for_users(
    store: &Store,
    user_ids: &[String],
    since_stream_id: i64,
) -> StorageResult<Vec<PresenceRecord>> {
    if user_ids.is_empty() {
        return Ok(vec![]);
    }
    match store {
        Store::Postgres(pool) => {
            pg::get_presence_updates_for_users(pool, user_ids, since_stream_id).await
        }
        Store::Mongo(backend) => {
            mongo::get_presence_updates_for_users(&backend.database, user_ids, since_stream_id)
                .await
        }
    }
}

mod pg {
    use super::PresenceRecord;
    use crate::db::StorageResult;

    pub async fn set_presence(pool: &sqlx::PgPool, record: &PresenceRecord) -> StorageResult<()> {
        sqlx::query(
            "INSERT INTO user_presence(user_id,presence,status_msg,last_active_ts,currently_active,stream_id,updated_at) \
             VALUES($1,$2,$3,$4,$5,$6,$7) \
             ON CONFLICT(user_id) DO UPDATE SET presence=$2,status_msg=$3,last_active_ts=$4,currently_active=$5,stream_id=$6,updated_at=$7"
        )
        .bind(&record.user_id)
        .bind(&record.presence)
        .bind(&record.status_msg)
        .bind(record.last_active_ts)
        .bind(record.currently_active)
        .bind(record.stream_id)
        .bind(record.updated_at)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_presence(
        pool: &sqlx::PgPool,
        user_id: &str,
    ) -> StorageResult<Option<PresenceRecord>> {
        Ok(sqlx::query_as::<_, PresenceRecord>(
            "SELECT user_id,presence,status_msg,last_active_ts,currently_active,stream_id,updated_at FROM user_presence WHERE user_id=$1"
        )
        .bind(user_id)
        .fetch_optional(pool)
        .await?)
    }

    pub async fn get_presence_updates_for_users(
        pool: &sqlx::PgPool,
        user_ids: &[String],
        since_stream_id: i64,
    ) -> StorageResult<Vec<PresenceRecord>> {
        Ok(sqlx::query_as::<_, PresenceRecord>(
            "SELECT user_id,presence,status_msg,last_active_ts,currently_active,stream_id,updated_at FROM user_presence WHERE user_id = ANY($1) AND stream_id > $2"
        )
        .bind(user_ids)
        .bind(since_stream_id)
        .fetch_all(pool)
        .await?)
    }
}

mod mongo {
    use super::PresenceRecord;
    use crate::db::StorageResult;
    use futures::stream::TryStreamExt;
    use mongodb::bson::doc;
    use mongodb::Database;

    fn presence(db: &Database) -> mongodb::Collection<PresenceRecord> {
        db.collection("user_presence")
    }

    pub async fn set_presence(db: &Database, record: &PresenceRecord) -> StorageResult<()> {
        presence(db)
            .find_one_and_replace(doc! { "user_id": &record.user_id }, record)
            .upsert(true)
            .await?;
        Ok(())
    }

    pub async fn get_presence(
        db: &Database,
        user_id: &str,
    ) -> StorageResult<Option<PresenceRecord>> {
        Ok(presence(db).find_one(doc! { "user_id": user_id }).await?)
    }

    pub async fn get_presence_updates_for_users(
        db: &Database,
        user_ids: &[String],
        since_stream_id: i64,
    ) -> StorageResult<Vec<PresenceRecord>> {
        let cursor = presence(db)
            .find(doc! {
                "user_id": { "$in": user_ids },
                "stream_id": { "$gt": since_stream_id },
            })
            .await?;
        Ok(cursor.try_collect().await?)
    }
}
