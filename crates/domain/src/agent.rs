//! Adapter-facing contracts and the Inbox (V1_DEVELOPMENT.md §29, §36-§37, §51).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::record::{Conversation, Message};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Capability {
    ReadHistory,
    ReadCurrentThread,
    OpenApp,
    OpenThread,
    InjectText,
    ReadLocalSession,
    Mcp,
    FileContext,
    StreamResult,
    /// Never assumed in V1 (V1_DEVELOPMENT.md §37).
    AutoExecute,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    pub adapter_id: String,
    pub capabilities: Vec<Capability>,
}

impl AgentCapabilities {
    pub fn has(&self, c: Capability) -> bool {
        self.capabilities.contains(&c)
    }
}

/// One cursor per source: sync stays incremental and resumable (§51).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncCursor {
    pub adapter_id: String,
    pub source_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_content_hash: Option<String>,
}

/// What an adapter hands back after ingesting since a cursor.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedConversationBatch {
    pub conversation: Conversation,
    pub messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// Messages dropped by dedup level L1/L2.
    #[serde(default)]
    pub duplicate_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTarget {
    Web,
    Cli,
    LocalAgent,
    Mcp,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedPayload {
    pub adapter_id: String,
    pub target: AgentTarget,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deep_link: Option<String>,
    /// True when the adapter is unavailable and the user must copy manually (§62).
    pub copy_fallback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InjectResult {
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InboxKind {
    LowConfidenceEvent,
    DecisionProposal,
    ChangeProposal,
    DriftAlert,
    MergeCandidate,
    UnverifiedClaim,
    Conflict,
}

/// A thing waiting for the user's judgement, never an automatic mutation (§29).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxItem {
    pub id: String,
    pub project_id: String,
    pub kind: InboxKind,
    pub summary: String,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub actions: Vec<String>,
}
