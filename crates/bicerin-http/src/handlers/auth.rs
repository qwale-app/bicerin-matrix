use crate::{extract::AuthUser, state::AppState, util::*};
use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use bicerin_error::{BicerinError, BicerinResult};
use serde::Deserialize;
use serde_json::{json, Value};

pub async fn get_versions() -> Json<Value> {
    Json(json!({
        "versions": [
            "r0.6.1", "v1.1", "v1.2", "v1.3", "v1.4", "v1.5",
            "v1.6", "v1.7", "v1.8", "v1.9", "v1.10", "v1.11"
        ],
        "unstable_features": {
            "com.bicerin.unfederated": true,
            "com.bicerin.e2ee": true
        }
    }))
}

pub async fn well_known_client(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "m.homeserver": { "base_url": state.public_url }
    }))
}

pub async fn get_login_flows() -> Json<Value> {
    Json(
        json!({ "flows": [ { "type": "m.login.password" }, { "type": "m.login.application_service" } ] }),
    )
}

#[derive(Debug, Deserialize)]
pub struct Identifier {
    #[serde(rename = "type")]
    #[allow(dead_code)]
    pub id_type: Option<String>,
    pub user: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    #[serde(rename = "type")]
    pub login_type: String,
    pub identifier: Option<Identifier>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub device_id: Option<String>,
    pub initial_device_display_name: Option<String>,
}

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> BicerinResult<Json<Value>> {
    if body.login_type == "m.login.application_service" {
        return login_as_appservice(state, headers, body).await;
    }

    if body.login_type != "m.login.password" {
        return Err(BicerinError::MatrixError {
            errcode: "M_UNKNOWN".to_string(),
            error: "Bad login type.".to_string(),
        });
    }

    let username = body
        .identifier
        .and_then(|i| i.user)
        .or(body.user)
        .ok_or_else(|| BicerinError::BadRequest("missing user identifier".to_string()))?;
    let password = body
        .password
        .ok_or_else(|| BicerinError::BadRequest("missing password".to_string()))?;

    let user_id = normalize_user_id(&username, &state.server_name);
    let user = bicerin_storage::users::get_user(&state.pool, &user_id)
        .await
        .map_err(|_| BicerinError::Forbidden)?;
    if user.is_deactivated {
        return Err(BicerinError::MatrixError {
            errcode: "M_USER_DEACTIVATED".to_string(),
            error: "This account has been deactivated".to_string(),
        });
    }
    let hash = user
        .password_hash
        .as_deref()
        .ok_or(BicerinError::Forbidden)?;
    if !bicerin_auth::password::verify_password(&password, hash) {
        return Err(BicerinError::Forbidden);
    }

    let (access_token, device_id) = issue_session(
        &state,
        &user_id,
        body.device_id,
        body.initial_device_display_name,
    )
    .await?;

    Ok(Json(json!({
        "access_token": access_token,
        "device_id": device_id,
        "user_id": user_id,
    })))
}

