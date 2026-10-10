pub mod general_settings;
pub mod gemini_settings;
pub mod conversation;
pub mod database;
pub mod identity;
pub mod memory;
pub mod provider_timeouts;
pub mod checkpoints;
pub mod continuations;
pub mod task_history;
pub mod migrations;
use database::{Database, PersistenceError};
use identity::IdentityInput;
use memory::MemoryInput;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bootstrap {
    identity: IdentityInput,
    memories: Vec<MemoryInput>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    identity_inserted: bool,
    memories_inserted: usize,
}
pub fn import_bootstrap(
    db: &Database,
    path: &std::path::Path,
) -> Result<ImportResult, PersistenceError> {
    let bytes = fs::read(path).map_err(|_| PersistenceError::InvalidBootstrap)?;
    let bootstrap: Bootstrap =
        serde_json::from_slice(&bytes).map_err(|_| PersistenceError::InvalidBootstrap)?;
    bootstrap.identity.validate()?;
    let mut keys = HashSet::new();
    for memory in &bootstrap.memories {
        memory.validate()?;
        if !keys.insert(&memory.import_key) {
            return Err(PersistenceError::Conflict);
        }
    }
    let mut conn = db.open()?;
    let tx = conn.transaction().map_err(|_| PersistenceError::Write)?;
    let identity_inserted = identity::insert_version(&tx, &bootstrap.identity)?;
    let mut memories_inserted = 0;
    for memory in &bootstrap.memories {
        if memory::import(&tx, memory)? {
            memories_inserted += 1;
        }
    }
    tx.commit().map_err(|_| PersistenceError::Write)?;
    Ok(ImportResult {
        identity_inserted,
        memories_inserted,
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lr4Status {
    pub database_available: bool,
    pub error_code: Option<&'static str>,
    pub identity_name: Option<String>,
    pub identity_version: Option<String>,
    pub memory_count: i64,
    pub conversation_count: i64,
    pub task_count: i64,
    pub memory_titles: Vec<String>,
}
pub fn status(db: &Database) -> Result<Lr4Status, PersistenceError> {
    let conn = db.open()?;
    let identity = identity::current_identity(&conn)?;
    let count = |table: &str| -> Result<i64, PersistenceError> {
        let sql = match table {
            "memory" => "SELECT COUNT(*) FROM memory_records",
            "conversation" => "SELECT COUNT(*) FROM conversation_sessions",
            _ => "SELECT COUNT(*) FROM task_records",
        };
        conn.query_row(sql, [], |r| r.get(0))
            .map_err(|_| PersistenceError::Read)
    };
    let memories = memory::list_active_memories(&conn, 5)?;
    Ok(Lr4Status {
        database_available: true,
        error_code: None,
        identity_name: identity.as_ref().map(|v| v.input.canonical_name.clone()),
        identity_version: identity.map(|v| v.input.version),
        memory_count: count("memory")?,
        conversation_count: count("conversation")?,
        task_count: count("task")?,
        memory_titles: memories.into_iter().map(|m| m.title).collect(),
    })
}

pub mod shell_settings;

pub mod ownership;
