use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct RoomRecord {
    pub room_id: String,
    pub creator: String,
    pub room_version: String,
    pub is_encrypted: bool,
    pub is_direct: bool,
    pub name: Option<String>,
    pub topic: Option<String>,
    pub canonical_alias: Option<String>,
    /// Room directory listing: `"public"` or `"private"` (default).
    #[serde(default = "default_visibility")]
    pub visibility: String,
    pub creation_ts: i64,
    pub created_at: DateTime<Utc>,
}

fn default_visibility() -> String {
    "private".to_string()
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct RoomAliasRecord {
    pub alias: String,
    pub room_id: String,
    pub creator: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct RoomMemberRecord {
    pub room_id: String,
    pub user_id: String,
    pub membership: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub sender: String,
    pub event_id: String,
    pub stream_id: i64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct RoomStateRecord {
    pub room_id: String,
    pub event_type: String,
    pub state_key: String,
    pub event_id: String,
    pub content: serde_json::Value,
    pub sender: String,
    pub stream_id: i64,
    pub updated_at: DateTime<Utc>,
}

use crate::db::StorageResult;
use crate::store::Store;

pub async fn create_room(store: &Store, room: &RoomRecord) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::create_room(pool, room).await,
        Store::Mongo(backend) => mongo::create_room(&backend.database, room).await,
    }
}

pub async fn get_room(store: &Store, room_id: &str) -> StorageResult<RoomRecord> {
    match store {
        Store::Postgres(pool) => pg::get_room(pool, room_id).await,
        Store::Mongo(backend) => mongo::get_room(&backend.database, room_id).await,
    }
}

/// Marks a room as encrypted once its encryption state event is accepted.
/// Encryption is permanent in Matrix rooms, so this is intentionally a one-way update.
pub async fn mark_room_encrypted(store: &Store, room_id: &str) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => {
            sqlx::query("UPDATE rooms SET is_encrypted=TRUE WHERE room_id=$1")
                .bind(room_id)
                .execute(pool)
                .await?;
            Ok(())
        }
        Store::Mongo(backend) => {
            backend
                .database
                .collection::<RoomRecord>("rooms")
                .update_one(
                    mongodb::bson::doc! { "room_id": room_id },
                    mongodb::bson::doc! { "$set": { "is_encrypted": true } },
                )
                .await?;
            Ok(())
        }
    }
}

pub async fn upsert_room_member(store: &Store, member: &RoomMemberRecord) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::upsert_room_member(pool, member).await,
        Store::Mongo(backend) => mongo::upsert_room_member(&backend.database, member).await,
    }
}

pub async fn get_room_member(
    store: &Store,
    room_id: &str,
    user_id: &str,
) -> StorageResult<RoomMemberRecord> {
    match store {
        Store::Postgres(pool) => pg::get_room_member(pool, room_id, user_id).await,
        Store::Mongo(backend) => mongo::get_room_member(&backend.database, room_id, user_id).await,
    }
}

pub async fn get_room_members(
    store: &Store,
    room_id: &str,
) -> StorageResult<Vec<RoomMemberRecord>> {
    match store {
        Store::Postgres(pool) => pg::get_room_members(pool, room_id).await,
        Store::Mongo(backend) => mongo::get_room_members(&backend.database, room_id).await,
    }
}

pub async fn get_room_members_by_membership(
    store: &Store,
    room_id: &str,
    membership: &str,
) -> StorageResult<Vec<RoomMemberRecord>> {
    match store {
        Store::Postgres(pool) => {
            pg::get_room_members_by_membership(pool, room_id, membership).await
        }
        Store::Mongo(backend) => {
            mongo::get_room_members_by_membership(&backend.database, room_id, membership).await
        }
    }
}

/// Returns the distinct joined users across a set of rooms in one storage
/// query. This is used for `/sync` presence fan-out.
pub async fn get_joined_member_user_ids(
    store: &Store,
    room_ids: &[String],
    excluded_user_id: &str,
) -> StorageResult<Vec<String>> {
    if room_ids.is_empty() {
        return Ok(vec![]);
    }
    match store {
        Store::Postgres(pool) => pg::get_joined_member_user_ids(pool, room_ids, excluded_user_id).await,
        Store::Mongo(backend) => mongo::get_joined_member_user_ids(&backend.database, room_ids, excluded_user_id).await,
    }
}

