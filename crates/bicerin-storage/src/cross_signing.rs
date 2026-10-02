use crate::{db::StorageResult, store::Store};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct CrossSigningKeyRecord {
    pub user_id: String,
    pub key_type: String,
    pub key_json: serde_json::Value,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct DeviceKeyChangeRecord {
    pub user_id: String,
    pub stream_id: i64,
    pub change_type: String,
    pub created_at: DateTime<Utc>,
}

pub async fn upsert_key(store: &Store, record: &CrossSigningKeyRecord) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::upsert_key(pool, record).await,
        Store::Mongo(backend) => mongo::upsert_key(&backend.database, record).await,
    }
}

pub async fn get_keys(store: &Store, user_id: &str) -> StorageResult<Vec<CrossSigningKeyRecord>> {
    match store {
        Store::Postgres(pool) => pg::get_keys(pool, user_id).await,
        Store::Mongo(backend) => mongo::get_keys(&backend.database, user_id).await,
    }
}

pub async fn record_device_change(
    store: &Store,
    user_id: &str,
    stream_id: i64,
    change_type: &str,
) -> StorageResult<()> {
    let record = DeviceKeyChangeRecord {
        user_id: user_id.to_string(),
        stream_id,
        change_type: change_type.to_string(),
        created_at: Utc::now(),
    };
    match store {
        Store::Postgres(pool) => pg::record_device_change(pool, &record).await,
        Store::Mongo(backend) => mongo::record_device_change(&backend.database, &record).await,
    }
}

pub async fn get_device_changes(
    store: &Store,
    since: i64,
    to: i64,
) -> StorageResult<Vec<DeviceKeyChangeRecord>> {
    match store {
        Store::Postgres(pool) => pg::get_device_changes(pool, since, to).await,
        Store::Mongo(backend) => mongo::get_device_changes(&backend.database, since, to).await,
    }
}

mod pg {
    use super::{CrossSigningKeyRecord, DeviceKeyChangeRecord};
    use crate::db::StorageResult;

    pub async fn upsert_key(
        pool: &sqlx::PgPool,
        record: &CrossSigningKeyRecord,
    ) -> StorageResult<()> {
        sqlx::query("INSERT INTO cross_signing_keys(user_id,key_type,key_json,updated_at) VALUES($1,$2,$3,$4) ON CONFLICT(user_id,key_type) DO UPDATE SET key_json=$3,updated_at=$4")
            .bind(&record.user_id).bind(&record.key_type).bind(&record.key_json).bind(record.updated_at).execute(pool).await?;
        Ok(())
    }

    pub async fn get_keys(
        pool: &sqlx::PgPool,
        user_id: &str,
    ) -> StorageResult<Vec<CrossSigningKeyRecord>> {
        Ok(sqlx::query_as::<_, CrossSigningKeyRecord>(
            "SELECT user_id,key_type,key_json,updated_at FROM cross_signing_keys WHERE user_id=$1",
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?)
    }

    pub async fn record_device_change(
        pool: &sqlx::PgPool,
        record: &DeviceKeyChangeRecord,
    ) -> StorageResult<()> {
        sqlx::query("INSERT INTO device_key_changes(user_id,stream_id,change_type,created_at) VALUES($1,$2,$3,$4)").bind(&record.user_id).bind(record.stream_id).bind(&record.change_type).bind(record.created_at).execute(pool).await?;
        Ok(())
    }

    pub async fn get_device_changes(
        pool: &sqlx::PgPool,
        since: i64,
        to: i64,
    ) -> StorageResult<Vec<DeviceKeyChangeRecord>> {
        Ok(sqlx::query_as::<_, DeviceKeyChangeRecord>("SELECT DISTINCT ON(user_id) user_id,stream_id,change_type,created_at FROM device_key_changes WHERE stream_id>$1 AND stream_id<=$2 ORDER BY user_id,stream_id DESC")
            .bind(since).bind(to).fetch_all(pool).await?)
    }
}

mod mongo {
    use super::{CrossSigningKeyRecord, DeviceKeyChangeRecord};
    use crate::db::StorageResult;
    use futures::stream::TryStreamExt;
    use mongodb::bson::doc;
    use mongodb::Database;

    pub async fn upsert_key(db: &Database, record: &CrossSigningKeyRecord) -> StorageResult<()> {
        db.collection::<CrossSigningKeyRecord>("cross_signing_keys")
            .replace_one(
                doc! {"user_id": &record.user_id, "key_type": &record.key_type},
                record,
            )
            .upsert(true)
            .await?;
        Ok(())
    }

    pub async fn get_keys(
        db: &Database,
        user_id: &str,
    ) -> StorageResult<Vec<CrossSigningKeyRecord>> {
        let cursor = db
            .collection::<CrossSigningKeyRecord>("cross_signing_keys")
            .find(doc! {"user_id": user_id})
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn record_device_change(
        db: &Database,
        record: &DeviceKeyChangeRecord,
    ) -> StorageResult<()> {
        db.collection::<DeviceKeyChangeRecord>("device_key_changes")
            .insert_one(record)
            .await?;
        Ok(())
    }

    pub async fn get_device_changes(
        db: &Database,
        since: i64,
        to: i64,
    ) -> StorageResult<Vec<DeviceKeyChangeRecord>> {
        let pipeline = vec![
            mongodb::bson::doc! {"$match": {"stream_id": {"$gt": since, "$lte": to}}},
            mongodb::bson::doc! {"$sort": {"stream_id": -1}},
            mongodb::bson::doc! {"$group": {"_id": "$user_id", "stream_id": {"$first": "$stream_id"}, "change_type": {"$first": "$change_type"}, "created_at": {"$first": "$created_at"}}},
        ];
        let cursor = db
            .collection::<DeviceKeyChangeRecord>("device_key_changes")
            .aggregate(pipeline)
            .await?;
        let docs: Vec<mongodb::bson::Document> = cursor.try_collect().await?;
        let changes = docs
            .into_iter()
            .filter_map(|doc| {
                Some(DeviceKeyChangeRecord {
                    user_id: doc.get_str("_id").ok()?.to_string(),
                    stream_id: doc.get_i64("stream_id").ok()?,
                    change_type: doc.get_str("change_type").ok()?.to_string(),
                    created_at: doc.get_datetime("created_at").ok()?.to_chrono(),
                })
            })
            .collect::<Vec<_>>();
        Ok(changes)
    }
}
