//! Event Store port. `seq` is assigned by SQLite so replay order is fixed, and
//! `dedup_key` makes re-ingestion a no-op instead of a duplicate fact (§51, §52).

use rusqlite::{params, params_from_iter, Row};
use vibe_core::error::{CoreError, Result};
use vibe_core::ports::{AppendOutcome, EventRepository, SearchIndex};
use vibe_domain::{ActorType, EventStatus, ProjectEvent};

use crate::{OptionalExt, SqliteStore, json_col, json_val, opt_text, parse_ts, storage_err, to_ts};

const EVENT_COLUMNS: &str = "seq, id, dedup_key, project_id, type, schema_version, actor_type, \
    actor_id, source_id, conversation_id, message_id, origin_node_id, causation_id, \
    correlation_id, subject_type, subject_id, payload_json, confidence, evidence_refs_json, \
    status, occurred_at, observed_at";

pub(crate) fn map_event(row: &Row<'_>) -> rusqlite::Result<ProjectEvent> {
    Ok(ProjectEvent {
        seq: Some(row.get::<_, i64>(0)?),
        id: row.get(1)?,
        project_id: row.get(3)?,
        r#type: row.get(4)?,
        schema_version: row.get::<_, i64>(5)? as u32,
        actor_type: match row.get::<_, String>(6)?.as_str() {
            "user" => ActorType::User,
            "agent" => ActorType::Agent,
            _ => ActorType::System,
        },
        actor_id: opt_text(row, 7),
        source_id: opt_text(row, 8),
        conversation_id: opt_text(row, 9),
        message_id: opt_text(row, 10),
        origin_node_id: opt_text(row, 11),
        causation_id: opt_text(row, 12),
        correlation_id: opt_text(row, 13),
        subject_type: opt_text(row, 14),
        subject_id: opt_text(row, 15),
        payload: json_val(row.get::<_, String>(16)?)?,
        confidence: row.get(17)?,
        evidence_refs: json_val(row.get::<_, String>(18)?)?,
        status: status_from(&row.get::<_, String>(19)?),
        occurred_at: opt_text(row, 20).map(|v| parse_ts(&v)),
        observed_at: parse_ts(&row.get::<_, String>(21)?),
    })
}

fn status_from(value: &str) -> EventStatus {
    match value {
        "accepted" => EventStatus::Accepted,
        "pending_review" => EventStatus::PendingReview,
        "rejected" => EventStatus::Rejected,
        _ => EventStatus::Superseded,
    }
}

pub(crate) fn status_to_str(status: EventStatus) -> &'static str {
    match status {
        EventStatus::Accepted => "accepted",
        EventStatus::PendingReview => "pending_review",
        EventStatus::Rejected => "rejected",
        EventStatus::Superseded => "superseded",
    }
}

impl SqliteStore {
    fn read_by_dedup(&self, conn: &rusqlite::Connection, dedup_key: &str) -> Result<ProjectEvent> {
        conn.query_row(
            &format!("SELECT {EVENT_COLUMNS} FROM events WHERE dedup_key = ?1"),
            params![dedup_key],
            map_event,
        )
        .map_err(|e| {
            CoreError::NotFound {
                what: format!("event for dedup key ({dedup_key}): {e}"),
            }
        })
    }
}

impl EventRepository for SqliteStore {
    fn append(&self, event: &ProjectEvent, dedup_key: &str) -> Result<AppendOutcome> {
        let conn = self.conn()?;
        let inserted = conn
            .execute(
                "INSERT INTO events (id, dedup_key, project_id, type, schema_version, actor_type,
                    actor_id, source_id, conversation_id, message_id, origin_node_id, causation_id,
                    correlation_id, subject_type, subject_id, payload_json, confidence,
                    evidence_refs_json, status, occurred_at, observed_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)
                 ON CONFLICT(dedup_key) DO NOTHING",
                params![
                    event.id,
                    dedup_key,
                    event.project_id,
                    event.r#type,
                    event.schema_version as i64,
                    match event.actor_type {
                        ActorType::User => "user",
                        ActorType::Agent => "agent",
                        ActorType::System => "system",
                    },
                    event.actor_id,
                    event.source_id,
                    event.conversation_id,
                    event.message_id,
                    event.origin_node_id,
                    event.causation_id,
                    event.correlation_id,
                    event.subject_type,
                    event.subject_id,
                    json_col(&event.payload)?,
                    event.confidence,
                    json_col(&event.evidence_refs)?,
                    status_to_str(event.status),
                    event.occurred_at.as_ref().map(to_ts),
                    to_ts(&event.observed_at)
                ],
            )
            .map_err(storage_err)?;

