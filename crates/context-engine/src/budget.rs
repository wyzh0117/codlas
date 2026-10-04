//! §43: the context budget, as data plus one documented heuristic.
//!
//! A pack is not "as much of the project as fits". It is a set of per-section
//! allowances compiled from the §43 shares, applied in §42 priority order. Nothing
//! is ever clipped mid-item — an item either travels or it is recorded as dropped,
//! and the drop is visible in the pack (§43, and the reason `Request` is exempt).

use vibe_domain::{ContextBudget, ContextPackItem, ContextSection};

use crate::section::{rank, tier, SECTION_ORDER};

/// A token is not a character. This is the estimate the whole crate uses: four
/// ASCII-ish characters per token, one token per wide (CJK, kana, Hangul,
/// full-width) character, and a non-empty run never rounds down to zero.
///
/// Deterministic and clock-free on purpose — tests pin it, and the CLI shows it.
pub fn estimate_tokens(text: &str) -> usize {
    let mut wide = 0usize;
    let mut narrow = 0usize;
    for ch in text.chars() {
        if is_wide(ch) {
            wide += 1;
        } else {
            narrow += 1;
        }
    }
    wide + narrow.div_ceil(4)
}

/// Ranges this build treats as one-token-per-character. Not an attempt at full
/// Unicode width tables: it covers the scripts the product is written for.
pub fn is_wide(ch: char) -> bool {
    let code = ch as u32;
    matches!(code,
        0x1100..=0x115F     // Hangul Jamo
        | 0x2E80..=0x303E   // CJK radicals, Kangxi, CJK punctuation
        | 0x3041..=0x33FF   // kana, Bopomofo, CJK compatibility
        | 0x3400..=0x4DBF   // CJK extension A
        | 0x4E00..=0x9FFF   // CJK unified
        | 0xA000..=0xA4CF   // Yi
        | 0xAC00..=0xD7A3   // Hangul syllables
        | 0xF900..=0xFAFF   // CJK compatibility ideographs
        | 0xFE30..=0xFE4F   // CJK compatibility forms
        | 0xFF00..=0xFF60   // full-width forms
        | 0xFFE0..=0xFFE6
        | 0x20000..=0x3FFFD) // CJK extensions B and beyond
}

/// The §43 shares, mapped onto the §41 heading set. Sums to 1.0:
///
/// ```text
/// 10% Original Intent / Charter   -> OriginalIntent .10
/// 15% Decisions / Constraints     -> ActiveDecisions .10 + ArchitectureConstraints .05
/// 20% Relevant State              -> SelectedContext .08 + RelevantScope .04
///                                   + RelatedIssue .05 + CurrentPhase .03
/// 25% Files / Artifacts / Evidence-> RelevantArtifacts .15
/// 15% Recent Discussion           -> RecentEvidence .15, plus the .10 evidence half
///                                   of the Files/Artifacts/Evidence bucket
///  5% AEP / Drift                 -> RelatedAep .03 + DriftWarnings .02
/// 10% Current Request             -> Request .10
/// ```
pub fn default_shares() -> Vec<(ContextSection, f64)> {
    vec![
        (ContextSection::SelectedContext, 0.08),
        (ContextSection::OriginalIntent, 0.10),
        (ContextSection::RelevantScope, 0.04),
        (ContextSection::CurrentPhase, 0.03),
        (ContextSection::ActiveDecisions, 0.10),
        (ContextSection::ArchitectureConstraints, 0.05),
        (ContextSection::RelatedIssue, 0.05),
        (ContextSection::RelevantArtifacts, 0.15),
        (ContextSection::RecentEvidence, 0.25),
        (ContextSection::RelatedAep, 0.03),
        (ContextSection::DriftWarnings, 0.02),
        (ContextSection::Request, 0.10),
    ]
}

/// A budget with the §43 shares at the caller's total.
pub fn budget_with(total_tokens: usize) -> ContextBudget {
    ContextBudget {
        total_tokens,
        shares: default_shares(),
    }
}

