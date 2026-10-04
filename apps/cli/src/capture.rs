//! Stage D commands: capture (§36-§38 adapters, §51 sync, §52 dedup) and the human
//! review of what the judge could not decide (§27, §29).
//!
//! Two properties the CLI depends on and never re-implements: running the same import
//! twice adds nothing, and a low-confidence reading is reported rather than stored.
//! The pipeline already does both; these commands only surface the counts.

use std::path::PathBuf;
use std::sync::Arc;

use clap::Subcommand;
use vibe_core::event_store::{EventDraft, EventStore};
use vibe_domain::{ActorType, EventStatus, SourceType};
use vibe_ingestion::{
    AdapterReport, AgentAdapter, IngestPipeline, ManualImportAdapter, TranscriptAdapter,
};

use crate::app::{read_json, App};

#[derive(Subcommand)]
pub enum CaptureCommand {
    #[command(subcommand)]
    Import(ImportCommand),
    #[command(subcommand)]
    Review(ReviewCommand),
    /// Append one event by hand — for the gap where no adapter reaches.
    Event {
        #[arg(long)]
        r#type: String,
        /// `user`, `agent` or `system`. A user event is trusted; an agent event is a
        /// claim until confirmed (§19).
        #[arg(long, default_value = "user", value_parser = parse_actor)]
        actor: ActorType,
        #[arg(long)]
        subject: Option<String>,
        #[arg(long)]
        payload: Option<String>,
    },
    /// Sources registered in this project and where each one stopped (§51).
    Sources,
}

#[derive(Subcommand)]
pub enum ImportCommand {
    /// Pasted chat text (§38.1).
    Paste {
        text: String,
        #[arg(long, default_value = "manual")]
        source_id: String,
    },
    /// A file of pasted chat text.
    Manual {
        file: PathBuf,
        #[arg(long, default_value = "manual")]
        source_id: String,
    },
    /// A transcript export: JSON array, ChatGPT `mapping` shape or JSONL (§38.3).
    Transcript {
        file: PathBuf,
        #[arg(long, default_value = "transcript")]
        source_id: String,
        #[arg(long, default_value = "chatgpt_web", value_parser = parse_source_type)]
        source_type: SourceType,
        /// Pick one thread out of a multi-conversation export.
        #[arg(long)]
        conversation: Option<String>,
    },
    /// A local agent session dump: a directory of JSONL/JSON sessions (§38.4).
    CliSession {
        dir: PathBuf,
        #[arg(long, default_value = "generic-cli")]
        source_id: String,
        /// Pin one session file when the directory holds several.
        #[arg(long)]
        file: Option<String>,
    },
    /// Replay every registered source in one project.
    All {
        #[arg(long)]
        file: PathBuf,
    },
    /// Commit history, as read-only `git log` (§39: a commit is evidence, never a conclusion).
    Git {
        /// Checkout to read. Defaults to the directory the project was bound to.
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Stable identity for a project with more than one checkout (§38).
        #[arg(long)]
        repo_id: Option<String>,
        #[arg(long, default_value_t = vibe_ingestion::DEFAULT_MAX_COMMITS)]
        max: usize,
    },
    /// The working tree: what is there and what changed since the last scan (§39).
    Files {
        /// Root to walk. Defaults to the directory the project was bound to.
        #[arg(long)]
        dir: Option<PathBuf>,
        #[arg(long, default_value = "workspace-files")]
        source_id: String,
    },
}

#[derive(Subcommand)]
pub enum ReviewCommand {
    /// Events waiting for a human decision.
    List,
    Accept {
        event_id: String,
        #[arg(long)]
        confidence: Option<f64>,
    },
    Reject {
        event_id: String,
    },
    /// Move the judgement bar itself, persisted in `settings` (§27).
    Thresholds {
        /// Below this, a reading is dropped; at or above the inbox floor it is kept.
        #[arg(long)]
        auto_accept: Option<f64>,
        #[arg(long)]
        inbox: Option<f64>,
    },
}

fn parse_actor(raw: &str) -> Result<ActorType, String> {
    match raw.trim().to_lowercase().as_str() {
        "user" | "me" | "human" => Ok(ActorType::User),
        "agent" | "ai" | "model" => Ok(ActorType::Agent),
        "system" | "sync" => Ok(ActorType::System),
        other => Err(format!("unknown actor '{other}' — user, agent or system")),
    }
}

