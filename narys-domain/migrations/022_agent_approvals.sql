-- Audit receipts are not capabilities. No raw command/content or secret is stored.
CREATE TABLE IF NOT EXISTS agent_approvals (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    approval_id TEXT NOT NULL UNIQUE,
    epoch TEXT NOT NULL,
    task_id INTEGER NOT NULL CHECK(task_id > 0),
    session_id TEXT NOT NULL,
    specialist_id TEXT NOT NULL,
    profile TEXT NOT NULL CHECK(profile IN ('assisted','isolated','explicit_yolo')),
    policy_version INTEGER NOT NULL CHECK(policy_version > 0),
    binding_digest TEXT NOT NULL,
    summary_json TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('pending','approved','denied','expired','cancelled','consumed','interrupted')),
    reason TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX IF NOT EXISTS agent_approvals_task ON agent_approvals(task_id, state);
CREATE TABLE IF NOT EXISTS agent_approval_events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    approval_id TEXT NOT NULL REFERENCES agent_approvals(approval_id),
    state TEXT NOT NULL,
    reason TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
