use crate::{db::StorageResult, store::Store};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct BackupVersionRecord {
    pub user_id: String,
    pub version: String,
    pub algorithm: String,
    pub auth_data: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct BackupSessionRecord {
    pub user_id: String,
    pub version: String,
    pub room_id: String,
    pub session_id: String,
    pub data: serde_json::Value,
    pub updated_at: DateTime<Utc>,
}

pub async fn create_version(store: &Store, record: &BackupVersionRecord) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::create_version(pool, record).await,
        Store::Mongo(backend) => mongo::create_version(&backend.database, record).await,
    }
}

pub async fn get_current_version(
    store: &Store,
    user_id: &str,
) -> StorageResult<Option<BackupVersionRecord>> {
    match store {
        Store::Postgres(pool) => pg::get_current_version(pool, user_id).await,
        Store::Mongo(backend) => mongo::get_current_version(&backend.database, user_id).await,
    }
}

pub async fn get_version(
    store: &Store,
    user_id: &str,
    version: &str,
) -> StorageResult<Option<BackupVersionRecord>> {
    match store {
        Store::Postgres(pool) => pg::get_version(pool, user_id, version).await,
        Store::Mongo(backend) => mongo::get_version(&backend.database, user_id, version).await,
    }
}

pub async fn update_version(
    store: &Store,
    user_id: &str,
    version: &str,
    auth_data: serde_json::Value,
) -> StorageResult<bool> {
    match store {
        Store::Postgres(pool) => pg::update_version(pool, user_id, version, auth_data).await,
        Store::Mongo(backend) => {
            mongo::update_version(&backend.database, user_id, version, auth_data).await
        }
    }
}

pub async fn delete_version(store: &Store, user_id: &str, version: &str) -> StorageResult<bool> {
    match store {
        Store::Postgres(pool) => pg::delete_version(pool, user_id, version).await,
        Store::Mongo(backend) => mongo::delete_version(&backend.database, user_id, version).await,
    }
}

pub async fn count_sessions(store: &Store, user_id: &str, version: &str) -> StorageResult<u64> {
    match store {
        Store::Postgres(pool) => pg::count_sessions(pool, user_id, version).await,
        Store::Mongo(backend) => mongo::count_sessions(&backend.database, user_id, version).await,
    }
}

pub async fn upsert_session(store: &Store, record: &BackupSessionRecord) -> StorageResult<bool> {
    match store {
        Store::Postgres(pool) => pg::upsert_session(pool, record).await,
        Store::Mongo(backend) => mongo::upsert_session(&backend.database, record).await,
    }
}

pub async fn get_sessions(
    store: &Store,
    user_id: &str,
    version: &str,
    room_id: Option<&str>,
    session_id: Option<&str>,
) -> StorageResult<Vec<BackupSessionRecord>> {
    match store {
        Store::Postgres(pool) => {
            pg::get_sessions(pool, user_id, version, room_id, session_id).await
        }
        Store::Mongo(backend) => {
            mongo::get_sessions(&backend.database, user_id, version, room_id, session_id).await
        }
    }
}

pub async fn delete_sessions(
    store: &Store,
    user_id: &str,
    version: &str,
    room_id: Option<&str>,
    session_id: Option<&str>,
) -> StorageResult<u64> {
    match store {
        Store::Postgres(pool) => {
            pg::delete_sessions(pool, user_id, version, room_id, session_id).await
        }
        Store::Mongo(backend) => {
            mongo::delete_sessions(&backend.database, user_id, version, room_id, session_id).await
        }
    }
}

/// Returns whether `candidate` is a better copy of an encrypted session than `stored`.
/// Servers must prefer verified keys, then the earliest decryptable index, then fewer forwards.
pub fn backup_key_is_better(stored: &serde_json::Value, candidate: &serde_json::Value) -> bool {
    let stored_verified = stored
        .get("is_verified")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let candidate_verified = candidate
        .get("is_verified")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if stored_verified != candidate_verified {
        return candidate_verified;
    }

    let stored_index = stored
        .get("first_message_index")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(i64::MAX);
    let candidate_index = candidate
        .get("first_message_index")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(i64::MAX);
    if stored_index != candidate_index {
        return candidate_index < stored_index;
    }

    let stored_forwards = stored
        .get("forwarded_count")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(i64::MAX);
    let candidate_forwards = candidate
        .get("forwarded_count")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(i64::MAX);
    candidate_forwards < stored_forwards
}

mod pg {
    use super::{backup_key_is_better, BackupSessionRecord, BackupVersionRecord};
    use crate::db::StorageResult;
    use sqlx::Row;

    pub async fn create_version(
        pool: &sqlx::PgPool,
        record: &BackupVersionRecord,
    ) -> StorageResult<()> {
        sqlx::query("INSERT INTO room_key_backup_versions(user_id,version,algorithm,auth_data,created_at) VALUES($1,$2,$3,$4,$5)")
            .bind(&record.user_id).bind(&record.version).bind(&record.algorithm).bind(&record.auth_data).bind(record.created_at).execute(pool).await?;
        Ok(())
    }

