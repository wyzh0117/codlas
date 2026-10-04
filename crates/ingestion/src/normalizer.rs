//! The conversation normalizer: somebody's export format in, faithful [`Message`]
//! records out (V1_DEVELOPMENT.md §14, §52).
//!
//! Two guarantees matter here and nothing else:
//!
//! * **Roles are mapped, never invented.** `human`, `prompt`, `user` all mean the user;
//!   `bot`, `model`, `output` all mean the agent. An unrecognised speaker falls back to
//!   `user`, which is the same fallback the storage layer uses, so a record cannot change
//!   meaning depending on which layer read it.
//! * **The content hash is the dedup key.** [`vibe_domain::content_hash`] normalises
//!   whitespace before hashing, so the same sentence pasted with different padding, or
//!   re-exported with extra indentation, is the *same* message (§52 L2).
//!
//! Nothing in this module judges content. A transcript line is recorded as it arrived;
//! whether it means anything to the project is the [`SignalJudge`](vibe_core::judge::SignalJudge)'s
//! decision downstream (§27, PROJECT_MASTER §19 禁止 9).

use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;
use vibe_core::error::{CoreError, Result};
use vibe_domain::{content_hash, Message, MessageRole};

/// One transcript line before it becomes a stored row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTurn {
    pub role: MessageRole,
    pub content: String,
    pub external_id: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
}

impl RawTurn {
    pub fn new(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            external_id: None,
            created_at: None,
        }
    }
}

pub struct Normalizer;

const ROLE_KEYS: &[&str] = &[
    "role", "author", "speaker", "from", "sender", "actor", "messageType", "message_type", "type",
];
const CONTENT_KEYS: &[&str] = &["content", "text", "message", "delta", "value", "body"];
const ID_KEYS: &[&str] = &["id", "uuid", "messageId", "message_id", "nodeId", "node_id"];
const TIME_KEYS: &[&str] = &["timestamp", "created_at", "createdAt", "create_time", "ts", "time"];

impl Normalizer {
    /// `§37` role mapping. Vendor spellings are vocabulary, not code paths: add a word
    /// here rather than teaching a new product to the pipeline.
    pub fn role(raw: &str) -> MessageRole {
        match raw.trim().to_lowercase().as_str() {
            "user" | "human" | "prompt" | "me" | "input" | "owner" | "用户" | "我" => {
                MessageRole::User
            }
            "assistant" | "ai" | "model" | "bot" | "agent" | "output" | "answer" | "助手"
            | "模型" => MessageRole::Assistant,
            "system" | "instruction" | "instructions" | "context" | "系统" => MessageRole::System,
            "tool" | "function" | "command" | "stdout" | "script" | "execution" | "工具" => {
                MessageRole::Tool
            }
            // Same fallback as the storage row mapper, so an unknown speaker never
            // silently becomes an agent claim.
            _ => MessageRole::User,
        }
    }

    /// Content hash used as dedup level L2 — whitespace-insensitive by construction.
    pub fn hash(content: &str) -> String {
        content_hash(content)
    }

    pub fn turns_to_messages(
        conversation_id: &str,
        turns: &[RawTurn],
        observed_at: DateTime<Utc>,
    ) -> Vec<Message> {
        turns
            .iter()
            .filter(|turn| !turn.content.trim().is_empty())
            .map(|turn| Message {
                id: vibe_domain::new_id(),
                conversation_id: conversation_id.to_string(),
                role: turn.role,
                content: turn.content.clone(),
                external_message_id: turn.external_id.clone(),
                content_hash: content_hash(&turn.content),
                created_at: turn.created_at,
                observed_at,
            })
            .collect()
    }

    /// `(speaker, text)` pairs, speaker mapped through [`Normalizer::role`].
    pub fn pairs(conversation_id: &str, pairs: &[(&str, &str)], observed_at: DateTime<Utc>) -> Vec<Message> {
        let turns: Vec<RawTurn> = pairs
            .iter()
            .map(|(speaker, text)| RawTurn::new(Self::role(speaker), *text))
            .collect();
        Self::turns_to_messages(conversation_id, &turns, observed_at)
    }

