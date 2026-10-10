-- UIP-6B: the human gate exposed an operationally aggressive 80 ms retry.
-- Existing policies deliberately move to a 1500 ms initial backoff.
ALTER TABLE cognitive_role_policies ADD COLUMN retry_enabled INTEGER NOT NULL DEFAULT 0 CHECK(retry_enabled IN (0,1));
ALTER TABLE cognitive_role_policies ADD COLUMN max_retries INTEGER NOT NULL DEFAULT 0 CHECK(max_retries >= 0);
ALTER TABLE cognitive_role_policies ADD COLUMN retry_backoff_ms INTEGER NOT NULL DEFAULT 1500 CHECK(retry_backoff_ms >= 0);
ALTER TABLE cognitive_role_policies ADD COLUMN history_max_messages INTEGER NOT NULL DEFAULT 8 CHECK(history_max_messages >= 0);
ALTER TABLE cognitive_role_policies ADD COLUMN history_max_bytes INTEGER NOT NULL DEFAULT 12288 CHECK(history_max_bytes >= 0);
ALTER TABLE cognitive_role_policies ADD COLUMN summary_input_max_bytes INTEGER NOT NULL DEFAULT 32768 CHECK(summary_input_max_bytes >= 0);
UPDATE cognitive_role_policies SET retry_enabled=1,max_retries=1 WHERE role='conversation';
CREATE TABLE general_settings (
  id INTEGER PRIMARY KEY CHECK(id=1),
  always_on_top INTEGER NOT NULL CHECK(always_on_top IN (0,1)),
  active_fps INTEGER NOT NULL CHECK(active_fps BETWEEN 1 AND 60),
  background_fps INTEGER NOT NULL CHECK(background_fps BETWEEN 1 AND 60),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
INSERT INTO general_settings(id,always_on_top,active_fps,background_fps) VALUES (1,0,30,24);
