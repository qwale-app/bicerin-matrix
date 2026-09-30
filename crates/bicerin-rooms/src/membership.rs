use crate::service::RoomService;
use bicerin_storage::{rooms::*, events::*};
use bicerin_error::{BicerinError, BicerinResult};
use serde_json::json;

impl RoomService {
    pub async fn join_room(
        &self,
        room_id: &str,
        user_id: &str,
        server_name: &str,
        stream_id: i64,
    ) -> BicerinResult<String> {
        bicerin_storage::rooms::get_room(&self.store, room_id)
            .await
            .map_err(|_| BicerinError::NotFound)?;

        let event_id = format!("${}:{}", uuid::Uuid::new_v4().to_string().replace('-', ""), server_name);
        let now_ts = chrono::Utc::now().timestamp_millis();

        let event = EventRecord {
            event_id: event_id.clone(),
            room_id: room_id.to_string(),
            sender: user_id.to_string(),
            stream_id,
            origin_server_ts: now_ts,
            event_type: "m.room.member".to_string(),
            state_key: Some(user_id.to_string()),
            room_version: bicerin_storage::rooms::get_room(&self.store, room_id)
                .await.map_err(|e| BicerinError::Internal(e.to_string()))?
                .room_version,
            content: json!({"membership": "join"}),
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

        bicerin_storage::rooms::upsert_room_member(&self.store, &RoomMemberRecord {
            room_id: room_id.to_string(),
            user_id: user_id.to_string(),
            membership: "join".to_string(),
            display_name: None,
            avatar_url: None,
            sender: user_id.to_string(),
            event_id: event_id.clone(),
            stream_id,
            updated_at: chrono::Utc::now(),
        }).await.map_err(|e| BicerinError::Internal(e.to_string()))?;

        bicerin_storage::rooms::upsert_room_state(&self.store, &RoomStateRecord {
            room_id: room_id.to_string(),
            event_type: "m.room.member".to_string(),
            state_key: user_id.to_string(),
            event_id: event_id.clone(),
            content: event.content.clone(),
            sender: user_id.to_string(),
            stream_id,
            updated_at: chrono::Utc::now(),
        }).await.map_err(|e| BicerinError::Internal(e.to_string()))?;

        Ok(event_id)
    }

    pub async fn leave_room(
        &self,
        room_id: &str,
        user_id: &str,
        server_name: &str,
        stream_id: i64,
    ) -> BicerinResult<String> {
        let room = bicerin_storage::rooms::get_room(&self.store, room_id)
            .await
            .map_err(|_| BicerinError::NotFound)?;

        let event_id = format!("${}:{}", uuid::Uuid::new_v4().to_string().replace('-', ""), server_name);
        let now_ts = chrono::Utc::now().timestamp_millis();

        let event = EventRecord {
            event_id: event_id.clone(),
            room_id: room_id.to_string(),
            sender: user_id.to_string(),
            stream_id,
            origin_server_ts: now_ts,
            event_type: "m.room.member".to_string(),
            state_key: Some(user_id.to_string()),
            room_version: room.room_version,
            content: json!({"membership": "leave"}),
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

        bicerin_storage::rooms::upsert_room_member(&self.store, &RoomMemberRecord {
            room_id: room_id.to_string(),
            user_id: user_id.to_string(),
            membership: "leave".to_string(),
            display_name: None,
            avatar_url: None,
            sender: user_id.to_string(),
            event_id: event_id.clone(),
            stream_id,
            updated_at: chrono::Utc::now(),
        }).await.map_err(|e| BicerinError::Internal(e.to_string()))?;

        bicerin_storage::rooms::upsert_room_state(&self.store, &RoomStateRecord {
            room_id: room_id.to_string(),
            event_type: "m.room.member".to_string(),
            state_key: user_id.to_string(),
            event_id: event_id.clone(),
            content: event.content.clone(),
            sender: user_id.to_string(),
            stream_id,
            updated_at: chrono::Utc::now(),
        }).await.map_err(|e| BicerinError::Internal(e.to_string()))?;

        Ok(event_id)
    }

    pub async fn invite_user(
        &self,
        room_id: &str,
        inviter_id: &str,
        invitee_id: &str,
        server_name: &str,
        stream_id: i64,
    ) -> BicerinResult<String> {
        let room = bicerin_storage::rooms::get_room(&self.store, room_id)
            .await
            .map_err(|_| BicerinError::NotFound)?;

        let inviter_member = bicerin_storage::rooms::get_room_member(&self.store, room_id, inviter_id)
            .await
            .map_err(|_| BicerinError::Forbidden)?;
        if inviter_member.membership != "join" {
            return Err(BicerinError::Forbidden);
        }

        let event_id = format!("${}:{}", uuid::Uuid::new_v4().to_string().replace('-', ""), server_name);
        let now_ts = chrono::Utc::now().timestamp_millis();

        let event = EventRecord {
            event_id: event_id.clone(),
            room_id: room_id.to_string(),
            sender: inviter_id.to_string(),
            stream_id,
            origin_server_ts: now_ts,
            event_type: "m.room.member".to_string(),
            state_key: Some(invitee_id.to_string()),
            room_version: room.room_version,
            content: json!({"membership": "invite"}),
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

        bicerin_storage::rooms::upsert_room_member(&self.store, &RoomMemberRecord {
            room_id: room_id.to_string(),
            user_id: invitee_id.to_string(),
            membership: "invite".to_string(),
            display_name: None,
            avatar_url: None,
            sender: inviter_id.to_string(),
            event_id: event_id.clone(),
            stream_id,
            updated_at: chrono::Utc::now(),
        }).await.map_err(|e| BicerinError::Internal(e.to_string()))?;

        bicerin_storage::rooms::upsert_room_state(&self.store, &RoomStateRecord {
            room_id: room_id.to_string(),
            event_type: "m.room.member".to_string(),
            state_key: invitee_id.to_string(),
            event_id: event_id.clone(),
            content: event.content.clone(),
            sender: inviter_id.to_string(),
            stream_id,
            updated_at: chrono::Utc::now(),
        }).await.map_err(|e| BicerinError::Internal(e.to_string()))?;

        Ok(event_id)
    }
}
