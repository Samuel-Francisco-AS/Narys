-- Core-authoritative records, never provider credentials or raw protocol/reasoning.
CREATE TABLE IF NOT EXISTS agent_sessions (
 session_ref TEXT PRIMARY KEY,
 specialist_id TEXT NOT NULL CHECK(specialist_id='copilot'),
 state TEXT NOT NULL CHECK(state IN ('creating','detached','resuming','cancelled','failed','interrupted','closed')),
 provider_session_id TEXT,
 provider_history_anchor TEXT,
 private_directory TEXT NOT NULL,
 active_task_id INTEGER,
 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE TABLE IF NOT EXISTS agent_runs (
 task_id INTEGER PRIMARY KEY CHECK(task_id BETWEEN 1 AND 9007199254740991),
 session_ref TEXT NOT NULL REFERENCES agent_sessions(session_ref),
 correlation_id TEXT NOT NULL UNIQUE,
 operation TEXT NOT NULL CHECK(operation IN ('create','resume')),
 state TEXT NOT NULL CHECK(state IN ('pending','running','completed','cancelled','failed','interrupted')),
 error_code TEXT,
 cleanup_verified INTEGER NOT NULL DEFAULT 0 CHECK(cleanup_verified IN (0,1)),
 observation_gaps INTEGER NOT NULL DEFAULT 0,
 runtime_ref TEXT,
 graph_state TEXT NOT NULL DEFAULT 'pending',
 started_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 finished_at TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS agent_session_active_run ON agent_runs(session_ref) WHERE state IN ('pending','running');
CREATE TABLE IF NOT EXISTS agent_runtime_owners (
 runtime_ref TEXT PRIMARY KEY,
 private_directory TEXT NOT NULL UNIQUE,
 boot_id TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('starting','ready','stopping','stopped','faulted')),
 cleanup_verified INTEGER NOT NULL DEFAULT 0 CHECK(cleanup_verified IN (0,1)),
 error_code TEXT,
 owner_json TEXT,
 cleanup_json TEXT,
 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
 finished_at TEXT
);
