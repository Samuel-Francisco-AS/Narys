CREATE TABLE cognitive_role_policies_v9 (
  role TEXT PRIMARY KEY CHECK(role IN ('conversation','summary','orchestrator')),
  max_output_tokens INTEGER CHECK(max_output_tokens > 0),
  max_provider_calls INTEGER NOT NULL CHECK(max_provider_calls > 0),
  retry_enabled INTEGER NOT NULL DEFAULT 1 CHECK(retry_enabled IN (0,1)),
  max_retries INTEGER NOT NULL DEFAULT 1 CHECK(max_retries >= 0),
  retry_backoff_ms INTEGER NOT NULL DEFAULT 1500 CHECK(retry_backoff_ms >= 0),
  history_max_messages INTEGER NOT NULL DEFAULT 8 CHECK(history_max_messages >= 0),
  history_max_bytes INTEGER NOT NULL DEFAULT 12288 CHECK(history_max_bytes >= 0),
  summary_input_max_bytes INTEGER NOT NULL DEFAULT 32768 CHECK(summary_input_max_bytes >= 0),
  routing_mode TEXT NOT NULL DEFAULT 'fixed' CHECK(routing_mode IN ('fixed','preferred','auto')),
  context_max_bytes INTEGER NOT NULL DEFAULT 8192 CHECK(context_max_bytes > 0),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
INSERT INTO cognitive_role_policies_v9(role,max_output_tokens,max_provider_calls,retry_enabled,max_retries,retry_backoff_ms,history_max_messages,history_max_bytes,summary_input_max_bytes,routing_mode,context_max_bytes,updated_at) SELECT role,max_output_tokens,max_provider_calls,retry_enabled,max_retries,retry_backoff_ms,history_max_messages,history_max_bytes,summary_input_max_bytes,routing_mode,context_max_bytes,updated_at FROM cognitive_role_policies;
CREATE TABLE cognitive_role_targets_v9 (
  role TEXT NOT NULL REFERENCES cognitive_role_policies_v9(role) ON DELETE CASCADE,
  position INTEGER NOT NULL CHECK(position >= 0 AND position < 8),
  provider_id TEXT NOT NULL CHECK(length(provider_id) BETWEEN 1 AND 64),
  model TEXT NOT NULL CHECK(length(model) BETWEEN 1 AND 128),
  thinking_level TEXT CHECK(thinking_level IN ('low','medium','high')),
  PRIMARY KEY(role,position),
  UNIQUE(role,provider_id)
);
INSERT INTO cognitive_role_targets_v9 SELECT role,0,provider_id,model,thinking_level FROM cognitive_role_policies;
-- Only an active v8 Preferred fallback becomes an authorized target.
INSERT INTO cognitive_role_targets_v9 SELECT role,1,fallback_provider_id,fallback_model,fallback_thinking_level
FROM cognitive_role_policies WHERE routing_mode='preferred';
DROP TABLE cognitive_role_policies;
ALTER TABLE cognitive_role_policies_v9 RENAME TO cognitive_role_policies;
ALTER TABLE cognitive_role_targets_v9 RENAME TO cognitive_role_targets;
