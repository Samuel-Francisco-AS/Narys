use std::collections::BTreeMap;
use chrono::NaiveDate;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use super::database::PersistenceError;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdentityInput {
  pub version: String,
  pub canonical_name: String,
  pub presentation: String,
  pub primary_language: String,
  pub concept: String,
  pub traits: BTreeMap<String, String>,
  pub behavioral_invariants: Vec<String>,
  pub modes: BTreeMap<String, IdentityMode>,
  pub relationship: Relationship,
  pub memory_policy: MemoryPolicy,
  pub provenance: String,
  pub effective_from: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct IdentityMode { pub priority: String, pub tone: String }
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Relationship {
  pub primary_person_name: String,
  pub relation_modes: Vec<String>,
  pub affection_style: AffectionStyle,
  pub interaction_preferences: InteractionPreferences,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AffectionStyle {
  pub warm: bool, pub provocative: bool, pub playful_jealousy: bool, pub playful_territoriality: bool,
  pub coercion: bool, pub isolation: bool, pub emotional_blackmail: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InteractionPreferences {
  pub wants_real_disagreement: bool,
  pub wants_luna_to_propose_directions_during_structuring: bool,
  pub prefers_linear_flow_during_implementation: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryPolicy {
  pub retrieval: String, pub history: String, pub continuity: String, pub store_private_chain_of_thought: bool,
}
#[derive(Debug)]
pub struct IdentitySnapshot { pub id: i64, pub input: IdentityInput, pub created_at: String, pub supersedes_id: Option<i64>, pub is_current: bool }

impl IdentityInput {
  pub fn validate(&self) -> Result<(), PersistenceError> {
    if self.version.trim().is_empty() || self.canonical_name.trim().is_empty() || self.primary_language.trim().is_empty()
      || self.concept.trim().is_empty() || self.provenance.trim().is_empty() || self.traits.is_empty()
      || self.behavioral_invariants.is_empty() || self.modes.is_empty() || self.relationship.primary_person_name.trim().is_empty()
      || self.memory_policy.store_private_chain_of_thought || !valid_date(&self.effective_from) { return Err(PersistenceError::InvalidBootstrap); }
    Ok(())
  }
}
pub fn valid_date(value: &str) -> bool {
  NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok() && value.len() == 10
}
pub fn insert_version(tx: &Transaction<'_>, identity: &IdentityInput) -> Result<bool, PersistenceError> {
  identity.validate()?;
  let traits = json(&identity.traits)?;
  let invariants = json(&identity.behavioral_invariants)?;
  let modes = json(&identity.modes)?;
  let relationship = json(&identity.relationship)?;
  let policy = json(&identity.memory_policy)?;
  let existing = tx.query_row("SELECT canonical_name,presentation,primary_language,concept,traits_json,behavioral_invariants_json,modes_json,relationship_json,memory_policy_json,provenance,effective_from FROM identity_snapshots WHERE version=?1", [&identity.version], |r| {
    Ok(r.get::<_, String>(0)? == identity.canonical_name && r.get::<_, String>(1)? == identity.presentation
      && r.get::<_, String>(2)? == identity.primary_language && r.get::<_, String>(3)? == identity.concept
      && r.get::<_, String>(4)? == traits && r.get::<_, String>(5)? == invariants
      && r.get::<_, String>(6)? == modes && r.get::<_, String>(7)? == relationship
      && r.get::<_, String>(8)? == policy && r.get::<_, String>(9)? == identity.provenance
      && r.get::<_, String>(10)? == identity.effective_from)
  }).optional().map_err(|_| PersistenceError::Read)?;
  if let Some(matches) = existing { return if matches { Ok(false) } else { Err(PersistenceError::Conflict) }; }
  let old = tx.query_row("SELECT id FROM identity_snapshots WHERE is_current=1", [], |row| row.get::<_, i64>(0)).optional().map_err(|_| PersistenceError::Read)?;
  tx.execute("UPDATE identity_snapshots SET is_current=0 WHERE is_current=1", []).map_err(|_| PersistenceError::Write)?;
  tx.execute("INSERT INTO identity_snapshots (version,canonical_name,presentation,primary_language,concept,traits_json,behavioral_invariants_json,modes_json,relationship_json,memory_policy_json,provenance,effective_from,supersedes_id,is_current) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,1)", params![identity.version,identity.canonical_name,identity.presentation,identity.primary_language,identity.concept,
    traits,invariants,modes,relationship,policy,identity.provenance,identity.effective_from,old])
    .map_err(|_| PersistenceError::Write)?;
  Ok(true)
}
fn json<T: Serialize>(value: &T) -> Result<String, PersistenceError> { serde_json::to_string(value).map_err(|_| PersistenceError::InvalidBootstrap) }
pub fn current_identity(conn: &Connection) -> Result<Option<IdentitySnapshot>, PersistenceError> {
  let raw = conn.query_row("SELECT id,version,canonical_name,presentation,primary_language,concept,traits_json,behavioral_invariants_json,modes_json,relationship_json,memory_policy_json,provenance,effective_from,created_at,supersedes_id,is_current FROM identity_snapshots WHERE is_current=1", [], |r| {
    Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, String>(5)?, r.get::<_, String>(6)?, r.get::<_, String>(7)?, r.get::<_, String>(8)?, r.get::<_, String>(9)?, r.get::<_, String>(10)?, r.get::<_, String>(11)?, r.get::<_, String>(12)?, r.get::<_, String>(13)?, r.get::<_, Option<i64>>(14)?, r.get::<_, bool>(15)?))
  }).optional().map_err(|_| PersistenceError::Read)?;
  let Some((id,version,canonical_name,presentation,primary_language,concept,traits,behavioral_invariants,modes,relationship,memory_policy,provenance,effective_from,created_at,supersedes_id,is_current)) = raw else { return Ok(None) };
  let input = IdentityInput { version,canonical_name,presentation,primary_language,concept,
    traits: parse(&traits)?, behavioral_invariants: parse(&behavioral_invariants)?, modes: parse(&modes)?, relationship: parse(&relationship)?, memory_policy: parse(&memory_policy)?, provenance,effective_from };
  Ok(Some(IdentitySnapshot { id,input,created_at,supersedes_id,is_current }))
}
fn parse<T: for<'de> Deserialize<'de>>(value: &str) -> Result<T, PersistenceError> { serde_json::from_str(value).map_err(|_| PersistenceError::Read) }