pub async fn get_joined_rooms(store: &Store, user_id: &str) -> StorageResult<Vec<String>> {
    match store {
        Store::Postgres(pool) => pg::get_joined_rooms(pool, user_id).await,
        Store::Mongo(backend) => mongo::get_joined_rooms(&backend.database, user_id).await,
    }
}

pub async fn get_room_members_by_user_membership(
    store: &Store,
    user_id: &str,
    membership: &str,
) -> StorageResult<Vec<RoomMemberRecord>> {
    match store {
        Store::Postgres(pool) => {
            pg::get_room_members_by_user_membership(pool, user_id, membership).await
        }
        Store::Mongo(backend) => {
            mongo::get_room_members_by_user_membership(&backend.database, user_id, membership).await
        }
    }
}

pub async fn upsert_room_state(store: &Store, state: &RoomStateRecord) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::upsert_room_state(pool, state).await,
        Store::Mongo(backend) => mongo::upsert_room_state(&backend.database, state).await,
    }
}

pub async fn get_room_state(
    store: &Store,
    room_id: &str,
    event_type: &str,
    state_key: &str,
) -> StorageResult<RoomStateRecord> {
    match store {
        Store::Postgres(pool) => pg::get_room_state(pool, room_id, event_type, state_key).await,
        Store::Mongo(backend) => {
            mongo::get_room_state(&backend.database, room_id, event_type, state_key).await
        }
    }
}

pub async fn get_full_room_state(
    store: &Store,
    room_id: &str,
) -> StorageResult<Vec<RoomStateRecord>> {
    match store {
        Store::Postgres(pool) => pg::get_full_room_state(pool, room_id).await,
        Store::Mongo(backend) => mongo::get_full_room_state(&backend.database, room_id).await,
    }
}

/// Registers a room alias. Fails with `StorageError::Conflict` if already taken.
pub async fn create_alias(
    store: &Store,
    alias: &str,
    room_id: &str,
    creator: &str,
) -> StorageResult<()> {
    let record = RoomAliasRecord {
        alias: alias.to_string(),
        room_id: room_id.to_string(),
        creator: creator.to_string(),
        created_at: chrono::Utc::now(),
    };
    match store {
        Store::Postgres(pool) => pg::create_alias(pool, &record).await,
        Store::Mongo(backend) => mongo::create_alias(&backend.database, &record).await,
    }
}

pub async fn get_room_id_for_alias(store: &Store, alias: &str) -> StorageResult<String> {
    match store {
        Store::Postgres(pool) => pg::get_room_id_for_alias(pool, alias).await,
        Store::Mongo(backend) => mongo::get_room_id_for_alias(&backend.database, alias).await,
    }
}

pub async fn delete_alias(store: &Store, alias: &str) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::delete_alias(pool, alias).await,
        Store::Mongo(backend) => mongo::delete_alias(&backend.database, alias).await,
    }
}

pub async fn list_aliases_for_room(store: &Store, room_id: &str) -> StorageResult<Vec<String>> {
    match store {
        Store::Postgres(pool) => pg::list_aliases_for_room(pool, room_id).await,
        Store::Mongo(backend) => mongo::list_aliases_for_room(&backend.database, room_id).await,
    }
}

pub async fn set_room_visibility(
    store: &Store,
    room_id: &str,
    visibility: &str,
) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => {
            sqlx::query("UPDATE rooms SET visibility=$2 WHERE room_id=$1")
                .bind(room_id)
                .bind(visibility)
                .execute(pool)
                .await?;
            Ok(())
        }
        Store::Mongo(backend) => {
            backend
                .database
                .collection::<RoomRecord>("rooms")
                .update_one(
                    mongodb::bson::doc! { "room_id": room_id },
                    mongodb::bson::doc! { "$set": { "visibility": visibility } },
                )
                .await?;
            Ok(())
        }
    }
}