    /// Pasted chat text. If the paste carries `user:` / `助手:` style prefixes those win;
    /// otherwise blank-line separated blocks alternate user / assistant starting with the
    /// user, which is what a copied conversation looks like.
    pub fn plain_text(conversation_id: &str, text: &str, observed_at: DateTime<Utc>) -> Vec<Message> {
        let mut turns: Vec<RawTurn> = Vec::new();
        let lines: Vec<&str> = text.lines().collect();
        let prefixed = lines.iter().any(|line| split_prefix(line).is_some());

        if prefixed {
            let mut current: Option<(MessageRole, Vec<String>)> = None;
            for line in &lines {
                if let Some((role, rest)) = split_prefix(line) {
                    if let Some((prev_role, body)) = current.take() {
                        push_turn(&mut turns, prev_role, &body);
                    }
                    current = Some((role, vec![rest.to_string()]));
                } else if let Some((_, body)) = current.as_mut() {
                    body.push(line.to_string());
                } else if !line.trim().is_empty() {
                    // Preamble before the first speaker label: keep it, as the user.
                    push_turn(&mut turns, MessageRole::User, &[line.to_string()]);
                }
            }
            if let Some((role, body)) = current.take() {
                push_turn(&mut turns, role, &body);
            }
            return Self::turns_to_messages(conversation_id, &turns, observed_at);
        }

        let mut block = String::new();
        let mut blocks: Vec<String> = Vec::new();
        for line in &lines {
            if line.trim().is_empty() {
                if !block.trim().is_empty() {
                    blocks.push(std::mem::take(&mut block));
                }
            } else {
                block.push_str(line);
                block.push('\n');
            }
        }
        if !block.trim().is_empty() {
            blocks.push(block);
        }
        for (index, body) in blocks.iter().enumerate() {
            let role = if index % 2 == 0 {
                MessageRole::User
            } else {
                MessageRole::Assistant
            };
            turns.push(RawTurn::new(role, body.trim()));
        }
        Self::turns_to_messages(conversation_id, &turns, observed_at)
    }

    /// A JSON export for one conversation: a turn array, or an object carrying
    /// `messages` / `items` / `log` / ChatGPT's `mapping`.
    pub fn json(conversation_id: &str, value: &Value, observed_at: DateTime<Utc>) -> Result<Vec<Message>> {
        let turns = Self::turns_of(value);
        if turns.is_empty() {
            return Err(CoreError::Encoding(
                "no conversation turns found: expected an array of {role, content} or an object \
                 with messages / items / log / mapping"
                    .into(),
            ));
        }
        Ok(Self::turns_to_messages(conversation_id, &turns, observed_at))
    }

    /// One JSON object per line — the shape local CLI agents write session logs in.
    pub fn jsonl(conversation_id: &str, text: &str, observed_at: DateTime<Utc>) -> Result<Vec<Message>> {
        let mut turns: Vec<RawTurn> = Vec::new();
        for (number, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(trimmed).map_err(|e| {
                CoreError::Encoding(format!("jsonl line {}: {e}", number + 1))
            })?;
            turns.extend(Self::turns_of(&value));
        }
        if turns.is_empty() {
            return Err(CoreError::Encoding("jsonl holds no turns".into()));
        }
        Ok(Self::turns_to_messages(conversation_id, &turns, observed_at))
    }

    /// An export file holding several conversations: pick one by external id or title,
    /// else the first in document order (§51 — one adapter instance is one source, so a
    /// caller that wants them all registers one adapter per conversation).
    pub fn select_conversation(value: &Value, external_id: Option<&str>) -> Value {
        if let Some(entries) = value.as_array() {
            let conversations: Vec<&Value> = entries.iter().filter(|e| is_conversation(e)).collect();
            if let Some(wanted) = external_id {
                for entry in &conversations {
                    let id = first_string(entry, &["id", "conversation_id", "conversationId"]);
                    let title = first_string(entry, &["title", "name"]);
                    if id.as_deref() == Some(wanted) || title.as_deref() == Some(wanted) {
                        return (*entry).clone();
                    }
                }
            }
            if let Some(entry) = conversations.first() {
                return (*entry).clone();
            }
        }
        value.clone()
    }

    pub fn conversation_count(value: &Value) -> usize {
        value
            .as_array()
            .map(|entries| entries.iter().filter(|e| is_conversation(e)).count())
            .unwrap_or(0)
    }

    /// External conversation id declared by the export itself, when it carries one.
    pub fn external_conversation_id(value: &Value) -> Option<String> {
        first_string(value, &["id", "conversation_id", "conversationId", "session_id", "uuid"])
    }

    pub fn conversation_title(value: &Value) -> Option<String> {
        first_string(value, &["title", "name", "summary"])
    }

    /// Flatten any supported conversation shape into ordered turns.
    pub fn turns_of(value: &Value) -> Vec<RawTurn> {
        match value {
            Value::Array(entries) => entries.iter().flat_map(Self::turns_of).collect(),
            Value::Object(map) => {
                if let Some(mapping) = map.get("mapping").and_then(Value::as_object) {
                    return chatgpt_mapping(mapping);
                }
                for key in ["messages", "items", "log", "turns", "conversation", "history"] {
                    if let Some(inner) = map.get(key) {
                        if inner.is_array() || inner.is_object() {
                            let turns = Self::turns_of(inner);
                            if !turns.is_empty() {
                                return turns;
                            }
                        }
                    }
                }
                turn_from_object(value).into_iter().collect()
            }
            _ => Vec::new(),
        }
    }
}

fn split_prefix(line: &str) -> Option<(MessageRole, &str)> {
    for marker in [
        "user", "human", "assistant", "ai", "system", "tool", "用户", "助手", "系统", "工具",
    ] {
        for separator in ['：', ':'] {
            if let Some(rest) = line.strip_prefix(&format!("{marker}{separator}")) {
                return Some((Normalizer::role(marker), rest.trim_start()));
            }
            if let Some(rest) = line.strip_prefix(&format!("[{marker}]{separator}")) {
                return Some((Normalizer::role(marker), rest.trim_start()));
            }
        }
    }
    None
}

