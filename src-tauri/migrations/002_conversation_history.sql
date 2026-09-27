ALTER TABLE conversation_sessions ADD COLUMN kind TEXT NOT NULL DEFAULT 'legacy' CHECK(kind IN ('product','legacy'));
ALTER TABLE conversation_sessions ADD COLUMN summary_status TEXT NOT NULL DEFAULT 'none' CHECK(summary_status IN ('none','pending','running','completed','failed'));
ALTER TABLE conversation_sessions ADD COLUMN summary TEXT;
ALTER TABLE conversation_sessions ADD COLUMN summary_updated_at TEXT;

-- UIP-4 product sessions have no title. The known LR-4 and LR-6 diagnostic
-- sessions have explicit titles and remain legacy regardless of status.
UPDATE conversation_sessions SET kind='product'
WHERE title IS NULL AND status IN ('active','closed');

CREATE INDEX conversation_history_order ON conversation_sessions(kind, updated_at DESC, id DESC);
