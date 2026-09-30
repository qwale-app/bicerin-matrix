use bicerin_error::{BicerinError, BicerinResult};
use bicerin_storage::{db::StorageError, Store};
use serde_json::Value;

pub async fn load_power_levels(store: &Store, room_id: &str) -> BicerinResult<Value> {
    match bicerin_storage::rooms::get_room_state(store, room_id, "m.room.power_levels", "").await {
        Ok(state) => Ok(state.content),
        Err(StorageError::NotFound) => Ok(serde_json::json!({})),
        Err(error) => Err(BicerinError::Internal(error.to_string())),
    }
}

pub fn check_power_level(power_levels: &Value, user_id: &str, required: i64) -> bool {
    let users_default = power_levels
        .get("users_default")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let user_level = power_levels
        .get("users")
        .and_then(|u| u.get(user_id))
        .and_then(|v| v.as_i64())
        .unwrap_or(users_default);
    user_level >= required
}

pub fn check_send_event(power_levels: &Value, user_id: &str, event_type: &str) -> bool {
    let events_default = power_levels
        .get("events_default")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let required = power_levels
        .get("events")
        .and_then(|e| e.get(event_type))
        .and_then(|v| v.as_i64())
        .unwrap_or(events_default);
    check_power_level(power_levels, user_id, required)
}

pub fn check_state_event(power_levels: &Value, user_id: &str, event_type: &str) -> bool {
    let state_default = power_levels
        .get("state_default")
        .and_then(|v| v.as_i64())
        .unwrap_or(50);
    let required = power_levels
        .get("events")
        .and_then(|e| e.get(event_type))
        .and_then(|v| v.as_i64())
        .unwrap_or(state_default);
    check_power_level(power_levels, user_id, required)
}

/// Prevents a sender from granting powers they do not currently hold or
/// changing a threshold that is above their current level.
pub fn can_change_power_levels(power_levels: &Value, sender: &str, updated: &Value) -> bool {
    let users_default = power_levels
        .get("users_default")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let sender_level = power_levels
        .get("users")
        .and_then(|users| users.get(sender))
        .and_then(Value::as_i64)
        .unwrap_or(users_default);

    let old_users = power_levels.get("users").and_then(Value::as_object);
    let new_users = updated.get("users").and_then(Value::as_object);
    if let Some(new_users) = new_users {
        for (user_id, new_level) in new_users {
            let Some(new_level) = new_level.as_i64() else {
                return false;
            };
            if new_level > sender_level {
                return false;
            }
            let old_level = old_users
                .and_then(|users| users.get(user_id))
                .and_then(Value::as_i64)
                .unwrap_or(users_default);
            if user_id != sender && old_level >= sender_level && old_level != new_level {
                return false;
            }
        }
    }
    if let Some(old_users) = old_users {
        for (user_id, old_level) in old_users {
            let old_level = old_level.as_i64().unwrap_or(users_default);
            let new_level = new_users
                .and_then(|users| users.get(user_id))
                .and_then(Value::as_i64)
                .unwrap_or(users_default);
            if user_id != sender && old_level >= sender_level && old_level != new_level {
                return false;
            }
        }
    }

    for field in [
        "ban",
        "events_default",
        "invite",
        "kick",
        "redact",
        "state_default",
        "users_default",
    ] {
        if let Some(new_level) = updated.get(field).and_then(Value::as_i64) {
            if new_level > sender_level {
                return false;
            }
            let old_level = power_levels.get(field).and_then(Value::as_i64).unwrap_or(0);
            if old_level > sender_level && old_level != new_level {
                return false;
            }
        } else if power_levels
            .get(field)
            .and_then(Value::as_i64)
            .is_some_and(|old| old > sender_level)
        {
            return false;
        }
    }

    let old_events = power_levels.get("events").and_then(Value::as_object);
    let new_events = updated.get("events").and_then(Value::as_object);
    if let Some(new_events) = new_events {
        for (event_type, new_level) in new_events {
            let Some(new_level) = new_level.as_i64() else {
                return false;
            };
            if new_level > sender_level {
                return false;
            }
            let old_level = old_events
                .and_then(|events| events.get(event_type))
                .and_then(Value::as_i64)
                .unwrap_or(0);
            if old_level > sender_level && old_level != new_level {
                return false;
            }
        }
    }
    if let Some(old_events) = old_events {
        for (event_type, old_level) in old_events {
            if old_level.as_i64().is_some_and(|old| old > sender_level) {
                let new_level = new_events
                    .and_then(|events| events.get(event_type))
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                if old_level.as_i64() != Some(new_level) {
                    return false;
                }
            }
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::{can_change_power_levels, check_send_event, check_state_event};
    use serde_json::json;

    #[test]
    fn default_power_levels_allow_messages_but_reserve_state_for_moderators() {
        let levels = json!({
            "users": { "@moderator:example.org": 50 },
            "users_default": 0,
            "events_default": 0,
            "state_default": 50
        });
        assert!(check_send_event(
            &levels,
            "@member:example.org",
            "m.room.message"
        ));
        assert!(!check_state_event(
            &levels,
            "@member:example.org",
            "m.room.name"
        ));
        assert!(check_state_event(
            &levels,
            "@moderator:example.org",
            "m.room.name"
        ));
    }

    #[test]
    fn event_specific_levels_override_the_default() {
        let levels = json!({
            "users": { "@member:example.org": 50 },
            "users_default": 0,
            "events_default": 0,
            "state_default": 50,
            "events": { "m.room.message": 60, "m.room.name": 40 }
        });
        assert!(!check_send_event(
            &levels,
            "@member:example.org",
            "m.room.message"
        ));
        assert!(check_state_event(
            &levels,
            "@member:example.org",
            "m.room.name"
        ));
    }

    #[test]
    fn power_level_updates_cannot_grant_more_than_the_sender_holds() {
        let levels = json!({ "users": { "@mod:example.org": 50 }, "users_default": 0 });
        assert!(can_change_power_levels(
            &levels,
            "@mod:example.org",
            &json!({ "users": { "@mod:example.org": 50, "@member:example.org": 40 } })
        ));
        assert!(!can_change_power_levels(
            &levels,
            "@mod:example.org",
            &json!({ "users": { "@member:example.org": 100 } })
        ));
    }
}
