-- LR-7C: explicit routing policy for real Gemini -> Groq overflow.
-- Preserve current behavior by default: routing remains fixed until the user enables Preferred.
ALTER TABLE cognitive_role_policies
  ADD COLUMN routing_mode TEXT NOT NULL DEFAULT 'fixed'
  CHECK(routing_mode IN ('fixed','preferred'));

ALTER TABLE cognitive_role_policies
  ADD COLUMN fallback_provider_id TEXT;

ALTER TABLE cognitive_role_policies
  ADD COLUMN fallback_model TEXT;

ALTER TABLE cognitive_role_policies
  ADD COLUMN fallback_thinking_level TEXT
  CHECK(fallback_thinking_level IN ('low','medium','high'));

UPDATE cognitive_role_policies
SET fallback_provider_id='groq',
    fallback_model='openai/gpt-oss-20b',
    fallback_thinking_level='low'
WHERE role='conversation';
