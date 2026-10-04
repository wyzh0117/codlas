//! Acceptance for the §26 drift rules (V1_DEVELOPMENT.md §26, PROJECT_MASTER §8.6).
//!
//! Inputs are hand-built `ProjectState` values plus events appended through the
//! ports — the guard engine never reaches into the reducer.

mod support;

use chrono::{Duration, TimeZone, Utc};
use serde_json::json;
use vibe_core::event_store::EventDraft;
use vibe_core::ports::EntityStore;
use vibe_domain::event_catalog;
use vibe_domain::*;

use support::{fixture as open, scope, state, task, Fixture};

fn fixture() -> Fixture {
    open("Guard Engine")
}

/// 1. forbidden work is prompted, in-boundary work is not.
#[test]
fn scope_drift_reads_the_boundary_and_only_the_boundary() {
    let f = fixture();
    let mut s = state(&f.project_id);
    let mut boundary = scope(&f.project_id);
    boundary.in_scope = vec!["事件溯源状态投影".into()];
    boundary.forbidden = vec!["多用户登录".into()];
    s.scope = Some(boundary);
    s.tasks = vec![task("t-1", &f.project_id, "支持多用户登录")];

    let alerts = f.engine.scope_drift(&s);
    assert_eq!(alerts.len(), 1, "one forbidden phrase, one prompt: {alerts:?}");
    let alert = &alerts[0];
    assert_eq!(alert.r#type, DriftType::ScopeDrift);
    assert_eq!(alert.severity, DriftSeverity::Warning);
    assert_eq!(alert.status, DriftStatus::Open);
    assert_eq!(alert.baseline_refs, vec!["scope:v1:forbidden:多用户登录"]);
    assert_eq!(alert.evidence_refs, vec!["t-1".to_string()]);

    // The very same task now sits inside the boundary: nothing to say.
    let mut moved = scope(&f.project_id);
    moved.in_scope = vec!["事件溯源状态投影".into(), "多用户登录".into()];
    s.scope = Some(moved);
    assert!(f.engine.scope_drift(&s).is_empty());

    // Out of scope is softer than forbidden.
    let mut softer = scope(&f.project_id);
    softer.out_of_scope = vec!["导出 PDF 报告".into()];
    s.scope = Some(softer);
    s.tasks = vec![task("t-2", &f.project_id, "新增导出 PDF 报告")];
    let alerts = f.engine.scope_drift(&s);
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].severity, DriftSeverity::Info);
}

