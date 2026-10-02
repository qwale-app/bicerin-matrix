use crate::{extract::AuthUser, state::AppState};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use bicerin_error::{BicerinError, BicerinResult};
use bicerin_storage::room_keys::{BackupSessionRecord, BackupVersionRecord};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap};

const BACKUP_ALGORITHM: &str = "m.megolm_backup.v1.curve25519-aes-sha2";

#[derive(Debug, Deserialize)]
pub struct CreateBackupBody {
    pub algorithm: String,
    pub auth_data: Value,
}

pub async fn get_current_version(
    user: AuthUser,
    State(state): State<AppState>,
) -> BicerinResult<Json<Value>> {
    let version = bicerin_storage::room_keys::get_current_version(&state.pool, &user.user_id)
        .await
        .map_err(internal)?
        .ok_or(BicerinError::NotFound)?;
    Ok(Json(version_info(&state, &version).await?))
}

pub async fn create_version(
    user: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CreateBackupBody>,
) -> BicerinResult<Json<Value>> {
    if body.algorithm != BACKUP_ALGORITHM || !body.auth_data.is_object() {
        return Err(BicerinError::BadRequest(
            "unsupported backup algorithm or invalid auth_data".into(),
        ));
    }
    let version = uuid::Uuid::new_v4().to_string();
    bicerin_storage::room_keys::create_version(
        &state.pool,
        &BackupVersionRecord {
            user_id: user.user_id,
            version: version.clone(),
            algorithm: body.algorithm,
            auth_data: body.auth_data,
            created_at: Utc::now(),
        },
    )
    .await
    .map_err(internal)?;
    Ok(Json(json!({"version": version})))
}

pub async fn get_version(
    user: AuthUser,
    State(state): State<AppState>,
    Path(version): Path<String>,
) -> BicerinResult<Json<Value>> {
    let version = bicerin_storage::room_keys::get_version(&state.pool, &user.user_id, &version)
        .await
        .map_err(internal)?
        .ok_or(BicerinError::NotFound)?;
    Ok(Json(version_info(&state, &version).await?))
}

#[derive(Debug, Deserialize)]
pub struct UpdateBackupVersionBody {
    pub algorithm: String,
    pub auth_data: Value,
    pub version: Option<String>,
}

pub async fn update_version(
    user: AuthUser,
    State(state): State<AppState>,
    Path(version): Path<String>,
    Json(body): Json<UpdateBackupVersionBody>,
) -> BicerinResult<Json<Value>> {
    if !body.auth_data.is_object() {
        return Err(BicerinError::BadRequest(
            "auth_data must be an object".into(),
        ));
    }
    let existing = bicerin_storage::room_keys::get_version(&state.pool, &user.user_id, &version)
        .await
        .map_err(internal)?
        .ok_or(BicerinError::NotFound)?;
    validate_backup_version_update(
        &version,
        &existing.algorithm,
        body.version.as_deref(),
        &body.algorithm,
    )?;
    if !bicerin_storage::room_keys::update_version(
        &state.pool,
        &user.user_id,
        &version,
        body.auth_data,
    )
    .await
    .map_err(internal)?
    {
        return Err(BicerinError::NotFound);
    }
    Ok(Json(json!({})))
}

fn validate_backup_version_update(
    path_version: &str,
    existing_algorithm: &str,
    requested_version: Option<&str>,
    requested_algorithm: &str,
) -> BicerinResult<()> {
    if requested_version.is_some_and(|requested| requested != path_version) {
        return Err(BicerinError::MatrixError {
            errcode: "M_INVALID_PARAM".into(),
            error: "Version in the body must match the path version".into(),
        });
    }
    if requested_algorithm != existing_algorithm {
        return Err(BicerinError::MatrixError {
            errcode: "M_INVALID_PARAM".into(),
            error: "Algorithm does not match the backup".into(),
        });
    }
    Ok(())
}

pub async fn delete_version(
    user: AuthUser,
    State(state): State<AppState>,
    Path(version): Path<String>,
) -> BicerinResult<Json<Value>> {
    if !bicerin_storage::room_keys::delete_version(&state.pool, &user.user_id, &version)
        .await
        .map_err(internal)?
    {
        return Err(BicerinError::NotFound);
    }
    Ok(Json(json!({})))
}

