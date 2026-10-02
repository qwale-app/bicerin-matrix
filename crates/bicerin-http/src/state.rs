use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone)]
pub struct AppState {
    /// The active datastore backend (PostgreSQL or MongoDB); see
    /// `bicerin_storage::Store`. Named `pool` for historical reasons even
    /// though it may hold a Mongo connection rather than a Postgres pool.
    pub pool: bicerin_storage::Store,
    pub auth: Arc<bicerin_auth::AuthService>,
    pub rooms: Arc<bicerin_rooms::RoomService>,
    pub events: Arc<bicerin_events::EventService>,
    pub sync: Arc<bicerin_sync::SyncService>,
    pub sync_bus: Arc<bicerin_sync::subscriptions::SyncBus>,
    pub media: Arc<bicerin_media::MediaService>,
    pub server_name: String,
    pub public_url: String,
    pub registration_enabled: bool,
    pub guest_access_enabled: bool,
    pub registration_shared_secret: Option<String>,
    pub default_room_version: String,
    pub max_upload_size: u64,
    pub rate_limiter: Arc<crate::ratelimit::RateLimiter>,
    pub signing_key: Arc<crate::server_keys::ServerSigningKey>,
    pub metrics_handle: Arc<metrics_exporter_prometheus::PrometheusHandle>,
    pub admin_api_token: Option<String>,
    /// Single-use nonces for shared-secret registration (`/_bicerin/admin/register`).
    pub registration_nonces: Arc<Mutex<HashMap<String, Instant>>>,
}

