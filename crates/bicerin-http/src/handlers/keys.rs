use crate::{extract::AuthUser, state::AppState};
use axum::{
    extract::{Query, State},
    Json,
};
use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
use bicerin_error::{BicerinError, BicerinResult};
use ed25519_dalek::{Signature, VerifyingKey};
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

pub async fn upload_keys(
    user: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<KeysUploadBody>,
) -> BicerinResult<Json<Value>> {
    if let Some(device_keys) = body.device_keys {
        validate_device_key_upload(&device_keys, &user.user_id, &user.device_id)?;
        let device_key_id = format!("ed25519:{}", user.device_id);
        let device_public_key = device_keys
            .get("keys")
            .and_then(Value::as_object)
            .and_then(|keys| keys.get(&device_key_id))
            .and_then(Value::as_str)
            .expect("device key validation requires an Ed25519 key");
        if !verify_matrix_signature(
            &device_keys,
            &user.user_id,
            &device_key_id,
            device_public_key,
        ) {
            return Err(invalid_signature("Device key signature is invalid"));
        }
        let existing_cross_signing =
            bicerin_storage::cross_signing::get_keys(&state.pool, &user.user_id)
                .await
                .map_err(|error| BicerinError::Internal(error.to_string()))?;
        let device_ids = std::collections::HashSet::from([user.device_id.clone()]);
        if existing_cross_signing
            .iter()
            .any(|key| cross_signing_id_collides(&key.key_json, &device_ids))
        {
            return Err(key_id_collision());
        }
        let mut device_keys = device_keys;
        if let Some(object) = device_keys.as_object_mut() {
            object.remove("unsigned");
        }
        let changed = match bicerin_storage::crypto::get_device_keys(
            &state.pool,
            &user.user_id,
            &user.device_id,
        )
        .await
        {
            Ok(existing) => existing.key_json != device_keys,
            Err(bicerin_storage::db::StorageError::NotFound) => true,
            Err(error) => return Err(BicerinError::Internal(error.to_string())),
        };
        if changed {
            bicerin_storage::crypto::upsert_device_keys(
                &state.pool,
                &bicerin_storage::crypto::DeviceKeyRecord {
                    user_id: user.user_id.clone(),
                    device_id: user.device_id.clone(),
                    key_json: device_keys,
                    updated_at: chrono::Utc::now(),
                },
            )
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;
            let stream_id = bicerin_storage::events::get_next_stream_id(&state.pool)
                .await
                .map_err(|e| BicerinError::Internal(e.to_string()))?;
            bicerin_storage::cross_signing::record_device_change(
                &state.pool,
                &user.user_id,
                stream_id,
                "changed",
            )
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;
            state.sync_bus.notify_all(stream_id);
        }
    }

    let device_signing_key = match bicerin_storage::crypto::get_device_keys(
        &state.pool,
        &user.user_id,
        &user.device_id,
    )
    .await
    {
        Ok(record) => record
            .key_json
            .get("keys")
            .and_then(Value::as_object)
            .and_then(|keys| keys.get(&format!("ed25519:{}", user.device_id)))
            .and_then(Value::as_str)
            .map(str::to_owned),
        Err(bicerin_storage::db::StorageError::NotFound) => None,
        Err(error) => return Err(BicerinError::Internal(error.to_string())),
    };

    if !body.one_time_keys.is_empty() {
        let records: Vec<_> = body
            .one_time_keys
            .into_iter()
            .map(|(key_id, key_json)| {
                let (algorithm, _) = split_key_id(&key_id)?;
                validate_uploaded_key(&key_json, false)?;
                if (algorithm == "signed_curve25519" || key_json.get("signatures").is_some())
                    && !device_signing_key.as_deref().is_some_and(|public_key| {
                        verify_matrix_signature(
                            &key_json,
                            &user.user_id,
                            &format!("ed25519:{}", user.device_id),
                            public_key,
                        )
                    })
                {
                    return Err(invalid_signature("One-time key signature is invalid"));
                }
                Ok(bicerin_storage::crypto::OneTimeKeyRecord {
                    user_id: user.user_id.clone(),
                    device_id: user.device_id.clone(),
                    key_id,
                    key_json,
                    created_at: chrono::Utc::now(),
                })
            })
            .collect::<BicerinResult<Vec<_>>>()?;
        bicerin_storage::crypto::insert_one_time_keys(&state.pool, &records)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;
    }

    for (key_id, key_json) in body.fallback_keys {
        let (algorithm, key_id_value) = split_key_id(&key_id)?;
        validate_uploaded_key(&key_json, true)?;
        if (algorithm == "signed_curve25519" || key_json.get("signatures").is_some())
            && !device_signing_key.as_deref().is_some_and(|public_key| {
                verify_matrix_signature(
                    &key_json,
                    &user.user_id,
                    &format!("ed25519:{}", user.device_id),
                    public_key,
                )
            })
        {
            return Err(invalid_signature("Fallback key signature is invalid"));
        }
        bicerin_storage::crypto::upsert_fallback_key(
            &state.pool,
            &bicerin_storage::crypto::FallbackKeyRecord {
                user_id: user.user_id.clone(),
                device_id: user.device_id.clone(),
                algorithm: algorithm.to_string(),
                key_id: key_id_value.to_string(),
                key_json,
                used: false,
                updated_at: chrono::Utc::now(),
            },
        )
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    }

    let counts =
        bicerin_storage::crypto::count_one_time_keys(&state.pool, &user.user_id, &user.device_id)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;

    let unused_fallback_key_types = bicerin_storage::crypto::count_unused_fallback_keys(
        &state.pool,
        &user.user_id,
        &user.device_id,
    )
    .await
    .map_err(|e| BicerinError::Internal(e.to_string()))?;
    Ok(Json(
        json!({ "one_time_key_counts": counts, "unused_fallback_key_types": unused_fallback_key_types }),
    ))
}

