//! §42: the retrieval ladder, and §59: the bounded windows it may read through.
//!
//! Retrieval is a walk down a priority ladder, not a search engine. Each rung asks
//! one question of the stores — "what did the user select", "what did the Charter
//! promise", "what is currently true", "what was recently seen" — and each rung is
//! allowed a *window*, never the whole history (§59: the context engine must never
//! resend everything).
//!
//! Every read is logged (`read_refs`) so the compiler can audit it (§54): an audit
//! row that says "this pack was built" is worthless unless it also says what was
//! looked at to build it.

use vibe_core::{EntityStore, Repositories, SearchFilters};
use vibe_domain::{ContextPackItem, ContextSection, EntityKind, EventStatus, Project, ProjectMap};

use crate::budget::estimate_tokens;
use vibe_security::refs::BODY_MARKER;
use crate::request::{ContextRequest, Excerpt, Selection};
use crate::section::{
    AEP, ARTIFACTS, CHARTER_AND_SCOPE, CONVERSATION, CURRENT_ISSUE, DIRECT_ENTITY, PROJECT_MAP,
    RECENT_EVENTS, REQUEST, SELECTED,
};

/// Items read per ladder rung when the caller has no preference. Small on purpose:
/// eight recent events is plenty of context, and 200 would not fit the budget (§43).
pub const DEFAULT_WINDOW: usize = 8;
/// The most a caller may ask for. A window is a bound, not a suggestion.
pub const MAX_WINDOW: usize = 40;
/// Characters of a message body that may enter a pack. §53 can remove the transcript
/// entirely; this is the bound that applies when it does not.
pub const MESSAGE_EXCERPT_CHARS: usize = 240;

/// Everything one retrieval pass produced.
#[derive(Debug, Clone, Default)]
pub struct Retrieved {
    /// Candidate items in §42 order, before the policy gate and before the budget.
    pub items: Vec<ContextPackItem>,
    /// Every ref this pass read, including refs whose items were later filtered.
    pub read_refs: Vec<String>,
    pub project: Option<Project>,
    pub map: Option<ProjectMap>,
    pub window: usize,
}

/// A compiled pack item, with its token cost already measured.
fn item(
    section: ContextSection,
    source_ref: impl Into<String>,
    content: String,
    priority: u8,
) -> ContextPackItem {
    let token_cost = estimate_tokens(&content);
    ContextPackItem {
        section,
        source_ref: source_ref.into(),
        content,
        priority,
        token_cost,
        redacted: false,
    }
}

/// Collects `read_refs` without repeating them: a project read once and used ten
/// times should still be one line in the audit trail.
#[derive(Debug, Default)]
struct ReadLog {
    seen: std::collections::HashSet<String>,
    refs: Vec<String>,
}

impl ReadLog {
    fn add(&mut self, r: impl Into<String>) {
        let r = r.into();
        if self.seen.insert(r.clone()) {
            self.refs.push(r);
        }
    }
}

/// Terms the pack should lean towards: the question plus any focus hints.
fn terms(req: &ContextRequest) -> Vec<String> {
    let mut joined = req.focus.join(" ").to_lowercase();
    joined.push(' ');
    joined.push_str(&req.question.to_lowercase());
    let mut out: Vec<String> = joined
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| token.chars().count() > 3)
        .take(48)
        .map(str::to_string)
        .collect();
    out.sort();
    out.dedup();
    out
}

/// How strongly one piece of content matches the terms. A count, so ordering is
/// deterministic; equal scores keep the order the store handed them over in.
fn score(content: &str, terms: &[String]) -> usize {
    let haystack = content.to_lowercase();
    terms
        .iter()
        .filter(|term| haystack.contains(term.as_str()))
        .count()
}

/// A stable preview of a body of text, cut at a word boundary and never inside a
/// multi-byte character. Longer than the window it ends in an ellipsis, so a reader
/// can always tell an excerpt from a complete statement (§59).
fn excerpt(text: &str, max_chars: usize) -> String {
    let text = text.trim();
    let cut = match text.char_indices().nth(max_chars) {
        Some((at, _)) => at,
        None => return text.to_string(),
    };
    let head = &text[..cut];
    let end = match head.rfind(' ') {
        Some(at) if at > cut / 2 => at,
        _ => cut,
    };
    format!("{} \u{2026}", head[..end].trim_end())
}

