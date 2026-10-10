-- Additive. Legacy LR-10A IDs remain in their original namespace/table.
CREATE TABLE IF NOT EXISTS headless_tasks (
 id INTEGER PRIMARY KEY, directory TEXT NOT NULL, objective TEXT NOT NULL,
 expected TEXT NOT NULL, state TEXT NOT NULL, result TEXT, error_code TEXT
);
CREATE TABLE IF NOT EXISTS server_migrations (
 name TEXT PRIMARY KEY, source TEXT NOT NULL, backup_directory TEXT NOT NULL,
 completed_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE TABLE IF NOT EXISTS server_events (
 sequence INTEGER PRIMARY KEY AUTOINCREMENT,
 namespace TEXT NOT NULL CHECK(namespace IN ('lr10a','product','runtime')),
 task_id INTEGER, code TEXT NOT NULL,
 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX IF NOT EXISTS server_events_task ON server_events(namespace,task_id,sequence);
