//! §52's L3: near-duplicate *hints*, never a deletion.
//!
//! L1 (external id) and L2 (content hash + role) are decided at insert time and drop
//! material, because "the same bytes again" is a fact a hash can prove. §52's L2 also names
//! a time window, and the store keeps it out of the key on purpose: §51 makes a re-push
//! idempotent, and a source timestamp can be absent or jittery, so identical text arriving
//! later stays this level's business — a hint, never a deletion. L3 is the soft case: two
//! turns that say almost the same thing. §52's one rule for it is that the workbench
//! **only hints** — 只提示，不直接删除 — so this module reads messages, returns
//! pairs, and writes nothing at all. A hint reaching the log as a fact would be a second
//! state source (禁止 4), and quietly merging two turns would destroy the trace §17 exists
//! to keep.
//!
//! V1 approximates "near" **lexically**, not semantically: Jaccard over token sets, with
//! CJK runs chopped into character bigrams because they have no spaces to split on. Real
//! semantic near-duplication needs an embedding or a model, which §60 prices as a Strong
//! Model call and 禁止 7 keeps out of the core path; the vector version is AEP-002's job.
//! What this file must be is honest about its own number, so `NearDuplicate::note` travels
//! with every pair and says the hint is an approximation.
//!
//! Cost (§59): pairs grow as the square of the window, so a caller bounds the window and
//! [`similarity_upper_bound`] rejects most pairs before any set is built — two texts whose
//! shorter token set is well below the threshold cannot reach it, whatever they say.

use serde::{Deserialize, Serialize};

use vibe_domain::Message;

/// The default bar for "near". 0.8 over token sets means eight of ten distinct words agree.
pub const DEFAULT_THRESHOLD: f64 = 0.8;

/// How many recent messages one scan reads. A hint surface must not stall a workbench, and
/// a quadratic pass over a 20 000-message thread is a stall; 240 covers the "the agent asked
/// the same question twice this afternoon" case this level exists for.
pub const DEFAULT_WINDOW: usize = 240;

/// The line every hint carries. §52: L3 只提示，不直接删除.
pub const NOTE: &str = "near-duplicate hint — both messages are still in the database; \
                        nothing was merged, deleted or rewritten";

/// One suspected pair, oldest first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NearDuplicate {
    /// Which thread the pair sits in. Two turns from unrelated conversations are not a
    /// repeat of each other, and the id is what lets a reader re-run the pass scoped to one
    /// conversation — the hint has to say where it came from to be checkable.
    pub conversation_id: String,
    pub first_id: String,
    pub second_id: String,
    /// 0.0-1.0, [`similarity`]'s Jaccard score.
    pub similarity: f64,
    /// A short, character-safe preview of each side, so a human can judge without opening
    /// the record.
    pub first_excerpt: String,
    pub second_excerpt: String,
    pub note: String,
}

/// A CJK ideograph: written without spaces, so it is tokenized by character pairs instead.
fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x3040..=0x30FF)
}

/// The bigrams of one unbroken CJK run — and the run itself when it is a single character.
fn push_cjk(out: &mut Vec<String>, run: &[char]) {
    if run.len() == 1 {
        out.push(run[0].to_string());
        return;
    }
    for pair in run.windows(2) {
        out.push(pair.iter().collect());
    }
}

/// The comparison's alphabet: lowercased alphanumeric words, CJK character bigrams,
/// punctuation and whitespace as boundaries. Deliberately dumb, because a dumb tokenizer is
/// the one a reader can check by eye.
pub fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for chunk in text.split_whitespace() {
        let mut word = String::new();
        let mut run: Vec<char> = Vec::new();
        for ch in chunk.chars() {
            if is_cjk(ch) {
                flush_word(&mut word, &mut out);
                run.push(ch);
            } else if ch.is_alphanumeric() {
                flush_run(&mut run, &mut out);
                word.push(ch.to_lowercase().next().unwrap_or(ch));
            } else {
                flush_word(&mut word, &mut out);
                flush_run(&mut run, &mut out);
            }
        }
        flush_word(&mut word, &mut out);
        flush_run(&mut run, &mut out);
    }
    out
}

