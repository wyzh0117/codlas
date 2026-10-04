//! Snapshots and the "since last snapshot" diff (V1_DEVELOPMENT.md §32, §33).
//!
//! A snapshot freezes the projection *and* the compiled map, so a later reader can
//! see both what the project was and what the workbench said about it. The diff is
//! computed from the two states only — never from a model's recollection.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use vibe_core::error::{CoreError, Result};
use vibe_core::ports::Repositories;
use vibe_domain::{ProjectMap, ProjectState, Snapshot};

use crate::map::{next_actions, ProjectMapCompiler};
use crate::reducer::{state_hash_of, Reducer};
use crate::payload::serde_slug;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffEntry {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
}

impl DiffEntry {
    fn transition(&self) -> String {
        match (&self.from, &self.to) {
            (Some(from), Some(to)) => format!("{}: {} → {}", self.label, from, to),
            (None, Some(to)) => format!("{}: → {}", self.label, to),
            _ => self.label.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffSection {
    #[serde(default)]
    pub added: Vec<DiffEntry>,
    #[serde(default)]
    pub removed: Vec<DiffEntry>,
    #[serde(default)]
    pub changed: Vec<DiffEntry>,
}

impl DiffSection {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NextActionsDiff {
    #[serde(default)]
    pub added: Vec<String>,
    #[serde(default)]
    pub removed: Vec<String>,
}

impl NextActionsDiff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDiff {
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_snapshot_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_taken_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_state_hash: Option<String>,
    pub current_state_hash: String,
    pub tasks: DiffSection,
    pub issues: DiffSection,
    pub decisions: DiffSection,
    pub artifacts: DiffSection,
    pub milestones: DiffSection,
    pub aeps: DiffSection,
    pub drift_alerts: DiffSection,
    /// Charter / architecture baseline version movement, read off the hydrated
    /// baseline sections of both states.
    pub charter_from_version: Option<u32>,
    pub charter_to_version: Option<u32>,
    pub baseline_from_version: Option<u32>,
    pub baseline_to_version: Option<u32>,
    pub next_actions: NextActionsDiff,
}

impl ProjectDiff {
    /// §33 shape: Since Last Snapshot / Added / Changed / Architecture / Evolution
    /// / Drift / Next.
    pub fn markdown(&self) -> String {
        let mut out = String::from("# Since Last Snapshot\n\n");
        match (&self.since_label, &self.since_taken_at) {
            (Some(label), Some(at)) => out.push_str(&format!(
                "- Baseline: {label} ({}), state {} → {}\n",
                crate::payload::human_time(*at),
                short(self.since_state_hash.as_deref().unwrap_or("-")),
                short(&self.current_state_hash)
            )),
            _ => out.push_str(&format!(
                "- No snapshot baseline: everything below is measured against an empty state.\n\
                 - Current state {}\n",
                short(&self.current_state_hash)
            )),
        }

        out.push_str("\n## Added\n\n");
        let mut added: Vec<String> = Vec::new();
        push_count(&mut added, "+", "Tasks", &self.tasks.added);
        push_count(&mut added, "+", "Decisions", &self.decisions.added);
        push_count(&mut added, "+", "Issues", &self.issues.added);
        push_count(&mut added, "+", "Artifacts", &self.artifacts.added);
        push_count(&mut added, "+", "Milestones", &self.milestones.added);
        push_count(&mut added, "+", "AEPs", &self.aeps.added);
        push_count(&mut added, "+", "Drift alerts", &self.drift_alerts.added);
        for entry in self
            .tasks
            .added
            .iter()
            .chain(self.decisions.added.iter())
            .chain(self.issues.added.iter())
        {
            added.push(format!("  · {}", entry.label));
        }
        push_lines(&mut out, &added);

        out.push_str("\n## Changed\n\n");
        let mut changed: Vec<String> = Vec::new();
        for section in [
            &self.tasks,
            &self.issues,
            &self.decisions,
            &self.artifacts,
            &self.milestones,
        ] {
            for entry in section.changed.iter().chain(section.removed.iter()) {
                changed.push(format!("- {}", entry.transition()));
            }
        }
        push_lines(&mut out, &changed);

        out.push_str("\n## Architecture\n\n");
        let mut architecture: Vec<String> = Vec::new();
        if self.charter_from_version != self.charter_to_version {
            architecture.push(format!(
                "- Charter v{} → v{}",
                version_text(self.charter_from_version),
                version_text(self.charter_to_version)
            ));
        }
        if self.baseline_from_version != self.baseline_to_version {
            architecture.push(format!(
                "- Architecture baseline v{} → v{}",
                version_text(self.baseline_from_version),
                version_text(self.baseline_to_version)
            ));
        }
        for entry in &self.decisions.changed {
            architecture.push(format!("- {}", entry.transition()));
        }
        push_lines(&mut out, &architecture);

        out.push_str("\n## Evolution\n\n");
        let mut evolution: Vec<String> = Vec::new();
        for entry in &self.aeps.changed {
            evolution.push(format!("- {}", entry.transition()));
        }
        for entry in &self.aeps.added {
            evolution.push(format!("- new plan: {}", entry.label));
        }
        for entry in &self.aeps.removed {
            evolution.push(format!("- gone: {}", entry.label));
        }
        push_lines(&mut out, &evolution);

        out.push_str("\n## Drift\n\n");
        let mut drift: Vec<String> = Vec::new();
        if !self.drift_alerts.added.is_empty() {
            drift.push(format!(
                "- {} new drift warning(s)",
                self.drift_alerts.added.len()
            ));
            for entry in &self.drift_alerts.added {
                drift.push(format!("  · {}", entry.label));
            }
        }
        for entry in self
            .drift_alerts
            .changed
            .iter()
            .chain(self.drift_alerts.removed.iter())
        {
            drift.push(format!("- {}", entry.transition()));
        }
        push_lines(&mut out, &drift);

        out.push_str("\n## Next\n\n");
        let mut next: Vec<String> = self
            .next_actions
            .added
            .iter()
            .map(|action| format!("- {action}"))
            .collect();
        next.extend(
            self.next_actions
                .removed
                .iter()
                .map(|action| format!("- done: {action}")),
        );
        push_lines(&mut out, &next);
        out
    }
}

pub struct SnapshotService {
    repos: Repositories,
    reducer: Reducer,
    compiler: ProjectMapCompiler,
}

impl SnapshotService {
    pub fn new(repos: Repositories) -> Self {
        Self {
            reducer: Reducer::new(repos.clone()),
            compiler: ProjectMapCompiler::new(repos.clone()),
            repos,
        }
    }

    /// Freeze state + map together (§32). The state is the projection the reducer
    /// just produced, so a snapshot is always replayable.
    pub fn take(&self, project_id: &str, label: &str) -> Result<Snapshot> {
        let mut state = self.reducer.project_state(project_id)?;
        let events = self.reducer.accepted_events(project_id)?;
        let map = self.compiler.compile_from(&state, &events);
        if state.state_hash.is_none() {
            state.state_hash = Some(state_hash_of(&state));
        }
        let snapshot = Snapshot {
            id: vibe_domain::new_id(),
            project_id: project_id.to_string(),
            label: label.to_string(),
            state,
            project_map_markdown: Some(map.markdown),
            taken_at: vibe_domain::now(),
        };
        self.repos.snapshots.save(&snapshot)?;
        Ok(snapshot)
    }

    pub fn list(&self, project_id: &str) -> Result<Vec<Snapshot>> {
        self.repos.snapshots.list(project_id)
    }

    pub fn latest(&self, project_id: &str) -> Result<Option<Snapshot>> {
        self.repos.snapshots.latest(project_id)
    }

    /// §33. `None` compares the current state against no baseline at all, which is
    /// what a first-run project needs: everything reads as added.
    pub fn diff(&self, project_id: &str, since_snapshot_id: Option<&str>) -> Result<ProjectDiff> {
        let current = self.reducer.project_state(project_id)?;
        let baseline = match since_snapshot_id {
            Some(id) => Some(
                self.repos
                    .snapshots
                    .get(id)?
                    .ok_or_else(|| CoreError::NotFound {
                        what: format!("snapshot {id}"),
                    })?,
            ),
            None => None,
        };
        Ok(diff_states(
            &current,
            baseline.as_ref().map(|snapshot| &snapshot.state),
            since_snapshot_id.map(str::to_string),
            baseline
                .as_ref()
                .map(|snapshot| snapshot.label.clone()),
            baseline.as_ref().map(|snapshot| snapshot.taken_at),
        ))
    }

    /// The §33 view most callers want: since the newest snapshot, or since nothing.
    pub fn since_last_snapshot(&self, project_id: &str) -> Result<ProjectDiff> {
        let current = self.reducer.project_state(project_id)?;
        let latest = self.repos.snapshots.latest(project_id)?;
        Ok(diff_states(
            &current,
            latest.as_ref().map(|snapshot| &snapshot.state),
            latest.as_ref().map(|snapshot| snapshot.id.clone()),
            latest.as_ref().map(|snapshot| snapshot.label.clone()),
            latest.as_ref().map(|snapshot| snapshot.taken_at),
        ))
    }

    /// The map as of a snapshot, without recompiling it.
    pub fn map_of(snapshot: &Snapshot) -> Option<&str> {
        snapshot.project_map_markdown.as_deref()
    }

    pub fn compile_of(snapshot: &Snapshot) -> ProjectMap {
        ProjectMap {
            project_id: snapshot.project_id.clone(),
            markdown: snapshot
                .project_map_markdown
                .clone()
                .unwrap_or_else(|| crate::map::render_markdown(&snapshot.state, &[])),
            generated_at: snapshot.taken_at,
            source_state_hash: snapshot
                .state
                .state_hash
                .clone()
                .unwrap_or_else(|| state_hash_of(&snapshot.state)),
        }
    }
}

/// Pure state-vs-state diff, so a caller holding two projections in memory (a
/// golden fixture, a saved snapshot) never needs a store to compare them.
pub fn diff_states(
    current: &ProjectState,
    baseline: Option<&ProjectState>,
    since_snapshot_id: Option<String>,
    since_label: Option<String>,
    since_taken_at: Option<DateTime<Utc>>,
) -> ProjectDiff {
    let empty = ProjectState::default();
    let before = baseline.unwrap_or(&empty);
    let mut diff = ProjectDiff {
        project_id: current.project_id.clone(),
        since_snapshot_id,
        since_label,
        since_taken_at,
        since_state_hash: baseline.and_then(|state| state.state_hash.clone()),
        current_state_hash: current
            .state_hash
            .clone()
            .unwrap_or_else(|| state_hash_of(current)),
        tasks: rows(&tasks_of(before), &tasks_of(current)),
        issues: rows(&issues_of(before), &issues_of(current)),
        decisions: rows(&decisions_of(before), &decisions_of(current)),
        artifacts: rows(&artifacts_of(before), &artifacts_of(current)),
        milestones: rows(&milestones_of(before), &milestones_of(current)),
        aeps: rows(&aeps_of(before), &aeps_of(current)),
        drift_alerts: rows(&drift_of(before), &drift_of(current)),
        charter_from_version: before.charter.as_ref().map(|c| c.version),
        charter_to_version: current.charter.as_ref().map(|c| c.version),
        baseline_from_version: before.baseline.as_ref().map(|b| b.version),
        baseline_to_version: current.baseline.as_ref().map(|b| b.version),
        next_actions: NextActionsDiff::default(),
    };
    let after_actions = next_actions(current);
    let before_actions = next_actions(before);
    diff.next_actions.added = after_actions
        .iter()
        .filter(|action| !before_actions.contains(action))
        .cloned()
        .collect();
    diff.next_actions.removed = before_actions
        .iter()
        .filter(|action| !after_actions.contains(action))
        .cloned()
        .collect();
    diff
}

struct Row {
    id: String,
    label: String,
    status: String,
}

fn rows(before: &[Row], after: &[Row]) -> DiffSection {
    let mut section = DiffSection::default();
    for row in after {
        match before.iter().find(|old| old.id == row.id) {
            None => section.added.push(DiffEntry {
                id: row.id.clone(),
                label: row.label.clone(),
                from: None,
                to: Some(row.status.clone()),
            }),
            Some(old) if old.status != row.status => section.changed.push(DiffEntry {
                id: row.id.clone(),
                label: row.label.clone(),
                from: Some(old.status.clone()),
                to: Some(row.status.clone()),
            }),
            Some(_) => {}
        }
    }
    for row in before {
        if !after.iter().any(|new| new.id == row.id) {
            section.removed.push(DiffEntry {
                id: row.id.clone(),
                label: row.label.clone(),
                from: Some(row.status.clone()),
                to: None,
            });
        }
    }
    section
}

fn tasks_of(state: &ProjectState) -> Vec<Row> {
    state
        .tasks
        .iter()
        .map(|task| {
            let mut status = serde_slug(&task.status);
            if task.unverified_claim {
                status.push_str("+claim");
            }
            Row {
                id: task.id.clone(),
                label: task.title.clone(),
                status,
            }
        })
        .collect()
}

fn issues_of(state: &ProjectState) -> Vec<Row> {
    state
        .issues
        .iter()
        .map(|issue| Row {
            id: issue.id.clone(),
            label: issue.title.clone(),
            status: serde_slug(&issue.status),
        })
        .collect()
}

fn decisions_of(state: &ProjectState) -> Vec<Row> {
    state
        .decisions
        .iter()
        .map(|decision| Row {
            id: decision.id.clone(),
            label: decision.title.clone(),
            status: serde_slug(&decision.status),
        })
        .collect()
}

fn artifacts_of(state: &ProjectState) -> Vec<Row> {
    state
        .artifacts
        .iter()
        .map(|artifact| Row {
            id: artifact.id.clone(),
            label: artifact.name.clone(),
            status: format!(
                "v{}{}",
                artifact.version,
                if artifact.deleted { " deleted" } else { "" }
            ),
        })
        .collect()
}

fn milestones_of(state: &ProjectState) -> Vec<Row> {
    state
        .milestones
        .iter()
        .map(|milestone| Row {
            id: milestone.id.clone(),
            label: milestone.title.clone(),
            status: if milestone.reached { "reached" } else { "planned" }.into(),
        })
        .collect()
}

fn aeps_of(state: &ProjectState) -> Vec<Row> {
    state
        .aeps
        .iter()
        .map(|aep| Row {
            id: aep.id.clone(),
            label: format!("{} {}", aep.code, aep.title),
            status: aep.status.label().to_ascii_lowercase(),
        })
        .collect()
}

fn drift_of(state: &ProjectState) -> Vec<Row> {
    state
        .drift_alerts
        .iter()
        .map(|alert| Row {
            id: alert.id.clone(),
            label: format!("{}: {}", alert.r#type.as_str(), alert.summary),
            status: serde_slug(&alert.status),
        })
        .collect()
}

const NOTHING_NEW: &str = "_nothing recorded since the last snapshot_";

fn push_lines(out: &mut String, lines: &[String]) {
    if lines.is_empty() {
        out.push_str(NOTHING_NEW);
        out.push('\n');
        return;
    }
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
}

fn push_count(lines: &mut Vec<String>, sign: &str, noun: &str, entries: &[DiffEntry]) {
    if !entries.is_empty() {
        lines.push(format!("{sign} {} {}", entries.len(), noun));
    }
}

fn short(hash: &str) -> String {
    hash.chars().take(12).collect()
}

fn version_text(version: Option<u32>) -> String {
    version.map(|v| v.to_string()).unwrap_or_else(|| "-".into())
}
