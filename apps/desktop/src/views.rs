//! The eleven views the shell draws, built by the engines that already exist.
//!
//! Nothing in this file decides anything. Each builder reads through the same ports the CLI
//! uses and reshapes the result for a webview — the JSON a screen shows is a projection of a
//! projection, never a second copy of project state (§18, 禁止 4). The one function that writes
//! is [`apply`], and it writes only the verdict the user already gave (§7.4).

use serde::Serialize;
use vibe_context_engine::{ContextCompiler, ContextRequest, Selection};
use vibe_core::error::{CoreError, Result};
use vibe_core::event_store::EventStore;
use vibe_guard_engine::{DriftEngine, Inbox, InboxAction, InboxRef};
use vibe_ingestion::{fallback_for, DEFAULT_THRESHOLD, DEFAULT_WINDOW};
use vibe_search::SearchService;
use vibe_state_engine::{ProjectMapCompiler, Reducer, SnapshotService};
use vibe_cli::app::App;
use vibe_cli::context::parse_ref;
use vibe_domain::{
    ChangeProposalStatus, DriftSeverity, EventStatus, InboxKind, ProjectEvent,
};

// ────────────────────────────── ① 变动 ──────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffEntryView {
    /// `label: old → new`, in the workbench's own words, so the webview adds no logic.
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffCard {
    pub name: String,
    /// `added` / `changed` / `removed` — the CSS class the module block wears.
    pub change: &'static str,
    pub entries: Vec<DiffEntryView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingItem {
    pub id: String,
    /// `proposal` for a change proposal, `inbox` for anything else waiting on a verdict.
    pub source: &'static str,
    pub kind: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// The verbs the record behind this item actually accepts (§29's button set).
    pub actions: Vec<String>,
    pub at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffView {
    pub from_snapshot: String,
    pub to_snapshot: String,
    pub captured_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub sections: Vec<DiffCard>,
    pub pending: Vec<PendingItem>,
}

pub fn diff_view(app: &App, project_id: &str) -> Result<DiffView> {
    let diff = SnapshotService::new(app.repos.clone()).since_last_snapshot(project_id)?;
    let inbox = Inbox::new(app.repos.clone()).list(project_id)?;

    let mut sections: Vec<DiffCard> = Vec::new();
    push_section(&mut sections, "任务", &diff.tasks);
    push_section(&mut sections, "问题", &diff.issues);
    push_section(&mut sections, "决策", &diff.decisions);
    push_section(&mut sections, "交付物", &diff.artifacts);
    push_section(&mut sections, "里程碑", &diff.milestones);
    push_section(&mut sections, "AEP", &diff.aeps);
    push_section(&mut sections, "偏航提示", &diff.drift_alerts);
    if !diff.next_actions.added.is_empty() {
        sections.push(card("下一步", "added", &diff.next_actions.added));
    }
    if !diff.next_actions.removed.is_empty() {
        sections.push(card("下一步", "removed", &diff.next_actions.removed));
    }
    if let Some(line) = version_move("Charter 版本", diff.charter_from_version, diff.charter_to_version)
    {
        sections.push(line);
    }
    if let Some(line) = version_move(
        "架构基线版本",
        diff.baseline_from_version,
        diff.baseline_to_version,
    ) {
        sections.push(line);
    }

    let pending = inbox.into_iter().map(PendingItem::from_inbox).collect();
    Ok(DiffView {
        from_snapshot: diff
            .since_label
            .clone()
            .unwrap_or_else(|| "无快照基线（全部视为新增）".to_string()),
        to_snapshot: short(&diff.current_state_hash),
        captured_at: vibe_domain::now().timestamp(),
        note: diff.since_label.is_none().then(|| {
            "还没有快照可比对，下面列出的都是当前状态的全部内容。".to_string()
        }),
        sections,
        pending,
    })
}

impl PendingItem {
    fn from_inbox(item: vibe_domain::InboxItem) -> Self {
        let kind = kind_slug(&item.kind).to_string();
        Self {
            source: if kind == "change_proposal" {
                "proposal"
            } else {
                "inbox"
            },
            detail: (!item.detail.trim().is_empty()).then_some(item.detail),
            kind,
            id: item.id,
            summary: item.summary,
            actions: item.actions,
            at: item.created_at.timestamp(),
        }
    }
}

fn push_section(cards: &mut Vec<DiffCard>, name: &str, section: &vibe_state_engine::DiffSection) {
    for (change, entries) in [
        ("added", &section.added),
        ("changed", &section.changed),
        ("removed", &section.removed),
    ] {
        if entries.is_empty() {
            continue;
        }
        cards.push(DiffCard {
            name: name.to_string(),
            change,
            entries: entries.iter().map(entry).collect(),
        });
    }
}

fn entry(item: &vibe_state_engine::DiffEntry) -> DiffEntryView {
    let text = match (&item.from, &item.to) {
        (Some(from), Some(to)) => format!("{}：{} → {}", item.label, from, to),
        (None, Some(to)) => format!("{}：→ {}", item.label, to),
        _ => item.label.clone(),
    };
    DiffEntryView { text }
}

fn card(name: &str, change: &'static str, lines: &[String]) -> DiffCard {
    DiffCard {
        name: name.to_string(),
        change,
        entries: lines
            .iter()
            .map(|text| DiffEntryView { text: text.clone() })
            .collect(),
    }
}

fn version_move(name: &str, from: Option<u32>, to: Option<u32>) -> Option<DiffCard> {
    match (from, to) {
        (Some(from), Some(to)) if from != to => Some(card(
            name,
            "changed",
            &[format!("v{from} → v{to}")],
        )),
        _ => None,
    }
}

fn short(hash: &str) -> String {
    let cut: String = hash.chars().take(12).collect();
    if cut.chars().count() < hash.chars().count() {
        format!("{cut}…")
    } else {
        cut
    }
}

// ─────────────────────────── 确认即锁定 ───────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Refusal {
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub applied: Vec<String>,
    pub refused: Vec<Refusal>,
}

/// One ticked row: which record, of what kind, and which of its own buttons the user chose.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingChoice {
    pub id: String,
    pub kind: String,
    pub action: String,
}

