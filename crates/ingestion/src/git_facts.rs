//! Git capture: commits become `GIT_COMMIT` events plus artifact rows and Git evidence
//! (V1_DEVELOPMENT.md §39, PROJECT_MASTER §7.3 Evidence-first).
//!
//! Two entry points, because the layer's contract is "inject the data, never discover it":
//!
//! * [`GitFacts::ingest_commits`] takes already-parsed [`CommitRecord`]s — canned from a
//!   test, a fixture file, or a UI paste. It runs no command at all.
//! * [`GitFacts::run`] reads history through the injected [`CommandRunner`] and parses the
//!   output. The runner is a trait precisely so tests can feed canned text instead of
//!   shelling out.
//! * [`GitFacts::watch_workspace`] diff-reads the *working copy* — branch and uncommitted
//!   paths — against the snapshot the previous pass left in `settings` (§39). V1 has no
//!   daemon, so watching means comparing two passes, and the only memory a pass needs is
//!   one row.
//!
//! Guards that make the subprocess side safe:
//!
//! * the working directory is a caller-supplied [`std::path::Path`]; nothing here
//!   hard-codes a location;
//! * only read-only `git` subcommands are allowed ([`READ_ONLY_GIT`]); anything else —
//!   `commit`, `checkout`, `reset`, `clean` — is refused before a process is spawned;
//! * commits are recorded as *facts about history*. A commit never completes a task:
//!   §39 says a change is evidence about an artifact, §19 says completion needs a
//!   confirmed event a human agreed to. The reducer's `GIT_COMMIT` branch folds only
//!   artifacts, which is the same conclusion this module reaches.
//!
//! Idempotency is the store's dedup key: a commit's event payload is built from the
//! commit itself (sha, subject, paths), never from a wall clock, so ingesting the same
//! log twice appends nothing and rewrites no artifact.

use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use vibe_core::error::{CoreError, Result};
use vibe_core::event_store::{EventDraft, EventStore};
use vibe_core::ports::{EntityStore, Repositories};
use vibe_domain::{
    event_catalog, ActorType, Artifact, EntityKind, EvidenceType, Source, SourceType,
};

use crate::pipeline::{artifact_kind_for, artifact_name_for, ensure_evidence, link_evidence};

/// `git` subcommands this layer may run. Every one of them only reads: a capture layer
/// must never be able to mutate a repository (§6 boundary, PROJECT_MASTER 禁止).
pub const READ_ONLY_GIT: &[&str] = &[
    "log",
    "show",
    "diff",
    "rev-parse",
    "ls-files",
    "status",
];

/// How many commits one scan looks at, newest first.
pub const DEFAULT_MAX_COMMITS: usize = 50;

/// Record separator: a byte that cannot appear in a commit subject, so one
/// `git log --pretty=format:` call still parses unambiguously.
const RECORD_SEP: char = '\u{1e}';
/// Field separator inside a record header.
const FIELD_SEP: char = '\u{1f}';

/// The `--pretty=format:` value [`GitFacts::run`] asks git for.
pub fn log_format() -> String {
    format!("%{RECORD_SEP}%H%{FIELD_SEP}%s%{FIELD_SEP}%an%{FIELD_SEP}%aI")
}

