//! The shell's own acceptance (§61 Stage E): every screen is compiled from a real store, and
//! the first screen is the diff — what moved, then what the user agrees to lock in.
//!
//! These tests call `wp_desktop::views` directly rather than driving a webview, because the
//! shell's promise is exactly that its screens are what the ports say, and a window is only how
//! they reach the eye.

use serde_json::json;
use vibe_cli::app::App;
use vibe_core::event_store::{EventDraft, EventStore};
use vibe_core::ports::EntityStore;
use vibe_core::project_service::ProjectService;
use vibe_domain::{ActorType, ChangeProposal, ChangeProposalStatus, EntityKind, ScopeSection};
use wp_desktop::views::{self, PendingChoice};
use vibe_guard_engine::DriftEngine;
use vibe_state_engine::{Reducer, SnapshotService};

struct Bench {
    app: App,
    dir: tempfile::TempDir,
    project_id: String,
    projects: ProjectService,
}

fn bench() -> Bench {
    let dir = tempfile::tempdir().expect("temp dir");
    let app = App::open(&dir.path().join("workbench.db")).expect("open the workbench store");
    let projects = ProjectService::new(app.repos.clone());
    let project = projects
        .create_project(
            "工作台",
            Some("本地优先的个人项目工作台".into()),
            Some("/tmp/repo".into()),
            None,
        )
        .expect("create project");
    app.remember_project(&project.id).expect("remember the pointer");
    Bench {
        app,
        dir,
        projects,
        project_id: project.id,
    }
}

impl Bench {
    fn task(&self, id: &str, title: &str) {
        EventStore::new(self.app.repos.clone())
            .append(
                EventDraft::new(self.project_id.clone(), vibe_domain::event_catalog::TASK_CREATED)
                    .user()
                    .subject("task", id)
                    .payload(json!({ "taskId": id, "title": title })),
            )
            .expect("append task");
    }

    fn snapshot(&self, label: &str) {
        SnapshotService::new(self.app.repos.clone())
            .take(&self.project_id, label)
            .expect("take snapshot");
    }

    fn proposal(&self, change: &str, item: &str) -> String {
        self.projects
            .open_change_proposal(
                &self.project_id,
                ChangeProposal {
                    id: String::new(),
                    project_id: self.project_id.clone(),
                    proposed_change: change.to_string(),
                    reason: "离线也要能接续工作".into(),
                    delta: Some(json!({
                        "appliesTo": "scope",
                        "add": [{ "section": "deferred", "item": item }],
                    })),
                    ..Default::default()
                },
                ActorType::Agent,
            )
            .expect("open proposal")
            .id
    }
}

/// ①: the screen the user meets first is what moved since the last snapshot, not a todo list.
#[test]
fn the_first_screen_is_the_diff_since_the_last_snapshot() {
    let f = bench();
    f.task("t-1", "搭起事件链");
    f.snapshot("round-1");
    f.task("t-2", "渲染 diff 首屏");

    let view = views::diff_view(&f.app, &f.project_id).expect("diff view");

    assert_eq!(view.from_snapshot, "round-1", "the baseline is named, not assumed");
    assert!(view.note.is_none(), "a real snapshot leaves no apologetic note");
    assert!(!view.to_snapshot.is_empty());

    let added = view
        .sections
        .iter()
        .find(|card| card.name == "任务" && card.change == "added")
        .expect("the new task reads as an added module");
    assert_eq!(added.entries.len(), 1, "{added:?}");
    assert!(added.entries[0].text.contains("渲染 diff 首屏"), "{added:?}");
    assert!(
        !view
            .sections
            .iter()
            .any(|card| card.name == "任务" && card.change == "added"
                && card.entries[0].text.contains("搭起事件链")),
        "what was already snapshotted is the baseline, not a change"
    );

    // The screen also says what is *not* a change: the read-only note is the shell's own words,
    // and no pending entry hides a folded fact.
    assert!(view.pending.is_empty(), "{:?}", view.pending);
}

