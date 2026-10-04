//! File capture: a directory scan becomes `FILE_CHANGED` / `ARTIFACT_DELETED` events and
//! artifact rows (V1_DEVELOPMENT.md §22, §39).
//!
//! The scan answers one question — *what does the project's file set look like now,
//! compared with what we recorded?* — and it answers it without ever storing a body:
//! only the relative path and a whitespace-normalised content hash leave this module
//! (§22). A file's contents are read, hashed and dropped.
//!
//! Division of labour with [`crate::git_facts`]:
//!
//! * git proves *that* a path was touched in a commit and gives it a placeholder hash
//!   (`git:{sha}`);
//! * the scan gives the path a real content baseline. Because a placeholder is not a
//!   baseline, the first scan that sees such a row reports [`ScanKind::Baseline`] rather
//!   than inventing a modification — and still backs it with an event, so the hash is
//!   reproducible from the log instead of living only in a cache row (§18).
//!
//! What is deliberately *not* here: a changed file never completes a task, and it never
//! rewrites a decision (§39). Change is evidence; judgement belongs to a confirmed event.
//!
//! Skips are counted and reported, not swallowed: files over [`DEFAULT_MAX_BYTES`] and
//! files whose first bytes contain a NUL (binary) are excluded from hashing, because a
//! memory tool that stalls on a 400 MB asset is worse than one that misses it.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use vibe_core::error::{CoreError, Result};
use vibe_core::event_store::{EventDraft, EventStore};
use vibe_core::ports::{EntityStore, Repositories};
use vibe_domain::{
    event_catalog, ActorType, Artifact, EntityKind, EvidenceType, Source, SourceType,
};

use crate::pipeline::{artifact_kind_for, artifact_name_for, ensure_evidence, link_evidence};

/// Cap on a single file's size, in bytes (1 MiB). Larger files are counted in
/// `FileScanReport::skipped_large` and not hashed; §22 keeps the database small and a
/// capture pass fast, and a deployment that wants big text artifacts injects a bigger cap.
pub const DEFAULT_MAX_BYTES: u64 = 1024 * 1024;

/// How many leading bytes are inspected for a NUL before a file is called binary.
pub const BINARY_SNIFF_BYTES: usize = 8 * 1024;

/// Length of a `content_hash` digest — used to tell a real hash from a placeholder.
const HASH_LEN: usize = 64;

/// What a scan decided about one path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanKind {
    /// Recorded for the first time.
    Created,
    /// Recorded before, with a different content hash now.
    Modified,
    /// Recorded with a placeholder hash (a git capture), given a real baseline. Not a
    /// claim that the file changed — nobody has a previous content hash to compare with.
    Baseline,
    /// Recorded before, now absent from the scanned root.
    Deleted,
}

impl ScanKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ScanKind::Created => "created",
            ScanKind::Modified => "modified",
            ScanKind::Baseline => "baseline",
            ScanKind::Deleted => "deleted",
        }
    }
}

/// One classification the scan made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanChange {
    /// Path relative to the injected root, always with `/` separators — §22 forbids
    /// putting a machine-specific absolute path in the database.
    pub path: String,
    pub kind: ScanKind,
    /// The new content hash; `None` for a deletion.
    pub hash: Option<String>,
}

/// Everything one scan decided.
#[derive(Debug, Clone, Default)]
pub struct FileScanReport {
    /// The source row these events were captured under.
    pub source_id: String,
    /// Files read and hashed.
    pub scanned: usize,
    /// Files left alone because they exceed the size cap.
    pub skipped_large: usize,
    /// Files left alone because they look binary.
    pub skipped_binary: usize,
    /// Files whose stored hash already matches, so nothing was written (§52).
    pub unchanged: usize,
    pub changes: Vec<ScanChange>,
    pub events_appended: usize,
    pub events_duplicate: usize,
    pub artifacts_recorded: usize,
    /// Paths that could not be read (permissions, a vanished file); the scan continues.
    pub errors: Vec<(String, String)>,
}

