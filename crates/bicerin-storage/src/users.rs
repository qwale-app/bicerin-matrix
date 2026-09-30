use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct UserRecord {
    pub user_id: String,
    pub localpart: String,
    pub password_hash: Option<String>,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub is_guest: bool,
    pub is_deactivated: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct AccessTokenRecord {
    pub token_hash: String,
    pub user_id: String,
    pub device_id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, Deserialize)]
pub struct DeviceRecord {
    pub device_id: String,
    pub user_id: String,
    pub display_name: Option<String>,
    pub last_seen_ip: Option<String>,
    pub last_seen_ts: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}


use crate::db::StorageResult;
use crate::store::Store;

pub async fn create_user(store: &Store, user: &UserRecord) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::create_user(pool, user).await,
        Store::Mongo(backend) => mongo::create_user(&backend.database, user).await,
    }
}

pub async fn get_user(store: &Store, user_id: &str) -> StorageResult<UserRecord> {
    match store {
        Store::Postgres(pool) => pg::get_user(pool, user_id).await,
        Store::Mongo(backend) => mongo::get_user(&backend.database, user_id).await,
    }
}

pub async fn get_user_by_localpart(store: &Store, localpart: &str, server_name: &str) -> StorageResult<UserRecord> {
    let user_id = format!("@{}:{}", localpart, server_name);
    get_user(store, &user_id).await
}

pub async fn create_access_token(store: &Store, token: &AccessTokenRecord) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::create_access_token(pool, token).await,
        Store::Mongo(backend) => mongo::create_access_token(&backend.database, token).await,
    }
}

pub async fn get_access_token(store: &Store, token_hash: &str) -> StorageResult<AccessTokenRecord> {
    match store {
        Store::Postgres(pool) => pg::get_access_token(pool, token_hash).await,
        Store::Mongo(backend) => mongo::get_access_token(&backend.database, token_hash).await,
    }
}

pub async fn delete_access_token(store: &Store, token_hash: &str) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::delete_access_token(pool, token_hash).await,
        Store::Mongo(backend) => mongo::delete_access_token(&backend.database, token_hash).await,
    }
}

pub async fn create_device(store: &Store, device: &DeviceRecord) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::create_device(pool, device).await,
        Store::Mongo(backend) => mongo::create_device(&backend.database, device).await,
    }
}

pub async fn get_device(store: &Store, user_id: &str, device_id: &str) -> StorageResult<DeviceRecord> {
    match store {
        Store::Postgres(pool) => pg::get_device(pool, user_id, device_id).await,
        Store::Mongo(backend) => mongo::get_device(&backend.database, user_id, device_id).await,
    }
}

pub async fn list_devices(store: &Store, user_id: &str) -> StorageResult<Vec<DeviceRecord>> {
    match store {
        Store::Postgres(pool) => pg::list_devices(pool, user_id).await,
        Store::Mongo(backend) => mongo::list_devices(&backend.database, user_id).await,
    }
}

pub async fn delete_device(store: &Store, user_id: &str, device_id: &str) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::delete_device(pool, user_id, device_id).await,
        Store::Mongo(backend) => mongo::delete_device(&backend.database, user_id, device_id).await,
    }
}

pub async fn update_device_display_name(store: &Store, user_id: &str, device_id: &str, display_name: &str) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::update_device_display_name(pool, user_id, device_id, display_name).await,
        Store::Mongo(backend) => mongo::update_device_display_name(&backend.database, user_id, device_id, display_name).await,
    }
}

pub async fn update_display_name(store: &Store, user_id: &str, display_name: Option<&str>) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::update_display_name(pool, user_id, display_name).await,
        Store::Mongo(backend) => mongo::update_display_name(&backend.database, user_id, display_name).await,
    }
}

pub async fn update_avatar_url(store: &Store, user_id: &str, avatar_url: Option<&str>) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::update_avatar_url(pool, user_id, avatar_url).await,
        Store::Mongo(backend) => mongo::update_avatar_url(&backend.database, user_id, avatar_url).await,
    }
}

pub async fn delete_access_tokens_for_device(store: &Store, user_id: &str, device_id: &str) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::delete_access_tokens_for_device(pool, user_id, device_id).await,
        Store::Mongo(backend) => mongo::delete_access_tokens_for_device(&backend.database, user_id, device_id).await,
    }
}

pub async fn delete_access_tokens_for_user(store: &Store, user_id: &str) -> StorageResult<()> {
    match store {
        Store::Postgres(pool) => pg::delete_access_tokens_for_user(pool, user_id).await,
        Store::Mongo(backend) => mongo::delete_access_tokens_for_user(&backend.database, user_id).await,
    }
}