pub async fn set_room_canonical_alias(
    store: &Store,
    room_id: &str,
    alias: Option<&str>,
) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => {
            sqlx::query("UPDATE rooms SET canonical_alias=$2 WHERE room_id=$1")
                .bind(room_id)
                .bind(alias)
                .execute(pool)
                .await?;
            Ok(())
        }
        Store::Mongo(backend) => {
            backend
                .database
                .collection::<RoomRecord>("rooms")
                .update_one(
                    mongodb::bson::doc! { "room_id": room_id },
                    mongodb::bson::doc! { "$set": { "canonical_alias": alias } },
                )
                .await?;
            Ok(())
        }
    }
}

pub async fn list_public_rooms(store: &Store, limit: i64) -> StorageResult<Vec<RoomRecord>> {
    match store {
        Store::Postgres(pool) => pg::list_public_rooms(pool, limit).await,
        Store::Mongo(backend) => mongo::list_public_rooms(&backend.database, limit).await,
    }
}

pub async fn count_rooms(store: &Store) -> StorageResult<u64> {
    match store {
        Store::Postgres(pool) => {
            let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM rooms")
                .fetch_one(pool)
                .await?;
            Ok(count.max(0) as u64)
        }
        Store::Mongo(backend) => Ok(backend
            .database
            .collection::<RoomRecord>("rooms")
            .count_documents(mongodb::bson::doc! {})
            .await?),
    }
}

mod pg {
    use super::{RoomAliasRecord, RoomMemberRecord, RoomRecord, RoomStateRecord};
    use crate::db::{is_pg_unique_violation, StorageError, StorageResult};

    pub async fn create_room(pool: &sqlx::PgPool, room: &RoomRecord) -> StorageResult<()> {
        sqlx::query(
            "INSERT INTO rooms (room_id, creator, room_version, is_encrypted, is_direct, name, topic, canonical_alias, visibility, creation_ts, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)"
        )
        .bind(&room.room_id)
        .bind(&room.creator)
        .bind(&room.room_version)
        .bind(room.is_encrypted)
        .bind(room.is_direct)
        .bind(&room.name)
        .bind(&room.topic)
        .bind(&room.canonical_alias)
        .bind(&room.visibility)
        .bind(room.creation_ts)
        .bind(room.created_at)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_room(pool: &sqlx::PgPool, room_id: &str) -> StorageResult<RoomRecord> {
        sqlx::query_as::<_, RoomRecord>(
            "SELECT room_id, creator, room_version, is_encrypted, is_direct, name, topic, canonical_alias, visibility, creation_ts, created_at FROM rooms WHERE room_id = $1"
        )
        .bind(room_id)
        .fetch_optional(pool)
        .await?
        .ok_or(StorageError::NotFound)
    }

    pub async fn create_alias(
        pool: &sqlx::PgPool,
        record: &RoomAliasRecord,
    ) -> StorageResult<()> {
        sqlx::query("INSERT INTO room_aliases(alias,room_id,creator,created_at) VALUES($1,$2,$3,$4)")
            .bind(&record.alias)
            .bind(&record.room_id)
            .bind(&record.creator)
            .bind(record.created_at)
            .execute(pool)
            .await
            .map_err(|e| {
                if is_pg_unique_violation(&e) {
                    StorageError::Conflict("alias already in use".to_string())
                } else {
                    e.into()
                }
            })?;
        Ok(())
    }

    pub async fn get_room_id_for_alias(pool: &sqlx::PgPool, alias: &str) -> StorageResult<String> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT room_id FROM room_aliases WHERE alias = $1")
                .bind(alias)
                .fetch_optional(pool)
                .await?;
        row.map(|(room_id,)| room_id).ok_or(StorageError::NotFound)
    }

    pub async fn delete_alias(pool: &sqlx::PgPool, alias: &str) -> StorageResult<()> {
        let result = sqlx::query("DELETE FROM room_aliases WHERE alias = $1")
            .bind(alias)
            .execute(pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(StorageError::NotFound);
        }
        Ok(())
    }

