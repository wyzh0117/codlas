//! `vibe-context-engine` — the Context Engine layer of the workbench
//! (V1_DEVELOPMENT.md §40-§44, §53-§54, §59, §60).
//!
//! ```text
//! bounded reads (§42 ladder, §59 windows)
//!        └─▶ §53 policy gate  ──▶ §43 token budget  ──▶ §41 markdown  ──▶ §54 audit row
//! ```
//!
//! Two rules hold the design up:
//!
//!   * **Compiled, never remembered.** Nothing in this crate calls a model (§60) and
//!     nothing dumps a raw transcript by default (§59). A pack is assembled from
//!     facts the store already holds, by rules, so the same state always produces the
//!     same bytes.
//!   * **The gate runs before the prose.** Policy is applied to *items*, and only
//!     survivors are turned into markdown, so redaction cannot be missed in a string.
//!     It stays observable afterwards in [`ContextPackItem::redacted`] and
//!     [`ContextPack::redacted_count`], and every build writes an audit row naming
//!     what was read, what was produced, for which agent, and what was withheld (§54).
//!
//! This crate is the read-and-compile half of the layer: it never writes a project
//! fact, never mutates Charter / Scope / Architecture Baseline / entities, and never
//! sends anything. [`ContextCompiler::prepare`] only *prepares* a payload — handing
//! it to an agent is the user's action (§7.8).
//!
//! Ports only: every read goes through [`vibe_core::Repositories`]. No SQLite, no
//! filesystem, no network here.

pub mod budget;
pub mod compiler;
pub mod policy;
pub mod render;
pub mod request;
pub mod retrieve;
pub mod route;
pub mod section;

pub use budget::{budget_with, default_shares, estimate_tokens, share_of, allowance_of, TrimReport, Trimmed};
pub use compiler::{
    ContextCompiler, PlannedItem, COMPONENT, ACTION_BUILD, ACTION_PLAN, ACTION_PREPARE,
    ACTION_REFUSE, DEFAULT_TOTAL_TOKENS,
};
pub use policy::{engine_default, policy_key, POLICY_KEY_PREFIX};
pub use render::{ladder_order, render_markdown, TITLE};
pub use request::{ContextRequest, Excerpt, Selection};
pub use retrieve::{attach_excerpts, gather, Retrieved, DEFAULT_WINDOW, MAX_WINDOW};
pub use route::{recommend, Recommendation, ANALYSIS, CODING, EXECUTION, MANUAL};
pub use section::{tier, SECTION_ORDER};

// The domain types a caller needs to read a pack, re-exported so the crate's public
// surface is complete without forcing every consumer to import vibe-domain.
pub use vibe_domain::{ContextPack, ContextPackItem};