fn parse_source_type(raw: &str) -> Result<SourceType, String> {
    let key = raw.trim().to_lowercase().replace('-', "_");
    let known = [
        (SourceType::ChatgptWeb, "chatgpt_web"),
        (SourceType::Codex, "codex"),
        (SourceType::Qoder, "qoder"),
        (SourceType::Hermes, "hermes"),
        (SourceType::Dsh, "dsh"),
        (SourceType::GenericWeb, "generic_web"),
        (SourceType::GenericCli, "generic_cli"),
        (SourceType::File, "file"),
        (SourceType::Git, "git"),
        (SourceType::Manual, "manual"),
        (SourceType::Mcp, "mcp"),
    ];
    known
        .iter()
        .find(|(_, slug)| *slug == key)
        .map(|(kind, _)| *kind)
        .ok_or_else(|| {
            format!(
                "unknown source type '{raw}' — {}",
                known.iter().map(|(_, s)| *s).collect::<Vec<_>>().join(", ")
            )
        })
}

pub fn dispatch(
    app: &App,
    project: Option<&str>,
    command: CaptureCommand,
) -> vibe_core::error::Result<()> {
    let id = app.project(project)?;
    match command {
        CaptureCommand::Import(inner) => import(app, &id, inner),
        CaptureCommand::Review(inner) => review(app, &id, inner),
        CaptureCommand::Event {
            r#type,
            actor,
            subject,
            payload,
        } => {
            let mut draft = EventDraft::new(id.clone(), r#type).actor(actor);
            if let Some(subject) = subject {
                // `kind:id` keeps the subject pair unambiguous.
                let (kind, rest) = match subject.split_once(':') {
                    Some((k, r)) => (k, r),
                    None => ("entity", subject.as_str()),
                };
                draft = draft.subject(kind, rest);
            }
            if let Some(raw) = payload {
                let value: serde_json::Value = if raw.trim_start().starts_with('{') {
                    serde_json::from_str(&raw)?
                } else {
                    serde_json::json!({ "summary": raw })
                };
                draft = draft.payload(value);
            }
            let outcome = EventStore::new(app.repos.clone()).append(draft)?;
            if app.json {
                return app.print_json(&serde_json::json!({
                    "inserted": outcome.inserted,
                    "event": outcome.event,
                }));
            }
            app.say(if outcome.inserted {
                format!("appended {}", outcome.event.id)
            } else {
                format!("already recorded — {}", outcome.event.id)
            });
            Ok(())
        }
        CaptureCommand::Sources => {
            let sources = app.repos.conversations.list_sources(&id)?;
            if app.json {
                return app.print_json(&sources);
            }
            if sources.is_empty() {
                app.say("no sources");
            }
            for source in sources {
                let cursor = app
                    .repos
                    .conversations
                    .get_cursor(&source.id, &source.id)?;
                app.say(format!(
                    "{:<12} {}\t{}\t{}",
                    source.r#type.as_str(),
                    source.id,
                    source.display_name,
                    cursor
                        .map(|c| format!("cursor {}", c.cursor.unwrap_or_else(|| "-".into())))
                        .unwrap_or_else(|| "never synced".into())
                ));
            }
            Ok(())
        }
    }
}

fn import(app: &App, id: &str, command: ImportCommand) -> vibe_core::error::Result<()> {
    match command {
        ImportCommand::All { file } => {
            let specs: Vec<SourceSpec> = read_json(&file)?;
            let mut pipeline = IngestPipeline::with_rules(app.repos.clone());
            for spec in specs {
                pipeline.register(spec.into_adapter()?);
            }
            let report = pipeline.run(id)?;
            print_ingest(
                app,
                &report.per_adapter,
                report.total_new_messages,
                report.total_duplicates,
                report.total_events,
            )
        }
        ImportCommand::Paste { text, source_id } => {
            let adapter = ManualImportAdapter::named(source_id.clone(), text);
            run_one(app, id, &source_id, Box::new(adapter))
        }
        ImportCommand::Manual { file, source_id } => {
            let adapter = ManualImportAdapter::file_as(source_id.clone(), file);
            run_one(app, id, &source_id, Box::new(adapter))
        }
        ImportCommand::Transcript {
            file,
            source_id,
            source_type,
            conversation,
        } => {
            let mut adapter = TranscriptAdapter::file(source_id.clone(), source_type, file);
            if let Some(thread) = conversation {
                adapter = adapter.on_conversation(thread);
            }
            run_one(app, id, &source_id, Box::new(adapter))
        }
        ImportCommand::CliSession {
            dir,
            source_id,
            file,
        } => {
            let mut adapter = vibe_ingestion::GenericCliAdapter::with_id(source_id.clone(), dir);
            if let Some(name) = file {
                adapter = adapter.on_file(name);
            }
            run_one(app, id, &source_id, Box::new(adapter))
        }
        ImportCommand::Git { dir, repo_id, max } => {
            let workdir = bound_dir(app, id, dir)?;
            let facts = vibe_ingestion::GitFacts::new(
                app.repos.clone(),
                Arc::new(vibe_ingestion::ProcessCommandRunner::new()),
            )
            .with_options(repo_id.as_deref(), max);
            let report = facts.run(id, &workdir)?;
            if app.json {
                return app.print_json(&serde_json::json!({
                    "sourceId": report.source_id,
                    "commitsSeen": report.commits_seen,
                    "eventsAppended": report.events_appended,
                    "eventsDuplicate": report.events_duplicate,
                    "artifactsRecorded": report.artifacts_recorded,
                    "artifactsDeleted": report.artifacts_deleted,
                    "paths": report.paths,
                    "skipped": report.skipped,
                }));
            }
            app.say(format!(
                "{}  {} commits: {} new, {} already recorded\t{} artifacts",
                report.source_id,
                report.commits_seen,
                report.events_appended,
                report.events_duplicate,
                report.artifacts_recorded
            ));
            Ok(())
        }
        ImportCommand::Files { dir, source_id } => {
            let root = bound_dir(app, id, dir)?;
            let scan = vibe_ingestion::FileScan::new(app.repos.clone(), root)
                .with_source_key(&source_id);
            let report = scan.run(id)?;
            if app.json {
                return app.print_json(&serde_json::json!({
                    "sourceId": report.source_id,
                    "scanned": report.scanned,
                    "unchanged": report.unchanged,
                    "skippedLarge": report.skipped_large,
                    "skippedBinary": report.skipped_binary,
                    "eventsAppended": report.events_appended,
                    "eventsDuplicate": report.events_duplicate,
                    "artifactsRecorded": report.artifacts_recorded,
                    "changed": report.changed_paths(),
                    "errors": report.errors,
                }));
            }
            app.say(format!(
                "{source_id}  {} read, {} unchanged\t{} changed → {} events, {} artifacts",
                report.scanned,
                report.unchanged,
                report.changes.len(),
                report.events_appended,
                report.artifacts_recorded
            ));
            for (path, why) in &report.errors {
                app.say(format!("  unreadable {path}: {why}"));
            }
            Ok(())
        }
    }
}

/// The directory a capture reads. `--dir` wins; otherwise the project's own bound
/// root, which is what §14's "绑定代码目录" means and why `wp init` records it.
fn bound_dir(app: &App, id: &str, explicit: Option<PathBuf>) -> vibe_core::error::Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    let root = app
        .repos
        .projects
        .get_project(id)?
        .and_then(|project| project.root_path)
        .filter(|path| !path.trim().is_empty())
        .ok_or_else(|| vibe_core::CoreError::Validation(
            "no directory to read: this project has no bound root_path — pass --dir or set it \
             with `wp project set`"
                .to_string(),
        ))?;
    Ok(PathBuf::from(root))
}

