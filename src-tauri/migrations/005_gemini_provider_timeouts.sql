-- UIP-6B: separate version because a local v4 database was created during
-- candidate validation before global Gemini timeouts were added.
CREATE TABLE gemini_provider_settings (
  id INTEGER PRIMARY KEY CHECK(id=1),
  request_timeout_ms INTEGER NOT NULL CHECK(request_timeout_ms > 0),
  stream_idle_timeout_ms INTEGER NOT NULL CHECK(stream_idle_timeout_ms > 0),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
INSERT INTO gemini_provider_settings(id,request_timeout_ms,stream_idle_timeout_ms) VALUES (1,45000,15000);
