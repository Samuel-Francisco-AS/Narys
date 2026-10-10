-- Policy is distinct from the legacy concrete preference. Preserve explicit 3D opt-in.
ALTER TABLE shell_settings ADD COLUMN presentation_policy TEXT NOT NULL DEFAULT 'economy'
    CHECK (presentation_policy IN ('economy','presence','headless','auto'));
UPDATE shell_settings SET presentation_policy = presentation_mode;
