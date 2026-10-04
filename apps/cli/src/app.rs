//! Shared runtime for the `wp` workbench binary (V1_DEVELOPMENT.md §61).
//!
//! The CLI is not a second product surface: it is the thinnest possible harness
//! over the same services the workbench UI will call. Every command goes through
//! [`vibe_core::ports`], so anything printed here is reachable without a GUI.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use vibe_core::error::{CoreError, Result};
use vibe_core::ports::{DatabaseMaintenance, Repositories};
use vibe_storage::SqliteStore;

/// Settings key holding the last project the CLI acted on, so `--project` is not
/// needed on every call.
pub const POINTER_KEY: &str = "workbench.current_project";

pub struct App {
    pub repos: Repositories,
    /// Not part of [`Repositories`]: no project logic reads the database as a file,
    /// so only the workbench surface holds this handle (§55).
    pub maintenance: Arc<dyn DatabaseMaintenance>,
    pub db: PathBuf,
    pub json: bool,
}

/// Where the workbench database lives: `--db`, then `$WP_DB`, then a `.vibe/`
/// directory next to the current working directory. Nothing in here guesses a
/// vendor location — the path is always the caller's.
pub fn default_db() -> PathBuf {
    if let Ok(path) = std::env::var("WP_DB") {
        if !path.trim().is_empty() {
            return PathBuf::from(path);
        }
    }
    let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    base.join(".vibe").join("workbench.db")
}

impl App {
    pub fn open(db: &Path) -> Result<Self> {
        if let Some(parent) = db.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| CoreError::Io(format!("cannot create {}: {e}", parent.display())))?;
            }
        }
        let store = SqliteStore::open(db)?;
        Ok(Self {
            repos: store.repositories(),
            maintenance: store.clone(),
            db: db.to_path_buf(),
            json: false,
        })
    }

    /// Resolve the project a command acts on: an explicit id always wins, then the
    /// stored pointer, then the lone project if the database holds exactly one.
    pub fn project(&self, explicit: Option<&str>) -> Result<String> {
        if let Some(id) = explicit {
            if self.repos.projects.get_project(id)?.is_some() {
                return Ok(id.to_string());
            }
            return Err(CoreError::NotFound {
                what: format!("project {id}"),
            });
        }
        if let Some(value) = self.repos.settings.get_json(POINTER_KEY)? {
            if let Some(id) = value.as_str() {
                if self.repos.projects.get_project(id)?.is_some() {
                    return Ok(id.to_string());
                }
            }
        }
        let projects = self.repos.projects.list_projects()?;
        match projects.as_slice() {
            [only] => Ok(only.id.clone()),
            [] => Err(CoreError::Validation(
                "no project yet — run `wp project create --name <name>` first".into(),
            )),
            many => Err(CoreError::Validation(format!(
                "{} projects exist; pass --project <id> or run `wp project use <id>`",
                many.len()
            ))),
        }
    }

    pub fn remember_project(&self, id: &str) -> Result<()> {
        self.repos
            .settings
            .set_json(POINTER_KEY, &serde_json::Value::String(id.to_string()))
    }

    /// Machine-readable output on stdout, one document per command.
    pub fn print_json<T: Serialize>(&self, value: &T) -> Result<()> {
        println!(
            "{}",
            serde_json::to_string_pretty(value).map_err(|e| CoreError::Io(e.to_string()))?
        );
        Ok(())
    }

    /// Human output is suppressed under `--json` so a caller can pipe stdout.
    pub fn say(&self, line: impl std::fmt::Display) {
        if !self.json {
            println!("{line}");
        }
    }
}

/// Read a command's data from a JSON file rather than inventing a flag grammar
/// for every field of a charter.
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = std::fs::read(path)
        .map_err(|e| CoreError::NotFound {
            what: format!("{}: {e}", path.display()),
        })?;
    serde_json::from_slice(&bytes).map_err(|e| {
        CoreError::Encoding(format!("{} is not the expected JSON: {e}", path.display()))
    })
}

pub fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| CoreError::Io(format!("cannot create {}: {e}", parent.display())))?;
        }
    }
    std::fs::write(path, bytes)
        .map_err(|e| CoreError::Io(format!("cannot write {}: {e}", path.display())))
}
