//! Bounded read-only views of server-owned records, without result/prompt payloads.
use crate::ipc::TaskNamespace;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
pub fn tasks(
    conn: &Connection,
    namespace: TaskNamespace,
    after: u64,
    limit: u16,
) -> Result<Value, &'static str> {
    let (name, sql) = match namespace {
        TaskNamespace::Product => ("product", "SELECT id,state FROM (SELECT task_id AS id,state FROM conversation_runs UNION ALL SELECT task_id AS id,state FROM agent_runs UNION ALL SELECT task_id AS id,state FROM task_records WHERE task_id NOT IN (SELECT task_id FROM conversation_runs UNION ALL SELECT task_id FROM agent_runs)) WHERE id>?1 ORDER BY id LIMIT ?2"),
        TaskNamespace::Lr10a => ("lr10a", "SELECT id,state FROM headless_tasks WHERE id>?1 ORDER BY id LIMIT ?2"),
    };
    let mut query = conn.prepare(sql).map_err(|_| "task_read_failed")?;
    let mut rows = query
        .query_map(params![after, limit as u64 + 1], |r| {
            Ok(json!({"namespace":name,"task_id":r.get::<_,u64>(0)?,"state":r.get::<_,String>(1)?}))
        })
        .map_err(|_| "task_read_failed")?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "task_read_failed")?;
    let has_more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    let next = rows
        .last()
        .and_then(|r| r["task_id"].as_u64())
        .unwrap_or(after);
    Ok(json!({"namespace":name,"tasks":rows,"next_task":next,"has_more":has_more}))
}
