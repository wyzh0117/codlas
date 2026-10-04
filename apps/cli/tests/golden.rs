//! Golden regression (V1_DEVELOPMENT.md §58): the four fixed inputs — Charter,
//! Conversation, Git events, Files — replayed through the whole loop, checked against
//! the outputs the documents freeze: Events, Decisions, Tasks, Drift Alerts, Project Map.
//!
//! Assertions name the *content* rather than snapshotting bytes, because the projection
//! legitimately carries wall-clock stamps and freshly generated ids. What must not vary
//! is what the map says about the project — that is checked line by line, and the
//! determinism test checks the bytes separately.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use vibe_cli::baseline::{BaselineSpec, CharterSpec, ProjectHead, ScopeSpec};
use vibe_core::event_store::EventStore;
use vibe_core::ports::Repositories;
use vibe_core::project_service::ProjectService;
use vibe_domain::{
    ActorType, DecisionStatus, DriftType, EventStatus, ProjectState, SourceType, TaskStatus,
};
use vibe_guard_engine::DriftEngine;
use vibe_ingestion::{
    CommandRunner, CommitRecord, FileScan, GitFacts, IngestPipeline, TranscriptAdapter,
};
use vibe_state_engine::{ProjectMapCompiler, Reducer};
use vibe_storage::SqliteStore;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .join("tests")
        .join("fixtures")
}

/// A runner that refuses to shell out. The fixture hands `GitFacts` its commits
/// directly, so if a future change makes capture reach for `/usr/bin/git`, this test
/// fails here instead of quietly depending on whatever history the machine has.
struct NoShell;

impl CommandRunner for NoShell {
    fn run(
        &self,
        program: &str,
        args: &[&str],
        _workdir: &Path,
    ) -> vibe_core::error::Result<vibe_ingestion::CommandOutcome> {
        Err(vibe_core::CoreError::Validation(format!(
            "golden fixture must not execute {program} {args:?}"
        )))
    }
}

/// One project's whole closed loop, rebuilt from the fixtures on every call.
struct ClosedLoop {
    repos: Repositories,
    project_id: String,
}

impl ClosedLoop {
    fn build() -> Self {
        let store = SqliteStore::open_in_memory().expect("in-memory store");
        let repos = store.repositories();
        let service = ProjectService::new(repos.clone());

        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Spec {
            project: ProjectHead,
            charter: CharterSpec,
            scope: ScopeSpec,
            baseline: BaselineSpec,
        }

        // Input 4: Charter. The fixture is this product's own charter, so the drift
        // rules judge the real boundaries rather than invented ones.
        let raw = std::fs::read_to_string(fixtures().join("charter.json")).expect("charter.json");
        let spec: Spec = serde_json::from_str(&raw).expect("charter fixture parses");
        let project = service
            .create_project(
                &spec.project.name,
                spec.project.description.clone(),
                spec.project.root_path.clone(),
                spec.project.current_phase.clone(),
            )
            .unwrap();
        service
            .save_charter(&project.id, spec.charter.into(), ActorType::User)
            .unwrap();
        service
            .update_scope(&project.id, spec.scope.additions(), Vec::new(), ActorType::User)
            .unwrap();
        service
            .set_baseline(&project.id, spec.baseline.into(), ActorType::User)
            .unwrap();

        // Input 1: Conversation.
        let mut pipeline = IngestPipeline::with_rules(repos.clone());
        pipeline.register(Arc::new(TranscriptAdapter::file(
            "golden-transcript",
            SourceType::ChatgptWeb,
            fixtures().join("transcript.json"),
        )));
        let report = pipeline.run(&project.id).expect("transcript ingest");
        assert!(
            report.total_new_messages > 0,
            "the transcript fixture must carry messages"
        );

        // Input 2: Git events, as recorded commits.
        let git = GitFacts::new(repos.clone(), Arc::new(NoShell));
        let commits = vec![
            CommitRecord::new(
                "1a2b3c4d5e6f",
                "state-engine: fold the accepted log into ProjectState",
                &["crates/state-engine/src/projector.rs"],
            ),
            CommitRecord::new(
                "7f8e9d0c1b2a",
                "guard-engine: lexical drift rules",
                &[
                    "crates/guard-engine/src/engine.rs",
                    "crates/guard-engine/src/text.rs",
                ],
            ),
        ];
        let first = git.ingest_commits(&project.id, &commits).expect("git ingest");
        assert_eq!(first.events_appended, 2, "each commit is one event");
        let again = git.ingest_commits(&project.id, &commits).expect("git re-ingest");
        assert_eq!(again.events_appended, 0, "re-ingesting commits writes nothing");
        assert_eq!(again.events_duplicate, 2);

        // Input 3: Files — one compliant module, the INV-001 violation, and a roadmap
        // full of out-of-scope wishes.
        let scan = FileScan::new(repos.clone(), fixtures().join("files"))
            .with_source_key("golden-files");
        let scanned = scan.run(&project.id).expect("file scan");
        assert!(scanned.scanned >= 3, "the fixture tree carries its files");
        let rescan = scan.run(&project.id).expect("file re-scan");
        assert_eq!(rescan.events_appended, 0, "an unchanged file is not a change");

        ClosedLoop {
            repos,
            project_id: project.id,
        }
    }

