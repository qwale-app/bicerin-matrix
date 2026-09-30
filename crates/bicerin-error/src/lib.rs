use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BicerinError {
    #[error("Not Found")]
    NotFound,
    #[error("Unauthorized")]
    Unauthorized,
    #[error("Forbidden")]
    Forbidden,
    #[error("Bad Request: {0}")]
    BadRequest(String),
    #[error("Rate Limited")]
    RateLimited,
    #[error("Internal Server Error: {0}")]
    Internal(String),
    #[error("Database Error: {0}")]
    DatabaseError(String),
    #[error("Matrix Error: {error}")]
    MatrixError { errcode: String, error: String },
    /// A User-Interactive Authentication challenge (401 with flows/session, no errcode).
    #[error("User-Interactive Authentication required")]
    UiaRequired(serde_json::Value),
}

impl IntoResponse for BicerinError {
    fn into_response(self) -> Response {
        if let BicerinError::UiaRequired(body) = self {
            return (StatusCode::UNAUTHORIZED, Json(body)).into_response();
        }

        let (status, errcode, error_message) = match self {
            BicerinError::NotFound => (
                StatusCode::NOT_FOUND,
                "M_NOT_FOUND".to_string(),
                "Not found".to_string(),
            ),
            BicerinError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "M_UNKNOWN_TOKEN".to_string(),
                "Unrecognized access token".to_string(),
            ),
            BicerinError::Forbidden => (
                StatusCode::FORBIDDEN,
                "M_FORBIDDEN".to_string(),
                "Forbidden".to_string(),
            ),
            BicerinError::BadRequest(msg) => (
                StatusCode::BAD_REQUEST,
                "M_BAD_JSON".to_string(),
                msg,
            ),
            BicerinError::RateLimited => (
                StatusCode::TOO_MANY_REQUESTS,
                "M_LIMIT_EXCEEDED".to_string(),
                "Too many requests".to_string(),
            ),
            BicerinError::Internal(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "M_UNKNOWN".to_string(),
                msg,
            ),
            BicerinError::DatabaseError(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "M_UNKNOWN".to_string(),
                format!("Database error: {}", msg),
            ),
            BicerinError::MatrixError { errcode, error } => (
                match errcode.as_str() {
                    "M_NOT_FOUND" => StatusCode::NOT_FOUND,
                    "M_UNKNOWN_TOKEN" => StatusCode::UNAUTHORIZED,
                    "M_FORBIDDEN" => StatusCode::FORBIDDEN,
                    "M_LIMIT_EXCEEDED" => StatusCode::TOO_MANY_REQUESTS,
                    "M_BAD_JSON" => StatusCode::BAD_REQUEST,
                    "M_USER_IN_USE" | "M_INVALID_USERNAME" => StatusCode::BAD_REQUEST,
                    "M_USER_DEACTIVATED" => StatusCode::FORBIDDEN,
                    "M_TOO_LARGE" => StatusCode::PAYLOAD_TOO_LARGE,
                    _ => StatusCode::INTERNAL_SERVER_ERROR, // fallback
                },
                errcode,
                error,
            ),
            BicerinError::UiaRequired(_) => unreachable!("handled above"),
        };

        let body = Json(json!({
            "errcode": errcode,
            "error": error_message,
        }));

        (status, body).into_response()
    }
}

pub type BicerinResult<T> = Result<T, BicerinError>;
