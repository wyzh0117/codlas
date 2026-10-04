//! State Judge (V1_DEVELOPMENT.md §27). Decides whether a captured message
//! becomes a Project Event, and how much trust that event deserves.
//!
//! Rules run first and cost nothing; an optional model judge can refine a
//! confidence that rules alone cannot settle.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use vibe_domain::{ActorType, Evidence, EvidenceType};

use crate::event_store::EventDraft;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JudgeThresholds {
    pub auto_accept: f64,
    pub inbox_floor: f64,
}

impl Default for JudgeThresholds {
    fn default() -> Self {
        // §27 suggested defaults.
        Self {
            auto_accept: 0.90,
            inbox_floor: 0.65,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JudgeRoute {
    /// Written and reduced.
    Accept,
    /// Written, but waits for a human in the Inbox.
    Inbox,
    /// Written for traceability only; must never change state.
    Candidate,
}

pub fn route_for(confidence: f64, thresholds: JudgeThresholds) -> JudgeRoute {
    if confidence >= thresholds.auto_accept {
        JudgeRoute::Accept
    } else if confidence >= thresholds.inbox_floor {
        JudgeRoute::Inbox
    } else {
        JudgeRoute::Candidate
    }
}

#[derive(Debug, Clone)]
pub struct CaptureSignal {
    pub project_id: String,
    pub source_id: String,
    pub conversation_id: String,
    pub message_id: String,
    pub actor_type: ActorType,
    pub text: String,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct JudgedSignal {
    pub draft: EventDraft,
    pub route: JudgeRoute,
    pub rationale: String,
}

/// A model-backed tie-breaker. Deliberately narrow: it adjusts confidence, it
/// never writes project facts by itself (PROJECT_MASTER §19 禁止 6).
pub trait LlmJudge: Send + Sync {
    fn refine(&self, signal: &CaptureSignal, candidate: &JudgedSignal) -> Option<f64>;
}

pub trait SignalJudge: Send + Sync {
    fn judge(&self, signal: &CaptureSignal) -> Vec<JudgedSignal>;
}

struct Rule {
    event_type: &'static str,
    /// Only the user may confirm these.
    user_only: bool,
    strong: &'static [&'static str],
    weak: &'static [&'static str],
    base_confidence: f64,
}

const RULES: &[Rule] = &[
    Rule {
        event_type: vibe_domain::event_catalog::TASK_COMPLETED_CLAIM,
        user_only: false,
        strong: &["已经实现", "完成了", "全部通过", "搞定了"],
        weak: &["done", "implemented", "finished", "i've added", "fixed it"],
        base_confidence: 0.82,
    },
    Rule {
        event_type: vibe_domain::event_catalog::DECISION_CONFIRMED,
        user_only: true,
        strong: &["就这样定", "确认保留", "我们采用", "保持不变"],
        weak: &["we decided", "let's go with", "decision:", "keep the", "stick with"],
        base_confidence: 0.93,
    },
    Rule {
        event_type: vibe_domain::event_catalog::DECISION_PROPOSED,
        user_only: false,
        strong: &["建议", "我建议", "不如改成"],
        weak: &["i propose", "we should", "recommend", "consider switching", "why not just"],
        base_confidence: 0.80,
    },
    Rule {
        event_type: vibe_domain::event_catalog::ISSUE_FOUND,
        user_only: false,
        strong: &["报错", "还是不对", "又不行", "崩溃", "仍然是"],
        weak: &["bug", "error", "failing", "broken", "still broken", "why is it still"],
        base_confidence: 0.86,
    },
    Rule {
        event_type: vibe_domain::event_catalog::TASK_CREATED,
        user_only: true,
        strong: &["接下来做", "需要实现", "新增任务"],
        weak: &["next step", "todo:", "we need to", "let's build"],
        base_confidence: 0.88,
    },
];

pub struct RuleJudge {
    thresholds: JudgeThresholds,
    llm: Option<Arc<dyn LlmJudge>>,
}

impl RuleJudge {
    pub fn new(thresholds: JudgeThresholds) -> Self {
        Self {
            thresholds,
            llm: None,
        }
    }

    pub fn with_llm(mut self, llm: Arc<dyn LlmJudge>) -> Self {
        self.llm = Some(llm);
        self
    }

    pub fn thresholds(&self) -> JudgeThresholds {
        self.thresholds
    }

    fn matched(rule: &Rule, lower: &str) -> Option<(f64, String)> {
        if let Some(hit) = rule.strong.iter().find(|k| lower.contains(*k)) {
            return Some((rule.base_confidence + 0.12, (*hit).to_string()));
        }
        if let Some(hit) = rule.weak.iter().find(|k| lower.contains(*k)) {
            return Some((rule.base_confidence, (*hit).to_string()));
        }
        None
    }

    /// A machine-sourced signal (git, tests, files) is fact, not interpretation.
    fn deterministic(signal: &CaptureSignal) -> Option<(&'static str, serde_json::Value)> {
        if signal.actor_type != ActorType::System {
            return None;
        }
        let lower = signal.text.to_lowercase();
        if lower.starts_with("commit ") || lower.contains("git commit") {
            return Some((
                vibe_domain::event_catalog::GIT_COMMIT,
                serde_json::json!({ "excerpt": excerpt(&signal.text) }),
            ));
        }
        if lower.contains("tests passed") || lower.contains("test passed") || lower.contains("测试通过") {
            return Some((
                vibe_domain::event_catalog::TEST_PASSED,
                serde_json::json!({ "excerpt": excerpt(&signal.text) }),
            ));
        }
        if lower.contains("test failed") || lower.contains("tests failed") || lower.contains("测试失败") {
            return Some((
                vibe_domain::event_catalog::TEST_FAILED,
                serde_json::json!({ "excerpt": excerpt(&signal.text) }),
            ));
        }
        if lower.starts_with("modified ") || lower.starts_with("file ") || lower.contains("文件变更") {
            return Some((
                vibe_domain::event_catalog::FILE_CHANGED,
                serde_json::json!({ "excerpt": excerpt(&signal.text) }),
            ));
        }
        None
    }
}

impl SignalJudge for RuleJudge {
    fn judge(&self, signal: &CaptureSignal) -> Vec<JudgedSignal> {
        let mut out = Vec::new();

        if let Some((event_type, payload)) = Self::deterministic(signal) {
            out.push(JudgedSignal {
                draft: base_draft(signal, event_type, ActorType::System, payload)
                    .confidence(1.0),
                route: JudgeRoute::Accept,
                rationale: "system signal is recorded as fact".into(),
            });
            return out;
        }

        let lower = signal.text.to_lowercase();
        let mut hits: Vec<(f64, String, &'static str)> = Vec::new();
        for rule in RULES {
            if rule.user_only && signal.actor_type != ActorType::User {
                continue;
            }
            if let Some((confidence, keyword)) = Self::matched(rule, &lower) {
                hits.push((confidence, keyword, rule.event_type));
            }
        }

        if hits.is_empty() {
            return out;
        }

        // Competing interpretations of the same sentence are exactly what the
        // Inbox is for: keep the strongest reading, lower its trust.
        hits.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let ambiguity_penalty = if hits.len() > 1 { -0.15 } else { 0.0 };
        let (confidence, keyword, event_type) = hits.remove(0);
        let final_confidence = (confidence + ambiguity_penalty).clamp(0.0, 1.0);
        let payload = serde_json::json!({
            "excerpt": excerpt(&signal.text),
            "matched": keyword,
            "alternatives": hits.len(),
        });
        let draft = base_draft(signal, event_type, signal.actor_type, payload)
            .confidence(final_confidence);
        let route = route_for(final_confidence, self.thresholds);
        let mut judged = JudgedSignal {
            draft,
            route,
            rationale: format!("rule match '{keyword}' at {final_confidence:.2}"),
        };
        if judged.route == JudgeRoute::Candidate {
            if let Some(llm) = &self.llm {
                if let Some(bumped) = llm.refine(signal, &judged) {
                    let bumped = bumped.clamp(0.0, 1.0);
                    judged.draft = judged.draft.confidence(bumped);
                    judged.route = route_for(bumped, self.thresholds);
                    judged.rationale =
                        format!("{}, model judge raised it to {bumped:.2}", judged.rationale);
                }
            }
        }
        out.push(judged);
        out
    }
}

fn base_draft(
    signal: &CaptureSignal,
    event_type: &str,
    actor: ActorType,
    payload: serde_json::Value,
) -> EventDraft {
    EventDraft::new(signal.project_id.clone(), event_type)
        .actor(actor)
        .from_message(
            signal.source_id.clone(),
            signal.conversation_id.clone(),
            signal.message_id.clone(),
        )
        .payload(payload)
}

pub fn excerpt(text: &str) -> String {
    let trimmed = text.trim();
    const MAX: usize = 600;
    if trimmed.chars().count() <= MAX {
        return trimmed.to_string();
    }
    let cut: String = trimmed.chars().take(MAX).collect();
    format!("{cut}…")
}

/// Evidence record for a message that justified an event (§17).
pub fn message_evidence(project_id: &str, message_id: &str, summary: &str) -> Evidence {
    Evidence {
        id: vibe_domain::new_id(),
        project_id: project_id.to_string(),
        r#type: EvidenceType::Message,
        ref_id: Some(message_id.to_string()),
        summary: Some(summary.to_string()),
        created_at: vibe_domain::now(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signal(text: &str, actor: ActorType) -> CaptureSignal {
        CaptureSignal {
            project_id: "p1".into(),
            source_id: "s1".into(),
            conversation_id: "c1".into(),
            message_id: "m1".into(),
            actor_type: actor,
            text: text.into(),
            observed_at: vibe_domain::now(),
        }
    }

    #[test]
    fn thresholds_split_the_three_routes() {
        let t = JudgeThresholds::default();
        assert_eq!(route_for(0.95, t), JudgeRoute::Accept);
        assert_eq!(route_for(0.70, t), JudgeRoute::Inbox);
        assert_eq!(route_for(0.40, t), JudgeRoute::Candidate);
    }

    #[test]
    fn agent_completion_claim_is_not_a_confirmation() {
        let judge = RuleJudge::new(JudgeThresholds::default());
        let hits = judge.judge(&signal("我已经实现了重连逻辑", ActorType::Agent));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].draft.event_type, "TASK_COMPLETED_CLAIM");
    }

    #[test]
    fn user_decision_confirmation_is_auto_accepted() {
        let judge = RuleJudge::new(JudgeThresholds::default());
        let hits = judge.judge(&signal("确认保留 Repository 抽象层", ActorType::User));
        assert_eq!(hits[0].draft.event_type, "DECISION_CONFIRMED");
        assert_eq!(hits[0].route, JudgeRoute::Accept);
    }

    #[test]
    fn system_test_signal_is_fact() {
        let judge = RuleJudge::new(JudgeThresholds::default());
        let hits = judge.judge(&signal("tests passed: 42 ok", ActorType::System));
        assert_eq!(hits[0].draft.event_type, "TEST_PASSED");
        assert_eq!(hits[0].draft.confidence, 1.0);
    }

    #[test]
    fn plain_chatter_produces_no_event() {
        let judge = RuleJudge::new(JudgeThresholds::default());
        assert!(judge.judge(&signal("早上好", ActorType::User)).is_empty());
    }
}
