//! Event Store record + the V1 event catalogue (V1_DEVELOPMENT.md §15, §16).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorType {
    User,
    Agent,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventStatus {
    Accepted,
    PendingReview,
    Rejected,
    Superseded,
}

/// V1 only ever reduces `Accepted` events. Anything else stays inert in the store.
impl EventStatus {
    pub fn is_reducible(self) -> bool {
        matches!(self, EventStatus::Accepted)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            EventStatus::Accepted => "accepted",
            EventStatus::PendingReview => "pending_review",
            EventStatus::Rejected => "rejected",
            EventStatus::Superseded => "superseded",
        }
    }
}

impl ActorType {
    pub fn as_str(self) -> &'static str {
        match self {
            ActorType::User => "user",
            ActorType::Agent => "agent",
            ActorType::System => "system",
        }
    }
}

/// An immutable fact about the project. State is derived from these, never written
/// directly by a model summary (PROJECT_MASTER.md §7.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEvent {
    pub id: String,
    pub project_id: String,

    /// One of [`crate::event_catalog`] constants.
    pub r#type: String,
    pub schema_version: u32,

    pub actor_type: ActorType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,

    /// Reserved for personal multi-device sync (AEP-001); always None in V1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_node_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_id: Option<String>,

    pub payload: serde_json::Value,

    pub confidence: f64,
    #[serde(default)]
    pub evidence_refs: Vec<String>,

    pub status: EventStatus,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<DateTime<Utc>>,
    pub observed_at: DateTime<Utc>,

    /// Monotonic global order assigned by the store; makes replay deterministic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<i64>,
}

/// Idempotency key: re-ingesting the same source material must not create a
/// second event (V1_DEVELOPMENT.md §51, §52). Who said it and what it currently
/// deserves to be are part of the identity: a pending candidate and the accepted
/// fact a user later confirms are two different statements about the project.
pub fn event_dedup_key(
    project_id: &str,
    event_type: &str,
    source_id: Option<&str>,
    message_id: Option<&str>,
    subject_id: Option<&str>,
    payload: &serde_json::Value,
    actor_type: ActorType,
    status: EventStatus,
) -> String {
    let body = serde_json::to_string(payload).unwrap_or_default();
    let mut parts = vec![
        project_id.to_string(),
        event_type.to_string(),
        source_id.unwrap_or("-").to_string(),
        message_id.unwrap_or("-").to_string(),
        subject_id.unwrap_or("-").to_string(),
        format!("actor={}", actor_type.as_str()),
        format!("status={}", status.as_str()),
        crate::ids::content_hash(&body),
    ];
    parts.sort();
    parts.join("|")
}

/// A reviewed event keeps its identity; only judgement fields move (§27, §29).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEventPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<EventStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_refs: Option<Vec<String>>,
}
