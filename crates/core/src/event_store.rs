//! Event Store front door: everything important becomes an Event first
//! (PROJECT_MASTER §7.2). Appends are idempotent and replay is ordered by the
//! store-assigned `seq`, which is what makes the reducer deterministic.

use vibe_domain::{
    event_dedup_key, ActorType, EventStatus, ProjectEvent, ProjectEventPatch,
};

use crate::error::{CoreError, Result};
use crate::ports::{AppendOutcome, Repositories};

/// Convenience constructor for the fields callers actually care about.
#[derive(Debug, Clone)]
pub struct EventDraft {
    pub project_id: String,
    pub event_type: String,
    pub actor_type: ActorType,
    pub actor_id: Option<String>,
    pub source_id: Option<String>,
    pub conversation_id: Option<String>,
    pub message_id: Option<String>,
    pub subject_type: Option<String>,
    pub subject_id: Option<String>,
    pub correlation_id: Option<String>,
    pub causation_id: Option<String>,
    pub payload: serde_json::Value,
    pub confidence: f64,
    pub status: EventStatus,
    pub evidence_refs: Vec<String>,
}

impl EventDraft {
    pub fn new(project_id: impl Into<String>, event_type: impl Into<String>) -> Self {
        Self {
            project_id: project_id.into(),
            event_type: event_type.into(),
            actor_type: ActorType::System,
            actor_id: None,
            source_id: None,
            conversation_id: None,
            message_id: None,
            subject_type: None,
            subject_id: None,
            correlation_id: None,
            causation_id: None,
            payload: serde_json::json!({}),
            confidence: 1.0,
            status: EventStatus::Accepted,
            evidence_refs: Vec::new(),
        }
    }

    pub fn actor(mut self, actor: ActorType) -> Self {
        self.actor_type = actor;
        self
    }

    pub fn agent(mut self) -> Self {
        self.actor_type = ActorType::Agent;
        self
    }

    pub fn user(mut self) -> Self {
        self.actor_type = ActorType::User;
        self
    }

    pub fn pending(mut self) -> Self {
        self.status = EventStatus::PendingReview;
        self
    }

    pub fn payload(mut self, payload: serde_json::Value) -> Self {
        self.payload = payload;
        self
    }

    pub fn subject(mut self, kind: impl Into<String>, id: impl Into<String>) -> Self {
        self.subject_type = Some(kind.into());
        self.subject_id = Some(id.into());
        self
    }

    pub fn from_message(
        mut self,
        source_id: impl Into<String>,
        conversation_id: impl Into<String>,
        message_id: impl Into<String>,
    ) -> Self {
        self.source_id = Some(source_id.into());
        self.conversation_id = Some(conversation_id.into());
        self.message_id = Some(message_id.into());
        self
    }

    pub fn confidence(mut self, value: f64) -> Self {
        self.confidence = value;
        self
    }

    pub fn evidence(mut self, refs: Vec<String>) -> Self {
        self.evidence_refs = refs;
        self
    }

    pub fn into_event(self) -> ProjectEvent {
        ProjectEvent {
            id: vibe_domain::new_id(),
            project_id: self.project_id,
            r#type: self.event_type,
            schema_version: 1,
            actor_type: self.actor_type,
            actor_id: self.actor_id,
            source_id: self.source_id,
            conversation_id: self.conversation_id,
            message_id: self.message_id,
            origin_node_id: None,
            causation_id: self.causation_id,
            correlation_id: self.correlation_id,
            subject_type: self.subject_type,
            subject_id: self.subject_id,
            payload: self.payload,
            confidence: self.confidence,
            evidence_refs: self.evidence_refs,
            status: self.status,
            occurred_at: None,
            observed_at: vibe_domain::now(),
            seq: None,
        }
    }
}

pub struct EventStore {
    repos: Repositories,
}

impl EventStore {
    pub fn new(repos: Repositories) -> Self {
        Self { repos }
    }

    pub fn append(&self, draft: EventDraft) -> Result<AppendOutcome> {
        let event = draft.into_event();
        self.append_event(event)
    }

    pub fn append_event(&self, event: ProjectEvent) -> Result<AppendOutcome> {
        if !vibe_domain::event_catalog::is_known(&event.r#type) {
            return Err(CoreError::Validation(format!(
                "unknown event type {}",
                event.r#type
            )));
        }
        let dedup = event_dedup_key(
            &event.project_id,
            &event.r#type,
            event.source_id.as_deref(),
            event.message_id.as_deref(),
            event.subject_id.as_deref(),
            &event.payload,
            event.actor_type,
            event.status,
        );
        self.repos.events.append(&event, &dedup)
    }

    /// Accepted events in `seq` order — the only input the reducer may read.
    pub fn accepted(&self, project_id: &str, after_seq: Option<i64>) -> Result<Vec<ProjectEvent>> {
        self.repos.events.list_accepted(project_id, after_seq)
    }

    pub fn list(
        &self,
        project_id: &str,
        statuses: Option<&[EventStatus]>,
        limit: Option<usize>,
    ) -> Result<Vec<ProjectEvent>> {
        self.repos.events.list_events(project_id, statuses, limit)
    }

    pub fn get(&self, id: &str) -> Result<Option<ProjectEvent>> {
        self.repos.events.get_event(id)
    }

    /// Inbox resolution: a human moves a pending event into a final judgement.
    pub fn review(
        &self,
        event_id: &str,
        status: EventStatus,
        confidence: Option<f64>,
    ) -> Result<ProjectEvent> {
        let existing = self
            .repos
            .events
            .get_event(event_id)?
            .ok_or_else(|| CoreError::NotFound {
                what: format!("event {event_id}"),
            })?;
        self.repos.events.patch_event(
            event_id,
            &ProjectEventPatch {
                status: Some(status),
                confidence,
                evidence_refs: None,
            },
        )?;
        Ok(ProjectEvent {
            status,
            confidence: confidence.unwrap_or(existing.confidence),
            ..existing
        })
    }

    pub fn is_duplicate(&self, draft: &EventDraft) -> Result<bool> {
        let dedup = event_dedup_key(
            &draft.project_id,
            &draft.event_type,
            draft.source_id.as_deref(),
            draft.message_id.as_deref(),
            draft.subject_id.as_deref(),
            &draft.payload,
            draft.actor_type,
            draft.status,
        );
        self.repos.events.contains_dedup_key(&dedup)
    }
}
