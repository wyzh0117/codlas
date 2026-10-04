//! §53 gate: one case per policy flag, plus the refusal shape the flags produce.
//! The gate only ever takes content away — these tests assert both what is gone
//! and what must still be there.

use vibe_domain::{ContextPackItem, ContextSection, ProjectPolicy};
use vibe_security::refs::BODY_MARKER;
use vibe_security::{PolicyFilter, FILE_BODY, RAW_CONVERSATION};

fn item(section: ContextSection, source_ref: &str, content: &str) -> ContextPackItem {
    ContextPackItem {
        section,
        source_ref: source_ref.to_string(),
        content: content.to_string(),
        priority: 3,
        token_cost: 0,
        redacted: false,
    }
}

fn candidates() -> Vec<ContextPackItem> {
    vec![
        item(
            ContextSection::OriginalIntent,
            "charter:c-1",
            "keep project continuity local",
        ),
        item(
            ContextSection::ActiveDecisions,
            "decision:D-011",
            "keep the repository port; api_key = qwertyuiopasdfghjklzxcvbnm123456",
        ),
        item(
            ContextSection::RelevantArtifacts,
            "artifact:a-9",
            format!(
                "crates/core/src/ports.rs (source_file, v3, sha256:aa11){BODY_MARKER}pub trait EntityStore {{ .. }}",
            )
            .as_str(),
        ),
        item(
            ContextSection::RecentEvidence,
            "message:m-7",
            "user: 为什么同步还是重复？ assistant: 因为 cursor 没有持久化",
        ),
    ]
}

fn policy() -> ProjectPolicy {
    ProjectPolicy {
        local_only: false,
        allow_cloud_model: true,
        never_send_files: false,
        never_send_raw_conversation: false,
        redact_secrets: true,
    }
}

const SECRET_FRAGMENT: &str = "qwertyuiopasdfghjklzxcvbnm";

#[test]
fn default_policy_only_redacts_the_secret() {
    let outcome = PolicyFilter::new().apply(&policy(), Some("chatgpt-web"), candidates());
    assert!(outcome.dropped.is_empty());
    assert_eq!(outcome.items.len(), 4);
    assert!(outcome.cloud_allowed);
    let decision = &outcome.items[1];
    assert!(decision.redacted);
    assert!(!decision.content.contains(SECRET_FRAGMENT));
    assert_eq!(
        decision.content,
        // The keyword survives so the reader knows what was hidden; only the
        // value is replaced.
        "keep the repository port; api_key = [REDACTED:api_key]"
    );
    assert_eq!(outcome.redactions[0].category, "api_key");
    assert_eq!(outcome.redacted_refs, vec!["decision:D-011#api_key".to_string()]);
    // Untouched items stay untouched.
    assert!(!outcome.items[0].redacted);
    assert!(!outcome.items[2].redacted);
}

#[test]
fn redact_secrets_off_leaves_the_value_where_it_is() {
    let mut off = policy();
    off.redact_secrets = false;
    let outcome = PolicyFilter::new().apply(&off, None, candidates());
    assert!(outcome.redactions.is_empty());
    assert!(outcome.items[1].content.contains(SECRET_FRAGMENT));
    assert!(outcome.items.iter().all(|i| !i.redacted));
}

#[test]
fn never_send_files_keeps_the_reference_and_drops_the_body() {
    let mut only = policy();
    only.never_send_files = true;
    let outcome = PolicyFilter::new().apply(&only, None, candidates());
    let artifact = &outcome.items[2];
    assert_eq!(artifact.content, "crates/core/src/ports.rs (source_file, v3, sha256:aa11)");
    assert!(artifact.redacted, "a stripped body is an observable redaction");
    assert!(artifact.content.contains("sha256:aa11"), "path + hash must survive");
    let category = outcome
        .redactions
        .iter()
        .find(|r| r.category == FILE_BODY)
        .expect("file body redaction recorded");
    assert_eq!(category.count, 1);
    assert!(outcome.redacted_refs.contains(&"artifact:a-9#file_body".to_string()));
    // Only artifact items are candidates for body stripping.
    assert_eq!(outcome.items.len(), 4, "nothing was dropped");
}

