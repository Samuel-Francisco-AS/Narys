//! C2 durable ledger. Deliberately not connected to dispatch, Scheduler or resume.
#![allow(dead_code)] // Candidate API remains dormant until independently audited C3.
mod contracts;
use crate::cognitive_resources::{EffectState, ExecutionUnitId, ExecutionUnitState};
pub use contracts::*;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::collections::BTreeSet;

pub struct CheckpointRepository;
impl CheckpointRepository {
    /// One final receipt per unit. Identical retries return the original receipt
    /// (including its factual timestamp); differing content never overwrites it.
    pub fn commit(
        conn: &mut Connection,
        checkpoint: &CognitiveCheckpoint,
    ) -> Result<CheckpointRecord, CheckpointError> {
        require_durable_connection(conn)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| CheckpointError::Write)?;
        validate_history(&tx, checkpoint)?;
        verify_references(&tx, checkpoint)?;
        let root = checkpoint.id.unit_id().root_task_id();
        let role = checkpoint.policy.role().as_str();
        let policy_json = serde_json::to_string(&checkpoint.policy)
            .map_err(|_| CheckpointError::InvalidPolicy)?;
        let existing_policy = read_policy(&tx, root, role)?;
        match existing_policy {
            Some(policy) if policy != checkpoint.policy => return Err(CheckpointError::Conflict),
            Some(_) => {}
            None => {
                tx.execute("INSERT INTO main.checkpoint_task_policies(root_task_id,role,snapshot_json) VALUES (?1,?2,?3)", params![root,role,policy_json]).map_err(write_error)?;
            }
        }
        let record = if let Some(record) = read_one(&tx, checkpoint.id.unit_id())? {
            if record.checkpoint != *checkpoint {
                return Err(CheckpointError::Conflict);
            }
            record
        } else {
            let (kind, key) = checkpoint.provenance.source.key();
            tx.execute("INSERT INTO main.cognitive_checkpoints(root_task_id,unit_sequence,checkpoint_sequence,role,source_kind,source_key,effect_state,checkpoint_json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![root,checkpoint.id.unit_id().sequence(),checkpoint.id.sequence(),role,kind,key,StoredEffect::from_effect(checkpoint.effects).code(),checkpoint.encode()?]).map_err(write_error)?;
            let saved =
                read_one(&tx, checkpoint.id.unit_id())?.ok_or(CheckpointError::InvalidRecord)?;
            if saved.checkpoint != *checkpoint {
                return Err(CheckpointError::Conflict);
            }
            saved
        };
        // A readback inside the transaction is not yet durable. Never return it
        // on a failed COMMIT; Transaction drop rolls back both writes.
        tx.commit().map_err(|_| CheckpointError::Write)?;
        Ok(record)
    }

    /// Factual lookup only: no dispatch and no inference that absence is fresh.
    pub fn lookup(
        conn: &Connection,
        unit: ExecutionUnitId,
        source: &ExecutionSource,
    ) -> Result<CheckpointLoadResult, CheckpointError> {
        source.validate()?;
        require_durable_connection(conn)?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| CheckpointError::Read)?;
        let result = match read_one(&tx, unit) {
            Ok(Some(record)) => {
                if &record.checkpoint.provenance.source != source {
                    CheckpointLoadResult::Invalid(CheckpointError::UnitMismatch)
                } else {
                    match validate_history(&tx, &record.checkpoint)
                        .and_then(|_| verify_references(&tx, &record.checkpoint))
                    {
                        Ok(()) => CheckpointLoadResult::Committed(record),
                        Err(error) => CheckpointLoadResult::Invalid(error),
                    }
                }
            }
            Ok(None) => {
                let (kind, key) = source.key();
                let bound: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM main.cognitive_checkpoints WHERE root_task_id=?1 AND source_kind=?2 AND source_key=?3)", params![unit.root_task_id(),kind,key], |r| r.get(0)).map_err(|_| CheckpointError::Read)?;
                if bound {
                    CheckpointLoadResult::Invalid(CheckpointError::UnitMismatch)
                } else {
                    match history_state(&tx, unit.root_task_id(), source) {
                        Ok(Some((state, _))) => {
                            CheckpointLoadResult::HistoryWithoutCheckpoint { state }
                        }
                        Ok(None) => CheckpointLoadResult::Absent,
                        Err(error) => CheckpointLoadResult::Invalid(error),
                    }
                }
            }
            Err(CheckpointError::Read) => return Err(CheckpointError::Read),
            Err(error) => CheckpointLoadResult::Invalid(error),
        };
        tx.commit().map_err(|_| CheckpointError::Read)?;
        Ok(result)
    }
}

