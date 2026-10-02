use crate::{
    filter::{EventFilter, RoomFilter, StateFilter, SyncFilter},
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
    pub account_data: AccountData,
    pub device_lists: DeviceLists,
    pub to_device: ToDevice,
    pub presence: Presence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_one_time_keys_count: Option<std::collections::HashMap<String, u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_unused_fallback_key_types: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Default)]
pub struct Presence {
    pub events: Vec<serde_json::Value>,
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
    #[serde(
        rename = "m.joined_member_count",
        skip_serializing_if = "Option::is_none"
    )]
    pub joined_member_count: Option<u64>,
    #[serde(
        rename = "m.invited_member_count",
        skip_serializing_if = "Option::is_none"
    )]
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
        Self {
            store,
            bus,
            server_name,
        }
    }

    pub async fn sync(
        &self,
        user_id: &str,
        device_id: &str,
        since: Option<String>,
        timeout_ms: u64,
        filter: Option<SyncFilter>,
    ) -> BicerinResult<SyncResponse> {
        let since_position = match since.as_deref() {
            Some(token) => SyncToken::parse(token)
                .map(|token| token.position())
                .ok_or_else(|| BicerinError::BadRequest("invalid since token".into()))?,
            None => 0,
        };

        if let Some(acknowledged_token) = since.as_deref() {
            bicerin_storage::client_data::acknowledge_to_device_messages(
                &self.store,
                user_id,
                device_id,
                acknowledged_token,
            )
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;
        }

        let mut joined_rooms = bicerin_storage::rooms::get_joined_rooms(&self.store, user_id)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;

        let mut invited_memberships = bicerin_storage::rooms::get_room_members_by_user_membership(
            &self.store,
            user_id,
            "invite",
        )
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
        let mut left_memberships = bicerin_storage::rooms::get_room_members_by_user_membership(
            &self.store,
            user_id,
            "leave",
        )
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;

        let rooms_with_updates = bicerin_storage::sync::get_rooms_with_new_events(
            &self.store,
            user_id,
            &joined_rooms,
            since_position,
        )
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;

        let membership_update_exists = invited_memberships
            .iter()
            .chain(left_memberships.iter())
            .any(|membership| membership.stream_id > since_position);

        let typing_update_exists = self.bus.has_typing_updates(&joined_rooms, since_position);
        let mut receipt_update_exists = false;
        for room_id in &joined_rooms {
            if !bicerin_storage::filters::get_receipts_since(
                &self.store,
                room_id,
                since_position,
                i64::MAX,
            )
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?
            .is_empty()
            {
                receipt_update_exists = true;
                break;
            }
        }

        let pending_to_device = bicerin_storage::client_data::get_pending_to_device_messages(
            &self.store,
            user_id,
            device_id,
            1,
        )
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
        if rooms_with_updates.is_empty()
            && !membership_update_exists
            && !typing_update_exists
            && !receipt_update_exists
            && pending_to_device.is_empty()
            && timeout_ms > 0
            && since.is_some()
        {
            let mut rx = self.bus.subscribe();
            let wait = Duration::from_millis(timeout_ms.min(30_000));

            let _ = timeout(wait, async {
                loop {
                    match rx.recv().await {
                        Ok(update) if update.user_id.as_deref() == Some(user_id) => break,
                        Ok(update) if update.room_id.is_empty() && update.user_id.is_none() => {
                            break
                        }
                        Ok(update) if joined_rooms.contains(&update.room_id) => break,
                        Ok(update) => {
                            let membership = bicerin_storage::rooms::get_room_member(
                                &self.store,
                                &update.room_id,
                                user_id,
                            )
                            .await;
                            if matches!(membership, Ok(ref member)
                                if matches!(member.membership.as_str(), "join" | "invite" | "leave")
                                    && member.stream_id > since_position)
                            {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            })
            .await;

            joined_rooms = bicerin_storage::rooms::get_joined_rooms(&self.store, user_id)
                .await
                .map_err(|e| BicerinError::Internal(e.to_string()))?;
            invited_memberships = bicerin_storage::rooms::get_room_members_by_user_membership(
                &self.store,
                user_id,
                "invite",
            )
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;
            left_memberships = bicerin_storage::rooms::get_room_members_by_user_membership(
                &self.store,
                user_id,
                "leave",
            )
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;
        }

        let current_position = bicerin_storage::sync::get_current_stream_position(&self.store)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;

        let to_device_messages = bicerin_storage::client_data::get_pending_to_device_messages(
            &self.store,
            user_id,
            device_id,
            100,
        )
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
        let next_batch = if to_device_messages.is_empty() {
            SyncToken::new(current_position).to_string()
        } else {
            format!(
                "{}~{}",
                SyncToken::new(current_position),
                uuid::Uuid::new_v4().simple()
            )
        };
        if !to_device_messages.is_empty() {
            let delivered_ids = to_device_messages
                .iter()
                .map(|message| message.message_id.clone())
                .collect::<Vec<_>>();
            bicerin_storage::client_data::mark_to_device_messages_delivered(
                &self.store,
                user_id,
                device_id,
                &delivered_ids,
                &next_batch,
            )
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;
        }
        let to_device_events = to_device_messages
            .into_iter()
            .map(|message| {
                serde_json::json!({
                    "sender": message.sender,
                    "type": message.event_type,
                    "content": message.content,
                })
            })
            .map(|event| apply_event_fields(event, filter.as_ref()))
            .filter(|event| {
                filter
                    .as_ref()
                    .and_then(|filter| filter.to_device.as_ref())
                    .is_none_or(|event_filter| matches_event_value(event, event_filter))
            })
            .collect();

        let global_account_data = if since.is_none() {
            bicerin_storage::client_data::get_account_data(&self.store, user_id, "").await
        } else {
            bicerin_storage::client_data::get_account_data_since(
                &self.store,
                user_id,
                since_position,
                current_position,
            )
            .await
        }
        .map_err(|e| BicerinError::Internal(e.to_string()))?
        .into_iter()
        .map(|record| serde_json::json!({"type": record.event_type, "content": record.content}))
        .filter(|event| {
            filter
                .as_ref()
                .and_then(|filter| filter.account_data.as_ref())
                .is_none_or(|event_filter| matches_event_value(event, event_filter))
        })
        .map(|event| apply_event_fields(event, filter.as_ref()))
        .collect();

        let device_updates = crate::device_lists::get_device_list_updates(
            &self.store,
            user_id,
            since_position,
            current_position,
        ).await?;

        let timeline_limit = filter
            .as_ref()
            .and_then(|f| f.room.as_ref())
            .and_then(|r| r.timeline.as_ref())
            .and_then(|t| t.limit)
            .unwrap_or(50)
            .clamp(0, 500);

        let room_filter = filter.as_ref().and_then(|filter| filter.room.as_ref());

        let mut join_map = std::collections::HashMap::new();
        let mut invite_map = std::collections::HashMap::new();
        let mut leave_map = std::collections::HashMap::new();

        for room_id in &joined_rooms {
            if !room_is_included(room_id, room_filter) {
                continue;
            }
            let is_initial = since.is_none();

            let events = bicerin_storage::events::get_events_in_room(
                &self.store,
                room_id,
                if is_initial {
                    i64::MAX
                } else {
                    current_position.saturating_add(1)
                },
                500,
                "b",
            )
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;

            let mut timeline_events: Vec<_> = events
                .iter()
                .rev()
                .filter(|e| e.stream_id > since_position)
                .filter(|event| {
                    room_filter
                        .and_then(|room| room.timeline.as_ref())
                        .is_none_or(|event_filter| matches_event_record(event, event_filter))
                })
                .map(|e| event_record_to_client_event(e))
                .collect();
            let has_more_filtered_events = timeline_events.len() > timeline_limit as usize;
            timeline_events.truncate(timeline_limit as usize);
            let timeline_events = timeline_events
                .into_iter()
                .map(|event| apply_event_fields(event, filter.as_ref()))
                .collect();

            let limited = has_more_filtered_events || events.len() >= 500;
            let prev_batch = events
                .first()
                .map(|e| SyncToken::new(e.stream_id).to_string());

            let mut state_events: Vec<_> = if is_initial {
                bicerin_storage::rooms::get_full_room_state(&self.store, room_id)
                    .await
                    .map_err(|e| BicerinError::Internal(e.to_string()))?
                    .into_iter()
                    .map(|s| state_record_to_client_event(&s))
                    .collect()
            } else {
                vec![]
            };
            if let Some(state_filter) = room_filter.and_then(|room| room.state.as_ref()) {
                state_events.retain(|event| matches_state_event(&event, state_filter));
                if let Some(limit) = state_filter.limit {
                    state_events.truncate(limit.clamp(0, 500) as usize);
                }
            }
            let state_events = state_events
                .into_iter()
                .map(|event| apply_event_fields(event, filter.as_ref()))
                .collect();

            let room_account_data = bicerin_storage::client_data::get_account_data(
                &self.store,
                user_id,
                room_id,
            )
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?
            .into_iter()
            .filter(|record| {
                is_initial
                    || (record.stream_id > since_position && record.stream_id <= current_position)
            })
            .map(|record| serde_json::json!({"type": record.event_type, "content": record.content}))
            .filter(|event| {
                room_filter
                    .and_then(|room| room.account_data.as_ref())
                    .is_none_or(|event_filter| matches_event_value(event, event_filter))
            })
            .map(|event| apply_event_fields(event, filter.as_ref()))
            .collect();

            let receipt_since = if is_initial { 0 } else { since_position };
            let receipt_records = bicerin_storage::filters::get_receipts_since(
                &self.store,
                room_id,
                receipt_since,
                current_position,
            )
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;
            let mut ephemeral_events = Vec::new();
            if let Some(content) = receipt_content(receipt_records, user_id) {
                ephemeral_events.push(serde_json::json!({"type":"m.receipt","content":content}));
            }
            if let Some(user_ids) = self.bus.typing_snapshot(room_id, since_position, is_initial) {
                ephemeral_events.push(serde_json::json!({
                    "type":"m.typing",
                    "content":{"user_ids":user_ids}
                }));
            }
            let ephemeral_events = ephemeral_events
                .into_iter()
                .filter(|event| {
                    room_filter
                        .and_then(|room| room.ephemeral.as_ref())
                        .is_none_or(|event_filter| matches_event_value(event, event_filter))
                })
                .map(|event| apply_event_fields(event, filter.as_ref()))
                .collect();

            let own_membership = bicerin_storage::rooms::get_room_member(
                &self.store,
                room_id,
                user_id,
            )
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;
            let public_receipt = bicerin_storage::filters::get_latest_receipt(
                &self.store,
                room_id,
                user_id,
                "m.read",
                "",
            )
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;
            let private_receipt = bicerin_storage::filters::get_latest_receipt(
                &self.store,
                room_id,
                user_id,
                "m.read.private",
                "",
            )
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;
            let read_position = public_receipt
                .into_iter()
                .chain(private_receipt)
                .map(|receipt| receipt.event_stream_id)
                .max()
                .unwrap_or(own_membership.stream_id);
            let notification_count = bicerin_storage::events::count_unread_messages(
                &self.store,
                room_id,
                user_id,
                read_position,
                current_position,
            )
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;
            let highlight_count = bicerin_storage::events::count_highlighted_messages(
                &self.store,
                room_id,
                user_id,
                user_id,
                read_position,
                current_position,
            )
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;

            join_map.insert(
                room_id.clone(),
                JoinedRoomSync {
                    timeline: Timeline {
                        events: timeline_events,
                        limited,
                        prev_batch,
                    },
                    state: State {
                        events: state_events,
                    },
                    ephemeral: Ephemeral {
                        events: ephemeral_events,
                    },
                    account_data: AccountData {
                        events: room_account_data,
                    },
                    summary: RoomSummary::default(),
                    unread_notifications: UnreadNotificationCounts {
                        notification_count,
                        highlight_count,
                    },
                },
            );
        }

        for membership in invited_memberships {
            if !room_is_included(&membership.room_id, room_filter) {
                continue;
            }
            if since.is_some() && membership.stream_id <= since_position {
                continue;
            }

            let states =
                bicerin_storage::rooms::get_full_room_state(&self.store, &membership.room_id)
                    .await
                    .map_err(|e| BicerinError::Internal(e.to_string()))?;
            let invite_state = states.iter().map(state_record_to_stripped_event).collect();
            invite_map.insert(
                membership.room_id,
                InvitedRoomSync {
                    invite_state: InviteState {
                        events: invite_state,
                    },
                },
            );
        }

        for membership in left_memberships {
            if !room_filter.is_none_or(|room| room.include_leave.unwrap_or(true))
                || !room_is_included(&membership.room_id, room_filter)
            {
                continue;
            }
            if since.is_some() && membership.stream_id <= since_position {
                continue;
            }

            let events = bicerin_storage::events::get_events_in_room(
                &self.store,
                &membership.room_id,
                current_position.saturating_add(1),
                500,
                "b",
            )
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;
            let mut timeline_events: Vec<_> = events
                .iter()
                .rev()
                .filter(|event| {
                    event.stream_id >= membership.stream_id && event.stream_id > since_position
                })
                .filter(|event| {
                    room_filter
                        .and_then(|room| room.timeline.as_ref())
                        .is_none_or(|event_filter| matches_event_record(event, event_filter))
                })
                .map(event_record_to_client_event)
                .collect();
            let has_more_filtered_events = timeline_events.len() > timeline_limit as usize;
            timeline_events.truncate(timeline_limit as usize);
            let timeline_events = timeline_events
                .into_iter()
                .map(|event| apply_event_fields(event, filter.as_ref()))
                .collect();
            let limited = has_more_filtered_events || events.len() >= 500;
            let prev_batch = events
                .first()
                .map(|event| SyncToken::new(event.stream_id).to_string());

            leave_map.insert(
                membership.room_id,
                LeftRoomSync {
                    timeline: Timeline {
                        events: timeline_events,
                        limited,
                        prev_batch,
                    },
                    state: State::default(),
                },
            );
        }

        let watched_users = bicerin_storage::rooms::get_joined_member_user_ids(
            &self.store,
            &joined_rooms,
            user_id,
        )
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
        let presence_since = if since.is_none() { -1 } else { since_position };
        let presence_updates = bicerin_storage::presence::get_presence_updates_for_users(
            &self.store,
            &watched_users,
            presence_since,
        )
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
        let presence_filter = filter.as_ref().and_then(|filter| filter.presence.as_ref());
        let presence_events: Vec<_> = presence_updates
            .into_iter()
            .map(|record| {
                serde_json::json!({
                    "type": "m.presence",
                    "sender": record.user_id,
                    "content": {
                        "presence": record.presence,
                        "status_msg": record.status_msg,
                        "currently_active": record.currently_active,
                        "last_active_ago": (chrono::Utc::now().timestamp_millis() - record.last_active_ts).max(0),
                    },
                })
            })
            .filter(|event| presence_filter.is_none_or(|event_filter| matches_event_value(event, event_filter)))
            .collect();

        Ok(SyncResponse {
            next_batch,
            rooms: SyncRooms {
                join: join_map,
                invite: invite_map,
                leave: leave_map,
            },
            account_data: AccountData {
                events: global_account_data,
            },
            device_lists: DeviceLists {
                changed: device_updates.changed,
                left: device_updates.left,
            },
            presence: Presence {
                events: presence_events,
            },
            to_device: ToDevice {
                events: to_device_events,
            },
            device_one_time_keys_count: Some(
                bicerin_storage::crypto::count_one_time_keys(&self.store, user_id, device_id)
                    .await
                    .map_err(|e| BicerinError::Internal(e.to_string()))?,
            ),
            device_unused_fallback_key_types: Some(
                bicerin_storage::crypto::count_unused_fallback_keys(
                    &self.store,
                    user_id,
                    device_id,
                )
                .await
                .map_err(|e| BicerinError::Internal(e.to_string()))?
                .into_keys()
                .collect(),
            ),
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

fn state_record_to_client_event(
    state: &bicerin_storage::rooms::RoomStateRecord,
) -> serde_json::Value {
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

fn state_record_to_stripped_event(
    state: &bicerin_storage::rooms::RoomStateRecord,
) -> serde_json::Value {
    serde_json::json!({
        "type": state.event_type,
        "state_key": state.state_key,
        "content": state.content,
        "sender": state.sender,
    })
}

fn receipt_content(
    receipts: Vec<bicerin_storage::filters::RoomReceiptRecord>,
    requesting_user: &str,
) -> Option<serde_json::Value> {
    let mut content = serde_json::Map::new();
    for receipt in receipts {
        if receipt.receipt_type == "m.read.private" && receipt.user_id != requesting_user {
            continue;
        }
        let mut data = serde_json::Map::new();
        if let Some(timestamp) = receipt.timestamp {
            data.insert("ts".into(), serde_json::Value::from(timestamp));
        }
        if !receipt.thread_id.is_empty() {
            data.insert(
                "thread_id".into(),
                serde_json::Value::String(receipt.thread_id),
            );
        }
        content
            .entry(receipt.event_id)
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()?
            .entry(receipt.receipt_type)
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()?
            .insert(receipt.user_id, serde_json::Value::Object(data));
    }
    (!content.is_empty()).then_some(serde_json::Value::Object(content))
}

fn room_is_included(room_id: &str, filter: Option<&RoomFilter>) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    if filter
        .rooms
        .as_ref()
        .is_some_and(|rooms| !rooms.iter().any(|room| room == room_id))
    {
        return false;
    }
    !filter
        .not_rooms
        .as_ref()
        .is_some_and(|rooms| rooms.iter().any(|room| room == room_id))
}

fn matches_event_record(
    event: &bicerin_storage::events::EventRecord,
    filter: &EventFilter,
) -> bool {
    matches_event_parts(&event.event_type, &event.sender, &event.content, filter)
}

fn matches_event_value(event: &serde_json::Value, filter: &EventFilter) -> bool {
    let event_type = event.get("type").and_then(serde_json::Value::as_str).unwrap_or("");
    let sender = event.get("sender").and_then(serde_json::Value::as_str).unwrap_or("");
    let content = event.get("content").unwrap_or(&serde_json::Value::Null);
    matches_event_parts(event_type, sender, content, filter)
}

fn matches_event_parts(
    event_type: &str,
    sender: &str,
    content: &serde_json::Value,
    filter: &EventFilter,
) -> bool {
    if filter.types.as_ref().is_some_and(|types| !types.iter().any(|t| wildcard_match(t, event_type)))
        || filter.not_types.as_ref().is_some_and(|types| types.iter().any(|t| wildcard_match(t, event_type)))
        || filter.senders.as_ref().is_some_and(|senders| !senders.iter().any(|value| value == sender))
        || filter.not_senders.as_ref().is_some_and(|senders| senders.iter().any(|value| value == sender))
    {
        return false;
    }
    if let Some(contains_url) = filter.contains_url {
        let has_url = content.get("url").is_some();
        if contains_url != has_url {
            return false;
        }
    }
    true
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == value,
        Some((_prefix, suffix)) if suffix.contains('*') => false,
        Some((prefix, suffix)) => value.starts_with(prefix) && value.ends_with(suffix),
    }
}

fn matches_state_event(event: &serde_json::Value, filter: &StateFilter) -> bool {
    let event_type = event.get("type").and_then(serde_json::Value::as_str).unwrap_or("");
    filter.types.as_ref().is_none_or(|types| types.iter().any(|t| wildcard_match(t, event_type)))
        && !filter.not_types.as_ref().is_some_and(|types| types.iter().any(|t| wildcard_match(t, event_type)))
}

fn apply_event_fields(mut event: serde_json::Value, filter: Option<&SyncFilter>) -> serde_json::Value {
    let Some(fields) = filter.and_then(|filter| filter.event_fields.as_ref()) else {
        return event;
    };
    let Some(object) = event.as_object_mut() else {
        return event;
    };
    object.retain(|field, _| fields.iter().any(|allowed| allowed == field));
    event
}

#[cfg(test)]
mod filter_tests {
    use super::{
        apply_event_fields, matches_event_parts, matches_state_event, room_is_included,
        wildcard_match,
    };
    use crate::filter::{EventFilter, RoomFilter, StateFilter, SyncFilter};
    use serde_json::json;

    #[test]
    fn room_filters_include_and_exclude_rooms() {
        let filter = RoomFilter {
            rooms: Some(vec!["!keep:example.org".into()]),
            not_rooms: Some(vec!["!drop:example.org".into()]),
            ..Default::default()
        };
        assert!(room_is_included("!keep:example.org", Some(&filter)));
        assert!(!room_is_included("!other:example.org", Some(&filter)));
        assert!(!room_is_included("!drop:example.org", Some(&filter)));
    }

    #[test]
    fn event_filters_match_types_senders_and_url_presence() {
        let filter = EventFilter {
            types: Some(vec!["m.room.*".into()]),
            senders: Some(vec!["@alice:example.org".into()]),
            contains_url: Some(true),
            ..Default::default()
        };
        assert!(matches_event_parts(
            "m.room.message",
            "@alice:example.org",
            &json!({"url":"mxc://example.org/id"}),
            &filter
        ));
        assert!(!matches_event_parts(
            "m.room.message",
            "@bob:example.org",
            &json!({"url":"mxc://example.org/id"}),
            &filter
        ));
        assert!(!matches_event_parts(
            "m.room.message",
            "@alice:example.org",
            &json!({}),
            &filter
        ));
        assert!(wildcard_match("m.*.message", "m.room.message"));
        assert!(!wildcard_match("m.*.message", "m.room.encrypted"));
    }

    #[test]
    fn state_filter_limits_types_and_event_fields_are_applied() {
        let state_filter = StateFilter {
            types: Some(vec!["m.room.name".into()]),
            ..Default::default()
        };
        assert!(matches_state_event(&json!({"type":"m.room.name"}), &state_filter));
        assert!(!matches_state_event(&json!({"type":"m.room.member"}), &state_filter));

        let filter = SyncFilter {
            event_fields: Some(vec!["type".into(), "content".into()]),
            ..Default::default()
        };
        assert_eq!(
            apply_event_fields(json!({"type":"m.room.message","sender":"@a:x","content":{}}), Some(&filter)),
            json!({"type":"m.room.message","content":{}})
        );
    }
}

#[cfg(test)]
mod receipt_tests {
    use super::receipt_content;
    use bicerin_storage::filters::RoomReceiptRecord;
    use chrono::Utc;

    fn receipt(user_id: &str, receipt_type: &str, event_id: &str) -> RoomReceiptRecord {
        RoomReceiptRecord {
            room_id: "!room:example.org".into(),
            user_id: user_id.into(),
            receipt_type: receipt_type.into(),
            thread_id: String::new(),
            event_id: event_id.into(),
            event_stream_id: 4,
            stream_id: 5,
            timestamp: Some(123),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn receipt_sync_payload_hides_other_users_private_receipts() {
        let payload = receipt_content(
            vec![
                receipt("@alice:example.org", "m.read.private", "$a:example.org"),
                receipt("@bob:example.org", "m.read.private", "$b:example.org"),
                receipt("@bob:example.org", "m.read", "$c:example.org"),
            ],
            "@alice:example.org",
        )
        .expect("public and own private receipts");

        assert!(payload["$a:example.org"]["m.read.private"].get("@alice:example.org").is_some());
        assert!(payload["$b:example.org"].is_null());
        assert!(payload["$c:example.org"]["m.read"].get("@bob:example.org").is_some());
    }
}