/// What a subprocess produced: exit code plus both streams, already decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutcome {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutcome {
    pub fn new(code: i32, stdout: impl Into<String>, stderr: impl Into<String>) -> Self {
        Self {
            code,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    pub fn ok(&self) -> bool {
        self.code == 0
    }

    /// Successful output, or an error carrying git's own complaint, so a report can say
    /// *why* a source produced nothing instead of silently reporting zero.
    pub fn text(&self) -> Result<String> {
        if self.ok() {
            Ok(self.stdout.clone())
        } else {
            Err(CoreError::Io(self.stderr.trim().replace('\n', " ")))
        }
    }
}

/// The subprocess boundary. Tests implement this and hand back canned text; production
/// uses [`ProcessCommandRunner`].
pub trait CommandRunner: Send + Sync {
    fn run(&self, program: &str, args: &[&str], workdir: &Path) -> Result<CommandOutcome>;
}

/// The real runner. Only ever `program = "git"` with a subcommand from [`READ_ONLY_GIT`];
/// both are enforced here, so a bug in a caller cannot turn capture into a mutation.
pub struct ProcessCommandRunner {
    /// Subcommands a deployment may allow beyond the read-only set.
    allowed: Vec<String>,
}

impl Default for ProcessCommandRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessCommandRunner {
    pub fn new() -> Self {
        Self {
            allowed: READ_ONLY_GIT.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Widens the allow-list. There is deliberately no way to narrow it, no way to allow
    /// a program other than `git`, and no method that takes a shell string.
    pub fn allow(mut self, subcommand: &str) -> Self {
        if !self.allowed.iter().any(|a| a == subcommand) {
            self.allowed.push(subcommand.to_string());
        }
        self
    }

    /// `git` plus a subcommand from the allow-list, nothing else. Checked on the first
    /// non-flag argument, which is where git's own grammar puts it.
    pub fn check(&self, program: &str, args: &[&str]) -> Result<()> {
        if program != "git" {
            return Err(CoreError::Validation(format!(
                "refusing to run {program:?}: this layer only speaks git"
            )));
        }
        let subcommand = args
            .iter()
            .find(|a| !a.starts_with('-'))
            .copied()
            .unwrap_or("");
        if !self.allowed.iter().any(|a| a == subcommand) {
            return Err(CoreError::Validation(format!(
                "refusing `git {subcommand}`: capture is read-only (allowed: {})",
                self.allowed.join(", ")
            )));
        }
        Ok(())
    }
}

impl CommandRunner for ProcessCommandRunner {
    fn run(&self, program: &str, args: &[&str], workdir: &Path) -> Result<CommandOutcome> {
        self.check(program, args)?;
        if !workdir.is_dir() {
            return Err(CoreError::NotFound {
                what: format!("repository directory {}", workdir.display()),
            });
        }
        // `Command::args` passes each argument as one argv entry: no shell, so no
        // word-splitting or injection through a path containing spaces.
        let output = std::process::Command::new(program)
            .args(args)
            .current_dir(workdir)
            .output()
            .map_err(|e| CoreError::Io(format!("{program} failed to start: {e}")))?;
        Ok(CommandOutcome {
            code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

/// A touched path plus git's status letter for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitFileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    Other,
}

impl GitFileStatus {
    pub fn from_letter(letter: &str) -> Self {
        match letter.chars().next() {
            Some('A') => GitFileStatus::Added,
            Some('M') => GitFileStatus::Modified,
            Some('D') => GitFileStatus::Deleted,
            Some('R') => GitFileStatus::Renamed,
            Some('C') => GitFileStatus::Copied,
            _ => GitFileStatus::Other,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            GitFileStatus::Added => "added",
            GitFileStatus::Modified => "modified",
            GitFileStatus::Deleted => "deleted",
            GitFileStatus::Renamed => "renamed",
            GitFileStatus::Copied => "copied",
            GitFileStatus::Other => "other",
        }
    }

    /// The path no longer exists after this commit.
    pub fn removes_file(&self) -> bool {
        matches!(self, GitFileStatus::Deleted)
    }
}

/// One path a commit touched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TouchedFile {
    pub path: String,
    pub status: GitFileStatus,
}

impl TouchedFile {
    pub fn new(path: impl Into<String>, status: GitFileStatus) -> Self {
        Self {
            path: path.into(),
            status,
        }
    }
}

/// One commit as the capture layer sees it: identity, message, author, date, paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CommitRecord {
    pub sha: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authored_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub files: Vec<TouchedFile>,
}

impl CommitRecord {
    /// The shorthand fixtures want: a sha, a subject and the paths it touched (tracked
    /// as modifications, which is what committing an existing file is).
    pub fn new(sha: &str, subject: &str, paths: &[&str]) -> Self {
        Self {
            sha: sha.to_string(),
            subject: subject.to_string(),
            body: None,
            author: None,
            authored_at: None,
            files: paths
                .iter()
                .map(|p| TouchedFile::new(*p, GitFileStatus::Modified))
                .collect(),
        }
    }

    pub fn paths(&self) -> Vec<&str> {
        self.files.iter().map(|f| f.path.as_str()).collect()
    }
}

/// Commit material handed to the capture layer, from whatever origin the caller chose.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedCommits {
    pub commits: Vec<CommitRecord>,
}

impl RecordedCommits {
    pub fn new(commits: Vec<CommitRecord>) -> Self {
        Self { commits }
    }

    /// A single commit, for callers that already have it (a HEAD detail view).
    pub fn one(commit: CommitRecord) -> Self {
        Self {
            commits: vec![commit],
        }
    }

    pub fn empty() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.commits.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commits.is_empty()
    }

    /// Parse the output of `git log --name-status --pretty=format:<log_format()>`: one
    /// record per commit, `RS` before each header, `US` between header fields, then
    /// git's `--name-status` lines.
    ///
    /// Tolerates the blank line git inserts between a header and its diff, and the
    /// newline that precedes every record after the first.
    pub fn parse(raw: &str) -> Result<Self> {
        let mut commits = Vec::new();
        for chunk in raw.split(RECORD_SEP) {
            let chunk = chunk.trim_start_matches(['\n', '\r']);
            if chunk.trim().is_empty() {
                continue;
            }
            let mut lines = chunk.lines();
            let header = lines.next().unwrap_or_default();
            let fields: Vec<&str> = header.split(FIELD_SEP).collect();
            let sha = fields
                .first()
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            if sha.is_empty() {
                return Err(CoreError::Validation(
                    "git log output has a record without a commit sha".into(),
                ));
            }
            let authored_at = fields
                .get(3)
                .and_then(|v| DateTime::parse_from_rfc3339(v.trim()).ok())
                .map(|v| v.with_timezone(&Utc));
            let files = lines.filter_map(parse_name_status).collect();
            commits.push(CommitRecord {
                sha,
                subject: fields.get(1).map(|s| s.trim().to_string()).unwrap_or_default(),
                body: None,
                author: fields
                    .get(2)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
                authored_at,
                files,
            });
        }
        Ok(Self { commits })
    }
}

/// `A\tpath`, `M\tpath`, `R100\told\tnew` — for a rename the new path is the one that
/// exists, so that is what gets recorded.
fn parse_name_status(line: &str) -> Option<TouchedFile> {
    let line = line.trim_end_matches(['\r', '\n']);
    let mut parts = line.split('\t');
    let status = parts.next()?;
    let path = parts.next_back()?;
    if path.trim().is_empty() {
        return None;
    }
    Some(TouchedFile::new(
        path.trim().to_string(),
        GitFileStatus::from_letter(status),
    ))
}

/// What one git capture did, per source.
#[derive(Debug, Clone, Default)]
pub struct GitFactsReport {
    pub source_id: String,
    /// Commits handed to the capture, including ones already known.
    pub commits_seen: usize,
    /// `GIT_COMMIT` events newly written.
    pub events_appended: usize,
    /// Commits whose event was already in the log (§52 idempotent re-ingest).
    pub events_duplicate: usize,
    /// Artifact rows created or updated — one per newly seen touched path.
    pub artifacts_recorded: usize,
    /// Paths this batch recorded as removed by a commit. Each one also writes an
    /// `ARTIFACT_DELETED` event, so a rebuild of the log agrees with the row (§18).
    pub artifacts_deleted: usize,
    /// Distinct paths the batch touched, sorted, for a surface that lists them.
    pub paths: Vec<String>,
    /// Commits skipped because they carry no sha (a malformed record, never a crash).
    pub skipped: Vec<String>,
}

impl GitFactsReport {
    fn record_path(&mut self, path: &str) {
        if !self.paths.iter().any(|p| p == path) {
            self.paths.push(path.to_string());
            self.paths.sort();
        }
    }
}

/// What the previous watch pass saw, kept as one row in `settings` (§39).
///
/// A snapshot rather than a log: without a daemon, the only thing a watch can diff
/// against is what it recorded last time. Paths and a sha, never file contents (§22).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub branch: String,
    #[serde(default)]
    pub head: Option<String>,
    /// `git status --porcelain` lines, sorted, so two identical trees compare equal.
    #[serde(default)]
    pub status: Vec<String>,
}

/// One watch pass over a working copy (§39).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceWatchReport {
    /// `false` when the directory holds no checkout, in which case every other field is
    /// empty and nothing was written (§14 binds a plain folder too).
    pub is_repository: bool,
    pub source_id: String,
    pub branch: String,
    #[serde(default)]
    pub head: Option<String>,
    /// The dirty set right now, git's own porcelain lines.
    #[serde(default)]
    pub dirty: Vec<String>,
    /// `false` on the first pass over a checkout: nothing changed *relative to* anything,
    /// so the run recorded a baseline and wrote no event.
    pub compared: bool,
    pub branch_changed: bool,
    pub status_changed: bool,
    #[serde(default)]
    pub previous_branch: Option<String>,
    pub events_appended: usize,
    /// Signals whose payload was already in the log — a tree that went dirty, clean, and
    /// dirty with the same paths again (§52 idempotent re-capture).
    pub events_duplicate: usize,
}