fn require_durable_connection(conn: &Connection) -> Result<(), CheckpointError> {
    if !conn.is_autocommit() {
        return Err(CheckpointError::TransactionActive);
    }
    let file: String = conn
        .query_row(
            "SELECT file FROM pragma_database_list WHERE name='main'",
            [],
            |r| r.get(0),
        )
        .map_err(|_| CheckpointError::DurabilityUnavailable)?;
    let sync: i64 = conn
        .pragma_query_value(Some(rusqlite::DatabaseName::Main), "synchronous", |r| {
            r.get(0)
        })
        .map_err(|_| CheckpointError::DurabilityUnavailable)?;
    let journal: String = conn
        .pragma_query_value(Some(rusqlite::DatabaseName::Main), "journal_mode", |r| {
            r.get(0)
        })
        .map_err(|_| CheckpointError::DurabilityUnavailable)?;
    let fk: bool = conn
        .pragma_query_value(None, "foreign_keys", |r| r.get(0))
        .map_err(|_| CheckpointError::DurabilityUnavailable)?;
    if file.is_empty()
        || !matches!(sync, 2 | 3)
        || !matches!(journal.as_str(), "delete" | "truncate" | "persist" | "wal")
        || !fk
    {
        return Err(CheckpointError::DurabilityUnavailable);
    }
    Ok(())
}
fn write_error(error: rusqlite::Error) -> CheckpointError {
    match error {
        rusqlite::Error::SqliteFailure(e, _)
            if e.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY
                || e.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
        {
            CheckpointError::Conflict
        }
        _ => CheckpointError::Write,
    }
}
fn read_error(error: rusqlite::Error) -> CheckpointError {
    match error {
        rusqlite::Error::SqliteFailure(_, _) => CheckpointError::Read,
        _ => CheckpointError::InvalidRecord,
    }
}
fn read_policy(
    conn: &Connection,
    root: u64,
    role: &str,
) -> Result<Option<TaskPolicySnapshot>, CheckpointError> {
    let json: Option<String> = conn.query_row("SELECT CASE WHEN typeof(snapshot_json)='text' AND length(CAST(snapshot_json AS BLOB)) BETWEEN 1 AND 16384 THEN snapshot_json ELSE NULL END FROM main.checkpoint_task_policies WHERE root_task_id=?1 AND role=?2", params![root,role], |r| r.get(0)).optional().map_err(read_error)?;
    json.map(|json| {
        let value: TaskPolicySnapshot =
            serde_json::from_str(&json).map_err(|_| CheckpointError::InvalidRecord)?;
        value.validate()?;
        if value.role().as_str() != role {
            return Err(CheckpointError::InvalidPolicy);
        }
        Ok(value)
    })
    .transpose()
}
fn read_one(
    conn: &Connection,
    unit: ExecutionUnitId,
) -> Result<Option<CheckpointRecord>, CheckpointError> {
    let raw = conn.query_row("SELECT checkpoint_sequence,role,source_kind,source_key,effect_state,commit_state,CASE WHEN typeof(checkpoint_json)='text' AND length(CAST(checkpoint_json AS BLOB)) BETWEEN 1 AND 8192 THEN checkpoint_json ELSE NULL END,CASE WHEN typeof(committed_at)='text' AND length(CAST(committed_at AS BLOB))=24 THEN committed_at ELSE NULL END FROM main.cognitive_checkpoints WHERE root_task_id=?1 AND unit_sequence=?2", params![unit.root_task_id(),unit.sequence()], |r| {
        Ok((r.get::<_, u64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,String>(6)?,r.get::<_,String>(7)?))
    }).optional().map_err(read_error)?;
    let Some((sequence, role, kind, key, effects, state, json, timestamp)) = raw else {
        return Ok(None);
    };
    if state != "committed" {
        return Err(CheckpointError::InvalidRecord);
    }
    let date = chrono::DateTime::parse_from_rfc3339(&timestamp)
        .map_err(|_| CheckpointError::InvalidRecord)?;
    if date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true) != timestamp {
        return Err(CheckpointError::InvalidRecord);
    }
    let policy =
        read_policy(conn, unit.root_task_id(), &role)?.ok_or(CheckpointError::InvalidPolicy)?;
    let wire: CheckpointWire =
        serde_json::from_str(&json).map_err(|_| CheckpointError::InvalidRecord)?;
    let checkpoint = wire.validated(policy)?;
    if checkpoint.id.unit_id() != unit
        || checkpoint.id.sequence() != sequence
        || checkpoint.provenance.source.key() != (kind.as_str(), key.as_str())
        || StoredEffect::from_effect(checkpoint.effects).code() != effects
    {
        return Err(CheckpointError::InvalidRecord);
    }
    Ok(Some(CheckpointRecord {
        checkpoint,
        committed_at: timestamp,
    }))
}

