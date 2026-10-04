//! (6) §41 + §42: the pack is the ladder, in order, with the documented headings.

mod support;

use support::fixture;
use vibe_context_engine::{section::tier, ContextRequest, Selection, SECTION_ORDER};
use vibe_domain::{ContextSection, MessageRole};

fn headings(markdown: &str) -> Vec<String> {
    markdown
        .lines()
        .filter_map(|line| line.strip_prefix("## "))
        .map(str::to_string)
        .collect()
}

fn rank_in(markdown: &str, section: &ContextSection) -> usize {
    headings(markdown)
        .iter()
        .position(|heading| heading == section.heading())
        .unwrap_or(usize::MAX)
}

/// §42's ladder, wearing §41's headings: Selected → Charter/Scope → Decisions →
/// Direct entity → Artifacts → Current issue → Recent events → AEP/Drift →
/// Conversation → General map. The Request heading closes the document because the
/// pack is *about* it, which is why the check below is on the rung order of the
/// sections that carry project facts.
#[test]
fn the_pack_walks_the_ladder_in_order() {
    let f = fixture();
    f.populate();
    f.save_project_map();

    let req = ContextRequest::new(
        f.project_id.clone(),
        "why is sync still duplicating messages?",
    );
    let pack = f.compiler.build(&req).expect("build");

    assert!(pack.markdown.starts_with("# Project Context\n"), "{}", pack.markdown);
    let present: Vec<ContextSection> = SECTION_ORDER
        .iter()
        .copied()
        .filter(|section| pack.items.iter().any(|item| item.section == *section))
        .collect();
    assert_eq!(headings(&pack.markdown), present.iter().map(|s| s.heading().to_string()).collect::<Vec<_>>());

    let positions: Vec<usize> = present.iter().map(|section| rank_in(&pack.markdown, section)).collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "headings out of order: {present:?} at {positions:?}"
    );

    // Reading the pack top to bottom walks the ladder: the section rank never goes
    // backwards, and every item carries a §42 rung number.
    let ranks: Vec<usize> = pack
        .items
        .iter()
        .map(|item| SECTION_ORDER.iter().position(|s| *s == item.section).unwrap())
        .collect();
    assert!(
        ranks.windows(2).all(|pair| pair[0] <= pair[1]),
        "items out of §42 order: {ranks:?}"
    );
    assert!(pack.items.iter().all(|item| (1..=10).contains(&item.priority)));
    assert!(pack
        .items
        .iter()
        .all(|item| item.priority >= tier(item.section) || item.priority == 10),
        "an item claimed a rung its section cannot hold"
    );
}

#[test]
fn every_rung_that_has_a_fact_has_a_line() {
    let f = fixture();
    f.populate();
    f.save_project_map();
    f.conversation(&[(
        "it duplicates on the second run",
        MessageRole::Assistant,
    )]);

    let req = ContextRequest::new(f.project_id.clone(), "sync duplication");
    let pack = f.compiler.build(&req).expect("build");

    for section in [
        ContextSection::OriginalIntent,
        ContextSection::RelevantScope,
        ContextSection::CurrentPhase,
        ContextSection::ActiveDecisions,
        ContextSection::ArchitectureConstraints,
        ContextSection::RelatedIssue,
        ContextSection::RelevantArtifacts,
        ContextSection::RecentEvidence,
        ContextSection::RelatedAep,
        ContextSection::DriftWarnings,
        ContextSection::Request,
    ] {
        assert!(
            pack.items.iter().any(|item| item.section == section),
            "{} is missing from the pack:\n{}",
            section.heading(),
            pack.markdown
        );
    }
}

#[test]
fn a_selection_is_the_first_thing_in_the_pack() {
    let f = fixture();
    f.populate();
    let req = ContextRequest::new(f.project_id.clone(), "this one")
        .select(Selection::new("issue", "I-018"));
    let pack = f.compiler.build(&req).expect("build");

    let first = pack.items.first().expect("a pack");
    assert_eq!(first.section, ContextSection::SelectedContext);
    assert_eq!(first.priority, 1);
    assert_eq!(tier(ContextSection::SelectedContext), 1);
    assert_eq!(
        pack.markdown.lines().nth(2).expect("first heading"),
        "## Selected Context"
    );
    assert!(pack
        .markdown
        .contains("- Open issue I-018: sync duplicates messages"));
}

#[test]
fn a_superseded_decision_does_not_travel_as_an_active_one() {
    let f = fixture();
    f.populate();
    let req = ContextRequest::new(f.project_id.clone(), "state");
    let pack = f.compiler.build(&req).expect("build");

    let refs: Vec<&str> = pack
        .items
        .iter()
        .filter(|item| item.section == ContextSection::ActiveDecisions)
        .map(|item| item.source_ref.as_str())
        .collect();
    assert_eq!(refs, vec!["decision:D-001"]);
    assert!(!pack.markdown.contains("mutable state row"));
    assert!(!pack.markdown.contains("old import path"), "resolved issue stays out");
    assert!(!pack.markdown.contains("ship stage A"), "a done task stays out");
}
