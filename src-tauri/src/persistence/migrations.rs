use rusqlite::Connection;
use super::database::PersistenceError;
const INITIAL: &str = include_str!("../../migrations/001_initial_persistence.sql");
const POLICY: &str = include_str!("../../migrations/003_cognitive_role_policy.sql");
const HISTORY: &str = include_str!("../../migrations/002_conversation_history.sql");
pub fn apply(conn: &Connection) -> Result<(), PersistenceError> {
  let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0)).map_err(|_| PersistenceError::Migration)?;
  if version > 3 { return Err(PersistenceError::Migration); }
  if version == 0 {
    conn.execute_batch(&format!("BEGIN IMMEDIATE; {INITIAL} PRAGMA user_version = 1; COMMIT;"))
      .map_err(|_| PersistenceError::Migration)?;
  }
  if version < 2 {
    conn.execute_batch(&format!("BEGIN IMMEDIATE; {HISTORY} PRAGMA user_version = 2; COMMIT;"))
      .map_err(|_| PersistenceError::Migration)?;
  }
  if version < 3 {
    conn.execute_batch(&format!("BEGIN IMMEDIATE; {POLICY} PRAGMA user_version = 3; COMMIT;"))
      .map_err(|_| PersistenceError::Migration)?;
  }
  Ok(())
}
