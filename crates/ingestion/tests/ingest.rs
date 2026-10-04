//! The pipeline's three contract tests (V1_DEVELOPMENT.md §51, §52, §58): a re-run changes
//! nothing, two adapters in one project stay two separate sources, and one failing source
//! costs nothing to the others.

use std::sync::Arc;

use vibe_core::error::CoreError;
use vibe_core::ports::{MessageInsertOutcome, Repositories};
use vibe_core::project_service::ProjectService;
use vibe_domain::{
    AgentCapabilities, AgentTarget, Capability, Message, NormalizedConversationBatch, SourceType,
    SyncCursor,
};
use vibe_ingestion::{
    AdapterReport, AgentAdapter, GenericCliAdapter, IngestPipeline, ManualImportAdapter,
    TranscriptAdapter,
};
use vibe_storage::SqliteStore;

fn repos() -> (Arc<SqliteStore>, Repositories, String) {
    let store = SqliteStore::open_in_memory().expect("in-memory store");
    let repositories = store.repositories();
    let project = ProjectService::new(repositories.clone())
        .create_project("Capture", None, None, None)
        .expect("project");
    (store, repositories, project.id)
}

fn all_messages(repositories: &Repositories, project_id: &str) -> Vec<Message> {
    let mut out = Vec::new();
    for conversation in repositories
        .conversations
        .list_conversations(project_id)
        .expect("conversations")
    {
        out.extend(
            repositories
                .conversations
                .list_messages(&conversation.id)
                .expect("messages"),
        );
    }
    out
}

fn event_count(repositories: &Repositories, project_id: &str) -> usize {
    repositories
        .events
        .list_events(project_id, None, None)
        .expect("events")
        .len()
}

fn cursor(repositories: &Repositories, adapter_id: &str, source_id: &str) -> Option<String> {
    repositories
        .conversations
        .get_cursor(adapter_id, source_id)
        .expect("cursor read")
        .and_then(|c| c.cursor)
}

fn source_id_of(repositories: &Repositories, project_id: &str, external_id: &str) -> String {
    repositories
        .conversations
        .list_sources(project_id)
        .expect("sources")
        .into_iter()
        .find(|source| source.external_id.as_deref() == Some(external_id))
        .unwrap_or_else(|| panic!("source {external_id} was never created"))
        .id
}

/// §51 + §52: the same adapter run twice adds no message and no event, and the stored
/// cursor moves exactly once.
#[test]
fn re_running_an_adapter_yields_no_duplicates_and_one_cursor_move() {
    let (_store, repositories, project) = repos();
    let mut pipeline = IngestPipeline::new(repositories.clone(), None);
    pipeline.register(Arc::new(ManualImportAdapter::named(
        "manual-thread",
        "user: 确认保留 Repository 抽象层\n\nassistant: we should switch to an LRU cache",
    )));

    let first = pipeline.run(&project).expect("first run");
    assert_eq!(first.total_new_messages, 2, "both turns are new");
    assert_eq!(first.total_events, 2, "one accepted decision, one pending proposal");
    let source = source_id_of(&repositories, &project, "manual-thread");
    let after_first = cursor(&repositories, "manual-thread", &source);
    assert_eq!(after_first.as_deref(), Some("resume:2"), "cursor advanced");

    let messages_after_first = all_messages(&repositories, &project).len();
    let events_after_first = event_count(&repositories, &project);

    let second = pipeline.run(&project).expect("second run");
    assert_eq!(second.total_new_messages, 0);
    assert_eq!(second.total_events, 0);
    assert_eq!(second.total_duplicates, 0, "nothing was even re-sent");
    assert_eq!(
        cursor(&repositories, "manual-thread", &source).as_deref(),
        after_first.as_deref(),
        "the cursor moved once, not twice"
    );
    assert_eq!(all_messages(&repositories, &project).len(), messages_after_first);
    assert_eq!(event_count(&repositories, &project), events_after_first);

    // The second shape a real adapter has: a full export that re-emits everything and
    // leans on dedup. Idempotency must hold for it too, just by a different route.
    let mut snapshot_pipeline = IngestPipeline::new(repositories.clone(), None);
    snapshot_pipeline.register(Arc::new(TranscriptAdapter::new(
        "export-thread",
        SourceType::ChatgptWeb,
        r#"[{"role":"user","content":"确认保留导出格式"},{"role":"assistant","content":"好的"}]"#,
    )));
    let snapshot_first = snapshot_pipeline.run(&project).expect("snapshot run 1");
    assert_eq!(snapshot_first.total_new_messages, 2);
    let snapshot_source = source_id_of(&repositories, &project, "export-thread");
    let snapshot_cursor = cursor(&repositories, "export-thread", &snapshot_source);
    assert!(
        snapshot_cursor.as_deref().unwrap_or_default().starts_with("snapshot:2:"),
        "a snapshot cursor records the position: {snapshot_cursor:?}"
    );

    let snapshot_second = snapshot_pipeline.run(&project).expect("snapshot run 2");
    assert_eq!(snapshot_second.total_new_messages, 0);
    assert_eq!(snapshot_second.total_events, 0);
    assert_eq!(
        snapshot_second.per_adapter[0].messages_duplicate, 2,
        "the port's dedup outcome is the truth (§52 L2)"
    );
    assert_eq!(all_messages(&repositories, &project).len(), messages_after_first + 2);
    assert_eq!(event_count(&repositories, &project), events_after_first + 1);
}

