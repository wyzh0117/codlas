//! §58's Adapter Contract as a checklist rather than a paragraph. The doc lists seven
//! verbs — detect, capabilities, ingest, open, prepare, inject, fallback — and the point of
//! listing them is that a caller can rely on all seven on *any* adapter. So this file runs
//! one review over each adapter the crate ships, over a reader that implements only the
//! required five, and over two synthetic extenders.
//!
//! The rule the review is built around is honesty rather than shape: `open`/`prepare`/`inject`
//! default to "cannot", and an adapter may not answer a verb it has not advertised in
//! `capabilities()` (§37). That is the one failure a SDK default invites, which is why the
//! negative control below is as important as the clean cases — a checker that cannot fail is
//! not a contract.

use std::path::Path;

use vibe_core::error::{CoreError, Result};
use vibe_domain::{
    AgentCapabilities, AgentTarget, Capability, Fallback, InjectResult, NormalizedConversationBatch,
    PreparedPayload, SourceType, SyncCursor,
};
use vibe_ingestion::{
    AgentAdapter, GenericCliAdapter, ManualImportAdapter, TranscriptAdapter,
};

/// Every §58 verb, one problem string per violation so a failing test names the verb and the
/// capability instead of tripping an `assert!` three layers down.
fn review(name: &str, adapter: &dyn AgentAdapter) -> Vec<String> {
    let mut problems = Vec::new();
    let id = adapter.id().to_string();
    if id.is_empty() {
        problems.push(format!("{name}: id() is empty — sources and cursors are keyed on it"));
    }
    if adapter.id() != id.as_str() {
        problems.push(format!("{name}: id() is not the same string twice in a row"));
    }

    let caps = adapter.capabilities();
    if caps.adapter_id != id {
        problems.push(format!(
            "{name}: capabilities() names '{}' while id() says '{id}'",
            caps.adapter_id
        ));
    }
    if caps.has(Capability::AutoExecute) {
        problems.push(format!(
            "{name}: claims AUTO_EXECUTE, which §37 forbids any adapter from claiming in V1"
        ));
    }

    match adapter.detect() {
        Ok(found) => {
            if adapter.detect().unwrap_or(!found) != found {
                problems.push(format!("{name}: detect() answered twice with two different bools"));
            }
        }
        Err(error) => problems.push(format!(
            "{name}: detect() returned an error instead of a bool — an absent source is data \
             (§62), not a failure: {error}"
        )),
    }

    // The read verb. A source that is not there is allowed to say so; a source that reads
    // differently twice in a row is not, because §59's incremental sync is part of the
    // contract rather than something the pipeline adds afterwards.
    if let Ok(first) = adapter.ingest(None) {
        match adapter.ingest(None) {
            Ok(again) if again.messages.len() != first.messages.len() => problems.push(format!(
                "{name}: two reads of the same source returned {} and {} messages",
                first.messages.len(),
                again.messages.len()
            )),
            Err(error) => problems.push(format!("{name}: ingest(None) read once and then failed: {error}")),
            _ => {}
        }
        // Identity belongs to the pipeline (§7.6, §51): an adapter that picks its own project
        // would be a second state source.
        if !first.conversation.project_id.is_empty() || !first.conversation.source_id.is_empty() {
            problems.push(format!(
                "{name}: the batch carries a project or source id — blank_batch() exists so \
                 adapters cannot choose identity"
            ));
        }
    }

    let payload = PreparedPayload {
        adapter_id: id.clone(),
        target: AgentTarget::Web,
        text: "a pack a user asked for".to_string(),
        deep_link: None,
        copy_fallback: true,
    };
    let stages_text = caps.has(Capability::InjectText);
    if !stages_text {
        if !matches!(adapter.prepare("a pack"), Ok(None)) {
            problems.push(format!("{name}: prepare() answered without advertising INJECT_TEXT"));
        }
        if !matches!(adapter.inject(&payload), Ok(None)) {
            problems.push(format!("{name}: inject() answered without advertising INJECT_TEXT"));
        }
    }
    if !(caps.has(Capability::OpenApp) || caps.has(Capability::OpenThread))
        && !matches!(adapter.open(&AgentTarget::Web), Ok(None))
    {
        problems.push(format!(
            "{name}: open() answered without advertising OPEN_APP or OPEN_THREAD"
        ));
    }
    let link = adapter.deep_link(&AgentTarget::Web);
    if !caps.has(Capability::OpenThread) && link.is_some() {
        problems.push(format!("{name}: deep_link() returned a link without advertising OPEN_THREAD"));
    }
    if let Some(raw) = &link {
        if !raw.contains("://") {
            problems.push(format!("{name}: deep_link() is not something a person can open: {raw}"));
        }
    }
    // An adapter that *does* advertise the verb still has to give an answer a user can read:
    // a verdict with no message is how a silent failure gets serialized.
    if stages_text && matches!(adapter.inject(&payload), Ok(Some(result)) if result.message.trim().is_empty())
    {
        problems.push(format!("{name}: inject() returned a verdict with nothing in it"));
    }

    // §58's seventh verb, and the one with no `Option`: it always answers (§62).
    let fallback = adapter.fallback(&AgentTarget::Web);
    if fallback.adapter_id != id {
        problems.push(format!("{name}: fallback() names a different adapter"));
    }
    if fallback.instruction.trim().is_empty() {
        problems.push(format!("{name}: fallback() says nothing — that silence is §62's failure"));
    }
    if fallback.deep_link != link {
        problems.push(format!("{name}: fallback() dropped the link the adapter knows"));
    }
    for claim in ["sent ", "delivered ", "injected "] {
        if fallback.instruction.contains(claim) {
            problems.push(format!(
                "{name}: fallback() claims the pack was already {} — the workbench sends \
                 nothing: {}",
                claim.trim(),
                fallback.instruction
            ));
        }
    }

    problems
}

