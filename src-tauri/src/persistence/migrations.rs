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
    if version > 16 {
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
    if version < 9 {
        conn.execute_batch(concat!(
            "BEGIN IMMEDIATE;",
            include_str!("../../migrations/009_cognitive_role_targets.sql"),
            "PRAGMA user_version = 9; COMMIT;"
        ))
        .map_err(|_| PersistenceError::Migration)?;
    }
    if version < 10 {
        conn.execute_batch(concat!(
            "BEGIN IMMEDIATE;",
            include_str!("../../migrations/010_provider_timeout_defaults.sql"),
            "PRAGMA user_version = 10; COMMIT;"
        ))
        .map_err(|_| PersistenceError::Migration)?;
    }
    if version < 11 {
        conn.execute_batch(concat!(
            "BEGIN IMMEDIATE;",
            include_str!("../../migrations/011_worker_role_and_task_graph.sql"),
            "PRAGMA user_version = 11; COMMIT;"
        ))
        .map_err(|_| PersistenceError::Migration)?;
    }
    if version < 12 {
        conn.execute_batch(concat!(
            "BEGIN IMMEDIATE;",
            include_str!("../../migrations/012_cognitive_rate_state.sql"),
            "PRAGMA user_version = 12; COMMIT;"
        ))
        .map_err(|_| PersistenceError::Migration)?;
    }
    if version < 13 {
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| PersistenceError::Migration)?;
        tx.execute_batch(include_str!(
            "../../migrations/013_cognitive_allocation_policy.sql"
        ))
        .map_err(|_| PersistenceError::Migration)?;
        tx.pragma_update(None, "user_version", 13)
            .map_err(|_| PersistenceError::Migration)?;
        tx.commit().map_err(|_| PersistenceError::Migration)?;
    }
    if version < 14 {
        let tx =
            rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
                .map_err(|_| PersistenceError::Migration)?;
        tx.execute_batch(include_str!(
            "../../migrations/014_cognitive_checkpoints.sql"
        ))
        .map_err(|_| PersistenceError::Migration)?;
        tx.pragma_update(None, "user_version", 14)
            .map_err(|_| PersistenceError::Migration)?;
        tx.commit().map_err(|_| PersistenceError::Migration)?;
    }
    if version < 15 {
        let tx =
            rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
                .map_err(|_| PersistenceError::Migration)?;
        tx.execute_batch(include_str!(
            "../../migrations/015_cognitive_continuations.sql"
        ))
        .map_err(|_| PersistenceError::Migration)?;
        tx.pragma_update(None, "user_version", 15)
            .map_err(|_| PersistenceError::Migration)?;
        tx.commit().map_err(|_| PersistenceError::Migration)?;
    }
    if version < 16 {
        let tx =
            rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
                .map_err(|_| PersistenceError::Migration)?;
        tx.execute_batch(include_str!(
            "../../migrations/016_cognitive_cancel_request.sql"
        ))
        .map_err(|_| PersistenceError::Migration)?;
        tx.pragma_update(None, "user_version", 16)
            .map_err(|_| PersistenceError::Migration)?;
        tx.commit().map_err(|_| PersistenceError::Migration)?;
    }
    Ok(())
}
