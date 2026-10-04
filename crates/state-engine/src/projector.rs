//! The pure half of the reducer: accepted events in `seq` order in, event-derived
//! [`ProjectState`] out. Nothing in this module can reach a repository, which is what
//! makes the projection reproducible and lets a rebuild be a re-fold rather than a
//! read-back (V1_DEVELOPMENT.md §18, PROJECT_MASTER §7.2 / 禁止 4).
//!
//! Rules applied here are the documented ones: §19 task claims, §20 decisions,
//! §21 issues, §22 artifacts, §23 change proposals, §24 AEPs, §25 drift.

use std::collections::HashMap;

use serde_json::Value;
use vibe_core::error::{CoreError, Result};
use vibe_domain::{
    event_catalog, ActorType, AepStatus, ArchitectureEvolutionPlan, Artifact, ArtifactKind,
    ChangeProposal, ChangeProposalStatus, Decision, DecisionStatus, DriftAlert, DriftSeverity,
    DriftStatus, DriftType, EntityKind, Evidence, EvidenceType, Issue, IssueStatus, Milestone,
    ProjectEvent, ProjectState, Task, TaskStatus,
};

use crate::payload::{
    boolean, clip, event_time, excerpt, number, resolve_id, serde_slug as slug, text, text_list,
};

/// A materialised row the fold produced, kept typed so persistence can call the
/// matching port instead of hand-decoding JSON.
#[derive(Debug, Clone)]
pub(crate) enum Projected {
    Task(Task),
    Issue(Issue),
    Decision(Decision),
    Artifact(Artifact),
    Milestone(Milestone),
    Proposal(ChangeProposal),
    Aep(ArchitectureEvolutionPlan),
    Drift(DriftAlert),
}

impl Projected {
    fn kind(&self) -> EntityKind {
        match self {
            Projected::Task(_) => EntityKind::Task,
            Projected::Issue(_) => EntityKind::Issue,
            Projected::Decision(_) => EntityKind::Decision,
            Projected::Artifact(_) => EntityKind::Artifact,
            Projected::Milestone(_) => EntityKind::Milestone,
            Projected::Proposal(_) => EntityKind::ChangeProposal,
            Projected::Aep(_) => EntityKind::Aep,
            Projected::Drift(_) => EntityKind::DriftAlert,
        }
    }

    fn id(&self) -> &str {
        match self {
            Projected::Task(t) => &t.id,
            Projected::Issue(i) => &i.id,
            Projected::Decision(d) => &d.id,
            Projected::Artifact(a) => &a.id,
            Projected::Milestone(m) => &m.id,
            Projected::Proposal(p) => &p.id,
            Projected::Aep(a) => &a.id,
            Projected::Drift(d) => &d.id,
        }
    }

    fn status(&self) -> String {
        match self {
            Projected::Task(t) => slug(&t.status),
            Projected::Issue(i) => slug(&i.status),
            Projected::Decision(d) => slug(&d.status),
            Projected::Artifact(a) => if a.deleted { "deleted" } else { "active" }.into(),
            Projected::Milestone(m) => if m.reached { "reached" } else { "planned" }.into(),
            Projected::Proposal(p) => slug(&p.status),
            Projected::Aep(a) => slug(&a.status),
            Projected::Drift(d) => slug(&d.status),
        }
    }

    fn json(&self) -> Result<Value> {
        Ok(match self {
            Projected::Task(t) => serde_json::to_value(t)?,
            Projected::Issue(i) => serde_json::to_value(i)?,
            Projected::Decision(d) => serde_json::to_value(d)?,
            Projected::Artifact(a) => serde_json::to_value(a)?,
            Projected::Milestone(m) => serde_json::to_value(m)?,
            Projected::Proposal(p) => serde_json::to_value(p)?,
            Projected::Aep(a) => serde_json::to_value(a)?,
            Projected::Drift(d) => serde_json::to_value(d)?,
        })
    }
}

pub(crate) struct VersionWrite {
    pub number: u32,
    pub before: Value,
    pub after: Value,
    pub reason: String,
}

/// One cache row to write, plus a version-log entry when a status moved.
pub(crate) struct EntityWrite {
    pub kind: EntityKind,
    pub id: String,
    pub status: String,
    pub entity: Projected,
    pub version: Option<VersionWrite>,
}

/// An evidence row and the entities it vouches for (§17).
pub(crate) struct EvidenceWrite {
    pub evidence: Evidence,
    pub subjects: Vec<(EntityKind, String)>,
}

pub(crate) struct FoldOutput {
    pub state: ProjectState,
    pub writes: Vec<EntityWrite>,
    pub evidence: Vec<EvidenceWrite>,
    /// Unknown event types: tolerated and counted, never fatal.
    pub unknown_types: Vec<String>,
}

pub(crate) fn fold(project_id: &str, events: &[ProjectEvent]) -> Result<FoldOutput> {
    let mut projector = Projector::new(project_id);
    for event in events {
        projector.apply(event)?;
    }
    Ok(projector.finish())
}

struct Projector {
    project_id: String,
    state: ProjectState,
    writes: Vec<EntityWrite>,
    evidence: Vec<EvidenceWrite>,
    versions: HashMap<String, u32>,
    unknown: Vec<String>,
}

