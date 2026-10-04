//! The V1 adapter set (V1_DEVELOPMENT.md §38): Manual Import, a Generic CLI session dump
//! and a ChatGPT-style transcript export.
//!
//! Every path here is *handed in*. An adapter holds a `PathBuf` the caller chose, or a
//! string the caller pasted; there is no default vendor directory anywhere in this crate,
//! because §7.6 forbids the core from knowing where any product stores anything.
//!
//! `TranscriptAdapter` takes its [`SourceType`] as constructor data on purpose: the same
//! reader serves a ChatGPT export, a Qoder export or a hand-rolled JSON dump. Registering
//! it under `chatgpt_web` is a *registration* decision, never a branch.

use std::path::{Path, PathBuf};

use serde_json::Value;
use vibe_core::error::{CoreError, Result};
use vibe_domain::{
    AgentCapabilities, Capability, Conversation, Message, NormalizedConversationBatch, SourceType,
    SyncCursor,
};

use crate::adapter::{blank_batch, read_text, AgentAdapter};
use crate::normalizer::Normalizer;

/// §38.1 — pasted chat text, or a file of messages the user pointed at.
pub struct ManualImportAdapter {
    id: String,
    content: ManualContent,
}

/// Where the manual material lives.
#[derive(Debug, Clone)]
pub enum ManualContent {
    Text(String),
    File(PathBuf),
}

impl ManualImportAdapter {
    /// The documented source id for manual capture is `manual`.
    pub const DEFAULT_ID: &'static str = "manual";

    pub fn text(content: impl Into<String>) -> Self {
        Self {
            id: Self::DEFAULT_ID.to_string(),
            content: ManualContent::Text(content.into()),
        }
    }

    /// A named manual source, so two pasted threads can coexist as two `Source` rows.
    pub fn named(id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            content: ManualContent::Text(content.into()),
        }
    }

    /// A file of messages. The path is caller-supplied; nothing here guesses a location.
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self {
            id: Self::DEFAULT_ID.to_string(),
            content: ManualContent::File(path.into()),
        }
    }

    pub fn file_as(id: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            id: id.into(),
            content: ManualContent::File(path.into()),
        }
    }

    fn raw(&self) -> Result<String> {
        match &self.content {
            ManualContent::Text(text) => Ok(text.clone()),
            ManualContent::File(path) => read_text(path),
        }
    }
}

impl AgentAdapter for ManualImportAdapter {
    fn id(&self) -> &str {
        &self.id
    }

    fn source_type(&self) -> SourceType {
        SourceType::Manual
    }

    fn detect(&self) -> Result<bool> {
        Ok(match &self.content {
            ManualContent::Text(text) => !text.trim().is_empty(),
            ManualContent::File(path) => path.is_file(),
        })
    }

    fn capabilities(&self) -> AgentCapabilities {
        let mut capabilities = vec![Capability::ReadHistory];
        if matches!(self.content, ManualContent::File(_)) {
            capabilities.push(Capability::FileContext);
        }
        AgentCapabilities {
            adapter_id: self.id.clone(),
            capabilities,
        }
    }

    fn ingest(&self, cursor: Option<&SyncCursor>) -> Result<NormalizedConversationBatch> {
        let observed_at = vibe_domain::now();
        let mut messages = Normalizer::plain_text("", &self.raw()?, observed_at);
        let total = messages.len();
        skip_already_seen(cursor, &mut messages);
        Ok(blank_batch(
            Some(self.id.clone()),
            Some(format!("manual import · {}", self.id)),
            messages,
            Some(format!("resume:{total}")),
        ))
    }
}

/// §38.4 — a JSONL or JSON conversation dump written by a local agent session directory.
pub struct GenericCliAdapter {
    id: String,
    dir: PathBuf,
    file: Option<String>,
}

impl GenericCliAdapter {
    pub const DEFAULT_ID: &'static str = "generic-cli";

