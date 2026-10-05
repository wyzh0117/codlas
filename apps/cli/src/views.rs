//! Read-side commands: the projections the workbench shows (V1_DEVELOPMENT.md §20
//! State, §30-§33 Project Map, §26 Guard, §29 Inbox, §40 Search, §54 Audit).
//!
//! These print what the engines already computed. Nothing here decides anything:
//! a state read never mutates the log, and `drift check` is the only command in this
//! file that writes, because an alert has to be stored to be resolvable later.

use std::path::PathBuf;

use clap::Subcommand;
use vibe_core::error::CoreError;
use vibe_core::ports::EntityKind;
use vibe_domain::{EventStatus, InboxKind};
use vibe_guard_engine::{DriftAction, DriftEngine, Inbox, InboxAction, InboxRef};
use vibe_state_engine::{ProjectMapCompiler, Reducer, SnapshotService};

use crate::app::App;

#[derive(Subcommand)]
pub enum ViewCommand {
    #[command(subcommand)]
    State(StateCommand),
    /// Fold the whole accepted log again and report what the replay did.
    Replay,
    #[command(subcommand)]
    Map(MapCommand),
    /// The event log in `seq` order — the traceability view (§45).
    Timeline {
        #[arg(long, default_value_t = 40)]
        limit: usize,
        /// Include events still awaiting judgement.
        #[arg(long)]
        pending: bool,
    },
    #[command(subcommand)]
    Snapshot(SnapshotCommand),
    /// What changed since a snapshot (default: the newest one).
    Diff {
        #[arg(long)]
        snapshot: Option<String>,
    },
    #[command(subcommand)]
    Drift(DriftCommand),
    #[command(subcommand)]
    Inbox(InboxCommand),
    /// Open the evidence a record cites (§17, §45 traceability).
    #[command(subcommand)]
    Evidence(EvidenceCommand),
    /// §52's L3: turns that say almost the same thing, reported as hints. Reads only —
    /// every message stays in the database exactly as it is.
    Duplicates {
        /// Scan one conversation instead of the project's recent window.
        #[arg(long)]
        conversation: Option<String>,
        /// Token overlap a pair needs to be reported, 0.0-1.0.
        #[arg(long, default_value_t = vibe_ingestion::DEFAULT_THRESHOLD)]
        threshold: f64,
        /// How many recent messages the pass reads; pairs grow with its square (§59).
        #[arg(long, default_value_t = vibe_ingestion::DEFAULT_WINDOW)]
        window: usize,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Search {
        query: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        kind: Vec<String>,
    },
    /// Rebuild the full-text index from the records themselves (§40).
    Reindex,
    /// Who read what, what pack it produced and what was redacted (§54).
    Audit {
        #[arg(long, default_value_t = 30)]
        limit: usize,
    },
}

#[derive(Subcommand)]
pub enum StateCommand {
    /// The materialized projection as JSON.
    Show,
    /// One-line counts, for a glance at where the project stands.
    Summary,
}

#[derive(Subcommand)]
pub enum MapCommand {
    /// Compile the map from current state without saving it.
    Show,
    /// Compile and store it as the workbench's cached view (§33).
    Save,
    /// Write the markdown somewhere outside the database.
    Export { file: PathBuf },
    /// The last map that was saved, rather than a fresh compile.
    Latest,
}

#[derive(Subcommand)]
pub enum SnapshotCommand {
    Take {
        #[arg(long, default_value = "checkpoint")]
        label: String,
    },
    List,
}

#[derive(Subcommand)]
pub enum DriftCommand {
    /// Run every enabled rule against current state.
    Check,
    List,
    Resolve {
        alert_id: String,
        #[arg(long, value_parser = parse_drift_action)]
        action: DriftAction,
        #[arg(long, default_value = "no reason given")]
        reason: String,
    },
}

#[derive(Subcommand)]
pub enum InboxCommand {
    List,
    Resolve {
        /// Which §29 queue the item came from.
        #[arg(long, value_parser = parse_inbox_kind)]
        kind: InboxKind,
        id: String,
        #[arg(long, value_parser = parse_inbox_action)]
        action: InboxAction,
        #[arg(long, default_value = "no reason given")]
        reason: String,
    },
}

#[derive(Subcommand)]
pub enum EvidenceCommand {
    /// Every evidence row the project has recorded.
    List {
        #[arg(long, default_value_t = 40)]
        limit: usize,
    },
    /// One row by its own id.
    Show { id: String },
    /// `kind:id` — the rows linked to that record, e.g. `wp evidence for decision:<id>`.
    For { target: String },
}

fn parse_drift_action(raw: &str) -> Result<DriftAction, String> {
    match raw.trim().to_lowercase().as_str() {
        "keep-boundary" | "keepboundary" => Ok(DriftAction::KeepBoundary),
        "accept-change" | "acceptchange" => Ok(DriftAction::AcceptChange),
        "dismiss" => Ok(DriftAction::Dismiss),
        "resolve" => Ok(DriftAction::Resolve),
        other => Err(format!(
            "unknown action '{other}' — keep-boundary, accept-change, dismiss, resolve"
        )),
    }
}

fn parse_inbox_kind(raw: &str) -> Result<InboxKind, String> {
    match raw.trim().to_lowercase().replace('-', "_").as_str() {
        "low-confidence" | "low_confidence_event" | "event" => Ok(InboxKind::LowConfidenceEvent),
        "decision-proposal" | "decision_proposal" => Ok(InboxKind::DecisionProposal),
        "change-proposal" | "change_proposal" | "proposal" => Ok(InboxKind::ChangeProposal),
        "drift" | "drift_alert" => Ok(InboxKind::DriftAlert),
        "merge" | "merge_candidate" => Ok(InboxKind::MergeCandidate),
        "claim" | "unverified_claim" => Ok(InboxKind::UnverifiedClaim),
        "conflict" => Ok(InboxKind::Conflict),
        other => Err(format!(
            "unknown inbox kind '{other}' — low-confidence-event, decision-proposal, \
             change-proposal, drift, merge-candidate, unverified-claim, conflict"
        )),
    }
}

fn parse_inbox_action(raw: &str) -> Result<InboxAction, String> {
    match raw.trim().to_lowercase().as_str() {
        "accept" => Ok(InboxAction::Accept),
        "reject" => Ok(InboxAction::Reject),
        "proposal" | "to-change-proposal" | "tochangeproposal" => {
            Ok(InboxAction::ToChangeProposal)
        }
        "keep" => Ok(InboxAction::Keep),
        "dismiss" => Ok(InboxAction::Dismiss),
        other => Err(format!(
            "unknown action '{other}' — accept, reject, to-change-proposal, keep, dismiss"
        )),
    }
}

/// Collapse a run of whitespace so a quoted message prints on one line.
pub fn one_line(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            space = true;
            continue;
        }
        if !out.is_empty() && space {
            out.push(' ');
        }
        out.push(ch);
        space = false;
    }
    out
}

