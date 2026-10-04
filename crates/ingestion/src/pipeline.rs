//! The ingest pipeline: adapter material → Source / Conversation / Message → judged
//! Events → an advanced cursor (V1_DEVELOPMENT.md §12-§15, §27, §51, §52).
//!
//! Three properties are the whole design:
//!
//! * **Idempotent end to end.** Identity is derived, never invented per run: the source is
//!   matched by `(project_id, type, external_id)`, the conversation id and the message
//!   dedup come from content, and event identity is the store's `dedup_key`. Running the
//!   same adapter twice inserts nothing the second time and leaves the cursor where it is.
//! * **Per-source cursoring (§51).** The cursor is read *before* the adapter runs and
//!   written only after that adapter's batch committed. A source that fails leaves its
//!   cursor untouched and does not cost the other sources their work.
//! * **Judgement, not transcription (§27).** Every newly stored message goes through the
//!   [`SignalJudge`]: `Accept` writes an accepted event, `Inbox` writes a pending event
//!   that the reducer ignores until a human rules on it, and `Candidate` writes nothing at
//!   all — it is reported, because `< 0.65` may not move project state.

use std::sync::Arc;

use vibe_core::error::{CoreError, Result};
use vibe_core::event_store::{EventDraft, EventStore};
use vibe_core::judge::{message_evidence, CaptureSignal, JudgedSignal, JudgeRoute, RuleJudge, SignalJudge};
use vibe_core::ports::Repositories;
use vibe_core::resolution::{EntityResolver, AUTO_MERGE_CONFIDENCE};
use vibe_core::settings::SettingsService;
use vibe_domain::{
    event_catalog, ActorType, AdapterHealth, AdapterState, Conversation, EntityKind, Message,
    MessageRole, NormalizedConversationBatch, Source, SourceType, SyncCursor,
};

use crate::adapter::AgentAdapter;

/// Everything one adapter contributed to one run.
#[derive(Debug, Clone, Default)]
pub struct AdapterReport {
    pub adapter_id: String,
    /// The `Source` row this adapter writes into — empty until the source is resolved.
    pub source_id: String,
    /// Conversations carried by the batch (0 when the adapter had nothing to say).
    pub conversations: usize,
    pub messages_inserted: usize,
    /// Messages dropped by dedup level L1/L2 (§52).
    pub messages_duplicate: usize,
    /// Event rows newly written: accepted *plus* pending.
    pub events_appended: usize,
    /// The `events_appended` subset that landed in the Inbox as `pending_review` (§29).
    pub events_pending: usize,
    /// Sub-threshold readings kept in the report and written nowhere (§27).
    pub events_candidate: usize,
    /// Set when this source failed. Other sources are unaffected (§51 partial failure).
    pub error: Option<String>,
    /// §27 readings below the inbox floor: kept here and written nowhere.
    pub candidates: Vec<CapturedCandidate>,
    /// §28 near-matches the pipeline refused to merge (`confidence < 0.90`).
    pub merge_candidates: Vec<MergeCandidate>,
}

impl AdapterReport {
    fn new(adapter_id: &str) -> Self {
        Self {
            adapter_id: adapter_id.to_string(),
            source_id: String::new(),
            conversations: 0,
            messages_inserted: 0,
            messages_duplicate: 0,
            events_appended: 0,
            events_pending: 0,
            events_candidate: 0,
            error: None,
            candidates: Vec::new(),
            merge_candidates: Vec::new(),
        }
    }

    fn failed(adapter_id: &str, source_id: &str, error: &str) -> Self {
        let mut report = AdapterReport::new(adapter_id);
        report.source_id = source_id.to_string();
        report.error = Some(error.to_string());
        report
    }

    /// A source that produced no error.
    pub fn ok(&self) -> bool {
        self.error.is_none()
    }
}

/// A reading the judge produced but the thresholds refused to write (§27 `< 0.65`).
/// It is memory-only in V1: the report carries it to the Inbox surface, and re-running
/// the message re-derives it, so nothing is lost by not storing a row.
#[derive(Debug, Clone, PartialEq)]
pub struct CapturedCandidate {
    pub event_type: String,
    pub message_id: String,
    pub confidence: f64,
    pub rationale: String,
}

/// A possible identity match the pipeline deliberately did *not* apply (§28: when in
/// doubt, do not merge — a human decides in the Inbox).
#[derive(Debug, Clone, PartialEq)]
pub struct MergeCandidate {
    pub kind: EntityKind,
    pub existing_id: String,
    pub title: String,
    pub confidence: f64,
    pub basis: &'static str,
}

