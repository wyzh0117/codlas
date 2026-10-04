//! Failure modes shared by every layer above storage.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("not found: {what}")]
    NotFound { what: String },

    #[error("validation: {0}")]
    Validation(String),

    /// Charter / locked decision / baseline writes that did not come from the
    /// user are refused rather than silently applied (V1_DEVELOPMENT.md §9).
    #[error("locked: {0}")]
    Locked(String),

    #[error("drift: {0}")]
    Drift(String),

    #[error("storage failure: {0}")]
    Storage(String),

    #[error("io failure: {0}")]
    Io(String),

    #[error("encoding failure: {0}")]
    Encoding(String),
}

impl From<serde_json::Error> for CoreError {
    fn from(e: serde_json::Error) -> Self {
        CoreError::Encoding(e.to_string())
    }
}

impl From<std::io::Error> for CoreError {
    fn from(e: std::io::Error) -> Self {
        CoreError::Io(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, CoreError>;
