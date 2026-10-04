//! §55 Recovery: online backup, verification and restore of the workbench database.
//!
//! SQLite's backup API is used in *both* directions. That is deliberate: a restore that
//! swapped files would be replacing a database this process still holds open, and a copy
//! made with `fs::copy` would freeze whatever the WAL had not yet checkpointed.

use std::path::Path;
use std::time::Duration;

use rusqlite::backup::Backup;
use rusqlite::{Connection, OpenFlags};
use vibe_core::error::{CoreError, Result};
use vibe_core::ports::{BackupReport, DatabaseMaintenance};

use crate::{not_found, storage_err, SqliteStore, SCHEMA_VERSION};

/// Pages per backup step. A workbench database is small; the number exists so the copy
/// makes progress in a handful of steps instead of one long lock.
const PAGES_PER_STEP: std::os::raw::c_int = 128;

fn integrity(conn: &Connection) -> Result<String> {
    conn.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
        .map_err(storage_err)
}

fn scalar(conn: &Connection, sql: &str) -> Result<i64> {
    conn.query_row(sql, [], |row| row.get::<_, i64>(0))
        .map_err(storage_err)
}

/// A backup is only worth restoring if it is a workbench database at all — an unrelated
/// SQLite file would otherwise replace project state with silence.
fn require_workbench(conn: &Connection, path: &Path) -> Result<()> {
    for table in ["schema_migrations", "events", "projects"] {
        let present: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                rusqlite::params![table],
                |row| row.get(0),
            )
            .map_err(storage_err)?;
        if present == 0 {
            return Err(CoreError::Validation(format!(
                "{} is not a workbench database — no '{table}' table in it",
                path.display()
            )));
        }
    }
    Ok(())
}

fn report(conn: &Connection) -> Result<BackupReport> {
    Ok(BackupReport {
        pages: scalar(conn, "PRAGMA page_count")?,
        page_size: scalar(conn, "PRAGMA page_size")?,
        schema_version: scalar(
            conn,
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        )?,
        events: scalar(conn, "SELECT COUNT(*) FROM events")?,
        projects: scalar(conn, "SELECT COUNT(*) FROM projects")?,
        integrity: integrity(conn)?,
    })
}

/// Read-only so a `verify` can never be the thing that damages a backup.
fn open_read_only(path: &Path) -> Result<Connection> {
    if !path.exists() {
        return Err(not_found(&format!("backup file {}", path.display())));
    }
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(storage_err)
}

fn copy(from: &Connection, to: &mut Connection) -> Result<()> {
    let backup = Backup::new(from, to).map_err(storage_err)?;
    backup
        .run_to_completion(PAGES_PER_STEP, Duration::ZERO, None)
        .map_err(storage_err)
}

impl DatabaseMaintenance for SqliteStore {
    fn backup_to(&self, dest: &Path) -> Result<BackupReport> {
        if let Some(parent) = dest.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    CoreError::Io(format!("cannot create {}: {e}", parent.display()))
                })?;
            }
        }
        let mut to = Connection::open(dest).map_err(storage_err)?;
        {
            let from = self.conn()?;
            copy(&from, &mut to)?;
        }
        // The report describes the copy, not the source: a backup is only proven by
        // reading it back.
        report(&to)
    }

    fn integrity_check(&self) -> Result<String> {
        let conn = self.conn()?;
        integrity(&conn)
    }

    fn inspect(&self, source: &Path) -> Result<BackupReport> {
        let from = open_read_only(source)?;
        require_workbench(&from, source)?;
        report(&from)
    }

    fn restore(&self, source: &Path) -> Result<BackupReport> {
        let from = open_read_only(source)?;
        require_workbench(&from, source)?;
        let wanted = report(&from)?;
        wanted.restorable(SCHEMA_VERSION).map_err(CoreError::Validation)?;
        {
            let mut to = self.conn()?;
            copy(&from, &mut to)?;
        }
        // The copied pages carry the backup's schema. Migrating forward here — rather
        // than waiting for the next open — keeps the live connection and the data it
        // now holds in agreement.
        self.migrate()?;
        let conn = self.conn()?;
        report(&conn)
    }

    fn schema_version(&self) -> Result<i64> {
        SqliteStore::schema_version(self)
    }
}
