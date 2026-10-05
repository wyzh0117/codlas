//! Project + baseline use-cases. The rules that matter here are the fidelity
//! rules: a non-user actor may never overwrite a locked charter, scope or
//! architecture baseline — it can only open a Change Proposal
//! (V1_DEVELOPMENT.md §9, PROJECT_MASTER §19 禁止 5).

use vibe_domain::{
    event_catalog, ActorType, ArchitectureBaseline, ArchitectureInvariant, ArchitectureLayer,
    ChangeProposal, ChangeProposalStatus, Project, ProjectCharter, ProjectStatus, ScopeBoundary,
    ScopeSection,
};

use crate::error::{CoreError, Result};
use crate::event_store::{EventDraft, EventStore};
use crate::ports::{EntityStore, Repositories};

#[derive(Debug, Clone, Default)]
pub struct CharterDraft {
    pub original_problem: String,
    pub target_user: Option<String>,
    pub core_value: String,
    pub must_have: Vec<String>,
    pub wont_have: Vec<String>,
    pub success_criteria: Vec<String>,
    pub architecture_principles: Vec<String>,
    pub constraints: Vec<String>,
    /// Only meaningful for a user-authored write; agents cannot lock or unlock.
    pub locked: bool,
}

#[derive(Debug, Clone, Default)]
pub struct BaselineDraft {
    pub summary: String,
    pub layers: Vec<ArchitectureLayer>,
    pub invariants: Vec<ArchitectureInvariant>,
    pub replaceable_parts: Vec<String>,
}

pub struct ProjectService {
    repos: Repositories,
    events: EventStore,
}

impl ProjectService {
    pub fn new(repos: Repositories) -> Self {
        let events = EventStore::new(repos.clone());
        Self { repos, events }
    }

    pub fn events(&self) -> &EventStore {
        &self.events
    }

    pub fn create_project(
        &self,
        name: &str,
        description: Option<String>,
        root_path: Option<String>,
        phase: Option<String>,
    ) -> Result<Project> {
        if name.trim().is_empty() {
            return Err(CoreError::Validation("project name is required".into()));
        }
        let now = vibe_domain::now();
        let project = Project {
            id: vibe_domain::new_id(),
            name: name.trim().to_string(),
            description,
            root_path,
            status: ProjectStatus::Active,
            current_phase: phase,
            created_at: now,
            updated_at: now,
        };
        self.repos.projects.create_project(&project)?;
        self.events.append(
            EventDraft::new(project.id.clone(), event_catalog::PROJECT_CREATED)
                .user()
                .payload(serde_json::json!({ "name": project.name })),
        )?;
        Ok(project)
    }