/// A source listed in an import manifest (§61 E2E: at least two sources, one project).
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SourceSpec {
    /// `manual` or `transcript`.
    r#type: String,
    #[serde(default)]
    id: Option<String>,
    file: PathBuf,
    #[serde(default)]
    source_type: Option<String>,
}

impl SourceSpec {
    fn into_adapter(self) -> vibe_core::error::Result<Arc<dyn AgentAdapter>> {
        use vibe_core::CoreError;
        let id = self.id.clone().unwrap_or_else(|| self.r#type.clone());
        match self.r#type.trim().to_lowercase().as_str() {
            "manual" => Ok(Arc::new(ManualImportAdapter::file_as(id, self.file))),
            "transcript" => {
                let kind = parse_source_type(
                    self.source_type.as_deref().unwrap_or("chatgpt_web"),
                )
                .map_err(|e| CoreError::Validation(e))?;
                Ok(Arc::new(TranscriptAdapter::file(id, kind, self.file)))
            }
            other => Err(CoreError::Validation(format!(
                "unknown import source type '{other}' — manual or transcript"
            ))),
        }
    }
}

fn run_one(
    app: &App,
    id: &str,
    adapter_id: &str,
    adapter: Box<dyn AgentAdapter>,
) -> vibe_core::error::Result<()> {
    let mut pipeline = IngestPipeline::with_rules(app.repos.clone());
    pipeline.register(Arc::from(adapter));
    let report = pipeline.run_one(id, adapter_id)?;
    print_ingest(
        app,
        std::slice::from_ref(&report),
        report.messages_inserted,
        report.messages_duplicate,
        report.events_appended,
    )
}

