//! State objects, artifacts, evidence and snapshots.
//!
//! Tasks, issues, decisions, milestones, change proposals, AEPs and drift
//! alerts share one `entities` table (kind + status + JSON payload) so adding a
//! state object does not need a migration — V1_DEVELOPMENT.md §50.

use rusqlite::{params, Row};
use vibe_core::error::{CoreError, Result};
use vibe_core::ports::{
    ArtifactRepository, EntityRepository, EvidenceRepository, SearchIndex, SnapshotRepository,
};
use vibe_domain::{Artifact, ArtifactKind, EntityKind, Evidence, EvidenceType, ProjectMap, Snapshot};

use crate::{OptionalExt, SqliteStore, bool_col, json_col, json_val, opt_text, parse_ts, storage_err, to_ts};

fn artifact_kind_from(slug: &str) -> ArtifactKind {
    serde_json::from_str(&format!("\"{slug}\"")).unwrap_or(ArtifactKind::SourceFile)
}

/// §40 searches what a record *says*, not how it is encoded. The payload reaches this
/// layer as JSON because the port is kind-blind, so the text is lifted out
/// structurally: every string leaf, minus the content hashes and ids that L1 already
/// matches exactly. Indexing the serialized form instead made field names and 64-hex
/// subject ids searchable, and turned every result snippet into a wall of braces.
pub(crate) fn searchable_text(value: &serde_json::Value) -> String {
    let mut found = Vec::new();
    collect_text(value, &mut found);
    found.join(". ")
}

fn collect_text<'a>(value: &'a serde_json::Value, out: &mut Vec<&'a str>) {
    match value {
        serde_json::Value::String(text) => {
            if !text.trim().is_empty() && !is_opaque(text) {
                out.push(text);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_text(item, out);
            }
        }
        serde_json::Value::Object(fields) => {
            for item in fields.values() {
                collect_text(item, out);
            }
        }
        _ => {}
    }
}

