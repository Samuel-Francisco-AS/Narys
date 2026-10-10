//! Durable adapter for the shared Conversation engine. Recovery never sends.
use super::{
    conversation,
    task_history::{self, TaskRecord},
};
use crate::{
    cognition::{policy::CognitiveRolePolicy, types::TaskResult},
    luna::task::{TaskEvent, TaskEventKind},
};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

pub fn event(
    conn: &Connection,
    id: Option<u64>,
    code: &str,
    details: Option<Value>,
) -> Result<(), &'static str> {
    conn.execute(
        "INSERT INTO server_events(namespace,task_id,code,details_json) VALUES('product',?1,?2,?3)",
        params![id, code, details.map(|d| d.to_string())],
    )
    .map_err(|_| "conversation_event_write_failed")?;
    conn.execute("DELETE FROM server_events WHERE sequence <= (SELECT COALESCE(max(sequence),0)-4096 FROM server_events)",[]).map_err(|_|"conversation_event_write_failed")?;
    Ok(())
}
pub fn admit(conn: &mut Connection, id: u64, session: i64, text: &str) -> Result<(), &'static str> {
    let tx = conn
        .transaction()
        .map_err(|_| "conversation_admission_failed")?;
    if !conversation::is_active_session(&tx, session).map_err(|e| e.code())? {
        return Err("session_invalid");
    }
    tx.execute(
        "INSERT INTO conversation_messages(session_id,role,content) VALUES(?1,'user',?2)",
        params![session, text],
    )
    .map_err(|_| "conversation_admission_failed")?;
    let message = tx.last_insert_rowid();
    tx.execute("INSERT INTO conversation_runs(task_id,session_id,user_message_id,state) VALUES(?1,?2,?3,'pending')",params![id,session,message]).map_err(|_|"conversation_busy")?;
    tx.execute("UPDATE conversation_sessions SET updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",[session]).map_err(|_|"conversation_admission_failed")?;
    event(
        &tx,
        Some(id),
        "conversation_admitted",
        Some(json!({"session_id":session,"no_restart_replay":true})),
    )?;
    tx.commit().map_err(|_| "conversation_admission_failed")
}
pub fn policy_snapshot(
    conn: &Connection,
    id: u64,
    policy: &CognitiveRolePolicy,
) -> Result<(), &'static str> {
    // All route candidates must be confirmed; fallback must never change the cost boundary.
    for target in &policy.targets {
        let allowed:bool=conn.query_row("SELECT enabled=1 AND free_tier_confirmed=1 FROM server_provider_permissions WHERE provider_id=?1",[&target.provider_id],|r|r.get(0)).optional().map_err(|_|"provider_permission_read_failed")?.unwrap_or(false);
        if !allowed {
            return Err("free_provider_authorization_required");
        }
    }
    conn.execute("UPDATE conversation_runs SET policy_json=?2 WHERE task_id=?1 AND state IN ('pending','running')",params![id,serde_json::to_string(policy).map_err(|_|"policy_encode_failed")?]).map_err(|_|"conversation_policy_write_failed")?;
    Ok(())
}
pub fn observe(conn: &mut Connection, event_value: &TaskEvent) -> Result<(), &'static str> {
    // Never persist raw chunks, prompts, provider errors or arbitrary subscriber data as events.
    let (code, details) = match &event_value.kind {
        TaskEventKind::TaskStarted => ("task_started", None),
        TaskEventKind::ContextBuilt {
            memory_count,
            recent_message_count,
        } => (
            "context_built",
            Some(json!({"memory_count":memory_count,"recent_message_count":recent_message_count})),
        ),
        TaskEventKind::ProviderSelected {
            provider_id,
            model,
            attempt,
            routing_reason,
            score,
        } => (
            "provider_selected",
            Some(
                json!({"provider_id":provider_id,"model":model,"attempt":attempt,"routing_reason":routing_reason,"score":score}),
            ),
        ),
        TaskEventKind::ProviderQueued {
            provider_id,
            traffic_class,
            queue_depth,
        } => (
            "provider_queued",
            Some(
                json!({"provider_id":provider_id,"traffic_class":traffic_class,"queue_depth":queue_depth}),
            ),
        ),
        TaskEventKind::ProviderAdmitted {
            provider_id,
            traffic_class,
            queue_delay_ms,
        } => (
            "provider_admitted",
            Some(
                json!({"provider_id":provider_id,"traffic_class":traffic_class,"queue_delay_ms":queue_delay_ms}),
            ),
        ),
        TaskEventKind::ProviderRetry {
            provider_id,
            reason_code,
        } => (
            "provider_retry",
            Some(json!({"provider_id":provider_id,"reason_code":reason_code})),
        ),
        TaskEventKind::ProviderFallback {
            from_provider_id,
            to_provider_id,
            reason_code,
        } => (
            "provider_fallback",
            Some(json!({"from":from_provider_id,"to":to_provider_id,"reason_code":reason_code})),
        ),
        TaskEventKind::ProviderOutputObserved { provider_id } => (
            "provider_output_observed",
            Some(json!({"provider_id":provider_id})),
        ),
        _ => return Ok(()),
    };
    let tx = conn
        .transaction()
        .map_err(|_| "conversation_event_write_failed")?;
    if code == "task_started" {
        tx.execute(
            "UPDATE conversation_runs SET state='running' WHERE task_id=?1 AND state='pending'",
            [event_value.task_id.0],
        )
        .map_err(|_| "conversation_event_write_failed")?;
    }
    event(&tx, Some(event_value.task_id.0), code, details)?;
    tx.commit().map_err(|_| "conversation_event_write_failed")
}
pub fn finish(
    conn: &mut Connection,
    id: u64,
    state: &str,
    error: Option<&str>,
    result: Option<&TaskResult>,
) -> Result<(), &'static str> {
    let tx = conn
        .transaction()
        .map_err(|_| "conversation_completion_failed")?;
    let (session,started):(i64,String)=tx.query_row("SELECT session_id,started_at FROM conversation_runs WHERE task_id=?1 AND state IN ('pending','running')",[id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|_|"conversation_not_active")?;
    if state == "completed" {
        let result = result.ok_or("conversation_result_missing")?;
        if !conversation::is_active_session(&tx, session).map_err(|e| e.code())? {
            return Err("session_invalid");
        }
        tx.execute(
            "INSERT INTO conversation_messages(session_id,role,content) VALUES(?1,'assistant',?2)",
            params![session, result.text],
        )
        .map_err(|_| "conversation_completion_failed")?;
        tx.execute("UPDATE conversation_sessions SET updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",[session]).map_err(|_|"conversation_completion_failed")?;
    }
    let record = TaskRecord {
        task_id: id,
        kind: "conversation".into(),
        state: if state == "interrupted" {
            "failed"
        } else {
            state
        }
        .into(),
        started_at: started,
        finished_at: chrono::Utc::now().to_rfc3339(),
        summary: Some("Headless Conversation".into()),
        error_code: error.map(str::to_owned),
    };
    task_history::insert(&tx, &record).map_err(|e| e.code())?;
    tx.execute("UPDATE conversation_runs SET state=?2,error_code=?3,result_json=?4,finished_at=?5 WHERE task_id=?1",params![id,state,error,result.map(serde_json::to_string).transpose().map_err(|_|"conversation_result_encode_failed")?,record.finished_at]).map_err(|_|"conversation_completion_failed")?;
    event(
        &tx,
        Some(id),
        if state == "interrupted" {
            "restart_never_retries"
        } else {
            state
        },
        Some(json!({"session_id":session,"error_code":error})),
    )?;
    tx.commit().map_err(|_| "conversation_completion_failed")
}
pub fn recover(conn: &mut Connection) -> Result<usize, &'static str> {
    let ids = conn
        .prepare("SELECT task_id FROM conversation_runs WHERE state IN ('pending','running')")
        .map_err(|_| "conversation_recovery_failed")?
        .query_map([], |r| r.get::<_, u64>(0))
        .map_err(|_| "conversation_recovery_failed")?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "conversation_recovery_failed")?;
    for id in &ids {
        finish(
            conn,
            *id,
            "interrupted",
            Some("restart_never_retries"),
            None,
        )?;
    }
    Ok(ids.len())
}
pub fn get(conn: &Connection, id: u64) -> Result<Value, &'static str> {
    let run=conn.query_row("SELECT session_id,state,error_code,result_json,policy_json,started_at,finished_at FROM conversation_runs WHERE task_id=?1",[id],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,Option<String>>(3)?,r.get::<_,Option<String>>(4)?,r.get::<_,String>(5)?,r.get::<_,Option<String>>(6)?))).optional().map_err(|_|"task_read_failed")?;
    if let Some((session, state, error, result, policy, started, finished)) = run {
        return Ok(
            json!({"task_id":id,"namespace":"product","kind":"conversation","session_id":session,"state":state,"error_code":error,"result":result.and_then(|s|serde_json::from_str::<Value>(&s).ok()),"policy":policy.and_then(|s|serde_json::from_str::<Value>(&s).ok()),"started_at":started,"finished_at":finished,"restart_replay":false}),
        );
    }
    conn.query_row("SELECT kind,state,error_code,started_at,finished_at,summary FROM task_records WHERE task_id=?1",[id],|r|Ok(json!({"task_id":id,"namespace":"product","kind":r.get::<_,String>(0)?,"state":r.get::<_,String>(1)?,"error_code":r.get::<_,Option<String>>(2)?,"started_at":r.get::<_,String>(3)?,"finished_at":r.get::<_,String>(4)?,"summary":r.get::<_,Option<String>>(5)?,"historical":true,"restart_replay":false}))).map_err(|_|"task_not_found")
}

#[cfg(test)]
mod tests;
