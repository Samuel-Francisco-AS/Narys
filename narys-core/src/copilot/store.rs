//! Uses the existing authoritative Database and writer lease, never a second DB.
use crate::{
    agents::lifecycle::{AgentLifecycleOperation, AgentSessionRef},
    persistence::database::Database,
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::path::Path;

pub fn event(
    conn: &Connection,
    id: u64,
    correlation: &str,
    code: &str,
) -> Result<(), &'static str> {
    crate::persistence::conversation_runs::event(
        conn,
        Some(id),
        code,
        Some(
            json!({"specialist_id":"copilot","correlation_id":correlation,"operation":"lifecycle","inference":false,"tools":false}),
        ),
    )
}
pub fn recover(conn: &mut Connection) -> Result<(), &'static str> {
    let tx = conn.transaction().map_err(|_| "agent_recovery_failed")?;
    tx.execute("INSERT INTO server_events(namespace,task_id,code,details_json) SELECT 'product',task_id,'agent_restart_never_retries',json_object('specialist_id','copilot','correlation_id',correlation_id) FROM agent_runs WHERE state IN ('pending','running')",[]).map_err(|_|"agent_recovery_failed")?;
    tx.execute("UPDATE agent_runs SET state='interrupted',graph_state='cancelled',error_code='restart_never_retries',finished_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE state IN ('pending','running')",[]).map_err(|_|"agent_recovery_failed")?;
    tx.execute("UPDATE agent_sessions SET state='interrupted',active_task_id=NULL,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE state IN ('creating','resuming')",[]).map_err(|_|"agent_recovery_failed")?;
    tx.commit().map_err(|_| "agent_recovery_failed")
}
pub fn max_id(conn: &Connection) -> Result<u64, &'static str> {
    conn.query_row("SELECT coalesce(max(task_id),0) FROM agent_runs", [], |r| {
        r.get(0)
    })
    .map_err(|_| "agent_read_failed")
}
pub fn admit(
    db: &Database,
    id: u64,
    reference: &AgentSessionRef,
    operation: AgentLifecycleOperation,
    directory: &Path,
    correlation: &str,
) -> Result<(Option<String>, Option<String>), &'static str> {
    let mut conn = db.open().map_err(|e| e.code())?;
    let tx = conn.transaction().map_err(|_| "agent_write_failed")?;
    let provider = match operation {
        AgentLifecycleOperation::Create => {
            tx.execute("INSERT INTO agent_sessions(session_ref,specialist_id,state,private_directory,active_task_id) VALUES(?1,'copilot','creating',?2,?3)",params![reference.0,directory.to_str(),id]).map_err(|_|"agent_write_failed")?;
            (None, None)
        }
        AgentLifecycleOperation::Resume => {
            let (provider,path,anchor):(String,String,String)=tx.query_row("SELECT provider_session_id,private_directory,provider_history_anchor FROM agent_sessions WHERE session_ref=?1 AND state='detached' AND active_task_id IS NULL",[&reference.0],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|_|"session_not_resumable")?;
            if Path::new(&path) != directory {
                return Err("session_directory_mismatch");
            }
            crate::policy::private_directory(directory)?;
            crate::policy::private_directory(&directory.join("session-state"))?;
            tx.execute(
                "UPDATE agent_sessions SET state='resuming',active_task_id=?2 WHERE session_ref=?1",
                params![reference.0, id],
            )
            .map_err(|_| "agent_write_failed")?;
            (Some(provider), Some(anchor))
        }
    };
    let operation = match operation {
        AgentLifecycleOperation::Create => "create",
        AgentLifecycleOperation::Resume => "resume",
    };
    tx.execute("INSERT INTO agent_runs(task_id,session_ref,correlation_id,operation,state) VALUES(?1,?2,?3,?4,'pending')",params![id,reference.0,correlation,operation]).map_err(|_|"agent_write_failed")?;
    // Audit admission is required before effects. Subsequent observation failures
    // are counted as gaps and do not change an otherwise valid operation outcome.
    event(&tx, id, correlation, "agent_admitted")?;
    tx.commit().map_err(|_| "agent_write_failed")?;
    Ok(provider)
}
pub fn task(conn: &Connection, id: u64) -> Result<Value, &'static str> {
    conn.query_row("SELECT session_ref,correlation_id,operation,state,error_code,cleanup_verified,observation_gaps,graph_state,started_at,finished_at FROM agent_runs WHERE task_id=?1",[id],|r|Ok(json!({"task_id":id,"namespace":"product","specialist_id":"copilot","session_ref":r.get::<_,String>(0)?,"correlation_id":r.get::<_,String>(1)?,"operation":r.get::<_,String>(2)?,"state":r.get::<_,String>(3)?,"error_code":r.get::<_,Option<String>>(4)?,"cleanup_verified":r.get::<_,bool>(5)?,"observation_gaps":r.get::<_,u64>(6)?,"graph_state":r.get::<_,String>(7)?,"started_at":r.get::<_,String>(8)?,"finished_at":r.get::<_,Option<String>>(9)?,"sdk_send_calls":0,"tools_executed":0}))).map_err(|_|"agent_task_not_found")
}
pub fn contains(conn: &Connection, id: u64) -> Result<bool, &'static str> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_runs WHERE task_id=?1)",
        [id],
        |r| r.get(0),
    )
    .map_err(|_| "agent_read_failed")
}
pub fn session(conn: &Connection, reference: &AgentSessionRef) -> Result<Value, &'static str> {
    conn.query_row("SELECT state,active_task_id,created_at,updated_at FROM agent_sessions WHERE session_ref=?1",[&reference.0],|r|Ok(json!({"session_ref":reference,"specialist_id":"copilot","state":r.get::<_,String>(0)?,"active_task_id":r.get::<_,Option<u64>>(1)?,"created_at":r.get::<_,String>(2)?,"updated_at":r.get::<_,String>(3)?,"durability":"core_record_durable_sdk_resume_requires_explicit_verification"}))).map_err(|_|"agent_session_not_found")
}
pub fn directory(
    conn: &Connection,
    reference: &AgentSessionRef,
) -> Result<std::path::PathBuf, &'static str> {
    conn.query_row(
        "SELECT private_directory FROM agent_sessions WHERE session_ref=?1",
        [&reference.0],
        |r| r.get::<_, String>(0),
    )
    .map(std::path::PathBuf::from)
    .map_err(|_| "agent_session_not_found")
}
pub fn finish(
    db: &Database,
    id: u64,
    state: &str,
    code: Option<&str>,
    provider: Option<&str>,
    cleanup: bool,
    gaps: u64,
    graph: &str,
    correlation: &str,
    runtime_ref: Option<&str>,
    anchor: Option<&str>,
) -> Result<(), &'static str> {
    let mut conn = db.open().map_err(|e| e.code())?;
    let tx = conn.transaction().map_err(|_| "agent_write_failed")?;
    let cleanup = cleanup
        || runtime_ref.is_some_and(|reference| {
            tx.query_row(
                "SELECT cleanup_verified FROM agent_runtime_owners WHERE runtime_ref=?1",
                [reference],
                |r| r.get::<_, bool>(0),
            )
            .unwrap_or(false)
        });
    let session_state = match state {
        "completed" => "detached",
        "cancelled" => "cancelled",
        _ => "failed",
    };
    tx.execute("UPDATE agent_sessions SET state=?2,provider_session_id=coalesce(?3,provider_session_id),provider_history_anchor=coalesce(?4,provider_history_anchor),active_task_id=NULL,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE active_task_id=?1",params![id,session_state,provider,anchor]).map_err(|_|"agent_write_failed")?;
    if tx.execute("UPDATE agent_runs SET state=?2,error_code=?3,cleanup_verified=?4,observation_gaps=?5,graph_state=?6,finished_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'),runtime_ref=?7 WHERE task_id=?1 AND state IN ('pending','running')",params![id,state,code,cleanup,gaps,graph,runtime_ref]).map_err(|_|"agent_write_failed")?!=1 {return Err("agent_terminal_conflict");}
    if cleanup {
        tx.execute(
            "UPDATE agent_runs SET cleanup_verified=1 WHERE runtime_ref=?1",
            [runtime_ref],
        )
        .map_err(|_| "agent_write_failed")?;
    }
    if event(
        &tx,
        id,
        correlation,
        match state {
            "completed" => "agent_completed",
            "cancelled" => "agent_cancelled",
            _ => "agent_failed",
        },
    )
    .is_err()
    {
        tx.execute(
            "UPDATE agent_runs SET observation_gaps=observation_gaps+1 WHERE task_id=?1",
            [id],
        )
        .map_err(|_| "agent_write_failed")?;
    }
    tx.commit().map_err(|_| "agent_write_failed")
}
