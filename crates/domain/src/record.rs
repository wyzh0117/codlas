//! Raw capture records: Source / Conversation / Message / Evidence
//! (V1_DEVELOPMENT.md §12-§17). Messages are faithful transcripts and carry no
//! project judgement of their own.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    ChatgptWeb,
    Codex,
    Qoder,
    Hermes,
    Dsh,
    GenericWeb,
    GenericCli,
    File,
    Git,
    Manual,
    Mcp,
}

impl SourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            SourceType::ChatgptWeb => "chatgpt_web",
            SourceType::Codex => "codex",
            SourceType::Qoder => "qoder",
            SourceType::Hermes => "hermes",
            SourceType::Dsh => "dsh",
            SourceType::GenericWeb => "generic_web",
            SourceType::GenericCli => "generic_cli",
            SourceType::File => "file",
            SourceType::Git => "git",
            SourceType::Manual => "manual",
            SourceType::Mcp => "mcp",
        }
    }

    pub fn parse(value: &str) -> Option<SourceType> {
        Some(match value {
            "chatgpt_web" => SourceType::ChatgptWeb,
            "codex" => SourceType::Codex,
            "qoder" => SourceType::Qoder,
            "hermes" => SourceType::Hermes,
            "dsh" => SourceType::Dsh,
            "generic_web" => SourceType::GenericWeb,
            "generic_cli" => SourceType::GenericCli,
            "file" => SourceType::File,
            "git" => SourceType::Git,
            "manual" => SourceType::Manual,
            "mcp" => SourceType::Mcp,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: String,
    pub project_id: String,
    pub r#type: SourceType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    pub display_name: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: String,
    pub project_id: String,
    pub source_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

impl MessageRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::System => "system",
            MessageRole::Tool => "tool",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub role: MessageRole,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_message_id: Option<String>,
    pub content_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    pub observed_at: DateTime<Utc>,
}

impl Message {
    pub fn new(
        conversation_id: String,
        role: MessageRole,
        content: String,
        external_message_id: Option<String>,
        observed_at: DateTime<Utc>,
    ) -> Self {
        let content_hash = crate::ids::content_hash(&content);
        Self {
            id: crate::ids::new_id(),
            conversation_id,
            role,
            created_at: None,
            content,
            external_message_id,
            observed_at,
            content_hash,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceType {
    Message,
    File,
    Git,
    Test,
    Build,
    Manual,
    System,
}

impl EvidenceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EvidenceType::Message => "message",
            EvidenceType::File => "file",
            EvidenceType::Git => "git",
            EvidenceType::Test => "test",
            EvidenceType::Build => "build",
            EvidenceType::Manual => "manual",
            EvidenceType::System => "system",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub id: String,
    pub project_id: String,
    pub r#type: EvidenceType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub created_at: DateTime<Utc>,
}
