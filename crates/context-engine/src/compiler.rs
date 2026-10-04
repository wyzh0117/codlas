//! The compiler itself: §42 retrieval → §53 gate → §43 budget → §41 markdown → §54
//! audit row, in that order and never a different one.
//!
//! The order is the point. Redaction that happens after assembly would have to be
//! found again in prose, and prose is where secrets hide; so the gate runs over
//! *items*, and only survivors are ever turned into text. Nothing here calls a model
//! (§60): a pack is compiled from facts the store already holds, by rules, so the
//! same project state always produces the same bytes.

use vibe_core::{CoreError, Repositories, Result};
use vibe_domain::{
    AuditLog, ContextBudget, ContextPack, ContextPackItem, ContextSection, PreparedPayload,
    ProjectPolicy,
};
use vibe_security::{is_cloud_target, PolicyFilter, PolicyOutcome};

use crate::budget::{self, estimate_tokens, TrimReport};
use crate::policy;
use crate::render::{ladder_order, render_markdown};
use crate::request::ContextRequest;
use crate::retrieve::{self, Retrieved};
use crate::route;
use crate::section::{rank, REQUEST};

/// The audit vocabulary of this crate (§54). `component` is always `context-engine`.
pub const COMPONENT: &str = "context-engine";
pub const ACTION_BUILD: &str = "build_context_pack";
pub const ACTION_PLAN: &str = "plan_context_pack";
pub const ACTION_PREPARE: &str = "prepare_for_agent";
pub const ACTION_REFUSE: &str = "refuse_cloud_target";

/// The total a caller gets when the request carries no budget: enough for a real
/// pack, small enough that §43's shares mean something.
pub const DEFAULT_TOTAL_TOKENS: usize = 3000;

/// What one rung of the ladder plans to do with an item — the dry run of `build`.
#[derive(Debug, Clone)]
pub struct PlannedItem {
    pub section: ContextSection,
    pub source_ref: String,
    pub content: String,
    /// Its §42 tier.
    pub priority: u8,
    pub estimated_tokens: usize,
    /// `false` means the policy gate or the budget withheld it.
    pub kept: bool,
    /// Why it did not travel, when it did not.
    pub reason: Option<String>,
}

/// Compiles context packs for one store.
#[derive(Clone)]
pub struct ContextCompiler {
    repos: Repositories,
    filter: PolicyFilter,
}

/// One compiled pass, before it becomes a pack.
struct Compiled {
    retrieved: Retrieved,
    policy: ProjectPolicy,
    budget: ContextBudget,
    outcome: PolicyOutcome,
    trimmed: TrimReport,
    kept: Vec<ContextPackItem>,
    candidates: Vec<ContextPackItem>,
}

impl ContextCompiler {
    pub fn new(repos: Repositories) -> Self {
        Self {
            repos,
            filter: PolicyFilter::new(),
        }
    }

    /// §43's default weighting at a caller's total.
    pub fn default_budget(total_tokens: usize) -> ContextBudget {
        budget::budget_with(total_tokens)
    }

    /// The policy in force for a project (§53).
    pub fn policy(&self, project_id: &str) -> Result<ProjectPolicy> {
        policy::load(&self.repos, project_id)
    }

    /// Store a project's policy. Policy is a human statement, so this is an
    /// application-facing convenience: nothing in the engine changes it itself.
    pub fn set_policy(&self, project_id: &str, policy: &ProjectPolicy) -> Result<()> {
        policy::save(&self.repos, project_id, policy)
    }

    /// Read-only view of the crate's own data path, for a "what would you include?"
    /// UI. It reads the stores exactly the way `build` does, and audits itself under
    /// [`ACTION_PLAN`]; it produces no pack.
    pub fn plan(&self, req: &ContextRequest) -> Result<Vec<PlannedItem>> {
        let compiled = self.compile(req)?;
        let kept_refs: Vec<&str> = compiled.kept.iter().map(|item| item.source_ref.as_str()).collect();
        let dropped: Vec<(String, String)> = compiled
            .outcome
            .dropped
            .iter()
            .map(|item| (item.source_ref.clone(), item.reason.clone()))
            .collect();
        let trimmed: Vec<(String, String)> = compiled
            .trimmed
            .trimmed
            .iter()
            .map(|item| {
                (
                    item.source_ref.clone(),
                    format!(
                        "over the §43 budget for {} ({} tokens)",
                        item.section.heading(),
                        compiled.budget.total_tokens
                    ),
                )
            })
            .collect();

        let mut out: Vec<PlannedItem> = Vec::with_capacity(compiled.candidates.len());
        for item in &compiled.candidates {
            let reason = dropped
                .iter()
                .find(|(r, _)| *r == item.source_ref)
                .map(|(_, why)| why.clone())
                .or_else(|| {
                    trimmed
                        .iter()
                        .find(|(r, _)| *r == item.source_ref)
                        .map(|(_, why)| why.clone())
                });
            out.push(PlannedItem {
                section: item.section,
                source_ref: item.source_ref.clone(),
                content: item.content.clone(),
                priority: item.priority,
                estimated_tokens: item.token_cost,
                kept: kept_refs.contains(&item.source_ref.as_str()),
                reason,
            });
        }
        let refs: Vec<String> = compiled
            .retrieved
            .read_refs
            .iter()
            .map(|r| r.clone())
            .collect();
        self.audit(
            req.project_id.clone(),
            ACTION_PLAN,
            refs,
            None,
            req.target_agent.clone(),
            Vec::new(),
        )?;
        Ok(out)
    }