/// `m.login.application_service`: the request must be authenticated with the
/// appservice's own `as_token` (not a normal user access token). The target
/// user (bot user by default, or `identifier.user`/`user` within the
/// appservice's namespace) is auto-created if it doesn't exist yet.
async fn login_as_appservice(
    state: AppState,
    headers: HeaderMap,
    body: LoginRequest,
) -> BicerinResult<Json<Value>> {
    let as_token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| BicerinError::MatrixError {
            errcode: "M_MISSING_TOKEN".to_string(),
            error: "Missing appservice access token".to_string(),
        })?;

    let appservice = bicerin_storage::appservice::get_appservice_by_as_token(&state.pool, as_token)
        .await
        .map_err(|_| BicerinError::Forbidden)?;

    let bot_user_id = bicerin_storage::appservice::bot_user_id(&appservice, &state.server_name);
    let user_id = match body.identifier.and_then(|i| i.user).or(body.user) {
        Some(username) => normalize_user_id(&username, &state.server_name),
        None => bot_user_id.clone(),
    };

    if !bicerin_storage::appservice::owns_user(&appservice, &state.server_name, &user_id) {
        return Err(BicerinError::MatrixError {
            errcode: "M_EXCLUSIVE".to_string(),
            error: format!("Appservice {} does not own user {}", appservice.id, user_id),
        });
    }

    if bicerin_storage::users::get_user(&state.pool, &user_id)
        .await
        .is_err()
    {
        let localpart = user_id
            .strip_prefix('@')
            .and_then(|rest| rest.split(':').next())
            .unwrap_or(&user_id)
            .to_string();
        bicerin_storage::users::create_user(
            &state.pool,
            &bicerin_storage::users::UserRecord {
                user_id: user_id.clone(),
                localpart,
                password_hash: None,
                display_name: None,
                avatar_url: None,
                is_guest: false,
                is_deactivated: false,
                created_at: chrono::Utc::now(),
            },
        )
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    }

    let (access_token, device_id) = issue_session(
        &state,
        &user_id,
        body.device_id,
        body.initial_device_display_name,
    )
    .await?;

    Ok(Json(json!({
        "access_token": access_token,
        "device_id": device_id,
        "user_id": user_id,
    })))
}

pub async fn logout(user: AuthUser, State(state): State<AppState>) -> BicerinResult<Json<Value>> {
    let hash = bicerin_types::auth::hash_access_token(&user.access_token).to_string();
    bicerin_storage::users::delete_access_token(&state.pool, &hash)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    state.auth.invalidate_token(&user.access_token).await;
    Ok(Json(json!({})))
}

pub async fn logout_all(
    user: AuthUser,
    State(state): State<AppState>,
) -> BicerinResult<Json<Value>> {
    bicerin_storage::users::delete_access_tokens_for_user(&state.pool, &user.user_id)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    state.auth.invalidate_token(&user.access_token).await;
    Ok(Json(json!({})))
}

