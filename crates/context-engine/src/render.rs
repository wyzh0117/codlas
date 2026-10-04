//! §41: the shape of a Context Pack. Headings in the §42 ladder order, every line
//! naming where it came from, and nothing else in the document.
//!
//! The renderer is a pure function over already-filtered, already-budgeted items.
//! That ordering is the whole design (§53): by the time text reaches here the policy
//! gate has run, so no rule can be forgotten in prose assembly, and the same items
//! always produce the same bytes — which is what lets a re-render diff be trusted.

use vibe_domain::ContextPackItem;

use crate::section::{rank, SECTION_ORDER};

/// The one heading above the sections (§41).
pub const TITLE: &str = "# Project Context";

/// Assemble the pack body. Deterministic: no clock, no ids, no randomness.
pub fn render_markdown(items: &[ContextPackItem]) -> String {
    let mut out = String::new();
    out.push_str(TITLE);
    out.push('\n');

    let mut redacted = 0usize;
    for section in SECTION_ORDER {
        let group: Vec<&ContextPackItem> =
            items.iter().filter(|item| item.section == section).collect();
        if group.is_empty() {
            continue;
        }
        out.push('\n');
        out.push_str("## ");
        out.push_str(section.heading());
        out.push('\n');
        for item in group {
            if item.redacted {
                redacted += 1;
            }
            push_bullet(&mut out, item);
        }
    }

    if redacted > 0 {
        // §53 redaction is observable in the document as well as in the pack fields.
        out.push('\n');
        out.push_str(&format!(
            "---\n{redacted} item(s) in this pack were redacted or withheld by project policy \
             (§53). The refs they describe are still readable locally."
        ));
        out.push('\n');
    }
    out
}

/// One item, one bullet, one provenance marker. A multi-line body keeps its lines and
/// indents them under the bullet, so the reference still governs the whole entry.
fn push_bullet(out: &mut String, item: &ContextPackItem) {
    let mut lines = item.content.lines();
    let first = lines.next().unwrap_or_default();
    out.push_str(&format!("- {first} [ref: {}]", item.source_ref));
    out.push('\n');
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        out.push_str("  ");
        out.push_str(line);
        out.push('\n');
    }
}

/// Items in §42 order, so the document reads top-down from the rungs that matter.
pub fn ladder_order(items: &[ContextPackItem]) -> Vec<ContextPackItem> {
    let mut out = items.to_vec();
    out.sort_by(|a, b| {
        rank(a.section)
            .cmp(&rank(b.section))
            .then_with(|| a.priority.cmp(&b.priority))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibe_domain::ContextSection;

    fn item(section: ContextSection, source_ref: &str, content: &str) -> ContextPackItem {
        ContextPackItem {
            section,
            source_ref: source_ref.to_string(),
            content: content.to_string(),
            priority: 1,
            token_cost: crate::budget::estimate_tokens(content),
            redacted: false,
        }
    }

    #[test]
    fn sections_appear_under_their_ladder_heading_in_order() {
        let items = vec![
            item(ContextSection::Request, "request:self", "why is sync slow"),
            item(
                ContextSection::OriginalIntent,
                "charter:c-1",
                "keep one source of truth",
            ),
            item(ContextSection::SelectedContext, "issue:I-1", "sync duplicates"),
        ];
        let text = render_markdown(&items);
        let title = text.lines().next().expect("title");
        assert_eq!(title, TITLE);
        let headings: Vec<&str> = text
            .lines()
            .filter(|line| line.starts_with("## "))
            .collect();
        assert_eq!(
            headings,
            vec![
                "## Selected Context",
                "## Original Intent",
                "## Request",
            ]
        );
        assert!(text.contains("- sync duplicates [ref: issue:I-1]"));
    }

    #[test]
    fn every_item_line_carries_its_source_ref() {
        let items = vec![
            item(ContextSection::RecentEvidence, "event:e-1", "ISSUE_FOUND"),
            item(
                ContextSection::RelevantArtifacts,
                "artifact:a-1",
                "src/sync.ts (source_file, v2, sha256:aa)",
            ),
        ];
        for item in &items {
            assert!(
                render_markdown(&items).contains(&format!("[ref: {}]", item.source_ref)),
                "{} is anonymous",
                item.source_ref
            );
        }
    }

    #[test]
    fn a_multi_line_body_stays_under_one_reference() {
        let items = vec![item(
            ContextSection::ActiveDecisions,
            "decision:d-1",
            "Event-sourced state\nreducer folds accepted events\nnothing else mutates state",
        )];
        let text = render_markdown(&items);
        assert!(text.contains("- Event-sourced state [ref: decision:d-1]\n"));
        assert!(text.contains("  reducer folds accepted events\n"));
        assert!(text.contains("  nothing else mutates state\n"));
    }

    #[test]
    fn a_redacted_pack_says_so_at_the_end() {
        let mut kept = item(ContextSection::RecentEvidence, "message:m-1", "withheld");
        kept.redacted = true;
        let text = render_markdown(&[
            kept,
            item(ContextSection::Request, "request:self", "question"),
        ]);
        assert!(text.contains("1 item(s) in this pack were redacted"), "{text}");
        // No redaction: no footer, byte for byte.
        assert!(!render_markdown(&[item(
            ContextSection::Request,
            "request:self",
            "question"
        )])
        .contains("redacted"));
    }

    #[test]
    fn the_same_items_render_to_the_same_bytes() {
        let items = vec![
            item(ContextSection::SelectedContext, "issue:I-1", "a"),
            item(ContextSection::Request, "request:self", "b"),
        ];
        assert_eq!(render_markdown(&items), render_markdown(&ladder_order(&items)));
    }
}
