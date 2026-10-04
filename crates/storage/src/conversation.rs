//! Capture records: sources, conversations, messages and sync cursors.
//! Message insertion is idempotent on external id (L1) then content hash (L2).

use rusqlite::{params, Row};
use vibe_core::error::{CoreError, Result};
use vibe_core::ports::{ConversationRepository, MessageInsertOutcome, SearchIndex};
use vibe_domain::{Conversation, Message, MessageRole, Source, SourceType, SyncCursor};

use crate::{OptionalExt, SqliteStore, json_col, json_val, opt_text, parse_ts, storage_err, to_ts};

fn role_from(value: &str) -> MessageRole {
    match value {
        "assistant" => MessageRole::Assistant,
        "system" => MessageRole::System,
        "tool" => MessageRole::Tool,
        _ => MessageRole::User,
    }
}

fn source_type_from(value: &str) -> SourceType {
    SourceType::parse(value).unwrap_or(SourceType::Manual)
}

fn map_source(row: &Row<'_>) -> rusqlite::Result<Source> {
    Ok(Source {
        id: row.get(0)?,
        project_id: row.get(1)?,
        r#type: source_type_from(&row.get::<_, String>(2)?),
        external_id: opt_text(row, 3),
        display_name: row.get(4)?,
        metadata: json_val(row.get::<_, String>(5)?)?,
    })
}

fn map_conversation(row: &Row<'_>) -> rusqlite::Result<Conversation> {
    Ok(Conversation {
        id: row.get(0)?,
        project_id: row.get(1)?,
        source_id: row.get(2)?,
        external_conversation_id: opt_text(row, 3),
        title: opt_text(row, 4),
        started_at: opt_text(row, 5).map(|v| parse_ts(&v)),
        last_seen_at: opt_text(row, 6).map(|v| parse_ts(&v)),
    })
}

fn map_message(row: &Row<'_>) -> rusqlite::Result<Message> {
    Ok(Message {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        role: role_from(&row.get::<_, String>(3)?),
        content: row.get(4)?,
        content_hash: row.get(5)?,
        external_message_id: opt_text(row, 6),
        created_at: opt_text(row, 7).map(|v| parse_ts(&v)),
        observed_at: parse_ts(&row.get::<_, String>(8)?),
    })
}

impl ConversationRepository for SqliteStore {
    fn upsert_source(&self, source: &Source) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO sources (id, project_id, type, external_id, display_name, metadata_json)
             VALUES (?1,?2,?3,?4,?5,?6)
             ON CONFLICT(id) DO UPDATE SET display_name = excluded.display_name,
                metadata_json = excluded.metadata_json",
            params![
                source.id,
                source.project_id,
                source.r#type.as_str(),
                source.external_id,
                source.display_name,
                json_col(&source.metadata)?
            ],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    fn get_source(&self, project_id: &str, id: &str) -> Result<Option<Source>> {
        let conn = self.conn()?;
        let rows1 = conn
            .query_row(
                "SELECT id, project_id, type, external_id, display_name, metadata_json
                 FROM sources WHERE project_id = ?1 AND id = ?2",
                params![project_id, id],
                map_source,
            )
            .optional()
            .map_err(storage_err)?;
        Ok(rows1)
    }

    fn find_source_by_external_id(
        &self,
        project_id: &str,
        source_type: &str,
        external_id: &str,
    ) -> Result<Option<Source>> {
        let conn = self.conn()?;
        let rows2 = conn
            .query_row(
                "SELECT id, project_id, type, external_id, display_name, metadata_json
                 FROM sources WHERE project_id = ?1 AND type = ?2 AND external_id = ?3",
                params![project_id, source_type, external_id],
                map_source,
            )
            .optional()
            .map_err(storage_err)?;
        Ok(rows2)
    }