fn assert_clean(name: &str, adapter: &dyn AgentAdapter) {
    let problems = review(name, adapter);
    assert!(problems.is_empty(), "contract broken:\n{}", problems.join("\n"));
}

/// A source directory the adapter is pointed at, kept alive by the caller.
fn session_dir(turns: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(dir.path().join("session.json"), turns).expect("session dump");
    dir
}

const TURNS: &str = r#"[
  {"role":"user","content":"the adapter contract needs inject and fallback too"},
  {"role":"assistant","content":"defaults answer cannot, so a reader still satisfies it"}
]"#;

#[test]
fn a_pasted_manual_meets_every_verb() {
    assert_clean(
        "manual",
        &ManualImportAdapter::text("user: the reducer folds accepted events only\n"),
    );
}

#[test]
fn a_transcript_export_meets_every_verb() {
    assert_clean(
        "transcript",
        &TranscriptAdapter::new("contract-transcript", SourceType::ChatgptWeb, TURNS),
    );
}

#[test]
fn a_thread_link_shows_up_in_the_fallback_it_belongs_to() {
    // OPEN_THREAD is the one capability a read-only adapter can earn without writing
    // anything, and §62's degraded path is the link, so the two have to agree.
    let adapter = TranscriptAdapter::new("linked", SourceType::ChatgptWeb, TURNS)
        .with_deep_link_base("https://chat.example/thread/");
    assert_clean("linked transcript", &adapter);
    let caps = adapter.capabilities();
    assert!(
        caps.has(Capability::OpenThread),
        "a base url is exactly the OPEN_THREAD claim: {:?}",
        caps.capabilities
    );
    let fallback = adapter.fallback(&AgentTarget::Web);
    assert!(
        fallback
            .deep_link
            .as_deref()
            .unwrap_or_default()
            .starts_with("https://chat.example/thread/"),
        "the link the adapter knows has to be the one it hands out: {fallback:?}"
    );
}

#[test]
fn a_local_session_dump_meets_every_verb() {
    let dir = session_dir(TURNS);
    assert_clean(
        "cli session",
        &GenericCliAdapter::new(dir.path().to_path_buf()),
    );
}

#[test]
fn an_absent_source_is_a_fact_not_a_breach() {
    // §62 Unavailable: detect() must return `false`, not an error, and the verbs must still
    // answer. A broken source is allowed to be silent about *content*, never about shape.
    let adapter = GenericCliAdapter::new(Path::new("/no/such/session/directory"));
    assert!(!adapter.detect().expect("a bool, not a failure"));
    assert_clean("missing source", &adapter);
}

