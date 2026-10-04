//! §40: three retrieval layers, one merged answer. The suite runs against the real
//! SQLite store, because what the layers promise is *index* behaviour — a fake that
//! returns what it is handed cannot fail in the ways an FTS index does.

use std::sync::Arc;

use vibe_core::event_store::{EventDraft, EventStore};
use vibe_core::ports::{EntityStore, Repositories, SearchHit};
use vibe_core::project_service::ProjectService;
use vibe_domain::{
    event_catalog, AepStatus, ArchitectureEvolutionPlan, Artifact, ArtifactKind, ChangeProposal,
    Decision, DecisionStatus, DriftAlert, DriftSeverity, DriftType, EntityKind, Issue, IssueStatus,
    Milestone, Task, TaskStatus,
};
use vibe_search::{
    catalog, Found, SearchLayer, SearchService, StructuredQuery, DEFAULT_LIMIT, KIND_DECISION,
    KIND_DRIFT, KIND_EVENT, KIND_ISSUE, KIND_TASK, SCORE_EXACT_ID, SCORE_FULL_TEXT,
    SCORE_RELATION,
};
use vibe_storage::SqliteStore;

struct Fixture {
    #[allow(dead_code)]
    store: Arc<SqliteStore>,
    repos: Repositories,
    search: SearchService,
    project_id: String,
    /// A second project in the same database: isolation is only worth testing when a
    /// leak is possible.
    other_id: String,
}

fn fixture() -> Fixture {
    let store = SqliteStore::open_in_memory().expect("open in-memory store");
    let repos = store.repositories();
    let projects = ProjectService::new(repos.clone());
    let project = projects
        .create_project("Search", None, None, None)
        .expect("project");
    let other = projects
        .create_project("Other", None, None, None)
        .expect("second project");
    Fixture {
        search: SearchService::new(repos.clone()),
        store,
        project_id: project.id.clone(),
        other_id: other.id.clone(),
        repos,
    }
}

fn task_in(f: &Fixture, project_id: &str, id: &str, title: &str, status: TaskStatus) {
    f.repos
        .entities
        .put_entity(
            EntityKind::Task,
            &Task {
                id: id.into(),
                project_id: project_id.into(),
                title: title.into(),
                status,
                ..Default::default()
            },
            Some(status.as_str()),
        )
        .expect("task");
}

fn task(f: &Fixture, id: &str, title: &str, status: TaskStatus) {
    task_in(f, &f.project_id, id, title, status);
}

fn issue(f: &Fixture, id: &str, title: &str) {
    f.repos
        .entities
        .put_entity(
            EntityKind::Issue,
            &Issue {
                id: id.into(),
                project_id: f.project_id.clone(),
                title: title.into(),
                status: IssueStatus::Open,
                ..Default::default()
            },
            Some("open"),
        )
        .expect("issue");
}

fn event(f: &Fixture, kind: &str, summary: &str) {
    EventStore::new(f.repos.clone())
        .append(
            EventDraft::new(f.project_id.clone(), kind)
                .agent()
                .payload(serde_json::json!({ "summary": summary })),
        )
        .expect("event");
}

