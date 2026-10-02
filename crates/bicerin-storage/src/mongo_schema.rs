use mongodb::bson::doc;
use mongodb::options::IndexOptions;
use mongodb::{Database, IndexModel};

use crate::db::StorageResult;

/// Creates the (mostly unique) indexes Bicerin relies on for MongoDB, mirroring
/// the primary keys / unique constraints declared in `migrations/0001_init.sql`
/// for Postgres. MongoDB is schemaless, so there's no migration step beyond this.
pub async fn ensure_indexes(db: &Database) -> StorageResult<()> {
    unique(db, "users", doc! { "user_id": 1 }).await?;
    unique(db, "devices", doc! { "user_id": 1, "device_id": 1 }).await?;
    unique(db, "access_tokens", doc! { "token_hash": 1 }).await?;

    unique(db, "rooms", doc! { "room_id": 1 }).await?;
    unique(db, "room_members", doc! { "room_id": 1, "user_id": 1 }).await?;
    index(db, "room_members", doc! { "user_id": 1, "membership": 1 }).await?;
    index(db, "room_members", doc! { "room_id": 1, "membership": 1, "user_id": 1 }).await?;
    unique(
        db,
        "room_state",
        doc! { "room_id": 1, "event_type": 1, "state_key": 1 },
    )
    .await?;
    unique(db, "room_aliases", doc! { "alias": 1 }).await?;
    index(db, "room_aliases", doc! { "room_id": 1 }).await?;
    unique(db, "user_presence", doc! { "user_id": 1 }).await?;
    unique(db, "pushers", doc! { "user_id": 1, "pushkey": 1, "app_id": 1 }).await?;
    unique(db, "push_rule_overrides", doc! { "user_id": 1, "rule_id": 1 }).await?;
    unique(db, "push_rules", doc! { "user_id": 1, "kind": 1, "rule_id": 1 }).await?;
    unique(db, "pending_pushes", doc! { "id": 1 }).await?;
    index(
        db,
        "pending_pushes",
        doc! { "delivered_at": 1, "next_retry_at": 1 },
    )
    .await?;

    unique(db, "events", doc! { "event_id": 1 }).await?;
    index(db, "events", doc! { "room_id": 1, "stream_id": 1 }).await?;
    index(db, "events", doc! { "stream_id": 1 }).await?;
    index(db, "events", doc! { "sender": 1, "stream_id": 1 }).await?;

    unique(
        db,
        "event_relations",
        doc! { "parent_event_id": 1, "child_event_id": 1, "rel_type": 1 },
    )
    .await?;
    index(db, "event_relations", doc! { "parent_event_id": 1 }).await?;

    unique(db, "user_room_cursors", doc! { "user_id": 1, "room_id": 1 }).await?;

    unique(db, "appservices", doc! { "id": 1 }).await?;
    unique(db, "appservices", doc! { "as_token": 1 }).await?;
    unique(
        db,
        "appservice_transactions",
        doc! { "appservice_id": 1, "transaction_id": 1 },
    )
    .await?;
    index(
        db,
        "appservice_transactions",
        doc! { "appservice_id": 1, "delivered_at": 1, "next_retry_at": 1 },
    )
    .await?;

    unique(db, "device_keys", doc! { "user_id": 1, "device_id": 1 }).await?;
    unique(
        db,
        "one_time_keys",
        doc! { "user_id": 1, "device_id": 1, "key_id": 1 },
    )
    .await?;
    unique(
        db,
        "fallback_keys",
        doc! { "user_id": 1, "device_id": 1, "algorithm": 1 },
    )
    .await?;
    unique(
        db,
        "cross_signing_keys",
        doc! { "user_id": 1, "key_type": 1 },
    )
    .await?;
    index(
        db,
        "device_key_changes",
        doc! { "stream_id": 1, "user_id": 1 },
    )
    .await?;
    unique(
        db,
        "account_data",
        doc! { "user_id": 1, "room_id": 1, "event_type": 1 },
    )
    .await?;
    unique(db, "to_device_messages", doc! { "message_id": 1 }).await?;
    index(
        db,
        "to_device_messages",
        doc! { "user_id": 1, "device_id": 1, "stream_id": 1 },
    )
    .await?;
    unique(
        db,
        "room_key_backup_versions",
        doc! { "user_id": 1, "version": 1 },
    )
    .await?;
    unique(
        db,
        "room_key_backup_sessions",
        doc! { "user_id": 1, "version": 1, "room_id": 1, "session_id": 1 },
    )
    .await?;
    unique(db, "user_filters", doc! { "user_id": 1, "filter_id": 1 }).await?;
    unique(
        db,
        "room_receipts",
        doc! { "room_id": 1, "user_id": 1, "receipt_type": 1, "thread_id": 1 },
    )
    .await?;
    index(db, "room_receipts", doc! { "room_id": 1, "stream_id": 1 }).await?;
    index(
        db,
        "room_key_backup_sessions",
        doc! { "user_id": 1, "version": 1, "room_id": 1 },
    )
    .await?;

    unique(
        db,
        "transactions",
        doc! { "user_id": 1, "device_id": 1, "txn_id": 1, "endpoint": 1 },
    )
    .await?;
    unique(db, "media", doc! { "server_name": 1, "media_id": 1 }).await?;

    // Seed the stream-id counter document if it doesn't exist yet.
    let counters = db.collection::<mongodb::bson::Document>("counters");
    counters
        .update_one(
            doc! { "_id": "event_stream" },
            doc! { "$setOnInsert": { "seq": 0i64 } },
        )
        .upsert(true)
        .await?;

    Ok(())
}

async fn unique(
    db: &Database,
    collection: &str,
    keys: mongodb::bson::Document,
) -> StorageResult<()> {
    let model = IndexModel::builder()
        .keys(keys)
        .options(IndexOptions::builder().unique(true).build())
        .build();
    db.collection::<mongodb::bson::Document>(collection)
        .create_index(model)
        .await?;
    Ok(())
}

async fn index(
    db: &Database,
    collection: &str,
    keys: mongodb::bson::Document,
) -> StorageResult<()> {
    let model = IndexModel::builder().keys(keys).build();
    db.collection::<mongodb::bson::Document>(collection)
        .create_index(model)
        .await?;
    Ok(())
}
