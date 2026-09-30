use crate::{extract::AuthUser, state::AppState};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use bicerin_error::{BicerinError, BicerinResult};
use bicerin_rooms::creation::{CreateRoomParams, InitialStateEvent};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize, Default)]
pub struct CreateRoomBody {
    pub name: Option<String>,
    pub topic: Option<String>,
    #[serde(default)]
    pub is_direct: bool,
    pub preset: Option<String>,
    pub room_alias_name: Option<String>,
    #[serde(default)]
    pub invite: Vec<String>,
    pub room_version: Option<String>,
    #[serde(default)]
    pub initial_state: Vec<InitialStateEvent>,
    pub power_level_content_override: Option<Value>,
}

pub async fn create_room(
    user: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CreateRoomBody>,
) -> BicerinResult<Json<Value>> {
    let room_version = body
        .room_version
        .unwrap_or_else(|| state.default_room_version.clone());

    let params = CreateRoomParams {
        creator: user.user_id.clone(),
        room_version,
        name: body.name,
        topic: body.topic,
        is_direct: body.is_direct,
        initial_state: body.initial_state,
        invite: body.invite.clone(),
        preset: body.preset,
        room_alias_name: body.room_alias_name,
        power_level_content_override: body.power_level_content_override,
    };

    let (room_id, event_ids) = state.rooms.create_room(params, &state.server_name).await?;

    for event_id in event_ids {
        dispatch_persisted_appservice_event(&state, &event_id).await;
    }

    for invitee in &body.invite {
        let stream_id = bicerin_storage::events::get_next_stream_id(&state.pool)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;
        match state
            .rooms
            .invite_user(
                &room_id,
                &user.user_id,
                invitee,
                &state.server_name,
                stream_id,
            )
            .await
        {
            Ok(event_id) => {
                dispatch_persisted_appservice_event(&state, &event_id).await;
                state.sync_bus.notify(room_id.clone(), stream_id);
            }
            Err(e) => {
                tracing::warn!(error = ?e, invitee = %invitee, "failed to invite user during room creation")
            }
        }
    }

    Ok(Json(json!({ "room_id": room_id })))
}