fn push_turn(turns: &mut Vec<RawTurn>, role: MessageRole, body: &[String]) {
    let content = body
        .iter()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    if !content.is_empty() {
        turns.push(RawTurn::new(role, content));
    }
}

fn is_conversation(value: &Value) -> bool {
    value.is_object()
        && [
            "mapping",
            "messages",
            "title",
            "node_id",
            "current_node",
            "conversation_id",
        ]
        .iter()
        .any(|key| value.get(*key).is_some())
}

fn first_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    })
}

fn role_of(value: &Value) -> Option<MessageRole> {
    ROLE_KEYS.iter().find_map(|key| {
        value.get(*key).map(|raw| match raw {
            Value::String(text) => Some(Normalizer::role(text)),
            // {"author": {"role": "assistant"}} and {"type":"message","author":..}
            Value::Object(_) => first_string(raw, &["role", "name", "type", "author"])
                .map(|text| Normalizer::role(&text)),
            _ => None,
        })?
    })
}

fn content_of(value: &Value) -> Option<String> {
    for key in CONTENT_KEYS {
        let Some(raw) = value.get(key) else { continue };
        if let Some(text) = raw.as_str() {
            if !text.trim().is_empty() {
                return Some(text.to_string());
            }
            continue;
        }
        if let Some(parts) = raw.get("parts").and_then(Value::as_array) {
            let joined = parts
                .iter()
                .filter_map(|p| p.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            if !joined.trim().is_empty() {
                return Some(joined);
            }
            continue;
        }
        if let Some(text) = first_string(raw, &["text", "value", "content", "stdout"]) {
            return Some(text);
        }
        if raw.is_object() {
            // {"message": {"content": {"parts": [...]}}} nests one level deeper.
            if let Some(deeper) = content_of(raw) {
                return Some(deeper);
            }
        }
        if let Some(array) = raw.as_array() {
            let joined = array
                .iter()
                .filter_map(|item| item.as_str().map(String::from))
                .collect::<Vec<_>>()
                .join("\n");
            if !joined.trim().is_empty() {
                return Some(joined);
            }
        }
    }
    None
}

fn timestamp_of(value: &Value) -> Option<DateTime<Utc>> {
    TIME_KEYS.iter().find_map(|key| {
        let raw = value.get(*key)?;
        if let Some(text) = raw.as_str() {
            if let Ok(parsed) = DateTime::parse_from_rfc3339(text) {
                return Some(parsed.with_timezone(&Utc));
            }
            if let Ok(seconds) = text.trim().parse::<i64>() {
                return Utc.timestamp_opt(seconds, 0).single();
            }
            return None;
        }
        if let Some(number) = raw.as_f64() {
            // ChatGPT exports use fractional unix seconds.
            let seconds = number.floor() as i64;
            let nanos = ((number - number.floor()) * 1_000_000_000.0).round() as u32;
            return Utc.timestamp_opt(seconds, nanos).single();
        }
        if let Some(number) = raw.as_i64() {
            return Utc.timestamp_opt(number, 0).single();
        }
        None
    })
}

fn turn_from_object(value: &Value) -> Option<RawTurn> {
    let inner = value.get("message").unwrap_or(value);
    let content = content_of(inner).or_else(|| content_of(value))?;
    Some(RawTurn {
        role: role_of(inner)
            .or_else(|| role_of(value))
            .unwrap_or(MessageRole::User),
        content,
        external_id: first_string(value, ID_KEYS).or_else(|| first_string(inner, ID_KEYS)),
        created_at: timestamp_of(inner).or_else(|| timestamp_of(value)),
    })
}

/// ChatGPT's `mapping` is a graph keyed by node id. Walk it from the root so the thread
/// order survives; entries with no content (the empty root node) drop out.
fn chatgpt_mapping(mapping: &serde_json::Map<String, Value>) -> Vec<RawTurn> {
    let mut order: Vec<RawTurn> = Vec::new();
    let root = mapping
        .iter()
        .find(|(_, node)| {
            node.get("parent")
                .map(|p| p.is_null())
                .unwrap_or(false)
        })
        .map(|(key, _)| key.clone())
        .or_else(|| mapping.keys().next().map(|k| k.to_string()));

    let Some(start) = root else {
        return order;
    };
    let mut cursor: Option<String> = if mapping
        .get(&start)
        .and_then(|node| node.get("message"))
        .is_some()
    {
        Some(start)
    } else {
        next_child(mapping, &start)
    };
    let mut guard = 0usize;
    while let Some(id) = cursor {
        if guard > mapping.len() {
            break; // cyclic export: stop rather than spin
        }
        guard += 1;
        if let Some(node) = mapping.get(&id) {
            if let Some(turn) = turn_from_object(node) {
                order.push(turn);
            }
            cursor = next_child(mapping, &id);
        } else {
            break;
        }
    }
    order
}

fn next_child(
    mapping: &serde_json::Map<String, Value>,
    id: &str,
) -> Option<String> {
    let children = mapping
        .get(id)?
        .get("children")?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .map(String::from)
        .collect::<Vec<_>>();
    children.into_iter().next()
}
