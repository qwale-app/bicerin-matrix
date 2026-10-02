use crate::{extract::AuthUser, state::AppState};
use axum::{extract::{Path, State}, Json};
use bicerin_error::{BicerinError, BicerinResult};
use bicerin_storage::filters::UserFilterRecord;
use serde_json::Value;

fn ensure_own_user(authenticated_user: &str, requested_user: &str) -> BicerinResult<()> {
    if authenticated_user == requested_user {
        Ok(())
    } else {
        Err(BicerinError::Forbidden)
    }
}

pub async fn create_filter(
    user: AuthUser,
    State(state): State<AppState>,
    Path(requested_user): Path<String>,
    Json(filter_json): Json<Value>,
) -> BicerinResult<Json<Value>> {
    ensure_own_user(&user.user_id, &requested_user)?;
    if !filter_json.is_object()
        || serde_json::from_value::<bicerin_sync::filter::SyncFilter>(filter_json.clone()).is_err()
    {
        return Err(BicerinError::BadRequest("filter must be a JSON object".into()));
    }

    let filter_id = uuid::Uuid::new_v4().simple().to_string();
    bicerin_storage::filters::upsert_filter(
        &state.pool,
        &UserFilterRecord {
            user_id: requested_user,
            filter_id: filter_id.clone(),
            filter_json,
            created_at: chrono::Utc::now(),
        },
    )
    .await
    .map_err(|error| BicerinError::Internal(error.to_string()))?;
    Ok(Json(serde_json::json!({"filter_id": filter_id})))
}

pub async fn get_filter(
    user: AuthUser,
    State(state): State<AppState>,
    Path((requested_user, filter_id)): Path<(String, String)>,
) -> BicerinResult<Json<Value>> {
    ensure_own_user(&user.user_id, &requested_user)?;
    let filter = bicerin_storage::filters::get_filter(&state.pool, &requested_user, &filter_id)
        .await
        .map_err(|error| BicerinError::Internal(error.to_string()))?
        .ok_or(BicerinError::NotFound)?;
    Ok(Json(filter.filter_json))
}

#[cfg(test)]
mod tests {
    use super::ensure_own_user;

    #[test]
    fn filters_are_private_to_the_authenticated_user() {
        assert!(ensure_own_user("@alice:example.org", "@alice:example.org").is_ok());
        assert!(ensure_own_user("@alice:example.org", "@bob:example.org").is_err());
    }
}
