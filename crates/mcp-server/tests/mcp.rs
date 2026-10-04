//! §38.5's basic interface, answered against a real workbench database. The protocol is a
//! string in and a string out, so a test can drive it without a child process — and the
//! database has to be real, because what the tools promise is what the *ports* return: a fake
//! that echoes its input cannot show a pack that skipped the Policy Filter.

use std::sync::Arc;

use serde_json::{json, Value};
use vibe_core::event_store::{EventDraft, EventStore};
use vibe_core::ports::{EntityKind, Repositories};
use vibe_core::project_service::ProjectService;
use vibe_domain::{event_catalog, ActorType, Evidence, EvidenceType};
use vibe_mcp_server::{tools, McpServer, INTERNAL_ERROR, INVALID_PARAMS, INVALID_REQUEST, METHOD_NOT_FOUND, PARSE_ERROR, PROTOCOL_VERSION, SERVER_NAME};
use vibe_storage::SqliteStore;

struct Harness {
    #[allow(dead_code)]
    store: Arc<SqliteStore>,
    repos: Repositories,
    server: McpServer,
    project: String,
}

/// A project with the material every tool reads: two accepted task events, a confirmed
/// decision with a linked evidence row, and two indexed records.
fn harness() -> Harness {
    let store = SqliteStore::open_in_memory().expect("in-memory store");
    let repos = store.repositories();
    let project = ProjectService::new(repos.clone())
        .create_project(
            "Served",
            Some("the workbench as a tool server".to_string()),
            None,
            None,
        )
        .expect("project");
    let id = project.id.clone();

    let events = EventStore::new(repos.clone());
    for kind in [event_catalog::TASK_CREATED, event_catalog::TASK_STARTED] {
        events
            .append(
                EventDraft::new(&id, kind)
                    .actor(ActorType::User)
                    .subject("task", "task-1")
                    .payload(json!({ "title": "expose the workbench over a pipe" })),
            )
            .expect("task event");
    }
    events
        .append(
            EventDraft::new(&id, event_catalog::DECISION_CONFIRMED)
                .actor(ActorType::User)
                .subject("decision", "d-1")
                .payload(json!({
                    "title": "keep the MCP surface read-only",
                    "decision": "tools may read, never judge",
                    "rationale": "judgement is the user's (§29)",
                })),
        )
        .expect("decision event");

    let evidence = Evidence {
        id: "ev-1".into(),
        project_id: id.clone(),
        r#type: EvidenceType::Message,
        ref_id: Some("msg-1".into()),
        summary: Some("the turn the decision was made from: stdio, no network".into()),
        created_at: vibe_domain::now(),
    };
    repos
        .evidence
        .insert_evidence(&evidence)
        .expect("evidence row");
    repos
        .evidence
        .link("ev-1", EntityKind::Decision, "d-1")
        .expect("link");

    repos
        .search
        .index(
            "message",
            "msg-1",
            &id,
            "how the server is wired",
            "the pipe speaks zkphyre jsonrpc and reads through the ports",
        )
        .expect("index");
    repos
        .search
        .index(
            "task",
            "task-1",
            &id,
            "expose the workbench over a pipe",
            "a basic interface, six tools, no writes",
        )
        .expect("index");

    Harness {
        server: McpServer::new(repos.clone(), Some(id.clone())),
        repos,
        store,
        project: id,
    }
}

/// One request, one parsed answer. Panics with the raw line when the server stayed silent,
/// because a missing response is the loudest failure a protocol test can have.
fn ask(server: &McpServer, id: Value, method: &str, params: Value) -> Value {
    let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string();
    let answer = server
        .handle(&line)
        .unwrap_or_else(|| panic!("`{method}` answered nothing at all"));
    serde_json::from_str(&answer)
        .unwrap_or_else(|e| panic!("`{method}` wrote a broken line ({e}): {answer}"))
}

/// The `result` of a successful call, or the `error.message` of a failed one.
fn result_of(server: &McpServer, method: &str, params: Value) -> Value {
    let answer = ask(server, json!(1), method, params);
    if let Some(error) = answer.get("error") {
        panic!("{} was refused: {}", method, error);
    }
    answer["result"].clone()
}

