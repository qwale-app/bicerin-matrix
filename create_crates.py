import os

root_dir = r"C:\Users\faraa\OneDrive\Desktop\bicerin-matrix\crates"

files = {
    "bicerin-auth/Cargo.toml": r"""[package]
name = "bicerin-auth"
version = "0.1.0"
edition = "2021"

[dependencies]
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
thiserror = { workspace = true }
sha2 = { workspace = true }
hex = { workspace = true }
rand = { workspace = true }
base64 = { workspace = true }
async-trait = { workspace = true }
moka = { workspace = true }
argon2 = "0.5"

bicerin-types = { path = "../bicerin-types" }
bicerin-error = { path = "../bicerin-error" }
bicerin-config = { path = "../bicerin-config" }
bicerin-storage = { path = "../bicerin-storage" }
""",

    "bicerin-auth/src/lib.rs": r"""pub mod password;
pub mod token;
pub mod middleware;

use bicerin_error::BicerinResult;
use moka::future::Cache;
use std::time::Duration;

#[derive(Clone)]
pub struct AuthService {
    pub pool: sqlx::PgPool,
    pub token_cache: Cache<String, (String, String)>,
}

impl AuthService {
    pub fn new(pool: sqlx::PgPool) -> Self {
        let token_cache = Cache::builder()
            .max_capacity(100_000)
            .time_to_live(Duration::from_secs(5 * 60))
            .build();
            
        Self { pool, token_cache }
    }

    pub async fn authenticate(&self, token: &str) -> BicerinResult<(String, String)> {
        if let Some(ident) = self.token_cache.get(token).await {
            return Ok(ident);
        }

        let hashed_token = bicerin_types::auth::hash_access_token(token);
        
        // Retrieve session (Assuming get_session_by_token exists)
        let session = bicerin_storage::auth::get_session_by_token(&self.pool, &hashed_token)
            .await
            .map_err(|_| bicerin_error::BicerinError::Unauthorized)?;

        let ident = (session.user_id, session.device_id);
        self.token_cache.insert(token.to_string(), ident.clone()).await;
        Ok(ident)
    }

    pub async fn invalidate_token(&self, token: &str) {
        self.token_cache.invalidate(token).await;
    }
}
""",

    "bicerin-auth/src/password.rs": r"""use sha2::{Sha256, Digest};

pub fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    use argon2::{password_hash::{SaltString, PasswordHasher}, Argon2};
    let salt = SaltString::generate(&mut rand::rngs::OsRng);
    let argon2 = Argon2::default();
    let hash = argon2.hash_password(password.as_bytes(), &salt)?;
    Ok(hash.to_string())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    use argon2::{password_hash::{PasswordHash, PasswordVerifier}, Argon2};
    let parsed_hash = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return false,
    };
    Argon2::default().verify_password(password.as_bytes(), &parsed_hash).is_ok()
}
""",

    "bicerin-auth/src/token.rs": r"""use rand::Rng;
use base64::Engine;

/// Generate a cryptographically random access token.
/// Returns a 32-byte random value, base64url-encoded.
pub fn generate_access_token() -> String {
    let bytes: Vec<u8> = (0..32).map(|_| rand::thread_rng().gen()).collect();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&bytes)
}

/// Generate a random device ID if not provided by client.
pub fn generate_device_id() -> String {
    let bytes: Vec<u8> = (0..8).map(|_| rand::thread_rng().gen()).collect();
    hex::encode(&bytes).to_uppercase()
}
""",

    "bicerin-auth/src/middleware.rs": r"""use axum::{extract::{FromRequestParts, State}, http::request::Parts, async_trait};
use axum::http::StatusCode;

// AuthenticatedUser is an extractor that pulls the Bearer token and validates it.
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub user_id: String,
    pub device_id: String,
}

// We use a wrapper type to avoid orphan rules:
// Implement FromRequestParts<AppState> in bicerin-http instead.
// This module just provides the struct and helper.

impl AuthenticatedUser {
    /// Extract Bearer token from Authorization header.
    pub fn extract_bearer(parts: &Parts) -> Option<String> {
        let auth_header = parts.headers.get(axum::http::header::AUTHORIZATION)?;
        let value = auth_header.to_str().ok()?;
        value.strip_prefix("Bearer ").map(|t| t.to_owned())
    }
}
""",

    "bicerin-rooms/Cargo.toml": r"""[package]
name = "bicerin-rooms"
version = "0.1.0"
edition = "2021"

[dependencies]
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
thiserror = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }
async-trait = { workspace = true }

bicerin-types = { path = "../bicerin-types" }
bicerin-error = { path = "../bicerin-error" }
bicerin-storage = { path = "../bicerin-storage" }
""",

    "bicerin-rooms/src/lib.rs": r"""pub mod service;
pub mod membership;
pub mod state;
pub mod powerlevels;
pub mod creation;

pub use service::RoomService;
""",

    "bicerin-rooms/src/service.rs": r"""use bicerin_storage::{PgStore, rooms::*, events::*};
use bicerin_error::{BicerinError, BicerinResult};

pub struct RoomService {
    pub pool: sqlx::PgPool,
}

impl RoomService {
    pub fn new(pool: sqlx::PgPool) -> Self { Self { pool } }
}
""",

    "bicerin-rooms/src/creation.rs": r"""use crate::service::RoomService;
use bicerin_storage::{rooms::*, events::*};
use bicerin_error::BicerinResult;
use serde_json::{json, Value};

#[derive(Debug, serde::Deserialize)]
pub struct CreateRoomParams {
    pub creator: String,
    pub room_version: String,
    pub name: Option<String>,
    pub topic: Option<String>,
    pub is_direct: bool,
    pub initial_state: Vec<InitialStateEvent>,
    pub invite: Vec<String>,
    pub preset: Option<String>,
    pub room_alias_name: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct InitialStateEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub state_key: Option<String>,
    pub content: Value,
}

impl RoomService {
    pub async fn create_room(
        &self,
        params: CreateRoomParams,
        server_name: &str,
        next_stream_id: i64,
    ) -> BicerinResult<(String, Vec<String>)> {
        let room_id = format!("!{}:{}", uuid::Uuid::new_v4().to_string().replace('-', ""), server_name);
        let now_ts = chrono::Utc::now().timestamp_millis();

        let room_record = RoomRecord {
            room_id: room_id.clone(),
            creator: params.creator.clone(),
            room_version: params.room_version.clone(),
            is_encrypted: false,
            is_direct: params.is_direct,
            name: params.name.clone(),
            topic: params.topic.clone(),
            canonical_alias: None,
            creation_ts: now_ts,
            created_at: chrono::Utc::now(),
        };

        bicerin_storage::rooms::create_room(&self.pool, &room_record)
            .await
            .map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;

        let mut event_ids = vec![];
        let creation_event_id = format!("${}:{}", uuid::Uuid::new_v4().to_string().replace('-', ""), server_name);
        let power_levels_event_id = format!("${}:{}", uuid::Uuid::new_v4().to_string().replace('-', ""), server_name);
        let join_rules_event_id = format!("${}:{}", uuid::Uuid::new_v4().to_string().replace('-', ""), server_name);
        let member_event_id = format!("${}:{}", uuid::Uuid::new_v4().to_string().replace('-', ""), server_name);

        let (join_rule, history_vis, guest_access, pl_state_default, pl_events_default) = match params.preset.as_deref() {
            Some("public_chat") => ("public", "shared", "forbidden", 50, 0),
            Some("trusted_private_chat") => ("invite", "shared", "forbidden", 0, 0),
            _ => ("invite", "shared", "forbidden", 50, 0),
        };

        let power_levels_content = json!({
            "ban": 50,
            "events": {},
            "events_default": pl_events_default,
            "invite": 0,
            "kick": 50,
            "notifications": {"room": 50},
            "redact": 50,
            "state_default": pl_state_default,
            "users": {&params.creator: 100},
            "users_default": 0
        });

        // Creation event
        let creation_event = EventRecord {
            event_id: creation_event_id.clone(),
            room_id: room_id.clone(),
            sender: params.creator.clone(),
            stream_id: next_stream_id,
            origin_server_ts: now_ts,
            event_type: "m.room.create".to_string(),
            state_key: Some("".to_string()),
            room_version: params.room_version.clone(),
            content: json!({"creator": params.creator, "room_version": params.room_version, "m.federate": false}),
            unsigned: None,
            redacts: None,
            depth: 1,
            auth_events: vec![],
            prev_events: vec![],
            created_at: chrono::Utc::now(),
        };
        bicerin_storage::events::insert_event(&self.pool, &creation_event)
            .await
            .map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;
        bicerin_storage::rooms::upsert_room_state(&self.pool, &bicerin_storage::rooms::RoomStateRecord {
            room_id: room_id.clone(), event_type: "m.room.create".to_string(), state_key: "".to_string(),
            event_id: creation_event_id.clone(), content: creation_event.content.clone(),
            sender: params.creator.clone(), stream_id: next_stream_id, updated_at: chrono::Utc::now(),
        }).await.map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;
        event_ids.push(creation_event_id.clone());

        // Member event
        let join_event = EventRecord {
            event_id: member_event_id.clone(),
            room_id: room_id.clone(),
            sender: params.creator.clone(),
            stream_id: next_stream_id + 1,
            origin_server_ts: now_ts,
            event_type: "m.room.member".to_string(),
            state_key: Some(params.creator.clone()),
            room_version: params.room_version.clone(),
            content: json!({"membership": "join", "displayname": null}),
            unsigned: None,
            redacts: None,
            depth: 2,
            auth_events: vec![creation_event_id.clone()],
            prev_events: vec![creation_event_id.clone()],
            created_at: chrono::Utc::now(),
        };
        bicerin_storage::events::insert_event(&self.pool, &join_event)
            .await
            .map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;
        bicerin_storage::rooms::upsert_room_member(&self.pool, &RoomMemberRecord {
            room_id: room_id.clone(), user_id: params.creator.clone(), membership: "join".to_string(),
            display_name: None, avatar_url: None, sender: params.creator.clone(),
            event_id: member_event_id.clone(), stream_id: next_stream_id + 1, updated_at: chrono::Utc::now(),
        }).await.map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;
        bicerin_storage::rooms::upsert_room_state(&self.pool, &bicerin_storage::rooms::RoomStateRecord {
            room_id: room_id.clone(), event_type: "m.room.member".to_string(), state_key: params.creator.clone(),
            event_id: member_event_id.clone(), content: join_event.content.clone(),
            sender: params.creator.clone(), stream_id: next_stream_id + 1, updated_at: chrono::Utc::now(),
        }).await.map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;
        event_ids.push(member_event_id);

        // Power levels event
        let pl_event = EventRecord {
            event_id: power_levels_event_id.clone(),
            room_id: room_id.clone(),
            sender: params.creator.clone(),
            stream_id: next_stream_id + 2,
            origin_server_ts: now_ts,
            event_type: "m.room.power_levels".to_string(),
            state_key: Some("".to_string()),
            room_version: params.room_version.clone(),
            content: power_levels_content.clone(),
            unsigned: None,
            redacts: None,
            depth: 3,
            auth_events: vec![creation_event_id.clone()],
            prev_events: vec![creation_event_id.clone()],
            created_at: chrono::Utc::now(),
        };
        bicerin_storage::events::insert_event(&self.pool, &pl_event)
            .await
            .map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;
        bicerin_storage::rooms::upsert_room_state(&self.pool, &bicerin_storage::rooms::RoomStateRecord {
            room_id: room_id.clone(), event_type: "m.room.power_levels".to_string(), state_key: "".to_string(),
            event_id: power_levels_event_id.clone(), content: power_levels_content,
            sender: params.creator.clone(), stream_id: next_stream_id + 2, updated_at: chrono::Utc::now(),
        }).await.map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;
        event_ids.push(power_levels_event_id);

        // Join rules event
        let jr_event = EventRecord {
            event_id: join_rules_event_id.clone(),
            room_id: room_id.clone(),
            sender: params.creator.clone(),
            stream_id: next_stream_id + 3,
            origin_server_ts: now_ts,
            event_type: "m.room.join_rules".to_string(),
            state_key: Some("".to_string()),
            room_version: params.room_version.clone(),
            content: json!({"join_rule": join_rule}),
            unsigned: None,
            redacts: None,
            depth: 4,
            auth_events: vec![creation_event_id.clone()],
            prev_events: vec![creation_event_id.clone()],
            created_at: chrono::Utc::now(),
        };
        bicerin_storage::events::insert_event(&self.pool, &jr_event)
            .await
            .map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;
        bicerin_storage::rooms::upsert_room_state(&self.pool, &bicerin_storage::rooms::RoomStateRecord {
            room_id: room_id.clone(), event_type: "m.room.join_rules".to_string(), state_key: "".to_string(),
            event_id: join_rules_event_id.clone(), content: jr_event.content.clone(),
            sender: params.creator.clone(), stream_id: next_stream_id + 3, updated_at: chrono::Utc::now(),
        }).await.map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;
        event_ids.push(join_rules_event_id);

        Ok((room_id, event_ids))
    }
}
""",

    "bicerin-rooms/src/membership.rs": r"""use crate::service::RoomService;
use bicerin_storage::{rooms::*, events::*};
use bicerin_error::{BicerinError, BicerinResult};
use serde_json::json;

impl RoomService {
    pub async fn join_room(
        &self,
        room_id: &str,
        user_id: &str,
        server_name: &str,
        stream_id: i64,
    ) -> BicerinResult<String> {
        bicerin_storage::rooms::get_room(&self.pool, room_id)
            .await
            .map_err(|_| BicerinError::NotFound)?;

        let event_id = format!("${}:{}", uuid::Uuid::new_v4().to_string().replace('-', ""), server_name);
        let now_ts = chrono::Utc::now().timestamp_millis();

        let event = EventRecord {
            event_id: event_id.clone(),
            room_id: room_id.to_string(),
            sender: user_id.to_string(),
            stream_id,
            origin_server_ts: now_ts,
            event_type: "m.room.member".to_string(),
            state_key: Some(user_id.to_string()),
            room_version: bicerin_storage::rooms::get_room(&self.pool, room_id)
                .await.map_err(|e| BicerinError::Internal(e.to_string()))?
                .room_version,
            content: json!({"membership": "join"}),
            unsigned: None,
            redacts: None,
            depth: 0,
            auth_events: vec![],
            prev_events: vec![],
            created_at: chrono::Utc::now(),
        };

        bicerin_storage::events::insert_event(&self.pool, &event)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;

        bicerin_storage::rooms::upsert_room_member(&self.pool, &RoomMemberRecord {
            room_id: room_id.to_string(),
            user_id: user_id.to_string(),
            membership: "join".to_string(),
            display_name: None,
            avatar_url: None,
            sender: user_id.to_string(),
            event_id: event_id.clone(),
            stream_id,
            updated_at: chrono::Utc::now(),
        }).await.map_err(|e| BicerinError::Internal(e.to_string()))?;

        bicerin_storage::rooms::upsert_room_state(&self.pool, &RoomStateRecord {
            room_id: room_id.to_string(),
            event_type: "m.room.member".to_string(),
            state_key: user_id.to_string(),
            event_id: event_id.clone(),
            content: event.content.clone(),
            sender: user_id.to_string(),
            stream_id,
            updated_at: chrono::Utc::now(),
        }).await.map_err(|e| BicerinError::Internal(e.to_string()))?;

        Ok(event_id)
    }

    pub async fn leave_room(
        &self,
        room_id: &str,
        user_id: &str,
        server_name: &str,
        stream_id: i64,
    ) -> BicerinResult<String> {
        let room = bicerin_storage::rooms::get_room(&self.pool, room_id)
            .await
            .map_err(|_| BicerinError::NotFound)?;

        let event_id = format!("${}:{}", uuid::Uuid::new_v4().to_string().replace('-', ""), server_name);
        let now_ts = chrono::Utc::now().timestamp_millis();

        let event = EventRecord {
            event_id: event_id.clone(),
            room_id: room_id.to_string(),
            sender: user_id.to_string(),
            stream_id,
            origin_server_ts: now_ts,
            event_type: "m.room.member".to_string(),
            state_key: Some(user_id.to_string()),
            room_version: room.room_version,
            content: json!({"membership": "leave"}),
            unsigned: None,
            redacts: None,
            depth: 0,
            auth_events: vec![],
            prev_events: vec![],
            created_at: chrono::Utc::now(),
        };

        bicerin_storage::events::insert_event(&self.pool, &event)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;

        bicerin_storage::rooms::upsert_room_member(&self.pool, &RoomMemberRecord {
            room_id: room_id.to_string(),
            user_id: user_id.to_string(),
            membership: "leave".to_string(),
            display_name: None,
            avatar_url: None,
            sender: user_id.to_string(),
            event_id: event_id.clone(),
            stream_id,
            updated_at: chrono::Utc::now(),
        }).await.map_err(|e| BicerinError::Internal(e.to_string()))?;

        bicerin_storage::rooms::upsert_room_state(&self.pool, &RoomStateRecord {
            room_id: room_id.to_string(),
            event_type: "m.room.member".to_string(),
            state_key: user_id.to_string(),
            event_id: event_id.clone(),
            content: event.content.clone(),
            sender: user_id.to_string(),
            stream_id,
            updated_at: chrono::Utc::now(),
        }).await.map_err(|e| BicerinError::Internal(e.to_string()))?;

        Ok(event_id)
    }

    pub async fn invite_user(
        &self,
        room_id: &str,
        inviter_id: &str,
        invitee_id: &str,
        server_name: &str,
        stream_id: i64,
    ) -> BicerinResult<String> {
        let room = bicerin_storage::rooms::get_room(&self.pool, room_id)
            .await
            .map_err(|_| BicerinError::NotFound)?;

        let inviter_member = bicerin_storage::rooms::get_room_member(&self.pool, room_id, inviter_id)
            .await
            .map_err(|_| BicerinError::Forbidden)?;
        if inviter_member.membership != "join" {
            return Err(BicerinError::Forbidden);
        }

        let event_id = format!("${}:{}", uuid::Uuid::new_v4().to_string().replace('-', ""), server_name);
        let now_ts = chrono::Utc::now().timestamp_millis();

        let event = EventRecord {
            event_id: event_id.clone(),
            room_id: room_id.to_string(),
            sender: inviter_id.to_string(),
            stream_id,
            origin_server_ts: now_ts,
            event_type: "m.room.member".to_string(),
            state_key: Some(invitee_id.to_string()),
            room_version: room.room_version,
            content: json!({"membership": "invite"}),
            unsigned: None,
            redacts: None,
            depth: 0,
            auth_events: vec![],
            prev_events: vec![],
            created_at: chrono::Utc::now(),
        };

        bicerin_storage::events::insert_event(&self.pool, &event)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;

        bicerin_storage::rooms::upsert_room_member(&self.pool, &RoomMemberRecord {
            room_id: room_id.to_string(),
            user_id: invitee_id.to_string(),
            membership: "invite".to_string(),
            display_name: None,
            avatar_url: None,
            sender: inviter_id.to_string(),
            event_id: event_id.clone(),
            stream_id,
            updated_at: chrono::Utc::now(),
        }).await.map_err(|e| BicerinError::Internal(e.to_string()))?;

        bicerin_storage::rooms::upsert_room_state(&self.pool, &RoomStateRecord {
            room_id: room_id.to_string(),
            event_type: "m.room.member".to_string(),
            state_key: invitee_id.to_string(),
            event_id: event_id.clone(),
            content: event.content.clone(),
            sender: inviter_id.to_string(),
            stream_id,
            updated_at: chrono::Utc::now(),
        }).await.map_err(|e| BicerinError::Internal(e.to_string()))?;

        Ok(event_id)
    }
}
""",

    "bicerin-rooms/src/state.rs": r"""use crate::service::RoomService;
use bicerin_storage::rooms::*;
use bicerin_error::BicerinResult;

impl RoomService {
    pub async fn get_state(
        &self,
        room_id: &str,
    ) -> BicerinResult<Vec<RoomStateRecord>> {
        bicerin_storage::rooms::get_full_room_state(&self.pool, room_id)
            .await
            .map_err(|_| bicerin_error::BicerinError::NotFound)
    }

    pub async fn get_state_event(
        &self,
        room_id: &str,
        event_type: &str,
        state_key: &str,
    ) -> BicerinResult<RoomStateRecord> {
        bicerin_storage::rooms::get_room_state(&self.pool, room_id, event_type, state_key)
            .await
            .map_err(|_| bicerin_error::BicerinError::NotFound)
    }
}
""",

    "bicerin-rooms/src/powerlevels.rs": r"""use serde_json::Value;

pub fn check_power_level(
    power_levels: &Value,
    user_id: &str,
    required: i64,
) -> bool {
    let users_default = power_levels.get("users_default")
        .and_then(|v| v.as_i64()).unwrap_or(0);
    let user_level = power_levels.get("users")
        .and_then(|u| u.get(user_id))
        .and_then(|v| v.as_i64())
        .unwrap_or(users_default);
    user_level >= required
}

pub fn check_send_event(
    power_levels: &Value,
    user_id: &str,
    event_type: &str,
) -> bool {
    let events_default = power_levels.get("events_default")
        .and_then(|v| v.as_i64()).unwrap_or(0);
    let required = power_levels.get("events")
        .and_then(|e| e.get(event_type))
        .and_then(|v| v.as_i64())
        .unwrap_or(events_default);
    check_power_level(power_levels, user_id, required)
}

pub fn check_state_event(
    power_levels: &Value,
    user_id: &str,
    event_type: &str,
) -> bool {
    let state_default = power_levels.get("state_default")
        .and_then(|v| v.as_i64()).unwrap_or(50);
    let required = power_levels.get("events")
        .and_then(|e| e.get(event_type))
        .and_then(|v| v.as_i64())
        .unwrap_or(state_default);
    check_power_level(power_levels, user_id, required)
}
""",

    "bicerin-events/Cargo.toml": r"""[package]
name = "bicerin-events"
version = "0.1.0"
edition = "2021"

[dependencies]
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
thiserror = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }
async-trait = { workspace = true }

bicerin-types = { path = "../bicerin-types" }
bicerin-error = { path = "../bicerin-error" }
bicerin-storage = { path = "../bicerin-storage" }
bicerin-rooms = { path = "../bicerin-rooms" }
""",

    "bicerin-events/src/lib.rs": r"""pub mod service;
pub mod validation;
pub mod relations;

pub use service::EventService;
""",

    "bicerin-events/src/service.rs": r"""use bicerin_storage::{PgStore, events::*};
use bicerin_error::{BicerinError, BicerinResult};
use serde_json::Value;

pub struct EventService {
    pub pool: sqlx::PgPool,
    pub server_name: String,
}

impl EventService {
    pub fn new(pool: sqlx::PgPool, server_name: String) -> Self { Self { pool, server_name } }

    pub async fn send_event(
        &self,
        room_id: &str,
        sender: &str,
        event_type: &str,
        content: Value,
        txn_id: Option<&str>,
    ) -> BicerinResult<String> {
        let member = bicerin_storage::rooms::get_room_member(&self.pool, room_id, sender)
            .await
            .map_err(|_| BicerinError::Forbidden)?;
        if member.membership != "join" {
            return Err(BicerinError::Forbidden);
        }

        let room = bicerin_storage::rooms::get_room(&self.pool, room_id)
            .await
            .map_err(|_| BicerinError::NotFound)?;

        let stream_id = bicerin_storage::events::get_next_stream_id(&self.pool)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;

        let event_id = format!("${}:{}", uuid::Uuid::new_v4().to_string().replace('-', ""), self.server_name);

        let event = EventRecord {
            event_id: event_id.clone(),
            room_id: room_id.to_string(),
            sender: sender.to_string(),
            stream_id,
            origin_server_ts: chrono::Utc::now().timestamp_millis(),
            event_type: event_type.to_string(),
            state_key: None,
            room_version: room.room_version,
            content,
            unsigned: None,
            redacts: None,
            depth: 0,
            auth_events: vec![],
            prev_events: vec![],
            created_at: chrono::Utc::now(),
        };

        bicerin_storage::events::insert_event(&self.pool, &event)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;

        crate::relations::index_relations(&self.pool, &event).await;

        tracing::debug!(event_id = %event_id, room_id = %room_id, event_type = %event_type, stream_id = stream_id, "event persisted");

        Ok(event_id)
    }

    pub async fn send_state_event(
        &self,
        room_id: &str,
        sender: &str,
        event_type: &str,
        state_key: &str,
        content: Value,
    ) -> BicerinResult<String> {
        let member = bicerin_storage::rooms::get_room_member(&self.pool, room_id, sender)
            .await
            .map_err(|_| BicerinError::Forbidden)?;
        if member.membership != "join" {
            return Err(BicerinError::Forbidden);
        }

        let room = bicerin_storage::rooms::get_room(&self.pool, room_id)
            .await
            .map_err(|_| BicerinError::NotFound)?;

        let stream_id = bicerin_storage::events::get_next_stream_id(&self.pool)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;

        let event_id = format!("${}:{}", uuid::Uuid::new_v4().to_string().replace('-', ""), self.server_name);

        let event = EventRecord {
            event_id: event_id.clone(),
            room_id: room_id.to_string(),
            sender: sender.to_string(),
            stream_id,
            origin_server_ts: chrono::Utc::now().timestamp_millis(),
            event_type: event_type.to_string(),
            state_key: Some(state_key.to_string()),
            room_version: room.room_version,
            content: content.clone(),
            unsigned: None,
            redacts: None,
            depth: 0,
            auth_events: vec![],
            prev_events: vec![],
            created_at: chrono::Utc::now(),
        };

        bicerin_storage::events::insert_event(&self.pool, &event)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;

        bicerin_storage::rooms::upsert_room_state(&self.pool, &bicerin_storage::rooms::RoomStateRecord {
            room_id: room_id.to_string(),
            event_type: event_type.to_string(),
            state_key: state_key.to_string(),
            event_id: event_id.clone(),
            content,
            sender: sender.to_string(),
            stream_id,
            updated_at: chrono::Utc::now(),
        }).await.map_err(|e| BicerinError::Internal(e.to_string()))?;

        Ok(event_id)
    }

    pub async fn get_room_messages(
        &self,
        room_id: &str,
        from: Option<i64>,
        to: Option<i64>,
        dir: &str,
        limit: i64,
    ) -> BicerinResult<(Vec<EventRecord>, Option<i64>)> {
        let events = bicerin_storage::events::get_events_in_room(
            &self.pool,
            room_id,
            from.unwrap_or(i64::MAX),
            limit,
            dir,
        ).await.map_err(|e| BicerinError::Internal(e.to_string()))?;

        let end_token = events.last().map(|e| e.stream_id);
        Ok((events, end_token))
    }
}
""",

    "bicerin-events/src/validation.rs": r"""use bicerin_error::{BicerinError, BicerinResult};
use serde_json::Value;

pub fn validate_event_type(event_type: &str) -> BicerinResult<()> {
    if event_type.is_empty() {
        return Err(BicerinError::BadRequest("event_type cannot be empty".to_string()));
    }
    if event_type.len() > 255 {
        return Err(BicerinError::BadRequest("event_type too long".to_string()));
    }
    Ok(())
}

pub fn validate_event_content(content: &Value) -> BicerinResult<()> {
    if !content.is_object() {
        return Err(BicerinError::BadRequest("event content must be a JSON object".to_string()));
    }
    Ok(())
}

pub fn validate_room_id(room_id: &str) -> BicerinResult<()> {
    if !room_id.starts_with('!') || !room_id.contains(':') {
        return Err(BicerinError::BadRequest("invalid room_id format".to_string()));
    }
    Ok(())
}

pub fn validate_user_id(user_id: &str) -> BicerinResult<()> {
    if !user_id.starts_with('@') || !user_id.contains(':') {
        return Err(BicerinError::BadRequest("invalid user_id format".to_string()));
    }
    Ok(())
}
""",

    "bicerin-events/src/relations.rs": r"""use bicerin_storage::events::{EventRecord, EventRelationRecord};

pub async fn index_relations(pool: &sqlx::PgPool, event: &EventRecord) {
    let relates_to = match event.content.get("m.relates_to") {
        Some(r) => r,
        None => return,
    };

    let rel_type = match relates_to.get("rel_type").and_then(|v| v.as_str()) {
        Some(r) => r.to_string(),
        None => return,
    };

    let parent_event_id = match relates_to.get("event_id").and_then(|v| v.as_str()) {
        Some(e) => e.to_string(),
        None => return,
    };

    let rel = EventRelationRecord {
        room_id: event.room_id.clone(),
        parent_event_id,
        child_event_id: event.event_id.clone(),
        rel_type,
    };

    if let Err(e) = bicerin_storage::events::insert_event_relation(pool, &rel).await {
        tracing::warn!(error = %e, "failed to index event relation");
    }
}
""",

    "bicerin-sync/Cargo.toml": r"""[package]
name = "bicerin-sync"
version = "0.1.0"
edition = "2021"

[dependencies]
tokio = { workspace = true, features = ["sync"] }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
thiserror = { workspace = true }
async-trait = { workspace = true }
futures = { workspace = true }

bicerin-types = { path = "../bicerin-types" }
bicerin-error = { path = "../bicerin-error" }
bicerin-storage = { path = "../bicerin-storage" }
""",

    "bicerin-sync/src/lib.rs": r"""pub mod service;
pub mod filter;
pub mod token;
pub mod subscriptions;

pub use service::SyncService;
""",

    "bicerin-sync/src/token.rs": r"""#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncToken(pub i64);

impl SyncToken {
    pub fn new(position: i64) -> Self { Self(position) }

    pub fn to_string(&self) -> String {
        format!("s{}", self.0)
    }

    pub fn parse(s: &str) -> Option<Self> {
        s.strip_prefix('s').and_then(|n| n.parse().ok()).map(SyncToken)
    }

    pub fn position(&self) -> i64 { self.0 }
}

impl std::fmt::Display for SyncToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "s{}", self.0)
    }
}
""",

    "bicerin-sync/src/filter.rs": r"""#[derive(Debug, Clone, serde::Deserialize, Default)]
pub struct SyncFilter {
    pub room: Option<RoomFilter>,
    pub event_fields: Option<Vec<String>>,
    pub event_format: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
pub struct RoomFilter {
    pub timeline: Option<EventFilter>,
    pub state: Option<StateFilter>,
    pub ephemeral: Option<EventFilter>,
    pub not_rooms: Option<Vec<String>>,
    pub rooms: Option<Vec<String>>,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
pub struct EventFilter {
    pub limit: Option<i64>,
    pub not_types: Option<Vec<String>>,
    pub types: Option<Vec<String>>,
    pub not_senders: Option<Vec<String>>,
    pub senders: Option<Vec<String>>,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
pub struct StateFilter {
    pub limit: Option<i64>,
    pub not_types: Option<Vec<String>>,
    pub types: Option<Vec<String>>,
    pub lazy_load_members: Option<bool>,
}
""",

    "bicerin-sync/src/subscriptions.rs": r"""use std::collections::HashMap;
use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub struct RoomUpdate {
    pub room_id: String,
    pub stream_id: i64,
}

pub struct SyncBus {
    sender: broadcast::Sender<RoomUpdate>,
}

impl SyncBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    pub fn notify(&self, room_id: String, stream_id: i64) {
        let _ = self.sender.send(RoomUpdate { room_id, stream_id });
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RoomUpdate> {
        self.sender.subscribe()
    }
}
""",

    "bicerin-sync/src/service.rs": r"""use crate::{
    filter::SyncFilter,
    subscriptions::{SyncBus, RoomUpdate},
    token::SyncToken,
};
use bicerin_error::{BicerinError, BicerinResult};
use bicerin_storage::events::EventRecord;
use bicerin_storage::rooms::RoomStateRecord;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::time::{timeout, Duration};

#[derive(Debug, Serialize)]
pub struct SyncResponse {
    pub next_batch: String,
    pub rooms: SyncRooms,
    pub device_lists: DeviceLists,
    pub to_device: ToDevice,
}

#[derive(Debug, Serialize, Default)]
pub struct SyncRooms {
    pub join: std::collections::HashMap<String, JoinedRoomSync>,
    pub invite: std::collections::HashMap<String, InvitedRoomSync>,
    pub leave: std::collections::HashMap<String, LeftRoomSync>,
}

#[derive(Debug, Serialize, Default)]
pub struct JoinedRoomSync {
    pub timeline: Timeline,
    pub state: State,
    pub ephemeral: Ephemeral,
    pub account_data: AccountData,
    pub summary: RoomSummary,
    pub unread_notifications: UnreadNotificationCounts,
}

#[derive(Debug, Serialize, Default)]
pub struct InvitedRoomSync {
    pub invite_state: InviteState,
}

#[derive(Debug, Serialize, Default)]
pub struct LeftRoomSync {
    pub timeline: Timeline,
    pub state: State,
}

#[derive(Debug, Serialize, Default)]
pub struct Timeline {
    pub events: Vec<serde_json::Value>,
    pub limited: bool,
    pub prev_batch: Option<String>,
}

#[derive(Debug, Serialize, Default)]
pub struct State {
    pub events: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize, Default)]
pub struct Ephemeral {
    pub events: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize, Default)]
pub struct AccountData {
    pub events: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize, Default)]
pub struct RoomSummary {
    #[serde(rename = "m.joined_member_count", skip_serializing_if = "Option::is_none")]
    pub joined_member_count: Option<u64>,
    #[serde(rename = "m.invited_member_count", skip_serializing_if = "Option::is_none")]
    pub invited_member_count: Option<u64>,
    #[serde(rename = "m.heroes", skip_serializing_if = "Option::is_none")]
    pub heroes: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Default)]
pub struct UnreadNotificationCounts {
    pub notification_count: u64,
    pub highlight_count: u64,
}

#[derive(Debug, Serialize, Default)]
pub struct InviteState {
    pub events: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize, Default)]
pub struct DeviceLists {
    pub changed: Vec<String>,
    pub left: Vec<String>,
}

#[derive(Debug, Serialize, Default)]
pub struct ToDevice {
    pub events: Vec<serde_json::Value>,
}

pub struct SyncService {
    pool: sqlx::PgPool,
    bus: Arc<SyncBus>,
    server_name: String,
}

impl SyncService {
    pub fn new(pool: sqlx::PgPool, bus: Arc<SyncBus>, server_name: String) -> Self {
        Self { pool, bus, server_name }
    }

    pub async fn sync(
        &self,
        user_id: &str,
        device_id: &str,
        since: Option<String>,
        timeout_ms: u64,
        filter: Option<SyncFilter>,
    ) -> BicerinResult<SyncResponse> {
        let since_position = since
            .as_deref()
            .and_then(SyncToken::parse)
            .map(|t| t.position())
            .unwrap_or(0);

        let joined_rooms = bicerin_storage::rooms::get_joined_rooms(&self.pool, user_id)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;

        let rooms_with_updates = bicerin_storage::sync::get_rooms_with_new_events(
            &self.pool,
            user_id,
            &joined_rooms,
            since_position,
        ).await.map_err(|e| BicerinError::Internal(e.to_string()))?;

        if rooms_with_updates.is_empty() && timeout_ms > 0 && since.is_some() {
            let mut rx = self.bus.subscribe();
            let wait = Duration::from_millis(timeout_ms.min(30_000));

            let _ = timeout(wait, async {
                loop {
                    match rx.recv().await {
                        Ok(update) if joined_rooms.contains(&update.room_id) => break,
                        Ok(_) => continue,
                        Err(_) => break,
                    }
                }
            }).await;
        }

        let current_position = bicerin_storage::sync::get_current_stream_position(&self.pool)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;

        let next_batch = SyncToken::new(current_position).to_string();

        let timeline_limit = filter.as_ref()
            .and_then(|f| f.room.as_ref())
            .and_then(|r| r.timeline.as_ref())
            .and_then(|t| t.limit)
            .unwrap_or(50);

        let mut join_map = std::collections::HashMap::new();

        for room_id in &joined_rooms {
            let is_initial = since.is_none();

            let events = bicerin_storage::events::get_events_in_room(
                &self.pool,
                room_id,
                if is_initial { i64::MAX } else { current_position },
                timeline_limit,
                "b",
            ).await.map_err(|e| BicerinError::Internal(e.to_string()))?;

            let mut timeline_events: Vec<_> = events.iter().rev()
                .filter(|e| e.stream_id > since_position)
                .map(|e| event_record_to_client_event(e))
                .collect();

            let limited = events.len() >= timeline_limit as usize;
            let prev_batch = events.first().map(|e| SyncToken::new(e.stream_id).to_string());

            let state_events = if is_initial {
                bicerin_storage::rooms::get_full_room_state(&self.pool, room_id)
                    .await
                    .map_err(|e| BicerinError::Internal(e.to_string()))?
                    .into_iter()
                    .map(|s| state_record_to_client_event(&s))
                    .collect()
            } else {
                vec![]
            };

            join_map.insert(room_id.clone(), JoinedRoomSync {
                timeline: Timeline {
                    events: timeline_events,
                    limited,
                    prev_batch,
                },
                state: State { events: state_events },
                ephemeral: Ephemeral::default(),
                account_data: AccountData::default(),
                summary: RoomSummary::default(),
                unread_notifications: UnreadNotificationCounts::default(),
            });
        }

        Ok(SyncResponse {
            next_batch,
            rooms: SyncRooms {
                join: join_map,
                invite: Default::default(),
                leave: Default::default(),
            },
            device_lists: DeviceLists::default(),
            to_device: ToDevice::default(),
        })
    }
}

fn event_record_to_client_event(event: &bicerin_storage::events::EventRecord) -> serde_json::Value {
    let mut obj = serde_json::json!({
        "event_id": event.event_id,
        "room_id": event.room_id,
        "sender": event.sender,
        "type": event.event_type,
        "origin_server_ts": event.origin_server_ts,
        "content": event.content,
    });
    if let Some(sk) = &event.state_key {
        obj["state_key"] = serde_json::Value::String(sk.clone());
    }
    if let Some(u) = &event.unsigned {
        obj["unsigned"] = u.clone();
    }
    obj
}

fn state_record_to_client_event(state: &bicerin_storage::rooms::RoomStateRecord) -> serde_json::Value {
    serde_json::json!({
        "type": state.event_type,
        "state_key": state.state_key,
        "content": state.content,
        "sender": state.sender,
        "event_id": state.event_id,
        "origin_server_ts": 0,
        "room_id": state.room_id,
    })
}
"""
}

for path, content in files.items():
    full_path = os.path.join(root_dir, path)
    os.makedirs(os.path.dirname(full_path), exist_ok=True)
    with open(full_path, "w", encoding="utf-8") as f:
        f.write(content)

print(f"Created {len(files)} files.")
