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
    pub visibility: Option<String>,
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
        visibility: body.visibility,
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
    let room_id = if room_id.starts_with('#') {
        bicerin_storage::rooms::get_room_id_for_alias(&state.pool, &room_id)
            .await
            .map_err(|_| BicerinError::NotFound)?
    } else {
        room_id
    };
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

#[derive(Debug, Deserialize)]
pub struct ModerateMemberBody {
    pub user_id: String,
    pub reason: Option<String>,
}

pub async fn kick_user(
    user: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(body): Json<ModerateMemberBody>,
) -> BicerinResult<Json<Value>> {
    moderate_member(state, user, room_id, body, MemberModeration::Kick).await
}

pub async fn ban_user(
    user: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(body): Json<ModerateMemberBody>,
) -> BicerinResult<Json<Value>> {
    moderate_member(state, user, room_id, body, MemberModeration::Ban).await
}

pub async fn unban_user(
    user: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(body): Json<ModerateMemberBody>,
) -> BicerinResult<Json<Value>> {
    moderate_member(state, user, room_id, body, MemberModeration::Unban).await
}

#[derive(Clone, Copy)]
enum MemberModeration {
    Kick,
    Ban,
    Unban,
}

async fn moderate_member(
    state: AppState,
    user: AuthUser,
    room_id: String,
    body: ModerateMemberBody,
    moderation: MemberModeration,
) -> BicerinResult<Json<Value>> {
    bicerin_events::validation::validate_room_id(&room_id)?;
    bicerin_events::validation::validate_user_id(&body.user_id)?;
    let stream_id = bicerin_storage::events::get_next_stream_id(&state.pool)
        .await
        .map_err(|error| BicerinError::Internal(error.to_string()))?;
    let event_id = match moderation {
        MemberModeration::Kick => {
            state
                .rooms
                .kick_user(
                    &room_id,
                    &user.user_id,
                    &body.user_id,
                    body.reason.as_deref(),
                    &state.server_name,
                    stream_id,
                )
                .await?
        }
        MemberModeration::Ban => {
            state
                .rooms
                .ban_user(
                    &room_id,
                    &user.user_id,
                    &body.user_id,
                    body.reason.as_deref(),
                    &state.server_name,
                    stream_id,
                )
                .await?
        }
        MemberModeration::Unban => {
            state
                .rooms
                .unban_user(
                    &room_id,
                    &user.user_id,
                    &body.user_id,
                    body.reason.as_deref(),
                    &state.server_name,
                    stream_id,
                )
                .await?
        }
    };
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

#[derive(Debug, Deserialize)]
pub struct PutAliasBody {
    pub room_id: String,
}

pub async fn get_room_id_for_alias(
    State(state): State<AppState>,
    Path(room_alias): Path<String>,
) -> BicerinResult<Json<Value>> {
    let room_id = bicerin_storage::rooms::get_room_id_for_alias(&state.pool, &room_alias)
        .await
        .map_err(|_| BicerinError::NotFound)?;
    Ok(Json(
        json!({ "room_id": room_id, "servers": [state.server_name.clone()] }),
    ))
}

pub async fn put_room_alias(
    user: AuthUser,
    State(state): State<AppState>,
    Path(room_alias): Path<String>,
    Json(body): Json<PutAliasBody>,
) -> BicerinResult<Json<Value>> {
    bicerin_events::validation::validate_room_id(&body.room_id)?;
    let member = bicerin_storage::rooms::get_room_member(&state.pool, &body.room_id, &user.user_id)
        .await
        .map_err(|_| BicerinError::Forbidden)?;
    if member.membership != "join" {
        return Err(BicerinError::Forbidden);
    }
    bicerin_storage::rooms::create_alias(&state.pool, &room_alias, &body.room_id, &user.user_id)
        .await
        .map_err(|error| match error {
            bicerin_storage::db::StorageError::Conflict(msg) => BicerinError::MatrixError {
                errcode: "M_ROOM_IN_USE".to_string(),
                error: msg,
            },
            other => BicerinError::Internal(other.to_string()),
        })?;
    Ok(Json(json!({})))
}

pub async fn delete_room_alias(
    user: AuthUser,
    State(state): State<AppState>,
    Path(room_alias): Path<String>,
) -> BicerinResult<Json<Value>> {
    let room_id = bicerin_storage::rooms::get_room_id_for_alias(&state.pool, &room_alias)
        .await
        .map_err(|_| BicerinError::NotFound)?;
    let power_levels = bicerin_rooms::powerlevels::load_power_levels(&state.pool, &room_id).await?;
    let state_default = power_levels
        .get("state_default")
        .and_then(Value::as_i64)
        .unwrap_or(50);
    if !bicerin_rooms::powerlevels::check_power_level(&power_levels, &user.user_id, state_default) {
        return Err(BicerinError::Forbidden);
    }
    bicerin_storage::rooms::delete_alias(&state.pool, &room_alias)
        .await
        .map_err(|_| BicerinError::NotFound)?;
    Ok(Json(json!({})))
}

pub async fn get_room_visibility(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> BicerinResult<Json<Value>> {
    let room = bicerin_storage::rooms::get_room(&state.pool, &room_id)
        .await
        .map_err(|_| BicerinError::NotFound)?;
    Ok(Json(json!({ "visibility": room.visibility })))
}

#[derive(Debug, Deserialize)]
pub struct SetVisibilityBody {
    pub visibility: String,
}

pub async fn put_room_visibility(
    user: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(body): Json<SetVisibilityBody>,
) -> BicerinResult<Json<Value>> {
    if body.visibility != "public" && body.visibility != "private" {
        return Err(BicerinError::BadRequest(
            "visibility must be \"public\" or \"private\"".to_string(),
        ));
    }
    let power_levels = bicerin_rooms::powerlevels::load_power_levels(&state.pool, &room_id).await?;
    let state_default = power_levels
        .get("state_default")
        .and_then(Value::as_i64)
        .unwrap_or(50);
    if !bicerin_rooms::powerlevels::check_power_level(&power_levels, &user.user_id, state_default) {
        return Err(BicerinError::Forbidden);
    }
    bicerin_storage::rooms::set_room_visibility(&state.pool, &room_id, &body.visibility)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    Ok(Json(json!({})))
}

#[derive(Debug, Deserialize)]
pub struct PublicRoomsQuery {
    pub limit: Option<i64>,
}

pub async fn get_public_rooms(
    _user: AuthUser,
    State(state): State<AppState>,
    Query(query): Query<PublicRoomsQuery>,
) -> BicerinResult<Json<Value>> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let rooms = bicerin_storage::rooms::list_public_rooms(&state.pool, limit)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    let mut chunk = Vec::with_capacity(rooms.len());
    for room in &rooms {
        let members = bicerin_storage::rooms::get_room_members_by_membership(
            &state.pool,
            &room.room_id,
            "join",
        )
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
        chunk.push(json!({
            "room_id": room.room_id,
            "name": room.name,
            "topic": room.topic,
            "canonical_alias": room.canonical_alias,
            "num_joined_members": members.len(),
            "world_readable": false,
            "guest_can_join": false,
        }));
    }
    Ok(Json(json!({
        "chunk": chunk,
        "total_room_count_estimate": chunk.len(),
    })))
}

#[derive(Debug, Deserialize)]
pub struct ContextQuery {
    pub limit: Option<i64>,
}

pub async fn get_context(
    _user: AuthUser,
    State(state): State<AppState>,
    Path((room_id, event_id)): Path<(String, String)>,
    Query(query): Query<ContextQuery>,
) -> BicerinResult<Json<Value>> {
    let event = bicerin_storage::events::get_event(&state.pool, &event_id)
        .await
        .map_err(|_| BicerinError::NotFound)?;
    if event.room_id != room_id {
        return Err(BicerinError::NotFound);
    }
    let limit = query.limit.unwrap_or(10).clamp(1, 100);

    let before = bicerin_storage::events::get_events_in_room(
        &state.pool,
        &room_id,
        event.stream_id,
        limit,
        "b",
    )
    .await
    .map_err(|e| BicerinError::Internal(e.to_string()))?;
    let after = bicerin_storage::events::get_events_in_room(
        &state.pool,
        &room_id,
        event.stream_id,
        limit,
        "f",
    )
    .await
    .map_err(|e| BicerinError::Internal(e.to_string()))?;
    let state_events = bicerin_storage::rooms::get_full_room_state(&state.pool, &room_id)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;

    let start = before
        .last()
        .map(|e| bicerin_sync::token::SyncToken::new(e.stream_id).to_string())
        .unwrap_or_else(|| bicerin_sync::token::SyncToken::new(event.stream_id).to_string());
    let end = after
        .last()
        .map(|e| bicerin_sync::token::SyncToken::new(e.stream_id).to_string())
        .unwrap_or_else(|| bicerin_sync::token::SyncToken::new(event.stream_id).to_string());

    Ok(Json(json!({
        "event": event_to_client_event(&event),
        "events_before": before.iter().map(event_to_client_event).collect::<Vec<_>>(),
        "events_after": after.iter().map(event_to_client_event).collect::<Vec<_>>(),
        "state": state_events.into_iter().map(state_to_client_event).collect::<Vec<_>>(),
        "start": start,
        "end": end,
    })))
}

async fn relations_chunk(
    state: &AppState,
    event_id: &str,
    rel_type: Option<&str>,
    event_type: Option<&str>,
) -> BicerinResult<Vec<Value>> {
    let relations = bicerin_storage::events::get_event_relations(&state.pool, event_id, rel_type)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    let mut chunk = Vec::with_capacity(relations.len());
    for relation in relations {
        if let Ok(child) =
            bicerin_storage::events::get_event(&state.pool, &relation.child_event_id).await
        {
            if event_type.is_none_or(|t| t == child.event_type) {
                chunk.push(event_to_client_event(&child));
            }
        }
    }
    chunk.sort_by_key(|event| event["origin_server_ts"].as_i64().unwrap_or(0));
    Ok(chunk)
}

pub async fn get_relations(
    _user: AuthUser,
    State(state): State<AppState>,
    Path((_room_id, event_id)): Path<(String, String)>,
) -> BicerinResult<Json<Value>> {
    let chunk = relations_chunk(&state, &event_id, None, None).await?;
    Ok(Json(json!({ "chunk": chunk })))
}

pub async fn get_relations_by_type(
    _user: AuthUser,
    State(state): State<AppState>,
    Path((_room_id, event_id, rel_type)): Path<(String, String, String)>,
) -> BicerinResult<Json<Value>> {
    let chunk = relations_chunk(&state, &event_id, Some(&rel_type), None).await?;
    Ok(Json(json!({ "chunk": chunk })))
}

pub async fn get_relations_by_type_and_event_type(
    _user: AuthUser,
    State(state): State<AppState>,
    Path((_room_id, event_id, rel_type, event_type)): Path<(String, String, String, String)>,
) -> BicerinResult<Json<Value>> {
    let chunk = relations_chunk(&state, &event_id, Some(&rel_type), Some(&event_type)).await?;
    Ok(Json(json!({ "chunk": chunk })))
}