pub fn dispatch(
    app: &App,
    project: Option<&str>,
    command: ViewCommand,
) -> vibe_core::error::Result<()> {
    let id = app.project(project)?;
    let reducer = Reducer::new(app.repos.clone());
    match command {
        ViewCommand::State(inner) => state(app, &reducer, &id, inner),
        ViewCommand::Replay => {
            let report = reducer.replay_report(&id)?;
            if app.json {
                return app.print_json(&report);
            }
            app.say(format!("{} events applied", report.applied_events));
            app.say(format!("last seq {}", report.last_seq));
            app.say(format!("{} rows written", report.rows_written));
            app.say(format!("{} evidence rows", report.evidence_rows));
            app.say(format!("state hash {}", report.state_hash));
            if !report.unknown_types.is_empty() {
                app.say(format!("unhandled types: {}", report.unknown_types.join(", ")));
            }
            Ok(())
        }
        ViewCommand::Map(inner) => map(app, &id, inner),
        ViewCommand::Timeline { limit, pending } => {
            timeline(app, &id, limit, pending)
        }
        ViewCommand::Snapshot(inner) => snapshot(app, &id, inner),
        ViewCommand::Diff { snapshot } => {
            let service = SnapshotService::new(app.repos.clone());
            // `id` is the project; a `--snapshot` value is the baseline, not the subject.
            let diff = match snapshot {
                Some(since) => service.diff(&id, Some(&since)),
                None => service.since_last_snapshot(&id),
            }?;
            if app.json {
                return app.print_json(&diff);
            }
            println!("{}", diff.markdown());
            Ok(())
        }
        ViewCommand::Drift(inner) => drift(app, &reducer, &id, inner),
        ViewCommand::Inbox(inner) => inbox(app, &id, inner),
        ViewCommand::Evidence(inner) => evidence(app, &id, inner),
        ViewCommand::Duplicates {
            conversation,
            threshold,
            window,
            limit,
        } => duplicates(app, &id, conversation.as_deref(), threshold, window, limit),
        ViewCommand::Search {
            query,
            limit,
            kind,
        } => {
            // §40's three layers, not just the indexed one: an id the user typed should
            // beat a paragraph that happens to contain it, and a linked record should
            // widen the answer. Going through the port alone gives only the middle layer.
            let found = vibe_search::SearchService::new(app.repos.clone())
                .find_in(&id, &query, &kind, limit)?;
            if app.json {
                return app.print_json(
                    &found
                        .iter()
                        .map(|item| {
                            serde_json::json!({
                                "kind": item.hit.kind,
                                "id": item.hit.id,
                                "projectId": item.hit.project_id,
                                "title": item.hit.title,
                                "snippet": item.hit.snippet,
                                "layer": item.layer.as_str(),
                                "score": item.score,
                            })
                        })
                        .collect::<Vec<_>>(),
                );
            }
            if found.is_empty() {
                app.say("nothing found");
            }
            for item in found {
                app.say(format!(
                    "{}\t{}\t{:<10}\t{:.3}\t{}",
                    item.hit.kind,
                    item.hit.id,
                    item.layer.as_str(),
                    item.score,
                    one_line(&item.hit.snippet)
                ));
            }
            Ok(())
        }
        ViewCommand::Reindex => {
            // §40 "keep the index honest": the index is a derived view, so rebuilding it
            // is a repair rather than a migration, and running it twice is safe.
            let rows = vibe_search::SearchService::new(app.repos.clone()).reindex(&id)?;
            if app.json {
                return app.print_json(&serde_json::json!({ "indexedRecords": rows }));
            }
            app.say(format!("{rows} records indexed"));
            Ok(())
        }
        ViewCommand::Audit { limit } => {
            let entries = app.repos.audit.list(&id, limit)?;
            if app.json {
                return app.print_json(&entries);
            }
            if entries.is_empty() {
                app.say("no audit entries yet");
            }
            for entry in entries {
                app.say(format!(
                    "{}\t{}\t{}\tread {} → {}\tredacted {}",
                    entry.created_at.to_rfc3339(),
                    entry.component,
                    entry.action,
                    entry.read_refs.len(),
                    entry.produced_ref.unwrap_or_else(|| "-".into()),
                    entry.redacted_refs.len()
                ));
            }
            Ok(())
        }
    }
}