impl WorkspaceWatchReport {
    /// What this pass leaves behind for the next one to diff against.
    fn as_after(&self) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            branch: self.branch.clone(),
            head: self.head.clone(),
            status: self.dirty.clone(),
        }
    }
}

/// Captures commit history into the event log.
pub struct GitFacts {
    repos: Repositories,
    events: EventStore,
    runner: Arc<dyn CommandRunner>,
    /// The repository's `Source` external id. When unset, [`GitFacts::run`] falls back to
    /// the injected directory and [`GitFacts::ingest_commits`] to the project, so a
    /// project with more than one checkout should set it (§38 one source per origin).
    repo_id: Option<String>,
    max_commits: usize,
}

impl GitFacts {
    /// `runner` is the only way this module reaches a subprocess, and the only way a
    /// test feeds canned git output.
    pub fn new(repos: Repositories, runner: Arc<dyn CommandRunner>) -> Self {
        Self {
            events: EventStore::new(repos.clone()),
            repos,
            runner,
            repo_id: None,
            max_commits: DEFAULT_MAX_COMMITS,
        }
    }

    /// A stable source identity (`my-app-main`) plus a scan window.
    pub fn with_options(mut self, git_repo_id: Option<&str>, max_commits: usize) -> Self {
        self.repo_id = git_repo_id.map(|s| s.to_string());
        if max_commits > 0 {
            self.max_commits = max_commits;
        }
        self
    }

