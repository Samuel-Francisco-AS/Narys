use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
};

use chrono::{SecondsFormat, Utc};
use tauri::ipc::Channel;

use super::{
    catalog,
    context::{ContextBuilder, ContextRequest},
    orchestrator::{self, OrchestratorResult},
    policy::CognitiveRole,
    scheduler::SchedulerEvent,
    task_graph::{SubtaskState, TaskGraph, TaskGraphResult, TaskGraphSubtaskResult},
    task_graph_handoff::TaskGraphHandoff,
    task_graph_worker::{
        add_usage, run_worker, worker_input, worker_system_instruction, WorkerTiming,
    },
    types::{SchedulerError, SchedulerUsage, TaskResult},
    ProviderRuntime,
};
use crate::{
    luna::{
        runtime::{ActiveTask, TaskRegistry},
        task::{TaskEvent, TaskEventKind, TaskId, TaskState},
    },
    persistence::{
        checkpoints::TaskPolicySnapshot,
        continuations::{
            self, ContinuationLease, ContinuationLoad, ContinuationManifest,
            ContinuationRepository, PauseReason,
        },
        database::Database,
        task_history::{self, SubtaskRecord, TaskRecord},
    },
    security::secrets::SecretStore,
};

const TASK_KIND: &str = "task_graph";
const MAX_PARALLEL_SUBTASKS: usize = 2;

#[derive(Clone, Debug)]
struct SubtaskMeta {
    provider_id: Option<String>,
    started_at: Option<String>,
    finished_at: Option<String>,
    error_code: Option<String>,
}

struct ExecutionOutcome {
    state: TaskState,
    error_code: Option<&'static str>,
    graph: Option<TaskGraph>,
    meta: HashMap<String, SubtaskMeta>,
    result: Option<TaskGraphResult>,
    lease: Option<ContinuationLease>,
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn emit(
    channel: &Channel<TaskEvent>,
    root: TaskId,
    sequence: &AtomicU32,
    state: TaskState,
    kind: TaskEventKind,
) -> Result<(), &'static str> {
    let sequence = sequence.fetch_add(1, Ordering::AcqRel).saturating_add(1);
    channel
        .send(TaskEvent {
            task_id: root,
            sequence,
            state,
            kind,
        })
        .map_err(|_| "channel_closed")
}

fn scheduler_events<'a>(
    channel: &'a Channel<TaskEvent>,
    root: TaskId,
    sequence: &'a AtomicU32,
    cancelled: &'a AtomicBool,
) -> impl FnMut(SchedulerEvent) -> Result<(), SchedulerError> + Send + 'a {
    move |event| {
        let kind = match event {
            SchedulerEvent::Queued {
                provider_id,
                traffic_class,
                queue_depth,
            } => TaskEventKind::ProviderQueued {
                provider_id,
                traffic_class,
                queue_depth,
            },
            SchedulerEvent::Admitted {
                provider_id,
                traffic_class,
                queue_delay_ms,
            } => TaskEventKind::ProviderAdmitted {
                provider_id,
                traffic_class,
                queue_delay_ms,
            },
            SchedulerEvent::Selected {
                provider_id,
                model,
                attempt,
                routing_reason,
                score,
            } => TaskEventKind::ProviderSelected {
                provider_id,
                model,
                attempt,
                routing_reason: routing_reason.into(),
                score,
            },
            SchedulerEvent::Retry {
                provider_id,
                reason_code,
            } => TaskEventKind::ProviderRetry {
                provider_id,
                reason_code: reason_code.into(),
            },
            SchedulerEvent::Fallback {
                from,
                to,
                reason_code,
            } => TaskEventKind::ProviderFallback {
                from_provider_id: from,
                to_provider_id: to,
                reason_code: reason_code.into(),
            },
            SchedulerEvent::Chunk { provider_id, .. }
            | SchedulerEvent::OutputObserved { provider_id } => {
                TaskEventKind::ProviderOutputObserved { provider_id }
            }
        };
        emit(channel, root, sequence, TaskState::Running, kind).map_err(|_| {
            cancelled.store(true, Ordering::Release);
            SchedulerError::EventSinkClosed
        })
    }
}

fn build_records(
    root: TaskId,
    graph: &TaskGraph,
    meta: &HashMap<String, SubtaskMeta>,
    root_error: Option<&'static str>,
    finished_at: &str,
) -> Vec<SubtaskRecord> {
    graph
        .subtasks()
        .iter()
        .map(|item| {
            let details = meta.get(&item.step.id);
            let state = match item.state {
                SubtaskState::Completed => "completed",
                SubtaskState::Cancelled => "cancelled",
                SubtaskState::Failed => "failed",
                SubtaskState::Blocked | SubtaskState::Pending | SubtaskState::Running => "blocked",
            };
            SubtaskRecord {
                root_task_id: root.0,
                subtask_id: item.step.id.clone(),
                provider_id: details.and_then(|value| value.provider_id.clone()),
                state: state.into(),
                started_at: details.and_then(|value| value.started_at.clone()),
                finished_at: details
                    .and_then(|value| value.finished_at.clone())
                    .unwrap_or_else(|| finished_at.to_owned()),
                error_code: details
                    .and_then(|value| value.error_code.clone())
                    .or_else(|| {
                        (state == "blocked")
                            .then(|| root_error.unwrap_or("dependency_blocked").to_owned())
                    }),
            }
        })
        .collect()
}