fn split_key_id(key_id: &str) -> BicerinResult<(&str, &str)> {
    let Some((algorithm, id)) = key_id.split_once(':') else {
        return Err(BicerinError::BadRequest(
            "key IDs must have the form algorithm:key_id".into(),
        ));
    };
    if algorithm.is_empty() || id.is_empty() {
        return Err(BicerinError::BadRequest(
            "key IDs must have the form algorithm:key_id".into(),
        ));
    }
    Ok((algorithm, id))
}

fn validate_uploaded_key(key: &Value, is_fallback: bool) -> BicerinResult<()> {
    let valid = match key {
        Value::String(value) => !value.is_empty(),
        Value::Object(object) => {
            object
                .get("key")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.is_empty())
                && object
                    .get("signatures")
                    .and_then(Value::as_object)
                    .is_some_and(|signatures| {
                        signatures.values().all(|signature| {
                            signature.as_object().is_some_and(|keys| {
                                keys.values().all(|value| value.as_str().is_some())
                            })
                        })
                    })
                && (!is_fallback || object.get("fallback") == Some(&Value::Bool(true)))
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else if is_fallback {
        Err(BicerinError::BadRequest(
            "fallback key must be a string or a signed key object with fallback=true".into(),
        ))
    } else {
        Err(BicerinError::BadRequest(
            "one-time key must be a string or a signed key object".into(),
        ))
    }
}

fn validate_device_key_upload(
    device_keys: &Value,
    user_id: &str,
    device_id: &str,
) -> BicerinResult<()> {
    if device_keys.get("user_id").and_then(Value::as_str) != Some(user_id)
        || device_keys.get("device_id").and_then(Value::as_str) != Some(device_id)
        || !device_keys
            .get("algorithms")
            .and_then(Value::as_array)
            .is_some_and(|values| !values.is_empty() && values.iter().all(Value::is_string))
        || !device_keys
            .get("keys")
            .and_then(Value::as_object)
            .is_some_and(|keys| {
                keys.get(&format!("ed25519:{device_id}"))
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.is_empty())
                    && keys.iter().all(|(key_id, value)| {
                        key_id
                            .split_once(':')
                            .is_some_and(|(algorithm, suffix)| {
                                !algorithm.is_empty()
                                    && suffix == device_id
                                    && value.as_str().is_some_and(|value| !value.is_empty())
                            })
                    })
            })
        || device_keys
            .get("signatures")
            .and_then(Value::as_object)
            .and_then(|signatures| signatures.get(user_id))
            .and_then(Value::as_object)
            .and_then(|signatures| signatures.get(&format!("ed25519:{device_id}")))
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    {
        return Err(BicerinError::BadRequest("device_keys must describe the authenticated user and device and contain algorithms, keys, and signatures".into()));
    }
    Ok(())
}

#[derive(Debug, Deserialize, Default)]
pub struct KeysQueryBody {
    #[serde(default)]
    pub device_keys: HashMap<String, Vec<String>>,
}