    fn events(&self) -> Vec<vibe_domain::ProjectEvent> {
        EventStore::new(self.repos.clone())
            .list(&self.project_id, None, Some(500))
            .expect("events")
    }

    fn state(&self) -> ProjectState {
        Reducer::new(self.repos.clone())
            .project_state(&self.project_id)
            .expect("state")
    }
}

/// Every accepted event of this type, so a claim and its confirmation are counted
/// separately (§28: they are different facts).
fn accepted<'a>(
    events: &'a [vibe_domain::ProjectEvent],
    kind: &str,
) -> Vec<&'a vibe_domain::ProjectEvent> {
    events
        .iter()
        .filter(|event| event.r#type == kind && event.status == EventStatus::Accepted)
        .collect()
}

#[test]
fn capture_turns_a_transcript_into_the_frozen_events() {
    let harness = ClosedLoop::build();
    let events = harness.events();

    for expected in [
        vibe_domain::event_catalog::TASK_CREATED,
        vibe_domain::event_catalog::DECISION_CONFIRMED,
        vibe_domain::event_catalog::DECISION_PROPOSED,
        vibe_domain::event_catalog::TASK_COMPLETED_CLAIM,
        vibe_domain::event_catalog::ISSUE_FOUND,
        vibe_domain::event_catalog::GIT_COMMIT,
        vibe_domain::event_catalog::FILE_CHANGED,
        vibe_domain::event_catalog::CHARTER_CREATED,
        vibe_domain::event_catalog::SCOPE_ADDED,
    ] {
        assert!(
            events.iter().any(|event| event.r#type == expected),
            "missing {expected} — captured: {:?}",
            events
                .iter()
                .map(|e| (&e.r#type, e.status, e.confidence))
                .collect::<Vec<_>>()
        );
    }

    // §27 routes on confidence, not on politeness, and the fixture carries one of each
    // kind of signal so both branches are pinned. "我建议" is a strong marker: 0.80 base
    // plus 0.12 clears the bar, so the proposal is accepted — as the fact *an agent
    // proposed this*. Refusing to turn that into an Active decision is the projector's
    // job, and the next test checks exactly that.
    let proposals: Vec<_> = events
        .iter()
        .filter(|event| event.r#type == vibe_domain::event_catalog::DECISION_PROPOSED)
        .collect();
    assert_eq!(
        proposals.len(),
        2,
        "the fixture should offer one strong and one weak suggestion, saw {:?}",
        events
            .iter()
            .map(|e| (&e.r#type, e.confidence, e.status))
            .collect::<Vec<_>>()
    );
    assert!(
        proposals.iter().all(|event| event.actor_type == ActorType::Agent),
        "only the assistant suggested anything"
    );
    assert!(
        proposals
            .iter()
            .any(|event| event.confidence >= 0.90 && event.status == EventStatus::Accepted),
        "a strong marker should clear the auto-accept bar"
    );

    // "we should" is a weak marker: 0.80, past the inbox floor but short of auto-accept,
    // so it waits for the user instead of silently becoming project history (§29).
    let pending: Vec<_> = events
        .iter()
        .filter(|event| event.status == EventStatus::PendingReview)
        .collect();
    assert_eq!(
        pending.len(),
        1,
        "exactly one reading should wait, saw {:?}",
        pending
            .iter()
            .map(|e| (&e.r#type, e.confidence))
            .collect::<Vec<_>>()
    );
    assert!(
        pending[0].r#type == vibe_domain::event_catalog::DECISION_PROPOSED
            && (0.65..0.90).contains(&pending[0].confidence),
        "the waiting item should be the weak suggestion, got {:?}",
        pending[0]
    );

    let confirmed = accepted(&events, vibe_domain::event_catalog::DECISION_CONFIRMED);
    assert_eq!(confirmed.len(), 1, "the user confirmed exactly one decision");
    assert_eq!(confirmed[0].actor_type, ActorType::User);
}

#[test]
fn an_agents_completion_claim_never_becomes_done() {
    let harness = ClosedLoop::build();
    let state = harness.state();

    let claimed = state
        .tasks
        .iter()
        .find(|task| task.unverified_claim)
        .expect("a claimed task");
    assert_ne!(
        claimed.status,
        TaskStatus::Done,
        "禁止 6: a model's summary may not overwrite a project fact"
    );
    assert!(
        state
            .tasks
            .iter()
            .all(|task| task.status != TaskStatus::Done),
        "nothing in the fixture is user-confirmed complete"
    );
}

#[test]
fn the_confirmed_decision_is_active_and_the_proposal_is_not() {
    let harness = ClosedLoop::build();
    let state = harness.state();

    let active: Vec<_> = state
        .decisions
        .iter()
        .filter(|decision| decision.status == DecisionStatus::Active)
        .collect();
    assert_eq!(active.len(), 1, "only a user-confirmed decision is active");
    assert!(
        active[0]
            .decision
            .contains("drift") || active[0].title.contains("drift"),
        "the confirmed decision should be the lexical-rule one, got {:?}",
        active[0].title
    );
    assert!(active[0].locked, "a user confirmation locks the decision (§7.4)");
}

#[test]
fn the_drift_rules_fire_on_the_material_they_are_for() {
    let harness = ClosedLoop::build();
    let state = harness.state();
    let engine = DriftEngine::new(harness.repos.clone());
    let alerts = engine.check(&state).expect("drift check");

    let kinds: Vec<DriftType> = alerts.iter().map(|alert| alert.r#type).collect();
    assert!(
        kinds.contains(&DriftType::ArchitectureDrift),
        "INV-001 must notice rusqlite in core, saw {kinds:?}"
    );
    assert!(
        kinds.contains(&DriftType::ScopeDrift) || kinds.contains(&DriftType::FeatureCreep),
        "the roadmap's team workspace and gantt chart are out of bounds, saw {kinds:?}"
    );

    // §26: an alert is a prompt, never a rewrite. The charter still says no.
    let charter = harness
        .repos
        .projects
        .get_active_charter(&harness.project_id)
        .expect("charter")
        .expect("a charter exists");
    assert!(
        charter
            .wont_have
            .iter()
            .any(|item| item.contains("多人协作")),
        "the drift alert must not have edited the charter"
    );
}

#[test]
fn the_project_map_says_what_the_project_actually_is() {
    let harness = ClosedLoop::build();
    let compiler = ProjectMapCompiler::new(harness.repos.clone());
    let map = compiler.compile(&harness.project_id).expect("map");
    let text = map.markdown;

    let headings = [
        "## Original Intent",
        "## Current Phase",
        "## Current Status",
        "## Scope",
        "## Architecture Baseline",
        "## Active Decisions",
        "## Active Tasks",
        "## Open Issues",
        "## Key Artifacts",
        "## Architecture Evolution",
        "## Recent Changes",
        "## Drift / Conflicts",
        "## Next Actions",
    ];
    let mut cursor = 0usize;
    for heading in headings {
        let at = text
            .find(heading)
            .unwrap_or_else(|| panic!("map is missing {heading}:\n{text}"));
        assert!(at >= cursor, "sections out of order: {heading}");
        cursor = at;
    }

    assert!(text.contains("event-first"), "the charter's intent is carried through");
    assert!(
        text.contains("drift 规则保持词法实现"),
        "the user's confirmed decision must be in the map:\n{text}"
    );
    assert!(
        text.contains("INV-001") || text.contains("Core 不直接依赖 SQLite"),
        "the architecture baseline must be visible in the map:\n{text}"
    );
    assert!(
        !text.contains("_none recorded_") || text.matches("_none recorded_").count() < 8,
        "a project with this much history should not read as empty"
    );
}

#[test]
fn replaying_the_same_log_twice_yields_the_same_state_and_map() {
    let harness = ClosedLoop::build();
    let reducer = Reducer::new(harness.repos.clone());
    let compiler = ProjectMapCompiler::new(harness.repos.clone());

    let first = reducer.project_state(&harness.project_id).expect("first");
    let second = reducer.project_state(&harness.project_id).expect("second");
    assert_eq!(first.state_hash, second.state_hash, "state is deterministic");
    assert_eq!(first.applied_events, second.applied_events);

    let events = reducer.accepted_events(&harness.project_id).expect("log");
    let folded = reducer
        .fold(&harness.project_id, &events)
        .expect("pure fold");
    assert_eq!(
        folded.state_hash, first.state_hash,
        "the pure fold agrees with the store-backed projection"
    );

    let a = compiler.compile(&harness.project_id).expect("map a");
    let b = compiler.compile(&harness.project_id).expect("map b");
    assert_eq!(a.markdown, b.markdown, "the map is a deterministic view");
    assert_eq!(a.source_state_hash, b.source_state_hash);
}

#[test]
fn capturing_the_same_transcript_again_changes_nothing() {
    let harness = ClosedLoop::build();
    let before = harness.events().len();
    let state_before = harness.state().state_hash;

    let mut pipeline = IngestPipeline::with_rules(harness.repos.clone());
    pipeline.register(Arc::new(TranscriptAdapter::file(
        "golden-transcript",
        SourceType::ChatgptWeb,
        fixtures().join("transcript.json"),
    )));
    let report = pipeline.run(&harness.project_id).expect("re-ingest");

    assert_eq!(report.total_new_messages, 0, "no message is a second fact");
    assert_eq!(report.total_events, 0, "no event is a second fact");
    assert_eq!(harness.events().len(), before, "the log did not grow");
    assert_eq!(
        harness.state().state_hash.as_deref(),
        Some(state_before.as_ref().unwrap().as_str()),
        "state is unchanged when nothing new was captured"
    );
}