/// Resolve one user selection. Entity kinds go through the generic entity store;
/// events and messages have their own ports.
fn selected(
    repos: &Repositories,
    sel: &Selection,
    req: &ContextRequest,
    read: &mut ReadLog,
) -> Vec<ContextPackItem> {
    let source_ref = sel.source_ref();
    if let Some(kind) = sel.entity_kind() {
        match repos.entities.get(kind, &sel.id) {
            Ok(Some(value)) => {
                read.add(source_ref.clone());
                let label = sel
                    .label
                    .clone()
                    .or_else(|| value.get("title").and_then(|v| v.as_str()).map(str::to_string))
                    .unwrap_or_else(|| format!("{kind:?} {}", sel.id));
                let detail = value
                    .get("detail")
                    .or_else(|| value.get("decision"))
                    .or_else(|| value.get("summary"))
                    .and_then(|v| v.as_str())
                    .map(|s| format!(" — {}", excerpt(s, 160)))
                    .unwrap_or_default();
                let status = value
                    .get("status")
                    .and_then(|v| v.as_str())
                    .map(|s| format!(" [{s}]"))
                    .unwrap_or_default();
                return vec![item(
                    ContextSection::SelectedContext,
                    source_ref,
                    format!("{label}{status}{detail}"),
                    SELECTED,
                )];
            }
            Ok(None) => {
                return vec![item(
                    ContextSection::SelectedContext,
                    source_ref.clone(),
                    format!("selected {sel:?} was not found in this project"),
                    SELECTED,
                )];
            }
            Err(_) => return Vec::new(),
        }
    }

    match sel.kind.as_str() {
        "event" => match repos.events.get_event(&sel.id) {
            Ok(Some(event)) => {
                read.add(source_ref.clone());
                vec![item(
                    ContextSection::SelectedContext,
                    source_ref,
                    format!(
                        "{} — {}",
                        event.r#type,
                        excerpt(&event.payload.to_string(), 160)
                    ),
                    SELECTED,
                )]
            }
            _ => Vec::new(),
        },
        "message" => {
            // Messages are addressed through their conversation, so a bare message
            // selection is matched inside the window the §42.9 rung is allowed to read.
            let limit = (req.window.unwrap_or(DEFAULT_WINDOW) * 4).min(MAX_WINDOW * 4);
            match repos.conversations.recent_messages(&req.project_id, limit) {
                Ok(messages) => {
                    read.add(format!("messages:recent:{limit}"));
                    messages
                        .iter()
                        .filter(|message| message.id == sel.id)
                        .map(|message| {
                            read.add(format!("message:{}", message.id));
                            item(
                                ContextSection::SelectedContext,
                                format!("message:{}", message.id),
                                format!(
                                    "{} said: {}",
                                    message.role.as_str(),
                                    excerpt(&message.content, MESSAGE_EXCERPT_CHARS)
                                ),
                                SELECTED,
                            )
                        })
                        .collect()
                }
                Err(_) => Vec::new(),
            }
        }
        "conversation" => {
            read.add(source_ref.clone());
            match repos.conversations.get_conversation(&req.project_id, &sel.id) {
                Ok(Some(conversation)) => {
                    let title = conversation
                        .title
                        .clone()
                        .unwrap_or_else(|| conversation.id.clone());
                    vec![item(
                        ContextSection::SelectedContext,
                        source_ref,
                        format!("conversation '{title}' selected by the user"),
                        SELECTED,
                    )]
                }
                _ => Vec::new(),
            }
        }
        other => vec![item(
            ContextSection::SelectedContext,
            source_ref,
            format!("selection of {other}:{} is not readable by this build", sel.id),
            SELECTED,
        )],
    }
}

