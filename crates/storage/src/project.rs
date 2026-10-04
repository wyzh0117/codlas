//! Project + baseline port implementation. Baseline tables are versioned: an
//! update inserts a new row and old rows stay readable for diff and audit.

use rusqlite::{params, Row};
use vibe_core::error::Result;
use vibe_core::ports::ProjectRepository;
use vibe_domain::{
    ArchitectureBaseline, Project, ProjectCharter, ProjectStatus, ScopeBoundary, UserOverride,
};

use crate::{json_col, json_val, not_found, opt_text, parse_ts, storage_err, to_ts, OptionalExt, SqliteStore};

const PROJECT_COLUMNS: &str =
    "id, name, description, root_path, status, current_phase, created_at, updated_at";

fn map_project(row: &Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        description: opt_text(row, 2),
        root_path: opt_text(row, 3),
        status: match row.get::<_, String>(4)?.as_str() {
            "paused" => ProjectStatus::Paused,
            "archived" => ProjectStatus::Archived,
            _ => ProjectStatus::Active,
        },
        current_phase: opt_text(row, 5),
        created_at: parse_ts(&row.get::<_, String>(6)?),
        updated_at: parse_ts(&row.get::<_, String>(7)?),
    })
}

fn map_charter(row: &Row<'_>) -> rusqlite::Result<ProjectCharter> {
    Ok(ProjectCharter {
        id: row.get(0)?,
        project_id: row.get(1)?,
        original_problem: row.get(2)?,
        target_user: opt_text(row, 3),
        core_value: row.get(4)?,
        must_have: json_val(row.get::<_, String>(5)?)?,
        wont_have: json_val(row.get::<_, String>(6)?)?,
        success_criteria: json_val(row.get::<_, String>(7)?)?,
        architecture_principles: json_val(row.get::<_, String>(8)?)?,
        constraints: json_val(row.get::<_, String>(9)?)?,
        version: row.get::<_, i64>(10)? as u32,
        locked: row.get::<_, i64>(11)? != 0,
        created_at: parse_ts(&row.get::<_, String>(12)?),
        updated_at: parse_ts(&row.get::<_, String>(13)?),
    })
}

fn map_scope(row: &Row<'_>) -> rusqlite::Result<ScopeBoundary> {
    Ok(ScopeBoundary {
        id: row.get(0)?,
        project_id: row.get(1)?,
        in_scope: json_val(row.get::<_, String>(2)?)?,
        out_of_scope: json_val(row.get::<_, String>(3)?)?,
        deferred: json_val(row.get::<_, String>(4)?)?,
        forbidden: json_val(row.get::<_, String>(5)?)?,
        version: row.get::<_, i64>(6)? as u32,
    })
}

fn map_baseline(row: &Row<'_>) -> rusqlite::Result<ArchitectureBaseline> {
    Ok(ArchitectureBaseline {
        id: row.get(0)?,
        project_id: row.get(1)?,
        summary: row.get(2)?,
        layers: json_val(row.get::<_, String>(3)?)?,
        invariants: json_val(row.get::<_, String>(4)?)?,
        replaceable_parts: json_val(row.get::<_, String>(5)?)?,
        version: row.get::<_, i64>(6)? as u32,
        active: row.get::<_, i64>(7)? != 0,
    })
}

impl ProjectRepository for SqliteStore {
    fn create_project(&self, project: &Project) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO projects (id, name, description, root_path, status, current_phase, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                project.id,
                project.name,
                project.description,
                project.root_path,
                status_str(project),
                project.current_phase,
                to_ts(&project.created_at),
                to_ts(&project.updated_at)
            ],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    fn update_project(&self, project: &Project) -> Result<()> {
        let conn = self.conn()?;
        let changed = conn
            .execute(
                "UPDATE projects SET name=?2, description=?3, root_path=?4, status=?5,
                 current_phase=?6, updated_at=?7 WHERE id=?1",
                params![
                    project.id,
                    project.name,
                    project.description,
                    project.root_path,
                    status_str(project),
                    project.current_phase,
                    to_ts(&project.updated_at)
                ],
            )
            .map_err(storage_err)?;
        if changed == 0 {
            return Err(not_found(&format!("project {}", project.id)));
        }
        Ok(())
    }

    fn get_project(&self, id: &str) -> Result<Option<Project>> {
        let conn = self.conn()?;
        let found = conn
            .query_row(
                &format!("SELECT {PROJECT_COLUMNS} FROM projects WHERE id = ?1"),
                params![id],
                map_project,
            )
            .optional()
            .map_err(storage_err)?;
        Ok(found)
    }

