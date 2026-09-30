use serde_json::Value;

pub fn check_power_level(
    power_levels: &Value,
    user_id: &str,
    required: i64,
) -> bool {
    let users_default = power_levels.get("users_default")
        .and_then(|v| v.as_i64()).unwrap_or(0);
    let user_level = power_levels.get("users")
        .and_then(|u| u.get(user_id))
        .and_then(|v| v.as_i64())
        .unwrap_or(users_default);
    user_level >= required
}

pub fn check_send_event(
    power_levels: &Value,
    user_id: &str,
    event_type: &str,
) -> bool {
    let events_default = power_levels.get("events_default")
        .and_then(|v| v.as_i64()).unwrap_or(0);
    let required = power_levels.get("events")
        .and_then(|e| e.get(event_type))
        .and_then(|v| v.as_i64())
        .unwrap_or(events_default);
    check_power_level(power_levels, user_id, required)
}

pub fn check_state_event(
    power_levels: &Value,
    user_id: &str,
    event_type: &str,
) -> bool {
    let state_default = power_levels.get("state_default")
        .and_then(|v| v.as_i64()).unwrap_or(50);
    let required = power_levels.get("events")
        .and_then(|e| e.get(event_type))
        .and_then(|v| v.as_i64())
        .unwrap_or(state_default);
    check_power_level(power_levels, user_id, required)
}
