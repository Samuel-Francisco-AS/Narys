use super::database::PersistenceError;
use rusqlite::Connection;
const INITIAL: &str = include_str!("../../migrations/001_initial_persistence.sql");
const POLICY: &str = include_str!("../../migrations/003_cognitive_role_policy.sql");
const HISTORY: &str = include_str!("../../migrations/002_conversation_history.sql");
const ADVANCED: &str =
    include_str!("../../migrations/004_cognitive_retry_and_general_settings.sql");
const GEMINI_TIMEOUTS: &str = include_str!("../../migrations/005_gemini_provider_timeouts.sql");
const COGNITIVE_ROUTING: &str = include_str!("../../migrations/006_cognitive_routing.sql");
const PROVIDER_TIMEOUTS: &str = include_str!("../../migrations/007_provider_timeouts.sql");
const ORCHESTRATOR: &str = include_str!("../../migrations/008_orchestrator_role.sql");
pub fn apply(conn: &Connection) -> Result<(), PersistenceError> {
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|_| PersistenceError::Migration)?;
    if version > 8 {
        return Err(PersistenceError::Migration);
    }
    if version == 0 {
        conn.execute_batch(&format!(
            "BEGIN IMMEDIATE; {INITIAL} PRAGMA user_version = 1; COMMIT;"
        ))
        .map_err(|_| PersistenceError::Migration)?;
    }
    if version < 2 {
        conn.execute_batch(&format!(
            "BEGIN IMMEDIATE; {HISTORY} PRAGMA user_version = 2; COMMIT;"
        ))
        .map_err(|_| PersistenceError::Migration)?;
    }
    if version < 3 {
        conn.execute_batch(&format!(
            "BEGIN IMMEDIATE; {POLICY} PRAGMA user_version = 3; COMMIT;"
        ))
        .map_err(|_| PersistenceError::Migration)?;
    }
    if version < 4 {
        conn.execute_batch(&format!(
            "BEGIN IMMEDIATE; {ADVANCED} PRAGMA user_version = 4; COMMIT;"
        ))
        .map_err(|_| PersistenceError::Migration)?;
    }
    if version < 5 {
        conn.execute_batch(&format!(
            "BEGIN IMMEDIATE; {GEMINI_TIMEOUTS} PRAGMA user_version = 5; COMMIT;"
        ))
        .map_err(|_| PersistenceError::Migration)?;
    }
    if version < 6 {
        conn.execute_batch(&format!(
            "BEGIN IMMEDIATE; {COGNITIVE_ROUTING} PRAGMA user_version = 6; COMMIT;"
        ))
        .map_err(|_| PersistenceError::Migration)?;
    }
    if version < 7 {
        conn.execute_batch(&format!(
            "BEGIN IMMEDIATE; {PROVIDER_TIMEOUTS} PRAGMA user_version = 7; COMMIT;"
        ))
        .map_err(|_| PersistenceError::Migration)?;
    }
    if version < 8 {
        conn.execute_batch(&format!(
            "BEGIN IMMEDIATE; {ORCHESTRATOR} PRAGMA user_version = 8; COMMIT;"
        ))
        .map_err(|_| PersistenceError::Migration)?;
    }
    Ok(())
}
