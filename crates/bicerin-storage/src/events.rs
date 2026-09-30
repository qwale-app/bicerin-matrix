use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRecord {
    pub event_id: String,
    pub room_id: String,
    pub sender: String,
    pub stream_id: i64,
    pub origin_server_ts: i64,
    pub event_type: String,
    pub state_key: Option<String>,
    pub room_version: String,
    pub content: serde_json::Value,
    pub unsigned: Option<serde_json::Value>,
    pub redacts: Option<String>,
    pub depth: i64,
    pub auth_events: Vec<String>,
    pub prev_events: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct EventRelationRecord {
    pub room_id: String,
    pub parent_event_id: String,
    pub child_event_id: String,
    pub rel_type: String,
}

use crate::db::StorageResult;
use crate::store::Store;

pub async fn insert_event(store: &Store, event: &EventRecord) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::insert_event(pool, event).await,
        Store::Mongo(backend) => mongo::insert_event(&backend.database, event).await,
    }
}

pub async fn get_event(store: &Store, event_id: &str) -> StorageResult<EventRecord> {
    match store {
        Store::Postgres(pool) => pg::get_event(pool, event_id).await,
        Store::Mongo(backend) => mongo::get_event(&backend.database, event_id).await,
    }
}

pub async fn get_events_in_room(store: &Store, room_id: &str, from_stream_id: i64, limit: i64, direction: &str) -> StorageResult<Vec<EventRecord>> {
    match store {
        Store::Postgres(pool) => pg::get_events_in_room(pool, room_id, from_stream_id, limit, direction).await,
        Store::Mongo(backend) => mongo::get_events_in_room(&backend.database, room_id, from_stream_id, limit, direction).await,
    }
}

pub async fn get_latest_events_in_room(store: &Store, room_id: &str, limit: i64) -> StorageResult<Vec<EventRecord>> {
    match store {
        Store::Postgres(pool) => pg::get_latest_events_in_room(pool, room_id, limit).await,
        Store::Mongo(backend) => mongo::get_latest_events_in_room(&backend.database, room_id, limit).await,
    }
}

pub async fn get_events_since(store: &Store, user_id: &str, room_ids: &[String], since_stream_id: i64) -> StorageResult<Vec<EventRecord>> {
    match store {
        Store::Postgres(pool) => pg::get_events_since(pool, user_id, room_ids, since_stream_id).await,
        Store::Mongo(backend) => mongo::get_events_since(&backend.database, user_id, room_ids, since_stream_id).await,
    }
}

/// Reserves the next global stream position. Backed by a Postgres sequence
/// or, for MongoDB, an atomic `$inc` on a singleton counter document.
pub async fn get_next_stream_id(store: &Store) -> StorageResult<i64> {
    match store {
        Store::Postgres(pool) => pg::get_next_stream_id(pool).await,
        Store::Mongo(backend) => mongo::get_next_stream_id(&backend.database).await,
    }
}

pub async fn get_current_stream_position(store: &Store) -> StorageResult<i64> {
    match store {
        Store::Postgres(pool) => pg::get_current_stream_position(pool).await,
        Store::Mongo(backend) => mongo::get_current_stream_position(&backend.database).await,
    }
}

pub async fn insert_event_relation(store: &Store, rel: &EventRelationRecord) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::insert_event_relation(pool, rel).await,
        Store::Mongo(backend) => mongo::insert_event_relation(&backend.database, rel).await,
    }
}

pub async fn get_event_relations(store: &Store, parent_event_id: &str, rel_type: Option<&str>) -> StorageResult<Vec<EventRelationRecord>> {
    match store {
        Store::Postgres(pool) => pg::get_event_relations(pool, parent_event_id, rel_type).await,
        Store::Mongo(backend) => mongo::get_event_relations(&backend.database, parent_event_id, rel_type).await,
    }
}

mod pg {
    use super::{EventRecord, EventRelationRecord};
    use crate::db::{StorageError, StorageResult};

    #[derive(Debug, sqlx::FromRow)]
    struct EventRow {
        pub event_id: String,
        pub room_id: String,
        pub sender: String,
        pub stream_id: i64,
        pub origin_server_ts: i64,
        pub event_type: String,
        pub state_key: Option<String>,
        pub room_version: String,
        pub content: sqlx::types::JsonValue,
        pub unsigned: Option<sqlx::types::JsonValue>,
        pub redacts: Option<String>,
        pub depth: i64,
        pub auth_events_json: sqlx::types::JsonValue,
        pub prev_events_json: sqlx::types::JsonValue,
        pub created_at: chrono::DateTime<chrono::Utc>,
    }

    fn row_to_record(row: EventRow) -> EventRecord {
        EventRecord {
            event_id: row.event_id,
            room_id: row.room_id,
            sender: row.sender,
            stream_id: row.stream_id,
            origin_server_ts: row.origin_server_ts,
            event_type: row.event_type,
            state_key: row.state_key,
            room_version: row.room_version,
            content: row.content,
            unsigned: row.unsigned,
            redacts: row.redacts,
            depth: row.depth,
            auth_events: serde_json::from_value(row.auth_events_json).unwrap_or_default(),
            prev_events: serde_json::from_value(row.prev_events_json).unwrap_or_default(),
            created_at: row.created_at,
        }
    }