#[test]
fn a_reader_that_implements_only_the_required_verbs_still_answers_all_seven() {
    struct Bare;
    impl AgentAdapter for Bare {
        fn id(&self) -> &str {
            "bare"
        }
        fn source_type(&self) -> SourceType {
            SourceType::Manual
        }
        fn detect(&self) -> Result<bool> {
            Ok(true)
        }
        fn capabilities(&self) -> AgentCapabilities {
            AgentCapabilities {
                adapter_id: "bare".to_string(),
                capabilities: vec![Capability::ReadHistory],
            }
        }
        fn ingest(&self, _cursor: Option<&SyncCursor>) -> Result<NormalizedConversationBatch> {
            Ok(NormalizedConversationBatch::default())
        }
    }

    assert_clean("bare", &Bare);
    let adapter = Bare;
    // The defaults are the "cannot" answers, and `fallback` is the verb that never is.
    assert!(matches!(adapter.open(&AgentTarget::Cli), Ok(None)));
    assert!(matches!(adapter.prepare("pack"), Ok(None)));
    assert!(adapter
        .inject(&PreparedPayload {
            adapter_id: "bare".into(),
            target: AgentTarget::Cli,
            text: "pack".into(),
            deep_link: None,
            copy_fallback: true,
        })
        .map(|answer| answer.is_none())
        .expect("inject cannot fail by default"));
    assert!(adapter.deep_link(&AgentTarget::Cli).is_none());
    let fallback = adapter.fallback(&AgentTarget::Cli);
    assert_eq!(fallback.adapter_id, "bare");
    assert!(
        fallback
            .instruction
            .contains("paste the prepared pack into Cli yourself"),
        "the fallback has to name the target: {}",
        fallback.instruction
    );
}

#[test]
fn an_adapter_may_not_answer_a_verb_it_did_not_advertise() {
    /// Same impls as `Willing` below, no capabilities claiming them — §37's dishonest reader,
    /// written to prove `review` can actually fail.
    struct Boastful {
        base: String,
    }

    impl AgentAdapter for Boastful {
        fn id(&self) -> &str {
            "boastful"
        }
        fn source_type(&self) -> SourceType {
            SourceType::Manual
        }
        fn detect(&self) -> Result<bool> {
            Ok(true)
        }
        fn capabilities(&self) -> AgentCapabilities {
            AgentCapabilities {
                adapter_id: "boastful".to_string(),
                capabilities: vec![Capability::ReadHistory],
            }
        }
        fn ingest(&self, _cursor: Option<&SyncCursor>) -> Result<NormalizedConversationBatch> {
            Ok(NormalizedConversationBatch::default())
        }
        fn open(&self, _target: &AgentTarget) -> Result<Option<InjectResult>> {
            Ok(Some(InjectResult {
                ok: true,
                message: "brought the window forward".into(),
            }))
        }
        fn prepare(&self, payload: &str) -> Result<Option<PreparedPayload>> {
            Ok(Some(PreparedPayload {
                adapter_id: self.id().to_string(),
                target: AgentTarget::Web,
                text: payload.to_string(),
                deep_link: None,
                copy_fallback: false,
            }))
        }
        fn inject(&self, payload: &PreparedPayload) -> Result<Option<InjectResult>> {
            Ok(Some(InjectResult {
                ok: true,
                message: format!("{} characters staged", payload.text.chars().count()),
            }))
        }
        fn deep_link(&self, _target: &AgentTarget) -> Option<String> {
            Some(format!("{}thread-1", self.base))
        }
    }

    let problems = review(
        "boastful",
        &Boastful {
            base: "https://chat.example/".to_string(),
        },
    );
    let text = problems.join("\n");
    for verb in ["prepare()", "inject()", "open()", "deep_link()"] {
        assert!(
            text.contains(verb),
            "{verb} was answered without a capability and the review missed it:\n{text}"
        );
    }
}

#[test]
fn an_honest_extender_passes_the_same_review() {
    /// V1 ships no adapter that writes to an agent (§7.8: the user carries the pack), but the
    /// contract is only real if an adapter that *does* implement the write-side verbs passes
    /// the same review by advertising them. This is that adapter, as a test fixture.
    struct Willing;

    impl AgentAdapter for Willing {
        fn id(&self) -> &str {
            "willing"
        }
        fn source_type(&self) -> SourceType {
            SourceType::Manual
        }
        fn detect(&self) -> Result<bool> {
            Ok(true)
        }
        fn capabilities(&self) -> AgentCapabilities {
            AgentCapabilities {
                adapter_id: "willing".to_string(),
                capabilities: vec![
                    Capability::ReadHistory,
                    Capability::OpenApp,
                    Capability::OpenThread,
                    Capability::InjectText,
                ],
            }
        }
        fn ingest(&self, _cursor: Option<&SyncCursor>) -> Result<NormalizedConversationBatch> {
            Ok(NormalizedConversationBatch::default())
        }
        fn open(&self, _target: &AgentTarget) -> Result<Option<InjectResult>> {
            Ok(Some(InjectResult {
                ok: true,
                message: "the thread is in front of you".into(),
            }))
        }
        fn prepare(&self, payload: &str) -> Result<Option<PreparedPayload>> {
            Ok(Some(PreparedPayload {
                adapter_id: self.id().to_string(),
                target: AgentTarget::Web,
                text: payload.to_string(),
                deep_link: Some("https://chat.example/thread/1".into()),
                copy_fallback: false,
            }))
        }
        fn inject(&self, payload: &PreparedPayload) -> Result<Option<InjectResult>> {
            Ok(Some(InjectResult {
                ok: true,
                message: format!("{} characters staged", payload.text.chars().count()),
            }))
        }
        fn deep_link(&self, _target: &AgentTarget) -> Option<String> {
            Some("https://chat.example/thread/1".into())
        }
    }

    assert_clean("willing", &Willing);
}

