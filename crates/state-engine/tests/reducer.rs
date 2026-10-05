//! Stage C acceptance for the projection: the accepted log is the only input, the
//! same log always yields the same state, and an agent's claim is not a completion
//! (V1_DEVELOPMENT.md §18, §19, §20; PROJECT_MASTER.md §7.2, 禁止 4).

use chrono::{DateTime, TimeZone, Utc};
use serde_json::{json, Value};
use vibe_core::ports::{EntityStore, Repositories};
use vibe_core::project_service::{CharterDraft, ProjectService};
use vibe_domain::{
    event_catalog, event_dedup_key, ActorType, AepStatus, ChangeProposal, ChangeProposalStatus,
    DecisionStatus, DriftStatus, EntityKind, EventStatus, Project, ProjectEvent, ProjectStatus,
    ScopeSection, Task, TaskStatus,
};
use vibe_state_engine::Reducer;
use vibe_storage::SqliteStore;

const EPOCH: i64 = 1_770_000_000;
const PROJECT: &str = "proj-under-test";

fn clock(seq: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(EPOCH + seq, 0).unwrap()
}

struct Fixture {
    repos: Repositories,
    reducer: Reducer,
}

/// A real store behind the ports, with the project row seeded directly so a second
/// store can hold a byte-for-byte identical log.
fn fixture() -> Fixture {
    let store = SqliteStore::open_in_memory().expect("open store");
    let repos = store.repositories();
    repos
        .projects
        .create_project(&Project {
            id: PROJECT.to_string(),
            name: "state engine".to_string(),
            description: None,
            root_path: None,
            status: ProjectStatus::Active,
            current_phase: None,
            created_at: clock(0),
            updated_at: clock(0),
        })
        .expect("seed project");
    Fixture {
        reducer: Reducer::new(repos.clone()),
        repos,
    }
}

fn event(project_id: &str, seq: i64, kind: &str, payload: Value) -> ProjectEvent {
    ProjectEvent {
        id: format!("e{seq}"),
        project_id: project_id.to_string(),
        r#type: kind.to_string(),
        schema_version: 1,
        actor_type: ActorType::User,
        actor_id: None,
        source_id: None,
        conversation_id: None,
        message_id: None,
        origin_node_id: None,
        causation_id: None,
        correlation_id: None,
        subject_type: None,
        subject_id: None,
        payload,
        confidence: 1.0,
        evidence_refs: Vec::new(),
        status: EventStatus::Accepted,
        occurred_at: Some(clock(seq)),
        observed_at: clock(seq),
        seq: Some(seq),
    }
}

fn subject(mut event: ProjectEvent, kind: &str, id: &str) -> ProjectEvent {
    event.subject_type = Some(kind.to_string());
    event.subject_id = Some(id.to_string());
    event
}

fn by_agent(mut event: ProjectEvent) -> ProjectEvent {
    event.actor_type = ActorType::Agent;
    event
}

fn with_refs(mut event: ProjectEvent, refs: &[&str]) -> ProjectEvent {
    event.evidence_refs = refs.iter().map(|reference| reference.to_string()).collect();
    event
}

fn awaiting_review(mut event: ProjectEvent) -> ProjectEvent {
    event.status = EventStatus::PendingReview;
    event
}

/// Append straight through the port so the test fixes `occurred_at`; `seq` is
/// assigned by the store in append order, which is what makes replay deterministic.
fn record(repos: &Repositories, event: &ProjectEvent) {
    let key = event_dedup_key(
        &event.project_id,
        &event.r#type,
        event.source_id.as_deref(),
        event.message_id.as_deref(),
        event.subject_id.as_deref(),
        &event.payload,
        event.actor_type,
        event.status,
    );
    repos.events.append(event, &key).expect("append event");
}

fn record_all(f: &Fixture, events: &[ProjectEvent]) {
    for event in events {
        record(&f.repos, event);
    }
}