/// 决定哪些变动生效: the tick is §7.4's confirmation, and it has to reach the same resolver
/// `wp inbox resolve` uses — including the proposal's machine-applicable delta.
#[test]
fn ticking_a_proposal_locks_in_the_change_it_proposes() {
    let f = bench();
    f.projects
        .update_scope(
            &f.project_id,
            vec![(ScopeSection::Forbidden, "多人协作".into())],
            vec![],
            ActorType::User,
        )
        .expect("seed the boundary");
    let proposal = f.proposal("把「多人协作」挪进暂缓区", "多人协作");

    let before = views::diff_view(&f.app, &f.project_id).expect("diff");
    assert_eq!(before.pending.len(), 1, "{:?}", before.pending);
    let row = &before.pending[0];
    assert_eq!(row.id, proposal, "the row on screen addresses the record itself");
    assert_eq!(row.source, "proposal");
    assert_eq!(row.kind, "change_proposal");
    assert_eq!(row.actions, vec!["Accept".to_string(), "Reject".to_string()]);

    let result = views::apply(
        &f.app,
        &f.project_id,
        &[PendingChoice {
            id: row.id.clone(),
            kind: row.kind.clone(),
            action: "Accept".into(),
        }],
    );
    assert_eq!(result.applied, vec![row.id.clone()], "{result:?}");
    assert!(result.refused.is_empty(), "{result:?}");

    let stored = f
        .app
        .repos
        .entities
        .entity::<ChangeProposal>(EntityKind::ChangeProposal, &row.id)
        .expect("read back")
        .expect("the proposal is still there");
    assert_eq!(stored.status, ChangeProposalStatus::Accepted, "{stored:?}");
    assert!(stored.delta.is_some(), "the delta survived the write: {stored:?}");

    let scope = f.projects.scope(&f.project_id).expect("scope").expect("scope");
    assert_eq!(scope.version, 2, "the accepted delta moved the boundary to a new version");
    assert!(scope.deferred.iter().any(|item| item == "多人协作"), "{scope:?}");

    // Once it is decided it is no longer waiting, so the next compile of the screen drops it.
    let after = views::diff_view(&f.app, &f.project_id).expect("diff again");
    assert!(after.pending.is_empty(), "{:?}", after.pending);
    let folded = Reducer::new(f.app.repos.clone())
        .project_read_only(&f.project_id)
        .expect("fold");
    assert_eq!(
        folded
            .change_proposals
            .iter()
            .filter(|p| p.status == vibe_domain::ChangeProposalStatus::Accepted)
            .count(),
        1
    );
}

/// A verb the record does not offer is refused with a reason — never half-applied, never silent.
#[test]
fn apply_refuses_what_the_record_does_not_offer() {
    let f = bench();
    let proposal = f.proposal("把地图存成缓存视图", "地图缓存");

    let result = views::apply(
        &f.app,
        &f.project_id,
        &[
            PendingChoice {
                id: proposal.clone(),
                kind: "change_proposal".into(),
                action: "Banana".into(),
            },
            PendingChoice {
                id: proposal.clone(),
                kind: "not_a_kind".into(),
                action: "Accept".into(),
            },
        ],
    );
    assert!(result.applied.is_empty(), "{result:?}");
    assert_eq!(result.refused.len(), 2, "{result:?}");
    assert!(result.refused[0].reason.contains("unknown action"), "{result:?}");
    assert!(result.refused[1].reason.contains("unknown inbox kind"), "{result:?}");

    assert_eq!(
        f.projects.scope(&f.project_id).expect("scope").map(|s| s.version),
        None,
        "a refused verdict moved nothing at all"
    );
}

/// ②: the map is the compiled markdown, section by section — one view, not a second state.
#[test]
fn the_map_screen_renders_the_compiled_sections() {
    let f = bench();
    f.task("t-1", "搭起事件链");

    let map = views::map_view(&f.app, &f.project_id).expect("map");
    assert_eq!(map.sections.len(), vibe_state_engine::MAP_SECTIONS.len());
    assert!(map.sections.iter().any(|s| s.name == "Active Tasks"));
    assert!(
        map.sections
            .iter()
            .any(|s| s.name == "Active Tasks" && s.lines.iter().any(|l| l.contains("搭起事件链"))),
        "{:?}",
        map.sections
    );
    let empty = map.sections.iter().find(|s| s.empty).expect("an empty section");
    assert_eq!(empty.lines[0], vibe_state_engine::EMPTY_PLACEHOLDER);
    assert!(!map.state_hash.is_empty());
    assert!(map.compiled_at > 0);
}