/// §42 ladder over one project. Pure reads; no policy, no budget, no rendering.
pub fn gather(repos: &Repositories, req: &ContextRequest) -> vibe_core::Result<Retrieved> {
    let window = req
        .window
        .unwrap_or(DEFAULT_WINDOW)
        .clamp(1, MAX_WINDOW);
    let terms = terms(req);
    let mut read = ReadLog::default();
    let mut items: Vec<ContextPackItem> = Vec::new();

    // ── §42.1 Selected Context ────────────────────────────────────────────────
    for sel in &req.selections {
        items.extend(selected(repos, sel, req, &mut read));
    }

    // ── Project / Charter / Scope ─────────────────────────────────────────────
    let project = repos.projects.get_project(&req.project_id)?;
    if let Some(project) = &project {
        read.add(format!("project:{}", project.id));
        let phase = project
            .current_phase
            .clone()
            .unwrap_or_else(|| "no phase recorded".to_string());
        items.push(item(
            ContextSection::CurrentPhase,
            format!("project:{}", project.id),
            format!("Project '{}' is in phase: {phase}", project.name),
            CHARTER_AND_SCOPE,
        ));
    }

    if let Some(charter) = repos.projects.get_active_charter(&req.project_id)? {
        read.add(format!("charter:{}", charter.id));
        items.push(item(
            ContextSection::OriginalIntent,
            format!("charter:{}", charter.id),
            format!(
                "Original problem (charter v{}): {} — core value: {}{}",
                charter.version,
                charter.original_problem,
                charter.core_value,
                charter
                    .target_user
                    .as_ref()
                    .map(|u| format!(" — for: {u}"))
                    .unwrap_or_default()
            ),
            CHARTER_AND_SCOPE,
        ));
        if !charter.constraints.is_empty() || !charter.architecture_principles.is_empty() {
            let guard: Vec<String> = charter
                .architecture_principles
                .iter()
                .chain(charter.constraints.iter())
                .cloned()
                .collect();
            items.push(item(
                ContextSection::ArchitectureConstraints,
                format!("charter:{}", charter.id),
                format!("Charter guardrails: {}", guard.join("; ")),
                CHARTER_AND_SCOPE + 1,
            ));
        }
    }

    if let Some(scope) = repos.projects.get_active_scope(&req.project_id)? {
        read.add(format!("scope:{}", scope.id));
        items.push(item(
            ContextSection::RelevantScope,
            format!("scope:{}", scope.id),
            format!(
                "Scope v{} — in: [{}]; out: [{}]{}",
                scope.version,
                scope.in_scope.join(", "),
                scope.out_of_scope.join(", "),
                if scope.forbidden.is_empty() {
                    String::new()
                } else {
                    format!("; forbidden: [{}]", scope.forbidden.join(", "))
                }
            ),
            CHARTER_AND_SCOPE,
        ));
    }

    if let Some(baseline) = repos.projects.get_active_baseline(&req.project_id)? {
        read.add(format!("baseline:{}", baseline.id));
        let invariants: Vec<String> = baseline
            .invariants
            .iter()
            .map(|invariant| format!("{}: {}", invariant.id, invariant.text))
            .collect();
        let layers: Vec<String> = baseline
            .layers
            .iter()
            .map(|layer| layer.name.clone())
            .collect();
        items.push(item(
            ContextSection::ArchitectureConstraints,
            format!("baseline:{}", baseline.id),
            format!(
                "Architecture baseline v{}: {} (layers: [{}]){}",
                baseline.version,
                baseline.summary,
                layers.join(", "),
                if invariants.is_empty() {
                    String::new()
                } else {
                    format!("; invariants: {}", invariants.join("; "))
                }
            ),
            ARTIFACTS,
        ));
    }

    // ── §42.3 Active Decisions ────────────────────────────────────────────────
    let decisions: Vec<vibe_domain::Decision> = repos
        .entities
        .entities_of(EntityKind::Decision, &req.project_id)?
        .into_iter()
        .filter(|decision: &vibe_domain::Decision| {
            decision.status == vibe_domain::DecisionStatus::Active
        })
        .collect();
    for decision in sorted(&decisions, &terms, |d| {
        format!("{} {}", d.title, d.decision)
    }) {
        read.add(format!("decision:{}", decision.id));
        items.push(item(
            ContextSection::ActiveDecisions,
            format!("decision:{}", decision.id),
            format!(
                "{}{}{}",
                decision.title,
                // A decision captured from a one-line summary has a title and no body;
                // joining an empty clause with " — " reads as a broken record. Same rule
                // the map applies at §31.
                if decision.decision.is_empty() || decision.decision == decision.title {
                    String::new()
                } else {
                    format!(" — {}", decision.decision)
                },
                if decision.locked {
                    " [user-confirmed, locked]"
                } else {
                    ""
                }
            ),
            DIRECT_ENTITY,
        ));
    }

    // ── §42.4 / §42.6 Direct entity and current issue ─────────────────────────
    let issues: Vec<vibe_domain::Issue> = repos
        .entities
        .entities_of(EntityKind::Issue, &req.project_id)?
        .into_iter()
        .filter(|issue: &vibe_domain::Issue| {
            !matches!(
                issue.status,
                vibe_domain::IssueStatus::Resolved | vibe_domain::IssueStatus::WontFix
            )
        })
        .collect();
    let selected_ids: Vec<&str> = req.selections.iter().map(|s| s.id.as_str()).collect();
    for issue in sorted(&issues, &terms, |i| format!("{} {}", i.title, i.status.as_str())) {
        let priority = if selected_ids.contains(&issue.id.as_str()) {
            DIRECT_ENTITY
        } else {
            CURRENT_ISSUE
        };
        read.add(format!("issue:{}", issue.id));
        items.push(item(
            ContextSection::RelatedIssue,
            format!("issue:{}", issue.id),
            format!("Open issue {}: {} ({})", issue.id, issue.title, issue.status.as_str()),
            priority,
        ));
    }

    let tasks: Vec<vibe_domain::Task> = repos
        .entities
        .entities_of(EntityKind::Task, &req.project_id)?
        .into_iter()
        .filter(|task: &vibe_domain::Task| {
            !matches!(
                task.status,
                vibe_domain::TaskStatus::Done | vibe_domain::TaskStatus::Cancelled
            )
        })
        .collect();
    for task in sorted(&tasks, &terms, |t| {
        format!("{} {}", t.title, t.status.as_str())
    }) {
        read.add(format!("task:{}", task.id));
        items.push(item(
            ContextSection::RelatedIssue,
            format!("task:{}", task.id),
            format!(
                "{} task {}: {}{}",
                task.status.as_str(),
                task.id,
                task.title,
                if task.unverified_claim {
                    " [completion claimed, no evidence yet]"
                } else {
                    ""
                }
            ),
            CURRENT_ISSUE,
        ));
    }

    // ── §42.5 Relevant Artifacts ──────────────────────────────────────────────
    let artifacts: Vec<vibe_domain::Artifact> = repos
        .artifacts
        .list_artifacts(&req.project_id)?
        .into_iter()
        .filter(|artifact: &vibe_domain::Artifact| !artifact.deleted)
        .collect();
    for artifact in sorted(&artifacts, &terms, |a| {
        format!("{} {}", a.name, a.path.clone().unwrap_or_default())
    }) {
        let source_ref = format!("artifact:{}", artifact.id);
        read.add(source_ref.clone());
        // §22: path, hash, type and version. Never the file body.
        let mut line = format!(
            "{} ({}, v{}, {})",
            artifact
                .path
                .clone()
                .unwrap_or_else(|| artifact.name.clone()),
            artifact_kind(&artifact),
            artifact.version,
            artifact
                .hash
                .clone()
                .unwrap_or_else(|| "no hash recorded".to_string())
        );
        if !artifact.relationships.is_empty() {
            line.push_str(&format!(" — relates to: {}", artifact.relationships.join(", ")));
        }
        items.push(item(
            ContextSection::RelevantArtifacts,
            source_ref,
            line,
            ARTIFACTS,
        ));
    }

    // ── §42.7 Recent Events (bounded, §59) ────────────────────────────────────
    let events = repos.events.list_events(
        &req.project_id,
        Some(&[EventStatus::Accepted][..]),
        Some(window),
    )?;
    read.add(format!("events:accepted:{}", events.len()));
    for event in events {
        read.add(format!("event:{}", event.id));
        let subject = event
            .subject_id
            .as_ref()
            .map(|id| format!(" → {id}"))
            .unwrap_or_default();
        items.push(item(
            ContextSection::RecentEvidence,
            format!("event:{}", event.id),
            format!(
                "{}{} (confidence {:.0})",
                event.r#type,
                subject,
                event.confidence
            ),
            RECENT_EVENTS,
        ));
    }

    // ── §42.8 AEP and Drift warnings ──────────────────────────────────────────
    let aeps: Vec<vibe_domain::ArchitectureEvolutionPlan> = repos
        .entities
        .entities_of(EntityKind::Aep, &req.project_id)?
        .into_iter()
        .filter(|aep: &vibe_domain::ArchitectureEvolutionPlan| {
            matches!(
                aep.status,
                vibe_domain::AepStatus::Dormant | vibe_domain::AepStatus::Watching
            )
        })
        .collect();
    for aep in aeps {
        read.add(format!("aep:{}", aep.id));
        items.push(item(
            ContextSection::RelatedAep,
            format!("aep:{}", aep.id),
            format!(
                "{} {} [{}]: {} — deferred because {}; revisit when [{}]",
                aep.code,
                aep.title,
                aep.status.label(),
                aep.limitation,
                aep.why_not_now,
                aep.trigger.join(", ")
            ),
            AEP,
        ));
    }

    let drift: Vec<vibe_domain::DriftAlert> = repos
        .entities
        .entities_of(EntityKind::DriftAlert, &req.project_id)?
        .into_iter()
        .filter(|alert: &vibe_domain::DriftAlert| alert.status == vibe_domain::DriftStatus::Open)
        .collect();
    for alert in sorted(&drift, &terms, |a| a.summary.clone()) {
        read.add(format!("drift:{}", alert.id));
        items.push(item(
            ContextSection::DriftWarnings,
            format!("drift:{}", alert.id),
            format!("Drift ({}): {}", alert.summary, alert.r#type.as_str()),
            AEP,
        ));
    }

    // ── §42.9 Related Conversation (bounded, §59) ─────────────────────────────
    let messages = repos.conversations.recent_messages(&req.project_id, window)?;
    read.add(format!("messages:recent:{}", messages.len()));
    let mut conversation_items: Vec<ContextPackItem> = Vec::new();
    for message in messages {
        read.add(format!("message:{}", message.id));
        conversation_items.push(item(
            ContextSection::RecentEvidence,
            format!("message:{}", message.id),
            format!(
                "{} said: {}",
                message.role.as_str(),
                excerpt(&message.content, MESSAGE_EXCERPT_CHARS)
            ),
            CONVERSATION,
        ));
    }
    items.extend(conversation_items);

    // ── §42.10 General Project Map ────────────────────────────────────────────
    let map = repos.snapshots.latest_project_map(&req.project_id)?;
    match &map {
        Some(map) => {
            read.add(format!("map:{}", map.project_id));
            for (heading, body) in split_map(&map.markdown) {
                items.push(item(
                    ContextSection::CurrentPhase,
                    format!("map:{}#{heading}", map.project_id),
                    format!("{heading}: {body}"),
                    PROJECT_MAP,
                ));
            }
        }
        // No compiled map yet: say so, rather than pretending the pack is complete.
        None => items.push(item(
            ContextSection::CurrentPhase,
            format!("map:{}", req.project_id),
            "No project map has been compiled for this project yet (§30); this pack is built \
             from the rungs above."
                .to_string(),
            PROJECT_MAP,
        )),
    }

    // ── §41 Request: the user's own words, first-hand ─────────────────────────
    items.push(item(
        ContextSection::Request,
        "request:self",
        req.question.clone(),
        REQUEST,
    ));

    // The ladder order: rung first, then priority inside the rung, then relevance.
    // The sort is stable, so two items with equal relevance keep the order the store
    // handed them over in — which is what makes a rebuilt pack diff-clean.
    items.sort_by(|a, b| {
        crate::section::rank(a.section)
            .cmp(&crate::section::rank(b.section))
            .then_with(|| a.priority.cmp(&b.priority))
            .then_with(|| score(&b.content, &terms).cmp(&score(&a.content, &terms)))
    });

    // Text a caller already holds in memory rides along on its own reference line,
    // where the §53 gate can see it and take it away again if it must.
    attach_excerpts(&mut items, &req.excerpts);

    Ok(Retrieved {
        items,
        read_refs: read.refs,
        project,
        map,
        window,
    })
}

/// Which store an entity list came from, in §42's ordering: relevance first, and
/// inside a tie the order the repository returned (stable sort).
fn sorted<T, F: Fn(&T) -> String>(rows: &[T], terms: &[String], text: F) -> Vec<T>
where
    T: Clone,
{
    let mut rows: Vec<T> = rows.to_vec();
    rows.sort_by(|a, b| score(&text(b), terms).cmp(&score(&text(a), terms)));
    rows
}

fn artifact_kind(artifact: &vibe_domain::Artifact) -> &'static str {
    match artifact.kind {
        vibe_domain::ArtifactKind::SourceFile => "source_file",
        vibe_domain::ArtifactKind::Config => "config",
        vibe_domain::ArtifactKind::Document => "document",
        vibe_domain::ArtifactKind::Database => "database",
        vibe_domain::ArtifactKind::Prompt => "prompt",
        vibe_domain::ArtifactKind::Diagram => "diagram",
        vibe_domain::ArtifactKind::GeneratedOutput => "generated_output",
        vibe_domain::ArtifactKind::Test => "test",
        vibe_domain::ArtifactKind::Script => "script",
        vibe_domain::ArtifactKind::Module => "module",
    }
}

