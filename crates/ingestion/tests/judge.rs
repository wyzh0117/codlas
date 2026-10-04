//! The judge routing test (V1_DEVELOPMENT.md §27, §29, §18): the same transcript yields an
//! accepted event, an inbox event, and — below the floor — *nothing at all*. Only accepted
//! events are allowed to move state, which is what the reducer is asked to confirm here.

use std::sync::Arc;

use vibe_core::judge::{JudgeThresholds, RuleJudge, SignalJudge};
use vibe_core::ports::Repositories;
use vibe_core::project_service::ProjectService;
use vibe_domain::{event_catalog, EventStatus, ProjectEvent};
use vibe_ingestion::{IngestPipeline, ManualImportAdapter};
use vibe_state_engine::Reducer;
use vibe_storage::SqliteStore;

fn repos() -> (Arc<SqliteStore>, Repositories, String) {
    let store = SqliteStore::open_in_memory().expect("in-memory store");
    let repositories = store.repositories();
    let project = ProjectService::new(repositories.clone())
        .create_project("Judge", None, None, None)
        .expect("project");
    (store, repositories, project.id)
}

fn events(repositories: &Repositories, project_id: &str) -> Vec<ProjectEvent> {
    repositories
        .events
        .list_events(project_id, None, None)
        .expect("events")
}