/// One record of every kind retrieval claims to cover, each with a term no other
/// record carries.
fn one_of_each_kind(f: &Fixture) {
    task(f, "T-1", "compile the project map", TaskStatus::Doing);
    issue(f, "I-1", "retrieval misses renamed tasks");
    f.repos
        .entities
        .put_entity(
            EntityKind::Decision,
            &Decision {
                id: "D-1".into(),
                project_id: f.project_id.clone(),
                title: "retrieval stays lexical in V1".into(),
                decision: "no vector index until a corpus proves the need".into(),
                status: DecisionStatus::Active,
                locked: true,
                ..Default::default()
            },
            Some("active"),
        )
        .expect("decision");
    f.repos
        .entities
        .put_entity(
            EntityKind::Milestone,
            &Milestone {
                id: "M-1".into(),
                project_id: f.project_id.clone(),
                title: "stage D closes".into(),
                ..Default::default()
            },
            Some("pending"),
        )
        .expect("milestone");
    f.repos
        .entities
        .put_entity(
            EntityKind::ChangeProposal,
            &ChangeProposal {
                id: "CP-1".into(),
                project_id: f.project_id.clone(),
                proposed_change: "index titles in both scripts".into(),
                reason: "a user asks in the language they think in".into(),
                ..Default::default()
            },
            Some("open"),
        )
        .expect("proposal");
    f.repos
        .entities
        .put_entity(
            EntityKind::Aep,
            &ArchitectureEvolutionPlan {
                id: "AEP-001".into(),
                code: "AEP-001".into(),
                project_id: f.project_id.clone(),
                title: "multi-device sync".into(),
                current: "single node".into(),
                limitation: "no failover".into(),
                why_not_now: "one device today".into(),
                trigger: vec!["second device".into()],
                target: "two nodes".into(),
                status: AepStatus::Dormant,
                ..Default::default()
            },
            Some("dormant"),
        )
        .expect("aep");
    f.repos
        .entities
        .put_entity(
            EntityKind::DriftAlert,
            &DriftAlert {
                id: "DR-1".into(),
                project_id: f.project_id.clone(),
                r#type: DriftType::ArchitectureDrift,
                severity: DriftSeverity::Warning,
                summary: "an invariant named a forbidden dependency".into(),
                ..Default::default()
            },
            Some("open"),
        )
        .expect("drift");
    f.repos
        .artifacts
        .upsert_artifact(&Artifact {
            id: "A-1".into(),
            project_id: f.project_id.clone(),
            name: "service.rs".into(),
            kind: ArtifactKind::SourceFile,
            path: Some("crates/search/src/service.rs".into()),
            ..Default::default()
        })
        .expect("artifact");
    event(f, event_catalog::ISSUE_FOUND, "checked the third transcript today");
}

fn keys(items: &[Found]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|item| (item.hit.kind.clone(), item.hit.id.clone()))
        .collect()
}

fn hit_keys(items: &[SearchHit]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|item| (item.kind.clone(), item.id.clone()))
        .collect()
}

#[test]
fn full_text_layer_finds_what_a_write_already_indexed() {
    let f = fixture();
    event(&f, event_catalog::TASK_CREATED, "compile the project map today");

    let hits = f
        .search
        .text(&f.project_id, "compile", &[], 10)
        .expect("text search");
    assert_eq!(hits.len(), 1, "{:?}", hit_keys(&hits));
    assert_eq!(hits[0].kind, KIND_EVENT);

    // …and the pipeline reports it as an L2 answer, not as something cheaper.
    let found = f.search.find(&f.project_id, "compile", 10).expect("find");
    assert_eq!(found[0].layer, SearchLayer::FullText);
    assert!(found[0].score <= SCORE_FULL_TEXT);
}

#[test]
fn a_structured_id_match_outranks_any_text_hit() {
    let f = fixture();
    task(&f, "T-42", "compile the project map", TaskStatus::Todo);
    // The event names the id in its body, so L2 claims it too.
    event(&f, event_catalog::ISSUE_FOUND, "blocked by T-42 for now");

    let found = f.search.find(&f.project_id, "T-42", 10).expect("find");
    assert_eq!(
        keys(&found).first().map(|(_, id)| id.as_str()),
        Some("T-42"),
        "{found:?}"
    );
    assert_eq!(found[0].layer, SearchLayer::Structured);
    assert_eq!(found[0].score, SCORE_EXACT_ID);
    assert!(
        found.iter().skip(1).all(|item| item.score < SCORE_EXACT_ID),
        "the bands must not overlap: {:?}",
        keys(&found)
    );
}

