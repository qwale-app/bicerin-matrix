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
            if member.membership == "ban" {
                return Err(BicerinError::Forbidden);
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

        if matches!(
            bicerin_storage::rooms::get_room_member(&self.store, room_id, invitee_id).await,
            Ok(ref member) if member.membership == "ban"
        ) {
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

impl RoomService {
    pub async fn kick_user(
        &self,
        room_id: &str,
        sender_id: &str,
        target_user_id: &str,
        reason: Option<&str>,
        server_name: &str,
        stream_id: i64,
    ) -> BicerinResult<String> {
        self.moderate_membership(
            room_id, sender_id, target_user_id, "leave", "kick", reason, server_name, stream_id,
        )
        .await
    }

    pub async fn ban_user(
        &self,
        room_id: &str,
        sender_id: &str,
        target_user_id: &str,
        reason: Option<&str>,
        server_name: &str,
        stream_id: i64,
    ) -> BicerinResult<String> {
        self.moderate_membership(
            room_id, sender_id, target_user_id, "ban", "ban", reason, server_name, stream_id,
        )
        .await
    }

    pub async fn unban_user(
        &self,
        room_id: &str,
        sender_id: &str,
        target_user_id: &str,
        reason: Option<&str>,
        server_name: &str,
        stream_id: i64,
    ) -> BicerinResult<String> {
        self.moderate_membership(
            room_id, sender_id, target_user_id, "leave", "ban", reason, server_name, stream_id,
        )
        .await
    }

    async fn moderate_membership(
        &self,
        room_id: &str,
        sender_id: &str,
        target_user_id: &str,
        membership: &str,
        power_field: &str,
        reason: Option<&str>,
        server_name: &str,
        stream_id: i64,
    ) -> BicerinResult<String> {
        if sender_id == target_user_id {
            return Err(BicerinError::Forbidden);
        }
        let room = bicerin_storage::rooms::get_room(&self.store, room_id)
            .await
            .map_err(|_| BicerinError::NotFound)?;
        let sender = bicerin_storage::rooms::get_room_member(&self.store, room_id, sender_id)
            .await
            .map_err(|_| BicerinError::Forbidden)?;
        if sender.membership != "join" {
            return Err(BicerinError::Forbidden);
        }
        let target = bicerin_storage::rooms::get_room_member(&self.store, room_id, target_user_id)
            .await
            .map_err(|_| BicerinError::NotFound)?;
        match (power_field, target.membership.as_str(), membership) {
            ("kick", "join" | "invite", "leave") => {}
            ("ban", "join" | "invite", "ban") => {}
            ("ban", "ban", "leave") => {}
            _ => return Err(BicerinError::Forbidden),
        }

        let power_levels = crate::powerlevels::load_power_levels(&self.store, room_id).await?;
        let required = moderation_required_level(
            &power_levels,
            power_field,
            &target.membership,
            membership,
        );
        let sender_level = crate::powerlevels::user_power_level(&power_levels, sender_id);
        let target_level = crate::powerlevels::user_power_level(&power_levels, target_user_id);
        if sender_level < required || sender_level <= target_level {
            return Err(BicerinError::Forbidden);
        }

        let event_id = format!("${}:{}", uuid::Uuid::new_v4().simple(), server_name);
        let mut content = json!({"membership": membership});
        if let Some(reason) = reason.filter(|reason| !reason.is_empty()) {
            content["reason"] = json!(reason);
        }
        let now = chrono::Utc::now();
        let event = EventRecord {
            event_id: event_id.clone(),
            room_id: room_id.to_string(),
            sender: sender_id.to_string(),
            stream_id,
            origin_server_ts: now.timestamp_millis(),
            event_type: "m.room.member".to_string(),
            state_key: Some(target_user_id.to_string()),
            room_version: room.room_version,
            content: content.clone(),
            unsigned: None,
            redacts: None,
            depth: 0,
            auth_events: vec![],
            prev_events: vec![],
            created_at: now,
        };
        bicerin_storage::events::insert_event(&self.store, &event)
            .await
            .map_err(|error| BicerinError::Internal(error.to_string()))?;
        bicerin_storage::rooms::upsert_room_member(
            &self.store,
            &RoomMemberRecord {
                room_id: room_id.to_string(),
                user_id: target_user_id.to_string(),
                membership: membership.to_string(),
                display_name: target.display_name,
                avatar_url: target.avatar_url,
                sender: sender_id.to_string(),
                event_id: event_id.clone(),
                stream_id,
                updated_at: now,
            },
        )
        .await
        .map_err(|error| BicerinError::Internal(error.to_string()))?;
        bicerin_storage::rooms::upsert_room_state(
            &self.store,
            &RoomStateRecord {
                room_id: room_id.to_string(),
                event_type: "m.room.member".to_string(),
                state_key: target_user_id.to_string(),
                event_id: event_id.clone(),
                content,
                sender: sender_id.to_string(),
                stream_id,
                updated_at: now,
            },
        )
        .await
        .map_err(|error| BicerinError::Internal(error.to_string()))?;
        Ok(event_id)
    }
}

fn moderation_required_level(
    power_levels: &serde_json::Value,
    power_field: &str,
    current_membership: &str,
    requested_membership: &str,
) -> i64 {
    let field_level = |field: &str| {
        power_levels
            .get(field)
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(50)
    };

    let required = field_level(power_field);
    if current_membership == "ban" && requested_membership == "leave" {
        required.max(field_level("kick"))
    } else {
        required
    }
}

#[cfg(test)]
mod tests {
    use super::moderation_required_level;
    use serde_json::json;

    #[test]
    fn unbanning_requires_both_ban_and_kick_levels() {
        let levels = json!({"ban": 75, "kick": 50});
        assert_eq!(moderation_required_level(&levels, "ban", "ban", "leave"), 75);
        assert_eq!(moderation_required_level(&levels, "ban", "join", "ban"), 75);
        assert_eq!(moderation_required_level(&levels, "kick", "join", "leave"), 50);
    }
}
