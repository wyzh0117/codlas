//! Stage A + B acceptance: the store must create projects, append events,
//! replay them in a fixed order, and refuse to duplicate work on re-ingest
//! (V1_DEVELOPMENT.md §61 Stage A / Stage B).

use vibe_core::event_store::{EventDraft, EventStore};
use vibe_core::judge::RuleJudge;
use vibe_core::ports::{SearchFilters, Repositories};
use vibe_core::project_service::{CharterDraft, ProjectService};
use vibe_domain::{event_catalog, ActorType, EventStatus, Message, MessageRole};
use vibe_storage::SqliteStore;

struct Fixture {
    repos: Repositories,
    projects: ProjectService,
    events: EventStore,
}

fn fixture() -> Fixture {
    let store = SqliteStore::open_in_memory().expect("open");
    let repos = store.repositories();
    Fixture {
        projects: ProjectService::new(repos.clone()),
        events: EventStore::new(repos.clone()),
        repos,
    }
}

fn services() -> (ProjectService, EventStore) {
    let f = fixture();
    (f.projects, f.events)
}

#[test]
fn migrations_run_once_and_report_version() {
    let store = SqliteStore::open_in_memory().unwrap();
    assert_eq!(store.schema_version().unwrap(), vibe_storage::SCHEMA_VERSION);
    // Re-opening an already migrated database must not fail.
    store.migrate().unwrap();
    assert_eq!(store.schema_version().unwrap(), vibe_storage::SCHEMA_VERSION);
}

#[test]
fn create_project_writes_a_project_created_event() {
    let (projects, events) = services();
    let project = projects
        .create_project("Workbench", Some("personal".into()), None, None)
        .unwrap();
    assert!(!project.id.is_empty());
    let stored = projects.project(&project.id).unwrap();
    assert_eq!(stored.name, "Workbench");

    let list = events.accepted(&project.id, None).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].r#type, event_catalog::PROJECT_CREATED);
}

#[test]
fn appending_the_same_event_twice_is_not_a_second_fact() {
    let (projects, events) = services();
    let project = projects.create_project("Idempotency", None, None, None).unwrap();

    let draft = EventDraft::new(project.id.clone(), event_catalog::TASK_STARTED)
        .agent()
        .subject("task", "task-1")
        .payload(serde_json::json!({ "title": "implement reducer" }));

    let first = events.append(draft.clone()).unwrap();
    assert!(first.inserted);
    let second = events.append(draft).unwrap();
    assert!(!second.inserted, "re-ingest must not create a new event");
    assert_eq!(second.event.id, first.event.id);

    // PROJECT_CREATED plus the single task event; the retry added nothing.
    let all = events.list(&project.id, None, None).unwrap();
    assert_eq!(all.len(), 2);
}

#[test]
fn replay_is_ordered_by_seq_and_skips_non_accepted() {
    let (projects, events) = services();
    let project = projects.create_project("Replay", None, None, None).unwrap();

    for index in 0..5 {
        events
            .append(
                EventDraft::new(project.id.clone(), event_catalog::TASK_CREATED)
                    .user()
                    .subject("task", format!("task-{index}"))
                    .payload(serde_json::json!({ "index": index })),
            )
            .unwrap();
    }
    events
        .append(
            EventDraft::new(project.id.clone(), event_catalog::TASK_CREATED)
                .agent()
                .pending()
                .subject("task", "pending-one")
                .confidence(0.7),
        )
        .unwrap();

    let accepted = events.accepted(&project.id, None).unwrap();
    // PROJECT_CREATED + five tasks; the pending one is not reducible.
    assert_eq!(accepted.len(), 6);
    let seqs: Vec<i64> = accepted.iter().map(|e| e.seq.unwrap()).collect();
    let mut sorted = seqs.clone();
    sorted.sort();
    assert_eq!(seqs, sorted, "replay must follow store order");
    assert!(accepted.iter().all(|e| e.status.is_reducible()));

    let incremental = events.accepted(&project.id, Some(accepted[2].seq.unwrap())).unwrap();
    assert_eq!(incremental.len(), 3);
    assert_eq!(incremental[0].seq.unwrap(), accepted[3].seq.unwrap());
}

