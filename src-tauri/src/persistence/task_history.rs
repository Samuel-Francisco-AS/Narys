use rusqlite::{params, Connection};
use super::database::PersistenceError;
#[derive(Debug)]
pub struct TaskRecord { pub task_id: u64, pub kind: String, pub state: String, pub started_at: String, pub finished_at: String, pub summary: Option<String>, pub error_code: Option<String> }
pub fn insert(conn: &Connection, record: &TaskRecord) -> Result<(), PersistenceError> {
  if !["completed","cancelled","failed"].contains(&record.state.as_str()) { return Err(PersistenceError::Write); }
  conn.execute("INSERT INTO task_records(task_id,kind,state,started_at,finished_at,summary,error_code) VALUES (?1,?2,?3,?4,?5,?6,?7)", params![record.task_id,record.kind,record.state,record.started_at,record.finished_at,record.summary,record.error_code]).map_err(|_| PersistenceError::Write)?;
  Ok(())
}
pub fn max_id(conn: &Connection) -> Result<u64, PersistenceError> { conn.query_row("SELECT COALESCE(MAX(task_id),0) FROM task_records", [], |r| r.get(0)).map_err(|_| PersistenceError::Read) }
pub fn mark_failed(conn: &Connection, task_id: u64, error_code: &str) -> Result<(), PersistenceError> {
  let updated = conn.execute("UPDATE task_records SET state='failed',error_code=?2 WHERE task_id=?1", params![task_id,error_code])
    .map_err(|_| PersistenceError::Write)?;
  if updated == 1 { Ok(()) } else { Err(PersistenceError::Write) }
}
