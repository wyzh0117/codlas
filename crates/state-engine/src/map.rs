// The PROJECT_MAP compiler: structured state in, deterministic markdown out
// (V1_DEVELOPMENT.md §30, §31). A model never authors this document wholesale; the
// only freedom one gets later is wording polish on top of these sections.
//
// Two guarantees the rest of the product leans on:
//   * section order is fixed and an empty section says so out loud, so the map
//     tells the reader what is missing instead of hiding it;
//   * the body is a pure function of the state — `generated_at` is the only clock
//     and it lives on the `ProjectMap` record, never in the text.

use vibe_core::error::Result;
use vibe_core::ports::Repositories;
use vibe_domain::{
    AepStatus, ArchitectureEvolutionPlan, Artifact, ChangeProposalStatus, Decision, DriftAlert,
    Issue, Milestone, ProjectEvent, ProjectMap, ProjectState, Task, TaskStatus,
};

use crate::payload::{clip, event_time, excerpt, human_time, serde_slug};
use crate::reducer::Reducer;

/// §31 section order. Not a coincidence, and not configurable.
pub const MAP_SECTIONS: [&str; 13] = [
    "Original Intent",
    "Current Phase",
    "Current Status",
    "Scope",
    "Architecture Baseline",
    "Active Decisions",
    "Active Tasks",
    "Open Issues",
    "Key Artifacts",
    "Architecture Evolution",
    "Recent Changes",
    "Drift / Conflicts",
    "Next Actions",
];

/// What the map shows when a section has nothing to say.
pub const EMPTY_PLACEHOLDER: &str = "_none recorded_";

const RECENT_LIMIT: usize = 10;
const NEXT_ACTION_LIMIT: usize = 8;
const ARTIFACT_LIMIT: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentChange {
    pub time: String,
    pub actor: String,
    pub event_type: String,
    pub summary: String,
}

impl RecentChange {
    fn line(&self) -> String {
        format!(
            "- {} {} {}: {}",
            self.time, self.actor, self.event_type, self.summary
        )
    }
}

pub struct ProjectMapCompiler {
    repos: Repositories,
    reducer: Reducer,
}

impl ProjectMapCompiler {
    pub fn new(repos: Repositories) -> Self {
        Self {
            reducer: Reducer::new(repos.clone()),
            repos,
        }
    }

    pub fn reducer(&self) -> &Reducer {
        &self.reducer
    }

    /// Projection plus the tail of the event log, ready to store.
    pub fn compile(&self, project_id: &str) -> Result<ProjectMap> {
        let state = self.reducer.project_state(project_id)?;
        let events = self.reducer.accepted_events(project_id)?;
        Ok(self.compile_from(&state, &events))
    }

    /// The compile step on data the caller already holds, so taking a snapshot
    /// never pays for a second projection.
    pub fn compile_from(&self, state: &ProjectState, events: &[ProjectEvent]) -> ProjectMap {
        ProjectMap {
            project_id: state.project_id.clone(),
            markdown: self.render_with(state, &recent_changes(events, RECENT_LIMIT)),
            generated_at: vibe_domain::now(),
            source_state_hash: state.state_hash.clone().unwrap_or_default(),
        }
    }

    /// Deterministic markdown from state alone. `render_with` is what `compile`
    /// uses; this variant derives Recent Changes from the projection's own
    /// timestamps instead of the log, which keeps it honest about having no
    /// repository access.
    pub fn render(&self, state: &ProjectState) -> String {
        render_markdown(state, &[])
    }

    pub fn render_with(&self, state: &ProjectState, recent: &[RecentChange]) -> String {
        render_markdown(state, recent)
    }

    /// Compile and cache the map (§32, §59).
    pub fn save(&self, project_id: &str) -> Result<ProjectMap> {
        let map = self.compile(project_id)?;
        self.repos.snapshots.save_project_map(&map, None)?;
        Ok(map)
    }

    pub fn latest(&self, project_id: &str) -> Result<Option<ProjectMap>> {
        self.repos.snapshots.latest_project_map(project_id)
    }
}

