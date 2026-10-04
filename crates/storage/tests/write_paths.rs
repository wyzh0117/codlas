//! Every write path in the store, exercised twice against real SQLite.
//!
//! The first round of tests caught an `ON CONFLICT` target that did not match its
//! schema — the statement only fails when something actually writes a second version
//! of a row, so a single insert per table proves nothing. This suite makes each port
//! perform its own documented repeat.

use vibe_core::ports::{EntityStore, Repositories};
use vibe_domain::{
    Artifact, ArtifactKind, AuditLog, Conversation, Decision, DecisionStatus, EntityKind,
    Evidence, EvidenceType, Milestone, Message, MessageRole, ProjectPolicy, Snapshot, Source,
    SourceType, Task, TaskStatus, UserOverride,
};
use vibe_storage::SqliteStore;

fn fixture() -> (Repositories, String) {
    let store = SqliteStore::open_in_memory().expect("open");
    let repos = store.repositories();
    let project = vibe_core::project_service::ProjectService::new(repos.clone())
        .create_project("write paths", None, None, None)
        .expect("project");
    (repos, project.id)
}

#[test]
fn artifacts_survive_a_second_version_of_the_same_path() {
    let (repos, project) = fixture();
    let make = |version: i64, hash: &str| Artifact {
        id: "art-1".into(),
        project_id: project.clone(),
        name: "judge".into(),
        kind: ArtifactKind::SourceFile,
        path: Some("crates/core/src/judge.rs".into()),
        hash: Some(hash.into()),
        version: version as u32,
        relationships: vec![],
        deleted: false,
        redesign_count: 0,
        updated_at: Some(vibe_domain::now()),
    };
    repos.artifacts.upsert_artifact(&make(1, "aaa")).expect("v1");
    repos.artifacts.upsert_artifact(&make(2, "bbb")).expect("v2");

    let all = repos.artifacts.list_artifacts(&project).expect("list");
    assert_eq!(all.len(), 1, "one path, one row");
    assert_eq!(all[0].version, 2);
    assert_eq!(all[0].hash.as_deref(), Some("bbb"));
    let found = repos
        .artifacts
        .find_artifact_by_path(&project, "crates/core/src/judge.rs")
        .expect("by path")
        .expect("present");
    assert_eq!(found.id, "art-1");
}

#[test]
fn conversations_sources_and_messages_round_trip() {
    let (repos, project) = fixture();
    let source = Source {
        id: "src-1".into(),
        project_id: project.clone(),
        r#type: SourceType::GenericCli,
        external_id: Some("session-77".into()),
        display_name: "cli session".into(),
        metadata: serde_json::json!({}),
    };
    repos.conversations.upsert_source(&source).expect("source");
    let mut renamed = source.clone();
    let Source {
        display_name,
        metadata,
        ..
    } = &mut renamed;
    *display_name = "renamed session".into();
    *metadata = serde_json::json!({ "cwd": "/tmp" });
    repos
        .conversations
        .upsert_source(&renamed)
        .expect("source upsert twice");
    assert_eq!(
        repos
            .conversations
            .list_sources(&project)
            .expect("sources")
            .len(),
        1
    );
    assert_eq!(
        repos
            .conversations
            .get_source(&project, "src-1")
            .expect("get")
            .expect("present")
            .display_name,
        "renamed session"
    );

    let conversation = Conversation {
        id: "conv-1".into(),
        project_id: project.clone(),
        source_id: "src-1".into(),
        external_conversation_id: Some("c-77".into()),
        title: Some("drift discussion".into()),
        started_at: None,
        last_seen_at: None,
    };
    repos
        .conversations
        .upsert_conversation(&conversation)
        .expect("conversation");
    repos
        .conversations
        .upsert_conversation(&conversation)
        .expect("conversation twice");

    let message = Message::new(
        "conv-1".into(),
        MessageRole::User,
        "  the judge keeps candidates  ".into(),
        None,
        vibe_domain::now(),
    );
    let outcome = repos
        .conversations
        .insert_messages("conv-1", &[message.clone()])
        .expect("first insert");
    assert_eq!(outcome.inserted.len(), 1);
    let again = repos
        .conversations
        .insert_messages("conv-1", &[message])
        .expect("second insert");
    assert!(again.inserted.is_empty(), "whitespace-only edit is still the same text");
    assert_eq!(again.duplicate_count, 1);
    assert_eq!(
        repos
            .conversations
            .list_messages("conv-1")
            .expect("messages")
            .len(),
        1
    );
    assert_eq!(
        repos
            .conversations
            .recent_messages(&project, 5)
            .expect("recent")
            .len(),
        1
    );

    // Columns must land in the fields they name: a swapped read makes every later
    // content-hash comparison wrong, which is how dedup silently breaks.
    let read = repos
        .conversations
        .list_messages("conv-1")
        .expect("messages")
        .remove(0);
    assert_eq!(
        read.content_hash,
        vibe_domain::content_hash("  the judge keeps candidates  "),
        "content_hash must be the hash of the content, not an id"
    );
    assert_eq!(read.role, MessageRole::User);
    assert_eq!(read.conversation_id, "conv-1");
    assert_eq!(
        read.content,
        "  the judge keeps candidates  ",
        "raw content is stored faithfully (§14)"
    );
    assert!(read.external_message_id.is_none());

    let with_external = Message::new(
        "conv-1".into(),
        MessageRole::Assistant,
        "candidates never change state".into(),
        Some("msg-500".into()),
        vibe_domain::now(),
    );
    repos
        .conversations
        .insert_messages("conv-1", &[with_external])
        .expect("external id insert");
    let read = repos
        .conversations
        .list_messages("conv-1")
        .expect("messages")
        .pop()
        .expect("second message");
    assert_eq!(read.external_message_id.as_deref(), Some("msg-500"));
    assert_eq!(
        read.content_hash,
        vibe_domain::content_hash("candidates never change state")
    );
}

