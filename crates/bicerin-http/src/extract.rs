use crate::state::AppState;
use axum::{extract::FromRequestParts, http::request::Parts};
use bicerin_error::BicerinError;

/// Extractor that authenticates a request via `Authorization: Bearer <token>`
/// or the deprecated `?access_token=` query parameter.
pub struct AuthUser {
    pub user_id: String,
    pub device_id: String,
    pub access_token: String,
}

#[axum::async_trait]
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = BicerinError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let token = extract_token(parts).ok_or_else(|| BicerinError::MatrixError {
            errcode: "M_MISSING_TOKEN".to_string(),
            error: "Missing access token".to_string(),
        })?;

        // Application services impersonate a user via `?user_id=`; see
        // AuthService::authenticate for identity assertion.
        let requested_user_id = extract_query_param(parts, "user_id");

        let (user_id, device_id) = state.auth.authenticate(&token, requested_user_id.as_deref()).await?;
        Ok(AuthUser { user_id, device_id, access_token: token })
    }
}

fn extract_token(parts: &Parts) -> Option<String> {
    if let Some(header) = parts.headers.get(axum::http::header::AUTHORIZATION) {
        if let Ok(value) = header.to_str() {
            if let Some(token) = value.strip_prefix("Bearer ") {
                return Some(token.to_string());
            }
        }
    }

    extract_query_param(parts, "access_token")
}

fn extract_query_param(parts: &Parts, name: &str) -> Option<String> {
    let query = parts.uri.query()?;
    for pair in query.split('&') {
        let mut kv = pair.splitn(2, '=');
        if let (Some(key), Some(value)) = (kv.next(), kv.next()) {
            if key == name {
                return Some(value.to_string());
            }
        }
    }
    None
}
