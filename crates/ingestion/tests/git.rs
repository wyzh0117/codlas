//! Git capture (§39, PROJECT_MASTER §7.3 Evidence-first): commits become `GIT_COMMIT`
//! events, an artifact per touched path and Git evidence — and nothing more. A commit is a
//! fact about history; it is not a completion, and the state must look the same whether it
//! was written by this layer or rebuilt from the log (§18).

use std::path::Path;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use vibe_core::error::{CoreError, Result};
use vibe_core::ports::Repositories;
use vibe_core::project_service::ProjectService;
use vibe_domain::{
    event_catalog, ActorType, EntityKind, EventStatus, EvidenceType, ProjectEvent, SourceType,
    TaskStatus,
};
use vibe_ingestion::{
    log_format, CommitRecord, CommandOutcome, CommandRunner, GitFacts, GitFileStatus,
    IngestPipeline, ManualImportAdapter, ProcessCommandRunner, RecordedCommits, TouchedFile,
    READ_ONLY_GIT,
};
use vibe_state_engine::Reducer;
use vibe_storage::SqliteStore;

fn repos() -> (Arc<SqliteStore>, Repositories, String) {
    let store = SqliteStore::open_in_memory().expect("in-memory store");
    let repositories = store.repositories();
    let project = ProjectService::new(repositories.clone())
        .create_project("Git capture", None, None, None)
        .expect("project");
    (store, repositories, project.id)
}

fn ts(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|v| v.with_timezone(&Utc))
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

/// A runner that fails the test if capture reaches for a process.
struct NeverRuns;
impl CommandRunner for NeverRuns {
    fn run(&self, program: &str, args: &[&str], _workdir: &Path) -> Result<CommandOutcome> {
        Err(CoreError::Validation(format!(
            "capture must not spawn anything here: {program} {}",
            args.join(" ")
        )))
    }
}

/// Canned commits, no subprocess, no repository on disk.
fn commits() -> Vec<CommitRecord> {
    vec![
        CommitRecord {
            sha: "a1b2c3d4e5".into(),
            subject: "cache: repository 层落地".into(),
            body: Some("一次提交说明为什么是端口而不是具体实现".into()),
            author: Some("youngi".into()),
            authored_at: ts("2026-04-05T10:00:00Z"),
            files: vec![
                TouchedFile::new("crates/core/src/ports.rs", GitFileStatus::Modified),
                TouchedFile::new("docs/plan.md", GitFileStatus::Added),
            ],
        },
        CommitRecord {
            sha: "f6a7b8c9d0".into(),
            subject: "drop the superseded draft".into(),
            body: None,
            author: None,
            authored_at: ts("2026-04-06T08:30:00Z"),
            files: vec![TouchedFile::new("docs/draft.md", GitFileStatus::Deleted)],
        },
    ]
}

