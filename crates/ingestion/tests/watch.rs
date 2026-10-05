//! §39's watch signals: which branch HEAD is on, which files are uncommitted, and which
//! recorded file has vanished.
//!
//! V1 has no daemon, so a watch pass is a diff against the snapshot the previous pass
//! stored. The git side runs on a canned [`CommandRunner`] — no repository on disk —
//! which is the layer's whole contract: git-shaped data in, events out. The file side
//! reads a real temporary directory, because a deletion is only visible as the diff
//! between two walks of one.
//!
//! The assertions that matter are the negative ones. A watch must write nothing when
//! nothing moved (§51 idempotent capture); it must create no artifact and change no task
//! (§39: a dirty tree is evidence about where the work is, §19 禁止 9: it is not a
//! completion); and a directory that holds no repository has to be reported as such rather
//! than turned into a guess.

use std::path::Path;
use std::sync::{Arc, Mutex};

use vibe_core::error::{CoreError, Result};
use vibe_core::ports::Repositories;
use vibe_core::project_service::ProjectService;
use vibe_domain::{event_catalog, ActorType, ProjectEvent, SourceType};
use vibe_ingestion::{
    CommandOutcome, CommandRunner, FileScan, GitFacts, ScanKind, WorkspaceSnapshot,
};
use vibe_state_engine::Reducer;
use vibe_storage::SqliteStore;

fn repos() -> (Arc<SqliteStore>, Repositories, String) {
    let store = SqliteStore::open_in_memory().expect("in-memory store");
    let repositories = store.repositories();
    let project = ProjectService::new(repositories.clone())
        .create_project("Watched", None, None, None)
        .expect("project");
    (store, repositories, project.id)
}

/// The checkout a test pretends to read.
#[derive(Default)]
struct State {
    /// `None` means "this directory is not a git repository": git exits non-zero and says
    /// so, which is the shape the capture layer has to handle.
    branch: Option<String>,
    head: Option<String>,
    status: Vec<String>,
}

/// A working copy plus every command the layer asked for, so a test can prove the pass
/// stayed read-only and asked only for what §39 needs.
#[derive(Default)]
struct FakeWorkspace {
    state: Mutex<State>,
    asked: Mutex<Vec<String>>,
}

impl FakeWorkspace {
    fn at(branch: &str, head: &str, status: &[&str]) -> Arc<Self> {
        let fake = Arc::new(FakeWorkspace::default());
        fake.edit(branch, head, status);
        fake
    }

    /// Move the pretend checkout: a new branch, a new tip, a new dirty set.
    fn edit(&self, branch: &str, head: &str, status: &[&str]) {
        let mut state = self.state.lock().expect("state lock");
        state.branch = Some(branch.to_string());
        state.head = Some(head.to_string());
        state.status = status.iter().map(|s| s.to_string()).collect();
    }

    /// A plain folder: git itself refuses, exactly as it would on a real machine.
    fn no_repository(&self) {
        *self.state.lock().expect("state lock") = State::default();
    }

    fn commands(&self) -> Vec<String> {
        self.asked.lock().expect("asked lock").clone()
    }
}

impl CommandRunner for FakeWorkspace {
    fn run(&self, program: &str, args: &[&str], _workdir: &Path) -> Result<CommandOutcome> {
        let line = format!("{program} {}", args.join(" "));
        self.asked.lock().expect("asked lock").push(line.clone());

        // The only commands a watch pass may want. Anything else is a bug in this layer,
        // so it fails loudly rather than returning canned text that hides it.
        let output = match args.first().copied() {
            Some("rev-parse") => {
                if args.contains(&"--abbrev-ref") {
                    "branch"
                } else {
                    "head"
                }
            }
            Some("status") => "status",
            other => {
                return Err(CoreError::Validation(format!(
                    "a watch pass should not ask for {line:?} (first argument {other:?})"
                )))
            }
        };

        let state = self.state.lock().expect("state lock");
        let (code, stdout): (i32, String) = match (output, state.branch.as_deref()) {
            (_, None) => (128, String::new()),
            ("branch", Some(branch)) => (0, branch.to_string()),
            ("head", Some(_)) => (0, state.head.clone().unwrap_or_default()),
            ("status", Some(_)) => (0, state.status.join("\n")),
            _ => (128, String::new()),
        };
        let stderr = if code == 0 {
            String::new()
        } else {
            "fatal: not a git repository (or any of the parent directories): .git".to_string()
        };
        Ok(CommandOutcome::new(code, stdout, stderr))
    }
}