#[test]
fn review_moves_a_pending_event_to_accepted() {
    let (projects, events) = services();
    let project = projects.create_project("Review", None, None, None).unwrap();
    let outcome = events
        .append(
            EventDraft::new(project.id.clone(), event_catalog::DECISION_PROPOSED)
                .agent()
                .pending()
                .confidence(0.7)
                .payload(serde_json::json!({ "title": "keep repository layer" })),
        )
        .unwrap();
    assert_eq!(outcome.event.status, EventStatus::PendingReview);
    assert_eq!(events.accepted(&project.id, None).unwrap().len(), 1);

    events
        .review(&outcome.event.id, EventStatus::Accepted, Some(0.95))
        .unwrap();
    let stored = events.get(&outcome.event.id).unwrap().unwrap();
    assert_eq!(stored.status, EventStatus::Accepted);
    assert_eq!(stored.confidence, 0.95);
}

#[test]
fn charter_updates_keep_history_and_lock_blocks_agents() {
    let f = fixture();
    let projects = &f.projects;
    let project = projects.create_project("Charter", None, None, None).unwrap();

    let v1 = projects
        .save_charter(
            &project.id,
            CharterDraft {
                original_problem: "vibe coding loses the thread".into(),
                core_value: "project continuity".into(),
                must_have: vec!["local project record".into()],
                wont_have: vec!["team collaboration".into()],
                ..Default::default()
            },
            ActorType::User,
        )
        .unwrap();
    assert_eq!(v1.version, 1);

    let repos = &f.repos;
    let project2 = projects.create_project("Lock", None, None, None).unwrap();
    projects
        .save_charter(
            &project2.id,
            CharterDraft {
                original_problem: "p".into(),
                core_value: "c".into(),
                locked: true,
                ..Default::default()
            },
            ActorType::User,
        )
        .unwrap();
    let blocked = projects.save_charter(
        &project2.id,
        CharterDraft {
            original_problem: "rewritten by agent".into(),
            core_value: "c".into(),
            ..Default::default()
        },
        ActorType::Agent,
    );
    assert!(
        matches!(blocked, Err(vibe_core::error::CoreError::Locked(_))),
        "an agent must never overwrite a locked charter"
    );

    let versions = repos.projects.list_charter_versions(&project.id).unwrap();
    assert_eq!(versions.len(), 1);
    let v2 = projects
        .save_charter(
            &project.id,
            CharterDraft {
                original_problem: "vibe coding loses the thread".into(),
                core_value: "project continuity".into(),
                must_have: vec!["local project record".into(), "context pack".into()],
                wont_have: vec!["team collaboration".into()],
                ..Default::default()
            },
            ActorType::User,
        )
        .unwrap();
    assert_eq!(v2.version, 2);
    let versions = repos.projects.list_charter_versions(&project.id).unwrap();
    assert_eq!(versions.len(), 2, "old charter versions are retained");
    assert_eq!(versions[0].must_have.len(), 1);
    assert_eq!(versions[1].must_have.len(), 2);
}

