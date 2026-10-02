#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct DeviceKeyRecord {
    pub user_id: String,
    pub device_id: String,
    pub key_json: serde_json::Value,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct OneTimeKeyRecord {
    pub user_id: String,
    pub device_id: String,
    pub key_id: String,
    pub key_json: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct FallbackKeyRecord {
    pub user_id: String,
    pub device_id: String,
    pub algorithm: String,
    #[serde(default)]
    pub key_id: String,
    pub key_json: serde_json::Value,
    pub used: bool,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

use crate::db::StorageResult;
use crate::store::Store;
use std::collections::HashMap;

pub async fn upsert_device_keys(store: &Store, record: &DeviceKeyRecord) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::upsert_device_keys(pool, record).await,
        Store::Mongo(backend) => mongo::upsert_device_keys(&backend.database, record).await,
    }
}

pub async fn get_device_keys(
    store: &Store,
    user_id: &str,
    device_id: &str,
) -> StorageResult<DeviceKeyRecord> {
    match store {
        Store::Postgres(pool) => pg::get_device_keys(pool, user_id, device_id).await,
        Store::Mongo(backend) => {
            mongo::get_device_keys(&backend.database, user_id, device_id).await
        }
    }
}

pub async fn get_all_device_keys_for_user(
    store: &Store,
    user_id: &str,
) -> StorageResult<Vec<DeviceKeyRecord>> {
    match store {
        Store::Postgres(pool) => pg::get_all_device_keys_for_user(pool, user_id).await,
        Store::Mongo(backend) => {
            mongo::get_all_device_keys_for_user(&backend.database, user_id).await
        }
    }
}

pub async fn delete_device_crypto_material(
    store: &Store,
    user_id: &str,
    device_id: &str,
) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => {
            let mut tx = pool.begin().await?;
            sqlx::query("DELETE FROM device_keys WHERE user_id=$1 AND device_id=$2")
                .bind(user_id)
                .bind(device_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM one_time_keys WHERE user_id=$1 AND device_id=$2")
                .bind(user_id)
                .bind(device_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM fallback_keys WHERE user_id=$1 AND device_id=$2")
                .bind(user_id)
                .bind(device_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(())
        }
        Store::Mongo(backend) => {
            mongo::delete_device_crypto_material(&backend.database, user_id, device_id).await
        }
    }
}

pub async fn insert_one_time_keys(
    store: &Store,
    records: &[OneTimeKeyRecord],
) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::insert_one_time_keys(pool, records).await,
        Store::Mongo(backend) => mongo::insert_one_time_keys(&backend.database, records).await,
    }
}

pub async fn claim_one_time_key(
    store: &Store,
    user_id: &str,
    device_id: &str,
    algorithm: &str,
) -> StorageResult<Option<OneTimeKeyRecord>> {
    match store {
        Store::Postgres(pool) => pg::claim_one_time_key(pool, user_id, device_id, algorithm).await,
        Store::Mongo(backend) => {
            mongo::claim_one_time_key(&backend.database, user_id, device_id, algorithm).await
        }
    }
}

pub async fn count_one_time_keys(
    store: &Store,
    user_id: &str,
    device_id: &str,
) -> StorageResult<HashMap<String, u64>> {
    match store {
        Store::Postgres(pool) => pg::count_one_time_keys(pool, user_id, device_id).await,
        Store::Mongo(backend) => {
            mongo::count_one_time_keys(&backend.database, user_id, device_id).await
        }
    }
}

pub async fn upsert_fallback_key(store: &Store, record: &FallbackKeyRecord) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::upsert_fallback_key(pool, record).await,
        Store::Mongo(backend) => mongo::upsert_fallback_key(&backend.database, record).await,
    }
}

pub async fn get_fallback_key(
    store: &Store,
    user_id: &str,
    device_id: &str,
    algorithm: &str,
) -> StorageResult<Option<FallbackKeyRecord>> {
    match store {
        Store::Postgres(pool) => pg::get_fallback_key(pool, user_id, device_id, algorithm).await,
        Store::Mongo(backend) => {
            mongo::get_fallback_key(&backend.database, user_id, device_id, algorithm).await
        }
    }
}

