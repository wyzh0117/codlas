//! The Adapter SDK contract (V1_DEVELOPMENT.md §36) plus the capability model (§37).
//!
//! An adapter is a *reader* of somebody else's material. It never writes to the project:
//! it hands back a [`NormalizedConversationBatch`] and the [`IngestPipeline`](crate::pipeline::IngestPipeline)
//! decides identity, dedup, judgement and cursoring. That is what keeps §7.6 true — the
//! core has no idea which product a batch came from.
//!
//! `open` / `prepare` / `inject` are optional in V1 (§7.8, §62 Degradation): the default
//! implementations report "unsupported" as `None`, so an adapter that can only read
//! history still satisfies the contract and the caller falls back to copy-and-paste.

use vibe_core::error::Result;
use vibe_domain::{
    AgentCapabilities, AgentTarget, InjectResult, NormalizedConversationBatch, PreparedPayload,
    SourceType, SyncCursor,
};

/// One adapter instance covers exactly one thread/session, because §51 makes the sync
/// cursor per source and a source is one conversation stream. A tool that exported many
/// conversations gets one adapter instance per conversation.
pub trait AgentAdapter: Send + Sync {
    /// Registration key (§36). Unique across the pipeline, stable across runs — it is
    /// also what the `Source.external_id` and the cursor are keyed on.
    fn id(&self) -> &str;

    /// Which `SourceType` row this adapter's material lands under (§12). Data, not a
    /// branch: the pipeline never reads it for anything but the source key.
    fn source_type(&self) -> SourceType;

    /// Is this source even reachable on this machine? A file existence check, a CLI
    /// presence probe — never a network call in V1.
    fn detect(&self) -> Result<bool>;

    /// What this adapter can honestly do (§37). `AUTO_EXECUTE` is never claimed in V1.
    fn capabilities(&self) -> AgentCapabilities;

    /// Read everything since `cursor`. Must be safe to call twice with the same cursor:
    /// the pipeline dedups, but a well-behaved adapter also skips what it already sent.
    fn ingest(&self, cursor: Option<&SyncCursor>) -> Result<NormalizedConversationBatch>;

    /// Bring the target agent to the front. `Ok(None)` means "this adapter cannot".
    fn open(&self, _target: &AgentTarget) -> Result<Option<InjectResult>> {
        Ok(None)
    }

    /// Stage a context pack for the target without sending it (§7.8 Preview first).
    fn prepare(&self, _payload: &str) -> Result<Option<PreparedPayload>> {
        Ok(None)
    }

    /// A deep link the user can click instead of pasting.
    fn deep_link(&self, _target: &AgentTarget) -> Option<String> {
        None
    }
}

/// The batch shape adapters actually return, with the fields the pipeline overwrites
/// (`project_id`, `source_id`, conversation `id`) already blanked, so a caller cannot
/// accidentally believe an adapter chose them.
pub fn blank_batch(
    external_conversation_id: Option<String>,
    title: Option<String>,
    messages: Vec<vibe_domain::Message>,
    next_cursor: Option<String>,
) -> NormalizedConversationBatch {
    let started_at = messages.iter().filter_map(|m| m.created_at).min();
    let last_seen_at = messages.iter().map(|m| m.observed_at).max();
    NormalizedConversationBatch {
        conversation: vibe_domain::Conversation {
            id: String::new(),
            project_id: String::new(),
            source_id: String::new(),
            external_conversation_id,
            title,
            started_at,
            last_seen_at,
        },
        messages,
        next_cursor,
        duplicate_count: 0,
    }
}

/// Read a file an adapter was *handed* — the injected root is the caller's decision, so
/// this helper takes the path as data and adds no default location of its own.
pub fn read_text(path: &std::path::Path) -> Result<String> {
    if !path.is_file() {
        return Err(vibe_core::CoreError::NotFound {
            what: format!("adapter source file {}", path.display()),
        });
    }
    let bytes = std::fs::read(path)?;
    String::from_utf8(bytes).map_err(|e| {
        vibe_core::CoreError::Encoding(format!("{} is not valid UTF-8: {e}", path.display()))
    })
}

/// `§37` helper: an adapter that can only read history.
pub fn read_only(adapter_id: &str, extra: Vec<vibe_domain::Capability>) -> AgentCapabilities {
    let mut capabilities = vec![vibe_domain::Capability::ReadHistory];
    capabilities.extend(extra);
    AgentCapabilities {
        adapter_id: adapter_id.to_string(),
        capabilities,
    }
}
