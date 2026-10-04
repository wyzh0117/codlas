//! The small vocabulary the compiler and the gate agree on (§53).
//!
//! The gate cannot see domain structs, only a compiled item's `source_ref` and its
//! content. These two things are enough to decide what may travel, as long as both
//! sides spell them the same way — so the spelling lives here, next to the rule
//! that reads it.

/// Every `source_ref` the compiler emits is `<kind>:<id>`: `issue:I-018`,
/// `message:m-1`, `artifact:a-9`. Returns the kind part.
pub fn ref_kind(source_ref: &str) -> &str {
    source_ref.split(':').next().unwrap_or(source_ref)
}

/// Kinds the gate has rules about.
pub const MESSAGE_REF: &str = "message";
pub const EVENT_REF: &str = "event";
pub const ARTIFACT_REF: &str = "artifact";

/// Separator marking the start of file content inside an item body. A compiled
/// reference line reads `path (kind, v3, sha256:…)`, and anything beyond the
/// marker is content from the file itself.
pub const BODY_MARKER: &str = "; body: ";

/// Split a body into its reference part and, when present, the file content the
/// `never_send_files` policy is not allowed to move.
pub fn split_body(content: &str) -> (&str, Option<&str>) {
    match content.find(BODY_MARKER) {
        Some(at) => (&content[..at], Some(&content[at + BODY_MARKER.len()..])),
        None => (content, None),
    }
}

/// True when the content carries file material that can be cut away.
pub fn has_body(content: &str) -> bool {
    content.contains(BODY_MARKER)
}

/// Which side of the machine a target lives on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetClass {
    /// Runs on this machine, or is a manual paste the user performs themselves.
    Local,
    /// Anything that could leave the machine: a hosted model, a web app, a remote
    /// agent, or a name this build does not recognise.
    Cloud,
}

/// Names this build is willing to treat as local. Deliberately a *deny-by-default*
/// list: §7.6 forbids hardcoding vendor identities into the core, so the trust
/// direction only ever recognises generic local hints (`cli`, `local`, `manual`,
/// `mcp`, `desktop`, `ollama`). An unknown name is a cloud target until proven
/// otherwise, which is the safe failure for a policy gate.
pub fn target_class(name: &str) -> TargetClass {
    let hint = name.to_ascii_lowercase();
    const LOCAL_HINTS: [&str; 8] = [
        "cli", "local", "manual", "mcp", "desktop", "shell", "terminal", "ollama",
    ];
    if LOCAL_HINTS.iter().any(|token| hint.contains(token)) {
        TargetClass::Local
    } else {
        TargetClass::Cloud
    }
}

pub fn is_cloud_target(name: &str) -> bool {
    matches!(target_class(name), TargetClass::Cloud)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_targets_are_cloud() {
        assert!(is_cloud_target("some-agent-i-have-never-seen"));
        assert_eq!(target_class("generic-cli"), TargetClass::Local);
        assert_eq!(target_class("MANUAL"), TargetClass::Local);
    }

    #[test]
    fn bodies_split_at_the_marker() {
        let line = format!("src/sync.ts (source_file, v2, sha256:aa){BODY_MARKER}reconnect loop");
        let (reference, body) = split_body(&line);
        assert_eq!(reference, "src/sync.ts (source_file, v2, sha256:aa)");
        assert_eq!(body, Some("reconnect loop"));
        assert!(!has_body("src/sync.ts"));
    }

    #[test]
    fn ref_kind_reads_the_prefix() {
        assert_eq!(ref_kind("message:m-1"), "message");
        assert_eq!(ref_kind("bare"), "bare");
    }
}
