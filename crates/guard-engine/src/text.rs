//! Cheap deterministic lexical comparison for the drift rules (V1_DEVELOPMENT.md §26).
//!
//! V1 has no model in this path, so "related" and "contradicts" are decided with
//! token overlap plus marker lists. Everything here is pure and order-stable, which
//! is what lets `check()` run repeatedly without piling up alerts.

use std::collections::HashSet;

fn is_han(c: char) -> bool {
    matches!(c, '\u{3400}'..='\u{9fff}' | '\u{f900}'..='\u{faff}' | '\u{20000}'..='\u{2ebef}')
}

/// Latin/digit words plus CJK bigrams: Chinese has no spaces, so a bigram is the
/// smallest unit that behaves like a word.
pub fn tokens(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.to_lowercase().chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if is_han(c) {
            let start = i;
            while i < chars.len() && is_han(chars[i]) {
                i += 1;
            }
            let run = &chars[start..i];
            if run.len() == 1 {
                out.push(run[0].to_string());
            } else {
                for pair in run.windows(2) {
                    out.push(pair.iter().collect());
                }
            }
        } else if c.is_alphanumeric() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i] == '_' || (chars[i].is_alphanumeric() && !is_han(chars[i]))) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if word.chars().filter(|c| c.is_alphanumeric()).count() > 1 {
                out.push(word);
            }
        } else {
            i += 1;
        }
    }
    out
}

pub fn unique_tokens(text: &str) -> HashSet<String> {
    tokens(text).into_iter().collect()
}

/// Share of `reference`'s tokens that appear in `candidate`.
pub fn overlap(candidate: &str, reference: &str) -> f64 {
    let a = unique_tokens(candidate);
    let b = unique_tokens(reference);
    if b.is_empty() {
        return 0.0;
    }
    a.intersection(&b).count() as f64 / b.len() as f64
}

pub fn shared_tokens(a: &str, b: &str) -> usize {
    let x = unique_tokens(a);
    let y = unique_tokens(b);
    x.intersection(&y).count()
}

/// Overlap in whichever direction is stronger — for two statements of similar length.
pub fn overlap_best(a: &str, b: &str) -> f64 {
    overlap(a, b).max(overlap(b, a))
}

pub fn squash(text: &str) -> String {
    text.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

/// True when `candidate` talks about `reference`: the reference appears verbatim,
/// or almost all of its tokens do.
pub fn covers(candidate: &str, reference: &str, threshold: f64) -> bool {
    let flat_reference = squash(reference);
    if flat_reference.is_empty() {
        return false;
    }
    if squash(candidate).contains(&flat_reference) {
        return true;
    }
    overlap(candidate, reference) >= threshold && shared_tokens(candidate, reference) >= 1
}

pub fn related(a: &str, b: &str, threshold: f64) -> bool {
    covers(a, b, threshold) || covers(b, a, threshold)
}

/// Literal or simple `*` wildcard match, as used by `ArchitectureInvariant.detect`.
pub fn pattern_hit(haystack: &str, pattern: &str) -> bool {
    let p = pattern.trim().to_lowercase();
    if p.is_empty() {
        return false;
    }
    let hay = haystack.to_lowercase();
    if !p.contains('*') {
        return hay.contains(&p);
    }
    let needle: Vec<char> = p.chars().collect();
    let text: Vec<char> = hay.chars().collect();
    let (mut i, mut j) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut mark = 0usize;
    while i < text.len() {
        if j < needle.len() && needle[j] == text[i] {
            i += 1;
            j += 1;
        } else if j < needle.len() && needle[j] == '*' {
            star = Some(j);
            mark = i;
            j += 1;
        } else if let Some(s) = star {
            j = s + 1;
            mark += 1;
            i = mark;
        } else {
            return false;
        }
    }
    while j < needle.len() && needle[j] == '*' {
        j += 1;
    }
    j == needle.len()
}

const NEGATIONS: &[&str] = &[
    "不", "勿", "非", "无", "没有", "禁止", "避免", "不得", "不能", "不要",
    "never", "cannot", "without", "avoid", "not ", "no ", "don't", "doesn't",
];

/// Words that only appear when a proposal pushes against an existing stance.
const OPPOSITION: &[&str] = &[
    "直接调用", "直接依赖", "直接访问", "直接读写", "改为", "换成", "去掉", "移除", "绕过", "跳过",
    "不再", "取消", "call directly", "depend on directly", "bypass", "remove", "drop",
    "instead", "switch to", "get rid of",
];

pub fn negated(text: &str) -> bool {
    let lower = text.to_lowercase();
    NEGATIONS.iter().any(|marker| lower.contains(marker))
}

pub fn has_opposition(text: &str) -> bool {
    let lower = text.to_lowercase();
    OPPOSITION.iter().any(|marker| lower.contains(marker))
}

/// V1 has no NLI model: a contradiction is the same wording with the negation
/// polarity flipped, plus oppositional vocabulary. Restating a decision in other
/// words never fires it, agreeing with it never fires it.
pub fn contradicts(baseline: &str, candidate: &str, threshold: f64) -> bool {
    let polarity_flip = negated(baseline) != negated(candidate);
    let oppositional = has_opposition(candidate) || has_opposition(baseline);
    polarity_flip
        && oppositional
        && overlap_best(baseline, candidate) >= threshold
}

pub fn excerpt(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    let count = trimmed.chars().count();
    if count <= max {
        return trimmed.to_string();
    }
    let cut: String = trimmed.chars().take(max).collect();
    format!("{cut}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_bigrams_let_a_short_item_match_a_longer_title() {
        assert!(covers("支持多用户登录", "多用户登录", 0.6));
        assert!(!covers("实现深色模式", "多用户登录", 0.6));
    }

    #[test]
    fn wildcard_detect_patterns_work() {
        assert!(pattern_hit("crates/core/src/db.rs", "crates/core/*"));
        assert!(pattern_hit("use rusqlite;", "rusqlite"));
        assert!(!pattern_hit("crates/search/src/lib.rs", "rusqlite"));
    }

    #[test]
    fn flipped_negation_reads_as_a_contradiction() {
        assert!(contradicts(
            "Core 不直接依赖 SQLite 存储",
            "让 Core 直接调用 SQLite 存储",
            0.4
        ));
        assert!(!contradicts(
            "Core 不直接依赖 SQLite 存储",
            "核心层不直接依赖 SQLite 存储",
            0.4
        ));
    }
}
