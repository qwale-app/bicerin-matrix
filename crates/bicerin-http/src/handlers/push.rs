use crate::{extract::AuthUser, state::AppState};
use axum::{
    extract::{Path, State},
    Json,
};
use bicerin_error::{BicerinError, BicerinResult};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
pub struct SetPusherBody {
    pub pushkey: String,
    pub kind: Option<String>,
    pub app_id: String,
    #[serde(default)]
    pub app_display_name: String,
    #[serde(default)]
    pub device_display_name: String,
    pub profile_tag: Option<String>,
    #[serde(default = "default_lang")]
    pub lang: String,
    #[serde(default)]
    pub data: Value,
}

fn default_lang() -> String {
    "en".to_string()
}

pub async fn set_pusher(
    user: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<SetPusherBody>,
) -> BicerinResult<Json<Value>> {
    bicerin_storage::push::set_pusher(
        &state.pool,
        &bicerin_storage::push::PusherRecord {
            user_id: user.user_id,
            pushkey: body.pushkey,
            app_id: body.app_id,
            kind: body.kind,
            app_display_name: body.app_display_name,
            device_display_name: body.device_display_name,
            profile_tag: body.profile_tag,
            lang: body.lang,
            data: body.data,
            created_at: chrono::Utc::now(),
        },
    )
    .await
    .map_err(|e| BicerinError::Internal(e.to_string()))?;
    Ok(Json(json!({})))
}

pub async fn list_pushers(
    user: AuthUser,
    State(state): State<AppState>,
) -> BicerinResult<Json<Value>> {
    let pushers = bicerin_storage::push::list_pushers(&state.pool, &user.user_id)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    let chunk: Vec<Value> = pushers
        .into_iter()
        .map(|p| {
            json!({
                "pushkey": p.pushkey,
                "kind": p.kind,
                "app_id": p.app_id,
                "app_display_name": p.app_display_name,
                "device_display_name": p.device_display_name,
                "profile_tag": p.profile_tag,
                "lang": p.lang,
                "data": p.data,
            })
        })
        .collect();
    Ok(Json(json!({ "pushers": chunk })))
}

pub async fn get_push_rules(
    user: AuthUser,
    State(state): State<AppState>,
) -> BicerinResult<Json<Value>> {
    let custom = bicerin_storage::push::list_push_rules(&state.pool, &user.user_id)
        .await
        .map_err(|error| BicerinError::Internal(error.to_string()))?;
    let mut groups = serde_json::Map::new();
    for kind in RULE_KINDS {
        groups.insert((*kind).to_string(), Value::Array(default_rules(kind)));
    }
    for rule in custom {
        let Some(group) = groups.get_mut(&rule.kind).and_then(Value::as_array_mut) else { continue; };
        group.push(json!({
            "rule_id": rule.rule_id,
            "default": false,
            "enabled": rule.enabled,
            "conditions": rule.conditions,
            "actions": rule.actions,
        }));
    }
    Ok(Json(json!({"global": groups})))
}

fn room_mute_rule_id(room_id: &str) -> String {
    format!(".m.bicerin.rule.room.{}", room_id)
}

#[derive(Debug, Deserialize)]
pub struct SetEnabledBody {
    pub enabled: bool,
}

pub async fn set_rule_enabled(
    user: AuthUser,
    State(state): State<AppState>,
    Path((kind, rule_id)): Path<(String, String)>,
    Json(body): Json<SetEnabledBody>,
) -> BicerinResult<Json<Value>> {
    validate_kind(&kind)?;
    if let Some(mut rule) = bicerin_storage::push::get_push_rule(&state.pool, &user.user_id, &kind, &rule_id)
        .await.map_err(|error| BicerinError::Internal(error.to_string()))? {
        rule.enabled = body.enabled;
        rule.updated_at = chrono::Utc::now();
        bicerin_storage::push::upsert_push_rule(&state.pool, &rule)
            .await.map_err(|error| BicerinError::Internal(error.to_string()))?;
    } else if kind == "override" && rule_id.starts_with('!') {
        bicerin_storage::push::set_push_rule_enabled(
            &state.pool,
            &user.user_id,
            &room_mute_rule_id(&rule_id),
            body.enabled,
        )
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    } else if is_default_rule(&kind, &rule_id) {
        bicerin_storage::push::set_push_rule_enabled(&state.pool, &user.user_id, &rule_id, body.enabled)
            .await.map_err(|error| BicerinError::Internal(error.to_string()))?;
    } else {
        return Err(BicerinError::NotFound);
    }
    Ok(Json(json!({})))
}