    /// Compile one pack. Every successful build writes exactly one audit row (§54).
    pub fn build(&self, req: &ContextRequest) -> Result<ContextPack> {
        let compiled = self.compile(req)?;
        let markdown = render_markdown(&compiled.kept);
        let token_estimate = compiled.kept.iter().map(|item| item.token_cost).sum();
        let redacted_count = compiled
            .outcome
            .redactions
            .iter()
            .map(|redaction| redaction.count as usize)
            .sum();

        let pack = ContextPack {
            id: vibe_domain::new_id(),
            project_id: req.project_id.clone(),
            request: req.question.clone(),
            items: compiled.kept.clone(),
            markdown,
            token_estimate,
            created_at: vibe_domain::now(),
            policy_applied: compiled.policy.clone(),
            redacted_count,
        };

        // §54: what was read, what was produced, for whom, and what was withheld.
        let mut read_refs = compiled.retrieved.read_refs.clone();
        for item in &compiled.candidates {
            if !read_refs.iter().any(|seen| seen == &item.source_ref) {
                read_refs.push(item.source_ref.clone());
            }
        }
        let mut redacted_refs = compiled.outcome.redacted_refs.clone();
        redacted_refs.extend(
            compiled
                .trimmed
                .trimmed
                .iter()
                .map(|item| format!("{}#over_budget", item.source_ref)),
        );
        self.audit(
            req.project_id.clone(),
            ACTION_BUILD,
            read_refs,
            Some(format!("context_pack:{}", pack.id)),
            req.target_agent.clone(),
            redacted_refs,
        )?;
        Ok(pack)
    }

    /// Re-assemble the document from a pack's own items. Deterministic by contract:
    /// this returns exactly the bytes in `pack.markdown`.
    pub fn render(&self, pack: &ContextPack) -> String {
        render_markdown(&ladder_order(&pack.items))
    }

    /// Turn a pack into what a user can hand to an agent.
    ///
    /// **This does not send anything.** §7.8: the send is the user's action. The
    /// payload is the text plus an adapter hint, and `copy_fallback` is always true,
    /// meaning the UI must show it as copyable text whether or not an adapter is
    /// wired up. The policy gate runs again here against the *current* policy, so a
    /// rule tightened after the pack was built is honoured at hand-off time.
    pub fn prepare(&self, pack: &ContextPack, target: &str) -> Result<PreparedPayload> {
        let policy = policy::load(&self.repos, &pack.project_id)?;
        if let Some(error) = policy::refusal(&policy, Some(target)) {
            self.audit(
                pack.project_id.clone(),
                ACTION_REFUSE,
                vec![format!("context_pack:{}", pack.id)],
                None,
                Some(target.to_string()),
                vec![format!("context_pack:{}#{}", pack.id, "refused")],
            )?;
            return Err(error);
        }

        // The pack was already filtered; this pass is for a policy that moved since.
        let outcome = self
            .filter
            .apply(&policy, Some(target), pack.items.clone());
        let items = ladder_order(&outcome.items);
        let text = render_markdown(&items);

        let mut redacted_refs = outcome.redacted_refs.clone();
        redacted_refs.extend(
            pack.items
                .iter()
                .filter(|item| item.redacted)
                .map(|item| format!("{}#redacted_at_build", item.source_ref)),
        );
        self.audit(
            pack.project_id.clone(),
            ACTION_PREPARE,
            pack.items.iter().map(|item| item.source_ref.clone()).collect(),
            Some(format!("prepared:{target}")),
            Some(target.to_string()),
            redacted_refs,
        )?;

        Ok(PreparedPayload {
            adapter_id: target.to_string(),
            target: route::agent_target_for(target),
            text,
            // §36: a deep link is an adapter's own knowledge, never something the
            // engine invents (§7.6 forbids hardcoding platforms into core logic).
            deep_link: None,
            copy_fallback: true,
        })
    }