/// Everything the pipeline wrote, minus the `PROJECT_CREATED` event the project service
/// appends when the project is opened.
fn captured(repositories: &Repositories, project_id: &str) -> Vec<ProjectEvent> {
    let mut out: Vec<ProjectEvent> = events(repositories, project_id)
        .into_iter()
        .filter(|e| e.r#type != event_catalog::PROJECT_CREATED)
        .collect();
    out.sort_by(|a, b| a.r#type.cmp(&b.r#type));
    out
}

fn one(event_type: &str, list: &[ProjectEvent]) -> ProjectEvent {
    list.iter()
        .find(|e| e.r#type == event_type)
        .unwrap_or_else(|| panic!("no {event_type} event in {:?}", types(list)))
        .clone()
}

fn types(list: &[ProjectEvent]) -> Vec<&str> {
    list.iter().map(|e| e.r#type.as_str()).collect()
}

const TRANSCRIPT: &str = concat!(
    "user: 接下来做 搜索层的测试覆盖\n",
    "assistant: we should switch to an LRU cache\n",
    "user: 确认保留 Repository 抽象层\n",
    "user: 早上好\n",
);

/// A task and two decision readings from one transcript, each on its own route (§27).
#[test]
fn routes_split_accepted_from_pending() {
    let (_store, repositories, project) = repos();
    let mut pipeline = IngestPipeline::new(repositories.clone(), None);
    pipeline.register(Arc::new(ManualImportAdapter::named("judge-thread", TRANSCRIPT)));

    let report = pipeline.run(&project).expect("run");
    let adapter = &report.per_adapter[0];
    assert_eq!(adapter.messages_inserted, 4, "one row per turn");
    assert_eq!(
        adapter.events_appended, 3,
        "confirmation + task + proposal, no more"
    );
    assert_eq!(adapter.events_pending, 1, "only the proposal waits in the Inbox");
    assert_eq!(adapter.events_candidate, 0, "nothing fell below the floor");
    assert_eq!(report.total_events, 3);

    let written = captured(&repositories, &project);
    assert_eq!(types(&written), vec![
        "DECISION_CONFIRMED",
        "DECISION_PROPOSED",
        "TASK_CREATED",
    ]);
    let confirmed = one(event_catalog::DECISION_CONFIRMED, &written);
    assert_eq!(confirmed.status, EventStatus::Accepted);
    assert_eq!(confirmed.actor_type, vibe_domain::ActorType::User);
    assert!(confirmed.confidence >= 0.90, "§27 auto-accept band");
    let proposal = one(event_catalog::DECISION_PROPOSED, &written);
    assert_eq!(
        proposal.status,
        EventStatus::PendingReview,
        "an agent proposal is written for traceability, not belief"
    );
    assert!(proposal.confidence < 0.90 && proposal.confidence >= 0.65);
    assert_eq!(
        one(event_catalog::TASK_CREATED, &written).status,
        EventStatus::Accepted
    );
    // The "早上好" turn is neither an event nor a candidate: a judge that finds nothing
    // writes nothing (PROJECT_MASTER 禁止 9).
    assert!(
        written.iter().all(|e| e.message_id.is_some()),
        "every captured event names the message that justified it (§17)"
    );

    // Only accepted events reduce. The pending proposal must not appear in state.
    let state = Reducer::new(repositories.clone())
        .project_read_only(&project)
        .expect("projection");
    assert_eq!(state.tasks.len(), 1, "{:?}", state.tasks.iter().map(|t| &t.title).collect::<Vec<_>>());
    assert_eq!(state.tasks[0].status, vibe_domain::TaskStatus::Todo);
    assert_eq!(state.decisions.len(), 1, "the pending proposal changed nothing");
    assert_eq!(state.decisions[0].status, vibe_domain::DecisionStatus::Active);
}

/// The same sentence under strict thresholds is a candidate: reported, written nowhere,
/// and provably unable to move state (§27).
#[test]
fn below_the_floor_a_reading_is_reported_not_written() {
    let (_store, repositories, project) = repos();
    let strict: Arc<dyn SignalJudge> = Arc::new(RuleJudge::new(JudgeThresholds {
        auto_accept: 0.95,
        inbox_floor: 0.90,
    }));
    let mut pipeline = IngestPipeline::new(repositories.clone(), Some(strict));
    pipeline.register(Arc::new(ManualImportAdapter::named(
        "strict-thread",
        concat!(
            "assistant: we should switch to an LRU cache\n",
            "user: 早上好\n",
        ),
    )));

    let before = events(&repositories, &project).len();
    let report = pipeline.run(&project).expect("run");
    let adapter = &report.per_adapter[0];
    assert_eq!(adapter.messages_inserted, 2, "the chat itself is still memory");
    assert_eq!(adapter.events_appended, 0);
    assert_eq!(adapter.events_pending, 0);
    assert_eq!(
        adapter.events_candidate, 1,
        "0.80 is under a 0.90 floor: a candidate, not a fact"
    );
    assert_eq!(report.total_events, 0);

    let candidates = report.candidates();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].event_type, event_catalog::DECISION_PROPOSED);
    assert!(candidates[0].confidence < 0.90);
    assert!(!candidates[0].rationale.trim().is_empty());
    assert_eq!(
        events(&repositories, &project).len(),
        before,
        "a candidate wrote no event row"
    );

    let state = Reducer::new(repositories.clone())
        .project_read_only(&project)
        .expect("projection");
    assert!(state.decisions.is_empty(), "{:?}", state.decisions);
    assert!(state.tasks.is_empty(), "{:?}", state.tasks);

    // And re-running the source repeats the whole result: no second candidate, no message.
    let again = pipeline.run(&project).expect("second run");
    assert_eq!(again.total_new_messages, 0);
    assert_eq!(again.per_adapter[0].events_candidate, 0);
    assert_eq!(events(&repositories, &project).len(), before);
}

/// §27's `user_only` rule is about the actor, not the wording: an assistant cannot confirm
/// a decision or open a task, so those readings never reach the log at all.
#[test]
fn an_agent_cannot_confirm_or_create() {
    let (_store, repositories, project) = repos();
    let mut pipeline = IngestPipeline::new(repositories.clone(), None);
    pipeline.register(Arc::new(ManualImportAdapter::named(
        "agent-thread",
        concat!(
            "assistant: 确认保留 Repository 抽象层\n",
            "assistant: 接下来做 搜索层的测试覆盖\n",
        ),
    )));

    let report = pipeline.run(&project).expect("run");
    assert_eq!(report.total_new_messages, 2, "the words are still chat memory");
    assert_eq!(
        report.per_adapter[0].events_appended,
        0,
        "both triggers are user-only rules"
    );
    assert_eq!(
        report.per_adapter[0].events_candidate,
        0,
        "a skipped rule is not a low-confidence reading either"
    );
    assert!(
        captured(&repositories, &project).is_empty(),
        "{:?}",
        types(&captured(&repositories, &project))
    );
}

/// PROJECT_MASTER 禁止 9: an agent saying it finished is a *claim*. It is written, it is
/// reducible, and it still does not make the task Done (§19).
#[test]
fn an_agent_claim_stays_a_claim() {
    let (_store, repositories, project) = repos();
    let mut pipeline = IngestPipeline::new(repositories.clone(), None);
    pipeline.register(Arc::new(ManualImportAdapter::named(
        "claim-thread",
        "assistant: 我已经实现了重连逻辑",
    )));

    let report = pipeline.run(&project).expect("run");
    let written = captured(&repositories, &project);
    assert_eq!(types(&written), vec![event_catalog::TASK_COMPLETED_CLAIM]);
    assert_eq!(written[0].status, EventStatus::Accepted);
    assert_eq!(written[0].actor_type, vibe_domain::ActorType::Agent);
    assert_eq!(report.per_adapter[0].events_pending, 0);

    let state = Reducer::new(repositories.clone())
        .project_read_only(&project)
        .expect("projection");
    assert_eq!(state.tasks.len(), 1);
    assert_ne!(
        state.tasks[0].status,
        vibe_domain::TaskStatus::Done,
        "a claim cannot complete work"
    );
    assert!(
        state.tasks[0].unverified_claim,
        "and it is marked unverified for the Inbox to chase"
    );
}