impl FileScanReport {
    /// The changes of one kind, in scan order.
    pub fn by_kind(&self, kind: ScanKind) -> Vec<&ScanChange> {
        self.changes.iter().filter(|c| c.kind == kind).collect()
    }

    /// Relative paths that changed, sorted — convenient for a surface or an assertion.
    pub fn changed_paths(&self) -> Vec<String> {
        let mut paths: Vec<String> = self.changes.iter().map(|c| c.path.clone()).collect();
        paths.sort();
        paths
    }

    /// Nothing happened, which is the expected steady state for an unchanged tree.
    pub fn is_quiet(&self) -> bool {
        self.changes.is_empty() && self.events_appended == 0
    }
}

/// Walks an injected root and reconciles it with the recorded artifact set.
pub struct FileScan {
    repos: Repositories,
    events: EventStore,
    root: PathBuf,
    max_bytes: u64,
    /// The scan's `Source` external id. Defaults to the injected root, so two roots are
    /// two sources; set it when a checkout moves on disk and must stay one source (§38).
    source_id_key: Option<String>,
}

impl FileScan {
    pub fn new(repos: Repositories, root: impl Into<PathBuf>) -> Self {
        Self {
            events: EventStore::new(repos.clone()),
            repos,
            root: root.into(),
            max_bytes: DEFAULT_MAX_BYTES,
            source_id_key: None,
        }
    }

    /// Override the documented size cap.
    pub fn with_max_bytes(mut self, max_bytes: u64) -> Self {
        if max_bytes > 0 {
            self.max_bytes = max_bytes;
        }
        self
    }