#[test]
fn evidence_links_and_versions_are_repeatable() {
    let (repos, project) = fixture();
    let evidence = Evidence {
        id: "ev-1".into(),
        project_id: project.clone(),
        r#type: EvidenceType::File,
        ref_id: Some("crates/core/src/judge.rs".into()),
        summary: Some("candidate routing".into()),
        created_at: vibe_domain::now(),
    };
    repos.evidence.insert_evidence(&evidence).expect("insert");
    repos.evidence.insert_evidence(&evidence).expect("idempotent");
    assert_eq!(
        repos.evidence.list_evidence(&project).expect("list").len(),
        1
    );
    assert!(repos
        .evidence
        .find_by_ref(EvidenceType::File, "crates/core/src/judge.rs")
        .expect("by ref")
        .is_some());

    repos
        .evidence
        .link("ev-1", EntityKind::Task, "task-1")
        .expect("link");
    repos
        .evidence
        .link("ev-1", EntityKind::Task, "task-1")
        .expect("same link twice");
    assert_eq!(
        repos
            .evidence
            .for_subject(EntityKind::Task, "task-1")
            .expect("for subject")
            .len(),
        1
    );

    repos
        .entities
        .record_version(
            EntityKind::Task,
            "task-1",
            1,
            None,
            &serde_json::json!({ "status": "doing" }),
            "status moved",
        )
        .expect("version 1");
    repos
        .entities
        .record_version(
            EntityKind::Task,
            "task-1",
            1,
            None,
            &serde_json::json!({ "status": "done" }),
            "same version again",
        )
        .expect("version write must not crash the caller");
}

#[test]
fn entity_links_survive_being_added_twice() {
    let (repos, _project) = fixture();
    for _ in 0..2 {
        repos
            .entities
            .add_link(EntityKind::Task, "task-1", EntityKind::Decision, "D-1", "evidenced_by")
            .expect("link");
    }
    let links = repos
        .entities
        .links_from(EntityKind::Task, "task-1")
        .expect("links");
    assert_eq!(links.len(), 1, "a repeated link is the same statement");
    assert_eq!(links[0].1, "D-1");
}

#[test]
fn related_follows_the_link_and_not_just_the_id() {
    let (repos, project) = fixture();
    // Two kinds, the same id: legal, and exactly what an id-only relation join used to
    // confuse.
    repos
        .entities
        .put_entity(
            EntityKind::Task,
            &Task {
                id: "1".into(),
                project_id: project.clone(),
                title: "compile the project map".into(),
                status: TaskStatus::Todo,
                ..Default::default()
            },
            Some("todo"),
        )
        .expect("task");
    repos
        .entities
        .put_entity(
            EntityKind::Decision,
            &Decision {
                id: "1".into(),
                project_id: project.clone(),
                title: "retrieval stays lexical".into(),
                decision: "no vector index in V1".into(),
                status: DecisionStatus::Active,
                ..Default::default()
            },
            Some("active"),
        )
        .expect("decision");
    repos
        .entities
        .add_link(EntityKind::Task, "1", EntityKind::Decision, "1", "evidenced_by")
        .expect("link");

    let related = repos.search.related("task", "1").expect("related");
    assert_eq!(
        related.iter().map(|hit| hit.kind.clone()).collect::<Vec<_>>(),
        vec!["decision".to_string()],
        "the hop lands on the linked kind, not on every row sharing the id"
    );
}

#[test]
fn snapshots_and_maps_round_trip_through_their_own_ids() {
    let (repos, project) = fixture();
    let snapshot = Snapshot {
        id: "snap-1".into(),
        project_id: project.clone(),
        label: "before hardening".into(),
        state: vibe_domain::ProjectState {
            project_id: project.clone(),
            ..Default::default()
        },
        project_map_markdown: Some("# Project Map\n".into()),
        taken_at: vibe_domain::now(),
    };
    repos.snapshots.save(&snapshot).expect("save");
    repos.snapshots.save(&snapshot).expect("save twice");
    assert_eq!(repos.snapshots.list(&project).expect("list").len(), 1);
    assert_eq!(
        repos
            .snapshots
            .latest(&project)
            .expect("latest")
            .expect("present")
            .label,
        "before hardening"
    );

    let map = vibe_domain::ProjectMap {
        project_id: project.clone(),
        markdown: "# Project Map\n\n## Original Intent\n".into(),
        generated_at: vibe_domain::now(),
        source_state_hash: "hash-1".into(),
    };
    repos.snapshots.save_project_map(&map, Some("snap-1")).expect("map");
    repos.snapshots.save_project_map(&map, Some("snap-1")).expect("map twice");
    assert_eq!(
        repos
            .snapshots
            .latest_project_map(&project)
            .expect("latest map")
            .expect("present")
            .source_state_hash,
        "hash-1"
    );
}