fn print_ingest(
    app: &App,
    per_adapter: &[AdapterReport],
    messages: usize,
    duplicates: usize,
    events: usize,
) -> vibe_core::error::Result<()> {
    if app.json {
        let adapters = per_adapter
            .iter()
            .map(|report| {
                serde_json::json!({
                    "adapterId": report.adapter_id,
                    "sourceId": report.source_id,
                    "conversations": report.conversations,
                    "messagesInserted": report.messages_inserted,
                    "messagesDuplicate": report.messages_duplicate,
                    "eventsAppended": report.events_appended,
                    "eventsPending": report.events_pending,
                    "eventsCandidate": report.events_candidate,
                    "error": report.error,
                })
            })
            .collect::<Vec<_>>();
        return app.print_json(&serde_json::json!({
            "perAdapter": adapters,
            "totalNewMessages": messages,
            "totalDuplicates": duplicates,
            "totalEvents": events,
        }));
    }
    for report in per_adapter {
        if let Some(error) = &report.error {
            app.say(format!("{} FAILED: {}", report.adapter_id, error));
            continue;
        }
        app.say(format!(
            "{}\t+{} messages\t{} duplicate\t{} events ({} pending, {} below the floor)",
            report.adapter_id,
            report.messages_inserted,
            report.messages_duplicate,
            report.events_appended,
            report.events_pending,
            report.events_candidate,
        ));
    }
    app.say(format!(
        "total: {messages} new messages, {duplicates} duplicates, {events} events"
    ));
    app.say("run `wp review list` for what needs a decision");
    Ok(())
}

fn review(app: &App, id: &str, command: ReviewCommand) -> vibe_core::error::Result<()> {
    let store = EventStore::new(app.repos.clone());
    let settings = vibe_core::SettingsService::new(app.repos.clone());
    match command {
        ReviewCommand::List => {
            let pending = store.list(id, Some([EventStatus::PendingReview].as_slice()), Some(50))?;
            if app.json {
                return app.print_json(&pending);
            }
            if pending.is_empty() {
                app.say("nothing waiting for review");
            }
            for event in pending {
                app.say(format!(
                    "{}\t{:.2}\t{}\t{}",
                    event.id,
                    event.confidence,
                    event.r#type,
                    event
                        .payload
                        .get("summary")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .chars()
                        .take(80)
                        .collect::<String>()
                ));
            }
            Ok(())
        }
        ReviewCommand::Accept {
            event_id,
            confidence,
        } => {
            let event = store.review(&event_id, EventStatus::Accepted, confidence)?;
            if app.json {
                return app.print_json(&event);
            }
            app.say(format!("accepted {} ({:.2})", event.id, event.confidence));
            Ok(())
        }
        ReviewCommand::Reject { event_id } => {
            let event = store.review(&event_id, EventStatus::Rejected, None)?;
            if app.json {
                return app.print_json(&event);
            }
            app.say(format!("rejected {}", event.id));
            Ok(())
        }
        ReviewCommand::Thresholds { auto_accept, inbox } => {
            if auto_accept.is_some() || inbox.is_some() {
                let mut current = settings.judge_thresholds()?;
                if let Some(value) = auto_accept {
                    current.auto_accept = value;
                }
                if let Some(value) = inbox {
                    current.inbox_floor = value;
                }
                settings.set_judge_thresholds(current)?;
            }
            app.print_json(&settings.judge_thresholds()?)
        }
    }
}
