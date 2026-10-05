//! §28: only an explicit id or an exact title merges. Anything weaker is reported as a
//! candidate and left for the user, because a wrong merge is a lost project fact.

use std::sync::Arc;

use vibe_core::ports::{EntityStore, Repositories};
use vibe_core::project_service::ProjectService;
use vibe_core::resolution::{EntityResolver, AUTO_MERGE_CONFIDENCE};
use vibe_domain::{EntityKind, Task};
use vibe_storage::SqliteStore;

fn seeded() -> (Arc<SqliteStore>, Repositories, String) {
    let store = SqliteStore::open_in_memory().expect("in-memory store");
    let repositories = store.repositories();
    let project = ProjectService::new(repositories.clone())
        .create_project("Resolution", None, None, None)
        .expect("project")
        .id;
    for (id, title) in [
        ("t-auth", "auth module"),
        ("t-cache", "cache design"),
        ("t-login", "登录模块"),
    ] {
        repositories
            .entities
            .put_entity(
                EntityKind::Task,
                &Task {
                    id: id.into(),
                    project_id: project.clone(),
                    title: title.into(),
                    ..Default::default()
                },
                Some("open"),
            )
            .expect("seed task");
    }
    (store, repositories, project)
}

fn resolve(repos: &Repositories, project: &str, title: &str) -> Option<f64> {
    EntityResolver::new(repos)
        .resolve_title(project, EntityKind::Task, title)
        .expect("resolve")
        .map(|found| found.confidence)
}

#[test]
fn an_explicit_id_and_an_exact_title_are_the_only_two_ways_to_merge() {
    let (_store, repos, project) = seeded();
    let resolver = EntityResolver::new(&repos);

    let by_id = resolver
        .resolve_title(&project, EntityKind::Task, "t-auth")
        .expect("resolve")
        .expect("explicit id");
    assert_eq!((by_id.id.as_str(), by_id.basis), ("t-auth", "explicit id"));
    assert_eq!(by_id.confidence, 1.0);
    assert!(by_id.auto_mergeable());

    let by_title = resolver
        .resolve_title(&project, EntityKind::Task, "登录模块")
        .expect("resolve")
        .expect("exact title");
    assert_eq!((by_title.id.as_str(), by_title.basis), ("t-login", "exact title"));
    assert!(
        by_title.auto_mergeable(),
        "0.95 is above the {AUTO_MERGE_CONFIDENCE} floor"
    );
}

#[test]
fn a_reordered_name_is_a_candidate_and_never_an_auto_merge() {
    let (_store, repos, project) = seeded();
    let found = EntityResolver::new(&repos)
        .resolve_title(&project, EntityKind::Task, "module auth")
        .expect("resolve")
        .expect("title overlap");
    assert_eq!((found.id.as_str(), found.basis), ("t-auth", "title overlap"));
    assert_eq!(found.confidence, 0.70);
    assert!(
        !found.auto_mergeable(),
        "an overlap is a hint for the user, not a merge (§28)"
    );
    assert!(
        resolve(&repos, &project, "auth module")
            .expect("exact match")
            >= AUTO_MERGE_CONFIDENCE,
        "the exact title still merges, so the floor is not the whole point"
    );
}

#[test]
fn a_name_nobody_used_yet_resolves_to_nothing() {
    let (_store, repos, project) = seeded();
    assert_eq!(resolve(&repos, &project, "云端同步"), None);
    assert_eq!(resolve(&repos, &project, "tokenizer"), None);
}