/// §38 two sources, one project (§58): a ChatGPT-shaped export and a CLI session dump live
/// side by side without leaking into each other.
#[test]
fn two_adapters_in_one_project_stay_two_sources() {
    let temp = tempfile::tempdir().expect("tempdir");
    let session = temp.path().join("session-42.jsonl");
    std::fs::write(
        &session,
        concat!(
            "{\"role\":\"user\",\"content\":\"cli session only: 给 CLI 会话加一层缓存\"}\n",
            "{\"role\":\"tool\",\"content\":\"cli session only: 读取的目录由调用方注入\"}\n",
        ),
    )
    .expect("write session");

    let (_store, repositories, project) = repos();
    let mut pipeline = IngestPipeline::new(repositories.clone(), None);
    pipeline.register(Arc::new(TranscriptAdapter::new(
        "chatgpt-main",
        SourceType::ChatgptWeb,
        serde_json::json!({
            "id": "conv-chatgpt-1",
            "title": "Search design",
            "mapping": {
                "n0": { "parent": null, "children": ["n1"], "message": null },
                "n1": {
                    "parent": "n0",
                    "children": ["n2"],
                    "message": { "author": "user", "content": { "parts": ["chatgpt only: 确认保留 Repository 抽象层"] } }
                },
                "n2": {
                    "parent": "n1",
                    "children": [],
                    "message": { "author": "assistant", "content": { "parts": ["chatgpt only: 我建议加一层缓存"] } }
                }
            }
        })
        .to_string(),
    )));
    pipeline.register(Arc::new(
        GenericCliAdapter::with_id("cli-agent", temp.path()).on_file("session-42.jsonl"),
    ));

    let report = pipeline.run(&project).expect("run");
    assert_eq!(report.per_adapter.len(), 2);
    assert!(report.failures().is_empty(), "{:?}", report.failures());
    assert_eq!(report.total_new_messages, 4);

    let sources = repositories
        .conversations
        .list_sources(&project)
        .expect("sources");
    assert_eq!(sources.len(), 2, "one source per adapter");
    let types: Vec<&str> = sources.iter().map(|s| s.r#type.as_str()).collect();
    assert!(types.contains(&"chatgpt_web") && types.contains(&"generic_cli"), "{types:?}");

    let conversations = repositories
        .conversations
        .list_conversations(&project)
        .expect("conversations");
    assert_eq!(conversations.len(), 2);
    for conversation in &conversations {
        let messages = repositories
            .conversations
            .list_messages(&conversation.id)
            .expect("messages");
        assert!(!messages.is_empty());
        let owner = sources
            .iter()
            .find(|s| s.id == conversation.source_id)
            .expect("conversation owns a known source");
        // No cross-contamination: every line of a thread came from that thread's adapter.
        let expected_tag = match owner.r#type {
            SourceType::ChatgptWeb => "chatgpt only:",
            _ => "cli session only:",
        };
        let foreign = match owner.r#type {
            SourceType::ChatgptWeb => "cli session only:",
            _ => "chatgpt only:",
        };
        for message in &messages {
            assert!(
                message.content.contains(expected_tag),
                "{expected_tag} missing from {:?}",
                message.content
            );
            assert!(
                !message.content.contains(foreign),
                "{} leaked into the {} source",
                foreign,
                owner.r#type.as_str()
            );
            assert_eq!(message.conversation_id, conversation.id);
        }
    }

    // Cursors are per (adapter, source), so one source's position never reads as another's.
    let chatgpt_source = sources
        .iter()
        .find(|s| s.r#type == SourceType::ChatgptWeb)
        .expect("chatgpt source");
    let cli_source = sources
        .iter()
        .find(|s| s.r#type == SourceType::GenericCli)
        .expect("cli source");
    assert!(cursor(&repositories, "chatgpt-main", &chatgpt_source.id).is_some());
    assert_eq!(
        cursor(&repositories, "cli-agent", &cli_source.id).as_deref(),
        Some("resume:2")
    );
    assert_ne!(chatgpt_source.id, cli_source.id);
}

/// A source that fails is reported and skipped; the others still commit and still advance
/// their own cursors (§51).
#[test]
fn one_failing_source_does_not_cost_the_others() {
    struct Broken;
    impl AgentAdapter for Broken {
        fn id(&self) -> &str {
            "broken-cli"
        }
        fn source_type(&self) -> SourceType {
            SourceType::GenericCli
        }
        fn detect(&self) -> Result<bool, CoreError> {
            Ok(true)
        }
        fn capabilities(&self) -> AgentCapabilities {
            AgentCapabilities {
                adapter_id: "broken-cli".into(),
                capabilities: vec![Capability::ReadHistory],
            }
        }
        fn ingest(&self, _cursor: Option<&SyncCursor>) -> Result<NormalizedConversationBatch, CoreError> {
            Err(CoreError::Io("session file vanished".into()))
        }
    }

    let (_store, repositories, project) = repos();
    let mut pipeline = IngestPipeline::new(repositories.clone(), None);
    pipeline.register(Arc::new(ManualImportAdapter::named(
        "alpha",
        "user: 接下来做 搜索层的测试覆盖",
    )));
    pipeline.register(Arc::new(Broken));
    pipeline.register(Arc::new(ManualImportAdapter::named(
        "beta",
        "user: 确认保留 三源并行采集",
    )));

    let report = pipeline.run(&project).expect("run");
    assert_eq!(report.per_adapter.len(), 3);
    let failures = report.failures();
    assert_eq!(failures.len(), 1, "exactly one source failed");
    assert_eq!(failures[0].adapter_id, "broken-cli");
    assert!(
        failures[0]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("session file vanished"),
        "the report carries the reason: {:?}",
        failures[0].error
    );

    let alpha = report
        .per_adapter
        .iter()
        .find(|r: &&AdapterReport| r.adapter_id == "alpha")
        .expect("alpha");
    let beta = report
        .per_adapter
        .iter()
        .find(|r| r.adapter_id == "beta")
        .expect("beta");
    assert_eq!(alpha.messages_inserted, 1);
    assert_eq!(beta.messages_inserted, 1);
    assert!(alpha.events_appended >= 1 && beta.events_appended >= 1);
    assert_eq!(report.total_new_messages, 2);

    let alpha_source = source_id_of(&repositories, &project, "alpha");
    let beta_source = source_id_of(&repositories, &project, "beta");
    let broken_source = source_id_of(&repositories, &project, "broken-cli");
    assert!(cursor(&repositories, "alpha", &alpha_source).is_some());
    assert!(cursor(&repositories, "beta", &beta_source).is_some());
    assert_eq!(
        cursor(&repositories, "broken-cli", &broken_source),
        None,
        "a failed adapter never advances its cursor"
    );

    // Retrying the run commits the survivors once: the failed source still has no cursor,
    // and the others still have nothing new to send.
    let retry = pipeline.run(&project).expect("retry");
    assert_eq!(retry.failures().len(), 1);
    assert_eq!(retry.total_new_messages, 0);
    assert!(cursor(&repositories, "broken-cli", &broken_source).is_none());
}

/// §36/§37/§58: adapters are registered by id and capability, injection is optional, and
/// a vendor name is data — never a branch.
#[test]
fn adapters_register_by_id_and_nothing_branches_on_a_vendor_name() {
    let (_store, repositories, project) = repos();
    let mut pipeline = IngestPipeline::new(repositories.clone(), None);

    let manual = Arc::new(ManualImportAdapter::text("user: 确认保留 注册表按 id 覆盖"));
    let text_payload = r#"[{"role":"human","content":"确认保留 同一份读取器"},{"role":"bot","content":"收到"}]"#;
    pipeline.register(Arc::new(TranscriptAdapter::new(
        "as-chatgpt",
        SourceType::ChatgptWeb,
        text_payload,
    )));
    assert_eq!(pipeline.adapters().len(), 1);
    // Re-registering the same id replaces, so a config reload cannot double-capture.
    pipeline.register(Arc::new(TranscriptAdapter::new(
        "as-chatgpt",
        SourceType::ChatgptWeb,
        text_payload,
    )));
    assert_eq!(pipeline.adapters().len(), 1);
    pipeline.register(manual.clone());
    assert_eq!(pipeline.adapters().len(), 2);
    assert!(pipeline.adapter("as-chatgpt").is_some());

    // The same reader under a different SourceType is the same behaviour: the vendor is a
    // column value, not a code path.
    let mut other = IngestPipeline::new(repositories.clone(), None);
    other.register(Arc::new(TranscriptAdapter::new(
        "as-qoder",
        SourceType::Qoder,
        text_payload,
    )));
    let chatgpt_run = pipeline.run_one(&project, "as-chatgpt").expect("chatgpt");
    let qoder_run = other.run_one(&project, "as-qoder").expect("qoder");
    assert_eq!(
        chatgpt_run.messages_inserted,
        qoder_run.messages_inserted,
        "identical payload, identical result, whatever the source type is called"
    );
    assert_eq!(chatgpt_run.messages_inserted, 2);

    // Every built-in adapter honestly reports read-only capability and no injection.
    for adapter in [&manual] {
        let capabilities = adapter.capabilities();
        assert!(capabilities.has(Capability::ReadHistory));
        assert!(!capabilities.has(Capability::AutoExecute), "§37: never claimed in V1");
        assert_eq!(capabilities.adapter_id, adapter.id());
        assert!(adapter
            .open(&AgentTarget::Web)
            .expect("open")
            .is_none());
        assert!(adapter.prepare("payload").expect("prepare").is_none());
        assert!(adapter.deep_link(&AgentTarget::Web).is_none());
    }
    // A deep link is opt-in data on the adapter, still not a vendor branch.
    let linked = TranscriptAdapter::new("linked", SourceType::ChatgptWeb, text_payload)
        .with_deep_link_base("https://example.test/thread/");
    assert_eq!(
        linked.deep_link(&AgentTarget::Web).as_deref(),
        Some("https://example.test/thread/linked-thread")
    );
    assert!(linked.capabilities().has(Capability::OpenThread));

    assert!(matches!(
        pipeline.run_one(&project, "never-registered"),
        Err(CoreError::NotFound { .. })
    ));
}

/// The dedup outcome is the port's, not the pipeline's guess.
#[test]
fn duplicate_count_comes_from_the_port() {
    let (_store, repositories, project) = repos();
    let pipeline = IngestPipeline::new(repositories.clone(), None);
    let now = vibe_domain::now();
    let messages: Vec<Message> = vibe_ingestion::Normalizer::pairs(
        "",
        &[("user", "确认保留 去重口径"), ("user", "确认保留  去重口径")],
        now,
    );
    let batch = vibe_ingestion::adapter::blank_batch(
        Some("padded-thread".into()),
        Some("padded".into()),
        messages,
        None,
    );
    let report = pipeline
        .ingest_messages(&project, &batch)
        .expect("ingest manual batch");
    assert_eq!(report.messages_inserted, 1);
    assert_eq!(
        report.messages_duplicate, 1,
        "whitespace padding is the same message (§52 L2)"
    );

    let outcome: MessageInsertOutcome = repositories
        .conversations
        .insert_messages(
            &repositories.conversations.list_conversations(&project).expect("c")[0]
                .id,
            &batch.messages,
        )
        .expect("re-insert");
    assert!(outcome.inserted.is_empty(), "the same call repeats nothing");
}
