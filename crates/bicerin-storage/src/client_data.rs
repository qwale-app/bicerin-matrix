use crate::{db::StorageResult, store::Store};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AccountDataRecord {
    pub user_id: String,
    /// Empty string denotes user-scoped account data; otherwise this is a room ID.
    pub room_id: String,
    pub event_type: String,
    pub content: serde_json::Value,
    pub stream_id: i64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ToDeviceMessageRecord {
    pub message_id: String,
    pub user_id: String,
    pub device_id: String,
    pub sender: String,
    pub event_type: String,
    pub content: serde_json::Value,
    pub stream_id: i64,
    /// The sync token for the response which most recently included this
    /// message. The row is deleted only when the client acknowledges that
    /// exact response by using its token as `since`.
    #[serde(default)]
    pub delivered_sync_token: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub async fn upsert_account_data(store: &Store, record: &AccountDataRecord) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::upsert_account_data(pool, record).await,
        Store::Mongo(backend) => mongo::upsert_account_data(&backend.database, record).await,
    }
}

pub async fn get_account_data(
    store: &Store,
    user_id: &str,
    room_id: &str,
) -> StorageResult<Vec<AccountDataRecord>> {
    match store {
        Store::Postgres(pool) => pg::get_account_data(pool, user_id, room_id).await,
        Store::Mongo(backend) => mongo::get_account_data(&backend.database, user_id, room_id).await,
    }
}

pub async fn get_account_data_since(
    store: &Store,
    user_id: &str,
    since: i64,
    to: i64,
) -> StorageResult<Vec<AccountDataRecord>> {
    match store {
        Store::Postgres(pool) => pg::get_account_data_since(pool, user_id, since, to).await,
        Store::Mongo(backend) => {
            mongo::get_account_data_since(&backend.database, user_id, since, to).await
        }
    }
}

pub async fn insert_to_device_message(
    store: &Store,
    record: &ToDeviceMessageRecord,
) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::insert_to_device_message(pool, record).await,
        Store::Mongo(backend) => mongo::insert_to_device_message(&backend.database, record).await,
    }
}

pub async fn get_pending_to_device_messages(
    store: &Store,
    user_id: &str,
    device_id: &str,
    limit: i64,
) -> StorageResult<Vec<ToDeviceMessageRecord>> {
    match store {
        Store::Postgres(pool) => {
            pg::get_pending_to_device_messages(pool, user_id, device_id, limit).await
        }
        Store::Mongo(backend) => {
            mongo::get_pending_to_device_messages(&backend.database, user_id, device_id, limit)
                .await
        }
    }
}

pub async fn mark_to_device_messages_delivered(
    store: &Store,
    user_id: &str,
    device_id: &str,
    message_ids: &[String],
    sync_token: &str,
) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => {
            pg::mark_to_device_messages_delivered(pool, user_id, device_id, message_ids, sync_token)
                .await
        }
        Store::Mongo(backend) => {
            mongo::mark_to_device_messages_delivered(
                &backend.database,
                user_id,
                device_id,
                message_ids,
                sync_token,
            )
            .await
        }
    }
}

pub async fn acknowledge_to_device_messages(
    store: &Store,
    user_id: &str,
    device_id: &str,
    sync_token: &str,
) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => {
            pg::acknowledge_to_device_messages(pool, user_id, device_id, sync_token).await
        }
        Store::Mongo(backend) => {
            mongo::acknowledge_to_device_messages(&backend.database, user_id, device_id, sync_token)
                .await
        }
    }
}

mod pg {
    use super::{AccountDataRecord, ToDeviceMessageRecord};
    use crate::db::StorageResult;

    pub async fn upsert_account_data(
        pool: &sqlx::PgPool,
        record: &AccountDataRecord,
    ) -> StorageResult<()> {
        sqlx::query(
            "INSERT INTO account_data (user_id, room_id, event_type, content, stream_id, updated_at) VALUES ($1,$2,$3,$4,$5,$6) ON CONFLICT (user_id,room_id,event_type) DO UPDATE SET content=$4,stream_id=$5,updated_at=$6"
        ).bind(&record.user_id).bind(&record.room_id).bind(&record.event_type).bind(&record.content).bind(record.stream_id).bind(record.updated_at).execute(pool).await?;
        Ok(())
    }

    pub async fn get_account_data(
        pool: &sqlx::PgPool,
        user_id: &str,
        room_id: &str,
    ) -> StorageResult<Vec<AccountDataRecord>> {
        Ok(sqlx::query_as::<_, AccountDataRecord>("SELECT user_id,room_id,event_type,content,stream_id,updated_at FROM account_data WHERE user_id=$1 AND room_id=$2 ORDER BY event_type")
            .bind(user_id).bind(room_id).fetch_all(pool).await?)
    }

    pub async fn get_account_data_since(
        pool: &sqlx::PgPool,
        user_id: &str,
        since: i64,
        to: i64,
    ) -> StorageResult<Vec<AccountDataRecord>> {
        Ok(sqlx::query_as::<_, AccountDataRecord>("SELECT user_id,room_id,event_type,content,stream_id,updated_at FROM account_data WHERE user_id=$1 AND room_id='' AND stream_id>$2 AND stream_id<=$3 ORDER BY stream_id")
            .bind(user_id).bind(since).bind(to).fetch_all(pool).await?)
    }

