//! The tool catalogue §38.5 hands an agent: the workbench's read surfaces, named the way a
//! model will reach for them.
//!
//! Two rules hold this file together. Every tool is a *read* of something `wp` already prints,
//! so the MCP surface adds an entry point rather than a second authority (§7.6, 禁止 4). And
//! §53 does not stop at the terminal: `context_pack` goes through the same compiler that runs
//! the Policy Filter and writes the §54 audit row, so a pack handed down a pipe is no freer
//! than a pack printed for the user.

use serde_json::{json, Map, Value};
use vibe_context_engine::{ContextCompiler, ContextRequest, Selection, DEFAULT_TOTAL_TOKENS};
use vibe_core::error::CoreError;
use vibe_core::event_store::EventStore;
use vibe_core::ports::{EntityKind, Repositories};
use vibe_domain::EventStatus;
use vibe_search::SearchService;
use vibe_state_engine::{ProjectMapCompiler, Reducer};

/// Why a call produced no content, in the three classes a protocol has to tell apart: a client
/// that named the wrong tool, a client that passed the wrong arguments, and a workbench that
/// said no. Only the last is a normal result to put in front of a model.
pub enum ToolError {
    /// No such tool — answered at the protocol level, since no agent can act on it.
    Unknown(String),
    /// The tool exists; its arguments do not fit the schema in [`definitions`].
    BadParams(String),
    /// Called correctly and refused — an honest `isError` result rather than a protocol error
    /// (§62: degradation is reported, not hidden).
    Execution(String),
}

/// One entry of `tools/list`. The schema is the only contract a third-party client reads, so
/// it says what the tool is for, not just what it takes.
pub struct ToolDefinition {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

/// The property every tool accepts: which project to read, when the server was started without
/// `--project` and the database holds more than one.
const PROJECT_DESCRIPTION: &str = "Workbench project to read. Omit when the database holds \
    exactly one project, or when the server was started with --project.";

/// An argument object schema: `projectId` plus the tool's own properties, with the required
/// names listed as the tool sees them.
fn schema(required: &[&str], properties: Vec<(&str, Value)>) -> Value {
    let mut named = Map::new();
    named.insert(
        "projectId".to_string(),
        json!({"type": "string", "description": PROJECT_DESCRIPTION}),
    );
    for (key, value) in properties {
        named.insert(key.to_string(), value);
    }
    json!({
        "type": "object",
        "properties": named,
        "required": required,
        "additionalProperties": false,
    })
}

/// The catalogue: state, map, timeline, search, evidence, and one compiled context pack. Six
/// reads, no writes to project memory — the judgement verbs (§29 review, §27 confirm) stay in
/// the CLI, where a human is the one pressing return.
pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "project_state",
            description: "The materialized Project State: tasks, issues, decisions, artifacts, milestones, drift alerts and the scope boundary. It is the fold of the accepted event log, so it is a projection rather than a second copy of the truth (§20).",
            input_schema: schema(&[], Vec::new()),
        },
        ToolDefinition {
            name: "project_map",
            description: "The PROJECT_MAP as markdown, compiled from current Project State instead of being hand-written (§30-§33). The cheapest way to get oriented in a project.",
            input_schema: schema(&[], Vec::new()),
        },
        ToolDefinition {
            name: "project_timeline",
            description: "The event log, oldest first, with seq, type, status and payload — the traceability view every other answer here cites (§45).",
            input_schema: schema(
                &[],
                vec![
                    (
                        "limit",
                        json!({"type": "integer", "minimum": 1, "maximum": 200, "default": 20}),
                    ),
                    (
                        "pending",
                        json!({"type": "boolean", "default": false, "description": "Include events still awaiting the user's judgement (§29)."}),
                    ),
                ],
            ),
        },
        ToolDefinition {
            name: "search",
            description: "Find records by text across the three retrieval layers — exact id, full-text index, relation expansion (§40). Each hit carries kind, id, title, snippet, layer and score.",
            input_schema: schema(
                &["query"],
                vec![
                    ("query", json!({"type": "string", "description": "Words to look for. Every token is matched as itself: no stemming, no fuzzy fallback."})),
                    ("kind", json!({"type": "array", "items": {"type": "string"}, "description": "Restrict to record kinds, e.g. [\"task\", \"decision\"]."})),
                    ("limit", json!({"type": "integer", "minimum": 1, "maximum": 50, "default": 10})),
                ],
            ),
        },
        ToolDefinition {
            name: "open_evidence",
            description: "The evidence rows one record cites, addressed as `kind:id` — e.g. target `decision:D-1`. This is the leg that makes a claim checkable (§17, §45).",
            input_schema: schema(
                &["target"],
                vec![(
                    "target",
                    json!({"type": "string", "description": "`kind:id` of a task, issue, decision, artifact, milestone, change proposal, AEP, drift alert or evidence row."}),
                )],
            ),
        },
        ToolDefinition {
            name: "context_pack",
            description: "Compile the Context Pack for a question: charter and scope first, then the selected records, inside a token budget, after the Policy Filter, with the audit row it produced (§41-§44, §53, §54). This is what the workbench hands an agent to work from.",
            input_schema: schema(
                &["question"],
                vec![
                    ("question", json!({"type": "string", "description": "What the agent is being asked to do, in the user's own words."})),
                    ("select", json!({"type": "array", "items": {"type": "string"}, "description": "Records to pin into the pack, each as `kind:id`."})),
                    ("focus", json!({"type": "array", "items": {"type": "string"}, "description": "Terms to bias retrieval towards."})),
                    ("tokens", json!({"type": "integer", "minimum": 256, "description": "Total token budget for the pack."})),
                ],
            ),
        },
    ]
}

