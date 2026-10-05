//! §34/§35 from the host's side of the pipe: the frame codec, the five verifications, and
//! what a browser push actually does to the workbench.
//!
//! The shape of these tests follows §35's own list — one refusal per named check, because a
//! check nobody asserts is a check that quietly stops working. The second half is §58's
//! E2E row: material pushed from a page must become Messages and Events in the same project
//! the user is looking at, and pushing it twice must change nothing.

use std::io::Cursor;
use std::sync::Arc;

use serde_json::{Value, json};
use vibe_core::ports::Repositories;
use vibe_core::project_service::ProjectService;
use vibe_ingestion::IngestPipeline;
use vibe_ingestion::native_messaging::{
    HostManifest, HostPolicy, MAX_FRAME_BYTES, NativeHost, frame_len, read_frame, write_frame,
};
use vibe_state_engine::Reducer;
use vibe_storage::SqliteStore;

fn workbench() -> (Arc<SqliteStore>, Repositories, String) {
    let store = SqliteStore::open_in_memory().expect("in-memory store");
    let repositories = store.repositories();
    let project = ProjectService::new(repositories.clone())
        .create_project("Browser", None, None, None)
        .expect("project");
    (store, repositories, project.id)
}

/// One request frame's body, as the extension writes it.
fn request(command: &str, origin: &str, project: Option<&str>, payload: Value) -> String {
    json!({
        "protocol": "codlas-nm/1",
        "origin": origin,
        "command": command,
        "project": project,
        "payload": payload
    })
    .to_string()
}

fn capture_body(conversation: &str, source: &str, content: &str) -> Value {
    json!({
        "source": source,
        "conversationId": conversation,
        "title": "login 500s",
        "messages": [
            {"role": "user", "content": content, "messageId": "m-1"},
            {"role": "assistant", "content": "我看看", "messageId": "m-2"}
        ]
    })
}

/// Ask the host one question and read the reply envelope back.
fn ask(host: &NativeHost, raw: String) -> Value {
    serde_json::from_str(&host.handle(&raw)).expect("the host always answers in JSON")
}