    pub fn repositories(&self) -> &Repositories {
        &self.repos
    }

    /// `git rev-parse HEAD` — the read-only way to ask "what is the tip?".
    /// `None` when the directory is not a repository or git is unavailable.
    pub fn head_sha(&self, workdir: &Path) -> Option<String> {
        self.runner
            .run("git", &["rev-parse", "HEAD"], workdir)
            .and_then(|o| o.text())
            .ok()
            .and_then(|stdout| {
                stdout
                    .lines()
                    .next()
                    .map(str::trim)
                    .filter(|s| s.len() >= 7 && s.chars().all(|c| c.is_ascii_hexdigit()))
                    .map(str::to_string)
            })
    }

    /// `git diff --name-status FROM TO`, the read-only way to ask "what moved since?".
    pub fn changed_files(
        &self,
        workdir: &Path,
        from: &str,
        to: &str,
    ) -> Result<Vec<TouchedFile>> {
        let stdout = self
            .runner
            .run("git", &["diff", "--name-status", from, to], workdir)
            .and_then(|o| o.text())?;
        Ok(stdout.lines().filter_map(parse_name_status).collect())
    }

    /// §39's watch, without a daemon: read the working copy, diff it against the snapshot
    /// the previous pass stored, write what differs, then store the new snapshot.
    ///
    /// The two signals are the ones no commit event can express — which branch HEAD sits on,
    /// and which files are uncommitted — and the reducer folds neither into project state,
    /// because a dirty tree says where the work is, not that any of it is finished (§39).
    ///
    /// A directory with no repository in it is a *result*, not a failure: §14 binds a plain
    /// folder as often as a checkout, so the report says `is_repository: false` and writes
    /// nothing. The caller decides whether that is worth a line.
    pub fn watch_workspace(
        &self,
        project_id: &str,
        workdir: &Path,
    ) -> Result<WorkspaceWatchReport> {
        let Some(branch) = self.first_line(&["rev-parse", "--abbrev-ref", "HEAD"], workdir)?
        else {
            return Ok(WorkspaceWatchReport {
                is_repository: false,
                ..WorkspaceWatchReport::default()
            });
        };
        let head = self.head_sha(workdir);
        let dirty = self.porcelain(workdir)?;
        let key = self
            .repo_id
            .clone()
            .unwrap_or_else(|| workdir.display().to_string());
        let source = self.git_source(project_id, &key)?;

        let previous = self.read_snapshot(project_id, &key)?;
        let mut report = WorkspaceWatchReport {
            is_repository: true,
            source_id: source.id.clone(),
            branch,
            head,
            dirty,
            compared: previous.is_some(),
            ..WorkspaceWatchReport::default()
        };

        // Nothing to compare against yet: the pass that sees a checkout for the first time
        // records where it stands and claims no change (§39 says watch, not invent).
        let Some(previous) = previous else {
            self.write_snapshot(project_id, &key, &report.as_after())?;
            return Ok(report);
        };

        report.previous_branch = Some(previous.branch.clone());
        report.branch_changed = previous.branch != report.branch;
        report.status_changed = previous.status != report.dirty;

        if report.branch_changed {
            let inserted = self.record_watch(
                project_id,
                &source.id,
                event_catalog::GIT_BRANCH_CHANGED,
                "branch",
                &report.branch,
                serde_json::json!({
                    "from": previous.branch,
                    "to": report.branch,
                    "head": report.head,
                    "repository": key,
                }),
            )?;
            if inserted {
                report.events_appended += 1;
            } else {
                report.events_duplicate += 1;
            }
        }
        if report.status_changed {
            let inserted = self.record_watch(
                project_id,
                &source.id,
                event_catalog::GIT_STATUS_CHANGED,
                "workspace",
                &report.branch,
                serde_json::json!({
                    "from": previous.status,
                    "to": report.dirty,
                    "head": report.head,
                    "clean": report.dirty.is_empty(),
                    "repository": key,
                }),
            )?;
            if inserted {
                report.events_appended += 1;
            } else {
                report.events_duplicate += 1;
            }
        }

        self.write_snapshot(project_id, &key, &report.as_after())?;
        Ok(report)
    }