    pub async fn list_aliases_for_room(
        pool: &sqlx::PgPool,
        room_id: &str,
    ) -> StorageResult<Vec<String>> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT alias FROM room_aliases WHERE room_id = $1")
                .bind(room_id)
                .fetch_all(pool)
                .await?;
        Ok(rows.into_iter().map(|(alias,)| alias).collect())
    }

    pub async fn list_public_rooms(pool: &sqlx::PgPool, limit: i64) -> StorageResult<Vec<RoomRecord>> {
        Ok(sqlx::query_as::<_, RoomRecord>(
            "SELECT room_id, creator, room_version, is_encrypted, is_direct, name, topic, canonical_alias, visibility, creation_ts, created_at FROM rooms WHERE visibility = 'public' ORDER BY creation_ts DESC LIMIT $1"
        )
        .bind(limit)
        .fetch_all(pool)
        .await?)
    }

    pub async fn upsert_room_member(
        pool: &sqlx::PgPool,
        member: &RoomMemberRecord,
    ) -> StorageResult<()> {
        sqlx::query(
            "INSERT INTO room_members (room_id, user_id, membership, display_name, avatar_url, sender, event_id, stream_id, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
             ON CONFLICT (room_id, user_id) DO UPDATE SET membership = $3, display_name = $4, avatar_url = $5, sender = $6, event_id = $7, stream_id = $8, updated_at = $9"
        )
        .bind(&member.room_id)
        .bind(&member.user_id)
        .bind(&member.membership)
        .bind(&member.display_name)
        .bind(&member.avatar_url)
        .bind(&member.sender)
        .bind(&member.event_id)
        .bind(member.stream_id)
        .bind(member.updated_at)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_room_member(
        pool: &sqlx::PgPool,
        room_id: &str,
        user_id: &str,
    ) -> StorageResult<RoomMemberRecord> {
        sqlx::query_as::<_, RoomMemberRecord>(
            "SELECT room_id, user_id, membership, display_name, avatar_url, sender, event_id, stream_id, updated_at FROM room_members WHERE room_id = $1 AND user_id = $2"
        )
        .bind(room_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?
        .ok_or(StorageError::NotFound)
    }

    pub async fn get_room_members(
        pool: &sqlx::PgPool,
        room_id: &str,
    ) -> StorageResult<Vec<RoomMemberRecord>> {
        let members = sqlx::query_as::<_, RoomMemberRecord>(
            "SELECT room_id, user_id, membership, display_name, avatar_url, sender, event_id, stream_id, updated_at FROM room_members WHERE room_id = $1"
        )
        .bind(room_id)
        .fetch_all(pool)
        .await?;
        Ok(members)
    }

    pub async fn get_room_members_by_membership(
        pool: &sqlx::PgPool,
        room_id: &str,
        membership: &str,
    ) -> StorageResult<Vec<RoomMemberRecord>> {
        let members = sqlx::query_as::<_, RoomMemberRecord>(
            "SELECT room_id, user_id, membership, display_name, avatar_url, sender, event_id, stream_id, updated_at FROM room_members WHERE room_id = $1 AND membership = $2"
        )
        .bind(room_id)
        .bind(membership)
        .fetch_all(pool)
        .await?;
        Ok(members)
    }

    pub async fn get_joined_member_user_ids(
        pool: &sqlx::PgPool,
        room_ids: &[String],
        excluded_user_id: &str,
    ) -> StorageResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT user_id FROM room_members WHERE room_id = ANY($1) AND membership = 'join' AND user_id <> $2",
        )
        .bind(room_ids)
        .bind(excluded_user_id)
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(|(user_id,)| user_id).collect())
    }

    pub async fn get_joined_rooms(
        pool: &sqlx::PgPool,
        user_id: &str,
    ) -> StorageResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT room_id FROM room_members WHERE user_id = $1 AND membership = 'join'",
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(|(r,)| r).collect())
    }

    pub async fn get_room_members_by_user_membership(
        pool: &sqlx::PgPool,
        user_id: &str,
        membership: &str,
    ) -> StorageResult<Vec<RoomMemberRecord>> {
        let rows = if membership == "leave" {
            sqlx::query_as::<_, RoomMemberRecord>(
                "SELECT room_id, user_id, membership, display_name, avatar_url, sender, event_id, stream_id, updated_at FROM room_members WHERE user_id = $1 AND membership IN ('leave', 'ban')"
            )
            .bind(user_id)
            .fetch_all(pool)
            .await?
        } else {
            sqlx::query_as::<_, RoomMemberRecord>(
                "SELECT room_id, user_id, membership, display_name, avatar_url, sender, event_id, stream_id, updated_at FROM room_members WHERE user_id = $1 AND membership = $2"
            )
            .bind(user_id)
            .bind(membership)
            .fetch_all(pool)
            .await?
        };
        Ok(rows)
    }

    pub async fn upsert_room_state(
        pool: &sqlx::PgPool,
        state: &RoomStateRecord,
    ) -> StorageResult<()> {
        sqlx::query(
            "INSERT INTO room_state (room_id, event_type, state_key, event_id, content, sender, stream_id, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             ON CONFLICT (room_id, event_type, state_key) DO UPDATE SET event_id = $4, content = $5, sender = $6, stream_id = $7, updated_at = $8"
        )
        .bind(&state.room_id)
        .bind(&state.event_type)
        .bind(&state.state_key)
        .bind(&state.event_id)
        .bind(&state.content)
        .bind(&state.sender)
        .bind(state.stream_id)
        .bind(state.updated_at)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_room_state(
        pool: &sqlx::PgPool,
        room_id: &str,
        event_type: &str,
        state_key: &str,
    ) -> StorageResult<RoomStateRecord> {
        sqlx::query_as::<_, RoomStateRecord>(
            "SELECT room_id, event_type, state_key, event_id, content, sender, stream_id, updated_at FROM room_state WHERE room_id = $1 AND event_type = $2 AND state_key = $3"
        )
        .bind(room_id)
        .bind(event_type)
        .bind(state_key)
        .fetch_optional(pool)
        .await?
        .ok_or(StorageError::NotFound)
    }

    pub async fn get_full_room_state(
        pool: &sqlx::PgPool,
        room_id: &str,
    ) -> StorageResult<Vec<RoomStateRecord>> {
        let states = sqlx::query_as::<_, RoomStateRecord>(
            "SELECT room_id, event_type, state_key, event_id, content, sender, stream_id, updated_at FROM room_state WHERE room_id = $1"
        )
        .bind(room_id)
        .fetch_all(pool)
        .await?;
        Ok(states)
    }
}

