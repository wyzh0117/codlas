//! §55 / §62 Recovery: back the workbench database up, prove a file is whole, put it
//! back.
//!
//! A `.projectpack` is a projection of one project; a backup is the whole workbench —
//! every project, the settings, the audit log and the indexes. Both are needed, and
//! neither substitutes for the other, which is why Recovery is only satisfied when a
//! backup can be verified *and* restored.
//!
//! Secret values are deliberately not part of any of this: they live outside the
//! database (ADR-017), so a backup carries only `secret_ref` strings and a restore
//! never moves a credential.

use std::path::{Path, PathBuf};

use clap::Subcommand;
use vibe_core::error::{CoreError, Result};
use vibe_core::ports::BackupReport;

use crate::app::App;

#[derive(Subcommand)]
pub enum BackupCommand {
    /// Write a consistent copy of the live database and prove the copy is whole.
    Create {
        /// Destination file. Defaults to `<db dir>/backups/<db name>-<utc>.db`.
        file: Option<PathBuf>,
    },
    /// The live database's own verdict: integrity, schema version, size.
    Status,
    /// Read a backup file's report without touching the live database.
    Verify { file: PathBuf },
    /// Put a backup's content back into the live database.
    Restore {
        file: PathBuf,
        /// Acknowledge that everything recorded after this backup was taken is discarded.
        #[arg(long)]
        yes: bool,
    },
}

pub fn dispatch(app: &App, command: BackupCommand) -> Result<()> {
    match command {
        BackupCommand::Create { file } => {
            let live = app.maintenance.integrity_check()?;
            if live != "ok" {
                return Err(CoreError::Validation(format!(
                    "the live database is not whole ({live}) — refusing to copy it into a backup"
                )));
            }
            let dest = file.unwrap_or_else(|| default_backup_path(&app.db));
            let report = app.maintenance.backup_to(&dest)?;
            // The copy is only useful if this very build could put it back.
            report
                .restorable(app.maintenance.schema_version()?)
                .map_err(CoreError::Validation)?;
            emit(app, &dest, &report)
        }
        BackupCommand::Status => {
            let integrity = app.maintenance.integrity_check()?;
            let version = app.maintenance.schema_version()?;
            if app.json {
                return app.print_json(&serde_json::json!({
                    "db": app.db.display().to_string(),
                    "schemaVersion": version,
                    "integrity": integrity,
                    "secrets": "outside the database (ADR-017)",
                }));
            }
            app.say(format!(
                "{}\tschema v{version}\tintegrity {integrity}",
                app.db.display()
            ));
            if integrity != "ok" {
                return Err(CoreError::Validation(format!(
                    "the live database is not whole: {integrity}"
                )));
            }
            Ok(())
        }
        BackupCommand::Verify { file } => {
            let report = app.maintenance.inspect(&file)?;
            report
                .restorable(app.maintenance.schema_version()?)
                .map_err(CoreError::Validation)?;
            emit(app, &file, &report)
        }
        BackupCommand::Restore { file, yes } => {
            if !yes {
                return Err(CoreError::Validation(format!(
                    "`wp backup restore` replaces the content of {} — everything recorded \
                     after {} was written would be discarded. Re-run with --yes.",
                    app.db.display(),
                    file.display()
                )));
            }
            if is_same_file(&app.db, &file)? {
                return Err(CoreError::Validation(format!(
                    "{} is the live database; restoring it onto itself would prove nothing",
                    file.display()
                )));
            }
            // Restore is the recovery path, so a damaged live database must not block
            // it — the snapshot is attempted and its failure only reported.
            match app.maintenance.backup_to(&pre_restore_path(&app.db)) {
                Ok(_) => {}
                Err(e) => app.say(format!("warning: no snapshot of the current database: {e}")),
            }
            let report = app.maintenance.restore(&file)?;
            report
                .restorable(app.maintenance.schema_version()?)
                .map_err(CoreError::Validation)?;
            app.say(format!(
                "restored {} into {}",
                file.display(),
                app.db.display()
            ));
            emit(app, &file, &report)
        }
    }
}

/// `pages × page_size`, since SQLite reports the file in pages and a user reads bytes.
fn emit(app: &App, file: &Path, report: &BackupReport) -> Result<()> {
    if app.json {
        return app.print_json(&serde_json::json!({
            "file": file.display().to_string(),
            "report": report,
        }));
    }
    app.say(format!(
        "{}\t{} events / {} projects\tschema v{}\t{} KiB\tintegrity {}",
        file.display(),
        report.events,
        report.projects,
        report.schema_version,
        (report.pages * report.page_size) / 1024,
        report.integrity,
    ));
    Ok(())
}

fn stamp() -> String {
    vibe_domain::now().format("%Y%m%dT%H%M%SZ").to_string()
}

fn backups_dir(db: &Path) -> PathBuf {
    let parent = db
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    parent.join("backups")
}

fn named(db: &Path, prefix: &str) -> PathBuf {
    let stem = db
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "workbench".to_string());
    let base = format!("{stem}-{prefix}{}", stamp());
    let mut candidate = backups_dir(db).join(format!("{base}.db"));
    // Two backups in the same second must not silently replace each other.
    let mut n = 1;
    while candidate.exists() {
        candidate = backups_dir(db).join(format!("{base}-{n}.db"));
        n += 1;
    }
    candidate
}

fn default_backup_path(db: &Path) -> PathBuf {
    named(db, "")
}

fn pre_restore_path(db: &Path) -> PathBuf {
    named(db, "pre-restore-")
}

fn is_same_file(db: &Path, other: &Path) -> Result<bool> {
    let Ok(right) = std::fs::canonicalize(other) else {
        return Ok(false);
    };
    let left = std::fs::canonicalize(db).map_err(|e| CoreError::NotFound {
        what: format!("{}: {e}", db.display()),
    })?;
    Ok(left == right)
}
