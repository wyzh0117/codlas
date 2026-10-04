//! search — structured, full-text and relation retrieval (V1_DEVELOPMENT.md §40, §60).
//!
//! This crate composes three ports — `SearchIndex` for L2, `EntityRepository` for L1 and
//! L3, and the event / conversation / artifact reads behind them — and knows nothing about
//! how any of them is stored. There is no SQL here and no database engine named: that is
//! §6's boundary, and it is the reason the same service runs against an in-memory fake in
//! tests and against SQLite in the app without either side knowing.
//!
//! ```text
//! find(query)
//!   ├─ L1 structured ── id / kind / status / title, read from the record shapes
//!   ├─ L2 full text ─── SearchIndex::search, one bm25 query, already ranked
//!   └─ L3 relation ───── one hop along entity_links from what L1/L2 returned
//!        └── merged by (kind, id): a record appears once, under the best layer that
//!            found it, so "which layer answered?" stays visible (§40).
//! ```
//!
//! Two rules shape the ranking. Cheap-first (§60): L1 and L2 are the cheap queries, L3 only
//! expands what they already found, and nothing re-scans the corpus per result. And the
//! layer bands never overlap (§40): a structured id match outranks any text hit, which
//! outranks any relation, so a link can never push an unrelated record to the top.

pub mod catalog;
pub mod service;

/// Default page size when a caller passes `0`.
pub const DEFAULT_LIMIT: usize = 25;
/// How much of a body a snippet shows.
pub const SNIPPET_CHARS: usize = 160;
/// How many top hits L3 expands from — the relation layer is a widening, not a second
/// full scan (§60).
pub const RELATION_SEEDS: usize = 5;

pub use catalog::{
    excerpt, ALL_KINDS, KIND_AEP, KIND_ARTIFACT, KIND_DECISION, KIND_DRIFT, KIND_EVENT,
    KIND_ISSUE, KIND_MESSAGE, KIND_MILESTONE, KIND_PROPOSAL, KIND_TASK, Record,
};
pub use service::{
    Found, SearchLayer, SearchService, StructuredQuery, SCORE_BROWSE, SCORE_EXACT_ID,
    SCORE_EXACT_TITLE, SCORE_FULL_TEXT, SCORE_RELATION, SCORE_TITLE_MATCH,
};

/// Re-exported so a caller can build a `SearchFilters` or read a `SearchHit` without
/// naming the port crate directly.
pub use vibe_core::ports::{SearchFilters, SearchHit};