/// §39 + §17: commits → events + artifacts + evidence, idempotent, and never a completion.
#[test]
fn canned_commits_become_facts_artifacts_and_evidence() {
    let (_store, repositories, project) = repos();
    // An agent's word that something is finished, captured first, so the git run below
    // has every chance to "prove" it — and must not.
    let mut pipeline = IngestPipeline::new(repositories.clone(), None);
    pipeline.register(Arc::new(ManualImportAdapter::named(
        "claim-thread",
        "assistant: 我已经实现了重连逻辑",
    )));
    pipeline.run(&project).expect("claim captured");

    let facts = GitFacts::new(repositories.clone(), Arc::new(NeverRuns))
        .with_options(Some("main-checkout"), 10);
    let report = facts.ingest_commits(&project, &commits()).expect("ingest");

    assert_eq!(report.commits_seen, 2);
    assert_eq!(report.events_appended, 2, "one GIT_COMMIT per commit");
    assert_eq!(report.events_duplicate, 0);
    assert_eq!(report.artifacts_recorded, 3);
    assert_eq!(report.artifacts_deleted, 1);
    assert_eq!(
        report.paths,
        vec!["crates/core/src/ports.rs", "docs/draft.md", "docs/plan.md"]
    );
    assert!(report.skipped.is_empty());

    let commits_written = events_of(&repositories, &project, event_catalog::GIT_COMMIT);
    assert_eq!(commits_written.len(), 2);
    for event in &commits_written {
        // A commit is observed, not asserted by a person in a chat window.
        assert_eq!(event.actor_type, ActorType::System);
        assert_eq!(event.status, EventStatus::Accepted);
        assert_eq!(event.subject_type.as_deref(), Some("commit"));
        assert_eq!(event.confidence, 1.0);
        assert_eq!(event.evidence_refs.len(), 1, "the sha is the reference (§17)");
        assert!(event.payload["files"].is_array());
        assert!(event.source_id.as_deref().is_some_and(|s| !s.is_empty()));
    }
    let first = commits_written
        .iter()
        .find(|e| e.subject_id.as_deref() == Some("a1b2c3d4e5"))
        .expect("first commit");
    assert_eq!(first.payload["message"], "cache: repository 层落地");
    assert_eq!(first.payload["author"], "youngi");
    assert_eq!(
        first.payload["fileStatuses"]["docs/plan.md"], "added",
        "git's status letter survives as data"
    );
    assert_eq!(
        first.occurred_at,
        ts("2026-04-05T10:00:00Z"),
        "the commit keeps its own date; capture time stays in observed_at"
    );

    // §22: a path and a placeholder hash, never a body.
    let ports = repositories
        .artifacts
        .find_artifact_by_path(&project, "crates/core/src/ports.rs")
        .expect("artifact")
        .expect("ports.rs recorded");
    assert_eq!(ports.id, "crates/core/src/ports.rs", "same key the fold uses");
    assert_eq!(ports.hash.as_deref(), Some("git:a1b2c3d4e5"));
    assert_eq!(ports.version, 1);
    assert!(!ports.deleted);
    let plan = repositories
        .artifacts
        .find_artifact_by_path(&project, "docs/plan.md")
        .expect("artifact")
        .expect("plan.md recorded");
    assert_eq!(plan.kind, vibe_domain::ArtifactKind::Document);
    let draft = repositories
        .artifacts
        .find_artifact_by_path(&project, "docs/draft.md")
        .expect("artifact")
        .expect("draft recorded");
    assert!(draft.deleted, "a commit that removed the path says so");

    // Evidence answers "why does the system believe this?" from both ends (§17).
    let git_evidence = repositories
        .evidence
        .find_by_ref(EvidenceType::Git, "a1b2c3d4e5")
        .expect("evidence")
        .expect("the commit has an evidence row");
    assert_eq!(git_evidence.project_id, project);
    let on_plan = repositories
        .evidence
        .for_subject(EntityKind::Artifact, "docs/plan.md")
        .expect("links");
    assert_eq!(on_plan.len(), 1, "the artifact points back at its commit");
    assert_eq!(on_plan[0].id, git_evidence.id);

    // A second capture of the same log changes nothing at all (§51, §52).
    let evidence_before = repositories
        .evidence
        .list_evidence(&project)
        .expect("evidence")
        .len();
    let again = facts.ingest_commits(&project, &commits()).expect("re-ingest");
    assert_eq!(again.commits_seen, 2);
    assert_eq!(again.events_appended, 0);
    assert_eq!(again.events_duplicate, 2, "the dedup key is the commit, not the clock");
    assert_eq!(
        again.artifacts_recorded, 0,
        "a duplicate commit rewrites no artifact and bumps no version"
    );
    assert_eq!(
        repositories
            .artifacts
            .find_artifact_by_path(&project, "crates/core/src/ports.rs")
            .expect("artifact")
            .expect("still there")
            .version,
        1
    );
    assert_eq!(events_of(&repositories, &project, event_catalog::GIT_COMMIT).len(), 2);
    assert_eq!(
        repositories.evidence.list_evidence(&project).expect("evidence").len(),
        evidence_before
    );

    // The claimed task is still not Done: a commit is evidence about files, never a
    // confirmation of work (PROJECT_MASTER 禁止 9, §19).
    let state = Reducer::new(repositories.clone())
        .project_state(&project)
        .expect("projection");
    let claim = state
        .tasks
        .iter()
        .find(|t| t.status != TaskStatus::Done)
        .expect("the claim stays open");
    assert!(claim.unverified_claim);
    assert!(
        state.tasks.iter().all(|t| t.status != TaskStatus::Done),
        "{:?}",
        state.tasks.iter().map(|t| (&t.title, t.status)).collect::<Vec<_>>()
    );
    // And the projection agrees with the cache: the fold mints the same path-keyed
    // artifacts, including the deletion (§18 rebuild purity).
    let folded: Vec<(&str, bool)> = state
        .artifacts
        .iter()
        .map(|a| (a.path.as_deref().unwrap_or_default(), a.deleted))
        .collect();
    for path in ["crates/core/src/ports.rs", "docs/plan.md"] {
        assert!(
            folded.iter().any(|(p, deleted)| *p == path && !deleted),
            "{path} missing or wrongly deleted: {folded:?}"
        );
    }
    assert!(
        folded
            .iter()
            .any(|(p, deleted)| *p == "docs/draft.md" && *deleted),
        "a removal must survive a rebuild: {folded:?}"
    );
}