/// §7.4 as the shell applies it: the verdict goes through the same `Inbox::resolve` the CLI's
/// `wp inbox resolve` calls, so accepting a proposal moves the baseline in exactly one way.
/// A bad kind or an unknown verb is refused with its reason rather than guessed at.
pub fn apply(app: &App, project_id: &str, choices: &[PendingChoice]) -> ApplyResult {
    let inbox = Inbox::new(app.repos.clone());
    let mut applied = Vec::new();
    let mut refused = Vec::new();
    for choice in choices {
        let outcome = (|| -> Result<()> {
            let kind = parse_kind(&choice.kind).ok_or_else(|| {
                CoreError::Validation(format!("unknown inbox kind '{}'", choice.kind))
            })?;
            let action = action_from_label(&choice.action).ok_or_else(|| {
                CoreError::Validation(format!("unknown action '{}'", choice.action))
            })?;
            inbox.resolve(&InboxRef::new(project_id, &choice.id, kind), action, "工作台确认")
        })();
        match outcome {
            Ok(()) => applied.push(choice.id.clone()),
            Err(error) => refused.push(Refusal {
                id: choice.id.clone(),
                reason: error.to_string(),
            }),
        }
    }
    ApplyResult { applied, refused }
}

fn kind_slug(kind: &InboxKind) -> &'static str {
    match kind {
        InboxKind::LowConfidenceEvent => "low_confidence_event",
        InboxKind::DecisionProposal => "decision_proposal",
        InboxKind::ChangeProposal => "change_proposal",
        InboxKind::DriftAlert => "drift_alert",
        InboxKind::MergeCandidate => "merge_candidate",
        InboxKind::UnverifiedClaim => "unverified_claim",
        InboxKind::Conflict => "conflict",
    }
}