#[test]
fn a_relation_widens_the_answer_without_outranking_it() {
    let f = fixture();
    task(&f, "T-42", "compile the project map", TaskStatus::Todo);
    issue(&f, "I-7", "the map drifts after a rename");
    f.repos
        .entities
        .add_link(EntityKind::Task, "T-42", EntityKind::Issue, "I-7", "blocks")
        .expect("link");

    let found = f.search.find(&f.project_id, "T-42", 10).expect("find");
    let relation = found
        .iter()
        .find(|item| item.hit.id == "I-7")
        .expect("the linked issue travels too");
    assert_eq!(relation.layer, SearchLayer::Relation);
    assert_eq!(relation.score, SCORE_RELATION);
    assert_eq!(found[0].hit.id, "T-42", "and never above the match itself");
}

#[test]
fn a_structured_query_returns_only_what_it_asks_for() {
    let f = fixture();
    task(&f, "T-1", "compile the project map", TaskStatus::Todo);
    task(&f, "T-2", "compile the context pack", TaskStatus::Done);

    let open = f
        .search
        .structured(
            &f.project_id,
            &StructuredQuery::by_status(&f.project_id, KIND_TASK, "todo"),
        )
        .expect("structured");
    assert_eq!(hit_keys(&open), vec![(KIND_TASK.into(), "T-1".into())]);

    let by_title = f
        .search
        .structured(
            &f.project_id,
            &StructuredQuery::by_title(&f.project_id, "context pack"),
        )
        .expect("title query");
    assert_eq!(hit_keys(&by_title), vec![(KIND_TASK.into(), "T-2".into())]);

    let one = f
        .search
        .structured(&f.project_id, &StructuredQuery::by_id(&f.project_id, "T-2"))
        .expect("id query");
    assert_eq!(hit_keys(&one), vec![(KIND_TASK.into(), "T-2".into())]);
}

#[test]
fn a_query_never_leaves_its_own_project() {
    let f = fixture();
    task(&f, "T-1", "compile the project map", TaskStatus::Todo);
    task_in(&f, &f.other_id, "T-9", "compile the project map", TaskStatus::Todo);

    let found = f.search.find(&f.project_id, "compile", 10).expect("find");
    assert_eq!(
        keys(&found),
        vec![(KIND_TASK.into(), "T-1".into())],
        "the other project's task leaked"
    );
    assert!(found
        .iter()
        .all(|item| item.hit.project_id == f.project_id));

    // Same through the index layer alone, which is where a missing filter would bite.
    let hits = f
        .search
        .text(&f.project_id, "compile", &[KIND_TASK], 10)
        .expect("text");
    assert_eq!(hit_keys(&hits), vec![(KIND_TASK.into(), "T-1".into())]);
}

#[test]
fn reindex_covers_every_kind_and_is_idempotent() {
    let f = fixture();
    one_of_each_kind(&f);

    let first = f.search.reindex(&f.project_id).expect("reindex");
    let again = f.search.reindex(&f.project_id).expect("second reindex");
    assert_eq!(first, again, "indexing twice is not indexing twice over");
    assert!(first >= 9, "one record per kind: {first}");

    // A kind the index omits stops being findable, so each term is checked at the kind
    // that carries it.
    for (query, kind) in [
        ("renamed", KIND_ISSUE),
        ("lexical", KIND_DECISION),
        ("invariant", KIND_DRIFT),
    ] {
        let hits = f
            .search
            .text(&f.project_id, query, &[], 10)
            .expect("text");
        assert_eq!(
            hits.iter().map(|hit| hit.kind.clone()).collect::<Vec<_>>(),
            vec![kind.to_string()],
            "{query} should be answered by {kind}"
        );
    }
}

#[test]
fn an_empty_query_answers_nothing_rather_than_everything() {
    let f = fixture();
    one_of_each_kind(&f);
    assert!(f
        .search
        .find(&f.project_id, "   ", 10)
        .expect("find")
        .is_empty());
    assert!(f
        .search
        .text(&f.project_id, "", &[], 10)
        .expect("text")
        .is_empty());
}

