//! §55 export: the six documents V1 must be able to give back to the user, plus the
//! `.projectpack` container the spec allows V1 to build out of zip + json + md.
//!
//! An export is a *projection*, never a second source of truth: everything here is
//! read from the event log through the same services the rest of the CLI uses, so a
//! pack written today re-creates exactly what the workbench believes today.

use std::io::{Cursor, Write};
use std::path::PathBuf;

use clap::Subcommand;
use vibe_core::error::{CoreError, Result};
use vibe_core::event_store::EventStore;
use vibe_core::project_service::ProjectService;
use vibe_state_engine::{ProjectMapCompiler, Reducer};
use zip::write::FileOptions;
use zip::CompressionMethod;
use zip::ZipWriter;

use crate::app::{write_file, App};
use crate::context::policy_summary;

#[derive(Subcommand)]
pub enum ExportCommand {
    /// The materialized Project State, as JSON.
    State { file: Option<PathBuf> },
    /// The PROJECT_MAP, as markdown (§30-§33).
    Map { file: Option<PathBuf> },
    /// Charter, scope and architecture baseline as one JSON document.
    Charter { file: Option<PathBuf> },
    /// Every decision the projection holds, active or not.
    Decisions { file: Option<PathBuf> },
    /// The AEP registry (§56).
    Aep { file: Option<PathBuf> },
    /// The accepted event log in `seq` order.
    Timeline { file: Option<PathBuf> },
    /// All six in one `.projectpack` zip.
    Pack {
        #[arg(default_value = "project.projectpack")]
        file: PathBuf,
    },
}

pub fn dispatch(app: &App, project: Option<&str>, command: ExportCommand) -> Result<()> {
    let id = app.project(project)?;
    let projects = ProjectService::new(app.repos.clone());
    match command {
        ExportCommand::State { file } => {
            let state = Reducer::new(app.repos.clone()).project_read_only(&id)?;
            write_or_print(app, json(&state)?, file.as_deref())
        }
        ExportCommand::Map { file } => {
            let map = ProjectMapCompiler::new(app.repos.clone()).compile(&id)?;
            write_or_print(app, map.markdown, file.as_deref())
        }
        ExportCommand::Charter { file } => {
            let document = serde_json::json!({
                "projectId": id,
                "charter": projects.charter(&id)?,
                "scope": projects.scope(&id)?,
                "architectureBaseline": projects.baseline(&id)?,
                "policy": policy_summary(&vibe_context_engine::policy::load(&app.repos, &id)?),
            });
            write_or_print(app, json(&document)?, file.as_deref())
        }
        ExportCommand::Decisions { file } => {
            let state = Reducer::new(app.repos.clone()).project_read_only(&id)?;
            write_or_print(
                app,
                json(&state.decisions)?,
                file.as_deref(),
            )
        }
        ExportCommand::Aep { file } => {
            let state = Reducer::new(app.repos.clone()).project_read_only(&id)?;
            write_or_print(
                app,
                json(&state.aeps)?,
                file.as_deref(),
            )
        }
        ExportCommand::Timeline { file } => {
            let events = EventStore::new(app.repos.clone()).list(&id, None, None)?;
            write_or_print(
                app,
                json(&events)?,
                file.as_deref(),
            )
        }
        ExportCommand::Pack { file } => {
            let reducer = Reducer::new(app.repos.clone());
            let state = reducer.project_read_only(&id)?;
            let map = ProjectMapCompiler::new(app.repos.clone()).compile(&id)?;
            let mut members: Vec<(String, Vec<u8>)> = vec![
                ("project-state.json".into(), json(&state)?.into_bytes()),
                ("project-map.md".into(), map.markdown.clone().into_bytes()),
                (
                    "charter.json".into(),
                    json(&serde_json::json!({
                        "charter": projects.charter(&id)?,
                        "scope": projects.scope(&id)?,
                        "architectureBaseline": projects.baseline(&id)?,
                    }))
                    .map(|text| text.into_bytes())?,
                ),
                ("decisions.json".into(), json(&state.decisions)?.into_bytes()),
                ("aep.json".into(), json(&state.aeps)?.into_bytes()),
                (
                    "timeline.json".into(),
                    json(&EventStore::new(app.repos.clone()).list(&id, None, None)?)?
                        .into_bytes(),
                ),
            ];
            members.insert(
                0,
                (
                    "manifest.json".into(),
                    json(&serde_json::json!({
                        "format": "projectpack",
                        "version": 1,
                        "projectId": id,
                        "projectName": projects.project(&id)?.name,
                        "stateHash": map.source_state_hash,
                        "members": members.iter().map(|(name, _)| name).collect::<Vec<_>>(),
                        // A pack is a projection: replaying it needs the event log,
                        // and the log is exactly what timeline.json carries.
                        "note": "projection of the accepted log, not a second source of truth",
                    }))
                    .map(|text| text.into_bytes())?,
                ),
            );
            let bytes = pack(&members)?;
            write_file(&file, &bytes)?;
            if app.json {
                return app.print_json(&serde_json::json!({
                    "file": file.display().to_string(),
                    "bytes": bytes.len(),
                    "members": members.iter().map(|(name, _)| name).collect::<Vec<_>>(),
                }));
            }
            app.say(format!(
                "{} ({} bytes, {} members)",
                file.display(),
                bytes.len(),
                members.len()
            ));
            Ok(())
        }
    }
}

fn json<T: serde::Serialize>(value: &T) -> Result<String> {
    Ok(serde_json::to_string_pretty(value).map_err(|e| CoreError::Encoding(e.to_string()))?)
}

/// Stored, not deflated: every member is JSON or markdown, and a reader that cannot
/// open a zip should still be able to `unzip` it with no codec installed.
fn pack(members: &[(String, Vec<u8>)]) -> Result<Vec<u8>> {
    let buffer = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(buffer);
    let options = FileOptions::<()>::default().compression_method(CompressionMethod::Stored);
    for (name, bytes) in members {
        zip.start_file(name, options)
            .map_err(|e| CoreError::Io(format!("cannot add {name}: {e}")))?;
        zip.write_all(bytes)
            .map_err(|e| CoreError::Io(format!("cannot write {name}: {e}")))?;
    }
    let bytes = zip
        .finish()
        .map_err(|e| CoreError::Io(format!("cannot close the pack: {e}")))?;
    Ok(bytes.into_inner())
}

fn write_or_print(app: &App, text: String, file: Option<&std::path::Path>) -> Result<()> {
    match file {
        Some(path) => {
            write_file(path, text.as_bytes())?;
            app.say(format!("{} ({} bytes)", path.display(), text.len()));
            Ok(())
        }
        None => {
            println!("{text}");
            Ok(())
        }
    }
}