fn frame(bytes: &[u8]) -> Vec<u8> {
    let mut out = (bytes.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(bytes);
    out
}

/// `CoreError`'s `Display` puts its own category in front of every message
/// (`validation: origin rejected: …`), so an assertion on the raw string would be testing
/// the prefix instead of §35's check. Every refusal assertion goes through here.
fn check(text: &str) -> &str {
    text.split_once(": ").map_or(text, |(_, rest)| rest)
}

// --------------------------------------------------------------------------- the codec

#[test]
fn a_frame_is_four_little_endian_bytes_then_json() {
    let body = r#"{"command":"hello"}"#;
    let mut buffer = Vec::new();
    write_frame(&mut buffer, body).expect("write");
    assert_eq!(buffer.len(), frame_len(body));
    assert_eq!(&buffer[..4], &(body.len() as u32).to_le_bytes());

    let mut reader = Cursor::new(buffer);
    assert_eq!(read_frame(&mut reader).expect("read").as_deref(), Some(body));
    // The peer closing is how Chrome ends a session: no frame, no error.
    assert!(read_frame(&mut reader).expect("clean eof").is_none());
}

#[test]
fn a_header_that_lies_about_its_size_is_refused_before_anything_is_allocated() {
    // §35's `size` check is the codec's job, and this is why: a four-byte header can
    // declare up to 4 GiB, and a host that trusted it is a memory exhaustion bug. This is
    // a bare lying header with no body at all — the frame the codec must survive.
    let oversize = (MAX_FRAME_BYTES as u32 + 1).to_le_bytes().to_vec();
    let error = read_frame(&mut Cursor::new(oversize)).expect_err("over-long frame");
    assert!(
        check(&error.to_string()).starts_with("size rejected"),
        "the refusal must name its check: {error}"
    );

    let empty = (0u32).to_le_bytes().to_vec();
    assert!(
        check(&read_frame(&mut Cursor::new(empty)).expect_err("empty frame").to_string())
            .starts_with("size rejected")
    );
    // A header that promises more than the pipe carries must not hang or half-parse.
    let mut truncated = frame(b"{\"command\"");
    truncated.truncate(truncated.len() - 2);
    assert!(read_frame(&mut Cursor::new(truncated)).is_err());
    // Nor may the body be arbitrary bytes.
    let binary = frame(&[0xff, 0xfe, 0xfd, 0xfc, 0x00]);
    let error = read_frame(&mut Cursor::new(binary)).expect_err("not UTF-8");
    assert!(
        check(&error.to_string()).starts_with("payload rejected"),
        "{error}"
    );
}

#[test]
fn a_reply_round_trips_through_the_same_codec_as_a_request() {
    let reply = json!({"ok": true, "data": {"action": "openTab"}}).to_string();
    let mut buffer = Vec::new();
    write_frame(&mut buffer, &reply).expect("write");
    let mut reader = Cursor::new(buffer);
    assert_eq!(read_frame(&mut reader).expect("read").as_deref(), Some(reply.as_str()));
}

// ---------------------------------------------------------------------- §35's five checks

#[test]
fn each_of_the_five_native_messaging_checks_has_its_own_refusal() {
    let (_store, repos, project) = workbench();
    let pipeline = IngestPipeline::with_rules(repos.clone());
    let host = NativeHost::new(&pipeline, HostPolicy::default()).for_project(Some(project));

    // 1. origin — an unknown page, and a well-formed origin wearing a path.
    let mut cases = vec![(
        "origin rejected",
        request("capture", "https://evil.example", None, capture_body("c-1", "chatgpt_web", "x")),
    )];
    // 2. command — an unknown verb, then a protocol this host does not speak.
    cases.push((
        "command rejected",
        request("exfiltrate", "https://chatgpt.com", None, json!({})),
    ));
    cases.push({
        let raw = request("hello", "https://chatgpt.com", None, json!({}))
            .replace("codlas-nm/1", "codlas-nm/2");
        ("command rejected", raw)
    });
    // 3. payload — a capture with nothing in it, a role the domain has not got, and a
    //    source that is not a web source.
    cases.push((
        "payload rejected",
        request("capture", "https://chatgpt.com", None, json!({"source":"chatgpt_web","conversationId":"c","messages":[]})),
    ));
    cases.push((
        "payload rejected",
        request("capture", "https://chatgpt.com", None, json!({"source":"chatgpt_web","conversationId":"c","messages":[{"role":"wizard","content":"hi"}]})),
    ));
    cases.push((
        "payload rejected",
        request("capture", "https://chatgpt.com", None, capture_body("c-9", "git", "报错：迁移失败")),
    ));
    // 4. size — a legal command carrying an illegal body.
    cases.push((
        "size rejected",
        request(
            "capture",
            "https://chatgpt.com",
            None,
            capture_body("c-3", "chatgpt_web", &"重".repeat(120_000)),
        ),
    ));
    // 5. project scope — a project this workbench has never heard of.
    cases.push((
        "project scope rejected",
        request("capture", "https://chatgpt.com", Some("nope"), capture_body("c-4", "chatgpt_web", "x")),
    ));

    for (name, raw) in cases {
        let reply = ask(&host, raw);
        assert_eq!(reply["ok"], Value::Bool(false), "{name} should have refused: {reply}");
        let error = reply["error"].as_str().unwrap_or_default();
        assert!(
            check(error).starts_with(name),
            "expected a `{name}` refusal, got: {error}"
        );
    }
}

#[test]
fn a_subdomain_of_a_trusted_origin_is_trusted_and_a_suffix_lookalike_is_not() {
    let (_store, repos, project) = workbench();
    let pipeline = IngestPipeline::with_rules(repos.clone());
    let host = NativeHost::new(&pipeline, HostPolicy::default()).for_project(Some(project));

    for (origin, allowed) in [
        ("https://chatgpt.com", true),
        ("https://x.chatgpt.com", true),
        ("https://chatgpt.com.evil.test", false),
        ("https://notchatgpt.com", false),
        ("http://chatgpt.com", false),
        ("https://chatgpt.com/some/path", false),
    ] {
        let reply = ask(&host, request("hello", origin, None, json!({})));
        assert_eq!(reply["ok"], Value::Bool(allowed), "{origin} → {reply}");
    }
}

#[test]
fn a_consented_origin_opens_the_door_it_was_named_for() {
    let (_store, repos, project) = workbench();
    let pipeline = IngestPipeline::with_rules(repos.clone());
    let policy = HostPolicy::default()
        .allow_origin("https://aidebug.internal")
        .allow_origin("https://aidebug.internal");
    let host = NativeHost::new(&pipeline, policy).for_project(Some(project));

    let hello = ask(&host, request("hello", "https://aidebug.internal", None, json!({})));
    assert_eq!(hello["ok"], Value::Bool(true));
    assert!(
        hello["data"]["allowedOrigins"]
            .as_array()
            .expect("list")
            .iter()
            .filter(|o| o.as_str() == Some("https://aidebug.internal"))
            .count()
            == 1,
        "a repeated --allow-origin must not duplicate: {}",
        hello["data"]["allowedOrigins"]
    );
    assert_eq!(hello["data"]["autoExecute"], Value::Bool(false));
}

// ------------------------------------------------------------- what a push does (§12-§15)

#[test]
fn a_pushed_thread_becomes_messages_an_issue_and_a_web_source() {
    let (_store, repos, project) = workbench();
    let pipeline = IngestPipeline::with_rules(repos.clone());
    let host = NativeHost::new(&pipeline, HostPolicy::default()).for_project(Some(project.clone()));

    let reply = ask(
        &host,
        request(
            "capture",
            "https://chatgpt.com",
            None,
            capture_body("conv-77", "chatgpt_web", "登录接口报错，返回 500"),
        ),
    );
    assert_eq!(reply["ok"], Value::Bool(true), "{}", reply["error"]);
    let data = &reply["data"];
    assert_eq!(data["action"], "captured");
    assert_eq!(data["messages"]["inserted"], 2);
    assert_eq!(data["messages"]["duplicate"], 0);
    assert!(data["events"]["appended"].as_u64().expect("count") >= 1);
    assert_eq!(data["project"], project.as_str());

    let sources = repos
        .conversations
        .list_sources(&project)
        .expect("sources");
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].r#type.as_str(), "chatgpt_web");
    assert_eq!(sources[0].external_id.as_deref(), Some("conv-77"));

    // §18: the accepted event is what a state view is built from, so the push has to reach
    // the projection and not stop at the log.
    let state = Reducer::new(repos.clone())
        .project_read_only(&project)
        .expect("projection");
    assert_eq!(state.issues.len(), 1, "the issue should be open in state");
    assert_eq!(state.issues[0].title, "登录接口报错，返回 500");
}