#[test]
fn never_send_raw_conversation_withholds_the_body_and_keeps_the_pointer() {
    let mut quiet = policy();
    quiet.never_send_raw_conversation = true;
    let outcome = PolicyFilter::new().apply(&quiet, None, candidates());
    let message = &outcome.items[3];
    assert!(message.redacted);
    assert!(!message.content.contains("为什么同步还是重复"), "{message:?}");
    assert!(message.content.contains("message:m-7"), "the pointer survives");
    assert!(outcome
        .redactions
        .iter()
        .any(|r| r.category == RAW_CONVERSATION));
    assert!(outcome
        .redacted_refs
        .contains(&"message:m-7#raw_conversation".to_string()));
    // Other sections are not conversation excerpts and stay whole.
    assert_eq!(outcome.items[0].content, "keep project continuity local");
}

#[test]
fn local_only_refuses_a_cloud_target_instead_of_shrinking_the_pack() {
    let mut strict = policy();
    strict.local_only = true;
    let outcome = PolicyFilter::new().apply(&strict, Some("chatgpt-web"), candidates());
    assert!(!outcome.cloud_allowed);
    assert!(outcome.items.is_empty(), "a refusal is not a smaller pack");
    assert_eq!(outcome.dropped.len(), 4);
    assert!(outcome.dropped.iter().all(|d| d.reason.contains("local_only")));
    assert!(outcome
        .dropped
        .iter()
        .any(|d| d.source_ref == "charter:c-1" && d.reason.contains("chatgpt-web")));
}

#[test]
fn local_only_still_builds_for_a_local_target() {
    let mut strict = policy();
    strict.local_only = true;
    let outcome = PolicyFilter::new().apply(&strict, Some("generic-cli"), candidates());
    assert_eq!(outcome.items.len(), 4);
    assert!(!outcome.cloud_allowed);
    assert!(outcome.dropped.is_empty());
    // Secrets are redacted for local targets too: the gate is not about the wire.
    assert!(outcome.items[1].redacted);
}

#[test]
fn allow_cloud_model_false_refuses_cloud_targets_too() {
    let mut no_cloud = policy();
    no_cloud.allow_cloud_model = false;
    let outcome = PolicyFilter::new().apply(&no_cloud, Some("hosted-model"), candidates());
    assert!(!outcome.cloud_allowed);
    assert!(outcome.items.is_empty());
    assert!(outcome
        .dropped
        .iter()
        .all(|d| d.reason.contains("allowCloudModel=false")));
}

#[test]
fn an_unknown_target_counts_as_cloud_not_local() {
    let mut strict = policy();
    strict.local_only = true;
    let outcome = PolicyFilter::new().apply(&strict, Some("mystery-agent"), candidates());
    assert!(outcome.items.is_empty(), "fail closed on a name we do not know");
}

#[test]
fn no_target_means_nothing_leaves_so_nothing_is_dropped() {
    let mut strict = policy();
    strict.local_only = true;
    let outcome = PolicyFilter::new().apply(&strict, None, candidates());
    assert_eq!(outcome.items.len(), 4);
    assert!(outcome.dropped.is_empty());
}

#[test]
fn a_body_emptied_by_the_policy_is_dropped_with_a_reason() {
    let mut quiet = policy();
    quiet.never_send_raw_conversation = true;
    let items = vec![item(ContextSection::RecentEvidence, "message:m-1", "")];
    let outcome = PolicyFilter::new().apply(&quiet, None, items);
    assert!(outcome.items.is_empty());
    assert_eq!(outcome.dropped[0].source_ref, "message:m-1");
}

#[test]
fn redaction_entries_never_carry_a_value() {
    let outcome = PolicyFilter::new().apply(&policy(), None, candidates());
    let shown = format!("{:?}", outcome.redactions);
    assert!(!shown.contains(SECRET_FRAGMENT), "{shown}");
    assert!(!shown.contains("aa11"), "fingerprints only");
}