#[test]
fn settings_overrides_and_audit_are_writable_more_than_once() {
    let (repos, project) = fixture();
    let value = serde_json::json!({ "autoAccept": 0.9 });
    repos.settings.set_json("judge", &value).expect("set");
    repos
        .settings
        .set_json("judge", &serde_json::json!({ "autoAccept": 0.8 }))
        .expect("overwrite");
    assert_eq!(
        repos
            .settings
            .get_json("judge")
            .expect("get")
            .expect("present")["autoAccept"],
        0.8
    );

    let override_ = UserOverride {
        id: "ovr-1".into(),
        project_id: project.clone(),
        subject_type: "task".into(),
        subject_id: "task-1".into(),
        field: "status".into(),
        value: serde_json::json!("done"),
        reason: "user confirmed by hand".into(),
        created_at: vibe_domain::now(),
    };
    repos.projects.save_override(&override_).expect("save");
    repos.projects.save_override(&override_).expect("save twice");
    let listed = repos.projects.list_overrides(&project).expect("list");
    assert_eq!(listed.len(), 1, "an override is keyed by id, not appended blindly");

    let audit = AuditLog {
        id: "audit-1".into(),
        project_id: project.clone(),
        component: "context-engine".into(),
        action: "build_context_pack".into(),
        read_refs: vec!["task-1".into()],
        produced_ref: Some("pack-1".into()),
        target_agent: Some("codex".into()),
        redacted_refs: vec![],
        created_at: vibe_domain::now(),
    };
    repos.audit.append(&audit).expect("append");
    repos.audit.append(&audit).expect("append twice");
    assert_eq!(repos.audit.list(&project, 10).expect("list").len(), 1);
}

#[test]
fn typed_entity_reads_agree_with_the_raw_port() {
    let (repos, project) = fixture();
    let milestone = Milestone {
        id: "m-1".into(),
        project_id: project.clone(),
        title: "V1-A foundation".into(),
        reached: false,
        reached_at: None,
    };
    repos
        .entities
        .put_entity(EntityKind::Milestone, &milestone, Some("open"))
        .expect("put");
    let typed: Vec<Milestone> = repos
        .entities
        .entities_of(EntityKind::Milestone, &project)
        .expect("entities_of");
    assert_eq!(typed.len(), 1);
    let one: Option<Milestone> = repos
        .entities
        .entity(EntityKind::Milestone, "m-1")
        .expect("entity");
    assert_eq!(one.expect("present").title, "V1-A foundation");
    // `status` is a promoted column for filtering, so it is deliberately not part of
    // the payload the typed read returns.
    assert_eq!(
        repos
            .entities
            .get(EntityKind::Milestone, "m-1")
            .expect("raw get")
            .expect("present")["title"]
            .as_str(),
        Some("V1-A foundation")
    );
    assert_eq!(
        repos
            .entities
            .list(EntityKind::Milestone, &project)
            .expect("list by kind")
            .len(),
        1
    );

    repos.entities.delete(EntityKind::Milestone, "m-1").expect("delete");
    assert!(repos
        .entities
        .list(EntityKind::Milestone, &project)
        .expect("list")
        .is_empty());
}

#[test]
fn a_policy_and_its_default_are_stored_verbatim() {
    let (repos, project) = fixture();
    let policy = ProjectPolicy {
        local_only: true,
        allow_cloud_model: false,
        never_send_files: true,
        never_send_raw_conversation: false,
        redact_secrets: true,
    };
    repos
        .settings
        .set_json(
            &format!("policy:{project}"),
            &serde_json::to_value(&policy).expect("encode"),
        )
        .expect("set");
    let back: ProjectPolicy = serde_json::from_value(
        repos
            .settings
            .get_json(&format!("policy:{project}"))
            .expect("get")
            .expect("present"),
    )
    .expect("decode");
    assert_eq!(back, policy);

    // A record written before a field existed must still decode to the safe default.
    let partial: ProjectPolicy =
        serde_json::from_value(serde_json::json!({ "localOnly": true })).expect("decode partial");
    assert!(partial.local_only);
    assert!(partial.redact_secrets, "redaction defaults to on (§53)");
    assert!(!partial.allow_cloud_model);
}

#[test]
fn unknown_entity_kind_and_source_type_fail_loudly() {
    let (repos, project) = fixture();
    assert!(EntityKind::parse("not-a-kind").is_none());
    assert!(SourceType::parse("not-a-source").is_none());
    assert_eq!(
        repos
            .entities
            .list(EntityKind::Aep, &project)
            .expect("empty kind list")
            .len(),
        0
    );
}
