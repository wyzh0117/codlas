//! Project state objects: Task / Issue / Decision / Artifact / Milestone /
//! ChangeProposal / AEP / DriftAlert (V1_DEVELOPMENT.md §19-§25).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    #[default]
    Todo,
    Doing,
    Blocked,
    Done,
    Cancelled,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Todo => "todo",
            TaskStatus::Doing => "doing",
            TaskStatus::Blocked => "blocked",
            TaskStatus::Done => "done",
            TaskStatus::Cancelled => "cancelled",
        }
    }
}

/// An agent claiming completion is not the same as completion (§19).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub project_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default)]
    pub status: TaskStatus,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    /// A completion claim that has not yet been backed by evidence.
    #[serde(default)]
    pub unverified_claim: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by_event: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

impl Default for Task {
    fn default() -> Self {
        Self {
            id: String::new(),
            project_id: String::new(),
            title: String::new(),
            detail: None,
            status: TaskStatus::Todo,
            evidence_refs: Vec::new(),
            unverified_claim: false,
            created_by_event: None,
            updated_at: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueStatus {
    #[default]
    Open,
    Investigating,
    Blocked,
    Resolved,
    WontFix,
}

impl IssueStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            IssueStatus::Open => "open",
            IssueStatus::Investigating => "investigating",
            IssueStatus::Blocked => "blocked",
            IssueStatus::Resolved => "resolved",
            IssueStatus::WontFix => "wont_fix",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub id: String,
    pub project_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default)]
    pub status: IssueStatus,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
    #[default]
    Active,
    Superseded,
    Reversed,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub decision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default)]
    pub alternatives: Vec<String>,
    #[serde(default)]
    pub status: DecisionStatus,
    /// User-confirmed decisions win over later agent claims (§7.4 Human Override).
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    #[default]
    SourceFile,
    Config,
    Document,
    Database,
    Prompt,
    Diagram,
    GeneratedOutput,
    Test,
    Script,
    Module,
}

/// Artifacts store path/hash/type/version, never full file bodies (§22).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub id: String,
    pub project_id: String,
    pub name: String,
    #[serde(default)]
    pub kind: ArtifactKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub relationships: Vec<String>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub redesign_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Milestone {
    pub id: String,
    pub project_id: String,
    pub title: String,
    #[serde(default)]
    pub reached: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reached_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeProposalStatus {
    #[default]
    Open,
    Accepted,
    Rejected,
    Superseded,
}

/// Any change touching Charter / Scope / Architecture baseline goes through here
/// before the baseline itself moves (§23, PROJECT_MASTER §19 禁止 5).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeProposal {
    pub id: String,
    pub project_id: String,
    pub proposed_change: String,
    pub reason: String,
    #[serde(default)]
    pub affected_scope: Vec<String>,
    #[serde(default)]
    pub affected_architecture: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub benefit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk: Option<String>,
    pub charter_conflict: bool,
    #[serde(default)]
    pub status: ChangeProposalStatus,
    /// Machine-applicable delta (charter / scope / baseline fields) so accepting
    /// a proposal never depends on re-reading prose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<serde_json::Value>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AepStatus {
    #[default]
    Dormant,
    Watching,
    Triggered,
    InProgress,
    Completed,
    Superseded,
}

impl AepStatus {
    pub fn label(&self) -> &'static str {
        match self {
            AepStatus::Dormant => "Dormant",
            AepStatus::Watching => "Watching",
            AepStatus::Triggered => "Triggered",
            AepStatus::InProgress => "In Progress",
            AepStatus::Completed => "Completed",
            AepStatus::Superseded => "Superseded",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchitectureEvolutionPlan {
    pub id: String,
    pub code: String,
    pub project_id: String,
    pub title: String,
    pub current: String,
    pub limitation: String,
    pub why_not_now: String,
    #[serde(default)]
    pub trigger: Vec<String>,
    pub target: String,
    #[serde(default)]
    pub change_surface: Vec<String>,
    #[serde(default)]
    pub preserved_core: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub benefit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<String>,
    #[serde(default)]
    pub status: AepStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriftType {
    #[default]
    ScopeDrift,
    ArchitectureDrift,
    DecisionConflict,
    FeatureCreep,
    RepeatedRedesign,
    StaleAssumption,
    UnplannedDependency,
}

impl DriftType {
    pub fn as_str(&self) -> &'static str {
        match self {
            DriftType::ScopeDrift => "scope_drift",
            DriftType::ArchitectureDrift => "architecture_drift",
            DriftType::DecisionConflict => "decision_conflict",
            DriftType::FeatureCreep => "feature_creep",
            DriftType::RepeatedRedesign => "repeated_redesign",
            DriftType::StaleAssumption => "stale_assumption",
            DriftType::UnplannedDependency => "unplanned_dependency",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriftSeverity {
    Info,
    #[default]
    Warning,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriftStatus {
    #[default]
    Open,
    AcceptedChange,
    Dismissed,
    Resolved,
}

/// A hint, never a block (PROJECT_MASTER §8.6).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriftAlert {
    pub id: String,
    pub project_id: String,
    pub r#type: DriftType,
    pub severity: DriftSeverity,
    pub summary: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub baseline_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub status: DriftStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detected_at: Option<DateTime<Utc>>,
}