#[test]
fn the_same_push_twice_adds_no_message_and_no_event() {
    let (_store, repos, project) = workbench();
    let pipeline = IngestPipeline::with_rules(repos.clone());
    let host = NativeHost::new(&pipeline, HostPolicy::default()).for_project(Some(project));

    let raw = request("capture", "https://chatgpt.com", None, capture_body("conv-1", "chatgpt_web", "登录接口报错"));
    let first = ask(&host, raw.clone());
    let second = ask(&host, raw);
    assert_eq!(first["data"]["messages"]["inserted"], 2);
    assert_eq!(second["data"]["messages"]["inserted"], 0, "Stage D: a repeat sync is empty");
    assert_eq!(second["data"]["messages"]["duplicate"], 2);
    assert_eq!(second["data"]["events"]["appended"], 0);
}

#[test]
fn two_products_capture_into_one_project_and_stay_two_sources() {
    let (_store, repos, project) = workbench();
    let pipeline = IngestPipeline::with_rules(repos.clone());
    let host = NativeHost::new(&pipeline, HostPolicy::default()).for_project(Some(project.clone()));

    let chatgpt = ask(
        &host,
        request("capture", "https://chatgpt.com", None, capture_body("gpt-1", "chatgpt_web", "登录接口报错")),
    );
    let generic = ask(
        &host,
        request("capture", "https://claude.ai", None, capture_body("web-1", "generic_web", "构建失败，需要先修依赖")),
    );
    assert_eq!(chatgpt["ok"], Value::Bool(true));
    assert_eq!(generic["ok"], Value::Bool(true));
    assert_ne!(chatgpt["data"]["sourceId"], generic["data"]["sourceId"]);

    let mut types: Vec<String> = repos
        .conversations
        .list_sources(&project)
        .expect("sources")
        .iter()
        .map(|s| s.r#type.as_str().to_string())
        .collect();
    types.sort();
    assert_eq!(types, vec!["chatgpt_web", "generic_web"]);
}

#[test]
fn the_popup_names_itself_on_hello_and_open_and_a_page_on_the_page_commands() {
    let (_store, repos, project) = workbench();
    let pipeline = IngestPipeline::with_rules(repos.clone());
    let host = NativeHost::new(&pipeline, HostPolicy::default()).for_project(Some(project));

    // §35's origin check has two faces: 自检 and 打开目标网页 have no tab to point at, so
    // the extension's own origin is what they honestly have.
    let itself = "chrome-extension://abcdefghijklmnopabcdefghijklmnop";
    let hello = ask(&host, request("hello", itself, None, json!({})));
    assert_eq!(hello["ok"], Value::Bool(true), "{hello}");
    assert_eq!(hello["data"]["autoExecute"], Value::Bool(false));

    let opened = ask(
        &host,
        request("open", "chrome-extension://abcdefghijklmnopabcdefghijklmnop/", None, json!({"url": "https://chatgpt.com/"})),
    );
    assert_eq!(opened["data"]["action"], "openTab");

    // The exemption covers the two verbs that carry no page material, not the channel.
    for (command, payload) in [
        ("capture", capture_body("c-ext", "chatgpt_web", "报错：迁移失败")),
        ("inject", json!({"text": "# Project Context"})),
    ] {
        let reply = ask(&host, request(command, itself, None, payload));
        assert_eq!(reply["ok"], Value::Bool(false), "{command} has to name a page");
        assert!(
            check(reply["error"].as_str().unwrap_or_default()).starts_with("origin rejected"),
            "{reply}"
        );
    }

    // And a page origin cannot borrow the exemption's scheme.
    let dressed = ask(&host, request("hello", "chrome-extension://id/settings.html", None, json!({})));
    assert_eq!(dressed["ok"], Value::Bool(false), "a path is not an origin");
}

// ------------------------------------------------------- §34's other two duties + serve

#[test]
fn open_and_inject_answer_with_an_instruction_and_never_a_send() {
    let (_store, repos, project) = workbench();
    let pipeline = IngestPipeline::with_rules(repos.clone());
    let host = NativeHost::new(&pipeline, HostPolicy::default()).for_project(Some(project));

    let opened = ask(
        &host,
        request("open", "https://chatgpt.com", None, json!({"url": "https://chatgpt.com/g/g-1"})),
    );
    assert_eq!(opened["data"]["action"], "openTab");
    assert_eq!(opened["data"]["url"], "https://chatgpt.com/g/g-1");

    // A prepared pack must not be able to point the user at a lookalike page.
    let spoofed = ask(
        &host,
        request("open", "https://chatgpt.com", None, json!({"url": "https://chatgpt.com.evil.test/"})),
    );
    assert_eq!(spoofed["ok"], Value::Bool(false));
    assert!(
        check(spoofed["error"].as_str().unwrap_or_default()).starts_with("origin rejected"),
        "{spoofed}"
    );

    let filled = ask(
        &host,
        request("inject", "https://chatgpt.com", None, json!({"text": "# Project Context\n…"})),
    );
    assert_eq!(filled["data"]["action"], "fillComposer");
    assert_eq!(filled["data"]["send"], Value::Bool(false), "§7.8: sending is a human action");
    assert_eq!(filled["data"]["text"], "# Project Context\n…");

    assert!(
        ask(&host, request("inject", "https://chatgpt.com", None, json!({"text": "  "})))["ok"]
            == Value::Bool(false)
    );
}

#[test]
fn one_bad_frame_does_not_end_the_session() {
    let (_store, repos, project) = workbench();
    let pipeline = IngestPipeline::with_rules(repos.clone());
    let host = NativeHost::new(&pipeline, HostPolicy::default()).for_project(Some(project.clone()));

    let mut input = Vec::new();
    write_frame(&mut input, &request("capture", "https://evil.example", None, capture_body("c", "chatgpt_web", "x"))).expect("f1");
    write_frame(&mut input, &request("hello", "https://chatgpt.com", None, json!({}))).expect("f2");
    let mut reader = Cursor::new(input);
    let mut out: Vec<u8> = Vec::new();
    let handled = host.serve(&mut reader, &mut out).expect("serve");

    assert_eq!(handled, 2, "§62: a refusal is an answer, not a closed channel");
    let mut reader = Cursor::new(out);
    let first: Value = serde_json::from_str(&read_frame(&mut reader).expect("r1").expect("f1"))
        .expect("reply 1 is JSON");
    let second: Value = serde_json::from_str(&read_frame(&mut reader).expect("r2").expect("f2"))
        .expect("reply 2 is JSON");
    assert_eq!(first["ok"], Value::Bool(false));
    assert_eq!(second["ok"], Value::Bool(true));
    assert_eq!(second["data"]["project"], project);
}

#[test]
fn the_host_record_lists_the_extensions_chrome_may_launch() {
    let record: Value =
        serde_json::from_str(&HostManifest::new("/Users/me/.vibe/host/app.codlas.workbench.host.sh", &["abcdefghijklmnopabcdefghijklmnop".to_string(), "chrome-extension://zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz/".to_string()]).render())
            .expect("Chrome reads this file, so it must parse");
    assert_eq!(record["name"], "app.codlas.workbench.host");
    assert_eq!(record["type"], "stdio");
    assert_eq!(
        record["path"],
        "/Users/me/.vibe/host/app.codlas.workbench.host.sh"
    );
    let origins = record["allowed_origins"].as_array().expect("origins");
    assert_eq!(origins.len(), 2, "snake_case is Chrome's spelling, and both ids survive");
    assert_eq!(origins[0], "chrome-extension://abcdefghijklmnopabcdefghijklmnop/");
    assert_eq!(origins[1], "chrome-extension://zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz/");
    assert!(
        serde_json::to_value(&HostManifest::new("/x", &[]))
            .expect("serialisable")
            .get("allowed_origins")
            .is_some(),
        "an empty list still has to be a valid record"
    );
}
