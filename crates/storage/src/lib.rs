//! SQLite implementation of every repository port. This is the only crate in
//! the workspace allowed to know about rusqlite (V1_DEVELOPMENT.md §6).

mod backup;
mod conversation;
mod entity;
mod event;
mod infra;
mod project;
mod schema;

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use chrono::{DateTime, TimeZone, Utc};
use rusqlite::Connection;
use vibe_core::error::CoreError;
use vibe_core::ports::Repositories;

pub use schema::MIGRATIONS;

/// Latest schema version this build knows how to create. Derived from the
/// migration list so the two can never disagree.
pub const SCHEMA_VERSION: i64 = MIGRATIONS[MIGRATIONS.len() - 1].version;

pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Arc<Self>, CoreError> {
        let conn = Connection::open(path).map_err(storage_err)?;
        Self::prepare(conn).map(Arc::new)
    }

    pub fn open_in_memory() -> Result<Arc<Self>, CoreError> {
        let conn = Connection::open_in_memory().map_err(storage_err)?;
        Self::prepare(conn).map(Arc::new)
    }

    fn prepare(conn: Connection) -> Result<Self, CoreError> {
        // WAL keeps a workbench that is open while events stream in from an
        // adapter readable (V1_DEVELOPMENT.md §4.1).
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;")
            .map_err(storage_err)?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn migrate(&self) -> Result<(), CoreError> {
        let conn = self.conn()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
               version INTEGER PRIMARY KEY,
               name TEXT NOT NULL,
               applied_at TEXT NOT NULL
             );",
        )
        .map_err(storage_err)?;

        for migration in MIGRATIONS {
            let applied: bool = conn
                .query_row(
                    "SELECT COUNT(*) FROM schema_migrations WHERE version = ?1",
                    rusqlite::params![migration.version],
                    |row| row.get::<_, i64>(0).map(|c| c > 0),
                )
                .map_err(storage_err)?;
            if applied {
                continue;
            }
            let tx = conn.unchecked_transaction().map_err(storage_err)?;
            tx.execute_batch(migration.sql).map_err(storage_err)?;
            tx.execute(
                "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    migration.version,
                    migration.name,
                    to_ts(&vibe_domain::now())
                ],
            )
            .map_err(storage_err)?;
            tx.commit().map_err(storage_err)?;
        }
        Ok(())
    }

    pub fn schema_version(&self) -> Result<i64, CoreError> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(storage_err)
    }

    fn conn(&self) -> Result<MutexGuard<'_, Connection>, CoreError> {
        self.conn
            .lock()
            .map_err(|e| CoreError::Storage(format!("database lock poisoned: {e}")))
    }

    /// Hand the same connection out as a set of ports.
    pub fn repositories(self: &Arc<Self>) -> Repositories {
        Repositories {
            projects: self.clone(),
            events: self.clone(),
            conversations: self.clone(),
            entities: self.clone(),
            artifacts: self.clone(),
            evidence: self.clone(),
            snapshots: self.clone(),
            search: self.clone(),
            settings: self.clone(),
            audit: self.clone(),
            adapters: self.clone(),
        }
    }
}

pub(crate) fn storage_err(err: rusqlite::Error) -> CoreError {
    CoreError::Storage(err.to_string())
}

pub(crate) fn not_found(what: &str) -> CoreError {
    CoreError::NotFound {
        what: what.to_string(),
    }
}

pub(crate) fn to_ts(value: &DateTime<Utc>) -> String {
    value.to_rfc3339()
}

pub(crate) fn parse_ts(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .map(|parsed| parsed.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc.timestamp_opt(0, 0).unwrap())
}

pub(crate) fn json_col<T: serde::Serialize>(value: &T) -> Result<String, CoreError> {
    serde_json::to_string(value).map_err(|e| CoreError::Encoding(e.to_string()))
}

/// Row mappers return `rusqlite::Result`, so a bad JSON column has to travel as
/// a from-sql conversion failure rather than a serde error.
pub(crate) fn json_val<T: serde::de::DeserializeOwned>(
    raw: String,
) -> std::result::Result<T, rusqlite::Error> {
    serde_json::from_str(&raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

pub(crate) fn opt_text(row: &rusqlite::Row<'_>, index: usize) -> Option<String> {
    row.get::<_, Option<String>>(index).ok().flatten()
}

pub(crate) fn bool_col(row: &rusqlite::Row<'_>, index: usize) -> bool {
    row.get::<_, i64>(index).map(|v| v != 0).unwrap_or(false)
}

/// query_row returns an error for "no row"; ports want an Option.
pub(crate) trait OptionalExt<T> {
    fn optional(self) -> std::result::Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for rusqlite::Result<T> {
    fn optional(self) -> std::result::Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(other) => Err(other),
        }
    }
}