async fn execute(
    db: Database,
    runtime: Arc<ProviderRuntime>,
    store: Arc<SecretStore>,
    objective: String,
    root: TaskId,
    cancelled: Arc<AtomicBool>,
    channel: &Channel<TaskEvent>,
    sequence: &AtomicU32,
) -> ExecutionOutcome {
    let fail = |code: &'static str| ExecutionOutcome {
        lease: None,
        state: if code == "cancelled" {
            TaskState::Cancelled
        } else {
            TaskState::Failed
        },
        error_code: Some(code),
        graph: None,
        meta: HashMap::new(),
        result: None,
    };

    let statuses = runtime.scheduler.status();
    let preflight_db = db.clone();
    let preflight_store = store.clone();
    let preflight = tauri::async_runtime::spawn_blocking(move || {
        let conn = preflight_db.open().map_err(|error| error.code())?;
        let mut snapshots = super::allocation_policy::load_role_runtime_policies(
            &conn,
            &[CognitiveRole::Orchestrator, CognitiveRole::Worker],
        )
        .map_err(|error| error.code())?
        .into_iter();
        let orchestrator = snapshots.next().expect("two requested roles");
        let worker = snapshots.next().expect("two requested roles");
        let orchestrator_policy = orchestrator.routing;
        let worker_snapshot =
            TaskPolicySnapshot::new(worker.routing.clone(), worker.allocation_snapshot)
                .map_err(|_| "handoff_policy_invalid")?;
        let worker_policy = worker.routing;
        orchestrator_policy.validate()?;
        worker_policy.validate()?;
        catalog::validate_policies(
            &[&orchestrator_policy, &worker_policy],
            &statuses,
            &preflight_store,
        )?;
        let orchestrator_timeouts = orchestrator_policy
            .load_timeouts(&conn)
            .map_err(|error| error.code())?;
        let worker_timeouts = worker_policy
            .load_timeouts(&conn)
            .map_err(|error| error.code())?;
        let worker_context = ContextBuilder::build(
            &conn,
            ContextRequest {
                domain: None,
                kind: None,
                min_importance: 0,
                memory_limit: 0,
                include_recent_conversation: false,
            },
        )
        .map_err(|error| error.code())?;
        Ok::<_, &'static str>((
            orchestrator_policy,
            orchestrator_timeouts,
            worker_timeouts,
            worker_context,
            orchestrator.allocation,
            worker_snapshot,
        ))
    })
    .await;
    let (
        orchestrator_policy,
        orchestrator_timeouts,
        worker_timeouts,
        worker_context,
        orchestrator_allocation,
        worker_snapshot,
    ) = match preflight {
        Ok(Ok(value)) => value,
        Ok(Err(code)) => return fail(code),
        Err(_) => return fail("worker_failed"),
    };
    if cancelled.load(Ordering::Acquire) {
        return fail("cancelled");
    }

    let mut planner_events = scheduler_events(channel, root, sequence, &cancelled);
    let planner = orchestrator::plan_task_graph(
        runtime.scheduler.clone(),
        orchestrator_policy,
        objective,
        orchestrator_timeouts,
        &cancelled,
        &mut planner_events,
        orchestrator_allocation,
    )
    .await;
    drop(planner_events);
    let OrchestratorResult {
        provider_id: planner_provider_id,
        plan,
        usage: planner_usage,
    } = match planner {
        Ok(value) => value,
        Err(code) => return fail(code),
    };
    if cancelled.load(Ordering::Acquire) {
        return fail("cancelled");
    }

    execute_workers(
        db,
        runtime,
        root,
        cancelled,
        channel,
        sequence,
        worker_snapshot,
        worker_timeouts,
        worker_context,
        OrchestratorResult {
            provider_id: planner_provider_id,
            plan,
            usage: planner_usage,
        },
    )
    .await
}

/// The real Worker graph phase; preflight supplies the captured B4/C2 policy.
/// Its separate boundary also permits deterministic tests without a provider
/// planner, credential vault or network.
#[allow(clippy::too_many_arguments)]
async fn execute_workers(
    db: Database,
    runtime: Arc<ProviderRuntime>,
    root: TaskId,
    cancelled: Arc<AtomicBool>,
    channel: &Channel<TaskEvent>,
    sequence: &AtomicU32,
    worker_snapshot: TaskPolicySnapshot,
    worker_timeouts: HashMap<String, super::types::ProviderTimeouts>,
    worker_context: super::types::ContextBundle,
    planner: OrchestratorResult,
) -> ExecutionOutcome {
    if let Err(code) = TaskGraph::compile(&planner.plan) {
        return early_failure(code);
    }
    if cancelled.load(Ordering::Acquire) {
        return ExecutionOutcome {
            lease: None,
            state: TaskState::Cancelled,
            error_code: Some("cancelled"),
            graph: None,
            meta: HashMap::new(),
            result: None,
        };
    }
    let manifest = ContinuationManifest {
        version: 1,
        root: root.0,
        objective: planner.plan.objective.clone(),
        steps: planner.plan.steps.clone(),
        policy: worker_snapshot.clone(),
        timeouts: worker_timeouts
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect(),
        identity_version: worker_context.metadata.identity_version.clone(),
        planner_provider_id: planner.provider_id.clone(),
        planner_usage: planner.usage.clone(),
    };
    if !worker_context.relevant_memories.is_empty() || !worker_context.recent_messages.is_empty() {
        return early_failure("continuation_context_not_supported");
    }
    let lease = match continuations::with_connection(&db, move |conn| {
        ContinuationRepository::create(conn, &manifest)
    })
    .await
    {
        Ok(lease) => lease,
        Err(code) => return early_failure(code),
    };
    let mut outcome = execute_workers_claimed(
        db.clone(),
        runtime,
        root,
        cancelled.clone(),
        channel,
        sequence,
        worker_snapshot,
        worker_timeouts,
        worker_context,
        planner,
        lease,
        None,
    )
    .await;
    outcome.lease = Some(lease);
    outcome
}

