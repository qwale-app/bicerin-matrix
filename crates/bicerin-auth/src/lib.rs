pub mod middleware;
pub mod password;
pub mod token;

use bicerin_error::BicerinResult;
use bicerin_storage::Store;
use moka::future::Cache;
use std::time::Duration;

#[derive(Clone)]
pub struct AuthService {
    pub store: Store,
    pub server_name: String,
    pub token_cache: Cache<String, (String, String)>,
}

impl AuthService {
    pub fn new(store: Store, server_name: String) -> Self {
        let token_cache = Cache::builder()
            .max_capacity(100_000)
            .time_to_live(Duration::from_secs(5 * 60))
            .build();

        Self {
            store,
            server_name,
            token_cache,
        }
    }

    /// Authenticates a request's bearer/query-param token.
    ///
    /// `requested_user_id` is the optional `?user_id=` query parameter used by
    /// application services for [identity assertion]: if `token` matches a
    /// registered appservice's `as_token`, the request is treated as coming
    /// from that user (defaulting to the appservice's own bot user), and the
    /// user is auto-created ("ghost vivification") if it doesn't exist yet.
    ///
    /// [identity assertion]: https://spec.matrix.org/v1.19/application-service-api/#identity-assertion
    pub async fn authenticate(
        &self,
        token: &str,
        requested_user_id: Option<&str>,
    ) -> BicerinResult<(String, String)> {
        if let Some(ident) = self.token_cache.get(token).await {
            return Ok(ident);
        }

        if let Ok(appservice) =
            bicerin_storage::appservice::get_appservice_by_as_token(&self.store, token).await
        {
            return self
                .authenticate_as_appservice(&appservice, requested_user_id)
                .await;
        }

        let hashed_token = bicerin_types::auth::hash_access_token(token);

        let record =
            bicerin_storage::users::get_access_token(&self.store, &hashed_token.to_string())
                .await
                .map_err(|_| bicerin_error::BicerinError::Unauthorized)?;

        if let Some(expires_at) = record.expires_at {
            if expires_at < chrono::Utc::now() {
                return Err(bicerin_error::BicerinError::Unauthorized);
            }
        }

        let ident = (record.user_id, record.device_id);
        self.token_cache
            .insert(token.to_string(), ident.clone())
            .await;
        Ok(ident)
    }

    async fn authenticate_as_appservice(
        &self,
        appservice: &bicerin_storage::appservice::AppserviceRecord,
        requested_user_id: Option<&str>,
    ) -> BicerinResult<(String, String)> {
        let bot_user_id = bicerin_storage::appservice::bot_user_id(appservice, &self.server_name);
        let user_id = requested_user_id.unwrap_or(&bot_user_id).to_string();

        if !bicerin_storage::appservice::owns_user(appservice, &self.server_name, &user_id) {
            return Err(bicerin_error::BicerinError::MatrixError {
                errcode: "M_EXCLUSIVE".to_string(),
                error: format!("Appservice {} does not own user {}", appservice.id, user_id),
            });
        }

        self.ensure_appservice_user(&user_id).await?;

        // Appservice-impersonated requests don't have a real per-device
        // access token, so they're modeled as a single synthetic device.
        Ok((user_id, "APPSERVICE".to_string()))
    }

    async fn ensure_appservice_user(&self, user_id: &str) -> BicerinResult<()> {
        if bicerin_storage::users::get_user(&self.store, user_id)
            .await
            .is_ok()
        {
            return Ok(());
        }

        let localpart = user_id
            .strip_prefix('@')
            .and_then(|rest| rest.split(':').next())
            .unwrap_or(user_id)
            .to_string();

        bicerin_storage::users::create_user(
            &self.store,
            &bicerin_storage::users::UserRecord {
                user_id: user_id.to_string(),
                localpart,
                password_hash: None,
                display_name: None,
                avatar_url: None,
                is_guest: false,
                is_deactivated: false,
                created_at: chrono::Utc::now(),
            },
        )
        .await
        .map_err(|e| bicerin_error::BicerinError::Internal(e.to_string()))?;

        Ok(())
    }

    pub async fn invalidate_token(&self, token: &str) {
        self.token_cache.invalidate(token).await;
    }
}
