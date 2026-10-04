//! Drift Engine V1 — rules plus structured comparison, no inference (V1_DEVELOPMENT.md §26).
//!
//! Every rule reads an already-projected [`ProjectState`] (plus the event log where the
//! rule is about history) and returns alert *prompts*. Nothing here may rewrite the
//! Charter, Scope or Architecture baseline; a conflict raises an alert and the user
//! decides, which is PROJECT_MASTER §19 禁止 5 and §8.6 in code form.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, Utc};
use serde_json::json;

use vibe_core::error::CoreError;
use vibe_core::event_store::{EventDraft, EventStore};
use vibe_core::ports::{EntityStore, Repositories};
use vibe_domain::event_catalog;
use vibe_domain::{
    ActorType, ArchitectureBaseline, ArchitectureInvariant, ArchitectureLayer, Artifact,
    ChangeProposalStatus, Decision, DecisionStatus, DriftAlert, DriftSeverity, DriftStatus,
    DriftType, EntityKind, EventStatus, ProjectCharter, ProjectEvent, ProjectState, ScopeBoundary,
};
use vibe_core::Result;

use crate::text;

/// §26.5 — an assumption older than this is worth re-confirming.
pub const DEFAULT_STALE_DAYS: i64 = 90;
/// §26.4 — same object redesigned this often without a decision is drift.
pub const REDESIGN_LIMIT: u32 = 3;
pub const SUPERSEDE_CHAIN_LIMIT: usize = 3;

const SCOPE_THRESHOLD: f64 = 0.6;
const CONFLICT_THRESHOLD: f64 = 0.4;
const ANCHOR_THRESHOLD: f64 = 0.6;

/// The §29 button set for a Drift Alert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftAction {
    /// Boundary stays where it is; the alert is closed as resolved.
    KeepBoundary,
    /// The user accepts the change (the baseline itself still moves only through a
    /// Change Proposal they confirm).
    AcceptChange,
    /// Not drift at all.
    Dismiss,
    /// Handled outside the engine.
    Resolve,
}

