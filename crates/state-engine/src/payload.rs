// Deterministic helpers that keep the projection reproducible: canonical JSON
// for hashing, plus tolerant readers for event payloads authored by other crates.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use vibe_domain::ProjectEvent;

/// Compact JSON with object keys sorted, so a `serde_json::Map` can never leak an
/// insertion order into `state_hash`.
pub(crate) fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let body = keys
                .iter()
                .map(|k| format!("{}:{}", quote(k), canonical_json(&map[*k])))
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{body}}}")
        }
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(canonical_json).collect::<Vec<_>>().join(",")
        ),
        other => serde_json::to_string(other).unwrap_or_else(|_| "null".to_string()),
    }
}

fn quote(text: &str) -> String {
    Value::String(text.to_string()).to_string()
}

/// String field, tried in key order. Both camelCase and snake_case spellings are
/// accepted because payloads come from adapters we do not control.
pub(crate) fn text(payload: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        match payload.get(*key) {
            Some(Value::String(s)) if !s.trim().is_empty() => return Some(s.trim().to_string()),
            Some(Value::Number(n)) => return Some(n.to_string()),
            Some(Value::Bool(b)) => return Some(b.to_string()),
            _ => {}
        }
    }
    None
}

pub(crate) fn text_list(payload: &Value, keys: &[&str]) -> Vec<String> {
    for key in keys {
        match payload.get(*key) {
            Some(Value::Array(items)) => {
                let out: Vec<String> = items
                    .iter()
                    .filter_map(|v| match v {
                        Value::String(s) => Some(s.trim().to_string()),
                        Value::Number(n) => Some(n.to_string()),
                        _ => None,
                    })
                    .filter(|s| !s.is_empty())
                    .collect();
                if !out.is_empty() {
                    return out;
                }
            }
            Some(Value::String(s)) if !s.trim().is_empty() => {
                return vec![s.trim().to_string()]
            }
            _ => {}
        }
    }
    Vec::new()
}

pub(crate) fn boolean(payload: &Value, keys: &[&str]) -> Option<bool> {
    keys.iter().find_map(|k| payload.get(*k).and_then(|v| v.as_bool()))
}

pub(crate) fn number(payload: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|k| payload.get(*k).and_then(|v| v.as_i64()))
}

/// The id an event talks about: explicit subject first, then payload aliases,
/// then the event's own id so no accepted event is ever silently dropped.
pub(crate) fn resolve_id(event: &ProjectEvent, payload_keys: &[&str]) -> String {
    if let Some(id) = event
        .subject_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return id.to_string();
    }
    text(&event.payload, payload_keys).unwrap_or_else(|| event.id.clone())
}

pub(crate) fn event_time(event: &ProjectEvent) -> DateTime<Utc> {
    event.occurred_at.unwrap_or(event.observed_at)
}

/// Fixed-format timestamp: no local timezone and no sub-second jitter, so the map
/// body stays byte-identical for an identical event log.
pub(crate) fn human_time(value: DateTime<Utc>) -> String {
    value.format("%Y-%m-%d %H:%M").to_string()
}

/// One-line excerpt for Recent Changes. Truncation is by chars, so it is stable
/// across locales and builds.
pub(crate) fn excerpt(event: &ProjectEvent, limit: usize) -> String {
    let raw = text(
        &event.payload,
        &[
            "title",
            "name",
            "summary",
            "excerpt",
            "detail",
            "proposedChange",
            "proposed_change",
            "content",
            "text",
            "commitMessage",
            "commit_message",
            "reason",
            "description",
        ],
    )
    .or_else(|| event.subject_id.clone())
    .unwrap_or_else(|| event.r#type.clone());
    clip(&raw.split_whitespace().collect::<Vec<_>>().join(" "), limit)
}

pub(crate) fn clip(text: &str, limit: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= limit {
        return text.to_string();
    }
    let mut head: String = chars[..limit].iter().collect();
    head.push('…');
    head
}

/// The wire spelling of a status enum. Going through `Serialize` instead of
/// `{:?}` means a map line never changes because a Rust variant was renamed.
pub(crate) fn serde_slug<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibe_domain::{ActorType, EventStatus};

    fn event(payload: Value) -> ProjectEvent {
        ProjectEvent {
            id: "e1".into(),
            project_id: "p1".into(),
            r#type: "TASK_CREATED".into(),
            schema_version: 1,
            actor_type: ActorType::System,
            actor_id: None,
            source_id: None,
            conversation_id: None,
            message_id: None,
            origin_node_id: None,
            causation_id: None,
            correlation_id: None,
            subject_type: None,
            subject_id: None,
            payload,
            confidence: 1.0,
            evidence_refs: Vec::new(),
            status: EventStatus::Accepted,
            occurred_at: None,
            observed_at: Utc::now(),
            seq: None,
        }
    }

    #[test]
    fn canonical_json_sorts_object_keys() {
        let a: Value = serde_json::from_str(r#"{"b":1,"a":{"d":2,"c":3}}"#).unwrap();
        let b: Value = serde_json::from_str(r#"{"a":{"c":3,"d":2},"b":1}"#).unwrap();
        assert_eq!(canonical_json(&a), canonical_json(&b));
        assert_eq!(canonical_json(&a), r#"{"a":{"c":3,"d":2},"b":1}"#);
    }

    #[test]
    fn payload_readers_are_spelling_tolerant() {
        let snake = event(serde_json::json!({ "proposed_change": "  drop the layer  " }));
        let camel = event(serde_json::json!({ "proposedChange": "drop the layer" }));
        assert_eq!(excerpt(&snake, 60), excerpt(&camel, 60));
        assert_eq!(resolve_id(&snake, &["id"]), "e1");
    }

    #[test]
    fn excerpt_clips_on_chars_not_bytes() {
        let long = event(serde_json::json!({ "title": "这是一段很长的中文标题用于验证截断行为" }));
        let out = excerpt(&long, 8);
        assert_eq!(out.chars().count(), 9);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn status_spelling_comes_from_serde() {
        assert_eq!(serde_slug(&vibe_domain::TaskStatus::Todo), "todo");
        assert_eq!(serde_slug(&ActorType::Agent), "agent");
    }

    #[test]
    fn text_list_accepts_array_and_single_string() {
        assert_eq!(
            text_list(&serde_json::json!({ "trigger": ["a", "b"] }), &["trigger"]),
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(
            text_list(&serde_json::json!({ "trigger": "only" }), &["trigger"]),
            vec!["only".to_string()]
        );
        assert!(text_list(&Value::Null, &["trigger"]).is_empty());
    }
}
