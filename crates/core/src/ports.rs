//! Repository ports. V1_DEVELOPMENT.md §6 makes this the only boundary between
//! project logic and a concrete store, so nothing in this crate (or any crate
//! that depends on it) may mention SQLite, rusqlite or a cloud SDK.
//!
//! The eight ports named in §6 are all here. Three more were added because the
//! documented tables deserve their own contract: [`EvidenceRepository`] and
//! [`AuditRepository`] (see `docs/adr/ADR-总览.html#adr-013`), and [`AdapterRepository`] for
//! `adapter_states` (see `docs/adr/ADR-总览.html#adr-021`).
//!
//! [`DatabaseMaintenance`] is the one port deliberately absent from [`Repositories`]:
//! no project logic reads the database as a file, so only the workbench surface holds it.

use std::path::Path;
use std::sync::Arc;

use vibe_domain::{
    AdapterState, ArchitectureBaseline, Artifact, AuditLog, Conversation, Evidence, EvidenceType,
    Message, Project, ProjectCharter, ProjectEvent, ProjectEventPatch, ScopeBoundary, Source,
    SyncCursor, UserOverride,
};

use crate::error::{CoreError, Result};

/// Re-export so downstream crates can name entity kinds without pulling domain paths.
pub use vibe_domain::EntityKind;

#[derive(Debug, Clone)]
pub struct AppendOutcome {
    pub event: ProjectEvent,
    /// False when an identical event already existed (idempotent re-ingest).
    pub inserted: bool,
}

