-- Admission and recovery facts in the existing authoritative database.
CREATE TABLE IF NOT EXISTS conversation_runs (
 task_id INTEGER PRIMARY KEY CHECK(task_id BETWEEN 1 AND 9007199254740991),
 session_id INTEGER NOT NULL REFERENCES conversation_sessions(id),
 user_message_id INTEGER NOT NULL UNIQUE REFERENCES conversation_messages(id),
 state TEXT NOT NULL CHECK(state IN ('pending','running','completed','cancelled','failed','interrupted')),
 started_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 finished_at TEXT, result_json TEXT, error_code TEXT,
 policy_json TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS conversation_run_active ON conversation_runs(session_id) WHERE state IN ('pending','running');
-- details_json is added by the migration runner after checking pragma_table_info.
-- Permission describes operator-confirmed free account use, never a credential.
CREATE TABLE IF NOT EXISTS server_provider_permissions (
 provider_id TEXT PRIMARY KEY CHECK(provider_id IN ('groq','gemini','cloudflare','mistral')),
 enabled INTEGER NOT NULL DEFAULT 0 CHECK(enabled IN (0,1)),
 free_tier_confirmed INTEGER NOT NULL DEFAULT 0 CHECK(free_tier_confirmed IN (0,1)),
 updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 CHECK(enabled=0 OR free_tier_confirmed=1)
);
INSERT OR IGNORE INTO server_provider_permissions(provider_id) VALUES ('groq'),('gemini'),('cloudflare'),('mistral');
