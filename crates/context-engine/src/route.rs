//! §44: agent routing. Recommendations only — the system never decides for the user.
//!
//! The rules are plain keyword arithmetic over the request and the compiled pack
//! (§60: zero-cost rules first, no model call). The output is advice: which kind of
//! adapter suits this question, why, and whether the project policy lets it through.
//! Choosing remains a human action, and sending the pack is a separate human action
//! again (§7.8).

use vibe_domain::{AgentTarget, ContextPack, ProjectPolicy};
use vibe_security::refs::{target_class, TargetClass};

/// An adapter worth suggesting, and the reason it was picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recommendation {
    /// A generic capability name, never a vendor: this crate does not know which
    /// adapters are installed (§7.6). The UI maps the hint onto its own adapters.
    pub adapter_hint: String,
    /// Where that adapter lives, as far as this crate can tell.
    pub target: AgentTarget,
    /// The §44 wording, in the user's terms: what this adapter is good at.
    pub suited_for: String,
    /// Why this pack points here.
    pub reason: String,
    /// `false` means the project policy (§53) forbids this route. The advice is still
    /// shown — the user is allowed to know what they are giving up — but a pack is
    /// never prepared for it.
    pub allowed_by_policy: bool,
    /// Higher is a better fit. Advisory, and deterministic.
    pub score: u8,
}

/// §44's three documented routes, expressed as capabilities.
pub const CODING: &str = "local-coding-agent";
pub const ANALYSIS: &str = "conversational-analysis";
pub const EXECUTION: &str = "local-execution";
/// The route that always exists: the user reads the pack and pastes it themselves.
pub const MANUAL: &str = "manual-copy";

const CODE_TERMS: [&str; 12] = [
    "代码", "修改", "重构", "实现", "修复", "报错", "测试", "code", "fix", "refactor", "implement",
    "test",
];
const ANALYSIS_TERMS: [&str; 12] = [
    "架构", "为什么", "方案", "评估", "产品", "分析", "权衡", "review", "architecture", "analyse",
    "analyze", "tradeoff",
];
const EXECUTION_TERMS: [&str; 10] = [
    "运行", "执行", "构建", "部署", "命令", "脚本", "build", "deploy", "command", "script",
];

fn hits(text: &str, terms: &[&str]) -> Vec<String> {
    let lower = text.to_lowercase();
    terms
        .iter()
        .filter(|term| lower.contains(*term))
        .map(|term| term.to_string())
        .collect()
}

/// The transport an adapter of this name most likely uses. Only used to label the
/// advice: `prepare` never sends anything itself.
pub fn agent_target_for(name: &str) -> AgentTarget {
    let lower = name.to_ascii_lowercase();
    if lower.contains("manual") || lower.contains("copy") || lower.contains("paste") {
        return AgentTarget::Manual;
    }
    if lower.contains("mcp") {
        return AgentTarget::Mcp;
    }
    if matches!(target_class(&lower), TargetClass::Cloud) || lower.contains("web") {
        return AgentTarget::Web;
    }
    if lower.contains("agent") || lower.contains("desktop") || lower.contains("ollama") {
        return AgentTarget::LocalAgent;
    }
    AgentTarget::Cli
}

/// Rank the routes for one request. Deterministic: same words, same pack, same list.
pub fn recommend(
    question: &str,
    policy: &ProjectPolicy,
    pack: Option<&ContextPack>,
) -> Vec<Recommendation> {
    let mut haystack = question.to_string();
    if let Some(pack) = pack {
        haystack.push(' ');
        haystack.push_str(&pack.request);
        for item in pack
            .items
            .iter()
            .filter(|item| item.priority <= 5)
            .take(12)
        {
            haystack.push(' ');
            haystack.push_str(&item.source_ref);
        }
    }

    let mut out = vec![
        route(
            CODING,
            "repo 内代码修改 / in-repo code changes",
            &hits(&haystack, &CODE_TERMS),
            "the request names code work, which belongs next to the repository itself",
            policy,
        ),
        route(
            ANALYSIS,
            "架构、产品、分析 / architecture, product, analysis",
            &hits(&haystack, &ANALYSIS_TERMS),
            "the request is a question about shape and tradeoffs, not an edit",
            policy,
        ),
        route(
            EXECUTION,
            "本地执行 / running things locally",
            &hits(&haystack, &EXECUTION_TERMS),
            "the request asks for something to be run, which needs a local shell",
            policy,
        ),
    ];
    out.push(Recommendation {
        adapter_hint: MANUAL.to_string(),
        target: AgentTarget::Manual,
        suited_for: "任何时候 / always available".to_string(),
        reason: "the pack is text; copying it yourself is the route that never depends on an \
                 adapter (§7.8 the send stays your action)."
            .to_string(),
        allowed_by_policy: true,
        score: 1,
    });
    out.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.allowed_by_policy.cmp(&a.allowed_by_policy))
            .then_with(|| a.adapter_hint.cmp(&b.adapter_hint))
    });
    out
}

fn route(
    hint: &str,
    suited_for: &str,
    matches: &[String],
    reason: &str,
    policy: &ProjectPolicy,
) -> Recommendation {
    let score = matches.len() as u8;
    let local = matches!(target_class(hint), TargetClass::Local);
    let allowed = local || (!policy.local_only && policy.allow_cloud_model);
    let reason = if matches.is_empty() {
        format!("{reason} — no wording in this request points here")
    } else {
        format!("{reason} (matched: {})", matches.join(", "))
    };
    Recommendation {
        adapter_hint: hint.to_string(),
        target: agent_target_for(hint),
        suited_for: suited_for.to_string(),
        reason,
        allowed_by_policy: allowed,
        score,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> ProjectPolicy {
        ProjectPolicy {
            local_only: false,
            allow_cloud_model: true,
            never_send_files: false,
            never_send_raw_conversation: false,
            redact_secrets: true,
        }
    }

    #[test]
    fn code_wording_points_at_the_repo_agent() {
        let advice = recommend("帮我修复这个登录 bug 的代码", &policy(), None);
        assert_eq!(advice[0].adapter_hint, CODING, "{advice:?}");
        assert!(advice[0].score > 0);
    }

    #[test]
    fn a_question_about_shape_points_at_analysis() {
        let advice = recommend("这个架构方案的产品权衡对不对", &policy(), None);
        assert_eq!(advice[0].adapter_hint, ANALYSIS, "{advice:?}");
    }

    #[test]
    fn manual_copy_is_always_on_the_list() {
        let advice = recommend("随便", &policy(), None);
        assert!(advice.iter().any(|item| item.adapter_hint == MANUAL));
    }

    #[test]
    fn a_local_only_project_flags_the_routes_it_forbids_without_hiding_them() {
        let strict = ProjectPolicy {
            local_only: true,
            allow_cloud_model: false,
            ..ProjectPolicy::default()
        };
        let advice = recommend("分析一下架构", &strict, None);
        assert!(
            advice.iter().any(|item| !item.allowed_by_policy),
            "the forbidden route is still named"
        );
        assert!(advice
            .iter()
            .any(|item| item.adapter_hint == CODING && item.allowed_by_policy));
    }

    #[test]
    fn the_target_label_follows_the_name() {
        assert_eq!(agent_target_for("generic-cli"), AgentTarget::Cli);
        assert_eq!(agent_target_for("chatgpt-web"), AgentTarget::Web);
        assert_eq!(agent_target_for("manual-copy"), AgentTarget::Manual);
        assert_eq!(agent_target_for("local-ollama"), AgentTarget::LocalAgent);
        assert_eq!(agent_target_for("some-tool"), AgentTarget::Web);
    }
}
