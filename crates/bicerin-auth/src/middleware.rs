use axum::http::request::Parts;

// AuthenticatedUser is an extractor that pulls the Bearer token and validates it.
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub user_id: String,
    pub device_id: String,
}

// We use a wrapper type to avoid orphan rules:
// Implement FromRequestParts<AppState> in bicerin-http instead.
// This module just provides the struct and helper.

impl AuthenticatedUser {
    /// Extract Bearer token from Authorization header.
    pub fn extract_bearer(parts: &Parts) -> Option<String> {
        let auth_header = parts.headers.get(axum::http::header::AUTHORIZATION)?;
        let value = auth_header.to_str().ok()?;
        value.strip_prefix("Bearer ").map(|t| t.to_owned())
    }
}
