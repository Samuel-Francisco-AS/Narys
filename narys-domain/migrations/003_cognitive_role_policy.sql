CREATE TABLE cognitive_role_policies (
  role TEXT PRIMARY KEY CHECK(role IN ('conversation','summary')),
  provider_id TEXT NOT NULL,
  model TEXT NOT NULL,
  thinking_level TEXT CHECK(thinking_level IN ('low','medium','high')),
  max_output_tokens INTEGER CHECK(max_output_tokens > 0),
  max_provider_calls INTEGER NOT NULL CHECK(max_provider_calls > 0),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
INSERT INTO cognitive_role_policies(role,provider_id,model,thinking_level,max_output_tokens,max_provider_calls)
VALUES ('conversation','gemini','gemini-3.8-flash','low',4096,2),
       ('summary','gemini','gemini-3.8-flash','low',1024,1);
