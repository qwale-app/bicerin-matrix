use std::sync::Arc;

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
    pub default_room_version: String,
    pub max_upload_size: u64,
}