    pub async fn get_current_version(
        pool: &sqlx::PgPool,
        user_id: &str,
    ) -> StorageResult<Option<BackupVersionRecord>> {
        Ok(sqlx::query_as::<_, BackupVersionRecord>("SELECT user_id,version,algorithm,auth_data,created_at FROM room_key_backup_versions WHERE user_id=$1 ORDER BY created_at DESC,version DESC LIMIT 1")
            .bind(user_id).fetch_optional(pool).await?)
    }

    pub async fn get_version(
        pool: &sqlx::PgPool,
        user_id: &str,
        version: &str,
    ) -> StorageResult<Option<BackupVersionRecord>> {
        Ok(sqlx::query_as::<_, BackupVersionRecord>("SELECT user_id,version,algorithm,auth_data,created_at FROM room_key_backup_versions WHERE user_id=$1 AND version=$2")
            .bind(user_id).bind(version).fetch_optional(pool).await?)
    }

    pub async fn update_version(
        pool: &sqlx::PgPool,
        user_id: &str,
        version: &str,
        auth_data: serde_json::Value,
    ) -> StorageResult<bool> {
        let result = sqlx::query(
            "UPDATE room_key_backup_versions SET auth_data=$3 WHERE user_id=$1 AND version=$2",
        )
        .bind(user_id)
        .bind(version)
        .bind(auth_data)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn delete_version(
        pool: &sqlx::PgPool,
        user_id: &str,
        version: &str,
    ) -> StorageResult<bool> {
        let mut tx = pool.begin().await?;
        sqlx::query("DELETE FROM room_key_backup_sessions WHERE user_id=$1 AND version=$2")
            .bind(user_id)
            .bind(version)
            .execute(&mut *tx)
            .await?;
        let result =
            sqlx::query("DELETE FROM room_key_backup_versions WHERE user_id=$1 AND version=$2")
                .bind(user_id)
                .bind(version)
                .execute(&mut *tx)
                .await?;
        tx.commit().await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn count_sessions(
        pool: &sqlx::PgPool,
        user_id: &str,
        version: &str,
    ) -> StorageResult<u64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM room_key_backup_sessions WHERE user_id=$1 AND version=$2",
        )
        .bind(user_id)
        .bind(version)
        .fetch_one(pool)
        .await?;
        Ok(count.max(0) as u64)
    }

    pub async fn upsert_session(
        pool: &sqlx::PgPool,
        record: &BackupSessionRecord,
    ) -> StorageResult<bool> {
        let mut tx = pool.begin().await?;
        let old: Option<serde_json::Value> = sqlx::query("SELECT data FROM room_key_backup_sessions WHERE user_id=$1 AND version=$2 AND room_id=$3 AND session_id=$4 FOR UPDATE")
            .bind(&record.user_id).bind(&record.version).bind(&record.room_id).bind(&record.session_id).fetch_optional(&mut *tx).await?.map(|row| row.try_get("data")).transpose()?;
        if old
            .as_ref()
            .is_some_and(|stored| !backup_key_is_better(stored, &record.data))
        {
            tx.commit().await?;
            return Ok(false);
        }
        sqlx::query("INSERT INTO room_key_backup_sessions(user_id,version,room_id,session_id,data,updated_at) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(user_id,version,room_id,session_id) DO UPDATE SET data=$5,updated_at=$6")
            .bind(&record.user_id).bind(&record.version).bind(&record.room_id).bind(&record.session_id).bind(&record.data).bind(record.updated_at).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(true)
    }

    pub async fn get_sessions(
        pool: &sqlx::PgPool,
        user_id: &str,
        version: &str,
        room_id: Option<&str>,
        session_id: Option<&str>,
    ) -> StorageResult<Vec<BackupSessionRecord>> {
        let rows = sqlx::query_as::<_, BackupSessionRecord>("SELECT user_id,version,room_id,session_id,data,updated_at FROM room_key_backup_sessions WHERE user_id=$1 AND version=$2 AND ($3::text IS NULL OR room_id=$3) AND ($4::text IS NULL OR session_id=$4) ORDER BY room_id,session_id")
            .bind(user_id).bind(version).bind(room_id).bind(session_id).fetch_all(pool).await?;
        Ok(rows)
    }

    pub async fn delete_sessions(
        pool: &sqlx::PgPool,
        user_id: &str,
        version: &str,
        room_id: Option<&str>,
        session_id: Option<&str>,
    ) -> StorageResult<u64> {
        let result = sqlx::query("DELETE FROM room_key_backup_sessions WHERE user_id=$1 AND version=$2 AND ($3::text IS NULL OR room_id=$3) AND ($4::text IS NULL OR session_id=$4)")
            .bind(user_id).bind(version).bind(room_id).bind(session_id).execute(pool).await?;
        Ok(result.rows_affected())
    }
}

