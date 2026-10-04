//! guard-engine — the anti-drift half of the workbench.
//!
//! [`DriftEngine`] runs the V1 rules (V1_DEVELOPMENT.md §26) over a projected
//! [`vibe_domain::ProjectState`], persists new [`vibe_domain::DriftAlert`]s through the
//! [`vibe_core::ports::Repositories`] ports and records a `DRIFT_DETECTED` event for each.
//! [`Inbox`] is the single queue where the user judges what the rules could not decide
//! (§29). Alerts are prompts: nothing here blocks development, and nothing here may
//! rewrite the Charter, Scope or Architecture baseline (PROJECT_MASTER §8.6, §19 禁止 5).

pub mod engine;
pub mod inbox;
pub mod text;

pub use engine::{
    status_slug, DriftAction, DriftConfig, DriftEngine, DEFAULT_STALE_DAYS, REDESIGN_LIMIT,
    SUPERSEDE_CHAIN_LIMIT,
};
pub use inbox::{Inbox, InboxAction, InboxRef};