    pub async fn insert_to_device_message(
        pool: &sqlx::PgPool,
        record: &ToDeviceMessageRecord,
    ) -> StorageResult<()> {
        sqlx::query("INSERT INTO to_device_messages (message_id,user_id,device_id,sender,event_type,content,stream_id,delivered_sync_token,created_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT (message_id) DO NOTHING")
            .bind(&record.message_id).bind(&record.user_id).bind(&record.device_id).bind(&record.sender).bind(&record.event_type).bind(&record.content).bind(record.stream_id).bind(&record.delivered_sync_token).bind(record.created_at).execute(pool).await?;
        Ok(())
    }

    pub async fn get_pending_to_device_messages(
        pool: &sqlx::PgPool,
        user_id: &str,
        device_id: &str,
        limit: i64,
    ) -> StorageResult<Vec<ToDeviceMessageRecord>> {
        Ok(sqlx::query_as::<_, ToDeviceMessageRecord>("SELECT message_id,user_id,device_id,sender,event_type,content,stream_id,delivered_sync_token,created_at FROM to_device_messages WHERE user_id=$1 AND device_id=$2 ORDER BY stream_id,message_id LIMIT $3")
            .bind(user_id).bind(device_id).bind(limit).fetch_all(pool).await?)
    }

    pub async fn mark_to_device_messages_delivered(
        pool: &sqlx::PgPool,
        user_id: &str,
        device_id: &str,
        message_ids: &[String],
        sync_token: &str,
    ) -> StorageResult<()> {
        sqlx::query("UPDATE to_device_messages SET delivered_sync_token=$4 WHERE user_id=$1 AND device_id=$2 AND message_id=ANY($3)")
            .bind(user_id).bind(device_id).bind(message_ids).bind(sync_token).execute(pool).await?;
        Ok(())
    }

    pub async fn acknowledge_to_device_messages(
        pool: &sqlx::PgPool,
        user_id: &str,
        device_id: &str,
        sync_token: &str,
    ) -> StorageResult<()> {
        sqlx::query("DELETE FROM to_device_messages WHERE user_id=$1 AND device_id=$2 AND delivered_sync_token=$3")
            .bind(user_id).bind(device_id).bind(sync_token).execute(pool).await?;
        Ok(())
    }
}

mod mongo {
    use super::{AccountDataRecord, ToDeviceMessageRecord};
    use crate::db::StorageResult;
    use futures::stream::TryStreamExt;
    use mongodb::bson::doc;
    use mongodb::Database;

    pub async fn upsert_account_data(
        db: &Database,
        record: &AccountDataRecord,
    ) -> StorageResult<()> {
        db.collection::<AccountDataRecord>("account_data")
            .replace_one(doc! {"user_id": &record.user_id, "room_id": &record.room_id, "event_type": &record.event_type}, record)
            .upsert(true).await?;
        Ok(())
    }

    pub async fn get_account_data(
        db: &Database,
        user_id: &str,
        room_id: &str,
    ) -> StorageResult<Vec<AccountDataRecord>> {
        let cursor = db
            .collection::<AccountDataRecord>("account_data")
            .find(doc! {"user_id": user_id, "room_id": room_id})
            .sort(doc! {"event_type": 1})
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn get_account_data_since(
        db: &Database,
        user_id: &str,
        since: i64,
        to: i64,
    ) -> StorageResult<Vec<AccountDataRecord>> {
        let cursor = db
            .collection::<AccountDataRecord>("account_data")
            .find(doc! {"user_id": user_id, "room_id": "", "stream_id": {"$gt": since, "$lte": to}})
            .sort(doc! {"stream_id": 1})
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn insert_to_device_message(
        db: &Database,
        record: &ToDeviceMessageRecord,
    ) -> StorageResult<()> {
        match db
            .collection::<ToDeviceMessageRecord>("to_device_messages")
            .insert_one(record)
            .await
        {
            Ok(_) => {}
            Err(error) if crate::db::is_duplicate_key_error(&error) => {}
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }

    pub async fn get_pending_to_device_messages(
        db: &Database,
        user_id: &str,
        device_id: &str,
        limit: i64,
    ) -> StorageResult<Vec<ToDeviceMessageRecord>> {
        let cursor = db
            .collection::<ToDeviceMessageRecord>("to_device_messages")
            .find(doc! {"user_id": user_id, "device_id": device_id})
            .sort(doc! {"stream_id": 1, "message_id": 1})
            .limit(limit)
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn mark_to_device_messages_delivered(
        db: &Database,
        user_id: &str,
        device_id: &str,
        message_ids: &[String],
        sync_token: &str,
    ) -> StorageResult<()> {
        if message_ids.is_empty() {
            return Ok(());
        }
        db.collection::<ToDeviceMessageRecord>("to_device_messages")
            .update_many(doc! {"user_id": user_id, "device_id": device_id, "message_id": {"$in": message_ids}}, doc! {"$set": {"delivered_sync_token": sync_token}})
            .await?;
        Ok(())
    }

    pub async fn acknowledge_to_device_messages(
        db: &Database,
        user_id: &str,
        device_id: &str,
        sync_token: &str,
    ) -> StorageResult<()> {
        db.collection::<ToDeviceMessageRecord>("to_device_messages").delete_many(doc! {"user_id": user_id, "device_id": device_id, "delivered_sync_token": sync_token}).await?;
        Ok(())
    }
}
