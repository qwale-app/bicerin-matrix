use crate::{extract::AuthUser, state::AppState};
use axum::{extract::{Path, State}, Json};
use bicerin_error::{BicerinError, BicerinResult};
use bicerin_storage::filters::RoomReceiptRecord;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Debug, Deserialize, Default)]
pub struct ReceiptBody {
    pub thread_id: Option<String>,
}

pub async fn send_receipt(
    user: AuthUser,
    State(state): State<AppState>,
    Path((room_id, receipt_type, event_id)): Path<(String, String, String)>,
    Json(body): Json<ReceiptBody>,
) -> BicerinResult<Json<Value>> {
    store_receipt(
        &state,
        &user.user_id,
        &room_id,
        &receipt_type,
        body.thread_id.as_deref().unwrap_or(""),
        &event_id,
    )
    .await?;
    Ok(Json(json!({})))
}

#[derive(Debug, Deserialize, Default)]
pub struct ReadMarkersBody {
    #[serde(rename = "m.fully_read")]
    pub fully_read: Option<String>,
    #[serde(rename = "m.read")]
    pub read: Option<String>,
    #[serde(rename = "m.read.private")]
    pub read_private: Option<String>,
}

pub async fn set_read_markers(
    user: AuthUser,
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(body): Json<ReadMarkersBody>,
) -> BicerinResult<Json<Value>> {
    let event_id = body
        .fully_read
        .as_deref()
        .or(body.read.as_deref())
        .or(body.read_private.as_deref());
    if event_id.is_none() {
        return Err(BicerinError::BadRequest("at least one read marker is required".into()));
    }
    if let Some(fully_read) = body.fully_read.as_deref() {
        ensure_room_event(&state, &user.user_id, &room_id, fully_read).await?;
        let stream_id = bicerin_storage::events::get_next_stream_id(&state.pool)
            .await
            .map_err(internal)?;
        bicerin_storage::client_data::upsert_account_data(
            &state.pool,
            &bicerin_storage::client_data::AccountDataRecord {
                user_id: user.user_id.clone(),
                room_id: room_id.clone(),
                event_type: "m.fully_read".into(),
                content: json!({"event_id": fully_read}),
                stream_id,
                updated_at: chrono::Utc::now(),
            },
        )
        .await
        .map_err(internal)?;
        state.sync_bus.notify(room_id.clone(), stream_id);
    }
    if let Some(read) = body.read.as_deref() {
        let event = ensure_room_event(&state, &user.user_id, &room_id, read).await?;
        write_receipt(&state, &user.user_id, &room_id, "m.read", "", read, event.stream_id)
            .await?;
    }
    if let Some(read_private) = body.read_private.as_deref() {
        let event = ensure_room_event(&state, &user.user_id, &room_id, read_private).await?;
        write_receipt(
            &state,
            &user.user_id,
            &room_id,
            "m.read.private",
            "",
            read_private,
            event.stream_id,
        )
        .await?;
    }
    Ok(Json(json!({})))
}

async fn store_receipt(
    state: &AppState,
    user_id: &str,
    room_id: &str,
    receipt_type: &str,
    thread_id: &str,
    event_id: &str,
) -> BicerinResult<()> {
    if !matches!(receipt_type, "m.read" | "m.read.private") {
        return Err(BicerinError::BadRequest("unsupported receipt type".into()));
    }
    let event = ensure_room_event(state, user_id, room_id, event_id).await?;
    write_receipt(
        state,
        user_id,
        room_id,
        receipt_type,
        thread_id,
        event_id,
        event.stream_id,
    )
    .await
}

async fn write_receipt(
    state: &AppState,
    user_id: &str,
    room_id: &str,
    receipt_type: &str,
    thread_id: &str,
    event_id: &str,
    event_stream_id: i64,
) -> BicerinResult<()> {
    let stream_id = bicerin_storage::events::get_next_stream_id(&state.pool)
        .await
        .map_err(internal)?;
    bicerin_storage::filters::upsert_receipt(
        &state.pool,
        &RoomReceiptRecord {
            room_id: room_id.into(),
            user_id: user_id.into(),
            receipt_type: receipt_type.into(),
            thread_id: thread_id.into(),
            event_id: event_id.into(),
            event_stream_id,
            stream_id,
            timestamp: Some(chrono::Utc::now().timestamp_millis()),
            updated_at: chrono::Utc::now(),
        },
    )
    .await
    .map_err(internal)?;
    state.sync_bus.notify(room_id.to_string(), stream_id);
    Ok(())
}

async fn ensure_room_event(
    state: &AppState,
    user_id: &str,
    room_id: &str,
    event_id: &str,
) -> BicerinResult<bicerin_storage::events::EventRecord> {
    match bicerin_storage::rooms::get_room_member(&state.pool, room_id, user_id).await {
        Ok(member) if member.membership == "join" => {}
        Ok(_) => return Err(BicerinError::Forbidden),
        Err(_) => return Err(BicerinError::NotFound),
    }
    let event = bicerin_storage::events::get_event(&state.pool, event_id)
        .await
        .map_err(|_| BicerinError::NotFound)?;
    if event.room_id != room_id {
        return Err(BicerinError::NotFound);
    }
    Ok(event)
}

#[derive(Debug, Deserialize)]
pub struct TypingBody {
    pub typing: bool,
    pub timeout: Option<u64>,
}

pub async fn set_typing(
    user: AuthUser,
    State(state): State<AppState>,
    Path((room_id, requested_user)): Path<(String, String)>,
    Json(body): Json<TypingBody>,
) -> BicerinResult<Json<Value>> {
    if requested_user != user.user_id {
        return Err(BicerinError::Forbidden);
    }
    match bicerin_storage::rooms::get_room_member(&state.pool, &room_id, &user.user_id).await {
        Ok(member) if member.membership == "join" => {}
        Ok(_) => return Err(BicerinError::Forbidden),
        Err(_) => return Err(BicerinError::NotFound),
    }

    let timeout_ms = body.timeout.unwrap_or(30_000).clamp(1_000, 120_000);
    let stream_id = bicerin_storage::events::get_next_stream_id(&state.pool)
        .await
        .map_err(internal)?;
    let timeout = Duration::from_millis(timeout_ms);
    state.sync_bus.set_typing(
        &room_id,
        &user.user_id,
        stream_id,
        body.typing,
        timeout,
    );
    state.sync_bus.notify(room_id.clone(), stream_id);

    if body.typing {
        let bus = state.sync_bus.clone();
        let store = state.pool.clone();
        let typing_room = room_id.clone();
        let typing_user = user.user_id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(timeout).await;
            let expiry_stream_id = match bicerin_storage::events::get_next_stream_id(&store).await {
                Ok(stream_id) => stream_id,
                Err(error) => {
                    tracing::warn!(error = %error, "failed to expire typing notification");
                    return;
                }
            };
            if bus.stop_typing_if_version(
                &typing_room,
                &typing_user,
                stream_id,
                expiry_stream_id,
            ) {
                bus.notify(typing_room, expiry_stream_id);
            }
        });
    }

    Ok(Json(json!({})))
}

fn internal(error: impl std::fmt::Display) -> BicerinError {
    BicerinError::Internal(error.to_string())
}