fn state(
    app: &App,
    reducer: &Reducer,
    id: &str,
    command: StateCommand,
) -> vibe_core::error::Result<()> {
    let state = reducer.project_read_only(id)?;
    match command {
        StateCommand::Show => app.print_json(&state),
        StateCommand::Summary => {
            if app.json {
                return app.print_json(&serde_json::json!({
                    "lastSeq": state.last_seq,
                    "appliedEvents": state.applied_events,
                    "tasks": state.tasks.len(),
                    "openIssues": state.issues.iter().filter(|i| i.status == vibe_domain::IssueStatus::Open).count(),
                    "activeDecisions": state.active_decisions().len(),
                    "artifacts": state.artifacts.len(),
                    "openDrift": state.drift_alerts.iter().filter(|a| a.status == vibe_domain::DriftStatus::Open).count(),
                }));
            }
            app.say(format!(
                "seq {}\t{} events\t{} tasks\t{} issues\t{} decisions\t{} artifacts",
                state.last_seq,
                state.applied_events,
                state.tasks.len(),
                state.issues.len(),
                state.decisions.len(),
                state.artifacts.len()
            ));
            let open = state
                .drift_alerts
                .iter()
                .filter(|alert| alert.status == vibe_domain::DriftStatus::Open)
                .count();
            app.say(format!("{open} open drift alert(s)"));
            Ok(())
        }
    }
}

