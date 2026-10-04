//! Fixture plumbing shared by the guard-engine suites: everything through the real
//! SQLite-backed ports, so a rule is tested against the store the CLI will use.

#![allow(dead_code)]

use vibe_core::event_store::EventStore;
use vibe_core::ports::Repositories;
use vibe_core::project_service::ProjectService;
use vibe_domain::{ProjectState, ScopeBoundary, Task};
use vibe_guard_engine::{DriftEngine, Inbox};
use vibe_storage::SqliteStore;

pub struct Fixture {
    pub repos: Repositories,
    pub events: EventStore,
    pub projects: ProjectService,
    pub engine: DriftEngine,
    pub inbox: Inbox,
    pub project_id: String,
}

pub fn fixture(name: &str) -> Fixture {
    let store = SqliteStore::open_in_memory().expect("open in-memory store");
    let repos = store.repositories();
    let projects = ProjectService::new(repos.clone());
    let project = projects
        .create_project(name, None, None, None)
        .expect("create project");
    Fixture {
        events: EventStore::new(repos.clone()),
        engine: DriftEngine::new(repos.clone()),
        inbox: Inbox::new(repos.clone()),
        project_id: project.id,
        projects,
        repos,
    }
}

pub fn state(project_id: &str) -> ProjectState {
    ProjectState {
        project_id: project_id.to_string(),
        ..Default::default()
    }
}

pub fn task(id: &str, project_id: &str, title: &str) -> Task {
    Task {
        id: id.to_string(),
        project_id: project_id.to_string(),
        title: title.to_string(),
        ..Default::default()
    }
}

pub fn scope(project_id: &str) -> ScopeBoundary {
    ScopeBoundary {
        id: "scope-1".to_string(),
        project_id: project_id.to_string(),
        version: 1,
        ..Default::default()
    }
}