/// Run one tool against one project. `Ok(value)` becomes a text content block; `Err` keeps the
/// distinction between the client's mistake and the workbench's.
pub fn call(
    repos: &Repositories,
    project_id: &str,
    name: &str,
    args: &Value,
) -> std::result::Result<Value, ToolError> {
    match name {
        "project_state" => {
            let state = Reducer::new(repos.clone())
                .project_read_only(project_id)
                .map_err(executed)?;
            serialized(state)
        }
        "project_map" => {
            let map = ProjectMapCompiler::new(repos.clone())
                .compile(project_id)
                .map_err(executed)?;
            Ok(json!({
                "markdown": map.markdown,
                "sourceStateHash": map.source_state_hash,
            }))
        }
        "project_timeline" => {
            let limit = bounded_int(args, "limit", 20, 200)?;
            let statuses = if boolean(args, "pending")? {
                None
            } else {
                Some([EventStatus::Accepted].as_slice())
            };
            let mut events = EventStore::new(repos.clone())
                .list(project_id, statuses, Some(limit))
                .map_err(executed)?;
            // The store cuts its LIMIT from the newest end and hands back `seq DESC`, which is
            // right for a tail but wrong for a history: a model reading events has to read them
            // in the order they happened. The window is the store's; the direction is ours.
            events.reverse();
            serialized(events)
        }
        "search" => {
            let query = required_str(args, "query")?;
            let kinds = string_array(args, "kind")?;
            let limit = bounded_int(args, "limit", 10, 50)?;
            let found = SearchService::new(repos.clone())
                .find_in(project_id, &query, &kinds, limit)
                .map_err(executed)?;
            // `Found` is not a serialized type — it is an engine's answer, and this is the
            // client's shape: the same projection `wp search --json` prints.
            Ok(Value::Array(
                found
                    .iter()
                    .map(|item| {
                        json!({
                            "kind": item.hit.kind,
                            "id": item.hit.id,
                            "projectId": item.hit.project_id,
                            "title": item.hit.title,
                            "snippet": item.hit.snippet,
                            "layer": item.layer.as_str(),
                            "score": item.score,
                        })
                    })
                    .collect(),
            ))
        }
        "open_evidence" => {
            let target = required_str(args, "target")?;
            let (kind, subject) = target.split_once(':').ok_or_else(|| {
                ToolError::BadParams(format!("expected `kind:id`, got '{target}'"))
            })?;
            let kind = EntityKind::parse(kind).ok_or_else(|| {
                ToolError::BadParams(format!(
                    "unknown entity kind '{kind}' — task, issue, decision, artifact, milestone, \
                     change_proposal, aep, drift_alert, evidence"
                ))
            })?;
            let rows = repos.evidence.for_subject(kind, subject).map_err(executed)?;
            serialized(rows)
        }
        "context_pack" => {
            let question = required_str(args, "question")?;
            let mut request = ContextRequest::new(project_id, question);
            for raw in string_array(args, "select")? {
                let (kind, id) = raw.split_once(':').ok_or_else(|| {
                    ToolError::BadParams(format!("a selection is `kind:id`, got '{raw}'"))
                })?;
                request = request.select(Selection::new(kind, id));
            }
            for term in string_array(args, "focus")? {
                request = request.focused_on(term);
            }
            let total = match args.get("tokens") {
                None | Some(Value::Null) => DEFAULT_TOTAL_TOKENS,
                Some(value) => match value.as_u64() {
                    Some(0) | None => {
                        return Err(ToolError::BadParams(
                            "'tokens' must be a whole number of at least 256".to_string(),
                        ))
                    }
                    Some(number) => number as usize,
                },
            };
            request = request.with_budget(ContextCompiler::default_budget(total));
            let pack = ContextCompiler::new(repos.clone())
                .build(&request)
                .map_err(executed)?;
            Ok(json!({
                "packId": pack.id,
                "markdown": pack.markdown,
                "tokenEstimate": pack.token_estimate,
                "items": pack.items.len(),
                "redacted": pack.redacted_count,
                "policyApplied": serialized(&pack.policy_applied)?,
            }))
        }
        other => Err(ToolError::Unknown(other.to_string())),
    }
}