    /// `dir` is the injected session directory — a caller's own choice of path.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            id: Self::DEFAULT_ID.to_string(),
            dir: dir.into(),
            file: None,
        }
    }

    pub fn with_id(id: impl Into<String>, dir: impl Into<PathBuf>) -> Self {
        Self {
            id: id.into(),
            dir: dir.into(),
            file: None,
        }
    }

    /// Pin one session file when the directory holds several.
    pub fn on_file(mut self, name: impl Into<String>) -> Self {
        self.file = Some(name.into());
        self
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn candidates(&self) -> Vec<PathBuf> {
        let mut entries: Vec<PathBuf> = match std::fs::read_dir(&self.dir) {
            Ok(reader) => reader
                .filter_map(|entry| entry.ok().map(|e| e.path()))
                .filter(|path| {
                    path.is_file()
                        && matches!(
                            path.extension().and_then(|e| e.to_str()),
                            Some("jsonl") | Some("json")
                        )
                })
                .collect(),
            Err(_) => return Vec::new(),
        };
        // Deterministic pick, so two runs on the same directory ingest the same file.
        entries.sort();
        entries
    }

    fn resolve_file(&self) -> Result<PathBuf> {
        if let Some(name) = &self.file {
            let path = self.dir.join(name);
            if !path.is_file() {
                return Err(CoreError::NotFound {
                    what: format!("session file {}", path.display()),
                });
            }
            return Ok(path);
        }
        let candidates = self.candidates();
        match candidates.len() {
            0 => Err(CoreError::NotFound {
                what: format!(
                    "no .jsonl / .json session dump under {}",
                    self.dir.display()
                ),
            }),
            1 => Ok(candidates[0].clone()),
            _ => Err(CoreError::Validation(format!(
                "{} holds {} session files; register one adapter per session with on_file()",
                self.dir.display(),
                candidates.len()
            ))),
        }
    }
}

impl AgentAdapter for GenericCliAdapter {
    fn id(&self) -> &str {
        &self.id
    }

    fn source_type(&self) -> SourceType {
        SourceType::GenericCli
    }

    fn detect(&self) -> Result<bool> {
        Ok(self.dir.is_dir() && !self.candidates().is_empty())
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            adapter_id: self.id.clone(),
            capabilities: vec![
                Capability::ReadHistory,
                Capability::ReadLocalSession,
                Capability::FileContext,
            ],
        }
    }

    fn ingest(&self, cursor: Option<&SyncCursor>) -> Result<NormalizedConversationBatch> {
        let path = self.resolve_file()?;
        let raw = read_text(&path)?;
        let observed_at = vibe_domain::now();
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("session")
            .to_string();

        // A `.jsonl` dump is a session log written one record per line; a `.json` dump is
        // either a turn array, an object with a turn list, or a multi-conversation export.
        let (messages, title) = match path.extension().and_then(|e| e.to_str()) {
            Some("jsonl") => (Normalizer::jsonl("", &raw, observed_at)?, None),
            _ => {
                let value: Value = serde_json::from_str(&raw)
                    .map_err(|e| CoreError::Encoding(format!("{}: {e}", path.display())))?;
                let selected = Normalizer::select_conversation(&value, None);
                (
                    Normalizer::json("", &selected, observed_at)?,
                    Normalizer::conversation_title(&selected),
                )
            }
        };
        let total = messages.len();
        let mut messages = messages;
        skip_already_seen(cursor, &mut messages);

        Ok(blank_batch(
            Some(format!("session-{stem}")),
            Some(title.unwrap_or_else(|| format!("cli session · {stem}"))),
            messages,
            Some(format!("resume:{total}")),
        ))
    }
}

/// §38.3 — a transcript export: JSON (ChatGPT `mapping` shape, or a `{role, content}`
/// array) or JSONL, from a string or an injected file path.
pub struct TranscriptAdapter {
    id: String,
    source_type: SourceType,
    content: TranscriptContent,
    conversation: Option<String>,
    deep_link_base: Option<String>,
}

#[derive(Debug, Clone)]
pub enum TranscriptContent {
    Text(String),
    File(PathBuf),
}

impl TranscriptAdapter {
    /// A reader for one exported thread, registered under `source_type` as data.
    pub fn new(id: impl Into<String>, source_type: SourceType, raw: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            source_type,
            content: TranscriptContent::Text(raw.into()),
            conversation: None,
            deep_link_base: None,
        }
    }

    pub fn file(
        id: impl Into<String>,
        source_type: SourceType,
        path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            id: id.into(),
            source_type,
            content: TranscriptContent::File(path.into()),
            conversation: None,
            deep_link_base: None,
        }
    }

    /// Pick one thread out of a multi-conversation export by its id or title.
    pub fn on_conversation(mut self, external_id: impl Into<String>) -> Self {
        self.conversation = Some(external_id.into());
        self
    }

    /// URL prefix used to build a deep link back to the thread. Supplied by the caller,
    /// never derived from a vendor name inside this crate.
    pub fn with_deep_link_base(mut self, base: impl Into<String>) -> Self {
        self.deep_link_base = Some(base.into());
        self
    }

    fn raw(&self) -> Result<String> {
        match &self.content {
            TranscriptContent::Text(text) => Ok(text.clone()),
            TranscriptContent::File(path) => read_text(path),
        }
    }

    fn external_id(&self) -> String {
        self.conversation
            .clone()
            .unwrap_or_else(|| format!("{}-thread", self.id))
    }
}

