pub mod extract;
pub mod handlers;
pub mod metrics;
pub mod ratelimit;
pub mod server_keys;
pub mod state;
pub mod util;

pub use state::AppState;

use axum::{
    routing::{get, post, put},
    Router,
};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

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
        .route(
            "/v3/login",
            get(handlers::auth::get_login_flows).post(handlers::auth::login),
        )
        .route("/v3/logout", post(handlers::auth::logout))
        .route("/v3/logout/all", post(handlers::auth::logout_all))
        .route("/v3/register", post(handlers::auth::register))
        .route(
            "/v3/register/available",
            get(handlers::auth::register_available),
        )
        .route("/v3/account/whoami", get(handlers::auth::whoami))
        .route("/v3/profile/:userId", get(handlers::auth::get_profile))
        .route(
            "/v3/profile/:userId/displayname",
            get(handlers::auth::get_display_name).put(handlers::auth::set_display_name),
        )
        .route(
            "/v3/profile/:userId/avatar_url",
            get(handlers::auth::get_avatar_url).put(handlers::auth::set_avatar_url),
        )
        .route("/v3/devices", get(handlers::auth::list_devices))
        .route(
            "/v3/devices/:deviceId",
            get(handlers::auth::get_device)
                .put(handlers::auth::update_device)
                .delete(handlers::auth::delete_device),
        )
        .route("/v3/createRoom", post(handlers::rooms::create_room))
        .route("/v3/join/:roomIdOrAlias", post(handlers::rooms::join_room))
        .route("/v3/rooms/:roomId/join", post(handlers::rooms::join_room))
        .route("/v3/rooms/:roomId/leave", post(handlers::rooms::leave_room))
        .route(
            "/v3/rooms/:roomId/invite",
            post(handlers::rooms::invite_user),
        )
        .route("/v3/rooms/:roomId/kick", post(handlers::rooms::kick_user))
        .route("/v3/rooms/:roomId/ban", post(handlers::rooms::ban_user))
        .route("/v3/rooms/:roomId/unban", post(handlers::rooms::unban_user))
        .route(
            "/v3/rooms/:roomId/receipt/:receiptType/:eventId",
            post(handlers::ephemeral::send_receipt),
        )
        .route(
            "/v3/rooms/:roomId/read_markers",
            post(handlers::ephemeral::set_read_markers),
        )
        .route(
            "/v3/rooms/:roomId/typing/:userId",
            put(handlers::ephemeral::set_typing),
        )
        .route(
            "/v3/rooms/:roomId/send/:eventType/:txnId",
            put(handlers::rooms::send_event),
        )
        .route(
            "/v3/rooms/:roomId/state",
            get(handlers::rooms::get_room_state),
        )
        .route(
            "/v3/rooms/:roomId/state/:eventType",
            get(handlers::rooms::get_state_event_no_key)
                .put(handlers::rooms::put_state_event_no_key),
        )
        .route(
            "/v3/rooms/:roomId/state/:eventType/:stateKey",
            get(handlers::rooms::get_state_event).put(handlers::rooms::put_state_event),
        )
        .route(
            "/v3/rooms/:roomId/messages",
            get(handlers::rooms::get_messages),
        )
        .route(
            "/v3/rooms/:roomId/members",
            get(handlers::rooms::get_members),
        )
        .route(
            "/v3/rooms/:roomId/context/:eventId",
            get(handlers::rooms::get_context),
        )
        .route(
            "/v1/rooms/:roomId/relations/:eventId",
            get(handlers::rooms::get_relations),
        )
        .route(
            "/v1/rooms/:roomId/relations/:eventId/:relType",
            get(handlers::rooms::get_relations_by_type),
        )
        .route(
            "/v1/rooms/:roomId/relations/:eventId/:relType/:eventType",
            get(handlers::rooms::get_relations_by_type_and_event_type),
        )
        .route(
            "/v3/directory/room/:roomAlias",
            get(handlers::rooms::get_room_id_for_alias)
                .put(handlers::rooms::put_room_alias)
                .delete(handlers::rooms::delete_room_alias),
        )
        .route(
            "/v3/directory/list/room/:roomId",
            get(handlers::rooms::get_room_visibility).put(handlers::rooms::put_room_visibility),
        )
        .route("/v3/publicRooms", get(handlers::rooms::get_public_rooms))
        .route("/v3/sync", get(handlers::sync::sync))
        .route("/unstable/org.bicerin.ws", get(handlers::websocket::sync_socket))
        .route(
            "/v3/user/:userId/filter",
            post(handlers::filters::create_filter),
        )
        .route(
            "/v3/user/:userId/filter/:filterId",
            get(handlers::filters::get_filter),
        )
        .route(
            "/v3/user/:userId/account_data/:type",
            get(handlers::client_data::get_account_data)
                .put(handlers::client_data::put_account_data),
        )
        .route(
            "/v3/user/:userId/rooms/:roomId/account_data/:type",
            get(handlers::client_data::get_room_account_data)
                .put(handlers::client_data::put_room_account_data),
        )
        .route(
            "/v3/sendToDevice/:eventType/:txnId",
            put(handlers::client_data::send_to_device),
        )
        .route("/v3/keys/upload", post(handlers::keys::upload_keys))
        .route("/v3/keys/query", post(handlers::keys::query_keys))
        .route("/v3/keys/claim", post(handlers::keys::claim_keys));
    let client_routes = client_routes
        .route(
            "/v3/presence/:userId/status",
            get(handlers::presence::get_presence).put(handlers::presence::set_presence),
        )
        .route(
            "/v3/pushers",
            get(handlers::push::list_pushers),
        )
        .route("/v3/pushers/set", post(handlers::push::set_pusher))
        .route("/v3/pushrules/", get(handlers::push::get_push_rules))
        .route(
            "/v3/pushrules/global/:kind/:ruleId/enabled",
            get(handlers::push::get_rule_enabled).put(handlers::push::set_rule_enabled),
        )
        .route(
            "/v3/pushrules/global/:kind/:ruleId/actions",
            get(handlers::push::get_rule_actions).put(handlers::push::set_rule_actions),
        )
        .route(
            "/v3/pushrules/global/:kind/:ruleId",
            put(handlers::push::put_push_rule).delete(handlers::push::delete_push_rule),
        )
        .route(
            "/v1/appservice/:appserviceId/ping",
            post(handlers::appservice::ping_appservice),
        )
        .route("/v3/keys/changes", get(handlers::keys::keys_changes))
        .route(
            "/v3/keys/device_signing/upload",
            post(handlers::keys::upload_device_signing_keys),
        )
        .route(
            "/v3/keys/signatures/upload",
            post(handlers::keys::upload_signatures),
        )
        .route(
            "/v3/room_keys/version",
            get(handlers::key_backup::get_current_version)
                .post(handlers::key_backup::create_version),
        )
        .route(
            "/v3/room_keys/version/:version",
            get(handlers::key_backup::get_version)
                .put(handlers::key_backup::update_version)
                .delete(handlers::key_backup::delete_version),
        )
        .route(
            "/v3/room_keys/keys",
            get(handlers::key_backup::get_keys)
                .put(handlers::key_backup::put_keys)
                .delete(handlers::key_backup::delete_keys),
        )
        .route(
            "/v3/room_keys/keys/:roomId",
            get(handlers::key_backup::get_room_keys)
                .put(handlers::key_backup::put_room_keys)
                .delete(handlers::key_backup::delete_room_keys),
        )
        .route(
            "/v3/room_keys/keys/:roomId/:sessionId",
            get(handlers::key_backup::get_room_session)
                .put(handlers::key_backup::put_room_session)
                .delete(handlers::key_backup::delete_room_session),
        );

    let media_routes = Router::new()
        .route("/v3/upload", post(handlers::media::upload_media))
        .route(
            "/v3/download/:serverName/:mediaId",
            get(handlers::media::download_media),
        )
        .route("/v3/config", get(handlers::media::media_config));

    Router::new()
        .route(
            "/.well-known/matrix/client",
            get(handlers::auth::well_known_client),
        )
        .route("/_matrix/key/v2/server", get(server_keys::get_server_key))
        .route("/_bicerin/metrics", get(metrics::get_metrics))
        .route("/_bicerin/ws", get(handlers::websocket::sync_socket))
        .route("/_bicerin/admin/stats", get(handlers::admin::get_stats))
        .route("/_bicerin/admin/users", get(handlers::admin::list_users))
        .route(
            "/_bicerin/admin/users/:userId/deactivate",
            post(handlers::admin::deactivate_user),
        )
        .route(
            "/_bicerin/admin/register/nonce",
            get(handlers::admin::registration_nonce),
        )
        .route(
            "/_bicerin/admin/register",
            post(handlers::admin::shared_secret_register),
        )
        .nest("/_matrix/client", client_routes)
        .nest("/_matrix/media", media_routes)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            ratelimit::rate_limit_middleware,
        ))
        .layer(axum::middleware::from_fn(metrics::metrics_middleware))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