/// A refusal from the workbench, as opposed to a malformed call.
fn executed(error: CoreError) -> ToolError {
    ToolError::Execution(error.to_string())
}

/// Serialize through the ports' own types. A conversion failure is the workbench's problem, not
/// the client's, so it lands in the `Execution` class.
fn serialized<T: serde::Serialize>(value: T) -> std::result::Result<Value, ToolError> {
    serde_json::to_value(value).map_err(|error| ToolError::Execution(error.to_string()))
}

/// A required string argument. Blank counts as missing: a search for nothing would be a scan.
fn required_str(args: &Value, key: &str) -> std::result::Result<String, ToolError> {
    match args.get(key).and_then(Value::as_str) {
        Some(text) if !text.trim().is_empty() => Ok(text.to_string()),
        Some(_) => Err(ToolError::BadParams(format!("'{key}' must not be blank"))),
        None => Err(ToolError::BadParams(format!("'{key}' is required"))),
    }
}

fn string_array(args: &Value, key: &str) -> std::result::Result<Vec<String>, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str().map(str::to_string).ok_or_else(|| {
                    ToolError::BadParams(format!("every entry of '{key}' must be a string"))
                })
            })
            .collect(),
        Some(_) => Err(ToolError::BadParams(format!(
            "'{key}' must be an array of strings"
        ))),
    }
}

fn boolean(args: &Value, key: &str) -> std::result::Result<bool, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(ToolError::BadParams(format!("'{key}' must be true or false"))),
    }
}

/// An integer with a ceiling: a tool that one argument can turn into "read the whole log" is
/// a tool that will be (§59).
fn bounded_int(
    args: &Value,
    key: &str,
    default: usize,
    max: usize,
) -> std::result::Result<usize, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Number(number)) => match number.as_u64() {
            Some(0) => Err(ToolError::BadParams(format!("'{key}' must be at least 1"))),
            Some(value) if value as usize > max => Err(ToolError::BadParams(format!(
                "'{key}' is capped at {max} so one call cannot read the whole database"
            ))),
            Some(value) => Ok(value as usize),
            None => Err(ToolError::BadParams(format!("'{key}' must be a whole number"))),
        },
        Some(_) => Err(ToolError::BadParams(format!("'{key}' must be a number"))),
    }
}