fn verify_references(
    conn: &Connection,
    checkpoint: &CognitiveCheckpoint,
) -> Result<(), CheckpointError> {
    let mut pending: Vec<_> = checkpoint
        .predecessor
        .into_iter()
        .chain(checkpoint.context.completed_dependencies().iter().copied())
        .collect();
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        if visited.len() > MAX_VERIFIED_CHECKPOINTS {
            return Err(CheckpointError::VerificationLimit);
        }
        let record = read_one(conn, id.unit_id())?.ok_or(CheckpointError::ReferenceNotCommitted)?;
        if record.checkpoint.id != id || record.checkpoint.effects == EffectState::UnknownOrInFlight
        {
            return Err(CheckpointError::ReferenceNotCommitted);
        }
        validate_history(conn, &record.checkpoint)?;
        pending.extend(record.checkpoint.predecessor);
        pending.extend(
            record
                .checkpoint
                .context
                .completed_dependencies()
                .iter()
                .copied(),
        );
    }
    Ok(())
}

fn parse_history_state(state: &str) -> Result<ExecutionUnitState, CheckpointError> {
    match state {
        "completed" => Ok(ExecutionUnitState::Completed),
        "cancelled" => Ok(ExecutionUnitState::Cancelled),
        "failed" => Ok(ExecutionUnitState::Failed),
        "blocked" => Ok(ExecutionUnitState::Unknown),
        _ => Err(CheckpointError::HistoryContradiction),
    }
}
/// Absence is allowed before terminal history is written. Existing terminal
/// history must agree with the particular unit, not just the root outcome.
fn history_state(
    conn: &Connection,
    root: u64,
    source: &ExecutionSource,
) -> Result<Option<(ExecutionUnitState, Option<String>)>, CheckpointError> {
    let root_row: Option<(String, String)> = conn
        .query_row(
            "SELECT state,kind FROM main.task_records WHERE task_id=?1",
            [root],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(read_error)?;
    match source {
        ExecutionSource::RootTask => root_row
            .map(|(state, _)| Ok((parse_history_state(&state)?, None)))
            .transpose(),
        ExecutionSource::TaskGraphSubtask(id) => {
            let subtask: Option<(String,Option<String>)> = conn.query_row("SELECT state,provider_id FROM main.task_subtask_records WHERE root_task_id=?1 AND subtask_id=?2", params![root,id], |r| Ok((r.get(0)?,r.get(1)?))).optional().map_err(read_error)?;
            match (root_row, subtask) {
                (None, None) => Ok(None),
                (Some((root_state, kind)), Some((state, runtime))) if kind == "task_graph" => {
                    if !matches!(root_state.as_str(), "completed" | "cancelled" | "failed") {
                        return Err(CheckpointError::HistoryContradiction);
                    }
                    parse_history_state(&root_state)?;
                    if runtime.as_ref().is_some_and(|v| v.len() > 64) {
                        return Err(CheckpointError::HistoryContradiction);
                    }
                    Ok(Some((parse_history_state(&state)?, runtime)))
                }
                _ => Err(CheckpointError::HistoryContradiction),
            }
        }
    }
}
fn validate_history(
    conn: &Connection,
    checkpoint: &CognitiveCheckpoint,
) -> Result<(), CheckpointError> {
    if let Some((state, runtime)) = history_state(
        conn,
        checkpoint.id.unit_id().root_task_id(),
        &checkpoint.provenance.source,
    )? {
        if state != ExecutionUnitState::Completed
            || (matches!(
                checkpoint.provenance.source,
                ExecutionSource::TaskGraphSubtask(_)
            ) && runtime.as_deref() != Some(checkpoint.provenance.runtime_id.as_str()))
        {
            return Err(CheckpointError::HistoryContradiction);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
