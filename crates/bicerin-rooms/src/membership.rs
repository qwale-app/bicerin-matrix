use crate::service::RoomService;
use bicerin_error::{BicerinError, BicerinResult};
use bicerin_storage::{db::StorageError, events::*, rooms::*};
use serde_json::json;

impl RoomService {
    pub async fn join_room(
        &self,
        room_id: &str,
        user_id: &str,
        server_name: &str,
        stream_id: i64,
    ) -> BicerinResult<String> {
        let room = bicerin_storage::rooms::get_room(&self.store, room_id)
            .await
            .map_err(|_| BicerinError::NotFound)?;

        let existing_membership =
            match bicerin_storage::rooms::get_room_member(&self.store, room_id, user_id).await {
                Ok(member) => Some(member),
                Err(StorageError::NotFound) => None,
                Err(error) => return Err(BicerinError::Internal(error.to_string())),
            };
        if let Some(member) = &existing_membership {
            if member.membership == "join" {
                return Ok(member.event_id.clone());
            }
        }

        let is_public_join = match bicerin_storage::rooms::get_room_state(
            &self.store,
            room_id,
            "m.room.join_rules",
            "",
        )
        .await
        {
            Ok(state) => {
                state
                    .content
                    .get("join_rule")
                    .and_then(|rule| rule.as_str())
                    == Some("public")
            }
            Err(StorageError::NotFound) => false,
            Err(error) => return Err(BicerinError::Internal(error.to_string())),
        };
        let has_invite = existing_membership
            .as_ref()
            .is_some_and(|member| member.membership == "invite");
        if !is_public_join && !has_invite {
            return Err(BicerinError::Forbidden);
        }

        let event_id = format!(
            "${}:{}",
            uuid::Uuid::new_v4().to_string().replace('-', ""),
            server_name
        );
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

        bicerin_storage::rooms::upsert_room_member(
            &self.store,
            &RoomMemberRecord {
                room_id: room_id.to_string(),
                user_id: user_id.to_string(),
                membership: "join".to_string(),
                display_name: None,
                avatar_url: None,
                sender: user_id.to_string(),
                event_id: event_id.clone(),
                stream_id,
                updated_at: chrono::Utc::now(),
            },
        )
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;

        bicerin_storage::rooms::upsert_room_state(
            &self.store,
            &RoomStateRecord {
                room_id: room_id.to_string(),
                event_type: "m.room.member".to_string(),
                state_key: user_id.to_string(),
                event_id: event_id.clone(),
                content: event.content.clone(),
                sender: user_id.to_string(),
                stream_id,
                updated_at: chrono::Utc::now(),
            },
        )
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;

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

        let current_member =
            match bicerin_storage::rooms::get_room_member(&self.store, room_id, user_id).await {
                Ok(member) => Some(member),
                Err(StorageError::NotFound) => None,
                Err(error) => return Err(BicerinError::Internal(error.to_string())),
            };
        if let Some(member) = &current_member {
            if member.membership == "leave" {
                return Ok(member.event_id.clone());
            }
            if !matches!(member.membership.as_str(), "join" | "invite") {
                return Err(BicerinError::Forbidden);
            }
        } else {
            return Err(BicerinError::Forbidden);
        }

        let event_id = format!(
            "${}:{}",
            uuid::Uuid::new_v4().to_string().replace('-', ""),
            server_name
        );
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

        bicerin_storage::rooms::upsert_room_member(
            &self.store,
            &RoomMemberRecord {
                room_id: room_id.to_string(),
                user_id: user_id.to_string(),
                membership: "leave".to_string(),
                display_name: None,
                avatar_url: None,
                sender: user_id.to_string(),
                event_id: event_id.clone(),
                stream_id,
                updated_at: chrono::Utc::now(),
            },
        )
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;

        bicerin_storage::rooms::upsert_room_state(
            &self.store,
            &RoomStateRecord {
                room_id: room_id.to_string(),
                event_type: "m.room.member".to_string(),
                state_key: user_id.to_string(),
                event_id: event_id.clone(),
                content: event.content.clone(),
                sender: user_id.to_string(),
                stream_id,
                updated_at: chrono::Utc::now(),
            },
        )
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;

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

        let inviter_member =
            bicerin_storage::rooms::get_room_member(&self.store, room_id, inviter_id)
                .await
                .map_err(|_| BicerinError::Forbidden)?;
        if inviter_member.membership != "join" {
            return Err(BicerinError::Forbidden);
        }

        let power_levels = crate::powerlevels::load_power_levels(&self.store, room_id).await?;
        let required_level = power_levels
            .get("invite")
            .and_then(|value| value.as_i64())
            .unwrap_or(0);
        if !crate::powerlevels::check_power_level(&power_levels, inviter_id, required_level) {
            return Err(BicerinError::Forbidden);
        }

        match bicerin_storage::rooms::get_room_member(&self.store, room_id, invitee_id).await {
            Ok(member) if member.membership == "invite" => return Ok(member.event_id),
            Ok(member) if member.membership == "join" => return Err(BicerinError::Forbidden),
            Ok(_) | Err(StorageError::NotFound) => {}
            Err(error) => return Err(BicerinError::Internal(error.to_string())),
        }

        let event_id = format!(
            "${}:{}",
            uuid::Uuid::new_v4().to_string().replace('-', ""),
            server_name
        );
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

        bicerin_storage::rooms::upsert_room_member(
            &self.store,
            &RoomMemberRecord {
                room_id: room_id.to_string(),
                user_id: invitee_id.to_string(),
                membership: "invite".to_string(),
                display_name: None,
                avatar_url: None,
                sender: inviter_id.to_string(),
                event_id: event_id.clone(),
                stream_id,
                updated_at: chrono::Utc::now(),
            },
        )
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;

        bicerin_storage::rooms::upsert_room_state(
            &self.store,
            &RoomStateRecord {
                room_id: room_id.to_string(),
                event_type: "m.room.member".to_string(),
                state_key: invitee_id.to_string(),
                event_id: event_id.clone(),
                content: event.content.clone(),
                sender: inviter_id.to_string(),
                stream_id,
                updated_at: chrono::Utc::now(),
            },
        )
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;

        Ok(event_id)
    }
}