/// The traceability screen: accepted events, newest first, in the payload's own words.
#[test]
fn the_timeline_screen_is_the_accepted_log() {
    let f = bench();
    f.task("t-1", "搭起事件链");
    f.task("t-2", "渲染 diff 首屏");
    // A pending event is evidence, not a fact: the shell's timeline shows accepted rows only.
    EventStore::new(f.app.repos.clone())
        .append(
            EventDraft::new(
                f.project_id.clone(),
                vibe_domain::event_catalog::DECISION_PROPOSED,
            )
            .agent()
            .pending()
            .confidence(0.6)
            .payload(json!({ "excerpt": "建议用 FTS5" })),
        )
        .expect("append pending");

    let log = views::timeline_view(&f.app, &f.project_id, 50).expect("timeline");
    // Opening a project is itself a recorded fact, so the log starts at seq 1 rather than at
    // the first task; a pending event stays out because nothing was decided about it yet.
    assert_eq!(log.events.len(), 3, "{:?}", log.events);
    assert_eq!(log.events[0].seq, 3, "newest first");
    assert_eq!(log.events[0].status, "accepted");
    assert!(log.events[0].summary.contains("渲染 diff 首屏"), "{:?}", log.events[0]);
    assert_eq!(log.events[0].kind, vibe_domain::event_catalog::TASK_CREATED);
    assert_eq!(log.events[2].kind, vibe_domain::event_catalog::PROJECT_CREATED);
}

/// The drift screen reads the same rules `wp drift check` ran, and the inbox rows point back at
/// the screen where the user decides.
#[test]
fn the_drift_screen_shows_what_the_rules_found() {
    let f = bench();
    f.projects
        .update_scope(
            &f.project_id,
            vec![(ScopeSection::Forbidden, "多人协作".into())],
            vec![],
            ActorType::User,
        )
        .expect("seed the boundary");
    f.task("t-1", "支持多人协作编辑");
    let state = Reducer::new(f.app.repos.clone())
        .project_read_only(&f.project_id)
        .expect("fold");
    DriftEngine::new(f.app.repos.clone())
        .check(&state)
        .expect("check drift");

    let view = views::drift_view(&f.app, &f.project_id).expect("drift");
    assert_eq!(view.findings.len(), 1, "{:?}", view.findings);
    assert_eq!(view.findings[0].rule, "scope_drift");
    assert_eq!(view.findings[0].severity, "warning");
    assert_eq!(view.findings[0].status, "open", "an unresolved alert stays open");
    assert_eq!(view.inbox.len(), 1, "{:?}", view.inbox);
    assert_eq!(view.inbox[0].kind, "drift_alert");
}

/// The context screen keeps §42's ladder visible — including what the budget held back — and
/// ends on the fallback, because sending is still the human's action (§7.8).
#[test]
fn the_context_screen_shows_what_travels_and_what_is_held() {
    let f = bench();
    f.task("t-1", "搭起事件链");
    let view = views::context_view(
        &f.app,
        &f.project_id,
        "为什么首屏是 diff",
        400,
        "local-coding-agent",
        None,
    )
    .expect("context");

    assert!(view.focus.is_none(), "{:?}", view.focus);
    assert!(view.total_count > 0, "{view:?}");
    assert!(view.kept_count <= view.total_count);
    assert!(view.rows.iter().any(|row| row.kept), "{:?}", view.rows);
    assert!(
        view.rows.iter().any(|row| !row.kept && row.reason.is_some()),
        "a pack that fits has to say what it left out: {:?}",
        view.rows
    );
    assert!(view.prepared.text.starts_with("# Project Context"), "{}", view.prepared.text);
    assert!(view.fallback.instruction.contains("sends nothing"), "{:?}", view.fallback);
    assert!(view.prepared.copy_fallback, "no adapter is wired, so the copy path is the answer");
    // The shell writes no file, so the fallback has no payload to name (§62 Degradation).
    assert!(view.fallback.payload_path.is_none(), "{:?}", view.fallback);
}

