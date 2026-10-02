use crate::{db::StorageResult, store::Store};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct UserFilterRecord {
    pub user_id: String,
    pub filter_id: String,
    pub filter_json: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

pub async fn upsert_filter(store: &Store, record: &UserFilterRecord) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => {
            sqlx::query("INSERT INTO user_filters(user_id,filter_id,filter_json,created_at) VALUES($1,$2,$3,$4) ON CONFLICT(user_id,filter_id) DO UPDATE SET filter_json=$3,created_at=$4")
                .bind(&record.user_id)
                .bind(&record.filter_id)
                .bind(&record.filter_json)
                .bind(record.created_at)
                .execute(pool)
                .await?;
        }
        Store::Mongo(backend) => {
            backend
                .database
                .collection::<UserFilterRecord>("user_filters")
                .replace_one(
                    mongodb::bson::doc! {"user_id": &record.user_id, "filter_id": &record.filter_id},
                    record,
                )
                .upsert(true)
                .await?;
        }
    }
    Ok(())
}

pub async fn get_filter(
    store: &Store,
    user_id: &str,
    filter_id: &str,
) -> StorageResult<Option<UserFilterRecord>> {
    match store {
        Store::Postgres(pool) => Ok(sqlx::query_as::<_, UserFilterRecord>(
            "SELECT user_id,filter_id,filter_json,created_at FROM user_filters WHERE user_id=$1 AND filter_id=$2",
        )
        .bind(user_id)
        .bind(filter_id)
        .fetch_optional(pool)
        .await?),
        Store::Mongo(backend) => Ok(backend
            .database
            .collection::<UserFilterRecord>("user_filters")
            .find_one(mongodb::bson::doc! {"user_id": user_id, "filter_id": filter_id})
            .await?),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RoomReceiptRecord {
    pub room_id: String,
    pub user_id: String,
    pub receipt_type: String,
    pub thread_id: String,
    pub event_id: String,
    pub event_stream_id: i64,
    pub stream_id: i64,
    pub timestamp: Option<i64>,
    pub updated_at: DateTime<Utc>,
}

pub async fn upsert_receipt(store: &Store, record: &RoomReceiptRecord) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => {
            sqlx::query("INSERT INTO room_receipts(room_id,user_id,receipt_type,thread_id,event_id,event_stream_id,stream_id,timestamp,updated_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT(room_id,user_id,receipt_type,thread_id) DO UPDATE SET event_id=$5,event_stream_id=$6,stream_id=$7,timestamp=$8,updated_at=$9 WHERE room_receipts.stream_id < $7")
                .bind(&record.room_id)
                .bind(&record.user_id)
                .bind(&record.receipt_type)
                .bind(&record.thread_id)
                .bind(&record.event_id)
                .bind(record.event_stream_id)
                .bind(record.stream_id)
                .bind(record.timestamp)
                .bind(record.updated_at)
                .execute(pool)
                .await?;
            Ok(())
        }
        Store::Mongo(backend) => {
            let collection = backend
                .database
                .collection::<RoomReceiptRecord>("room_receipts");
            let identity = mongodb::bson::doc! {
                "room_id": &record.room_id,
                "user_id": &record.user_id,
                "receipt_type": &record.receipt_type,
                "thread_id": &record.thread_id,
            };
            if collection
                .find_one(identity.clone())
                .await?
                .is_some_and(|existing| existing.stream_id >= record.stream_id)
            {
                return Ok(());
            }
            collection
                .replace_one(identity, record)
                .upsert(true)
                .await?;
            Ok(())
        }
    }
}

pub async fn get_receipts_since(
    store: &Store,
    room_id: &str,
    since: i64,
    to: i64,
) -> StorageResult<Vec<RoomReceiptRecord>> {
    match store {
        Store::Postgres(pool) => Ok(sqlx::query_as::<_, RoomReceiptRecord>(
            "SELECT room_id,user_id,receipt_type,thread_id,event_id,event_stream_id,stream_id,timestamp,updated_at FROM room_receipts WHERE room_id=$1 AND stream_id>$2 AND stream_id<=$3 ORDER BY stream_id",
        )
        .bind(room_id)
        .bind(since)
        .bind(to)
        .fetch_all(pool)
        .await?),
        Store::Mongo(backend) => {
            use futures::stream::TryStreamExt;
            let cursor = backend
                .database
                .collection::<RoomReceiptRecord>("room_receipts")
                .find(mongodb::bson::doc! {
                    "room_id": room_id,
                    "stream_id": {"$gt": since, "$lte": to},
                })
                .sort(mongodb::bson::doc! {"stream_id": 1})
                .await?;
            Ok(cursor.try_collect().await?)
        }
    }
}

pub async fn get_latest_receipt(
    store: &Store,
    room_id: &str,
    user_id: &str,
    receipt_type: &str,
    thread_id: &str,
) -> StorageResult<Option<RoomReceiptRecord>> {
    match store {
        Store::Postgres(pool) => Ok(sqlx::query_as::<_, RoomReceiptRecord>(
            "SELECT room_id,user_id,receipt_type,thread_id,event_id,event_stream_id,stream_id,timestamp,updated_at FROM room_receipts WHERE room_id=$1 AND user_id=$2 AND receipt_type=$3 AND thread_id=$4",
        )
        .bind(room_id)
        .bind(user_id)
        .bind(receipt_type)
        .bind(thread_id)
        .fetch_optional(pool)
        .await?),
        Store::Mongo(backend) => Ok(backend
            .database
            .collection::<RoomReceiptRecord>("room_receipts")
            .find_one(mongodb::bson::doc! {
                "room_id": room_id,
                "user_id": user_id,
                "receipt_type": receipt_type,
                "thread_id": thread_id,
            })
            .await?),
    }
}
