//! catalog — the L1 side of retrieval: project records read through the ports.
//!
//! V1_DEVELOPMENT.md §40 splits search into three layers, and §6 makes the ports the only
//! way to reach any of them. This module turns the typed rows (`Task`, `Decision`,
//! `ProjectEvent`, `Message`, …) into one uniform [`Record`] shape, so structured lookup,
//! the full-text reindex and relation expansion all speak the same language without any
//! of them knowing how a row is stored — or that a database exists at all.

use vibe_core::error::Result;
use vibe_core::ports::{Repositories, SearchFilters, SearchHit};
use vibe_domain::{
    ArchitectureEvolutionPlan, Artifact, ChangeProposal, Decision, DriftAlert, EntityKind, Issue,
    Milestone, ProjectEvent, Task,
};

/// The record kinds retrieval can name. These mirror the slugs the store indexes under,
/// so a `kinds` filter written here matches what L2 holds.
pub const KIND_MESSAGE: &str = "message";
pub const KIND_EVENT: &str = "event";
pub const KIND_TASK: &str = "task";
pub const KIND_ISSUE: &str = "issue";
pub const KIND_DECISION: &str = "decision";
pub const KIND_ARTIFACT: &str = "artifact";
pub const KIND_AEP: &str = "aep";
pub const KIND_MILESTONE: &str = "milestone";
pub const KIND_PROPOSAL: &str = "change_proposal";
pub const KIND_DRIFT: &str = "drift_alert";

/// Every kind [`crate::SearchService::reindex`] writes — the §40 coverage list (messages,
/// events, tasks, issues, decisions, artifacts, AEPs) plus the remaining state objects.
pub const ALL_KINDS: &[&str] = &[
    KIND_MESSAGE,
    KIND_EVENT,
    KIND_TASK,
    KIND_ISSUE,
    KIND_DECISION,
    KIND_ARTIFACT,
    KIND_AEP,
    KIND_MILESTONE,
    KIND_PROPOSAL,
    KIND_DRIFT,
];

/// One retrievable thing, however it is stored.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub kind: String,
    pub id: String,
    pub title: String,
    /// The state object's status slug (`todo`, `active`, `deleted`…), when it has one.
    pub status: Option<String>,
    /// The text retrieval indexes and excerpts from. Never a file body (§22).
    pub body: String,
}

impl Record {
    /// The same record in the port's hit shape, carrying the project it belongs to.
    pub fn hit(&self, project_id: &str, snippet_chars: usize) -> SearchHit {
        // A record with no body still has to show what matched: the title is the only
        // text it carries, and an empty snippet makes an L1 hit unreadable in the CLI
        // and unreadable in the UI that will later reuse it.
        let body = excerpt(&self.body, snippet_chars);
        let snippet = if body.trim().is_empty() {
            excerpt(&self.title, snippet_chars)
        } else {
            body
        };
        SearchHit {
            kind: self.kind.clone(),
            id: self.id.clone(),
            project_id: project_id.to_string(),
            title: self.title.clone(),
            snippet,
            rank: 0.0,
        }
    }

    /// Case-insensitive substring match over title and body.
    pub fn matches(&self, needle: &str) -> bool {
        let needle = needle.to_lowercase();
        self.title.to_lowercase().contains(&needle) || self.body.to_lowercase().contains(&needle)
    }

    pub fn title_matches(&self, needle: &str) -> bool {
        let needle = needle.to_lowercase();
        self.title.to_lowercase().contains(&needle)
    }

    pub fn status_matches(&self, wanted: &str) -> bool {
        self.status
            .as_deref()
            .is_some_and(|status| status.eq_ignore_ascii_case(wanted))
    }
}

/// Clip to a stable single-line excerpt. Counted in `char`s, because titles and snippets
/// mix Chinese and Latin text and a byte slice would panic on a boundary.
pub fn excerpt(text: &str, limit: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= limit {
        return flat;
    }
    let mut clipped: String = flat.chars().take(limit).collect();
    clipped.push(' ');
    clipped.push_str("…");
    clipped
}

