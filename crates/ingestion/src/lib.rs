//! ingestion — the outermost capture layer of the workbench (V1_DEVELOPMENT.md §36-§38, §51).
//!
//! This is the only layer allowed to touch a vendor's files or run a subprocess, and it
//! does so through injected data only: a [`std::path::PathBuf`] root here, a
//! [`CommandRunner`] there. Nothing below it knows a platform exists — adapters are
//! *registered* by id and capability (PROJECT_MASTER §7.6), and vendor names are
//! [`vibe_domain::SourceType`] data, never a branch target in the pipeline.
//!
//! The layer boundary is deliberate:
//!
//! ```text
//! adapter transcript ──▶ Normalizer ──▶ Source / Conversation / Message   (§12-§14)
//!                                             │
//!                                             ├──▶ SignalJudge ──▶ EventStore        (§27, §15)
//!                                             └──▶ SyncCursor (per source)          (§51)
//! git log / file tree ─▶ GitFacts / FileScan ─▶ GIT_COMMIT / FILE_CHANGED + Artifact (§39)
//! ```
//!
//! Two rules hold everywhere in here. Capture is *idempotent*: re-running an adapter
//! yields no duplicate message and no duplicate event, and the stored cursor only moves
//! once (§51, §52). And captured material is *evidence, not conclusion* — a commit or a
//! changed file never completes a task; that judgement belongs to a confirmed event the
//! reducer folds (§39, PROJECT_MASTER §19 禁止 9: chat history is not project memory).

pub mod adapter;
pub mod adapters;
pub mod file_scan;
pub mod git_facts;
pub mod near_duplicate;
pub mod normalizer;
pub mod pipeline;

pub use adapter::AgentAdapter;
pub use adapters::{GenericCliAdapter, ManualImportAdapter, TranscriptAdapter};
pub use file_scan::{
    FileScan, FileScanReport, ScanChange, ScanKind, BINARY_SNIFF_BYTES, DEFAULT_MAX_BYTES,
};
pub use git_facts::{
    log_format, CommandOutcome, CommandRunner, CommitRecord, GitFileStatus, GitFacts,
    GitFactsReport, ProcessCommandRunner, RecordedCommits, TouchedFile, WorkspaceSnapshot,
    WorkspaceWatchReport, DEFAULT_MAX_COMMITS, READ_ONLY_GIT,
};
pub use normalizer::{Normalizer, RawTurn};
pub use near_duplicate::{
    scan as scan_near_duplicates, similarity, similarity_upper_bound, tokens, NearDuplicate,
    DEFAULT_THRESHOLD, DEFAULT_WINDOW, NOTE,
};
pub use pipeline::{
    AdapterReport, CapturedCandidate, IngestPipeline, IngestReport, MergeCandidate, report_health,
};