#[derive(Debug, Deserialize)]
pub struct BackupVersionQuery {
    pub version: Option<String>,
}

async fn required_version(
    state: &AppState,
    user_id: &str,
    version: Option<&str>,
    write: bool,
) -> BicerinResult<String> {
    let Some(version) = version else {
        return Err(BicerinError::BadRequest(
            "version query parameter is required".into(),
        ));
    };
    let current = bicerin_storage::room_keys::get_current_version(&state.pool, user_id)
        .await
        .map_err(internal)?
        .ok_or(BicerinError::NotFound)?;
    let exists = bicerin_storage::room_keys::get_version(&state.pool, user_id, version)
        .await
        .map_err(internal)?
        .is_some();
    if !exists {
        return Err(BicerinError::NotFound);
    }
    if write && current.version != version {
        return Err(BicerinError::WrongRoomKeysVersion {
            current_version: current.version,
        });
    }
    Ok(version.to_string())
}

#[derive(Debug, Deserialize)]
pub struct BackupRoomBody {
    pub sessions: HashMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct BackupRoomsBody {
    pub rooms: HashMap<String, BackupRoomBody>,
}

pub async fn put_keys(
    user: AuthUser,
    State(state): State<AppState>,
    Query(query): Query<BackupVersionQuery>,
    Json(body): Json<BackupRoomsBody>,
) -> BicerinResult<Json<Value>> {
    let version = required_version(&state, &user.user_id, query.version.as_deref(), true).await?;
    let mut updated = 0u64;
    for (room_id, room) in body.rooms {
        for (session_id, data) in room.sessions {
            validate_session_data(&data)?;
            updated += write_session(&state, &user.user_id, &version, &room_id, &session_id, data)
                .await? as u64;
        }
    }
    backup_update_response(&state, &user.user_id, &version, updated).await
}

pub async fn put_room_keys(
    user: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Query(query): Query<BackupVersionQuery>,
    Json(body): Json<BackupRoomBody>,
) -> BicerinResult<Json<Value>> {
    let version = required_version(&state, &user.user_id, query.version.as_deref(), true).await?;
    let mut updated = 0u64;
    for (session_id, data) in body.sessions {
        validate_session_data(&data)?;
        updated += write_session(&state, &user.user_id, &version, &room_id, &session_id, data)
            .await? as u64;
    }
    backup_update_response(&state, &user.user_id, &version, updated).await
}

pub async fn put_room_session(
    user: AuthUser,
    State(state): State<AppState>,
    Path((room_id, session_id)): Path<(String, String)>,
    Query(query): Query<BackupVersionQuery>,
    Json(data): Json<Value>,
) -> BicerinResult<Json<Value>> {
    let version = required_version(&state, &user.user_id, query.version.as_deref(), true).await?;
    validate_session_data(&data)?;
    let updated =
        write_session(&state, &user.user_id, &version, &room_id, &session_id, data).await? as u64;
    backup_update_response(&state, &user.user_id, &version, updated).await
}

fn validate_session_data(data: &Value) -> BicerinResult<()> {
    if !data
        .get("first_message_index")
        .and_then(Value::as_i64)
        .is_some_and(|value| value >= 0)
        || !data
            .get("forwarded_count")
            .and_then(Value::as_i64)
            .is_some_and(|value| value >= 0)
        || !data.get("is_verified").is_some_and(Value::is_boolean)
        || !data.get("session_data").is_some_and(Value::is_object)
    {
        return Err(BicerinError::BadRequest("backup session requires first_message_index, forwarded_count, is_verified, and session_data".into()));
    }
    Ok(())
}

async fn write_session(
    state: &AppState,
    user_id: &str,
    version: &str,
    room_id: &str,
    session_id: &str,
    data: Value,
) -> BicerinResult<bool> {
    Ok(bicerin_storage::room_keys::upsert_session(
        &state.pool,
        &BackupSessionRecord {
            user_id: user_id.to_string(),
            version: version.to_string(),
            room_id: room_id.to_string(),
            session_id: session_id.to_string(),
            data,
            updated_at: Utc::now(),
        },
    )
    .await
    .map_err(internal)?)
}

pub async fn get_keys(
    user: AuthUser,
    State(state): State<AppState>,
    Query(query): Query<BackupVersionQuery>,
) -> BicerinResult<Json<Value>> {
    let version = required_version(&state, &user.user_id, query.version.as_deref(), false).await?;
    let sessions =
        bicerin_storage::room_keys::get_sessions(&state.pool, &user.user_id, &version, None, None)
            .await
            .map_err(internal)?;
    let mut rooms: HashMap<String, Map<String, Value>> = HashMap::new();
    for session in sessions {
        rooms
            .entry(session.room_id)
            .or_default()
            .entry(session.session_id)
            .or_insert(session.data);
    }
    let rooms: HashMap<String, Value> = rooms
        .into_iter()
        .map(|(room_id, sessions)| (room_id, json!({"sessions": sessions})))
        .collect();
    let etag = etag_for_rooms(&rooms);
    Ok(Json(json!({"rooms": rooms, "etag": etag})))
}

pub async fn get_room_keys(
    user: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Query(query): Query<BackupVersionQuery>,
) -> BicerinResult<Json<Value>> {
    let version = required_version(&state, &user.user_id, query.version.as_deref(), false).await?;
    let sessions = bicerin_storage::room_keys::get_sessions(
        &state.pool,
        &user.user_id,
        &version,
        Some(&room_id),
        None,
    )
    .await
    .map_err(internal)?;
    let sessions: Map<String, Value> = sessions
        .into_iter()
        .map(|session| (session.session_id, session.data))
        .collect();
    Ok(Json(json!({"sessions": sessions})))
}

pub async fn get_room_session(
    user: AuthUser,
    State(state): State<AppState>,
    Path((room_id, session_id)): Path<(String, String)>,
    Query(query): Query<BackupVersionQuery>,
) -> BicerinResult<Json<Value>> {
    let version = required_version(&state, &user.user_id, query.version.as_deref(), false).await?;
    let session = bicerin_storage::room_keys::get_sessions(
        &state.pool,
        &user.user_id,
        &version,
        Some(&room_id),
        Some(&session_id),
    )
    .await
    .map_err(internal)?
    .into_iter()
    .next()
    .ok_or(BicerinError::NotFound)?;
    Ok(Json(session.data))
}

pub async fn delete_keys(
    user: AuthUser,
    State(state): State<AppState>,
    Query(query): Query<BackupVersionQuery>,
) -> BicerinResult<Json<Value>> {
    let version = required_version(&state, &user.user_id, query.version.as_deref(), false).await?;
    let deleted = bicerin_storage::room_keys::delete_sessions(
        &state.pool,
        &user.user_id,
        &version,
        None,
        None,
    )
    .await
    .map_err(internal)?;
    backup_update_response(&state, &user.user_id, &version, deleted).await
}

pub async fn delete_room_keys(
    user: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Query(query): Query<BackupVersionQuery>,
) -> BicerinResult<Json<Value>> {
    let version = required_version(&state, &user.user_id, query.version.as_deref(), false).await?;
    let deleted = bicerin_storage::room_keys::delete_sessions(
        &state.pool,
        &user.user_id,
        &version,
        Some(&room_id),
        None,
    )
    .await
    .map_err(internal)?;
    backup_update_response(&state, &user.user_id, &version, deleted).await
}

pub async fn delete_room_session(
    user: AuthUser,
    State(state): State<AppState>,
    Path((room_id, session_id)): Path<(String, String)>,
    Query(query): Query<BackupVersionQuery>,
) -> BicerinResult<Json<Value>> {
    let version = required_version(&state, &user.user_id, query.version.as_deref(), false).await?;
    let deleted = bicerin_storage::room_keys::delete_sessions(
        &state.pool,
        &user.user_id,
        &version,
        Some(&room_id),
        Some(&session_id),
    )
    .await
    .map_err(internal)?;
    backup_update_response(&state, &user.user_id, &version, deleted).await
}

async fn backup_update_response(
    state: &AppState,
    user_id: &str,
    version: &str,
    count: u64,
) -> BicerinResult<Json<Value>> {
    let sessions =
        bicerin_storage::room_keys::get_sessions(&state.pool, user_id, version, None, None)
            .await
            .map_err(internal)?;
    let rooms = sessions
        .into_iter()
        .fold(
            HashMap::<String, Map<String, Value>>::new(),
            |mut rooms, session| {
                rooms
                    .entry(session.room_id)
                    .or_default()
                    .insert(session.session_id, session.data);
                rooms
            },
        )
        .into_iter()
        .map(|(room_id, sessions)| (room_id, json!({"sessions": sessions})))
        .collect::<HashMap<_, _>>();
    let total_count = bicerin_storage::room_keys::count_sessions(&state.pool, user_id, version)
        .await
        .map_err(internal)?;
    let _ = count;
    Ok(Json(
        json!({"count": total_count, "etag": etag_for_rooms(&rooms)}),
    ))
}

async fn version_info(state: &AppState, version: &BackupVersionRecord) -> BicerinResult<Value> {
    let sessions = bicerin_storage::room_keys::get_sessions(
        &state.pool,
        &version.user_id,
        &version.version,
        None,
        None,
    )
    .await
    .map_err(internal)?;
    let rooms = sessions
        .into_iter()
        .fold(
            HashMap::<String, Map<String, Value>>::new(),
            |mut rooms, session| {
                rooms
                    .entry(session.room_id)
                    .or_default()
                    .insert(session.session_id, session.data);
                rooms
            },
        )
        .into_iter()
        .map(|(room_id, sessions)| (room_id, json!({"sessions": sessions})))
        .collect::<HashMap<_, _>>();
    let count =
        bicerin_storage::room_keys::count_sessions(&state.pool, &version.user_id, &version.version)
            .await
            .map_err(internal)?;
    Ok(
        json!({"algorithm": version.algorithm, "auth_data": version.auth_data, "version": version.version, "count": count, "etag": etag_for_rooms(&rooms)}),
    )
}

fn etag_for_rooms(rooms: &HashMap<String, Value>) -> String {
    let canonical = rooms
        .iter()
        .map(|(room_id, value)| (room_id, value))
        .collect::<BTreeMap<_, _>>();
    bicerin_types::auth::hash_access_token(&serde_json::to_string(&canonical).unwrap_or_default())
        .to_string()
}

fn internal(error: impl std::fmt::Display) -> BicerinError {
    BicerinError::Internal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{etag_for_rooms, validate_backup_version_update, validate_session_data};
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn room_key_backup_accepts_opaque_encrypted_session_payloads() {
        let data = json!({"first_message_index":0,"forwarded_count":0,"is_verified":true,"session_data":{"ciphertext":"encrypted"}});
        assert!(validate_session_data(&data).is_ok());
    }

    #[test]
    fn room_key_backup_rejects_incomplete_session_metadata() {
        assert!(validate_session_data(&json!({"session_data":{}})).is_err());
    }

    #[test]
    fn room_key_backup_rejects_negative_or_fractional_indices() {
        assert!(validate_session_data(&json!({"first_message_index":-1,"forwarded_count":0,"is_verified":true,"session_data":{}})).is_err());
        assert!(validate_session_data(&json!({"first_message_index":1.5,"forwarded_count":0,"is_verified":true,"session_data":{}})).is_err());
        assert!(validate_session_data(&json!({"first_message_index":0,"forwarded_count":-1,"is_verified":true,"session_data":{}})).is_err());
    }

    #[test]
    fn room_key_backup_version_updates_must_preserve_version_and_algorithm() {
        let algorithm = "m.megolm_backup.v1.curve25519-aes-sha2";
        assert!(validate_backup_version_update("v1", algorithm, Some("v1"), algorithm).is_ok());
        assert!(validate_backup_version_update("v1", algorithm, None, algorithm).is_ok());
        assert!(validate_backup_version_update("v1", algorithm, Some("v2"), algorithm).is_err());
        assert!(validate_backup_version_update("v1", algorithm, None, "unsupported").is_err());
    }

    #[test]
    fn backup_etags_are_stable_for_identical_room_data_and_change_with_content() {
        let first = HashMap::from([(
            "!room:example.org".to_string(),
            json!({"sessions":{"S":"a"}}),
        )]);
        let same = first.clone();
        let changed = HashMap::from([(
            "!room:example.org".to_string(),
            json!({"sessions":{"S":"b"}}),
        )]);
        assert_eq!(etag_for_rooms(&first), etag_for_rooms(&same));
        assert_ne!(etag_for_rooms(&first), etag_for_rooms(&changed));
    }
}