/// The whole run, reported per source (§51 demands partial-failure visibility).
#[derive(Debug, Clone, Default)]
pub struct IngestReport {
    pub per_adapter: Vec<AdapterReport>,
    pub total_new_messages: usize,
    pub total_duplicates: usize,
    pub total_events: usize,
}

impl IngestReport {
    fn push(&mut self, report: AdapterReport) {
        self.total_new_messages += report.messages_inserted;
        self.total_duplicates += report.messages_duplicate;
        self.total_events += report.events_appended;
        self.per_adapter.push(report);
    }

    /// Sources that failed, for a caller that has to warn about them.
    pub fn failures(&self) -> Vec<&AdapterReport> {
        self.per_adapter.iter().filter(|r| !r.ok()).collect()
    }

    /// §28 candidates across every source in this run.
    pub fn merge_candidates(&self) -> Vec<&MergeCandidate> {
        self.per_adapter
            .iter()
            .flat_map(|r| r.merge_candidates.iter())
            .collect()
    }

    /// §27 candidates across every source in this run.
    pub fn candidates(&self) -> Vec<&CapturedCandidate> {
        self.per_adapter
            .iter()
            .flat_map(|r| r.candidates.iter())
            .collect()
    }
}

/// Orchestrator: adapter registry + capture + judgement + cursoring.
pub struct IngestPipeline {
    repos: Repositories,
    events: EventStore,
    judge: Arc<dyn SignalJudge>,
    resolver: EntityResolver,
    adapters: Vec<Arc<dyn AgentAdapter>>,
}

impl IngestPipeline {
    /// The brief's `new(repos, judge?)`: `None` keeps §27's rule judge with whatever
    /// thresholds the user last set (§27 allows retuning after Inbox feedback).
    pub fn new(repos: Repositories, judge: Option<Arc<dyn SignalJudge>>) -> Self {
        let judge = judge.unwrap_or_else(|| {
            let thresholds = SettingsService::new(repos.clone())
                .judge_thresholds()
                .unwrap_or_default();
            Arc::new(RuleJudge::new(thresholds))
        });
        Self {
            events: EventStore::new(repos.clone()),
            resolver: EntityResolver::new(&repos),
            repos,
            judge,
            adapters: Vec::new(),
        }
    }

    /// Shorthand for the common "use the defaults" construction.
    pub fn with_rules(repos: Repositories) -> Self {
        Self::new(repos, None)
    }

    pub fn repositories(&self) -> &Repositories {
        &self.repos
    }

    pub fn events(&self) -> &EventStore {
        &self.events
    }

    /// §36 registration by id. Re-registering an id replaces it, so reloading a config
    /// cannot capture the same source twice.
    pub fn register(&mut self, adapter: Arc<dyn AgentAdapter>) {
        let id = adapter.id();
        match self.adapters.iter().position(|a| a.id() == id) {
            Some(slot) => self.adapters[slot] = adapter,
            None => self.adapters.push(adapter),
        }
    }

    pub fn adapters(&self) -> &[Arc<dyn AgentAdapter>] {
        &self.adapters
    }

    pub fn adapter(&self, adapter_id: &str) -> Option<&Arc<dyn AgentAdapter>> {
        self.adapters.iter().find(|a| a.id() == adapter_id)
    }

    /// Run every registered adapter. One source failing never aborts the run.
    pub fn run(&self, project_id: &str) -> Result<IngestReport> {
        self.require_project(project_id)?;
        let mut report = IngestReport::default();
        for adapter in &self.adapters {
            match adapter.detect() {
                Ok(true) => {}
                // An adapter that is not present on this machine contributes nothing;
                // it is not an error, and §62 Degradation covers the user's side. It is
                // still recorded, because "which adapters does this workbench have?" is
                // answered better from a stored verdict than from a silent skip.
                Ok(false) => {
                    self.record_health(adapter.id(), AdapterHealth::Unavailable, None)?;
                    continue;
                }
                Err(error) => {
                    self.record_health(
                        adapter.id(),
                        AdapterHealth::Failed,
                        Some(format!("detect failed: {error}")),
                    )?;
                    report.push(AdapterReport::failed(
                        adapter.id(),
                        "",
                        &format!("detect failed: {error}"),
                    ));
                    continue;
                }
            }
            let outcome = self.ingest_adapter(project_id, adapter);
            self.note_health(&outcome)?;
            report.push(outcome);
        }
        Ok(report)
    }