    pub async fn insert_event(pool: &sqlx::PgPool, event: &EventRecord) -> StorageResult<()> {
        let auth_json = serde_json::to_value(&event.auth_events).unwrap_or(serde_json::json!([]));
        let prev_json = serde_json::to_value(&event.prev_events).unwrap_or(serde_json::json!([]));
        sqlx::query(
            "INSERT INTO events (event_id, room_id, sender, stream_id, origin_server_ts, event_type, state_key, room_version, content, unsigned, redacts, depth, auth_events_json, prev_events_json, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13::jsonb, $14::jsonb, $15) ON CONFLICT (event_id) DO NOTHING"
        )
        .bind(&event.event_id)
        .bind(&event.room_id)
        .bind(&event.sender)
        .bind(event.stream_id)
        .bind(event.origin_server_ts)
        .bind(&event.event_type)
        .bind(&event.state_key)
        .bind(&event.room_version)
        .bind(&event.content)
        .bind(&event.unsigned)
        .bind(&event.redacts)
        .bind(event.depth)
        .bind(&auth_json)
        .bind(&prev_json)
        .bind(event.created_at)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_event(pool: &sqlx::PgPool, event_id: &str) -> StorageResult<EventRecord> {
        let row = sqlx::query_as::<_, EventRow>(
            "SELECT event_id, room_id, sender, stream_id, origin_server_ts, event_type, state_key, room_version, content, unsigned, redacts, depth, auth_events_json, prev_events_json, created_at FROM events WHERE event_id = $1"
        )
        .bind(event_id)
        .fetch_optional(pool)
        .await?
        .ok_or(StorageError::NotFound)?;
        Ok(row_to_record(row))
    }

    pub async fn get_events_in_room(pool: &sqlx::PgPool, room_id: &str, from_stream_id: i64, limit: i64, direction: &str) -> StorageResult<Vec<EventRecord>> {
        let rows = if direction == "b" {
            sqlx::query_as::<_, EventRow>(
                "SELECT event_id, room_id, sender, stream_id, origin_server_ts, event_type, state_key, room_version, content, unsigned, redacts, depth, auth_events_json, prev_events_json, created_at FROM events WHERE room_id = $1 AND stream_id < $2 ORDER BY stream_id DESC LIMIT $3"
            )
            .bind(room_id)
            .bind(from_stream_id)
            .bind(limit)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as::<_, EventRow>(
                "SELECT event_id, room_id, sender, stream_id, origin_server_ts, event_type, state_key, room_version, content, unsigned, redacts, depth, auth_events_json, prev_events_json, created_at FROM events WHERE room_id = $1 AND stream_id > $2 ORDER BY stream_id ASC LIMIT $3"
            )
            .bind(room_id)
            .bind(from_stream_id)
            .bind(limit)
            .fetch_all(pool)
            .await?
        };
        Ok(rows.into_iter().map(row_to_record).collect())
    }

    pub async fn get_latest_events_in_room(pool: &sqlx::PgPool, room_id: &str, limit: i64) -> StorageResult<Vec<EventRecord>> {
        let rows = sqlx::query_as::<_, EventRow>(
            "SELECT event_id, room_id, sender, stream_id, origin_server_ts, event_type, state_key, room_version, content, unsigned, redacts, depth, auth_events_json, prev_events_json, created_at FROM events WHERE room_id = $1 ORDER BY stream_id DESC LIMIT $2"
        )
        .bind(room_id)
        .bind(limit)
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(row_to_record).collect())
    }

    pub async fn get_events_since(pool: &sqlx::PgPool, _user_id: &str, room_ids: &[String], since_stream_id: i64) -> StorageResult<Vec<EventRecord>> {
        if room_ids.is_empty() {
            return Ok(vec![]);
        }
        let placeholders: String = room_ids.iter().enumerate()
            .map(|(i, _)| format!("${}", i + 2))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT event_id, room_id, sender, stream_id, origin_server_ts, event_type, state_key, room_version, content, unsigned, redacts, depth, auth_events_json, prev_events_json, created_at FROM events WHERE stream_id > $1 AND room_id IN ({}) ORDER BY stream_id ASC LIMIT 500",
            placeholders
        );
        let mut q = sqlx::query_as::<_, EventRow>(&sql).bind(since_stream_id);
        for room_id in room_ids {
            q = q.bind(room_id);
        }
        let rows = q.fetch_all(pool).await?;
        Ok(rows.into_iter().map(row_to_record).collect())
    }

    pub async fn get_next_stream_id(pool: &sqlx::PgPool) -> StorageResult<i64> {
        let row: (i64,) = sqlx::query_as("SELECT nextval('event_stream_seq')")
            .fetch_one(pool)
            .await?;
        Ok(row.0)
    }

    pub async fn get_current_stream_position(pool: &sqlx::PgPool) -> StorageResult<i64> {
        let row: (Option<i64>,) = sqlx::query_as("SELECT MAX(stream_id) FROM events")
            .fetch_one(pool)
            .await?;
        Ok(row.0.unwrap_or(0))
    }

    pub async fn insert_event_relation(pool: &sqlx::PgPool, rel: &EventRelationRecord) -> StorageResult<()> {
        sqlx::query(
            "INSERT INTO event_relations (room_id, parent_event_id, child_event_id, rel_type) VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING"
        )
        .bind(&rel.room_id)
        .bind(&rel.parent_event_id)
        .bind(&rel.child_event_id)
        .bind(&rel.rel_type)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_event_relations(pool: &sqlx::PgPool, parent_event_id: &str, rel_type: Option<&str>) -> StorageResult<Vec<EventRelationRecord>> {
        let relations = if let Some(rt) = rel_type {
            sqlx::query_as::<_, EventRelationRecord>(
                "SELECT room_id, parent_event_id, child_event_id, rel_type FROM event_relations WHERE parent_event_id = $1 AND rel_type = $2"
            )
            .bind(parent_event_id)
            .bind(rt)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as::<_, EventRelationRecord>(
                "SELECT room_id, parent_event_id, child_event_id, rel_type FROM event_relations WHERE parent_event_id = $1"
            )
            .bind(parent_event_id)
            .fetch_all(pool)
            .await?
        };
        Ok(relations)
    }
}