    fn list_sources(&self, project_id: &str) -> Result<Vec<Source>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, type, external_id, display_name, metadata_json
                 FROM sources WHERE project_id = ?1 ORDER BY display_name",
            )
            .map_err(storage_err)?;
        let rows3 = stmt
            .query_map(params![project_id], map_source)
            .map_err(storage_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(storage_err)?;
        Ok(rows3)
    }

    fn upsert_conversation(&self, conversation: &Conversation) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO conversations (id, project_id, source_id, external_conversation_id, title,
                started_at, last_seen_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(id) DO UPDATE SET title = COALESCE(excluded.title, conversations.title),
                last_seen_at = COALESCE(excluded.last_seen_at, conversations.last_seen_at)",
            params![
                conversation.id,
                conversation.project_id,
                conversation.source_id,
                conversation.external_conversation_id,
                conversation.title,
                conversation.started_at.as_ref().map(to_ts),
                conversation.last_seen_at.as_ref().map(to_ts)
            ],
        )
        .map_err(storage_err)?;
        Ok(())
    }

    fn get_conversation(&self, project_id: &str, id: &str) -> Result<Option<Conversation>> {
        let conn = self.conn()?;
        let rows4 = conn
            .query_row(
                "SELECT id, project_id, source_id, external_conversation_id, title, started_at,
                        last_seen_at
                 FROM conversations WHERE project_id = ?1 AND id = ?2",
                params![project_id, id],
                map_conversation,
            )
            .optional()
            .map_err(storage_err)?;
        Ok(rows4)
    }

    fn list_conversations(&self, project_id: &str) -> Result<Vec<Conversation>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, source_id, external_conversation_id, title, started_at,
                        last_seen_at
                 FROM conversations WHERE project_id = ?1
                 ORDER BY COALESCE(last_seen_at, started_at) DESC",
            )
            .map_err(storage_err)?;
        let rows5 = stmt
            .query_map(params![project_id], map_conversation)
            .map_err(storage_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(storage_err)?;
        Ok(rows5)
    }

    fn insert_messages(
        &self,
        conversation_id: &str,
        messages: &[Message],
    ) -> Result<MessageInsertOutcome> {
        let mut conn = self.conn()?;
        let project_id: String = conn
            .query_row(
                "SELECT project_id FROM conversations WHERE id = ?1",
                params![conversation_id],
                |row| row.get(0),
            )
            .map_err(|_| CoreError::NotFound {
                what: format!("conversation {conversation_id}"),
            })?;

        let mut outcome = MessageInsertOutcome::default();
        for message in messages {
            let inserted = conn
                .execute(
                    "INSERT INTO messages (id, conversation_id, project_id, role, content,
                            content_hash, external_message_id, created_at, observed_at)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
                     ON CONFLICT(conversation_id, content_hash) DO NOTHING",
                    params![
                        message.id,
                        conversation_id,
                        project_id,
                        message.role.as_str(),
                        message.content,
                        message.content_hash,
                        message.external_message_id,
                        message.created_at.as_ref().map(to_ts),
                        to_ts(&message.observed_at)
                    ],
                )
                .map_err(storage_err)?;
            if inserted == 0 {
                outcome.duplicate_count += 1;
                continue;
            }
            drop(conn);
            self.index(
                "message",
                &message.id,
                &project_id,
                message.role.as_str(),
                &message.content,
            )?;
            conn = self.conn()?;
            outcome.inserted.push(message.clone());
        }
        Ok(outcome)
    }

    fn list_messages(&self, conversation_id: &str) -> Result<Vec<Message>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, conversation_id, project_id, role, content, content_hash,
                        external_message_id, created_at, observed_at
                 FROM messages WHERE conversation_id = ?1 ORDER BY observed_at, rowid",
            )
            .map_err(storage_err)?;
        let rows6 = stmt
            .query_map(params![conversation_id], map_message)
            .map_err(storage_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(storage_err)?;
        Ok(rows6)
    }

    fn recent_messages(&self, project_id: &str, limit: usize) -> Result<Vec<Message>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, conversation_id, project_id, role, content, content_hash,
                        external_message_id, created_at, observed_at
                 FROM messages WHERE project_id = ?1 ORDER BY observed_at DESC LIMIT ?2",
            )
            .map_err(storage_err)?;
        let mut rows = stmt
            .query_map(params![project_id, limit as i64], map_message)
            .map_err(storage_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_err)?;
        rows.reverse();
        Ok(rows)
    }

    fn get_cursor(&self, adapter_id: &str, source_id: &str) -> Result<Option<SyncCursor>> {
        let conn = self.conn()?;
        let rows7 = conn
            .query_row(
                "SELECT adapter_id, source_id, cursor, last_seen_at, last_content_hash
                 FROM sync_cursors WHERE adapter_id = ?1 AND source_id = ?2",
                params![adapter_id, source_id],
                |row| {
                    Ok(SyncCursor {
                        adapter_id: row.get(0)?,
                        source_id: row.get(1)?,
                        cursor: opt_text(row, 2),
                        last_seen_at: opt_text(row, 3).map(|v| parse_ts(&v)),
                        last_content_hash: opt_text(row, 4),
                    })
                },
            )
            .optional()
            .map_err(storage_err)?;
        Ok(rows7)
    }

    fn set_cursor(&self, cursor: &SyncCursor) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO sync_cursors (adapter_id, source_id, cursor, last_seen_at, last_content_hash)
             VALUES (?1,?2,?3,?4,?5)
             ON CONFLICT(adapter_id, source_id) DO UPDATE SET cursor = excluded.cursor,
                last_seen_at = excluded.last_seen_at,
                last_content_hash = excluded.last_content_hash",
            params![
                cursor.adapter_id,
                cursor.source_id,
                cursor.cursor,
                cursor.last_seen_at.as_ref().map(to_ts),
                cursor.last_content_hash
            ],
        )
        .map_err(storage_err)?;
        Ok(())
    }
}