/// Ids, hashes and refs: long, hex-shaped, and L1's business.
fn is_opaque(text: &str) -> bool {
    text.len() >= 32 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn map_artifact(row: &Row<'_>) -> rusqlite::Result<Artifact> {
    Ok(Artifact {
        id: row.get(0)?,
        project_id: row.get(1)?,
        name: row.get(2)?,
        kind: artifact_kind_from(&row.get::<_, String>(3)?),
        path: opt_text(row, 4),
        hash: opt_text(row, 5),
        version: row.get::<_, i64>(6)? as u32,
        relationships: json_val(row.get::<_, String>(7)?)?,
        deleted: bool_col(row, 8),
        redesign_count: row.get::<_, i64>(9)? as u32,
        updated_at: opt_text(row, 10).map(|v| parse_ts(&v)),
    })
}

impl EntityRepository for SqliteStore {
    fn upsert(
        &self,
        kind: EntityKind,
        project_id: &str,
        id: &str,
        status: Option<&str>,
        data: &serde_json::Value,
    ) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO entities (kind, id, project_id, status, data_json, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6)
             ON CONFLICT(kind, id) DO UPDATE SET status = excluded.status,
                data_json = excluded.data_json, updated_at = excluded.updated_at",
            params![
                kind.as_str(),
                id,
                project_id,
                status,
                json_col(data)?,
                to_ts(&vibe_domain::now())
            ],
        )
        .map_err(storage_err)?;
        drop(conn);
        let title = data
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or(kind.as_str())
            .to_string();
        self.index(kind.as_str(), id, project_id, &title, &searchable_text(data))
    }

    fn get(&self, kind: EntityKind, id: &str) -> Result<Option<serde_json::Value>> {
        let conn = self.conn()?;
        let raw: Option<String> = conn
            .query_row(
                "SELECT data_json FROM entities WHERE kind = ?1 AND id = ?2",
                params![kind.as_str(), id],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_err)?;
        raw.map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(|e| CoreError::Encoding(e.to_string()))
    }

    fn list(&self, kind: EntityKind, project_id: &str) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT data_json FROM entities WHERE kind = ?1 AND project_id = ?2 ORDER BY rowid")
            .map_err(storage_err)?;
        let rows = stmt
            .query_map(params![kind.as_str(), project_id], |row| row.get::<_, String>(0))
            .map_err(storage_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_err)?;
        rows
            .iter()
            .map(|raw| {
                serde_json::from_str(raw)
                    .map_err(|e| CoreError::Encoding(format!("{kind:?}: {e}")))
            })
            .collect()
    }

    fn delete(&self, kind: EntityKind, id: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "DELETE FROM entities WHERE kind = ?1 AND id = ?2",
            params![kind.as_str(), id],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    fn add_link(
        &self,
        from_kind: EntityKind,
        from_id: &str,
        to_kind: EntityKind,
        to_id: &str,
        relation: &str,
    ) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR IGNORE INTO entity_links (from_kind, from_id, to_kind, to_id, relation)
             VALUES (?1,?2,?3,?4,?5)",
            params![from_kind.as_str(), from_id, to_kind.as_str(), to_id, relation],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    fn links_from(
        &self,
        from_kind: EntityKind,
        from_id: &str,
    ) -> Result<Vec<(EntityKind, String, String)>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT to_kind, to_id, relation FROM entity_links WHERE from_kind = ?1 AND from_id = ?2")
            .map_err(storage_err)?;
        let rows = stmt
            .query_map(params![from_kind.as_str(), from_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
            })
            .map_err(storage_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_err)?;
        Ok(rows
            .into_iter()
            .filter_map(|(kind, id, relation)| {
                EntityKind::parse(&kind).map(|k| (k, id, relation))
            })
            .collect())
    }

    fn record_version(
        &self,
        kind: EntityKind,
        id: &str,
        version: u32,
        before: Option<&serde_json::Value>,
        after: &serde_json::Value,
        reason: &str,
    ) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO entity_versions
                (kind, entity_id, version, before_json, after_json, reason, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                kind.as_str(),
                id,
                version as i64,
                before.map(|v| v.to_string()),
                after.to_string(),
                reason,
                to_ts(&vibe_domain::now())
            ],
        )
        .map_err(storage_err)?;
        Ok(())
    }
}

impl ArtifactRepository for SqliteStore {
    fn upsert_artifact(&self, artifact: &Artifact) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO artifacts (id, project_id, name, kind, path, hash, version,
                    relationships_json, deleted, redesign_count, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, hash = excluded.hash,
                version = excluded.version, relationships_json = excluded.relationships_json,
                deleted = excluded.deleted, redesign_count = excluded.redesign_count,
                updated_at = excluded.updated_at",
            params![
                artifact.id,
                artifact.project_id,
                artifact.name,
                kind_slug(&artifact.kind),
                artifact.path,
                artifact.hash,
                artifact.version as i64,
                json_col(&artifact.relationships)?,
                artifact.deleted as i64,
                artifact.redesign_count as i64,
                artifact
                    .updated_at
                    .map(|v| to_ts(&v))
                    .unwrap_or_else(|| to_ts(&vibe_domain::now()))
            ],
        )
        .map_err(storage_err)?;
        drop(conn);
        self.index(
            "artifact",
            &artifact.id,
            &artifact.project_id,
            &artifact.name,
            &artifact.path.clone().unwrap_or_default(),
        )
    }

    fn get_artifact(&self, project_id: &str, id: &str) -> Result<Option<Artifact>> {
        let conn = self.conn()?;
        let rows1 = conn
            .query_row(
                &format!("SELECT {} FROM artifacts WHERE project_id = ?1 AND id = ?2", ART_COLUMNS),
                params![project_id, id],
                map_artifact,
            )
            .optional()
            .map_err(storage_err)?;
        Ok(rows1)
    }

    fn find_artifact_by_path(&self, project_id: &str, path: &str) -> Result<Option<Artifact>> {
        let conn = self.conn()?;
        let rows2 = conn
            .query_row(
                &format!("SELECT {ART_COLUMNS} FROM artifacts WHERE project_id = ?1 AND path = ?2"),
                params![project_id, path],
                map_artifact,
            )
            .optional()
            .map_err(storage_err)?;
        Ok(rows2)
    }

    fn list_artifacts(&self, project_id: &str) -> Result<Vec<Artifact>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {ART_COLUMNS} FROM artifacts WHERE project_id = ?1 ORDER BY path"
            ))
            .map_err(storage_err)?;
        let rows3 = stmt
            .query_map(params![project_id], map_artifact)
            .map_err(storage_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(storage_err)?;
        Ok(rows3)
    }
}

