//! Versioned schema. `schema_migrations` makes the database self-describing so a
//! later release can migrate user data instead of discarding it
//! (V1_DEVELOPMENT.md §49, V1-H Hardening).

pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub sql: &'static str,
}

pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "baseline",
        sql: r#"
CREATE TABLE projects (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  description TEXT,
  root_path TEXT,
  status TEXT NOT NULL DEFAULT 'active',
  current_phase TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE project_charters (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  original_problem TEXT NOT NULL,
  target_user TEXT,
  core_value TEXT NOT NULL,
  must_have_json TEXT NOT NULL DEFAULT '[]',
  wont_have_json TEXT NOT NULL DEFAULT '[]',
  success_criteria_json TEXT NOT NULL DEFAULT '[]',
  architecture_principles_json TEXT NOT NULL DEFAULT '[]',
  constraints_json TEXT NOT NULL DEFAULT '[]',
  version INTEGER NOT NULL,
  locked INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (project_id, version)
);
CREATE INDEX idx_charters_project ON project_charters (project_id, version DESC);

CREATE TABLE scope_boundaries (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  in_scope_json TEXT NOT NULL DEFAULT '[]',
  out_of_scope_json TEXT NOT NULL DEFAULT '[]',
  deferred_json TEXT NOT NULL DEFAULT '[]',
  forbidden_json TEXT NOT NULL DEFAULT '[]',
  version INTEGER NOT NULL,
  UNIQUE (project_id, version)
);

CREATE TABLE architecture_baselines (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  summary TEXT NOT NULL,
  layers_json TEXT NOT NULL DEFAULT '[]',
  invariants_json TEXT NOT NULL DEFAULT '[]',
  replaceable_json TEXT NOT NULL DEFAULT '[]',
  version INTEGER NOT NULL,
  active INTEGER NOT NULL DEFAULT 1,
  UNIQUE (project_id, version)
);

CREATE TABLE sources (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  type TEXT NOT NULL,
  external_id TEXT,
  display_name TEXT NOT NULL,
  metadata_json TEXT NOT NULL DEFAULT '{}'
);
CREATE UNIQUE INDEX idx_sources_external ON sources (project_id, type, external_id)
  WHERE external_id IS NOT NULL;

CREATE TABLE conversations (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  source_id TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
  external_conversation_id TEXT,
  title TEXT,
  started_at TEXT,
  last_seen_at TEXT,
  UNIQUE (source_id, external_conversation_id)
);

CREATE TABLE messages (
  id TEXT PRIMARY KEY,
  conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  project_id TEXT NOT NULL,
  role TEXT NOT NULL,
  content TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  external_message_id TEXT,
  created_at TEXT,
  observed_at TEXT NOT NULL,
  UNIQUE (conversation_id, content_hash)
);
CREATE UNIQUE INDEX idx_messages_external ON messages (conversation_id, external_message_id)
  WHERE external_message_id IS NOT NULL;
CREATE INDEX idx_messages_project ON messages (project_id, observed_at DESC);

CREATE TABLE events (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  id TEXT NOT NULL UNIQUE,
  dedup_key TEXT NOT NULL UNIQUE,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  type TEXT NOT NULL,
  schema_version INTEGER NOT NULL DEFAULT 1,
  actor_type TEXT NOT NULL,
  actor_id TEXT,
  source_id TEXT,
  conversation_id TEXT,
  message_id TEXT,
  origin_node_id TEXT,
  causation_id TEXT,
  correlation_id TEXT,
  subject_type TEXT,
  subject_id TEXT,
  payload_json TEXT NOT NULL DEFAULT '{}',
  confidence REAL NOT NULL DEFAULT 1,
  evidence_refs_json TEXT NOT NULL DEFAULT '[]',
  status TEXT NOT NULL,
  occurred_at TEXT,
  observed_at TEXT NOT NULL
);
CREATE INDEX idx_events_project_seq ON events (project_id, seq);
CREATE INDEX idx_events_status ON events (project_id, status);
CREATE INDEX idx_events_type ON events (project_id, type);

CREATE TABLE evidence (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  type TEXT NOT NULL,
  ref_id TEXT,
  summary TEXT,
  created_at TEXT NOT NULL
);
CREATE INDEX idx_evidence_project ON evidence (project_id, created_at DESC);

CREATE TABLE evidence_links (
  evidence_id TEXT NOT NULL REFERENCES evidence(id) ON DELETE CASCADE,
  subject_kind TEXT NOT NULL,
  subject_id TEXT NOT NULL,
  PRIMARY KEY (evidence_id, subject_kind, subject_id)
);

CREATE TABLE entities (
  kind TEXT NOT NULL,
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  status TEXT,
  data_json TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_entities_project_kind ON entities (project_id, kind, status);

CREATE TABLE entity_links (
  from_kind TEXT NOT NULL,
  from_id TEXT NOT NULL,
  to_kind TEXT NOT NULL,
  to_id TEXT NOT NULL,
  relation TEXT NOT NULL,
  PRIMARY KEY (from_id, to_id, relation)
);
CREATE INDEX idx_entity_links_from ON entity_links (from_kind, from_id);

CREATE TABLE entity_versions (
  kind TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  version INTEGER NOT NULL,
  before_json TEXT,
  after_json TEXT NOT NULL,
  reason TEXT,
  created_at TEXT NOT NULL,
  PRIMARY KEY (kind, entity_id, version)
);

CREATE TABLE artifacts (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,
  path TEXT,
  hash TEXT,
  version INTEGER NOT NULL DEFAULT 1,
  relationships_json TEXT NOT NULL DEFAULT '[]',
  deleted INTEGER NOT NULL DEFAULT 0,
  redesign_count INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_artifacts_path ON artifacts (project_id, path) WHERE path IS NOT NULL;

CREATE TABLE snapshots (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  label TEXT NOT NULL,
  state_json TEXT NOT NULL,
  project_map_markdown TEXT,
  taken_at TEXT NOT NULL
);
CREATE INDEX idx_snapshots_project ON snapshots (project_id, taken_at DESC);

CREATE TABLE project_map_snapshots (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  snapshot_id TEXT,
  markdown TEXT NOT NULL,
  generated_at TEXT NOT NULL,
  source_state_hash TEXT NOT NULL
);
CREATE INDEX idx_maps_project ON project_map_snapshots (project_id, generated_at DESC);

CREATE TABLE sync_cursors (
  adapter_id TEXT NOT NULL,
  source_id TEXT NOT NULL,
  cursor TEXT,
  last_seen_at TEXT,
  last_content_hash TEXT,
  PRIMARY KEY (adapter_id, source_id)
);

CREATE TABLE adapter_states (
  adapter_id TEXT PRIMARY KEY,
  health TEXT NOT NULL,
  detail TEXT,
  updated_at TEXT NOT NULL
);

CREATE TABLE settings (
  key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL
);

CREATE TABLE user_overrides (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  subject_type TEXT NOT NULL,
  subject_id TEXT NOT NULL,
  field TEXT NOT NULL,
  value_json TEXT NOT NULL,
  reason TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE audit_logs (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL,
  component TEXT NOT NULL,
  action TEXT NOT NULL,
  read_refs_json TEXT NOT NULL DEFAULT '[]',
  produced_ref TEXT,
  target_agent TEXT,
  redacted_json TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL
);
CREATE INDEX idx_audit_project ON audit_logs (project_id, created_at DESC);
"#,
    },
    Migration {
        version: 2,
        name: "fts5_index",
        sql: r#"
CREATE VIRTUAL TABLE search_index USING fts5(
  kind UNINDEXED,
  record_id UNINDEXED,
  project_id UNINDEXED,
  title,
  body,
  tokenize = 'unicode61 remove_diacritics 2'
);
"#,
    },
    // Every read of `entities` addresses a row by (kind, id), but the baseline made
    // `id` the sole primary key and upsert conflicted on it without rewriting `kind`.
    // Two entities of different kinds sharing an id therefore left one row carrying
    // one kind and the other kind's body — a confirmed decision whose subject id was
    // also an issue's id silently turned that issue into decision JSON. The key is
    // now kind-scoped, so a collision can neither happen nor hide.
    Migration {
        version: 3,
        name: "entities_keyed_by_kind_and_id",
        sql: r#"
CREATE TABLE entities_v3 (
  kind TEXT NOT NULL,
  id TEXT NOT NULL,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  status TEXT,
  data_json TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (kind, id)
);

INSERT INTO entities_v3 (kind, id, project_id, status, data_json, updated_at)
  SELECT kind, id, project_id, status, data_json, updated_at FROM entities;

DROP TABLE entities;
ALTER TABLE entities_v3 RENAME TO entities;
CREATE INDEX idx_entities_project_kind ON entities (project_id, kind, status);
"#,
    },
];
