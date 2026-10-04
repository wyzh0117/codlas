//! Project identity and the three baselines: Charter / Scope / Architecture
//! (V1_DEVELOPMENT.md §8-§11).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    #[default]
    Active,
    Paused,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_path: Option<String>,
    #[serde(default)]
    pub status: ProjectStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_phase: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The project's original design intent. AI may only propose changes to it;
/// every write keeps the previous version (V1_DEVELOPMENT.md §9).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCharter {
    pub id: String,
    pub project_id: String,

    pub original_problem: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_user: Option<String>,
    pub core_value: String,

    #[serde(default)]
    pub must_have: Vec<String>,
    #[serde(default)]
    pub wont_have: Vec<String>,

    #[serde(default)]
    pub success_criteria: Vec<String>,
    #[serde(default)]
    pub architecture_principles: Vec<String>,
    #[serde(default)]
    pub constraints: Vec<String>,

    pub version: u32,
    /// When locked, only an explicit user action may change the charter.
    pub locked: bool,

    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScopeSection {
    InScope,
    OutOfScope,
    Deferred,
    Forbidden,
}

impl ScopeSection {
    pub fn as_str(&self) -> &'static str {
        match self {
            ScopeSection::InScope => "inScope",
            ScopeSection::OutOfScope => "outOfScope",
            ScopeSection::Deferred => "deferred",
            ScopeSection::Forbidden => "forbidden",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeBoundary {
    pub id: String,
    pub project_id: String,

    #[serde(default)]
    pub in_scope: Vec<String>,
    #[serde(default)]
    pub out_of_scope: Vec<String>,
    #[serde(default)]
    pub deferred: Vec<String>,
    #[serde(default)]
    pub forbidden: Vec<String>,

    pub version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchitectureLayer {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub responsibility: Option<String>,
}

/// An invariant the project promises to keep. `detect` holds literal patterns
/// (module paths, import tokens) the guard engine matches against evidence
/// (V1_DEVELOPMENT.md §11, §26.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchitectureInvariant {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub detect: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchitectureBaseline {
    pub id: String,
    pub project_id: String,

    pub summary: String,
    #[serde(default)]
    pub layers: Vec<ArchitectureLayer>,
    #[serde(default)]
    pub invariants: Vec<ArchitectureInvariant>,
    #[serde(default)]
    pub replaceable_parts: Vec<String>,

    pub version: u32,
    pub active: bool,
}
