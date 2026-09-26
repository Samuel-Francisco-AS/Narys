use rusqlite::Connection;
use super::database::PersistenceError;
const INITIAL: &str = include_str!("../../migrations/001_initial_persistence.sql");
pub fn apply(conn: &Connection) -> Result<(), PersistenceError> {
  let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0)).map_err(|_| PersistenceError::Migration)?;
  if version > 1 { return Err(PersistenceError::Migration); }
  if version == 0 {
    conn.execute_batch(&format!("BEGIN IMMEDIATE; {INITIAL} PRAGMA user_version = 1; COMMIT;"))
      .map_err(|_| PersistenceError::Migration)?;
  }
  Ok(())
}
