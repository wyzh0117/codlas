//! The §53 policy gate, applied to compiled pack items *before* any markdown is
//! assembled.
//!
//! The gate may only ever take content away. It never adds a fact, never rewrites
//! prose beyond the markers it inserts, and never downgrades a refusal into a
//! smaller pack: a cloud target the policy forbids leaves with nothing at all.
//! Every removal is reported so the caller can audit it (§54).

use serde::{Deserialize, Serialize};
use vibe_domain::{ContextPackItem, ContextSection, ProjectPolicy};

use crate::refs::{ref_kind, split_body, has_body, is_cloud_target, MESSAGE_REF};
use crate::secrets::{record, SecretScanner, Redaction, FILE_BODY, RAW_CONVERSATION};

/// Runs the project policy over a candidate pack.
#[derive(Debug, Clone, Copy, Default)]
pub struct PolicyFilter;

impl PolicyFilter {
    pub fn new() -> Self {
        Self
    }

    /// Returns the surviving items, redacted in place where the policy requires it,
    /// plus the dropped `source_ref`s with reasons so the caller can audit.
    pub fn apply(
        &self,
        policy: &ProjectPolicy,
        target: Option<&str>,
        items: Vec<ContextPackItem>,
    ) -> PolicyOutcome {
        let cloud_allowed = !policy.local_only && policy.allow_cloud_model;
        let scanner = SecretScanner::new();

        let cloud_target = target.map(is_cloud_target).unwrap_or(false);
        if cloud_target && !cloud_allowed {
            let reason = if policy.local_only {
                format!(
                    "project policy is local_only: nothing may be sent to cloud target \
                     '{target}'; §53 refuses instead of downgrading",
                    target = target.unwrap_or_default()
                )
            } else {
                format!(
                    "project policy has allowCloudModel=false: cloud target '{}' is refused",
                    target.unwrap_or_default()
                )
            };
            let dropped = items
                .iter()
                .map(|item| DroppedItem {
                    source_ref: item.source_ref.clone(),
                    reason: reason.clone(),
                })
                .collect();
            return PolicyOutcome {
                items: Vec::new(),
                dropped,
                redactions: Vec::new(),
                redacted_refs: Vec::new(),
                cloud_allowed,
            };
        }

        let mut outcome = PolicyOutcome {
            items: Vec::with_capacity(items.len()),
            dropped: Vec::new(),
            redactions: Vec::new(),
            redacted_refs: Vec::new(),
            cloud_allowed,
        };

        for mut item in items {
            // §53 never_send_raw_conversation: the message id stays, the transcript goes.
            if policy.never_send_raw_conversation
                && ref_kind(&item.source_ref) == MESSAGE_REF
                && !item.content.is_empty()
            {
                record(
                    &mut outcome.redactions,
                    RAW_CONVERSATION,
                    &vibe_domain::content_hash(&item.content),
                );
                outcome
                    .redacted_refs
                    .push(format!("{}#{RAW_CONVERSATION}", item.source_ref));
                item.content = format!(
                    "conversation content withheld by project policy (§53 \
                     never_sendRawConversation) — the message itself is still available \
                     locally as {id}",
                    id = item.source_ref
                );
                item.redacted = true;
            }

            // §53 never_send_files: keep the path + hash reference, cut the body.
            if policy.never_send_files
                && item.section == ContextSection::RelevantArtifacts
                && has_body(&item.content)
            {
                let (reference, body) = split_body(&item.content);
                if let Some(body) = body {
                    record(
                        &mut outcome.redactions,
                        FILE_BODY,
                        &vibe_domain::content_hash(body),
                    );
                    outcome
                        .redacted_refs
                        .push(format!("{}#{FILE_BODY}", item.source_ref));
                }
                item.content = reference.to_string();
                item.redacted = true;
            }

            // §53 redact_secrets (default on) over every remaining body.
            if policy.redact_secrets {
                let (clean, hits) = scanner.redact(&item.content);
                if !hits.is_empty() {
                    for hit in hits {
                        record(&mut outcome.redactions, hit.category, &hit.fingerprint);
                        outcome
                            .redacted_refs
                            .push(format!("{}#{}", item.source_ref, hit.category));
                    }
                    item.content = clean;
                    item.redacted = true;
                }
            }

            if item.content.trim().is_empty() {
                outcome.dropped.push(DroppedItem {
                    source_ref: item.source_ref.clone(),
                    reason: "item was empty after the policy filter".into(),
                });
                continue;
            }
            outcome.items.push(item);
        }
        outcome
    }
}

/// What survived, what was withheld, and what was redacted.
#[derive(Debug, Clone, Default)]
pub struct PolicyOutcome {
    pub items: Vec<ContextPackItem>,
    pub dropped: Vec<DroppedItem>,
    /// Aggregated across the whole pack: category + fingerprint + count. A
    /// fingerprint is one-way, so this list is safe to store and to show.
    pub redactions: Vec<Redaction>,
    /// `<source_ref>#<category>` per altered item, in item order. The pack itself
    /// only carries a boolean per item, so this is what the audit row records (§54).
    pub redacted_refs: Vec<String>,
    /// `!local_only && allowCloudModel`. Callers refuse a cloud target when false
    /// rather than compiling a smaller pack.
    pub cloud_allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DroppedItem {
    pub source_ref: String,
    pub reason: String,
}
