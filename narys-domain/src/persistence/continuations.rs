//! Core-owned continuation ledger. Loading/recovery never executes a provider.
use super::{
    checkpoints::*,
    database::Database,
    task_history::{self, SubtaskRecord, TaskRecord},
};
use crate::{
    agents::planner::{PlanStepV1, PlanV1},
    cognition::{
        policy::CognitiveRole,
        scheduler::AllocationSelection,
        task_graph::{TaskGraph, TaskGraphSubtaskResult},
        types::{ProviderTimeouts, SchedulerUsage, TaskBudget},
    },
    cognitive_resources::{EffectState, ExecutionUnitId, RuntimeId, MAX_HANDOFF_SEQUENCE},
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_MANIFEST_BYTES: usize = 48 * 1024;
pub const MAX_RESULT_BYTES: usize = 16 * 1024;
const MAX_RESULT_JSON_BYTES: usize = 100 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PauseReason {
    EconomicAuthorization,
    RecoveryRequired,
    UncertainExecution,
    InsufficientDurableContext,
    InvalidRecovery,
}
impl PauseReason {
    pub fn code(self) -> &'static str {
        match self {
            Self::EconomicAuthorization => "economic_authorization",
            Self::RecoveryRequired => "recovery_required",
            Self::UncertainExecution => "uncertain_execution",
            Self::InsufficientDurableContext => "insufficient_durable_context",
            Self::InvalidRecovery => "invalid_recovery",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContinuationManifest {
    pub version: u8,
    pub root: u64,
    pub objective: String,
    pub steps: Vec<PlanStepV1>,
    pub policy: TaskPolicySnapshot,
    pub timeouts: BTreeMap<String, ProviderTimeouts>,
    pub identity_version: String,
    pub planner_provider_id: String,
    pub planner_usage: SchedulerUsage,
}
impl ContinuationManifest {
    pub fn plan(&self) -> PlanV1 {
        PlanV1 {
            version: 1,
            objective: self.objective.clone(),
            steps: self.steps.clone(),
            risks: vec![],
            questions: vec![],
            needs_user_input: false,
        }
    }
    fn validate(&self, root: u64) -> Result<(), &'static str> {
        if self.version != 1 || self.root != root || ExecutionUnitId::new(root, 1).is_err() {
            return Err("continuation_identity_invalid");
        }
        TaskGraph::compile(&self.plan()).map_err(|_| "continuation_manifest_invalid")?;
        if self
            .steps
            .iter()
            .any(|s| s.depends_on.iter().collect::<BTreeSet<_>>().len() != s.depends_on.len())
        {
            return Err("continuation_manifest_invalid");
        }
        TaskPolicySnapshot::new(
            self.policy.routing().clone(),
            self.policy.allocation().cloned(),
        )
        .map_err(|_| "continuation_policy_invalid")?;
        if self.policy.role() != CognitiveRole::Worker
            || self.identity_version.trim().is_empty()
            || self.identity_version.len() > 128
            || RuntimeId::new(&self.planner_provider_id).is_err()
        {
            return Err("continuation_manifest_invalid");
        }
        if self.timeouts.len() != self.policy.routing().targets.len()
            || self
                .policy
                .routing()
                .targets
                .iter()
                .any(|t| !self.timeouts.contains_key(&t.provider_id))
            || self
                .timeouts
                .values()
                .any(|t| t.request_timeout_ms == 0 || t.stream_idle_timeout_ms == 0)
        {
            return Err("continuation_manifest_invalid");
        }
        if serde_json::to_vec(self)
            .map_err(|_| "continuation_manifest_invalid")?
            .len()
            > MAX_MANIFEST_BYTES
        {
            return Err("continuation_manifest_bounds");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ContinuationLease {
    pub root: u64,
    pub generation: u64,
}
pub struct RestoredUnit {
    pub receipt: CheckpointRecord,
    pub result: TaskGraphSubtaskResult,
    pub selection: AllocationSelection,
}
pub struct ContinuationLoad {
    pub manifest: ContinuationManifest,
    pub state: String,
    pub cancel_requested: bool,
    pub completed: BTreeMap<String, RestoredUnit>,
    pub uncertain: BTreeSet<String>,
    pub generation: u64,
    pub max_sequence: u64,
    pub uncertain_budget: SchedulerUsage,
    pub pause_reason: Option<PauseReason>,
}

pub struct ContinuationRepository;
impl ContinuationRepository {
    pub fn create(
        conn: &mut Connection,
        manifest: &ContinuationManifest,
    ) -> Result<ContinuationLease, &'static str> {
        manifest.validate(manifest.root)?;
        durable(conn)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| "continuation_write_failed")?;
        // A manifest cannot attach itself to an old receipt or terminal root.
        let known: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM main.cognitive_checkpoints WHERE root_task_id=?1 UNION ALL SELECT 1 FROM main.task_records WHERE task_id=?1)", [manifest.root], |r| r.get(0)).map_err(|_| "continuation_read_failed")?;
        if known {
            return Err("continuation_identity_conflict");
        }
        tx.execute("INSERT INTO main.cognitive_continuations(root_task_id,manifest_json,state,generation) VALUES (?1,?2,'running',1)",params![manifest.root, encode(manifest)?]).map_err(|_| "continuation_identity_conflict")?;
        for step in &manifest.steps {
            tx.execute("INSERT INTO main.cognitive_continuation_units(root_task_id,subtask_id,state) VALUES (?1,?2,'not_started')",params![manifest.root,step.id]).map_err(|_| "continuation_write_failed")?;
        }
        tx.commit().map_err(|_| "continuation_write_failed")?;
        Ok(ContinuationLease {
            root: manifest.root,
            generation: 1,
        })
    }

    pub fn load(conn: &Connection, root: u64) -> Result<ContinuationLoad, &'static str> {
        durable(conn)?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|_| "continuation_read_failed")?;
        let value = load_in_transaction(&tx, root)?;
        tx.commit().map_err(|_| "continuation_read_failed")?;
        Ok(value)
    }

    pub fn claim(
        conn: &mut Connection,
        root: u64,
    ) -> Result<(ContinuationLease, ContinuationLoad), &'static str> {
        durable(conn)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| "continuation_claim_failed")?;
        let loaded = load_in_transaction(&tx, root)?;
        if loaded.cancel_requested {
            return Err("continuation_cancel_requested");
        }
        if loaded.state != "paused" {
            return Err(if loaded.state == "running" {
                "continuation_resume_busy"
            } else {
                "continuation_terminal"
            });
        }
        if !loaded.uncertain.is_empty()
            && !loaded.manifest.steps.iter().any(|s| {
                !loaded.completed.contains_key(&s.id)
                    && !loaded.uncertain.contains(&s.id)
                    && s.depends_on
                        .iter()
                        .all(|d| loaded.completed.contains_key(d) && !loaded.uncertain.contains(d))
            })
        {
            return Err("continuation_uncertain_execution");
        }
        let generation = loaded
            .generation
            .checked_add(1)
            .filter(|g| *g <= MAX_HANDOFF_SEQUENCE)
            .ok_or("continuation_claim_exhausted")?;
        let changed = tx.execute("UPDATE main.cognitive_continuations SET state='running',pause_reason=NULL,generation=?2 WHERE root_task_id=?1 AND state='paused' AND generation=?3",params![root,generation,loaded.generation]).map_err(|_| "continuation_claim_failed")?;
        if changed != 1 {
            return Err("continuation_resume_busy");
        }
        tx.commit().map_err(|_| "continuation_claim_failed")?;
        Ok((ContinuationLease { root, generation }, loaded))
    }

    pub fn mark_started(
        conn: &mut Connection,
        lease: ContinuationLease,
        step: &str,
        unit: ExecutionUnitId,
        pin: &crate::cognition::scheduler::PinnedProviderAllocation,
        budget: TaskBudget,
    ) -> Result<(), &'static str> {
        durable(conn)?;
        if unit.root_task_id() != lease.root {
            return Err("continuation_identity_invalid");
        }
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| "continuation_write_failed")?;
        require_lease(&tx, lease)?;
        let manifest_json: String = tx
            .query_row(
                "SELECT manifest_json FROM main.cognitive_continuations WHERE root_task_id=?1",
                [lease.root],
                |r| r.get(0),
            )
            .map_err(|_| "continuation_read_failed")?;
        let manifest: ContinuationManifest =
            serde_json::from_str(&manifest_json).map_err(|_| "continuation_manifest_invalid")?;
        manifest.validate(lease.root)?;
        validate_budget(budget, &manifest.policy)?;
        validate_pin(
            &encode(pin.variant())?,
            &encode(pin.selection())?,
            &manifest.policy,
        )?;
        let changed = tx.execute("UPDATE main.cognitive_continuation_units SET state='started',unit_sequence=?3,allocation_json=?4,selection_json=?5,budget_json=?6 WHERE root_task_id=?1 AND subtask_id=?2 AND state='not_started'",params![lease.root,step,unit.sequence(),encode(pin.variant())?,encode(pin.selection())?,encode(&budget)?]).map_err(|_| "continuation_write_failed")?;
        if changed != 1 {
            return Err("continuation_unit_not_fresh");
        }
        tx.commit().map_err(|_| "continuation_write_failed")
    }

    /// Receipt, useful output and completed marker are one logical durable fact.
    pub fn commit_result(
        conn: &mut Connection,
        lease: ContinuationLease,
        cp: &CognitiveCheckpoint,
        result: &TaskGraphSubtaskResult,
    ) -> Result<CheckpointRecord, &'static str> {
        if cp.provenance().source
            != ExecutionSource::subtask(&result.subtask_id)
                .map_err(|_| "continuation_identity_invalid")?
        {
            return Err("continuation_identity_invalid");
        }
        validate_result(
            result,
            &result.subtask_id,
            cp.provenance().runtime_id.as_str(),
            cp.policy(),
        )?;
        if cp.id().unit_id().root_task_id() != lease.root {
            return Err("continuation_identity_invalid");
        }
        let json = encode(result)?;
        if json.len() > MAX_RESULT_JSON_BYTES {
            return Err("continuation_result_bounds");
        }
        CheckpointRepository::commit_with(conn, cp, |tx, receipt| {
            let (generation, manifest_json, root_state, cancel_requested): (u64,String,String,bool) = tx.query_row("SELECT generation,manifest_json,state,cancel_requested FROM main.cognitive_continuations WHERE root_task_id=?1", [lease.root], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|_| CheckpointError::Write)?;
            let manifest: ContinuationManifest = serde_json::from_str(&manifest_json).map_err(|_| CheckpointError::InvalidRecord)?;
            manifest.validate(lease.root).map_err(|_| CheckpointError::InvalidRecord)?;
            if cp.policy() != &manifest.policy { return Err(CheckpointError::InvalidPolicy); }
            if generation != lease.generation || cancel_requested || !matches!(root_state.as_str(), "running" | "completed") { return Err(CheckpointError::Conflict); }
            let (state, allocation, prior, sequence, budget_json): (String,String,Option<String>,u64,String) = tx.query_row("SELECT state,allocation_json,result_json,unit_sequence,budget_json FROM main.cognitive_continuation_units WHERE root_task_id=?1 AND subtask_id=?2",params![lease.root,result.subtask_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(|_| CheckpointError::Write)?;
            let budget: TaskBudget = serde_json::from_str(&budget_json).map_err(|_| CheckpointError::InvalidRecord)?;
            validate_budget(budget, &manifest.policy).map_err(|_| CheckpointError::InvalidRecord)?;
            if result.usage.provider_calls > budget.max_provider_calls || budget.max_output_tokens.is_some_and(|limit| result.usage.output_tokens_accounted > limit) { return Err(CheckpointError::InvalidRecord); }
            if sequence != cp.id().unit_id().sequence() || allocation != encode(cp.allocation()).map_err(|_| CheckpointError::InvalidRecord)? { return Err(CheckpointError::Conflict); }
            if state == "completed" {
                if prior.as_deref() != Some(json.as_str()) { return Err(CheckpointError::Conflict); }
            } else if state == "started" && root_state == "running" {
                let changed = tx.execute("UPDATE main.cognitive_continuation_units SET state='completed',result_json=?3,checkpoint_sequence=?4 WHERE root_task_id=?1 AND subtask_id=?2 AND state='started'",params![lease.root,result.subtask_id,json,receipt.checkpoint().id().sequence()]).map_err(|_| CheckpointError::Write)?;
                if changed != 1 { return Err(CheckpointError::Conflict); }
            } else { return Err(CheckpointError::Conflict); }
            Ok(())
        }).map_err(|_| "handoff_checkpoint_write_failed")
    }

    /// Pauses have no terminal history. Terminal states use finish_terminal.
    pub fn finish(
        conn: &Connection,
        lease: ContinuationLease,
        state: &str,
        reason: Option<PauseReason>,
    ) -> Result<bool, &'static str> {
        durable(conn)?;
        if state != "paused" || reason.is_none() {
            return Err("continuation_state_invalid");
        }
        let tx = rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
            .map_err(|_| "continuation_write_failed")?;
        let (stored, generation, requested): (String, u64, bool) = tx
            .query_row(
                "SELECT state,generation,cancel_requested FROM main.cognitive_continuations WHERE root_task_id=?1",
                [lease.root],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|_| "continuation_read_failed")?;
        if generation != lease.generation {
            return Err("continuation_claim_lost");
        }
        if stored == "cancelled" || requested {
            tx.commit().map_err(|_| "continuation_write_failed")?;
            return Ok(true);
        }
        if stored != "running" {
            return Err("continuation_claim_lost");
        }
        tx.execute("UPDATE main.cognitive_continuations SET state=?3,pause_reason=?4 WHERE root_task_id=?1 AND generation=?2 AND state='running'",params![lease.root,lease.generation,state,reason.map(PauseReason::code)]).map_err(|_| "continuation_write_failed")?;
        tx.commit().map_err(|_| "continuation_write_failed")?;
        Ok(false)
    }

    /// Terminal lifecycle and history are one SQLite fact. Cancellation that
    /// committed before this writer acquired its lock takes precedence.
    pub fn finish_terminal(
        conn: &mut Connection,
        lease: ContinuationLease,
        record: TaskRecord,
        subtasks: Vec<SubtaskRecord>,
    ) -> Result<bool, &'static str> {
        durable(conn)?;
        if record.task_id != lease.root
            || record.kind != "task_graph"
            || !matches!(record.state.as_str(), "completed" | "cancelled" | "failed")
        {
            return Err("continuation_state_invalid");
        }
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| "continuation_write_failed")?;
        let cancelled = finish_terminal_in_transaction(&tx, lease, record, subtasks)?;
        tx.commit().map_err(|_| "continuation_write_failed")?;
        Ok(cancelled)
    }

    /// Running cancellation is intent only. A paused task has no owner left to
    /// close its lifecycle, so this transaction also writes its terminal history.
    pub fn cancel(conn: &Connection, root: u64) -> Result<bool, &'static str> {
        durable(conn)?;
        let tx = rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
            .map_err(|_| "continuation_write_failed")?;
        let exists: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM main.cognitive_continuations WHERE root_task_id=?1)",
                [root],
                |r| r.get(0),
            )
            .map_err(|_| "continuation_read_failed")?;
        if !exists {
            return Ok(false);
        }
        let loaded = load_in_transaction(&tx, root)?;
        let accepted = match loaded.state.as_str() {
            "running" => {
                tx.execute("UPDATE main.cognitive_continuations SET cancel_requested=1 WHERE root_task_id=?1 AND state='running'", [root]).map_err(|_| "continuation_write_failed")?;
                true
            }
            "paused" => {
                let timestamp = chrono::Utc::now().to_rfc3339();
                finish_terminal_in_transaction(
                    &tx,
                    ContinuationLease {
                        root,
                        generation: loaded.generation,
                    },
                    TaskRecord {
                        task_id: root,
                        kind: "task_graph".into(),
                        state: "cancelled".into(),
                        started_at: timestamp.clone(),
                        finished_at: timestamp,
                        summary: None,
                        error_code: Some("cancelled".into()),
                    },
                    vec![],
                )?;
                true
            }
            "cancelled" => true,
            "completed" | "failed" => false,
            _ => return Err("continuation_state_invalid"),
        };
        tx.commit().map_err(|_| "continuation_write_failed")?;
        Ok(accepted)
    }

    /// Called once during startup, before providers are exposed. Interrupted
    /// claims become paused, never dispatchable merely because SQLite reopened.
    /// Cancel intent survives this change and forbids claim; explicit cancel
    /// then closes history without executing any remaining work.
    pub fn recover(conn: &mut Connection) -> Result<Vec<(u64, PauseReason)>, &'static str> {
        durable(conn)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| "continuation_write_failed")?;
        let roots = {
            let mut stmt = tx.prepare("SELECT root_task_id FROM main.cognitive_continuations WHERE state IN ('running','paused') ORDER BY root_task_id").map_err(|_| "continuation_read_failed")?;
            let rows = stmt
                .query_map([], |r| r.get::<_, u64>(0))
                .map_err(|_| "continuation_read_failed")?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|_| "continuation_read_failed")?
        };
        let mut recovered = Vec::new();
        for root in roots {
            let reason = match load_in_transaction(&tx, root) {
                Err(_) => PauseReason::InvalidRecovery,
                Ok(loaded) if !loaded.uncertain.is_empty() => PauseReason::UncertainExecution,
                Ok(loaded) => loaded.pause_reason.unwrap_or(PauseReason::RecoveryRequired),
            };
            // Invalid content still retains its indexed root and identity fence.
            tx.execute("UPDATE main.cognitive_continuations SET state='paused',pause_reason=?2 WHERE root_task_id=?1",params![root,reason.code()]).map_err(|_| "continuation_write_failed")?;
            recovered.push((root, reason));
        }
        tx.commit().map_err(|_| "continuation_write_failed")?;
        Ok(recovered)
    }
}
fn finish_terminal_in_transaction(
    tx: &rusqlite::Transaction<'_>,
    lease: ContinuationLease,
    mut record: TaskRecord,
    mut subtasks: Vec<SubtaskRecord>,
) -> Result<bool, &'static str> {
    let loaded = load_in_transaction(tx, lease.root)?;
    if loaded.generation != lease.generation {
        return Err("continuation_claim_lost");
    }
    if matches!(loaded.state.as_str(), "completed" | "cancelled" | "failed") {
        return if loaded.state == record.state || loaded.state == "cancelled" {
            Ok(loaded.state == "cancelled")
        } else {
            Err("continuation_terminal")
        };
    }
    let cancelled = loaded.cancel_requested || record.state == "cancelled";
    if loaded.state != "running" && !(loaded.state == "paused" && cancelled) {
        return Err("continuation_claim_lost");
    }
    if cancelled {
        record.state = "cancelled".into();
        record.error_code = Some("cancelled".into());
        for item in &mut subtasks {
            if item.state != "completed" {
                item.state = "cancelled".into();
                item.error_code = Some("cancelled".into());
            }
        }
    }
    if record.state == "completed"
        && (loaded.completed.len() != loaded.manifest.steps.len() || !loaded.uncertain.is_empty())
    {
        return Err("continuation_state_invalid");
    }
    // Early cancellation/failure can precede construction of the in-memory
    // graph. Its ledger still supplies all IDs and confirmed completions.
    if subtasks.is_empty() && record.state != "completed" {
        subtasks = loaded
            .manifest
            .steps
            .iter()
            .map(|step| {
                let done = loaded
                    .completed
                    .get(&step.id)
                    .filter(|_| cancelled || !loaded.uncertain.contains(&step.id));
                SubtaskRecord {
                    root_task_id: lease.root,
                    subtask_id: step.id.clone(),
                    provider_id: done.map(|u| u.result.provider_id.clone()),
                    state: if done.is_some() {
                        "completed"
                    } else if cancelled {
                        "cancelled"
                    } else {
                        "blocked"
                    }
                    .into(),
                    started_at: None,
                    finished_at: done
                        .map(|u| u.receipt.committed_at().to_owned())
                        .unwrap_or_else(|| record.finished_at.clone()),
                    error_code: done.is_none().then(|| record.error_code.clone()).flatten(),
                }
            })
            .collect();
    }
    let ids: BTreeSet<_> = subtasks.iter().map(|s| s.subtask_id.as_str()).collect();
    if subtasks.len() != loaded.manifest.steps.len()
        || ids.len() != subtasks.len()
        || loaded
            .manifest
            .steps
            .iter()
            .any(|s| !ids.contains(s.id.as_str()))
        || subtasks.iter().any(|s| {
            s.root_task_id != lease.root
                || (record.state == "completed" && s.state != "completed")
                || (s.state == "completed"
                    && ((!cancelled && loaded.uncertain.contains(&s.subtask_id))
                        || loaded.completed.get(&s.subtask_id).is_none_or(|u| {
                            s.provider_id.as_deref() != Some(u.result.provider_id.as_str())
                        })))
                || (loaded.completed.contains_key(&s.subtask_id)
                    && (cancelled || !loaded.uncertain.contains(&s.subtask_id))
                    && s.state != "completed")
        })
    {
        return Err("continuation_history_invalid");
    }
    task_history::insert_with_subtasks_in_transaction(tx, &record, &subtasks)
        .map_err(|_| "task_history_write_failed")?;
    let changed = tx.execute("UPDATE main.cognitive_continuations SET state=?3,pause_reason=NULL,cancel_requested=0 WHERE root_task_id=?1 AND generation=?2 AND state IN ('running','paused')",params![lease.root,lease.generation,record.state])
            .map_err(|_| "continuation_write_failed")?;
    if changed != 1 {
        return Err("continuation_claim_lost");
    }
    Ok(cancelled)
}