mod mongo {
    use super::{EventRecord, EventRelationRecord};
    use crate::db::{is_duplicate_key_error, StorageError, StorageResult};
    use futures::stream::TryStreamExt;
    use mongodb::bson::{doc, Document};
    use mongodb::options::ReturnDocument;
    use mongodb::Database;

    fn events(db: &Database) -> mongodb::Collection<EventRecord> {
        db.collection("events")
    }
    fn event_relations(db: &Database) -> mongodb::Collection<EventRelationRecord> {
        db.collection("event_relations")
    }
    fn counters(db: &Database) -> mongodb::Collection<Document> {
        db.collection("counters")
    }

    pub async fn insert_event(db: &Database, event: &EventRecord) -> StorageResult<()> {
        match events(db).insert_one(event).await {
            Ok(_) => Ok(()),
            Err(e) if is_duplicate_key_error(&e) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn get_event(db: &Database, event_id: &str) -> StorageResult<EventRecord> {
        events(db)
            .find_one(doc! { "event_id": event_id })
            .await?
            .ok_or(StorageError::NotFound)
    }

    pub async fn get_events_in_room(db: &Database, room_id: &str, from_stream_id: i64, limit: i64, direction: &str) -> StorageResult<Vec<EventRecord>> {
        let (filter, sort) = if direction == "b" {
            (doc! { "room_id": room_id, "stream_id": { "$lt": from_stream_id } }, doc! { "stream_id": -1 })
        } else {
            (doc! { "room_id": room_id, "stream_id": { "$gt": from_stream_id } }, doc! { "stream_id": 1 })
        };
        let cursor = events(db).find(filter).sort(sort).limit(limit).await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn get_latest_events_in_room(db: &Database, room_id: &str, limit: i64) -> StorageResult<Vec<EventRecord>> {
        let cursor = events(db)
            .find(doc! { "room_id": room_id })
            .sort(doc! { "stream_id": -1 })
            .limit(limit)
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn get_events_since(db: &Database, _user_id: &str, room_ids: &[String], since_stream_id: i64) -> StorageResult<Vec<EventRecord>> {
        if room_ids.is_empty() {
            return Ok(vec![]);
        }
        let cursor = events(db)
            .find(doc! { "stream_id": { "$gt": since_stream_id }, "room_id": { "$in": room_ids } })
            .sort(doc! { "stream_id": 1 })
            .limit(500)
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn get_next_stream_id(db: &Database) -> StorageResult<i64> {
        let doc = counters(db)
            .find_one_and_update(doc! { "_id": "event_stream" }, doc! { "$inc": { "seq": 1i64 } })
            .upsert(true)
            .return_document(ReturnDocument::After)
            .await?
            .ok_or_else(|| StorageError::Internal("counter upsert returned no document".to_string()))?;
        Ok(doc.get_i64("seq").unwrap_or(0))
    }

    pub async fn get_current_stream_position(db: &Database) -> StorageResult<i64> {
        let doc = counters(db).find_one(doc! { "_id": "event_stream" }).await?;
        Ok(doc.and_then(|d| d.get_i64("seq").ok()).unwrap_or(0))
    }

    pub async fn insert_event_relation(db: &Database, rel: &EventRelationRecord) -> StorageResult<()> {
        match event_relations(db).insert_one(rel).await {
            Ok(_) => Ok(()),
            Err(e) if is_duplicate_key_error(&e) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn get_event_relations(db: &Database, parent_event_id: &str, rel_type: Option<&str>) -> StorageResult<Vec<EventRelationRecord>> {
        let mut filter = doc! { "parent_event_id": parent_event_id };
        if let Some(rt) = rel_type {
            filter.insert("rel_type", rt);
        }
        let cursor = event_relations(db).find(filter).await?;
        Ok(cursor.try_collect().await?)
    }
}