/// Read the project's records.
///
/// `kinds` narrows which collections are read at all — the cheap-first rule of §60: a
/// query about decisions must not pay for a message scan. Empty means "every kind".
pub fn records(repos: &Repositories, project_id: &str, kinds: &[String]) -> Result<Vec<Record>> {
    let wanted = |kind: &str| kinds.is_empty() || kinds.iter().any(|k| k == kind);
    let mut out = Vec::new();

    if wanted(KIND_TASK) {
        for task in typed::<Task>(repos, EntityKind::Task, project_id)? {
            out.push(Record {
                kind: KIND_TASK.to_string(),
                id: task.id,
                title: task.title,
                status: Some(slug(&task.status)),
                body: task.detail.unwrap_or_default(),
            });
        }
    }
    if wanted(KIND_ISSUE) {
        for issue in typed::<Issue>(repos, EntityKind::Issue, project_id)? {
            out.push(Record {
                kind: KIND_ISSUE.to_string(),
                id: issue.id,
                title: issue.title,
                status: Some(slug(&issue.status)),
                body: issue.detail.unwrap_or_default(),
            });
        }
    }
    if wanted(KIND_DECISION) {
        for decision in typed::<Decision>(repos, EntityKind::Decision, project_id)? {
            out.push(Record {
                kind: KIND_DECISION.to_string(),
                id: decision.id,
                title: decision.title,
                status: Some(slug(&decision.status)),
                body: format!(
                    "{} {} {}",
                    decision.decision,
                    decision.rationale.unwrap_or_default(),
                    decision.alternatives.join(" ")
                ),
            });
        }
    }
    if wanted(KIND_MILESTONE) {
        for milestone in typed::<Milestone>(repos, EntityKind::Milestone, project_id)? {
            out.push(Record {
                kind: KIND_MILESTONE.to_string(),
                id: milestone.id,
                title: milestone.title,
                status: Some(if milestone.reached { "reached" } else { "open" }.to_string()),
                body: String::new(),
            });
        }
    }
    if wanted(KIND_PROPOSAL) {
        for proposal in typed::<ChangeProposal>(repos, EntityKind::ChangeProposal, project_id)? {
            out.push(Record {
                kind: KIND_PROPOSAL.to_string(),
                id: proposal.id,
                title: proposal.proposed_change,
                status: Some(slug(&proposal.status)),
                body: proposal.reason,
            });
        }
    }
    if wanted(KIND_AEP) {
        for aep in typed::<ArchitectureEvolutionPlan>(repos, EntityKind::Aep, project_id)? {
            out.push(Record {
                kind: KIND_AEP.to_string(),
                id: aep.id,
                title: aep.title,
                status: Some(slug(&aep.status)),
                body: format!("{} {} {}", aep.current, aep.limitation, aep.target),
            });
        }
    }
    if wanted(KIND_DRIFT) {
        for alert in typed::<DriftAlert>(repos, EntityKind::DriftAlert, project_id)? {
            out.push(Record {
                kind: KIND_DRIFT.to_string(),
                id: alert.id,
                title: alert.summary,
                status: Some(slug(&alert.status)),
                body: alert.detail.unwrap_or_default(),
            });
        }
    }
    // Artifacts come from their dedicated table: it is keyed by path and is what
    // `find_artifact_by_path` answers from, so it is the authority for this kind.
    if wanted(KIND_ARTIFACT) {
        for artifact in repos.artifacts.list_artifacts(project_id)? {
            out.push(artifact_record(artifact));
        }
    }
    if wanted(KIND_EVENT) {
        for event in repos.events.list_events(project_id, None, None)? {
            out.push(event_record(&event));
        }
    }
    if wanted(KIND_MESSAGE) {
        for conversation in repos.conversations.list_conversations(project_id)? {
            for message in repos.conversations.list_messages(&conversation.id)? {
                out.push(Record {
                    kind: KIND_MESSAGE.to_string(),
                    id: message.id.clone(),
                    title: format!("{} · {}", message.role.as_str(), excerpt(&message.content, 60)),
                    status: None,
                    body: message.content,
                });
            }
        }
    }
    Ok(out)
}

fn artifact_record(artifact: Artifact) -> Record {
    Record {
        kind: KIND_ARTIFACT.to_string(),
        id: artifact.id,
        title: artifact.name,
        status: Some(if artifact.deleted { "deleted" } else { "active" }.to_string()),
        body: format!(
            "{} {}",
            artifact.path.unwrap_or_default(),
            artifact.relationships.join(" ")
        ),
    }
}

fn event_record(event: &ProjectEvent) -> Record {
    Record {
        kind: KIND_EVENT.to_string(),
        id: event.id.clone(),
        title: event.r#type.clone(),
        status: Some(slug(&event.status)),
        body: format!(
            "{} {} {}",
            event.r#type,
            event.subject_id.clone().unwrap_or_default(),
            event.payload
        ),
    }
}

/// Read one typed entity collection. A row that fails to decode is skipped rather than
/// failing the whole query: one bad cache row must not switch retrieval off (§40).
pub fn typed<T: serde::de::DeserializeOwned>(
    repos: &Repositories,
    kind: EntityKind,
    project_id: &str,
) -> Result<Vec<T>> {
    Ok(repos
        .entities
        .list(kind, project_id)?
        .iter()
        .filter_map(|value| serde_json::from_value::<T>(value.clone()).ok())
        .collect())
}

/// One record by id, whatever kind it is — for L3, where a link names a target that may
/// not be in the full-text index at all.
pub fn find_by_id(
    repos: &Repositories,
    project_id: &str,
    kind: &str,
    id: &str,
) -> Result<Option<Record>> {
    Ok(records(repos, project_id, &[kind.to_string()])?
        .into_iter()
        .find(|record| record.id == id))
}

/// Port filters for one project, with a default page size.
pub fn filters(project_id: &str, kinds: &[&str], limit: usize) -> SearchFilters {
    SearchFilters {
        project_id: Some(project_id.to_string()),
        kinds: kinds.iter().map(|k| k.to_string()).collect(),
        limit: if limit == 0 { 25 } else { limit },
    }
}

/// A status as its serde slug (`TaskStatus::Todo` → `todo`), which is what `entities`
/// stores in its `status` column and what a structured query filters on.
fn slug<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}