/// Ensure-then-edit-then-stage for one section. `f` receives the row plus whether
/// it was created by this event.
macro_rules! section {
    ($name:ident, $field:ident, $ty:ty, $variant:ident) => {
        fn $name<F>(&mut self, ev: &ProjectEvent, id: &str, f: F) -> Result<()>
        where
            F: FnOnce(&mut $ty, bool),
        {
            let before = self.state.$field.iter().find(|row| row.id == id).cloned();
            let fresh = before.is_none();
            if fresh {
                let mut row = <$ty>::default();
                row.id = id.to_string();
                row.project_id = self.project_id.clone();
                self.state.$field.push(row);
            }
            let after = {
                let row = self
                    .state
                    .$field
                    .iter_mut()
                    .find(|row| row.id == id)
                    .ok_or_else(|| {
                        CoreError::Validation(format!("missing row {} in {}", id, stringify!($field)))
                    })?;
                f(row, fresh);
                Projected::$variant(row.clone())
            };
            self.record(ev, before.map(Projected::$variant), after)
        }
    };
}

impl Projector {
    fn new(project_id: &str) -> Self {
        Self {
            project_id: project_id.to_string(),
            state: ProjectState {
                project_id: project_id.to_string(),
                ..Default::default()
            },
            writes: Vec::new(),
            evidence: Vec::new(),
            versions: HashMap::new(),
            unknown: Vec::new(),
        }
    }

    fn finish(mut self) -> FoldOutput {
        self.unknown.sort();
        self.unknown.dedup();
        FoldOutput {
            state: self.state,
            writes: self.writes,
            evidence: self.evidence,
            unknown_types: self.unknown,
        }
    }

    section!(task, tasks, Task, Task);
    section!(issue, issues, Issue, Issue);
    section!(decision, decisions, Decision, Decision);
    section!(artifact, artifacts, Artifact, Artifact);
    section!(milestone, milestones, Milestone, Milestone);
    section!(proposal, change_proposals, ChangeProposal, Proposal);
    section!(aep, aeps, ArchitectureEvolutionPlan, Aep);
    section!(drift, drift_alerts, DriftAlert, Drift);

    fn apply(&mut self, ev: &ProjectEvent) -> Result<()> {
        self.state.applied_events += 1;
        if let Some(seq) = ev.seq {
            self.state.last_seq = self.state.last_seq.max(seq);
        }
        if !event_catalog::is_known(&ev.r#type) {
            self.unknown.push(ev.r#type.clone());
        }
        // Event-level trace first: it vouches for the named subject even when the
        // event changes no state.
        self.trace_evidence(ev);

        match ev.r#type.as_str() {
            event_catalog::TASK_CREATED => self.task_created(ev),
            event_catalog::TASK_STARTED => self.task_status(ev, TaskStatus::Doing),
            event_catalog::TASK_BLOCKED => self.task_blocked(ev),
            event_catalog::TASK_COMPLETED_CLAIM => self.task_claimed(ev),
            event_catalog::TASK_COMPLETED_CONFIRMED => self.task_confirmed(ev),
            event_catalog::TASK_CANCELLED => self.task_status(ev, TaskStatus::Cancelled),

            event_catalog::ISSUE_FOUND => self.issue_found(ev),
            event_catalog::ISSUE_RESOLVED => self.issue_resolved(ev),

            event_catalog::DECISION_CONFIRMED => self.decision_confirmed(ev),
            event_catalog::DECISION_SUPERSEDED => self.decision_superseded(ev),
            event_catalog::DECISION_REVERSED => self.decision_reversed(ev),
            // §7.4 and 禁止 6: a proposal on its own never becomes an Active
            // decision. It stays in the log, in Recent Changes and in the Inbox;
            // DECISION_CONFIRMED is what makes it a fact.
            event_catalog::DECISION_PROPOSED => Ok(()),

            event_catalog::ARTIFACT_CREATED => self.artifact_upsert(ev, false),
            event_catalog::ARTIFACT_CHANGED => self.artifact_upsert(ev, true),
            event_catalog::ARTIFACT_DELETED => self.artifact_deleted(ev),
            event_catalog::FILE_CHANGED => self.file_changed(ev),
            event_catalog::GIT_COMMIT => self.git_commit(ev),
            // §19/§39: a test or build result is evidence about its subject. A
            // completion confirmation may cite it; it never completes work itself,
            // so the reducer changes no state for it.
            event_catalog::TEST_PASSED
            | event_catalog::TEST_FAILED
            | event_catalog::BUILD_PASSED
            | event_catalog::BUILD_FAILED
            | event_catalog::PROJECT_CREATED
            // Charter, scope and project identity live in the versioned baseline
            // tables (禁止 4: one state source), so these are recorded, not folded.
            | event_catalog::CHARTER_CREATED
            | event_catalog::CHARTER_UPDATED
            | event_catalog::CHARTER_CHANGE_PROPOSED
            | event_catalog::SCOPE_ADDED
            | event_catalog::SCOPE_REMOVED => Ok(()),

            event_catalog::MILESTONE_CREATED => self.milestone_created(ev),
            event_catalog::MILESTONE_REACHED => self.milestone_reached(ev),

            event_catalog::CHANGE_PROPOSAL_CREATED => self.proposal_created(ev),
            event_catalog::CHANGE_PROPOSAL_ACCEPTED => {
                self.proposal_status(ev, ChangeProposalStatus::Accepted)
            }
            event_catalog::CHANGE_PROPOSAL_REJECTED => {
                self.proposal_status(ev, ChangeProposalStatus::Rejected)
            }

            event_catalog::AEP_CREATED => self.aep_created(ev),
            event_catalog::AEP_TRIGGER_DETECTED => self.aep_status(ev, AepStatus::Triggered, true),
            event_catalog::AEP_STARTED => self.aep_status(ev, AepStatus::InProgress, false),
            event_catalog::AEP_COMPLETED => self.aep_status(ev, AepStatus::Completed, false),

            event_catalog::DRIFT_DETECTED => self.drift_detected(ev),
            event_catalog::DRIFT_DISMISSED => self.drift_status(ev, DriftStatus::Dismissed),
            event_catalog::DRIFT_ACCEPTED => self.drift_status(ev, DriftStatus::AcceptedChange),
            event_catalog::SCOPE_CONFLICT_DETECTED => self.scope_conflict(ev),

            _ => Ok(()),
        }
    }