#[test]
fn duplicate_messages_are_dropped_on_reingest() {
    let f = fixture();
    let repos = &f.repos;
    let projects = ProjectService::new(repos.clone());
    let project = projects.create_project("Capture", None, None, None).unwrap();

    let source = vibe_domain::Source {
        id: vibe_domain::new_id(),
        project_id: project.id.clone(),
        r#type: vibe_domain::SourceType::GenericCli,
        external_id: Some("session-9".into()),
        display_name: "codex session".into(),
        metadata: serde_json::json!({}),
    };
    repos.conversations.upsert_source(&source).unwrap();
    let conversation = vibe_domain::Conversation {
        id: vibe_domain::new_id(),
        project_id: project.id.clone(),
        source_id: source.id.clone(),
        external_conversation_id: Some("session-9".into()),
        title: Some("reconnect work".into()),
        started_at: None,
        last_seen_at: None,
    };
    repos.conversations.upsert_conversation(&conversation).unwrap();

    let observed = vibe_domain::now();
    let batch = vec![
        Message::new(
            conversation.id.clone(),
            MessageRole::User,
            "为什么同步还是重复？".into(),
            Some("m-1".into()),
            observed,
        ),
        Message::new(
            conversation.id.clone(),
            MessageRole::Assistant,
            "因为 cursor 没有持久化".into(),
            Some("m-2".into()),
            observed,
        ),
    ];
    let first = repos
        .conversations
        .insert_messages(&conversation.id, &batch)
        .unwrap();
    assert_eq!(first.inserted.len(), 2);
    assert_eq!(first.duplicate_count, 0);

    // Same external ids, same content, new local ids: still no duplicates.
    let replay = batch
        .iter()
        .map(|m| Message {
            id: vibe_domain::new_id(),
            ..m.clone()
        })
        .collect::<Vec<_>>();
    let second = repos
        .conversations
        .insert_messages(&conversation.id, &replay)
        .unwrap();
    assert!(second.inserted.is_empty());
    assert_eq!(second.duplicate_count, 2);
    assert_eq!(
        repos.conversations.list_messages(&conversation.id).unwrap().len(),
        2
    );
}

#[test]
fn full_text_search_finds_messages_and_events() {
    let f = fixture();
    let (projects, events) = (&f.projects, &f.events);
    let project = projects.create_project("Search", None, None, None).unwrap();
    events
        .append(
            EventDraft::new(project.id.clone(), event_catalog::ISSUE_FOUND)
                .user()
                .payload(serde_json::json!({ "excerpt": "reconnect loop duplicates messages" })),
        )
        .unwrap();

    let repos = &f.repos;
    repos
        .search
        .index(
            "message",
            "m-1",
            &project.id,
            "user",
            "the reconnect loop duplicates every message",
        )
        .unwrap();
    let hits = repos
        .search
        .search(
            "reconnect duplicates",
            &SearchFilters {
                project_id: Some(project.id.clone()),
                kinds: vec!["message".into()],
                limit: 10,
            },
        )
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0].snippet.contains("reconnect"));

    // Unquoted punctuation from user queries must not break the match syntax.
    let weird = repos
        .search
        .search(
            "reconnect \" : - ^",
            &SearchFilters {
                project_id: Some(project.id.clone()),
                kinds: Vec::new(),
                limit: 10,
            },
        )
        .unwrap();
    assert!(weird.is_empty() || weird.len() <= 1);
}

#[test]
fn judge_thresholds_are_tunable_and_persist() {
    use vibe_core::judge::JudgeThresholds;
    use vibe_core::settings::SettingsService;

    let f = fixture();
    let repos = &f.repos;
    let settings = SettingsService::new(repos.clone());
    assert_eq!(settings.judge_thresholds().unwrap().auto_accept, 0.90);
    settings
        .set_judge_thresholds(JudgeThresholds {
            auto_accept: 0.85,
            inbox_floor: 0.60,
        })
        .unwrap();
    assert_eq!(settings.judge_thresholds().unwrap().auto_accept, 0.85);
    assert!(settings
        .set_judge_thresholds(JudgeThresholds {
            auto_accept: 0.5,
            inbox_floor: 0.8,
        })
        .is_err());

    // The judge itself reads whatever the user last set.
    let judge = RuleJudge::new(settings.judge_thresholds().unwrap());
    assert_eq!(judge.thresholds().inbox_floor, 0.60);
}

