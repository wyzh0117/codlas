//! Full-text index (SQLite FTS5), settings and audit trail.
//!
//! L1 structured search and L3 relation search are orchestrated in
//! `vibe-search`; this file only provides the store-specific L2 primitive
//! behind the [`SearchIndex`] port.

use rusqlite::{params, Row};
use vibe_core::error::{CoreError, Result};
use vibe_core::ports::{AuditRepository, SearchFilters, SearchHit, SearchIndex, SettingsRepository};
use vibe_domain::AuditLog;

use crate::{OptionalExt, SqliteStore, json_col, json_val, opt_text, parse_ts, storage_err, to_ts};

/// FTS5 has its own query grammar, so user text is turned into quoted phrases
/// ANDed together. This keeps a stray `"` or `:` from becoming syntax.
fn build_match(query: &str) -> String {
    let tokens: Vec<String> = query
        .split_whitespace()
        .map(|token| format!("\"{}\"", token.replace('"', "\"\"")))
        .collect();
    if tokens.is_empty() {
        return String::new();
    }
    tokens.join(" AND ")
}

fn map_hit(row: &Row<'_>) -> rusqlite::Result<SearchHit> {
    let rank: f64 = row.get(5)?;
    Ok(SearchHit {
        kind: row.get(0)?,
        id: row.get(1)?,
        project_id: row.get(2)?,
        title: row.get(3)?,
        snippet: row.get(4)?,
        // bm25 is negative-and-lower-is-better; flip it so callers can sort desc.
        rank: -rank,
    })
}

impl SqliteStore {
    fn rows_to_hits(
        &self,
        mut stmt: rusqlite::Statement,
        bind_values: &[&dyn rusqlite::ToSql],
    ) -> Result<Vec<SearchHit>> {
        let rows1 = stmt
            .query_map(bind_values, map_hit)
            .map_err(storage_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(storage_err)?;
        Ok(rows1)
    }
}

impl SearchIndex for SqliteStore {
    fn index(&self, kind: &str, id: &str, project_id: &str, title: &str, body: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "DELETE FROM search_index WHERE record_id = ?1 AND kind = ?2",
            params![id, kind],
        )
        .map_err(storage_err)?;
        conn.execute(
            "INSERT INTO search_index (kind, record_id, project_id, title, body) VALUES (?1,?2,?3,?4,?5)",
            params![kind, id, project_id, title, body],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    fn remove(&self, kind: &str, id: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "DELETE FROM search_index WHERE record_id = ?1 AND kind = ?2",
            params![id, kind],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    fn search(&self, query: &str, filters: &SearchFilters) -> Result<Vec<SearchHit>> {
        let match_query = build_match(query);
        if match_query.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn()?;
        let mut sql = String::from(
            "SELECT kind, record_id, project_id, title,
                    snippet(search_index, 4, '[', ']', ' … ', 12), rank
             FROM search_index WHERE search_index MATCH ?1",
        );
        let mut bind: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(match_query)];
        let mut index = 2;

        if let Some(project_id) = &filters.project_id {
            sql.push_str(&format!(" AND project_id = ?{index}"));
            bind.push(Box::new(project_id.clone()));
            index += 1;
        }
        if !filters.kinds.is_empty() {
            let placeholders = (0..filters.kinds.len())
                .map(|offset| format!("?{}", index + offset))
                .collect::<Vec<_>>()
                .join(", ");
            sql.push_str(&format!(" AND kind IN ({placeholders})"));
            for kind in &filters.kinds {
                bind.push(Box::new(kind.clone()));
            }
            index += filters.kinds.len();
        }
        sql.push_str(&format!(" ORDER BY rank LIMIT ?{index}"));
        bind.push(Box::new(filters.limit as i64));

        let refs: Vec<&dyn rusqlite::ToSql> = bind.iter().map(|b| b.as_ref()).collect();
        let stmt = conn.prepare(&sql).map_err(storage_err)?;
        self.rows_to_hits(stmt, &refs)
    }

    /// L3: everything the relation graph hangs off this record, in either
    /// direction (§40).
    fn related(&self, kind: &str, id: &str) -> Result<Vec<SearchHit>> {
        let conn = self.conn()?;
        let stmt = conn
            .prepare(
                "SELECT kind, record_id, project_id, title,
                        snippet(search_index, 4, '[', ']', ' … ', 12), 0.0
                 FROM search_index
                 WHERE (kind, record_id) IN (
                     SELECT CASE WHEN from_kind = ?1 AND from_id = ?2 THEN to_kind ELSE from_kind END,
                            CASE WHEN from_kind = ?1 AND from_id = ?2 THEN to_id ELSE from_id END
                     FROM entity_links
                     WHERE (from_kind = ?1 AND from_id = ?2) OR (to_kind = ?1 AND to_id = ?2)
                 )",
            )
            .map_err(storage_err)?;
        // The query is written with numbered parameters, so each value is bound once
        // and every `?1`/`?2` in the text reads that same binding.
        let refs: Vec<&dyn rusqlite::ToSql> = vec![&kind, &id];
        self.rows_to_hits(stmt, &refs)
    }
}

impl SettingsRepository for SqliteStore {
    fn get_json(&self, key: &str) -> Result<Option<serde_json::Value>> {
        let conn = self.conn()?;
        let raw: Option<String> = conn
            .query_row(
                "SELECT value_json FROM settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_err)?;
        raw.map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(|e| CoreError::Encoding(e.to_string()))
    }

    fn set_json(&self, key: &str, value: &serde_json::Value) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO settings (key, value_json) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
            params![key, json_col(value)?],
        )
        .map_err(storage_err)?;
        Ok(())
    }
}

impl AuditRepository for SqliteStore {
    fn append(&self, entry: &AuditLog) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO audit_logs (id, project_id, component, action, read_refs_json,
                    produced_ref, target_agent, redacted_json, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(id) DO NOTHING",
            params![
                entry.id,
                entry.project_id,
                entry.component,
                entry.action,
                json_col(&entry.read_refs)?,
                entry.produced_ref,
                entry.target_agent,
                json_col(&entry.redacted_refs)?,
                to_ts(&entry.created_at)
            ],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    fn list(&self, project_id: &str, limit: usize) -> Result<Vec<AuditLog>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, component, action, read_refs_json, produced_ref,
                        target_agent, redacted_json, created_at
                 FROM audit_logs WHERE project_id = ?1 ORDER BY created_at DESC LIMIT ?2",
            )
            .map_err(storage_err)?;
        let rows = stmt
            .query_map(params![project_id, limit as i64], |row| {
                Ok(AuditLog {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    component: row.get(2)?,
                    action: row.get(3)?,
                    read_refs: json_val(row.get::<_, String>(4)?)?,
                    produced_ref: opt_text(row, 5),
                    target_agent: opt_text(row, 6),
                    redacted_refs: json_val(row.get::<_, String>(7)?)?,
                    created_at: parse_ts(&row.get::<_, String>(8)?),
                })
            })
            .map_err(storage_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_err)?;
        Ok(rows)
    }
}