/// A commit with no sha is a malformed record, not a crash.
#[test]
fn a_commit_without_a_sha_is_skipped_and_reported() {
    let (_store, repositories, project) = repos();
    let facts = GitFacts::new(repositories.clone(), Arc::new(NeverRuns));
    let report = facts
        .ingest_commits(
            &project,
            &[
                CommitRecord::new("", "没有 sha", &["x.rs"]),
                CommitRecord::new("cafe1234", "真的提交", &["y.rs"]),
            ],
        )
        .expect("ingest");
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.events_appended, 1);
    assert_eq!(report.paths, vec!["y.rs"]);
    assert_eq!(
        events_of(&repositories, &project, event_catalog::GIT_COMMIT).len(),
        1
    );
}

/// A runner that records what it was asked for and hands back canned text — the only way
/// to test a subprocess layer without a repository (§7.6).
struct Canned {
    log: String,
    diff: String,
    head: String,
    calls: Mutex<Vec<Vec<String>>>,
}

impl Canned {
    fn new(log: &str, diff: &str, head: &str) -> Self {
        Self {
            log: log.to_string(),
            diff: diff.to_string(),
            head: head.to_string(),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().expect("calls").clone()
    }
}

impl CommandRunner for Canned {
    fn run(&self, program: &str, args: &[&str], _workdir: &Path) -> Result<CommandOutcome> {
        let argv: Vec<String> = std::iter::once(program.to_string())
            .chain(args.iter().map(|a| a.to_string()))
            .collect();
        self.calls.lock().expect("calls").push(argv);
        let subcommand = args.iter().find(|a| !a.starts_with('-')).copied().unwrap_or("");
        // The test-side guard mirrors the production one, so an accidental
        // `git commit` here fails loudly instead of touching a repository.
        assert!(
            READ_ONLY_GIT.contains(&subcommand),
            "capture asked for a write command: {subcommand}"
        );
        let stdout = match subcommand {
            "log" => self.log.clone(),
            "diff" => self.diff.clone(),
            "rev-parse" => self.head.clone(),
            other => return Err(CoreError::Io(format!("no canned output for {other}"))),
        };
        Ok(CommandOutcome::new(0, stdout, ""))
    }
}

/// The header is RS + sha US subject US author US ISO date, then `--name-status` lines.
fn canned_log() -> String {
    format!(
        "\u{1e}9f8e7d6c\u{1f}search: 三层检索\u{1f}youngi\u{1f}2026-04-07T09:15:00+08:00\n\
         M\tcrates/search/src/service.rs\n\
         A\tcrates/search/src/catalog.rs\n\
         \n\
         \u{1e}1a2b3c4d\u{1f}docs: 记录一次重命名\u{1f}qa\u{1f}2026-04-08T11:00:00Z\n\
         R100\tcrates/search/src/old.rs\tcrates/search/src/service.rs\n"
    )
}

#[test]
fn git_log_output_is_parsed_and_captured() {
    let (_store, repositories, project) = repos();
    let runner = Arc::new(Canned::new(
        &canned_log(),
        "M\tcrates/search/src/service.rs\n",
        "9f8e7d6c5b4a3f2e1d0c\n",
    ));
    let facts = GitFacts::new(repositories.clone(), runner.clone());
    // Any directory the caller injects; nothing is read from it.
    let workdir = Path::new(".");

    assert_eq!(
        facts.head_sha(workdir).as_deref(),
        Some("9f8e7d6c5b4a3f2e1d0c"),
        "`rev-parse HEAD` is the read-only way to ask for the tip"
    );
    let changed = facts.changed_files(workdir, "HEAD~1", "HEAD").expect("diff");
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].status, GitFileStatus::Modified);

    let report = facts.run(&project, workdir).expect("run");
    assert_eq!(runner.calls().len(), 3, "rev-parse, diff, log — all read-only");
    let log_call = runner
        .calls()
        .iter()
        .find(|call| call.get(1).map(String::as_str) == Some("log"))
        .expect("a log call")
        .join(" ");
    assert!(log_call.contains("--name-status"), "{log_call}");
    assert!(
        log_call.contains(&format!("--pretty=format:{}", log_format())),
        "the parser and the format are one contract: {log_call}"
    );
    assert!(log_call.contains("--max-count="), "{log_call}");

    assert_eq!(report.commits_seen, 2);
    assert_eq!(report.events_appended, 2);
    assert_eq!(report.artifacts_recorded, 3);
    assert_eq!(
        report.paths,
        vec![
            "crates/search/src/catalog.rs",
            "crates/search/src/service.rs",
        ],
        "a rename records the path that now exists"
    );

    // The source is a `git` row keyed by the injected directory, so two checkouts are
    // two sources (§38) and one checkout never forks into a second row.
    let sources = repositories
        .conversations
        .list_sources(&project)
        .expect("sources");
    assert_eq!(sources.len(), 1, "{:?}", sources.iter().map(|s| &s.external_id).collect::<Vec<_>>());
    assert_eq!(sources[0].r#type, SourceType::Git);
    assert_eq!(sources[0].external_id.as_deref(), Some("."));
    assert_eq!(report.source_id, sources[0].id);

    // Re-running the same history is a no-op, cursor-free by construction (§52).
    let again = facts.run(&project, workdir).expect("re-run");
    assert_eq!(again.events_appended, 0);
    assert_eq!(again.events_duplicate, 2);
    assert_eq!(again.artifacts_recorded, 0);
}

