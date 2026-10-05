//! Acceptance for the Project Inbox (§29): one queue, one action per button, and the
//! record behind it actually moves.

mod support;

use chrono::TimeZone;
use serde_json::json;
use vibe_core::event_store::EventDraft;
use vibe_core::ports::EntityStore;
use vibe_domain::event_catalog;
use vibe_domain::*;
use vibe_guard_engine::{InboxAction, InboxRef};

use support::{fixture as open, Fixture};

fn fixture() -> Fixture {
    open("Inbox")
}

impl Fixture {
    fn events_of(&self, kind: &str) -> Vec<ProjectEvent> {
        self.events
            .list(&self.project_id, None, None)
            .unwrap()
            .into_iter()
            .filter(|e| e.r#type == kind)
            .collect()
    }

    fn alert(&self, id: &str) -> DriftAlert {
        self.repos
            .entities
            .entity::<DriftAlert>(EntityKind::DriftAlert, id)
            .unwrap()
            .expect("alert")
    }

    fn proposal(&self, id: &str) -> ChangeProposal {
        self.repos
            .entities
            .entity::<ChangeProposal>(EntityKind::ChangeProposal, id)
            .unwrap()
            .expect("proposal")
    }
}

/// Three forbidden phrases, two pending events, one open proposal.
fn seed(f: &Fixture) -> (Vec<String>, Vec<String>, String) {
    let mut state = ProjectState {
        project_id: f.project_id.clone(),
        ..Default::default()
    };
    let boundary = ScopeBoundary {
        id: "scope-1".into(),
        project_id: f.project_id.clone(),
        version: 1,
        forbidden: vec![
            "多用户登录".to_string(),
            "云端同步".to_string(),
            "团队权限".to_string(),
        ],
        ..Default::default()
    };
    state.scope = Some(boundary);
    state.tasks = vec![
        Task { id: "t-1".into(), project_id: f.project_id.clone(), title: "支持多用户登录".into(), ..Default::default() },
        Task { id: "t-2".into(), project_id: f.project_id.clone(), title: "增加云端同步".into(), ..Default::default() },
        Task { id: "t-3".into(), project_id: f.project_id.clone(), title: "实现团队权限".into(), ..Default::default() },
    ];
    let alerts = f.engine.check(&state).unwrap();
    assert_eq!(alerts.len(), 3, "{alerts:?}");
    let alert_ids = alerts.iter().map(|a| a.id.clone()).collect::<Vec<_>>();

    let proposal = f
        .projects
        .open_change_proposal(
            &f.project_id,
            ChangeProposal {
                id: String::new(),
                project_id: f.project_id.clone(),
                proposed_change: "把 AEP 状态纳入项目地图".into(),
                reason: "需要看到演进计划".into(),
                ..Default::default()
            },
            ActorType::Agent,
        )
        .unwrap();

    let pending = f
        .events
        .append(
            EventDraft::new(f.project_id.clone(), event_catalog::DECISION_PROPOSED)
                .agent()
                .pending()
                .confidence(0.7)
                .payload(json!({ "excerpt": "建议直接让 Core 调 SQLite", "matched": "建议" })),
        )
        .unwrap()
        .event;
    let other = f
        .events
        .append(
            EventDraft::new(f.project_id.clone(), event_catalog::ISSUE_FOUND)
                .agent()
                .pending()
                .confidence(0.68)
                .payload(json!({ "excerpt": "同步仍然重复写入消息" })),
        )
        .unwrap()
        .event;

    (alert_ids, vec![pending.id, other.id], proposal.id)
}

#[test]
fn inbox_aggregates_everything_waiting_for_the_user() {
    let f = fixture();
    let (alerts, events, proposal) = seed(&f);

    let items = f.inbox.list(&f.project_id).unwrap();
    assert_eq!(items.len(), 6, "{:?}", items.iter().map(|i| (i.kind, &i.summary)).collect::<Vec<_>>());
    assert_eq!(f.inbox.count(&f.project_id).unwrap(), 6);

    let mut kinds: Vec<(InboxKind, String)> = items
        .iter()
        .map(|i| (i.kind, i.ref_id.clone().unwrap_or_default()))
        .collect();
    kinds.sort_by(|a, b| a.1.cmp(&b.1));
    let expected = {
        let mut v = vec![
            (InboxKind::DriftAlert, alerts[0].clone()),
            (InboxKind::DriftAlert, alerts[1].clone()),
            (InboxKind::DriftAlert, alerts[2].clone()),
            (InboxKind::ChangeProposal, proposal.clone()),
            (InboxKind::DecisionProposal, events[0].clone()),
            (InboxKind::LowConfidenceEvent, events[1].clone()),
        ];
        v.sort_by(|a, b| a.1.cmp(&b.1));
        v
    };
    assert_eq!(kinds, expected);

    // Each item says what it is, why it is here and which buttons apply.
    for item in &items {
        assert!(!item.summary.trim().is_empty());
        assert!(item.detail.contains("scope:v1") || item.detail.contains("confidence") || item.detail.contains("baseline"));
        assert!(!item.actions.is_empty());
    }
    let drift = items
        .iter()
        .find(|i| i.kind == InboxKind::DriftAlert)
        .expect("drift item");
    assert!(drift.actions.contains(&InboxAction::Keep.label().to_string()));
    assert!(drift.actions.contains(&InboxAction::ToChangeProposal.label().to_string()));
    assert!(drift.actions.contains(&InboxAction::Dismiss.label().to_string()));
}

#[test]
fn resolving_an_event_records_the_users_judgement() {
    let f = fixture();
    let (_, events, _) = seed(&f);
    let proposed = InboxRef::new(&f.project_id, &events[0], InboxKind::DecisionProposal);
    let issue = InboxRef::new(&f.project_id, &events[1], InboxKind::LowConfidenceEvent);

    f.inbox.resolve(&proposed, InboxAction::Accept, "就这么定").unwrap();
    assert_eq!(
        f.events.get(&events[0]).unwrap().unwrap().status,
        EventStatus::Accepted
    );
    f.inbox.resolve(&issue, InboxAction::Reject, "不是事实").unwrap();
    assert_eq!(
        f.events.get(&events[1]).unwrap().unwrap().status,
        EventStatus::Rejected
    );
    assert_eq!(
        f.events
            .list(&f.project_id, Some(&[EventStatus::PendingReview]), None)
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn resolving_a_drift_alert_moves_the_alert_and_records_an_event() {
    let f = fixture();
    let (alerts, _, _) = seed(&f);

    f.inbox
        .resolve(
            &InboxRef::new(&f.project_id, &alerts[0], InboxKind::DriftAlert),
            InboxAction::Dismiss,
            "这就是个实验",
        )
        .unwrap();
    assert_eq!(f.alert(&alerts[0]).status, DriftStatus::Dismissed);
    let dismissed = f.events_of(event_catalog::DRIFT_DISMISSED);
    assert_eq!(dismissed.len(), 1);
    assert_eq!(dismissed[0].payload["reason"], json!("这就是个实验"));

    // "Keep Boundary" closes the alert as resolved: the boundary wins.
    f.inbox
        .resolve(
            &InboxRef::new(&f.project_id, &alerts[1], InboxKind::DriftAlert),
            InboxAction::Keep,
            "保持不变",
        )
        .unwrap();
    assert_eq!(f.alert(&alerts[1]).status, DriftStatus::Resolved);
    assert_eq!(f.events_of(event_catalog::DRIFT_DISMISSED).len(), 2);
    assert!(f.alert(&alerts[1]).detail.unwrap().contains("保持不变"));

    // Accepting the change does not move the baseline by itself.
    f.inbox
        .resolve(
            &InboxRef::new(&f.project_id, &alerts[2], InboxKind::DriftAlert),
            InboxAction::Accept,
            "接受这次范围变化",
        )
        .unwrap();
    assert_eq!(f.alert(&alerts[2]).status, DriftStatus::AcceptedChange);
    assert_eq!(f.events_of(event_catalog::DRIFT_ACCEPTED).len(), 1);
    assert!(f
        .repos
        .projects
        .get_active_scope(&f.project_id)
        .unwrap()
        .is_none());
}

#[test]
fn turning_an_item_into_a_change_proposal_creates_one() {
    let f = fixture();
    let (alerts, events, _) = seed(&f);
    let before = f.events_of(event_catalog::CHANGE_PROPOSAL_CREATED).len();

    f.inbox
        .resolve(
            &InboxRef::new(&f.project_id, &alerts[0], InboxKind::DriftAlert),
            InboxAction::ToChangeProposal,
            "把登录挪出禁止区",
        )
        .unwrap();
    let created = f.events_of(event_catalog::CHANGE_PROPOSAL_CREATED);
    assert_eq!(created.len(), before + 1);
    let new_id = created[0].subject_id.clone().unwrap();
    let proposal = f.proposal(&new_id);
    assert_eq!(proposal.status, ChangeProposalStatus::Open);
    assert!(proposal.proposed_change.contains("多用户登录"));
    assert_eq!(proposal.affected_scope, vec!["多用户登录".to_string()]);
    assert_eq!(proposal.evidence_refs, vec![alerts[0].clone()]);
    // The alert itself stays open: nothing moved yet.
    assert_eq!(f.alert(&alerts[0]).status, DriftStatus::Open);

    f.inbox
        .resolve(
            &InboxRef::new(&f.project_id, &events[0], InboxKind::DecisionProposal),
            InboxAction::ToChangeProposal,
            "按建议建提案",
        )
        .unwrap();
    assert_eq!(
        f.events_of(event_catalog::CHANGE_PROPOSAL_CREATED).len(),
        before + 2
    );
    assert_eq!(
        f.events.get(&events[0]).unwrap().unwrap().status,
        EventStatus::Accepted
    );
}

#[test]
fn resolving_a_change_proposal_accepts_or_rejects_it() {
    let f = fixture();
    let (_, _, first) = seed(&f);
    let second = f
        .projects
        .open_change_proposal(
            &f.project_id,
            ChangeProposal {
                id: String::new(),
                project_id: f.project_id.clone(),
                proposed_change: "增加多人协作权限".into(),
                reason: "临时想法".into(),
                ..Default::default()
            },
            ActorType::Agent,
        )
        .unwrap();

    f.inbox
        .resolve(
            &InboxRef::new(&f.project_id, &first, InboxKind::ChangeProposal),
            InboxAction::Accept,
            "同意",
        )
        .unwrap();
    assert_eq!(f.proposal(&first).status, ChangeProposalStatus::Accepted);
    assert_eq!(f.events_of(event_catalog::CHANGE_PROPOSAL_ACCEPTED).len(), 1);

    f.inbox
        .resolve(
            &InboxRef::new(&f.project_id, &second.id, InboxKind::ChangeProposal),
            InboxAction::Reject,
            "偏航了",
        )
        .unwrap();
    assert_eq!(f.proposal(&second.id).status, ChangeProposalStatus::Rejected);
    assert_eq!(f.events_of(event_catalog::CHANGE_PROPOSAL_REJECTED).len(), 1);

    // Keeping a proposal open is not an action with a record to move.
    let err = f
        .inbox
        .resolve(
            &InboxRef::new(&f.project_id, &second.id, InboxKind::ChangeProposal),
            InboxAction::ToChangeProposal,
            "",
        )
        .unwrap_err();
    assert!(err.to_string().contains("already"));
}

#[test]
fn the_inbox_surfaces_conflicts_before_check_has_run() {
    let f = fixture();
    let decided = chrono::Utc.with_ymd_and_hms(2026, 6, 1, 9, 0, 0).unwrap();
    f.repos
        .entities
        .put_entity(
            EntityKind::Decision,
            &Decision {
                id: "d-1".into(),
                project_id: f.project_id.clone(),
                title: "存储访问".into(),
                decision: "Core 不直接依赖 SQLite".into(),
                status: DecisionStatus::Active,
                decided_at: Some(decided),
                ..Default::default()
            },
            Some("active"),
        )
        .unwrap();
    f.repos
        .entities
        .put_entity(
            EntityKind::Decision,
            &Decision {
                id: "d-2".into(),
                project_id: f.project_id.clone(),
                title: "存储简化".into(),
                decision: "Core 直接调用 SQLite".into(),
                status: DecisionStatus::Active,
                decided_at: Some(decided + chrono::Duration::days(20)),
                ..Default::default()
            },
            Some("active"),
        )
        .unwrap();

    let mut probe = ProjectState {
        project_id: f.project_id.clone(),
        ..Default::default()
    };
    probe.decisions = f
        .repos
        .entities
        .entities_of(EntityKind::Decision, &f.project_id)
        .unwrap();
    let conflicts = f.engine.decision_conflict(&probe);
    assert_eq!(conflicts.len(), 1);

    let items = f.inbox.list(&f.project_id).unwrap();
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0].kind, InboxKind::Conflict);
    assert_eq!(items[0].ref_id.as_deref(), Some(conflicts[0].id.as_str()));
}

/// §7.4's whole point: the user's verdict is what makes a change take effect. Before this,
/// `ProjectService::accept_change_proposal` — the only code that applies a proposal's
/// machine-applicable delta — had no caller in any surface, so accepting from the Inbox
/// flipped a status and left the boundary where it was.
#[test]
fn accepting_a_proposal_from_the_inbox_applies_the_delta_it_carries() {
    let f = fixture();
    f.projects
        .update_scope(
            &f.project_id,
            vec![(ScopeSection::Forbidden, "云端同步".into())],
            vec![],
            ActorType::User,
        )
        .unwrap();

    let proposal = f
        .projects
        .open_change_proposal(
            &f.project_id,
            ChangeProposal {
                id: String::new(),
                project_id: f.project_id.clone(),
                proposed_change: "把「云端同步」从禁区挪到暂缓区".into(),
                reason: "离线也要能接续工作".into(),
                delta: Some(json!({
                    "appliesTo": "scope",
                    "remove": [{ "section": "forbidden", "item": "云端同步" }],
                    "add": [{ "section": "deferred", "item": "云端同步" }],
                })),
                ..Default::default()
            },
            ActorType::Agent,
        )
        .unwrap();

    f.inbox
        .resolve(
            &InboxRef::new(&f.project_id, &proposal.id, InboxKind::ChangeProposal),
            InboxAction::Accept,
            "同意，先记为暂缓",
        )
        .unwrap();

    let scope = f.projects.scope(&f.project_id).unwrap().expect("scope");
    assert_eq!(scope.version, 2, "the accepted delta moved the boundary to a new version");
    assert!(scope.deferred.iter().any(|item| item == "云端同步"), "{scope:?}");
    assert!(!scope.forbidden.iter().any(|item| item == "云端同步"), "{scope:?}");
    assert_eq!(
        f.repos.projects.list_scope_versions(&f.project_id).unwrap().len(),
        2,
        "the old version stays (§9): a move inserts, never overwrites"
    );

    assert_eq!(f.proposal(&proposal.id).status, ChangeProposalStatus::Accepted);
    let verdicts = f.events_of(event_catalog::CHANGE_PROPOSAL_ACCEPTED);
    assert_eq!(verdicts.len(), 1, "one verdict, one event");
    assert_eq!(verdicts[0].payload["reason"], json!("同意，先记为暂缓"));

    // Saying it again is not a second change: the status already matches, so nothing moves.
    f.inbox
        .resolve(
            &InboxRef::new(&f.project_id, &proposal.id, InboxKind::ChangeProposal),
            InboxAction::Accept,
            "同意，先记为暂缓",
        )
        .unwrap();
    assert_eq!(f.projects.scope(&f.project_id).unwrap().unwrap().version, 2);
    assert_eq!(f.events_of(event_catalog::CHANGE_PROPOSAL_ACCEPTED).len(), 1);
}

/// Rejecting is the other half of 确认即锁定: the record closes, the boundary does not move.
#[test]
fn rejecting_a_proposal_from_the_inbox_leaves_the_boundary_alone() {
    let f = fixture();
    f.projects
        .update_scope(
            &f.project_id,
            vec![(ScopeSection::Forbidden, "多人协作".into())],
            vec![],
            ActorType::User,
        )
        .unwrap();
    let proposal = f
        .projects
        .open_change_proposal(
            &f.project_id,
            ChangeProposal {
                id: String::new(),
                project_id: f.project_id.clone(),
                proposed_change: "把「多人协作」挪进暂缓区".into(),
                reason: "也许以后要".into(),
                delta: Some(json!({
                    "appliesTo": "scope",
                    "add": [{ "section": "deferred", "item": "多人协作" }],
                })),
                ..Default::default()
            },
            ActorType::Agent,
        )
        .unwrap();

    f.inbox
        .resolve(
            &InboxRef::new(&f.project_id, &proposal.id, InboxKind::ChangeProposal),
            InboxAction::Reject,
            "V1 不做团队功能",
        )
        .unwrap();

    assert_eq!(f.proposal(&proposal.id).status, ChangeProposalStatus::Rejected);
    let scope = f.projects.scope(&f.project_id).unwrap().unwrap();
    assert_eq!(scope.version, 1, "a rejected proposal never touches a baseline");
    assert!(scope.deferred.is_empty(), "{scope:?}");
}
