//! Materialized state for a local-first vibe-coding workbench.
//!
//! The crate has exactly one job: turn the accepted event log back into
//! [`ProjectState`] and render that state as the PROJECT_MAP, deterministically
//! (V1_DEVELOPMENT.md §18-§33, PROJECT_MASTER.md §7.2 / §7.3).
//!
//! ```text
//! accepted events ──projector::fold──▶ ProjectState ──▶ state_hash
//!                                            │
//!                                            ├──▶ ProjectMapCompiler ─▶ PROJECT_MAP
//!                                            └──▶ SnapshotService ─▶ Snapshot / ProjectDiff
//! ```
//!
//! Invariants the rest of the product relies on:
//!   * state is a *projection* of the log; cache rows written here are disposable
//!     and are never read back during a fold;
//!   * the fold is clock-free and model-free, so the same log always yields the same
//!     `state_hash` and the same map bytes;
//!   * charter, scope, baseline and the project row are user-authored versioned
//!     records: hydrated read-only into the state, excluded from the hash;
//!   * an agent's claim of completion is recorded as a claim, never as done
//!     (§19) — confirmation needs a separate, evidenced event.

mod map;
mod payload;
mod projector;
mod reducer;
mod snapshot;

pub use map::{
    next_actions, recent_changes, render_markdown, ProjectMapCompiler, RecentChange,
    EMPTY_PLACEHOLDER, MAP_SECTIONS,
};
pub use reducer::{state_hash, state_hash_of, Reducer, ReplayReport};
pub use snapshot::{
    diff_states, DiffEntry, DiffSection, NextActionsDiff, ProjectDiff, SnapshotService,
};
