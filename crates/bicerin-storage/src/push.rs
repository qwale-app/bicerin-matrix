use crate::{db::StorageResult, store::Store};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PusherRecord {
    pub user_id: String,
    pub pushkey: String,
    pub app_id: String,
    pub kind: Option<String>,
    pub app_display_name: String,
    pub device_display_name: String,
    pub profile_tag: Option<String>,
    pub lang: String,
    pub data: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PushRuleOverrideRecord {
    pub user_id: String,
    pub rule_id: String,
    pub enabled: bool,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PushRuleRecord {
    pub user_id: String,
    pub kind: String,
    pub rule_id: String,
    pub enabled: bool,
    pub conditions: serde_json::Value,
    pub actions: serde_json::Value,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PendingPushRecord {
    pub id: String,
    pub user_id: String,
    pub pushkey: String,
    pub app_id: String,
    pub url: String,
    pub payload: serde_json::Value,
    pub attempts: i32,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// `kind: None` deletes the pusher (per the Matrix `/pushers/set` spec).
pub async fn set_pusher(store: &Store, record: &PusherRecord) -> StorageResult<()> {
    if record.kind.is_none() {
        return delete_pusher(store, &record.user_id, &record.pushkey, &record.app_id).await;
    }
    match store {
        Store::Postgres(pool) => pg::set_pusher(pool, record).await,
        Store::Mongo(backend) => mongo::set_pusher(&backend.database, record).await,
    }
}

pub async fn delete_pusher(
    store: &Store,
    user_id: &str,
    pushkey: &str,
    app_id: &str,
) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::delete_pusher(pool, user_id, pushkey, app_id).await,
        Store::Mongo(backend) => {
            mongo::delete_pusher(&backend.database, user_id, pushkey, app_id).await
        }
    }
}

pub async fn list_pushers(store: &Store, user_id: &str) -> StorageResult<Vec<PusherRecord>> {
    match store {
        Store::Postgres(pool) => pg::list_pushers(pool, user_id).await,
        Store::Mongo(backend) => mongo::list_pushers(&backend.database, user_id).await,
    }
}

pub async fn set_push_rule_enabled(
    store: &Store,
    user_id: &str,
    rule_id: &str,
    enabled: bool,
) -> StorageResult<()> {
    let record = PushRuleOverrideRecord {
        user_id: user_id.to_string(),
        rule_id: rule_id.to_string(),
        enabled,
        updated_at: Utc::now(),
    };
    match store {
        Store::Postgres(pool) => pg::set_push_rule_enabled(pool, &record).await,
        Store::Mongo(backend) => mongo::set_push_rule_enabled(&backend.database, &record).await,
    }
}

pub async fn get_push_rule_enabled(
    store: &Store,
    user_id: &str,
    rule_id: &str,
) -> StorageResult<Option<bool>> {
    match store {
        Store::Postgres(pool) => pg::get_push_rule_enabled(pool, user_id, rule_id).await,
        Store::Mongo(backend) => mongo::get_push_rule_enabled(&backend.database, user_id, rule_id).await,
    }
}

pub async fn upsert_push_rule(store: &Store, record: &PushRuleRecord) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::upsert_push_rule(pool, record).await,
        Store::Mongo(backend) => mongo::upsert_push_rule(&backend.database, record).await,
    }
}

pub async fn list_push_rules(store: &Store, user_id: &str) -> StorageResult<Vec<PushRuleRecord>> {
    match store {
        Store::Postgres(pool) => pg::list_push_rules(pool, user_id).await,
        Store::Mongo(backend) => mongo::list_push_rules(&backend.database, user_id).await,
    }
}

pub async fn get_push_rule(
    store: &Store,
    user_id: &str,
    kind: &str,
    rule_id: &str,
) -> StorageResult<Option<PushRuleRecord>> {
    match store {
        Store::Postgres(pool) => pg::get_push_rule(pool, user_id, kind, rule_id).await,
        Store::Mongo(backend) => mongo::get_push_rule(&backend.database, user_id, kind, rule_id).await,
    }
}

pub async fn delete_push_rule(store: &Store, user_id: &str, kind: &str, rule_id: &str) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::delete_push_rule(pool, user_id, kind, rule_id).await,
        Store::Mongo(backend) => mongo::delete_push_rule(&backend.database, user_id, kind, rule_id).await,
    }
}

