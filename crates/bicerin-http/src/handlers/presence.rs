use crate::{extract::AuthUser, state::AppState};
use axum::{extract::State, Json};
use bicerin_error::{BicerinError, BicerinResult};
use axum::extract::Path;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
pub struct SetPresenceBody {
    pub presence: String,
    pub status_msg: Option<String>,
}

pub async fn set_presence(
    user: AuthUser,
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    Json(body): Json<SetPresenceBody>,
) -> BicerinResult<Json<Value>> {
    if user_id != user.user_id {
        return Err(BicerinError::Forbidden);
    }
    if !matches!(body.presence.as_str(), "online" | "unavailable" | "offline") {
        return Err(BicerinError::BadRequest(
            "presence must be \"online\", \"unavailable\", or \"offline\"".to_string(),
        ));
    }
    let stream_id = bicerin_storage::events::get_next_stream_id(&state.pool)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    let now = chrono::Utc::now();
    bicerin_storage::presence::set_presence(
        &state.pool,
        &bicerin_storage::presence::PresenceRecord {
            user_id: user.user_id.clone(),
            presence: body.presence.clone(),
            status_msg: body.status_msg,
            last_active_ts: now.timestamp_millis(),
            currently_active: body.presence == "online",
            stream_id,
            updated_at: now,
        },
    )
    .await
    .map_err(|e| BicerinError::Internal(e.to_string()))?;
    state.sync_bus.notify_all(stream_id);
    Ok(Json(json!({})))
}

pub async fn get_presence(
    _user: AuthUser,
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> BicerinResult<Json<Value>> {
    let record = bicerin_storage::presence::get_presence(&state.pool, &user_id)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?
        .ok_or(BicerinError::NotFound)?;
    let last_active_ago = (chrono::Utc::now().timestamp_millis() - record.last_active_ts).max(0);
    Ok(Json(json!({
        "presence": record.presence,
        "status_msg": record.status_msg,
        "last_active_ago": last_active_ago,
        "currently_active": record.currently_active,
    })))
}