fn map(app: &App, id: &str, command: MapCommand) -> vibe_core::error::Result<()> {
    let compiler = ProjectMapCompiler::new(app.repos.clone());
    match command {
        MapCommand::Show => {
            let map = compiler.compile(id)?;
            print_map(app, &map.markdown, &map.source_state_hash)
        }
        MapCommand::Save => {
            let map = compiler.save(id)?;
            print_map(app, &map.markdown, &map.source_state_hash)
        }
        MapCommand::Export { file } => {
            let map = compiler.compile(id)?;
            crate::app::write_file(&file, map.markdown.as_bytes())?;
            app.say(format!("{} bytes → {}", file.display(), map.markdown.len()));
            Ok(())
        }
        MapCommand::Latest => match compiler.latest(id)? {
            Some(map) => print_map(app, &map.markdown, &map.source_state_hash),
            None => {
                app.say("no map saved yet — run `wp map save`");
                Ok(())
            }
        },
    }
}

fn print_map(app: &App, markdown: &str, hash: &str) -> vibe_core::error::Result<()> {
    if app.json {
        return app.print_json(&serde_json::json!({ "markdown": markdown, "sourceStateHash": hash }));
    }
    println!("{markdown}");
    Ok(())
}

fn timeline(app: &App, id: &str, limit: usize, pending: bool) -> vibe_core::error::Result<()> {
    let store = vibe_core::event_store::EventStore::new(app.repos.clone());
    let statuses = if pending {
        None
    } else {
        Some([EventStatus::Accepted].as_slice())
    };
    let mut events = store.list(id, statuses, Some(limit))?;
    // Newest first for reading; the log itself stays seq-ordered everywhere else.
    events.sort_by_key(|event| std::cmp::Reverse(event.seq.unwrap_or(0)));
    if app.json {
        return app.print_json(&events);
    }
    for event in events {
        app.say(format!(
            "{:>5}  {}  {:<7} {:<7} {}",
            event.seq.unwrap_or(0),
            event.observed_at.format("%Y-%m-%d %H:%M"),
            event.actor_type.as_str(),
            status_slug(event.status),
            one_line(&event_summary(&event)),
        ));
    }
    Ok(())
}

fn status_slug(status: EventStatus) -> &'static str {
    match status {
        EventStatus::Accepted => "accepted",
        EventStatus::PendingReview => "pending",
        EventStatus::Rejected => "rejected",
        EventStatus::Superseded => "superseded",
    }
}

