//! The commands the webview may call — eleven reads and one verdict.
//!
//! Every handler resolves the current project through [`App::project`] and then hands the work
//! to a pure builder in [`crate::views`]. Errors come back as the engine's own sentence: the
//! shell does not translate a refusal into a blank screen, and it never invents a fallback state
//! (§62's Degradation gate).

use std::result::Result;

use tauri::State;
use vibe_core::error::CoreError;
use wp_desktop::views::{
    self, ApplyResult, ContextView, DecisionsView, DiffView, DriftView, EvolutionView, HealthView,
    MapView, OverviewView, PendingChoice, ProjectView, SearchRow, TimelineView,
};
use wp_desktop::workspace::Shell;

#[tauri::command]
pub fn project_info(shell: State<'_, Shell>) -> Result<ProjectView, String> {
    let id = project(shell.inner())?;
    views::project_view(&shell.app, &id).map_err(plain)
}

#[tauri::command]
pub fn diff_view(shell: State<'_, Shell>) -> Result<DiffView, String> {
    let id = project(shell.inner())?;
    views::diff_view(&shell.app, &id).map_err(plain)
}

/// §7.4: the user ticking a box is the confirmation. Everything else in this shell is read-only.
#[tauri::command]
pub fn apply_pending(shell: State<'_, Shell>, choices: Vec<PendingChoice>) -> Result<ApplyResult, String> {
    let id = project(shell.inner())?;
    Ok(views::apply(&shell.app, &id, &choices))
}

#[tauri::command]
pub fn map_view(shell: State<'_, Shell>) -> Result<MapView, String> {
    let id = project(shell.inner())?;
    views::map_view(&shell.app, &id).map_err(plain)
}

#[tauri::command]
pub fn overview_view(shell: State<'_, Shell>) -> Result<OverviewView, String> {
    let id = project(shell.inner())?;
    views::overview_view(&shell.app, &id).map_err(plain)
}

#[tauri::command]
pub fn decisions_view(shell: State<'_, Shell>) -> Result<DecisionsView, String> {
    let id = project(shell.inner())?;
    views::decisions_view(&shell.app, &id).map_err(plain)
}

#[tauri::command]
pub fn evolution_view(shell: State<'_, Shell>) -> Result<EvolutionView, String> {
    let id = project(shell.inner())?;
    views::evolution_view(&shell.app, &id).map_err(plain)
}

#[tauri::command]
pub fn timeline_view(shell: State<'_, Shell>, limit: usize) -> Result<TimelineView, String> {
    let id = project(shell.inner())?;
    views::timeline_view(&shell.app, &id, limit).map_err(plain)
}

#[tauri::command]
pub fn drift_view(shell: State<'_, Shell>) -> Result<DriftView, String> {
    let id = project(shell.inner())?;
    views::drift_view(&shell.app, &id).map_err(plain)
}

#[tauri::command]
pub fn search_view(
    shell: State<'_, Shell>,
    query: String,
    limit: usize,
) -> Result<Vec<SearchRow>, String> {
    let id = project(shell.inner())?;
    views::search_view(&shell.app, &id, &query, limit).map_err(plain)
}

#[tauri::command]
pub fn context_view(
    shell: State<'_, Shell>,
    question: String,
    max_tokens: usize,
    to: String,
    select: Option<String>,
) -> Result<ContextView, String> {
    let id = project(shell.inner())?;
    views::context_view(&shell.app, &id, &question, max_tokens, &to, select.as_deref())
        .map_err(plain)
}

#[tauri::command]
pub fn health_view(shell: State<'_, Shell>) -> Result<HealthView, String> {
    let id = project(shell.inner())?;
    views::health_view(&shell.app, &id).map_err(plain)
}

/// The pointer, not a fourth copy of the resolution order (§61: one loop, one rule).
fn project(shell: &Shell) -> Result<String, String> {
    shell.app.project(None).map_err(plain)
}

fn plain(error: CoreError) -> String {
    error.to_string()
}
