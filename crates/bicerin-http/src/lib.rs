pub mod extract;
pub mod handlers;
pub mod state;
pub mod util;

pub use state::AppState;

use axum::{
    routing::{get, post, put},
    Router,
};
use tower_http::{cors::{Any, CorsLayer}, trace::TraceLayer};

/// Builds the Bicerin Matrix Client-Server API router.
///
/// This intentionally implements only the subset of the Client-Server API
/// described as "Phase 1/2 essential" in designplan.txt; see
/// BICERIN_MATRIX_API.md for the full endpoint compatibility matrix.
pub fn router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let client_routes = Router::new()
        .route("/versions", get(handlers::auth::get_versions))
        .route("/client/v3/login", get(handlers::auth::get_login_flows).post(handlers::auth::login))
        .route("/client/v3/logout", post(handlers::auth::logout))
        .route("/client/v3/logout/all", post(handlers::auth::logout_all))
        .route("/client/v3/register", post(handlers::auth::register))
        .route("/client/v3/register/available", get(handlers::auth::register_available))
        .route("/client/v3/account/whoami", get(handlers::auth::whoami))
        .route(
            "/client/v3/profile/:userId",
            get(handlers::auth::get_profile),
        )
        .route(
            "/client/v3/profile/:userId/displayname",
            get(handlers::auth::get_display_name).put(handlers::auth::set_display_name),
        )
        .route(
            "/client/v3/profile/:userId/avatar_url",
            get(handlers::auth::get_avatar_url).put(handlers::auth::set_avatar_url),
        )
        .route("/client/v3/devices", get(handlers::auth::list_devices))
        .route(
            "/client/v3/devices/:deviceId",
            get(handlers::auth::get_device)
                .put(handlers::auth::update_device)
                .delete(handlers::auth::delete_device),
        )
        .route("/client/v3/createRoom", post(handlers::rooms::create_room))
        .route("/client/v3/join/:roomIdOrAlias", post(handlers::rooms::join_room))
        .route("/client/v3/rooms/:roomId/join", post(handlers::rooms::join_room))
        .route("/client/v3/rooms/:roomId/leave", post(handlers::rooms::leave_room))
        .route("/client/v3/rooms/:roomId/invite", post(handlers::rooms::invite_user))
        .route(
            "/client/v3/rooms/:roomId/send/:eventType/:txnId",
            put(handlers::rooms::send_event),
        )
        .route(
            "/client/v3/rooms/:roomId/state",
            get(handlers::rooms::get_room_state),
        )
        .route(
            "/client/v3/rooms/:roomId/state/:eventType",
            get(handlers::rooms::get_state_event_no_key).put(handlers::rooms::put_state_event_no_key),
        )
        .route(
            "/client/v3/rooms/:roomId/state/:eventType/:stateKey",
            get(handlers::rooms::get_state_event).put(handlers::rooms::put_state_event),
        )
        .route("/client/v3/rooms/:roomId/messages", get(handlers::rooms::get_messages))
        .route("/client/v3/rooms/:roomId/members", get(handlers::rooms::get_members))
        .route("/client/v3/sync", get(handlers::sync::sync))
        .route("/client/v3/keys/upload", post(handlers::keys::upload_keys))
        .route("/client/v3/keys/query", post(handlers::keys::query_keys))
        .route("/client/v3/keys/claim", post(handlers::keys::claim_keys));

    let media_routes = Router::new()
        .route("/v3/upload", post(handlers::media::upload_media))
        .route("/v3/download/:serverName/:mediaId", get(handlers::media::download_media))
        .route("/v3/config", get(handlers::media::media_config));

    Router::new()
        .route("/.well-known/matrix/client", get(handlers::auth::well_known_client))
        .nest("/_matrix/client", client_routes)
        .nest("/_matrix/media", media_routes)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