    // ---------------------------------------------------------------- tasks §19

    fn task_created(&mut self, ev: &ProjectEvent) -> Result<()> {
        let id = resolve_id(ev, &["taskId", "task_id", "id"]);
        let title = field_title(ev);
        let detail = text(&ev.payload, &["detail", "description", "body"]);
        let time = event_time(ev);
        self.task(ev, &id, |task, fresh| {
            task.title = title;
            if fresh {
                task.created_by_event = Some(ev.id.clone());
            }
            if detail.is_some() {
                task.detail = detail;
            }
            task.updated_at = Some(time);
        })
    }

    /// A later accepted event wins: the log is the only truth, so no event is
    /// second-guessed by comparing it with the row it meets.
    fn task_status(&mut self, ev: &ProjectEvent, status: TaskStatus) -> Result<()> {
        let id = resolve_id(ev, &["taskId", "task_id", "id"]);
        let title = field_title(ev);
        let detail = text(&ev.payload, &["detail", "description", "reason"]);
        let time = event_time(ev);
        self.task(ev, &id, |task, fresh| {
            if fresh {
                task.title = title;
                task.created_by_event = Some(ev.id.clone());
            }
            if detail.is_some() {
                task.detail = detail;
            }
            task.status = status;
            task.updated_at = Some(time);
        })
    }

    fn task_blocked(&mut self, ev: &ProjectEvent) -> Result<()> {
        let id = resolve_id(ev, &["taskId", "task_id", "id"]);
        let reason = text(
            &ev.payload,
            &["reason", "blockedBy", "blocked_by", "detail", "description"],
        );
        let title = field_title(ev);
        let time = event_time(ev);
        self.task(ev, &id, |task, fresh| {
            if fresh {
                task.title = title;
                task.created_by_event = Some(ev.id.clone());
            }
            if reason.is_some() {
                task.detail = reason;
            }
            task.status = TaskStatus::Blocked;
            task.updated_at = Some(time);
        })
    }

    /// §19: an agent saying "done" is a claim. Status does not move.
    fn task_claimed(&mut self, ev: &ProjectEvent) -> Result<()> {
        let id = resolve_id(ev, &["taskId", "task_id", "id"]);
        let title = field_title(ev);
        let claim = excerpt(ev, 200);
        let time = event_time(ev);
        self.task(ev, &id, |task, fresh| {
            if fresh {
                task.title = title;
                task.created_by_event = Some(ev.id.clone());
            }
            task.unverified_claim = true;
            task.detail = Some(claim);
            task.updated_at = Some(time);
        })
    }

    fn task_confirmed(&mut self, ev: &ProjectEvent) -> Result<()> {
        let id = resolve_id(ev, &["taskId", "task_id", "id"]);
        let title = field_title(ev);
        let time = event_time(ev);
        self.task(ev, &id, |task, fresh| {
            if fresh {
                task.title = title;
                task.created_by_event = Some(ev.id.clone());
            }
            task.status = TaskStatus::Done;
            task.unverified_claim = false;
            task.updated_at = Some(time);
        })
    }

    // ---------------------------------------------------------------- issues §21

    fn issue_found(&mut self, ev: &ProjectEvent) -> Result<()> {
        let id = resolve_id(ev, &["issueId", "issue_id", "id"]);
        let title = field_title(ev);
        let detail = text(&ev.payload, &["detail", "description", "excerpt", "content"]);
        let time = event_time(ev);
        self.issue(ev, &id, |issue, fresh| {
            issue.title = title;
            if detail.is_some() {
                issue.detail = detail;
            }
            if fresh {
                issue.status = IssueStatus::Open;
            }
            issue.updated_at = Some(time);
        })
    }

    fn issue_resolved(&mut self, ev: &ProjectEvent) -> Result<()> {
        let id = resolve_id(ev, &["issueId", "issue_id", "id"]);
        let title = field_title(ev);
        let time = event_time(ev);
        self.issue(ev, &id, |issue, fresh| {
            if fresh {
                issue.title = title;
            }
            issue.status = IssueStatus::Resolved;
            issue.updated_at = Some(time);
        })
    }

    // ------------------------------------------------------------ decisions §20