    /// A location-independent identity for this scan's source row.
    pub fn with_source_key(mut self, key: &str) -> Self {
        self.source_id_key = Some(key.to_string());
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn repositories(&self) -> &Repositories {
        &self.repos
    }

    /// Reconcile the tree against what is recorded and append what differs.
    pub fn run(&self, project_id: &str) -> Result<FileScanReport> {
        if !self.root.is_dir() {
            return Err(CoreError::NotFound {
                what: format!("scan root {}", self.root.display()),
            });
        }
        let key = self
            .source_id_key
            .clone()
            .unwrap_or_else(|| self.root.display().to_string());
        let source = self.file_source(project_id, &key)?;
        let mut report = FileScanReport {
            source_id: source.id.clone(),
            ..FileScanReport::default()
        };

        let mut seen: HashSet<String> = HashSet::new();
        for path in self.walk()? {
            let rel = relative(&self.root, &path);
            // Every file present on disk is "seen", hashed or not, otherwise a skipped
            // large file would be reported as deleted on the next run.
            seen.insert(rel.clone());

            let size = fs::metadata(&path)
                .map(|m| m.len())
                .map_err(|e| CoreError::Io(format!("{}: {e}", path.display())))?;
            if size > self.max_bytes {
                report.skipped_large += 1;
                continue;
            }
            let bytes = match fs::read(&path) {
                Ok(bytes) => bytes,
                Err(e) => {
                    report.errors.push((rel, format!("{e}")));
                    continue;
                }
            };
            if is_binary(&bytes) {
                report.skipped_binary += 1;
                continue;
            }
            // Whitespace-normalised, exactly like message dedup L2: reflowing a file is
            // not a content change (§52).
            let hash = vibe_domain::content_hash(&String::from_utf8_lossy(&bytes));
            report.scanned += 1;
            let existing = self.repos.artifacts.find_artifact_by_path(project_id, &rel)?;
            let kind = match existing.as_ref() {
                None => ScanKind::Created,
                Some(row) if row.deleted => ScanKind::Created,
                // A git placeholder is not a content baseline, so agreeing with it is
                // impossible: establish one instead of claiming a change.
                Some(row) if !is_content_hash(row.hash.as_deref()) => ScanKind::Baseline,
                Some(row) if row.hash.as_deref() == Some(hash.as_str()) => {
                    report.unchanged += 1;
                    continue;
                }
                Some(_) => ScanKind::Modified,
            };
            report.changes.push(ScanChange {
                path: rel.clone(),
                kind,
                hash: Some(hash.clone()),
            });
            self.record(project_id, &source.id, &rel, &hash, kind, &mut report)?;
        }

        // Deletions: recorded artifacts that are gone from the tree.
        for artifact in self.repos.artifacts.list_artifacts(project_id)? {
            let Some(stored) = artifact.path.as_deref() else {
                continue;
            };
            // Only paths this layer could have written — a relative path. Anything
            // absolute belongs to some other root and is not this scan's business.
            if !is_relative(stored) || artifact.deleted || seen.contains(stored) {
                continue;
            }
            report.changes.push(ScanChange {
                path: stored.to_string(),
                kind: ScanKind::Deleted,
                hash: None,
            });
            self.record_deletion(project_id, &source.id, &artifact, &mut report)?;
        }
        Ok(report)
    }

    /// Sorted, deterministic walk: hidden entries (which is what keeps `.git`, `.env`
    /// caches and editor state out) and symlinks are not followed.
    fn walk(&self) -> Result<Vec<PathBuf>> {
        let mut out = Vec::new();
        let mut stack = vec![self.root.clone()];
        while let Some(dir) = stack.pop() {
            let entries = fs::read_dir(&dir).map_err(|e| {
                CoreError::Io(format!("cannot read {}: {e}", dir.display()))
            })?;
            for entry in entries {
                let entry = entry.map_err(|e| CoreError::Io(format!("{}: {e}", dir.display())))?;
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with('.') {
                    continue;
                }
                let path = entry.path();
                let file_type = entry.file_type().map_err(|e| {
                    CoreError::Io(format!("{}: {e}", path.display()))
                })?;
                if file_type.is_symlink() {
                    continue;
                }
                if file_type.is_dir() {
                    stack.push(path);
                } else if file_type.is_file() {
                    out.push(path);
                }
            }
        }
        out.sort();
        Ok(out)
    }

    /// Append the change event and reconcile the artifact row.
    fn record(
        &self,
        project_id: &str,
        source_id: &str,
        rel: &str,
        hash: &str,
        kind: ScanKind,
        report: &mut FileScanReport,
    ) -> Result<()> {
        let draft = EventDraft::new(project_id, event_catalog::FILE_CHANGED)
            // Observed by the tool, not asserted by a person: a fact, never a conclusion
            // about what the change means (§39).
            .actor(ActorType::System)
            .subject("artifact", rel)
            .payload(serde_json::json!({
                "path": rel,
                "hash": hash,
                "change": kind.as_str(),
                "name": artifact_name_for(rel),
                "kind": serde_json::to_value(artifact_kind_for(rel))
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_else(|| "source_file".to_string()),
            }));
        let mut event = draft.into_event();
        event.source_id = Some(source_id.to_string());
        event.evidence_refs = vec![rel.to_string()];
        let outcome = self.events.append_event(event)?;
        if !outcome.inserted {
            report.events_duplicate += 1;
            // The log already carries this exact change, so the caches do too.
            return Ok(());
        }
        report.events_appended += 1;

        let evidence_id = ensure_evidence(
            &self.repos,
            project_id,
            EvidenceType::File,
            rel,
            &format!("file {rel}"),
        )?;
        link_evidence(&self.repos, &evidence_id, EntityKind::Artifact, rel)?;

        let existing = self.repos.artifacts.find_artifact_by_path(project_id, rel)?;
        let mut artifact = existing.unwrap_or_else(|| Artifact {
            id: rel.to_string(),
            project_id: project_id.to_string(),
            name: artifact_name_for(rel),
            kind: artifact_kind_for(rel),
            path: Some(rel.to_string()),
            hash: None,
            version: 0,
            relationships: Vec::new(),
            deleted: false,
            redesign_count: 0,
            updated_at: None,
        });
        artifact.project_id = project_id.to_string();
        artifact.path = Some(rel.to_string());
        artifact.hash = Some(hash.to_string());
        artifact.deleted = false;
        artifact.updated_at = Some(vibe_domain::now());
        // One version per recorded change, which is also what the reducer's
        // `FILE_CHANGED` branch counts (§22).
        artifact.version = artifact.version.saturating_add(1);
        self.repos.artifacts.upsert_artifact(&artifact)?;
        self.repos
            .entities
            .put_entity(EntityKind::Artifact, &artifact, Some("active"))?;
        report.artifacts_recorded += 1;
        Ok(())
    }

    /// A vanished file gets `ARTIFACT_DELETED`, not a silent row edit: `FILE_CHANGED`
    /// folds into an artifact upsert, which would mark the file present again (§18).
    fn record_deletion(
        &self,
        project_id: &str,
        source_id: &str,
        artifact: &Artifact,
        report: &mut FileScanReport,
    ) -> Result<()> {
        let rel = artifact.path.clone().unwrap_or_else(|| artifact.id.clone());
        let draft = EventDraft::new(project_id, event_catalog::ARTIFACT_DELETED)
            .actor(ActorType::System)
            .subject("artifact", &artifact.id)
            .payload(serde_json::json!({ "path": rel }));
        let mut event = draft.into_event();
        event.source_id = Some(source_id.to_string());
        event.evidence_refs = vec![rel.clone()];
        let outcome = self.events.append_event(event)?;
        if outcome.inserted {
            report.events_appended += 1;
            let evidence_id = ensure_evidence(
                &self.repos,
                project_id,
                EvidenceType::File,
                &rel,
                &format!("file {rel} removed"),
            )?;
            link_evidence(&self.repos, &evidence_id, EntityKind::Artifact, &artifact.id)?;
        } else {
            report.events_duplicate += 1;
        }
        let mut removed = artifact.clone();
        removed.deleted = true;
        removed.updated_at = Some(vibe_domain::now());
        self.repos.artifacts.upsert_artifact(&removed)?;
        self.repos.entities.put_entity(
            EntityKind::Artifact,
            &removed,
            Some("deleted"),
        )?;
        report.artifacts_recorded += 1;
        Ok(())
    }

    fn file_source(&self, project_id: &str, key: &str) -> Result<Source> {
        let existing = self
            .repos
            .conversations
            .find_source_by_external_id(project_id, SourceType::File.as_str(), key)?;
        let source = match existing {
            Some(source) => source,
            None => Source {
                id: vibe_domain::new_id(),
                project_id: project_id.to_string(),
                r#type: SourceType::File,
                external_id: Some(key.to_string()),
                display_name: key.to_string(),
                metadata: serde_json::json!({ "adapter": "file-scan" }),
            },
        };
        self.repos.conversations.upsert_source(&source)?;
        Ok(source)
    }
}

/// Forward-slash path relative to `root`, so a stored artifact path means the same thing
/// on every machine (§22).
fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// A `content_hash` output: 64 hex chars. Anything else (`git:{sha}` from the commit
/// capture, a truncated value, `None`) is not a content baseline.
fn is_content_hash(value: Option<&str>) -> bool {
    value.is_some_and(|v| {
        v.len() == HASH_LEN && v.chars().all(|c| c.is_ascii_hexdigit())
    })
}

/// Paths this layer owns: relative, forward-slashed, no Windows drive prefix.
fn is_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.starts_with("..")
        && !path.contains('\\')
        && !(path.len() > 2 && path.as_bytes()[1] == b':')
}

/// Binary sniff: a NUL anywhere in the first [`BINARY_SNIFF_BYTES`]. Cheap, and good
/// enough for "is this text a memory tool should read".
fn is_binary(bytes: &[u8]) -> bool {
    let window = bytes.len().min(BINARY_SNIFF_BYTES);
    bytes[..window].contains(&0)
}