    pub fn project(&self, id: &str) -> Result<Project> {
        self.repos
            .projects
            .get_project(id)?
            .ok_or_else(|| CoreError::NotFound {
                what: format!("project {id}"),
            })
    }

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        self.repos.projects.list_projects()
    }

    pub fn update_phase(&self, project_id: &str, phase: &str) -> Result<Project> {
        let mut project = self.project(project_id)?;
        project.current_phase = Some(phase.to_string());
        project.updated_at = vibe_domain::now();
        self.repos.projects.update_project(&project)?;
        Ok(project)
    }

    pub fn charter(&self, project_id: &str) -> Result<Option<ProjectCharter>> {
        self.repos.projects.get_active_charter(project_id)
    }

    pub fn save_charter(
        &self,
        project_id: &str,
        draft: CharterDraft,
        actor: ActorType,
    ) -> Result<ProjectCharter> {
        let previous = self.charter(project_id)?;
        if let Some(prev) = &previous {
            if prev.locked && actor != ActorType::User {
                return Err(CoreError::Locked(format!(
                    "charter v{} is user-locked; open a change proposal instead of editing it",
                    prev.version
                )));
            }
        }
        if draft.original_problem.trim().is_empty() || draft.core_value.trim().is_empty() {
            return Err(CoreError::Validation(
                "charter needs originalProblem and coreValue".into(),
            ));
        }
        let now = vibe_domain::now();
        let version = previous.as_ref().map(|p| p.version + 1).unwrap_or(1);
        let created_at = previous
            .as_ref()
            .map(|p| p.created_at)
            .unwrap_or(now);
        let charter = ProjectCharter {
            id: vibe_domain::new_id(),
            project_id: project_id.to_string(),
            original_problem: draft.original_problem,
            target_user: draft.target_user,
            core_value: draft.core_value,
            must_have: draft.must_have,
            wont_have: draft.wont_have,
            success_criteria: draft.success_criteria,
            architecture_principles: draft.architecture_principles,
            constraints: draft.constraints,
            version,
            locked: if actor == ActorType::User {
                draft.locked
            } else {
                previous.as_ref().map(|p| p.locked).unwrap_or(false)
            },
            created_at,
            updated_at: now,
        };
        self.repos.projects.insert_charter_version(&charter)?;
        let event_type = if version == 1 {
            event_catalog::CHARTER_CREATED
        } else {
            event_catalog::CHARTER_UPDATED
        };
        self.events.append(
            EventDraft::new(project_id.to_string(), event_type)
                .actor(actor)
                .subject("charter", charter.id.clone())
                .payload(serde_json::json!({ "version": charter.version })),
        )?;
        self.index_charter(&charter)?;
        Ok(charter)
    }

    pub fn lock_charter(&self, project_id: &str, locked: bool) -> Result<ProjectCharter> {
        let current = self.charter(project_id)?.ok_or_else(|| CoreError::NotFound {
            what: "charter".into(),
        })?;
        let mut next = current.clone();
        next.id = vibe_domain::new_id();
        next.version = current.version + 1;
        next.locked = locked;
        next.updated_at = vibe_domain::now();
        self.repos.projects.insert_charter_version(&next)?;
        Ok(next)
    }

    pub fn scope(&self, project_id: &str) -> Result<Option<ScopeBoundary>> {
        self.repos.projects.get_active_scope(project_id)
    }

    /// Scope moves are user actions. An agent that wants different scope files a
    /// change proposal, which a user then accepts.
    pub fn update_scope(
        &self,
        project_id: &str,
        add: Vec<(ScopeSection, String)>,
        remove: Vec<(ScopeSection, String)>,
        actor: ActorType,
    ) -> Result<ScopeBoundary> {
        if actor != ActorType::User {
            return Err(CoreError::Locked(
                "only the user may move the scope boundary; propose a change instead".into(),
            ));
        }
        let previous = self.repos.projects.get_active_scope(project_id)?;
        let mut lists: [Vec<String>; 4] = [
            previous.as_ref().map(|s| s.in_scope.clone()).unwrap_or_default(),
            previous
                .as_ref()
                .map(|s| s.out_of_scope.clone())
                .unwrap_or_default(),
            previous.as_ref().map(|s| s.deferred.clone()).unwrap_or_default(),
            previous.as_ref().map(|s| s.forbidden.clone()).unwrap_or_default(),
        ];

        for (section, item) in &remove {
            lists[index_of(section)].retain(|x| x != item);
        }
        for (section, item) in &add {
            // An item cannot live in two sections at once: a later move wins.
            for list in lists.iter_mut() {
                list.retain(|x| x != item);
            }
            let target = &mut lists[index_of(section)];
            if !target.iter().any(|x| x == item) {
                target.push(item.clone());
            }
        }
        let [in_scope, out_of_scope, deferred, forbidden] = lists;

        let version = previous.as_ref().map(|s| s.version + 1).unwrap_or(1);
        let scope = ScopeBoundary {
            id: vibe_domain::new_id(),
            project_id: project_id.to_string(),
            in_scope,
            out_of_scope,
            deferred,
            forbidden,
            version,
        };
        self.repos.projects.insert_scope_version(&scope)?;

        for (section, item) in add {
            self.events.append(
                EventDraft::new(project_id.to_string(), event_catalog::SCOPE_ADDED)
                    .user()
                    .subject("scope", scope.id.clone())
                    .payload(serde_json::json!({ "section": section.as_str(), "item": item })),
            )?;
        }
        for (section, item) in remove {
            self.events.append(
                EventDraft::new(project_id.to_string(), event_catalog::SCOPE_REMOVED)
                    .user()
                    .subject("scope", scope.id.clone())
                    .payload(serde_json::json!({ "section": section.as_str(), "item": item })),
            )?;
        }
        Ok(scope)
    }

    pub fn baseline(&self, project_id: &str) -> Result<Option<ArchitectureBaseline>> {
        self.repos.projects.get_active_baseline(project_id)
    }

    pub fn set_baseline(
        &self,
        project_id: &str,
        draft: BaselineDraft,
        actor: ActorType,
    ) -> Result<ArchitectureBaseline> {
        if actor != ActorType::User {
            return Err(CoreError::Locked(
                "architecture baseline is user-controlled; propose a change instead".into(),
            ));
        }
        let previous = self.repos.projects.get_active_baseline(project_id)?;
        let version = previous.as_ref().map(|b| b.version + 1).unwrap_or(1);
        let baseline = ArchitectureBaseline {
            id: vibe_domain::new_id(),
            project_id: project_id.to_string(),
            summary: draft.summary,
            layers: draft.layers,
            invariants: draft.invariants,
            replaceable_parts: draft.replaceable_parts,
            version,
            active: true,
        };
        self.repos.projects.insert_baseline_version(&baseline)?;
        Ok(baseline)
    }

    pub fn open_change_proposal(
        &self,
        project_id: &str,
        mut proposal: ChangeProposal,
        actor: ActorType,
    ) -> Result<ChangeProposal> {
        proposal.id = if proposal.id.is_empty() {
            vibe_domain::new_id()
        } else {
            proposal.id
        };
        proposal.project_id = project_id.to_string();
        proposal.status = ChangeProposalStatus::Open;
        proposal.created_at = Some(vibe_domain::now());
        proposal.charter_conflict = self.charter_conflicts(project_id, &proposal)?;

        self.repos.entities.put_entity(
            vibe_domain::EntityKind::ChangeProposal,
            &proposal,
            Some(status_str(&proposal).as_str()),
        )?;
        self.events.append(
            EventDraft::new(project_id.to_string(), event_catalog::CHANGE_PROPOSAL_CREATED)
                .actor(actor)
                .subject("change_proposal", proposal.id.clone())
                // The event is the record's origin, so it has to carry what the record holds:
                // a fold that met a thinner payload would rebuild the proposal without `delta`
                // and §7.4's acceptance would then have nothing machine-applicable to apply.
                .payload(serde_json::json!({
                    "proposalId": proposal.id,
                    "proposedChange": proposal.proposed_change,
                    "reason": proposal.reason,
                    "affectedScope": proposal.affected_scope,
                    "affectedArchitecture": proposal.affected_architecture,
                    "benefit": proposal.benefit,
                    "cost": proposal.cost,
                    "risk": proposal.risk,
                    "charterConflict": proposal.charter_conflict,
                    "delta": proposal.delta,
                })),
        )?;
        if let Some(charter) = self.charter(project_id)? {
            if proposal.charter_conflict {
                self.events.append(
                    EventDraft::new(project_id.to_string(), event_catalog::CHARTER_CHANGE_PROPOSED)
                        .actor(actor)
                        .subject("charter", charter.id.clone())
                        .payload(serde_json::json!({ "proposalId": proposal.id })),
                )?;
            }
        }
        Ok(proposal)
    }

    fn charter_conflicts(
        &self,
        project_id: &str,
        proposal: &ChangeProposal,
    ) -> Result<bool> {
        let charter = match self.charter(project_id)? {
            Some(c) => c,
            None => return Ok(false),
        };
        let text = format!(
            "{} {}",
            proposal.proposed_change,
            proposal.affected_scope.join(" ")
        )
        .to_lowercase();
        let conflicts = charter
            .wont_have
            .iter()
            .chain(charter.constraints.iter())
            .any(|item| overlap(&text, &item.to_lowercase()) >= 0.4);
        Ok(conflicts)
    }

    /// A user accepting a proposal is the only way a baseline moves without the
    /// user editing it directly.
    pub fn accept_change_proposal(
        &self,
        project_id: &str,
        proposal_id: &str,
        reason: &str,
    ) -> Result<ChangeProposal> {
        let mut proposal = self
            .repos
            .entities
            .entity::<ChangeProposal>(vibe_domain::EntityKind::ChangeProposal, proposal_id)?
            .ok_or_else(|| CoreError::NotFound {
                what: format!("change proposal {proposal_id}"),
            })?;
        proposal.status = ChangeProposalStatus::Accepted;
        self.repos.entities.put_entity(
            vibe_domain::EntityKind::ChangeProposal,
            &proposal,
            Some("accepted"),
        )?;
        self.events.append(
            EventDraft::new(project_id.to_string(), event_catalog::CHANGE_PROPOSAL_ACCEPTED)
                .user()
                .subject("change_proposal", proposal.id.clone())
                .payload(serde_json::json!({
                    "proposalId": proposal.id.clone(),
                    "proposedChange": proposal.proposed_change,
                    "status": "accepted",
                    "reason": reason.trim(),
                })),
        )?;

        if let Some(delta) = proposal.delta.clone() {
            self.apply_delta(project_id, &delta)?;
        }
        Ok(proposal)
    }

    pub fn reject_change_proposal(
        &self,
        project_id: &str,
        proposal_id: &str,
        reason: &str,
    ) -> Result<ChangeProposal> {
        let mut proposal = self
            .repos
            .entities
            .entity::<ChangeProposal>(vibe_domain::EntityKind::ChangeProposal, proposal_id)?
            .ok_or_else(|| CoreError::NotFound {
                what: format!("change proposal {proposal_id}"),
            })?;
        proposal.status = ChangeProposalStatus::Rejected;
        self.repos.entities.put_entity(
            vibe_domain::EntityKind::ChangeProposal,
            &proposal,
            Some("rejected"),
        )?;
        self.events.append(
            EventDraft::new(project_id.to_string(), event_catalog::CHANGE_PROPOSAL_REJECTED)
                .user()
                .subject("change_proposal", proposal.id.clone())
                .payload(serde_json::json!({
                    "proposalId": proposal.id.clone(),
                    "proposedChange": proposal.proposed_change,
                    "status": "rejected",
                    "reason": reason.trim(),
                })),
        )?;
        Ok(proposal)
    }

    fn apply_delta(&self, project_id: &str, delta: &serde_json::Value) -> Result<()> {
        match delta.get("appliesTo").and_then(|v| v.as_str()) {
            Some("charter") => {
                let current = self.charter(project_id)?.ok_or_else(|| CoreError::NotFound {
                    what: "charter".into(),
                })?;
                let mut draft = CharterDraft {
                    original_problem: current.original_problem.clone(),
                    target_user: current.target_user.clone(),
                    core_value: current.core_value.clone(),
                    must_have: current.must_have.clone(),
                    wont_have: current.wont_have.clone(),
                    success_criteria: current.success_criteria.clone(),
                    architecture_principles: current.architecture_principles.clone(),
                    constraints: current.constraints.clone(),
                    locked: current.locked,
                };
                if let Some(list) = delta.get("wontHave").and_then(|v| v.as_array()) {
                    draft.wont_have = list.iter().filter_map(|v| v.as_str().map(String::from)).collect();
                }
                if let Some(list) = delta.get("mustHave").and_then(|v| v.as_array()) {
                    draft.must_have = list.iter().filter_map(|v| v.as_str().map(String::from)).collect();
                }
                self.save_charter(project_id, draft, ActorType::User)?;
            }
            Some("scope") => {
                let mut add = Vec::new();
                let mut remove = Vec::new();
                if let Some(items) = delta.get("add").and_then(|v| v.as_array()) {
                    for item in items {
                        if let (Some(section), Some(text)) = (
                            item.get("section").and_then(|v| v.as_str()),
                            item.get("item").and_then(|v| v.as_str()),
                        ) {
                            add.push((parse_section(section), text.to_string()));
                        }
                    }
                }
                if let Some(items) = delta.get("remove").and_then(|v| v.as_array()) {
                    for item in items {
                        if let (Some(section), Some(text)) = (
                            item.get("section").and_then(|v| v.as_str()),
                            item.get("item").and_then(|v| v.as_str()),
                        ) {
                            remove.push((parse_section(section), text.to_string()));
                        }
                    }
                }
                self.update_scope(project_id, add, remove, ActorType::User)?;
            }
            Some("architecture") => {
                let current = self.baseline(project_id)?;
                let mut draft = current
                    .map(|b| BaselineDraft {
                        summary: b.summary,
                        layers: b.layers,
                        invariants: b.invariants,
                        replaceable_parts: b.replaceable_parts,
                    })
                    .unwrap_or_default();
                if let Some(summary) = delta.get("summary").and_then(|v| v.as_str()) {
                    draft.summary = summary.to_string();
                }
                if let Some(invariants) = delta.get("invariants").and_then(|v| v.as_array()) {
                    draft.invariants = invariants
                        .iter()
                        .filter_map(|v| serde_json::from_value::<ArchitectureInvariant>(v.clone()).ok())
                        .collect();
                }
                self.set_baseline(project_id, draft, ActorType::User)?;
            }
            _ => return Err(CoreError::Validation(
                "change proposal delta needs appliesTo = charter | scope | architecture".into(),
            )),
        }
        Ok(())
    }

    fn index_charter(&self, charter: &ProjectCharter) -> Result<()> {
        let body = format!(
            "{}\n{}\nmust: {}\nwont: {}\nconstraints: {}",
            charter.original_problem,
            charter.core_value,
            charter.must_have.join("; "),
            charter.wont_have.join("; "),
            charter.constraints.join("; ")
        );
        self.repos.search.index(
            "charter",
            &charter.id,
            &charter.project_id,
            &format!("Charter v{}", charter.version),
            &body,
        )
    }
}

