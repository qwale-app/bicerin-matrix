use crate::{extract::AuthUser, state::AppState};
use axum::{extract::State, Json};
use bicerin_error::{BicerinError, BicerinResult};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::HashMap;

#[derive(Debug, Deserialize, Default)]
pub struct KeysUploadBody {
    pub device_keys: Option<Value>,
    #[serde(default)]
    pub one_time_keys: HashMap<String, Value>,
    #[serde(default)]
    pub fallback_keys: HashMap<String, Value>,
}

pub async fn upload_keys(user: AuthUser, State(state): State<AppState>, Json(body): Json<KeysUploadBody>) -> BicerinResult<Json<Value>> {
    if let Some(device_keys) = body.device_keys {
        bicerin_storage::crypto::upsert_device_keys(&state.pool, &bicerin_storage::crypto::DeviceKeyRecord {
            user_id: user.user_id.clone(),
            device_id: user.device_id.clone(),
            key_json: device_keys,
            updated_at: chrono::Utc::now(),
        }).await.map_err(|e| BicerinError::Internal(e.to_string()))?;
    }

    if !body.one_time_keys.is_empty() {
        let records: Vec<_> = body.one_time_keys.into_iter().map(|(key_id, key_json)| {
            bicerin_storage::crypto::OneTimeKeyRecord {
                user_id: user.user_id.clone(),
                device_id: user.device_id.clone(),
                key_id,
                key_json,
                created_at: chrono::Utc::now(),
            }
        }).collect();
        bicerin_storage::crypto::insert_one_time_keys(&state.pool, &records)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;
    }

    for (key_id, key_json) in body.fallback_keys {
        let algorithm = key_id.split(':').next().unwrap_or(&key_id).to_string();
        bicerin_storage::crypto::upsert_fallback_key(&state.pool, &bicerin_storage::crypto::FallbackKeyRecord {
            user_id: user.user_id.clone(),
            device_id: user.device_id.clone(),
            algorithm,
            key_json,
            used: false,
            updated_at: chrono::Utc::now(),
        }).await.map_err(|e| BicerinError::Internal(e.to_string()))?;
    }

    let counts = bicerin_storage::crypto::count_one_time_keys(&state.pool, &user.user_id, &user.device_id)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;

    Ok(Json(json!({ "one_time_key_counts": counts })))
}

#[derive(Debug, Deserialize, Default)]
pub struct KeysQueryBody {
    #[serde(default)]
    pub device_keys: HashMap<String, Vec<String>>,
}

pub async fn query_keys(_user: AuthUser, State(state): State<AppState>, Json(body): Json<KeysQueryBody>) -> BicerinResult<Json<Value>> {
    let mut result: HashMap<String, HashMap<String, Value>> = HashMap::new();

    for (target_user, device_ids) in body.device_keys {
        let records = if device_ids.is_empty() {
            bicerin_storage::crypto::get_all_device_keys_for_user(&state.pool, &target_user)
                .await
                .map_err(|e| BicerinError::Internal(e.to_string()))?
        } else {
            let mut out = Vec::new();
            for device_id in &device_ids {
                if let Ok(record) = bicerin_storage::crypto::get_device_keys(&state.pool, &target_user, device_id).await {
                    out.push(record);
                }
            }
            out
        };

        let per_device: HashMap<String, Value> = records.into_iter().map(|r| (r.device_id, r.key_json)).collect();
        result.insert(target_user, per_device);
    }

    Ok(Json(json!({ "device_keys": result, "failures": {} })))
}

#[derive(Debug, Deserialize, Default)]
pub struct KeysClaimBody {
    #[serde(default)]
    pub one_time_keys: HashMap<String, HashMap<String, String>>,
}

pub async fn claim_keys(_user: AuthUser, State(state): State<AppState>, Json(body): Json<KeysClaimBody>) -> BicerinResult<Json<Value>> {
    let mut result: HashMap<String, HashMap<String, Value>> = HashMap::new();

    for (target_user, devices) in body.one_time_keys {
        let mut per_device: HashMap<String, Value> = HashMap::new();
        for (device_id, algorithm) in devices {
            if let Ok(Some(key)) = bicerin_storage::crypto::claim_one_time_key(&state.pool, &target_user, &device_id, &algorithm).await {
                let mut map = Map::new();
                map.insert(key.key_id, key.key_json);
                per_device.insert(device_id, Value::Object(map));
            }
        }
        result.insert(target_user, per_device);
    }

    Ok(Json(json!({ "one_time_keys": result })))
}