/// The template itself: state and the §31 order in, markdown out. Public because a
/// caller with an in-memory state (a snapshot, a golden fixture) may want the text
/// without a store behind it.
pub fn render_markdown(state: &ProjectState, recent: &[RecentChange]) -> String {
    let mut out = String::from("# Project Map\n");
    for heading in MAP_SECTIONS {
        let body: Vec<String> = match heading {
            "Original Intent" => original_intent(state).unwrap_or_default(),
            "Current Phase" => current_phase(state),
            "Current Status" => current_status(state),
            "Scope" => scope(state).unwrap_or_default(),
            "Architecture Baseline" => baseline(state).unwrap_or_default(),
            "Active Decisions" => state.active_decisions().into_iter().map(decision_line).collect(),
            "Active Tasks" => state.open_tasks().into_iter().map(task_line).collect(),
            "Open Issues" => state.open_issues().into_iter().map(issue_line).collect(),
            "Key Artifacts" => key_artifacts(state),
            "Architecture Evolution" => state.aeps.iter().map(aep_line).collect(),
            "Recent Changes" => {
                if recent.is_empty() {
                    state_derived_changes(state)
                } else {
                    recent.iter().map(RecentChange::line).collect()
                }
            }
            "Drift / Conflicts" => state.drift_alerts.iter().map(drift_line).collect(),
            _ => next_actions(state)
                .into_iter()
                .map(|action| format!("- {action}"))
                .collect(),
        };
        out.push_str(&section(heading, &body));
    }
    out
}

/// The most recent accepted events, oldest first, as the map shows them.
pub fn recent_changes(events: &[ProjectEvent], limit: usize) -> Vec<RecentChange> {
    let start = events.len().saturating_sub(limit);
    events[start..]
        .iter()
        .map(|event| RecentChange {
            time: human_time(event_time(event)),
            actor: actor_label(event),
            event_type: event.r#type.clone(),
            summary: excerpt(event, 80),
        })
        .collect()
}

/// §31 spells the actor as a role, not a raw id: a reader of the map cares who
/// moved the project, not which uuid signed the row.
fn actor_label(event: &ProjectEvent) -> String {
    serde_slug(&event.actor_type)
}

/// What the reader should do next, derived from state only: gaps in the baselines,
/// claims that need evidence, blocked work, open issues, unresolved drift, woken
/// evolution plans and undecided proposals.
pub fn next_actions(state: &ProjectState) -> Vec<String> {
    let mut actions = Vec::new();
    if state.charter.is_none() {
        actions.push("Record the project charter so intent stops living in chat".into());
    }
    if state.scope.is_none() {
        actions.push("Define the scope boundary so drift has something to measure".into());
    }
    if state.baseline.is_none() {
        actions.push("Set the architecture baseline: layers, invariants, replaceable parts".into());
    }
    for task in state
        .tasks
        .iter()
        .filter(|t| t.unverified_claim && t.status != TaskStatus::Done)
    {
        actions.push(format!(
            "Verify the agent's completion claim on '{}' against evidence (§19)",
            task.title
        ));
    }
    for task in state.tasks.iter().filter(|t| t.status == TaskStatus::Blocked) {
        actions.push(format!("Clear the blocker on '{}'", task.title));
    }
    for issue in state.open_issues() {
        actions.push(format!("Resolve issue '{}'", issue.title));
    }
    for alert in state.open_drift() {
        actions.push(format!(
            "Decide on {} drift: {}",
            alert.r#type.as_str(),
            alert.summary
        ));
    }
    for aep in state
        .aeps
        .iter()
        .filter(|a| matches!(a.status, AepStatus::Triggered | AepStatus::InProgress))
    {
        actions.push(format!("Progress {} — {}", aep.code, aep.title));
    }
    for proposal in state
        .change_proposals
        .iter()
        .filter(|p| p.status == ChangeProposalStatus::Open)
    {
        actions.push(format!(
            "Review change proposal: {}",
            clip(&proposal.proposed_change, 80)
        ));
    }
    for task in state.tasks.iter().filter(|t| t.status == TaskStatus::Doing) {
        actions.push(format!("Finish in-progress task '{}'", task.title));
    }
    actions.truncate(NEXT_ACTION_LIMIT);
    actions
}

// ------------------------------------------------------------------- sections

fn original_intent(state: &ProjectState) -> Option<Vec<String>> {
    let charter = state.charter.as_ref()?;
    let mut lines = vec![
        format!("- Problem: {}", charter.original_problem),
        format!("- Core value: {}", charter.core_value),
    ];
    if let Some(user) = &charter.target_user {
        lines.push(format!("- Target user: {user}"));
    }
    push_lists(
        &mut lines,
        &[
            ("Must have", &charter.must_have),
            ("Won't have", &charter.wont_have),
            ("Success criteria", &charter.success_criteria),
            ("Principles", &charter.architecture_principles),
            ("Constraints", &charter.constraints),
        ],
    );
    lines.push(format!(
        "- Charter v{}{}",
        charter.version,
        if charter.locked { " (locked by user)" } else { "" }
    ));
    Some(lines)
}

