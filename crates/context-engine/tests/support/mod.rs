//! Fixture plumbing for the context-engine suites: a real SQLite-backed project,
//! reached through the same `Repositories` the CLI will use, so a pack is compiled
//! against storage behaviour rather than a mock's opinion of it.

#![allow(dead_code)]

use std::sync::Arc;

use vibe_core::event_store::{EventDraft, EventStore};
use vibe_core::ports::{EntityStore, Repositories};
use vibe_core::project_service::{BaselineDraft, CharterDraft, ProjectService};
use vibe_core::Result;
use vibe_context_engine::ContextCompiler;
use vibe_domain::{
    event_catalog, ActorType, AepStatus, ArchitectureEvolutionPlan, ArchitectureInvariant,
    ArchitectureLayer, Artifact, ArtifactKind, Conversation, Decision, DecisionStatus, DriftAlert,
    DriftSeverity, DriftType, EntityKind, Issue, IssueStatus, Message, MessageRole, ScopeSection,
    Source, SourceType, Task, TaskStatus,
};
use vibe_state_engine::ProjectMapCompiler;
use vibe_storage::SqliteStore;

pub struct Fixture {
    #[allow(dead_code)]
    pub store: Arc<SqliteStore>,
    pub repos: Repositories,
    pub projects: ProjectService,
    pub events: EventStore,
    pub compiler: ContextCompiler,
    pub project_id: String,
}

/// A project with something on every rung of the §42 ladder.
pub fn fixture() -> Fixture {
    let store = SqliteStore::open_in_memory().expect("open in-memory store");
    let repos = store.repositories();
    let projects = ProjectService::new(repos.clone());
    let project = projects
        .create_project(
            "构览 Workbench",
            Some("personal vibe-coding workbench".into()),
            Some("/tmp/workbench".into()),
            Some("Stage C: context engine".into()),
        )
        .expect("create project");
    let project_id = project.id.clone();

    projects
        .save_charter(
            &project_id,
            CharterDraft {
                original_problem: "chat history is scattered across agents".into(),
                core_value: "one local project the user trusts".into(),
                target_user: Some("a solo developer".into()),
                must_have: vec!["local-first storage".into()],
                constraints: vec!["no cloud sync in V1".into()],
                architecture_principles: vec!["event-sourced state".into()],
                ..Default::default()
            },
            ActorType::User,
        )
        .expect("charter");

    projects
        .update_scope(
            &project_id,
            vec![
                (ScopeSection::InScope, "context engine".into()),
                (ScopeSection::OutOfScope, "multi-device sync".into()),
            ],
            Vec::new(),
            ActorType::User,
        )
        .expect("scope");

    projects
        .set_baseline(
            &project_id,
            BaselineDraft {
                summary: "ports in core, adapters in storage".into(),
                layers: vec![ArchitectureLayer {
                    id: "L1".into(),
                    name: "domain".into(),
                    responsibility: Some("types only".into()),
                }],
                invariants: vec![ArchitectureInvariant {
                    id: "INV-1".into(),
                    text: "the engine never talks to SQLite".into(),
                    detect: vec!["rusqlite".into()],
                }],
                replaceable_parts: vec!["adapters".into()],
            },
            ActorType::User,
        )
        .expect("baseline");

    let events = EventStore::new(repos.clone());
    let compiler = ContextCompiler::new(repos.clone());
    Fixture {
        store,
        repos,
        projects,
        events,
        compiler,
        project_id,
    }
}

impl Fixture {
    pub fn issue(&self, id: &str, title: &str, status: IssueStatus) {
        self.repos
            .entities
            .put_entity(
                EntityKind::Issue,
                &Issue {
                    id: id.to_string(),
                    project_id: self.project_id.clone(),
                    title: title.to_string(),
                    status,
                    ..Default::default()
                },
                Some(status.as_str()),
            )
            .expect("issue");
    }

    pub fn task(&self, id: &str, title: &str, status: TaskStatus) {
        self.repos
            .entities
            .put_entity(
                EntityKind::Task,
                &Task {
                    id: id.to_string(),
                    project_id: self.project_id.clone(),
                    title: title.to_string(),
                    status,
                    ..Default::default()
                },
                Some(status.as_str()),
            )
            .expect("task");
    }

    pub fn decision(&self, id: &str, title: &str, decision: &str, status: DecisionStatus) {
        self.repos
            .entities
            .put_entity(
                EntityKind::Decision,
                &Decision {
                    id: id.to_string(),
                    project_id: self.project_id.clone(),
                    title: title.to_string(),
                    decision: decision.to_string(),
                    status,
                    locked: status == DecisionStatus::Active,
                    ..Default::default()
                },
                Some("active"),
            )
            .expect("decision");
    }

    pub fn artifact(&self, id: &str, path: &str) {
        self.repos
            .artifacts
            .upsert_artifact(&Artifact {
                id: id.to_string(),
                project_id: self.project_id.clone(),
                name: path.rsplit('/').next().unwrap_or(path).to_string(),
                kind: ArtifactKind::SourceFile,
                path: Some(path.to_string()),
                hash: Some(format!("sha256:{id}")),
                version: 3,
                ..Default::default()
            })
            .expect("artifact");
    }

