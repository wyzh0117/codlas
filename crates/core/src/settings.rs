//! Tunable, user-adjustable settings. Judge thresholds are the first ones that
//! matter: §27 explicitly allows adjusting them after user feedback.

use crate::error::Result;
use crate::judge::JudgeThresholds;
use crate::ports::Repositories;

const JUDGE_THRESHOLDS_KEY: &str = "judge.thresholds";

pub struct SettingsService {
    repos: Repositories,
}

impl SettingsService {
    pub fn new(repos: Repositories) -> Self {
        Self { repos }
    }

    pub fn judge_thresholds(&self) -> Result<JudgeThresholds> {
        Ok(self
            .repos
            .settings
            .get_json(JUDGE_THRESHOLDS_KEY)?
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default())
    }

    pub fn set_judge_thresholds(&self, thresholds: JudgeThresholds) -> Result<()> {
        if thresholds.inbox_floor > thresholds.auto_accept {
            return Err(crate::error::CoreError::Validation(
                "inbox floor must not exceed the auto-accept threshold".into(),
            ));
        }
        let value = serde_json::to_value(thresholds)?;
        self.repos.settings.set_json(JUDGE_THRESHOLDS_KEY, &value)
    }
}
