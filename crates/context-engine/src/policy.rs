//! §53 from the engine's side: where a project's policy comes from, and what a
//! refusal looks like.
//!
//! The gate itself lives in `vibe-security` and is the only place that decides what
//! may travel. This module supplies the two things the engine has to know: which
//! policy applies to a project, and how to say "no" in a way the caller can show.

use vibe_core::{CoreError, Repositories, Result};
use vibe_domain::ProjectPolicy;
use vibe_security::refs::{is_cloud_target, TargetClass};
use vibe_security::refs::target_class;

/// One policy per project, kept in the settings store. Nothing else in the workspace
/// reserves a location for it (§53 describes the rules, not their table).
pub const POLICY_KEY_PREFIX: &str = "context.policy.";

pub fn policy_key(project_id: &str) -> String {
    format!("{POLICY_KEY_PREFIX}{project_id}")
}

/// The policy a project runs under when it has never stated one.
///
/// `ProjectPolicy::default()` leaves `redact_secrets` false, because `Default` is
/// derived and serde's `default = "default_true"` only fires while deserialising. A
/// project that has said nothing has certainly not opted out of secret redaction, so
/// the engine's fallback turns it on here.
pub fn engine_default() -> ProjectPolicy {
    ProjectPolicy {
        redact_secrets: true,
        ..ProjectPolicy::default()
    }
}

/// Read the stored policy, falling back to [`engine_default`].
pub fn load(repos: &Repositories, project_id: &str) -> Result<ProjectPolicy> {
    match repos.settings.get_json(&policy_key(project_id))? {
        Some(value) => Ok(serde_json::from_value::<ProjectPolicy>(value)?),
        None => Ok(engine_default()),
    }
}

pub fn save(repos: &Repositories, project_id: &str, policy: &ProjectPolicy) -> Result<()> {
    repos
        .settings
        .set_json(&policy_key(project_id), &serde_json::to_value(policy)?)
}

/// Why a target may not receive this pack, in the words the UI shows.
///
/// `None` means the target is fine. A local target is never refused for §53 reasons:
/// local_only restricts what leaves the machine, and nothing leaves when it stays.
pub fn refusal_reason(policy: &ProjectPolicy, target: Option<&str>) -> Option<String> {
    let target = target?;
    if !matches!(target_class(target), TargetClass::Cloud) {
        return None;
    }
    if policy.local_only {
        return Some(format!(
            "project policy is local_only: '{target}' is a cloud target, so this pack is \
             refused instead of downgraded (§53)"
        ));
    }
    if !policy.allow_cloud_model {
        return Some(format!(
            "project policy has allowCloudModel=false: '{target}' is a cloud target, so this \
             pack is refused (§53)"
        ));
    }
    None
}

/// The hard `CoreError::Validation` a refused build or send answers with.
pub fn refusal(policy: &ProjectPolicy, target: Option<&str>) -> Option<CoreError> {
    refusal_reason(policy, target).map(CoreError::Validation)
}

/// Convenience for callers that only want the yes/no.
pub fn allowed(policy: &ProjectPolicy, target: &str) -> bool {
    !is_cloud_target(target) || (!policy.local_only && policy.allow_cloud_model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_undocumented_project_still_gets_secret_redaction() {
        assert!(engine_default().redact_secrets);
        assert!(!ProjectPolicy::default().redact_secrets);
        assert!(!engine_default().local_only);
    }

    #[test]
    fn local_targets_are_never_refused_for_being_local() {
        let strict = ProjectPolicy {
            local_only: true,
            allow_cloud_model: false,
            ..ProjectPolicy::default()
        };
        assert_eq!(refusal_reason(&strict, Some("generic-cli")), None);
        assert_eq!(refusal_reason(&strict, None), None);
        assert!(refusal_reason(&strict, Some("chatgpt-web")).is_some());
    }

    #[test]
    fn a_cloud_model_without_permission_is_refused_in_words() {
        let policy = ProjectPolicy {
            allow_cloud_model: false,
            ..engine_default()
        };
        let reason = refusal_reason(&policy, Some("chatgpt-web")).expect("refused");
        assert!(reason.contains("allowCloudModel=false"), "{reason}");
        assert!(reason.contains("chatgpt-web"));
    }

    #[test]
    fn the_policy_key_is_stable_per_project() {
        assert_eq!(policy_key("p-1"), "context.policy.p-1");
    }
}