    pub fn aep(&self, code: &str, title: &str, status: AepStatus) {
        self.repos
            .entities
            .put_entity(
                EntityKind::Aep,
                &ArchitectureEvolutionPlan {
                    id: code.to_string(),
                    code: code.to_string(),
                    project_id: self.project_id.clone(),
                    title: title.to_string(),
                    current: "single node".into(),
                    limitation: "no failover".into(),
                    why_not_now: "one device today".into(),
                    trigger: vec!["second device".into()],
                    target: "two nodes".into(),
                    status,
                    ..Default::default()
                },
                Some(status.label()),
            )
            .expect("aep");
    }

    pub fn drift(&self, id: &str, summary: &str) {
        self.repos
            .entities
            .put_entity(
                EntityKind::DriftAlert,
                &DriftAlert {
                    id: id.to_string(),
                    project_id: self.project_id.clone(),
                    r#type: DriftType::ArchitectureDrift,
                    severity: DriftSeverity::Warning,
                    summary: summary.to_string(),
                    ..Default::default()
                },
                Some("open"),
            )
            .expect("drift");
    }

    /// An event's subject is the entity the projection writes, so it has to be of the
    /// kind that event type resolves to — `resolve_id` prefers `subject_id`.
    pub fn event(&self, kind: &str, summary: &str) -> Result<()> {
        let subject: (&str, &str) = match kind {
            event_catalog::DECISION_CONFIRMED
            | event_catalog::DECISION_PROPOSED
            | event_catalog::DECISION_REVERSED => ("decision", "D-001"),
            event_catalog::ISSUE_RESOLVED => ("issue", "I-019"),
            event_catalog::TASK_STARTED
            | event_catalog::TASK_BLOCKED
            | event_catalog::TASK_COMPLETED_CLAIM
            | event_catalog::TASK_COMPLETED_CONFIRMED
            | event_catalog::TASK_CANCELLED => ("task", "T-1"),
            _ => ("issue", "I-018"),
        };
        self.events
            .append(
                EventDraft::new(self.project_id.clone(), kind)
                    .agent()
                    .subject(subject.0, subject.1)
                    .payload(serde_json::json!({ "summary": summary })),
            )
            .map(|_| ())
    }

    /// One source, one conversation, and the messages the §42.9 rung is allowed to read.
    pub fn conversation(&self, messages: &[(&str, MessageRole)]) -> Vec<String> {
        let source = Source {
            id: "src-1".into(),
            project_id: self.project_id.clone(),
            r#type: SourceType::GenericCli,
            external_id: None,
            display_name: "cli".into(),
            metadata: serde_json::json!({}),
        };
        self.repos
            .conversations
            .upsert_source(&source)
            .expect("source");
        let conversation = Conversation {
            id: "conv-1".into(),
            project_id: self.project_id.clone(),
            source_id: source.id.clone(),
            external_conversation_id: None,
            title: Some("reconnect work".into()),
            started_at: None,
            last_seen_at: None,
        };
        self.repos
            .conversations
            .upsert_conversation(&conversation)
            .expect("conversation");
        let batch: Vec<Message> = messages
            .iter()
            .map(|(text, role)| {
                Message::new(
                    conversation.id.clone(),
                    *role,
                    (*text).to_string(),
                    None,
                    vibe_domain::now(),
                )
            })
            .collect();
        let ids: Vec<String> = batch.iter().map(|m| m.id.clone()).collect();
        self.repos
            .conversations
            .insert_messages(&conversation.id, &batch)
            .expect("messages");
        ids
    }

    /// The §42.10 rung reads the cached map, so a fixture that wants it must compile
    /// it the way the app would.
    pub fn save_project_map(&self) {
        ProjectMapCompiler::new(self.repos.clone())
            .save(&self.project_id)
            .expect("save map");
    }

    /// Every §42 rung with at least one fact on it.
    pub fn populate(&self) {
        self.decision(
            "D-001",
            "State is a projection",
            "fold accepted events into ProjectState",
            DecisionStatus::Active,
        );
        self.decision(
            "D-000",
            "Superseded approach",
            "keep a mutable state row",
            DecisionStatus::Superseded,
        );
        self.issue("I-018", "sync duplicates messages", IssueStatus::Open);
        self.issue("I-019", "old import path", IssueStatus::Resolved);
        self.task("T-1", "implement the context engine", TaskStatus::Doing);
        self.task("T-2", "retire the legacy importer", TaskStatus::Todo);
        self.task("T-0", "ship stage A", TaskStatus::Done);
        self.artifact("A-1", "crates/context-engine/src/compiler.rs");
        self.artifact("A-2", "crates/core/src/ports.rs");
        self.aep("AEP-001", "multi-device sync", AepStatus::Dormant);
        self.drift("DR-1", "a cloud call appeared in the ingest path");
        let _ = self.event(event_catalog::ISSUE_FOUND, "sync duplicates messages");
        let _ = self.event(event_catalog::DECISION_CONFIRMED, "state is a projection");
    }
}
