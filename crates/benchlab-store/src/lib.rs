//! SQLite persistence, migrations, artifacts, and secret storage.
//!
//! SQLite runs on a dedicated thread owned by `tokio-rusqlite`. Public methods
//! are async and must be awaited from a background task, never from GPUI's UI
//! thread (SPEC.md §9.3).

use std::path::{Path, PathBuf};

use benchlab_core::{FieldError, now};
use chrono::SecondsFormat;
use tokio_rusqlite::{Connection, params};

mod environments;
pub mod paths;
mod projects;
mod scenarios;
pub mod secrets;
mod settings;

pub use paths::{AppPaths, InstanceLock, LockError, PathError};
pub use secrets::{
    KeyringSecretStore, SecretError, SecretPersistence, SecretStore, SessionSecretStore,
    default_secret_store, fingerprint,
};

/// Returns the application data directory name used by the store.
pub const DATA_DIR_NAME: &str = "BenchLab";

const MIGRATION_0001: &str = include_str!("../../../migrations/0001_init.sql");

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(String),
    #[error("database connection closed")]
    Closed,
    #[error("migration {version} failed: {message}")]
    Migration { version: i64, message: String },
    #[error("the database schema version {0} is newer than this build supports")]
    UnsupportedSchema(i64),
    #[error("not found")]
    NotFound,
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("invalid data: {0}")]
    Invalid(String),
    #[error("validation failed")]
    Validation(Vec<FieldError>),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        StoreError::Database(error.to_string())
    }
}

impl From<tokio_rusqlite::Error<StoreError>> for StoreError {
    fn from(error: tokio_rusqlite::Error<StoreError>) -> Self {
        match error {
            tokio_rusqlite::Error::Error(error) => error,
            _ => StoreError::Closed,
        }
    }
}

#[derive(Clone)]
pub struct Store {
    pub(crate) conn: Connection,
    path: Option<PathBuf>,
}

impl Store {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path).await?;
        initialize(&conn).await?;
        tracing::info!(path = %path.display(), "opened SQLite store");
        Ok(Self {
            conn,
            path: Some(path),
        })
    }

    pub async fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory().await?;
        initialize(&conn).await?;
        Ok(Self { conn, path: None })
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub(crate) async fn call<F, R>(&self, function: F) -> Result<R, StoreError>
    where
        F: FnOnce(&mut rusqlite::Connection) -> Result<R, StoreError> + Send + 'static,
        R: Send + 'static,
    {
        self.conn.call(function).await.map_err(StoreError::from)
    }
}

async fn initialize(conn: &Connection) -> Result<(), StoreError> {
    conn.call(|conn| {
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             PRAGMA busy_timeout=5000;
             PRAGMA synchronous=NORMAL;",
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
               version INTEGER PRIMARY KEY,
               applied_at TEXT NOT NULL
             );",
        )?;

        let version: i64 = conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?;

        if version > 1 {
            return Err(StoreError::UnsupportedSchema(version));
        }

        if version < 1 {
            let transaction = conn.transaction()?;
            if let Err(error) = transaction.execute_batch(MIGRATION_0001) {
                return Err(StoreError::Migration {
                    version: 1,
                    message: error.to_string(),
                });
            }
            transaction.execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                params![1_i64, now().to_rfc3339_opts(SecondsFormat::Millis, true)],
            )?;
            transaction
                .commit()
                .map_err(|error| StoreError::Migration {
                    version: 1,
                    message: error.to_string(),
                })?;
            tracing::info!("applied migration 0001_init");
        }

        Ok(())
    })
    .await
    .map_err(StoreError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_and_are_idempotent() {
        pollster::block_on(async {
            let store = Store::open_in_memory().await.unwrap();
            let version: i64 = store
                .call(|conn| {
                    Ok(
                        conn.query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                            row.get(0)
                        })?,
                    )
                })
                .await
                .unwrap();
            assert_eq!(version, 1);
        });
    }
}
