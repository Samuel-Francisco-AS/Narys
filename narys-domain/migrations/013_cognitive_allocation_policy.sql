-- User policy only, never economic observations or commercial metadata.
CREATE TABLE cognitive_role_allocation_policies (
  role TEXT PRIMARY KEY NOT NULL
    CHECK(role IN ('conversation','summary','orchestrator','worker'))
    REFERENCES cognitive_role_policies(role) ON DELETE CASCADE,
  allocation_profile TEXT NOT NULL CHECK(allocation_profile IN ('economy','balanced','fast')),
  variant_selection_mode TEXT NOT NULL CHECK(variant_selection_mode IN ('explicit','auto')),
  minimum_cognitive_tier INTEGER CHECK(minimum_cognitive_tier IS NULL OR
    (typeof(minimum_cognitive_tier)='integer' AND minimum_cognitive_tier BETWEEN 0 AND 255)),
  paid_use_policy TEXT NOT NULL CHECK(paid_use_policy IN ('deny','allow_known_cost_within_budget')),
  max_paid_currency TEXT,
  max_paid_micros INTEGER,
  reduced_below_percent INTEGER,
  reserve_below_percent INTEGER,
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  CHECK (
    (paid_use_policy='deny' AND max_paid_currency IS NULL AND max_paid_micros IS NULL) OR
    (paid_use_policy='allow_known_cost_within_budget' AND
      max_paid_currency IS NOT NULL AND typeof(max_paid_currency)='text' AND
      length(CAST(max_paid_currency AS BLOB))=3 AND max_paid_currency GLOB '[A-Z][A-Z][A-Z]' AND
      max_paid_micros IS NOT NULL AND typeof(max_paid_micros)='integer' AND
      max_paid_micros BETWEEN 0 AND 9007199254740991)
  ),
  CHECK (
    (reduced_below_percent IS NULL AND reserve_below_percent IS NULL) OR
    (reduced_below_percent IS NOT NULL AND reserve_below_percent IS NOT NULL AND
      typeof(reduced_below_percent)='integer' AND typeof(reserve_below_percent)='integer' AND
      reserve_below_percent BETWEEN 0 AND reduced_below_percent AND reduced_below_percent <= 100)
  )
);
-- Exactly the B3 production policy for every existing role; no behavior change.
INSERT INTO cognitive_role_allocation_policies
  (role,allocation_profile,variant_selection_mode,paid_use_policy)
SELECT role,'balanced','auto','deny' FROM cognitive_role_policies;
