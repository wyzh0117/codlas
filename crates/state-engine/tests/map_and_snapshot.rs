//! Stage E acceptance for the compiled view: the map is a deterministic projection
//! of state (V1_DEVELOPMENT.md §30, §31), and a snapshot plus the diff since it is
//! what a user reads when returning to a project (§32, §33; PROJECT_MASTER §10).

use vibe_core::event_store::{EventDraft, EventStore};
use vibe_core::ports::Repositories;
use vibe_core::project_service::{BaselineDraft, CharterDraft, ProjectService};
use vibe_domain::{
    event_catalog, ActorType, ArchitectureInvariant, ProjectMap, Snapshot,
};
use vibe_state_engine::{ProjectMapCompiler, Reducer, SnapshotService, MAP_SECTIONS};
use vibe_storage::SqliteStore;

struct Bench {
    repos: Repositories,
    projects: ProjectService,
    events: EventStore,
    reducer: Reducer,
    maps: ProjectMapCompiler,
    snapshots: SnapshotService,
    project_id: String,
}

fn bench(name: &str) -> Bench {
    let store = SqliteStore::open_in_memory().expect("open store");
    let repos = store.repositories();
    let projects = ProjectService::new(repos.clone());
    let project = projects.create_project(name, None, None, None).expect("project");
    Bench {
        reducer: Reducer::new(repos.clone()),
        maps: ProjectMapCompiler::new(repos.clone()),
        snapshots: SnapshotService::new(repos.clone()),
        events: EventStore::new(repos.clone()),
        project_id: project.id,
        projects,
        repos,
    }
}

fn task(b: &Bench, id: &str, title: &str) {
    b.events
        .append(
            EventDraft::new(&b.project_id, event_catalog::TASK_CREATED)
                .user()
                .subject("task", id)
                .payload(serde_json::json!({ "title": title })),
        )
        .expect("task created");
}

fn issue(b: &Bench, id: &str, title: &str) {
    b.events
        .append(
            EventDraft::new(&b.project_id, event_catalog::ISSUE_FOUND)
                .agent()
                .subject("issue", id)
                .payload(serde_json::json!({ "title": title })),
        )
        .expect("issue found");
}

fn resolve(b: &Bench, id: &str) {
    b.events
        .append(
            EventDraft::new(&b.project_id, event_catalog::ISSUE_RESOLVED)
                .user()
                .subject("issue", id)
                .payload(serde_json::json!({ "title": id })),
        )
        .expect("issue resolved");
}

fn start(b: &Bench, id: &str, title: &str) {
    b.events
        .append(
            EventDraft::new(&b.project_id, event_catalog::TASK_STARTED)
                .agent()
                .subject("task", id)
                .payload(serde_json::json!({ "title": title })),
        )
        .expect("task started");
}

fn decision(b: &Bench, id: &str, text: &str) {
    b.events
        .append(
            EventDraft::new(&b.project_id, event_catalog::DECISION_CONFIRMED)
                .user()
                .subject("decision", id)
                .payload(serde_json::json!({ "title": id, "decision": text })),
        )
        .expect("decision confirmed");
}

/// The 13 sections of §31, in the documented order, and nothing else at `##` level.
#[test]
fn the_map_carries_every_required_section_in_order() {
    let b = bench("ordered map");
    task(&b, "task-1", "wire the reducer");
    issue(&b, "issue-1", "replay duplicates rows");
    decision(&b, "D-001", "keep the repository port");

    let map = b.maps.compile(&b.project_id).expect("compile");
    let positions: Vec<usize> = MAP_SECTIONS
        .iter()
        .map(|heading| {
            map.markdown
                .find(&format!("## {heading}"))
                .unwrap_or_else(|| panic!("missing section {heading}:\n{}", map.markdown))
        })
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "sections must appear in §31 order: {positions:?}"
    );
    assert_eq!(MAP_SECTIONS.len(), 13);
    assert!(map.markdown.starts_with("# Project Map"));
}

#[test]
fn an_empty_project_still_compiles_and_says_so() {
    let b = bench("empty");
    let map = b.maps.compile(&b.project_id).expect("compile empty");
    for heading in MAP_SECTIONS {
        assert!(
            map.markdown.contains(&format!("## {heading}")),
            "{heading} must not be dropped"
        );
    }
    // What is missing is declared, not silently blank — the map has to stay honest.
    assert!(map.markdown.contains("_none recorded_"));
    assert!(
        map.markdown
            .contains("Record the project charter so intent stops living in chat"),
        "a project without a charter must be told what to do next:\n{}",
        map.markdown
    );
}

#[test]
fn compiling_twice_over_the_same_log_yields_the_same_bytes() {
    let b = bench("deterministic map");
    task(&b, "task-1", "one");
    task(&b, "task-2", "two");
    issue(&b, "issue-1", "boom");

    let first = b.maps.compile(&b.project_id).expect("first");
    let second = b.maps.compile(&b.project_id).expect("second");
    assert_eq!(
        first.markdown, second.markdown,
        "the map is compiled, so it may not vary between runs (§30)"
    );
    assert_eq!(
        first.source_state_hash, second.source_state_hash,
        "and it must record the state it was compiled from"
    );
}

