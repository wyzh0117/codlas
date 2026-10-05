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
use vibe_core::project_service::{BaselineDraft, CharterDraft, ProjectService};
use vibe_domain::{
    ActorType, ArchitectureInvariant, ArchitectureLayer, ChangeProposal, ChangeProposalStatus,
    EntityKind, ScopeSection,
};
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

    /// One accepted event, written the way the CLI writes it, so a fixture never invents a fact
    /// the engines would not have produced.
    fn record(&self, kind: &str, subject_kind: &str, subject_id: &str, payload: serde_json::Value) {
        EventStore::new(self.app.repos.clone())
            .append(
                EventDraft::new(self.project_id.clone(), kind)
                    .user()
                    .subject(subject_kind, subject_id)
                    .payload(payload),
            )
            .expect(kind);
    }

    /// §9's Charter — the only record ① can be read from.
    fn charter(&self) {
        self.projects
            .save_charter(
                &self.project_id,
                CharterDraft {
                    original_problem: "决定散落在聊天记录里，没人说得清项目走到了哪一步".into(),
                    target_user: Some("同时推进两三个项目的独立开发者".into()),
                    core_value: "任何时候都能回答「现在最该做什么」".into(),
                    must_have: vec!["本地优先".into(), "事件是唯一事实源".into()],
                    ..Default::default()
                },
                ActorType::User,
            )
            .expect("save charter");
    }

    fn phase(&self, phase: &str) {
        self.projects
            .update_phase(&self.project_id, phase)
            .expect("declare the phase");
    }

    /// §11's Architecture Baseline — the line the drift rules measure against, and §47's first leg.
    fn baseline(&self) {
        self.projects
            .set_baseline(
                &self.project_id,
                BaselineDraft {
                    summary: "单体 CLI 折叠事件，桌面壳只画投影".into(),
                    layers: vec![ArchitectureLayer {
                        id: "layer-shell".into(),
                        name: "展示层".into(),
                        responsibility: Some("把投影画出来，不解释事件".into()),
                    }],
                    invariants: vec![ArchitectureInvariant {
                        id: "inv-one-state".into(),
                        text: "壳里不得出现第二套 Project State（禁止 4）".into(),
                        detect: vec!["apps/desktop/src/views.rs".into()],
                    }],
                    replaceable_parts: vec!["存储实现".into()],
                },
                ActorType::User,
            )
            .expect("set baseline");
    }

    /// A boundary that excludes nothing, so a fixture can hold three baselines and still have
    /// no drift to report.
    fn boundary(&self) {
        self.projects
            .update_scope(
                &self.project_id,
                vec![(ScopeSection::InScope, "本地 SQLite 单文件".into())],
                vec![],
                ActorType::User,
            )
            .expect("seed the boundary");
    }

    fn aep(&self, id: &str, code: &str, title: &str) {
        self.record(
            vibe_domain::event_catalog::AEP_CREATED,
            "aep",
            id,
            json!({
                "aepId": id,
                "code": code,
                "title": title,
                "current": "只有一台机器上的 SQLite",
                "limitation": "换设备要搬文件",
                "whyNotNow": "没有任何触发条件成立",
                "trigger": ["出现第二台常用设备"],
                "target": "同一套事件日志在两台机器上收敛",
                "changeSurface": ["storage"],
                "preservedCore": ["事件优先"],
            }),
        );
    }

    /// The record moves only by its own event: a shell-side flag would be a second truth.
    fn move_aep(&self, id: &str, kind: &str) {
        self.record(kind, "aep", id, json!({ "aepId": id }));
    }

    fn issue(&self, id: &str, title: &str) {
        self.record(
            vibe_domain::event_catalog::ISSUE_FOUND,
            "issue",
            id,
            json!({
                "issueId": id,
                "title": title,
                "detail": "§43 的裁决在 CLI 有输出，桌面上看不到",
            }),
        );
    }

    fn blocked(&self, id: &str, title: &str) {
        self.record(
            vibe_domain::event_catalog::TASK_BLOCKED,
            "task",
            id,
            json!({ "taskId": id, "title": title, "reason": "等 §43 的口径" }),
        );
    }

    fn milestone(&self, id: &str, title: &str, reached: bool) {
        self.record(
            vibe_domain::event_catalog::MILESTONE_CREATED,
            "milestone",
            id,
            json!({ "milestoneId": id, "title": title }),
        );
        if reached {
            self.record(
                vibe_domain::event_catalog::MILESTONE_REACHED,
                "milestone",
                id,
                json!({ "milestoneId": id }),
            );
        }
    }

    /// The rules run exactly as `wp drift check` runs them — nothing here is authored for the
    /// screen.
    fn drift(&self) {
        let state = Reducer::new(self.app.repos.clone())
            .project_read_only(&self.project_id)
            .expect("fold");
        DriftEngine::new(self.app.repos.clone())
            .check(&state)
            .expect("check drift");
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

/// §46 is the first screen's contract, and it is six questions in a fixed order. A project with
/// every record the questions can be read from answers all six — and then announces nothing.
#[test]
fn the_overview_answers_the_six_questions_in_order() {
    assert_eq!(
        views::QUESTIONS,
        [
            "最初目标是什么？",
            "当前阶段是什么？",
            "最近发生了什么？",
            "当前最大问题是什么？",
            "有没有偏航？",
            "下一步是什么？",
        ],
        "the screen walks §46's list, it does not write its own"
    );

    let f = bench();
    f.charter();
    f.phase("MVP 收尾");
    f.baseline();
    f.projects
        .update_scope(
            &f.project_id,
            vec![(ScopeSection::Forbidden, "多人协作".into())],
            vec![],
            ActorType::User,
        )
        .expect("seed the boundary");
    f.milestone("m-1", "首个可用版本", true);
    f.task("t-1", "支持多人协作编辑");
    f.blocked("t-2", "把裁决口径写进视图");
    f.issue("I-1", "首屏没有说明哪些来源被预算挡下");
    f.drift();

    let view = views::overview_view(&f.app, &f.project_id).expect("overview");
    assert_eq!(view.questions, views::QUESTIONS, "the screen renders the list it was handed");

    // ① the charter's own words, hydrated by the reducer, versioned like every baseline.
    let intent = view.intent.expect("a charter answers ①");
    assert!(intent.original_problem.contains("聊天记录"), "{intent:?}");
    assert_eq!(intent.core_value, "任何时候都能回答「现在最该做什么」");
    assert_eq!(intent.target_user.as_deref(), Some("同时推进两三个项目的独立开发者"));
    assert_eq!(intent.must_have, vec!["本地优先", "事件是唯一事实源"]);
    assert_eq!(intent.version, 1, "the version is the record's, not the screen's");
    assert!(!intent.locked, "nothing locked this charter");

    // ② what the project row declares, plus the milestones that mark it.
    assert!(view.phase.declared, "{:?}", view.phase);
    assert_eq!(view.phase.phase.as_deref(), Some("MVP 收尾"));
    assert_eq!(view.phase.status, "active", "the lifecycle word is the record's");
    assert_eq!(view.phase.milestones, vec!["首个可用版本 — reached"]);

    // ③ the newest accepted events, newest first, in the timeline's own rows.
    assert_eq!(view.recent.len(), 3, "「最近」 is bounded on purpose: {:?}", view.recent);
    assert_eq!(view.recent[0].seq, view.last_seq, "the first row is the newest fact");
    assert_eq!(view.recent[0].kind, vibe_domain::event_catalog::DRIFT_DETECTED);

    // ④ the head of the queue §29 already ranked, with the counts behind it.
    let problem = &view.problem;
    let headline = problem.headline.as_ref().expect("the drift waits on a verdict");
    assert_eq!(headline.kind, "drift_alert", "{headline:?}");
    assert_eq!(problem.queue, 3, "{problem:?}");
    assert_eq!(problem.open_issues, 1);
    assert_eq!(problem.blocked_tasks, 1);

    // ⑤ counted from the alerts the rules recorded, rule · severity, no shell-side re-ranking.
    // Three rules fired on this fixture and the screen names all three: it does not pick the
    // scary one.
    assert!(view.drift.drifting, "{:?}", view.drift);
    assert_eq!(
        view.drift.kinds.iter().map(|row| row.count).sum::<usize>(),
        3,
        "{:?}",
        view.drift.kinds
    );
    assert_eq!(view.drift.open, view.drift.recorded, "nothing has been judged yet");
    assert!(
        view.drift
            .kinds
            .iter()
            .any(|row| row.status == "scope_drift · warning"),
        "{:?}",
        view.drift.kinds
    );

    // ⑥ PROJECT_MAP's own section, not a recomposition of it.
    assert!(
        view.next_actions
            .iter()
            .any(|line| line.contains("Decide on scope_drift")),
        "{:?}",
        view.next_actions
    );
    assert!(
        view.next_actions
            .iter()
            .any(|line| line.contains("首屏没有说明哪些来源被预算挡下")),
        "{:?}",
        view.next_actions
    );

    assert!(view.gaps.is_empty(), "every question had a record to read: {:?}", view.gaps);
}

/// ⑥ is the one question the shell must not answer twice: the overview shows the map's
/// `Next Actions`, line for line, because §31 already composes them.
#[test]
fn the_overview_shows_the_maps_next_actions_instead_of_writing_its_own() {
    let f = bench();
    f.task("t-1", "搭起事件链");
    f.issue("I-1", "首屏没有说明哪些来源被预算挡下");

    let overview = views::overview_view(&f.app, &f.project_id).expect("overview");
    let map = views::map_view(&f.app, &f.project_id).expect("map");
    let section = map
        .sections
        .iter()
        .find(|section| section.name == "Next Actions")
        .expect("the map has the section §31 names");

    assert_eq!(
        overview.next_actions, section.lines,
        "the same compiled lines, in the same order"
    );
    assert!(overview.next_actions.len() >= 4, "{:?}", overview.next_actions);
    assert!(
        !overview.gaps.iter().any(|gap| gap.label == views::Q_NEXT),
        "an answered question is not announced as a gap: {:?}",
        overview.gaps
    );
}

/// ⑤ has a real answer even when the answer is no: `drifting: false` is stated, not hidden
/// behind an empty box.
#[test]
fn the_overview_says_no_when_the_rules_found_nothing() {
    let f = bench();
    f.task("t-1", "搭起事件链");

    let view = views::overview_view(&f.app, &f.project_id).expect("overview");

    assert!(!view.drift.drifting, "no rule raised anything, and the screen says so");
    assert_eq!(view.drift.open, 0);
    assert_eq!(view.drift.recorded, 0);
    assert!(view.drift.kinds.is_empty(), "{:?}", view.drift.kinds);
    assert_eq!(view.drift_findings, 0, "the old count agrees with the new answer");
    assert!(
        !view.gaps.iter().any(|gap| gap.label == views::Q_DRIFT),
        "「没有偏航」 is an answer, not a gap: {:?}",
        view.gaps
    );
}

/// A question the records cannot answer is announced with its reason instead of shown as a
/// fact the shell made up — ADR-011's rule for an empty map section, applied to a screen.
#[test]
fn the_overview_announces_what_the_records_do_not_hold() {
    let f = bench();

    let view = views::overview_view(&f.app, &f.project_id).expect("overview");

    assert!(view.intent.is_none(), "① has nothing to read");
    assert!(!view.phase.declared, "② has nothing to read");
    assert_eq!(view.gaps.len(), 2, "{:?}", view.gaps);
    assert_eq!(view.gaps[0].label, views::Q_INTENT);
    assert!(view.gaps[0].reason.contains("Charter"), "{:?}", view.gaps[0]);
    assert_eq!(view.gaps[1].label, views::Q_PHASE);
    assert!(view.gaps[1].reason.contains("current_phase"), "{:?}", view.gaps[1]);

    // ③ has the project opening, ④ has nothing waiting, ⑤ answers no, ⑥ has the baseline gaps
    // to report — so only the two records that were never written are missing.
    assert_eq!(view.recent.len(), 1, "{:?}", view.recent);
    assert_eq!(view.recent[0].kind, vibe_domain::event_catalog::PROJECT_CREATED);
    assert!(view.problem.headline.is_none(), "{:?}", view.problem);
    assert_eq!(view.problem.queue, 0);
    assert!(!view.next_actions.is_empty(), "{:?}", view.next_actions);
}

/// An Issue carries no priority field, so when nothing waits on a verdict the screen reports
/// counts rather than naming a winner it would have invented.
#[test]
fn the_overview_refuses_to_rank_problems_the_engine_left_unranked() {
    let f = bench();
    f.issue("I-1", "首屏没有说明哪些来源被预算挡下");
    f.issue("I-2", "变更记录里没有 actor 的角色");

    let view = views::overview_view(&f.app, &f.project_id).expect("overview");

    assert!(view.problem.headline.is_none(), "{:?}", view.problem);
    assert_eq!(view.problem.open_issues, 2, "the count is still the honest part");
    assert_eq!(view.problem.queue, 0);
    let gap = view
        .gaps
        .iter()
        .find(|gap| gap.label == views::Q_PROBLEM)
        .expect("the unranked pair is announced, not silently topped");
    assert!(gap.reason.contains("优先级"), "{gap:?}");
}

/// §31 says `_none recorded_` and means it: a project with three baselines and no open work has
/// no next action to show, and that is the answer.
#[test]
fn the_overview_announces_an_empty_next_actions_section() {
    let f = bench();
    f.charter();
    f.baseline();
    f.boundary();
    f.phase("MVP 收尾");

    let view = views::overview_view(&f.app, &f.project_id).expect("overview");

    assert!(view.next_actions.is_empty(), "{:?}", view.next_actions);
    assert_eq!(view.gaps.len(), 1, "{:?}", view.gaps);
    assert_eq!(view.gaps[0].label, views::Q_NEXT);
    assert!(
        view.gaps[0].reason.contains("_none recorded_"),
        "{:?}",
        view.gaps[0]
    );
    assert!(view.gaps.iter().all(|gap| gap.label != views::Q_INTENT));
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

/// §47's five legs, in §47's order — the list is the screen's contract, same as §46's.
#[test]
fn the_evolution_screen_names_the_five_legs_and_no_more() {
    assert_eq!(
        views::LEGS,
        [
            "Architecture Baseline",
            "Active ADR",
            "AEP Registry",
            "Triggered AEP",
            "Completed Evolution",
        ]
    );

    let f = bench();
    let view = views::evolution_view(&f.app, &f.project_id).expect("evolution");
    assert_eq!(view.legs, views::LEGS, "the webview reads the legs off the payload");
}

/// Leg 1 is the baseline the reducer hydrates: every write keeps the previous version (§9's rule,
/// §11 for baselines), so the screen shows the head and says how long the history is.
#[test]
fn the_evolution_screen_shows_the_baseline_the_reducer_hydrates() {
    let f = bench();
    f.baseline();
    f.baseline();

    let view = views::evolution_view(&f.app, &f.project_id).expect("evolution");
    let baseline = view.baseline.expect("leg 1 has a record");

    assert_eq!(baseline.version, 2, "the version is the record's, not the screen's");
    assert_eq!(baseline.history, 2, "both rows are still in the store");
    assert_eq!(baseline.summary, "单体 CLI 折叠事件，桌面壳只画投影");
    assert_eq!(
        baseline.layers,
        vec!["展示层 — 把投影画出来，不解释事件"],
        "a layer is its name and its responsibility, the way §11 stores it"
    );
    assert!(baseline.invariants[0].contains("禁止 4"), "{:?}", baseline.invariants);
    assert_eq!(baseline.replaceable_parts, vec!["存储实现"]);
    assert!(
        view.gaps.iter().all(|gap| gap.label != views::LEGS[0]),
        "an answered leg is not announced: {:?}",
        view.gaps
    );
}

/// Legs 3, 4 and 5 are one list split by the record's own `AepStatus` — the shell adds no flag,
/// and §31's `Next Actions` picks the same pair up.
#[test]
fn the_evolution_screen_splits_the_aeps_by_their_own_status() {
    let f = bench();
    f.aep("aep-wait", "AEP-001", "本地图谱到云端同步");
    f.aep("aep-live", "AEP-002", "把索引换成向量检索");
    f.aep("aep-done", "AEP-003", "把 CLI 拆成可复用的库");
    f.move_aep(
        "aep-live",
        vibe_domain::event_catalog::AEP_TRIGGER_DETECTED,
    );
    f.move_aep("aep-done", vibe_domain::event_catalog::AEP_COMPLETED);

    let view = views::evolution_view(&f.app, &f.project_id).expect("evolution");

    assert_eq!(view.aeps.len(), 3, "the registry is all of them, once each");
    assert_eq!(view.dormant.len(), 1, "{:?}", view.dormant);
    assert_eq!(view.dormant[0].code, "AEP-001");
    assert_eq!(view.dormant[0].status, "dormant", "a trigger that never fired");
    assert_eq!(view.triggered.len(), 1, "{:?}", view.triggered);
    assert_eq!(view.triggered[0].code, "AEP-002");
    assert_eq!(view.triggered[0].status, "triggered");
    assert_eq!(view.completed.len(), 1, "{:?}", view.completed);
    assert_eq!(view.completed[0].code, "AEP-003");
    assert_eq!(view.completed[0].status, "completed");

    let overview = views::overview_view(&f.app, &f.project_id).expect("overview");
    assert!(
        overview
            .next_actions
            .iter()
            .any(|line| line.contains("AEP-002")),
        "the woken plan is the map's next action too: {:?}",
        overview.next_actions
    );
}

/// Leg 2 cannot be served from the store: an ADR is a document in `docs/adr/`, and a Decision
/// record has no ADR type on it. The honest answer is the announcement — no table, no second
/// source of truth, no Decision dressed up as an ADR.
#[test]
fn the_evolution_screen_announces_the_adr_leg_it_cannot_read() {
    let f = bench();
    EventStore::new(f.app.repos.clone())
        .append(
            EventDraft::new(
                f.project_id.clone(),
                vibe_domain::event_catalog::DECISION_CONFIRMED,
            )
            .user()
            .subject("decision", "d-adr")
            .payload(json!({
                "decisionId": "d-adr",
                "title": "壳不建 ADR 表",
                "decision": "ADR 留在 docs/adr/，桌面只读投影",
            })),
        )
        .expect("confirm a decision");

    let view = views::evolution_view(&f.app, &f.project_id).expect("evolution");

    let adr = view
        .gaps
        .iter()
        .find(|gap| gap.label == views::LEGS[1])
        .expect("leg 2 is announced, never faked");
    assert!(adr.reason.contains("docs/adr/"), "{adr:?}");
    assert!(adr.reason.contains("Decision"), "{adr:?}");
    assert!(view.baseline.is_none(), "leg 1 is empty here too");
    assert_eq!(view.gaps.len(), 2, "{:?}", view.gaps);
    assert_eq!(view.gaps[0].label, views::LEGS[0], "the gaps read in §47's order");
    assert!(view.aeps.is_empty(), "no AEP was written in this fixture");
    assert!(view.note.contains("禁止 8"), "{}", view.note);
}

/// A renamed field is a blank screen in the window, not a compile error, so the wire shape the
/// webview reads is part of what these tests hold still.
#[test]
fn the_new_answers_reach_the_webview_under_the_keys_it_reads() {
    let f = bench();
    f.charter();
    f.baseline();
    f.phase("MVP 收尾");
    f.aep("aep-1", "AEP-001", "本地图谱到云端同步");

    let overview = serde_json::to_value(views::overview_view(&f.app, &f.project_id).expect("overview"))
        .expect("serialize overview");
    for key in [
        "questions",
        "intent",
        "phase",
        "recent",
        "problem",
        "drift",
        "nextActions",
        "gaps",
    ] {
        assert!(overview.get(key).is_some(), "no `{key}` in {overview}");
    }
    assert_eq!(overview["questions"].as_array().map(Vec::len), Some(6));
    assert_eq!(
        overview["intent"]["originalProblem"],
        "决定散落在聊天记录里，没人说得清项目走到了哪一步"
    );
    assert_eq!(overview["phase"]["declared"], json!(true));
    assert_eq!(overview["phase"]["phase"], "MVP 收尾");
    assert_eq!(overview["drift"]["drifting"], json!(false), "a flat ⑤ still rides the wire");
    assert!(overview["problem"]["headline"].is_null(), "nothing waits on a verdict");

    let evolution =
        serde_json::to_value(views::evolution_view(&f.app, &f.project_id).expect("evolution"))
            .expect("serialize evolution");
    for key in [
        "legs",
        "baseline",
        "aeps",
        "triggered",
        "completed",
        "dormant",
        "gaps",
    ] {
        assert!(evolution.get(key).is_some(), "no `{key}` in {evolution}");
    }
    assert_eq!(evolution["legs"].as_array().map(Vec::len), Some(5));
    assert_eq!(evolution["baseline"]["replaceableParts"], json!(["存储实现"]));
    assert_eq!(evolution["dormant"][0]["whyNotNow"], "没有任何触发条件成立");
    assert_eq!(evolution["triggered"].as_array().map(Vec::len), Some(0));
    assert_eq!(evolution["gaps"][0]["label"], "Active ADR", "leg 2 is the announced one here");
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