#[derive(Debug, Deserialize)]
pub struct AuthData {
    #[serde(rename = "type")]
    pub auth_type: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct RegisterRequest {
    #[serde(rename = "type")]
    pub login_type: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub device_id: Option<String>,
    pub initial_device_display_name: Option<String>,
    #[serde(default)]
    pub inhibit_login: bool,
    pub auth: Option<AuthData>,
}

#[derive(Debug, Deserialize, Default)]
pub struct RegisterQuery {
    pub kind: Option<String>,
}

/// Registration implements a single-stage `m.login.dummy` User-Interactive
/// Authentication flow. Bicerin does not implement recaptcha/email/msisdn/terms
/// stages; see BICERIN_MATRIX_API.md for the full compatibility matrix.
pub async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<RegisterQuery>,
    Json(body): Json<RegisterRequest>,
) -> BicerinResult<Json<Value>> {
    if body.login_type.as_deref() == Some("m.login.application_service") {
        return register_as_appservice(state, headers, body).await;
    }

    if query.kind.as_deref() == Some("guest") {
        if !state.guest_access_enabled {
            return Err(BicerinError::MatrixError {
                errcode: "M_FORBIDDEN".to_string(),
                error: "Guest access is disabled".to_string(),
            });
        }
        let localpart = generate_localpart();
        let user_id = format!("@{}:{}", localpart, state.server_name);
        bicerin_storage::users::create_user(
            &state.pool,
            &bicerin_storage::users::UserRecord {
                user_id: user_id.clone(),
                localpart,
                password_hash: None,
                display_name: None,
                avatar_url: None,
                is_guest: true,
                is_deactivated: false,
                created_at: chrono::Utc::now(),
            },
        )
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;

        if body.inhibit_login {
            return Ok(Json(json!({ "user_id": user_id })));
        }
        let (access_token, device_id) = issue_session(
            &state,
            &user_id,
            body.device_id,
            body.initial_device_display_name,
        )
        .await?;
        return Ok(Json(json!({
            "access_token": access_token,
            "device_id": device_id,
            "user_id": user_id,
        })));
    }

    if !state.registration_enabled {
        return Err(BicerinError::MatrixError {
            errcode: "M_FORBIDDEN".to_string(),
            error: "Registration is disabled".to_string(),
        });
    }

    let completed_dummy = body
        .auth
        .as_ref()
        .map(|a| a.auth_type.as_deref() == Some("m.login.dummy"))
        .unwrap_or(false);

    if !completed_dummy {
        let session = uuid::Uuid::new_v4().simple().to_string();
        return Err(BicerinError::UiaRequired(json!({
            "flows": [ { "stages": [ "m.login.dummy" ] } ],
            "params": {},
            "session": session,
        })));
    }

    let localpart = match body.username {
        Some(u) => u,
        None => generate_localpart(),
    };
    validate_localpart(&localpart)?;

    let password = body
        .password
        .ok_or_else(|| BicerinError::BadRequest("missing password".to_string()))?;

    let user_id = format!("@{}:{}", localpart, state.server_name);
    if bicerin_storage::users::get_user(&state.pool, &user_id)
        .await
        .is_ok()
    {
        return Err(BicerinError::MatrixError {
            errcode: "M_USER_IN_USE".to_string(),
            error: "Desired user ID is already taken.".to_string(),
        });
    }

    let password_hash = bicerin_auth::password::hash_password(&password)
        .map_err(|e| BicerinError::Internal(e.to_string()))?;

    bicerin_storage::users::create_user(
        &state.pool,
        &bicerin_storage::users::UserRecord {
            user_id: user_id.clone(),
            localpart,
            password_hash: Some(password_hash),
            display_name: None,
            avatar_url: None,
            is_guest: false,
            is_deactivated: false,
            created_at: chrono::Utc::now(),
        },
    )
    .await
    .map_err(|e| BicerinError::Internal(e.to_string()))?;

    if body.inhibit_login {
        return Ok(Json(json!({ "user_id": user_id })));
    }

    let (access_token, device_id) = issue_session(
        &state,
        &user_id,
        body.device_id,
        body.initial_device_display_name,
    )
    .await?;

    Ok(Json(json!({
        "access_token": access_token,
        "device_id": device_id,
        "user_id": user_id,
    })))
}

/// `m.login.application_service`-authenticated registration: the standard
/// mechanism mautrix/matrix-appservice-bridge use to provision ghost users
/// (`Intent.ensure_registered`/`IntentAPI.ensure_registered`). Authenticated
/// via the appservice's own `as_token`, not UIA — bypasses
/// `registration_enabled` since this isn't public signup. Idempotent: an
/// already-registered ghost still succeeds instead of `M_USER_IN_USE`, since
/// bridges call this on every startup.
async fn register_as_appservice(
    state: AppState,
    headers: HeaderMap,
    body: RegisterRequest,
) -> BicerinResult<Json<Value>> {
    let as_token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| BicerinError::MatrixError {
            errcode: "M_MISSING_TOKEN".to_string(),
            error: "Missing appservice access token".to_string(),
        })?;

    let appservice = bicerin_storage::appservice::get_appservice_by_as_token(&state.pool, as_token)
        .await
        .map_err(|_| BicerinError::Forbidden)?;

    let localpart = body
        .username
        .ok_or_else(|| BicerinError::BadRequest("missing username".to_string()))?;
    validate_localpart(&localpart)?;
    let user_id = format!("@{}:{}", localpart, state.server_name);

    if !bicerin_storage::appservice::owns_user(&appservice, &state.server_name, &user_id) {
        return Err(BicerinError::MatrixError {
            errcode: "M_EXCLUSIVE".to_string(),
            error: format!("Appservice {} does not own user {}", appservice.id, user_id),
        });
    }

    if bicerin_storage::users::get_user(&state.pool, &user_id)
        .await
        .is_err()
    {
        bicerin_storage::users::create_user(
            &state.pool,
            &bicerin_storage::users::UserRecord {
                user_id: user_id.clone(),
                localpart,
                password_hash: None,
                display_name: None,
                avatar_url: None,
                is_guest: false,
                is_deactivated: false,
                created_at: chrono::Utc::now(),
            },
        )
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    }

    if body.inhibit_login {
        return Ok(Json(json!({ "user_id": user_id })));
    }

    let (access_token, device_id) = issue_session(
        &state,
        &user_id,
        body.device_id,
        body.initial_device_display_name,
    )
    .await?;

    Ok(Json(json!({
        "access_token": access_token,
        "device_id": device_id,
        "user_id": user_id,
    })))
}

