use config::{Config, Environment, File};
use std::path::Path;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct BicerinConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub redis: RedisConfig,
    pub media: MediaConfig,
    pub matrix: MatrixConfig,
    #[serde(default)]
    pub appservices: AppserviceConfig,
    #[serde(default)]
    pub rate_limiting: RateLimitConfig,
    #[serde(default)]
    pub admin: AdminConfig,
}

/// Admin API (`/_bicerin/admin/*`) configuration. The whole admin API is
/// disabled unless `api_token` is set.
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct AdminConfig {
    pub api_token: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct RateLimitConfig {
    pub enabled: bool,
    /// Requests per minute per client IP for most endpoints.
    pub general_per_minute: u32,
    /// Requests per minute per client IP for `/login` and `/register`.
    pub sensitive_per_minute: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            general_per_minute: 300,
            sensitive_per_minute: 10,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ServerConfig {
    pub bind: String,        // e.g. "0.0.0.0:8448"
    pub server_name: String, // e.g. "example.com"
    pub public_url: String,  // base URL
    pub request_id_header: Option<String>,
    #[serde(default = "default_signing_key_path")]
    pub signing_key_path: String,
}

fn default_signing_key_path() -> String {
    "./signing.key".to_string()
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct DatabaseConfig {
    /// Which datastore backend to use: `"postgres"` (default) or `"mongodb"`.
    pub backend: String,
    /// PostgreSQL connection string. Only used when `backend = "postgres"`.
    pub url: String,
    pub max_connections: u32,
    pub min_connections: u32,
    /// MongoDB connection string. Only used when `backend = "mongodb"`. Accepts
    /// both `mongodb://` and `mongodb+srv://` (e.g. MongoDB Atlas) URIs; no
    /// assumption is made about the server being local. Credentials, TLS, and
    /// replica set/SRV discovery are all expressed in the URI itself.
    pub mongo_uri: Option<String>,
    /// Database name to use within the MongoDB cluster.
    pub mongo_database: String,
}

impl DatabaseConfig {
    pub fn is_mongo(&self) -> bool {
        self.backend.eq_ignore_ascii_case("mongodb") || self.backend.eq_ignore_ascii_case("mongo")
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct RedisConfig {
    pub url: String,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct MediaConfig {
    pub storage_backend: String, // "local" or "s3"
    pub local_path: Option<String>,
    pub s3_bucket: Option<String>,
    pub s3_endpoint: Option<String>,
    pub max_upload_size_bytes: u64,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct MatrixConfig {
    pub server_name: String,
    pub default_room_version: String,
    pub registration_enabled: bool,
    pub registration_shared_secret: Option<String>,
    #[serde(default)]
    pub guest_access_enabled: bool,
}

/// Application-service (bridge) registration. Paths to `registration.yaml`
/// files (the standard mautrix/matrix-appservice-bridge format) to load and
/// upsert into the datastore on every startup.
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct AppserviceConfig {
    #[serde(default)]
    pub registrations: Vec<String>,
}

impl Default for BicerinConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                bind: "0.0.0.0:8448".to_string(),
                server_name: "localhost".to_string(),
                public_url: "http://localhost:8448".to_string(),
                request_id_header: None,
                signing_key_path: default_signing_key_path(),
            },
            database: DatabaseConfig {
                backend: "postgres".to_string(),
                url: "postgres://postgres:postgres@localhost:5432/bicerin".to_string(),
                max_connections: 10,
                min_connections: 1,
                mongo_uri: None,
                mongo_database: "bicerin".to_string(),
            },
            redis: RedisConfig {
                url: "redis://localhost:6379/0".to_string(),
            },
            media: MediaConfig {
                storage_backend: "local".to_string(),
                local_path: Some("./media".to_string()),
                s3_bucket: None,
                s3_endpoint: None,
                max_upload_size_bytes: 50 * 1024 * 1024,
            },
            matrix: MatrixConfig {
                server_name: "localhost".to_string(),
                default_room_version: "10".to_string(),
                registration_enabled: false,
                registration_shared_secret: None,
                guest_access_enabled: false,
            },
            appservices: AppserviceConfig::default(),
            rate_limiting: RateLimitConfig::default(),
            admin: AdminConfig::default(),
        }
    }
}

impl BicerinConfig {
    pub fn load(config_path: Option<&str>) -> anyhow::Result<Self> {
        let mut builder = Config::builder();

        let default_config = serde_json::to_string(&BicerinConfig::default())?;
        builder = builder.add_source(config::File::from_str(
            &default_config,
            config::FileFormat::Json,
        ));

        if let Some(path) = config_path {
            if Path::new(path).exists() {
                builder = builder.add_source(File::with_name(path));
            } else {
                tracing::warn!("Config file {} not found, using defaults", path);
            }
        }

        builder = builder.add_source(Environment::with_prefix("BICERIN").separator("__"));

        let config = builder.build()?;
        let bicerin_config: BicerinConfig = config.try_deserialize()?;

        Ok(bicerin_config)
    }
}

#[cfg(test)]
mod tests {
    use super::{BicerinConfig, DatabaseConfig};

    #[test]
    fn defaults_select_local_postgres_and_matrix_server_settings() {
        let config = BicerinConfig::default();

        assert_eq!(config.database.backend, "postgres");
        assert_eq!(config.database.mongo_database, "bicerin");
        assert!(!config.database.is_mongo());
        assert_eq!(config.matrix.default_room_version, "10");
        assert!(!config.matrix.registration_enabled);
    }

    #[test]
    fn mongo_backend_recognizes_supported_names_case_insensitively() {
        for backend in ["mongo", "MONGO", "mongodb", "MongoDB"] {
            let config = DatabaseConfig {
                backend: backend.to_string(),
                url: String::new(),
                max_connections: 1,
                min_connections: 0,
                mongo_uri: Some("mongodb://localhost".to_string()),
                mongo_database: "bicerin_test".to_string(),
            };
            assert!(config.is_mongo(), "backend {backend} should select MongoDB");
        }
    }
}