    fn decision_confirmed(&mut self, ev: &ProjectEvent) -> Result<()> {
        // Some producers only emit a confirm that names what it replaces.
        if let Some(old) = supersedes_target(ev) {
            self.mark_decision(ev, &old, DecisionStatus::Superseded)?;
        }
        let id = resolve_id(ev, &["decisionId", "decision_id", "id"]);
        self.confirm_decision(ev, &id)
    }

    fn confirm_decision(&mut self, ev: &ProjectEvent, id: &str) -> Result<()> {
        let time = event_time(ev);
        let user_authored = ev.actor_type == ActorType::User;
        self.decision(ev, id, |decision, fresh| {
            // A locked decision is user property: an agent may add evidence to it
            // but may not rewrite or re-open it (§7.4).
            if decision.locked && !user_authored && !fresh {
                decision.status = DecisionStatus::Active;
                return;
            }
            if let Some(title) = text(&ev.payload, &["title", "name", "summary"]) {
                decision.title = title;
            } else if fresh {
                decision.title = excerpt(ev, 120);
            }
            if let Some(body) = text(&ev.payload, &["decision", "statement", "text", "content"]) {
                decision.decision = body;
            }
            if let Some(rationale) = text(&ev.payload, &["rationale", "why", "reason"]) {
                decision.rationale = Some(rationale);
            }
            let alternatives = text_list(&ev.payload, &["alternatives"]);
            if !alternatives.is_empty() {
                decision.alternatives = alternatives;
            }
            if user_authored {
                // A confirmation the user made is their own judgement, so it locks on
                // arrival — that is the only thing that keeps a later agent claim from
                // rewriting it (§7.4). An explicit `locked: false` is the sole way to
                // confirm without binding; before this, the flag was never raised and
                // the guard above could not engage.
                decision.locked = boolean(&ev.payload, &["locked", "lock"]).unwrap_or(true);
            }
            if fresh || decision.decided_at.is_none() {
                decision.decided_at = Some(time);
            }
            decision.status = DecisionStatus::Active;
        })
    }

    fn mark_decision(
        &mut self,
        ev: &ProjectEvent,
        id: &str,
        status: DecisionStatus,
    ) -> Result<()> {
        // History is never invented or erased: superseding an id that has no
        // decision row leaves the row absent rather than creating a stub.
        if !self.state.decisions.iter().any(|d| d.id == id) {
            return Ok(());
        }
        let reason = text(&ev.payload, &["reason", "rationale", "why"]);
        let word = status_word(status);
        self.decision(ev, id, |decision, _| {
            if let Some(reason) = reason.clone() {
                decision.rationale = Some(match decision.rationale.as_deref() {
                    Some(previous) if !previous.is_empty() => {
                        format!("{previous} | {word}: {reason}")
                    }
                    _ => format!("{word}: {reason}"),
                });
            }
            decision.status = status;
        })
    }

    /// §20: the old decision is marked, the new one is recorded, both stay listed.
    fn decision_superseded(&mut self, ev: &ProjectEvent) -> Result<()> {
        let subject = ev
            .subject_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let explicit_new = text(
            &ev.payload,
            &[
                "decisionId",
                "newDecisionId",
                "new_decision_id",
                "newId",
                "supersededBy",
            ],
        );
        let target = supersedes_target(ev).or_else(|| match (&subject, &explicit_new) {
            // The payload names the survivor, so the subject must be the old one.
            (Some(s), Some(n)) if s != n => Some(s.clone()),
            _ => None,
        });
        // `id` is only a last resort for the survivor: it doubles as the target
        // fallback, and reading it first would collapse both onto one row.
        let new_id = explicit_new
            .or_else(|| match (&subject, &target) {
                (Some(s), Some(t)) if s != t => Some(s.clone()),
                (Some(s), None) => Some(s.clone()),
                _ => None,
            })
            .or_else(|| text(&ev.payload, &["id"]))
            .unwrap_or_else(|| ev.id.clone());

        if let Some(old) = &target {
            self.mark_decision(ev, old, DecisionStatus::Superseded)?;
        }
        if target.as_deref() != Some(new_id.as_str()) {
            self.confirm_decision(ev, &new_id)?;
            if let Some(row) = self
                .state
                .decisions
                .iter_mut()
                .find(|d| d.id == new_id && d.supersedes.is_none())
            {
                row.supersedes = target;
            }
        }
        Ok(())
    }

    fn decision_reversed(&mut self, ev: &ProjectEvent) -> Result<()> {
        let id = supersedes_target(ev)
            .unwrap_or_else(|| resolve_id(ev, &["decisionId", "decision_id", "id"]));
        if self.state.decisions.iter().any(|d| d.id == id) {
            return self.mark_decision(ev, &id, DecisionStatus::Reversed);
        }
        // Nothing was ever active under that id: keep the reversal as its own row
        // so the record shows the attempt without presenting it as Active.
        self.confirm_decision(ev, &id)?;
        self.mark_decision(ev, &id, DecisionStatus::Reversed)
    }

    // ------------------------------------------------------------ artifacts §22