/// Health is stated about the file this shell actually opened, in the same numbers `wp backup
/// status` prints.
#[test]
fn the_health_screen_describes_the_store_it_opened() {
    let f = bench();
    f.task("t-1", "搭起事件链");

    let view = views::health_view(&f.app, &f.project_id).expect("health");
    assert_eq!(view.backup.integrity, "ok", "{:?}", view.backup);
    assert!(view.backup.restorable);
    assert!(view.backup.path.ends_with("workbench.db"), "{}", view.backup.path);
    assert_eq!(view.backup.rows_in_store, 2, "the project opening and the task are both recorded");
    assert_eq!(view.backup.projects_in_store, 1);
    assert!(view.backup.size_bytes > 0);
    assert!(view.adapters.is_empty(), "nothing has been collected yet");
    assert!(view.backup.note.contains("nothing"), "{}", view.backup.note);

    let project = views::project_view(&f.app, &f.project_id).expect("project");
    assert_eq!(project.project_name, "工作台");
    assert_eq!(project.root_path.as_deref(), Some("/tmp/repo"));
    assert_eq!(project.last_seq, 2);
    assert_eq!(project.applied_events, 2);
    assert_eq!(project.db_path, f.dir.path().join("workbench.db").display().to_string());

    let hits = views::search_view(&f.app, &f.project_id, "搭起事件链", 10).expect("search");
    // The row L2 finds is the event that recorded the task, so it is titled by its kind and the
    // wording lives in the snippet — the same shape `wp search` prints.
    assert!(hits.iter().any(|row| row.snippet.contains("搭起事件链")), "{hits:?}");
    assert!(hits.iter().all(|row| !row.layer.is_empty()));
}

/// §21's Overview screen: counts the fold produced, and nothing the shell estimated. A baseline
/// the fixture never wrote must show as absent rather than as version zero, because "no charter
/// yet" and "charter v0" are different statements about the project.
#[test]
fn the_overview_counts_what_the_fold_produced_and_admits_what_is_absent() {
    let f = bench();
    f.task("t-1", "搭起事件链");
    f.task("t-2", "首屏渲染 diff");
    f.snapshot("round-1");

    let view = views::overview_view(&f.app, &f.project_id).expect("overview");

    assert_eq!(view.project_name, "工作台");
    assert_eq!(view.root_path.as_deref(), Some("/tmp/repo"));
    assert!(view.charter_version.is_none(), "bench() never wrote a charter");
    assert!(view.scope_version.is_none());
    assert!(view.baseline_version.is_none());
    assert!(!view.state_hash.is_empty(), "the hash names the log this state came from");

    let tasks = view
        .kinds
        .iter()
        .find(|row| row.name == "任务")
        .expect("the task row");
    assert_eq!(tasks.total, 2);
    assert_eq!(
        tasks.tally,
        vec![views::StatusTally {
            status: "todo".into(),
            count: 2
        }],
        "a created task is Todo until an event says otherwise"
    );

    let evidence = view.kinds.iter().find(|row| row.name == "证据").expect("row");
    assert!(
        evidence.tally.is_empty(),
        "evidence has no status, so the screen says so instead of inventing one"
    );

    let snapshot = view.latest_snapshot.expect("one snapshot was taken");
    assert!(snapshot.starts_with("round-1"), "{snapshot}");
    assert_eq!(view.open_proposals, 0, "nothing is waiting on a verdict here");
}

/// §21's Decisions screen. The lock is the point of showing this at all: §7.4 makes a
/// user-authored confirmation their own judgement, and the screen must render the flag the fold
/// raised rather than a label the webview guessed from the actor.
#[test]
fn the_decisions_screen_shows_the_lock_the_fold_raised() {
    let f = bench();
    EventStore::new(f.app.repos.clone())
        .append(
            EventDraft::new(
                f.project_id.clone(),
                vibe_domain::event_catalog::DECISION_CONFIRMED,
            )
            .user()
            .subject("decision", "d-desktop")
            .payload(json!({
                "decisionId": "d-desktop",
                "title": "桌面壳只做展示层",
                "decision": "九个命令里八个只读，唯一的写是 §7.4 的确认",
                "rationale": "两套状态必然漂移",
                "alternatives": ["壳自己折叠事件", "壳直连 SQLite"],
            })),
        )
        .expect("confirm a decision");

    let view = views::decisions_view(&f.app, &f.project_id).expect("decisions");
    assert_eq!(view.decisions.len(), 1, "{:?}", view.decisions);
    let row = &view.decisions[0];
    assert_eq!(row.id, "d-desktop", "the row addresses the record, not a screen-local index");
    assert_eq!(row.status, "active");
    assert!(row.locked, "a user confirmation locks on arrival (§7.4)");
    assert_eq!(row.alternatives.len(), 2);
    assert!(row.decision.contains("§7.4"), "{}", row.decision);
    assert!(row.at > 0, "a decision without a time cannot be put on a timeline");
    assert!(view.note.contains("不提供编辑"), "{}", view.note);
}

