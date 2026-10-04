//! Golden fixture: the compliant side of INV-001. This module talks to the
//! repository ports and compiles to nothing that names a database.

pub fn load_project_state(repos: &Ports, project_id: &str) -> Option<ProjectState> {
    repos.events.list_accepted(project_id).and_then(|rows| {
        let hash = crate::hash_of(&rows);
        repos.state.put(ProjectState {
            project_id: project_id.to_string(),
            state_hash: Some(hash),
            ..Default::default()
        })
        .ok()?;
        repos.state.get(project_id)
    })
}