    fn list_projects(&self) -> Result<Vec<Project>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {PROJECT_COLUMNS} FROM projects ORDER BY created_at DESC"
            ))
            .map_err(storage_err)?;
        let rows = stmt
            .query_map([], map_project)
            .map_err(storage_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(storage_err)?;
        Ok(rows)
    }

    fn insert_charter_version(&self, charter: &ProjectCharter) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO project_charters (id, project_id, original_problem, target_user, core_value,
                must_have_json, wont_have_json, success_criteria_json, architecture_principles_json,
                constraints_json, version, locked, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            params![
                charter.id,
                charter.project_id,
                charter.original_problem,
                charter.target_user,
                charter.core_value,
                json_col(&charter.must_have)?,
                json_col(&charter.wont_have)?,
                json_col(&charter.success_criteria)?,
                json_col(&charter.architecture_principles)?,
                json_col(&charter.constraints)?,
                charter.version as i64,
                charter.locked as i64,
                to_ts(&charter.created_at),
                to_ts(&charter.updated_at)
            ],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    fn get_active_charter(&self, project_id: &str) -> Result<Option<ProjectCharter>> {
        let conn = self.conn()?;
        let rows1 = conn
            .query_row(
                "SELECT id, project_id, original_problem, target_user, core_value, must_have_json,
                        wont_have_json, success_criteria_json, architecture_principles_json,
                        constraints_json, version, locked, created_at, updated_at
                 FROM project_charters WHERE project_id = ?1 ORDER BY version DESC LIMIT 1",
                params![project_id],
                map_charter,
            )
            .optional()
            .map_err(storage_err)?;
        Ok(rows1)
    }

    fn list_charter_versions(&self, project_id: &str) -> Result<Vec<ProjectCharter>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, original_problem, target_user, core_value, must_have_json,
                        wont_have_json, success_criteria_json, architecture_principles_json,
                        constraints_json, version, locked, created_at, updated_at
                 FROM project_charters WHERE project_id = ?1 ORDER BY version",
            )
            .map_err(storage_err)?;
        let rows2 = stmt
            .query_map(params![project_id], map_charter)
            .map_err(storage_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(storage_err)?;
        Ok(rows2)
    }

    fn insert_scope_version(&self, scope: &ScopeBoundary) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO scope_boundaries (id, project_id, in_scope_json, out_of_scope_json,
                deferred_json, forbidden_json, version) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                scope.id,
                scope.project_id,
                json_col(&scope.in_scope)?,
                json_col(&scope.out_of_scope)?,
                json_col(&scope.deferred)?,
                json_col(&scope.forbidden)?,
                scope.version as i64
            ],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    fn get_active_scope(&self, project_id: &str) -> Result<Option<ScopeBoundary>> {
        let conn = self.conn()?;
        let rows3 = conn
            .query_row(
                "SELECT id, project_id, in_scope_json, out_of_scope_json, deferred_json,
                        forbidden_json, version
                 FROM scope_boundaries WHERE project_id = ?1 ORDER BY version DESC LIMIT 1",
                params![project_id],
                map_scope,
            )
            .optional()
            .map_err(storage_err)?;
        Ok(rows3)
    }

    fn list_scope_versions(&self, project_id: &str) -> Result<Vec<ScopeBoundary>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, in_scope_json, out_of_scope_json, deferred_json,
                        forbidden_json, version
                 FROM scope_boundaries WHERE project_id = ?1 ORDER BY version",
            )
            .map_err(storage_err)?;
        let rows4 = stmt
            .query_map(params![project_id], map_scope)
            .map_err(storage_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(storage_err)?;
        Ok(rows4)
    }

    fn insert_baseline_version(&self, baseline: &ArchitectureBaseline) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO architecture_baselines (id, project_id, summary, layers_json,
                invariants_json, replaceable_json, version, active) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                baseline.id,
                baseline.project_id,
                baseline.summary,
                json_col(&baseline.layers)?,
                json_col(&baseline.invariants)?,
                json_col(&baseline.replaceable_parts)?,
                baseline.version as i64,
                baseline.active as i64
            ],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    fn get_active_baseline(&self, project_id: &str) -> Result<Option<ArchitectureBaseline>> {
        let conn = self.conn()?;
        let rows5 = conn
            .query_row(
                "SELECT id, project_id, summary, layers_json, invariants_json, replaceable_json,
                        version, active
                 FROM architecture_baselines
                 WHERE project_id = ?1 AND active = 1 ORDER BY version DESC LIMIT 1",
                params![project_id],
                map_baseline,
            )
            .optional()
            .map_err(storage_err)?;
        Ok(rows5)
    }

    fn list_baseline_versions(&self, project_id: &str) -> Result<Vec<ArchitectureBaseline>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, summary, layers_json, invariants_json, replaceable_json,
                        version, active
                 FROM architecture_baselines WHERE project_id = ?1 ORDER BY version",
            )
            .map_err(storage_err)?;
        let rows6 = stmt
            .query_map(params![project_id], map_baseline)
            .map_err(storage_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(storage_err)?;
        Ok(rows6)
    }

    fn save_override(&self, override_: &UserOverride) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO user_overrides (id, project_id, subject_type, subject_id, field,
                value_json, reason, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                override_.id,
                override_.project_id,
                override_.subject_type,
                override_.subject_id,
                override_.field,
                json_col(&override_.value)?,
                override_.reason,
                to_ts(&override_.created_at)
            ],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    fn list_overrides(&self, project_id: &str) -> Result<Vec<UserOverride>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, subject_type, subject_id, field, value_json, reason, created_at
                 FROM user_overrides WHERE project_id = ?1 ORDER BY created_at",
            )
            .map_err(storage_err)?;
        let rows = stmt
            .query_map(params![project_id], |row| {
                Ok(UserOverride {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    subject_type: row.get(2)?,
                    subject_id: row.get(3)?,
                    field: row.get(4)?,
                    value: json_val(row.get::<_, String>(5)?)?,
                    reason: row.get(6)?,
                    created_at: parse_ts(&row.get::<_, String>(7)?),
                })
            })
            .map_err(storage_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(storage_err)?;
        Ok(rows)
    }
}

fn status_str(project: &Project) -> &'static str {
    match project.status {
        ProjectStatus::Active => "active",
        ProjectStatus::Paused => "paused",
        ProjectStatus::Archived => "archived",
    }
}
