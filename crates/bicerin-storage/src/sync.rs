use crate::db::StorageResult;
use crate::store::Store;

pub async fn upsert_user_room_cursor(store: &Store, user_id: &str, room_id: &str, stream_id: i64) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::upsert_user_room_cursor(pool, user_id, room_id, stream_id).await,
        Store::Mongo(backend) => mongo::upsert_user_room_cursor(&backend.database, user_id, room_id, stream_id).await,
    }
}

pub async fn get_user_room_cursor(store: &Store, user_id: &str, room_id: &str) -> StorageResult<i64> {
    match store {
        Store::Postgres(pool) => pg::get_user_room_cursor(pool, user_id, room_id).await,
        Store::Mongo(backend) => mongo::get_user_room_cursor(&backend.database, user_id, room_id).await,
    }
}

pub async fn get_rooms_with_new_events(store: &Store, user_id: &str, joined_rooms: &[String], since_stream_id: i64) -> StorageResult<Vec<String>> {
    match store {
        Store::Postgres(pool) => pg::get_rooms_with_new_events(pool, user_id, joined_rooms, since_stream_id).await,
        Store::Mongo(backend) => mongo::get_rooms_with_new_events(&backend.database, user_id, joined_rooms, since_stream_id).await,
    }
}

pub async fn get_current_stream_position(store: &Store) -> StorageResult<i64> {
    crate::events::get_current_stream_position(store).await
}

mod pg {
    use crate::db::StorageResult;

    pub async fn upsert_user_room_cursor(pool: &sqlx::PgPool, user_id: &str, room_id: &str, stream_id: i64) -> StorageResult<()> {
        sqlx::query(
            "INSERT INTO user_room_cursors (user_id, room_id, last_stream_id) VALUES ($1, $2, $3) ON CONFLICT (user_id, room_id) DO UPDATE SET last_stream_id = $3"
        )
        .bind(user_id)
        .bind(room_id)
        .bind(stream_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_user_room_cursor(pool: &sqlx::PgPool, user_id: &str, room_id: &str) -> StorageResult<i64> {
        let row: Option<(i64,)> = sqlx::query_as(
            "SELECT last_stream_id FROM user_room_cursors WHERE user_id = $1 AND room_id = $2"
        )
        .bind(user_id)
        .bind(room_id)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(|(v,)| v).unwrap_or(0))
    }

    pub async fn get_rooms_with_new_events(pool: &sqlx::PgPool, _user_id: &str, joined_rooms: &[String], since_stream_id: i64) -> StorageResult<Vec<String>> {
        if joined_rooms.is_empty() {
            return Ok(vec![]);
        }
        let placeholders: String = joined_rooms.iter().enumerate()
            .map(|(i, _)| format!("${}", i + 2))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT DISTINCT room_id FROM events WHERE stream_id > $1 AND room_id IN ({}) LIMIT 100",
            placeholders
        );
        let mut q = sqlx::query_as::<_, (String,)>(&sql).bind(since_stream_id);
        for room_id in joined_rooms {
            q = q.bind(room_id);
        }
        let rows = q.fetch_all(pool).await?;
        Ok(rows.into_iter().map(|(r,)| r).collect())
    }
}

mod mongo {
    use crate::db::StorageResult;
    use mongodb::bson::{doc, Bson, Document};
    use mongodb::options::ReturnDocument;
    use mongodb::Database;

    fn cursors(db: &Database) -> mongodb::Collection<Document> {
        db.collection("user_room_cursors")
    }

    pub async fn upsert_user_room_cursor(db: &Database, user_id: &str, room_id: &str, stream_id: i64) -> StorageResult<()> {
        cursors(db)
            .find_one_and_update(
                doc! { "user_id": user_id, "room_id": room_id },
                doc! { "$set": { "last_stream_id": stream_id } },
            )
            .upsert(true)
            .return_document(ReturnDocument::After)
            .await?;
        Ok(())
    }

    pub async fn get_user_room_cursor(db: &Database, user_id: &str, room_id: &str) -> StorageResult<i64> {
        let doc = cursors(db).find_one(doc! { "user_id": user_id, "room_id": room_id }).await?;
        Ok(doc.and_then(|d| d.get_i64("last_stream_id").ok()).unwrap_or(0))
    }

    pub async fn get_rooms_with_new_events(db: &Database, _user_id: &str, joined_rooms: &[String], since_stream_id: i64) -> StorageResult<Vec<String>> {
        if joined_rooms.is_empty() {
            return Ok(vec![]);
        }
        let events: mongodb::Collection<Document> = db.collection("events");
        let values = events
            .distinct(
                "room_id",
                doc! { "stream_id": { "$gt": since_stream_id }, "room_id": { "$in": joined_rooms } },
            )
            .await?;
        Ok(values
            .into_iter()
            .filter_map(|v| if let Bson::String(s) = v { Some(s) } else { None })
            .take(100)
            .collect())
    }
}
