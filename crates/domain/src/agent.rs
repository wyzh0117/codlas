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

/// §58's last verb: what a human does when the adapter cannot inject. Unlike `open`,
/// `prepare` and `inject` this always has an answer — a run that ends in silence is the
/// §62 failure mode, whereas "paste it yourself, here is the link" is a degraded but
/// complete path. It carries no claim of having sent anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fallback {
    pub adapter_id: String,
    /// The instruction printed to the user, in the workbench's own words.
    pub instruction: String,
    /// A link worth clicking when the adapter knows one (§36 `deepLink`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deep_link: Option<String>,
    /// Where the payload is on disk when the adapter wrote one, else the caller's own
    /// `--out`. `None` means "the text is only in the terminal".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_path: Option<String>,
}

/// Whether an adapter is usable right now — Stage H "Adapter Health" and the input to
/// §62 Degradation. The spec freezes the `adapter_states` table but not its values, so
/// these are exactly the four outcomes a pipeline run can observe (ADR-021).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterHealth {
    /// Detected, ingested, cursor advanced.
    Ok,
    /// `detect()` found no such source on this machine. Nothing was attempted.
    Unavailable,
    /// Material landed but part of the run failed — usually the cursor write.
    Degraded,
    /// The run produced nothing and the cursor stayed where it was.
    Failed,
}

impl AdapterHealth {
    pub fn as_str(&self) -> &'static str {
        match self {
            AdapterHealth::Ok => "ok",
            AdapterHealth::Unavailable => "unavailable",
            AdapterHealth::Degraded => "degraded",
            AdapterHealth::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Option<AdapterHealth> {
        Some(match value {
            "ok" => AdapterHealth::Ok,
            "unavailable" => AdapterHealth::Unavailable,
            "degraded" => AdapterHealth::Degraded,
            "failed" => AdapterHealth::Failed,
            _ => return None,
        })
    }
}

/// One row of `adapter_states`: what the most recent run learned about one adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterState {
    pub adapter_id: String,
    pub health: AdapterHealth,
    /// The error text, or an empty-batch note. Shown as-is; never a stack trace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub updated_at: DateTime<Utc>,
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
