//! The §42 priority ladder and the §41 heading set, as data.
//!
//! Two orders exist and they are not the same thing:
//!   * `tier` is the §42 retrieval priority — how much the engine trusts this
//!     section to answer the question in front of it. It is what a `PlannedItem`
//!     reports, and what budget trimming reads from the bottom up.
//!   * `rank` is the order the compiled markdown shows those sections in: §42's
//!     ladder, wearing §41's headings.

use vibe_domain::ContextSection;

/// §42.1 Selected Context.
pub const SELECTED: u8 = 1;
/// §42.2 Charter / Relevant Scope.
pub const CHARTER_AND_SCOPE: u8 = 2;
/// §42.3 Active Decision.
pub const DECISIONS: u8 = 3;
/// §42.4 Direct Entity.
pub const DIRECT_ENTITY: u8 = 4;
/// §42.5 Relevant Artifact.
pub const ARTIFACTS: u8 = 5;
/// §42.6 Current Issue.
pub const CURRENT_ISSUE: u8 = 6;
/// §42.7 Recent Events.
pub const RECENT_EVENTS: u8 = 7;
/// §42.8 AEP (and the drift warnings that come with it).
pub const AEP: u8 = 8;
/// §42.9 Related Conversation.
pub const CONVERSATION: u8 = 9;
/// §42.10 General Project Map.
pub const PROJECT_MAP: u8 = 10;
/// The user's own words: retrieved first, and the one section budget trimming may
/// never touch (§43).
pub const REQUEST: u8 = 1;

/// The §42 tier of a section.
///
/// Two sections span more than one tier because the §41 heading set is coarser than
/// the §42 ladder: `RecentEvidence` receives both "7. Recent Events" and "9. Related
/// Conversation", `RelatedIssue` both "4. Direct Entity" and "6. Current Issue",
/// `CurrentPhase` both "2. Charter-adjacent phase" and "10. General Project Map".
/// Every item carries its own precise tier, so trimming can still tell a conversation
/// excerpt from a recent event.
pub fn tier(section: ContextSection) -> u8 {
    match section {
        ContextSection::SelectedContext => SELECTED,
        ContextSection::OriginalIntent
        | ContextSection::RelevantScope
        | ContextSection::CurrentPhase => CHARTER_AND_SCOPE,
        ContextSection::ActiveDecisions | ContextSection::ArchitectureConstraints => DECISIONS,
        ContextSection::RelatedIssue => DIRECT_ENTITY,
        ContextSection::RelevantArtifacts => ARTIFACTS,
        ContextSection::RecentEvidence => RECENT_EVENTS,
        ContextSection::RelatedAep | ContextSection::DriftWarnings => AEP,
        ContextSection::Request => REQUEST,
    }
}

/// The render order: §42 tiers, §41 headings, ties broken by the pack shape in §41.
pub const SECTION_ORDER: [ContextSection; 12] = [
    ContextSection::SelectedContext,
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
];

/// Position of a section in the rendered pack.
pub fn rank(section: ContextSection) -> usize {
    SECTION_ORDER
        .iter()
        .position(|candidate| *candidate == section)
        .unwrap_or(SECTION_ORDER.len())
}

/// The two numbers the trim pass orders on, borrowed out of a pack item.
#[derive(Debug, Clone, Copy)]
pub struct ItemKey {
    pub section: ContextSection,
    pub priority: u8,
}

/// Sort key for the trimming pass: the most expendable item first, meaning the
/// highest (least important) tier number, then the heading that appears latest.
/// Dropping walks this list from the front, so the request — tier 1, last heading —
/// is the last thing it ever reaches.
pub fn expendable(a: &ItemKey, b: &ItemKey) -> std::cmp::Ordering {
    b.priority
        .cmp(&a.priority)
        .then_with(|| rank(b.section).cmp(&rank(a.section)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_section_has_a_place_in_the_pack() {
        for section in SECTION_ORDER {
            assert_ne!(rank(section), SECTION_ORDER.len(), "{section:?} is unreachable");
            assert!((1..=10).contains(&tier(section)), "{section:?} tier");
        }
        assert_eq!(rank(ContextSection::SelectedContext), 0);
        assert_eq!(rank(ContextSection::Request), SECTION_ORDER.len() - 1);
    }

    #[test]
    fn the_request_is_the_last_thing_dropped() {
        let map = ItemKey {
            section: ContextSection::CurrentPhase,
            priority: PROJECT_MAP,
        };
        let request = ItemKey {
            section: ContextSection::Request,
            priority: REQUEST,
        };
        // "map comes before request in the expendable list" == "map is dropped first".
        assert_eq!(expendable(&map, &request), std::cmp::Ordering::Less);
        assert_eq!(expendable(&request, &map), std::cmp::Ordering::Greater);
    }

    #[test]
    fn same_tier_breaks_on_the_heading() {
        let decisions = ItemKey {
            section: ContextSection::ActiveDecisions,
            priority: DECISIONS,
        };
        let constraints = ItemKey {
            section: ContextSection::ArchitectureConstraints,
            priority: DECISIONS,
        };
        assert_eq!(expendable(&constraints, &decisions), std::cmp::Ordering::Less);
    }
}