/// §21's Evolution screen. An AEP is "when this should change", so the honest rendering is a
/// proposal that is asleep: nothing in this round triggered it, and the screen shows `dormant`
/// rather than dressing it up as work in progress (禁止 8).
#[test]
fn the_evolution_screen_shows_an_aep_as_a_proposal_that_is_not_yet_work() {
    let f = bench();
    EventStore::new(f.app.repos.clone())
        .append(
            EventDraft::new(f.project_id.clone(), vibe_domain::event_catalog::AEP_CREATED)
                .agent()
                .subject("aep", "aep-sync")
                .payload(json!({
                    "aepId": "aep-sync",
                    "code": "AEP-001",
                    "title": "本地图谱到云端同步",
                    "current": "只有一台机器上的 SQLite",
                    "limitation": "换设备要搬文件",
                    "whyNotNow": "没有任何触发条件成立",
                    "trigger": ["出现第二台常用设备", "需要两个人看同一张图"],
                    "target": "同一套事件日志在两台机器上收敛",
                    "changeSurface": ["storage"],
                    "preservedCore": ["事件优先", "只折叠已接受事件"],
                })),
        )
        .expect("open an AEP");

    let view = views::evolution_view(&f.app, &f.project_id).expect("evolution");
    assert_eq!(view.aeps.len(), 1, "{:?}", view.aeps);
    let row = &view.aeps[0];
    assert_eq!(row.code, "AEP-001");
    assert_eq!(row.status, "dormant", "creating an AEP is not starting one");
    assert_eq!(row.trigger.len(), 2, "the trigger is the whole point of the record");
    assert_eq!(row.preserved_core, vec!["事件优先", "只折叠已接受事件"]);
    assert!(row.why_not_now.contains("触发"), "{}", row.why_not_now);
    assert!(view.note.contains("禁止 8"), "{}", view.note);
}

/// §14 item 7 wanted a click on an Issue, and the shell is where the hand lands. The reference
/// *is* the intent, so the screen asks for no prose the user never typed, names what it focused
/// on, and refuses when neither a question nor a click arrived (§62: no invented fallback state).
#[test]
fn a_click_on_a_record_focuses_the_pack_without_a_sentence() {
    let f = bench();
    EventStore::new(f.app.repos.clone())
        .append(
            EventDraft::new(f.project_id.clone(), vibe_domain::event_catalog::ISSUE_FOUND)
                .user()
                .subject("issue", "I-1")
                .payload(json!({
                    "issueId": "I-1",
                    "title": "首屏没有说明哪些来源被预算挡下",
                    "detail": "§43 的裁决在 CLI 有输出，桌面上看不到",
                })),
        )
        .expect("found an issue");

    let view = views::context_view(&f.app, &f.project_id, "", 400, "generic-cli", Some("issue:I-1"))
        .expect("a click is a request");
    assert_eq!(view.focus.as_deref(), Some("issue:I-1"), "{view:?}");
    let selected = view
        .rows
        .iter()
        .find(|row| row.section == "Selected Context")
        .expect("the clicked record is its own section");
    assert_eq!(selected.priority, 1, "{selected:?}");
    assert!(selected.kept, "{selected:?}");
    assert_eq!(selected.source_ref, "issue:I-1", "{selected:?}");
    assert!(
        view.prepared.text.contains("首屏没有说明哪些来源被预算挡下"),
        "the pack carries the record that was clicked:\n{}",
        view.prepared.text
    );

    let empty = views::context_view(&f.app, &f.project_id, "  ", 400, "generic-cli", Some("   "));
    assert!(
        empty.is_err(),
        "nothing to compile is a refusal, not an empty pack: {empty:?}"
    );
    let malformed = views::context_view(&f.app, &f.project_id, "", 400, "generic-cli", Some("I-1"));
    assert!(malformed.is_err(), "a focus has to name its kind: {malformed:?}");
}
