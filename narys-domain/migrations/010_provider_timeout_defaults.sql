INSERT OR IGNORE INTO provider_timeout_settings(provider_id,request_timeout_ms,stream_idle_timeout_ms)
VALUES ('mistral',45000,15000);
INSERT OR IGNORE INTO provider_timeout_settings(provider_id,request_timeout_ms,stream_idle_timeout_ms)
VALUES ('cloudflare',45000,15000);
