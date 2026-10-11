-- FIX-1: one authoritative DB. No grant, operator secret or raw tool args.
CREATE TABLE IF NOT EXISTS agent_local_tasks (
 task_id INTEGER PRIMARY KEY CHECK(task_id>0), session_id TEXT NOT NULL UNIQUE,
 workspace TEXT NOT NULL, epoch TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('active','cancel_requested','cancelled','completed','failed','interrupted','cleanup_uncertain')),
 peer_cleanup_verified INTEGER NOT NULL DEFAULT 0 CHECK(peer_cleanup_verified IN (0,1))
);
CREATE TABLE IF NOT EXISTS agent_tool_executions (
 approval_id TEXT PRIMARY KEY REFERENCES agent_approvals(approval_id),
 task_id INTEGER NOT NULL REFERENCES agent_local_tasks(task_id),
 phase TEXT NOT NULL CHECK(phase IN ('claimed','started','completed','failed','cancelled','uncertain')),
 effect_started INTEGER NOT NULL DEFAULT 0 CHECK(effect_started IN (0,1)),
 cancel_requested INTEGER NOT NULL DEFAULT 0 CHECK(cancel_requested IN (0,1)),
 cleanup_verified INTEGER NOT NULL DEFAULT 0 CHECK(cleanup_verified IN (0,1)),
 result_json TEXT, updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX IF NOT EXISTS agent_tool_execution_task ON agent_tool_executions(task_id);
CREATE TABLE IF NOT EXISTS agent_execution_events (
 sequence INTEGER PRIMARY KEY AUTOINCREMENT,
 approval_id TEXT NOT NULL REFERENCES agent_tool_executions(approval_id),
 phase TEXT NOT NULL,
 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
