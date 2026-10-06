//! The normaliser against the shapes a real agent session log uses — `content` as an
//! array of typed blocks, with thinking and tool machinery interleaved — plus the
//! shorter shapes earlier imports relied on.

use chrono::{DateTime, Utc};
use vibe_domain::MessageRole;
use vibe_ingestion::Normalizer;

fn at() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-02T14:21:46.252Z")
        .unwrap()
        .with_timezone(&Utc)
}

#[test]
fn jsonl_reads_text_blocks_and_skips_thinking_and_tool_blocks() {
    let raw = [
        r#"{"type":"user","uuid":"u-1","timestamp":"2026-10-02T14:21:46.252Z","message":{"role":"user","content":[{"type":"text","text":"根据文档进行开发，注意开发完成后回写总控文件"}]}}"#,
        r#"{"type":"assistant","uuid":"a-1","timestamp":"2026-10-02T14:21:50.785Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"Let me start by reading the two attached files."}]}}"#,
        r#"{"type":"assistant","uuid":"a-2","timestamp":"2026-10-02T14:21:52.100Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"plan"},{"type":"text","text":"先读两份文件，再动手。"},{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"V1_DEVELOPMENT.md"}}]}}"#,
        r#"{"type":"user","uuid":"u-2","timestamp":"2026-10-02T14:21:53.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"…file contents…"}]}}"#,
        r#"{"type":"user","uuid":"u-3","timestamp":"2026-10-02T14:21:54.000Z","message":{"role":"user","content":["older shape: a plain string array"]}}"#,
    ]
    .join("\n");

    let messages =
        Normalizer::jsonl("", &raw, at()).expect("a log in this shape has to parse");

    let bodies: Vec<(&MessageRole, &str)> =
        messages.iter().map(|m| (&m.role, m.content.as_str())).collect();
    assert_eq!(
        bodies.len(),
        3,
        "thinking-only and tool_result-only records are machinery, not turns"
    );
    assert_eq!(bodies[0].1, "根据文档进行开发，注意开发完成后回写总控文件");
    assert_eq!(
        bodies[1].1, "先读两份文件，再动手。",
        "the text block — not the thinking, not the tool call"
    );
    assert_eq!(bodies[2].1, "older shape: a plain string array");
    assert_eq!(bodies[0].0, &MessageRole::User);
    assert_eq!(bodies[1].0, &MessageRole::Assistant);
}

#[test]
fn a_block_without_a_type_but_with_text_is_still_text() {
    let raw = r#"{"type":"user","uuid":"u-9","message":{"role":"user","content":[{"text":"no type field"}]}}"#;
    let messages = Normalizer::jsonl("", raw, at()).expect("parses");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, "no type field");
}
