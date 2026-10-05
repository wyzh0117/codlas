//! V1-E desktop shell — the workbench's own window (§61 Stage E).
//!
//! The shell is a reader, not a second brain. Every view in [`views`] is compiled by the same
//! services the `wp` CLI calls, over the same ports, on the same database file. The shell folds
//! no events, judges no evidence and writes no state (禁止 4, 禁止 7, ADR-020); the single write
//! it can issue is the user's own confirm-to-lock decision (§7.4).

pub mod views;

// The `#[tauri::command]` wrappers are compiled into the binary only: they exist to hand
// these views to a webview, and `tests/views.rs` drives the builders directly.
pub mod workspace;

pub use workspace::Shell;
