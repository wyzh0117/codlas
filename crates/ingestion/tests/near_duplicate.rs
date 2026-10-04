//! §52's L3: a near-duplicate pass that hints and cannot delete.
//!
//! The interesting assertions are the two halves of the promise. It must actually find the
//! soft case — two turns that say nearly the same thing in nearly the same words — and it
//! must not touch a row. A level that quietly merges messages would be the worst thing in
//! this system: the log is the product (§17).

use chrono::{DateTime, TimeZone, Utc};
use vibe_domain::{Message, MessageRole};
use vibe_ingestion::{near_duplicate, NearDuplicate};

fn at(seconds: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(1_770_000_000 + seconds, 0).unwrap()
}

fn message(id: &str, role: MessageRole, content: &str, offset: i64) -> Message {
    let mut message = Message::new(
        "conv-1".to_string(),
        role,
        content.to_string(),
        Some(id.to_string()),
        at(offset),
    );
    message.id = id.to_string();
    message
}

fn pair(first: &str, second: &str) -> Vec<Message> {
    vec![
        message(first, MessageRole::User, FIRST_TEXT, 0),
        message(second, MessageRole::Assistant, SECOND_TEXT, 1),
    ]
}

const FIRST_TEXT: &str = "we decided to store every important change as an event first";
const SECOND_TEXT: &str = "we decided to store every important change as an event first, right";

/// §52: near, not equal. L2 already dropped the identical text; this level catches the
/// reworded repeat that a hash cannot see.
#[test]
fn a_reworded_repeat_is_reported_and_an_unrelated_pair_is_not() {
    let hits = near_duplicate::scan(&pair("m1", "m2"), 0.6, 10);
    assert_eq!(hits.len(), 1, "one pair, one hint: {hits:?}");
    let hit: &NearDuplicate = &hits[0];
    assert_eq!((hit.first_id.as_str(), hit.second_id.as_str()), ("m1", "m2"));
    assert!(hit.similarity >= 0.6 && hit.similarity < 1.0, "{hit:?}");
    assert!(
        hit.note.contains("nothing was merged"),
        "the hint says what it is: {}",
        hit.note
    );

    let unrelated = near_duplicate::scan(
        &[
            message("a", MessageRole::User, FIRST_TEXT, 0),
            message(
                "b",
                MessageRole::Assistant,
                "the backup restores the whole database into an earlier file",
                1,
            ),
        ],
        0.6,
        10,
    );
    assert!(unrelated.is_empty(), "different subjects are not duplicates: {unrelated:?}");

    // Identical text is L2's business, but the pass has to see it as the top score, not as
    // nothing — otherwise a caller cannot tell "identical" from "unrelated".
    let same = near_duplicate::scan(
        &[
            message("c", MessageRole::User, FIRST_TEXT, 0),
            message("d", MessageRole::User, FIRST_TEXT, 1),
        ],
        0.6,
        10,
    );
    assert_eq!(same.len(), 1);
    assert!((same[0].similarity - 1.0).abs() < f64::EPSILON);
}

/// §52 at the exact bar: a pair below the threshold is not a hint, and the same pair above
/// it is. A threshold nobody can move is a policy, not a tool.
#[test]
fn the_threshold_is_the_whole_difference_between_a_hint_and_silence() {
    let messages = pair("m1", "m2");
    let score = near_duplicate::similarity(FIRST_TEXT, SECOND_TEXT);
    let strict = near_duplicate::scan(&messages, score + 0.01, 10);
    let loose = near_duplicate::scan(&messages, score - 0.01, 10);
    assert!(strict.is_empty(), "a bar above the score must say nothing");
    assert_eq!(loose.len(), 1, "a bar below it must say so");
}

/// Chinese has no spaces, so a word-splitting tokenizer sees one giant token per line and
/// finds nothing. The bigram pass is the reason this module is not `split_whitespace` alone.
#[test]
fn a_chinese_run_is_compared_by_character_pairs_not_by_the_whole_line() {
    let left = "我们决定把每次重要变更先写成事件再投影状态";
    let right = "我们决定把每次重要变更先写成事件再投影状态了";
    let hits = near_duplicate::scan(
        &[
            message("z1", MessageRole::User, left, 0),
            message("z2", MessageRole::Assistant, right, 1),
        ],
        0.6,
        10,
    );
    assert_eq!(hits.len(), 1, "one added character is a near-duplicate: {hits:?}");
    assert!(hits[0].similarity > 0.9, "{hits:?}");

    // Same script, different subject: two CJK lines about unrelated things are not a hint.
    let other = near_duplicate::scan(
        &[
            message("z3", MessageRole::User, left, 0),
            message(
                "z4",
                MessageRole::Assistant,
                "备份可以把整个数据库恢复到更早的一个文件",
                1,
            ),
        ],
        0.6,
        10,
    );
    assert!(other.is_empty(), "script alone is not similarity: {other:?}");
}