fn early_failure(code: &'static str) -> ExecutionOutcome {
    ExecutionOutcome {
        lease: None,
        state: TaskState::Failed,
        error_code: Some(code),
        graph: None,
        meta: HashMap::new(),
        result: None,
    }
}
fn pause_reason(code: Option<&str>) -> PauseReason {
    match code {
        Some("handoff_economic_authorization_required") => PauseReason::EconomicAuthorization,
        Some("continuation_uncertain_execution" | "handoff_boundary_unsafe") => {
            PauseReason::UncertainExecution
        }
        Some("continuation_context_not_supported" | "continuation_identity_context_changed") => {
            PauseReason::InsufficientDurableContext
        }
        _ => PauseReason::InvalidRecovery,
    }
}
async fn finalize_execution(
    db: &Database,
    root: TaskId,
    cancelled: &AtomicBool,
    outcome: &mut ExecutionOutcome,
    started_at: String,
) -> Option<PauseReason> {
    if cancelled.load(Ordering::Acquire) && outcome.error_code != Some("channel_closed") {
        outcome.state = TaskState::Cancelled;
        outcome.error_code = Some("cancelled");
        outcome.result = None;
        if let Some(graph) = outcome.graph.as_mut() {
            graph.cancel_unfinished();
        }
    }
    let lease = outcome.lease;
    let write = async {
        if outcome.state == TaskState::Paused {
            let reason = pause_reason(outcome.error_code);
            let lease = lease.ok_or("continuation_absent")?;
            let cancelled_durably = continuations::with_connection(db, move |conn| {
                ContinuationRepository::finish(conn, lease, "paused", Some(reason))
            })
            .await?;
            if !cancelled_durably {
                return Ok(false);
            }
            // Durable cancellation won against the requested pause. Its terminal
            // history still goes through the same atomic writer below.
            outcome.state = TaskState::Cancelled;
            outcome.result = None;
            if let Some(graph) = outcome.graph.as_mut() {
                graph.cancel_unfinished();
            }
        }
        let finished_at = now();
        let state = match outcome.state {
            TaskState::Completed => "completed",
            TaskState::Cancelled => "cancelled",
            _ => "failed",
        };
        let error = if state == "failed" {
            Some(outcome.error_code.unwrap_or("task_graph_failed"))
        } else {
            None
        };
        let records = outcome
            .graph
            .as_ref()
            .map(|g| build_records(root, g, &outcome.meta, error, &finished_at))
            .unwrap_or_default();
        let record = TaskRecord {
            task_id: root.0,
            kind: TASK_KIND.into(),
            state: state.into(),
            started_at,
            finished_at,
            summary: Some("LR-7D3 task graph".into()),
            error_code: error.map(str::to_owned),
        };
        continuations::with_connection(db, move |conn| {
            if let Some(lease) = lease {
                ContinuationRepository::finish_terminal(conn, lease, record, records)
            } else {
                // Preflight failed before any continuation was created.
                task_history::insert_with_subtasks(conn, &record, &records)
                    .map_err(|_| "task_history_write_failed")?;
                Ok(state == "cancelled")
            }
        })
        .await
    }
    .await;
    let persistence_failed = write.is_err();
    match write {
        Ok(true) => {
            outcome.state = TaskState::Cancelled;
            outcome.error_code = Some("cancelled");
            outcome.result = None;
            if let Some(graph) = outcome.graph.as_mut() {
                graph.cancel_unfinished();
            }
        }
        Ok(false) => {}
        Err(code) => {
            outcome.result = None;
            outcome.error_code = Some(code);
            // The terminal transaction has rolled back. A separate pause may
            // preserve recovery, but never compensates a committed terminal.
            let pause = match lease {
                Some(lease) => {
                    continuations::with_connection(db, move |conn| {
                        ContinuationRepository::finish(
                            conn,
                            lease,
                            "paused",
                            Some(PauseReason::RecoveryRequired),
                        )
                    })
                    .await
                }
                None => Err("continuation_absent"),
            };
            outcome.state = if matches!(pause, Ok(false)) {
                TaskState::Paused
            } else {
                TaskState::Failed
            };
        }
    }
    (outcome.state == TaskState::Paused).then(|| {
        if persistence_failed {
            PauseReason::RecoveryRequired
        } else {
            pause_reason(outcome.error_code)
        }
    })
}

