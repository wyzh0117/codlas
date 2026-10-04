//! Project logic and its storage boundary. This crate must stay free of any
//! concrete database, browser or agent SDK (PROJECT_MASTER §7.5).

pub mod error;
pub mod event_store;
pub mod identified;
pub mod judge;
pub mod ports;
pub mod project_service;
pub mod resolution;
pub mod settings;

pub use error::{CoreError, Result};
pub use event_store::{EventDraft, EventStore};
pub use identified::Identified;
pub use judge::{
    route_for, CaptureSignal, JudgedSignal, JudgeRoute, JudgeThresholds, LlmJudge, RuleJudge,
    SignalJudge,
};
pub use ports::{
    AppendOutcome, ArtifactRepository, AuditRepository, ConversationRepository, EntityRepository,
    EntityStore, EventRepository, MessageInsertOutcome, ProjectRepository, Repositories, SearchFilters,
    SearchHit, SearchIndex, SettingsRepository, SnapshotRepository, EvidenceRepository,
};
pub use project_service::{BaselineDraft, CharterDraft, ProjectService};
pub use resolution::{EntityResolver, Resolved, AUTO_MERGE_CONFIDENCE};
pub use settings::SettingsService;