pub async fn query_keys(
    user: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<KeysQueryBody>,
) -> BicerinResult<Json<Value>> {
    let mut result: HashMap<String, HashMap<String, Value>> = HashMap::new();
    let mut master_keys = HashMap::new();
    let mut self_signing_keys = HashMap::new();
    let mut user_signing_keys = HashMap::new();

    for (target_user, device_ids) in body.device_keys {
        let records = if device_ids.is_empty() {
            bicerin_storage::crypto::get_all_device_keys_for_user(&state.pool, &target_user)
                .await
                .map_err(|e| BicerinError::Internal(e.to_string()))?
        } else {
            let mut out = Vec::new();
            for device_id in &device_ids {
                match bicerin_storage::crypto::get_device_keys(&state.pool, &target_user, device_id)
                    .await
                {
                    Ok(record) => out.push(record),
                    Err(bicerin_storage::db::StorageError::NotFound) => {}
                    Err(error) => return Err(BicerinError::Internal(error.to_string())),
                }
            }
            out
        };

        let per_device: HashMap<String, Value> = records
            .into_iter()
            .map(|r| (r.device_id, r.key_json))
            .collect();
        result.insert(target_user, per_device);
    }

    for target_user in result.keys() {
        let cross_signing_keys = bicerin_storage::cross_signing::get_keys(&state.pool, target_user)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;
        let owner_devices =
            bicerin_storage::crypto::get_all_device_keys_for_user(&state.pool, target_user)
                .await
                .map_err(|error| BicerinError::Internal(error.to_string()))?;
        let owner_device_key_ids = owner_devices
            .iter()
            .filter_map(|device| device.key_json.get("keys").and_then(Value::as_object))
            .flat_map(|keys| keys.keys())
            .filter(|key_id| key_id.starts_with("ed25519:"))
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        let owner_master_key_id = cross_signing_keys
            .iter()
            .find(|key| key.key_type == "master")
            .and_then(|key| cross_signing_signer(&key.key_json))
            .map(|(key_id, _)| key_id.to_owned());
        for key in cross_signing_keys {
            let key_json = visible_cross_signing_key(
                key.key_json,
                &key.key_type,
                &user.user_id,
                target_user,
                &owner_device_key_ids,
                owner_master_key_id.as_deref(),
            );
            match key.key_type.as_str() {
                "master" => {
                    master_keys.insert(target_user.clone(), key_json);
                }
                "self_signing" => {
                    self_signing_keys.insert(target_user.clone(), key_json);
                }
                "user_signing" if target_user == &user.user_id => {
                    user_signing_keys.insert(target_user.clone(), key_json);
                }
                _ => {}
            }
        }
    }

    Ok(Json(json!({
        "device_keys": result,
        "master_keys": master_keys,
        "self_signing_keys": self_signing_keys,
        "user_signing_keys": user_signing_keys,
        "failures": {}
    })))
}

#[derive(Debug, Deserialize, Default)]
pub struct KeysClaimBody {
    #[serde(default)]
    pub one_time_keys: HashMap<String, HashMap<String, String>>,
}

pub async fn claim_keys(
    _user: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<KeysClaimBody>,
) -> BicerinResult<Json<Value>> {
    let mut result: HashMap<String, HashMap<String, Value>> = HashMap::new();

    for (target_user, devices) in body.one_time_keys {
        let mut per_device: HashMap<String, Value> = HashMap::new();
        for (device_id, algorithm) in devices {
            if let Some(key) = bicerin_storage::crypto::claim_one_time_key(
                &state.pool,
                &target_user,
                &device_id,
                &algorithm,
            )
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?
            {
                let mut map = Map::new();
                map.insert(key.key_id, key.key_json);
                per_device.insert(device_id, Value::Object(map));
            } else if let Some(key) = bicerin_storage::crypto::get_fallback_key(
                &state.pool,
                &target_user,
                &device_id,
                &algorithm,
            )
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?
            {
                let mut key_json = key.key_json;
                if let Some(object) = key_json.as_object_mut() {
                    object.insert("fallback".to_string(), Value::Bool(true));
                }
                let mut map = Map::new();
                map.insert(format!("{}:{}", key.algorithm, key.key_id), key_json);
                per_device.insert(device_id, Value::Object(map));
            }
        }
        result.insert(target_user, per_device);
    }

    Ok(Json(json!({ "one_time_keys": result })))
}

#[derive(Debug, Deserialize, Default)]
pub struct CrossSigningUploadBody {
    pub master_key: Option<Value>,
    pub self_signing_key: Option<Value>,
    pub user_signing_key: Option<Value>,
    pub auth: Option<Value>,
}