#[test]
fn the_map_shows_the_baselines_the_user_authored() {
    let b = bench("baselines in map");
    b.projects
        .save_charter(
            &b.project_id,
            CharterDraft {
                original_problem: "projects lose their design history".into(),
                core_value: "keep project continuity".into(),
                must_have: vec!["event store".into()],
                wont_have: vec!["team collaboration".into()],
                locked: true,
                ..Default::default()
            },
            ActorType::User,
        )
        .expect("charter");
    b.projects
        .set_baseline(
            &b.project_id,
            BaselineDraft {
                summary: "layered core over ports".into(),
                invariants: vec![ArchitectureInvariant {
                    id: "INV-001".into(),
                    text: "core never imports sqlite".into(),
                    detect: vec!["rusqlite".into()],
                }],
                ..Default::default()
            },
            ActorType::User,
        )
        .expect("baseline");

    let map = b.maps.compile(&b.project_id).expect("compile");
    assert!(map.markdown.contains("projects lose their design history"));
    assert!(map.markdown.contains("team collaboration"));
    assert!(map.markdown.contains("core never imports sqlite"));
}

#[test]
fn a_saved_map_is_the_one_the_workbench_reads_back() {
    let b = bench("stored map");
    task(&b, "task-1", "persist the view");
    let saved: ProjectMap = b.maps.save(&b.project_id).expect("save");

    let latest = b
        .repos
        .snapshots
        .latest_project_map(&b.project_id)
        .expect("latest")
        .expect("a stored map must exist");
    assert_eq!(latest.markdown, saved.markdown);
    assert_eq!(latest.source_state_hash, saved.source_state_hash);
}

#[test]
fn a_snapshot_freezes_state_and_the_map_that_went_with_it() {
    let b = bench("snapshot");
    task(&b, "task-1", "take the checkpoint");
    decision(&b, "D-001", "compile the map, never author it");

    let snap: Snapshot = b.snapshots.take(&b.project_id, "before stage E").expect("take");
    assert_eq!(snap.label, "before stage E");
    assert!(snap.project_map_markdown.is_some(), "a snapshot keeps its map");
    let hash_at_snapshot = snap
        .state
        .state_hash
        .clone()
        .expect("snapshot state is hashed");

    let latest = b
        .repos
        .snapshots
        .latest(&b.project_id)
        .expect("latest")
        .expect("one snapshot");
    assert_eq!(latest.id, snap.id);

    task(&b, "task-2", "prove the diff moves");
    let after = b.reducer.project_state(&b.project_id).expect("refold");
    assert_ne!(
        after.state_hash.as_deref().unwrap(),
        hash_at_snapshot,
        "new facts must change the state fingerprint"
    );
    assert_eq!(snap.state.tasks.len(), 1);
    assert_eq!(after.tasks.len(), 2);
}

#[test]
fn the_diff_reports_additions_and_transitions_since_the_snapshot() {
    let b = bench("diff");
    task(&b, "task-1", "seed the workbench");
    issue(&b, "issue-1", "the reducer double-counts");
    let snap = b.snapshots.take(&b.project_id, "checkpoint").expect("take");

    issue(&b, "issue-2", "search misses cjk");
    resolve(&b, "issue-1");
    start(&b, "task-1", "seed the workbench");

    let diff = b
        .snapshots
        .diff(&b.project_id, Some(&snap.id))
        .expect("diff");
    assert_eq!(diff.since_snapshot_id.as_deref(), Some(snap.id.as_str()));
    assert_eq!(diff.issues.added.len(), 1, "issue-2 is new");
    assert_eq!(diff.issues.changed.len(), 1, "issue-1 moved state");
    assert_eq!(diff.issues.changed[0].to.as_deref(), Some("resolved"));
    assert!(
        diff.tasks
            .changed
            .iter()
            .any(|entry| entry.label.contains("seed the workbench")
                && entry.from.as_deref() == Some("todo")
                && entry.to.as_deref() == Some("doing")),
        "a status move on an existing task is a transition: {:?}",
        diff.tasks
    );
    assert_ne!(diff.current_state_hash, snap.state.state_hash.unwrap());

    let rendered = diff.markdown();
    assert!(rendered.starts_with("# Since Last Snapshot"));
    for section in ["## Added", "## Changed", "## Next"] {
        assert!(rendered.contains(section), "missing {section} in {rendered}");
    }
}

#[test]
fn with_no_snapshot_the_diff_measures_against_nothing() {
    let b = bench("diff without baseline");
    task(&b, "task-1", "first thing");
    let diff = b.snapshots.since_last_snapshot(&b.project_id).expect("diff");
    assert!(diff.since_snapshot_id.is_none());
    assert_eq!(diff.tasks.added.len(), 1);
    assert!(diff
        .markdown()
        .contains("No snapshot baseline: everything below is measured against an empty state."));
}

#[test]
fn decisions_that_were_superseded_stay_visible_in_the_map() {
    let b = bench("superseded decision");
    decision(&b, "D-001", "keep the repository port");
    b.events
        .append(
            EventDraft::new(&b.project_id, event_catalog::DECISION_CONFIRMED)
                .user()
                .subject("decision", "D-002")
                .payload(serde_json::json!({
                    "title": "D-002",
                    "decision": "keep the port and add a second adapter",
                    "supersedes": "D-001",
                })),
        )
        .expect("superseding decision");

    let map = b.maps.compile(&b.project_id).expect("compile");
    let state = b.reducer.project_state(&b.project_id).expect("state");
    assert_eq!(state.active_decisions().len(), 1);
    assert_eq!(state.decisions.len(), 2, "history is never deleted");
    let section = map
        .markdown
        .split("## Active Decisions")
        .nth(1)
        .expect("section")
        .split("## ")
        .next()
        .expect("body");
    assert!(section.contains("D-002"), "{section}");
    assert!(
        !section.contains("keep the repository port\n"),
        "a superseded decision must not sit in the active section"
    );
}