fn task_log() -> Vec<ProjectEvent> {
    vec![
        subject(
            event(
                PROJECT,
                10,
                event_catalog::TASK_CREATED,
                json!({ "title": "wire the reducer" }),
            ),
            "task",
            "task-1",
        ),
        subject(
            event(
                PROJECT,
                11,
                event_catalog::TASK_STARTED,
                json!({ "title": "wire the reducer" }),
            ),
            "task",
            "task-1",
        ),
    ]
}

fn status_of(tasks: &[Task], id: &str) -> Option<String> {
    tasks.iter().find(|task| task.id == id).map(|task| {
        serde_json::to_value(task.status)
            .expect("status")
            .as_str()
            .expect("str status")
            .to_string()
    })
}

#[test]
fn the_same_log_projects_to_the_same_hash_in_two_stores() {
    let first = fixture();
    let second = fixture();
    record_all(&first, &task_log());
    record_all(&second, &task_log());

    let a = first.reducer.project_state(PROJECT).expect("fold a");
    let b = second.reducer.project_state(PROJECT).expect("fold b");
    assert_eq!(a.state_hash, b.state_hash, "replay must be reproducible");
    assert_eq!(a.applied_events, 2);
    assert_eq!(status_of(&a.tasks, "task-1").as_deref(), Some("doing"));
    assert_eq!(
        a.tasks[0].updated_at,
        Some(clock(11)),
        "timestamps come from the event, never from the clock of the run"
    );
}

#[test]
fn folding_is_a_pure_function_of_the_events_handed_in() {
    let f = fixture();
    let events = task_log();
    let pure = f.reducer.fold(PROJECT, &events).expect("pure fold");
    let again = f.reducer.fold(PROJECT, &events).expect("pure fold 2");
    assert_eq!(pure.state_hash, again.state_hash);

    record_all(&f, &events);
    let stored = f.reducer.project_state(PROJECT).expect("project");
    let stored_events = f.reducer.accepted_events(PROJECT).expect("accepted log");
    let from_store = f.reducer.fold(PROJECT, &stored_events).expect("fold of stored events");
    assert_eq!(
        stored.state_hash, from_store.state_hash,
        "the projection depends on the event list alone, not on who produced it"
    );
    assert_eq!(
        pure.tasks.len(),
        stored.tasks.len(),
        "caller seq is ignored, so only ordering may differ — never content"
    );
}

#[test]
fn replay_is_idempotent_in_rows_and_hash() {
    let f = fixture();
    let events = task_log();
    record_all(&f, &events);
    // Re-recording the very same facts changes nothing.
    record_all(&f, &events);

    let first = f.reducer.project_state(PROJECT).expect("fold 1");
    let second = f.reducer.project_state(PROJECT).expect("fold 2");
    assert_eq!(first.state_hash, second.state_hash);
    assert_eq!(first.last_seq, second.last_seq);

    let rows: Vec<Task> = f
        .repos
        .entities
        .entities_of(EntityKind::Task, PROJECT)
        .expect("tasks");
    assert_eq!(rows.len(), 1, "one task row, however many times we replay");
    assert_eq!(status_of(&rows, "task-1").as_deref(), Some("doing"));
}

#[test]
fn state_comes_from_events_not_from_the_cache() {
    let f = fixture();
    record_all(&f, &task_log());
    let before = f.reducer.project_state(PROJECT).expect("fold");

    // Drop the derived rows: a projection that quietly read them back would change.
    f.repos
        .entities
        .delete(EntityKind::Task, "task-1")
        .expect("delete row");
    let after = f.reducer.project_state(PROJECT).expect("refold");

    assert_eq!(before.state_hash, after.state_hash);
    assert_eq!(after.tasks.len(), 1);

    let rebuilt = f.reducer.rebuild(PROJECT).expect("rebuild");
    assert_eq!(rebuilt.state_hash, before.state_hash);
    let cached: Vec<Task> = f
        .repos
        .entities
        .entities_of(EntityKind::Task, PROJECT)
        .expect("tasks");
    assert_eq!(cached.len(), 1, "the cache is rewritten, not doubled");
}

