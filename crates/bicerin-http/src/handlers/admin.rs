use crate::{extract::AdminAuth, state::AppState};
use axum::{
    extract::{Path, State},
    Json,
};
use bicerin_error::{BicerinError, BicerinResult};
use serde::Deserialize;
use serde_json::{json, Value};

pub async fn get_stats(_admin: AdminAuth, State(state): State<AppState>) -> BicerinResult<Json<Value>> {
    let users = bicerin_storage::users::count_users(&state.pool)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    let rooms = bicerin_storage::rooms::count_rooms(&state.pool)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    Ok(Json(json!({ "users": users, "rooms": rooms })))
}

pub async fn list_users(_admin: AdminAuth, State(state): State<AppState>) -> BicerinResult<Json<Value>> {
    let users = bicerin_storage::users::list_users(&state.pool, 500)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    let chunk: Vec<Value> = users
        .into_iter()
        .map(|user| {
            json!({
                "user_id": user.user_id,
                "is_guest": user.is_guest,
                "is_deactivated": user.is_deactivated,
                "created_at": user.created_at,
            })
        })
        .collect();
    Ok(Json(json!({ "users": chunk })))
}

pub async fn deactivate_user(
    _admin: AdminAuth,
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> BicerinResult<Json<Value>> {
    bicerin_storage::users::set_user_deactivated(&state.pool, &user_id, true)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    bicerin_storage::users::delete_access_tokens_for_user(&state.pool, &user_id)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    Ok(Json(json!({})))
}

const NONCE_TTL: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// Issues a single-use nonce for shared-secret registration, mirroring
/// Synapse's admin registration flow. Requires `matrix.registration_shared_secret`
/// to be configured; otherwise the whole mechanism is disabled.
pub async fn registration_nonce(State(state): State<AppState>) -> BicerinResult<Json<Value>> {
    if state.registration_shared_secret.is_none() {
        return Err(BicerinError::NotFound);
    }
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let now = std::time::Instant::now();
    let mut nonces = state
        .registration_nonces
        .lock()
        .expect("registration nonce lock poisoned");
    nonces.retain(|_, issued_at| now.duration_since(*issued_at) < NONCE_TTL);
    nonces.insert(nonce.clone(), now);
    Ok(Json(json!({ "nonce": nonce })))
}

#[derive(Debug, Deserialize)]
pub struct SharedSecretRegisterBody {
    pub nonce: String,
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub admin: bool,
    pub mac: String,
}

/// Shared-secret registration (`/_bicerin/admin/register`): a Synapse-style
/// out-of-band registration mechanism for provisioning accounts (e.g. from a
/// deployment script) using `matrix.registration_shared_secret` instead of
/// UIA. The `mac` is `HMAC-SHA256(shared_secret, nonce\0username\0password\0
/// (admin|notadmin))`, hex-encoded. Not gated by `matrix.registration_enabled`
/// since it's an operator mechanism, not public signup.
pub async fn shared_secret_register(
    State(state): State<AppState>,
    Json(body): Json<SharedSecretRegisterBody>,
) -> BicerinResult<Json<Value>> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let Some(secret) = state.registration_shared_secret.as_deref() else {
        return Err(BicerinError::NotFound);
    };

    {
        let mut nonces = state
            .registration_nonces
            .lock()
            .expect("registration nonce lock poisoned");
        if nonces.remove(&body.nonce).is_none() {
            return Err(BicerinError::BadRequest("unknown or expired nonce".to_string()));
        }
    }

    let admin_flag = if body.admin { "admin" } else { "notadmin" };
    let message = format!(
        "{}\0{}\0{}\0{}",
        body.nonce, body.username, body.password, admin_flag
    );
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    mac.update(message.as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());
    if expected.as_bytes() != body.mac.to_lowercase().as_bytes() {
        return Err(BicerinError::Forbidden);
    }

    crate::util::validate_localpart(&body.username)?;
    let user_id = format!("@{}:{}", body.username, state.server_name);
    if bicerin_storage::users::get_user(&state.pool, &user_id)
        .await
        .is_ok()
    {
        return Err(BicerinError::MatrixError {
            errcode: "M_USER_IN_USE".to_string(),
            error: "Desired user ID is already taken.".to_string(),
        });
    }
    let password_hash = bicerin_auth::password::hash_password(&body.password)
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    bicerin_storage::users::create_user(
        &state.pool,
        &bicerin_storage::users::UserRecord {
            user_id: user_id.clone(),
            localpart: body.username,
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

    Ok(Json(json!({ "user_id": user_id })))
}