#[test]
fn full_text_matches_a_whole_token_not_a_piece_of_a_chinese_run() {
    // unicode61 does not segment Chinese: a run of CJK characters is one token. Tested
    // so the limit is a recorded fact rather than a surprise in use; a trigram tokenizer
    // is the way out if Chinese retrieval ships.
    let f = fixture();
    event(&f, event_catalog::DECISION_CONFIRMED, "上下文引擎保持词法实现");

    let whole = f
        .search
        .text(&f.project_id, "上下文引擎保持词法实现", &[], 10)
        .expect("whole run");
    assert_eq!(whole.len(), 1, "the exact run matches");

    let piece = f
        .search
        .text(&f.project_id, "上下文", &[], 10)
        .expect("substring");
    assert!(piece.is_empty(), "a fragment of the run does not");

    // Browsing by kind is unaffected: L1 reads records, not the index.
    let browsed = f
        .search
        .structured(
            &f.project_id,
            &StructuredQuery {
                project_id: f.project_id.clone(),
                kind: Some(KIND_EVENT.into()),
                ..Default::default()
            },
        )
        .expect("browse");
    // L1 is token-blind: a kind filter reads records, not the index, so both of the
    // project's events browse back — its own creation event included.
    assert_eq!(browsed.len(), 2, "{:?}", hit_keys(&browsed));
    assert!(browsed.iter().all(|hit| hit.kind == KIND_EVENT));
}

#[test]
fn a_page_size_of_zero_still_returns_a_page() {
    let f = fixture();
    for index in 0..30 {
        task(
            &f,
            &format!("T-{index}"),
            "compile the project map",
            TaskStatus::Todo,
        );
    }
    let found = f.search.find(&f.project_id, "compile", 0).expect("find");
    assert_eq!(found.len(), DEFAULT_LIMIT);

    let filters = catalog::filters(&f.project_id, &[], 0);
    assert_eq!(filters.limit, DEFAULT_LIMIT, "the port is never asked for zero rows");
}

/// §40 answers with what a record *says*. An entity arrives at the index as JSON, so
/// serialising the payload instead of lifting its text would make a field name
/// searchable, put every 64-hex id into the matchable body, and print braces where a
/// snippet should be.
#[test]
fn a_hit_reads_as_prose_and_its_encoding_matches_nothing() {
    let f = fixture();
    f.repos
        .entities
        .put_entity(
            EntityKind::Task,
            &Task {
                id: "T-9".into(),
                project_id: f.project_id.clone(),
                title: "harden the replay".into(),
                detail: Some("fold only the accepted events, twice over".into()),
                status: TaskStatus::Todo,
                evidence_refs: vec!["a".repeat(64)],
                created_by_event: Some("b".repeat(64)),
                ..Default::default()
            },
            Some("todo"),
        )
        .expect("task");

    let hits = f
        .search
        .text(&f.project_id, "twice over", &[], 10)
        .expect("text search");
    assert_eq!(hit_keys(&hits), vec![("task".to_string(), "T-9".to_string())]);
    let snippet = &hits[0].snippet;
    // FTS marks the matched text with `[ … ]`, so the markers come off before the
    // prose is compared.
    let plain = snippet.replace('[', "").replace(']', "");
    assert!(
        plain.contains("twice over") && snippet.contains("[over]"),
        "the detail text should read back as prose, with the match marked: {snippet}"
    );
    assert!(
        !snippet.contains('{') && !snippet.contains("evidenceRefs"),
        "a snippet is not a wire format: {snippet}"
    );

    for not_content in ["evidenceRefs", "createdByEvent", &"a".repeat(64)] {
        assert!(
            f.search
                .text(&f.project_id, not_content, &[], 10)
                .expect("search")
                .is_empty(),
            "'{not_content}' is structure, and L1 owns ids"
        );
    }
}