    fn artifact_upsert(&mut self, ev: &ProjectEvent, bump: bool) -> Result<()> {
        let path = artifact_path(ev);
        let requested = resolve_id(ev, &["artifactId", "artifact_id", "id", "hash"]);
        // The store keeps one artifact per path, so a second id for a known path
        // has to fold onto the existing row instead of creating a twin.
        let id = path
            .as_deref()
            .and_then(|p| {
                self.state
                    .artifacts
                    .iter()
                    .find(|a| a.path.as_deref() == Some(p))
                    .map(|a| a.id.clone())
            })
            .unwrap_or(requested);
        let name = artifact_name(ev, path.as_deref());
        let kind = artifact_kind(ev, path.as_deref());
        let hash = text(&ev.payload, &["hash", "contentHash", "content_hash"]);
        let relationships = text_list(&ev.payload, &["relationships", "dependsOn", "depends_on"]);
        let explicit_version = number(&ev.payload, &["version"]);
        let redesign = boolean(&ev.payload, &["redesign", "redesigned"]).unwrap_or(false);
        let time = event_time(ev);
        self.artifact(ev, &id, |artifact, fresh| {
            if fresh {
                artifact.name = name;
                artifact.kind = kind;
            }
            if path.is_some() {
                artifact.path = path;
            }
            if hash.is_some() {
                artifact.hash = hash;
            }
            if !relationships.is_empty() {
                artifact.relationships = relationships;
            }
            artifact.version = match explicit_version {
                Some(value) if value > 0 => value as u32,
                _ => artifact.version + u32::from(bump || fresh),
            };
            if redesign {
                artifact.redesign_count += 1;
            }
            artifact.deleted = false;
            artifact.updated_at = Some(time);
        })
    }

    fn artifact_deleted(&mut self, ev: &ProjectEvent) -> Result<()> {
        let path = artifact_path(ev);
        let requested = resolve_id(ev, &["artifactId", "artifact_id", "id"]);
        let index = self
            .state
            .artifacts
            .iter()
            .position(|a| a.id == requested)
            .or_else(|| {
                path.as_deref()
                    .and_then(|p| self.state.artifacts.iter().position(|a| a.path.as_deref() == Some(p)))
            });
        // Deleting something never seen is a no-op, not a phantom row.
        let Some(index) = index else { return Ok(()) };
        let id = self.state.artifacts[index].id.clone();
        let time = event_time(ev);
        self.artifact(ev, &id, |artifact, _| {
            artifact.deleted = true;
            artifact.updated_at = Some(time);
        })
    }

    /// §39: a file change is evidence about an artifact, never proof of a feature.
    fn file_changed(&mut self, ev: &ProjectEvent) -> Result<()> {
        if artifact_path(ev).is_none() {
            return Ok(());
        }
        self.artifact_upsert(ev, true)
    }

    fn git_commit(&mut self, ev: &ProjectEvent) -> Result<()> {
        let message = text(
            &ev.payload,
            &["message", "commitMessage", "commit_message", "subject", "title"],
        );
        let files = text_list(&ev.payload, &["files", "changedFiles", "changed_files", "paths"]);
        for file in files {
            let mut event = ev.clone();
            event.subject_type = Some("artifact".into());
            event.subject_id = Some(file.clone());
            event.payload = serde_json::json!({ "path": file, "message": message });
            self.artifact_upsert(&event, true)?;
        }
        Ok(())
    }

    // ----------------------------------------------------------- milestones §24

    fn milestone_created(&mut self, ev: &ProjectEvent) -> Result<()> {
        let id = resolve_id(ev, &["milestoneId", "milestone_id", "id"]);
        let title = field_title(ev);
        self.milestone(ev, &id, |milestone, _| {
            milestone.title = title;
        })
    }

    fn milestone_reached(&mut self, ev: &ProjectEvent) -> Result<()> {
        let id = resolve_id(ev, &["milestoneId", "milestone_id", "id"]);
        let title = field_title(ev);
        let time = event_time(ev);
        self.milestone(ev, &id, |milestone, fresh| {
            if fresh {
                milestone.title = title;
            }
            milestone.reached = true;
            milestone.reached_at = Some(time);
        })
    }

    // ------------------------------------------------------ change proposals §23

    fn proposal_created(&mut self, ev: &ProjectEvent) -> Result<()> {
        let id = resolve_id(
            ev,
            &["proposalId", "proposal_id", "changeProposalId", "change_proposal_id", "id"],
        );
        let proposed_change = text(
            &ev.payload,
            &[
                "proposedChange",
                "proposed_change",
                "change",
                "title",
                "summary",
                "name",
            ],
        )
        .unwrap_or_else(|| excerpt(ev, 160));
        let reason = text(&ev.payload, &["reason", "why", "detail", "description"]);
        let affected_scope = text_list(&ev.payload, &["affectedScope", "affected_scope"]);
        let affected_architecture =
            text_list(&ev.payload, &["affectedArchitecture", "affected_architecture"]);
        let benefit = text(&ev.payload, &["benefit"]);
        let cost = text(&ev.payload, &["cost"]);
        let risk = text(&ev.payload, &["risk"]);
        let conflict = boolean(&ev.payload, &["charterConflict", "charter_conflict"]);
        let delta = ev.payload.get("delta").cloned();
        let time = event_time(ev);
        self.proposal(ev, &id, |proposal, fresh| {
            proposal.proposed_change = proposed_change;
            if reason.is_some() {
                proposal.reason = reason.unwrap_or_default();
            }
            proposal.affected_scope = affected_scope;
            proposal.affected_architecture = affected_architecture;
            proposal.benefit = benefit.or_else(|| proposal.benefit.clone());
            proposal.cost = cost.or_else(|| proposal.cost.clone());
            proposal.risk = risk.or_else(|| proposal.risk.clone());
            proposal.charter_conflict = conflict.unwrap_or(proposal.charter_conflict);
            proposal.delta = delta.or_else(|| proposal.delta.clone());
            if fresh {
                proposal.created_at = Some(time);
            }
            proposal.status = ChangeProposalStatus::Open;
        })
    }

