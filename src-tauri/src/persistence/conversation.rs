use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use super::database::PersistenceError;
const TITLE: &str = "Diagnóstico LR-4";
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSession { pub id: i64, pub created_at: String, pub updated_at: String, pub title: Option<String>, pub status: Option<String>, pub messages: Vec<ConversationMessage> }
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMessage { pub id: i64, pub session_id: i64, pub role: String, pub content: String, pub created_at: String }
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
  Ok(Some(ConversationSession { id,created_at,updated_at,title,status,messages }))
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