mod pg {
    use super::{AccessTokenRecord, DeviceRecord, UserRecord};
    use crate::db::{StorageError, StorageResult};

    pub async fn create_user(pool: &sqlx::PgPool, user: &UserRecord) -> StorageResult<()> {
        sqlx::query(
            "INSERT INTO users (user_id, localpart, password_hash, display_name, avatar_url, is_guest, is_deactivated, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) ON CONFLICT (user_id) DO NOTHING"
        )
        .bind(&user.user_id)
        .bind(&user.localpart)
        .bind(&user.password_hash)
        .bind(&user.display_name)
        .bind(&user.avatar_url)
        .bind(user.is_guest)
        .bind(user.is_deactivated)
        .bind(user.created_at)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_user(pool: &sqlx::PgPool, user_id: &str) -> StorageResult<UserRecord> {
        sqlx::query_as::<_, UserRecord>(
            "SELECT user_id, localpart, password_hash, display_name, avatar_url, is_guest, is_deactivated, created_at FROM users WHERE user_id = $1"
        )
        .bind(user_id)
        .fetch_optional(pool)
        .await?
        .ok_or(StorageError::NotFound)
    }

    pub async fn create_access_token(pool: &sqlx::PgPool, token: &AccessTokenRecord) -> StorageResult<()> {
        sqlx::query(
            "INSERT INTO access_tokens (token_hash, user_id, device_id, created_at, expires_at, last_used_at) VALUES ($1, $2, $3, $4, $5, $6)"
        )
        .bind(&token.token_hash)
        .bind(&token.user_id)
        .bind(&token.device_id)
        .bind(token.created_at)
        .bind(token.expires_at)
        .bind(token.last_used_at)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_access_token(pool: &sqlx::PgPool, token_hash: &str) -> StorageResult<AccessTokenRecord> {
        sqlx::query_as::<_, AccessTokenRecord>(
            "SELECT token_hash, user_id, device_id, created_at, expires_at, last_used_at FROM access_tokens WHERE token_hash = $1"
        )
        .bind(token_hash)
        .fetch_optional(pool)
        .await?
        .ok_or(StorageError::NotFound)
    }

    pub async fn delete_access_token(pool: &sqlx::PgPool, token_hash: &str) -> StorageResult<()> {
        sqlx::query("DELETE FROM access_tokens WHERE token_hash = $1")
            .bind(token_hash)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn create_device(pool: &sqlx::PgPool, device: &DeviceRecord) -> StorageResult<()> {
        sqlx::query(
            "INSERT INTO devices (device_id, user_id, display_name, last_seen_ip, last_seen_ts, created_at) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (user_id, device_id) DO NOTHING"
        )
        .bind(&device.device_id)
        .bind(&device.user_id)
        .bind(&device.display_name)
        .bind(&device.last_seen_ip)
        .bind(device.last_seen_ts)
        .bind(device.created_at)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_device(pool: &sqlx::PgPool, user_id: &str, device_id: &str) -> StorageResult<DeviceRecord> {
        sqlx::query_as::<_, DeviceRecord>(
            "SELECT device_id, user_id, display_name, last_seen_ip, last_seen_ts, created_at FROM devices WHERE user_id = $1 AND device_id = $2"
        )
        .bind(user_id)
        .bind(device_id)
        .fetch_optional(pool)
        .await?
        .ok_or(StorageError::NotFound)
    }

    pub async fn list_devices(pool: &sqlx::PgPool, user_id: &str) -> StorageResult<Vec<DeviceRecord>> {
        let devices = sqlx::query_as::<_, DeviceRecord>(
            "SELECT device_id, user_id, display_name, last_seen_ip, last_seen_ts, created_at FROM devices WHERE user_id = $1 ORDER BY created_at"
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;
        Ok(devices)
    }

    pub async fn delete_device(pool: &sqlx::PgPool, user_id: &str, device_id: &str) -> StorageResult<()> {
        sqlx::query("DELETE FROM devices WHERE user_id = $1 AND device_id = $2")
            .bind(user_id)
            .bind(device_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn update_device_display_name(pool: &sqlx::PgPool, user_id: &str, device_id: &str, display_name: &str) -> StorageResult<()> {
        sqlx::query("UPDATE devices SET display_name = $3 WHERE user_id = $1 AND device_id = $2")
            .bind(user_id)
            .bind(device_id)
            .bind(display_name)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn update_display_name(pool: &sqlx::PgPool, user_id: &str, display_name: Option<&str>) -> StorageResult<()> {
        sqlx::query("UPDATE users SET display_name = $2 WHERE user_id = $1")
            .bind(user_id)
            .bind(display_name)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn update_avatar_url(pool: &sqlx::PgPool, user_id: &str, avatar_url: Option<&str>) -> StorageResult<()> {
        sqlx::query("UPDATE users SET avatar_url = $2 WHERE user_id = $1")
            .bind(user_id)
            .bind(avatar_url)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn delete_access_tokens_for_device(pool: &sqlx::PgPool, user_id: &str, device_id: &str) -> StorageResult<()> {
        sqlx::query("DELETE FROM access_tokens WHERE user_id = $1 AND device_id = $2")
            .bind(user_id)
            .bind(device_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn delete_access_tokens_for_user(pool: &sqlx::PgPool, user_id: &str) -> StorageResult<()> {
        sqlx::query("DELETE FROM access_tokens WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await?;
        Ok(())
    }
}

mod mongo {
    use super::{AccessTokenRecord, DeviceRecord, UserRecord};
    use crate::db::{is_duplicate_key_error, StorageError, StorageResult};
    use mongodb::bson::doc;
    use mongodb::Database;

    fn users(db: &Database) -> mongodb::Collection<UserRecord> {
        db.collection("users")
    }
    fn devices(db: &Database) -> mongodb::Collection<DeviceRecord> {
        db.collection("devices")
    }
    fn access_tokens(db: &Database) -> mongodb::Collection<AccessTokenRecord> {
        db.collection("access_tokens")
    }

    pub async fn create_user(db: &Database, user: &UserRecord) -> StorageResult<()> {
        match users(db).insert_one(user).await {
            Ok(_) => Ok(()),
            Err(e) if is_duplicate_key_error(&e) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn get_user(db: &Database, user_id: &str) -> StorageResult<UserRecord> {
        users(db)
            .find_one(doc! { "user_id": user_id })
            .await?
            .ok_or(StorageError::NotFound)
    }

    pub async fn create_access_token(db: &Database, token: &AccessTokenRecord) -> StorageResult<()> {
        access_tokens(db).insert_one(token).await?;
        Ok(())
    }

    pub async fn get_access_token(db: &Database, token_hash: &str) -> StorageResult<AccessTokenRecord> {
        access_tokens(db)
            .find_one(doc! { "token_hash": token_hash })
            .await?
            .ok_or(StorageError::NotFound)
    }

    pub async fn delete_access_token(db: &Database, token_hash: &str) -> StorageResult<()> {
        access_tokens(db).delete_one(doc! { "token_hash": token_hash }).await?;
        Ok(())
    }

    pub async fn create_device(db: &Database, device: &DeviceRecord) -> StorageResult<()> {
        match devices(db).insert_one(device).await {
            Ok(_) => Ok(()),
            Err(e) if is_duplicate_key_error(&e) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn get_device(db: &Database, user_id: &str, device_id: &str) -> StorageResult<DeviceRecord> {
        devices(db)
            .find_one(doc! { "user_id": user_id, "device_id": device_id })
            .await?
            .ok_or(StorageError::NotFound)
    }

    pub async fn list_devices(db: &Database, user_id: &str) -> StorageResult<Vec<DeviceRecord>> {
        use futures::stream::TryStreamExt;
        let cursor = devices(db)
            .find(doc! { "user_id": user_id })
            .sort(doc! { "created_at": 1 })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    pub async fn delete_device(db: &Database, user_id: &str, device_id: &str) -> StorageResult<()> {
        devices(db).delete_one(doc! { "user_id": user_id, "device_id": device_id }).await?;
        Ok(())
    }

    pub async fn update_device_display_name(db: &Database, user_id: &str, device_id: &str, display_name: &str) -> StorageResult<()> {
        devices(db)
            .update_one(
                doc! { "user_id": user_id, "device_id": device_id },
                doc! { "$set": { "display_name": display_name } },
            )
            .await?;
        Ok(())
    }

    pub async fn update_display_name(db: &Database, user_id: &str, display_name: Option<&str>) -> StorageResult<()> {
        users(db)
            .update_one(doc! { "user_id": user_id }, doc! { "$set": { "display_name": display_name } })
            .await?;
        Ok(())
    }

    pub async fn update_avatar_url(db: &Database, user_id: &str, avatar_url: Option<&str>) -> StorageResult<()> {
        users(db)
            .update_one(doc! { "user_id": user_id }, doc! { "$set": { "avatar_url": avatar_url } })
            .await?;
        Ok(())
    }

    pub async fn delete_access_tokens_for_device(db: &Database, user_id: &str, device_id: &str) -> StorageResult<()> {
        access_tokens(db)
            .delete_many(doc! { "user_id": user_id, "device_id": device_id })
            .await?;
        Ok(())
    }

    pub async fn delete_access_tokens_for_user(db: &Database, user_id: &str) -> StorageResult<()> {
        access_tokens(db).delete_many(doc! { "user_id": user_id }).await?;
        Ok(())
    }
}