/// The compiled map arrives as markdown; a pack needs it in pieces small enough for
/// the §43 map share to spend them last.
fn split_map(markdown: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut heading: Option<String> = None;
    let mut body: Vec<String> = Vec::new();
    for line in markdown.lines() {
        let line = line.trim_end();
        if let Some(title) = line.strip_prefix("## ") {
            if let Some(heading) = heading.take() {
                out.push((heading, body.join(" ").trim().to_string()));
            }
            body.clear();
            heading = Some(title.to_string());
        } else if !line.is_empty() && !line.starts_with("# ") {
            body.push(line.trim().to_string());
        }
    }
    if let Some(heading) = heading {
        out.push((heading, body.join(" ").trim().to_string()));
    }
    out.into_iter()
        .filter(|(_, body)| !body.is_empty())
        .collect()
}

/// Hand a caller-owned excerpt to one compiled reference line.
///
/// The engine never opens a file (§22 keeps bodies out of the store, and this crate
/// is forbidden from touching the filesystem). An adapter or app that already holds
/// text in memory may attach it here so the pack can carry it — and so the §53 gate
/// has something to cut back off when `neverSendFiles` says it may not travel.
pub fn attach_excerpts(items: &mut [ContextPackItem], excerpts: &[Excerpt]) {
    for excerpt in excerpts {
        for item in items.iter_mut().filter(|item| item.source_ref == excerpt.source_ref) {
            item.content = format!("{}{BODY_MARKER}{}", item.content, excerpt.text);
            item.token_cost = estimate_tokens(&item.content);
        }
    }
}

/// §40 L2: the ranked search hits a pack can lean on. Only used to *order* what
/// retrieval already found — a hit is never a fact the store did not return.
pub fn search_refs(
    repos: &Repositories,
    req: &ContextRequest,
    kinds: &[&str],
    limit: usize,
) -> vibe_core::Result<Vec<String>> {
    let filters = SearchFilters {
        project_id: Some(req.project_id.clone()),
        kinds: kinds.iter().map(|kind| kind.to_string()).collect(),
        limit,
    };
    Ok(repos
        .search
        .search(&req.question, &filters)?
        .into_iter()
        .map(|hit| format!("{}:{}", hit.kind, hit.id))
        .collect())
}