    /// The settings row a watch pass reads back next time. Public because a surface that
    /// shows "what did the last watch see?" must not re-derive the key format.
    pub fn snapshot_key(project_id: &str, repo_key: &str) -> String {
        format!("watch:{project_id}:{repo_key}")
    }

    /// One read-only git call and its first stdout line — `None` when the call failed or
    /// said nothing, which is how "this directory is not a checkout" reaches the caller.
    fn first_line(&self, args: &[&str], workdir: &Path) -> Result<Option<String>> {
        match self.runner.run("git", args, workdir).and_then(|o| o.text()) {
            Ok(stdout) => Ok(stdout
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(str::to_string)),
            Err(_) => Ok(None),
        }
    }

    /// `git status --porcelain`, sorted, empty lines dropped: the dirty set as data.
    fn porcelain(&self, workdir: &Path) -> Result<Vec<String>> {
        let stdout = self
            .runner
            .run("git", &["status", "--porcelain"], workdir)
            .and_then(|o| o.text())?;
        let mut lines: Vec<String> = stdout
            .lines()
            .map(str::trim_end)
            .filter(|line| !line.trim().is_empty())
            .map(str::to_string)
            .collect();
        lines.sort();
        Ok(lines)
    }

    fn read_snapshot(&self, project_id: &str, repo_key: &str) -> Result<Option<WorkspaceSnapshot>> {
        let Some(value) = self
            .repos
            .settings
            .get_json(&Self::snapshot_key(project_id, repo_key))?
        else {
            return Ok(None);
        };
        Ok(Some(serde_json::from_value::<WorkspaceSnapshot>(value).map_err(
            |e| CoreError::Validation(format!("the stored workspace snapshot is unreadable: {e}")),
        )?))
    }

    /// Stores what the pass saw. A snapshot that never lands would make the next pass
    /// re-report the same diff as news, so this is part of the write, not a nicety.
    fn write_snapshot(
        &self,
        project_id: &str,
        repo_key: &str,
        snapshot: &WorkspaceSnapshot,
    ) -> Result<()> {
        self.repos.settings.set_json(
            &Self::snapshot_key(project_id, repo_key),
            &serde_json::to_value(snapshot)
                .map_err(|e| CoreError::Validation(format!("snapshot: {e}")))?,
        )
    }