const ART_COLUMNS: &str = "id, project_id, name, kind, path, hash, version, relationships_json, \
    deleted, redesign_count, updated_at";

fn kind_slug(kind: &ArtifactKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "source_file".to_string())
}

impl EvidenceRepository for SqliteStore {
    fn insert_evidence(&self, evidence: &Evidence) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR IGNORE INTO evidence (id, project_id, type, ref_id, summary, created_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                evidence.id,
                evidence.project_id,
                evidence.r#type.as_str(),
                evidence.ref_id,
                evidence.summary,
                to_ts(&evidence.created_at)
            ],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    fn get_evidence(&self, id: &str) -> Result<Option<Evidence>> {
        let conn = self.conn()?;
        let rows4 = conn
            .query_row(
                "SELECT id, project_id, type, ref_id, summary, created_at FROM evidence WHERE id = ?1",
                params![id],
                map_evidence,
            )
            .optional()
            .map_err(storage_err)?;
        Ok(rows4)
    }

    fn list_evidence(&self, project_id: &str) -> Result<Vec<Evidence>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT id, project_id, type, ref_id, summary, created_at FROM evidence WHERE project_id = ?1 ORDER BY created_at")
            .map_err(storage_err)?;
        let rows5 = stmt
            .query_map(params![project_id], map_evidence)
            .map_err(storage_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(storage_err)?;
        Ok(rows5)
    }

    fn find_by_ref(&self, r#type: EvidenceType, ref_id: &str) -> Result<Option<Evidence>> {
        let conn = self.conn()?;
        let rows6 = conn
            .query_row(
                "SELECT id, project_id, type, ref_id, summary, created_at FROM evidence WHERE type = ?1 AND ref_id = ?2",
                params![r#type.as_str(), ref_id],
                map_evidence,
            )
            .optional()
            .map_err(storage_err)?;
        Ok(rows6)
    }

    fn link(&self, evidence_id: &str, subject_kind: EntityKind, subject_id: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR IGNORE INTO evidence_links (evidence_id, subject_kind, subject_id) VALUES (?1,?2,?3)",
            params![evidence_id, subject_kind.as_str(), subject_id],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    fn for_subject(&self, subject_kind: EntityKind, subject_id: &str) -> Result<Vec<Evidence>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT e.id, e.project_id, e.type, e.ref_id, e.summary, e.created_at
                 FROM evidence e JOIN evidence_links l ON l.evidence_id = e.id
                 WHERE l.subject_kind = ?1 AND l.subject_id = ?2 ORDER BY e.created_at",
            )
            .map_err(storage_err)?;
        let rows7 = stmt
            .query_map(params![subject_kind.as_str(), subject_id], map_evidence)
            .map_err(storage_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(storage_err)?;
        Ok(rows7)
    }
}

fn map_evidence(row: &Row<'_>) -> rusqlite::Result<Evidence> {
    Ok(Evidence {
        id: row.get(0)?,
        project_id: row.get(1)?,
        r#type: match row.get::<_, String>(2)?.as_str() {
            "message" => EvidenceType::Message,
            "file" => EvidenceType::File,
            "git" => EvidenceType::Git,
            "test" => EvidenceType::Test,
            "build" => EvidenceType::Build,
            "system" => EvidenceType::System,
            _ => EvidenceType::Manual,
        },
        ref_id: opt_text(row, 3),
        summary: opt_text(row, 4),
        created_at: parse_ts(&row.get::<_, String>(5)?),
    })
}