pub async fn upload_device_signing_keys(
    user: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CrossSigningUploadBody>,
) -> BicerinResult<Json<Value>> {
    let mut uploads = Vec::new();
    for (key_type, key, usage) in [
        ("master", body.master_key, "master"),
        ("self_signing", body.self_signing_key, "self_signing"),
        ("user_signing", body.user_signing_key, "user_signing"),
    ] {
        if let Some(key) = key {
            validate_cross_signing_key(&key, &user.user_id, usage)?;
            uploads.push((key_type, key));
        }
    }
    if uploads.is_empty() {
        return Err(BicerinError::BadRequest(
            "at least one cross-signing key is required".into(),
        ));
    }

    let existing = bicerin_storage::cross_signing::get_keys(&state.pool, &user.user_id)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    let known_device_ids = bicerin_storage::users::list_devices(&state.pool, &user.user_id)
        .await
        .map_err(|error| BicerinError::Internal(error.to_string()))?
        .into_iter()
        .map(|device| device.device_id)
        .collect::<std::collections::HashSet<_>>();
    if uploads
        .iter()
        .any(|(_, key)| cross_signing_id_collides(key, &known_device_ids))
    {
        return Err(key_id_collision());
    }
    let existing_master = existing
        .iter()
        .find(|key| key.key_type == "master")
        .map(|key| &key.key_json);
    let request_master = uploads
        .iter()
        .find(|(kind, _)| *kind == "master")
        .map(|(_, key)| key);
    if existing_master.is_none()
        && request_master.is_none()
        && uploads.iter().any(|(kind, _)| *kind != "master")
    {
        return Err(BicerinError::MatrixError {
            errcode: "M_MISSING_PARAM".into(),
            error: "A master cross-signing key must be uploaded before signing keys".into(),
        });
    }
    let can_upload_without_uia = existing_master.is_none()
        || request_master.is_some_and(|candidate| {
            existing_master == Some(candidate)
                && uploads.iter().all(|(kind, candidate)| {
                    existing
                        .iter()
                        .find(|key| key.key_type == *kind)
                        .is_some_and(|stored| stored.key_json == *candidate)
                })
        });
    if !can_upload_without_uia
        && user.device_id != "APPSERVICE"
        && !valid_password_uia(&state, &user.user_id, body.auth.as_ref()).await?
    {
        return Err(BicerinError::UiaRequired(json!({
            "errcode": "M_UNAUTHORIZED",
            "error": "Additional authentication is required to replace cross-signing keys",
            "flows": [{"stages": ["m.login.password"]}],
            "params": {},
            "session": uuid::Uuid::new_v4().to_string()
        })));
    }

    let effective_master = request_master.or(existing_master);
    for (key_type, key) in uploads.iter().filter(|(key_type, _)| *key_type != "master") {
        let Some((master_key_id, master_public_key)) =
            effective_master.and_then(cross_signing_signer)
        else {
            return Err(BicerinError::BadRequest(format!(
                "{key_type} key requires an uploaded master key"
            )));
        };
        if !is_signed_by_cross_signing_key(
            key,
            &user.user_id,
            master_key_id,
            master_public_key,
        ) {
            return Err(invalid_signature(format!(
                "{key_type} key must be signed by the user's master key"
            )));
        }
    }

    let keys_changed = uploads.iter().any(|(key_type, key_json)| {
        existing
            .iter()
            .find(|stored| stored.key_type == *key_type)
            .is_none_or(|stored| stored.key_json != *key_json)
    });
    if !keys_changed {
        return Ok(Json(json!({})));
    }

    let stream_id = bicerin_storage::events::get_next_stream_id(&state.pool)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    for (key_type, key_json) in uploads {
        bicerin_storage::cross_signing::upsert_key(
            &state.pool,
            &bicerin_storage::cross_signing::CrossSigningKeyRecord {
                user_id: user.user_id.clone(),
                key_type: key_type.to_string(),
                key_json,
                updated_at: chrono::Utc::now(),
            },
        )
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    }
    bicerin_storage::cross_signing::record_device_change(
        &state.pool,
        &user.user_id,
        stream_id,
        "changed",
    )
    .await
    .map_err(|e| BicerinError::Internal(e.to_string()))?;
    state.sync_bus.notify_all(stream_id);
    Ok(Json(json!({})))
}

fn validate_cross_signing_key(key: &Value, user_id: &str, usage: &str) -> BicerinResult<()> {
    let signatures_valid = match key.get("signatures") {
        None => usage == "master",
        Some(signatures) => signatures.as_object().is_some_and(|signatures| {
            signatures.values().all(|user_signatures| {
                user_signatures.as_object().is_some_and(|user_signatures| {
                    user_signatures.iter().all(|(key_id, signature)| {
                        key_id.starts_with("ed25519:")
                            && signature.as_str().is_some_and(|value| !value.is_empty())
                    })
                })
            })
        }),
    };
    if key.get("user_id").and_then(Value::as_str) != Some(user_id)
        || key
            .get("usage")
            .and_then(Value::as_array)
            .is_none_or(|values| values.len() != 1 || values[0].as_str() != Some(usage))
        || key
            .get("keys")
            .and_then(Value::as_object)
            .is_none_or(|keys| {
                keys.len() != 1
                    || !keys.iter().all(|(key_id, public_key)| {
                        key_id
                            .strip_prefix("ed25519:")
                            .is_some_and(|key_id| {
                                !key_id.is_empty() && public_key.as_str() == Some(key_id)
                            })
                    })
            })
        || !signatures_valid
        || (usage != "master"
            && key
                .get("signatures")
                .and_then(Value::as_object)
                .and_then(|signatures| signatures.get(user_id))
                .and_then(Value::as_object)
                .is_none())
    {
        return Err(BicerinError::BadRequest(format!(
            "invalid {usage} cross-signing key"
        )));
    }
    Ok(())
}

fn cross_signing_signer(key: &Value) -> Option<(&str, &str)> {
    key.get("keys")?.as_object()?.iter().find_map(|(key_id, public_key)| {
        let public_key = public_key.as_str()?;
        (key_id == &format!("ed25519:{public_key}")).then_some((key_id.as_str(), public_key))
    })
}

