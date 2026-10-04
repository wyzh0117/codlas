//! The state engine's front door: read the accepted log, fold it, hand the result
//! to the caches (V1_DEVELOPMENT.md §18, §50, §59).
//!
//! Direction of data flow is one-way and that is the whole point:
//!
//! ```text
//! events ──fold──▶ ProjectState ──▶ entities / artifacts / evidence rows
//! ```
//!
//! `fold` never reads an entity, artifact or evidence row back, so the projection
//! is reproducible from the log alone (`rebuild` is just `project_state` again).
//! Persistence is a cache write; dropping every cache table and re-folding must
//! yield the same `state_hash`.

use vibe_core::error::Result;
use vibe_core::ports::{EntityStore, Repositories};
use vibe_domain::{EntityKind, ProjectEvent, ProjectState};
use serde::Serialize;

use crate::payload::canonical_json;
use crate::projector::{self, EntityWrite, FoldOutput, Projected};

/// Everything a caller may want to know about a replay besides the state itself.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayReport {
    pub project_id: String,
    pub applied_events: u64,
    pub last_seq: i64,
    pub rows_written: usize,
    pub evidence_rows: usize,
    /// Registered types the fold deliberately does not act on, plus unknown types
    /// it tolerated. Neither is an error; both are visible.
    pub unknown_types: Vec<String>,
    pub state_hash: String,
}

/// The pure fold's result plus what the fold noticed while producing it.
#[derive(Debug, Clone)]
pub struct ProjectionReport {
    pub state: ProjectState,
    pub rows_written: usize,
    pub evidence_rows: usize,
    pub unknown_types: Vec<String>,
}

pub struct Reducer {
    repos: Repositories,
}

impl Reducer {
    pub fn new(repos: Repositories) -> Self {
        Self { repos }
    }

    pub fn repositories(&self) -> &Repositories {
        &self.repos
    }

    /// Accepted events in store order, re-sorted by `seq` defensively so a store
    /// that ever hands them back shuffled still projects identically.
    pub fn accepted_events(&self, project_id: &str) -> Result<Vec<ProjectEvent>> {
        let mut events = self.repos.events.list_accepted(project_id, None)?;
        events.sort_by_key(|event| event.seq.unwrap_or(i64::MIN));
        debug_assert!(
            events.windows(2).all(|pair| pair[0].seq <= pair[1].seq),
            "replay order must be monotonic in seq"
        );
        Ok(events)
    }

    /// The full projection: fold the accepted log, add the read-only baseline
    /// sections, refresh the caches, stamp the hash.
    pub fn project_state(&self, project_id: &str) -> Result<ProjectState> {
        let events = self.accepted_events(project_id)?;
        let output = projector::fold(project_id, &events)?;
        self.persist(&output)?;
        self.finish(output, project_id)
    }

    /// The same projection without refreshing the caches. Reading the state (a map
    /// compile, a snapshot) does not need to rewrite every derived row first;
    /// `project_state` is the call that materializes.
    pub fn project_read_only(&self, project_id: &str) -> Result<ProjectState> {
        let events = self.accepted_events(project_id)?;
        let output = projector::fold(project_id, &events)?;
        self.finish(output, project_id)
    }

    fn finish(&self, output: FoldOutput, project_id: &str) -> Result<ProjectState> {
        // Hashed before the read-only sections are attached: the hash is the
        // fingerprint of the log, and baselines can change without a new event.
        let hash = state_hash_of(&output.state);
        let mut state = self.with_baselines(output.state, project_id)?;
        state.state_hash = Some(hash);
        state.built_at = Some(vibe_domain::now());
        Ok(state)
    }

    /// The same projection without any store access: pure function of the events
    /// handed in, in the order handed in.
    pub fn fold(&self, project_id: &str, events: &[ProjectEvent]) -> Result<ProjectState> {
        Ok(self.report(project_id, events)?.state)
    }

    /// The fold plus what it noticed — the bookkeeping a caller shows instead of
    /// guessing. Pure: nothing here touches a repository.
    pub fn report(&self, project_id: &str, events: &[ProjectEvent]) -> Result<ProjectionReport> {
        let output = projector::fold(project_id, events)?;
        Ok(ProjectionReport {
            state: with_hash(output.state),
            rows_written: output.writes.len(),
            evidence_rows: output.evidence.len(),
            unknown_types: output.unknown_types.clone(),
        })
    }

    /// Fold plus the replay bookkeeping, for callers that want to show what the
    /// reducer did instead of guessing.
    pub fn replay_report(&self, project_id: &str) -> Result<ReplayReport> {
        let events = self.accepted_events(project_id)?;
        let output = projector::fold(project_id, &events)?;
        let report = ReplayReport {
            applied_events: output.state.applied_events,
            last_seq: output.state.last_seq,
            rows_written: output.writes.len(),
            evidence_rows: output.evidence.len(),
            unknown_types: output.unknown_types.clone(),
            state_hash: state_hash_of(&output.state),
            project_id: project_id.to_string(),
        };
        self.persist(&output)?;
        Ok(report)
    }

    /// Write only the evidence rows and their subject links: the half of
    /// materialization that answers "why does the system believe this?". A caller
    /// that wants the audit trail without rewriting every state row uses this.
    pub fn materialize_evidence(&self, project_id: &str) -> Result<usize> {
        let events = self.accepted_events(project_id)?;
        let output = projector::fold(project_id, &events)?;
        self.write_evidence(&output.evidence)
    }