#[test]
fn an_agents_claim_stays_a_claim() {
    let f = fixture();
    let created = subject(
        event(
            PROJECT,
            20,
            event_catalog::TASK_CREATED,
            json!({ "title": "add the map compiler" }),
        ),
        "task",
        "task-2",
    );
    let claim = subject(
        by_agent(event(
            PROJECT,
            21,
            event_catalog::TASK_COMPLETED_CLAIM,
            json!({ "detail": "the agent says it is finished" }),
        )),
        "task",
        "task-2",
    );
    // A confirmation that has not passed human review is not a fact yet (§16).
    let queued = subject(
        by_agent(awaiting_review(event(
            PROJECT,
            22,
            event_catalog::TASK_COMPLETED_CONFIRMED,
            json!({ "title": "add the map compiler" }),
        ))),
        "task",
        "task-2",
    );
    record_all(&f, &[created, claim, queued]);

    let state = f.reducer.project_state(PROJECT).expect("fold");
    assert_eq!(state.applied_events, 2, "the pending event is not reducible");
    let task = state.tasks.iter().find(|t| t.id == "task-2").expect("task-2");
    assert_ne!(task.status, TaskStatus::Done, "a claim never completes work");
    assert!(task.unverified_claim);
    assert_eq!(state.open_tasks().len(), 1);

    // Once a human confirms, the claim resolves.
    let confirmed = subject(
        event(
            PROJECT,
            23,
            event_catalog::TASK_COMPLETED_CONFIRMED,
            json!({ "title": "add the map compiler" }),
        ),
        "task",
        "task-2",
    );
    record(&f.repos, &confirmed);
    let state = f.reducer.project_state(PROJECT).expect("refold");
    let task = state.tasks.iter().find(|t| t.id == "task-2").expect("task-2");
    assert_eq!(task.status, TaskStatus::Done);
    assert!(
        !task.unverified_claim,
        "confirmation clears the claim flag (§19)"
    );
}

#[test]
fn a_confirmation_without_a_prior_claim_still_completes() {
    let f = fixture();
    record_all(
        &f,
        &[
            subject(
                event(
                    PROJECT,
                    30,
                    event_catalog::TASK_CREATED,
                    json!({ "title": "user authored task" }),
                ),
                "task",
                "task-3",
            ),
            subject(
                event(
                    PROJECT,
                    31,
                    event_catalog::TASK_COMPLETED_CONFIRMED,
                    json!({ "title": "user authored task" }),
                ),
                "task",
                "task-3",
            ),
        ],
    );
    let state = f.reducer.project_state(PROJECT).expect("fold");
    let task = state.tasks.iter().find(|t| t.id == "task-3").expect("task-3");
    assert_eq!(task.status, TaskStatus::Done);
    assert!(!task.unverified_claim);
}