        if inserted == 0 {
            let existing = self.read_by_dedup(&conn, dedup_key)?;
            return Ok(AppendOutcome {
                event: existing,
                inserted: false,
            });
        }

        let seq = conn.last_insert_rowid();
        drop(conn);

        let body = format!(
            "{} {}",
            event.r#type,
            crate::entity::searchable_text(&event.payload)
        );
        self.index("event", &event.id, &event.project_id, &event.r#type, &body)?;

        Ok(AppendOutcome {
            event: ProjectEvent {
                seq: Some(seq),
                ..event.clone()
            },
            inserted: true,
        })
    }

    fn list_accepted(&self, project_id: &str, after_seq: Option<i64>) -> Result<Vec<ProjectEvent>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {EVENT_COLUMNS} FROM events
                 WHERE project_id = ?1 AND status = 'accepted' AND (?2 IS NULL OR seq > ?2)
                 ORDER BY seq"
            ))
            .map_err(storage_err)?;
        let rows1 = stmt
            .query_map(params![project_id, after_seq], map_event)
            .map_err(storage_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(storage_err)?;
        Ok(rows1)
    }

    fn list_events(
        &self,
        project_id: &str,
        statuses: Option<&[EventStatus]>,
        limit: Option<usize>,
    ) -> Result<Vec<ProjectEvent>> {
        let conn = self.conn()?;
        let mut sql = format!("SELECT {EVENT_COLUMNS} FROM events WHERE project_id = ?1");
        let mut status_values: Vec<String> = Vec::new();
        if let Some(list) = statuses {
            if !list.is_empty() {
                status_values = list.iter().map(|s| status_to_str(*s).to_string()).collect();
                let placeholders = (0..status_values.len())
                    .map(|i| format!("?{}", i + 2))
                    .collect::<Vec<_>>()
                    .join(", ");
                sql.push_str(&format!(" AND status IN ({placeholders})"));
            }
        }
        sql.push_str(" ORDER BY seq DESC");
        if let Some(size) = limit {
            sql.push_str(&format!(" LIMIT {size}"));
        }
        let mut stmt = conn.prepare(&sql).map_err(storage_err)?;
        let mut args: Vec<&dyn rusqlite::ToSql> = vec![&project_id];
        for value in &status_values {
            args.push(value);
        }
        let rows2 = stmt
            .query_map(params_from_iter(args), map_event)
            .map_err(storage_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(storage_err)?;
        Ok(rows2)
    }

    fn get_event(&self, id: &str) -> Result<Option<ProjectEvent>> {
        let conn = self.conn()?;
        let rows3 = conn
            .query_row(
                &format!("SELECT {EVENT_COLUMNS} FROM events WHERE id = ?1"),
                params![id],
                map_event,
            )
            .optional()
            .map_err(storage_err)?;
        Ok(rows3)
    }

    fn patch_event(&self, id: &str, patch: &vibe_domain::ProjectEventPatch) -> Result<()> {
        if patch.status.is_none() && patch.confidence.is_none() && patch.evidence_refs.is_none() {
            return Ok(());
        }
        let conn = self.conn()?;
        conn.execute(
            "UPDATE events SET
                status = COALESCE(?2, status),
                confidence = COALESCE(?3, confidence),
                evidence_refs_json = COALESCE(?4, evidence_refs_json)
             WHERE id = ?1",
            params![
                id,
                patch.status.map(status_to_str),
                patch.confidence,
                match &patch.evidence_refs {
                    Some(refs) => Some(json_col(refs)?),
                    None => None,
                }
            ],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    fn contains_dedup_key(&self, dedup_key: &str) -> Result<bool> {
        let conn = self.conn()?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE dedup_key = ?1",
                params![dedup_key],
                |row| row.get(0),
            )
            .map_err(storage_err)?;
        Ok(count > 0)
    }
}