/// The preview must never cut a multibyte character in half — the failure mode a
/// byte-budget truncator has and a character-budget one does not.
#[test]
fn an_excerpt_is_truncated_on_characters_and_stays_whole() {
    let text = "事件是唯一的真相来源，缓存投影可以随时删除重建，这条规则撑起整个系统";
    let cut = near_duplicate::excerpt(text, 12);
    assert!(cut.is_char_boundary(cut.len()), "valid UTF-8");
    assert_eq!(cut.chars().count(), 13, "12 characters plus the ellipsis");
    assert!(text.starts_with(&cut[..cut.chars().count() - 1]));

    let short = near_duplicate::excerpt("three little words", 90);
    assert_eq!(short, "three little words", "nothing to cut, nothing to mark");

    // Whitespace runs collapse, because a preview of a paste should read as prose.
    assert_eq!(
        near_duplicate::excerpt("a   b\n\t c", 90),
        "a b c",
        "the preview is flattened"
    );
}

/// The cheap prune must be *sound*: it may reject a pair it cannot possibly accept, and
/// must never reject one it would. That is the difference between a fast path and a bug.
#[test]
fn the_size_bound_never_rejects_a_pair_the_score_would_accept() {
    let left = near_duplicate::tokens(FIRST_TEXT);
    let right = near_duplicate::tokens(SECOND_TEXT);
    let bound = near_duplicate::similarity_upper_bound(&left, &right);
    let score = near_duplicate::similarity(FIRST_TEXT, SECOND_TEXT);
    assert!(
        score <= bound + f64::EPSILON,
        "Jaccard {score} may not exceed the size ratio {bound}"
    );
    // And a mixed-script line tokenizes into both alphabets at once: latin words as words,
    // lone CJK characters as themselves.
    let mixed = near_duplicate::tokens("用 SQLite 存 events 表 with FTS5 for search");
    assert!(mixed.contains(&"sqlite".to_string()), "{mixed:?}");
    assert!(mixed.contains(&"events".to_string()), "{mixed:?}");
    assert!(mixed.contains(&"fts5".to_string()), "{mixed:?}");
    assert!(mixed.contains(&"用".to_string()), "{mixed:?}");
    assert!(mixed.contains(&"表".to_string()), "{mixed:?}");
}

/// Order and limit: the list a surface shows must be the most similar pairs, stably.
#[test]
fn the_list_is_most_similar_first_and_capped() {
    let messages = vec![
        message("a", MessageRole::User, FIRST_TEXT, 0),
        message("b", MessageRole::Assistant, SECOND_TEXT, 1),
        message(
            "c",
            MessageRole::User,
            "we decided to store every change as an event",
            2,
        ),
        message(
            "d",
            MessageRole::Assistant,
            "the backup restores the whole database into an earlier file",
            3,
        ),
    ];
    let all = near_duplicate::scan(&messages, 0.5, 20);
    assert!(all.len() >= 3, "three of these say much the same: {all:?}");
    assert!(
        all.windows(2).all(|w| w[0].similarity >= w[1].similarity),
        "sorted by score: {all:?}"
    );

    let capped = near_duplicate::scan(&messages, 0.5, 2);
    assert_eq!(capped.len(), 2, "the limit is honoured");
    assert_eq!(capped[0].similarity, all[0].similarity, "…and keeps the best ones");

    // An empty window is an empty answer, not a panic and not "everything".
    assert!(near_duplicate::scan(&[], 0.5, 10).is_empty());
}

/// §52's promise, read off the signature the code can actually reach: the pass takes a
/// slice it cannot write to and returns values. Nothing here can delete a message.
#[test]
fn the_pass_holds_no_way_to_write() {
    // `scan` and `similarity` are pure: same input, same output, twice over.
    let messages = pair("m1", "m2");
    let first = near_duplicate::scan(&messages, 0.6, 10);
    let second = near_duplicate::scan(&messages, 0.6, 10);
    assert_eq!(first, second, "a hint list a human may compare between runs");
    assert_eq!(
        messages.len(),
        2,
        "and the messages it read are still the ones it was given"
    );
}
