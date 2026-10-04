//! Stage H "Adapter Health": every run leaves a verdict behind, so §62 Degradation —
//! "an adapter is broken and the workbench keeps working" — is answerable after the run
//! rather than only while it is happening.
//!
//! The four states and what each one costs the user are the point of the table: an
//! unavailable source is not a failure, and a source whose material landed but whose
//! cursor did not is not either.

use std::sync::Arc;

use vibe_core::error::CoreError;
use vibe_core::ports::Repositories;
use vibe_domain::{
    AgentCapabilities, AdapterHealth, Capability, NormalizedConversationBatch, SourceType,
    SyncCursor,
};
use vibe_ingestion::{
    report_health, AdapterReport, AgentAdapter, IngestPipeline, ManualImportAdapter,
};
use vibe_storage::SqliteStore;

fn fixture() -> (Repositories, String) {
    let store = SqliteStore::open_in_memory().expect("open");
    let repositories = store.repositories();
    let project = vibe_core::project_service::ProjectService::new(repositories.clone())
        .create_project("health", None, None, None)
        .expect("project")
        .id;
    (repositories, project)
}

fn verdict(repositories: &Repositories, adapter_id: &str) -> AdapterHealth {
    repositories
        .adapters
        .get_state(adapter_id)
        .expect("read")
        .unwrap_or_else(|| panic!("no verdict recorded for {adapter_id}"))
        .health
}

/// detect() true, ingest() always fails — the §51 partial-failure shape.
struct Broken;

impl AgentAdapter for Broken {
    fn id(&self) -> &str {
        "broken-cli"
    }
    fn source_type(&self) -> SourceType {
        SourceType::GenericCli
    }
    fn detect(&self) -> Result<bool, CoreError> {
        Ok(true)
    }
    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            adapter_id: "broken-cli".into(),
            capabilities: vec![Capability::ReadHistory],
        }
    }
    fn ingest(
        &self,
        _cursor: Option<&SyncCursor>,
    ) -> Result<NormalizedConversationBatch, CoreError> {
        Err(CoreError::Io("session file vanished".into()))
    }
}

#[test]
fn a_clean_run_leaves_an_ok_verdict() {
    let (repositories, project) = fixture();
    let mut pipeline = IngestPipeline::with_rules(repositories.clone());
    pipeline.register(Arc::new(ManualImportAdapter::named(
        "healthy",
        "user: 确认保留 Repository 抽象层",
    )));

    pipeline.run(&project).expect("run");

    assert_eq!(verdict(&repositories, "healthy"), AdapterHealth::Ok);
    let state = repositories
        .adapters
        .get_state("healthy")
        .expect("read")
        .expect("recorded");
    assert_eq!(
        state.detail, None,
        "an ok verdict carries no complaint"
    );
}

#[test]
fn a_source_that_is_not_here_is_unavailable_not_failed() {
    let (repositories, project) = fixture();
    let mut pipeline = IngestPipeline::with_rules(repositories.clone());
    pipeline.register(Arc::new(ManualImportAdapter::named(
        "healthy",
        "user: 确认保留 事件溯源",
    )));
    pipeline.register(Arc::new(ManualImportAdapter::file_as(
        "gone",
        "/definitely/not/here/transcript.txt",
    )));

    let report = pipeline.run(&project).expect("run");

    assert_eq!(
        report.per_adapter.len(),
        1,
        "an absent source contributes nothing to the run it is skipped from"
    );
    assert!(report.failures().is_empty(), "{:?}", report.failures());
    assert_eq!(verdict(&repositories, "gone"), AdapterHealth::Unavailable);
    assert_eq!(verdict(&repositories, "healthy"), AdapterHealth::Ok);
}

#[test]
fn a_failing_adapter_keeps_its_error_in_the_verdict() {
    let (repositories, project) = fixture();
    let mut pipeline = IngestPipeline::with_rules(repositories.clone());
    pipeline.register(Arc::new(Broken));
    pipeline.register(Arc::new(ManualImportAdapter::named(
        "healthy",
        "user: 确认保留 三源并行采集",
    )));

    pipeline.run(&project).expect("run");

    assert_eq!(verdict(&repositories, "broken-cli"), AdapterHealth::Failed);
    // The other source is unaffected: §51 partial failure, now visible afterwards.
    assert_eq!(verdict(&repositories, "healthy"), AdapterHealth::Ok);

    // Broken first: the listing exists to show what is wrong.
    let listed = repositories.adapters.list_states().expect("list");
    assert_eq!(listed[0].adapter_id, "broken-cli");
    assert_eq!(listed[0].health, AdapterHealth::Failed);
    assert!(
        listed[0]
            .detail
            .as_deref()
            .unwrap_or_default()
            .contains("session file vanished"),
        "the verdict says why: {:?}",
        listed[0].detail
    );
}

#[test]
fn a_second_run_replaces_the_verdict_rather_than_stacking_history() {
    let (repositories, project) = fixture();
    let mut pipeline = IngestPipeline::with_rules(repositories.clone());
    pipeline.register(Arc::new(Broken));
    pipeline.run(&project).expect("first");
    pipeline.run(&project).expect("second");

    let listed = repositories.adapters.list_states().expect("list");
    assert_eq!(listed.len(), 1, "one adapter, one row");
    assert_eq!(listed[0].health, AdapterHealth::Failed);
}

/// The mapping itself, including the case no fake adapter can reach cheaply: material
/// committed and *then* the cursor write failed.
#[test]
fn material_that_landed_with_a_broken_cursor_is_degraded() {
    let landed = AdapterReport {
        adapter_id: "partial".into(),
        messages_inserted: 3,
        error: Some("cursor write failed: disk full".into()),
        ..Default::default()
    };
    assert_eq!(report_health(&landed), AdapterHealth::Degraded);

    let nothing = AdapterReport {
        adapter_id: "nothing".into(),
        error: Some("session file vanished".into()),
        ..Default::default()
    };
    assert_eq!(report_health(&nothing), AdapterHealth::Failed);

    // Idempotency (§51) means an unchanged source legitimately reports zero rows.
    let quiet = AdapterReport {
        adapter_id: "quiet".into(),
        ..Default::default()
    };
    assert_eq!(report_health(&quiet), AdapterHealth::Ok);
}