fn parse_kind(raw: &str) -> Option<InboxKind> {
    Some(match raw {
        "low_confidence_event" => InboxKind::LowConfidenceEvent,
        "decision_proposal" => InboxKind::DecisionProposal,
        "change_proposal" => InboxKind::ChangeProposal,
        "drift_alert" => InboxKind::DriftAlert,
        "merge_candidate" => InboxKind::MergeCandidate,
        "unverified_claim" => InboxKind::UnverifiedClaim,
        "conflict" => InboxKind::Conflict,
        _ => return None,
    })
}

/// [`InboxItem::actions`] carries display labels; the shell sends one back, so the round trip
/// has to be by label rather than by a slug the webview was never shown.
fn action_from_label(label: &str) -> Option<InboxAction> {
    Some(match label.trim() {
        "Accept" => InboxAction::Accept,
        "Reject" => InboxAction::Reject,
        "Create Change Proposal" => InboxAction::ToChangeProposal,
        "Keep Boundary" => InboxAction::Keep,
        "Dismiss" => InboxAction::Dismiss,
        _ => return None,
    })
}

// ────────────────────────────── ② 项目地图 ──────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapSection {
    pub name: String,
    pub lines: Vec<String>,
    pub empty: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapView {
    pub compiled_at: i64,
    pub state_hash: String,
    pub sections: Vec<MapSection>,
}

pub fn map_view(app: &App, project_id: &str) -> Result<MapView> {
    let map = ProjectMapCompiler::new(app.repos.clone()).compile(project_id)?;
    Ok(MapView {
        compiled_at: map.generated_at.timestamp(),
        state_hash: map.source_state_hash.clone(),
        sections: split_sections(&map.markdown),
    })
}

/// The compiled markdown is the artifact (§33), so the shell renders *it* rather than
/// re-deriving sections: one compiler, one shape, no second map language.
fn split_sections(markdown: &str) -> Vec<MapSection> {
    let mut sections = Vec::new();
    for block in markdown.split("\n## ").skip(1) {
        let mut lines = block.lines();
        let name = lines.next().unwrap_or_default().trim().to_string();
        let body: Vec<String> = lines
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(|line| line.strip_prefix("- ").unwrap_or(line).to_string())
            .collect();
        let empty = body.len() == 1
            && body[0].replace('_', "") == "none recorded"
            || body.iter().all(|line| line.starts_with('_'));
        sections.push(MapSection {
            name,
            empty,
            lines: body,
        });
    }
    sections
}

