use crate::{extract::AuthUser, state::AppState};
use axum::{
    extract::{Path, State},
    Json,
};
use bicerin_error::{BicerinError, BicerinResult};
use bicerin_storage::client_data::{AccountDataRecord, ToDeviceMessageRecord};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

fn ensure_own_user(authenticated_user: &str, requested_user: &str) -> BicerinResult<()> {
    if authenticated_user == requested_user {
        Ok(())
    } else {
        Err(BicerinError::Forbidden)
    }
}

pub async fn get_account_data(
    user: AuthUser,
    State(state): State<AppState>,
    Path((requested_user, event_type)): Path<(String, String)>,
) -> BicerinResult<Json<Value>> {
    ensure_own_user(&user.user_id, &requested_user)?;
    let records = bicerin_storage::client_data::get_account_data(&state.pool, &requested_user, "")
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    let record = records
        .into_iter()
        .find(|record| record.event_type == event_type)
        .ok_or(BicerinError::NotFound)?;
    Ok(Json(record.content))
}

pub async fn put_account_data(
    user: AuthUser,
    State(state): State<AppState>,
    Path((requested_user, event_type)): Path<(String, String)>,
    Json(content): Json<Value>,
) -> BicerinResult<Json<Value>> {
    ensure_own_user(&user.user_id, &requested_user)?;
    if !content.is_object() {
        return Err(BicerinError::BadRequest(
            "account data content must be a JSON object".into(),
        ));
    }
    let stream_id = bicerin_storage::events::get_next_stream_id(&state.pool)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    bicerin_storage::client_data::upsert_account_data(
        &state.pool,
        &AccountDataRecord {
            user_id: user.user_id.clone(),
            room_id: String::new(),
            event_type,
            content,
            stream_id,
            updated_at: chrono::Utc::now(),
        },
    )
    .await
    .map_err(|e| BicerinError::Internal(e.to_string()))?;
    state.sync_bus.notify_user(user.user_id, stream_id);
    Ok(Json(json!({})))
}

pub async fn get_room_account_data(
    user: AuthUser,
    State(state): State<AppState>,
    Path((requested_user, room_id, event_type)): Path<(String, String, String)>,
) -> BicerinResult<Json<Value>> {
    ensure_own_user(&user.user_id, &requested_user)?;
    ensure_joined(&state, &user.user_id, &room_id).await?;
    let records =
        bicerin_storage::client_data::get_account_data(&state.pool, &requested_user, &room_id)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;
    let record = records
        .into_iter()
        .find(|record| record.event_type == event_type)
        .ok_or(BicerinError::NotFound)?;
    Ok(Json(record.content))
}

pub async fn put_room_account_data(
    user: AuthUser,
    State(state): State<AppState>,
    Path((requested_user, room_id, event_type)): Path<(String, String, String)>,
    Json(content): Json<Value>,
) -> BicerinResult<Json<Value>> {
    ensure_own_user(&user.user_id, &requested_user)?;
    ensure_joined(&state, &user.user_id, &room_id).await?;
    if !content.is_object() {
        return Err(BicerinError::BadRequest(
            "account data content must be a JSON object".into(),
        ));
    }
    let stream_id = bicerin_storage::events::get_next_stream_id(&state.pool)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    bicerin_storage::client_data::upsert_account_data(
        &state.pool,
        &AccountDataRecord {
            user_id: user.user_id.clone(),
            room_id: room_id.clone(),
            event_type,
            content,
            stream_id,
            updated_at: chrono::Utc::now(),
        },
    )
    .await
    .map_err(|e| BicerinError::Internal(e.to_string()))?;
    state.sync_bus.notify(room_id, stream_id);
    Ok(Json(json!({})))
}

async fn ensure_joined(state: &AppState, user_id: &str, room_id: &str) -> BicerinResult<()> {
    match bicerin_storage::rooms::get_room_member(&state.pool, room_id, user_id).await {
        Ok(member) if member.membership == "join" => Ok(()),
        Ok(_) => Err(BicerinError::Forbidden),
        Err(_) => Err(BicerinError::NotFound),
    }
}

