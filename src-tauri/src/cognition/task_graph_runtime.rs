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
    policy::{self, CognitiveRole},
    scheduler::SchedulerEvent,
    task_graph::{SubtaskState, TaskGraph, TaskGraphResult, TaskGraphSubtaskResult},
    task_graph_worker::{add_usage, run_worker, worker_input},
    types::{ProviderCapabilities, SchedulerError, SchedulerUsage},
    ProviderRuntime,
};
use crate::{
    luna::{
        runtime::{ActiveTask, TaskRegistry},
        task::{TaskEvent, TaskEventKind, TaskId, TaskState},
    },
    persistence::{
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
            SchedulerEvent::Selected {
                provider_id,
                attempt,
                routing_reason,
                score,
            } => TaskEventKind::ProviderSelected {
                provider_id,
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
            SchedulerEvent::Chunk { provider_id, .. } => {
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
        let orchestrator_policy =
            policy::load(&conn, CognitiveRole::Orchestrator).map_err(|error| error.code())?;
        let worker_policy =
            policy::load(&conn, CognitiveRole::Worker).map_err(|error| error.code())?;
        orchestrator_policy.validate()?;
        worker_policy.validate()?;
        catalog::validate_policy(&orchestrator_policy, &statuses, &preflight_store)?;
        catalog::validate_policy(&worker_policy, &statuses, &preflight_store)?;
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
            worker_policy,
            worker_timeouts,
            worker_context,
        ))
    })
    .await;
    let (
        orchestrator_policy,
        orchestrator_timeouts,
        worker_policy,
        worker_timeouts,
        worker_context,
    ) = match preflight {
            Ok(Ok(value)) => value,
            Ok(Err(code)) => return fail(code),
            Err(_) => return fail("worker_failed"),
        };
    if cancelled.load(Ordering::Acquire) {
        return fail("cancelled");
    }

    let mut planner_events = scheduler_events(channel, root, sequence, &cancelled);
    let planner = orchestrator::plan(
        runtime.scheduler.clone(),
        orchestrator_policy,
        objective,
        orchestrator_timeouts,
        &cancelled,
        &mut planner_events,
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
                state: TaskState::Failed,
                error_code: Some(code),
                graph: Some(graph),
                meta,
                result: None,
            };
        }
    };
    let ranked = match runtime.scheduler.ranked_provider_ids(
        &worker_policy.selection(),
        &worker_targets,
        &ProviderCapabilities::text_stream(),
    ) {
        Ok(value) => value,
        Err(error) => {
            graph.block_unfinished();
            return ExecutionOutcome {
                state: TaskState::Failed,
                error_code: Some(error.code()),
                graph: Some(graph),
                meta,
                result: None,
            };
        }
    };
    let worker_context = Arc::new(worker_context);
    let targets: HashMap<_, _> = worker_targets
        .into_iter()
        .map(|target| (target.provider_id.clone(), target))
        .collect();
    let retry = worker_policy.retry_policy();
    let reserved_calls = if retry.enabled {
        retry.max_retries.saturating_add(1)
    } else {
        1
    };
    let mut provider_cursor = 0usize;
    let mut results: HashMap<String, TaskGraphSubtaskResult> = HashMap::new();
    let mut worker_usage = SchedulerUsage::default();

    while !graph.all_completed() {
        if cancelled.load(Ordering::Acquire) {
            graph.cancel_unfinished();
            return ExecutionOutcome {
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
                state: TaskState::Failed,
                error_code: Some(if graph.has_failure() {
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
            let remaining_output = limit.saturating_sub(worker_usage.output_tokens);
            count = count.min(remaining_output as usize);
        }
        if count == 0 {
            graph.block_unfinished();
            return ExecutionOutcome {
                state: TaskState::Failed,
                error_code: Some("task_graph_budget_exceeded"),
                graph: Some(graph),
                meta,
                result: None,
            };
        }

        let output_share = worker_policy.max_output_tokens.map(|limit| {
            limit
                .saturating_sub(worker_usage.output_tokens)
                .checked_div(count as u32)
                .unwrap_or(0)
        });
        if output_share == Some(0) {
            graph.block_unfinished();
            return ExecutionOutcome {
                state: TaskState::Failed,
                error_code: Some("task_graph_budget_exceeded"),
                graph: Some(graph),
                meta,
                result: None,
            };
        }

        let mut specs = Vec::with_capacity(count);
        for (offset, subtask_id) in ready.into_iter().take(count).enumerate() {
            let provider_id = ranked[(provider_cursor + offset) % ranked.len()].clone();
            let target = match targets.get(&provider_id) {
                Some(value) => value.clone(),
                None => {
                    graph.block_unfinished();
                    return ExecutionOutcome {
                        state: TaskState::Failed,
                        error_code: Some("provider_config_invalid"),
                        graph: Some(graph),
                        meta,
                        result: None,
                    };
                }
            };
            let step = graph.step(&subtask_id).expect("validated graph step").clone();
            let input = match worker_input(&step, &results, worker_policy.context_max_bytes as usize)
            {
                Ok(value) => value,
                Err(code) => {
                    graph.block_unfinished();
                    return ExecutionOutcome {
                        state: TaskState::Failed,
                        error_code: Some(code),
                        graph: Some(graph),
                        meta,
                        result: None,
                    };
                }
            };
            if graph.mark_running(&subtask_id).is_err() {
                graph.block_unfinished();
                return ExecutionOutcome {
                    state: TaskState::Failed,
                    error_code: Some("subtask_state_invalid"),
                    graph: Some(graph),
                    meta,
                    result: None,
                };
            }
            let started_at = now();
            if let Some(value) = meta.get_mut(&subtask_id) {
                value.provider_id = Some(provider_id.clone());
                value.started_at = Some(started_at);
            }
            specs.push((subtask_id, provider_id, target, input));
        }
        provider_cursor = (provider_cursor + specs.len()) % ranked.len();

        let mut outcomes = Vec::with_capacity(specs.len());
        if specs.len() == 2 {
            let a = &specs[0];
            let b = &specs[1];
            let fa = run_worker(
                runtime.scheduler.clone(),
                root,
                a.0.clone(),
                a.2.clone(),
                a.3.clone(),
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
        if outcomes
            .iter()
            .any(|(_, _, outcome)| matches!(outcome, Err("channel_closed")))
        {
            cancelled.store(true, Ordering::Release);
            graph.cancel_unfinished();
            return ExecutionOutcome {
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
                state: TaskState::Cancelled,
                error_code: Some("cancelled"),
                graph: Some(graph),
                meta,
                result: None,
            };
        }

        let mut wave_failed = false;
        for (subtask_id, provider_id, outcome) in outcomes {
            let finished_at = now();
            if let Some(value) = meta.get_mut(&subtask_id) {
                value.finished_at = Some(finished_at);
            }
            match outcome {
                Ok(result) => {
                    if graph.mark_completed(&subtask_id).is_err() {
                        graph.block_unfinished();
                        return ExecutionOutcome {
                            state: TaskState::Failed,
                            error_code: Some("subtask_state_invalid"),
                            graph: Some(graph),
                            meta,
                            result: None,
                        };
                    }
                    add_usage(&mut worker_usage, &result.usage);
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
                        },
                    )
                    .is_err()
                    {
                        cancelled.store(true, Ordering::Release);
                        graph.cancel_unfinished();
                        return ExecutionOutcome {
                            state: TaskState::Failed,
                            error_code: Some("channel_closed"),
                            graph: Some(graph),
                            meta,
                            result: None,
                        };
                    }
                }
                Err(code) if code == "cancelled" || cancelled.load(Ordering::Acquire) => {
                    graph.cancel_unfinished();
                    return ExecutionOutcome {
                        state: if code == "channel_closed" {
                            TaskState::Failed
                        } else {
                            TaskState::Cancelled
                        },
                        error_code: Some(code),
                        graph: Some(graph),
                        meta,
                        result: None,
                    };
                }
                Err(code) => {
                    wave_failed = true;
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
        if wave_failed {
            graph.block_unfinished();
            return ExecutionOutcome {
                state: TaskState::Failed,
                error_code: Some("task_graph_subtask_failed"),
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
        .map(|item| format!("[{} · {}]\n{}", item.subtask_id, item.provider_id, item.text))
        .collect::<Vec<_>>()
        .join("\n\n");
    ExecutionOutcome {
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
    if objective.trim().is_empty() || objective.len() > crate::agents::planner::MAX_OBJECTIVE_BYTES {
        return Err("task_graph_request_invalid".into());
    }
    let (id, cancelled) = registry.register()?;
    let started_at = now();
    tauri::async_runtime::spawn(async move {
        let _active = ActiveTask::new(registry.clone(), id);
        let sequence = AtomicU32::new(0);
        registry.mark_running(id);
        if emit(
            &channel,
            id,
            &sequence,
            TaskState::Running,
            TaskEventKind::TaskStarted,
        )
        .is_err()
        {
            cancelled.store(true, Ordering::Release);
        }

        let mut execution = if cancelled.load(Ordering::Acquire) {
            ExecutionOutcome {
                state: TaskState::Failed,
                error_code: Some("channel_closed"),
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

        let finished_at = now();
        let error_code = if state == TaskState::Failed {
            Some(execution.error_code.unwrap_or("task_graph_failed"))
        } else {
            None
        };
        let record = TaskRecord {
            task_id: id.0,
            kind: TASK_KIND.into(),
            state: match state {
                TaskState::Completed => "completed",
                TaskState::Cancelled => "cancelled",
                _ => "failed",
            }
            .into(),
            started_at,
            finished_at: finished_at.clone(),
            summary: Some("LR-7D3 task graph".into()),
            error_code: error_code.map(str::to_owned),
        };
        let subtask_records = execution
            .graph
            .as_ref()
            .map(|graph| build_records(id, graph, &execution.meta, error_code, &finished_at))
            .unwrap_or_default();
        let db_record = db.clone();
        let history = tauri::async_runtime::spawn_blocking(move || {
            let mut conn = db_record.open().map_err(|error| error.code())?;
            task_history::insert_with_subtasks(&mut conn, &record, &subtask_records)
                .map_err(|error| error.code())
        })
        .await;
        if !matches!(history, Ok(Ok(()))) {
            state = TaskState::Failed;
            execution.result = None;
            execution.error_code = Some("task_history_write_failed");
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
                    let db_update = db.clone();
                    let _ = tauri::async_runtime::spawn_blocking(move || {
                        let conn = db_update.open().map_err(|error| error.code())?;
                        task_history::mark_failed(&conn, id.0, "channel_closed")
                            .map_err(|error| error.code())
                    })
                    .await;
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
            let db_update = db.clone();
            let _ = tauri::async_runtime::spawn_blocking(move || {
                let conn = db_update.open().map_err(|error| error.code())?;
                task_history::mark_failed(&conn, id.0, "channel_closed")
                    .map_err(|error| error.code())
            })
            .await;
        }
    });
    Ok(id)
}
