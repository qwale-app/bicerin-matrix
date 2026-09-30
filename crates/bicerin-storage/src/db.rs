#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("mongodb error: {0}")]
    Mongo(#[from] mongodb::error::Error),
    #[error("not found")]
    NotFound,
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("internal: {0}")]
    Internal(String),
}

pub type StorageResult<T> = Result<T, StorageError>;

/// True if a MongoDB error is a duplicate-key error (code 11000), i.e. the
/// Mongo equivalent of Postgres' `ON CONFLICT ... DO NOTHING`.
pub fn is_duplicate_key_error(err: &mongodb::error::Error) -> bool {
    use mongodb::error::ErrorKind;
    matches!(err.kind.as_ref(), ErrorKind::Write(mongodb::error::WriteFailure::WriteError(e)) if e.code == 11000)
}