impl SnapshotRepository for SqliteStore {
    fn save(&self, snapshot: &Snapshot) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO snapshots (id, project_id, label, state_json,
                    project_map_markdown, taken_at) VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                snapshot.id,
                snapshot.project_id,
                snapshot.label,
                json_col(&snapshot.state)?,
                snapshot.project_map_markdown,
                to_ts(&snapshot.taken_at)
            ],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    fn get(&self, id: &str) -> Result<Option<Snapshot>> {
        let conn = self.conn()?;
        let rows8 = conn
            .query_row(
                "SELECT id, project_id, label, state_json, project_map_markdown, taken_at FROM snapshots WHERE id = ?1",
                params![id],
                map_snapshot,
            )
            .optional()
            .map_err(storage_err)?;
        Ok(rows8)
    }

    fn list(&self, project_id: &str) -> Result<Vec<Snapshot>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT id, project_id, label, state_json, project_map_markdown, taken_at FROM snapshots WHERE project_id = ?1 ORDER BY taken_at")
            .map_err(storage_err)?;
        let rows9 = stmt
            .query_map(params![project_id], map_snapshot)
            .map_err(storage_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(storage_err)?;
        Ok(rows9)
    }

    fn latest(&self, project_id: &str) -> Result<Option<Snapshot>> {
        let conn = self.conn()?;
        let rows10 = conn
            .query_row(
                "SELECT id, project_id, label, state_json, project_map_markdown, taken_at
                 FROM snapshots WHERE project_id = ?1 ORDER BY taken_at DESC LIMIT 1",
                params![project_id],
                map_snapshot,
            )
            .optional()
            .map_err(storage_err)?;
        Ok(rows10)
    }

    fn save_project_map(&self, map: &ProjectMap, snapshot_id: Option<&str>) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO project_map_snapshots (id, project_id, snapshot_id, markdown, generated_at, source_state_hash)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                vibe_domain::new_id(),
                map.project_id,
                snapshot_id,
                map.markdown,
                to_ts(&map.generated_at),
                map.source_state_hash
            ],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    fn list_project_maps(&self, project_id: &str) -> Result<Vec<ProjectMap>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT project_id, markdown, generated_at, source_state_hash FROM project_map_snapshots WHERE project_id = ?1 ORDER BY generated_at")
            .map_err(storage_err)?;
        let rows11 = stmt
            .query_map(params![project_id], map_project_map)
            .map_err(storage_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(storage_err)?;
        Ok(rows11)
    }

    fn latest_project_map(&self, project_id: &str) -> Result<Option<ProjectMap>> {
        let conn = self.conn()?;
        let rows12 = conn
            .query_row(
                "SELECT project_id, markdown, generated_at, source_state_hash
                 FROM project_map_snapshots WHERE project_id = ?1 ORDER BY generated_at DESC LIMIT 1",
                params![project_id],
                map_project_map,
            )
            .optional()
            .map_err(storage_err)?;
        Ok(rows12)
    }
}

fn map_snapshot(row: &Row<'_>) -> rusqlite::Result<Snapshot> {
    Ok(Snapshot {
        id: row.get(0)?,
        project_id: row.get(1)?,
        label: row.get(2)?,
        state: json_val(row.get::<_, String>(3)?)?,
        project_map_markdown: opt_text(row, 4),
        taken_at: parse_ts(&row.get::<_, String>(5)?),
    })
}

fn map_project_map(row: &Row<'_>) -> rusqlite::Result<ProjectMap> {
    Ok(ProjectMap {
        project_id: row.get(0)?,
        markdown: row.get(1)?,
        generated_at: parse_ts(&row.get::<_, String>(2)?),
        source_state_hash: row.get(3)?,
    })
}
