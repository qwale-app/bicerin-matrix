use std::sync::Arc;

use bicerin_http::AppState;
use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "bicerin-server", about = "Bicerin Matrix homeserver")]
struct Args {
    /// Path to a config file (TOML/JSON/YAML). Falls back to defaults + BICERIN__* env vars.
    #[arg(short, long)]
    config: Option<String>,
}

/// The subset of a standard mautrix/matrix-appservice-bridge `registration.yaml`
/// that Bicerin needs. See https://spec.matrix.org/v1.19/application-service-api/#registration
#[derive(Debug, serde::Deserialize)]
struct RegistrationFile {
    id: String,
    url: String,
    as_token: String,
    hs_token: String,
    sender_localpart: String,
    #[serde(default)]
    namespaces: serde_yaml::Value,
    #[serde(default)]
    rate_limited: bool,
    #[serde(default)]
    protocols: Option<serde_yaml::Value>,
}

/// Loads and upserts every `registration.yaml` referenced by
/// `appservices.registrations` in the config, plus a convenience
/// comma-separated `BICERIN_APPSERVICE_REGISTRATIONS` env var (the generic
/// `config` crate doesn't cleanly support `Vec<String>` via env vars).
async fn load_appservice_registrations(
    store: &bicerin_storage::Store,
    config: &bicerin_config::BicerinConfig,
) -> anyhow::Result<()> {
    let mut paths = config.appservices.registrations.clone();
    if let Ok(extra) = std::env::var("BICERIN_APPSERVICE_REGISTRATIONS") {
        paths.extend(extra.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
    }

    for path in paths {
        let contents = tokio::fs::read_to_string(&path).await.map_err(|e| {
            anyhow::anyhow!("failed to read appservice registration {}: {}", path, e)
        })?;
        let reg: RegistrationFile = serde_yaml::from_str(&contents).map_err(|e| {
            anyhow::anyhow!("failed to parse appservice registration {}: {}", path, e)
        })?;

        let record = bicerin_storage::appservice::AppserviceRecord {
            id: reg.id,
            url: reg.url,
            as_token: reg.as_token,
            hs_token: reg.hs_token,
            sender_localpart: reg.sender_localpart,
            namespaces: serde_json::to_value(&reg.namespaces).unwrap_or(serde_json::json!({})),
            rate_limited: reg.rate_limited,
            protocols: reg.protocols.map(|p| serde_json::to_value(&p).unwrap_or(serde_json::Value::Null)),
            created_at: chrono::Utc::now(),
        };

        tracing::info!(appservice_id = %record.id, url = %record.url, registration = %path, "registered appservice");
        bicerin_storage::appservice::upsert_appservice(store, &record).await?;
    }

    Ok(())
}

/// Background loop that delivers queued application-service transactions
/// (persisted by `bicerin_events::appservice_dispatch`) to each bridge's
/// `/_matrix/app/v1/transactions/{txnId}` endpoint, retrying with capped
/// exponential backoff on failure. Never blocks client requests.
async fn run_appservice_delivery_worker(store: bicerin_storage::Store) {
    let client = reqwest::Client::new();
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));

    loop {
        interval.tick().await;

        let appservices = match bicerin_storage::appservice::list_appservices(&store).await {
            Ok(list) => list,
            Err(e) => {
                tracing::warn!(error = %e, "appservice delivery worker: failed to list appservices");
                continue;
            }
        };

        for appservice in appservices {
            let transactions = match bicerin_storage::appservice::get_pending_transactions(&store, &appservice.id, 10).await {
                Ok(txns) => txns,
                Err(e) => {
                    tracing::warn!(error = %e, appservice_id = %appservice.id, "appservice delivery worker: failed to list pending transactions");
                    continue;
                }
            };

            for txn in transactions {
                let url = format!(
                    "{}/_matrix/app/v1/transactions/{}",
                    appservice.url.trim_end_matches('/'),
                    txn.transaction_id
                );

                let result = client
                    .put(&url)
                    .bearer_auth(&appservice.hs_token)
                    .json(&txn.payload)
                    .send()
                    .await;

                let delivered = matches!(&result, Ok(resp) if resp.status().is_success());

                if delivered {
                    if let Err(e) = bicerin_storage::appservice::mark_transaction_delivered(&store, &txn.transaction_id).await {
                        tracing::warn!(error = %e, transaction_id = %txn.transaction_id, "failed to mark appservice transaction delivered");
                    }
                    continue;
                }

                if let Err(e) = &result {
                    tracing::warn!(error = %e, appservice_id = %appservice.id, transaction_id = %txn.transaction_id, "appservice transaction delivery failed");
                } else if let Ok(resp) = &result {
                    tracing::warn!(status = %resp.status(), appservice_id = %appservice.id, transaction_id = %txn.transaction_id, "appservice transaction delivery rejected");
                }

                // Capped exponential backoff: 2s, 4s, 8s, ... up to 5 minutes.
                let backoff_secs = 2u64.saturating_pow((txn.attempts + 1).min(8) as u32).min(300);
                let next_retry_at = chrono::Utc::now() + chrono::Duration::seconds(backoff_secs as i64);
                if let Err(e) = bicerin_storage::appservice::increment_transaction_attempts(&store, &txn.transaction_id, next_retry_at).await {
                    tracing::warn!(error = %e, transaction_id = %txn.transaction_id, "failed to record appservice transaction retry");
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();
    let config = bicerin_config::BicerinConfig::load(args.config.as_deref())?;

    tracing::info!(server_name = %config.server.server_name, bind = %config.server.bind, backend = %config.database.backend, "starting bicerin-server");

    let store = if config.database.is_mongo() {
        let uri = config.database.mongo_uri.as_deref().ok_or_else(|| {
            anyhow::anyhow!("database.backend is \"mongodb\" but database.mongo_uri is not set")
        })?;
        bicerin_storage::Store::connect_mongo(uri, &config.database.mongo_database).await?
    } else {
        bicerin_storage::Store::connect_postgres(
            &config.database.url,
            config.database.max_connections,
            config.database.min_connections,
        ).await?
    };

    store.init_schema().await?;
    load_appservice_registrations(&store, &config).await?;

    let auth = Arc::new(bicerin_auth::AuthService::new(store.clone(), config.server.server_name.clone()));
    let rooms = Arc::new(bicerin_rooms::RoomService::new(store.clone()));
    let events = Arc::new(bicerin_events::EventService::new(store.clone(), config.server.server_name.clone()));
    let sync_bus = Arc::new(bicerin_sync::subscriptions::SyncBus::new(4096));
    let sync = Arc::new(bicerin_sync::SyncService::new(store.clone(), sync_bus.clone(), config.server.server_name.clone()));

    let media_root = config.media.local_path.clone().unwrap_or_else(|| "./media".to_string());
    tokio::fs::create_dir_all(&media_root).await.ok();
    let media = Arc::new(bicerin_media::MediaService::new(
        store.clone(),
        media_root,
        config.server.server_name.clone(),
        config.media.max_upload_size_bytes,
    ));

    tokio::spawn(run_appservice_delivery_worker(store.clone()));

    let state = AppState {
        pool: store,
        auth,
        rooms,
        events,
        sync,
        sync_bus,
        media,
        server_name: config.server.server_name.clone(),
        public_url: config.server.public_url.clone(),
        registration_enabled: config.matrix.registration_enabled,
        default_room_version: config.matrix.default_room_version.clone(),
        max_upload_size: config.media.max_upload_size_bytes,
    };

    let app = bicerin_http::router(state);

    let listener = tokio::net::TcpListener::bind(&config.server.bind).await?;
    tracing::info!(addr = %config.server.bind, "listening");
    axum::serve(listener, app).await?;

    Ok(())
}