// ─────────────────────────────── 总览 ───────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusTally {
    pub status: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KindRow {
    pub name: &'static str,
    pub total: usize,
    /// Empty where a kind has no status to group by — the shell does not invent one.
    pub tally: Vec<StatusTally>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewView {
    pub project_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub charter_version: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_version: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_version: Option<u32>,
    pub last_seq: i64,
    pub applied_events: u64,
    pub state_hash: String,
    pub built_at: i64,
    pub kinds: Vec<KindRow>,
    pub open_proposals: usize,
    pub drift_findings: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_snapshot: Option<String>,
    pub note: String,
}

/// The wire string a status enum already wears, so a screen never carries a third vocabulary
/// next to the stored one (§13's one-language rule, same reason the CLI prints these).
fn slug<S: Serialize>(status: &S) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Counted, not sorted: the fold's order is the log's order, and a screen that re-ranks it
/// would be deciding what matters.
fn tally<T, F>(items: &[T], status_of: F) -> Vec<StatusTally>
where
    F: Fn(&T) -> String,
{
    let mut out: Vec<StatusTally> = Vec::new();
    for item in items {
        let status = status_of(item);
        match out.iter_mut().find(|row| row.status == status) {
            Some(row) => row.count += 1,
            None => out.push(StatusTally { status, count: 1 }),
        }
    }
    out
}

pub fn overview_view(app: &App, project_id: &str) -> Result<OverviewView> {
    let state = Reducer::new(app.repos.clone()).project_read_only(project_id)?;
    let project = state
        .project
        .clone()
        .ok_or_else(|| CoreError::NotFound {
            what: format!("project {project_id}"),
        })?;
    let snapshot = SnapshotService::new(app.repos.clone()).latest(project_id)?;
    let kinds = vec![
        KindRow {
            name: "任务",
            total: state.tasks.len(),
            tally: tally(&state.tasks, |task| slug(&task.status)),
        },
        KindRow {
            name: "问题",
            total: state.issues.len(),
            tally: tally(&state.issues, |issue| slug(&issue.status)),
        },
        KindRow {
            name: "决策",
            total: state.decisions.len(),
            tally: tally(&state.decisions, |decision| slug(&decision.status)),
        },
        KindRow {
            name: "产物",
            total: state.artifacts.len(),
            tally: Vec::new(),
        },
        KindRow {
            name: "里程碑",
            total: state.milestones.len(),
            tally: Vec::new(),
        },
        KindRow {
            name: "变更提案",
            total: state.change_proposals.len(),
            tally: tally(&state.change_proposals, |proposal| slug(&proposal.status)),
        },
        KindRow {
            name: "演进提案",
            total: state.aeps.len(),
            tally: tally(&state.aeps, |aep| slug(&aep.status)),
        },
        KindRow {
            name: "证据",
            total: state.evidence.len(),
            tally: Vec::new(),
        },
    ];

    Ok(OverviewView {
        project_name: project.name,
        root_path: project.root_path,
        charter_version: state.charter.as_ref().map(|item| item.version),
        scope_version: state.scope.as_ref().map(|item| item.version),
        baseline_version: state.baseline.as_ref().map(|item| item.version),
        last_seq: state.last_seq,
        applied_events: state.applied_events,
        state_hash: state.state_hash.clone().unwrap_or_default(),
        built_at: state.built_at.map(|time| time.timestamp()).unwrap_or(0),
        open_proposals: state
            .change_proposals
            .iter()
            .filter(|proposal| proposal.status == ChangeProposalStatus::Open)
            .count(),
        drift_findings: state.drift_alerts.len(),
        latest_snapshot: snapshot
            .as_ref()
            .map(|item| format!("{} · seq {}", item.label, item.state.last_seq)),
        kinds,
        note: "这些数字是已接受事件折叠出来的结果，不是第二套状态：要看某一条，去时间线或项目地图。".to_string(),
    })
}

// ─────────────────────────────── 决策 ───────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionRow {
    pub id: String,
    pub title: String,
    pub decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    pub alternatives: Vec<String>,
    pub status: String,
    /// §7.4's mark: a user-confirmed decision does not move because a later agent claim says so.
    pub locked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
    pub at: i64,
    pub evidence: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionsView {
    pub decisions: Vec<DecisionRow>,
    pub note: String,
}

pub fn decisions_view(app: &App, project_id: &str) -> Result<DecisionsView> {
    let state = Reducer::new(app.repos.clone()).project_read_only(project_id)?;
    Ok(DecisionsView {
        decisions: state
            .decisions
            .iter()
            .map(|decision| DecisionRow {
                id: decision.id.clone(),
                title: decision.title.clone(),
                decision: decision.decision.clone(),
                rationale: decision.rationale.clone(),
                alternatives: decision.alternatives.clone(),
                status: slug(&decision.status),
                locked: decision.locked,
                supersedes: decision.supersedes.clone(),
                at: decision.decided_at.map(|time| time.timestamp()).unwrap_or(0),
                evidence: decision.evidence_refs.len(),
            })
            .collect(),
        note: "改一条决策要回到事件本身：被取代由新事件说明，锁定由用户确认。这一屏不提供编辑。"
            .to_string(),
    })
}

// ────────────────────────────── 演进（AEP） ──────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AepRow {
    pub id: String,
    pub code: String,
    pub title: String,
    pub current: String,
    pub limitation: String,
    pub why_not_now: String,
    pub trigger: Vec<String>,
    pub target: String,
    pub change_surface: Vec<String>,
    pub preserved_core: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub benefit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvolutionView {
    pub aeps: Vec<AepRow>,
    pub note: String,
}

pub fn evolution_view(app: &App, project_id: &str) -> Result<EvolutionView> {
    let state = Reducer::new(app.repos.clone()).project_read_only(project_id)?;
    Ok(EvolutionView {
        aeps: state
            .aeps
            .iter()
            .map(|aep| AepRow {
                id: aep.id.clone(),
                code: aep.code.clone(),
                title: aep.title.clone(),
                current: aep.current.clone(),
                limitation: aep.limitation.clone(),
                why_not_now: aep.why_not_now.clone(),
                trigger: aep.trigger.clone(),
                target: aep.target.clone(),
                change_surface: aep.change_surface.clone(),
                preserved_core: aep.preserved_core.clone(),
                benefit: aep.benefit.clone(),
                cost: aep.cost.clone(),
                status: slug(&aep.status),
            })
            .collect(),
        note: "AEP 说的是「什么时候该改」，不是待办清单：触发条件没到，它就应当停在这里（禁止 8）。"
            .to_string(),
    })
}

// ─────────────────────────────── 时间线 ───────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineRow {
    pub seq: i64,
    pub at: i64,
    pub kind: String,
    pub actor: String,
    pub status: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineView {
    pub events: Vec<TimelineRow>,
}

pub fn timeline_view(app: &App, project_id: &str, limit: usize) -> Result<TimelineView> {
    let mut events = EventStore::new(app.repos.clone()).list(
        project_id,
        Some([EventStatus::Accepted].as_slice()),
        Some(limit),
    )?;
    events.sort_by_key(|event| std::cmp::Reverse(event.seq.unwrap_or(0)));
    Ok(TimelineView {
        events: events.iter().map(TimelineRow::of).collect(),
    })
}

impl TimelineRow {
    fn of(event: &ProjectEvent) -> Self {
        Self {
            seq: event.seq.unwrap_or(0) as i64,
            at: event.observed_at.timestamp(),
            kind: event.r#type.clone(),
            actor: event.actor_type.as_str().to_string(),
            status: status_slug(event.status).to_string(),
            summary: summary_of(event),
        }
    }
}

fn status_slug(status: EventStatus) -> &'static str {
    match status {
        EventStatus::Accepted => "accepted",
        EventStatus::PendingReview => "pending",
        EventStatus::Rejected => "rejected",
        EventStatus::Superseded => "superseded",
    }
}

