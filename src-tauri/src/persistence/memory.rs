use rusqlite::{params, Connection, Transaction};
use serde::{Deserialize, Serialize};
use super::{database::PersistenceError, identity::valid_date};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryInput {
  pub import_key: String,
  #[serde(rename = "type")]
  pub kind: String,
  pub domains: Vec<String>,
  pub state: String,
  pub title: String,
  pub summary: String,
  pub content: Option<String>,
  pub retrieval_hint: Option<String>,
  pub source_context: Option<String>,
  pub importance: i64,
  pub confidence: String,
  pub event_date: Option<String>,
}
#[derive(Clone, Debug)]
pub struct MemoryRecord { pub id: i64, pub import_key: Option<String>, pub kind: String, pub domains: Vec<String>, pub state: String, pub title: String, pub summary: String, pub content: Option<String>, pub retrieval_hint: Option<String>, pub source_context: Option<String>, pub importance: i64, pub confidence: String, pub event_date: Option<String>, pub created_at: String, pub updated_at: String, pub supersedes_id: Option<i64> }
#[derive(Default)]
pub struct MemoryFilter<'a> { pub kind: Option<&'a str>, pub domain: Option<&'a str>, pub min_importance: Option<i64>, pub limit: u32 }
impl MemoryInput {
  pub fn validate(&self) -> Result<(), PersistenceError> {
    if self.import_key.trim().is_empty() || self.title.trim().is_empty() || self.summary.trim().is_empty()
      || self.domains.is_empty() || self.domains.iter().any(|d| d.trim().is_empty()) || !(0..=10).contains(&self.importance)
      || !["preference","decision","project","episode","reflection","relationship","identity"].contains(&self.kind.as_str())
      || !["active","review","historical","superseded"].contains(&self.state.as_str())
      || !["high","medium","low"].contains(&self.confidence.as_str())
      || self.event_date.as_deref().is_some_and(|d| !valid_date(d)) { return Err(PersistenceError::InvalidBootstrap); }
    Ok(())
  }
}
pub fn import(tx: &Transaction<'_>, memory: &MemoryInput) -> Result<bool, PersistenceError> {
  memory.validate()?;
  let domains = serde_json::to_string(&memory.domains).map_err(|_| PersistenceError::InvalidBootstrap)?;
  let changed = tx.execute("INSERT INTO memory_records (import_key,type,domains_json,state,title,summary,content,retrieval_hint,source_context,importance,confidence,event_date) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12) ON CONFLICT(import_key) DO NOTHING",
    params![memory.import_key,memory.kind,domains,memory.state,memory.title,memory.summary,memory.content,memory.retrieval_hint,memory.source_context,memory.importance,memory.confidence,memory.event_date])
    .map_err(|_| PersistenceError::Write)?;
  if changed == 0 {
    let previous = tx.query_row("SELECT type,domains_json,state,title,summary,content,retrieval_hint,source_context,importance,confidence,event_date FROM memory_records WHERE import_key=?1", [&memory.import_key], |r| {
      Ok((r.get::<_, String>(0)?,r.get::<_, String>(1)?,r.get::<_, String>(2)?,r.get::<_, String>(3)?,r.get::<_, String>(4)?,r.get::<_, Option<String>>(5)?,r.get::<_, Option<String>>(6)?,r.get::<_, Option<String>>(7)?,r.get::<_, i64>(8)?,r.get::<_, String>(9)?,r.get::<_, Option<String>>(10)?))
    }).map_err(|_| PersistenceError::Read)?;
    let (kind,old_domains,state,title,summary,content,retrieval_hint,source_context,importance,confidence,event_date) = previous;
    let old = MemoryInput { import_key:memory.import_key.clone(),kind,domains:serde_json::from_str(&old_domains).map_err(|_| PersistenceError::Read)?,state,title,summary,content,retrieval_hint,source_context,importance,confidence,event_date };
    if old != *memory { return Err(PersistenceError::Conflict); }
  }
  Ok(changed == 1)
}
pub fn active_memories(conn: &Connection, filter: MemoryFilter<'_>) -> Result<Vec<MemoryRecord>, PersistenceError> {
  let mut statement = conn.prepare("SELECT id,import_key,type,domains_json,state,title,summary,content,retrieval_hint,source_context,importance,confidence,event_date,created_at,updated_at,supersedes_id FROM memory_records WHERE state='active' AND (?1 IS NULL OR type=?1) AND (?2 IS NULL OR EXISTS (SELECT 1 FROM json_each(domains_json) WHERE value=?2)) AND importance>=?3 ORDER BY importance DESC, event_date DESC, id DESC LIMIT ?4")
    .map_err(|_| PersistenceError::Read)?;
  let rows = statement.query_map(params![filter.kind,filter.domain,filter.min_importance.unwrap_or(0),filter.limit.min(100)], |r| {
    Ok((r.get::<_, i64>(0)?,r.get::<_, Option<String>>(1)?,r.get::<_, String>(2)?,r.get::<_, String>(3)?,r.get::<_, String>(4)?,r.get::<_, String>(5)?,r.get::<_, String>(6)?,r.get::<_, Option<String>>(7)?,r.get::<_, Option<String>>(8)?,r.get::<_, Option<String>>(9)?,r.get::<_, i64>(10)?,r.get::<_, String>(11)?,r.get::<_, Option<String>>(12)?,r.get::<_, String>(13)?,r.get::<_, String>(14)?,r.get::<_, Option<i64>>(15)?))
  }).map_err(|_| PersistenceError::Read)?;
  rows.map(|row| {
    let (id,import_key,kind,domains_json,state,title,summary,content,retrieval_hint,source_context,importance,confidence,event_date,created_at,updated_at,supersedes_id) = row.map_err(|_| PersistenceError::Read)?;
    Ok(MemoryRecord { id,import_key,kind,domains: serde_json::from_str(&domains_json).map_err(|_| PersistenceError::Read)?,state,title,summary,content,retrieval_hint,source_context,importance,confidence,event_date,created_at,updated_at,supersedes_id })
  }).collect()
}
pub fn list_active_memories(conn: &Connection, limit: u32) -> Result<Vec<MemoryRecord>, PersistenceError> { active_memories(conn, MemoryFilter { limit, ..Default::default() }) }
pub fn list_memories_by_domain(conn: &Connection, domain: &str, limit: u32) -> Result<Vec<MemoryRecord>, PersistenceError> { active_memories(conn, MemoryFilter { domain: Some(domain), limit, ..Default::default() }) }