pub async fn count_unused_fallback_keys(
    store: &Store,
    user_id: &str,
    device_id: &str,
) -> StorageResult<HashMap<String, u64>> {
    match store {
        Store::Postgres(pool) => pg::count_unused_fallback_keys(pool, user_id, device_id).await,
        Store::Mongo(backend) => {
            mongo::count_unused_fallback_keys(&backend.database, user_id, device_id).await
        }
    }
}

mod pg {
    use super::{DeviceKeyRecord, FallbackKeyRecord, OneTimeKeyRecord};
    use crate::db::{StorageError, StorageResult};
    use std::collections::HashMap;

    pub async fn upsert_device_keys(
        pool: &sqlx::PgPool,
        record: &DeviceKeyRecord,
    ) -> StorageResult<()> {
        sqlx::query(
            "INSERT INTO device_keys (user_id, device_id, key_json, updated_at) VALUES ($1, $2, $3, $4) ON CONFLICT (user_id, device_id) DO UPDATE SET key_json = $3, updated_at = $4"
        )
        .bind(&record.user_id)
        .bind(&record.device_id)
        .bind(&record.key_json)
        .bind(record.updated_at)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_device_keys(
        pool: &sqlx::PgPool,
        user_id: &str,
        device_id: &str,
    ) -> StorageResult<DeviceKeyRecord> {
        sqlx::query_as::<_, DeviceKeyRecord>(
            "SELECT user_id, device_id, key_json, updated_at FROM device_keys WHERE user_id = $1 AND device_id = $2"
        )
        .bind(user_id)
        .bind(device_id)
        .fetch_optional(pool)
        .await?
        .ok_or(StorageError::NotFound)
    }

    pub async fn get_all_device_keys_for_user(
        pool: &sqlx::PgPool,
        user_id: &str,
    ) -> StorageResult<Vec<DeviceKeyRecord>> {
        let records = sqlx::query_as::<_, DeviceKeyRecord>(
            "SELECT user_id, device_id, key_json, updated_at FROM device_keys WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;
        Ok(records)
    }

    pub async fn insert_one_time_keys(
        pool: &sqlx::PgPool,
        records: &[OneTimeKeyRecord],
    ) -> StorageResult<()> {
        for record in records {
            sqlx::query(
                "INSERT INTO one_time_keys (user_id, device_id, key_id, key_json, created_at) VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING"
            )
            .bind(&record.user_id)
            .bind(&record.device_id)
            .bind(&record.key_id)
            .bind(&record.key_json)
            .bind(record.created_at)
            .execute(pool)
            .await?;
        }
        Ok(())
    }

    pub async fn claim_one_time_key(
        pool: &sqlx::PgPool,
        user_id: &str,
        device_id: &str,
        algorithm: &str,
    ) -> StorageResult<Option<OneTimeKeyRecord>> {
        let pattern = format!("{}:%", algorithm);
        let row = sqlx::query_as::<_, OneTimeKeyRecord>(
            "DELETE FROM one_time_keys WHERE ctid = (SELECT ctid FROM one_time_keys WHERE user_id = $1 AND device_id = $2 AND key_id LIKE $3 LIMIT 1) RETURNING user_id, device_id, key_id, key_json, created_at"
        )
        .bind(user_id)
        .bind(device_id)
        .bind(&pattern)
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    pub async fn count_one_time_keys(
        pool: &sqlx::PgPool,
        user_id: &str,
        device_id: &str,
    ) -> StorageResult<HashMap<String, u64>> {
        #[derive(sqlx::FromRow)]
        struct Row {
            algorithm: String,
            count: i64,
        }

        let rows: Vec<Row> = sqlx::query_as(
            "SELECT split_part(key_id, ':', 1) AS algorithm, COUNT(*) AS count FROM one_time_keys WHERE user_id = $1 AND device_id = $2 GROUP BY split_part(key_id, ':', 1)"
        )
        .bind(user_id)
        .bind(device_id)
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| (r.algorithm, r.count as u64))
            .collect())
    }

    pub async fn upsert_fallback_key(
        pool: &sqlx::PgPool,
        record: &FallbackKeyRecord,
    ) -> StorageResult<()> {
        sqlx::query(
            "INSERT INTO fallback_keys (user_id, device_id, algorithm, key_id, key_json, used, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (user_id, device_id, algorithm) DO UPDATE SET key_id = $4, key_json = $5, used = $6, updated_at = $7"
        )
        .bind(&record.user_id)
        .bind(&record.device_id)
        .bind(&record.algorithm)
        .bind(&record.key_id)
        .bind(&record.key_json)
        .bind(record.used)
        .bind(record.updated_at)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_fallback_key(
        pool: &sqlx::PgPool,
        user_id: &str,
        device_id: &str,
        algorithm: &str,
    ) -> StorageResult<Option<FallbackKeyRecord>> {
        let record = sqlx::query_as::<_, FallbackKeyRecord>("UPDATE fallback_keys SET used=TRUE WHERE user_id=$1 AND device_id=$2 AND algorithm=$3 RETURNING user_id,device_id,algorithm,key_id,key_json,used,updated_at")
            .bind(user_id).bind(device_id).bind(algorithm).fetch_optional(pool).await?;
        Ok(record)
    }

    pub async fn count_unused_fallback_keys(
        pool: &sqlx::PgPool,
        user_id: &str,
        device_id: &str,
    ) -> StorageResult<HashMap<String, u64>> {
        #[derive(sqlx::FromRow)]
        struct Row {
            algorithm: String,
            count: i64,
        }
        let rows = sqlx::query_as::<_, Row>("SELECT algorithm, COUNT(*) AS count FROM fallback_keys WHERE user_id=$1 AND device_id=$2 AND used=FALSE GROUP BY algorithm")
            .bind(user_id).bind(device_id).fetch_all(pool).await?;
        Ok(rows
            .into_iter()
            .map(|row| (row.algorithm, row.count.max(0) as u64))
            .collect())
    }
}

mod mongo {
    use super::{DeviceKeyRecord, FallbackKeyRecord, OneTimeKeyRecord};
    use crate::db::{is_duplicate_key_error, StorageError, StorageResult};
    use futures::stream::TryStreamExt;
    use mongodb::bson::{doc, Document};
    use mongodb::options::ReturnDocument;
    use mongodb::Database;
    use std::collections::HashMap;

    fn device_keys(db: &Database) -> mongodb::Collection<DeviceKeyRecord> {
        db.collection("device_keys")
    }
    fn one_time_keys(db: &Database) -> mongodb::Collection<OneTimeKeyRecord> {
        db.collection("one_time_keys")
    }
    fn fallback_keys(db: &Database) -> mongodb::Collection<FallbackKeyRecord> {
        db.collection("fallback_keys")
    }

    pub async fn upsert_device_keys(db: &Database, record: &DeviceKeyRecord) -> StorageResult<()> {
        device_keys(db)
            .find_one_and_replace(
                doc! { "user_id": &record.user_id, "device_id": &record.device_id },
                record,
            )
            .upsert(true)
            .await?;
        Ok(())
    }

    pub async fn get_device_keys(
        db: &Database,
        user_id: &str,
        device_id: &str,
    ) -> StorageResult<DeviceKeyRecord> {
        device_keys(db)
            .find_one(doc! { "user_id": user_id, "device_id": device_id })
            .await?
            .ok_or(StorageError::NotFound)
    }

    pub async fn get_all_device_keys_for_user(
        db: &Database,
        user_id: &str,
    ) -> StorageResult<Vec<DeviceKeyRecord>> {
        let cursor = device_keys(db).find(doc! { "user_id": user_id }).await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn delete_device_crypto_material(
        db: &Database,
        user_id: &str,
        device_id: &str,
    ) -> StorageResult<()> {
        device_keys(db)
            .delete_one(doc! {"user_id": user_id, "device_id": device_id})
            .await?;
        one_time_keys(db)
            .delete_many(doc! {"user_id": user_id, "device_id": device_id})
            .await?;
        fallback_keys(db)
            .delete_many(doc! {"user_id": user_id, "device_id": device_id})
            .await?;
        Ok(())
    }

    pub async fn insert_one_time_keys(
        db: &Database,
        records: &[OneTimeKeyRecord],
    ) -> StorageResult<()> {
        for record in records {
            match one_time_keys(db).insert_one(record).await {
                Ok(_) => {}
                Err(e) if is_duplicate_key_error(&e) => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    pub async fn claim_one_time_key(
        db: &Database,
        user_id: &str,
        device_id: &str,
        algorithm: &str,
    ) -> StorageResult<Option<OneTimeKeyRecord>> {
        let pattern = format!("^{}:", regex_escape(algorithm));
        let record = one_time_keys(db)
            .find_one_and_delete(doc! {
                "user_id": user_id,
                "device_id": device_id,
                "key_id": { "$regex": pattern },
            })
            .await?;
        Ok(record)
    }

    pub async fn count_one_time_keys(
        db: &Database,
        user_id: &str,
        device_id: &str,
    ) -> StorageResult<HashMap<String, u64>> {
        let collection: mongodb::Collection<Document> = db.collection("one_time_keys");
        let pipeline = vec![
            doc! { "$match": { "user_id": user_id, "device_id": device_id } },
            doc! { "$group": {
                "_id": { "$arrayElemAt": [{ "$split": ["$key_id", ":"] }, 0] },
                "count": { "$sum": 1 },
            } },
        ];
        let mut cursor = collection.aggregate(pipeline).await?;
        let mut counts = HashMap::new();
        while let Some(doc) = cursor.try_next().await? {
            if let (Ok(algorithm), Ok(count)) = (doc.get_str("_id"), doc.get_i32("count")) {
                counts.insert(algorithm.to_string(), count as u64);
            }
        }
        Ok(counts)
    }

    pub async fn upsert_fallback_key(
        db: &Database,
        record: &FallbackKeyRecord,
    ) -> StorageResult<()> {
        fallback_keys(db)
            .find_one_and_replace(
                doc! { "user_id": &record.user_id, "device_id": &record.device_id, "algorithm": &record.algorithm },
                record,
            )
            .upsert(true)
            .return_document(ReturnDocument::After)
            .await?;
        Ok(())
    }

    pub async fn get_fallback_key(
        db: &Database,
        user_id: &str,
        device_id: &str,
        algorithm: &str,
    ) -> StorageResult<Option<FallbackKeyRecord>> {
        let record = fallback_keys(db)
            .find_one_and_update(
                doc! {"user_id": user_id, "device_id": device_id, "algorithm": algorithm},
                doc! {"$set": {"used": true}},
            )
            .return_document(ReturnDocument::After)
            .await?;
        Ok(record)
    }

    pub async fn count_unused_fallback_keys(
        db: &Database,
        user_id: &str,
        device_id: &str,
    ) -> StorageResult<HashMap<String, u64>> {
        let pipeline = vec![
            doc! {"$match": {"user_id": user_id, "device_id": device_id, "used": false}},
            doc! {"$group": {"_id": "$algorithm", "count": {"$sum": 1}}},
        ];
        let mut cursor = db
            .collection::<Document>("fallback_keys")
            .aggregate(pipeline)
            .await?;
        let mut counts = HashMap::new();
        while let Some(doc) = cursor.try_next().await? {
            if let (Ok(algorithm), Ok(count)) = (doc.get_str("_id"), doc.get_i32("count")) {
                counts.insert(algorithm.to_string(), count.max(0) as u64);
            }
        }
        Ok(counts)
    }

    /// Escapes regex metacharacters so `algorithm` is matched literally.
    fn regex_escape(input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        for c in input.chars() {
            if "\\.+*?()|[]{}^$".contains(c) {
                out.push('\\');
            }
            out.push(c);
        }
        out
    }
}
