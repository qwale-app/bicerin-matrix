use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use bicerin_error::{BicerinError, BicerinResult};
use serde_json::Value;

/// `POST /_matrix/client/v1/appservice/{appserviceId}/ping` (MSC2659):
/// authenticated with the appservice's own `as_token`, triggers an outbound
/// `POST {as_url}/_matrix/app/v1/ping` and reports the round-trip time, so a
/// bridge can verify its registration is correctly wired up.
pub async fn ping_appservice(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(appservice_id): Path<String>,
    Json(body): Json<Value>,
) -> BicerinResult<Json<Value>> {
    let as_token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(BicerinError::Unauthorized)?;

    let appservice =
        bicerin_storage::appservice::get_appservice_by_as_token(&state.pool, as_token)
            .await
            .map_err(|_| BicerinError::Forbidden)?;
    if appservice.id != appservice_id {
        return Err(BicerinError::Forbidden);
    }

    let url = format!("{}/_matrix/app/v1/ping", appservice.url.trim_end_matches('/'));
    let start = std::time::Instant::now();
    let client = reqwest::Client::new();
    let result = client
        .post(&url)
        .bearer_auth(&appservice.hs_token)
        .json(&body)
        .send()
        .await;

    match result {
        Ok(resp) if resp.status().is_success() => Ok(Json(serde_json::json!({
            "duration_ms": start.elapsed().as_millis() as u64,
        }))),
        Ok(resp) => Err(BicerinError::MatrixError {
            errcode: "M_CONNECTION_FAILED".to_string(),
            error: format!("Ping returned status {}", resp.status()),
        }),
        Err(e) => Err(BicerinError::MatrixError {
            errcode: "M_CONNECTION_FAILED".to_string(),
            error: e.to_string(),
        }),
    }
}