impl DriftAction {
    pub fn slug(&self) -> &'static str {
        match self {
            DriftAction::KeepBoundary => "keep_boundary",
            DriftAction::AcceptChange => "accept_change",
            DriftAction::Dismiss => "dismiss",
            DriftAction::Resolve => "resolve",
        }
    }

    fn event_type(&self) -> &'static str {
        match self {
            DriftAction::AcceptChange => event_catalog::DRIFT_ACCEPTED,
            _ => event_catalog::DRIFT_DISMISSED,
        }
    }

    fn status(&self) -> DriftStatus {
        match self {
            DriftAction::AcceptChange => DriftStatus::AcceptedChange,
            DriftAction::Dismiss => DriftStatus::Dismissed,
            DriftAction::KeepBoundary | DriftAction::Resolve => DriftStatus::Resolved,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DriftConfig {
    pub enabled: Vec<DriftType>,
    pub stale_after_days: i64,
    pub scope_match_threshold: f64,
    pub conflict_overlap_threshold: f64,
    pub anchor_match_threshold: f64,
}

impl Default for DriftConfig {
    fn default() -> Self {
        Self {
            enabled: vec![
                DriftType::ScopeDrift,
                DriftType::DecisionConflict,
                DriftType::ArchitectureDrift,
                DriftType::FeatureCreep,
                DriftType::RepeatedRedesign,
                DriftType::StaleAssumption,
            ],
            stale_after_days: DEFAULT_STALE_DAYS,
            scope_match_threshold: SCOPE_THRESHOLD,
            conflict_overlap_threshold: CONFLICT_THRESHOLD,
            anchor_match_threshold: ANCHOR_THRESHOLD,
        }
    }
}

impl DriftConfig {
    pub fn enable(mut self, kind: DriftType, on: bool) -> Self {
        self.enabled.retain(|t| *t != kind);
        if on {
            self.enabled.push(kind);
        }
        self
    }

    pub fn is_enabled(&self, kind: DriftType) -> bool {
        self.enabled.contains(&kind)
    }
}

pub struct DriftEngine {
    repos: Repositories,
    events: EventStore,
    config: DriftConfig,
}

/// A state object a rule wants to measure, flattened to comparable text.
struct Candidate<'a> {
    kind: &'static str,
    id: &'a str,
    title: String,
    text: String,
}

impl DriftEngine {
    pub fn new(repos: Repositories) -> Self {
        let events = EventStore::new(repos.clone());
        Self {
            repos,
            events,
            config: DriftConfig::default(),
        }
    }

    pub fn with_config(repos: Repositories, config: DriftConfig) -> Self {
        let events = EventStore::new(repos.clone());
        Self {
            repos,
            events,
            config,
        }
    }

    pub fn config(&self) -> &DriftConfig {
        &self.config
    }

    pub fn repositories(&self) -> &Repositories {
        &self.repos
    }

    /// Runs every enabled rule, persists the genuinely new alerts and returns them.
    pub fn check(&self, state: &ProjectState) -> Result<Vec<DriftAlert>> {
        if state.project_id.trim().is_empty() {
            return Err(CoreError::Validation(
                "drift check needs a state with a project id".into(),
            ));
        }
        let now = vibe_domain::now();
        let mut raised = Vec::new();
        let mut known: BTreeSet<String> = self
            .stored_alerts(&state.project_id)?
            .iter()
            .map(signature)
            .collect();
        // An alert can also arrive inside the projected state before it is stored.
        known.extend(state.drift_alerts.iter().map(signature));

        for alert in self.detect(state, now) {
            // Any existing alert with this signature — open *or* already judged —
            // stops a second copy; re-opening a dismissed one would be worse.
            if !known.insert(signature(&alert)) {
                continue;
            }
            if self
                .repos
                .entities
                .get(EntityKind::DriftAlert, &alert.id)?
                .is_some()
            {
                continue;
            }
            if self.persist(&alert)? {
                raised.push(alert.clone());
            }
        }
        Ok(raised)
    }

    fn detect(&self, state: &ProjectState, now: DateTime<Utc>) -> Vec<DriftAlert> {
        let mut out = Vec::new();
        let push = |kind: DriftType, alerts: Vec<DriftAlert>, out: &mut Vec<DriftAlert>| {
            if self.config.is_enabled(kind) {
                out.extend(alerts);
            }
        };
        push(DriftType::ScopeDrift, self.scope_drift(state), &mut out);
        push(DriftType::DecisionConflict, self.decision_conflict(state), &mut out);
        push(DriftType::ArchitectureDrift, self.architecture_drift(state), &mut out);
        push(DriftType::FeatureCreep, self.feature_creep(state), &mut out);
        push(DriftType::RepeatedRedesign, self.repeated_redesign(state), &mut out);
        push(DriftType::StaleAssumption, self.stale_assumption(state, now), &mut out);
        out
    }

    fn persist(&self, alert: &DriftAlert) -> Result<bool> {
        self.repos.entities.put_entity(
            EntityKind::DriftAlert,
            alert,
            Some(status_slug(alert.status)),
        )?;
        let outcome = self.events.append(
            EventDraft::new(alert.project_id.clone(), event_catalog::DRIFT_DETECTED)
                // Rules, not a model: the detection is derived, never asserted (§19 禁止 6).
                .actor(ActorType::System)
                .subject("drift_alert", alert.id.clone())
                .payload(json!({
                    "alertId": alert.id,
                    "type": alert.r#type.as_str(),
                    "severity": severity_slug(alert.severity),
                    "summary": alert.summary,
                    "subject": subject_of(alert),
                    "baselineRefs": alert.baseline_refs,
                    "evidenceRefs": alert.evidence_refs,
                }))
                .evidence(alert.evidence_refs.clone()),
        )?;
        Ok(outcome.inserted)
    }

    // ---------------------------------------------------------------- rules

    /// §26.1 — open work that is worded like something the boundary excludes.
    /// In-scope items are context, never a trigger, so work inside the boundary is silent.
    pub fn scope_drift(&self, state: &ProjectState) -> Vec<DriftAlert> {
        let Some(scope) = self.scope_of(state) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for cand in self.scope_candidates(state) {
            for (section, items, severity) in [
                ("forbidden", &scope.forbidden, DriftSeverity::Warning),
                ("outOfScope", &scope.out_of_scope, DriftSeverity::Info),
            ] {
                for item in items {
                    if !text::covers(&cand.text, item, self.config.scope_match_threshold) {
                        continue;
                    }
                    let shared = text::shared_tokens(&cand.text, item);
                    out.push(self.alert(
                        state,
                        DriftType::ScopeDrift,
                        severity,
                        format!("{} \"{}\" touches {section} item \"{}\"", cand.kind, cand.title, text::excerpt(item, 80)),
                        Some(format!(
                            "Boundary v{} item \"{item}\" is matched by {} of its tokens. This is a prompt, not a block (§8.6).",
                            scope.version,
                            shared
                        )),
                        vec![format!("scope:v{}:{section}:{item}", scope.version)],
                        vec![cand.id.to_string()],
                    ));
                }
            }
        }
        out
    }

    /// §26.2 — new wording that flips the stance of a decision still in force.
    pub fn decision_conflict(&self, state: &ProjectState) -> Vec<DriftAlert> {
        let mut out = Vec::new();
        for (index, base) in state.decisions.iter().enumerate() {
            if base.status != DecisionStatus::Active {
                continue;
            }
            let baseline = decision_text(base);

            for (other_index, other) in state.decisions.iter().enumerate() {
                if other_index <= index || other.status != DecisionStatus::Active {
                    continue;
                }
                if !newer_decision(other, base, other_index, index) {
                    continue;
                }
                let candidate = decision_text(other);
                if !text::contradicts(&baseline, &candidate, self.config.conflict_overlap_threshold) {
                    continue;
                }
                out.push(self.conflict_alert(state, base, &candidate, &other.id, vec![other.id.to_string()]));
            }

            for cand in self.wording_candidates(state) {
                // Decision pairs are handled above; task and issue wording is the "new
                // suggestion" channel here.
                if cand.id == base.id || cand.kind == "decision" {
                    continue;
                }
                if !text::contradicts(&baseline, &cand.text, self.config.conflict_overlap_threshold) {
                    continue;
                }
                out.push(self.conflict_alert(
                    state,
                    base,
                    &cand.text,
                    cand.id,
                    vec![cand.id.to_string()],
                ));
            }

            for event in self.proposal_events(&state.project_id) {
                if let Some(date) = base.decided_at {
                    if event.observed_at <= date {
                        continue;
                    }
                }
                let Some(candidate) = event_text(&event) else {
                    continue;
                };
                if !text::contradicts(&baseline, &candidate, self.config.conflict_overlap_threshold) {
                    continue;
                }
                out.push(self.conflict_alert(
                    state,
                    base,
                    &candidate,
                    &event.id,
                    vec![event.id.clone()],
                ));
            }
        }
        out
    }

    fn conflict_alert(
        &self,
        state: &ProjectState,
        base: &Decision,
        candidate: &str,
        candidate_id: &str,
        mut evidence: Vec<String>,
    ) -> DriftAlert {
        let refs = vec![decision_ref(base)];
        let subject = candidate_id.to_string();
        // Convention: evidence_refs[0] is the subject, and it is what makes a repeated
        // finding hash to the same id instead of piling up.
        evidence.retain(|e| e != &subject);
        evidence.insert(0, subject);
        self.alert(
            state,
            DriftType::DecisionConflict,
            DriftSeverity::Warning,
            format!(
                "\"{}\" contradicts active decision \"{}\"",
                text::excerpt(candidate, 80),
                text::excerpt(&decision_label(base), 80)
            ),
            Some(format!(
                "V1 compares wording, not meaning: the two statements share wording but take opposite stances. Decision \"{}\" is still Active, so the new wording needs your call.",
                text::excerpt(&decision_label(base), 80)
            )),
            refs,
            evidence,
        )
    }

    /// §26.3 — recorded artifacts and current wording measured against invariants.
    pub fn architecture_drift(&self, state: &ProjectState) -> Vec<DriftAlert> {
        let Some(baseline) = self.baseline_of(state) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for invariant in &baseline.invariants {
            if invariant.detect.is_empty() {
                continue;
            }
            for artifact in state.artifacts.iter().filter(|a| !a.deleted && is_code_artifact(a)) {
                let path = artifact.path.clone().unwrap_or_else(|| artifact.name.clone());
                let haystack = format!(
                    "{} {} {}",
                    path,
                    artifact.name,
                    artifact.relationships.join(" ")
                );
                let hits: Vec<&String> = invariant
                    .detect
                    .iter()
                    .filter(|pattern| text::pattern_hit(&haystack, pattern))
                    .collect();
                let exposure = self.exposure(&baseline, invariant, &path);
                if exposure == Exposure::Owner {
                    // The dependency is allowed to live in this layer.
                    continue;
                }
                // A detect-pattern hit always flags; sitting in an invariant-named layer
                // flags too, because §22 records only path + hash for an artifact.
                if hits.is_empty() && exposure != Exposure::Constrained {
                    continue;
                }
                let layer_name = layer_label(&baseline, &path);
                let severity = match exposure {
                    Exposure::Constrained => DriftSeverity::High,
                    Exposure::Unknown | Exposure::Other => DriftSeverity::Warning,
                    Exposure::Owner => DriftSeverity::Info,
                };
                let summary = if hits.is_empty() {
                    format!(
                        "Artifact \"{path}\" sits in {} which invariant \"{}\" constrains",
                        layer_name,
                        text::excerpt(&invariant.text, 80)
                    )
                } else {
                    format!(
                        "Artifact \"{}\" matches detect pattern(s) [{}] of invariant \"{}\"",
                        path,
                        hits.iter().map(|p| p.as_str()).collect::<Vec<_>>().join(", "),
                        text::excerpt(&invariant.text, 80)
                    )
                };
                let detail = if hits.is_empty() {
                    // §22 keeps only path/hash in the record, so location is the signal
                    // and the user confirms with `scan_source`.
                    format!(
                        "The recorded metadata of this artifact carries no trace of the dependency, but it lives in {layer_name}. V1 stores paths and hashes only, so run the invariant scan over its source to confirm."
                    )
                } else {
                    format!(
                        "Invariant \"{text}\" keeps the dependency behind its own layer; {layer_name} reached it.",
                        text = text::excerpt(&invariant.text, 80),
                        layer_name = layer_name
                    )
                };
                let mut evidence = vec![artifact.id.clone()];
                if !artifact.id.eq(&path) {
                    evidence.push(path.clone());
                }
                out.push(self.alert(
                    state,
                    DriftType::ArchitectureDrift,
                    severity,
                    summary,
                    Some(detail),
                    vec![invariant_ref(&baseline, invariant)],
                    evidence,
                ));
            }

            for cand in self.wording_candidates(state) {
                let hits: Vec<&String> = invariant
                    .detect
                    .iter()
                    .filter(|pattern| text::pattern_hit(&cand.text, pattern))
                    .collect();
                if hits.is_empty() {
                    continue;
                }
                out.push(self.alert(
                    state,
                    DriftType::ArchitectureDrift,
                    DriftSeverity::Warning,
                    format!(
                        "{} \"{}\" wording matches invariant \"{}\"",
                        cand.kind,
                        text::excerpt(&cand.title, 60),
                        text::excerpt(&invariant.text, 80)
                    ),
                    Some(format!(
                        "Discussion is starting to route around the baseline via [{}] (§26.3). A prompt: the baseline only moves through a change proposal you confirm.",
                        hits.iter().map(|p| p.as_str()).collect::<Vec<_>>().join(", ")
                    )),
                    vec![invariant_ref(&baseline, invariant)],
                    vec![cand.id.to_string()],
                ));
            }
        }
        out
    }

    /// The machine-checkable half of §26.3: run the invariant patterns over real source
    /// text. Returns alerts; persistence stays with `check()` so callers can preview.
    pub fn scan_source(&self, state: &ProjectState, path: &str, content: &str) -> Vec<DriftAlert> {
        let Some(baseline) = self.baseline_of(state) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for invariant in &baseline.invariants {
            if invariant.detect.is_empty() {
                continue;
            }
            for (number, line) in content.lines().enumerate() {
                let hits: Vec<&String> = invariant
                    .detect
                    .iter()
                    .filter(|pattern| text::pattern_hit(line, pattern))
                    .collect();
                if hits.is_empty() {
                    continue;
                }
                let exposure = self.exposure(&baseline, invariant, path);
                if exposure == Exposure::Owner {
                    // The dependency is allowed to live here.
                    continue;
                }
                let severity = match exposure {
                    Exposure::Constrained => DriftSeverity::High,
                    _ => DriftSeverity::Warning,
                };
                let layer_name = layer_label(&baseline, path);
                let line_no = number + 1;
                let mut evidence = vec![format!("file:{path}:{line_no}")];
                if let Some(artifact) = state
                    .artifacts
                    .iter()
                    .find(|a| a.path.as_deref() == Some(path))
                {
                    evidence.push(artifact.id.clone());
                }
                out.push(self.alert(
                    state,
                    DriftType::ArchitectureDrift,
                    severity,
                    format!(
                        "{path}:{line_no} uses [{}] against invariant \"{}\"",
                        hits.iter().map(|p| p.as_str()).collect::<Vec<_>>().join(", "),
                        text::excerpt(&invariant.text, 80)
                    ),
                    Some(format!(
                        "{layer_name} imports the dependency directly: \"{}\"",
                        text::excerpt(line.trim(), 160)
                    )),
                    vec![invariant_ref(&baseline, invariant)],
                    evidence,
                ));
            }
        }
        out
    }

    /// §26.5 — work with no visible link back to the original design lands in the Inbox.
    pub fn feature_creep(&self, state: &ProjectState) -> Vec<DriftAlert> {
        let anchors = self.anchor_corpus(state);
        // Nothing to measure against yet: flagging every task of an un-initialised
        // project would drown the Inbox the rule exists to feed.
        if anchors.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut candidates: Vec<Candidate> = Vec::new();
        candidates.extend(state.open_tasks().into_iter().map(|t| Candidate {
            kind: "task",
            id: &t.id,
            title: t.title.clone(),
            text: format!("{} {}", t.title, t.detail.clone().unwrap_or_default()),
        }));
        for artifact in state.artifacts.iter().filter(|a| !a.deleted) {
            candidates.push(Candidate {
                kind: "artifact",
                id: &artifact.id,
                title: artifact.name.clone(),
                text: format!(
                    "{} {} {}",
                    artifact.name,
                    artifact.path.clone().unwrap_or_default(),
                    artifact.relationships.join(" ")
                ),
            });
        }
        for decision in state.active_decisions() {
            candidates.push(Candidate {
                kind: "decision",
                id: &decision.id,
                title: decision.title.clone(),
                text: decision_text(decision),
            });
        }

        for cand in candidates {
            if anchors
                .iter()
                .any(|(_, text)| text::related(&cand.text, text, self.config.anchor_match_threshold))
            {
                continue;
            }
            if self.has_provenance_link(EntityKind::Task, cand.id)
                || self.has_provenance_link(EntityKind::Artifact, cand.id)
            {
                continue;
            }
            let refs = anchor_refs(&anchors);
            out.push(self.alert(
                state,
                DriftType::FeatureCreep,
                DriftSeverity::Info,
                format!(
                    "{} \"{}\" links to no Charter mustHave, in-scope item, open change proposal or AEP",
                    cand.kind,
                    text::excerpt(&cand.title, 80)
                ),
                Some(
                    "New capability without a stated relation to the original design (§19 禁止 10). \
                     It is queued in the Inbox for you; nothing is blocked."
                        .into(),
                ),
                refs,
                vec![cand.id.to_string()],
            ));
        }
        out
    }

    /// §26.4 — the same object churned without a decision settling it.
    pub fn repeated_redesign(&self, state: &ProjectState) -> Vec<DriftAlert> {
        let mut out = Vec::new();
        for artifact in state.artifacts.iter().filter(|a| !a.deleted) {
            if artifact.redesign_count < REDESIGN_LIMIT {
                continue;
            }
            let mut refs = vec![format!("rule:repeated_redesign>={REDESIGN_LIMIT}")];
            if let Some(baseline) = self.baseline_of(state) {
                if !baseline.invariants.is_empty() {
                    refs.push(format!("baseline:v{}#{}", baseline.version, baseline.id));
                }
            }
            out.push(self.alert(
                state,
                DriftType::RepeatedRedesign,
                DriftSeverity::Warning,
                format!(
                    "Artifact \"{}\" was redesigned {} times",
                    text::excerpt(
                        artifact.path.as_deref().unwrap_or(&artifact.name),
                        80
                    ),
                    artifact.redesign_count
                ),
                Some(
                    "No superseding decision closes the loop, so the earlier designs keep competing (§26.4). \
                     Record which one stands, or stop redesigning."
                        .into(),
                ),
                refs,
                vec![artifact.id.clone()],
            ));
        }

        let mut chains: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for event in self.accepted_events(&state.project_id) {
            if !matches!(
                event.r#type.as_str(),
                event_catalog::DECISION_SUPERSEDED | event_catalog::DECISION_REVERSED
            ) {
                continue;
            }
            let Some(subject) = event.subject_id.clone() else {
                continue;
            };
            chains.entry(subject).or_default().push(event.id.clone());
        }
        for (subject, chain) in chains {
            if chain.len() < SUPERSEDE_CHAIN_LIMIT {
                continue;
            }
            let settled = state.decisions.iter().any(|d| {
                d.status == DecisionStatus::Active && d.supersedes.as_deref() == Some(subject.as_str())
            });
            if settled {
                continue;
            }
            let mut refs = vec![format!("rule:supersede_chain>={SUPERSEDE_CHAIN_LIMIT}")];
            if state.decisions.iter().any(|d| d.id == subject) {
                refs.push(format!("decision:{subject}"));
            }
            let mut evidence = vec![subject.clone()];
            evidence.extend(chain);
            out.push(self.alert(
                state,
                DriftType::RepeatedRedesign,
                DriftSeverity::Warning,
                format!("\"{subject}\" was reversed {count} times without a superseding decision", count = evidence.len() - 1),
                Some(
                    "The same object keeps coming back to the table. A decision that supersedes it will end the churn."
                        .into(),
                ),
                refs,
                evidence,
            ));
        }
        out
    }

    /// §8.6 / §26 — an active assumption that time and newer facts have caught up with.
    pub fn stale_assumption(
        &self,
        state: &ProjectState,
        now: DateTime<Utc>,
    ) -> Vec<DriftAlert> {
        let limit = Duration::days(self.config.stale_after_days.max(0));
        let events = self.accepted_events(&state.project_id);
        let mut out = Vec::new();
        for decision in state.active_decisions() {
            let Some(basis) = self.assumption_date(decision) else {
                continue;
            };
            let age = now.signed_duration_since(basis);
            if age < limit {
                continue;
            }
            let reopens: Vec<&ProjectEvent> = events
                .iter()
                .filter(|e| {
                    e.observed_at > basis
                        && !e.r#type.starts_with("DRIFT")
                        && mentions_decision(e, decision)
                })
                .collect();
            if reopens.is_empty() {
                continue;
            };
            let mut evidence = vec![decision.id.clone()];
            evidence.extend(reopens.iter().take(5).map(|e| e.id.clone()));
            let refs = vec![decision_ref(decision)];
            out.push(self.alert(
                state,
                DriftType::StaleAssumption,
                DriftSeverity::Info,
                format!(
                    "Active decision \"{}\" is {} days old and {} newer event(s) re-open it",
                    text::excerpt(&decision_label(decision), 80),
                    age.num_days(),
                    reopens.len()
                ),
                Some(format!(
                    "The assumption behind \"{}\" predates {} later accepted events (§26). Confirm it still holds or supersede it; nothing is blocked.",
                    text::excerpt(&decision_label(decision), 60),
                    reopens.len()
                )),
                refs,
                evidence,
            ));
        }
        out
    }

    // ------------------------------------------------------------- queries

    pub fn list(&self, project_id: &str) -> Result<Vec<DriftAlert>> {
        let mut alerts = self.stored_alerts(project_id)?;
        // Highest severity first, then detection time, then id: a stable reading order.
        alerts.sort_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then_with(|| a.detected_at.cmp(&b.detected_at))
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(alerts)
    }

    /// The user's judgement on an alert. The engine records it; it never edits a
    /// baseline as a side effect (禁止 5).
    pub fn resolve(
        &self,
        project_id: &str,
        alert_id: &str,
        action: DriftAction,
        reason: &str,
    ) -> Result<DriftAlert> {
        let mut alert = self
            .repos
            .entities
            .entity::<DriftAlert>(EntityKind::DriftAlert, alert_id)?
            .ok_or_else(|| CoreError::NotFound {
                what: format!("drift alert {alert_id}"),
            })?;
        if !project_id.is_empty() && alert.project_id != project_id {
            return Err(CoreError::NotFound {
                what: format!("drift alert {alert_id} in project {project_id}"),
            });
        }
        alert.status = action.status();
        let note = format!(
            "[{}] {}{}",
            action.slug(),
            reason.trim(),
            alert
                .detail
                .as_ref()
                .map(|d| format!("\n\n{d}"))
                .unwrap_or_default()
        );
        alert.detail = Some(note);
        self.repos.entities.put_entity(
            EntityKind::DriftAlert,
            &alert,
            Some(status_slug(alert.status)),
        )?;
        self.events.append(
            EventDraft::new(alert.project_id.clone(), action.event_type())
                .user()
                .subject("drift_alert", alert.id.clone())
                .payload(json!({
                    "alertId": alert.id,
                    "action": action.slug(),
                    "reason": reason.trim(),
                    "type": alert.r#type.as_str(),
                    "summary": alert.summary,
                })),
        )?;
        Ok(alert)
    }

    // -------------------------------------------------------------- helpers

    fn alert(
        &self,
        state: &ProjectState,
        kind: DriftType,
        severity: DriftSeverity,
        summary: String,
        detail: Option<String>,
        baseline_refs: Vec<String>,
        evidence_refs: Vec<String>,
    ) -> DriftAlert {
        let evidence_refs = if evidence_refs.is_empty() {
            vec![format!("state:{}", state.project_id)]
        } else {
            evidence_refs
        };
        let signature = signature_of(kind, &summary, &evidence_refs);
        let id = stable_id(&state.project_id, &signature);
        DriftAlert {
            id,
            project_id: state.project_id.clone(),
            r#type: kind,
            severity,
            summary,
            detail,
            baseline_refs,
            evidence_refs,
            status: DriftStatus::Open,
            detected_at: Some(vibe_domain::now()),
        }
    }

    fn stored_alerts(&self, project_id: &str) -> Result<Vec<DriftAlert>> {
        self.repos
            .entities
            .entities_of::<DriftAlert>(EntityKind::DriftAlert, project_id)
    }

    pub fn open_alerts(&self, state: &ProjectState) -> Result<Vec<DriftAlert>> {
        Ok(self
            .stored_alerts(&state.project_id)?
            .into_iter()
            .filter(|a| a.status == DriftStatus::Open)
            .collect())
    }

    fn scope_of(&self, state: &ProjectState) -> Option<ScopeBoundary> {
        if state.scope.is_some() {
            return state.scope.clone();
        }
        self.repos
            .projects
            .get_active_scope(&state.project_id)
            .ok()
            .flatten()
    }

    pub(crate) fn charter_of(&self, state: &ProjectState) -> Option<ProjectCharter> {
        if state.charter.is_some() {
            return state.charter.clone();
        }
        self.repos
            .projects
            .get_active_charter(&state.project_id)
            .ok()
            .flatten()
    }

    pub(crate) fn baseline_of(&self, state: &ProjectState) -> Option<ArchitectureBaseline> {
        if let Some(b) = &state.baseline {
            return if b.active { Some(b.clone()) } else { None };
        }
        self.repos
            .projects
            .get_active_baseline(&state.project_id)
            .ok()
            .flatten()
    }

    fn scope_candidates<'a>(&self, state: &'a ProjectState) -> Vec<Candidate<'a>> {
        let mut out: Vec<Candidate> = Vec::new();
        for t in state.open_tasks() {
            out.push(Candidate {
                kind: "task",
                id: &t.id,
                title: t.title.clone(),
                text: format!("{} {}", t.title, t.detail.clone().unwrap_or_default()),
            });
        }
        for i in state.open_issues() {
            out.push(Candidate {
                kind: "issue",
                id: &i.id,
                title: i.title.clone(),
                text: format!("{} {}", i.title, i.detail.clone().unwrap_or_default()),
            });
        }
        for p in state
            .change_proposals
            .iter()
            .filter(|p| p.status == ChangeProposalStatus::Open)
        {
            out.push(Candidate {
                kind: "change proposal",
                id: &p.id,
                title: p.proposed_change.clone(),
                text: format!(
                    "{} {} {}",
                    p.proposed_change,
                    p.reason,
                    p.affected_scope.join(" ")
                ),
            });
        }
        out
    }

    fn wording_candidates<'a>(&self, state: &'a ProjectState) -> Vec<Candidate<'a>> {
        let mut out: Vec<Candidate> = Vec::new();
        for t in state.open_tasks() {
            out.push(Candidate {
                kind: "task",
                id: &t.id,
                title: t.title.clone(),
                text: format!("{} {}", t.title, t.detail.clone().unwrap_or_default()),
            });
        }
        for i in state.open_issues() {
            out.push(Candidate {
                kind: "issue",
                id: &i.id,
                title: i.title.clone(),
                text: format!("{} {}", i.title, i.detail.clone().unwrap_or_default()),
            });
        }
        for d in state.active_decisions() {
            out.push(Candidate {
                kind: "decision",
                id: &d.id,
                title: d.title.clone(),
                text: decision_text(d),
            });
        }
        out
    }

    /// Charter mustHave, in-scope items, open proposals and AEPs: the four anchors
    /// §26.5 recognises as provenance for new work.
    fn anchor_corpus(&self, state: &ProjectState) -> Vec<(String, String)> {
        let mut anchors = Vec::new();
        if let Some(charter) = self.charter_of(state) {
            for item in &charter.must_have {
                anchors.push((format!("charter:v{}:mustHave", charter.version), item.clone()));
            }
        }
        if let Some(scope) = self.scope_of(state) {
            for item in &scope.in_scope {
                anchors.push((format!("scope:v{}:inScope", scope.version), item.clone()));
            }
        }
        for p in state
            .change_proposals
            .iter()
            .filter(|p| p.status == ChangeProposalStatus::Open)
        {
            anchors.push(("changeProposal:open".into(), format!("{} {}", p.proposed_change, p.reason)));
        }
        for a in state.aeps.iter().filter(|a| {
            !matches!(a.status, vibe_domain::AepStatus::Superseded)
        }) {
            anchors.push(("aep:active".into(), format!("{} {} {}", a.title, a.target, a.trigger.join(" "))));
        }
        anchors
    }

    fn has_provenance_link(&self, kind: EntityKind, id: &str) -> bool {
        if id.is_empty() {
            return false;
        }
        match self.repos.entities.links_from(kind, id) {
            Ok(links) => links.iter().any(|(to, _, _)| {
                matches!(
                    to,
                    EntityKind::ChangeProposal | EntityKind::Aep | EntityKind::Decision | EntityKind::Milestone
                )
            }),
            Err(_) => false,
        }
    }

    fn accepted_events(&self, project_id: &str) -> Vec<ProjectEvent> {
        // Rule signatures cannot fail; an unreadable log simply yields no history.
        self.events
            .list(project_id, Some(&[EventStatus::Accepted]), None)
            .unwrap_or_default()
    }

    fn proposal_events(&self, project_id: &str) -> Vec<ProjectEvent> {
        self.events
            .list(project_id, None, None)
            .unwrap_or_default()
            .into_iter()
            .filter(|e| {
                e.r#type == event_catalog::DECISION_PROPOSED
                    && !matches!(e.status, EventStatus::Rejected | EventStatus::Superseded)
            })
            .collect()
    }

    fn assumption_date(&self, decision: &Decision) -> Option<DateTime<Utc>> {
        if decision.decided_at.is_some() {
            return decision.decided_at;
        }
        // An assumption is only as old as its newest proof.
        decision
            .evidence_refs
            .iter()
            .filter_map(|id| self.repos.evidence.get_evidence(id).ok().flatten())
            .map(|e| e.created_at)
            .max()
    }

    /// Which layer a path sits in, and whether the dependency is allowed there.
    fn exposure(
        &self,
        baseline: &ArchitectureBaseline,
        invariant: &ArchitectureInvariant,
        path: &str,
    ) -> Exposure {
        let owners: Vec<&ArchitectureLayer> = baseline
            .layers
            .iter()
            .filter(|l| owns_dependency(invariant, l))
            .collect();
        let Some(layer) = layer_for_path(baseline, path) else {
            return Exposure::Unknown;
        };
        if owns_dependency(invariant, layer) {
            return Exposure::Owner;
        }
        if !owners.is_empty() && invariant_names_layer(invariant, layer) {
            return Exposure::Constrained;
        }
        Exposure::Other
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exposure {
    /// The dependency legitimately lives here.
    Owner,
    /// A layer the invariant singles out, while the dependency has a home elsewhere.
    Constrained,
    /// A declared layer the invariant says nothing about.
    Other,
    /// Not under any declared layer.
    Unknown,
}

fn is_code_artifact(artifact: &Artifact) -> bool {
    matches!(
        artifact.kind,
        vibe_domain::ArtifactKind::SourceFile
            | vibe_domain::ArtifactKind::Module
            | vibe_domain::ArtifactKind::Config
            | vibe_domain::ArtifactKind::Database
            | vibe_domain::ArtifactKind::Script
    )
}

// ----------------------------------------------------------- free helpers

/// `(type, summary, subject)` — the identity of one finding. `evidence_refs[0]` is
/// the subject by convention, so the same finding always hashes to the same id.
fn signature_of(kind: DriftType, summary: &str, evidence_refs: &[String]) -> String {
    let subject = evidence_refs.first().map(String::as_str).unwrap_or("-");
    format!("{}\u{1}{}\u{1}{subject}", kind.as_str(), summary)
}

fn signature(alert: &DriftAlert) -> String {
    signature_of(alert.r#type, &alert.summary, &alert.evidence_refs)
}

pub(crate) fn subject_of(alert: &DriftAlert) -> String {
    alert
        .evidence_refs
        .first()
        .cloned()
        .unwrap_or_else(|| "-".into())
}

/// Content-addressed id: a re-run of the same finding is the same entity, so the
/// store upserts instead of accumulating duplicates.
fn stable_id(project_id: &str, signature: &str) -> String {
    let hash = vibe_domain::content_hash(&format!("{project_id}\u{1}{signature}"));
    format!("drift-{}", &hash[..16])
}

pub fn status_slug(status: DriftStatus) -> &'static str {
    match status {
        DriftStatus::Open => "open",
        DriftStatus::AcceptedChange => "accepted_change",
        DriftStatus::Dismissed => "dismissed",
        DriftStatus::Resolved => "resolved",
    }
}

fn severity_slug(severity: DriftSeverity) -> &'static str {
    match severity {
        DriftSeverity::Info => "info",
        DriftSeverity::Warning => "warning",
        DriftSeverity::High => "high",
    }
}

