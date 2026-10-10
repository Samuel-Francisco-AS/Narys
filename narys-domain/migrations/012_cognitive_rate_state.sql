-- Local policy and conservative local-window debits only. Never remote quota,
-- credentials/fingerprints, HTTP metadata, prompts, responses or reasoning.
CREATE TABLE cognitive_rate_state (
    provider_id TEXT PRIMARY KEY NOT NULL,
    local_state TEXT NOT NULL
);