/// Share for one section: whatever the caller set, else the §43 default. A budget
/// that names no sections therefore behaves exactly like the default.
pub fn share_of(budget: &ContextBudget, section: ContextSection) -> f64 {
    budget
        .shares
        .iter()
        .find(|(candidate, _)| *candidate == section)
        .map(|(_, share)| *share)
        .unwrap_or_else(|| {
            default_shares()
                .into_iter()
                .find(|(candidate, _)| *candidate == section)
                .map(|(_, share)| share)
                .unwrap_or(0.0)
        })
}

/// Tokens a section may use.
pub fn allowance_of(budget: &ContextBudget, section: ContextSection) -> usize {
    let share = share_of(budget, section).clamp(0.0, 1.0);
    (budget.total_tokens as f64 * share).round() as usize
}

/// One item that did not fit.
#[derive(Debug, Clone)]
pub struct Trimmed {
    pub source_ref: String,
    pub section: ContextSection,
    pub priority: u8,
    pub tokens: usize,
}

/// What the budget pass took out. The pack has no field for this, so the compiler
/// surfaces it as a visible note under `Request` (§43: never truncate silently).
#[derive(Debug, Clone, Default)]
pub struct TrimReport {
    pub trimmed: Vec<Trimmed>,
}

impl TrimReport {
    pub fn is_empty(&self) -> bool {
        self.trimmed.is_empty()
    }

    pub fn dropped_tokens(&self) -> usize {
        self.trimmed.iter().map(|item| item.tokens).sum()
    }

    /// The line the compiled pack carries when anything was dropped.
    pub fn note(&self, total_tokens: usize) -> Option<String> {
        if self.trimmed.is_empty() {
            return None;
        }
        const LISTED: usize = 6;
        let refs: Vec<&str> = self
            .trimmed
            .iter()
            .take(LISTED)
            .map(|item| item.source_ref.as_str())
            .collect();
        let more = self.trimmed.len() - refs.len();
        let tail = if more > 0 {
            format!(", +{more} more")
        } else {
            String::new()
        };
        Some(format!(
            "[budget] {} item(s) / {} token(s) omitted to stay inside the {total_tokens}-token \
             context budget (§43); the request below is intact: {}{tail}",
            self.trimmed.len(),
            self.dropped_tokens(),
            refs.join(", ")
        ))
    }
}