pub async fn enqueue_push(store: &Store, record: &PendingPushRecord) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::enqueue_push(pool, record).await,
        Store::Mongo(backend) => mongo::enqueue_push(&backend.database, record).await,
    }
}

pub async fn get_pending_pushes(store: &Store, limit: i64) -> StorageResult<Vec<PendingPushRecord>> {
    match store {
        Store::Postgres(pool) => pg::get_pending_pushes(pool, limit).await,
        Store::Mongo(backend) => mongo::get_pending_pushes(&backend.database, limit).await,
    }
}

pub async fn mark_push_delivered(store: &Store, id: &str) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::mark_push_delivered(pool, id).await,
        Store::Mongo(backend) => mongo::mark_push_delivered(&backend.database, id).await,
    }
}

pub async fn increment_push_attempts(
    store: &Store,
    id: &str,
    next_retry_at: DateTime<Utc>,
) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::increment_push_attempts(pool, id, next_retry_at).await,
        Store::Mongo(backend) => {
            mongo::increment_push_attempts(&backend.database, id, next_retry_at).await
        }
    }
}

mod pg {
    use super::{PendingPushRecord, PushRuleOverrideRecord, PushRuleRecord, PusherRecord};
    use crate::db::StorageResult;

    pub async fn set_pusher(pool: &sqlx::PgPool, record: &PusherRecord) -> StorageResult<()> {
        sqlx::query(
            "INSERT INTO pushers(user_id,pushkey,app_id,kind,app_display_name,device_display_name,profile_tag,lang,data,created_at) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) \
             ON CONFLICT(user_id,pushkey,app_id) DO UPDATE SET kind=$4,app_display_name=$5,device_display_name=$6,profile_tag=$7,lang=$8,data=$9"
        )
        .bind(&record.user_id).bind(&record.pushkey).bind(&record.app_id).bind(&record.kind)
        .bind(&record.app_display_name).bind(&record.device_display_name).bind(&record.profile_tag)
        .bind(&record.lang).bind(&record.data).bind(record.created_at)
        .execute(pool).await?;
        Ok(())
    }

    pub async fn delete_pusher(
        pool: &sqlx::PgPool,
        user_id: &str,
        pushkey: &str,
        app_id: &str,
    ) -> StorageResult<()> {
        sqlx::query("DELETE FROM pushers WHERE user_id=$1 AND pushkey=$2 AND app_id=$3")
            .bind(user_id).bind(pushkey).bind(app_id)
            .execute(pool).await?;
        Ok(())
    }

    pub async fn list_pushers(pool: &sqlx::PgPool, user_id: &str) -> StorageResult<Vec<PusherRecord>> {
        Ok(sqlx::query_as::<_, PusherRecord>(
            "SELECT user_id,pushkey,app_id,kind,app_display_name,device_display_name,profile_tag,lang,data,created_at FROM pushers WHERE user_id=$1"
        ).bind(user_id).fetch_all(pool).await?)
    }

    pub async fn set_push_rule_enabled(
        pool: &sqlx::PgPool,
        record: &PushRuleOverrideRecord,
    ) -> StorageResult<()> {
        sqlx::query(
            "INSERT INTO push_rule_overrides(user_id,rule_id,enabled,updated_at) VALUES($1,$2,$3,$4) \
             ON CONFLICT(user_id,rule_id) DO UPDATE SET enabled=$3,updated_at=$4"
        )
        .bind(&record.user_id).bind(&record.rule_id).bind(record.enabled).bind(record.updated_at)
        .execute(pool).await?;
        Ok(())
    }

    pub async fn get_push_rule_enabled(
        pool: &sqlx::PgPool,
        user_id: &str,
        rule_id: &str,
    ) -> StorageResult<Option<bool>> {
        let row: Option<(bool,)> = sqlx::query_as(
            "SELECT enabled FROM push_rule_overrides WHERE user_id=$1 AND rule_id=$2",
        )
        .bind(user_id).bind(rule_id).fetch_optional(pool).await?;
        Ok(row.map(|(enabled,)| enabled))
    }

