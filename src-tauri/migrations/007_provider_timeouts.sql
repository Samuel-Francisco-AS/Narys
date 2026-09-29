-- Keep the legacy Gemini row for non-destructive upgrade and copy its current values.
CREATE TABLE provider_timeout_settings (
  provider_id TEXT PRIMARY KEY,
  request_timeout_ms INTEGER NOT NULL CHECK(request_timeout_ms > 0),
  stream_idle_timeout_ms INTEGER NOT NULL CHECK(stream_idle_timeout_ms > 0),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
INSERT INTO provider_timeout_settings(provider_id,request_timeout_ms,stream_idle_timeout_ms)
SELECT 'gemini',request_timeout_ms,stream_idle_timeout_ms FROM gemini_provider_settings WHERE id=1;
INSERT INTO provider_timeout_settings(provider_id,request_timeout_ms,stream_idle_timeout_ms)
VALUES ('groq',45000,15000);