#[allow(clippy::too_many_arguments)]
async fn execute_workers_claimed(
    db: Database,
    runtime: Arc<ProviderRuntime>,
    root: TaskId,
    cancelled: Arc<AtomicBool>,
    channel: &Channel<TaskEvent>,
    sequence: &AtomicU32,
    worker_snapshot: TaskPolicySnapshot,
    worker_timeouts: HashMap<String, super::types::ProviderTimeouts>,
    worker_context: super::types::ContextBundle,
    planner: OrchestratorResult,
    lease: ContinuationLease,
    restored: Option<ContinuationLoad>,
) -> ExecutionOutcome {
    let fail = |code| ExecutionOutcome {
        lease: None,
        state: if code == "cancelled" {
            TaskState::Cancelled
        } else {
            TaskState::Failed
        },
        error_code: Some(code),
        graph: None,
        meta: HashMap::new(),
        result: None,
    };
    if cancelled.load(Ordering::Acquire) {
        return fail("cancelled");
    }
    let worker_policy = worker_snapshot.routing().clone();
    let mut handoff = match TaskGraphHandoff::new(root.0, worker_snapshot) {
        Ok(value) => value,
        Err(code) => return fail(code),
    };
    let OrchestratorResult {
        provider_id: planner_provider_id,
        plan,
        usage: planner_usage,
    } = planner;
    let mut graph = match TaskGraph::compile(&plan) {
        Ok(value) => value,
        Err(code) => return fail(code),
    };
    let mut meta: HashMap<String, SubtaskMeta> = graph
        .subtasks()
        .iter()
        .map(|item| {
            (
                item.step.id.clone(),
                SubtaskMeta {
                    provider_id: None,
                    started_at: None,
                    finished_at: None,
                    error_code: None,
                },
            )
        })
        .collect();

    if emit(
        channel,
        root,
        sequence,
        TaskState::Running,
        TaskEventKind::TaskPlanned {
            step_count: graph.len(),
        },
    )
    .is_err()
    {
        cancelled.store(true, Ordering::Release);
        graph.cancel_unfinished();
        return ExecutionOutcome {
            lease: None,
            state: TaskState::Failed,
            error_code: Some("channel_closed"),
            graph: Some(graph),
            meta,
            result: None,
        };
    }
    let waiting: Vec<_> = graph
        .subtasks()
        .iter()
        .filter(|item| !item.step.depends_on.is_empty())
        .map(|item| (item.step.id.clone(), item.step.depends_on.clone()))
        .collect();
    for (subtask_id, depends_on) in waiting {
        if emit(
            channel,
            root,
            sequence,
            TaskState::Running,
            TaskEventKind::SubtaskWaiting {
                subtask_id,
                depends_on,
            },
        )
        .is_err()
        {
            cancelled.store(true, Ordering::Release);
            graph.cancel_unfinished();
            return ExecutionOutcome {
                lease: None,
                state: TaskState::Failed,
                error_code: Some("channel_closed"),
                graph: Some(graph),
                meta,
                result: None,
            };
        }
    }

    let worker_targets = match worker_policy.provider_targets(&worker_timeouts) {
        Ok(value) => value,
        Err(code) => {
            graph.block_unfinished();
            return ExecutionOutcome {
                lease: None,
                state: TaskState::Failed,
                error_code: Some(code),
                graph: Some(graph),
                meta,
                result: None,
            };
        }
    };
    let worker_context = Arc::new(worker_context);
    let retry = worker_policy.retry_policy();
    let reserved_calls = if retry.enabled {
        retry.max_retries.saturating_add(1)
    } else {
        1
    };
    let mut provider_cursor = 0usize;
    let mut results: HashMap<String, TaskGraphSubtaskResult> = HashMap::new();
    let mut worker_usage = SchedulerUsage {
        output_tokens_measured: true,
        ..SchedulerUsage::default()
    };

    let mut recovery_uncertain = false;
    if let Some(loaded) = restored {
        recovery_uncertain = !loaded.uncertain.is_empty();
        add_usage(&mut worker_usage, &loaded.uncertain_budget);
        if let Err(code) = handoff.advance_past(loaded.max_sequence) {
            return fail(code);
        }
        provider_cursor = loaded.max_sequence as usize;
        for key in &loaded.uncertain {
            if graph
                .mark_running(key)
                .and_then(|_| graph.mark_failed(key))
                .is_err()
            {
                return fail("continuation_graph_invalid");
            }
        }
        let records = loaded
            .completed
            .iter()
            .filter(|(key, _)| !loaded.uncertain.contains(*key))
            .map(|(key, unit)| (key.clone(), unit.receipt.clone()))
            .collect();
        if let Err(code) = handoff.restore(records) {
            return fail(code);
        }
        for (key, unit) in loaded.completed {
            if loaded.uncertain.contains(&key) {
                continue;
            }
            if graph
                .mark_running(&key)
                .and_then(|_| graph.mark_completed(&key))
                .is_err()
            {
                return fail("continuation_graph_invalid");
            }
            if let Some(item) = meta.get_mut(&key) {
                item.provider_id = Some(unit.result.provider_id.clone());
                item.finished_at = Some(unit.receipt.committed_at().to_owned());
            }
            add_usage(&mut worker_usage, &unit.result.usage);
            results.insert(key, unit.result);
        }
    }

    while !graph.all_completed() {
        if cancelled.load(Ordering::Acquire) {
            graph.cancel_unfinished();
            return ExecutionOutcome {
                lease: None,
                state: TaskState::Cancelled,
                error_code: Some("cancelled"),
                graph: Some(graph),
                meta,
                result: None,
            };
        }
        let ready = graph.ready_ids();
        if ready.is_empty() {
            graph.block_unfinished();
            return ExecutionOutcome {
                lease: None,
                state: if recovery_uncertain {
                    TaskState::Paused
                } else {
                    TaskState::Failed
                },
                error_code: Some(if recovery_uncertain {
                    "continuation_uncertain_execution"
                } else if graph.has_failure() {
                    "task_graph_subtask_failed"
                } else {
                    "task_graph_deadlock"
                }),
                graph: Some(graph),
                meta,
                result: None,
            };
        }

        let remaining_calls = worker_policy
            .max_provider_calls
            .saturating_sub(worker_usage.provider_calls);
        let by_calls = remaining_calls / reserved_calls.max(1);
        let mut count = ready
            .len()
            .min(MAX_PARALLEL_SUBTASKS)
            .min(by_calls as usize);
        if let Some(limit) = worker_policy.max_output_tokens {
            let remaining_output = limit.saturating_sub(worker_usage.output_tokens_accounted);
            count = count.min(remaining_output as usize);
        }
        if count == 0 {
            graph.block_unfinished();
            return ExecutionOutcome {
                lease: None,
                state: if recovery_uncertain {
                    TaskState::Paused
                } else {
                    TaskState::Failed
                },
                error_code: Some(if recovery_uncertain {
                    "continuation_uncertain_execution"
                } else {
                    "task_graph_budget_exceeded"
                }),
                graph: Some(graph),
                meta,
                result: None,
            };
        }

        let output_share = worker_policy.max_output_tokens.map(|limit| {
            limit
                .saturating_sub(worker_usage.output_tokens_accounted)
                .checked_div(count as u32)
                .unwrap_or(0)
        });
        if output_share == Some(0) {
            graph.block_unfinished();
            return ExecutionOutcome {
                lease: None,
                state: if recovery_uncertain {
                    TaskState::Paused
                } else {
                    TaskState::Failed
                },
                error_code: Some(if recovery_uncertain {
                    "continuation_uncertain_execution"
                } else {
                    "task_graph_budget_exceeded"
                }),
                graph: Some(graph),
                meta,
                result: None,
            };
        }

        let mut specs = Vec::with_capacity(count);
        for (offset, subtask_id) in ready.into_iter().take(count).enumerate() {
            let ordinal = if handoff.auto() {
                offset
            } else {
                provider_cursor + offset
            };
            let mut unit = match handoff
                .prepare(
                    &db,
                    &graph,
                    &subtask_id,
                    &runtime.scheduler,
                    &worker_targets,
                    ordinal,
                    &cancelled,
                )
                .await
            {
                Ok(value) => value,
                Err(code) => {
                    if code == "cancelled" {
                        graph.cancel_unfinished();
                    } else {
                        graph.block_unfinished();
                    }
                    return ExecutionOutcome {
                        lease: None,
                        state: if code == "cancelled" {
                            TaskState::Cancelled
                        } else if matches!(
                            code,
                            "handoff_economic_authorization_required" | "handoff_boundary_unsafe"
                        ) {
                            TaskState::Paused
                        } else {
                            TaskState::Failed
                        },
                        error_code: Some(code),
                        graph: Some(graph),
                        meta,
                        result: None,
                    };
                }
            };
            unit.attach_durable(db.clone(), lease);
            if let Err(code) = handoff.attach_durable(&subtask_id, db.clone(), lease) {
                return fail(code);
            }
            let provider_id = unit.pin().target().provider_id.clone();
            let step = graph
                .step(&subtask_id)
                .expect("validated graph step")
                .clone();
            let input =
                match worker_input(&step, &results, worker_policy.context_max_bytes as usize) {
                    Ok(value) => value,
                    Err(code) => {
                        graph.block_unfinished();
                        return ExecutionOutcome {
                            lease: None,
                            state: TaskState::Failed,
                            error_code: Some(code),
                            graph: Some(graph),
                            meta,
                            result: None,
                        };
                    }
                };
            if let Some(value) = meta.get_mut(&subtask_id) {
                value.provider_id = Some(provider_id.clone());
            }
            let internal_instruction = worker_system_instruction(&step);
            specs.push((subtask_id, provider_id, unit, input, internal_instruction));
        }
        provider_cursor += specs.len();

        let mut outcomes: Vec<(
            String,
            String,
            (Option<WorkerTiming>, Result<TaskResult, &'static str>),
        )> = Vec::with_capacity(specs.len());
        if specs.len() == 2 {
            let a = &specs[0];
            let b = &specs[1];
            let fa = run_worker(
                runtime.scheduler.clone(),
                root,
                a.0.clone(),
                a.2.clone(),
                a.3.clone(),
                a.4.clone(),
                output_share,
                reserved_calls,
                retry,
                worker_context.clone(),
                &cancelled,
                channel,
                sequence,
            );
            let fb = run_worker(
                runtime.scheduler.clone(),
                root,
                b.0.clone(),
                b.2.clone(),
                b.3.clone(),
                b.4.clone(),
                output_share,
                reserved_calls,
                retry,
                worker_context.clone(),
                &cancelled,
                channel,
                sequence,
            );
            let (ra, rb) = tokio::join!(fa, fb);
            outcomes.push((a.0.clone(), a.1.clone(), ra));
            outcomes.push((b.0.clone(), b.1.clone(), rb));
        } else {
            let item = &specs[0];
            let result = run_worker(
                runtime.scheduler.clone(),
                root,
                item.0.clone(),
                item.2.clone(),
                item.3.clone(),
                item.4.clone(),
                output_share,
                reserved_calls,
                retry,
                worker_context.clone(),
                &cancelled,
                channel,
                sequence,
            )
            .await;
            outcomes.push((item.0.clone(), item.1.clone(), result));
        }

        // Delivery failure outranks cancellation: the shared cancellation flag is
        // also used to stop sibling workers when the Channel closes.
        let mut wave_failed = false;
        let mut wave_channel_failed = false;
        let mut checkpoint_error = None;
        for (subtask_id, provider_id, (started, outcome)) in outcomes {
            if let Some(timing) = started {
                if graph.mark_running(&subtask_id).is_err() {
                    graph.block_unfinished();
                    return ExecutionOutcome {
                        lease: None,
                        state: TaskState::Failed,
                        error_code: Some("subtask_state_invalid"),
                        graph: Some(graph),
                        meta,
                        result: None,
                    };
                }
                if let Some(value) = meta.get_mut(&subtask_id) {
                    value.started_at = Some(timing.started_at);
                    value.finished_at = Some(timing.finished_at);
                }
            }
            match outcome {
                Ok(result) => {
                    if graph.state(&subtask_id) == Some(SubtaskState::Pending)
                        && graph.mark_running(&subtask_id).is_err()
                    {
                        graph.block_unfinished();
                        return ExecutionOutcome {
                            lease: None,
                            state: TaskState::Failed,
                            error_code: Some("subtask_state_invalid"),
                            graph: Some(graph),
                            meta,
                            result: None,
                        };
                    }
                    add_usage(&mut worker_usage, &result.usage);
                    let receipt = match handoff.commit_completed(&db, &subtask_id, &result).await {
                        Ok(receipt) => receipt,
                        Err(code) => {
                            checkpoint_error = Some(code);
                            let _ = graph.mark_failed(&subtask_id);
                            if let Some(value) = meta.get_mut(&subtask_id) {
                                value.error_code = Some(code.into());
                            }
                            if emit(
                                channel,
                                root,
                                sequence,
                                TaskState::Running,
                                TaskEventKind::SubtaskFailed {
                                    subtask_id,
                                    provider_id: Some(provider_id),
                                    error_code: code.into(),
                                },
                            )
                            .is_err()
                            {
                                wave_channel_failed = true;
                                cancelled.store(true, Ordering::Release);
                            }
                            continue;
                        }
                    };
                    if graph.mark_completed(&subtask_id).is_err() {
                        graph.block_unfinished();
                        return ExecutionOutcome {
                            lease: None,
                            state: TaskState::Failed,
                            error_code: Some("subtask_state_invalid"),
                            graph: Some(graph),
                            meta,
                            result: None,
                        };
                    }
                    let subtask_result = TaskGraphSubtaskResult {
                        subtask_id: subtask_id.clone(),
                        provider_id: provider_id.clone(),
                        text: result.text,
                        usage: result.usage,
                    };
                    results.insert(subtask_id.clone(), subtask_result);
                    if emit(
                        channel,
                        root,
                        sequence,
                        TaskState::Running,
                        TaskEventKind::SubtaskCompleted {
                            subtask_id,
                            provider_id,
                            checkpoint_id: receipt.checkpoint().id(),
                        },
                    )
                    .is_err()
                    {
                        cancelled.store(true, Ordering::Release);
                        graph.cancel_unfinished();
                        return ExecutionOutcome {
                            lease: None,
                            state: TaskState::Failed,
                            error_code: Some("channel_closed"),
                            graph: Some(graph),
                            meta,
                            result: None,
                        };
                    }
                }
                Err("channel_closed") => {
                    wave_channel_failed = true;
                    cancelled.store(true, Ordering::Release);
                }
                Err("cancelled") => {}
                Err(code) => {
                    if cancelled.load(Ordering::Acquire) {
                        continue;
                    }
                    wave_failed = true;
                    if graph.state(&subtask_id) == Some(SubtaskState::Pending) {
                        let _ = graph.mark_running(&subtask_id);
                    }
                    let _ = graph.mark_failed(&subtask_id);
                    if let Some(value) = meta.get_mut(&subtask_id) {
                        value.error_code = Some(code.into());
                    }
                    if emit(
                        channel,
                        root,
                        sequence,
                        TaskState::Running,
                        TaskEventKind::SubtaskFailed {
                            subtask_id,
                            provider_id: Some(provider_id),
                            error_code: code.into(),
                        },
                    )
                    .is_err()
                    {
                        cancelled.store(true, Ordering::Release);
                        graph.cancel_unfinished();
                        return ExecutionOutcome {
                            lease: None,
                            state: TaskState::Failed,
                            error_code: Some("channel_closed"),
                            graph: Some(graph),
                            meta,
                            result: None,
                        };
                    }
                }
            }
        }
        if wave_channel_failed {
            graph.cancel_unfinished();
            return ExecutionOutcome {
                lease: None,
                state: TaskState::Failed,
                error_code: Some("channel_closed"),
                graph: Some(graph),
                meta,
                result: None,
            };
        }
        if cancelled.load(Ordering::Acquire) {
            graph.cancel_unfinished();
            return ExecutionOutcome {
                lease: None,
                state: TaskState::Cancelled,
                error_code: Some("cancelled"),
                graph: Some(graph),
                meta,
                result: None,
            };
        }
        if wave_failed || checkpoint_error.is_some() {
            graph.block_unfinished();
            return ExecutionOutcome {
                lease: None,
                state: TaskState::Failed,
                error_code: Some(checkpoint_error.unwrap_or("task_graph_subtask_failed")),
                graph: Some(graph),
                meta,
                result: None,
            };
        }
    }

    let mut ordered = Vec::with_capacity(graph.len());
    for item in graph.subtasks() {
        if let Some(result) = results.remove(&item.step.id) {
            ordered.push(result);
        }
    }
    let consolidated_text = ordered
        .iter()
        .map(|item| {
            format!(
                "[{} · {}]\n{}",
                item.subtask_id, item.provider_id, item.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    ExecutionOutcome {
        lease: None,
        state: TaskState::Completed,
        error_code: None,
        graph: Some(graph),
        meta,
        result: Some(TaskGraphResult {
            planner_provider_id,
            planner_usage,
            plan,
            subtasks: ordered,
            consolidated_text,
            worker_usage,
        }),
    }
}

pub fn start_task(
    registry: Arc<TaskRegistry>,
    db: Database,
    runtime: Arc<ProviderRuntime>,
    store: Arc<SecretStore>,
    objective: String,
    channel: Channel<TaskEvent>,
) -> Result<TaskId, String> {
    if objective.trim().is_empty() || objective.len() > crate::agents::planner::MAX_OBJECTIVE_BYTES
    {
        return Err("task_graph_request_invalid".into());
    }
    let (id, cancelled) = registry.register()?;
    let started_at = now();
    tauri::async_runtime::spawn(async move {
        let _active = ActiveTask::new(registry.clone(), id);
        let sequence = AtomicU32::new(0);
        registry.mark_running(id);
        let start_delivery_failed = emit(
            &channel,
            id,
            &sequence,
            TaskState::Running,
            TaskEventKind::TaskStarted,
        )
        .is_err();
        if start_delivery_failed {
            cancelled.store(true, Ordering::Release);
        }

        let mut execution = if start_delivery_failed {
            ExecutionOutcome {
                lease: None,
                state: TaskState::Failed,
                error_code: Some("channel_closed"),
                graph: None,
                meta: HashMap::new(),
                result: None,
            }
        } else if cancelled.load(Ordering::Acquire) {
            ExecutionOutcome {
                lease: None,
                state: TaskState::Cancelled,
                error_code: Some("cancelled"),
                graph: None,
                meta: HashMap::new(),
                result: None,
            }
        } else {
            execute(
                db.clone(),
                runtime,
                store,
                objective,
                id,
                cancelled.clone(),
                &channel,
                &sequence,
            )
            .await
        };

        let requested = execution.state;
        let mut state = if execution.error_code == Some("channel_closed") {
            registry.finish_channel_closed(id)
        } else {
            registry.finish(id, requested)
        };
        if state == TaskState::Cancelled {
            execution.error_code = Some("cancelled");
            execution.result = None;
            if let Some(graph) = execution.graph.as_mut() {
                graph.cancel_unfinished();
            }
        } else if state == TaskState::Failed {
            execution.result = None;
            if let Some(graph) = execution.graph.as_mut() {
                graph.block_unfinished();
            }
        }

        execution.state = state;
        let paused_reason =
            finalize_execution(&db, id, &cancelled, &mut execution, started_at).await;
        state = execution.state;
        if state == TaskState::Paused {
            let _ = emit(
                &channel,
                id,
                &sequence,
                state,
                TaskEventKind::TaskPaused {
                    reason: paused_reason.unwrap_or_else(|| pause_reason(execution.error_code)),
                },
            );
            return;
        }

        if state == TaskState::Completed {
            if let Some(result) = execution.result.take() {
                if emit(
                    &channel,
                    id,
                    &sequence,
                    TaskState::Running,
                    TaskEventKind::TaskGraphResultReady { result },
                )
                .is_err()
                {
                    cancelled.store(true, Ordering::Release);
                    state = TaskState::Failed;
                    execution.error_code = Some("channel_closed");
                }
            }
        }

        let terminal = match state {
            TaskState::Completed => TaskEventKind::TaskCompleted,
            TaskState::Cancelled => TaskEventKind::TaskCancelled,
            _ => TaskEventKind::TaskFailed {
                detail: execution.error_code.unwrap_or("task_graph_failed").into(),
            },
        };
        if emit(&channel, id, &sequence, state, terminal).is_err() {
            cancelled.store(true, Ordering::Release);
        }
    });
    Ok(id)
}

/// Explicit Core entry point. Startup never invokes this path. A SQLite claim,
/// not the registry alone, prevents concurrent resumes of the same root.
#[allow(dead_code)]
pub(crate) async fn resume_task_graph(
    registry: Arc<TaskRegistry>,
    db: Database,
    runtime: Arc<ProviderRuntime>,
    root: TaskId,
    channel: Channel<TaskEvent>,
) -> Result<TaskState, &'static str> {
    let resumed_at = now();
    let (lease, loaded) = continuations::with_connection(&db, move |conn| {
        ContinuationRepository::claim(conn, root.0)
    })
    .await?;
    let cancelled = match registry.register_existing(root) {
        Ok(flag) => flag,
        Err(_) => {
            let reason = loaded.pause_reason.unwrap_or(PauseReason::RecoveryRequired);
            continuations::with_connection(&db, move |conn| {
                ContinuationRepository::finish(conn, lease, "paused", Some(reason))
            })
            .await?;
            return Err("continuation_resume_busy");
        }
    };
    let _active = ActiveTask::new(registry.clone(), root);
    registry.mark_running(root);
    let sequence = AtomicU32::new(0);
    let expected_version = loaded.manifest.identity_version.clone();
    let context = continuations::with_connection(&db, move |conn| {
        let context = ContextBuilder::build(
            conn,
            ContextRequest {
                domain: None,
                kind: None,
                min_importance: 0,
                memory_limit: 0,
                include_recent_conversation: false,
            },
        )
        .map_err(|_| "continuation_identity_context_changed")?;
        if context.metadata.identity_version != expected_version {
            return Err("continuation_identity_context_changed");
        }
        context
            .identity
            .validate()
            .map_err(|_| "continuation_identity_context_changed")?;
        Ok(context)
    })
    .await;
    let mut execution = match context {
        Err(code) => ExecutionOutcome {
            lease: None,
            state: TaskState::Paused,
            error_code: Some(code),
            graph: None,
            meta: HashMap::new(),
            result: None,
        },
        Ok(context) => {
            let manifest = &loaded.manifest;
            let planner = OrchestratorResult {
                provider_id: manifest.planner_provider_id.clone(),
                plan: manifest.plan(),
                usage: manifest.planner_usage.clone(),
            };
            execute_workers_claimed(
                db.clone(),
                runtime,
                root,
                cancelled.clone(),
                &channel,
                &sequence,
                manifest.policy.clone(),
                manifest
                    .timeouts
                    .iter()
                    .map(|(k, v)| (k.clone(), *v))
                    .collect(),
                context,
                planner,
                lease,
                Some(loaded),
            )
            .await
        }
    };
    let state = registry.finish(root, execution.state);
    execution.state = state;
    execution.lease = Some(lease);
    let paused_reason = finalize_execution(&db, root, &cancelled, &mut execution, resumed_at).await;
    let state = execution.state;
    if state == TaskState::Completed {
        if let Some(result) = execution.result {
            emit(
                &channel,
                root,
                &sequence,
                TaskState::Running,
                TaskEventKind::TaskGraphResultReady { result },
            )?;
        }
    }
    let event = match state {
        TaskState::Completed => TaskEventKind::TaskCompleted,
        TaskState::Cancelled => TaskEventKind::TaskCancelled,
        TaskState::Paused => TaskEventKind::TaskPaused {
            reason: paused_reason.unwrap_or_else(|| pause_reason(execution.error_code)),
        },
        _ => TaskEventKind::TaskFailed {
            detail: execution.error_code.unwrap_or("continuation_failed").into(),
        },
    };
    emit(&channel, root, &sequence, state, event)?;
    Ok(state)
}

/// Read-only target projection retained for B regression fixtures. C3 uses the
/// same engine through ranked_provider_allocations to retain complete identity.
#[cfg(test)]
pub(crate) fn rank_worker_targets(
    scheduler: &super::scheduler::Scheduler,
    policy: &super::policy::CognitiveRolePolicy,
    targets: &[super::types::ProviderTarget],
    allocation: Option<&crate::cognitive_resources::AllocationRuntimePolicy>,
) -> Result<Vec<super::types::ProviderTarget>, super::types::SchedulerError> {
    scheduler.ranked_provider_targets(
        &policy.selection(),
        targets,
        &super::types::ProviderCapabilities::text_stream(),
        &super::types::InvocationMode::default(),
        allocation,
    )
}

#[cfg(test)]
#[path = "task_graph_runtime/c3_tests.rs"]
mod c3_tests;

#[cfg(test)]
#[path = "task_graph_runtime/c4_tests.rs"]
mod c4_tests;