/// The subprocess gate is enforced by the real runner, not just by the trait contract.
#[test]
fn the_process_runner_refuses_everything_but_read_only_git() {
    let runner = ProcessCommandRunner::new();
    for args in [
        &["log", "--oneline"][..],
        &["diff", "--name-status", "HEAD~1", "HEAD"][..],
        &["rev-parse", "HEAD"][..],
        &["show", "HEAD"][..],
        &["ls-files"][..],
        &["status", "--porcelain"][..],
    ] {
        assert!(runner.check("git", args).is_ok(), "{args:?} must be allowed");
    }
    for args in [
        &["commit", "-m", "x"][..],
        &["checkout", "main"][..],
        &["reset", "--hard"][..],
        &["clean", "-fd"][..],
        &["push"][..],
        &["rm", "src/main.rs"][..],
        &["init"][..],
    ] {
        let error = runner
            .check("git", args)
            .expect_err("{args:?} must be refused before a process spawns");
        assert!(error.to_string().contains("read-only"), "{error}");
    }
    // Not even another program can be named, whatever the arguments.
    assert!(runner.check("sh", &["-c", "git log"]).is_err());
    assert!(runner.check("rm", &["-rf", "target"]).is_err());
    // A deployment can widen the list, and still only for git.
    let widened = ProcessCommandRunner::new().allow("blame");
    assert!(widened.check("git", &["blame", "src/lib.rs"]).is_ok());
    assert!(widened.run("git", &["commit"], Path::new(".")).is_err());
}

/// Recorded commits round-trip through serde, so a fixture file or a UI paste can be the
/// injected data.
#[test]
fn commit_records_are_data() {
    let encoded = serde_json::to_string(&RecordedCommits::new(commits())).expect("to json");
    let decoded: RecordedCommits = serde_json::from_str(&encoded).expect("from json");
    assert_eq!(decoded.commits, commits());
    assert_eq!(decoded.len(), 2);
    assert_eq!(decoded.commits[0].paths(), vec!["crates/core/src/ports.rs", "docs/plan.md"]);
}