    /// One watch signal: a `System` event on the repository's own source, keyed on a
    /// payload of branch names and paths so a repeated pass dedups instead of stacking
    /// (§51). Watch material is evidence, never a conclusion (§39) — which is also why it
    /// carries no `evidence_refs`: an evidence row points at material that vouches for a
    /// claim, and a branch name *is* the fact the event states. The trace runs
    /// source → event, which §17 asks of a signal that changes no state.
    fn record_watch(
        &self,
        project_id: &str,
        source_id: &str,
        event_type: &str,
        subject_type: &str,
        subject_id: &str,
        payload: Value,
    ) -> Result<bool> {
        let draft = EventDraft::new(project_id, event_type)
            .actor(ActorType::System)
            .subject(subject_type, subject_id)
            .payload(payload);
        let mut event = draft.into_event();
        event.source_id = Some(source_id.to_string());
        Ok(self.events.append_event(event)?.inserted)
    }

    /// Read history with the injected runner and capture it.
    pub fn run(&self, project_id: &str, workdir: &Path) -> Result<GitFactsReport> {
        let stdout = self
            .runner
            .run(
                "git",
                &[
                    "log",
                    "--no-color",
                    "--name-status",
                    &format!("--pretty=format:{}", log_format()),
                    &format!("--max-count={}", self.max_commits),
                ],
                workdir,
            )
            .and_then(|o| o.text())?;
        let commits = RecordedCommits::parse(&stdout)?;
        let key = self
            .repo_id
            .clone()
            .unwrap_or_else(|| workdir.display().to_string());
        self.capture(project_id, &key, &commits.commits)
    }

    /// Capture canned commits: no subprocess, no filesystem — which is what makes the
    /// pipeline testable without a repository, and what §7.6 asks of an adapter
    /// (vendor-shaped data in, events out, discovery left to the caller).
    pub fn ingest_commits(
        &self,
        project_id: &str,
        commits: &[CommitRecord],
    ) -> Result<GitFactsReport> {
        let key = self
            .repo_id
            .clone()
            .unwrap_or_else(|| format!("git:{project_id}"));
        self.capture(project_id, &key, commits)
    }

    fn capture(
        &self,
        project_id: &str,
        repo_key: &str,
        commits: &[CommitRecord],
    ) -> Result<GitFactsReport> {
        let source = self.git_source(project_id, repo_key)?;
        let mut report = GitFactsReport {
            source_id: source.id.clone(),
            commits_seen: commits.len(),
            ..GitFactsReport::default()
        };

        for commit in commits {
            let sha = commit.sha.trim();
            if sha.is_empty() {
                report.skipped.push("(commit without a sha)".to_string());
                continue;
            }
            // The path list is built once and shared by the event payload and the
            // artifact rows: it is part of the dedup key, so the order is deliberate.
            let mut files = Vec::new();
            let mut statuses = serde_json::Map::new();
            for touched in &commit.files {
                let path = touched.path.trim();
                if path.is_empty() {
                    continue;
                }
                files.push(Value::String(path.to_string()));
                statuses.insert(
                    path.to_string(),
                    Value::String(touched.status.as_str().to_string()),
                );
                report.record_path(path);
            }

            let mut payload = serde_json::Map::new();
            payload.insert("sha".into(), Value::String(sha.to_string()));
            payload.insert(
                "message".into(),
                Value::String(if commit.subject.trim().is_empty() {
                    format!("commit {sha}")
                } else {
                    commit.subject.clone()
                }),
            );
            if let Some(body) = commit.body.as_deref().filter(|b| !b.trim().is_empty()) {
                payload.insert("body".into(), Value::String(body.to_string()));
            }
            if let Some(author) = commit.author.as_deref().filter(|a| !a.trim().is_empty()) {
                payload.insert("author".into(), Value::String(author.to_string()));
            }
            payload.insert("files".into(), Value::Array(files));
            payload.insert("fileStatuses".into(), Value::Object(statuses));

            let draft = EventDraft::new(project_id, event_catalog::GIT_COMMIT)
                // A commit is a fact about history the tool observed, not a claim made by
                // a person in a chat window (§7.2).
                .actor(ActorType::System)
                .subject("commit", sha)
                .payload(Value::Object(payload));
            let mut event = draft.into_event();
            event.source_id = Some(source.id.clone());
            // The commit's own date belongs in the log; `observed_at` stays the capture
            // time, so replay order remains the store's `seq` (§18).
            event.occurred_at = commit.authored_at;
            // `evidence_refs` carries the *reference*, so this layer's evidence row and
            // the reducer's meet on the same sha (§17).
            event.evidence_refs = vec![sha.to_string()];
            event.actor_id = commit
                .author
                .as_deref()
                .map(str::trim)
                .filter(|a| !a.is_empty())
                .map(str::to_string);

            let outcome = self.events.append_event(event)?;
            if !outcome.inserted {
                report.events_duplicate += 1;
                // Nothing new happened, so nothing else may move either: re-ingesting a
                // log must not bump artifact versions or rewrite rows.
                continue;
            }
            report.events_appended += 1;
            let evidence_id =
                ensure_evidence(&self.repos, project_id, EvidenceType::Git, sha, &format!(
                    "commit {sha}: {}",
                    commit.subject.trim()
                ))?;

            for touched in &commit.files {
                let path = touched.path.trim();
                if path.is_empty() {
                    continue;
                }
                let deleted = touched.status.removes_file();
                let id = self.record_artifact(project_id, path, sha, deleted)?;
                report.artifacts_recorded += 1;
                if deleted {
                    report.artifacts_deleted += 1;
                    // A removal has to be an event too. `GIT_COMMIT` folds into an
                    // artifact *upsert*, which always marks the path present, so a row
                    // flipped to deleted here would resurrect on the next rebuild (§18).
                    self.record_removal(project_id, &source.id, path, sha)?;
                }
                // "Which commits back this file?" is answerable from the artifact, and
                // "which files did this commit change?" from the commit (§17).
                link_evidence(&self.repos, &evidence_id, EntityKind::Artifact, &id)?;
            }
        }
        Ok(report)
    }