#[test]
fn a_fallback_the_adapter_writes_by_hand_is_still_a_fallback() {
    /// `Fallback` is a struct, so an adapter can replace the default wording — e.g. one that
    /// wrote the payload to a file. The review's no-claim rule is what keeps that honest.
    struct WritesAFile;

    impl AgentAdapter for WritesAFile {
        fn id(&self) -> &str {
            "writes-a-file"
        }
        fn source_type(&self) -> SourceType {
            SourceType::File
        }
        fn detect(&self) -> Result<bool> {
            Ok(true)
        }
        fn capabilities(&self) -> AgentCapabilities {
            AgentCapabilities {
                adapter_id: "writes-a-file".to_string(),
                capabilities: vec![Capability::ReadHistory, Capability::FileContext],
            }
        }
        fn ingest(&self, _cursor: Option<&SyncCursor>) -> Result<NormalizedConversationBatch> {
            Err(CoreError::NotFound {
                what: "nothing to read".into(),
            })
        }
        fn fallback(&self, _target: &AgentTarget) -> Fallback {
            Fallback {
                adapter_id: self.id().to_string(),
                instruction: "open .vibe/pack.md and paste the section into the agent".into(),
                deep_link: None,
                payload_path: Some(".vibe/pack.md".into()),
            }
        }
    }

    assert_clean("writes a file", &WritesAFile);
    assert_eq!(
        WritesAFile.fallback(&AgentTarget::Manual).payload_path.as_deref(),
        Some(".vibe/pack.md"),
        "payload_path is the field that exists for this adapter"
    );
}

#[test]
fn a_resumed_cursor_asks_for_nothing_new_when_the_reader_keeps_its_own_position() {
    // §51 allows two cursor shapes, and a contract test that recognised only one would call
    // the other a bug. `resume:<count>` is the reader's own position, so `ingest` honours it;
    // a `snapshot:<len>:<hash>` id is a claim about the batch, which the reader deliberately
    // ignores and §52's L1/L2 dedup turns into zero new rows (proved in `ingest.rs`).
    let manual = ManualImportAdapter::text("user: the reducer folds accepted events only\n");
    let transcript = TranscriptAdapter::new("resume", SourceType::ChatgptWeb, TURNS);
    for (name, adapter) in [
        ("manual", &manual as &dyn AgentAdapter),
        ("transcript", &transcript as &dyn AgentAdapter),
    ] {
        let first = adapter.ingest(None).expect("a batch");
        assert!(
            !first.messages.is_empty(),
            "{name}: nothing was read, so the cursor says nothing either"
        );
        let next = first
            .next_cursor
            .clone()
            .unwrap_or_else(|| panic!("{name}: an adapter may omit a cursor, these two claim one"));
        let resumed = adapter
            .ingest(Some(&SyncCursor {
                adapter_id: name.into(),
                source_id: String::new(),
                cursor: Some(next.clone()),
                last_seen_at: None,
                last_content_hash: None,
            }))
            .expect("a resumed read never fails on a cursor this adapter just returned");
        if next.starts_with("resume:") {
            assert!(
                resumed.messages.is_empty(),
                "{name}: everything before {next} was already handed over, but {} rows came \
                 back",
                resumed.messages.len()
            );
        } else {
            assert_eq!(
                resumed.messages.len(),
                first.messages.len(),
                "{name}: an opaque cursor is an assumption-free re-read"
            );
        }
    }

    let dir = session_dir(TURNS);
    let cli = GenericCliAdapter::new(dir.path().to_path_buf());
    let read = cli.ingest(None).expect("a session dump");
    let resumed = cli
        .ingest(Some(&SyncCursor {
            adapter_id: read.conversation.source_id.clone(),
            source_id: String::new(),
            cursor: read.next_cursor.clone(),
            last_seen_at: None,
            last_content_hash: None,
        }))
        .expect("the same cursor shape is honoured by every shipped reader");
    assert!(
        resumed.messages.is_empty(),
        "a second pass over an unchanged dump must not resend it"
    );
}
