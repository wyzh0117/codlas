//! §38.5's MCP basic interface: the workbench as a tool server on a pipe.
//!
//! The transport is MCP's stdio one — newline-delimited JSON-RPC 2.0, one message per line,
//! read from stdin and answered on stdout — so this crate needs no network stack and no async
//! runtime. §60 prices the deterministic chain as zero-cost, and a local pipe is exactly that:
//! nothing here opens a port, and stdout carries only JSON-RPC, never a diagnostic.
//!
//! Four messages plus one notification are the whole "basic interface" a client has to have to
//! reach a tool: `initialize`, `ping`, `tools/list`, `tools/call` and
//! `notifications/initialized`, which must not be answered. What the tools are is the crate's
//! other half, in [`tools`], and every one of them reads through the §6 ports — the server is a
//! second door onto the CLI's surfaces, not a second implementation behind it (禁止 7).
//!
//! A response never contains a raw newline: a tool's markdown lives inside a JSON string, where
//! serde escapes it, which is what the newline-delimited transport requires.

pub mod tools;

use std::io::{BufRead, Write};

use serde_json::{json, Value};
use vibe_core::ports::Repositories;

/// The MCP revision this server speaks. A client that asks for another one gets this answer
/// rather than a refusal: `2024-11-05` differs from the later revisions only in tool-annotation
/// and sampling details, none of which a six-read-tool server uses.
pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// What `serverInfo` reports, so a client's config file names the product rather than a binary.
pub const SERVER_NAME: &str = "codlas-workbench";

/// The settings row `wp project use` writes. Named here as a string rather than imported from
/// `apps/cli`, because a server crate must not depend on a surface crate; the two are kept
/// honest by `tests/mcp.rs`, which reads the key through the pointer the CLI would set.
pub const POINTER_KEY: &str = "workbench.current_project";

// JSON-RPC 2.0's reserved codes, with the meanings MCP assigns them.
pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;

/// One server: one workbench database, optionally pinned to one project.
pub struct McpServer {
    repos: Repositories,
    project: Option<String>,
}

impl McpServer {
    pub fn new(repos: Repositories, project: Option<String>) -> Self {
        Self { repos, project }
    }

    /// The tool catalogue, as the wire shape `tools/list` returns.
    pub fn tool_list() -> Value {
        Value::Array(
            tools::definitions()
                .iter()
                .map(|tool| {
                    json!({
                        "name": tool.name,
                        "description": tool.description,
                        "inputSchema": tool.input_schema,
                    })
                })
                .collect(),
        )
    }