#[derive(Debug, Clone, Default)]
pub struct MessageInsertOutcome {
    pub inserted: Vec<Message>,
    pub duplicate_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchFilters {
    pub project_id: Option<String>,
    /// Restrict to indexed record kinds: message, event, decision, task, issue, artifact, aep.
    pub kinds: Vec<String>,
    pub limit: usize,
}

impl Default for SearchFilters {
    fn default() -> Self {
        Self {
            project_id: None,
            kinds: Vec::new(),
            limit: 25,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub kind: String,
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub snippet: String,
    pub rank: f64,
}

/// Projects plus the three baselines that guard them from silent drift.
pub trait ProjectRepository: Send + Sync {
    fn create_project(&self, project: &Project) -> Result<()>;
    fn update_project(&self, project: &Project) -> Result<()>;
    fn get_project(&self, id: &str) -> Result<Option<Project>>;
    fn list_projects(&self) -> Result<Vec<Project>>;

    /// Always inserts a new charter version; never overwrites in place (§9).
    fn insert_charter_version(&self, charter: &ProjectCharter) -> Result<()>;
    fn get_active_charter(&self, project_id: &str) -> Result<Option<ProjectCharter>>;
    fn list_charter_versions(&self, project_id: &str) -> Result<Vec<ProjectCharter>>;

    fn insert_scope_version(&self, scope: &ScopeBoundary) -> Result<()>;
    fn get_active_scope(&self, project_id: &str) -> Result<Option<ScopeBoundary>>;
    fn list_scope_versions(&self, project_id: &str) -> Result<Vec<ScopeBoundary>>;

    fn insert_baseline_version(&self, baseline: &ArchitectureBaseline) -> Result<()>;
    fn get_active_baseline(&self, project_id: &str) -> Result<Option<ArchitectureBaseline>>;
    fn list_baseline_versions(&self, project_id: &str) -> Result<Vec<ArchitectureBaseline>>;

    fn save_override(&self, override_: &UserOverride) -> Result<()>;
    fn list_overrides(&self, project_id: &str) -> Result<Vec<UserOverride>>;
}

pub trait EventRepository: Send + Sync {
    /// Idempotent on `dedup_key`. Returning `inserted = false` must hand back the
    /// event that is already stored.
    fn append(&self, event: &ProjectEvent, dedup_key: &str) -> Result<AppendOutcome>;
    /// Accepted events only, in deterministic `seq` order (§18).
    fn list_accepted(&self, project_id: &str, after_seq: Option<i64>) -> Result<Vec<ProjectEvent>>;
    fn list_events(
        &self,
        project_id: &str,
        statuses: Option<&[vibe_domain::EventStatus]>,
        limit: Option<usize>,
    ) -> Result<Vec<ProjectEvent>>;
    fn get_event(&self, id: &str) -> Result<Option<ProjectEvent>>;
    fn patch_event(&self, id: &str, patch: &ProjectEventPatch) -> Result<()>;
    /// True when a dedup key already exists — lets capture skip work entirely.
    fn contains_dedup_key(&self, dedup_key: &str) -> Result<bool>;
}

pub trait ConversationRepository: Send + Sync {
    fn upsert_source(&self, source: &Source) -> Result<()>;
    fn get_source(&self, project_id: &str, id: &str) -> Result<Option<Source>>;
    fn find_source_by_external_id(
        &self,
        project_id: &str,
        source_type: &str,
        external_id: &str,
    ) -> Result<Option<Source>>;
    fn list_sources(&self, project_id: &str) -> Result<Vec<Source>>;

    fn upsert_conversation(&self, conversation: &Conversation) -> Result<()>;
    fn get_conversation(&self, project_id: &str, id: &str) -> Result<Option<Conversation>>;
    fn list_conversations(&self, project_id: &str) -> Result<Vec<Conversation>>;

    /// Insertion is idempotent: external id first, then content hash **per role** within
    /// the conversation (§52 L1 + L2). The key carries no timestamp — a re-push must be
    /// idempotent (§51) while a source time can be absent or jittery, so "same text,
    /// later" is L3's hint, never a deletion.
    fn insert_messages(
        &self,
        conversation_id: &str,
        messages: &[Message],
    ) -> Result<MessageInsertOutcome>;
    fn list_messages(&self, conversation_id: &str) -> Result<Vec<Message>>;
    fn recent_messages(&self, project_id: &str, limit: usize) -> Result<Vec<Message>>;

    fn get_cursor(&self, adapter_id: &str, source_id: &str) -> Result<Option<SyncCursor>>;
    fn set_cursor(&self, cursor: &SyncCursor) -> Result<()>;
}

/// Tasks, issues, decisions, milestones, change proposals, AEPs, drift alerts.
/// Stored as kind + status + JSON payload so a new state object does not need a
/// migration (V1_DEVELOPMENT.md §50).
pub trait EntityRepository: Send + Sync {
    fn upsert(
        &self,
        kind: EntityKind,
        project_id: &str,
        id: &str,
        status: Option<&str>,
        data: &serde_json::Value,
    ) -> Result<()>;
    fn get(&self, kind: EntityKind, id: &str) -> Result<Option<serde_json::Value>>;
    fn list(&self, kind: EntityKind, project_id: &str) -> Result<Vec<serde_json::Value>>;
    fn delete(&self, kind: EntityKind, id: &str) -> Result<()>;

    fn add_link(
        &self,
        from_kind: EntityKind,
        from_id: &str,
        to_kind: EntityKind,
        to_id: &str,
        relation: &str,
    ) -> Result<()>;
    fn links_from(&self, from_kind: EntityKind, from_id: &str) -> Result<Vec<(EntityKind, String, String)>>;

    fn record_version(
        &self,
        kind: EntityKind,
        id: &str,
        version: u32,
        before: Option<&serde_json::Value>,
        after: &serde_json::Value,
        reason: &str,
    ) -> Result<()>;
}

/// Typed helpers over [`EntityRepository`] so callers never hand-decode JSON.
pub trait EntityStore {
    fn entities_of<T: serde::de::DeserializeOwned>(
        &self,
        kind: EntityKind,
        project_id: &str,
    ) -> Result<Vec<T>>;
    fn entity<T: serde::de::DeserializeOwned>(
        &self,
        kind: EntityKind,
        id: &str,
    ) -> Result<Option<T>>;
    fn put_entity<T: serde::Serialize + crate::identified::Identified>(
        &self,
        kind: EntityKind,
        entity: &T,
        status: Option<&str>,
    ) -> Result<()>;
}

impl<R: EntityRepository + ?Sized> EntityStore for R {
    fn entities_of<T: serde::de::DeserializeOwned>(
        &self,
        kind: EntityKind,
        project_id: &str,
    ) -> Result<Vec<T>> {
        self.list(kind, project_id)?
            .iter()
            .map(|v| serde_json::from_value(v.clone()).map_err(CoreError::from))
            .collect()
    }

    fn entity<T: serde::de::DeserializeOwned>(
        &self,
        kind: EntityKind,
        id: &str,
    ) -> Result<Option<T>> {
        match self.get(kind, id)? {
            Some(v) => Ok(Some(serde_json::from_value(v).map_err(CoreError::from)?)),
            None => Ok(None),
        }
    }

    fn put_entity<T: serde::Serialize + crate::identified::Identified>(
        &self,
        kind: EntityKind,
        entity: &T,
        status: Option<&str>,
    ) -> Result<()> {
        let value = serde_json::to_value(entity).map_err(CoreError::from)?;
        self.upsert(
            kind,
            entity.project_id(),
            entity.id(),
            status,
            &value,
        )
    }
}

pub trait ArtifactRepository: Send + Sync {
    /// Path / hash / kind / version only — file bodies never enter the database (§22).
    fn upsert_artifact(&self, artifact: &Artifact) -> Result<()>;
    fn get_artifact(&self, project_id: &str, id: &str) -> Result<Option<Artifact>>;
    fn find_artifact_by_path(&self, project_id: &str, path: &str) -> Result<Option<Artifact>>;
    fn list_artifacts(&self, project_id: &str) -> Result<Vec<Artifact>>;
}

pub trait EvidenceRepository: Send + Sync {
    fn insert_evidence(&self, evidence: &Evidence) -> Result<()>;
    fn get_evidence(&self, id: &str) -> Result<Option<Evidence>>;
    fn list_evidence(&self, project_id: &str) -> Result<Vec<Evidence>>;
    fn find_by_ref(&self, r#type: EvidenceType, ref_id: &str) -> Result<Option<Evidence>>;
    fn link(&self, evidence_id: &str, subject_kind: EntityKind, subject_id: &str) -> Result<()>;
    fn for_subject(&self, subject_kind: EntityKind, subject_id: &str) -> Result<Vec<Evidence>>;
}

pub trait SnapshotRepository: Send + Sync {
    fn save(&self, snapshot: &vibe_domain::Snapshot) -> Result<()>;
    fn get(&self, id: &str) -> Result<Option<vibe_domain::Snapshot>>;
    fn list(&self, project_id: &str) -> Result<Vec<vibe_domain::Snapshot>>;
    fn latest(&self, project_id: &str) -> Result<Option<vibe_domain::Snapshot>>;
    fn save_project_map(&self, map: &vibe_domain::ProjectMap, snapshot_id: Option<&str>) -> Result<()>;
    fn list_project_maps(&self, project_id: &str) -> Result<Vec<vibe_domain::ProjectMap>>;
    fn latest_project_map(&self, project_id: &str) -> Result<Option<vibe_domain::ProjectMap>>;
}

/// Structured (L1), full-text (L2) and relation (L3) retrieval per §40.
/// Implementations are store-specific; logic only ever sees this port.
pub trait SearchIndex: Send + Sync {
    fn index(&self, kind: &str, id: &str, project_id: &str, title: &str, body: &str) -> Result<()>;
    fn remove(&self, kind: &str, id: &str) -> Result<()>;
    fn search(&self, query: &str, filters: &SearchFilters) -> Result<Vec<SearchHit>>;
    fn related(&self, kind: &str, id: &str) -> Result<Vec<SearchHit>>;
}

/// What each adapter's last run did to it (§51 partial failure, Stage H Adapter Health,
/// §62 Degradation). Keyed by adapter id alone, because a source that is not on this
/// machine is not on it for any project.
pub trait AdapterRepository: Send + Sync {
    /// The verdict replaces the previous one rather than appending history.
    fn record_state(&self, state: &AdapterState) -> Result<()>;
    fn get_state(&self, adapter_id: &str) -> Result<Option<AdapterState>>;
    fn list_states(&self) -> Result<Vec<AdapterState>>;
}

pub trait SettingsRepository: Send + Sync {
    fn get_json(&self, key: &str) -> Result<Option<serde_json::Value>>;
    fn set_json(&self, key: &str, value: &serde_json::Value) -> Result<()>;
}

pub trait AuditRepository: Send + Sync {
    fn append(&self, entry: &AuditLog) -> Result<()>;
    fn list(&self, project_id: &str, limit: usize) -> Result<Vec<AuditLog>>;
}

/// Everything a use-case service needs, as borrowed trait objects. Keeps services
/// free of `Arc` plumbing while staying testable with in-memory fakes.
#[derive(Clone)]
pub struct Repositories {
    pub projects: Arc<dyn ProjectRepository>,
    pub events: Arc<dyn EventRepository>,
    pub conversations: Arc<dyn ConversationRepository>,
    pub entities: Arc<dyn EntityRepository>,
    pub artifacts: Arc<dyn ArtifactRepository>,
    pub evidence: Arc<dyn EvidenceRepository>,
    pub snapshots: Arc<dyn SnapshotRepository>,
    pub search: Arc<dyn SearchIndex>,
    pub settings: Arc<dyn SettingsRepository>,
    pub audit: Arc<dyn AuditRepository>,
    pub adapters: Arc<dyn AdapterRepository>,
}

/// What a database file claims to be (§55 Recovery). `integrity` is SQLite's own verdict
/// — the string `"ok"` or its first complaint — so a backup can be proved whole before
/// anything is allowed to depend on it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupReport {
    pub pages: i64,
    pub page_size: i64,
    pub schema_version: i64,
    pub events: i64,
    pub projects: i64,
    pub integrity: String,
}

impl BackupReport {
    /// A restore is only safe against a file this build can read *and* that is whole.
    pub fn restorable(&self, known_version: i64) -> std::result::Result<(), String> {
        if self.schema_version > known_version {
            return Err(format!(
                "backup is schema version {}, this build knows {known_version}",
                self.schema_version
            ));
        }
        if self.integrity != "ok" {
            return Err(format!("backup failed its integrity check: {}", self.integrity));
        }
        Ok(())
    }
}

/// Database upkeep the workbench surface needs but no project logic does: backup, verify,
/// restore. Paths are the only storage concept that crosses this port — what lives behind
/// them stays the store's business (§7.5), including which schema version this build knows.
pub trait DatabaseMaintenance: Send + Sync {
    /// Copy the live database into `dest` without closing it, so a WAL workbench stays
    /// readable while it is being backed up. The report describes the copy.
    fn backup_to(&self, dest: &Path) -> Result<BackupReport>;
    fn integrity_check(&self) -> Result<String>;
    /// Read a backup file for what it says about itself, touching nothing.
    fn inspect(&self, source: &Path) -> Result<BackupReport>;
    /// Replace the live database's content with a backup's, refusing a file that is not a
    /// workbench database, is newer than this build, or fails its integrity check. An older
    /// schema is migrated forward afterwards, because the copied pages carry the backup's
    /// schema rather than this build's.
    fn restore(&self, source: &Path) -> Result<BackupReport>;
    /// The newest schema version this build can create.
    fn schema_version(&self) -> Result<i64>;
}
