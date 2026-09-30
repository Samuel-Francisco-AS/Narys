CREATE TABLE cognitive_role_policies_v8 (
  role TEXT PRIMARY KEY CHECK(role IN ('conversation','summary','orchestrator')),
  provider_id TEXT NOT NULL,
  model TEXT NOT NULL,
  thinking_level TEXT CHECK(thinking_level IN ('low','medium','high')),
  max_output_tokens INTEGER CHECK(max_output_tokens > 0),
  max_provider_calls INTEGER NOT NULL CHECK(max_provider_calls > 0),
  retry_enabled INTEGER NOT NULL DEFAULT 1 CHECK(retry_enabled IN (0,1)),
  max_retries INTEGER NOT NULL DEFAULT 1 CHECK(max_retries >= 0),
  retry_backoff_ms INTEGER NOT NULL DEFAULT 1500 CHECK(retry_backoff_ms >= 0),
  history_max_messages INTEGER NOT NULL DEFAULT 8 CHECK(history_max_messages >= 0),
  history_max_bytes INTEGER NOT NULL DEFAULT 12288 CHECK(history_max_bytes >= 0),
  summary_input_max_bytes INTEGER NOT NULL DEFAULT 32768 CHECK(summary_input_max_bytes >= 0),
  routing_mode TEXT NOT NULL DEFAULT 'fixed' CHECK(routing_mode IN ('fixed','preferred')),
  fallback_provider_id TEXT,
  fallback_model TEXT,
  fallback_thinking_level TEXT CHECK(fallback_thinking_level IN ('low','medium','high')),
  context_max_bytes INTEGER NOT NULL DEFAULT 8192 CHECK(context_max_bytes > 0),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
INSERT INTO cognitive_role_policies_v8(
  role,provider_id,model,thinking_level,max_output_tokens,max_provider_calls,
  retry_enabled,max_retries,retry_backoff_ms,history_max_messages,history_max_bytes,
  summary_input_max_bytes,routing_mode,fallback_provider_id,fallback_model,
  fallback_thinking_level,context_max_bytes,updated_at
)
SELECT role,provider_id,model,thinking_level,max_output_tokens,max_provider_calls,
  retry_enabled,max_retries,retry_backoff_ms,history_max_messages,history_max_bytes,
  summary_input_max_bytes,routing_mode,fallback_provider_id,fallback_model,
  fallback_thinking_level,8192,updated_at
FROM cognitive_role_policies;
DROP TABLE cognitive_role_policies;
ALTER TABLE cognitive_role_policies_v8 RENAME TO cognitive_role_policies;
UPDATE cognitive_role_policies SET context_max_bytes=32768 WHERE role IN ('conversation','summary');

INSERT INTO cognitive_role_policies(
  role, provider_id, model, thinking_level, max_output_tokens, max_provider_calls,
  retry_enabled, max_retries, retry_backoff_ms, history_max_messages,
  history_max_bytes, summary_input_max_bytes, routing_mode, context_max_bytes
) VALUES (
  'orchestrator', 'gemini', 'gemini-3.8-flash', 'low', 4096, 2,
  true, 1, 1500, 0, 0, 0, 'fixed', 8192
);