fn durable(conn: &Connection) -> Result<(), &'static str> {
    super::checkpoints::require_durable_connection(conn)
        .map_err(|_| "continuation_durability_unavailable")
}
fn encode<T: Serialize>(value: &T) -> Result<String, &'static str> {
    serde_json::to_string(value).map_err(|_| "continuation_record_invalid")
}
fn require_lease(conn: &Connection, lease: ContinuationLease) -> Result<(), &'static str> {
    let (state, generation, requested): (String, u64, bool) = conn.query_row(
        "SELECT state,generation,cancel_requested FROM main.cognitive_continuations WHERE root_task_id=?1",
        [lease.root], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
    ).map_err(|_| "continuation_read_failed")?;
    if generation != lease.generation || state != "running" {
        return Err("continuation_claim_lost");
    }
    if requested {
        return Err("cancelled");
    }
    Ok(())
}
fn validate_result(
    result: &TaskGraphSubtaskResult,
    step: &str,
    provider: &str,
    policy: &TaskPolicySnapshot,
) -> Result<(), &'static str> {
    if result.text.trim().is_empty() || result.text.len() > MAX_RESULT_BYTES {
        return Err("continuation_result_bounds");
    }
    let usage = &result.usage;
    if result.subtask_id != step
        || result.provider_id != provider
        || usage.providers_used != [provider]
        || usage.provider_calls == 0
        || usage.provider_calls > policy.routing().max_provider_calls
        || usage.fallbacks != 0
        || usage.retries >= usage.provider_calls
    {
        return Err("continuation_result_invalid");
    }
    Ok(())
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AllocationWire {
    resource_id: String,
    access_path: String,
    billing_domain_id: String,
    model_id: String,
    effort: Option<String>,
}
fn validate_pin(
    json: &str,
    selection: &str,
    policy: &TaskPolicySnapshot,
) -> Result<AllocationSelection, &'static str> {
    use crate::cognitive_resources::{AccessPath, BillingDomainId, EffortId, ModelId, ResourceId};
    let pin: AllocationWire =
        serde_json::from_str(json).map_err(|_| "continuation_allocation_invalid")?;
    ResourceId::new(&pin.resource_id).map_err(|_| "continuation_allocation_invalid")?;
    AccessPath::new(&pin.access_path).map_err(|_| "continuation_allocation_invalid")?;
    BillingDomainId::new(&pin.billing_domain_id).map_err(|_| "continuation_allocation_invalid")?;
    ModelId::new(&pin.model_id).map_err(|_| "continuation_allocation_invalid")?;
    if let Some(effort) = &pin.effort {
        EffortId::new(effort).map_err(|_| "continuation_allocation_invalid")?;
    }
    let target = policy
        .routing()
        .targets
        .iter()
        .find(|t| t.provider_id == pin.resource_id)
        .ok_or("continuation_allocation_invalid")?;
    if policy.allocation().is_none_or(|a| {
        a.variant_selection_mode == crate::cognitive_resources::VariantSelectionMode::Explicit
    }) && (target.model != pin.model_id
        || target.thinking_level.map(|t| t.as_str()) != pin.effort.as_deref())
    {
        return Err("continuation_allocation_invalid");
    }
    let selection: AllocationSelection =
        serde_json::from_str(selection).map_err(|_| "continuation_selection_invalid")?;
    if selection.mode != policy.routing().routing_mode
        || (selection.mode == crate::cognition::policy::RoutingMode::Auto)
            != selection.score.is_some()
    {
        return Err("continuation_selection_invalid");
    }
    Ok(selection)
}
fn validate_budget(budget: TaskBudget, policy: &TaskPolicySnapshot) -> Result<(), &'static str> {
    if budget.max_provider_calls == 0
        || budget.max_provider_calls > policy.routing().max_provider_calls
        || matches!(
            (policy.routing().max_output_tokens, budget.max_output_tokens),
            (Some(_), None) | (_, Some(0))
        )
        || policy
            .routing()
            .max_output_tokens
            .zip(budget.max_output_tokens)
            .is_some_and(|(limit, grant)| grant > limit)
    {
        return Err("continuation_budget_invalid");
    }
    Ok(())
}
fn load_in_transaction(conn: &Connection, root: u64) -> Result<ContinuationLoad, &'static str> {
    let (json,state,generation,reason,cancel_requested): (String,String,u64,Option<String>,bool) = conn.query_row("SELECT CASE WHEN typeof(manifest_json)='text' AND length(CAST(manifest_json AS BLOB)) BETWEEN 1 AND 49152 THEN manifest_json ELSE NULL END,state,generation,pause_reason,cancel_requested FROM main.cognitive_continuations WHERE root_task_id=?1",[root], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(|_| "continuation_record_invalid")?.ok_or("continuation_absent")?;
    let manifest: ContinuationManifest =
        serde_json::from_str(&json).map_err(|_| "continuation_manifest_invalid")?;
    manifest.validate(root)?;
    if generation == 0
        || generation > MAX_HANDOFF_SEQUENCE
        || !matches!(
            state.as_str(),
            "running" | "paused" | "completed" | "cancelled" | "failed"
        )
        || (state == "paused") != reason.is_some()
        || reason.as_ref().is_some_and(|r| {
            serde_json::from_value::<PauseReason>(serde_json::Value::String(r.clone())).is_err()
        })
    {
        return Err("continuation_state_invalid");
    }
    let history: Option<(String, String)> = conn
        .query_row(
            "SELECT kind,state FROM main.task_records WHERE task_id=?1",
            [root],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|_| "continuation_read_failed")?;
    if (matches!(state.as_str(), "completed" | "cancelled" | "failed")
        && (history.is_none() || cancel_requested))
        || history.as_ref().is_some_and(|(kind, terminal)| {
            kind != "task_graph"
                || terminal != &state
                || matches!(state.as_str(), "running" | "paused")
        })
    {
        return Err("continuation_history_contradiction");
    }
    let mut statement = conn.prepare("SELECT subtask_id,state,unit_sequence,CASE WHEN typeof(allocation_json)='text' AND length(CAST(allocation_json AS BLOB)) BETWEEN 1 AND 4096 THEN allocation_json ELSE NULL END,CASE WHEN typeof(selection_json)='text' AND length(CAST(selection_json AS BLOB)) BETWEEN 1 AND 512 THEN selection_json ELSE NULL END,CASE WHEN typeof(result_json)='text' AND length(CAST(result_json AS BLOB)) BETWEEN 1 AND 102400 THEN result_json ELSE NULL END,checkpoint_sequence,CASE WHEN typeof(budget_json)='text' AND length(CAST(budget_json AS BLOB)) BETWEEN 1 AND 256 THEN budget_json ELSE NULL END FROM main.cognitive_continuation_units WHERE root_task_id=?1 ORDER BY subtask_id").map_err(|_| "continuation_read_failed")?;
    let rows = statement
        .query_map([root], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<u64>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<u64>>(6)?,
                r.get::<_, Option<String>>(7)?,
            ))
        })
        .map_err(|_| "continuation_read_failed")?;
    let mut completed = BTreeMap::new();
    let mut uncertain = BTreeSet::new();
    let mut found = BTreeSet::new();
    let mut max_sequence = 0;
    let mut sequences = BTreeSet::new();
    let mut uncertain_budget = SchedulerUsage::default();
    for row in rows {
        let (
            id,
            unit_state,
            sequence,
            allocation,
            selection,
            result,
            checkpoint_sequence,
            budget_json,
        ) = row.map_err(|_| "continuation_record_invalid")?;
        if !manifest.steps.iter().any(|s| s.id == id) || !found.insert(id.clone()) {
            return Err("continuation_unit_invalid");
        }
        if let Some(sequence) = sequence {
            ExecutionUnitId::new(root, sequence).map_err(|_| "continuation_unit_invalid")?;
            if sequence > manifest.steps.len() as u64 || !sequences.insert(sequence) {
                return Err("continuation_unit_invalid");
            }
            max_sequence = max_sequence.max(sequence);
        }
        let source = ExecutionSource::subtask(&id).map_err(|_| "continuation_unit_invalid")?;
        let budget = budget_json
            .as_deref()
            .map(|json| {
                serde_json::from_str::<TaskBudget>(json).map_err(|_| "continuation_budget_invalid")
            })
            .transpose()?;
        if let Some(budget) = budget {
            validate_budget(budget, &manifest.policy)?;
        }
        match unit_state.as_str() {
            "not_started"
                if budget.is_none()
                    && sequence.is_none()
                    && allocation.is_none()
                    && selection.is_none()
                    && result.is_none()
                    && checkpoint_sequence.is_none() =>
            {
                let contrary: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM main.cognitive_checkpoints WHERE root_task_id=?1 AND source_key=?2 UNION ALL SELECT 1 FROM main.task_subtask_records WHERE root_task_id=?1 AND subtask_id=?2 AND NOT (?3 AND state IN ('cancelled','blocked')))",params![root,id,matches!(state.as_str(), "cancelled" | "failed")], |r| r.get(0)).map_err(|_| "continuation_read_failed")?;
                if contrary {
                    return Err("continuation_history_contradiction");
                }
            }
            "started"
                if budget.is_some()
                    && sequence.is_some()
                    && allocation.is_some()
                    && selection.is_some()
                    && result.is_none()
                    && checkpoint_sequence.is_none() =>
            {
                validate_pin(
                    allocation
                        .as_deref()
                        .ok_or("continuation_allocation_invalid")?,
                    selection
                        .as_deref()
                        .ok_or("continuation_selection_invalid")?,
                    &manifest.policy,
                )?;
                let budget = budget.ok_or("continuation_budget_invalid")?;
                uncertain_budget.provider_calls = uncertain_budget
                    .provider_calls
                    .saturating_add(budget.max_provider_calls);
                uncertain_budget.output_tokens_accounted = uncertain_budget
                    .output_tokens_accounted
                    .saturating_add(budget.max_output_tokens.unwrap_or(0));
                uncertain.insert(id);
            }
            "completed" => {
                let unit = ExecutionUnitId::new(root, sequence.ok_or("continuation_unit_invalid")?)
                    .map_err(|_| "continuation_unit_invalid")?;
                let receipt = CheckpointRepository::committed_in_transaction(conn, unit, &source)
                    .map_err(|_| "continuation_checkpoint_invalid")?;
                let cp = receipt.checkpoint();
                if cp.policy() != &manifest.policy
                    || checkpoint_sequence != Some(cp.id().sequence())
                    || allocation.as_deref() != Some(encode(cp.allocation())?.as_str())
                {
                    return Err("continuation_checkpoint_mismatch");
                }
                let selection = validate_pin(
                    allocation
                        .as_deref()
                        .ok_or("continuation_allocation_invalid")?,
                    selection
                        .as_deref()
                        .ok_or("continuation_selection_invalid")?,
                    &manifest.policy,
                )?;
                let result: TaskGraphSubtaskResult =
                    serde_json::from_str(&result.ok_or("continuation_result_missing")?)
                        .map_err(|_| "continuation_result_invalid")?;
                validate_result(
                    &result,
                    &id,
                    cp.provenance().runtime_id.as_str(),
                    &manifest.policy,
                )?;
                let budget = budget.ok_or("continuation_budget_invalid")?;
                if result.usage.provider_calls > budget.max_provider_calls
                    || budget
                        .max_output_tokens
                        .is_some_and(|limit| result.usage.output_tokens_accounted > limit)
                {
                    return Err("continuation_budget_invalid");
                }
                if cp.effects() == EffectState::UnknownOrInFlight {
                    uncertain_budget.provider_calls = uncertain_budget
                        .provider_calls
                        .saturating_add(result.usage.provider_calls);
                    uncertain_budget.output_tokens_accounted = uncertain_budget
                        .output_tokens_accounted
                        .saturating_add(result.usage.output_tokens_accounted);
                }
                if cp.effects() == EffectState::UnknownOrInFlight {
                    uncertain.insert(id.clone());
                }
                completed.insert(
                    id,
                    RestoredUnit {
                        receipt,
                        result,
                        selection,
                    },
                );
            }
            _ => return Err("continuation_unit_invalid"),
        }
    }
    if found.len() != manifest.steps.len() {
        return Err("continuation_unit_missing");
    }
    for step in &manifest.steps {
        if let Some(unit) = completed.get(&step.id) {
            let mut expected = step
                .depends_on
                .iter()
                .map(|id| {
                    completed
                        .get(id)
                        .map(|u| u.receipt.checkpoint().id())
                        .ok_or("continuation_dependency_missing")
                })
                .collect::<Result<Vec<_>, _>>()?;
            expected.sort();
            expected.dedup();
            if unit.receipt.checkpoint().context().completed_dependencies() != expected {
                return Err("continuation_dependency_mismatch");
            }
        }
    }
    if state == "completed" && (completed.len() != manifest.steps.len() || !uncertain.is_empty()) {
        return Err("continuation_state_invalid");
    }
    if matches!(state.as_str(), "completed" | "cancelled" | "failed") {
        let mut stmt = conn.prepare("SELECT subtask_id,state,provider_id FROM main.task_subtask_records WHERE root_task_id=?1").map_err(|_| "continuation_read_failed")?;
        let rows = stmt
            .query_map([root], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(|_| "continuation_read_failed")?;
        let mut ids = BTreeSet::new();
        for row in rows {
            let (id, unit_state, provider) = row.map_err(|_| "continuation_read_failed")?;
            let done = completed
                .get(&id)
                .filter(|_| state == "cancelled" || !uncertain.contains(&id));
            if !found.contains(&id)
                || !ids.insert(id.clone())
                || done.is_some_and(|u| {
                    unit_state != "completed"
                        || provider.as_deref() != Some(u.result.provider_id.as_str())
                })
                || (done.is_none()
                    && (unit_state == "completed"
                        || (state == "cancelled" && unit_state != "cancelled")))
            {
                return Err("continuation_history_contradiction");
            }
        }
        if ids != found {
            return Err("continuation_history_contradiction");
        }
    }
    Ok(ContinuationLoad {
        manifest,
        state,
        cancel_requested,
        completed,
        uncertain,
        generation,
        max_sequence,
        uncertain_budget,
        pause_reason: reason
            .map(|r| {
                serde_json::from_value(serde_json::Value::String(r))
                    .map_err(|_| "continuation_state_invalid")
            })
            .transpose()?,
    })
}

pub async fn with_connection<T: Send + 'static>(
    db: &Database,
    f: impl FnOnce(&mut Connection) -> Result<T, &'static str> + Send + 'static,
) -> Result<T, &'static str> {
    let db = db.clone();
    crate::runtime::spawn_blocking(move || {
        let mut conn = db.open().map_err(|_| "continuation_storage_unavailable")?;
        f(&mut conn)
    })
    .await
    .map_err(|_| "continuation_storage_unavailable")?
}