    /// `ARTIFACT_DELETED` for one path a commit removed, keyed on the same relative path
    /// the artifact row and the fold both use.
    fn record_removal(&self, project_id: &str, source_id: &str, path: &str, sha: &str) -> Result<()> {
        let draft = EventDraft::new(project_id, event_catalog::ARTIFACT_DELETED)
            .actor(ActorType::System)
            .subject("artifact", path)
            .payload(serde_json::json!({ "path": path, "sha": sha }));
        let mut event = draft.into_event();
        event.source_id = Some(source_id.to_string());
        event.evidence_refs = vec![path.to_string()];
        self.events.append_event(event)?;
        Ok(())
    }

    /// One artifact row per touched path: path, a placeholder hash (the commit sha,
    /// because §22 forbids file bodies in the database) and a version. Returns the row id.
    fn record_artifact(&self, project_id: &str, path: &str, sha: &str, deleted: bool) -> Result<String> {
        // Id equals the path, because that is what the reducer's `GIT_COMMIT` branch mints
        // for these same paths: one row per file, whichever layer wrote it first (§50).
        let existing = self.repos.artifacts.find_artifact_by_path(project_id, path)?;
        let mut artifact = existing.unwrap_or_else(|| Artifact {
            id: path.to_string(),
            project_id: project_id.to_string(),
            name: artifact_name_for(path),
            kind: artifact_kind_for(path),
            path: Some(path.to_string()),
            hash: None,
            version: 0,
            relationships: Vec::new(),
            deleted: false,
            redesign_count: 0,
            updated_at: None,
        });
        artifact.project_id = project_id.to_string();
        artifact.path = Some(path.to_string());
        // `git:{sha}` marks a placeholder, not content: `FileScan` reads it as "no
        // baseline yet" instead of inventing a change (§39 division of labour).
        artifact.hash = Some(format!("git:{sha}"));
        artifact.version = artifact.version.saturating_add(1);
        artifact.deleted = deleted;
        artifact.updated_at = Some(vibe_domain::now());
        self.repos.artifacts.upsert_artifact(&artifact)?;
        self.repos.entities.put_entity(
            EntityKind::Artifact,
            &artifact,
            Some(if artifact.deleted { "deleted" } else { "active" }),
        )?;
        Ok(artifact.id)
    }

    fn git_source(&self, project_id: &str, repo_key: &str) -> Result<Source> {
        let existing = self
            .repos
            .conversations
            .find_source_by_external_id(project_id, SourceType::Git.as_str(), repo_key)?;
        let source = match existing {
            Some(source) => source,
            None => Source {
                id: vibe_domain::new_id(),
                project_id: project_id.to_string(),
                r#type: SourceType::Git,
                external_id: Some(repo_key.to_string()),
                display_name: repo_key.to_string(),
                metadata: serde_json::json!({ "adapter": "git-facts", "repository": repo_key }),
            },
        };
        self.repos.conversations.upsert_source(&source)?;
        Ok(source)
    }
}