impl AgentAdapter for TranscriptAdapter {
    fn id(&self) -> &str {
        &self.id
    }

    fn source_type(&self) -> SourceType {
        self.source_type
    }

    fn detect(&self) -> Result<bool> {
        Ok(match &self.content {
            TranscriptContent::Text(text) => !text.trim().is_empty(),
            TranscriptContent::File(path) => path.is_file(),
        })
    }

    fn capabilities(&self) -> AgentCapabilities {
        let mut capabilities = vec![Capability::ReadHistory, Capability::ReadCurrentThread];
        if matches!(self.content, TranscriptContent::File(_)) {
            capabilities.push(Capability::FileContext);
        }
        if self.deep_link_base.is_some() {
            capabilities.push(Capability::OpenThread);
        }
        AgentCapabilities {
            adapter_id: self.id.clone(),
            capabilities,
        }
    }

    fn ingest(&self, cursor: Option<&SyncCursor>) -> Result<NormalizedConversationBatch> {
        let raw = self.raw()?;
        let observed_at = vibe_domain::now();
        let trimmed = raw.trim_start();
        let messages = if trimmed.starts_with('[') || trimmed.starts_with('{') {
            let value: Value = serde_json::from_str(trimmed)
                .map_err(|e| CoreError::Encoding(format!("{} transcript: {e}", self.id)))?;
            let selected =
                Normalizer::select_conversation(&value, self.conversation.as_deref());
            let title = Normalizer::conversation_title(&selected);
            let external = self.conversation.clone().or_else(|| {
                Normalizer::external_conversation_id(&selected)
            });
            let messages = Normalizer::json("", &selected, observed_at)?;
            return Ok(finalize_transcript(
                self,
                cursor,
                messages,
                external,
                title,
            ));
        } else {
            Normalizer::jsonl("", trimmed, observed_at)?
        };
        Ok(finalize_transcript(
            self,
            cursor,
            messages,
            None,
            None,
        ))
    }

    fn deep_link(&self, _target: &vibe_domain::AgentTarget) -> Option<String> {
        let base = self.deep_link_base.as_deref()?;
        let id = self.external_id();
        // The base is honoured exactly as the caller wrote it: a prefix that already ends
        // in a separator (`/`, `?`, `=`) gets the id appended, anything else gets one `/`.
        // A scheme never ends this way, so no real URL is mangled either way.
        Some(if base.ends_with(['/', '?', '=', '&']) {
            format!("{base}{id}")
        } else {
            format!("{base}/{id}")
        })
    }
}

fn finalize_transcript(
    adapter: &TranscriptAdapter,
    cursor: Option<&SyncCursor>,
    mut messages: Vec<Message>,
    external: Option<String>,
    title: Option<String>,
) -> NormalizedConversationBatch {
    // A web export is a full snapshot, not a stream: everything is re-read every run and
    // dedup (§52 L1/L2) is what makes that cheap. The cursor still records the position.
    let total = messages.len();
    let fingerprint = messages
        .last()
        .map(|m| m.content_hash.clone())
        .unwrap_or_default();
    skip_already_seen(cursor, &mut messages);
    blank_batch(
        Some(external.unwrap_or_else(|| adapter.id().to_string())),
        Some(title.unwrap_or_else(|| format!("transcript · {}", adapter.id()))),
        messages,
        Some(format!("snapshot:{total}:{fingerprint}")),
    )
}

/// `§51` resume position for the record-oriented adapters. Only the exact
/// `resume:<count>` form is trusted, where `<count>` is the number of turns already
/// handed over. Anything else (a snapshot cursor, an opaque id) means "no assumption",
/// and the pipeline's dedup (§52 L1/L2) handles the overlap.
fn skip_already_seen(cursor: Option<&SyncCursor>, messages: &mut Vec<Message>) {
    let Some(count) = cursor
        .and_then(|c| c.cursor.as_deref())
        .and_then(|raw| raw.strip_prefix("resume:"))
        .and_then(|tail| tail.parse::<usize>().ok())
    else {
        return;
    };
    let drop = count.min(messages.len());
    if drop > 0 {
        messages.drain(0..drop);
    }
}

/// A conversation-only batch builder for tests and callers that already normalised their
/// material: keeps the crate's identity convention in one place.
pub fn batch_from_conversation(
    conversation: Conversation,
    messages: Vec<Message>,
    next_cursor: Option<String>,
) -> NormalizedConversationBatch {
    NormalizedConversationBatch {
        conversation,
        messages,
        next_cursor,
        duplicate_count: 0,
    }
}
