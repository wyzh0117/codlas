//! Project Inbox — the one queue where the user finishes what rules started
//! (V1_DEVELOPMENT.md §29). Read side aggregates; write side only ever applies the
//! action the user clicked.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use vibe_core::error::CoreError;
use vibe_core::event_store::EventStore;
use vibe_core::ports::{EntityStore, Repositories};
use vibe_core::project_service::ProjectService;
use vibe_core::Result;
use vibe_domain::event_catalog;
use vibe_domain::{
    ActorType, ChangeProposal, ChangeProposalStatus, DriftAlert, DriftStatus, DriftType,
    EntityKind, EventStatus, InboxItem, InboxKind, ProjectState,
};

use crate::engine::{event_text, subject_of, DriftAction, DriftEngine};
use crate::text;

/// The §29 button set. Which subset applies depends on the item's kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InboxAction {
    Accept,
    Reject,
    ToChangeProposal,
    Keep,
    Dismiss,
}

impl InboxAction {
    pub fn slug(&self) -> &'static str {
        match self {
            InboxAction::Accept => "accept",
            InboxAction::Reject => "reject",
            InboxAction::ToChangeProposal => "to_change_proposal",
            InboxAction::Keep => "keep",
            InboxAction::Dismiss => "dismiss",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            InboxAction::Accept => "Accept",
            InboxAction::Reject => "Reject",
            InboxAction::ToChangeProposal => "Create Change Proposal",
            InboxAction::Keep => "Keep Boundary",
            InboxAction::Dismiss => "Dismiss",
        }
    }
}

/// Enough to address one queued item: the record behind it plus who owns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxRef {
    pub project_id: String,
    pub id: String,
    pub kind: InboxKind,
}

impl InboxRef {
    pub fn new(project_id: impl Into<String>, id: impl Into<String>, kind: InboxKind) -> Self {
        Self {
            project_id: project_id.into(),
            id: id.into(),
            kind,
        }
    }
}

pub struct Inbox {
    repos: Repositories,
    events: EventStore,
    projects: ProjectService,
    engine: DriftEngine,
}

impl Inbox {
    pub fn new(repos: Repositories) -> Self {
        Self {
            events: EventStore::new(repos.clone()),
            projects: ProjectService::new(repos.clone()),
            engine: DriftEngine::new(repos.clone()),
            repos,
        }
    }

    pub fn engine(&self) -> &DriftEngine {
        &self.engine
    }

    pub fn count(&self, project_id: &str) -> Result<usize> {
        Ok(self.list(project_id)?.len())
    }