#[test]
fn superseding_a_decision_keeps_the_old_one_and_says_why() {
    let f = fixture();
    record_all(
        &f,
        &[
            subject(
                event(
                    PROJECT,
                    40,
                    event_catalog::DECISION_CONFIRMED,
                    json!({
                        "title": "store events in sqlite",
                        "decision": "sqlite",
                        "locked": true
                    }),
                ),
                "decision",
                "dec-1",
            ),
            event(
                PROJECT,
                41,
                event_catalog::DECISION_SUPERSEDED,
                json!({
                    "supersedes": "dec-1",
                    "decisionId": "dec-2",
                    "title": "store events in a file log",
                    "decision": "append-only files",
                    "reason": "portability across machines"
                }),
            ),
        ],
    );

    let state = f.reducer.project_state(PROJECT).expect("fold");
    assert_eq!(state.decisions.len(), 2, "history is never erased (§20)");
    let old = state.decisions.iter().find(|d| d.id == "dec-1").expect("dec-1");
    let new = state.decisions.iter().find(|d| d.id == "dec-2").expect("dec-2");
    assert_eq!(old.status, DecisionStatus::Superseded);
    assert_eq!(new.status, DecisionStatus::Active);
    assert_eq!(new.supersedes.as_deref(), Some("dec-1"));
    assert!(
        old.rationale
            .as_deref()
            .unwrap_or_default()
            .contains("portability"),
        "the reason for the change stays attached to the old decision"
    );
    assert_eq!(state.active_decisions().len(), 1);
    assert!(old.locked, "a user lock survives a supersede (§7.4)");

    // Reversal marks the survivor reversed; nothing is deleted.
    let reversed = subject(
        event(
            PROJECT,
            42,
            event_catalog::DECISION_REVERSED,
            json!({ "reason": "the file log was slower" }),
        ),
        "decision",
        "dec-2",
    );
    record(&f.repos, &reversed);
    let state = f.reducer.project_state(PROJECT).expect("refold");
    assert_eq!(state.decisions.len(), 2);
    assert_eq!(
        state.decisions.iter().find(|d| d.id == "dec-2").expect("dec-2").status,
        DecisionStatus::Reversed
    );
    assert!(state.active_decisions().is_empty());
}

#[test]
fn a_proposal_is_not_a_decision() {
    let f = fixture();
    record_all(
        &f,
        &[subject(
            by_agent(event(
                PROJECT,
                45,
                event_catalog::DECISION_PROPOSED,
                json!({ "title": "switch to files", "decision": "files" }),
            )),
            "decision",
            "dec-9",
        )],
    );
    let state = f.reducer.project_state(PROJECT).expect("fold");
    assert!(
        state.decisions.is_empty(),
        "禁止 6: an agent proposal never becomes an active decision"
    );
    assert_eq!(state.applied_events, 1, "yet the event is still in the log");
}

#[test]
fn evidence_answers_why_the_system_believes_it() {
    let f = fixture();
    record_all(
        &f,
        &[
            subject(
                event(
                    PROJECT,
                    50,
                    event_catalog::TASK_CREATED,
                    json!({ "title": "make the map" }),
                ),
                "task",
                "task-9",
            ),
            with_refs(
                subject(
                    by_agent(event(
                        PROJECT,
                        51,
                        event_catalog::TASK_COMPLETED_CLAIM,
                        json!({ "detail": "tests green" }),
                    )),
                    "task",
                    "task-9",
                ),
                &["msg-77", "file:src/map.rs"],
            ),
            // A bare test result: evidence about its subject, no state move of its own.
            with_refs(
                subject(
                    event(
                        PROJECT,
                        52,
                        event_catalog::TEST_PASSED,
                        json!({ "name": "cargo test -p vibe-state-engine" }),
                    ),
                    "task",
                    "task-9",
                ),
                &["build-1"],
            ),
        ],
    );

    let state = f.reducer.project_state(PROJECT).expect("fold");
    assert_eq!(state.evidence.len(), 3, "one row per distinct reference");
    assert_eq!(state.tasks[0].status, TaskStatus::Todo, "a test is not a confirm");

    let traced = f
        .repos
        .evidence
        .for_subject(EntityKind::Task, "task-9")
        .expect("evidence trace");
    let refs: Vec<&str> = traced.iter().filter_map(|e| e.ref_id.as_deref()).collect();
    assert!(refs.contains(&"msg-77"), "refs: {refs:?}");
    assert!(refs.contains(&"file:src/map.rs"), "refs: {refs:?}");
    assert!(refs.contains(&"build-1"), "refs: {refs:?}");

    // Replaying must not mint a second row for the same reference.
    let again = f.reducer.project_state(PROJECT).expect("refold");
    assert_eq!(again.evidence.len(), state.evidence.len());
    let stored = f.repos.evidence.list_evidence(PROJECT).expect("list");
    assert_eq!(
        stored
            .iter()
            .filter(|e| e.ref_id.as_deref() == Some("msg-77"))
            .count(),
        1
    );
}

