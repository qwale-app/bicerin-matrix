use crate::service::RoomService;
use bicerin_storage::{rooms::*, events::*};
use bicerin_error::BicerinResult;
use serde_json::{json, Value};

#[derive(Debug, serde::Deserialize)]
pub struct CreateRoomParams {
    pub creator: String,
    pub room_version: String,
    pub name: Option<String>,
    pub topic: Option<String>,
    pub is_direct: bool,
    pub initial_state: Vec<InitialStateEvent>,
    pub invite: Vec<String>,
    pub preset: Option<String>,
    pub room_alias_name: Option<String>,
    pub power_level_content_override: Option<Value>,
}

#[derive(Debug, serde::Deserialize)]
pub struct InitialStateEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub state_key: Option<String>,
    pub content: Value,
}

impl RoomService {
    pub async fn create_room(
        &self,
        params: CreateRoomParams,
        server_name: &str,
    ) -> BicerinResult<(String, Vec<String>)> {
        let room_id = format!("!{}:{}", uuid::Uuid::new_v4().to_string().replace('-', ""), server_name);
        let now_ts = chrono::Utc::now().timestamp_millis();

        // Bridges commonly pass `m.room.encryption` via `initial_state` to
        // create encrypted portals; reflect that in the room record.
        let is_encrypted = params
            .initial_state
            .iter()
            .any(|s| s.event_type == "m.room.encryption");

        let room_record = RoomRecord {
            room_id: room_id.clone(),
            creator: params.creator.clone(),
            room_version: params.room_version.clone(),
            is_encrypted,
            is_direct: params.is_direct,
            name: params.name.clone(),
            topic: params.topic.clone(),
            canonical_alias: None,
            creation_ts: now_ts,
            created_at: chrono::Utc::now(),
        };

        bicerin_storage::rooms::create_room(&self.store, &room_record)
            .await
            .map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;

        let mut event_ids = vec![];
        let creation_event_id = format!("${}:{}", uuid::Uuid::new_v4().to_string().replace('-', ""), server_name);
        let power_levels_event_id = format!("${}:{}", uuid::Uuid::new_v4().to_string().replace('-', ""), server_name);
        let join_rules_event_id = format!("${}:{}", uuid::Uuid::new_v4().to_string().replace('-', ""), server_name);
        let member_event_id = format!("${}:{}", uuid::Uuid::new_v4().to_string().replace('-', ""), server_name);

        let (join_rule, _history_vis, _guest_access, pl_state_default, pl_events_default) = match params.preset.as_deref() {
            Some("public_chat") => ("public", "shared", "forbidden", 50, 0),
            Some("trusted_private_chat") => ("invite", "shared", "forbidden", 0, 0),
            _ => ("invite", "shared", "forbidden", 50, 0),
        };

        let power_levels_content = json!({
            "ban": 50,
            "events": {},
            "events_default": pl_events_default,
            "invite": 0,
            "kick": 50,
            "notifications": {"room": 50},
            "redact": 50,
            "state_default": pl_state_default,
            "users": {&params.creator: 100},
            "users_default": 0
        });
        let power_levels_content = merge_power_level_override(power_levels_content, params.power_level_content_override.as_ref());

        // Creation event
        let creation_stream_id = bicerin_storage::events::get_next_stream_id(&self.store)
            .await
            .map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;
        let creation_event = EventRecord {
            event_id: creation_event_id.clone(),
            room_id: room_id.clone(),
            sender: params.creator.clone(),
            stream_id: creation_stream_id,
            origin_server_ts: now_ts,
            event_type: "m.room.create".to_string(),
            state_key: Some("".to_string()),
            room_version: params.room_version.clone(),
            content: json!({"creator": params.creator, "room_version": params.room_version, "m.federate": false}),
            unsigned: None,
            redacts: None,
            depth: 1,
            auth_events: vec![],
            prev_events: vec![],
            created_at: chrono::Utc::now(),
        };
        bicerin_storage::events::insert_event(&self.store, &creation_event)
            .await
            .map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;
        bicerin_storage::rooms::upsert_room_state(&self.store, &bicerin_storage::rooms::RoomStateRecord {
            room_id: room_id.clone(), event_type: "m.room.create".to_string(), state_key: "".to_string(),
            event_id: creation_event_id.clone(), content: creation_event.content.clone(),
            sender: params.creator.clone(), stream_id: creation_stream_id, updated_at: chrono::Utc::now(),
        }).await.map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;
        event_ids.push(creation_event_id.clone());

        // Member event
        let member_stream_id = bicerin_storage::events::get_next_stream_id(&self.store)
            .await
            .map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;
        let join_event = EventRecord {
            event_id: member_event_id.clone(),
            room_id: room_id.clone(),
            sender: params.creator.clone(),
            stream_id: member_stream_id,
            origin_server_ts: now_ts,
            event_type: "m.room.member".to_string(),
            state_key: Some(params.creator.clone()),
            room_version: params.room_version.clone(),
            content: json!({"membership": "join", "displayname": null}),
            unsigned: None,
            redacts: None,
            depth: 2,
            auth_events: vec![creation_event_id.clone()],
            prev_events: vec![creation_event_id.clone()],
            created_at: chrono::Utc::now(),
        };
        bicerin_storage::events::insert_event(&self.store, &join_event)
            .await
            .map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;
        bicerin_storage::rooms::upsert_room_member(&self.store, &RoomMemberRecord {
            room_id: room_id.clone(), user_id: params.creator.clone(), membership: "join".to_string(),
            display_name: None, avatar_url: None, sender: params.creator.clone(),
            event_id: member_event_id.clone(), stream_id: member_stream_id, updated_at: chrono::Utc::now(),
        }).await.map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;
        bicerin_storage::rooms::upsert_room_state(&self.store, &bicerin_storage::rooms::RoomStateRecord {
            room_id: room_id.clone(), event_type: "m.room.member".to_string(), state_key: params.creator.clone(),
            event_id: member_event_id.clone(), content: join_event.content.clone(),
            sender: params.creator.clone(), stream_id: member_stream_id, updated_at: chrono::Utc::now(),
        }).await.map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;
        event_ids.push(member_event_id);

        // Power levels event
        let power_levels_stream_id = bicerin_storage::events::get_next_stream_id(&self.store)
            .await
            .map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;
        let pl_event = EventRecord {
            event_id: power_levels_event_id.clone(),
            room_id: room_id.clone(),
            sender: params.creator.clone(),
            stream_id: power_levels_stream_id,
            origin_server_ts: now_ts,
            event_type: "m.room.power_levels".to_string(),
            state_key: Some("".to_string()),
            room_version: params.room_version.clone(),
            content: power_levels_content.clone(),
            unsigned: None,
            redacts: None,
            depth: 3,
            auth_events: vec![creation_event_id.clone()],
            prev_events: vec![creation_event_id.clone()],
            created_at: chrono::Utc::now(),
        };
        bicerin_storage::events::insert_event(&self.store, &pl_event)
            .await
            .map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;
        bicerin_storage::rooms::upsert_room_state(&self.store, &bicerin_storage::rooms::RoomStateRecord {
            room_id: room_id.clone(), event_type: "m.room.power_levels".to_string(), state_key: "".to_string(),
            event_id: power_levels_event_id.clone(), content: power_levels_content,
            sender: params.creator.clone(), stream_id: power_levels_stream_id, updated_at: chrono::Utc::now(),
        }).await.map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;
        event_ids.push(power_levels_event_id);

        // Join rules event
        let join_rules_stream_id = bicerin_storage::events::get_next_stream_id(&self.store)
            .await
            .map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;
        let jr_event = EventRecord {
            event_id: join_rules_event_id.clone(),
            room_id: room_id.clone(),
            sender: params.creator.clone(),
            stream_id: join_rules_stream_id,
            origin_server_ts: now_ts,
            event_type: "m.room.join_rules".to_string(),
            state_key: Some("".to_string()),
            room_version: params.room_version.clone(),
            content: json!({"join_rule": join_rule}),
            unsigned: None,
            redacts: None,
            depth: 4,
            auth_events: vec![creation_event_id.clone()],
            prev_events: vec![creation_event_id.clone()],
            created_at: chrono::Utc::now(),
        };
        bicerin_storage::events::insert_event(&self.store, &jr_event)
            .await
            .map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;
        bicerin_storage::rooms::upsert_room_state(&self.store, &bicerin_storage::rooms::RoomStateRecord {
            room_id: room_id.clone(), event_type: "m.room.join_rules".to_string(), state_key: "".to_string(),
            event_id: join_rules_event_id.clone(), content: jr_event.content.clone(),
            sender: params.creator.clone(), stream_id: join_rules_stream_id, updated_at: chrono::Utc::now(),
        }).await.map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;
        event_ids.push(join_rules_event_id);

        // Client-supplied `initial_state` (e.g. `m.room.encryption`,
        // `m.bridge`/`uk.half-shot.bridge` bridge-info state used by mautrix
        // bridges and clients). Applied as real state events layered on top
        // of the defaults above, so a client can also override
        // join_rules/history_visibility/etc. `m.room.create` can't be
        // overridden this way.
        let mut depth = 5i64;
        for state_event in params.initial_state {
            if state_event.event_type == "m.room.create" {
                continue;
            }
            let state_key = state_event.state_key.unwrap_or_default();
            let stream_id = bicerin_storage::events::get_next_stream_id(&self.store)
                .await
                .map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;
            let event_id = format!("${}:{}", uuid::Uuid::new_v4().to_string().replace('-', ""), server_name);
            let event = EventRecord {
                event_id: event_id.clone(),
                room_id: room_id.clone(),
                sender: params.creator.clone(),
                stream_id,
                origin_server_ts: now_ts,
                event_type: state_event.event_type.clone(),
                state_key: Some(state_key.clone()),
                room_version: params.room_version.clone(),
                content: state_event.content.clone(),
                unsigned: None,
                redacts: None,
                depth,
                auth_events: vec![creation_event_id.clone()],
                prev_events: vec![creation_event_id.clone()],
                created_at: chrono::Utc::now(),
            };
            depth += 1;
            bicerin_storage::events::insert_event(&self.store, &event)
                .await
                .map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;
            bicerin_storage::rooms::upsert_room_state(&self.store, &bicerin_storage::rooms::RoomStateRecord {
                room_id: room_id.clone(), event_type: state_event.event_type, state_key,
                event_id: event_id.clone(), content: state_event.content,
                sender: params.creator.clone(), stream_id, updated_at: chrono::Utc::now(),
            }).await.map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;
            event_ids.push(event_id);
        }

        Ok((room_id, event_ids))
    }
}

/// Shallow top-level merge of `power_level_content_override` into the
/// computed default power-levels content, per the Matrix `createRoom` spec.
fn merge_power_level_override(mut base: Value, override_content: Option<&Value>) -> Value {
    let Some(Value::Object(override_map)) = override_content else {
        return base;
    };
    if let Value::Object(base_map) = &mut base {
        for (key, value) in override_map {
            base_map.insert(key.clone(), value.clone());
        }
    }
    base
}
