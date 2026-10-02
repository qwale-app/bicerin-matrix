use crate::{extract::AuthUser, state::AppState};
use axum::{
    extract::{Query, State},
    Json,
};
use bicerin_error::{BicerinError, BicerinResult};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize, Default)]
pub struct SyncQuery {
    pub since: Option<String>,
    pub timeout: Option<u64>,
    #[allow(dead_code)]
    pub filter: Option<String>,
    #[serde(default)]
    pub full_state: bool,
}

pub async fn sync(
    user: AuthUser,
    State(state): State<AppState>,
    Query(query): Query<SyncQuery>,
) -> BicerinResult<Json<Value>> {
    let since = if query.full_state { None } else { query.since };
    let timeout_ms = query.timeout.unwrap_or(0);
    let filter = if let Some(filter) = query.filter {
        if filter.trim_start().starts_with('{') {
            let value: Value = serde_json::from_str(&filter)
                .map_err(|error| BicerinError::BadRequest(format!("invalid filter JSON: {error}")))?;
            Some(serde_json::from_value(value)
                .map_err(|error| BicerinError::BadRequest(format!("invalid sync filter: {error}")))?)
        } else {
            let record = bicerin_storage::filters::get_filter(
                &state.pool,
                &user.user_id,
                &filter,
            )
            .await
            .map_err(|error| BicerinError::Internal(error.to_string()))?
            .ok_or(BicerinError::NotFound)?;
            Some(serde_json::from_value(record.filter_json)
                .map_err(|error| BicerinError::Internal(format!("stored sync filter is invalid: {error}")))?)
        }
    } else {
        None
    };

    let response = state
        .sync
        .sync(&user.user_id, &user.device_id, since, timeout_ms, filter)
        .await?;

    serde_json::to_value(response)
        .map(Json)
        .map_err(|e| BicerinError::Internal(e.to_string()))
}