pub async fn get_rule_enabled(
    user: AuthUser,
    State(state): State<AppState>,
    Path((kind, rule_id)): Path<(String, String)>,
) -> BicerinResult<Json<Value>> {
    validate_kind(&kind)?;
    if let Some(rule) = bicerin_storage::push::get_push_rule(&state.pool, &user.user_id, &kind, &rule_id)
        .await.map_err(|error| BicerinError::Internal(error.to_string()))? {
        return Ok(Json(json!({ "enabled": rule.enabled })));
    }
    let lookup_rule_id = if kind == "override" && rule_id.starts_with('!') { room_mute_rule_id(&rule_id) } else { rule_id.clone() };
    if !is_default_rule(&kind, &rule_id) && lookup_rule_id == rule_id {
        return Err(BicerinError::NotFound);
    }
    let enabled = bicerin_storage::push::get_push_rule_enabled(
        &state.pool,
        &user.user_id,
        &lookup_rule_id,
    )
    .await
    .map_err(|e| BicerinError::Internal(e.to_string()))?
    .unwrap_or(true);
    Ok(Json(json!({ "enabled": enabled })))
}

#[derive(Debug, Deserialize)]
pub struct PutPushRuleBody {
    #[serde(default)]
    pub conditions: Vec<Value>,
    pub actions: Vec<Value>,
}

pub async fn put_push_rule(
    user: AuthUser,
    State(state): State<AppState>,
    Path((kind, rule_id)): Path<(String, String)>,
    Json(body): Json<PutPushRuleBody>,
) -> BicerinResult<Json<Value>> {
    validate_kind(&kind)?;
    if rule_id.is_empty() || body.actions.is_empty() {
        return Err(BicerinError::BadRequest("rule_id and actions are required".to_string()));
    }
    let rule = bicerin_storage::push::PushRuleRecord {
        user_id: user.user_id,
        kind,
        rule_id,
        enabled: true,
        conditions: Value::Array(body.conditions),
        actions: Value::Array(body.actions),
        updated_at: chrono::Utc::now(),
    };
    bicerin_storage::push::upsert_push_rule(&state.pool, &rule)
        .await.map_err(|error| BicerinError::Internal(error.to_string()))?;
    Ok(Json(json!({})))
}

pub async fn delete_push_rule(
    user: AuthUser,
    State(state): State<AppState>,
    Path((kind, rule_id)): Path<(String, String)>,
) -> BicerinResult<Json<Value>> {
    validate_kind(&kind)?;
    bicerin_storage::push::delete_push_rule(&state.pool, &user.user_id, &kind, &rule_id)
        .await.map_err(|error| BicerinError::Internal(error.to_string()))?;
    Ok(Json(json!({})))
}

#[derive(Debug, Deserialize)]
pub struct SetActionsBody { pub actions: Vec<Value> }

pub async fn get_rule_actions(
    user: AuthUser,
    State(state): State<AppState>,
    Path((kind, rule_id)): Path<(String, String)>,
) -> BicerinResult<Json<Value>> {
    validate_kind(&kind)?;
    if let Some(rule) = bicerin_storage::push::get_push_rule(&state.pool, &user.user_id, &kind, &rule_id)
        .await.map_err(|error| BicerinError::Internal(error.to_string()))? {
        return Ok(Json(json!({"actions": rule.actions})));
    }
    default_rules(&kind).into_iter().find(|rule| rule.get("rule_id").and_then(Value::as_str) == Some(&rule_id))
        .map(|rule| Json(json!({"actions": rule["actions"]})))
        .ok_or(BicerinError::NotFound)
}

pub async fn set_rule_actions(
    user: AuthUser,
    State(state): State<AppState>,
    Path((kind, rule_id)): Path<(String, String)>,
    Json(body): Json<SetActionsBody>,
) -> BicerinResult<Json<Value>> {
    validate_kind(&kind)?;
    let Some(mut rule) = bicerin_storage::push::get_push_rule(&state.pool, &user.user_id, &kind, &rule_id)
        .await.map_err(|error| BicerinError::Internal(error.to_string()))? else {
        return Err(BicerinError::NotFound);
    };
    rule.actions = Value::Array(body.actions);
    rule.updated_at = chrono::Utc::now();
    bicerin_storage::push::upsert_push_rule(&state.pool, &rule)
        .await.map_err(|error| BicerinError::Internal(error.to_string()))?;
    Ok(Json(json!({})))
}

const RULE_KINDS: &[&str] = &["override", "content", "room", "sender", "underride"];

fn validate_kind(kind: &str) -> BicerinResult<()> {
    if RULE_KINDS.contains(&kind) { Ok(()) } else { Err(BicerinError::BadRequest("invalid push rule kind".to_string())) }
}

fn is_default_rule(kind: &str, rule_id: &str) -> bool {
    default_rules(kind).iter().any(|rule| rule.get("rule_id") == Some(&Value::String(rule_id.to_string())))
}

fn default_rules(kind: &str) -> Vec<Value> {
    match kind {
        "override" => vec![json!({"rule_id":".m.rule.master","default":true,"enabled":false,"conditions":[],"actions":["dont_notify"]})],
        "underride" => vec![json!({"rule_id":".m.rule.message","default":true,"enabled":true,"conditions":[{"kind":"event_match","key":"type","pattern":"m.room.message"}],"actions":["notify"]})],
        _ => vec![],
    }
}