    /// §44 advice for a request: which adapter suits it, and whether policy allows.
    /// Recommendation only — the user picks, and the user sends (§7.8).
    pub fn recommend(
        &self,
        req: &ContextRequest,
        pack: Option<&ContextPack>,
    ) -> Result<Vec<route::Recommendation>> {
        let policy = policy::load(&self.repos, &req.project_id)?;
        Ok(route::recommend(&req.question, &policy, pack))
    }

    fn compile(&self, req: &ContextRequest) -> Result<Compiled> {
        let policy = policy::load(&self.repos, &req.project_id)?;
        // Refuse before reading anything: a cloud target the policy forbids gets no
        // pack and no look inside the project (§53).
        if let Some(error) = policy::refusal(&policy, req.target_agent.as_deref()) {
            self.audit(
                req.project_id.clone(),
                ACTION_REFUSE,
                Vec::new(),
                None,
                req.target_agent.clone(),
                vec![format!(
                    "{}#{}",
                    req.target_agent.clone().unwrap_or_default(),
                    if policy.local_only {
                        "local_only"
                    } else {
                        "allow_cloud_model"
                    }
                )],
            )?;
            return Err(error);
        }

        let retrieved = retrieve::gather(&self.repos, req)?;
        if retrieved.project.is_none() {
            return Err(CoreError::NotFound {
                what: format!("project {}", req.project_id),
            });
        }

        let candidates = retrieved.items.clone();
        let mut outcome = self
            .filter
            .apply(&policy, req.target_agent.as_deref(), retrieved.items.clone());
        // Redaction changes bytes, so the §43 arithmetic must be re-measured before
        // anything is dropped for being too expensive.
        for item in outcome.items.iter_mut() {
            item.token_cost = estimate_tokens(&item.content);
        }

        let total = req
            .budget
            .as_ref()
            .map(|b| b.total_tokens)
            .unwrap_or(DEFAULT_TOTAL_TOKENS);
        let weighting = req
            .budget
            .clone()
            .unwrap_or_else(|| Self::default_budget(total));
        let (mut kept, trimmed) = budget::trim(outcome.items.clone(), &weighting);

        // §43: a pack that gave anything up says so, inside the pack.
        if let Some(note) = trimmed.note(weighting.total_tokens) {
            let priority = REQUEST + 1;
            kept.push(ContextPackItem {
                section: ContextSection::Request,
                source_ref: "budget:context".to_string(),
                token_cost: estimate_tokens(&note),
                content: note,
                priority,
                redacted: false,
            });
            kept.sort_by_key(|item| (rank(item.section), item.priority));
        }
        // A request that reached this far but had nothing to say: the gate dropped
        // every item, which is a refusal the caller must see as such.
        if kept.iter().all(|item| item.section != ContextSection::Request)
            && outcome.items.is_empty()
        {
            outcome.dropped.push(vibe_security::DroppedItem {
                source_ref: "pack:all".to_string(),
                reason: "the policy gate left no item for this target".to_string(),
            });
        }

        Ok(Compiled {
            retrieved,
            policy,
            budget: weighting,
            outcome,
            trimmed,
            kept,
            candidates,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn audit(
        &self,
        project_id: String,
        action: &str,
        read_refs: Vec<String>,
        produced_ref: Option<String>,
        target_agent: Option<String>,
        redacted_refs: Vec<String>,
    ) -> Result<()> {
        self.repos.audit.append(&AuditLog {
            id: vibe_domain::new_id(),
            project_id,
            component: COMPONENT.to_string(),
            action: action.to_string(),
            read_refs,
            produced_ref,
            target_agent,
            redacted_refs,
            created_at: vibe_domain::now(),
        })
    }
}

/// The §53 decision for one target, exposed for callers that want to ask before
/// building.
pub fn would_refuse(policy: &ProjectPolicy, target: Option<&str>) -> bool {
    target.map(is_cloud_target).unwrap_or(false)
        && (policy.local_only || !policy.allow_cloud_model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_budget_carries_the_documented_shares() {
        let budget = ContextCompiler::default_budget(1000);
        assert_eq!(budget.total_tokens, 1000);
        assert_eq!(budget::allowance_of(&budget, ContextSection::RelevantArtifacts), 150);
    }

    #[test]
    fn refusal_is_a_policy_question_not_a_size_question() {
        let local_only = ProjectPolicy {
            local_only: true,
            ..ProjectPolicy::default()
        };
        assert!(would_refuse(&local_only, Some("chatgpt-web")));
        assert!(!would_refuse(&local_only, Some("generic-cli")));
        assert!(!would_refuse(&local_only, None));
        let open = ProjectPolicy {
            allow_cloud_model: true,
            ..ProjectPolicy::default()
        };
        assert!(!would_refuse(&open, Some("chatgpt-web")));
    }
}