    fn proposal_status(&mut self, ev: &ProjectEvent, status: ChangeProposalStatus) -> Result<()> {
        let id = resolve_id(
            ev,
            &["proposalId", "proposal_id", "changeProposalId", "change_proposal_id", "id"],
        );
        // Accepting a proposal moves a baseline only through ProjectService, which
        // writes the versioned table; the reducer must not mirror that (§9, 禁止 5).
        if !self.state.change_proposals.iter().any(|p| p.id == id) {
            return Ok(());
        }
        self.proposal(ev, &id, |proposal, _| proposal.status = status)
    }

    // ------------------------------------------------------------------- AEP §24

    fn aep_created(&mut self, ev: &ProjectEvent) -> Result<()> {
        let id = resolve_id(ev, &["aepId", "aep_id", "id"]);
        // A whole AEP may arrive as a payload object; scalar fields still override it.
        let whole = ev
            .payload
            .get("aep")
            .and_then(|v| serde_json::from_value::<ArchitectureEvolutionPlan>(v.clone()).ok());
        let title = text(&ev.payload, &["title", "name"]);
        let code = text(&ev.payload, &["code", "aepCode", "aep_code"]);
        let current = text(&ev.payload, &["current", "currentState", "current_state"]);
        let limitation = text(&ev.payload, &["limitation", "problem"]);
        let why_not_now = text(&ev.payload, &["whyNotNow", "why_not_now"]);
        let target = text(&ev.payload, &["target", "targetState", "target_state"]);
        let trigger = text_list(&ev.payload, &["trigger", "triggers"]);
        let change_surface = text_list(&ev.payload, &["changeSurface", "change_surface"]);
        let preserved_core = text_list(&ev.payload, &["preservedCore", "preserved_core"]);
        let benefit = text(&ev.payload, &["benefit"]);
        let cost = text(&ev.payload, &["cost"]);
        let status = text(&ev.payload, &["status"])
            .and_then(|v| serde_json::from_value::<AepStatus>(Value::String(v)).ok());

        self.aep(ev, &id, |aep, fresh| {
            if let Some(parsed) = whole.clone() {
                *aep = ArchitectureEvolutionPlan {
                    id: aep.id.clone(),
                    project_id: aep.project_id.clone(),
                    ..parsed
                };
            }
            if fresh && aep.title.is_empty() {
                aep.title = title.clone().unwrap_or_else(|| excerpt(ev, 120));
            } else if title.is_some() {
                aep.title = title.clone().unwrap_or_default();
            }
            if aep.code.is_empty() {
                aep.code = code.clone().unwrap_or_else(|| aep.id.clone());
            } else if code.is_some() {
                aep.code = code.clone().unwrap_or_default();
            }
            assign(&mut aep.current, current.clone());
            assign(&mut aep.limitation, limitation.clone());
            assign(&mut aep.why_not_now, why_not_now.clone());
            assign(&mut aep.target, target.clone());
            if !trigger.is_empty() {
                aep.trigger = trigger.clone();
            }
            if !change_surface.is_empty() {
                aep.change_surface = change_surface.clone();
            }
            if !preserved_core.is_empty() {
                aep.preserved_core = preserved_core.clone();
            }
            if benefit.is_some() {
                aep.benefit = benefit.clone();
            }
            if cost.is_some() {
                aep.cost = cost.clone();
            }
            if let Some(value) = status {
                aep.status = value;
            }
        })
    }

    fn aep_status(&mut self, ev: &ProjectEvent, status: AepStatus, only_if_waiting: bool) -> Result<()> {
        let id = resolve_id(ev, &["aepId", "aep_id", "id"]);
        let Some(current) = self.state.aeps.iter().find(|a| a.id == id).map(|a| a.status) else {
            return Ok(());
        };
        // A trigger only wakes a waiting plan; an in-flight or finished evolution
        // must not be pulled backwards by a late signal.
        if only_if_waiting && !matches!(current, AepStatus::Dormant | AepStatus::Watching) {
            return Ok(());
        }
        self.aep(ev, &id, |aep, _| aep.status = status)
    }

    // ------------------------------------------------------------------ drift §25

