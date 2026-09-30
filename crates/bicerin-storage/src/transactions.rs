use serde::{Deserialize, Serialize};

/// Records the result of a client-supplied transaction ID so retried
/// requests to the same endpoint return the original result instead of
/// creating a duplicate event. See designplan.txt section 60.
#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct TransactionRecord {
    pub user_id: String,
    pub device_id: String,
    pub txn_id: String,
    pub endpoint: String,
    pub result: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

use crate::db::StorageResult;
use crate::store::Store;

pub async fn get_transaction(
    store: &Store,
    user_id: &str,
    device_id: &str,
    txn_id: &str,
    endpoint: &str,
) -> StorageResult<Option<TransactionRecord>> {
    match store {
        Store::Postgres(pool) => pg::get_transaction(pool, user_id, device_id, txn_id, endpoint).await,
        Store::Mongo(backend) => mongo::get_transaction(&backend.database, user_id, device_id, txn_id, endpoint).await,
    }
}

pub async fn record_transaction(store: &Store, record: &TransactionRecord) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::record_transaction(pool, record).await,
        Store::Mongo(backend) => mongo::record_transaction(&backend.database, record).await,
    }
}

mod pg {
    use super::TransactionRecord;
    use crate::db::StorageResult;

    pub async fn get_transaction(
        pool: &sqlx::PgPool,
        user_id: &str,
        device_id: &str,
        txn_id: &str,
        endpoint: &str,
    ) -> StorageResult<Option<TransactionRecord>> {
        let record = sqlx::query_as::<_, TransactionRecord>(
            "SELECT user_id, device_id, txn_id, endpoint, result, created_at FROM transactions \
             WHERE user_id = $1 AND device_id = $2 AND txn_id = $3 AND endpoint = $4"
        )
        .bind(user_id)
        .bind(device_id)
        .bind(txn_id)
        .bind(endpoint)
        .fetch_optional(pool)
        .await?;
        Ok(record)
    }

    pub async fn record_transaction(pool: &sqlx::PgPool, record: &TransactionRecord) -> StorageResult<()> {
        sqlx::query(
            "INSERT INTO transactions (user_id, device_id, txn_id, endpoint, result, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (user_id, device_id, txn_id, endpoint) DO NOTHING"
        )
        .bind(&record.user_id)
        .bind(&record.device_id)
        .bind(&record.txn_id)
        .bind(&record.endpoint)
        .bind(&record.result)
        .bind(record.created_at)
        .execute(pool)
        .await?;
        Ok(())
    }
}

mod mongo {
    use super::TransactionRecord;
    use crate::db::{is_duplicate_key_error, StorageResult};
    use mongodb::bson::doc;
    use mongodb::Database;

    fn transactions(db: &Database) -> mongodb::Collection<TransactionRecord> {
        db.collection("transactions")
    }

    pub async fn get_transaction(
        db: &Database,
        user_id: &str,
        device_id: &str,
        txn_id: &str,
        endpoint: &str,
    ) -> StorageResult<Option<TransactionRecord>> {
        let record = transactions(db)
            .find_one(doc! {
                "user_id": user_id,
                "device_id": device_id,
                "txn_id": txn_id,
                "endpoint": endpoint,
            })
            .await?;
        Ok(record)
    }

    pub async fn record_transaction(db: &Database, record: &TransactionRecord) -> StorageResult<()> {
        match transactions(db).insert_one(record).await {
            Ok(_) => Ok(()),
            Err(e) if is_duplicate_key_error(&e) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}