#[test]
fn unknown_event_types_are_counted_not_fatal() {
    let f = fixture();
    let mut events = task_log();
    events.push(event(
        PROJECT,
        12,
        "SOMETHING_INVENTED_LATER",
        json!({ "title": "a type this build cannot know" }),
    ));
    record_all(&f, &events);

    let report = f.reducer.replay_report(PROJECT).expect("report");
    assert_eq!(report.applied_events, 3);
    assert_eq!(
        report.last_seq, 3,
        "seq is assigned by the store in append order, not taken from the caller"
    );
    assert!(report.unknown_types.contains(&"SOMETHING_INVENTED_LATER".to_string()));
    assert_eq!(report.state_hash, vibe_state_engine::state_hash_of(
        &f.reducer.project_state(PROJECT).expect("fold")
    ), "the report and the projection must agree");

    let state = f.reducer.project_state(PROJECT).expect("fold");
    assert_eq!(state.tasks.len(), 1, "the tolerated event changed nothing");
}

#[test]
fn drift_scope_conflicts_and_aeps_project() {
    let f = fixture();
    record_all(
        &f,
        &[
            event(
                PROJECT,
                60,
                event_catalog::DRIFT_DETECTED,
                json!({
                    "type": "architecture_drift",
                    "severity": "high",
                    "summary": "auth layer imports ui"
                }),
            ),
            event(
                PROJECT,
                61,
                event_catalog::SCOPE_CONFLICT_DETECTED,
                json!({ "summary": "chat export is outside the boundary" }),
            ),
            subject(
                event(
                    PROJECT,
                    62,
                    event_catalog::AEP_CREATED,
                    json!({
                        "code": "AEP-002",
                        "title": "multi-device sync",
                        "current": "single node",
                        "whyNotNow": "one user",
                        "trigger": ["a second device appears"],
                        "target": "replicated log"
                    }),
                ),
                "aep",
                "aep-1",
            ),
            subject(
                event(
                    PROJECT,
                    63,
                    event_catalog::AEP_TRIGGER_DETECTED,
                    json!({ "title": "multi-device sync" }),
                ),
                "aep",
                "aep-1",
            ),
        ],
    );

    let state = f.reducer.project_state(PROJECT).expect("fold");
    assert_eq!(state.drift_alerts.len(), 2, "scope conflict surfaces as drift");
    assert_eq!(state.open_drift().len(), 2);
    let aep = state.aeps.first().expect("aep");
    assert_eq!(aep.status, AepStatus::Triggered, "a detected trigger wakes the plan");
    assert_eq!(aep.code, "AEP-002");

    // Dismissing drift moves its status; it never deletes the alert.
    let id = state.drift_alerts[0].id.clone();
    record(
        &f.repos,
        &subject(
            event(
                PROJECT,
                64,
                event_catalog::DRIFT_DISMISSED,
                json!({ "reason": "expected for now" }),
            ),
            "drift",
            &id,
        ),
    );
    let state = f.reducer.project_state(PROJECT).expect("refold");
    assert_eq!(state.drift_alerts.len(), 2);
    assert_eq!(state.open_drift().len(), 1);
    assert_eq!(state.drift_alerts[0].status, DriftStatus::Dismissed);
}

#[test]
fn artifacts_from_git_and_file_events_land_in_the_cache() {
    let f = fixture();
    record_all(
        &f,
        &[
            subject(
                with_refs(
                    event(
                        PROJECT,
                        70,
                        event_catalog::FILE_CHANGED,
                        json!({ "path": "src/map.rs", "hash": "abc123" }),
                    ),
                    &["file:src/map.rs"],
                ),
                "artifact",
                "art-1",
            ),
            event(
                PROJECT,
                71,
                event_catalog::GIT_COMMIT,
                json!({
                    "sha": "deadbeef",
                    "commitMessage": "project map compiler",
                    "files": ["src/map.rs", "src/snapshot.rs"]
                }),
            ),
        ],
    );

    let state = f.reducer.project_state(PROJECT).expect("fold");
    let paths: Vec<&str> = state
        .artifacts
        .iter()
        .filter_map(|a| a.path.as_deref())
        .collect();
    assert!(paths.contains(&"src/map.rs"), "paths: {paths:?}");
    assert!(paths.contains(&"src/snapshot.rs"), "paths: {paths:?}");

    let cached = f.repos.artifacts.list_artifacts(PROJECT).expect("artifacts");
    assert_eq!(cached.len(), paths.len(), "the artifact cache mirrors the fold");
}