fn cross_signing_id_collides(
    key: &Value,
    device_ids: &std::collections::HashSet<String>,
) -> bool {
    cross_signing_signer(key)
        .is_some_and(|(_, public_key)| device_ids.contains(public_key))
}

fn is_signed_by_cross_signing_key(
    key: &Value,
    user_id: &str,
    signer_key_id: &str,
    signer_public_key: &str,
) -> bool {
    verify_matrix_signature(key, user_id, signer_key_id, signer_public_key)
}

fn verify_matrix_signature(
    signed_json: &Value,
    signer_user: &str,
    signer_key_id: &str,
    signer_public_key: &str,
) -> bool {
    let Some(signature) = signed_json
        .get("signatures")
        .and_then(Value::as_object)
        .and_then(|signatures| signatures.get(signer_user))
        .and_then(Value::as_object)
        .and_then(|signatures| signatures.get(signer_key_id))
        .and_then(Value::as_str)
    else {
        return false;
    };
    let (Ok(public_key), Ok(signature)) = (
        STANDARD_NO_PAD.decode(signer_public_key),
        STANDARD_NO_PAD.decode(signature),
    ) else {
        return false;
    };
    let (Ok(public_key), Ok(signature)) = (
        <[u8; 32]>::try_from(public_key.as_slice()),
        <[u8; 64]>::try_from(signature.as_slice()),
    ) else {
        return false;
    };
    let Ok(public_key) = VerifyingKey::from_bytes(&public_key) else {
        return false;
    };
    let signature = Signature::from_bytes(&signature);
    let Some(payload) = canonical_signed_payload(signed_json) else {
        return false;
    };
    public_key.verify_strict(&payload, &signature).is_ok()
}

fn canonical_signed_payload(signed_json: &Value) -> Option<Vec<u8>> {
    let mut payload = signed_json.clone();
    let object = payload.as_object_mut()?;
    object.remove("signatures");
    object.remove("unsigned");
    canonical_json_values_are_supported(&payload).then(|| serde_json::to_vec(&payload).ok())?
}

fn canonical_json_values_are_supported(value: &Value) -> bool {
    const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

    match value {
        Value::Null | Value::Bool(_) | Value::String(_) => true,
        Value::Number(number) => {
            number
                .as_i64()
                .is_some_and(|number| number.unsigned_abs() <= MAX_SAFE_INTEGER)
                || number
                    .as_u64()
                    .is_some_and(|number| number <= MAX_SAFE_INTEGER)
        }
        Value::Array(values) => values.iter().all(canonical_json_values_are_supported),
        Value::Object(values) => values.values().all(canonical_json_values_are_supported),
    }
}

fn invalid_signature(error: impl Into<String>) -> BicerinError {
    BicerinError::MatrixError {
        errcode: "M_INVALID_SIGNATURE".into(),
        error: error.into(),
    }
}

fn key_id_collision() -> BicerinError {
    BicerinError::MatrixError {
        errcode: "M_FORBIDDEN".into(),
        error: "A device ID cannot match a cross-signing key ID".into(),
    }
}

async fn valid_password_uia(
    state: &AppState,
    user_id: &str,
    auth: Option<&Value>,
) -> BicerinResult<bool> {
    let Some(auth) = auth else {
        return Ok(false);
    };
    if auth.get("type").and_then(Value::as_str) != Some("m.login.password") {
        return Ok(false);
    }
    let Some(password) = auth.get("password").and_then(Value::as_str) else {
        return Ok(false);
    };
    let auth_user = auth
        .get("identifier")
        .and_then(|id| id.get("user"))
        .and_then(Value::as_str)
        .or_else(|| auth.get("user").and_then(Value::as_str));
    if auth_user.is_some_and(|name| {
        name != user_id
            && user_id
                .strip_prefix('@')
                .and_then(|id| id.split(':').next())
                != Some(name)
    }) {
        return Ok(false);
    }
    let record = bicerin_storage::users::get_user(&state.pool, user_id)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    Ok(record
        .password_hash
        .as_deref()
        .is_some_and(|hash| bicerin_auth::password::verify_password(password, hash)))
}

