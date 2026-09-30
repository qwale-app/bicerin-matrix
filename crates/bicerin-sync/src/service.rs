use crate::{
    filter::SyncFilter,
    subscriptions::SyncBus,
    token::SyncToken,
};
use bicerin_error::{BicerinError, BicerinResult};
use serde::Serialize;
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
    store: bicerin_storage::Store,
    bus: Arc<SyncBus>,
    #[allow(dead_code)]
    server_name: String,
}

impl SyncService {
    pub fn new(store: bicerin_storage::Store, bus: Arc<SyncBus>, server_name: String) -> Self {
        Self { store, bus, server_name }
    }

    pub async fn sync(
        &self,
        user_id: &str,
        _device_id: &str,
        since: Option<String>,
        timeout_ms: u64,
        filter: Option<SyncFilter>,
    ) -> BicerinResult<SyncResponse> {
        let since_position = since
            .as_deref()
            .and_then(SyncToken::parse)
            .map(|t| t.position())
            .unwrap_or(0);

        let joined_rooms = bicerin_storage::rooms::get_joined_rooms(&self.store, user_id)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;

        let rooms_with_updates = bicerin_storage::sync::get_rooms_with_new_events(
            &self.store,
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

        let current_position = bicerin_storage::sync::get_current_stream_position(&self.store)
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
                &self.store,
                room_id,
                if is_initial { i64::MAX } else { current_position },
                timeline_limit,
                "b",
            ).await.map_err(|e| BicerinError::Internal(e.to_string()))?;

            let timeline_events: Vec<_> = events.iter().rev()
                .filter(|e| e.stream_id > since_position)
                .map(|e| event_record_to_client_event(e))
                .collect();

            let limited = events.len() >= timeline_limit as usize;
            let prev_batch = events.first().map(|e| SyncToken::new(e.stream_id).to_string());

            let state_events = if is_initial {
                bicerin_storage::rooms::get_full_room_state(&self.store, room_id)
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