#[derive(Debug, Deserialize)]
pub struct AvailableQuery {
    pub username: String,
}

pub async fn register_available(
    State(state): State<AppState>,
    Query(query): Query<AvailableQuery>,
) -> BicerinResult<Json<Value>> {
    validate_localpart(&query.username)?;
    let user_id = format!("@{}:{}", query.username, state.server_name);
    if bicerin_storage::users::get_user(&state.pool, &user_id)
        .await
        .is_ok()
    {
        return Err(BicerinError::MatrixError {
            errcode: "M_USER_IN_USE".to_string(),
            error: "Desired user ID is already taken.".to_string(),
        });
    }
    Ok(Json(json!({ "available": true })))
}

pub async fn whoami(user: AuthUser, State(state): State<AppState>) -> BicerinResult<Json<Value>> {
    let record = bicerin_storage::users::get_user(&state.pool, &user.user_id)
        .await
        .map_err(|_| BicerinError::Unauthorized)?;
    Ok(Json(json!({
        "user_id": user.user_id,
        "device_id": user.device_id,
        "is_guest": record.is_guest,
    })))
}

async fn issue_session(
    state: &AppState,
    user_id: &str,
    device_id: Option<String>,
    display_name: Option<String>,
) -> BicerinResult<(String, String)> {
    let device_id = device_id.unwrap_or_else(bicerin_auth::token::generate_device_id);

    bicerin_storage::users::create_device(
        &state.pool,
        &bicerin_storage::users::DeviceRecord {
            device_id: device_id.clone(),
            user_id: user_id.to_string(),
            display_name,
            last_seen_ip: None,
            last_seen_ts: Some(chrono::Utc::now()),
            created_at: chrono::Utc::now(),
        },
    )
    .await
    .map_err(|e| BicerinError::Internal(e.to_string()))?;

    let access_token = bicerin_auth::token::generate_access_token();
    let token_hash = bicerin_types::auth::hash_access_token(&access_token).to_string();

    bicerin_storage::users::create_access_token(
        &state.pool,
        &bicerin_storage::users::AccessTokenRecord {
            token_hash,
            user_id: user_id.to_string(),
            device_id: device_id.clone(),
            created_at: chrono::Utc::now(),
            expires_at: None,
            last_used_at: None,
        },
    )
    .await
    .map_err(|e| BicerinError::Internal(e.to_string()))?;

    Ok((access_token, device_id))
}

// --- Profile -----------------------------------------------------------