    /// Rebuild the caches from the log. There is nothing to clear first: every row
    /// is an upsert of a value the fold just derived, so a rebuild is idempotent.
    pub fn rebuild(&self, project_id: &str) -> Result<ProjectState> {
        self.project_state(project_id)
    }

    /// Charter / scope / baseline / project are versioned tables the user writes
    /// through `ProjectService`; here they are only ever read (§19 禁止 4).
    fn with_baselines(&self, mut state: ProjectState, project_id: &str) -> Result<ProjectState> {
        state.project = self.repos.projects.get_project(project_id)?;
        state.charter = self.repos.projects.get_active_charter(project_id)?;
        state.scope = self.repos.projects.get_active_scope(project_id)?;
        state.baseline = self.repos.projects.get_active_baseline(project_id)?;
        Ok(state)
    }

    fn persist(&self, output: &FoldOutput) -> Result<()> {
        self.write_evidence(&output.evidence)?;
        for write in &output.writes {
            self.write_row(write)?;
        }
        Ok(())
    }

    fn write_evidence(&self, rows: &[crate::projector::EvidenceWrite]) -> Result<usize> {
        for row in rows {
            if self.repos.evidence.get_evidence(&row.evidence.id)?.is_none() {
                self.repos.evidence.insert_evidence(&row.evidence)?;
            }
            // A ref that already names a row the capture layer created gets linked
            // too, so a claim traces back to the original message or file.
            if let Some(reference) = row.evidence.ref_id.as_deref() {
                if let Some(original) = self.repos.evidence.find_by_ref(row.evidence.r#type, reference)?
                {
                    if original.id != row.evidence.id {
                        for (kind, id) in &row.subjects {
                            self.repos.evidence.link(&original.id, *kind, id)?;
                        }
                    }
                }
            }
            for (kind, id) in &row.subjects {
                self.repos.evidence.link(&row.evidence.id, *kind, id)?;
            }
        }
        Ok(rows.len())
    }

    fn write_row(&self, write: &EntityWrite) -> Result<()> {
        let status = write.status.as_str();
        match &write.entity {
            Projected::Task(task) => {
                self.repos.entities.put_entity(EntityKind::Task, task, Some(status))?
            }
            Projected::Issue(issue) => {
                self.repos.entities.put_entity(EntityKind::Issue, issue, Some(status))?
            }
            Projected::Decision(decision) => self
                .repos
                .entities
                .put_entity(EntityKind::Decision, decision, Some(status))?,
            Projected::Artifact(artifact) => {
                self.repos.entities.put_entity(EntityKind::Artifact, artifact, Some(status))?;
                // The dedicated artifact table is what path lookups and
                // `find_artifact_by_path` read from; both are caches of this fold.
                self.repos.artifacts.upsert_artifact(artifact)?;
            }
            Projected::Milestone(milestone) => self
                .repos
                .entities
                .put_entity(EntityKind::Milestone, milestone, Some(status))?,
            Projected::Proposal(proposal) => self
                .repos
                .entities
                .put_entity(EntityKind::ChangeProposal, proposal, Some(status))?,
            Projected::Aep(aep) => {
                self.repos.entities.put_entity(EntityKind::Aep, aep, Some(status))?
            }
            Projected::Drift(alert) => {
                self.repos.entities.put_entity(EntityKind::DriftAlert, alert, Some(status))?
            }
        }
        if let Some(version) = &write.version {
            self.repos.entities.record_version(
                write.kind,
                &write.id,
                version.number,
                Some(&version.before),
                &version.after,
                &version.reason,
            )?;
        }
        Ok(())
    }
}

fn with_hash(mut state: ProjectState) -> ProjectState {
    state.state_hash = Some(state_hash_of(&state));
    state
}

/// The hashable part of a projection: everything the events produced, in a fixed
/// field order. `built_at`, the hash itself and the read-only baseline sections are
/// excluded so two runs over the same log always agree (§18 determinism).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectionCore<'a> {
    project_id: &'a str,
    tasks: &'a [vibe_domain::Task],
    issues: &'a [vibe_domain::Issue],
    decisions: &'a [vibe_domain::Decision],
    artifacts: &'a [vibe_domain::Artifact],
    milestones: &'a [vibe_domain::Milestone],
    change_proposals: &'a [vibe_domain::ChangeProposal],
    aeps: &'a [vibe_domain::ArchitectureEvolutionPlan],
    drift_alerts: &'a [vibe_domain::DriftAlert],
    evidence: &'a [vibe_domain::Evidence],
    last_seq: i64,
    applied_events: u64,
}

pub fn state_hash_of(state: &ProjectState) -> String {
    let core = ProjectionCore {
        project_id: &state.project_id,
        tasks: &state.tasks,
        issues: &state.issues,
        decisions: &state.decisions,
        artifacts: &state.artifacts,
        milestones: &state.milestones,
        change_proposals: &state.change_proposals,
        aeps: &state.aeps,
        drift_alerts: &state.drift_alerts,
        evidence: &state.evidence,
        last_seq: state.last_seq,
        applied_events: state.applied_events,
    };
    let value = serde_json::to_value(&core).unwrap_or(serde_json::Value::Null);
    vibe_domain::content_hash(&canonical_json(&value))
}

/// Convenience for callers that only have a state in hand.
pub fn state_hash(state: &ProjectState) -> &str {
    static EMPTY: &str = "";
    state.state_hash.as_deref().unwrap_or(EMPTY)
}