    pub async fn upsert_push_rule(pool: &sqlx::PgPool, record: &PushRuleRecord) -> StorageResult<()> {
        sqlx::query(
            "INSERT INTO push_rules(user_id,kind,rule_id,enabled,conditions,actions,updated_at) VALUES($1,$2,$3,$4,$5,$6,$7) \
             ON CONFLICT(user_id,kind,rule_id) DO UPDATE SET enabled=$4,conditions=$5,actions=$6,updated_at=$7",
        )
        .bind(&record.user_id).bind(&record.kind).bind(&record.rule_id).bind(record.enabled)
        .bind(&record.conditions).bind(&record.actions).bind(record.updated_at)
        .execute(pool).await?;
        Ok(())
    }

    pub async fn list_push_rules(pool: &sqlx::PgPool, user_id: &str) -> StorageResult<Vec<PushRuleRecord>> {
        Ok(sqlx::query_as::<_, PushRuleRecord>(
            "SELECT user_id,kind,rule_id,enabled,conditions,actions,updated_at FROM push_rules WHERE user_id=$1 ORDER BY updated_at DESC",
        ).bind(user_id).fetch_all(pool).await?)
    }

    pub async fn get_push_rule(pool: &sqlx::PgPool, user_id: &str, kind: &str, rule_id: &str) -> StorageResult<Option<PushRuleRecord>> {
        Ok(sqlx::query_as::<_, PushRuleRecord>(
            "SELECT user_id,kind,rule_id,enabled,conditions,actions,updated_at FROM push_rules WHERE user_id=$1 AND kind=$2 AND rule_id=$3",
        ).bind(user_id).bind(kind).bind(rule_id).fetch_optional(pool).await?)
    }

    pub async fn delete_push_rule(pool: &sqlx::PgPool, user_id: &str, kind: &str, rule_id: &str) -> StorageResult<()> {
        sqlx::query("DELETE FROM push_rules WHERE user_id=$1 AND kind=$2 AND rule_id=$3")
            .bind(user_id).bind(kind).bind(rule_id).execute(pool).await?;
        Ok(())
    }

    pub async fn enqueue_push(pool: &sqlx::PgPool, record: &PendingPushRecord) -> StorageResult<()> {
        sqlx::query(
            "INSERT INTO pending_pushes(id,user_id,pushkey,app_id,url,payload,attempts,next_retry_at,delivered_at,created_at) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)"
        )
        .bind(&record.id).bind(&record.user_id).bind(&record.pushkey).bind(&record.app_id)
        .bind(&record.url).bind(&record.payload).bind(record.attempts).bind(record.next_retry_at)
        .bind(record.delivered_at).bind(record.created_at)
        .execute(pool).await?;
        Ok(())
    }

    pub async fn get_pending_pushes(
        pool: &sqlx::PgPool,
        limit: i64,
    ) -> StorageResult<Vec<PendingPushRecord>> {
        Ok(sqlx::query_as::<_, PendingPushRecord>(
            "SELECT id,user_id,pushkey,app_id,url,payload,attempts,next_retry_at,delivered_at,created_at FROM pending_pushes \
             WHERE delivered_at IS NULL AND (next_retry_at IS NULL OR next_retry_at <= NOW()) ORDER BY created_at LIMIT $1"
        ).bind(limit).fetch_all(pool).await?)
    }

    pub async fn mark_push_delivered(pool: &sqlx::PgPool, id: &str) -> StorageResult<()> {
        sqlx::query("UPDATE pending_pushes SET delivered_at=NOW() WHERE id=$1")
            .bind(id).execute(pool).await?;
        Ok(())
    }

    pub async fn increment_push_attempts(
        pool: &sqlx::PgPool,
        id: &str,
        next_retry_at: chrono::DateTime<chrono::Utc>,
    ) -> StorageResult<()> {
        sqlx::query("UPDATE pending_pushes SET attempts=attempts+1, next_retry_at=$2 WHERE id=$1")
            .bind(id).bind(next_retry_at).execute(pool).await?;
        Ok(())
    }
}

mod mongo {
    use super::{PendingPushRecord, PushRuleOverrideRecord, PushRuleRecord, PusherRecord};
    use crate::db::StorageResult;
    use futures::stream::TryStreamExt;
    use mongodb::bson::doc;
    use mongodb::Database;

    fn pushers(db: &Database) -> mongodb::Collection<PusherRecord> {
        db.collection("pushers")
    }
    fn push_rule_overrides(db: &Database) -> mongodb::Collection<PushRuleOverrideRecord> {
        db.collection("push_rule_overrides")
    }
    fn push_rules(db: &Database) -> mongodb::Collection<PushRuleRecord> {
        db.collection("push_rules")
    }
    fn pending_pushes(db: &Database) -> mongodb::Collection<PendingPushRecord> {
        db.collection("pending_pushes")
    }

