#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct AppserviceRecord {
    pub id: String,
    pub url: String,
    pub as_token: String,
    pub hs_token: String,
    pub sender_localpart: String,
    pub namespaces: serde_json::Value,
    pub rate_limited: bool,
    pub protocols: Option<serde_json::Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
pub struct AppserviceTransactionRecord {
    pub transaction_id: String,
    pub appservice_id: String,
    pub first_stream_id: i64,
    pub last_stream_id: i64,
    pub payload: serde_json::Value,
    pub attempts: i32,
    pub next_retry_at: Option<chrono::DateTime<chrono::Utc>>,
    pub delivered_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// The appservice's own bot user, per its `sender_localpart`. This user is
/// always implicitly permitted regardless of the `users` namespace regexes.
pub fn bot_user_id(record: &AppserviceRecord, server_name: &str) -> String {
    format!("@{}:{}", record.sender_localpart, server_name)
}

/// Checks whether `value` matches any regex in the given namespace `kind`
/// (`"users"`, `"aliases"`, or `"rooms"`) of an appservice registration's
/// `namespaces` JSON blob (`{"users": [{"regex": "...", "exclusive": true}], ...}`).
pub fn namespace_matches(namespaces: &serde_json::Value, kind: &str, value: &str) -> bool {
    let Some(list) = namespaces.get(kind).and_then(|v| v.as_array()) else {
        return false;
    };
    list.iter().any(|entry| {
        entry
            .get("regex")
            .and_then(|v| v.as_str())
            .and_then(|pattern| regex::Regex::new(pattern).ok())
            .map(|re| re.is_match(value))
            .unwrap_or(false)
    })
}

/// True if the appservice is a bridge is interested in `user_id`: either it's
/// the appservice's own bot user, or it falls within the `users` namespace.
pub fn owns_user(record: &AppserviceRecord, server_name: &str, user_id: &str) -> bool {
    user_id == bot_user_id(record, server_name) || namespace_matches(&record.namespaces, "users", user_id)
}

use crate::db::StorageResult;
use crate::store::Store;

pub async fn upsert_appservice(store: &Store, record: &AppserviceRecord) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::upsert_appservice(pool, record).await,
        Store::Mongo(backend) => mongo::upsert_appservice(&backend.database, record).await,
    }
}

pub async fn get_appservice(store: &Store, id: &str) -> StorageResult<AppserviceRecord> {
    match store {
        Store::Postgres(pool) => pg::get_appservice(pool, id).await,
        Store::Mongo(backend) => mongo::get_appservice(&backend.database, id).await,
    }
}

pub async fn get_appservice_by_as_token(store: &Store, as_token: &str) -> StorageResult<AppserviceRecord> {
    match store {
        Store::Postgres(pool) => pg::get_appservice_by_as_token(pool, as_token).await,
        Store::Mongo(backend) => mongo::get_appservice_by_as_token(&backend.database, as_token).await,
    }
}

pub async fn list_appservices(store: &Store) -> StorageResult<Vec<AppserviceRecord>> {
    match store {
        Store::Postgres(pool) => pg::list_appservices(pool).await,
        Store::Mongo(backend) => mongo::list_appservices(&backend.database).await,
    }
}

pub async fn create_appservice_transaction(store: &Store, txn: &AppserviceTransactionRecord) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::create_appservice_transaction(pool, txn).await,
        Store::Mongo(backend) => mongo::create_appservice_transaction(&backend.database, txn).await,
    }
}

pub async fn get_pending_transactions(store: &Store, appservice_id: &str, limit: i64) -> StorageResult<Vec<AppserviceTransactionRecord>> {
    match store {
        Store::Postgres(pool) => pg::get_pending_transactions(pool, appservice_id, limit).await,
        Store::Mongo(backend) => mongo::get_pending_transactions(&backend.database, appservice_id, limit).await,
    }
}