    /// Run one registered adapter by id.
    pub fn run_one(&self, project_id: &str, adapter_id: &str) -> Result<AdapterReport> {
        self.require_project(project_id)?;
        let adapter = self
            .adapter(adapter_id)
            .cloned()
            .ok_or_else(|| CoreError::NotFound {
                what: format!("adapter {adapter_id}"),
            })?;
        let outcome = self.ingest_adapter(project_id, &adapter);
        self.note_health(&outcome)?;
        Ok(outcome)
    }

    /// Stage H: one verdict per adapter, so §62 Degradation is answerable after the run
    /// rather than only while it is happening.
    fn note_health(&self, outcome: &AdapterReport) -> Result<()> {
        self.record_health(
            &outcome.adapter_id,
            report_health(outcome),
            outcome.error.clone(),
        )
    }

    /// A health write that fails is a storage problem, not the adapter's, so callers keep
    /// the run's own error in the report and this only adds the verdict to the log.
    fn record_health(
        &self,
        adapter_id: &str,
        health: AdapterHealth,
        detail: Option<String>,
    ) -> Result<()> {
        self.repos.adapters.record_state(&AdapterState {
            adapter_id: adapter_id.to_string(),
            health,
            detail,
            updated_at: vibe_domain::now(),
        })
    }

    /// Store an already-normalised batch — the entry point for material the caller read
    /// itself (a browser push, a test fixture). No cursor moves: identity and dedup make
    /// the call repeatable. Batches produced by a registered adapter should go through
    /// [`IngestPipeline::run`] instead, which also knows the adapter's `SourceType`.
    pub fn ingest_messages(
        &self,
        project_id: &str,
        batch: &NormalizedConversationBatch,
    ) -> Result<AdapterReport> {
        self.require_project(project_id)?;
        let key = batch
            .conversation
            .external_conversation_id
            .clone()
            .or_else(|| {
                (!batch.conversation.source_id.trim().is_empty())
                    .then(|| batch.conversation.source_id.clone())
            })
            .unwrap_or_else(|| crate::adapters::ManualImportAdapter::DEFAULT_ID.to_string());
        let source_type = SourceType::parse(&batch.conversation.source_id).unwrap_or(SourceType::Manual);
        let source = self.resolve_source(project_id, &key, source_type)?;
        let mut report = self.store_batch(project_id, &key, &source, batch)?;
        report.source_id = source.id;
        Ok(report)
    }

    fn ingest_adapter(&self, project_id: &str, adapter: &Arc<dyn AgentAdapter>) -> AdapterReport {
        let adapter_id = adapter.id().to_string();
        let mut report = AdapterReport::new(&adapter_id);
        let source = match self.resolve_source(project_id, &adapter_id, adapter.source_type()) {
            Ok(source) => source,
            Err(error) => {
                report.error = Some(format!("source resolution failed: {error}"));
                return report;
            }
        };
        report.source_id = source.id.clone();

        // §51: read before ingesting, write only after this batch committed.
        let prior = match self
            .repos
            .conversations
            .get_cursor(&adapter_id, &source.id)
        {
            Ok(cursor) => cursor,
            Err(error) => {
                report.error = Some(format!("cursor read failed: {error}"));
                return report;
            }
        };

        let batch = match adapter.ingest(prior.as_ref()) {
            Ok(batch) => batch,
            Err(error) => {
                // Cursor untouched, other sources unaffected.
                report.error = Some(error.to_string());
                return report;
            }
        };

        match self.store_batch(project_id, &adapter_id, &source, &batch) {
            Ok(stored) => report = stored,
            Err(error) => {
                report.error = Some(format!("ingest failed: {error}"));
                return report;
            }
        }

        if let Err(error) = self.advance_cursor(&adapter_id, &source.id, &batch, prior.as_ref()) {
            report.error = Some(format!("cursor write failed: {error}"));
        }
        report
    }