    /// One line in, one answer out. `None` for a notification or a blank line, so the serve
    /// loop never writes a stray response — and so the whole protocol is testable without a
    /// child process or a socket.
    pub fn handle(&self, line: &str) -> Option<String> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        let incoming: Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(error) => {
                return Some(rpc_error(
                    Value::Null,
                    PARSE_ERROR,
                    &format!("the line is not JSON: {error}"),
                ))
            }
        };
        // No `id` field at all means a notification, which is answered with silence even when
        // it names a method this server does not know.
        let notification = incoming.get("id").is_none();
        let id = incoming.get("id").cloned().unwrap_or(Value::Null);
        let Some(method) = incoming.get("method").and_then(Value::as_str) else {
            return (!notification)
                .then(|| rpc_error(id, INVALID_REQUEST, "a request needs a method name"));
        };
        let params = incoming.get("params").cloned().unwrap_or(Value::Null);
        let outcome = self.dispatch(method, &params);
        if notification {
            return None;
        }
        Some(match outcome {
            Ok(result) => rpc_result(id, result),
            Err((code, message)) => rpc_error(id, code, &message),
        })
    }

    /// The message loop: lines in from `input`, answers out to `output`, until stdin closes.
    /// Returns how many messages were answered, which is what a smoke test asserts.
    pub fn serve<R: BufRead, W: Write>(&self, input: R, mut output: W) -> std::io::Result<usize> {
        let mut answered = 0;
        for line in input.lines() {
            let line = line?;
            if let Some(answer) = self.handle(&line) {
                writeln!(output, "{answer}")?;
                // A client is waiting on this line. Leaving it in the buffer until exit is not
                // buffering, it is a hang.
                output.flush()?;
                answered += 1;
            }
        }
        Ok(answered)
    }

    fn dispatch(&self, method: &str, params: &Value) -> std::result::Result<Value, (i64, String)> {
        match method {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {
                    "name": SERVER_NAME,
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "instructions": "Read-only view of a Codlas workbench: project state, the \
                    PROJECT_MAP, the event log, search, the evidence a record cites, and one \
                    compiled Context Pack. Nothing here writes to project memory; judgement \
                    belongs to the user in `wp`.",
            })),
            // The handshake's only notification. Answered with `{}` when a client insists on an
            // id, which some do; there is nothing to report either way.
            "notifications/initialized" | "notifications/cancelled" => Ok(json!({})),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": Self::tool_list()})),
            "tools/call" => self.call_tool(params),
            "notifications/progress" => Ok(json!({})),
            other => Err((
                METHOD_NOT_FOUND,
                format!(
                    "'{other}' is not a method this server speaks — try initialize, ping, \
                     tools/list, tools/call"
                ),
            )),
        }
    }

    fn call_tool(&self, params: &Value) -> std::result::Result<Value, (i64, String)> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| (INVALID_PARAMS, "tools/call needs a `name`".to_string()))?;
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if !arguments.is_object() {
            return Err((INVALID_PARAMS, "`arguments` must be an object".to_string()));
        }
        let outcome = (|| -> std::result::Result<Value, tools::ToolError> {
            let project = self.project_for(&arguments)?;
            tools::call(&self.repos, &project, name, &arguments)
        })();
        Ok(match outcome {
            Ok(value) => tool_content(value, false),
            // A wrong name or bad arguments are the client's mistake, so the protocol answers
            // them. A refusal from the workbench is a *result* the agent can read (§62).
            Err(tools::ToolError::Unknown(message)) | Err(tools::ToolError::BadParams(message)) => {
                return Err((INVALID_PARAMS, message))
            }
            Err(tools::ToolError::Execution(message)) => tool_content(json!({"error": message}), true),
        })
    }

    /// Which project a call reads: the argument, then the `--project` the server started with,
    /// then the pointer `wp project use` left, then the only project in the database. The same
    /// order the CLI uses, so both surfaces answer for the same project by default.
    fn project_for(
        &self,
        arguments: &Value,
    ) -> std::result::Result<String, tools::ToolError> {
        if let Some(explicit) = arguments.get("projectId").and_then(Value::as_str) {
            if !explicit.trim().is_empty() {
                return match self.repos.projects.get_project(explicit) {
                    Ok(Some(_)) => Ok(explicit.to_string()),
                    Ok(None) => Err(tools::ToolError::BadParams(format!(
                        "no project '{explicit}' in this workbench"
                    ))),
                    Err(error) => Err(tools::ToolError::Execution(error.to_string())),
                };
            }
        }
        if let Some(pinned) = &self.project {
            return match self.repos.projects.get_project(pinned) {
                Ok(Some(_)) => Ok(pinned.clone()),
                Ok(None) => Err(tools::ToolError::Execution(format!(
                    "this server was started with --project {pinned}, which is not in the \
                     database"
                ))),
                Err(error) => Err(tools::ToolError::Execution(error.to_string())),
            };
        }
        let pointer: Option<String> = self
            .repos
            .settings
            .get_json(POINTER_KEY)
            .ok()
            .flatten()
            .and_then(|value| value.as_str().map(str::to_string));
        if let Some(id) = pointer {
            if matches!(self.repos.projects.get_project(&id), Ok(Some(_))) {
                return Ok(id);
            }
        }
        let projects = self
            .repos
            .projects
            .list_projects()
            .map_err(|error| tools::ToolError::Execution(error.to_string()))?;
        match projects.as_slice() {
            [only] => Ok(only.id.clone()),
            [] => Err(tools::ToolError::Execution(
                "this workbench holds no project yet — create one with `wp project create`".into(),
            )),
            many => Err(tools::ToolError::Execution(format!(
                "{} projects exist, so a call has to name one: {}",
                many.len(),
                many.iter()
                    .map(|project| format!("{} ({})", project.id, project.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        }
    }
}

/// The MCP tool result shape: an array of content blocks. The payload goes in as pretty JSON
/// text, because a text block is what every client can render and a model reads JSON best when
/// it is laid out.
fn tool_content(value: Value, is_error: bool) -> Value {
    let text = match &value {
        Value::String(raw) => raw.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string()),
    };
    json!({
        "content": [{"type": "text", "text": text}],
        "isError": is_error,
    })
}

fn rpc_result(id: Value, result: Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string()
}

fn rpc_error(id: Value, code: i64, message: &str) -> String {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}).to_string()
}