fn events_of(repositories: &Repositories, project_id: &str, ty: &str) -> Vec<ProjectEvent> {
    repositories
        .events
        .list_events(project_id, None, None)
        .expect("events")
        .into_iter()
        .filter(|e| e.r#type == ty)
        .collect()
}

/// The directory a pass reads. The fake runner ignores it, and the snapshot key is derived
/// from it, so a stable path keeps the assertions readable.
fn checkout() -> std::path::PathBuf {
    Path::new("/workspaces/codlas").to_path_buf()
}

/// §39: the first pass has nothing to compare against, so it records where the checkout
/// stands and invents no change.
#[test]
fn a_first_pass_leaves_a_baseline_and_claims_no_change() {
    let (_store, repositories, project) = repos();
    let fake = FakeWorkspace::at("main", "aaa1111", &["M src/auth.rs", "?? notes.md"]);
    let facts = GitFacts::new(repositories.clone(), fake.clone());

    let report = facts
        .watch_workspace(&project, &checkout())
        .expect("first pass");

    assert!(report.is_repository);
    assert!(!report.compared, "there was no earlier snapshot");
    assert_eq!(report.branch, "main");
    assert_eq!(report.head.as_deref(), Some("aaa1111"));
    assert_eq!(
        report.dirty,
        // Sorted, deliberately: the snapshot is compared by equality, so the order a
        // tool happened to print in cannot be allowed to look like a change.
        vec!["?? notes.md".to_string(), "M src/auth.rs".to_string()]
    );
    assert_eq!(report.events_appended, 0, "a baseline is not a change");
    assert_eq!(report.events_duplicate, 0);

    // ...and the snapshot it left behind is what the next pass will diff against.
    let stored = repositories
        .settings
        .get_json(&GitFacts::snapshot_key(&project, "/workspaces/codlas"))
        .expect("settings read")
        .expect("a snapshot was stored");
    let snapshot: WorkspaceSnapshot = serde_json::from_value(stored).expect("snapshot json");
    assert_eq!(snapshot.branch, "main");
    assert_eq!(snapshot.status, report.dirty);
}

/// §39 + §51: watching twice with nothing touched writes no event a second time.
#[test]
fn watching_twice_without_touching_anything_writes_nothing() {
    let (_store, repositories, project) = repos();
    let fake = FakeWorkspace::at("main", "aaa1111", &["M src/auth.rs"]);
    let facts = GitFacts::new(repositories.clone(), fake.clone());

    let first = facts.watch_workspace(&project, &checkout()).expect("first");
    let second = facts
        .watch_workspace(&project, &checkout())
        .expect("second");

    assert!(second.compared, "the first pass left a snapshot");
    assert!(!second.branch_changed);
    assert!(!second.status_changed);
    assert_eq!(second.events_appended, 0);
    assert_eq!(second.previous_branch.as_deref(), Some("main"));
    assert!(first.events_appended + second.events_appended == 0);
    assert!(
        events_of(&repositories, &project, event_catalog::GIT_BRANCH_CHANGED).is_empty(),
        "nothing moved, so the log stays empty"
    );
}

/// A branch switch and a new dirty set are two different facts, so they get two events —
/// and neither of them is a commit.
#[test]
fn a_branch_switch_and_a_new_dirty_set_each_leave_their_own_signal() {
    let (_store, repositories, project) = repos();
    let fake = FakeWorkspace::at("main", "aaa1111", &["M src/auth.rs"]);
    let facts = GitFacts::new(repositories.clone(), fake.clone());
    facts
        .watch_workspace(&project, &checkout())
        .expect("baseline");

    fake.edit("feat/reconnect", "bbb2222", &["M src/auth.rs", "M src/cache.rs"]);
    let report = facts
        .watch_workspace(&project, &checkout())
        .expect("second pass");

    assert!(report.branch_changed);
    assert!(report.status_changed);
    assert_eq!(report.previous_branch.as_deref(), Some("main"));
    assert_eq!(report.events_appended, 2, "one signal per kind of change");

    let branch = events_of(&repositories, &project, event_catalog::GIT_BRANCH_CHANGED);
    assert_eq!(branch.len(), 1);
    assert_eq!(branch[0].payload["from"], serde_json::json!("main"));
    assert_eq!(branch[0].payload["to"], serde_json::json!("feat/reconnect"));
    assert_eq!(branch[0].payload["head"], serde_json::json!("bbb2222"));

    let status = events_of(&repositories, &project, event_catalog::GIT_STATUS_CHANGED);
    assert_eq!(status.len(), 1);
    assert_eq!(
        status[0].payload["to"],
        serde_json::json!(["M src/auth.rs", "M src/cache.rs"])
    );
    assert_eq!(status[0].payload["clean"], serde_json::json!(false));

    // Both belong to the repository's own source, so "what did this checkout tell us?" is
    // one query (§17 traceability).
    assert_eq!(branch[0].source_id, Some(report.source_id.clone()));
    assert_eq!(status[0].source_id, Some(report.source_id.clone()));
    let sources = repositories.conversations.list_sources(&project).expect("sources");
    assert_eq!(sources.len(), 1, "watching does not mint a second source");
    assert_eq!(sources[0].r#type, SourceType::Git);
}

/// §39's signals are evidence. A fold of the log must come back with the same project it
/// started with — no artifact row, no completed task, no drift verdict (§18 rebuild purity,
/// §19 禁止 9).
#[test]
fn watch_signals_fold_into_no_project_state_at_all() {
    let (_store, repositories, project) = repos();
    let fake = FakeWorkspace::at("main", "aaa1111", &["M src/auth.rs"]);
    let facts = GitFacts::new(repositories.clone(), fake.clone());
    facts.watch_workspace(&project, &checkout()).expect("baseline");

    let before = Reducer::new(repositories.clone())
        .project_state(&project)
        .expect("projection");

    fake.edit("release/v1", "ccc3333", &[]);
    let report = facts
        .watch_workspace(&project, &checkout())
        .expect("release cut");
    assert_eq!(report.events_appended, 2);

    let after = Reducer::new(repositories.clone())
        .project_state(&project)
        .expect("reprojection");

    assert!(after.tasks.is_empty(), "a watch signal completes nothing");
    assert!(after.artifacts.is_empty(), "a watch writes no artifact row");
    assert!(after.decisions.is_empty());
    assert!(after.evidence.is_empty(), "a watch mints no evidence row either");
    assert_eq!(after.milestones.len(), before.milestones.len());
    assert_eq!(after.scope.is_some(), before.scope.is_some());

    // The events are still in the log — evidence kept, conclusions not drawn.
    assert_eq!(
        repositories
            .events
            .list_events(&project, None, None)
            .expect("events")
            .len(),
        3,
        "the project's own creation event plus the two signals"
    );
}

/// §39's third file signal, proven against a directory that really loses a file: the
/// removal is one signal, the artifact row is marked rather than dropped, and the pass
/// draws no conclusion (§19 禁止 9 — a vanished file completes nothing and decides nothing).
#[test]
fn a_vanished_file_leaves_a_removal_signal_and_no_new_verdict() {
    let (_store, repositories, project) = repos();
    let tree = tempfile::tempdir().expect("a directory to scan");
    std::fs::create_dir_all(tree.path().join("src")).expect("src");
    let gone = tree.path().join("src/auth.rs");
    std::fs::write(&gone, "pub fn login() {}\n").expect("file");

    let scan = FileScan::new(repositories.clone(), tree.path()).with_source_key("watched-tree");
    let baseline = scan.run(&project).expect("first pass");
    assert_eq!(baseline.by_kind(ScanKind::Created).len(), 1);
    let recorded = baseline.by_kind(ScanKind::Created)[0]
        .hash
        .clone()
        .expect("a first pass leaves a content baseline");

    let before = Reducer::new(repositories.clone())
        .project_state(&project)
        .expect("projection");
    assert_eq!(before.artifacts.len(), 1, "the fold knows the file");
    assert!(!before.artifacts[0].deleted);

    std::fs::remove_file(&gone).expect("the file is gone");
    let report = scan.run(&project).expect("the pass that finds it gone");

    let deleted = report.by_kind(ScanKind::Deleted);
    assert_eq!(deleted.len(), 1, "{report:?}");
    assert_eq!(deleted[0].path, "src/auth.rs", "relative, forward-slashed (§22)");
    assert_eq!(
        deleted[0].hash, None,
        "a removal carries no new content hash, only the loss of the old one"
    );
    assert_eq!(report.events_appended, 1);
    assert_eq!(report.scanned, 0);

    // ADR-022's decision 5: the vanished file is an `ARTIFACT_DELETED` event, not a
    // `FILE_CHANGED` one — the latter folds into an upsert and would put the file back.
    let removal = events_of(&repositories, &project, event_catalog::ARTIFACT_DELETED);
    assert_eq!(removal.len(), 1);
    assert_eq!(removal[0].payload["path"], serde_json::json!("src/auth.rs"));
    assert_eq!(removal[0].actor_type, ActorType::System, "observed, not asserted");
    assert_eq!(removal[0].subject_id.as_deref(), Some("src/auth.rs"));
    assert!(
        events_of(&repositories, &project, event_catalog::FILE_CHANGED)
            .iter()
            .all(|event| event.payload["change"] != serde_json::json!("deleted")),
        "a deletion is not a content change: {:?}",
        events_of(&repositories, &project, event_catalog::FILE_CHANGED)
    );

    // The row is kept and marked: §45 has to be able to point at the file's history.
    let row = repositories
        .artifacts
        .find_artifact_by_path(&project, "src/auth.rs")
        .expect("artifact read")
        .expect("the row survives its own deletion");
    assert!(row.deleted, "marked, not removed");
    assert_eq!(row.hash.as_deref(), Some(recorded.as_str()));

    // A second pass over the same absence is not a second fact (§51).
    let again = scan.run(&project).expect("rescan");
    assert!(again.is_quiet(), "the removal is already recorded: {again:?}");
    assert_eq!(
        events_of(&repositories, &project, event_catalog::ARTIFACT_DELETED).len(),
        1
    );

    let after = Reducer::new(repositories.clone())
        .project_state(&project)
        .expect("reprojection");
    assert_eq!(after.artifacts.len(), before.artifacts.len());
    assert!(
        after.artifacts[0].deleted,
        "the fold and the cache row agree about the same log (§18)"
    );
    assert!(after.tasks.is_empty(), "a vanished file finishes nothing");
    assert!(after.decisions.is_empty(), "and it decides nothing");
    assert!(after.change_proposals.is_empty());
    assert!(after.drift_alerts.is_empty(), "and it condemns nothing");
    assert_eq!(after.milestones.len(), before.milestones.len());
}

/// `dirty` then clean then `dirty` with the same paths is one fact told twice: the payload
/// carries no clock, so the log says so instead of stacking a second row (§52).
#[test]
fn a_round_trip_over_the_same_paths_is_not_two_facts() {
    let (_store, repositories, project) = repos();
    let fake = FakeWorkspace::at("main", "aaa1111", &[]);
    let facts = GitFacts::new(repositories.clone(), fake.clone());
    facts.watch_workspace(&project, &checkout()).expect("clean baseline");

    fake.edit("main", "aaa1111", &["M src/auth.rs"]);
    let dirtied = facts
        .watch_workspace(&project, &checkout())
        .expect("went dirty");
    assert!(dirtied.status_changed);
    assert_eq!(dirtied.events_appended, 1);

    fake.edit("main", "aaa1111", &[]);
    let cleaned = facts
        .watch_workspace(&project, &checkout())
        .expect("went clean");
    assert!(cleaned.status_changed);
    assert_eq!(cleaned.events_appended, 1);

    fake.edit("main", "aaa1111", &["M src/auth.rs"]);
    let again = facts
        .watch_workspace(&project, &checkout())
        .expect("dirty once more");
    assert!(again.status_changed, "the snapshot really did move");
    assert_eq!(
        again.events_duplicate, 1,
        "the same from/to pair is already in the log"
    );
    assert_eq!(again.events_appended, 0);
    assert_eq!(
        events_of(&repositories, &project, event_catalog::GIT_STATUS_CHANGED).len(),
        2,
        "two distinct transitions, not three rows"
    );
}

/// A plain folder — §14 binds those too — has to be reported as one. No source, no
/// snapshot, no event, and no error pretending the pass succeeded.
#[test]
fn a_directory_with_no_repository_in_it_is_reported_rather_than_invented() {
    let (_store, repositories, project) = repos();
    let fake = Arc::new(FakeWorkspace::default());
    fake.no_repository();
    let facts = GitFacts::new(repositories.clone(), fake.clone());

    let report = facts
        .watch_workspace(&project, &checkout())
        .expect("a non-repository is a result, not a failure");

    assert!(!report.is_repository);
    assert_eq!(report.branch, "");
    assert!(report.dirty.is_empty());
    assert_eq!(report.events_appended, 0);
    assert!(repositories
        .conversations
        .list_sources(&project)
        .expect("sources")
        .is_empty());
    assert!(repositories
        .settings
        .get_json(&GitFacts::snapshot_key(&project, "/workspaces/codlas"))
        .expect("settings read")
        .is_none());

    // And a pass that *does* find a checkout writes into the same key, so the two readings
    // above are not an artifact of the fixture never running.
    fake.edit("main", "aaa1111", &["M src/auth.rs"]);
    let second = facts.watch_workspace(&project, &checkout()).expect("now a repo");
    assert!(second.is_repository);
    assert_eq!(
        repositories
            .conversations
            .list_sources(&project)
            .expect("sources")
            .len(),
        1,
        "the same key that refused a moment ago now owns a source"
    );
    assert!(repositories
        .settings
        .get_json(&GitFacts::snapshot_key(&project, "/workspaces/codlas"))
        .expect("settings read")
        .is_some());
    assert!(
        fake.commands()
            .iter()
            .all(|c| c.starts_with("git rev-parse") || c.starts_with("git status")),
        "watch stays read-only: {:?}",
        fake.commands()
    );
}
