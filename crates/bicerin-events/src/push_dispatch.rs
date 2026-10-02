use bicerin_storage::events::EventRecord;
use bicerin_storage::Store;
use serde_json::Value;

const NOTIFIABLE_EVENT_TYPES: &[&str] = &["m.room.message", "m.sticker"];

/// After a notifiable message event is persisted, best-effort enqueues an
/// outbound Push Gateway delivery (https://spec.matrix.org/v1.19/push-gateway-api/)
/// for every joined room member (other than the sender) who has registered
/// an `http`-kind pusher and hasn't muted the room. Failures are logged, not
/// propagated — same fire-and-forget pattern as `appservice_dispatch`.
pub async fn dispatch(store: &Store, event: &EventRecord) {
    if !NOTIFIABLE_EVENT_TYPES.contains(&event.event_type.as_str()) {
        return;
    }

    let members =
        bicerin_storage::rooms::get_room_members_by_membership(store, &event.room_id, "join")
            .await
            .unwrap_or_default();

    for member in members {
        if member.user_id == event.sender {
            continue;
        }

        if !should_notify(store, event, &member.user_id).await {
            continue;
        }

        let pushers = match bicerin_storage::push::list_pushers(store, &member.user_id).await {
            Ok(pushers) => pushers,
            Err(e) => {
                tracing::warn!(error = %e, user_id = %member.user_id, "failed to list pushers for push dispatch");
                continue;
            }
        };

        for pusher in pushers {
            if pusher.kind.as_deref() != Some("http") {
                continue;
            }
            let Some(url) = pusher.data.get("url").and_then(|v| v.as_str()) else {
                continue;
            };

            let payload = serde_json::json!({
                "notification": {
                    "id": event.event_id,
                    "room_id": event.room_id,
                    "type": event.event_type,
                    "sender": event.sender,
                    "event_id": event.event_id,
                    "counts": { "unread": 1 },
                    "devices": [{
                        "app_id": pusher.app_id,
                        "pushkey": pusher.pushkey,
                        "pushkey_ts": chrono::Utc::now().timestamp(),
                        "data": pusher.data,
                        "tweaks": {},
                    }],
                }
            });

            let record = bicerin_storage::push::PendingPushRecord {
                id: uuid::Uuid::new_v4().to_string(),
                user_id: member.user_id.clone(),
                pushkey: pusher.pushkey.clone(),
                app_id: pusher.app_id.clone(),
                url: url.to_string(),
                payload,
                attempts: 0,
                next_retry_at: None,
                delivered_at: None,
                created_at: chrono::Utc::now(),
            };
            if let Err(e) = bicerin_storage::push::enqueue_push(store, &record).await {
                tracing::warn!(error = %e, user_id = %member.user_id, "failed to enqueue push notification");
            }
        }
    }
}

async fn should_notify(store: &Store, event: &EventRecord, user_id: &str) -> bool {
    // Preserve the pre-existing room-mute rule during upgrades; user-created
    // rules below take care of all normal Matrix rule kinds.
    let legacy_mute_rule_id = format!(".m.bicerin.rule.room.{}", event.room_id);
    if bicerin_storage::push::get_push_rule_enabled(store, user_id, &legacy_mute_rule_id)
        .await
        .ok()
        .flatten()
        == Some(false)
    {
        return false;
    }

    let rules = match bicerin_storage::push::list_push_rules(store, user_id).await {
        Ok(rules) => rules,
        Err(error) => {
            tracing::warn!(error = %error, user_id, "failed to load push rules for dispatch");
            return true;
        }
    };

    for kind in ["override", "content", "room", "sender", "underride"] {
        if let Some(rule) = rules.iter().find(|rule| {
            rule.kind == kind && rule.enabled && rule_matches(rule, event)
        }) {
            return rule_notifies(&rule.actions);
        }
    }

    let master_enabled = bicerin_storage::push::get_push_rule_enabled(store, user_id, ".m.rule.master")
        .await.ok().flatten().unwrap_or(false);
    if master_enabled {
        return false;
    }
    bicerin_storage::push::get_push_rule_enabled(store, user_id, ".m.rule.message")
        .await.ok().flatten().unwrap_or(true)
}

fn rule_matches(rule: &bicerin_storage::push::PushRuleRecord, event: &EventRecord) -> bool {
    match rule.kind.as_str() {
        "content" if !event.content.get("body").and_then(Value::as_str).is_some_and(|body| wildcard_matches(&rule.rule_id, body)) => return false,
        "room" if rule.rule_id != event.room_id => return false,
        "sender" if rule.rule_id != event.sender => return false,
        _ => {}
    }

    rule.conditions.as_array().is_some_and(|conditions| {
        conditions.iter().all(|condition| condition_matches(condition, event))
    })
}

fn condition_matches(condition: &Value, event: &EventRecord) -> bool {
    match condition.get("kind").and_then(Value::as_str) {
        Some("event_match") => {
            let Some(key) = condition.get("key").and_then(Value::as_str) else { return false; };
            let Some(pattern) = condition.get("pattern").and_then(Value::as_str) else { return false; };
            if key == "type" {
                wildcard_matches(pattern, &event.event_type)
            } else {
                event_field(event, key).and_then(Value::as_str).is_some_and(|value| wildcard_matches(pattern, value))
            }
        }
        Some("event_property_is") => {
            let Some(key) = condition.get("key").and_then(Value::as_str) else { return false; };
            event_field(event, key) == condition.get("value")
        }
        _ => false,
    }
}

fn event_field<'a>(event: &'a EventRecord, key: &str) -> Option<&'a Value> {
    let key = key.strip_prefix("content.")?;
    key.split('.').try_fold(&event.content, |value, segment| value.get(segment))
}

fn wildcard_matches(pattern: &str, value: &str) -> bool {
    if pattern.len() >= 2 && pattern.starts_with('*') && pattern.ends_with('*') {
        value.contains(&pattern[1..pattern.len() - 1])
    } else if let Some(suffix) = pattern.strip_prefix('*') {
        value.ends_with(suffix)
    } else if let Some(prefix) = pattern.strip_suffix('*') {
        value.starts_with(prefix)
    } else {
        pattern == value
    }
}

fn rule_notifies(actions: &Value) -> bool {
    let Some(actions) = actions.as_array() else { return false; };
    !actions.iter().any(|action| action == "dont_notify")
        && actions.iter().any(|action| action == "notify")
}

#[cfg(test)]
mod tests {
    use super::wildcard_matches;

    #[test]
    fn event_match_patterns_support_prefix_and_suffix_wildcards() {
        assert!(wildcard_matches("m.room.*", "m.room.message"));
        assert!(wildcard_matches("*:example.test", "@alice:example.test"));
        assert!(!wildcard_matches("m.room.*", "m.presence"));
    }
}
