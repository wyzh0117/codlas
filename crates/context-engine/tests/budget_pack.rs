//! (7) §43: the budget is spent by section, and a pack that gave something up says so.

mod support;

use support::{fixture, Fixture};
use vibe_context_engine::{
    allowance_of, engine_default, estimate_tokens, ContextCompiler, ContextRequest,
};
use vibe_domain::{ContextBudget, ContextSection, ProjectPolicy};

fn artifacts_fixture(count: usize) -> Fixture {
    let fixture = fixture();
    fixture.populate();
    for index in 0..count {
        fixture.artifact(&format!("A-{index:03}"), &format!("crates/core/src/file{index}.rs"));
    }
    fixture
}

#[test]
fn a_section_stays_inside_its_share_and_the_cheaper_rungs_keep_their_items() {
    let fixture = artifacts_fixture(40);
    let budget = ContextBudget {
        total_tokens: 400,
        shares: vec![
            (ContextSection::RelevantArtifacts, 0.1),
            (ContextSection::Request, 0.3),
        ],
    };
    let pack = fixture
        .compiler
        .build(
            &ContextRequest::new(fixture.project_id.clone(), "what files are there?")
                .with_budget(budget.clone()),
        )
        .expect("build");

    let spent: usize = pack
        .items
        .iter()
        .filter(|item| item.section == ContextSection::RelevantArtifacts)
        .map(|item| item.token_cost)
        .sum();
    assert!(
        spent <= allowance_of(&budget, ContextSection::RelevantArtifacts),
        "{spent} tokens of artifacts against a {} token allowance",
        allowance_of(&budget, ContextSection::RelevantArtifacts)
    );

    let issue_kept = pack
        .items
        .iter()
        .any(|item| item.section == ContextSection::RelatedIssue);
    assert!(issue_kept, "the cheaper rung is not the one that pays");

    // And the pack admits what it dropped, inside the document (§43 never silent).
    assert!(pack.markdown.contains("[budget]"), "{}", pack.markdown);
    assert!(pack.markdown.contains("the request below is intact"));
}

#[test]
fn the_users_own_words_are_the_last_thing_dropped() {
    let fixture = artifacts_fixture(5);
    let question = "why does the ingest path keep writing the same message twice?".repeat(6);
    let pack = fixture
        .compiler
        .build(
            &ContextRequest::new(fixture.project_id.clone(), question.clone())
                .with_budget(ContextCompiler::default_budget(24)),
        )
        .expect("build");
    let request = pack
        .items
        .iter()
        .find(|item| item.section == ContextSection::Request)
        .expect("the request always travels");
    assert_eq!(request.content, question, "and never truncated");
    assert!(pack.markdown.contains(&question));
}

#[test]
fn an_oversized_pack_reports_every_token_it_left_behind() {
    let fixture = artifacts_fixture(60);
    let pack = fixture
        .compiler
        .build(
            &ContextRequest::new(fixture.project_id.clone(), "artifacts")
                .with_budget(ContextCompiler::default_budget(120)),
        )
        .expect("build");
    let note = pack
        .items
        .iter()
        .find(|item| item.source_ref == "budget:context")
        .expect("a visible budget note");
    assert_eq!(note.section, ContextSection::Request);
    let words: Vec<&str> = note.content.split_whitespace().collect();
    // "[budget] N item(s) / M token(s) omitted …" with a non-zero count.
    let claimed_items: usize = words[1].parse().expect("item count");
    assert!(claimed_items > 0, "{:?}", note.content);
    let total_dropped: usize = pack
        .items
        .iter()
        .filter(|item| item.section == ContextSection::RelevantArtifacts)
        .count()
        .min(usize::MAX);
    assert!(
        total_dropped < 60,
        "the artifact section should have given way"
    );
    assert!(estimate_tokens(&pack.markdown) > 0);
}

#[test]
fn a_pack_that_fits_needs_no_apology() {
    let fixture = fixture();
    fixture.populate();
    let pack = fixture
        .compiler
        .build(
            &ContextRequest::new(fixture.project_id.clone(), "state")
                .with_budget(ContextCompiler::default_budget(4000)),
        )
        .expect("build");
    assert!(
        !pack.items.iter().any(|item| item.source_ref == "budget:context"),
        "nothing was dropped, so nothing is claimed"
    );
    assert!(!pack.markdown.contains("[budget]"));
}

#[test]
fn the_gate_runs_before_the_budget_so_redaction_is_what_costs() {
    // A redacted body is shorter than the original: the sizes in the pack must be the
    // sizes of what actually travels, not of what was read.
    let fixture = fixture();
    fixture.conversation(&[("api_key = sk-abcdefghijklmnopqrstuvwxyz012345", vibe_domain::MessageRole::User)]);
    let policy = ProjectPolicy {
        redact_secrets: true,
        ..engine_default()
    };
    fixture
        .compiler
        .set_policy(&fixture.project_id, &policy)
        .expect("policy");
    let pack = fixture
        .compiler
        .build(&ContextRequest::new(fixture.project_id.clone(), "key"))
        .expect("build");
    let item = pack
        .items
        .iter()
        .find(|item| item.section == ContextSection::RecentEvidence)
        .expect("a conversation item");
    assert_eq!(item.token_cost, estimate_tokens(&item.content));
    assert!(item.token_cost < 20, "the marker is cheaper than the secret: {item:?}");
}