/// 2. a contradicting statement against an Active decision, and silence once it is not.
#[test]
fn decision_conflict_needs_a_live_decision() {
    let f = fixture();
    let decided = Utc.with_ymd_and_hms(2026, 6, 1, 9, 0, 0).unwrap();
    let later = Utc.with_ymd_and_hms(2026, 7, 1, 9, 0, 0).unwrap();
    let mut s = state(&f.project_id);
    s.decisions = vec![
        Decision {
            id: "d-1".to_string(),
            project_id: f.project_id.clone(),
            title: "存储访问".to_string(),
            decision: "Core 不直接依赖 SQLite".to_string(),
            status: DecisionStatus::Active,
            decided_at: Some(decided),
            ..Default::default()
        },
        Decision {
            id: "d-2".to_string(),
            project_id: f.project_id.clone(),
            title: "存储简化".to_string(),
            decision: "Core 直接调用 SQLite".to_string(),
            status: DecisionStatus::Active,
            decided_at: Some(later),
            ..Default::default()
        },
    ];

    let alerts = f.engine.decision_conflict(&s);
    assert_eq!(alerts.len(), 1, "the newer statement contradicts d-1: {alerts:?}");
    assert_eq!(alerts[0].r#type, DriftType::DecisionConflict);
    assert_eq!(alerts[0].severity, DriftSeverity::Warning);
    assert_eq!(alerts[0].baseline_refs[0], "decision:d-1:active");
    assert_eq!(alerts[0].evidence_refs[0], "d-2");

    s.decisions[0].status = DecisionStatus::Superseded;
    assert!(
        f.engine.decision_conflict(&s).is_empty(),
        "a superseded decision no longer guards the baseline"
    );
}

/// 3. invariants are checked by where an artifact lives and by scan_source.
#[test]
fn architecture_drift_knows_which_layer_may_hold_the_dependency() {
    let f = fixture();
    let mut s = state(&f.project_id);
    s.baseline = Some(ArchitectureBaseline {
        id: "base-1".to_string(),
        project_id: f.project_id.clone(),
        summary: "Core 只依赖端口，Storage 实现持久化".to_string(),
        layers: vec![
            ArchitectureLayer {
                id: "l-core".to_string(),
                name: "Core".to_string(),
                responsibility: Some("业务规则与端口定义".to_string()),
            },
            ArchitectureLayer {
                id: "l-storage".to_string(),
                name: "Storage".to_string(),
                responsibility: Some("SQLite 持久化实现".to_string()),
            },
        ],
        invariants: vec![ArchitectureInvariant {
            id: "inv-1".to_string(),
            text: "Core 不直接访问 SQLite".to_string(),
            detect: vec!["rusqlite".to_string(), "sqlite".to_string()],
        }],
        replaceable_parts: vec![],
        version: 1,
        active: true,
    });
    s.artifacts = vec![Artifact {
        id: "a-1".to_string(),
        project_id: f.project_id.clone(),
        name: "db.rs".to_string(),
        kind: ArtifactKind::SourceFile,
        path: Some("crates/core/src/db.rs".to_string()),
        ..Default::default()
    }];

    let alerts = f.engine.architecture_drift(&s);
    assert_eq!(alerts.len(), 1, "{alerts:?}");
    assert_eq!(alerts[0].r#type, DriftType::ArchitectureDrift);
    assert_eq!(alerts[0].severity, DriftSeverity::High);
    assert_eq!(alerts[0].baseline_refs, vec!["baseline:v1:invariant:inv-1"]);
    assert_eq!(alerts[0].evidence_refs[0], "a-1");

    // The dependency is allowed where the baseline says it lives.
    s.artifacts = vec![Artifact {
        id: "a-2".to_string(),
        project_id: f.project_id.clone(),
        name: "db.rs".to_string(),
        kind: ArtifactKind::SourceFile,
        path: Some("crates/storage/src/db.rs".to_string()),
        ..Default::default()
    }];
    assert!(f.engine.architecture_drift(&s).is_empty());

    // §26.3's real signal: the source text itself.
    let core = vec![Artifact {
        id: "a-1".to_string(),
        project_id: f.project_id.clone(),
        name: "lib.rs".to_string(),
        kind: ArtifactKind::SourceFile,
        path: Some("crates/core/src/lib.rs".to_string()),
        ..Default::default()
    }];
    s.artifacts = core;
    let found = f.engine.scan_source(
        &s,
        "crates/core/src/lib.rs",
        "use vibe_domain::Task;\nlet conn = rusqlite::Connection::open(path);\n",
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].severity, DriftSeverity::High);
    assert_eq!(found[0].evidence_refs[0], "file:crates/core/src/lib.rs:2");
    assert_eq!(found[0].baseline_refs, vec!["baseline:v1:invariant:inv-1"]);

    // Same line inside the storage layer is not drift.
    s.artifacts = vec![Artifact {
        id: "a-2".to_string(),
        project_id: f.project_id.clone(),
        name: "lib.rs".to_string(),
        kind: ArtifactKind::SourceFile,
        path: Some("crates/storage/src/lib.rs".to_string()),
        ..Default::default()
    }];
    assert!(f
        .engine
        .scan_source(&s, "crates/storage/src/lib.rs", "rusqlite::Connection::open(path)")
        .is_empty());
}

/// 4. new capability without a link back to the design waits in the Inbox.
#[test]
fn feature_creep_is_only_a_prompt_when_something_covers_it() {
    let f = fixture();
    let stamp = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let mut s = state(&f.project_id);
    s.charter = Some(ProjectCharter {
        id: "c-1".to_string(),
        project_id: f.project_id.clone(),
        original_problem: "vibe coding 会丢失原始设计".to_string(),
        target_user: None,
        core_value: "反偏航".to_string(),
        must_have: vec!["防偏航提醒".to_string()],
        wont_have: vec![],
        success_criteria: vec![],
        architecture_principles: vec![],
        constraints: vec![],
        version: 1,
        locked: false,
        created_at: stamp,
        updated_at: stamp,
    });
    let mut boundary = scope(&f.project_id);
    boundary.in_scope = vec!["事件溯源状态投影".to_string()];
    s.scope = Some(boundary);
    s.tasks = vec![task("t-9", &f.project_id, "增加深色模式切换")];

    let alerts = f.engine.feature_creep(&s);
    assert_eq!(alerts.len(), 1, "{alerts:?}");
    assert_eq!(alerts[0].r#type, DriftType::FeatureCreep);
    assert_eq!(alerts[0].severity, DriftSeverity::Info);
    assert!(alerts[0]
        .baseline_refs
        .contains(&"charter:v1:mustHave".to_string()));

    // An open change proposal that says the same thing is a legitimate reason.
    s.change_proposals = vec![ChangeProposal {
        id: "cp-1".to_string(),
        project_id: f.project_id.clone(),
        proposed_change: "支持深色模式切换".to_string(),
        reason: "用户想要".to_string(),
        status: ChangeProposalStatus::Open,
        ..Default::default()
    }];
    assert!(f.engine.feature_creep(&s).is_empty());
}

/// 5. three redesigns of one object is churn, not iteration.
#[test]
fn repeated_redesign_fires_at_three() {
    let f = fixture();
    let mut s = state(&f.project_id);
    s.artifacts = vec![Artifact {
        id: "a-3".to_string(),
        project_id: f.project_id.clone(),
        name: "sync.rs".to_string(),
        path: Some("crates/sync/src/sync.rs".to_string()),
        redesign_count: 3,
        ..Default::default()
    }];
    let alerts = f.engine.repeated_redesign(&s);
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].r#type, DriftType::RepeatedRedesign);
    assert_eq!(alerts[0].severity, DriftSeverity::Warning);
    assert!(!alerts[0].baseline_refs.is_empty());

    s.artifacts[0].redesign_count = 2;
    assert!(f.engine.repeated_redesign(&s).is_empty());
}

/// 6. the clock is injected, so the rule is testable rather than flaky.
#[test]
fn stale_assumption_is_deterministic_with_injected_now() {
    let f = fixture();
    let decided = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let mut s = state(&f.project_id);
    s.decisions = vec![Decision {
        id: "d-old".to_string(),
        project_id: f.project_id.clone(),
        title: "本地优先".to_string(),
        decision: "所有数据只存本机".to_string(),
        status: DecisionStatus::Active,
        decided_at: Some(decided),
        ..Default::default()
    }];
    f.events
        .append(
            EventDraft::new(f.project_id.clone(), event_catalog::TASK_CREATED)
                .user()
                .subject("decision", "d-old")
                .payload(json!({ "title": "本地优先" })),
        )
        .unwrap();

    let much_later = decided + Duration::days(120);
    let alerts = f.engine.stale_assumption(&s, much_later);
    assert_eq!(alerts.len(), 1, "{alerts:?}");
    assert_eq!(alerts[0].r#type, DriftType::StaleAssumption);
    assert_eq!(alerts[0].severity, DriftSeverity::Info);
    assert_eq!(alerts[0].evidence_refs[0], "d-old");
    assert_eq!(alerts[0].baseline_refs, vec!["decision:d-old:active"]);

    let still_fresh = decided + Duration::days(30);
    assert!(f.engine.stale_assumption(&s, still_fresh).is_empty());
}

/// 7. running the rules twice must not say the same thing twice.
#[test]
fn check_is_idempotent_and_never_resurrects_a_judged_alert() {
    let f = fixture();
    let mut s = state(&f.project_id);
    let mut boundary = scope(&f.project_id);
    boundary.forbidden = vec!["多用户登录".to_string()];
    s.scope = Some(boundary);
    s.tasks = vec![task("t-1", &f.project_id, "支持多用户登录")];

    let first = f.engine.check(&s).unwrap();
    assert_eq!(first.len(), 1);
    let second = f.engine.check(&s).unwrap();
    assert!(second.is_empty(), "no second copy of the same finding");

    let detected = detected_events(&f);
    assert_eq!(detected.len(), 1, "one DRIFT_DETECTED for one finding");
    let open = f
        .engine
        .list(&f.project_id)
        .unwrap()
        .into_iter()
        .filter(|a| a.status == DriftStatus::Open)
        .collect::<Vec<_>>();
    assert_eq!(open.len(), 1);

    // The user dismissed it; the same state must not bring it back.
    let resolved = f
        .engine
        .resolve(&f.project_id, &open[0].id, vibe_guard_engine::DriftAction::Dismiss, "不是偏航")
        .unwrap();
    assert_eq!(resolved.status, DriftStatus::Dismissed);
    assert!(f.engine.check(&s).unwrap().is_empty());
    assert_eq!(detected_events(&f).len(), 1);
    let stored = f
        .repos
        .entities
        .entity::<DriftAlert>(EntityKind::DriftAlert, &open[0].id)
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, DriftStatus::Dismissed);
}

fn detected_events(f: &Fixture) -> Vec<ProjectEvent> {
    f.events
        .list(&f.project_id, None, None)
        .unwrap()
        .into_iter()
        .filter(|e| e.r#type == event_catalog::DRIFT_DETECTED)
        .collect()
}