fn status_str(p: &ChangeProposal) -> String {
    match p.status {
        ChangeProposalStatus::Open => "open".into(),
        ChangeProposalStatus::Accepted => "accepted".into(),
        ChangeProposalStatus::Rejected => "rejected".into(),
        ChangeProposalStatus::Superseded => "superseded".into(),
    }
}

fn parse_section(value: &str) -> ScopeSection {
    match value {
        "inScope" => ScopeSection::InScope,
        "outOfScope" => ScopeSection::OutOfScope,
        "deferred" => ScopeSection::Deferred,
        _ => ScopeSection::Forbidden,
    }
}

fn index_of(section: &ScopeSection) -> usize {
    match section {
        ScopeSection::InScope => 0,
        ScopeSection::OutOfScope => 1,
        ScopeSection::Deferred => 2,
        ScopeSection::Forbidden => 3,
    }
}

/// Cheap token overlap used for proposal-vs-charter screening.
pub fn overlap(a: &str, b: &str) -> f64 {
    let ta: Vec<&str> = tokenize(a);
    let tb: Vec<&str> = tokenize(b);
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let hits = ta.iter().filter(|t| tb.contains(t)).count();
    hits as f64 / (tb.len().max(1)) as f64
}

pub fn tokenize(text: &str) -> Vec<&str> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| t.chars().count() > 1)
        .collect()
}
