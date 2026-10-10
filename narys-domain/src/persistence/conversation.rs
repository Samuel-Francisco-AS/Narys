use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use super::database::PersistenceError;
#[cfg(test)]
pub const OUTBOUND_HISTORY_MESSAGES: usize = 8;
#[cfg(test)]
pub const OUTBOUND_HISTORY_BYTES: usize = 12 * 1024;
const SUMMARY_CANDIDATE_MESSAGES: i64 = 256;
const TITLE: &str = "Diagnóstico LR-4";
const GEMINI_TITLE: &str = "Luna · Gemini LR-6";
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSession { pub id: i64, pub created_at: String, pub updated_at: String, pub title: Option<String>, pub status: Option<String>, pub summary_status: String, pub summary: Option<String>, pub summary_updated_at: Option<String>, pub messages: Vec<ConversationMessage> }
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationHistoryItem {
  pub id: i64, pub created_at: String, pub updated_at: String, pub title: String,
  pub status: String, pub summary_status: String, pub message_count: i64, pub preview: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMessage { pub id: i64, pub session_id: i64, pub role: String, pub content: String, pub created_at: String }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionRole { User, Assistant }
#[derive(Clone, Debug)]
pub struct SessionTurn { pub role: SessionRole, pub content: String }
#[derive(Debug)]
pub struct ClaimedSummary { pub id: i64, pub messages: Vec<ConversationMessage>, pub truncated: bool }
fn automatic_summary_enabled(conn: &Connection) -> Result<bool, PersistenceError> {
  let bytes: i64 = conn
    .query_row("SELECT summary_input_max_bytes FROM cognitive_role_policies WHERE role='summary'", [], |row| row.get(0))
    .map_err(|_| PersistenceError::Read)?;
  Ok(bytes > 0)
}

/// Startup recovery runs after orphan closure and before the summary worker starts.
pub fn reset_interrupted_summaries(conn: &Connection) -> Result<usize, PersistenceError> {
  let state = if automatic_summary_enabled(conn)? { "pending" } else { "none" };
  conn.execute("UPDATE conversation_sessions SET summary_status=?1 WHERE kind='product' AND status='closed' AND summary_status='running'", [state])
    .map_err(|_| PersistenceError::Write)
}

/// Disabling automatic summaries must leave no queued/running session looking active.
pub fn disable_pending_summaries(conn: &Connection) -> Result<usize, PersistenceError> {
  conn.execute("UPDATE conversation_sessions SET summary_status='none',summary=NULL,summary_updated_at=NULL
    WHERE kind='product' AND status='closed' AND summary_status IN ('pending','running')", [])
    .map_err(|_| PersistenceError::Write)
}

pub fn clear_claimed_summary(conn: &Connection, id: i64) -> Result<bool, PersistenceError> {
  Ok(conn.execute("UPDATE conversation_sessions SET summary_status='none',summary=NULL,summary_updated_at=NULL
    WHERE id=?1 AND kind='product' AND status='closed' AND summary_status='running'", [id])
    .map_err(|_| PersistenceError::Write)? == 1)
}
pub fn claim_next_pending_summary(conn: &mut Connection) -> Result<Option<ClaimedSummary>, PersistenceError> {
  let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|_| PersistenceError::Write)?;
  let id: Option<i64> = tx.query_row("SELECT s.id FROM conversation_sessions s WHERE s.kind='product' AND s.status='closed'
    AND s.summary_status='pending' AND EXISTS(SELECT 1 FROM conversation_messages m WHERE m.session_id=s.id)
    ORDER BY s.updated_at,s.id LIMIT 1", [], |r| r.get(0)).optional().map_err(|_| PersistenceError::Read)?;
  let Some(id) = id else { return Ok(None) };
  let changed = tx.execute("UPDATE conversation_sessions SET summary_status='running' WHERE id=?1 AND kind='product' AND status='closed' AND summary_status='pending'", [id])
    .map_err(|_| PersistenceError::Write)?;
  if changed != 1 { return Ok(None); }
  tx.commit().map_err(|_| PersistenceError::Write)?;
  // Read potentially long transcripts after releasing the SQLite write lock.
  let mut stmt = conn.prepare("SELECT id,session_id,role,substr(content,1,8193),created_at FROM conversation_messages
    WHERE session_id=?1 AND (id=(SELECT id FROM conversation_messages WHERE session_id=?1 AND role='user' ORDER BY id LIMIT 1)
      OR id IN (SELECT id FROM conversation_messages WHERE session_id=?1 ORDER BY id DESC LIMIT ?2)) ORDER BY id")
    .map_err(|_| PersistenceError::Read)?;
  let messages = stmt.query_map(rusqlite::params![id,SUMMARY_CANDIDATE_MESSAGES], |r| Ok(ConversationMessage { id:r.get(0)?,session_id:r.get(1)?,role:r.get(2)?,content:r.get(3)?,created_at:r.get(4)? }))
    .map_err(|_| PersistenceError::Read)?.collect::<Result<Vec<_>,_>>().map_err(|_| PersistenceError::Read)?;
  drop(stmt);
  let total: i64 = conn.query_row("SELECT COUNT(*) FROM conversation_messages WHERE session_id=?1",[id],|r|r.get(0))
    .map_err(|_| PersistenceError::Read)?;
  Ok(Some(ClaimedSummary { id, truncated: total > messages.len() as i64, messages }))
}
pub fn complete_summary(conn: &Connection, id: i64, title: &str, summary: &str) -> Result<bool, PersistenceError> {
  Ok(conn.execute("UPDATE conversation_sessions SET title=?2,summary=?3,summary_status='completed',
    summary_updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1 AND kind='product' AND status='closed' AND summary_status='running'",
    rusqlite::params![id,title,summary]).map_err(|_| PersistenceError::Write)? == 1)
}
pub fn fail_summary(conn: &Connection, id: i64, transient: bool) -> Result<bool, PersistenceError> {
  // summary_updated_at records completion only; failures keep it NULL.
  let state = if transient { "pending" } else { "failed" };
  Ok(conn.execute("UPDATE conversation_sessions SET summary_status=?2 WHERE id=?1 AND kind='product' AND status='closed' AND summary_status='running'",
    rusqlite::params![id,state]).map_err(|_| PersistenceError::Write)? == 1)
}
pub fn create_session(conn: &Connection) -> Result<i64, PersistenceError> {
  conn.execute("INSERT INTO conversation_sessions(status,kind) VALUES ('active','product')", []).map_err(|_| PersistenceError::Write)?;
  Ok(conn.last_insert_rowid())
}
pub fn is_active_session(conn: &Connection, id: i64) -> Result<bool, PersistenceError> {
  if id <= 0 { return Ok(false); }
  conn.query_row("SELECT EXISTS(SELECT 1 FROM conversation_sessions WHERE id=?1 AND kind='product' AND status='active')", [id], |r| r.get(0)).map_err(|_| PersistenceError::Read)
}
pub fn close_session(conn: &Connection, id: i64) -> Result<bool, PersistenceError> {
  if id <= 0 { return Ok(false); }
  let summary_enabled = automatic_summary_enabled(conn)?;
  Ok(conn.execute("UPDATE conversation_sessions SET status='closed',updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'),
      summary_status=CASE WHEN ?2=1 AND summary_status='none' AND
        EXISTS(SELECT 1 FROM conversation_messages WHERE session_id=?1 AND role='user') AND
        EXISTS(SELECT 1 FROM conversation_messages WHERE session_id=?1 AND role='assistant')
        THEN 'pending' ELSE summary_status END
      WHERE id=?1 AND kind='product' AND status='active'", rusqlite::params![id, if summary_enabled { 1 } else { 0 }])
    .map_err(|_| PersistenceError::Write)? == 1)
}
/// The caller holds CurrentRunSessions across this transaction and registry update.
pub fn resume_session(conn: &mut Connection, target_id: i64, current_id: Option<i64>) -> Result<ConversationSession, &'static str> {
  if target_id <= 0 || current_id.is_some_and(|id| id <= 0 || id == target_id) { return Err("session_invalid"); }
  let tx = conn.transaction().map_err(|_| "write_failed")?;
  let target = tx.query_row("SELECT kind,status,summary_status,EXISTS(SELECT 1 FROM conversation_messages WHERE session_id=?1) FROM conversation_sessions WHERE id=?1",
    [target_id], |r| Ok((r.get::<_, String>(0)?,r.get::<_, String>(1)?,r.get::<_, String>(2)?,r.get::<_, bool>(3)?)))
    .optional().map_err(|_| "read_failed")?.ok_or("session_invalid")?;
  if target.0 != "product" || target.1 != "closed" || !target.3 { return Err("session_invalid"); }
  if target.2 == "running" { return Err("summary_busy"); }
  if let Some(id) = current_id {
    if !is_active_session(&tx, id).map_err(|_| "read_failed")? { return Err("session_invalid"); }
    if !close_session(&tx, id).map_err(|_| "write_failed")? { return Err("session_invalid"); }
  }
  let changed = tx.execute("UPDATE conversation_sessions SET status='active',updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'),
    summary_status='none',summary=NULL,summary_updated_at=NULL WHERE id=?1 AND kind='product' AND status='closed' AND summary_status!='running'",
    [target_id]).map_err(|_| "write_failed")?;
  if changed != 1 { return Err("session_invalid"); }
  let resumed = session(&tx, target_id).map_err(|_| "read_failed")?.ok_or("session_invalid")?;
  tx.commit().map_err(|_| "write_failed")?;
  Ok(resumed)
}
/// Called once in Tauri setup, while CurrentRunSessions is still empty.
pub fn close_orphaned_product_sessions(conn: &Connection) -> Result<usize, PersistenceError> {
  let summary_enabled = automatic_summary_enabled(conn)?;
  conn.execute("UPDATE conversation_sessions SET status='closed',updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'),
    summary_status=CASE WHEN ?1=1 AND summary_status='none' AND
      EXISTS(SELECT 1 FROM conversation_messages WHERE session_id=conversation_sessions.id AND role='user') AND
      EXISTS(SELECT 1 FROM conversation_messages WHERE session_id=conversation_sessions.id AND role='assistant')
      THEN 'pending' ELSE summary_status END
    WHERE kind='product' AND status='active'", [if summary_enabled { 1 } else { 0 }])
    .map_err(|_| PersistenceError::Write)
}
fn short_text(value: &str, max_chars: usize) -> String {
  let trimmed = value.trim();
  let mut chars = trimmed.chars();
  let short: String = chars.by_ref().take(max_chars).collect();
  if chars.next().is_some() { format!("{short}…") } else { short }
}
pub fn list_history(conn: &Connection, limit: usize) -> Result<Vec<ConversationHistoryItem>, PersistenceError> {
  let mut stmt = conn.prepare("SELECT s.id,s.created_at,s.updated_at,substr(s.title,1,71),s.status,s.summary_status,
    (SELECT COUNT(*) FROM conversation_messages m WHERE m.session_id=s.id) AS message_count,
    (SELECT substr(m.content,1,121) FROM conversation_messages m WHERE m.session_id=s.id AND m.role='user' ORDER BY m.id LIMIT 1) AS first_user,
    substr(s.summary,1,121)
    FROM conversation_sessions s WHERE s.kind='product' AND EXISTS
      (SELECT 1 FROM conversation_messages m WHERE m.session_id=s.id)
    ORDER BY s.updated_at DESC,s.id DESC LIMIT ?1").map_err(|_| PersistenceError::Read)?;
  let rows = stmt.query_map([limit.clamp(1, 100) as i64], |row| {
    let stored_title: Option<String> = row.get(3)?;
    let first_user: Option<String> = row.get(7)?;
    let summary: Option<String> = row.get(8)?;
    let summary_status: String = row.get(5)?;
    let preview = summary.as_deref().filter(|_| summary_status == "completed")
      .or(first_user.as_deref()).map(|text| short_text(text, 100)).unwrap_or_default();
    let title = stored_title.as_deref().map(|text| short_text(text, 70)).filter(|text| !text.is_empty())
      .unwrap_or_else(|| first_user.as_deref().map(|text| short_text(text, 60)).filter(|text| !text.is_empty()).unwrap_or_else(|| "Conversa sem mensagens".into()));
    Ok(ConversationHistoryItem { id:row.get(0)?,created_at:row.get(1)?,updated_at:row.get(2)?,title,
      status:row.get(4)?,summary_status,message_count:row.get(6)?,preview })
  }).map_err(|_| PersistenceError::Read)?;
  rows.collect::<Result<Vec<_>,_>>().map_err(|_| PersistenceError::Read)
}
pub fn history_session(conn: &Connection, id: i64) -> Result<Option<ConversationSession>, PersistenceError> {
  if id <= 0 { return Ok(None); }
  let is_product: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM conversation_sessions WHERE id=?1 AND kind='product')", [id], |row| row.get(0))
    .map_err(|_| PersistenceError::Read)?;
  if !is_product { return Ok(None); }
  session(conn, id)
}
pub fn outbound_history(conn: &Connection, id: i64, max_messages: usize, max_bytes: usize) -> Result<Vec<SessionTurn>, PersistenceError> {
  outbound_history_before(conn, id, max_messages, max_bytes, i64::MAX)
}
pub fn outbound_history_before(conn: &Connection, id: i64, max_messages: usize, max_bytes: usize, before: i64) -> Result<Vec<SessionTurn>, PersistenceError> {
  if !is_active_session(conn, id)? { return Err(PersistenceError::Read); }
  let mut stmt = conn.prepare("SELECT role,content FROM conversation_messages WHERE session_id=?1 AND id<?3 ORDER BY id DESC LIMIT ?2")
    .map_err(|_| PersistenceError::Read)?;
  let rows = stmt.query_map(rusqlite::params![id, max_messages as i64, before], |row| {
    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
  }).map_err(|_| PersistenceError::Read)?;
  let mut newest = Vec::new();
  let mut bytes: usize = 0;
  for row in rows {
    let (role, content) = row.map_err(|_| PersistenceError::Read)?;
    let role = match role.as_str() { "user" => SessionRole::User, "assistant" => SessionRole::Assistant, _ => return Err(PersistenceError::Read) };
    if bytes.saturating_add(content.len()) > max_bytes { break; }
    bytes += content.len();
    newest.push(SessionTurn { role, content });
  }
  newest.reverse();
  Ok(newest)
}
pub fn session(conn: &Connection, id: i64) -> Result<Option<ConversationSession>, PersistenceError> {
  if id <= 0 { return Ok(None); }
  let row = conn.query_row("SELECT created_at,updated_at,title,status,summary_status,summary,summary_updated_at FROM conversation_sessions WHERE id=?1", [id],
    |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).optional().map_err(|_| PersistenceError::Read)?;
  let Some((created_at,updated_at,title,status,summary_status,summary,summary_updated_at)) = row else { return Ok(None) };
  let mut stmt = conn.prepare("SELECT id,session_id,role,content,created_at FROM conversation_messages WHERE session_id=?1 ORDER BY id").map_err(|_| PersistenceError::Read)?;
  let messages = stmt.query_map([id], |r| Ok(ConversationMessage { id:r.get(0)?,session_id:r.get(1)?,role:r.get(2)?,content:r.get(3)?,created_at:r.get(4)? }))
    .map_err(|_| PersistenceError::Read)?.collect::<Result<Vec<_>,_>>().map_err(|_| PersistenceError::Read)?;
  Ok(Some(ConversationSession { id,created_at,updated_at,title,status,summary_status,summary,summary_updated_at,messages }))
}
pub fn append_exchange_to_session(conn: &mut Connection, id: i64, user: &str, assistant: &str) -> Result<(), PersistenceError> {
  let tx = conn.transaction().map_err(|_| PersistenceError::Write)?;
  let exists = is_active_session(&tx, id)?;
  if !exists { return Err(PersistenceError::Read); }
  tx.execute("INSERT INTO conversation_messages(session_id,role,content) VALUES (?1,'user',?2)", rusqlite::params![id,user]).map_err(|_| PersistenceError::Write)?;
  tx.execute("INSERT INTO conversation_messages(session_id,role,content) VALUES (?1,'assistant',?2)", rusqlite::params![id,assistant]).map_err(|_| PersistenceError::Write)?;
  tx.execute("UPDATE conversation_sessions SET updated_at=CURRENT_TIMESTAMP WHERE id=?1", [id]).map_err(|_| PersistenceError::Write)?;
  tx.commit().map_err(|_| PersistenceError::Write)
}
pub fn create_diagnostic(conn: &mut Connection) -> Result<i64, PersistenceError> {
  if let Some(id) = conn.query_row("SELECT id FROM conversation_sessions WHERE title=?1 LIMIT 1", [TITLE], |r| r.get(0)).optional().map_err(|_| PersistenceError::Read)? { return Ok(id); }
  let tx = conn.transaction().map_err(|_| PersistenceError::Write)?;
  tx.execute("INSERT INTO conversation_sessions(title,status) VALUES (?1,'diagnostic')", [TITLE]).map_err(|_| PersistenceError::Write)?;
  let id = tx.last_insert_rowid();
  tx.execute("INSERT INTO conversation_messages(session_id,role,content) VALUES (?1,'user','Mensagem de diagnóstico LR-4'),(?1,'assistant','Persistência local confirmada.')", [id]).map_err(|_| PersistenceError::Write)?;
  tx.commit().map_err(|_| PersistenceError::Write)?;
  Ok(id)
}
pub fn recent(conn: &Connection) -> Result<Option<ConversationSession>, PersistenceError> {
  let session = conn.query_row("SELECT id,created_at,updated_at,title,status FROM conversation_sessions ORDER BY updated_at DESC,id DESC LIMIT 1", [], |r| Ok((r.get::<_, i64>(0)?,r.get::<_, String>(1)?,r.get::<_, String>(2)?,r.get::<_, Option<String>>(3)?,r.get::<_, Option<String>>(4)?))).optional().map_err(|_| PersistenceError::Read)?;
  let Some((id,created_at,updated_at,title,status)) = session else { return Ok(None) };
  let mut stmt = conn.prepare("SELECT id,session_id,role,content,created_at FROM conversation_messages WHERE session_id=?1 ORDER BY id").map_err(|_| PersistenceError::Read)?;
  let messages = stmt.query_map([id], |r| Ok(ConversationMessage { id:r.get(0)?,session_id:r.get(1)?,role:r.get(2)?,content:r.get(3)?,created_at:r.get(4)? })).map_err(|_| PersistenceError::Read)?
    .collect::<Result<Vec<_>,_>>().map_err(|_| PersistenceError::Read)?;
  Ok(Some(ConversationSession { id,created_at,updated_at,title,status,summary_status:"none".into(),summary:None,summary_updated_at:None,messages }))
}

pub fn recent_messages_limited(conn: &Connection, limit: usize) -> Result<Vec<ConversationMessage>, PersistenceError> {
  let session_id = conn.query_row("SELECT id FROM conversation_sessions ORDER BY updated_at DESC,id DESC LIMIT 1", [], |r| r.get::<_, i64>(0))
    .optional().map_err(|_| PersistenceError::Read)?;
  let Some(session_id) = session_id else { return Ok(vec![]) };
  let mut stmt = conn.prepare("SELECT id,session_id,role,content,created_at FROM conversation_messages WHERE session_id=?1 ORDER BY id DESC LIMIT ?2")
    .map_err(|_| PersistenceError::Read)?;
  let mut messages = stmt.query_map(rusqlite::params![session_id, limit.min(6) as i64], |r| Ok(ConversationMessage {
    id:r.get(0)?,session_id:r.get(1)?,role:r.get(2)?,content:r.get(3)?,created_at:r.get(4)?
  })).map_err(|_| PersistenceError::Read)?.collect::<Result<Vec<_>,_>>().map_err(|_| PersistenceError::Read)?;
  messages.reverse(); Ok(messages)
}

pub fn gemini_session(conn: &Connection) -> Result<Option<ConversationSession>, PersistenceError> {
  let id = conn.query_row("SELECT id FROM conversation_sessions WHERE title=?1 LIMIT 1", [GEMINI_TITLE], |r| r.get::<_, i64>(0))
    .optional().map_err(|_| PersistenceError::Read)?;
  let Some(id) = id else { return Ok(None) };
  let (created_at, updated_at, status) = conn.query_row("SELECT created_at,updated_at,status FROM conversation_sessions WHERE id=?1", [id],
    |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|_| PersistenceError::Read)?;
  let mut stmt = conn.prepare("SELECT id,session_id,role,content,created_at FROM conversation_messages WHERE session_id=?1 ORDER BY id")
    .map_err(|_| PersistenceError::Read)?;
  let messages = stmt.query_map([id], |r| Ok(ConversationMessage { id:r.get(0)?,session_id:r.get(1)?,role:r.get(2)?,content:r.get(3)?,created_at:r.get(4)? }))
    .map_err(|_| PersistenceError::Read)?.collect::<Result<Vec<_>,_>>().map_err(|_| PersistenceError::Read)?;
  Ok(Some(ConversationSession { id, created_at, updated_at, title: Some(GEMINI_TITLE.into()), status, summary_status:"none".into(),summary:None,summary_updated_at:None, messages }))
}

pub fn append_gemini_exchange(conn: &mut Connection, user: &str, assistant: &str) -> Result<(), PersistenceError> {
  let tx = conn.transaction().map_err(|_| PersistenceError::Write)?;
  let existing = tx.query_row("SELECT id FROM conversation_sessions WHERE title=?1 LIMIT 1", [GEMINI_TITLE], |r| r.get::<_, i64>(0))
    .optional().map_err(|_| PersistenceError::Read)?;
  let id = if let Some(id) = existing { id } else {
    tx.execute("INSERT INTO conversation_sessions(title,status) VALUES (?1,'active')", [GEMINI_TITLE]).map_err(|_| PersistenceError::Write)?;
    tx.last_insert_rowid()
  };
  tx.execute("INSERT INTO conversation_messages(session_id,role,content) VALUES (?1,'user',?2)", rusqlite::params![id,user]).map_err(|_| PersistenceError::Write)?;
  tx.execute("INSERT INTO conversation_messages(session_id,role,content) VALUES (?1,'assistant',?2)", rusqlite::params![id,assistant]).map_err(|_| PersistenceError::Write)?;
  tx.execute("UPDATE conversation_sessions SET updated_at=CURRENT_TIMESTAMP WHERE id=?1", [id]).map_err(|_| PersistenceError::Write)?;
  tx.commit().map_err(|_| PersistenceError::Write)
}
