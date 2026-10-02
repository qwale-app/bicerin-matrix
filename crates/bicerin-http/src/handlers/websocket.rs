use crate::{extract::AuthUser, state::AppState};
use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, Query, State},
    response::Response,
};
use bicerin_sync::subscriptions::RoomUpdate;
use serde::Deserialize;
use std::collections::HashSet;

/// A Bicerin extension that streams normal `/sync` responses over a WebSocket.
/// The Matrix Client-Server specification does not define a WebSocket transport,
/// so this intentionally lives under an unstable, vendor-namespaced endpoint.
#[derive(Debug, Deserialize, Default)]
pub struct SocketQuery {
    pub since: Option<String>,
}

pub async fn sync_socket(
    user: AuthUser,
    State(state): State<AppState>,
    Query(query): Query<SocketQuery>,
    websocket: WebSocketUpgrade,
) -> Response {
    websocket.on_upgrade(move |socket| run_socket(socket, state, user, query.since))
}

async fn run_socket(
    mut socket: WebSocket,
    state: AppState,
    user: AuthUser,
    mut since: Option<String>,
) {
    let mut updates = state.sync_bus.subscribe();
    let mut room_ids = joined_room_ids(&state, &user.user_id).await;

    if !send_sync(&mut socket, &state, &user, &mut since).await {
        return;
    }

    loop {
        tokio::select! {
            update = updates.recv() => match update {
                Ok(update) if update_is_relevant(&update, &user.user_id, &room_ids) => {
                    if !send_sync(&mut socket, &state, &user, &mut since).await {
                        return;
                    }
                    room_ids = joined_room_ids(&state, &user.user_id).await;
                }
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            },
            message = socket.recv() => match message {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                Some(Ok(Message::Ping(payload))) => {
                    if socket.send(Message::Pong(payload)).await.is_err() {
                        return;
                    }
                }
                Some(Ok(_)) => {}
            }
        }
    }
}

async fn send_sync(
    socket: &mut WebSocket,
    state: &AppState,
    user: &AuthUser,
    since: &mut Option<String>,
) -> bool {
    let response = match state
        .sync
        .sync(&user.user_id, &user.device_id, since.clone(), 0, None)
        .await
    {
        Ok(response) => response,
        Err(error) => {
            let payload = serde_json::json!({
                "errcode": "M_UNKNOWN",
                "error": error.to_string(),
            });
            let _ = socket.send(Message::Text(payload.to_string())).await;
            return false;
        }
    };

    since.replace(response.next_batch.clone());
    match serde_json::to_string(&response) {
        Ok(payload) => socket.send(Message::Text(payload)).await.is_ok(),
        Err(error) => {
            tracing::warn!(error = %error, "failed to serialize WebSocket sync response");
            false
        }
    }
}

async fn joined_room_ids(state: &AppState, user_id: &str) -> HashSet<String> {
    match bicerin_storage::rooms::get_joined_rooms(&state.pool, user_id).await {
        Ok(room_ids) => room_ids.into_iter().collect(),
        Err(error) => {
            tracing::warn!(error = %error, user_id, "failed to refresh WebSocket room subscription set");
            HashSet::new()
        }
    }
}

fn update_is_relevant(update: &RoomUpdate, user_id: &str, room_ids: &HashSet<String>) -> bool {
    update.user_id.as_deref() == Some(user_id)
        || (update.user_id.is_none() && (update.room_id.is_empty() || room_ids.contains(&update.room_id)))
}

#[cfg(test)]
mod tests {
    use super::update_is_relevant;
    use bicerin_sync::subscriptions::RoomUpdate;
    use std::collections::HashSet;

    #[test]
    fn only_user_global_or_joined_room_updates_are_streamed() {
        let rooms = HashSet::from(["!joined:example.test".to_string()]);
        assert!(update_is_relevant(&RoomUpdate { room_id: "!joined:example.test".into(), stream_id: 1, user_id: None }, "@alice:example.test", &rooms));
        assert!(update_is_relevant(&RoomUpdate { room_id: String::new(), stream_id: 1, user_id: Some("@alice:example.test".into()) }, "@alice:example.test", &rooms));
        assert!(!update_is_relevant(&RoomUpdate { room_id: "!other:example.test".into(), stream_id: 1, user_id: None }, "@alice:example.test", &rooms));
    }
}