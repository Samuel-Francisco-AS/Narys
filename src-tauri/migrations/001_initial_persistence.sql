CREATE TABLE identity_snapshots (
  id INTEGER PRIMARY KEY,
  version TEXT NOT NULL UNIQUE,
  canonical_name TEXT NOT NULL,
  presentation TEXT NOT NULL,
  primary_language TEXT NOT NULL,
  concept TEXT NOT NULL,
  traits_json TEXT NOT NULL,
  behavioral_invariants_json TEXT NOT NULL,
  modes_json TEXT NOT NULL,
  relationship_json TEXT NOT NULL,
  memory_policy_json TEXT NOT NULL,
  provenance TEXT NOT NULL,
  effective_from TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  supersedes_id INTEGER REFERENCES identity_snapshots(id),
  is_current INTEGER NOT NULL CHECK(is_current IN (0,1))
);
CREATE UNIQUE INDEX one_current_identity ON identity_snapshots(is_current) WHERE is_current = 1;
CREATE TABLE memory_records (
  id INTEGER PRIMARY KEY,
  import_key TEXT UNIQUE,
  type TEXT NOT NULL CHECK(type IN ('preference','decision','project','episode','reflection','relationship','identity')),
  domains_json TEXT NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('active','review','historical','superseded')),
  title TEXT NOT NULL,
  summary TEXT NOT NULL,
  content TEXT,
  retrieval_hint TEXT,
  source_context TEXT,
  importance INTEGER NOT NULL CHECK(importance BETWEEN 0 AND 10),
  confidence TEXT NOT NULL CHECK(confidence IN ('high','medium','low')),
  event_date TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  supersedes_id INTEGER REFERENCES memory_records(id)
);
CREATE INDEX memory_active_rank ON memory_records(state, importance DESC, event_date DESC);
CREATE TABLE conversation_sessions (
  id INTEGER PRIMARY KEY,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  title TEXT,
  status TEXT
);
CREATE TABLE conversation_messages (
  id INTEGER PRIMARY KEY,
  session_id INTEGER NOT NULL REFERENCES conversation_sessions(id) ON DELETE CASCADE,
  role TEXT NOT NULL CHECK(role IN ('user','assistant','system')),
  content TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX conversation_messages_session ON conversation_messages(session_id, id);
CREATE TABLE task_records (
  task_id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('completed','cancelled','failed')),
  started_at TEXT NOT NULL,
  finished_at TEXT NOT NULL,
  summary TEXT,
  error_code TEXT
);