fn decision_label(decision: &Decision) -> String {
    if !decision.title.is_empty() {
        return decision.title.clone();
    }
    text::excerpt(&decision.decision, 60)
}

fn decision_text(decision: &Decision) -> String {
    format!(
        "{} {} {}",
        decision.title,
        decision.decision,
        decision.rationale.clone().unwrap_or_default()
    )
}

fn decision_ref(decision: &Decision) -> String {
    let key = if decision.id.is_empty() {
        text::excerpt(&decision.decision, 40)
    } else {
        decision.id.clone()
    };
    format!(
        "decision:{key}:{}",
        if decision.locked { "locked" } else { "active" }
    )
}

fn invariant_ref(baseline: &ArchitectureBaseline, invariant: &ArchitectureInvariant) -> String {
    let key = if invariant.id.is_empty() {
        text::excerpt(&invariant.text, 40)
    } else {
        invariant.id.clone()
    };
    format!("baseline:v{}:invariant:{key}", baseline.version)
}

fn anchor_refs(anchors: &[(String, String)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (label, _) in anchors {
        if !out.contains(label) {
            out.push(label.clone());
        }
    }
    if out.is_empty() {
        out.push("charter:absent".into());
    }
    out
}

fn owns_dependency(invariant: &ArchitectureInvariant, layer: &ArchitectureLayer) -> bool {
    let haystack = format!(
        "{} {}",
        layer.name,
        layer.responsibility.clone().unwrap_or_default()
    );
    invariant.detect.iter().any(|p| text::pattern_hit(&haystack, p))
}

fn invariant_names_layer(invariant: &ArchitectureInvariant, layer: &ArchitectureLayer) -> bool {
    let name = text::squash(&layer.name);
    !name.is_empty() && text::tokens(&invariant.text).iter().any(|t| text::squash(t) == name)
}

fn layer_for_path<'a>(baseline: &'a ArchitectureBaseline, path: &str) -> Option<&'a ArchitectureLayer> {
    let segments: Vec<String> = path
        .to_lowercase()
        .split(['/', '\\'])
        .map(|s| text::squash(s))
        .filter(|s| !s.is_empty())
        .collect();
    baseline
        .layers
        .iter()
        .find(|l| !text::squash(&l.name).is_empty() && segments.contains(&text::squash(&l.name)))
}

fn layer_label(baseline: &ArchitectureBaseline, path: &str) -> String {
    match layer_for_path(baseline, path) {
        Some(layer) => format!("layer \"{}\"", layer.name),
        None => format!("no declared layer ({})", text::excerpt(path, 40)),
    }
}

pub(crate) fn event_text(event: &ProjectEvent) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for key in ["excerpt", "title", "summary", "text", "proposedChange", "decision"] {
        if let Some(value) = event.payload.get(key).and_then(|v| v.as_str()) {
            parts.push(value.to_string());
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join(" "))
}

fn mentions_decision(event: &ProjectEvent, decision: &Decision) -> bool {
    if event.subject_id.as_deref() == Some(decision.id.as_str()) {
        return true;
    }
    event_text(event)
        .map(|text| text::related(&text, &decision_text(decision), SCOPE_THRESHOLD))
        .unwrap_or(false)
}

fn newer_decision(candidate: &Decision, base: &Decision, ci: usize, bi: usize) -> bool {
    match (candidate.decided_at, base.decided_at) {
        (Some(a), Some(b)) => a > b,
        (Some(_), None) => true,
        (None, Some(_)) => false,
        // No timestamps at all: state order is the recorded chronology.
        (None, None) => ci > bi,
    }
}