    /// `(project_id, type, external_id)` is the source key; the external id is the
    /// adapter's registration id, so two adapters are two sources (§38 two sources, one
    /// project) and re-running the same adapter reuses the same source row.
    fn resolve_source(
        &self,
        project_id: &str,
        adapter_id: &str,
        source_type: SourceType,
    ) -> Result<Source> {
        let existing = self
            .repos
            .conversations
            .find_source_by_external_id(project_id, source_type.as_str(), adapter_id)?;
        let source = match existing {
            Some(mut source) => {
                source.display_name = adapter_id.to_string();
                source
            }
            None => Source {
                id: vibe_domain::new_id(),
                project_id: project_id.to_string(),
                r#type: source_type,
                external_id: Some(adapter_id.to_string()),
                display_name: adapter_id.to_string(),
                metadata: serde_json::json!({ "adapter": adapter_id }),
            },
        };
        self.repos.conversations.upsert_source(&source)?;
        Ok(source)
    }

    fn store_batch(
        &self,
        project_id: &str,
        adapter_id: &str,
        source: &Source,
        batch: &NormalizedConversationBatch,
    ) -> Result<AdapterReport> {
        let mut report = AdapterReport::new(adapter_id);
        report.source_id = source.id.clone();

        let external = batch
            .conversation
            .external_conversation_id
            .clone()
            .unwrap_or_else(|| adapter_id.to_string());
        // Derived, not invented: the same adapter + thread always lands in the same
        // conversation row, which is what makes content-hash dedup bite (§52 L2).
        let conversation_id = stable_id(&[project_id, source.r#type.as_str(), adapter_id, &external]);
        let seen = batch
            .conversation
            .last_seen_at
            .or_else(|| batch.messages.iter().map(|m| m.observed_at).max())
            .unwrap_or_else(vibe_domain::now);
        let conversation = Conversation {
            id: conversation_id.clone(),
            project_id: project_id.to_string(),
            source_id: source.id.clone(),
            external_conversation_id: Some(external),
            title: batch.conversation.title.clone(),
            started_at: batch.conversation.started_at,
            last_seen_at: Some(seen),
        };
        self.repos.conversations.upsert_conversation(&conversation)?;

        let prepared: Vec<Message> = batch
            .messages
            .iter()
            .map(|m| Message {
                conversation_id: conversation_id.clone(),
                ..m.clone()
            })
            .collect();
        report.conversations = usize::from(!batch.messages.is_empty());

        let outcome = self
            .repos
            .conversations
            .insert_messages(&conversation_id, &prepared)?;
        // The port's outcome is the truth about dedup (§52).
        report.messages_inserted = outcome.inserted.len();
        report.messages_duplicate = outcome.duplicate_count;

        for message in &outcome.inserted {
            self.capture(project_id, source, &conversation_id, message, &mut report)?;
        }
        Ok(report)
    }

    /// One message, judged once per reading the judge finds (§27 can return several).
    fn capture(
        &self,
        project_id: &str,
        source: &Source,
        conversation_id: &str,
        message: &Message,
        report: &mut AdapterReport,
    ) -> Result<()> {
        let signal = CaptureSignal {
            project_id: project_id.to_string(),
            source_id: source.id.clone(),
            conversation_id: conversation_id.to_string(),
            message_id: message.id.clone(),
            actor_type: actor_of(message.role),
            text: message.content.clone(),
            observed_at: message.observed_at,
        };
        // §17: the message that justified an event becomes Evidence for it.
        let evidence_id = ensure_evidence(
            &self.repos,
            project_id,
            vibe_domain::EvidenceType::Message,
            &message.id,
            &vibe_core::judge::excerpt(&message.content),
        )?;

        for judged in self.judge.judge(&signal) {
            if judged.route == JudgeRoute::Candidate {
                // < 0.65: a candidate is not a fact. Nothing is written, so no state can
                // move (V1_DEVELOPMENT.md §27).
                report.events_candidate += 1;
                report.candidates.push(CapturedCandidate {
                    event_type: judged.draft.event_type.clone(),
                    message_id: message.id.clone(),
                    confidence: judged.draft.confidence,
                    rationale: judged.rationale.clone(),
                });
                continue;
            }
            let (draft, merge) = self.with_subject(project_id, &judged)?;
            if let Some(candidate) = merge {
                report.merge_candidates.push(candidate);
            }
            let draft = match judged.route {
                JudgeRoute::Inbox => draft.pending(),
                _ => draft,
            };
            let subject = draft.subject_id.clone();
            let event_type = draft.event_type.clone();
            let outcome = self
                .events
                .append(draft.evidence(vec![message.id.clone()]))?;
            if !outcome.inserted {
                continue;
            }
            report.events_appended += 1;
            if judged.route == JudgeRoute::Inbox {
                report.events_pending += 1;
            }
            if let (Some(id), Some(kind)) = (subject, subject_kind(&event_type)) {
                link_evidence(&self.repos, &evidence_id, kind, &id)?;
            }
        }
        Ok(())
    }

    /// §28: name the subject, then let the resolver decide whether that name is already
    /// taken. Only an explicit id or an exact title match merges.
    fn with_subject(
        &self,
        project_id: &str,
        judged: &JudgedSignal,
    ) -> Result<(EventDraft, Option<MergeCandidate>)> {
        let mut draft = judged.draft.clone();
        let Some(kind) = subject_kind(&draft.event_type) else {
            return Ok((draft, None));
        };
        let title = subject_title(judged);
        if title.is_empty() {
            return Ok((draft, None));
        }
        let resolved = self.resolver.resolve_title(project_id, kind, &title)?;
        match resolved {
            Some(found) if found.confidence >= AUTO_MERGE_CONFIDENCE => {
                draft.subject_type = Some(kind.as_str().to_string());
                draft.subject_id = Some(found.id);
                Ok((draft, None))
            }
            Some(found) => Ok((
                draft,
                Some(MergeCandidate {
                    kind,
                    existing_id: found.id,
                    title,
                    confidence: found.confidence,
                    basis: found.basis,
                }),
            )),
            // No existing object: mint a deterministic id from the name, so a second
            // message about the same thing meets the same subject key instead of opening
            // a twin object.
            None => {
                draft.subject_type = Some(kind.as_str().to_string());
                draft.subject_id = Some(stable_id(&[project_id, kind.as_str(), &title]));
                Ok((draft, None))
            }
        }
    }

    fn advance_cursor(
        &self,
        adapter_id: &str,
        source_id: &str,
        batch: &NormalizedConversationBatch,
        prior: Option<&SyncCursor>,
    ) -> Result<()> {
        let last_seen = batch
            .conversation
            .last_seen_at
            .or_else(|| batch.messages.iter().map(|m| m.observed_at).max())
            .or(prior.and_then(|p| p.last_seen_at))
            .unwrap_or_else(vibe_domain::now);
        let last_content_hash = batch
            .messages
            .last()
            .map(|m| m.content_hash.clone())
            .or_else(|| prior.and_then(|p| p.last_content_hash.clone()));
        self.repos.conversations.set_cursor(&SyncCursor {
            adapter_id: adapter_id.to_string(),
            source_id: source_id.to_string(),
            cursor: batch
                .next_cursor
                .clone()
                .or_else(|| prior.and_then(|p| p.cursor.clone())),
            last_seen_at: Some(last_seen),
            last_content_hash,
        })
    }

    fn require_project(&self, project_id: &str) -> Result<()> {
        if project_id.trim().is_empty() {
            return Err(CoreError::Validation("project id is required".into()));
        }
        if self.repos.projects.get_project(project_id)?.is_none() {
            return Err(CoreError::NotFound {
                what: format!("project {project_id}"),
            });
        }
        Ok(())
    }
}

/// Which of the four health states a run produced (Stage H, ADR-021). A report carrying
/// an error *after* it committed material is `degraded`: the user's data landed, the sync
/// position did not — which is a different thing to tell the user than a failure. An
/// uneventful run with nothing new is `ok`, not degraded; §51's idempotency means an
/// unchanged source legitimately reports zero rows.
pub fn report_health(outcome: &AdapterReport) -> AdapterHealth {
    match &outcome.error {
        None => AdapterHealth::Ok,
        Some(_) if outcome.messages_inserted + outcome.events_appended > 0 => {
            AdapterHealth::Degraded
        }
        Some(_) => AdapterHealth::Failed,
    }
}

/// A message's speaker is the event's actor — an assistant line can produce a claim,
/// never a confirmation (§19).
pub fn actor_of(role: MessageRole) -> ActorType {
    match role {
        MessageRole::User => ActorType::User,
        MessageRole::Assistant => ActorType::Agent,
        MessageRole::System | MessageRole::Tool => ActorType::System,
    }
}

/// Which state object an event type argues about, when it argues about one.
pub fn subject_kind(event_type: &str) -> Option<EntityKind> {
    Some(match event_type {
        event_catalog::TASK_CREATED
        | event_catalog::TASK_STARTED
        | event_catalog::TASK_BLOCKED
        | event_catalog::TASK_COMPLETED_CLAIM
        | event_catalog::TASK_COMPLETED_CONFIRMED
        | event_catalog::TASK_CANCELLED => EntityKind::Task,
        event_catalog::ISSUE_FOUND | event_catalog::ISSUE_RESOLVED => EntityKind::Issue,
        event_catalog::DECISION_PROPOSED
        | event_catalog::DECISION_CONFIRMED
        | event_catalog::DECISION_SUPERSEDED
        | event_catalog::DECISION_REVERSED => EntityKind::Decision,
        event_catalog::ARTIFACT_CREATED
        | event_catalog::ARTIFACT_CHANGED
        | event_catalog::ARTIFACT_DELETED => EntityKind::Artifact,
        _ => return None,
    })
}

/// The object name a judged reading is about: the message with the trigger phrase and
/// trailing sentences removed, clamped so a title stays a title.
fn subject_title(judged: &JudgedSignal) -> String {
    let text = judged
        .draft
        .payload
        .get("excerpt")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let matched = judged
        .draft
        .payload
        .get("matched")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let first_sentence: String = text
        .split(['。', '！', '？', '\n', '.', '!', '?'])
        .next()
        .unwrap_or(&text)
        .chars()
        .collect();
    let candidate = if !matched.is_empty() && !first_sentence.replace(matched, "").trim().is_empty() {
        first_sentence.replace(matched, "")
    } else {
        first_sentence
    };
    vibe_domain::normalize_for_hash(&candidate)
        .trim_matches(|c: char| {
            matches!(c, '，' | ',' | '、' | '：' | ':' | '；' | ';' | '「' | '」' | '"')
        })
        .chars()
        .take(80)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Deterministic identity from meaningful parts — no uuid, so a re-run meets the same row.
pub(crate) fn stable_id(parts: &[&str]) -> String {
    vibe_domain::content_hash(&parts.join("\u{241f}"))
}

/// Persist evidence for a reference (§17) without ever creating a second row for it, and
/// hand back the row id. Events carry the *reference* in `evidence_refs`: the reducer
/// derives its own evidence row from that reference and links both, which is what makes
/// "why does the system believe this?" answerable from either end.
pub(crate) fn ensure_evidence(
    repos: &Repositories,
    project_id: &str,
    r#type: vibe_domain::EvidenceType,
    reference: &str,
    summary: &str,
) -> Result<String> {
    if let Some(existing) = repos.evidence.find_by_ref(r#type, reference)? {
        return Ok(existing.id);
    }
    let base = match r#type {
        vibe_domain::EvidenceType::Message => message_evidence(project_id, reference, summary),
        other => vibe_domain::Evidence {
            id: String::new(),
            project_id: project_id.to_string(),
            r#type: other,
            ref_id: Some(reference.to_string()),
            summary: Some(summary.to_string()),
            created_at: vibe_domain::now(),
        },
    };
    // A reference-keyed id (not `message_evidence`'s fresh uuid) is what makes a re-run
    // land on the same row: `insert_evidence` ignores duplicates by id.
    let evidence = vibe_domain::Evidence {
        id: stable_id(&[project_id, r#type.as_str(), reference]),
        ..base
    };
    repos.evidence.insert_evidence(&evidence)?;
    Ok(evidence.id)
}

/// The display name of the file at `path` — its last segment, which is all §22 allows.
pub(crate) fn artifact_name_for(path: &str) -> String {
    path.rsplit('/')
        .next()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(path)
        .to_string()
}

/// Artifact kind inferred from the extension, using the same table the reducer's
/// `ARTIFACT_*` branch applies, so a row written here and a row written by a projection
/// disagree about nothing (§22).
pub(crate) fn artifact_kind_for(path: &str) -> vibe_domain::ArtifactKind {
    use vibe_domain::ArtifactKind;
    if path.ends_with(".md") || path.ends_with(".txt") {
        ArtifactKind::Document
    } else if path.ends_with(".toml") || path.ends_with(".json") || path.ends_with(".yaml") {
        ArtifactKind::Config
    } else if path.contains("test") {
        ArtifactKind::Test
    } else if path.ends_with(".sh") {
        ArtifactKind::Script
    } else if path.ends_with(".sql") || path.ends_with(".db") {
        ArtifactKind::Database
    } else {
        ArtifactKind::SourceFile
    }
}

/// Attach an evidence row to the object it vouches for, when the object is known.
pub(crate) fn link_evidence(
    repos: &Repositories,
    evidence_id: &str,
    kind: EntityKind,
    subject_id: &str,
) -> Result<()> {
    repos.evidence.link(evidence_id, kind, subject_id)
}
