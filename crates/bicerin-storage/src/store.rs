use crate::db::StorageResult;
use mongodb::bson::doc;

/// Backing datastore for Bicerin. PostgreSQL remains the default/reference
/// backend; MongoDB (including externally-hosted clusters such as MongoDB
/// Atlas, reachable via a `mongodb+srv://` connection string) is supported
/// as an alternative. See DOCS.md's "Storage backends" section.
#[derive(Clone)]
pub enum Store {
    Postgres(sqlx::PgPool),
    Mongo(MongoBackend),
}

#[derive(Clone)]
pub struct MongoBackend {
    pub database: mongodb::Database,
}

impl Store {
    pub async fn connect_postgres(
        url: &str,
        max_connections: u32,
        min_connections: u32,
    ) -> StorageResult<Self> {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(max_connections)
            .min_connections(min_connections)
            .connect(url)
            .await?;
        Ok(Store::Postgres(pool))
    }

    /// Connects to MongoDB using a full connection string (`mongodb://` or
    /// `mongodb+srv://`). No assumption is made about the server being local
    /// — this is the expected path for e.g. a MongoDB Atlas cluster, where
    /// TLS and auth credentials are already encoded in the URI.
    pub async fn connect_mongo(uri: &str, database: &str) -> StorageResult<Self> {
        let client = mongodb::Client::with_uri_str(uri).await?;
        // Fail fast on a bad URI/unreachable cluster instead of discovering it
        // on the first real query.
        client
            .database(database)
            .run_command(doc! { "ping": 1 })
            .await?;
        Ok(Store::Mongo(MongoBackend {
            database: client.database(database),
        }))
    }

    /// Prepares the datastore for use: runs SQL migrations for Postgres, or
    /// ensures the expected indexes/counters exist for MongoDB. Safe to call
    /// on every startup (idempotent).
    pub async fn init_schema(&self) -> StorageResult<()> {
        match self {
            Store::Postgres(pool) => {
                sqlx::migrate!("../../migrations")
                    .run(pool)
                    .await
                    .map_err(|e| crate::db::StorageError::Internal(e.to_string()))?;
            }
            Store::Mongo(backend) => {
                crate::mongo_schema::ensure_indexes(&backend.database).await?;
            }
        }
        Ok(())
    }

    pub fn as_postgres(&self) -> Option<&sqlx::PgPool> {
        match self {
            Store::Postgres(pool) => Some(pool),
            Store::Mongo(_) => None,
        }
    }

    pub fn as_mongo(&self) -> Option<&mongodb::Database> {
        match self {
            Store::Postgres(_) => None,
            Store::Mongo(backend) => Some(&backend.database),
        }
    }
}