/// Apply the §43 shares to a policy-filtered pack. Items arrive in §42 order; a
/// section keeps its highest-priority items and drops the rest. `Request` is
/// exempt — the user's own question always survives, in full.
pub fn trim(
    items: Vec<ContextPackItem>,
    budget: &ContextBudget,
) -> (Vec<ContextPackItem>, TrimReport) {
    let mut report = TrimReport::default();
    let mut kept: Vec<ContextPackItem> = Vec::with_capacity(items.len());

    for section in SECTION_ORDER {
        let mut group: Vec<ContextPackItem> = items
            .iter()
            .filter(|item| item.section == section)
            .cloned()
            .collect();
        if group.is_empty() {
            continue;
        }
        // Priority first inside a section, stable so equal tiers keep retrieval order.
        group.sort_by_key(|item| (item.priority, rank(item.section)));

        if section == ContextSection::Request {
            kept.extend(group);
            continue;
        }

        let allowance = allowance_of(budget, section);
        let mut used = 0usize;
        let mut fits = 0usize;
        for (index, item) in group.iter().enumerate() {
            if index > 0 && used + item.token_cost > allowance {
                break;
            }
            used += item.token_cost;
            fits = index + 1;
        }
        for item in group.drain(fits..) {
            report.trimmed.push(Trimmed {
                source_ref: item.source_ref,
                section: item.section,
                priority: item.priority,
                tokens: item.token_cost,
            });
        }
        kept.extend(group);
    }

    kept.sort_by_key(|item| (rank(item.section), item.priority));
    report
        .trimmed
        .sort_by(|a, b| b.priority.cmp(&a.priority).then_with(|| tier(b.section).cmp(&tier(a.section))));
    (kept, report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(section: ContextSection, priority: u8, content: &str) -> ContextPackItem {
        ContextPackItem {
            section,
            source_ref: format!("{section:?}-{priority}"),
            content: content.to_string(),
            priority,
            token_cost: estimate_tokens(content),
            redacted: false,
        }
    }

    #[test]
    fn the_heuristic_is_documented_and_pinned() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        assert_eq!(estimate_tokens("为什么还保留"), 6);
        assert_eq!(estimate_tokens("sk-1"), 1);
        // Mixed text: each wide character is a token, the rest counts at 4:1.
        // "中文 abcd" = 2 wide + 5 narrow -> 2 + ceil(5/4) = 4.
        assert_eq!(estimate_tokens("中文 abcd"), 4);
    }

    #[test]
    fn default_shares_add_up_to_one_whole_budget() {
        let total: f64 = default_shares().iter().map(|(_, share)| *share).sum();
        assert!((total - 1.0).abs() < 1e-9, "shares sum to {total}");
        let budget = budget_with(1000);
        assert_eq!(allowance_of(&budget, ContextSection::OriginalIntent), 100);
        assert_eq!(allowance_of(&budget, ContextSection::RecentEvidence), 250);
    }

    #[test]
    fn an_unlisted_section_keeps_its_default_share() {
        // Overriding one share does not silently zero the others: a caller tuning the
        // request budget still gets §43 everywhere else.
        let mut budget = ContextBudget {
            total_tokens: 1000,
            shares: vec![(ContextSection::Request, 0.5)],
        };
        assert_eq!(allowance_of(&budget, ContextSection::Request), 500);
        assert_eq!(allowance_of(&budget, ContextSection::RelevantArtifacts), 150);
        budget.shares.clear();
        assert_eq!(allowance_of(&budget, ContextSection::RelevantArtifacts), 150);
        // A share the caller *does* name to zero is honoured as zero.
        budget.shares.push((ContextSection::RelevantArtifacts, 0.0));
        assert_eq!(allowance_of(&budget, ContextSection::RelevantArtifacts), 0);
    }

    #[test]
    fn the_lowest_priority_items_in_a_section_go_first() {
        let items = vec![
            item(ContextSection::RecentEvidence, 7, "aaaaaaaa"),
            item(ContextSection::RecentEvidence, 7, "bbbbbbbb"),
            item(ContextSection::RecentEvidence, 9, "cccccccc"),
            item(ContextSection::Request, 1, "the question"),
        ];
        let budget = ContextBudget {
            total_tokens: 1000,
            shares: vec![
                (ContextSection::RecentEvidence, 0.005),
                (ContextSection::Request, 0.9),
            ],
        };
        let (kept, report) = trim(items, &budget);
        assert_eq!(kept.len(), 3, "{kept:?}");
        assert_eq!(report.trimmed.len(), 1);
        assert_eq!(report.trimmed[0].priority, 9, "the conversation excerpt goes first");
        assert!(kept
            .iter()
            .any(|item| item.section == ContextSection::Request));
    }

    #[test]
    fn the_request_section_survives_anything_the_budget_can_do() {
        let question = "why is the sync still duplicating messages? ".repeat(20);
        let items = vec![
            item(ContextSection::OriginalIntent, 2, "intent"),
            item(ContextSection::OriginalIntent, 2, "more intent than the share allows"),
            item(ContextSection::Request, 1, &question),
        ];
        let (kept, report) = trim(items, &budget_with(8));
        let request = kept
            .iter()
            .find(|item| item.section == ContextSection::Request)
            .expect("the request always travels");
        assert_eq!(request.content, question, "and never truncated");
        assert_eq!(report.trimmed.len(), 1, "the other section gave way");
    }

    #[test]
    fn a_drop_leaves_a_visible_note() {
        let items = vec![
            item(ContextSection::RecentEvidence, 7, "aaaaaaaa"),
            item(ContextSection::RecentEvidence, 9, "bbbbbbbb"),
        ];
        let budget = ContextBudget {
            total_tokens: 6,
            shares: vec![(ContextSection::RecentEvidence, 0.5)],
        };
        let (_, report) = trim(items, &budget);
        let note = report.note(6).expect("note");
        assert!(note.contains("1 item(s) / 2 token(s) omitted"), "{note}");
        assert!(note.contains("the request below is intact"));
        assert!(TrimReport::default().note(8).is_none());
    }
}