pub async fn mark_transaction_delivered(store: &Store, transaction_id: &str) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::mark_transaction_delivered(pool, transaction_id).await,
        Store::Mongo(backend) => mongo::mark_transaction_delivered(&backend.database, transaction_id).await,
    }
}

pub async fn increment_transaction_attempts(store: &Store, transaction_id: &str, next_retry_at: chrono::DateTime<chrono::Utc>) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::increment_transaction_attempts(pool, transaction_id, next_retry_at).await,
        Store::Mongo(backend) => mongo::increment_transaction_attempts(&backend.database, transaction_id, next_retry_at).await,
    }
}

mod pg {
    use super::{AppserviceRecord, AppserviceTransactionRecord};
    use crate::db::{StorageError, StorageResult};

    pub async fn upsert_appservice(pool: &sqlx::PgPool, record: &AppserviceRecord) -> StorageResult<()> {
        sqlx::query(
            "INSERT INTO appservices (id, url, as_token, hs_token, sender_localpart, namespaces, rate_limited, protocols, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) ON CONFLICT (id) DO UPDATE SET url = $2, as_token = $3, hs_token = $4, sender_localpart = $5, namespaces = $6, rate_limited = $7, protocols = $8"
        )
        .bind(&record.id)
        .bind(&record.url)
        .bind(&record.as_token)
        .bind(&record.hs_token)
        .bind(&record.sender_localpart)
        .bind(&record.namespaces)
        .bind(record.rate_limited)
        .bind(&record.protocols)
        .bind(record.created_at)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_appservice(pool: &sqlx::PgPool, id: &str) -> StorageResult<AppserviceRecord> {
        sqlx::query_as::<_, AppserviceRecord>(
            "SELECT id, url, as_token, hs_token, sender_localpart, namespaces, rate_limited, protocols, created_at FROM appservices WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(StorageError::NotFound)
    }

    pub async fn get_appservice_by_as_token(pool: &sqlx::PgPool, as_token: &str) -> StorageResult<AppserviceRecord> {
        sqlx::query_as::<_, AppserviceRecord>(
            "SELECT id, url, as_token, hs_token, sender_localpart, namespaces, rate_limited, protocols, created_at FROM appservices WHERE as_token = $1"
        )
        .bind(as_token)
        .fetch_optional(pool)
        .await?
        .ok_or(StorageError::NotFound)
    }

    pub async fn list_appservices(pool: &sqlx::PgPool) -> StorageResult<Vec<AppserviceRecord>> {
        let records = sqlx::query_as::<_, AppserviceRecord>(
            "SELECT id, url, as_token, hs_token, sender_localpart, namespaces, rate_limited, protocols, created_at FROM appservices ORDER BY created_at"
        )
        .fetch_all(pool)
        .await?;
        Ok(records)
    }

    pub async fn create_appservice_transaction(pool: &sqlx::PgPool, txn: &AppserviceTransactionRecord) -> StorageResult<()> {
        sqlx::query(
            "INSERT INTO appservice_transactions (transaction_id, appservice_id, first_stream_id, last_stream_id, payload, attempts, next_retry_at, delivered_at, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"
        )
        .bind(&txn.transaction_id)
        .bind(&txn.appservice_id)
        .bind(txn.first_stream_id)
        .bind(txn.last_stream_id)
        .bind(&txn.payload)
        .bind(txn.attempts)
        .bind(txn.next_retry_at)
        .bind(txn.delivered_at)
        .bind(txn.created_at)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_pending_transactions(pool: &sqlx::PgPool, appservice_id: &str, limit: i64) -> StorageResult<Vec<AppserviceTransactionRecord>> {
        let records = sqlx::query_as::<_, AppserviceTransactionRecord>(
            "SELECT transaction_id, appservice_id, first_stream_id, last_stream_id, payload, attempts, next_retry_at, delivered_at, created_at FROM appservice_transactions WHERE appservice_id = $1 AND delivered_at IS NULL AND (next_retry_at IS NULL OR next_retry_at <= NOW()) ORDER BY first_stream_id LIMIT $2"
        )
        .bind(appservice_id)
        .bind(limit)
        .fetch_all(pool)
        .await?;
        Ok(records)
    }

    pub async fn mark_transaction_delivered(pool: &sqlx::PgPool, transaction_id: &str) -> StorageResult<()> {
        sqlx::query(
            "UPDATE appservice_transactions SET delivered_at = NOW() WHERE transaction_id = $1"
        )
        .bind(transaction_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn increment_transaction_attempts(pool: &sqlx::PgPool, transaction_id: &str, next_retry_at: chrono::DateTime<chrono::Utc>) -> StorageResult<()> {
        sqlx::query(
            "UPDATE appservice_transactions SET attempts = attempts + 1, next_retry_at = $2 WHERE transaction_id = $1"
        )
        .bind(transaction_id)
        .bind(next_retry_at)
        .execute(pool)
        .await?;
        Ok(())
    }
}

mod mongo {
    use super::{AppserviceRecord, AppserviceTransactionRecord};
    use crate::db::{StorageError, StorageResult};
    use futures::stream::TryStreamExt;
    use mongodb::bson::doc;
    use mongodb::Database;

    fn appservices(db: &Database) -> mongodb::Collection<AppserviceRecord> {
        db.collection("appservices")
    }
    fn appservice_transactions(db: &Database) -> mongodb::Collection<AppserviceTransactionRecord> {
        db.collection("appservice_transactions")
    }

    pub async fn upsert_appservice(db: &Database, record: &AppserviceRecord) -> StorageResult<()> {
        appservices(db)
            .find_one_and_replace(doc! { "id": &record.id }, record)
            .upsert(true)
            .await?;
        Ok(())
    }

    pub async fn get_appservice(db: &Database, id: &str) -> StorageResult<AppserviceRecord> {
        appservices(db).find_one(doc! { "id": id }).await?.ok_or(StorageError::NotFound)
    }

    pub async fn get_appservice_by_as_token(db: &Database, as_token: &str) -> StorageResult<AppserviceRecord> {
        appservices(db)
            .find_one(doc! { "as_token": as_token })
            .await?
            .ok_or(StorageError::NotFound)
    }

    pub async fn list_appservices(db: &Database) -> StorageResult<Vec<AppserviceRecord>> {
        let cursor = appservices(db).find(doc! {}).sort(doc! { "created_at": 1 }).await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn create_appservice_transaction(db: &Database, txn: &AppserviceTransactionRecord) -> StorageResult<()> {
        appservice_transactions(db).insert_one(txn).await?;
        Ok(())
    }

    pub async fn get_pending_transactions(db: &Database, appservice_id: &str, limit: i64) -> StorageResult<Vec<AppserviceTransactionRecord>> {
        let now = chrono::Utc::now();
        let cursor = appservice_transactions(db)
            .find(doc! {
                "appservice_id": appservice_id,
                "delivered_at": null,
                "$or": [
                    { "next_retry_at": null },
                    { "next_retry_at": { "$lte": now } },
                ],
            })
            .sort(doc! { "first_stream_id": 1 })
            .limit(limit)
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn mark_transaction_delivered(db: &Database, transaction_id: &str) -> StorageResult<()> {
        appservice_transactions(db)
            .update_one(
                doc! { "transaction_id": transaction_id },
                doc! { "$set": { "delivered_at": chrono::Utc::now() } },
            )
            .await?;
        Ok(())
    }

    pub async fn increment_transaction_attempts(db: &Database, transaction_id: &str, next_retry_at: chrono::DateTime<chrono::Utc>) -> StorageResult<()> {
        appservice_transactions(db)
            .update_one(
                doc! { "transaction_id": transaction_id },
                doc! { "$inc": { "attempts": 1 }, "$set": { "next_retry_at": next_retry_at } },
            )
            .await?;
        Ok(())
    }
}