#[test]
fn an_entity_can_be_written_again_without_becoming_a_second_row() {
    use vibe_core::ports::EntityStore;
    use vibe_domain::{EntityKind, Task, TaskStatus};

    let f = fixture();
    let project = f
        .projects
        .create_project("cache", None, None, None)
        .unwrap();
    let mut task = Task {
        id: "task-1".into(),
        project_id: project.id.clone(),
        title: "materialize the projection".into(),
        ..Default::default()
    };

    f.repos
        .entities
        .put_entity(EntityKind::Task, &task, Some("todo"))
        .expect("first insert");
    task.status = TaskStatus::Done;
    f.repos
        .entities
        .put_entity(EntityKind::Task, &task, Some("done"))
        .expect("update in place");

    let all: Vec<Task> = f
        .repos
        .entities
        .entities_of(EntityKind::Task, &project.id)
        .unwrap();
    assert_eq!(
        all.len(),
        1,
        "the cache holds one row per id, not one per write"
    );
    assert_eq!(all[0].status, TaskStatus::Done);
}

#[test]
fn two_entities_of_different_kinds_never_share_a_row() {
    use vibe_core::ports::EntityStore;
    use vibe_domain::{EntityKind, Issue, IssueStatus, Task, TaskStatus};

    let f = fixture();
    let project = f
        .projects
        .create_project("kinds", None, None, None)
        .unwrap();
    // External ids collide all the time: issue #1 and task #1 from the same
    // tracker are two facts, not one silently overwriting the other.
    f.repos
        .entities
        .put_entity(
            EntityKind::Task,
            &Task {
                id: "1".into(),
                project_id: project.id.clone(),
                title: "land stage C".into(),
                status: TaskStatus::Doing,
                ..Default::default()
            },
            Some("doing"),
        )
        .expect("task");
    f.repos
        .entities
        .put_entity(
            EntityKind::Issue,
            &Issue {
                id: "1".into(),
                project_id: project.id.clone(),
                title: "sync duplicates messages".into(),
                status: IssueStatus::Open,
                ..Default::default()
            },
            Some("open"),
        )
        .expect("issue");

    let tasks: Vec<Task> = f
        .repos
        .entities
        .entities_of(EntityKind::Task, &project.id)
        .unwrap();
    let issues: Vec<Issue> = f
        .repos
        .entities
        .entities_of(EntityKind::Issue, &project.id)
        .unwrap();
    assert_eq!(tasks.len(), 1, "the task survived");
    assert_eq!(issues.len(), 1, "and the issue did not replace it");
    assert_eq!(tasks[0].status, TaskStatus::Doing);
    assert_eq!(issues[0].status, IssueStatus::Open);
}

#[test]
fn a_confirmation_is_a_different_fact_than_the_candidate_it_answered() {
    let f = fixture();
    let project = f
        .projects
        .create_project("review", None, None, None)
        .unwrap();
    let payload = serde_json::json!({ "title": "ship the map compiler" });

    // Same subject, same words: the only difference is the user's judgement, and
    // that must be enough to survive the idempotency key (§27, §52).
    let candidate = EventDraft::new(&project.id, event_catalog::TASK_COMPLETED_CLAIM)
        .agent()
        .subject("task", "task-9")
        .payload(payload.clone())
        .pending();
    let confirmed = EventDraft::new(&project.id, event_catalog::TASK_COMPLETED_CLAIM)
        .user()
        .subject("task", "task-9")
        .payload(payload);

    assert!(f.events.append(candidate).unwrap().inserted);
    assert!(
        f.events.append(confirmed).unwrap().inserted,
        "an accepted fact must not be swallowed by the candidate it answers"
    );

    let accepted = f
        .events
        .accepted(&project.id, None)
        .unwrap()
        .into_iter()
        .filter(|event| event.r#type == event_catalog::TASK_COMPLETED_CLAIM)
        .collect::<Vec<_>>();
    assert_eq!(accepted.len(), 1, "only the confirmed statement reduces");
    assert_eq!(accepted[0].actor_type, ActorType::User);
}