pub async fn upload_signatures(
    user: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> BicerinResult<Json<Value>> {
    let Some(users) = body.as_object() else {
        return Err(BicerinError::BadRequest(
            "signature upload must be an object".into(),
        ));
    };
    let own_devices =
        bicerin_storage::crypto::get_all_device_keys_for_user(&state.pool, &user.user_id)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;
    let own_cross_signing = bicerin_storage::cross_signing::get_keys(&state.pool, &user.user_id)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    let mut permitted_signing_keys = HashMap::new();
    for record in &own_devices {
        if let Some(keys) = record.key_json.get("keys").and_then(Value::as_object) {
            for (key_id, public_key) in keys {
                if key_id.starts_with("ed25519:") {
                    if let Some(public_key) = public_key.as_str() {
                    permitted_signing_keys.insert(key_id.clone(), public_key.to_string());
                    }
                }
            }
        }
    }
    for record in &own_cross_signing {
        if let Some(keys) = record.key_json.get("keys").and_then(Value::as_object) {
            for (key_id, public_key) in keys {
                if key_id.starts_with("ed25519:") {
                    if let Some(public_key) = public_key.as_str() {
                    permitted_signing_keys.insert(key_id.clone(), public_key.to_string());
                    }
                }
            }
        }
    }

    let mut failures = Map::new();
    let mut changed_users = std::collections::HashSet::new();
    for (target_user, keys) in users {
        let Some(keys) = keys.as_object() else {
            continue;
        };
        for (target_key_id, signed_key) in keys {
            match store_signature(
                &state,
                &user.user_id,
                target_user,
                target_key_id,
                signed_key,
                &permitted_signing_keys,
            )
            .await?
            {
                Ok(()) => {
                    changed_users.insert(target_user.clone());
                }
                Err(message) => {
                    let per_user = failures
                        .entry(target_user.clone())
                        .or_insert_with(|| json!({}));
                    per_user
                        .as_object_mut()
                        .expect("failure map is an object")
                        .insert(
                            target_key_id.clone(),
                            json!({
                                "errcode": "M_INVALID_SIGNATURE", "error": message,
                            }),
                        );
                }
            }
        }
    }
    for changed_user in changed_users {
        let stream_id = bicerin_storage::events::get_next_stream_id(&state.pool)
            .await
            .map_err(|error| BicerinError::Internal(error.to_string()))?;
        bicerin_storage::cross_signing::record_device_change(
            &state.pool,
            &changed_user,
            stream_id,
            "changed",
        )
        .await
        .map_err(|error| BicerinError::Internal(error.to_string()))?;
        state.sync_bus.notify_all(stream_id);
    }
    Ok(Json(json!({ "failures": failures })))
}

async fn store_signature(
    state: &AppState,
    signer_user: &str,
    target_user: &str,
    target_key_id: &str,
    signed_key: &Value,
    permitted_signing_keys: &HashMap<String, String>,
) -> BicerinResult<Result<(), String>> {
    let Some(signed_object) = signed_key.as_object() else {
        return Ok(Err("Signed key must be an object".into()));
    };
    let Some(signer_signatures) = signed_object
        .get("signatures")
        .and_then(Value::as_object)
        .and_then(|signatures| signatures.get(signer_user))
        .and_then(Value::as_object)
    else {
        return Ok(Err(
            "Signature upload contains no signatures from the authenticated user".into(),
        ));
    };
    if signer_signatures.is_empty()
        || signer_signatures.iter().any(|(key_id, signature)| {
            permitted_signing_keys
                .get(key_id)
                .is_none_or(|public_key| {
                    !verify_matrix_signature(
                        signed_key,
                        signer_user,
                        key_id,
                        public_key,
                    )
                })
                || !signature.is_string()
        })
    {
        return Ok(Err(
            "Signature is invalid or was not made by a key owned by the authenticated user"
                .into(),
        ));
    }

    let device_records =
        bicerin_storage::crypto::get_all_device_keys_for_user(&state.pool, target_user)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;
    if let Some(mut record) = device_records
        .into_iter()
        .find(|record| record.device_id == target_key_id)
    {
        if !signed_key_matches(&record.key_json, signed_key) {
            return Ok(Err(
                "Signed object does not match the uploaded device key".into()
            ));
        }
        for (key_id, signature) in signer_signatures {
            merge_signature(
                &mut record.key_json,
                signer_user,
                key_id,
                signature.as_str().expect("checked string"),
            );
        }
        record.updated_at = chrono::Utc::now();
        bicerin_storage::crypto::upsert_device_keys(&state.pool, &record)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;
        return Ok(Ok(()));
    }

    let cross_signing_records = bicerin_storage::cross_signing::get_keys(&state.pool, target_user)
        .await
        .map_err(|e| BicerinError::Internal(e.to_string()))?;
    if let Some(mut record) = cross_signing_records
        .into_iter()
        .find(|record| cross_signing_key_id(&record.key_json) == Some(target_key_id))
    {
        if !signed_key_matches(&record.key_json, signed_key) {
            return Ok(Err(
                "Signed object does not match the uploaded cross-signing key".into(),
            ));
        }
        for (key_id, signature) in signer_signatures {
            merge_signature(
                &mut record.key_json,
                signer_user,
                key_id,
                signature.as_str().expect("checked string"),
            );
        }
        record.updated_at = chrono::Utc::now();
        bicerin_storage::cross_signing::upsert_key(&state.pool, &record)
            .await
            .map_err(|e| BicerinError::Internal(e.to_string()))?;
        return Ok(Ok(()));
    }
    Ok(Err("Unknown signing key".into()))
}

fn signed_key_matches(stored: &Value, uploaded: &Value) -> bool {
    fn without_signatures(value: &Value) -> Value {
        let mut value = value.clone();
        if let Some(object) = value.as_object_mut() {
            object.remove("signatures");
        }
        value
    }
    without_signatures(stored) == without_signatures(uploaded)
}

fn cross_signing_key_id(key: &Value) -> Option<&str> {
    key.get("keys")?.as_object()?.values().next()?.as_str()
}

fn merge_signature(key: &mut Value, signer_user: &str, signer_key_id: &str, signature: &str) {
    let signatures = key
        .as_object_mut()
        .expect("validated key object")
        .entry("signatures")
        .or_insert_with(|| json!({}));
    let by_user = signatures
        .as_object_mut()
        .expect("signatures object")
        .entry(signer_user)
        .or_insert_with(|| json!({}));
    let by_key = by_user
        .as_object_mut()
        .expect("user signatures object")
        .entry(signer_key_id)
        .or_insert_with(|| json!({}));
    *by_key = Value::String(signature.to_string());
}

fn visible_cross_signing_key(
    mut key: Value,
    key_type: &str,
    requester_user: &str,
    owner_user: &str,
    owner_device_key_ids: &std::collections::HashSet<String>,
    owner_master_key_id: Option<&str>,
) -> Value {
    if requester_user == owner_user {
        return key;
    }
    if let Some(signatures) = key.get_mut("signatures").and_then(Value::as_object_mut) {
        signatures.retain(|signer, signatures| {
            if signer == requester_user {
                return true;
            }
            if signer != owner_user {
                return false;
            }
            let Some(signatures) = signatures.as_object_mut() else {
                return false;
            };
            signatures.retain(|key_id, _| match key_type {
                "master" => owner_device_key_ids.contains(key_id),
                "self_signing" => Some(key_id.as_str()) == owner_master_key_id,
                _ => false,
            });
            !signatures.is_empty()
        });
    }
    key
}

#[derive(Debug, Deserialize)]
pub struct KeysChangesQuery {
    pub from: String,
    pub to: String,
}

pub async fn keys_changes(
    user: AuthUser,
    State(state): State<AppState>,
    Query(query): Query<KeysChangesQuery>,
) -> BicerinResult<Json<Value>> {
    let from = bicerin_sync::token::SyncToken::parse(&query.from)
        .ok_or_else(|| BicerinError::BadRequest("invalid from token".into()))?
        .position();
    let to = bicerin_sync::token::SyncToken::parse(&query.to)
        .ok_or_else(|| BicerinError::BadRequest("invalid to token".into()))?
        .position();
    let current = bicerin_storage::sync::get_current_stream_position(&state.pool)
        .await.map_err(|e| BicerinError::Internal(e.to_string()))?;
    if to < from || to > current {
        return Err(BicerinError::BadRequest(
            "invalid token range".into(),
        ));
    }
    let updates = bicerin_sync::device_lists::get_device_list_updates(
        &state.pool, &user.user_id, from, to,
    ).await?;
    Ok(Json(json!({
        "changed": updates.changed,
        "left": updates.left
    })))
}

#[cfg(test)]
mod extended_tests {
    use super::{
        canonical_signed_payload, cross_signing_id_collides, is_signed_by_cross_signing_key,
        merge_signature, split_key_id, validate_cross_signing_key, validate_device_key_upload,
        verify_matrix_signature, visible_cross_signing_key,
    };
    use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine as _};
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::json;

    fn sign_json(value: &mut serde_json::Value, user_id: &str, key_id: &str, key: &SigningKey) {
        let signature = key.sign(&canonical_signed_payload(value).unwrap());
        value["signatures"][user_id][key_id] =
            json!(STANDARD_NO_PAD.encode(signature.to_bytes()));
    }

    #[test]
    fn uploads_must_match_authenticated_device_and_matrix_key_shape() {
        let key = json!({
            "user_id": "@alice:example.org", "device_id": "DEVICE1", "algorithms": ["m.olm.v1.curve25519-aes-sha2"],
            "keys": {"ed25519:DEVICE1": "public"}, "signatures": {"@alice:example.org": {"ed25519:DEVICE1": "signature"}}
        });
        assert!(validate_device_key_upload(&key, "@alice:example.org", "DEVICE1").is_ok());
        assert!(validate_device_key_upload(&key, "@bob:example.org", "DEVICE1").is_err());
        assert!(validate_device_key_upload(&key, "@alice:example.org", "OTHER").is_err());

        let mut missing_self_signature = key.clone();
        missing_self_signature["signatures"]["@alice:example.org"].as_object_mut().unwrap().clear();
        assert!(validate_device_key_upload(&missing_self_signature, "@alice:example.org", "DEVICE1").is_err());
    }

    #[test]
    fn cross_signing_keys_are_bound_to_their_owner_and_usage() {
        let key = json!({"user_id":"@alice:example.org","usage":["master"],"keys":{"ed25519:abc":"abc"},"signatures":{"@alice:example.org":{"ed25519:DEVICE":"sig"}}});
        assert!(validate_cross_signing_key(&key, "@alice:example.org", "master").is_ok());
        assert!(validate_cross_signing_key(&key, "@bob:example.org", "master").is_err());
        assert!(validate_cross_signing_key(&key, "@alice:example.org", "self_signing").is_err());
    }

    #[test]
    fn self_and_user_signing_keys_must_be_signed_by_the_master_key() {
        let master = SigningKey::from_bytes(&[7; 32]);
        let master_public = STANDARD_NO_PAD.encode(master.verifying_key().to_bytes());
        let master_key_id = format!("ed25519:{master_public}");
        let self_signing = json!({
            "user_id":"@alice:example.org",
            "usage":["self_signing"],
            "keys":{"ed25519:selfpub":"selfpub"},
            "signatures":{}
        });
        let mut self_signing = self_signing;
        sign_json(&mut self_signing, "@alice:example.org", &master_key_id, &master);
        assert!(validate_cross_signing_key(&self_signing, "@alice:example.org", "self_signing").is_ok());
        assert!(is_signed_by_cross_signing_key(
            &self_signing,
            "@alice:example.org",
            &master_key_id,
            &master_public
        ));
        assert!(!is_signed_by_cross_signing_key(
            &self_signing,
            "@alice:example.org",
            "ed25519:othermaster",
            &master_public
        ));

        let mut empty_signature = self_signing.clone();
        empty_signature["signatures"]["@alice:example.org"][&master_key_id] = json!("");
        assert!(!is_signed_by_cross_signing_key(
            &empty_signature,
            "@alice:example.org",
            &master_key_id,
            &master_public
        ));
    }

    #[test]
    fn signatures_are_verified_over_canonical_json_and_ignore_unsigned_data() {
        let signer = SigningKey::from_bytes(&[19; 32]);
        let public = STANDARD_NO_PAD.encode(signer.verifying_key().to_bytes());
        let key_id = "ed25519:DEVICE";
        let mut key = json!({"user_id":"@alice:example.org","device_id":"DEVICE","keys":{"ed25519:DEVICE":public},"signatures":{}});
        sign_json(&mut key, "@alice:example.org", key_id, &signer);
        key["unsigned"] = json!({"device_display_name":"Laptop"});
        assert!(verify_matrix_signature(&key, "@alice:example.org", key_id, &public));
        key["device_id"] = json!("OTHER");
        assert!(!verify_matrix_signature(&key, "@alice:example.org", key_id, &public));
    }

    #[test]
    fn cross_signing_public_keys_cannot_reuse_a_device_id() {
        let key = json!({"keys":{"ed25519:DEVICE":"DEVICE"}});
        assert!(cross_signing_id_collides(
            &key,
            &std::collections::HashSet::from(["DEVICE".to_string()])
        ));
        assert!(!cross_signing_id_collides(
            &key,
            &std::collections::HashSet::from(["OTHER".to_string()])
        ));
    }

    #[test]
    fn cross_signing_query_filters_other_users_private_signatures() {
        let owner_device_key_ids = std::collections::HashSet::from(["ed25519:DEVICE".into()]);
        let master_key = json!({"signatures":{
            "@alice:example.org":{"ed25519:alice-user-signing":"alice signature"},
            "@bob:example.org":{"ed25519:DEVICE":"device signature","ed25519:bob-user-signing":"private signature"},
            "@carol:example.org":{"ed25519:CAROL":"unrelated signature"}
        }});
        let visible = visible_cross_signing_key(
            master_key,
            "master",
            "@alice:example.org",
            "@bob:example.org",
            &owner_device_key_ids,
            Some("ed25519:BOB_MASTER"),
        );
        assert_eq!(visible["signatures"]["@alice:example.org"]["ed25519:alice-user-signing"], "alice signature");
        assert_eq!(visible["signatures"]["@bob:example.org"]["ed25519:DEVICE"], "device signature");
        assert!(visible["signatures"]["@bob:example.org"].get("ed25519:bob-user-signing").is_none());
        assert!(visible["signatures"].get("@carol:example.org").is_none());
    }

    #[test]
    fn signing_keys_are_merged_without_discarding_other_signatures() {
        let mut key = json!({"signatures":{"@alice:example.org":{"ed25519:MASTER":"first"}}});
        merge_signature(&mut key, "@bob:example.org", "ed25519:MASTER", "second");
        assert_eq!(
            key["signatures"]["@alice:example.org"]["ed25519:MASTER"],
            "first"
        );
        assert_eq!(
            key["signatures"]["@bob:example.org"]["ed25519:MASTER"],
            "second"
        );
    }

    #[test]
    fn key_ids_must_name_an_algorithm_and_identifier() {
        assert_eq!(
            split_key_id("signed_curve25519:one").unwrap(),
            ("signed_curve25519", "one")
        );
        assert!(split_key_id("missing-separator").is_err());
        assert!(split_key_id("algorithm:").is_err());
    }
}
