//! C3 owns unit birth and durable boundaries, not provider execution or replay.
use super::{
    policy::RoutingMode,
    scheduler::{PinnedProviderAllocation, Scheduler},
    task_graph::{SubtaskState, TaskGraph},
    types::{ProviderTarget, TaskResult},
};
use crate::{
    cognitive_resources::*,
    persistence::{checkpoints::*, database::Database},
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AllocationChange {
    pub resource: bool,
    pub access_path: bool,
    pub model: bool,
    pub effort: bool,
}
impl AllocationChange {
    fn between(a: &AllocationVariant, b: &AllocationVariant) -> Self {
        Self {
            resource: a.resource_id != b.resource_id,
            access_path: a.access_path != b.access_path,
            model: a.model_id != b.model_id,
            effort: a.effort != b.effort,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AllocationTransition {
    pub predecessor: CheckpointId,
    pub reason: HandoffReason,
    pub change: AllocationChange,
}

#[derive(Clone)]
pub(crate) struct PreparedUnit {
    id: ExecutionUnitId,
    source: ExecutionSource,
    pin: PinnedProviderAllocation,
    dependencies: Vec<CheckpointRecord>,
    reason: HandoffReason,
    transitions: Vec<AllocationTransition>,
    durable: Option<(
        Database,
        crate::persistence::continuations::ContinuationLease,
    )>,
}
impl std::fmt::Debug for PreparedUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedUnit")
            .field("id", &self.id)
            .field("pin", &self.pin)
            .field("reason", &self.reason)
            .finish_non_exhaustive()
    }
}
impl PreparedUnit {
    pub fn attach_durable(
        &mut self,
        db: Database,
        lease: crate::persistence::continuations::ContinuationLease,
    ) {
        self.durable = Some((db, lease));
    }
    pub async fn mark_started(
        &self,
        step: &str,
        budget: super::types::TaskBudget,
    ) -> Result<(), &'static str> {
        if let Some((db, lease)) = &self.durable {
            let lease = *lease;
            let id = self.id;
            let pin = self.pin.clone();
            let step = step.to_owned();
            crate::persistence::continuations::with_connection(db, move |conn| {
                crate::persistence::continuations::ContinuationRepository::mark_started(
                    conn, lease, &step, id, &pin, budget,
                )
            })
            .await?;
        }
        Ok(())
    }
    pub fn id(&self) -> ExecutionUnitId {
        self.id
    }
    pub fn pin(&self) -> &PinnedProviderAllocation {
        &self.pin
    }
    pub fn reason(&self) -> HandoffReason {
        self.reason
    }
    pub fn transitions(&self) -> &[AllocationTransition] {
        &self.transitions
    }
}
struct UnitSlot {
    prepared: PreparedUnit,
    receipt: Option<CheckpointRecord>,
}
pub(crate) struct TaskGraphHandoff {
    root: u64,
    next_sequence: u64,
    policy: TaskPolicySnapshot,
    allocation: Option<AllocationRuntimePolicy>,
    units: BTreeMap<String, UnitSlot>,
    restored: BTreeMap<String, CheckpointRecord>,
}
impl TaskGraphHandoff {
    pub fn new(root: u64, policy: TaskPolicySnapshot) -> Result<Self, &'static str> {
        ExecutionUnitId::new(root, 1).map_err(|_| "handoff_identity_invalid")?;
        let allocation = policy
            .allocation()
            .map(|dto| dto.to_runtime())
            .transpose()?;
        Ok(Self {
            root,
            next_sequence: 1,
            policy,
            allocation,
            units: BTreeMap::new(),
            restored: BTreeMap::new(),
        })
    }
    pub fn advance_past(&mut self, sequence: u64) -> Result<(), &'static str> {
        self.next_sequence = self
            .next_sequence
            .max(sequence.checked_add(1).ok_or("handoff_identity_invalid")?);
        Ok(())
    }
    pub fn restore(
        &mut self,
        records: BTreeMap<String, CheckpointRecord>,
    ) -> Result<(), &'static str> {
        for record in records.values() {
            let cp = record.checkpoint();
            if cp.id().unit_id().root_task_id() != self.root || cp.policy() != &self.policy {
                return Err("handoff_checkpoint_mismatch");
            }
            self.next_sequence = self.next_sequence.max(
                cp.id()
                    .unit_id()
                    .sequence()
                    .checked_add(1)
                    .ok_or("handoff_identity_invalid")?,
            );
        }
        self.restored = records;
        Ok(())
    }
    pub async fn prepare(
        &mut self,
        db: &Database,
        graph: &TaskGraph,
        subtask: &str,
        scheduler: &Scheduler,
        targets: &[ProviderTarget],
        ordinal: usize,
        cancelled: &AtomicBool,
    ) -> Result<PreparedUnit, &'static str> {
        if cancelled.load(Ordering::Acquire) {
            return Err("cancelled");
        }
        if self.units.contains_key(subtask) || self.restored.contains_key(subtask) {
            return Err("handoff_unit_already_allocated");
        }
        if graph.state(subtask) != Some(SubtaskState::Pending)
            || !graph.ready_ids().iter().any(|id| id == subtask)
        {
            return Err("handoff_unit_not_ready");
        }
        if targets.len() != self.policy.routing().targets.len()
            || targets
                .iter()
                .zip(&self.policy.routing().targets)
                .any(|(target, authorized)| {
                    target.provider_id != authorized.provider_id
                        || target.invocation.model != authorized.model
                        || target.invocation.thinking_level != authorized.thinking_level
                })
        {
            return Err("handoff_targets_mismatch");
        }
        let step = graph.step(subtask).ok_or("subtask_unknown")?;
        let id = ExecutionUnitId::new(self.root, self.next_sequence)
            .map_err(|_| "handoff_identity_invalid")?;
        let dependencies = step
            .depends_on
            .iter()
            .map(|key| {
                let record = self
                    .units
                    .get(key)
                    .and_then(|slot| slot.receipt.clone())
                    .or_else(|| self.restored.get(key).cloned())
                    .ok_or("handoff_checkpoint_missing")?;
                if record.checkpoint().provenance().source
                    != ExecutionSource::subtask(key.clone())
                        .map_err(|_| "handoff_identity_invalid")?
                    || record.checkpoint().policy() != &self.policy
                {
                    return Err("handoff_checkpoint_mismatch");
                }
                Ok(record)
            })
            .collect::<Result<Vec<_>, &'static str>>()?;
        // Validate exact receipts against durable storage once per new unit,
        // outside transport and without holding a connection across await.
        let verify = dependencies.clone();
        if !verify.is_empty() {
            let db = db.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let conn = db.open().map_err(|_| "handoff_checkpoint_read_failed")?;
                for expected in verify {
                    let cp = expected.checkpoint();
                    match CheckpointRepository::lookup(
                        &conn,
                        cp.id().unit_id(),
                        &cp.provenance().source,
                    ) {
                        Ok(CheckpointLoadResult::Committed(actual)) if actual == expected => {}
                        _ => return Err("handoff_checkpoint_mismatch"),
                    }
                }
                Ok(())
            })
            .await
            .map_err(|_| "handoff_checkpoint_read_failed")??;
        }
        let reason = validate_boundary(id, &dependencies, cancelled.load(Ordering::Acquire))?;
        // Auto restarts at the current winner each wave; independent ready peers
        // retain D3 distribution within the freshly evaluated authorized chain.
        let pins = scheduler
            .ranked_provider_allocations(
                &self.policy.routing().selection(),
                targets,
                self.allocation.as_ref(),
            )
            .map_err(|error| match error {
                super::types::SchedulerError::NoProvider => "handoff_allocation_unavailable",
                _ => error.code(),
            })?;
        let pin = pins
            .get(ordinal % pins.len())
            .ok_or("provider_config_invalid")?
            .clone();
        if cancelled.load(Ordering::Acquire) {
            return Err("cancelled");
        }
        let transitions = dependencies
            .iter()
            .map(|record| AllocationTransition {
                predecessor: record.checkpoint().id(),
                reason,
                change: AllocationChange::between(record.checkpoint().allocation(), pin.variant()),
            })
            .collect();
        let prepared = PreparedUnit {
            id,
            source: ExecutionSource::subtask(subtask).map_err(|_| "handoff_identity_invalid")?,
            pin,
            dependencies,
            reason,
            transitions,
            durable: None,
        };
        // Claim once, including unsuccessful/cancelled execution. Only Scheduler
        // can retry the same pinned invocation; this layer never recreates it.
        self.units.insert(
            subtask.into(),
            UnitSlot {
                prepared: prepared.clone(),
                receipt: None,
            },
        );
        self.next_sequence += 1;
        Ok(prepared)
    }
    pub fn attach_durable(
        &mut self,
        step: &str,
        db: Database,
        lease: crate::persistence::continuations::ContinuationLease,
    ) -> Result<(), &'static str> {
        self.units
            .get_mut(step)
            .ok_or("handoff_unit_unknown")?
            .prepared
            .attach_durable(db, lease);
        Ok(())
    }
    pub fn auto(&self) -> bool {
        self.policy.routing().routing_mode == RoutingMode::Auto
    }

    pub async fn commit_completed(
        &mut self,
        db: &Database,
        subtask: &str,
        result: &TaskResult,
    ) -> Result<CheckpointRecord, &'static str> {
        let slot = self.units.get(subtask).ok_or("handoff_unit_unknown")?;
        if slot.receipt.is_some() {
            return Err("handoff_unit_already_committed");
        }
        let unit = &slot.prepared;
        let provider = &unit.pin.target().provider_id;
        if &result.provider_id != provider
            || result.usage.providers_used.iter().any(|id| id != provider)
        {
            return Err("handoff_execution_allocation_mismatch");
        }
        let checkpoint = CheckpointId::new(unit.id, 1).map_err(|_| "handoff_identity_invalid")?;
        let context = HandoffContext::new(
            unit.dependencies
                .iter()
                .map(|r| r.checkpoint().id())
                .collect(),
        )
        .map_err(|_| "handoff_context_invalid")?;
        let cp = CognitiveCheckpoint::confirmed(
            checkpoint,
            ExecutionUnitFacts {
                id: unit.id,
                state: ExecutionUnitState::Completed,
                effects: EffectState::NotStarted,
            },
            HandoffBoundary::ConfirmedCompletion { checkpoint },
            None,
            self.policy.clone(),
            unit.pin.variant().clone(),
            CheckpointProvenance {
                source: unit.source.clone(),
                runtime_id: RuntimeId::new(provider).map_err(|_| "handoff_identity_invalid")?,
            },
            context,
        )
        .map_err(|_| "handoff_checkpoint_invalid")?;
        let durable = unit.durable.as_ref().map(|(_, lease)| *lease);
        let useful_result = super::task_graph::TaskGraphSubtaskResult {
            subtask_id: subtask.into(),
            provider_id: result.provider_id.clone(),
            text: result.text.clone(),
            usage: result.usage.clone(),
        };
        let db = db.clone();
        let receipt = tauri::async_runtime::spawn_blocking(move || {
            let mut conn = db.open().map_err(|_| "handoff_checkpoint_write_failed")?;
            if let Some(lease) = durable {
                crate::persistence::continuations::ContinuationRepository::commit_result(
                    &mut conn,
                    lease,
                    &cp,
                    &useful_result,
                )
            } else {
                CheckpointRepository::commit(&mut conn, &cp)
                    .map_err(|_| "handoff_checkpoint_write_failed")
            }
        })
        .await
        .map_err(|_| "handoff_checkpoint_write_failed")??;
        self.units
            .get_mut(subtask)
            .ok_or("handoff_unit_unknown")?
            .receipt = Some(receipt.clone());
        Ok(receipt)
    }
}
fn validate_boundary(
    id: ExecutionUnitId,
    dependencies: &[CheckpointRecord],
    cancelled: bool,
) -> Result<HandoffReason, &'static str> {
    let fresh = ExecutionUnitFacts {
        id,
        state: ExecutionUnitState::NotStarted,
        effects: EffectState::NotStarted,
    };
    let evaluate = |previous: Option<&CheckpointRecord>| {
        let decision = can_handoff(&HandoffRequest {
            requested_unit: fresh,
            previous_unit: previous.map(|r| ExecutionUnitFacts {
                id: r.checkpoint().id().unit_id(),
                state: ExecutionUnitState::Completed,
                effects: r.checkpoint().effects(),
            }),
            boundary: previous.map_or(
                HandoffBoundary::ConfirmedBeforeStart { unit_id: id },
                CheckpointRecord::boundary,
            ),
            cancellation_observed: cancelled,
        });
        if decision.status() == HandoffStatus::Eligible {
            Ok(decision.reason())
        } else if decision.reason() == HandoffReason::CancellationObserved {
            Err("cancelled")
        } else {
            Err("handoff_boundary_unsafe")
        }
    };
    let mut reason = evaluate(dependencies.first())?;
    for record in dependencies.iter().skip(1) {
        reason = evaluate(Some(record))?;
    }
    Ok(reason)
}

#[cfg(test)]
mod tests;