/// The `type` of each event of a timeline answer, in the order the server sent them. Every
/// ordering assertion here is really about this list, and reading a list of names is easier
/// than reading four event records in a failure message.
fn event_types(rows: &[Value]) -> Vec<&str> {
    rows.iter()
        .map(|event| event["type"].as_str().unwrap_or_default())
        .collect()
}

/// A tool's content block, unwrapped: `isError` is returned alongside so a test can tell a
/// refusal from a protocol error instead of reading a string that happens to contain both.
fn call_tool(server: &McpServer, name: &str, args: Value) -> (Value, bool) {
    let result = result_of(server, "tools/call", json!({"name": name, "arguments": args}));
    let is_error = result["isError"].as_bool().unwrap_or(false);
    let text = result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("{name} sent no text block: {result}"));
    // A tool that returns markdown hands back a bare string; one that returns a record hands
    // back JSON text. Both are legitimate, so a test asks for whichever it expects.
    match serde_json::from_str(text) {
        Ok(parsed) => (parsed, is_error),
        Err(_) => (Value::String(text.to_string()), is_error),
    }
}

#[test]
fn a_client_can_shake_hands() {
    let work = harness();
    let hello = result_of(&work.server, "initialize", json!({"protocolVersion": "2024-11-05", "capabilities": {}}));
    assert_eq!(hello["protocolVersion"], json!(PROTOCOL_VERSION));
    assert_eq!(hello["serverInfo"]["name"], json!(SERVER_NAME));
    assert!(
        hello["capabilities"]["tools"].is_object(),
        "a client with no tools capability finds nothing to call: {hello}"
    );
    assert!(
        hello["instructions"]
            .as_str()
            .unwrap_or_default()
            .to_lowercase()
            .contains("read-only"),
        "a client has to be told the boundary before it offers a tool: {hello}"
    );
    assert!(hello["instructions"]
        .as_str()
        .unwrap_or_default()
        .contains("judgement"));

    // `ping` is the one message that must succeed before any project exists.
    assert_eq!(result_of(&work.server, "ping", json!({})), json!({}));

    // A notification is answered with silence, even a well-formed one.
    assert!(work
        .server
        .handle(r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#)
        .is_none());
    // …and so is one naming a method nobody speaks: replying to a notification is a protocol
    // violation regardless of what was asked.
    assert!(work
        .server
        .handle(r#"{"jsonrpc":"2.0","method":"notifications/frobnicated"}"#)
        .is_none());
}

#[test]
fn the_catalogue_lists_reads_and_no_writes() {
    let catalogue = result_of(&harness().server, "tools/list", json!({}))["tools"]
        .as_array()
        .expect("a list")
        .clone();
    let names: Vec<&str> = catalogue
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    for expected in [
        "project_state",
        "project_map",
        "project_timeline",
        "search",
        "open_evidence",
        "context_pack",
    ] {
        assert!(names.contains(&expected), "{expected} is missing from {names:?}");
    }
    for tool in &catalogue {
        let name = tool["name"].as_str().unwrap_or_default();
        // §29's judgement verbs are the user's, so nothing here may be named like one.
        for forbidden in ["confirm", "set_", "create", "delete", "resolve", "append"] {
            assert!(
                !name.contains(forbidden),
                "`{name}` reads like a write, and the MCP surface has no writes"
            );
        }
        assert!(
            tool["description"].as_str().unwrap_or_default().len() > 40,
            "a model picks tools by description: {tool}"
        );
        let schema = &tool["inputSchema"];
        assert_eq!(schema["type"], json!("object"), "{name} takes an object");
        assert_eq!(
            schema["additionalProperties"],
            json!(false),
            "{name} should reject a typo rather than ignore it"
        );
        assert!(
            schema["properties"]["projectId"].is_object(),
            "every tool can be pointed at a project: {name}"
        );
    }
    // The catalogue and the dispatcher are one list, not two that drift.
    assert_eq!(
        catalogue.len(),
        tools::definitions().len(),
        "tools/list and `call` disagree about how many tools exist"
    );
}

#[test]
fn state_and_map_answer_from_the_same_projection() {
    let work = harness();
    let (state, failed) = call_tool(&work.server, "project_state", json!({}));
    assert!(!failed, "project_state refused: {state}");
    assert_eq!(state["tasks"].as_array().unwrap().len(), 1, "{state}");
    assert_eq!(state["tasks"][0]["title"], json!("expose the workbench over a pipe"));
    assert_eq!(state["decisions"].as_array().unwrap().len(), 1);
    assert!(state["lastSeq"].as_u64().unwrap() >= 3, "{state}");

    let (map, failed) = call_tool(&work.server, "project_map", json!({}));
    assert!(!failed, "project_map refused: {map}");
    let markdown = map["markdown"].as_str().unwrap_or_default();
    assert!(
        markdown.contains("## Active Tasks"),
        "the map is a compiled view, not a summary: {markdown}"
    );
    assert!(markdown.contains("expose the workbench over a pipe"));
    assert!(
        !map["sourceStateHash"]
            .as_str()
            .unwrap_or_default()
            .is_empty(),
        "a map without the hash it was compiled from is not traceable (§33)"
    );
}

#[test]
fn the_timeline_is_the_trace_the_other_answers_cite() {
    let work = harness();
    let (events, failed) = call_tool(&work.server, "project_timeline", json!({}));
    assert!(!failed, "project_timeline refused: {events}");
    let rows = events.as_array().expect("a list of events");
    // `create_project` appends PROJECT_CREATED like everything else, so a real trace starts with
    // the project's own birth — four events, not the three the fixture writes by hand.
    assert_eq!(rows.len(), 4, "{:?}", event_types(rows));
    let seqs: Vec<u64> = rows
        .iter()
        .map(|event| event["seq"].as_u64().unwrap_or(0))
        .collect();
    assert!(
        seqs.windows(2).all(|pair| pair[0] < pair[1]),
        "oldest first, so a model reads a history in the order it happened: {seqs:?}"
    );
    assert_eq!(
        event_types(rows),
        vec![
            event_catalog::PROJECT_CREATED,
            event_catalog::TASK_CREATED,
            event_catalog::TASK_STARTED,
            event_catalog::DECISION_CONFIRMED,
        ],
        "the log is the whole story of the project, in order"
    );
    // §45's point: the payload a claim was made from is on the record, not paraphrased.
    let decision = &rows[3];
    assert!(
        serde_json::to_string(decision)
            .unwrap()
            .contains("judgement is the user"),
        "the decision's own words have to survive the round trip: {decision}"
    );
    assert_eq!(
        decision["status"],
        json!("accepted"),
        "a pending event would not belong in the projection: {decision}"
    );

    // The capped window is the *most recent* two, still oldest first: truncation and direction
    // are two different promises and a caller needs both.
    let (capped, _) = call_tool(&work.server, "project_timeline", json!({"limit": 2}));
    let window = capped.as_array().unwrap();
    assert_eq!(window.len(), 2, "--limit is honoured");
    assert_eq!(event_types(window), vec![event_catalog::TASK_STARTED, event_catalog::DECISION_CONFIRMED], "{window:?}");

    // One argument must not be able to ask for the whole database (§59).
    let answer = ask(
        &work.server,
        json!(2),
        "tools/call",
        json!({"name": "project_timeline", "arguments": {"limit": 5000}}),
    );
    assert_eq!(answer["error"]["code"], json!(INVALID_PARAMS), "{answer}");
    assert!(answer["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("capped at 200"));
}

#[test]
fn search_answers_from_the_index_and_stays_in_its_project() {
    let work = harness();
    let (hits, failed) = call_tool(&work.server, "search", json!({"query": "zkphyre"}));
    assert!(!failed, "search refused: {hits}");
    let rows = hits.as_array().expect("a list");
    assert_eq!(rows.len(), 1, "the rare token is in one record only: {rows:?}");
    assert_eq!(rows[0]["id"], json!("msg-1"));
    assert_eq!(rows[0]["projectId"], json!(work.project));
    assert!(
        matches!(rows[0]["layer"].as_str(), Some(layer) if !layer.is_empty()),
        "a hit that does not say which layer found it cannot be judged: {}",
        rows[0]
    );

    let (tasks, _) = call_tool(
        &work.server,
        "search",
        json!({"query": "pipe", "kind": ["task"]}),
    );
    assert!(
        tasks
            .as_array()
            .unwrap()
            .iter()
            .all(|hit| hit["kind"] == json!("task")),
        "--kind narrows the answer: {tasks}"
    );

    // A blank query is a scan, so it is refused rather than run.
    let answer = ask(
        &work.server,
        json!(3),
        "tools/call",
        json!({"name": "search", "arguments": {"query": "   "}}),
    );
    assert_eq!(answer["error"]["code"], json!(INVALID_PARAMS), "{answer}");

    // Isolation: a second project in the same database never appears in the first one's answer.
    let other = ProjectService::new(work.repos.clone())
        .create_project("Elsewhere", None, None, None)
        .expect("project");
    work.repos
        .search
        .index(
            "message",
            "msg-2",
            &other.id,
            "a note in another project",
            "zkphyre appears here too",
        )
        .expect("index");
    let (mine, _) = call_tool(&work.server, "search", json!({"query": "zkphyre"}));
    assert_eq!(
        mine.as_array().unwrap().len(),
        1,
        "the other project's record leaked into this answer"
    );
    let (theirs, _) = call_tool(
        &work.server,
        "search",
        json!({"query": "zkphyre", "projectId": other.id}),
    );
    assert_eq!(theirs.as_array().unwrap().len(), 1);
    assert_eq!(theirs.as_array().unwrap()[0]["id"], json!("msg-2"));
}

#[test]
fn evidence_opens_onto_the_record_that_cites_it() {
    let work = harness();
    let (rows, failed) = call_tool(&work.server, "open_evidence", json!({"target": "decision:d-1"}));
    assert!(!failed, "open_evidence refused: {rows}");
    let list = rows.as_array().expect("a list");
    assert_eq!(list.len(), 1, "{rows}");
    assert_eq!(list[0]["id"], json!("ev-1"));
    assert!(list[0]["summary"]
        .as_str()
        .unwrap_or_default()
        .contains("stdio"));

    // Both malformed shapes are the client's mistake, answered as one.
    for target in ["decision-without-a-colon", "not_a_kind:d-1"] {
        let answer = ask(
            &work.server,
            json!(4),
            "tools/call",
            json!({"name": "open_evidence", "arguments": {"target": target}}),
        );
        assert_eq!(
            answer["error"]["code"],
            json!(INVALID_PARAMS),
            "`{target}` should be refused: {answer}"
        );
    }
    // A record that cites nothing answers, and says so by being empty.
    let (empty, failed) = call_tool(&work.server, "open_evidence", json!({"target": "task:task-1"}));
    assert!(!failed, "an unlinked record is not an error: {empty}");
    assert!(empty.as_array().unwrap().is_empty());
}

/// §53 and §54 do not stop at the terminal: the pack a pipe carries is the same pack, with the
/// same policy applied and the same audit row left behind. Without this, the MCP surface would
/// be a way to read a project without recording that somebody did.
#[test]
fn a_pack_is_compiled_through_the_same_gate_and_audited() {
    let work = harness();
    let (pack, failed) = call_tool(
        &work.server,
        "context_pack",
        json!({"question": "why is the server read only", "focus": ["ports"]}),
    );
    assert!(!failed, "context_pack refused: {pack}");
    let markdown = pack["markdown"].as_str().unwrap_or_default();
    assert!(
        markdown.contains('#'),
        "a pack with no sections is not a pack: {markdown}"
    );
    assert!(
        pack["items"].as_u64().unwrap_or(0) >= 1,
        "nothing was retrieved for a question that matches an indexed record: {pack}"
    );
    assert!(
        !pack["packId"].as_str().unwrap_or_default().is_empty(),
        "a pack a caller cannot cite afterwards is not auditable"
    );
    assert!(pack["policyApplied"].is_object(), "{pack}");

    let entries = work.repos.audit.list(&work.project, 20).expect("audit");
    assert!(
        !entries.is_empty(),
        "the pack was built but nobody recorded that it was read (§54)"
    );
}

#[test]
fn a_refusal_is_a_result_and_a_mistake_is_an_error() {
    // Three failure shapes, told apart on purpose: an unknown tool, an unknown project, and a
    // server pinned to a project that is not in the database.
    let work = harness();
    let answer = ask(
        &work.server,
        json!(5),
        "tools/call",
        json!({"name": "projec_state", "arguments": {}}),
    );
    assert_eq!(answer["error"]["code"], json!(INVALID_PARAMS), "{answer}");
    assert!(
        answer["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("projec_state"),
        "the message should name what the client said"
    );

    let answer = ask(
        &work.server,
        json!(6),
        "tools/call",
        json!({"name": "project_state", "arguments": {"projectId": "no-such-project"}}),
    );
    assert_eq!(answer["error"]["code"], json!(INVALID_PARAMS), "{answer}");

    let ghost = McpServer::new(work.repos.clone(), Some("the-server-was-told-wrong".into()));
    let (text, failed) = call_tool(&ghost, "project_state", json!({}));
    assert!(
        failed,
        "a pinned project that does not exist is a workbench answer, not a client typo: {text}"
    );
    assert!(text["error"]
        .as_str()
        .unwrap_or_default()
        .contains("--project"));
}

/// Two projects and no pointer: the server must say which ones exist rather than pick one,
/// because guessing a project would make one project's facts answer for another.
#[test]
fn an_ambiguous_project_is_named_rather_than_guessed() {
    let work = harness();
    let second = ProjectService::new(work.repos.clone())
        .create_project("Second", None, None, None)
        .expect("project");
    let unpinned = McpServer::new(work.repos.clone(), None);

    let (text, failed) = call_tool(&unpinned, "project_state", json!({}));
    assert!(failed, "two projects, no answer expected: {text}");
    let message = text["error"].as_str().unwrap_or_default();
    assert!(
        message.contains("2 projects exist"),
        "the refusal should count them: {message}"
    );
    assert!(
        message.contains(&second.name) && message.contains(&work.project),
        "and name them: {message}"
    );

    // Naming one is all the call needed.
    let (state, failed) = call_tool(&unpinned, "project_state", json!({"projectId": work.project}));
    assert!(!failed, "an explicit id resolves: {state}");
    assert_eq!(state["tasks"].as_array().unwrap().len(), 1);
}

#[test]
fn a_broken_line_is_answered_not_dropped() {
    let work = harness();
    let parse = work.server.handle("this is not json").expect("an answer");
    let parsed: Value = serde_json::from_str(&parse).expect("valid JSON out");
    assert_eq!(parsed["error"]["code"], json!(PARSE_ERROR), "{parsed}");
    assert_eq!(parsed["id"], json!(null), "a line with no id can be answered with none");

    let no_method: Value =
        serde_json::from_str(&work.server.handle(r#"{"jsonrpc":"2.0","id":7}"#).unwrap())
            .unwrap();
    assert_eq!(no_method["error"]["code"], json!(INVALID_REQUEST), "{no_method}");

    let unknown = ask(&work.server, json!(8), "projects/whatever", json!({}));
    assert_eq!(unknown["error"]["code"], json!(METHOD_NOT_FOUND), "{unknown}");
    assert!(unknown["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("tools/list"), "the refusal should say what to call instead");

    let no_name = ask(&work.server, json!(9), "tools/call", json!({"arguments": {}}));
    assert_eq!(no_name["error"]["code"], json!(INVALID_PARAMS), "{no_name}");

    // An internal bug would be -32603; nothing here produces one, so the constant is only
    // referenced to keep the code table honest.
    assert_eq!(INTERNAL_ERROR, -32603);
}

/// `serve` is the only part that touches a stream, and the transport's one hard rule is that a
/// message never contains an embedded newline. The map tool is chosen on purpose: its markdown
/// is full of newlines, which must arrive escaped inside the JSON string.
#[test]
fn a_pipe_carries_one_message_per_line() {
    let work = harness();
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"project_map","arguments":{}}}"#,
        "not json at all",
    ]
    .join("\n");
    let mut output = Vec::new();
    let answered = work
        .server
        .serve(input.as_bytes(), &mut output)
        .expect("the loop runs to stdin's end");
    assert_eq!(answered, 3, "two requests and one broken line, no notification");

    let text = String::from_utf8(output).expect("utf-8 out");
    let lines: Vec<&str> = text.lines().filter(|line| !line.trim().is_empty()).collect();
    assert_eq!(lines.len(), 3, "one answer per line, and nothing between them");
    for line in &lines {
        let parsed: Value = serde_json::from_str(line).expect("every line is one message");
        assert!(parsed["jsonrpc"].is_string(), "the envelope is JSON-RPC 2.0");
    }
    let markdown = lines[1];
    assert!(
        markdown.contains("\\n"),
        "the markdown's newlines have to travel escaped: {markdown}"
    );
    // The garbage line got answered in-band, in order, and did not break the framing for the
    // line after it — which is the whole difficulty of a line-delimited transport.
    let broken: Value = serde_json::from_str(lines[2]).expect("framing survived");
    assert_eq!(broken["error"]["code"], json!(PARSE_ERROR), "{broken}");
}

/// The library tests above prove the protocol; this one proves the *process*, because a client
/// only ever sees the binary: stdout must carry JSON-RPC and nothing else, diagnostics must go
/// to stderr, and a closed stdin must be a clean exit rather than a hang.
#[test]
fn the_built_binary_serves_a_real_pipe() {
    let dir = tempfile::tempdir().expect("temp dir");
    let db = dir.path().join("workbench.db");

    let wire = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
        "not json at all",
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"project_state","arguments":{}}}"#,
    ]
    .join("\n");

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_wp-mcp"))
        .arg("--db")
        .arg(&db)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("wp-mcp starts");
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().expect("a stdin to write into");
        stdin.write_all(wire.as_bytes()).expect("the wire is writable");
        stdin.flush().expect("flushed");
    }
    let done = child.wait_with_output().expect("the child exits when stdin closes");
    assert!(done.status.success(), "a closed stdin is a clean exit, not a hang");

    let stdout = String::from_utf8(done.stdout).expect("utf-8 out");
    let stderr = String::from_utf8_lossy(&done.stderr).to_string();
    let lines: Vec<&str> = stdout.lines().filter(|line| !line.trim().is_empty()).collect();
    assert_eq!(lines.len(), 4, "four answers, and the notification got none: {stdout}");
    let answers: Vec<Value> = lines
        .iter()
        .map(|line| serde_json::from_str(line).expect("every line is one message"))
        .collect();
    for answer in &answers {
        assert_eq!(answer["jsonrpc"], json!("2.0"), "{answer}");
    }
    assert_eq!(answers[0]["result"]["serverInfo"]["name"], json!(SERVER_NAME));
    assert_eq!(
        answers[1]["result"]["tools"].as_array().unwrap().len(),
        tools::definitions().len(),
        "a client sees the same catalogue the library publishes"
    );
    assert_eq!(
        answers[2]["error"]["code"],
        json!(PARSE_ERROR),
        "{}",
        answers[2]
    );
    // A brand-new database holds no project, and the honest answer is an `isError` result that
    // names the command to run — a refusal, not a crash and not an empty state.
    let empty = &answers[3];
    assert_eq!(empty["result"]["isError"], json!(true), "{empty}");
    assert!(
        empty["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("wp project create"),
        "the refusal should tell the agent what to do: {empty}"
    );

    assert!(stderr.contains("stdio"), "diagnostics go to stderr: {stderr}");
    assert!(!stdout.contains("wp-mcp:"), "stdout is protocol and nothing else: {stdout}");
    assert!(db.exists(), "the pipe opened a real workbench file: {stderr}");
}