fn current_phase(state: &ProjectState) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(project) = &state.project {
        lines.push(format!(
            "- Phase: {}",
            project
                .current_phase
                .clone()
                .unwrap_or_else(|| "not declared".to_string())
        ));
        lines.push(format!("- Project status: {}", serde_slug(&project.status)));
    }
    for milestone in &state.milestones {
        lines.push(milestone_line(milestone));
    }
    lines
}

fn current_status(state: &ProjectState) -> Vec<String> {
    let open = state.open_tasks().len();
    let done = state
        .tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Done)
        .count();
    let cancelled = state
        .tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Cancelled)
        .count();
    if state.applied_events == 0 && state.tasks.is_empty() && state.issues.is_empty() {
        return Vec::new();
    }
    let claims = state.tasks.iter().filter(|t| t.unverified_claim).count();
    let mut lines = vec![
        format!("- Tasks: {open} open, {done} done, {cancelled} cancelled"),
        format!("- Open issues: {}", state.open_issues().len()),
        format!("- Active decisions: {}", state.active_decisions().len()),
        format!("- Open drift alerts: {}", state.open_drift().len()),
        format!(
            "- Accepted events folded: {} (last seq {})",
            state.applied_events, state.last_seq
        ),
    ];
    if claims > 0 {
        lines.push(format!("- Unverified completion claims: {claims}"));
    }
    if let Some(hash) = &state.state_hash {
        lines.push(format!("- State hash: {hash}"));
    }
    lines
}

fn scope(state: &ProjectState) -> Option<Vec<String>> {
    let scope = state.scope.as_ref()?;
    let mut lines = Vec::new();
    push_lists(
        &mut lines,
        &[
            ("In scope", &scope.in_scope),
            ("Out of scope", &scope.out_of_scope),
            ("Deferred", &scope.deferred),
            ("Forbidden", &scope.forbidden),
        ],
    );
    lines.push(format!("- Boundary v{}", scope.version));
    Some(lines)
}

fn baseline(state: &ProjectState) -> Option<Vec<String>> {
    let baseline = state.baseline.as_ref()?;
    let mut lines = vec![
        format!("- Summary: {}", baseline.summary),
        format!("- Version: v{}", baseline.version),
    ];
    for layer in &baseline.layers {
        lines.push(match &layer.responsibility {
            Some(duty) => format!("- Layer {} — {duty}", layer.name),
            None => format!("- Layer {}", layer.name),
        });
    }
    for invariant in &baseline.invariants {
        lines.push(format!("- Invariant: {}", invariant.text));
    }
    let mut lists = Vec::new();
    push_lists(&mut lists, &[("Replaceable", &baseline.replaceable_parts)]);
    lines.extend(lists);
    Some(lines)
}

fn key_artifacts(state: &ProjectState) -> Vec<String> {
    state
        .artifacts
        .iter()
        .filter(|artifact| !artifact.deleted)
        .take(ARTIFACT_LIMIT)
        .map(artifact_line)
        .collect()
}

// --------------------------------------------------------------------- lines