fn flush_word(word: &mut String, out: &mut Vec<String>) {
    if !word.is_empty() {
        out.push(std::mem::take(word));
    }
}

fn flush_run(run: &mut Vec<char>, out: &mut Vec<String>) {
    if !run.is_empty() {
        push_cjk(out, run);
        run.clear();
    }
}

/// The set sizes' ratio: Jaccard can never exceed it, so it prunes a pair for the cost of
/// two numbers.
pub fn similarity_upper_bound(a: &[String], b: &[String]) -> f64 {
    let (small, large) = (a.len().min(b.len()), a.len().max(b.len()));
    if large == 0 {
        return 0.0;
    }
    small as f64 / large as f64
}

/// Jaccard overlap of the two texts' token sets. `0.0` for either empty — an empty message
/// is not "infinitely similar" to another empty one, it is nothing to compare.
pub fn similarity(a: &str, b: &str) -> f64 {
    let left = tokens(a);
    let right = tokens(b);
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    if similarity_upper_bound(&left, &right) < f64::EPSILON {
        return 0.0;
    }
    jaccard(&left, &right)
}

/// The score over tokens already cut, so [`scan`] and [`similarity`] agree by sharing one
/// formula rather than by two near-identical blocks. `0.0` on an empty union, which the
/// callers already exclude but a ratio must not divide by.
fn jaccard(left: &[String], right: &[String]) -> f64 {
    let left_set: std::collections::HashSet<String> = left.iter().cloned().collect();
    let right_set: std::collections::HashSet<String> = right.iter().cloned().collect();
    let union = left_set.union(&right_set).count();
    if union == 0 {
        return 0.0;
    }
    left_set.intersection(&right_set).count() as f64 / union as f64
}

/// A preview that cannot cut a multibyte character in half — the mistake this whole module
/// exists to not make.
pub fn excerpt(text: &str, budget: usize) -> String {
    let flattened = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::new();
    for ch in flattened.chars().take(budget) {
        out.push(ch);
    }
    if flattened.chars().count() > budget {
        out.push('…');
    }
    out
}

/// Every pair in `messages` at or above `threshold`, most similar first, capped at `limit`.
///
/// Reads only. There is no `&mut`, no repository and no store in this signature, which is
/// the §52 rule expressed as a type: the near-duplicate pass cannot delete anything.
pub fn scan(messages: &[Message], threshold: f64, limit: usize) -> Vec<NearDuplicate> {
    // One tokenizer pass per message, not per pair: the pairs are quadratic, and re-splitting
    // the same text 240 times over would spend the whole window's budget on one message.
    let tokenized: Vec<Vec<String>> = messages.iter().map(|m| tokens(&m.content)).collect();
    let mut found: Vec<NearDuplicate> = Vec::new();
    for index in 0..tokenized.len() {
        for second in index + 1..tokenized.len() {
            let left = &tokenized[index];
            let right = &tokenized[second];
            if left.is_empty() || right.is_empty() {
                continue;
            }
            // Cheap bound first: a pair that cannot reach the bar is not worth a set.
            if similarity_upper_bound(left, right) < threshold {
                continue;
            }
            let score = jaccard(left, right);
            if score < threshold {
                continue;
            }
            found.push(NearDuplicate {
                conversation_id: messages[index].conversation_id.clone(),
                first_id: messages[index].id.clone(),
                second_id: messages[second].id.clone(),
                similarity: score,
                first_excerpt: excerpt(&messages[index].content, 90),
                second_excerpt: excerpt(&messages[second].content, 90),
                note: NOTE.to_string(),
            });
        }
    }
    // Most similar first; the ids break ties so two runs of the same data agree (§59's
    // determinism, applied to a hint list too).
    found.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.first_id.cmp(&b.first_id))
            .then_with(|| a.second_id.cmp(&b.second_id))
    });
    found.truncate(limit);
    found
}
