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

/// Implements incremental/initial `/sync` as described in designplan.txt
/// sections 23-25. Named filters (by ID) are not implemented; only the
/// unfiltered timeline/state is returned. See BICERIN_MATRIX_API.md.
pub async fn sync(user: AuthUser, State(state): State<AppState>, Query(query): Query<SyncQuery>) -> BicerinResult<Json<Value>> {
    let since = if query.full_state { None } else { query.since };
    let timeout_ms = query.timeout.unwrap_or(0);

    let response = state
        .sync
        .sync(&user.user_id, &user.device_id, since, timeout_ms, None)
        .await?;

    serde_json::to_value(response)
        .map(Json)
        .map_err(|e| BicerinError::Internal(e.to_string()))
}
