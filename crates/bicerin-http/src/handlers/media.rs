use crate::{extract::AuthUser, state::AppState};
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderValue},
    response::{IntoResponse, Response},
    Json,
};
use bicerin_error::BicerinResult;
use bytes::Bytes;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize, Default)]
pub struct UploadQuery {
    pub filename: Option<String>,
}

pub async fn upload_media(
    user: AuthUser,
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(query): Query<UploadQuery>,
    body: Bytes,
) -> BicerinResult<Json<Value>> {
    let mime_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    let media_id = state
        .media
        .upload(&user.user_id, &mime_type, query.filename, body.to_vec())
        .await?;

    Ok(Json(
        json!({ "content_uri": format!("mxc://{}/{}", state.server_name, media_id) }),
    ))
}

pub async fn download_media(
    State(state): State<AppState>,
    Path((server_name, media_id)): Path<(String, String)>,
) -> BicerinResult<Response> {
    let (record, bytes) = state.media.download(&server_name, &media_id).await?;

    let mut response = Body::from(bytes).into_response();
    if let Ok(value) = HeaderValue::from_str(&record.mime_type) {
        response.headers_mut().insert(header::CONTENT_TYPE, value);
    }
    if let Some(name) = &record.upload_name {
        let sanitized = name.replace(['"', '\r', '\n'], "");
        if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{sanitized}\"")) {
            response
                .headers_mut()
                .insert(header::CONTENT_DISPOSITION, value);
        }
    }
    Ok(response)
}

pub async fn media_config(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "m.upload.size": state.max_upload_size }))
}