fn section(heading: &str, body: &[String]) -> String {
    let mut out = format!("\n## {heading}\n\n");
    if body.is_empty() {
        out.push_str(EMPTY_PLACEHOLDER);
        out.push('\n');
        return out;
    }
    for line in body {
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn push_lists(lines: &mut Vec<String>, items: &[(&str, &Vec<String>)]) {
    for (label, values) in items {
        if !values.is_empty() {
            lines.push(format!("- {label}: {}", values.join("; ")));
        }
    }
}

fn task_line(task: &Task) -> String {
    let mut line = format!("- [{}] {}", serde_slug(&task.status), task.title);
    if task.unverified_claim {
        line.push_str(" (agent claims done, unverified)");
    }
    if let Some(detail) = &task.detail {
        line.push_str(&format!(" — {}", clip(detail, 80)));
    }
    line
}

fn issue_line(issue: &Issue) -> String {
    format!("- [{}] {}", serde_slug(&issue.status), issue.title)
}

fn decision_line(decision: &Decision) -> String {
    let mut line = format!("- {}", decision.title);
    if !decision.decision.is_empty() && decision.decision != decision.title {
        line.push_str(&format!(" — {}", decision.decision));
    }
    if decision.locked {
        line.push_str(" [locked]");
    }
    if let Some(rationale) = &decision.rationale {
        line.push_str(&format!(" (why: {})", clip(rationale, 80)));
    }
    line
}

fn artifact_line(artifact: &Artifact) -> String {
    let mut line = format!(
        "- {} ({}, v{}",
        artifact.name,
        serde_slug(&artifact.kind),
        artifact.version
    );
    if let Some(path) = &artifact.path {
        line.push_str(&format!(", {path}"));
    }
    if artifact.redesign_count > 0 {
        line.push_str(&format!(", redesigned {}x", artifact.redesign_count));
    }
    line.push(')');
    line
}

fn milestone_line(milestone: &Milestone) -> String {
    format!(
        "- Milestone {} — {}",
        milestone.title,
        if milestone.reached { "reached" } else { "planned" }
    )
}

fn aep_line(aep: &ArchitectureEvolutionPlan) -> String {
    let mut line = format!("- {} {} — {}", aep.code, aep.title, aep.status.label());
    if !aep.trigger.is_empty() {
        line.push_str(&format!(" (trigger: {})", aep.trigger.join("; ")));
    }
    if !aep.why_not_now.is_empty() {
        line.push_str(&format!(" (why not now: {})", clip(&aep.why_not_now, 60)));
    }
    line
}

fn drift_line(alert: &DriftAlert) -> String {
    format!(
        "- [{}] {} ({}): {}",
        serde_slug(&alert.status),
        alert.r#type.as_str(),
        serde_slug(&alert.severity),
        alert.summary
    )
}

/// Fallback Recent Changes when only a state is in hand: the projection's own
/// timestamps, sorted by time then label so the order never depends on a map.
fn state_derived_changes(state: &ProjectState) -> Vec<String> {
    let mut rows: Vec<(String, String)> = Vec::new();
    let mut push = |at: Option<chrono::DateTime<chrono::Utc>>, label: String| {
        if let Some(at) = at {
            rows.push((human_time(at), label));
        }
    };
    for task in &state.tasks {
        push(task.updated_at, format!("- task: {} → {}", task.title, serde_slug(&task.status)));
    }
    for issue in &state.issues {
        push(issue.updated_at, format!("- issue: {} → {}", issue.title, serde_slug(&issue.status)));
    }
    for decision in &state.decisions {
        push(decision.decided_at, format!("- decision: {} → {}", decision.title, serde_slug(&decision.status)));
    }
    for artifact in &state.artifacts {
        push(artifact.updated_at, format!("- artifact: {} v{}", artifact.name, artifact.version));
    }
    for milestone in &state.milestones {
        push(milestone.reached_at, format!("- milestone: {}", milestone.title));
    }
    for proposal in &state.change_proposals {
        push(proposal.created_at, format!("- proposal: {}", clip(&proposal.proposed_change, 60)));
    }
    for alert in &state.drift_alerts {
        push(alert.detected_at, format!("- drift: {}", clip(&alert.summary, 60)));
    }
    rows.sort();
    let start = rows.len().saturating_sub(RECENT_LIMIT);
    rows[start..].iter().map(|(_, line)| line.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> ProjectState {
        ProjectState {
            project_id: "p1".into(),
            tasks: vec![Task {
                id: "t1".into(),
                project_id: "p1".into(),
                title: "wire reducer".into(),
                status: TaskStatus::Doing,
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn empty_sections_declare_themselves_missing() {
        let body = render_markdown(
            &ProjectState {
                project_id: "p1".into(),
                ..Default::default()
            },
            &[],
        );
        // Twelve sections have nothing to show for themselves; only Next Actions
        // speaks up, and it says what is missing.
        assert_eq!(body.matches(EMPTY_PLACEHOLDER).count(), MAP_SECTIONS.len() - 1);
        assert!(body.contains("Record the project charter"));
    }

    #[test]
    fn render_is_byte_identical_for_the_same_state() {
        let state = state();
        assert_eq!(render_markdown(&state, &[]), render_markdown(&state, &[]));
        assert_eq!(
            render_markdown(&state, &recent_changes(&[], RECENT_LIMIT)),
            render_markdown(&state, &[])
        );
    }

    #[test]
    fn next_actions_surfaces_an_unverified_claim() {
        let mut state = state();
        state.tasks[0].unverified_claim = true;
        let actions = next_actions(&state);
        assert!(actions
            .iter()
            .any(|action| action.contains("Verify the agent's completion claim")));
    }
}