mod mongo {
    use super::{RoomAliasRecord, RoomMemberRecord, RoomRecord, RoomStateRecord};
    use crate::db::{is_duplicate_key_error, StorageError, StorageResult};
    use futures::stream::TryStreamExt;
    use mongodb::bson::doc;
    use mongodb::options::ReturnDocument;
    use mongodb::Database;

    fn rooms(db: &Database) -> mongodb::Collection<RoomRecord> {
        db.collection("rooms")
    }
    fn room_members(db: &Database) -> mongodb::Collection<RoomMemberRecord> {
        db.collection("room_members")
    }
    fn room_state(db: &Database) -> mongodb::Collection<RoomStateRecord> {
        db.collection("room_state")
    }
    fn room_aliases(db: &Database) -> mongodb::Collection<RoomAliasRecord> {
        db.collection("room_aliases")
    }

    pub async fn create_room(db: &Database, room: &RoomRecord) -> StorageResult<()> {
        match rooms(db).insert_one(room).await {
            Ok(_) => Ok(()),
            Err(e) if is_duplicate_key_error(&e) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn get_room(db: &Database, room_id: &str) -> StorageResult<RoomRecord> {
        rooms(db)
            .find_one(doc! { "room_id": room_id })
            .await?
            .ok_or(StorageError::NotFound)
    }

    pub async fn upsert_room_member(db: &Database, member: &RoomMemberRecord) -> StorageResult<()> {
        room_members(db)
            .find_one_and_replace(
                doc! { "room_id": &member.room_id, "user_id": &member.user_id },
                member,
            )
            .upsert(true)
            .await?;
        Ok(())
    }

    pub async fn get_room_member(
        db: &Database,
        room_id: &str,
        user_id: &str,
    ) -> StorageResult<RoomMemberRecord> {
        room_members(db)
            .find_one(doc! { "room_id": room_id, "user_id": user_id })
            .await?
            .ok_or(StorageError::NotFound)
    }

    pub async fn get_room_members(
        db: &Database,
        room_id: &str,
    ) -> StorageResult<Vec<RoomMemberRecord>> {
        let cursor = room_members(db).find(doc! { "room_id": room_id }).await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn get_room_members_by_membership(
        db: &Database,
        room_id: &str,
        membership: &str,
    ) -> StorageResult<Vec<RoomMemberRecord>> {
        let cursor = room_members(db)
            .find(doc! { "room_id": room_id, "membership": membership })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn get_joined_member_user_ids(
        db: &Database,
        room_ids: &[String],
        excluded_user_id: &str,
    ) -> StorageResult<Vec<String>> {
        let values = room_members(db)
            .distinct(
                "user_id",
                doc! { "room_id": { "$in": room_ids }, "membership": "join", "user_id": { "$ne": excluded_user_id } },
            )
            .await?;
        Ok(values.into_iter().filter_map(|value| value.as_str().map(str::to_owned)).collect())
    }

    pub async fn get_joined_rooms(db: &Database, user_id: &str) -> StorageResult<Vec<String>> {
        let cursor = room_members(db)
            .find(doc! { "user_id": user_id, "membership": "join" })
            .await?;
        let members: Vec<RoomMemberRecord> = cursor.try_collect().await?;
        Ok(members.into_iter().map(|m| m.room_id).collect())
    }

    pub async fn get_room_members_by_user_membership(
        db: &Database,
        user_id: &str,
        membership: &str,
    ) -> StorageResult<Vec<RoomMemberRecord>> {
        let filter = if membership == "leave" {
            doc! { "user_id": user_id, "membership": { "$in": ["leave", "ban"] } }
        } else {
            doc! { "user_id": user_id, "membership": membership }
        };
        let cursor = room_members(db).find(filter).await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn upsert_room_state(db: &Database, state: &RoomStateRecord) -> StorageResult<()> {
        room_state(db)
            .find_one_and_replace(
                doc! { "room_id": &state.room_id, "event_type": &state.event_type, "state_key": &state.state_key },
                state,
            )
            .upsert(true)
            .return_document(ReturnDocument::After)
            .await?;
        Ok(())
    }

    pub async fn get_room_state(
        db: &Database,
        room_id: &str,
        event_type: &str,
        state_key: &str,
    ) -> StorageResult<RoomStateRecord> {
        room_state(db)
            .find_one(doc! { "room_id": room_id, "event_type": event_type, "state_key": state_key })
            .await?
            .ok_or(StorageError::NotFound)
    }

    pub async fn get_full_room_state(
        db: &Database,
        room_id: &str,
    ) -> StorageResult<Vec<RoomStateRecord>> {
        let cursor = room_state(db).find(doc! { "room_id": room_id }).await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn create_alias(db: &Database, record: &RoomAliasRecord) -> StorageResult<()> {
        match room_aliases(db).insert_one(record).await {
            Ok(_) => Ok(()),
            Err(e) if is_duplicate_key_error(&e) => {
                Err(StorageError::Conflict("alias already in use".to_string()))
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn get_room_id_for_alias(db: &Database, alias: &str) -> StorageResult<String> {
        room_aliases(db)
            .find_one(doc! { "alias": alias })
            .await?
            .map(|record| record.room_id)
            .ok_or(StorageError::NotFound)
    }

    pub async fn delete_alias(db: &Database, alias: &str) -> StorageResult<()> {
        let result = room_aliases(db).delete_one(doc! { "alias": alias }).await?;
        if result.deleted_count == 0 {
            return Err(StorageError::NotFound);
        }
        Ok(())
    }

    pub async fn list_aliases_for_room(db: &Database, room_id: &str) -> StorageResult<Vec<String>> {
        let cursor = room_aliases(db).find(doc! { "room_id": room_id }).await?;
        let records: Vec<RoomAliasRecord> = cursor.try_collect().await?;
        Ok(records.into_iter().map(|record| record.alias).collect())
    }

    pub async fn list_public_rooms(db: &Database, limit: i64) -> StorageResult<Vec<RoomRecord>> {
        let cursor = rooms(db)
            .find(doc! { "visibility": "public" })
            .sort(doc! { "creation_ts": -1 })
            .limit(limit)
            .await?;
        Ok(cursor.try_collect().await?)
    }
}
