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

pub fn user_power_level(power_levels: &Value, user_id: &str) -> i64 {
    let users_default = power_levels
        .get("users_default")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    power_levels
        .get("users")
        .and_then(Value::as_object)
        .and_then(|users| users.get(user_id))
        .and_then(Value::as_i64)
        .unwrap_or(users_default)
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
    if !valid_power_level_content(power_levels) || !valid_power_level_content(updated) {
        return false;
    }

    let Some(old_users_default) = integer_field(power_levels, "users_default", 0) else {
        return false;
    };
    let Some(new_users_default) = integer_field(updated, "users_default", 0) else {
        return false;
    };
    let Some(sender_level) = power_levels
        .get("users")
        .and_then(Value::as_object)
        .and_then(|users| users.get(sender))
        .and_then(Value::as_i64)
        .or(Some(old_users_default))
    else {
        return false;
    };

    let Some(old_users) = object_field(power_levels, "users") else {
        return false;
    };
    let Some(new_users) = object_field(updated, "users") else {
        return false;
    };
    if !can_change_map_levels(
        old_users,
        new_users,
        old_users_default,
        new_users_default,
        sender,
        sender_level,
        true,
    ) {
        return false;
    }

    for (field, default) in [
        ("ban", 50),
        ("events_default", 0),
        ("invite", 0),
        ("kick", 50),
        ("redact", 50),
        ("state_default", 50),
        ("users_default", 0),
    ] {
        let Some(old_level) = integer_field(power_levels, field, default) else {
            return false;
        };
        let Some(new_level) = integer_field(updated, field, default) else {
            return false;
        };
        if old_level != new_level
            && (new_level > sender_level || old_level > sender_level)
        {
            return false;
        }
    }

    let Some(old_events_default) = integer_field(power_levels, "events_default", 0) else {
        return false;
    };
    let Some(new_events_default) = integer_field(updated, "events_default", 0) else {
        return false;
    };
    let Some(old_events) = object_field(power_levels, "events") else {
        return false;
    };
    let Some(new_events) = object_field(updated, "events") else {
        return false;
    };
    if !can_change_map_levels(
        old_events,
        new_events,
        old_events_default,
        new_events_default,
        sender,
        sender_level,
        false,
    ) {
        return false;
    }

    let Some(old_notifications) = object_field(power_levels, "notifications") else {
        return false;
    };
    let Some(new_notifications) = object_field(updated, "notifications") else {
        return false;
    };
    can_change_map_levels(
        old_notifications,
        new_notifications,
        50,
        50,
        sender,
        sender_level,
        false,
    )
}

fn valid_power_level_content(content: &Value) -> bool {
    const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

    let is_safe_integer = |value: &Value| {
        value
            .as_i64()
            .is_some_and(|number| (-MAX_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(&number))
    };

    [
        "ban",
        "events_default",
        "invite",
        "kick",
        "redact",
        "state_default",
        "users_default",
    ]
    .iter()
    .all(|field| content.get(*field).is_none_or(&is_safe_integer))
        && ["events", "notifications", "users"].iter().all(|field| {
            content.get(*field).is_none_or(|value| {
                value.as_object().is_some_and(|entries| {
                    entries.values().all(&is_safe_integer)
                })
            })
        })
}

fn integer_field(content: &Value, field: &str, default: i64) -> Option<i64> {
    match content.get(field) {
        None => Some(default),
        Some(value) => value.as_i64(),
    }
}

fn object_field<'a>(content: &'a Value, field: &str) -> Option<&'a serde_json::Map<String, Value>> {
    match content.get(field) {
        None => Some(empty_object()),
        Some(value) => value.as_object(),
    }
}

fn empty_object() -> &'static serde_json::Map<String, Value> {
    static EMPTY: std::sync::OnceLock<serde_json::Map<String, Value>> = std::sync::OnceLock::new();
    EMPTY.get_or_init(serde_json::Map::new)
}

fn can_change_map_levels(
    old: &serde_json::Map<String, Value>,
    new: &serde_json::Map<String, Value>,
    old_default: i64,
    new_default: i64,
    sender: &str,
    sender_level: i64,
    disallow_equal_target_level: bool,
) -> bool {
    let keys = old
        .keys()
        .chain(new.keys())
        .map(String::as_str)
        .collect::<std::collections::HashSet<_>>();
    for key in keys {
        let Some(old_level) = old.get(key).map(Value::as_i64).unwrap_or(Some(old_default)) else {
            return false;
        };
        let Some(new_level) = new.get(key).map(Value::as_i64).unwrap_or(Some(new_default)) else {
            return false;
        };
        if old_level != new_level {
            let threshold_blocks_change = if disallow_equal_target_level {
                old_level >= sender_level
            } else {
                old_level > sender_level
            };
            if new_level > sender_level
                || (key != sender && threshold_blocks_change)
            {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{can_change_map_levels, can_change_power_levels, check_send_event, check_state_event};
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

    #[test]
    fn power_level_updates_respect_implicit_defaults_and_notification_thresholds() {
        let levels = json!({
            "users": { "@member:example.org": 20 },
            "users_default": 0,
            "events_default": 80
        });
        assert!(!can_change_power_levels(
            &levels,
            "@member:example.org",
            &json!({"users":{"@member:example.org":20},"users_default":0,"events_default":80,"events":{"m.room.message":0}})
        ));

        let levels = json!({
            "users": { "@member:example.org": 40 },
            "users_default": 0,
            "events_default": 80,
            "notifications": { "room": 50 }
        });
        assert!(!can_change_power_levels(
            &levels,
            "@member:example.org",
            &json!({"users":{"@member:example.org":40},"users_default":0,"notifications":{"room":0}})
        ));
        assert!(!can_change_power_levels(
            &levels,
            "@member:example.org",
            &json!({"users":{"@member:example.org":40},"users_default":0,"events_default":0,"notifications":{"room":50}})
        ));
        assert!(can_change_power_levels(
            &levels,
            "@member:example.org",
            &json!({"users":{"@member:example.org":40},"users_default":0,"events_default":80,"notifications":{"room":50}})
        ));
    }

    #[test]
    fn unchanged_higher_power_map_entries_are_allowed_but_cannot_be_edited() {
        let current = serde_json::from_value(json!({"room":50})).unwrap();
        let same = serde_json::from_value(json!({"room":50})).unwrap();
        let lowered = serde_json::from_value(json!({"room":30})).unwrap();
        assert!(can_change_map_levels(&current, &same, 50, 50, "@member:example.org", 40, false));
        assert!(!can_change_map_levels(&current, &lowered, 50, 50, "@member:example.org", 40, false));
    }

    #[test]
    fn power_level_content_rejects_non_integer_and_unsafe_values() {
        let previous = json!({"users":{"@mod:example.org":100}});
        assert!(!can_change_power_levels(
            &previous,
            "@mod:example.org",
            &json!({"events_default":"0"})
        ));
        assert!(!can_change_power_levels(
            &previous,
            "@mod:example.org",
            &json!({"events":{"m.room.message":9_007_199_254_740_992_i64}})
        ));
    }
}
