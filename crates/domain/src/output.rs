//! Materialised state, snapshots, context packs, overrides and audit records
//! (V1_DEVELOPMENT.md §18, §29-§33, §41, §53-§54).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::baseline::*;
use crate::entities::*;
use crate::record::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Task,
    Issue,
    Decision,
    Artifact,
    Milestone,
    ChangeProposal,
    Aep,
    DriftAlert,
    Evidence,
}

impl EntityKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntityKind::Task => "task",
            EntityKind::Issue => "issue",
            EntityKind::Decision => "decision",
            EntityKind::Artifact => "artifact",
            EntityKind::Milestone => "milestone",
            EntityKind::ChangeProposal => "change_proposal",
            EntityKind::Aep => "aep",
            EntityKind::DriftAlert => "drift_alert",
            EntityKind::Evidence => "evidence",
        }
    }

    pub fn parse(value: &str) -> Option<EntityKind> {
        Some(match value {
            "task" => EntityKind::Task,
            "issue" => EntityKind::Issue,
            "decision" => EntityKind::Decision,
            "artifact" => EntityKind::Artifact,
            "milestone" => EntityKind::Milestone,
            "change_proposal" => EntityKind::ChangeProposal,
            "aep" => EntityKind::Aep,
            "drift_alert" => EntityKind::DriftAlert,
            "evidence" => EntityKind::Evidence,
            _ => return None,
        })
    }
}

/// Deterministic projection of the accepted event log (V1_DEVELOPMENT.md §18).
/// The same set of accepted events must always produce the same state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectState {
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<Project>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub charter: Option<ProjectCharter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ScopeBoundary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<ArchitectureBaseline>,

    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub issues: Vec<Issue>,
    #[serde(default)]
    pub decisions: Vec<Decision>,
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
    #[serde(default)]
    pub milestones: Vec<Milestone>,
    #[serde(default)]
    pub change_proposals: Vec<ChangeProposal>,
    #[serde(default)]
    pub aeps: Vec<ArchitectureEvolutionPlan>,
    #[serde(default)]
    pub drift_alerts: Vec<DriftAlert>,
    #[serde(default)]
    pub evidence: Vec<Evidence>,

    /// Sequence number of the last event folded into this state.
    #[serde(default)]
    pub last_seq: i64,
    #[serde(default)]
    pub applied_events: u64,
    /// Content hash over the accepted event window that produced this state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub built_at: Option<DateTime<Utc>>,
}

impl ProjectState {
    pub fn active_decisions(&self) -> Vec<&Decision> {
        self.decisions
            .iter()
            .filter(|d| d.status == DecisionStatus::Active)
            .collect()
    }

    pub fn open_issues(&self) -> Vec<&Issue> {
        self.issues
            .iter()
            .filter(|i| !matches!(i.status, IssueStatus::Resolved | IssueStatus::WontFix))
            .collect()
    }

    pub fn open_tasks(&self) -> Vec<&Task> {
        self.tasks
            .iter()
            .filter(|t| !matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled))
            .collect()
    }

    pub fn open_drift(&self) -> Vec<&DriftAlert> {
        self.drift_alerts
            .iter()
            .filter(|d| d.status == DriftStatus::Open)
            .collect()
    }

    pub fn dormant_aeps(&self) -> Vec<&ArchitectureEvolutionPlan> {
        self.aeps
            .iter()
            .filter(|a| matches!(a.status, AepStatus::Dormant | AepStatus::Watching))
            .collect()
    }
}

/// A readable projection of the state, compiled not authored (V1_DEVELOPMENT.md §30).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMap {
    pub project_id: String,
    pub markdown: String,
    pub generated_at: DateTime<Utc>,
    pub source_state_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub id: String,
    pub project_id: String,
    pub label: String,
    pub state: ProjectState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_map_markdown: Option<String>,
    pub taken_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSection {
    OriginalIntent,
    CurrentPhase,
    SelectedContext,
    RelevantScope,
    ActiveDecisions,
    ArchitectureConstraints,
    RelatedIssue,
    RelevantArtifacts,
    RecentEvidence,
    RelatedAep,
    DriftWarnings,
    Request,
}

impl ContextSection {
    pub fn heading(&self) -> &'static str {
        match self {
            ContextSection::OriginalIntent => "Original Intent",
            ContextSection::CurrentPhase => "Current Phase",
            ContextSection::SelectedContext => "Selected Context",
            ContextSection::RelevantScope => "Relevant Scope",
            ContextSection::ActiveDecisions => "Active Decisions",
            ContextSection::ArchitectureConstraints => "Architecture Constraints",
            ContextSection::RelatedIssue => "Related Issue",
            ContextSection::RelevantArtifacts => "Relevant Artifacts",
            ContextSection::RecentEvidence => "Recent Evidence",
            ContextSection::RelatedAep => "Related AEP",
            ContextSection::DriftWarnings => "Drift Warnings",
            ContextSection::Request => "Request",
        }
    }
}

/// Weighting strategy from V1_DEVELOPMENT.md §43. Kept as data so it can be
/// tuned without touching the compiler.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextBudget {
    pub total_tokens: usize,
    /// section -> share of the total budget, ratios need not sum to 1.
    pub shares: Vec<(ContextSection, f64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackItem {
    pub section: ContextSection,
    pub source_ref: String,
    pub content: String,
    pub priority: u8,
    pub token_cost: usize,
    #[serde(default)]
    pub redacted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPack {
    pub id: String,
    pub project_id: String,
    pub request: String,
    pub items: Vec<ContextPackItem>,
    pub markdown: String,
    pub token_estimate: usize,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub policy_applied: ProjectPolicy,
    #[serde(default)]
    pub redacted_count: usize,
}

/// Per-project data handling policy (V1_DEVELOPMENT.md §53).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPolicy {
    /// Nothing leaves the machine.
    #[serde(default)]
    pub local_only: bool,
    #[serde(default)]
    pub allow_cloud_model: bool,
    #[serde(default)]
    pub never_send_files: bool,
    #[serde(default)]
    pub never_send_raw_conversation: bool,
    #[serde(default = "default_true")]
    pub redact_secrets: bool,
}

fn default_true() -> bool {
    true
}

/// Explicit user statement that outranks any derived fact (PROJECT_MASTER §7.4).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserOverride {
    pub id: String,
    pub project_id: String,
    pub subject_type: String,
    pub subject_id: String,
    pub field: String,
    pub value: serde_json::Value,
    pub reason: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditLog {
    pub id: String,
    pub project_id: String,
    pub component: String,
    pub action: String,
    #[serde(default)]
    pub read_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub produced_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_agent: Option<String>,
    #[serde(default)]
    pub redacted_refs: Vec<String>,
    pub created_at: DateTime<Utc>,
}