pub async fn get_profile(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> BicerinResult<Json<Value>> {
    let record = bicerin_storage::users::get_user(&state.pool, &user_id)
        .await
        .map_err(|_| BicerinError::NotFound)?;
    Ok(Json(
        json!({ "displayname": record.display_name, "avatar_url": record.avatar_url }),
    ))
}

pub async fn get_display_name(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> BicerinResult<Json<Value>> {
    let record = bicerin_storage::users::get_user(&state.pool, &user_id)
        .await
        .map_err(|_| BicerinError::NotFound)?;
    Ok(Json(json!({ "displayname": record.display_name })))
}

#[derive(Debug, Deserialize, Default)]
pub struct DisplayNameBody {
    pub displayname: Option<String>,
}

pub async fn set_display_name(
    user: AuthUser,
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    Json(body): Json<DisplayNameBody>,
) -> BicerinResult<Json<Value>> {
    if user.user_id != user_id {
        return Err(BicerinError::Forbidden);
    }
    bicerin_storage::users::update_display_name(&state.pool, &user_id, body.displayname.as_deref())
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    Ok(Json(json!({})))
}

pub async fn get_avatar_url(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> BicerinResult<Json<Value>> {
    let record = bicerin_storage::users::get_user(&state.pool, &user_id)
        .await
        .map_err(|_| BicerinError::NotFound)?;
    Ok(Json(json!({ "avatar_url": record.avatar_url })))
}

#[derive(Debug, Deserialize, Default)]
pub struct AvatarUrlBody {
    pub avatar_url: Option<String>,
}

pub async fn set_avatar_url(
    user: AuthUser,
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    Json(body): Json<AvatarUrlBody>,
) -> BicerinResult<Json<Value>> {
    if user.user_id != user_id {
        return Err(BicerinError::Forbidden);
    }
    bicerin_storage::users::update_avatar_url(&state.pool, &user_id, body.avatar_url.as_deref())
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    Ok(Json(json!({})))
}

// --- Devices -------------------------------------------------------------

pub async fn list_devices(
    user: AuthUser,
    State(state): State<AppState>,
) -> BicerinResult<Json<Value>> {
    let devices = bicerin_storage::users::list_devices(&state.pool, &user.user_id)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    let devices: Vec<Value> = devices.into_iter().map(device_to_json).collect();
    Ok(Json(json!({ "devices": devices })))
}

pub async fn get_device(
    user: AuthUser,
    State(state): State<AppState>,
    Path(device_id): Path<String>,
) -> BicerinResult<Json<Value>> {
    let device = bicerin_storage::users::get_device(&state.pool, &user.user_id, &device_id)
        .await
        .map_err(|_| BicerinError::NotFound)?;
    Ok(Json(device_to_json(device)))
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateDeviceBody {
    pub display_name: Option<String>,
}

pub async fn update_device(
    user: AuthUser,
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    Json(body): Json<UpdateDeviceBody>,
) -> BicerinResult<Json<Value>> {
    if let Some(name) = body.display_name {
        bicerin_storage::users::update_device_display_name(
            &state.pool,
            &user.user_id,
            &device_id,
            &name,
        )
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    }
    Ok(Json(json!({})))
}

pub async fn delete_device(
    user: AuthUser,
    State(state): State<AppState>,
    Path(device_id): Path<String>,
) -> BicerinResult<Json<Value>> {
    let had_identity_keys = match bicerin_storage::crypto::get_device_keys(
        &state.pool,
        &user.user_id,
        &device_id,
    )
    .await
    {
        Ok(_) => true,
        Err(bicerin_storage::db::StorageError::NotFound) => false,
        Err(error) => return Err(BicerinError::Internal(error.to_string())),
    };
    bicerin_storage::crypto::delete_device_crypto_material(&state.pool, &user.user_id, &device_id)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    if had_identity_keys {
        let stream_id = bicerin_storage::events::get_next_stream_id(&state.pool)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;
        bicerin_storage::cross_signing::record_device_change(
            &state.pool,
            &user.user_id,
            stream_id,
            "changed",
        )
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
        state.sync_bus.notify_all(stream_id);
    }
    bicerin_storage::users::delete_access_tokens_for_device(&state.pool, &user.user_id, &device_id)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    bicerin_storage::users::delete_device(&state.pool, &user.user_id, &device_id)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    Ok(Json(json!({})))
}

fn device_to_json(device: bicerin_storage::users::DeviceRecord) -> Value {
    json!({
        "device_id": device.device_id,
        "display_name": device.display_name,
        "last_seen_ip": device.last_seen_ip,
        "last_seen_ts": device.last_seen_ts.map(|t| t.timestamp_millis()),
    })
}