/// The payload's own words when it has any, else the record it points at — the same rule the
/// CLI's timeline uses, so both surfaces describe one event the same way.
fn summary_of(event: &ProjectEvent) -> String {
    for key in ["summary", "title", "text", "item", "decision"] {
        if let Some(value) = event.payload.get(key).and_then(|v| v.as_str()) {
            return format!("{} · {}", event.r#type, one_line(value));
        }
    }
    match (&event.subject_type, &event.subject_id) {
        (Some(kind), Some(subject)) => format!("{} · {kind} {subject}", event.r#type),
        _ => event.r#type.clone(),
    }
}

pub fn one_line(text: &str) -> String {
    vibe_cli::views::one_line(text)
}

// ───────────────────────────── 偏航 / Inbox ─────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriftRow {
    pub id: String,
    pub rule: String,
    pub severity: String,
    pub status: String,
    pub at: i64,
    pub message: String,
    pub subjects: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxRow {
    pub id: String,
    pub kind: String,
    pub summary: String,
    pub at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriftView {
    pub findings: Vec<DriftRow>,
    pub inbox: Vec<InboxRow>,
}

pub fn drift_view(app: &App, project_id: &str) -> Result<DriftView> {
    let alerts = DriftEngine::new(app.repos.clone()).list(project_id)?;
    let inbox = Inbox::new(app.repos.clone()).list(project_id)?;
    Ok(DriftView {
        findings: alerts
            .iter()
            .map(|alert| DriftRow {
                id: alert.id.clone(),
                rule: alert.r#type.as_str().to_string(),
                severity: severity_slug(alert.severity).to_string(),
                status: format!("{:?}", alert.status).to_lowercase(),
                at: alert.detected_at.map(|at| at.timestamp()).unwrap_or(0),
                message: alert.summary.clone(),
                subjects: alert
                    .baseline_refs
                    .iter()
                    .chain(alert.evidence_refs.iter())
                    .cloned()
                    .collect(),
            })
            .collect(),
        inbox: inbox
            .into_iter()
            .map(|item| InboxRow {
                id: item.id,
                kind: kind_slug(&item.kind).to_string(),
                summary: item.summary,
                at: item.created_at.timestamp(),
            })
            .collect(),
    })
}

fn severity_slug(severity: DriftSeverity) -> &'static str {
    match severity {
        DriftSeverity::Info => "info",
        DriftSeverity::Warning => "warning",
        DriftSeverity::High => "high",
    }
}

// ─────────────────────────────── 检索 ───────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRow {
    pub kind: String,
    pub id: String,
    pub title: String,
    pub snippet: String,
    pub layer: String,
    pub score: f64,
}

pub fn search_view(
    app: &App,
    project_id: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchRow>> {
    let found = SearchService::new(app.repos.clone()).find_in(project_id, query, &[], limit)?;
    Ok(found
        .iter()
        .map(|item| SearchRow {
            kind: item.hit.kind.clone(),
            id: item.hit.id.clone(),
            title: item.hit.title.clone(),
            snippet: one_line(&item.hit.snippet),
            layer: item.layer.as_str().to_string(),
            score: item.score,
        })
        .collect())
}

// ───────────────────────────── 上下文预览 ─────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanRow {
    pub priority: u8,
    pub section: String,
    pub source_ref: String,
    pub tokens: usize,
    pub kept: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedView {
    pub adapter_id: String,
    pub text: String,
    /// True when no adapter can inject, so the copy fallback is the whole path (§62).
    pub copy_fallback: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FallbackView {
    pub instruction: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deep_link: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextView {
    pub kept_count: usize,
    pub total_count: usize,
    pub token_estimate: usize,
    pub redacted_count: usize,
    /// The `kind:id` this pack was focused on, when a click rather than a sentence asked for it
    /// (§14 item 7). The screen shows it so a focused pack never looks like a general one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
    pub rows: Vec<PlanRow>,
    pub prepared: PreparedView,
    pub fallback: FallbackView,
}

pub fn context_view(
    app: &App,
    project_id: &str,
    question: &str,
    max_tokens: usize,
    to: &str,
    select: Option<&str>,
) -> Result<ContextView> {
    let compiler = ContextCompiler::new(app.repos.clone());
    let focus = select.map(str::trim).filter(|text| !text.is_empty());
    // §42's ladder reads the materialized rows, so the screen that shows a pack first makes sure
    // they are the current fold — the same write-through `wp map` and `wp snapshot take` do.
    // Without it a record accepted after the last map visit compiles as "not found in this
    // project", which is ADR-030's `project_read_only` hazard arriving through the click path.
    Reducer::new(app.repos.clone()).project_state(project_id)?;
    let (req, focused) = request(project_id, question, max_tokens, to, focus)?;
    let planned = compiler.plan(&req)?;
    let pack = compiler.build(&req)?;
    let payload = compiler.prepare(&pack, to)?;
    let fallback = fallback_for(&payload.target, &payload.adapter_id, None);
    Ok(ContextView {
        kept_count: planned.iter().filter(|item| item.kept).count(),
        total_count: planned.len(),
        token_estimate: pack.token_estimate,
        redacted_count: pack.redacted_count,
        focus: focused,
        rows: planned
            .iter()
            .map(|item| PlanRow {
                priority: item.priority,
                section: item.section.heading().to_string(),
                source_ref: item.source_ref.clone(),
                tokens: item.estimated_tokens,
                kept: item.kept,
                reason: item.reason.clone(),
            })
            .collect(),
        prepared: PreparedView {
            adapter_id: payload.adapter_id.clone(),
            text: payload.text.clone(),
            copy_fallback: payload.copy_fallback,
        },
        fallback: FallbackView {
            instruction: fallback.instruction,
            payload_path: fallback.payload_path,
            deep_link: fallback.deep_link,
        },
    })
}

/// The same defaults `wp context build` uses: a bounded budget, and the destination stated so
/// §53's policy filter has something to judge against. A click on one record is itself an intent,
/// so §14's item-7 path needs no prose — the reference is written into the question verbatim,
/// exactly as the CLI does it, and the returned `kind:id` is what the screen names as the focus.
fn request(
    project_id: &str,
    question: &str,
    max_tokens: usize,
    to: &str,
    focus: Option<&str>,
) -> Result<(ContextRequest, Option<String>)> {
    let text = match (question.trim(), focus) {
        (text, _) if !text.is_empty() => text.to_string(),
        (_, Some(reference)) => format!("context for {reference}"),
        _ => {
            return Err(CoreError::Validation(
                "nothing to compile: ask a question, or click a record to focus on".into(),
            ))
        }
    };
    let mut req = ContextRequest::new(project_id, text)
        .for_agent(to)
        .with_budget(ContextCompiler::default_budget(max_tokens));
    let mut focused = None;
    if let Some(reference) = focus {
        let (kind, id) = parse_ref(reference, "selection")?;
        focused = Some(format!("{kind}:{id}"));
        req = req.select(Selection::new(kind, id));
    }
    Ok((req, focused))
}

// ───────────────────────────── 健康与备份 ─────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterRow {
    pub id: String,
    pub outcome: String,
    pub last_run: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateRow {
    pub left: String,
    pub right: String,
    pub score: f64,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupView {
    pub path: String,
    pub size_bytes: u64,
    pub version: i64,
    pub restorable: bool,
    pub integrity: String,
    pub rows_in_store: i64,
    pub projects_in_store: i64,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthView {
    pub adapters: Vec<AdapterRow>,
    pub duplicates: Vec<DuplicateRow>,
    pub backup: BackupView,
}

pub fn health_view(app: &App, project_id: &str) -> Result<HealthView> {
    let adapters = app
        .repos
        .adapters
        .list_states()?
        .into_iter()
        .map(|state| AdapterRow {
            id: state.adapter_id,
            outcome: state.health.as_str().to_string(),
            last_run: state.updated_at.timestamp(),
            detail: state.detail,
        })
        .collect();

    // §52's L3 stays a hint: this reads a bounded window and writes nothing.
    let messages = app.repos.conversations.recent_messages(project_id, DEFAULT_WINDOW)?;
    let duplicates = vibe_ingestion::scan_near_duplicates(&messages, DEFAULT_THRESHOLD, 20)
        .into_iter()
        .map(|hint| DuplicateRow {
            left: hint.first_id,
            right: hint.second_id,
            score: hint.similarity,
            preview: format!(
                "{} ↔ {}",
                one_line(&hint.first_excerpt),
                one_line(&hint.second_excerpt)
            ),
        })
        .collect();

    // `inspect` reads the file the workbench lives in, so these are the same numbers
    // `wp backup status` prints rather than a second opinion about them.
    let report = app.maintenance.inspect(&app.db)?;
    let version = app.maintenance.schema_version()?;
    let size_bytes = std::fs::metadata(&app.db).map(|m| m.len()).unwrap_or(0);
    Ok(HealthView {
        adapters,
        duplicates,
        backup: BackupView {
            path: app.db.display().to_string(),
            size_bytes,
            version,
            restorable: report.restorable(version).is_ok(),
            integrity: report.integrity,
            rows_in_store: report.events,
            projects_in_store: report.projects,
            note: vibe_ingestion::NOTE.to_string(),
        },
    })
}

// ─────────────────────────────── 项目 ───────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectView {
    pub project_id: String,
    pub project_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_path: Option<String>,
    pub db_path: String,
    pub last_seq: i64,
    pub applied_events: u64,
}

pub fn project_view(app: &App, project_id: &str) -> Result<ProjectView> {
    let project = app
        .repos
        .projects
        .get_project(project_id)?
        .ok_or_else(|| CoreError::NotFound {
            what: format!("project {project_id}"),
        })?;
    let state = Reducer::new(app.repos.clone()).project_read_only(project_id)?;
    Ok(ProjectView {
        project_id: project.id,
        project_name: project.name,
        root_path: project.root_path,
        db_path: app.db.display().to_string(),
        last_seq: state.last_seq,
        applied_events: state.applied_events,
    })
}