    pub async fn set_pusher(db: &Database, record: &PusherRecord) -> StorageResult<()> {
        pushers(db)
            .find_one_and_replace(
                doc! { "user_id": &record.user_id, "pushkey": &record.pushkey, "app_id": &record.app_id },
                record,
            )
            .upsert(true)
            .await?;
        Ok(())
    }

    pub async fn delete_pusher(
        db: &Database,
        user_id: &str,
        pushkey: &str,
        app_id: &str,
    ) -> StorageResult<()> {
        pushers(db)
            .delete_one(doc! { "user_id": user_id, "pushkey": pushkey, "app_id": app_id })
            .await?;
        Ok(())
    }

    pub async fn list_pushers(db: &Database, user_id: &str) -> StorageResult<Vec<PusherRecord>> {
        let cursor = pushers(db).find(doc! { "user_id": user_id }).await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn set_push_rule_enabled(
        db: &Database,
        record: &PushRuleOverrideRecord,
    ) -> StorageResult<()> {
        push_rule_overrides(db)
            .find_one_and_replace(
                doc! { "user_id": &record.user_id, "rule_id": &record.rule_id },
                record,
            )
            .upsert(true)
            .await?;
        Ok(())
    }

    pub async fn get_push_rule_enabled(
        db: &Database,
        user_id: &str,
        rule_id: &str,
    ) -> StorageResult<Option<bool>> {
        Ok(push_rule_overrides(db)
            .find_one(doc! { "user_id": user_id, "rule_id": rule_id })
            .await?
            .map(|record| record.enabled))
    }

    pub async fn upsert_push_rule(db: &Database, record: &PushRuleRecord) -> StorageResult<()> {
        push_rules(db)
            .find_one_and_replace(
                doc! { "user_id": &record.user_id, "kind": &record.kind, "rule_id": &record.rule_id },
                record,
            )
            .upsert(true)
            .await?;
        Ok(())
    }

    pub async fn list_push_rules(db: &Database, user_id: &str) -> StorageResult<Vec<PushRuleRecord>> {
        let cursor = push_rules(db).find(doc! { "user_id": user_id }).sort(doc! { "updated_at": -1 }).await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn get_push_rule(db: &Database, user_id: &str, kind: &str, rule_id: &str) -> StorageResult<Option<PushRuleRecord>> {
        Ok(push_rules(db).find_one(doc! { "user_id": user_id, "kind": kind, "rule_id": rule_id }).await?)
    }

    pub async fn delete_push_rule(db: &Database, user_id: &str, kind: &str, rule_id: &str) -> StorageResult<()> {
        push_rules(db).delete_one(doc! { "user_id": user_id, "kind": kind, "rule_id": rule_id }).await?;
        Ok(())
    }

    pub async fn enqueue_push(db: &Database, record: &PendingPushRecord) -> StorageResult<()> {
        pending_pushes(db).insert_one(record).await?;
        Ok(())
    }

    pub async fn get_pending_pushes(
        db: &Database,
        limit: i64,
    ) -> StorageResult<Vec<PendingPushRecord>> {
        let now = mongodb::bson::DateTime::now();
        let cursor = pending_pushes(db)
            .find(doc! {
                "delivered_at": null,
                "$or": [ { "next_retry_at": null }, { "next_retry_at": { "$lte": now } } ],
            })
            .sort(doc! { "created_at": 1 })
            .limit(limit)
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn mark_push_delivered(db: &Database, id: &str) -> StorageResult<()> {
        pending_pushes(db)
            .update_one(
                doc! { "id": id },
                doc! { "$set": { "delivered_at": mongodb::bson::DateTime::now() } },
            )
            .await?;
        Ok(())
    }

    pub async fn increment_push_attempts(
        db: &Database,
        id: &str,
        next_retry_at: chrono::DateTime<chrono::Utc>,
    ) -> StorageResult<()> {
        pending_pushes(db)
            .update_one(
                doc! { "id": id },
                doc! { "$inc": { "attempts": 1 }, "$set": { "next_retry_at": mongodb::bson::DateTime::from_chrono(next_retry_at) } },
            )
            .await?;
        Ok(())
    }
}
