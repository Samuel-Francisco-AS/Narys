-- Task history is terminal-only; this ledger can precede the root history row.
CREATE TABLE checkpoint_task_policies (
  root_task_id INTEGER NOT NULL CHECK(typeof(root_task_id)='integer' AND root_task_id BETWEEN 1 AND 9007199254740991),
  role TEXT NOT NULL CHECK(role IN ('conversation','summary','orchestrator','worker')),
  snapshot_json TEXT NOT NULL CHECK(typeof(snapshot_json)='text' AND length(CAST(snapshot_json AS BLOB)) BETWEEN 1 AND 16384),
  PRIMARY KEY(root_task_id,role)
);
CREATE TABLE cognitive_checkpoints (
  root_task_id INTEGER NOT NULL CHECK(typeof(root_task_id)='integer' AND root_task_id BETWEEN 1 AND 9007199254740991),
  unit_sequence INTEGER NOT NULL CHECK(typeof(unit_sequence)='integer' AND unit_sequence BETWEEN 1 AND 9007199254740991),
  checkpoint_sequence INTEGER NOT NULL CHECK(typeof(checkpoint_sequence)='integer' AND checkpoint_sequence BETWEEN 1 AND 9007199254740991),
  role TEXT NOT NULL CHECK(role IN ('conversation','summary','orchestrator','worker')),
  source_kind TEXT NOT NULL CHECK(source_kind IN ('root_task','task_graph_subtask')),
  source_key TEXT NOT NULL CHECK(typeof(source_key)='text' AND ((source_kind='root_task' AND source_key='') OR (source_kind='task_graph_subtask' AND length(CAST(source_key AS BLOB)) BETWEEN 1 AND 64))),
  effect_state TEXT NOT NULL CHECK(effect_state IN ('not_started','committed','unknown_or_in_flight')),
  commit_state TEXT NOT NULL DEFAULT 'committed' CHECK(commit_state='committed'),
  checkpoint_json TEXT NOT NULL CHECK(typeof(checkpoint_json)='text' AND length(CAST(checkpoint_json AS BLOB)) BETWEEN 1 AND 8192),
  committed_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY(root_task_id,unit_sequence),
  UNIQUE(root_task_id,source_kind,source_key),
  FOREIGN KEY(root_task_id,role) REFERENCES checkpoint_task_policies(root_task_id,role)
);
