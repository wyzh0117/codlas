//! The security gate of the workbench (V1_DEVELOPMENT.md §53, §54).
//!
//! Three concerns live here and nowhere else:
//!
//! ```text
//! compiled pack items ──▶ PolicyFilter ──▶ markdown assembly (context-engine)
//!                            │
//!                            └── SecretScanner: detect + redact, pure
//!
//! secret values ──▶ SecretStore (local file / macOS keychain) ──▶ `secret_ref` only
//! ```
//!
//! Invariants:
//!   * detection and redaction are pure functions over `&str`; they never touch a
//!     clock, a file or a network, so every rule is testable without a project;
//!   * nothing in this crate ever prints or stores a secret value. A `Redaction`
//!     carries a category and a one-way fingerprint, which is what §54 lets the
//!     audit log record;
//!   * the gate is the last line of defence, not a courtesy: a cloud target the
//!     project policy forbids leaves with *no* items rather than a smaller pack.

pub mod policy;
pub mod refs;
pub mod secrets;
pub mod store;

pub use policy::{DroppedItem, PolicyFilter, PolicyOutcome};
pub use refs::{
    is_cloud_target, ref_kind, split_body, target_class, TargetClass, BODY_MARKER,
};
pub use secrets::{
    marker, API_KEY, AWS_ACCESS_KEY_ID, FILE_BODY, GITHUB_TOKEN, JWT, PASSWORD, PRIVATE_KEY,
    RAW_CONVERSATION, SECRET, SLACK_TOKEN, TOKEN,
};
pub use secrets::{Redaction, SecretScanner, SecretSpan};
pub use store::{
    KeychainConsent, KeychainSecretStore, LocalSecretStore, SecretMetadata, SecretStore,
};
