use bicerin_storage::events::*;
use bicerin_error::{BicerinError, BicerinResult};
use serde_json::Value;

pub struct EventService {
    pub store: bicerin_storage::Store,
    pub server_name: String,
}

impl EventService {
    pub fn new(store: bicerin_storage::Store, server_name: String) -> Self { Self { store, server_name } }

    pub async fn send_event(
        &self,
        room_id: &str,
        sender: &str,
        event_type: &str,
        content: Value,
        _txn_id: Option<&str>,
    ) -> BicerinResult<(String, i64)> {
        let member = bicerin_storage::rooms::get_room_member(&self.store, room_id, sender)
            .await
            .map_err(|_| BicerinError::Forbidden)?;
        if member.membership != "join" {
            return Err(BicerinError::Forbidden);
        }

        let room = bicerin_storage::rooms::get_room(&self.store, room_id)
            .await
            .map_err(|_| BicerinError::NotFound)?;

        let stream_id = bicerin_storage::events::get_next_stream_id(&self.store)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;

        let event_id = format!("${}:{}", uuid::Uuid::new_v4().to_string().replace('-', ""), self.server_name);

        let event = EventRecord {
            event_id: event_id.clone(),
            room_id: room_id.to_string(),
            sender: sender.to_string(),
            stream_id,
            origin_server_ts: chrono::Utc::now().timestamp_millis(),
            event_type: event_type.to_string(),
            state_key: None,
            room_version: room.room_version,
            content,
            unsigned: None,
            redacts: None,
            depth: 0,
            auth_events: vec![],
            prev_events: vec![],
            created_at: chrono::Utc::now(),
        };

        bicerin_storage::events::insert_event(&self.store, &event)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;

        crate::relations::index_relations(&self.store, &event).await;
        crate::appservice_dispatch::dispatch(&self.store, &self.server_name, &event).await;

        tracing::debug!(event_id = %event_id, room_id = %room_id, event_type = %event_type, stream_id = stream_id, "event persisted");

        Ok((event_id, stream_id))
    }

    pub async fn send_state_event(
        &self,
        room_id: &str,
        sender: &str,
        event_type: &str,
        state_key: &str,
        content: Value,
    ) -> BicerinResult<(String, i64)> {
        let member = bicerin_storage::rooms::get_room_member(&self.store, room_id, sender)
            .await
            .map_err(|_| BicerinError::Forbidden)?;
        if member.membership != "join" {
            return Err(BicerinError::Forbidden);
        }

        let room = bicerin_storage::rooms::get_room(&self.store, room_id)
            .await
            .map_err(|_| BicerinError::NotFound)?;

        let stream_id = bicerin_storage::events::get_next_stream_id(&self.store)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;

        let event_id = format!("${}:{}", uuid::Uuid::new_v4().to_string().replace('-', ""), self.server_name);

        let event = EventRecord {
            event_id: event_id.clone(),
            room_id: room_id.to_string(),
            sender: sender.to_string(),
            stream_id,
            origin_server_ts: chrono::Utc::now().timestamp_millis(),
            event_type: event_type.to_string(),
            state_key: Some(state_key.to_string()),
            room_version: room.room_version,
            content: content.clone(),
            unsigned: None,
            redacts: None,
            depth: 0,
            auth_events: vec![],
            prev_events: vec![],
            created_at: chrono::Utc::now(),
        };

        bicerin_storage::events::insert_event(&self.store, &event)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;

        bicerin_storage::rooms::upsert_room_state(&self.store, &bicerin_storage::rooms::RoomStateRecord {
            room_id: room_id.to_string(),
            event_type: event_type.to_string(),
            state_key: state_key.to_string(),
            event_id: event_id.clone(),
            content,
            sender: sender.to_string(),
            stream_id,
            updated_at: chrono::Utc::now(),
        }).await.map_err(|e| BicerinError::Internal(e.to_string()))?;

        crate::appservice_dispatch::dispatch(&self.store, &self.server_name, &event).await;

        Ok((event_id, stream_id))
    }

    pub async fn get_room_messages(
        &self,
        room_id: &str,
        from: Option<i64>,
        _to: Option<i64>,
        dir: &str,
        limit: i64,
    ) -> BicerinResult<(Vec<EventRecord>, Option<i64>)> {
        let events = bicerin_storage::events::get_events_in_room(
            &self.store,
            room_id,
            from.unwrap_or(i64::MAX),
            limit,
            dir,
        ).await.map_err(|e| BicerinError::Internal(e.to_string()))?;

        let end_token = events.last().map(|e| e.stream_id);
        Ok((events, end_token))
    }
}