mod mongo {
    use super::{backup_key_is_better, BackupSessionRecord, BackupVersionRecord};
    use crate::db::{StorageError, StorageResult};
    use futures::stream::TryStreamExt;
    use mongodb::bson::doc;
    use mongodb::options::ReturnDocument;
    use mongodb::Database;

    fn versions(db: &Database) -> mongodb::Collection<BackupVersionRecord> {
        db.collection("room_key_backup_versions")
    }
    fn sessions(db: &Database) -> mongodb::Collection<BackupSessionRecord> {
        db.collection("room_key_backup_sessions")
    }

    pub async fn create_version(db: &Database, record: &BackupVersionRecord) -> StorageResult<()> {
        versions(db).insert_one(record).await?;
        Ok(())
    }
    pub async fn get_current_version(
        db: &Database,
        user_id: &str,
    ) -> StorageResult<Option<BackupVersionRecord>> {
        Ok(versions(db)
            .find_one(doc! {"user_id": user_id})
            .sort(doc! {"created_at": -1, "version": -1})
            .await?)
    }
    pub async fn get_version(
        db: &Database,
        user_id: &str,
        version: &str,
    ) -> StorageResult<Option<BackupVersionRecord>> {
        Ok(versions(db)
            .find_one(doc! {"user_id": user_id, "version": version})
            .await?)
    }

    pub async fn update_version(
        db: &Database,
        user_id: &str,
        version: &str,
        auth_data: serde_json::Value,
    ) -> StorageResult<bool> {
        let auth_data = mongodb::bson::to_bson(&auth_data)
            .map_err(|error| StorageError::Internal(error.to_string()))?;
        let result = versions(db)
            .update_one(
                doc! {"user_id": user_id, "version": version},
                doc! {"$set": {"auth_data": auth_data}},
            )
            .await?;
        Ok(result.matched_count > 0)
    }

    pub async fn delete_version(
        db: &Database,
        user_id: &str,
        version: &str,
    ) -> StorageResult<bool> {
        sessions(db)
            .delete_many(doc! {"user_id": user_id, "version": version})
            .await?;
        Ok(versions(db)
            .delete_one(doc! {"user_id": user_id, "version": version})
            .await?
            .deleted_count
            > 0)
    }

    pub async fn count_sessions(db: &Database, user_id: &str, version: &str) -> StorageResult<u64> {
        Ok(sessions(db)
            .count_documents(doc! {"user_id": user_id, "version": version})
            .await?)
    }

    pub async fn upsert_session(
        db: &Database,
        record: &BackupSessionRecord,
    ) -> StorageResult<bool> {
        let filter = doc! {"user_id": &record.user_id, "version": &record.version, "room_id": &record.room_id, "session_id": &record.session_id};
        if let Some(stored) = sessions(db).find_one(filter.clone()).await? {
            if !backup_key_is_better(&stored.data, &record.data) {
                return Ok(false);
            }
        }
        sessions(db)
            .find_one_and_replace(filter, record)
            .upsert(true)
            .return_document(ReturnDocument::After)
            .await?;
        Ok(true)
    }

    pub async fn get_sessions(
        db: &Database,
        user_id: &str,
        version: &str,
        room_id: Option<&str>,
        session_id: Option<&str>,
    ) -> StorageResult<Vec<BackupSessionRecord>> {
        let mut filter = doc! {"user_id": user_id, "version": version};
        if let Some(room_id) = room_id {
            filter.insert("room_id", room_id);
        }
        if let Some(session_id) = session_id {
            filter.insert("session_id", session_id);
        }
        let cursor = sessions(db)
            .find(filter)
            .sort(doc! {"room_id": 1, "session_id": 1})
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn delete_sessions(
        db: &Database,
        user_id: &str,
        version: &str,
        room_id: Option<&str>,
        session_id: Option<&str>,
    ) -> StorageResult<u64> {
        let mut filter = doc! {"user_id": user_id, "version": version};
        if let Some(room_id) = room_id {
            filter.insert("room_id", room_id);
        }
        if let Some(session_id) = session_id {
            filter.insert("session_id", session_id);
        }
        Ok(sessions(db).delete_many(filter).await?.deleted_count)
    }
}

#[cfg(test)]
mod tests {
    use super::backup_key_is_better;
    use serde_json::json;

    #[test]
    fn backup_prefers_verified_keys_then_better_coverage_and_fewer_forwards() {
        let stored = json!({"is_verified": false, "first_message_index": 1, "forwarded_count": 0});
        assert!(backup_key_is_better(
            &stored,
            &json!({"is_verified": true, "first_message_index": 50, "forwarded_count": 10})
        ));
        assert!(backup_key_is_better(
            &json!({"is_verified": true, "first_message_index": 10, "forwarded_count": 3}),
            &json!({"is_verified": true, "first_message_index": 2, "forwarded_count": 3})
        ));
        assert!(backup_key_is_better(
            &json!({"is_verified": false, "first_message_index": 2, "forwarded_count": 7}),
            &json!({"is_verified": false, "first_message_index": 2, "forwarded_count": 1})
        ));
        assert!(!backup_key_is_better(&stored, &stored));
    }
}