    fn drift_detected(&mut self, ev: &ProjectEvent) -> Result<()> {
        let id = resolve_id(ev, &["driftId", "drift_id", "alertId", "alert_id", "id"]);
        let r#type = drift_value::<DriftType>(&ev.payload, &["type", "driftType", "drift_type"]);
        let severity = drift_value::<DriftSeverity>(&ev.payload, &["severity"]);
        let summary = text(&ev.payload, &["summary", "title", "message"])
            .unwrap_or_else(|| excerpt(ev, 160));
        let detail = text(&ev.payload, &["detail", "reason", "why", "description"]);
        let baseline_refs = text_list(&ev.payload, &["baselineRefs", "baseline_refs"]);
        let time = event_time(ev);
        self.drift(ev, &id, |alert, fresh| {
            if fresh {
                alert.detected_at = Some(time);
            }
            if r#type.is_some() {
                alert.r#type = r#type.unwrap_or(alert.r#type);
            }
            if severity.is_some() {
                alert.severity = severity.unwrap_or(alert.severity);
            }
            alert.summary = summary;
            if detail.is_some() {
                alert.detail = detail;
            }
            if !baseline_refs.is_empty() {
                alert.baseline_refs = baseline_refs;
            }
            alert.status = DriftStatus::Open;
        })
    }

    fn drift_status(&mut self, ev: &ProjectEvent, status: DriftStatus) -> Result<()> {
        let id = resolve_id(ev, &["driftId", "drift_id", "alertId", "alert_id", "id"]);
        if !self.state.drift_alerts.iter().any(|a| a.id == id) {
            return Ok(());
        }
        self.drift(ev, &id, |alert, _| alert.status = status)
    }

    fn scope_conflict(&mut self, ev: &ProjectEvent) -> Result<()> {
        let mut event = ev.clone();
        event.r#type = event_catalog::DRIFT_DETECTED.into();
        event.payload = serde_json::json!({
            "summary": text(&ev.payload, &["summary", "title", "conflict", "detail"])
                .unwrap_or_else(|| excerpt(ev, 160)),
            "type": "scope_drift",
            "severity": "high",
        });
        self.drift_detected(&event)
    }

    // ------------------------------------------------------------------ plumbing

    /// Stage the cache write for a row this event touched, and let its evidence
    /// vouch for that row (§7.3).
    fn record(
        &mut self,
        ev: &ProjectEvent,
        before: Option<Projected>,
        after: Projected,
    ) -> Result<()> {
        let kind = after.kind();
        let id = after.id().to_string();
        let status = after.status();
        let moved = before
            .as_ref()
            .map(|previous| previous.status() != status)
            .unwrap_or(true);
        let version = if moved {
            let key = format!("{}:{id}", kind.as_str());
            let number = self.versions.get(&key).copied().unwrap_or(0) + 1;
            self.versions.insert(key, number);
            Some(VersionWrite {
                number,
                before: before
                    .clone()
                    .map(|previous| previous.json())
                    .transpose()?
                    .unwrap_or(Value::Null),
                after: after.json()?,
                reason: ev.r#type.clone(),
            })
        } else {
            None
        };
        self.writes.push(EntityWrite {
            kind,
            id: id.clone(),
            status,
            entity: after.clone(),
            version,
        });
        if !ev.evidence_refs.is_empty() {
            let r#type = evidence_type_for(ev);
            let summary = evidence_summary(ev);
            for reference in &ev.evidence_refs {
                self.attach(ev, reference, &r#type, &summary, &[(kind, id.clone())]);
            }
        }
        Ok(())
    }

    fn trace_evidence(&mut self, ev: &ProjectEvent) {
        if ev.evidence_refs.is_empty() {
            return;
        }
        let subjects = subject_of(ev);
        if subjects.is_empty() {
            // Still keep the evidence itself reachable from the project.
            let r#type = evidence_type_for(ev);
            let summary = evidence_summary(ev);
            for reference in &ev.evidence_refs {
                self.attach(ev, reference, &r#type, &summary, &[]);
            }
            return;
        }
        let r#type = evidence_type_for(ev);
        let summary = evidence_summary(ev);
        for reference in &ev.evidence_refs {
            self.attach(ev, reference, &r#type, &summary, &subjects);
        }
    }

    /// Evidence identity comes from the reference and the event clock, never from
    /// a wall clock, so replay yields the same rows and the same hash.
    fn attach(
        &mut self,
        ev: &ProjectEvent,
        reference: &str,
        r#type: &EvidenceType,
        summary: &str,
        subjects: &[(EntityKind, String)],
    ) {
        if reference.trim().is_empty() {
            return;
        }
        let id = evidence_id(&self.project_id, r#type, reference);
        if let Some(existing) = self.evidence.iter_mut().find(|row| row.evidence.id == id) {
            for subject in subjects {
                if !existing.subjects.contains(subject) {
                    existing.subjects.push(subject.clone());
                }
            }
            return;
        }
        let evidence = Evidence {
            id,
            project_id: self.project_id.clone(),
            r#type: *r#type,
            ref_id: Some(reference.to_string()),
            summary: Some(summary.to_string()),
            created_at: event_time(ev),
        };
        self.state.evidence.push(evidence.clone());
        self.evidence.push(EvidenceWrite {
            evidence,
            subjects: subjects.to_vec(),
        });
    }
}

fn status_word(status: DecisionStatus) -> &'static str {
    match status {
        DecisionStatus::Active => "confirmed",
        DecisionStatus::Superseded => "superseded",
        DecisionStatus::Reversed => "reversed",
    }
}

/// `Some(value)` overwrites, `None` keeps what the row already held.
fn assign(slot: &mut String, value: Option<String>) {
    if let Some(value) = value {
        *slot = value;
    }
}

fn field_title(ev: &ProjectEvent) -> String {
    text(
        &ev.payload,
        &["title", "name", "summary", "task", "issue", "milestone"],
    )
    .unwrap_or_else(|| excerpt(ev, 120))
}

fn artifact_path(ev: &ProjectEvent) -> Option<String> {
    text(&ev.payload, &["path", "filePath", "file_path", "file"])
        .or_else(|| matches!(ev.subject_type.as_deref(), Some("artifact")).then(|| ev.subject_id.clone()).flatten())
}

fn artifact_name(ev: &ProjectEvent, path: Option<&str>) -> String {
    text(&ev.payload, &["name", "title"])
        .or_else(|| {
            path.map(|p| {
                p.rsplit('/')
                    .next()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(p)
                    .to_string()
            })
        })
        .unwrap_or_else(|| excerpt(ev, 80))
}

fn artifact_kind(ev: &ProjectEvent, path: Option<&str>) -> ArtifactKind {
    if let Some(kind) = text(&ev.payload, &["kind", "artifactType", "artifact_type"])
        .and_then(|k| serde_json::from_value::<ArtifactKind>(Value::String(k)).ok())
    {
        return kind;
    }
    let name = path.unwrap_or_default();
    if name.ends_with(".md") || name.ends_with(".txt") {
        ArtifactKind::Document
    } else if name.ends_with(".toml") || name.ends_with(".json") || name.ends_with(".yaml") {
        ArtifactKind::Config
    } else if name.contains("test") {
        ArtifactKind::Test
    } else if name.ends_with(".sh") {
        ArtifactKind::Script
    } else if name.ends_with(".sql") || name.ends_with(".db") {
        ArtifactKind::Database
    } else {
        ArtifactKind::SourceFile
    }
}

fn supersedes_target(ev: &ProjectEvent) -> Option<String> {
    text(
        &ev.payload,
        &[
            "supersedes",
            "supersededId",
            "superseded_id",
            "oldDecisionId",
            "old_decision_id",
            "targetId",
            "target_id",
        ],
    )
}

fn drift_value<T>(payload: &Value, keys: &[&str]) -> Option<T>
where
    T: serde::de::DeserializeOwned,
{
    text(payload, keys).and_then(|v| serde_json::from_value::<T>(Value::String(v)).ok())
}

/// The entity an event argues about. Non-entity subjects (charter, scope, project)
/// intentionally produce no link: those facts live in the versioned tables.
fn subject_of(ev: &ProjectEvent) -> Vec<(EntityKind, String)> {
    let Some(id) = ev
        .subject_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Vec::new();
    };
    let named = ev.subject_type.as_deref().unwrap_or("");
    let kind = match named {
        "task" => EntityKind::Task,
        "issue" => EntityKind::Issue,
        "decision" => EntityKind::Decision,
        "artifact" => EntityKind::Artifact,
        "milestone" => EntityKind::Milestone,
        "change_proposal" | "proposal" => EntityKind::ChangeProposal,
        "aep" | "plan" => EntityKind::Aep,
        "drift_alert" | "drift" | "alert" => EntityKind::DriftAlert,
        "evidence" => EntityKind::Evidence,
        // Unnamed subject: only trust it when the event type pins the kind.
        _ if !named.is_empty() => return Vec::new(),
        _ => match default_subject(&ev.r#type) {
            Some(kind) => kind,
            None => return Vec::new(),
        },
    };
    vec![(kind, id.to_string())]
}

fn default_subject(event_type: &str) -> Option<EntityKind> {
    Some(match event_type {
        event_catalog::TASK_CREATED
        | event_catalog::TASK_STARTED
        | event_catalog::TASK_BLOCKED
        | event_catalog::TASK_COMPLETED_CLAIM
        | event_catalog::TASK_COMPLETED_CONFIRMED
        | event_catalog::TASK_CANCELLED => EntityKind::Task,
        event_catalog::ISSUE_FOUND | event_catalog::ISSUE_RESOLVED => EntityKind::Issue,
        event_catalog::DECISION_CONFIRMED
        | event_catalog::DECISION_SUPERSEDED
        | event_catalog::DECISION_REVERSED => EntityKind::Decision,
        event_catalog::ARTIFACT_CREATED
        | event_catalog::ARTIFACT_CHANGED
        | event_catalog::ARTIFACT_DELETED => EntityKind::Artifact,
        _ => return None,
    })
}

fn evidence_type_for(ev: &ProjectEvent) -> EvidenceType {
    match ev.r#type.as_str() {
        event_catalog::GIT_COMMIT => EvidenceType::Git,
        event_catalog::TEST_PASSED | event_catalog::TEST_FAILED => EvidenceType::Test,
        event_catalog::BUILD_PASSED | event_catalog::BUILD_FAILED => EvidenceType::Build,
        event_catalog::FILE_CHANGED
        | event_catalog::ARTIFACT_CREATED
        | event_catalog::ARTIFACT_CHANGED
        | event_catalog::ARTIFACT_DELETED => EvidenceType::File,
        _ if ev.message_id.is_some() => EvidenceType::Message,
        _ if ev.actor_type == ActorType::System => EvidenceType::System,
        _ => EvidenceType::Manual,
    }
}

fn evidence_summary(ev: &ProjectEvent) -> String {
    clip(&format!("{} {}", ev.r#type, excerpt(ev, 80)), 120)
}

/// Evidence identity is content-derived so replaying a log is idempotent.
pub(crate) fn evidence_id(project_id: &str, r#type: &EvidenceType, reference: &str) -> String {
    vibe_domain::content_hash(&format!(
        "{project_id}|{}|{reference}",
        r#type.as_str()
    ))
}