pub async fn join_room(
    user: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> BicerinResult<Json<Value>> {
    if room_id.starts_with('#') {
        return Err(BicerinError::BadRequest(
            "room alias resolution is not implemented".to_string(),
        ));
    }
    let stream_id = bicerin_storage::events::get_next_stream_id(&state.pool)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    let event_id = state
        .rooms
        .join_room(&room_id, &user.user_id, &state.server_name, stream_id)
        .await?;
    dispatch_persisted_appservice_event(&state, &event_id).await;
    state.sync_bus.notify(room_id.clone(), stream_id);
    Ok(Json(json!({ "room_id": room_id })))
}

pub async fn leave_room(
    user: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> BicerinResult<Json<Value>> {
    let stream_id = bicerin_storage::events::get_next_stream_id(&state.pool)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    let event_id = state
        .rooms
        .leave_room(&room_id, &user.user_id, &state.server_name, stream_id)
        .await?;
    dispatch_persisted_appservice_event(&state, &event_id).await;
    state.sync_bus.notify(room_id, stream_id);
    Ok(Json(json!({})))
}

#[derive(Debug, Deserialize)]
pub struct InviteBody {
    pub user_id: String,
}

pub async fn invite_user(
    user: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(body): Json<InviteBody>,
) -> BicerinResult<Json<Value>> {
    let stream_id = bicerin_storage::events::get_next_stream_id(&state.pool)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    let event_id = state
        .rooms
        .invite_user(
            &room_id,
            &user.user_id,
            &body.user_id,
            &state.server_name,
            stream_id,
        )
        .await?;
    dispatch_persisted_appservice_event(&state, &event_id).await;
    state.sync_bus.notify(room_id, stream_id);
    Ok(Json(json!({})))
}

async fn dispatch_persisted_appservice_event(state: &AppState, event_id: &str) {
    match bicerin_storage::events::get_event(&state.pool, event_id).await {
        Ok(event) => {
            bicerin_events::appservice_dispatch::dispatch(&state.pool, &state.server_name, &event)
                .await
        }
        Err(error) => {
            tracing::warn!(error = %error, event_id = %event_id, "failed to load event for appservice dispatch")
        }
    }
}

pub async fn send_event(
    user: AuthUser,
    State(state): State<AppState>,
    Path((room_id, event_type, txn_id)): Path<(String, String, String)>,
    Json(content): Json<Value>,
) -> BicerinResult<Json<Value>> {
    let endpoint = format!("send:{}:{}", room_id, event_type);

    if let Some(existing) = bicerin_storage::transactions::get_transaction(
        &state.pool,
        &user.user_id,
        &user.device_id,
        &txn_id,
        &endpoint,
    )
    .await
    .map_err(|e| BicerinError::Internal(e.to_string()))?
    {
        return Ok(Json(existing.result));
    }

    let (event_id, stream_id) = state
        .events
        .send_event(&room_id, &user.user_id, &event_type, content, Some(&txn_id))
        .await?;
    let result = json!({ "event_id": event_id });

    if let Err(e) = bicerin_storage::transactions::record_transaction(
        &state.pool,
        &bicerin_storage::transactions::TransactionRecord {
            user_id: user.user_id.clone(),
            device_id: user.device_id.clone(),
            txn_id,
            endpoint,
            result: result.clone(),
            created_at: chrono::Utc::now(),
        },
    )
    .await
    {
        tracing::warn!(error = %e, "failed to record transaction idempotency record");
    }

    state.sync_bus.notify(room_id, stream_id);
    Ok(Json(result))
}

pub async fn put_state_event(
    user: AuthUser,
    State(state): State<AppState>,
    Path((room_id, event_type, state_key)): Path<(String, String, String)>,
    Json(content): Json<Value>,
) -> BicerinResult<Json<Value>> {
    let (event_id, stream_id) = state
        .events
        .send_state_event(&room_id, &user.user_id, &event_type, &state_key, content)
        .await?;
    state.sync_bus.notify(room_id, stream_id);
    Ok(Json(json!({ "event_id": event_id })))
}

pub async fn put_state_event_no_key(
    user: AuthUser,
    State(state): State<AppState>,
    Path((room_id, event_type)): Path<(String, String)>,
    Json(content): Json<Value>,
) -> BicerinResult<Json<Value>> {
    put_state_event(
        user,
        State(state),
        Path((room_id, event_type, String::new())),
        Json(content),
    )
    .await
}

pub async fn get_room_state(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> BicerinResult<Json<Value>> {
    let events = state.rooms.get_state(&room_id).await?;
    Ok(Json(json!(events
        .into_iter()
        .map(state_to_client_event)
        .collect::<Vec<_>>())))
}

pub async fn get_state_event(
    State(state): State<AppState>,
    Path((room_id, event_type, state_key)): Path<(String, String, String)>,
) -> BicerinResult<Json<Value>> {
    let record = state
        .rooms
        .get_state_event(&room_id, &event_type, &state_key)
        .await?;
    Ok(Json(record.content))
}

pub async fn get_state_event_no_key(
    State(state): State<AppState>,
    Path((room_id, event_type)): Path<(String, String)>,
) -> BicerinResult<Json<Value>> {
    get_state_event(State(state), Path((room_id, event_type, String::new()))).await
}

pub async fn get_members(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> BicerinResult<Json<Value>> {
    let members = bicerin_storage::rooms::get_room_members(&state.pool, &room_id)
        .await
        .map_err(|_| BicerinError::NotFound)?;
    let chunk: Vec<Value> = members
        .into_iter()
        .map(|m| {
            json!({
                "type": "m.room.member",
                "state_key": m.user_id,
                "room_id": m.room_id,
                "sender": m.sender,
                "event_id": m.event_id,
                "origin_server_ts": 0,
                "content": {
                    "membership": m.membership,
                    "displayname": m.display_name,
                    "avatar_url": m.avatar_url,
                },
            })
        })
        .collect();
    Ok(Json(json!({ "chunk": chunk })))
}

#[derive(Debug, Deserialize)]
pub struct MessagesQuery {
    pub from: Option<String>,
    pub to: Option<String>,
    #[serde(default = "default_dir")]
    pub dir: String,
    pub limit: Option<i64>,
}

fn default_dir() -> String {
    "b".to_string()
}

pub async fn get_messages(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Query(query): Query<MessagesQuery>,
) -> BicerinResult<Json<Value>> {
    let from = query
        .from
        .as_deref()
        .and_then(bicerin_sync::token::SyncToken::parse)
        .map(|t| t.position());
    let limit = query.limit.unwrap_or(10).clamp(1, 100);

    let (events, end) = state
        .events
        .get_room_messages(&room_id, from, None, &query.dir, limit)
        .await?;

    let chunk: Vec<Value> = events.iter().map(event_to_client_event).collect();
    let start_token = query
        .from
        .unwrap_or_else(|| bicerin_sync::token::SyncToken::new(0).to_string());
    let end_token = end
        .map(|e| bicerin_sync::token::SyncToken::new(e).to_string())
        .unwrap_or(start_token.clone());

    Ok(Json(json!({
        "chunk": chunk,
        "start": start_token,
        "end": end_token,
    })))
}

fn event_to_client_event(event: &bicerin_storage::events::EventRecord) -> Value {
    json!({
        "event_id": event.event_id,
        "room_id": event.room_id,
        "sender": event.sender,
        "type": event.event_type,
        "state_key": event.state_key,
        "origin_server_ts": event.origin_server_ts,
        "content": event.content,
    })
}

fn state_to_client_event(state: bicerin_storage::rooms::RoomStateRecord) -> Value {
    json!({
        "event_id": state.event_id,
        "room_id": state.room_id,
        "sender": state.sender,
        "type": state.event_type,
        "state_key": state.state_key,
        "origin_server_ts": 0,
        "content": state.content,
    })
}