#[derive(Debug, Deserialize)]
pub struct SendToDeviceBody {
    pub messages: HashMap<String, HashMap<String, Value>>,
}

pub async fn send_to_device(
    user: AuthUser,
    State(state): State<AppState>,
    Path((event_type, txn_id)): Path<(String, String)>,
    Json(body): Json<SendToDeviceBody>,
) -> BicerinResult<Json<Value>> {
    bicerin_events::validation::validate_event_type(&event_type)?;
    let endpoint = format!("sendToDevice:{event_type}");
    if bicerin_storage::transactions::get_transaction(
        &state.pool,
        &user.user_id,
        &user.device_id,
        &txn_id,
        &endpoint,
    )
    .await
    .map_err(|e| BicerinError::Internal(e.to_string()))?
    .is_some()
    {
        return Ok(Json(json!({})));
    }

    let mut planned = Vec::new();
    for (target_user, target_devices) in body.messages {
        bicerin_events::validation::validate_user_id(&target_user)?;
        let Some((_, target_server)) = target_user.rsplit_once(':') else {
            unreachable!("validated Matrix user ID")
        };
        if target_server != state.server_name {
            continue; // Federation is not enabled by this single-homeserver deployment.
        }
        let target_user_exists = bicerin_storage::users::get_user(&state.pool, &target_user)
            .await
            .is_ok();
        if !target_user_exists {
            continue;
        }
        let known_devices = bicerin_storage::users::list_devices(&state.pool, &target_user)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;
        let mut addressed_devices = std::collections::HashSet::new();
        for (requested_device, content) in target_devices {
            if requested_device.is_empty() || !content.is_object() {
                return Err(BicerinError::BadRequest(
                    "each send-to-device entry needs a device ID and object content".into(),
                ));
            }
            let devices: Vec<String> = if requested_device == "*" {
                known_devices
                    .iter()
                    .map(|device| device.device_id.clone())
                    .collect()
            } else if known_devices
                .iter()
                .any(|device| device.device_id == requested_device)
            {
                vec![requested_device]
            } else {
                Vec::new()
            };
            for device_id in devices {
                if !addressed_devices.insert(device_id.clone()) {
                    return Err(BicerinError::BadRequest(
                        "a device may receive only one message per transaction".into(),
                    ));
                }
                planned.push((target_user.clone(), device_id, content.clone()));
            }
        }
    }

    for (target_user, device_id, content) in planned {
        let identity = json!([
            user.user_id,
            user.device_id,
            event_type,
            txn_id,
            target_user,
            device_id
        ]);
        let message_id = bicerin_types::auth::hash_access_token(&identity.to_string()).to_string();
        let stream_id = bicerin_storage::events::get_next_stream_id(&state.pool)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;
        bicerin_storage::client_data::insert_to_device_message(
            &state.pool,
            &ToDeviceMessageRecord {
                message_id,
                user_id: target_user.clone(),
                device_id,
                sender: user.user_id.clone(),
                event_type: event_type.clone(),
                content,
                stream_id,
                delivered_sync_token: None,
                created_at: chrono::Utc::now(),
            },
        )
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
        state.sync_bus.notify_user(target_user, stream_id);
    }

    let result = json!({});
    bicerin_storage::transactions::record_transaction(
        &state.pool,
        &bicerin_storage::transactions::TransactionRecord {
            user_id: user.user_id,
            device_id: user.device_id,
            txn_id,
            endpoint,
            result: result.clone(),
            created_at: chrono::Utc::now(),
        },
    )
    .await
    .map_err(|e| BicerinError::Internal(e.to_string()))?;
    Ok(Json(result))
}

#[cfg(test)]
mod tests {
    use super::ensure_own_user;
    use bicerin_error::BicerinError;

    #[test]
    fn account_data_paths_are_limited_to_the_authenticated_user() {
        assert!(ensure_own_user("@alice:example.org", "@alice:example.org").is_ok());
        assert!(matches!(
            ensure_own_user("@alice:example.org", "@bob:example.org"),
            Err(BicerinError::Forbidden)
        ));
    }
}