/// The payload's own words when it has any, else the subject it points at.
fn event_summary(event: &vibe_domain::ProjectEvent) -> String {
    for key in ["summary", "title", "text", "item", "decision"] {
        if let Some(value) = event.payload.get(key).and_then(|v| v.as_str()) {
            return format!("{} · {}", event.r#type, value);
        }
    }
    match (&event.subject_type, &event.subject_id) {
        (Some(kind), Some(subject)) => format!("{} · {kind} {subject}", event.r#type),
        _ => event.r#type.clone(),
    }
}

fn snapshot(app: &App, id: &str, command: SnapshotCommand) -> vibe_core::error::Result<()> {
    let service = SnapshotService::new(app.repos.clone());
    match command {
        SnapshotCommand::Take { label } => {
            let taken = service.take(id, &label)?;
            if app.json {
                return app.print_json(&taken);
            }
            app.say(format!(
                "{}  {}  seq {}",
                taken.id,
                taken.label,
                taken.state.last_seq
            ));
            Ok(())
        }
        SnapshotCommand::List => {
            let list = service.list(id)?;
            if app.json {
                return app.print_json(&list);
            }
            if list.is_empty() {
                app.say("no snapshots");
            }
            for item in list {
                app.say(format!(
                    "{}\t{}\t{} events\tseq {}",
                    item.id,
                    item.taken_at.to_rfc3339(),
                    item.label,
                    item.state.last_seq
                ));
            }
            Ok(())
        }
    }
}

fn drift(
    app: &App,
    reducer: &Reducer,
    id: &str,
    command: DriftCommand,
) -> vibe_core::error::Result<()> {
    let engine = DriftEngine::new(app.repos.clone());
    match command {
        DriftCommand::Check => {
            let state = reducer.project_read_only(id)?;
            let alerts = engine.check(&state)?;
            if app.json {
                return app.print_json(&alerts);
            }
            if alerts.is_empty() {
                app.say("no drift detected");
            }
            for alert in alerts {
                app.say(format!(
                    "{}\t{:<12} {:<8} {}",
                    alert.id,
                    alert.r#type.as_str(),
                    format!("{:?}", alert.severity).to_lowercase(),
                    one_line(&alert.summary)
                ));
            }
            Ok(())
        }
        DriftCommand::List => {
            let alerts = engine.list(id)?;
            if app.json {
                return app.print_json(&alerts);
            }
            if alerts.is_empty() {
                app.say("no alerts recorded");
            }
            for alert in alerts {
                app.say(format!(
                    "{}\t{:<12} {:?} {}",
                    alert.id,
                    alert.r#type.as_str(),
                    alert.status,
                    one_line(&alert.summary)
                ));
            }
            Ok(())
        }
        DriftCommand::Resolve {
            alert_id,
            action,
            reason,
        } => {
            let alert = engine.resolve(id, &alert_id, action, &reason)?;
            if app.json {
                return app.print_json(&alert);
            }
            app.say(format!("{} → {:?}", alert.id, alert.status));
            Ok(())
        }
    }
}

fn inbox(app: &App, id: &str, command: InboxCommand) -> vibe_core::error::Result<()> {
    let inbox = Inbox::new(app.repos.clone());
    match command {
        InboxCommand::List => {
            let items = inbox.list(id)?;
            if app.json {
                return app.print_json(&items);
            }
            if items.is_empty() {
                app.say("inbox empty");
            }
            for item in items {
                app.say(format!(
                    "{:<18} {}\t{}\t[{}]",
                    format!("{:?}", item.kind).to_lowercase(),
                    item.id,
                    one_line(&item.summary),
                    item.actions.join(", ")
                ));
            }
            Ok(())
        }
        InboxCommand::Resolve {
            kind,
            id: item_id,
            action,
            reason,
        } => {
            inbox.resolve(&InboxRef::new(id, item_id, kind), action, &reason)?;
            app.say("resolved");
            Ok(())
        }
    }
}

/// §45's traceability leg: a decision or an issue has to open onto the evidence it
/// cites. Replay maintains `evidence_links`, so this reads stored rows instead of
/// guessing from the record's own `evidenceRefs` array.
fn evidence(
    app: &App,
    project_id: &str,
    command: EvidenceCommand,
) -> vibe_core::error::Result<()> {
    let (rows, empty) = match command {
        EvidenceCommand::List { limit } => {
            let mut rows = app.repos.evidence.list_evidence(project_id)?;
            rows.truncate(limit);
            (rows, "no evidence recorded".to_string())
        }
        EvidenceCommand::Show { id } => (
            vec![app.repos.evidence.get_evidence(&id)?.ok_or_else(|| {
                CoreError::NotFound {
                    what: format!("evidence {id}"),
                }
            })?],
            String::new(),
        ),
        EvidenceCommand::For { target } => {
            let (kind, subject) = target.split_once(':').ok_or_else(|| {
                CoreError::Validation(format!("expected `kind:id`, got '{target}'"))
            })?;
            let kind = EntityKind::parse(kind).ok_or_else(|| {
                CoreError::Validation(format!(
                    "unknown entity kind '{kind}' — task, issue, decision, artifact, milestone, \
                     change_proposal, aep, drift_alert, evidence"
                ))
            })?;
            (
                app.repos.evidence.for_subject(kind, subject)?,
                format!("nothing linked to {target}"),
            )
        }
    };
    if app.json {
        return app.print_json(&rows);
    }
    if rows.is_empty() {
        app.say(empty);
    }
    for row in rows {
        app.say(format!(
            "{:<8}\t{}\t{}\t{}",
            row.r#type.as_str(),
            row.id,
            row.ref_id.as_deref().unwrap_or("-"),
            row.summary.as_deref().map(one_line).unwrap_or_default()
        ));
    }
    Ok(())
}

/// §52's L3 surface. What makes this command safe is its type rather than a promise: the
/// pass takes a slice and returns pairs, and every port call here is a `list_*`, so there is
/// no path from `wp duplicates` to a write. The count of messages still scanned is printed
/// because the whole point of the hint is that the material was not touched.
fn duplicates(
    app: &App,
    project_id: &str,
    conversation: Option<&str>,
    threshold: f64,
    window: usize,
    limit: usize,
) -> vibe_core::error::Result<()> {
    if !(0.0..=1.0).contains(&threshold) {
        return Err(CoreError::Validation(format!(
            "threshold must be between 0.0 and 1.0, got {threshold}"
        )));
    }
    let (scope, messages) = match conversation {
        Some(handle) => {
            let found = app
                .repos
                .conversations
                .get_conversation(project_id, handle)?
                .ok_or_else(|| CoreError::NotFound {
                    what: format!("conversation {handle}"),
                })?;
            let mut all = app.repos.conversations.list_messages(&found.id)?;
            // Oldest-first, so trimming keeps the recent end — the same window the
            // project-wide branch reads.
            if all.len() > window {
                all.drain(..all.len() - window);
            }
            (found.id, all)
        }
        None => (
            format!("project {project_id} (recent window)"),
            app.repos.conversations.recent_messages(project_id, window)?,
        ),
    };
    let hints = vibe_ingestion::scan_near_duplicates(&messages, threshold, limit);
    if app.json {
        return app.print_json(&serde_json::json!({
            "scope": scope,
            "threshold": threshold,
            "window": window,
            "messagesScanned": messages.len(),
            "messagesStillStored": messages.len(),
            "hints": hints,
            "note": vibe_ingestion::NOTE,
        }));
    }
    if hints.is_empty() {
        app.say(format!(
            "no near-duplicate hints in {} messages at threshold {threshold}",
            messages.len()
        ));
        return Ok(());
    }
    for hint in &hints {
        // The conversation id on its own line so `wp duplicates --conversation <id>` is
        // reachable from the hint, without the pair's two lines competing for width.
        app.say(format!(
            "{:.3}\tthread {}",
            hint.similarity, hint.conversation_id
        ));
        app.say(format!(
            "\tfirst  {}\t{}",
            hint.first_id,
            one_line(&hint.first_excerpt)
        ));
        app.say(format!(
            "\tsecond {}\t{}",
            hint.second_id,
            one_line(&hint.second_excerpt)
        ));
    }
    app.say(format!(
        "{} hint(s) over {} messages — both sides are still in the database; nothing was \
         merged, deleted or rewritten",
        hints.len(),
        messages.len()
    ));
    Ok(())
}