    /// Pending events + open drift alerts + open change proposals + conflicts the
    /// rules can already see but nobody has recorded yet.
    pub fn list(&self, project_id: &str) -> Result<Vec<InboxItem>> {
        let state = self.state_for(project_id)?;
        let mut items: Vec<InboxItem> = Vec::new();
        // Alert ids are content-addressed, so a live conflict and its stored twin share one id.
        let mut seen: HashSet<String> = HashSet::new();

        for event in self
            .repos
            .events
            .list_events(project_id, Some(&[EventStatus::PendingReview]), None)?
        {
            let kind = if event.r#type == event_catalog::DECISION_PROPOSED {
                InboxKind::DecisionProposal
            } else {
                InboxKind::LowConfidenceEvent
            };
            if !seen.insert(event.id.clone()) {
                continue;
            }
            let body = event_text(&event).unwrap_or_else(|| event.r#type.clone());
            let matched = event
                .payload
                .get("matched")
                .and_then(|v| v.as_str())
                .map(|m| format!(" Matched \"{m}\"."))
                .unwrap_or_default();
            items.push(InboxItem {
                id: event.id.clone(),
                project_id: event.project_id.clone(),
                kind,
                summary: text::excerpt(&body, 140),
                detail: format!(
                    "{} recorded at confidence {:.2}.{matched} It is held here instead of being \
                     folded into project state (§27), so nothing you have not confirmed is real.",
                    event.r#type,
                    event.confidence
                ),
                ref_id: Some(event.id.clone()),
                created_at: event.observed_at,
                actions: vec![
                    InboxAction::Accept.label().to_string(),
                    InboxAction::Reject.label().to_string(),
                    InboxAction::ToChangeProposal.label().to_string(),
                ],
            });
        }

        for alert in state.open_drift() {
            if !seen.insert(alert.id.clone()) {
                continue;
            }
            items.push(self.alert_item(alert));
        }

        // A conflict is worth queueing the moment a rule sees it, even if `check()`
        // has not run yet; the stable alert id keeps it from being listed twice.
        for alert in self.engine.decision_conflict(&state) {
            if alert.status != DriftStatus::Open || !seen.insert(alert.id.clone()) {
                continue;
            }
            items.push(self.alert_item(&alert));
        }

        for proposal in state
            .change_proposals
            .iter()
            .filter(|p| p.status == ChangeProposalStatus::Open)
        {
            if !seen.insert(proposal.id.clone()) {
                continue;
            }
            let why = if proposal.charter_conflict {
                "It touches the Charter, so it can only move with your explicit acceptance."
            } else {
                "A baseline-touching change waits for a decision here (§23)."
            };
            items.push(InboxItem {
                id: proposal.id.clone(),
                project_id: proposal.project_id.clone(),
                kind: InboxKind::ChangeProposal,
                summary: text::excerpt(&proposal.proposed_change, 140),
                detail: format!(
                    "{} {why} Affected scope: {}.",
                    text::excerpt(&proposal.reason, 160),
                    if proposal.affected_scope.is_empty() {
                        "-".to_string()
                    } else {
                        proposal.affected_scope.join("; ")
                    }
                ),
                ref_id: Some(proposal.id.clone()),
                created_at: proposal.created_at.unwrap_or_else(vibe_domain::now),
                actions: vec![
                    InboxAction::Accept.label().to_string(),
                    InboxAction::Reject.label().to_string(),
                ],
            });
        }

        items.sort_by(|a, b| {
            rank(a.kind)
                .cmp(&rank(b.kind))
                .then_with(|| b.created_at.cmp(&a.created_at))
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(items)
    }

    /// Applies the action to the record behind the item.
    pub fn resolve(&self, item: &InboxRef, action: InboxAction, reason: &str) -> Result<()> {
        match item.kind {
            InboxKind::LowConfidenceEvent | InboxKind::DecisionProposal => {
                self.resolve_event(item, action, reason)
            }
            InboxKind::DriftAlert | InboxKind::Conflict => self.resolve_alert(item, action, reason),
            InboxKind::ChangeProposal => self.resolve_proposal(item, action, reason),
            InboxKind::MergeCandidate | InboxKind::UnverifiedClaim => Err(CoreError::Validation(
                "merge candidates and unverified claims are surfaced for reading in V1; \
                 resolving them is not wired yet"
                    .into(),
            )),
        }
    }

    fn resolve_event(&self, item: &InboxRef, action: InboxAction, reason: &str) -> Result<()> {
        let event = self
            .repos
            .events
            .get_event(&item.id)?
            .ok_or_else(|| CoreError::NotFound {
                what: format!("event {}", item.id),
            })?;
        match action {
            InboxAction::Accept | InboxAction::Keep => {
                self.events.review(&event.id, EventStatus::Accepted, None)?;
            }
            InboxAction::Reject | InboxAction::Dismiss => {
                self.events.review(&event.id, EventStatus::Rejected, None)?;
            }
            InboxAction::ToChangeProposal => {
                // Accepting the reading is what makes the proposal's source real.
                self.events.review(&event.id, EventStatus::Accepted, None)?;
                let body = event_text(&event).unwrap_or_else(|| event.r#type.clone());
                self.open_proposal(
                    &event.project_id,
                    &body,
                    reason,
                    vec![event.id.clone()],
                    &[],
                )?;
            }
        }
        Ok(())
    }

    fn resolve_alert(&self, item: &InboxRef, action: InboxAction, reason: &str) -> Result<()> {
        let drift = match action {
            InboxAction::Accept => Some(DriftAction::AcceptChange),
            InboxAction::Keep | InboxAction::Reject => Some(DriftAction::KeepBoundary),
            InboxAction::Dismiss => Some(DriftAction::Dismiss),
            InboxAction::ToChangeProposal => None,
        };
        let Some(drift) = drift else {
            let alert = self.alert(&item.project_id, &item.id)?;
            self.open_proposal(
                &alert.project_id,
                &alert.summary,
                reason,
                vec![alert.id.clone()],
                &alert.baseline_refs,
            )?;
            // The boundary has not moved yet, so the alert stays open beside the proposal.
            return Ok(());
        };
        self.engine.resolve(&item.project_id, &item.id, drift, reason)?;
        Ok(())
    }

    /// §7.4's confirm-to-lock, routed through the one service that owns it. Accepting
    /// here and accepting from `wp proposal accept` cannot be allowed to mean two things:
    /// the verdict is recorded *and* the proposal's machine-applicable `delta` is applied,
    /// because a human said so — 禁止 5 bars the model from rewriting a baseline, not the
    /// user's own acceptance from taking effect.
    fn resolve_proposal(&self, item: &InboxRef, action: InboxAction, reason: &str) -> Result<()> {
        let status = match action {
            InboxAction::Accept => ChangeProposalStatus::Accepted,
            InboxAction::Reject | InboxAction::Dismiss => ChangeProposalStatus::Rejected,
            // Keeping a proposal means leaving it open for later — no record moves.
            InboxAction::Keep => return Ok(()),
            InboxAction::ToChangeProposal => {
                return Err(CoreError::Validation(
                    "this item is already a change proposal".into(),
                ))
            }
        };
        let proposal = self
            .repos
            .entities
            .entity::<ChangeProposal>(EntityKind::ChangeProposal, &item.id)?
            .ok_or_else(|| CoreError::NotFound {
                what: format!("change proposal {}", item.id),
            })?;
        if proposal.status == status {
            return Ok(());
        }
        match status {
            ChangeProposalStatus::Accepted => {
                self.projects.accept_change_proposal(&item.project_id, &item.id, reason)?;
            }
            _ => {
                self.projects.reject_change_proposal(&item.project_id, &item.id, reason)?;
            }
        }
        Ok(())
    }

    fn alert_item(&self, alert: &DriftAlert) -> InboxItem {
        let kind = inbox_kind_for(alert);
        let why = match alert.r#type {
            DriftType::ScopeDrift => "work is worded like something the scope boundary excludes (§26.1)",
            DriftType::DecisionConflict => "new wording takes the opposite stance of a decision still in force (§26.2)",
            DriftType::ArchitectureDrift => "evidence reaches across an architecture invariant (§26.3)",
            DriftType::FeatureCreep => "new capability has no visible link back to the original design (§26.5)",
            DriftType::RepeatedRedesign => "the same object keeps being redesigned without a decision settling it (§26.4)",
            DriftType::StaleAssumption => "an active assumption is older than the facts around it (§26)",
            DriftType::UnplannedDependency => "a dependency appeared that the baseline never planned",
        };
        let subject = subject_of(alert);
        let mut actions = vec![
            InboxAction::Keep.label().to_string(),
            InboxAction::ToChangeProposal.label().to_string(),
            InboxAction::Dismiss.label().to_string(),
        ];
        if alert.r#type == DriftType::ScopeDrift || alert.r#type == DriftType::ArchitectureDrift {
            actions.insert(0, InboxAction::Accept.label().to_string());
        }
        InboxItem {
            id: alert.id.clone(),
            project_id: alert.project_id.clone(),
            kind,
            summary: text::excerpt(&alert.summary, 140),
            detail: format!(
                "Queued because {why}. Measured against {}; subject {subject}. \
                 Evidence: {}. Severity {:?} — a prompt, never a block (§8.6).",
                if alert.baseline_refs.is_empty() {
                    "the recorded state".to_string()
                } else {
                    alert.baseline_refs.join("; ")
                },
                if alert.evidence_refs.is_empty() {
                    "-".to_string()
                } else {
                    alert.evidence_refs.join("; ")
                },
                alert.severity
            ),
            ref_id: Some(alert.id.clone()),
            created_at: alert.detected_at.unwrap_or_else(vibe_domain::now),
            actions,
        }
    }

    /// The Inbox reads the same objects the reducer maintains; when no projection has
    /// been built yet it assembles the same shape straight from the ports.
    fn state_for(&self, project_id: &str) -> Result<ProjectState> {
        let entities = &self.repos.entities;
        Ok(ProjectState {
            project_id: project_id.to_string(),
            project: self.repos.projects.get_project(project_id)?,
            charter: self.repos.projects.get_active_charter(project_id)?,
            scope: self.repos.projects.get_active_scope(project_id)?,
            baseline: self.repos.projects.get_active_baseline(project_id)?,
            tasks: entities.entities_of(EntityKind::Task, project_id)?,
            issues: entities.entities_of(EntityKind::Issue, project_id)?,
            decisions: entities.entities_of(EntityKind::Decision, project_id)?,
            artifacts: entities.entities_of(EntityKind::Artifact, project_id)?,
            milestones: entities.entities_of(EntityKind::Milestone, project_id)?,
            change_proposals: entities.entities_of(EntityKind::ChangeProposal, project_id)?,
            aeps: entities.entities_of(EntityKind::Aep, project_id)?,
            drift_alerts: entities.entities_of(EntityKind::DriftAlert, project_id)?,
            evidence: Vec::new(),
            ..ProjectState::default()
        })
    }

    fn alert(&self, project_id: &str, alert_id: &str) -> Result<DriftAlert> {
        let alert = self
            .repos
            .entities
            .entity::<DriftAlert>(EntityKind::DriftAlert, alert_id)?
            .ok_or_else(|| CoreError::NotFound {
                what: format!("drift alert {alert_id}"),
            })?;
        if alert.project_id != project_id {
            return Err(CoreError::NotFound {
                what: format!("drift alert {alert_id} in project {project_id}"),
            });
        }
        Ok(alert)
    }

    /// Filing a proposal is the only sanctioned way for a detected conflict to reach a
    /// baseline, and it still needs the user's acceptance (PROJECT_MASTER §19 禁止 5).
    fn open_proposal(
        &self,
        project_id: &str,
        proposed_change: &str,
        reason: &str,
        evidence_refs: Vec<String>,
        baseline_refs: &[String],
    ) -> Result<ChangeProposal> {
        let proposal = ChangeProposal {
            id: String::new(),
            project_id: project_id.to_string(),
            proposed_change: text::excerpt(proposed_change, 200),
            reason: if reason.trim().is_empty() {
                "raised from the Project Inbox".to_string()
            } else {
                reason.trim().to_string()
            },
            affected_scope: baseline_refs
                .iter()
                .filter(|r| r.starts_with("scope:"))
                .filter_map(|r| r.split(':').nth(3).map(String::from))
                .collect(),
            affected_architecture: baseline_refs
                .iter()
                .filter(|r| r.starts_with("baseline:"))
                .cloned()
                .collect(),
            evidence_refs,
            ..Default::default()
        };
        self.projects
            .open_change_proposal(project_id, proposal, ActorType::User)
    }
}

fn inbox_kind_for(alert: &DriftAlert) -> InboxKind {
    match alert.r#type {
        DriftType::DecisionConflict => InboxKind::Conflict,
        _ => InboxKind::DriftAlert,
    }
}

fn rank(kind: InboxKind) -> u8 {
    match kind {
        InboxKind::Conflict => 0,
        InboxKind::DriftAlert => 1,
        InboxKind::DecisionProposal => 2,
        InboxKind::ChangeProposal => 3,
        InboxKind::UnverifiedClaim => 4,
        InboxKind::MergeCandidate => 5,
        InboxKind::LowConfidenceEvent => 6,
    }
}