#[test]
fn charter_scope_and_baseline_are_read_only_and_outside_the_hash() {
    let f = fixture();
    record_all(&f, &task_log());
    let projects = ProjectService::new(f.repos.clone());
    projects
        .save_charter(
            PROJECT,
            CharterDraft {
                original_problem: "my notes are scattered".into(),
                core_value: "one place to think".into(),
                target_user: Some("me".into()),
                must_have: vec!["local first".into()],
                locked: true,
                ..Default::default()
            },
            ActorType::User,
        )
        .expect("charter");
    projects
        .update_scope(
            PROJECT,
            vec![
                (ScopeSection::InScope, "notes".into()),
                (ScopeSection::OutOfScope, "team features".into()),
            ],
            vec![],
            ActorType::User,
        )
        .expect("scope");

    let state = f.reducer.project_state(PROJECT).expect("fold");
    let charter = state.charter.as_ref().expect("charter");
    assert_eq!(charter.version, 1);
    assert!(charter.locked, "an agent-authored lock is ignored (§9)");
    assert_eq!(
        state.scope.as_ref().expect("scope").out_of_scope,
        vec!["team features".to_string()]
    );
    assert_eq!(state.project.as_ref().expect("project").name, "state engine");

    // The hash covers the event projection only, so the same log hashes identically
    // whether or not baselines exist.
    let log = f.reducer.accepted_events(PROJECT).expect("log");
    let folded = f.reducer.fold(PROJECT, &log).expect("pure");
    assert_eq!(folded.state_hash, state.state_hash);
    assert!(folded.charter.is_none(), "a pure fold has no baseline access");
    assert_eq!(
        vibe_state_engine::state_hash(&state),
        state.state_hash.as_deref().expect("hash")
    );
}

/// 确认即锁定 (§7.4) works because a proposal carries a machine-applicable `delta` — so the
/// delta has to survive the call that rewrites the row from the log. A creating event that
/// named only the change text made the projection rebuild the record without its reason and
/// without its delta, silently turning the user's acceptance into a status flip.
#[test]
fn a_proposal_survives_materialization_with_the_delta_it_was_opened_with() {
    let f = fixture();
    let projects = ProjectService::new(f.repos.clone());
    let delta = json!({
        "appliesTo": "scope",
        "add": [{ "section": "deferred", "item": "云端同步" }],
    });
    let proposal = projects
        .open_change_proposal(
            PROJECT,
            ChangeProposal {
                id: String::new(),
                project_id: PROJECT.to_string(),
                proposed_change: "把「云端同步」从禁区挪到暂缓区".into(),
                reason: "离线也要能接续工作".into(),
                delta: Some(delta.clone()),
                ..Default::default()
            },
            ActorType::Agent,
        )
        .expect("open proposal");

    f.reducer.project_state(PROJECT).expect("materialize");

    let stored = f
        .repos
        .entities
        .entity::<ChangeProposal>(EntityKind::ChangeProposal, &proposal.id)
        .expect("read the row")
        .expect("the row is still a record");
    assert_eq!(
        stored.reason, "离线也要能接续工作",
        "the log is the only input, so the log has to carry the reason"
    );
    assert_eq!(
        stored.delta,
        Some(delta),
        "accepting a proposal whose delta was lost would move nothing"
    );
    assert_eq!(stored.status, ChangeProposalStatus::Open, "a fold does not decide");
}
