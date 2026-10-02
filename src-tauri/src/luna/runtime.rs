use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, Weak,
    },
    time::Duration,
};

use crate::cognition::gemini_commands::CurrentRunSessions;
use crate::cognition::policy::{self, CognitiveRole, CognitiveRolePolicy};
use crate::cognition::{
    context::{ContextBuilder, ContextRequest},
    scheduler::SchedulerEvent,
    types::{
        ContextBundle, ProviderCapabilities, ProviderInvocationConfig, ProviderMessage,
        ProviderRole, ProviderSelection, ProviderTarget, ProviderTaskRequest, SchedulerError,
        TaskBudget,
    },
    ProviderRuntime,
};
#[cfg(debug_assertions)]
use crate::cognition::{CognitionRuntime, DiagnosticScenario};
use crate::persistence::conversation;
use crate::persistence::{
    database::Database,
    task_history::{self, TaskRecord},
};
use crate::security::secrets::SecretStore;
use chrono::{SecondsFormat, Utc};
use tauri::ipc::Channel;

use super::task::{TaskEvent, TaskEventKind, TaskId, TaskState, TaskStep};

struct TaskControl {
    state: TaskState,
    cancelled: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct TaskRegistry {
    next_id: AtomicU64,
    active: Mutex<HashMap<TaskId, TaskControl>>,
    foreground_provider_tasks: Mutex<HashMap<i64, usize>>,
    summary_worker: Mutex<Weak<crate::cognition::summary::SummaryWorker>>,
}

impl TaskRegistry {
    pub fn has_foreground_provider_work_for_session(&self, session_id: i64) -> bool {
        self.foreground_provider_tasks
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .contains_key(&session_id)
    }
    pub fn has_foreground_provider_work(&self) -> bool {
        !self
            .foreground_provider_tasks
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .is_empty()
    }
    pub fn attach_summary_worker(&self, worker: &Arc<crate::cognition::summary::SummaryWorker>) {
        *self
            .summary_worker
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = Arc::downgrade(worker);
    }
    #[cfg(test)]
    pub fn mark_foreground_provider_work_for_test(&self, session_id: i64) {
        self.foreground_provider_tasks
            .lock()
            .unwrap()
            .insert(session_id, 1);
    }
    #[cfg(test)]
    pub fn foreground_provider_work_for_test_clear(&self, session_id: i64) {
        self.foreground_provider_tasks
            .lock()
            .unwrap()
            .remove(&session_id);
    }
    #[cfg(test)]
    pub fn foreground_guard_for_test(self: &Arc<Self>, session_id: i64) -> impl Drop {
        let (id, _) = self.register().unwrap();
        *self
            .foreground_provider_tasks
            .lock()
            .unwrap()
            .entry(session_id)
            .or_default() += 1;
        ActiveTask {
            registry: self.clone(),
            id,
            session_id: Some(session_id),
        }
    }
    pub fn seed_next_id(&self, last: u64) {
        self.next_id.store(last, Ordering::Relaxed);
    }
    /// Reserve a monotonic ID for background work without making it cancelable in the conversation UI.
    pub fn reserve_background_id(&self) -> Result<TaskId, String> {
        let last = self
            .next_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |last| {
                (last < 9_007_199_254_740_991).then_some(last + 1)
            })
            .map_err(|_| "task_id_exhausted".to_string())?;
        Ok(TaskId(last + 1))
    }
    pub fn register(&self) -> Result<(TaskId, Arc<AtomicBool>), String> {
        // JavaScript numbers represent integers exactly only through 2^53 - 1.
        let id = self
            .next_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |last| {
                (last < 9_007_199_254_740_991).then_some(last + 1)
            })
            .map_err(|_| "Limite de identificadores de tarefa atingido".to_string())?;
        let id = TaskId(id + 1);
        let cancelled = Arc::new(AtomicBool::new(false));
        self.active
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .insert(
                id,
                TaskControl {
                    state: TaskState::Pending,
                    cancelled: cancelled.clone(),
                },
            );
        Ok((id, cancelled))
    }

    pub fn mark_running(&self, id: TaskId) {
        if let Some(task) = self
            .active
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get_mut(&id)
        {
            task.state = TaskState::Running;
        }
    }

    pub fn cancel(&self, id: TaskId) -> bool {
        let active = self
            .active
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(task) = active.get(&id) {
            task.cancelled.store(true, Ordering::Release);
            true
        } else {
            false
        }
    }

    #[cfg(test)]
    pub fn contains_for_test(&self, id: TaskId) -> bool {
        self.active
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .contains_key(&id)
    }

    fn remove(&self, id: TaskId) {
        self.active
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .remove(&id);
    }

    // Resolve cancellation and remove under the same lock. If cancel_task returns
    // true, the worker will publish TaskCancelled rather than TaskCompleted.
    pub(crate) fn finish(&self, id: TaskId, outcome: TaskState) -> TaskState {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let cancelled = active
            .get(&id)
            .is_some_and(|task| task.cancelled.load(Ordering::Acquire));
        active.remove(&id);
        if cancelled {
            TaskState::Cancelled
        } else {
            outcome
        }
    }

    pub(crate) fn finish_channel_closed(&self, id: TaskId) -> TaskState {
        self.remove(id);
        TaskState::Failed
    }
}

impl ActiveTask {
    pub(crate) fn new(registry: Arc<TaskRegistry>, id: TaskId) -> Self {
        Self {
            registry,
            id,
            session_id: None,
        }
    }
}

// Also removes the registration if the spawned future is dropped unexpectedly.
pub(crate) struct ActiveTask {
    registry: Arc<TaskRegistry>,
    id: TaskId,
    session_id: Option<i64>,
}

impl Drop for ActiveTask {
    fn drop(&mut self) {
        self.registry.remove(self.id);
        if let Some(session_id) = self.session_id {
            let mut in_flight = self
                .registry
                .foreground_provider_tasks
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if let Some(count) = in_flight.get_mut(&session_id) {
                *count -= 1;
                if *count == 0 {
                    in_flight.remove(&session_id);
                }
            }
            let foreground_finished = in_flight.is_empty();
            drop(in_flight);
            if foreground_finished {
                if let Some(worker) = self
                    .registry
                    .summary_worker
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .upgrade()
                {
                    worker.kick();
                }
            }
        }
    }
}

fn emit(
    channel: &Channel<TaskEvent>,
    id: TaskId,
    sequence: &mut u32,
    state: TaskState,
    kind: TaskEventKind,
) -> Result<(), String> {
    *sequence += 1;
    channel
        .send(TaskEvent {
            task_id: id,
            sequence: *sequence,
            state,
            kind,
        })
        .map_err(|error| error.to_string())
}

fn emit_cognitive(
    channel: &Channel<TaskEvent>,
    id: TaskId,
    sequence: &mut u32,
    kind: TaskEventKind,
    interrupted: &AtomicBool,
) -> Result<(), ()> {
    emit(channel, id, sequence, TaskState::Running, kind).map_err(|_| {
        interrupted.store(true, Ordering::Release);
    })
}

fn chat_budget_and_request(
    session_id: i64,
    message: String,
    history: Vec<ProviderMessage>,
    context: ContextBundle,
    policy: &CognitiveRolePolicy,
    timeouts: &HashMap<String, crate::cognition::types::ProviderTimeouts>,
) -> Result<(TaskBudget, ProviderTaskRequest), &'static str> {
    let budget = TaskBudget {
        max_provider_calls: policy.max_provider_calls,
        max_output_tokens: policy.max_output_tokens,
    };
    policy.validate()?;
    let targets = policy.provider_targets(timeouts)?;
    let selection = policy.selection();
    const MAX_ESTIMATED_CONTEXT_BYTES: usize = 1024 * 1024;
    let estimated_context_bytes = history.iter().fold(
        message.len().min(MAX_ESTIMATED_CONTEXT_BYTES),
        |bytes, turn| {
            bytes
                .saturating_add(turn.content.len())
                .min(MAX_ESTIMATED_CONTEXT_BYTES)
        },
    );
    let request = ProviderTaskRequest {
        input: message,
        internal_system_instruction: None,
        history,
        context: Arc::new(context),
        max_output_tokens: policy.max_output_tokens,
        selection,
        targets,
        affinity_key: Some(format!("conversation:{session_id}")),
        estimated_context_bytes,
        required_capabilities: ProviderCapabilities::text_stream(),
    };
    Ok((budget, request))
}

fn preflight_stage<T>(_stage: &'static str, operation: impl FnOnce() -> T) -> T {
    #[cfg(debug_assertions)]
    let started = std::time::Instant::now();
    let result = operation();
    #[cfg(debug_assertions)]
    eprintln!(
        "[Conversation][diag] {_stage}_ms={}",
        started.elapsed().as_millis()
    );
    result
}

pub fn start_conversation(
    registry: Arc<TaskRegistry>,
    db: Database,
    runtime: Arc<ProviderRuntime>,
    store: Arc<SecretStore>,
    sessions: CurrentRunSessions,
    session_id: i64,
    message: String,
    channel: Channel<TaskEvent>,
) -> Result<TaskId, String> {
    // This is the entire synchronous IPC path: structural checks and registration.
    // Session/SQLite/Stronghold validation belongs to the blocking preflight worker.
    if message.trim().is_empty() || message.len() > 4096 {
        return Err("conversation_input_invalid".into());
    }
    if session_id <= 0 {
        return Err("session_invalid".into());
    }
    let (id, cancelled) = registry.register()?;
    *registry
        .foreground_provider_tasks
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .entry(session_id)
        .or_default() += 1;
    let started_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    tauri::async_runtime::spawn(async move {
        let _active = ActiveTask {
            registry: registry.clone(),
            id,
            session_id: Some(session_id),
        };
        let mut sequence = 0;
        registry.mark_running(id);
        let result = async {
            emit_cognitive(
                &channel,
                id,
                &mut sequence,
                TaskEventKind::TaskStarted,
                &cancelled,
            )
            .map_err(|_| "channel_closed")?;
            if cancelled.load(Ordering::Acquire) {
                return Err("cancelled");
            }
            let db_context = db.clone();
            let preflight_cancelled = cancelled.clone();
            let preflight_runtime = runtime.clone();
            #[cfg(debug_assertions)]
            let preflight_started = std::time::Instant::now();
            let preflight = tauri::async_runtime::spawn_blocking(move || {
                #[cfg(test)]
                conversation_preflight_tests::wait_at_gate(&sessions, false);
                let prepared = (|| {
                    if preflight_cancelled.load(Ordering::Acquire) {
                        return Err("cancelled");
                    }
                    let (conn, policy) = preflight_stage("session_policy", || {
                        // Never hold the session registry lock across DB/credential I/O.
                        if !sessions
                            .0
                            .lock()
                            .map_err(|_| "session_registry_failed")?
                            .contains(&session_id)
                        {
                            return Err("session_invalid");
                        }
                        let conn = db_context.open().map_err(|e| e.code())?;
                        if !conversation::is_active_session(&conn, session_id)
                            .map_err(|e| e.code())?
                        {
                            return Err("session_invalid");
                        }
                        let policy = policy::load(&conn, CognitiveRole::Conversation)
                            .map_err(|e| e.code())?;
                        Ok::<_, &'static str>((conn, policy))
                    })?;
                    preflight_stage("credentials", || {
                        crate::cognition::catalog::validate_policy(
                            &policy,
                            &preflight_runtime.scheduler.status(),
                            &store,
                        )
                    })?;
                    let timeouts = preflight_stage("timeouts", || policy.load_timeouts(&conn))
                        .map_err(|e| e.code())?;
                    if preflight_cancelled.load(Ordering::Acquire) {
                        return Err("cancelled");
                    }
                    let (context, history) = preflight_stage("history_context", || {
                        let history = conversation::outbound_history(
                            &conn,
                            session_id,
                            policy.history_max_messages as usize,
                            policy.history_max_bytes as usize,
                        )
                        .map_err(|e| e.code())?;
                        let context = ContextBuilder::build(
                            &conn,
                            ContextRequest {
                                domain: None,
                                kind: None,
                                min_importance: 0,
                                memory_limit: 0,
                                include_recent_conversation: false,
                            },
                        )
                        .map_err(|e| e.code())?;
                        Ok::<_, &'static str>((context, history))
                    })?;
                    Ok::<_, &'static str>((policy, timeouts, context, history))
                })();
                #[cfg(test)]
                conversation_preflight_tests::wait_at_gate(&sessions, true);
                prepared
            })
            .await;
            #[cfg(debug_assertions)]
            eprintln!(
                "[Conversation][diag] preflight_total_ms={}",
                preflight_started.elapsed().as_millis()
            );
            // Cancellation wins even when the blocking worker returned an error.
            if cancelled.load(Ordering::Acquire) {
                return Err("cancelled");
            }
            let (policy, timeouts, context, history) = preflight.map_err(|_| "worker_failed")??;
            emit_cognitive(
                &channel,
                id,
                &mut sequence,
                TaskEventKind::ContextBuilt {
                    memory_count: 0,
                    recent_message_count: 0,
                },
                &cancelled,
            )
            .map_err(|_| "channel_closed")?;
            let history = history
                .into_iter()
                .map(|turn| ProviderMessage {
                    role: match turn.role {
                        conversation::SessionRole::User => ProviderRole::User,
                        conversation::SessionRole::Assistant => ProviderRole::Assistant,
                    },
                    content: turn.content,
                })
                .collect();
            let (budget, request) = chat_budget_and_request(
                session_id,
                message.clone(),
                history,
                context,
                &policy,
                &timeouts,
            )?;
            let result = runtime
                .scheduler
                .run_with_retry(
                    request,
                    budget,
                    policy.retry_policy(),
                    &cancelled,
                    &mut |event| {
                        let kind = match event {
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
                            SchedulerEvent::Chunk { provider_id, text } => {
                                TaskEventKind::ProviderChunk {
                                    provider_id,
                                    chunk: text,
                                }
                            }
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
                        };
                        emit_cognitive(&channel, id, &mut sequence, kind, &cancelled)
                            .map_err(|_| SchedulerError::EventSinkClosed)
                    },
                )
                .await
                .map_err(|e| e.code())?;
            // From this point the provider call is finished and the local exchange is
            // entering its commit phase. A cancel accepted before this boundary wins;
            // a later cancel is rejected instead of leaving a cancelled task with a
            // persisted final assistant answer.
            if registry.finish(id, TaskState::Completed) == TaskState::Cancelled {
                return Err("cancelled");
            }
            let db_write = db.clone();
            let user = message.clone();
            let answer = result.text.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let mut conn = db_write.open().map_err(|e| e.code())?;
                conversation::append_exchange_to_session(&mut conn, session_id, &user, &answer)
                    .map_err(|e| e.code())
            })
            .await
            .map_err(|_| "worker_failed")??;
            emit_cognitive(
                &channel,
                id,
                &mut sequence,
                TaskEventKind::TaskResultReady { result },
                &cancelled,
            )
            .map_err(|_| "channel_closed")?;
            Ok::<(), &'static str>(())
        }
        .await;
        let (outcome, error_code) = match result {
            Ok(()) => (TaskState::Completed, None),
            Err("cancelled") => (TaskState::Cancelled, None),
            Err(code) => (TaskState::Failed, Some(code)),
        };
        let mut state = if error_code == Some("channel_closed") {
            registry.finish_channel_closed(id)
        } else {
            registry.finish(id, outcome)
        };
        let mut error_code = if state == TaskState::Cancelled {
            None
        } else {
            error_code
        };
        let record = TaskRecord {
            task_id: id.0,
            kind: "conversation".into(),
            state: match state {
                TaskState::Completed => "completed",
                TaskState::Cancelled => "cancelled",
                _ => "failed",
            }
            .into(),
            started_at,
            finished_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            summary: Some("Conversa multi-provider LR-7C".into()),
            error_code: error_code.map(str::to_owned),
        };
        let db_record = db.clone();
        let write = tauri::async_runtime::spawn_blocking(move || {
            let conn = db_record.open()?;
            task_history::insert(&conn, &record)
        })
        .await;
        if !matches!(write, Ok(Ok(()))) {
            eprintln!(
                "[Luna Core] task_history code=write_failed task_id={}",
                id.0
            );
            // A cancelled preflight must stay cancelled, including when the DB
            // itself is unavailable. The failed history write is still diagnosed.
            if state != TaskState::Cancelled {
                state = TaskState::Failed;
                error_code = Some("task_history_write_failed");
            }
        }
        let terminal = match state {
            TaskState::Completed => TaskEventKind::TaskCompleted,
            TaskState::Cancelled => TaskEventKind::TaskCancelled,
            _ => TaskEventKind::TaskFailed {
                detail: error_code.unwrap_or("task_failed").into(),
            },
        };
        if emit(&channel, id, &mut sequence, state, terminal).is_err() {
            cancelled.store(true, Ordering::Release);
            error_code = Some("channel_closed");
            let update = tauri::async_runtime::spawn_blocking(move || {
                let conn = db.open()?;
                task_history::mark_failed(&conn, id.0, "channel_closed")
            })
            .await;
            if !matches!(update, Ok(Ok(()))) {
                eprintln!(
                    "[Luna Core] task_history code=channel_update_failed task_id={}",
                    id.0
                );
            }
        }
        if matches!(error_code, Some("provider_auth_failed" | "channel_closed")) {
            crate::security::audit::AuditEvent::new(
                crate::security::audit::Action::SecurityError,
                crate::security::audit::Outcome::Failed,
            )
            .with_detail(error_code.unwrap())
            .with_task_id(id.0)
            .emit();
        }
    });
    Ok(id)
}

async fn wait_or_cancel(cancelled: &AtomicBool, duration: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + duration;
    while !cancelled.load(Ordering::Acquire) && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    cancelled.load(Ordering::Acquire)
}

async fn run_mock_task(
    registry: &TaskRegistry,
    id: TaskId,
    cancelled: &AtomicBool,
    channel: &Channel<TaskEvent>,
    sequence: &mut u32,
) -> Result<TaskState, String> {
    if cancelled.load(Ordering::Acquire) {
        return finish_and_emit(registry, channel, id, sequence, TaskState::Cancelled);
    }
    registry.mark_running(id);
    emit(
        channel,
        id,
        sequence,
        TaskState::Running,
        TaskEventKind::TaskStarted,
    )?;

    for step in [TaskStep::Prepare, TaskStep::Verify] {
        if cancelled.load(Ordering::Acquire) {
            return finish_and_emit(registry, channel, id, sequence, TaskState::Cancelled);
        }
        emit(
            channel,
            id,
            sequence,
            TaskState::Running,
            TaskEventKind::StepStarted { step },
        )?;
        if wait_or_cancel(cancelled, Duration::from_millis(850)).await {
            return finish_and_emit(registry, channel, id, sequence, TaskState::Cancelled);
        }
        emit(
            channel,
            id,
            sequence,
            TaskState::Running,
            TaskEventKind::StepCompleted { step },
        )?;
    }

    finish_and_emit(registry, channel, id, sequence, TaskState::Completed)
}

fn finish_and_emit(
    registry: &TaskRegistry,
    channel: &Channel<TaskEvent>,
    id: TaskId,
    sequence: &mut u32,
    outcome: TaskState,
) -> Result<TaskState, String> {
    let state = registry.finish(id, outcome);
    let kind = if state == TaskState::Cancelled {
        TaskEventKind::TaskCancelled
    } else {
        TaskEventKind::TaskCompleted
    };
    emit(channel, id, sequence, state, kind)?;
    Ok(state)
}

pub fn start(
    registry: Arc<TaskRegistry>,
    db: Database,
    channel: Channel<TaskEvent>,
) -> Result<TaskId, String> {
    let (id, cancelled) = registry.register()?;
    let started_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    tauri::async_runtime::spawn(async move {
        let _active = ActiveTask {
            registry: registry.clone(),
            id,
            session_id: None,
        };
        let mut sequence = 0;
        let outcome = match run_mock_task(&registry, id, &cancelled, &channel, &mut sequence).await
        {
            Ok(state) => state,
            Err(_) => {
                let state = registry.finish(id, TaskState::Failed);
                let kind = if state == TaskState::Cancelled {
                    TaskEventKind::TaskCancelled
                } else {
                    TaskEventKind::TaskFailed {
                        detail: "Falha no canal ou worker".into(),
                    }
                };
                let _ = emit(&channel, id, &mut sequence, state, kind);
                eprintln!(
                    "[Luna Core] tarefa {} falhou; code=channel_or_worker_error",
                    id.0
                );
                state
            }
        };
        let state = match outcome {
            TaskState::Completed => "completed",
            TaskState::Cancelled => "cancelled",
            _ => "failed",
        };
        let record = TaskRecord {
            task_id: id.0,
            kind: "mock".into(),
            state: state.into(),
            started_at,
            finished_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            summary: Some("Tarefa mock LR-2".into()),
            error_code: if state == "failed" {
                Some("channel_or_worker_error".into())
            } else {
                None
            },
        };
        let write = tauri::async_runtime::spawn_blocking(move || {
            let conn = db.open()?;
            task_history::insert(&conn, &record)
        })
        .await;
        if !matches!(write, Ok(Ok(()))) {
            eprintln!(
                "[Luna Core] task_history code=write_failed task_id={}",
                id.0
            );
        }
    });
    Ok(id)
}

#[cfg(debug_assertions)]
pub fn start_cognition(
    registry: Arc<TaskRegistry>,
    db: Database,
    cognition: Arc<CognitionRuntime>,
    scenario: DiagnosticScenario,
    channel: Channel<TaskEvent>,
) -> Result<TaskId, String> {
    let (id, cancelled) = registry.register()?;
    let started_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    tauri::async_runtime::spawn(async move {
        let _active = ActiveTask {
            registry: registry.clone(),
            id,
            session_id: None,
        };
        let mut sequence = 0;
        registry.mark_running(id);
        let started = emit_cognitive(
            &channel,
            id,
            &mut sequence,
            TaskEventKind::TaskStarted,
            &cancelled,
        );
        let db_for_context = db.clone();
        let context = if started.is_ok() {
            Some(
                tauri::async_runtime::spawn_blocking(move || {
                    let conn = db_for_context.open().map_err(|e| e.code())?;
                    ContextBuilder::build(
                        &conn,
                        ContextRequest {
                            domain: None,
                            kind: None,
                            min_importance: 0,
                            memory_limit: 3,
                            include_recent_conversation: true,
                        },
                    )
                    .map_err(|e| e.code())
                })
                .await,
            )
        } else {
            None
        };
        let result = match context {
            None => Err("channel_closed"),
            Some(Ok(Ok(context))) => {
                let context_event = emit_cognitive(
                    &channel,
                    id,
                    &mut sequence,
                    TaskEventKind::ContextBuilt {
                        memory_count: context.metadata.memory_count,
                        recent_message_count: context.metadata.recent_message_count,
                    },
                    &cancelled,
                );
                let budget = if scenario == DiagnosticScenario::BudgetExhausted {
                    TaskBudget {
                        max_provider_calls: 1,
                        max_output_tokens: Some(32),
                    }
                } else {
                    TaskBudget {
                        max_provider_calls: 3,
                        max_output_tokens: Some(32),
                    }
                };
                let request = ProviderTaskRequest {
                    input: "Execute o diagnóstico cognitivo LR-5.".into(),
                    internal_system_instruction: None,
                    history: vec![],
                    context: Arc::new(context),
                    max_output_tokens: budget.max_output_tokens,
                    selection: ProviderSelection::Auto,
                    targets: ["mock-primary", "mock-fallback"]
                        .into_iter()
                        .map(|id| ProviderTarget {
                            provider_id: id.into(),
                            invocation: ProviderInvocationConfig {
                                model: "mock".into(),
                                thinking_level: None,
                                timeouts: None,
                            },
                        })
                        .collect(),
                    affinity_key: None,
                    estimated_context_bytes: 0,
                    required_capabilities: ProviderCapabilities::text_stream(),
                };
                if context_event.is_err() {
                    Err("channel_closed")
                } else {
                    cognition
                        .scheduler(scenario)
                        .run(request, budget, &cancelled, &mut |event| {
                            let kind = match event {
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
                                SchedulerEvent::Chunk { provider_id, text } => {
                                    TaskEventKind::ProviderChunk {
                                        provider_id,
                                        chunk: text,
                                    }
                                }
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
                            };
                            emit_cognitive(&channel, id, &mut sequence, kind, &cancelled)
                                .map_err(|_| SchedulerError::EventSinkClosed)
                        })
                        .await
                        .map_err(|e| e.code())
                }
            }
            Some(Ok(Err(code))) => Err(code),
            Some(Err(_)) => Err("worker_failed"),
        };
        let (mut outcome, mut error_code, result_ready) = match result {
            Ok(result) => (TaskState::Completed, None, Some(result)),
            Err("cancelled") => (TaskState::Cancelled, None, None),
            Err(code) => (TaskState::Failed, Some(code), None),
        };
        if outcome == TaskState::Completed && !cancelled.load(Ordering::Acquire) {
            if let Some(result) = result_ready {
                if emit_cognitive(
                    &channel,
                    id,
                    &mut sequence,
                    TaskEventKind::TaskResultReady { result },
                    &cancelled,
                )
                .is_err()
                {
                    outcome = TaskState::Failed;
                    error_code = Some("channel_closed");
                }
            }
        }
        let mut state = if error_code == Some("channel_closed") {
            registry.finish_channel_closed(id)
        } else {
            registry.finish(id, outcome)
        };
        let terminal = match state {
            TaskState::Completed => TaskEventKind::TaskCompleted,
            TaskState::Cancelled => TaskEventKind::TaskCancelled,
            _ => TaskEventKind::TaskFailed {
                detail: error_code.unwrap_or("task_failed").into(),
            },
        };
        if emit(&channel, id, &mut sequence, state, terminal).is_err() {
            cancelled.store(true, Ordering::Release);
            state = TaskState::Failed;
            error_code = Some("channel_closed");
        }
        if error_code == Some("channel_closed") {
            crate::security::audit::AuditEvent::new(
                crate::security::audit::Action::SecurityError,
                crate::security::audit::Outcome::Failed,
            )
            .with_task_id(id.0)
            .with_detail("channel_closed")
            .emit();
        }
        let record = TaskRecord {
            task_id: id.0,
            kind: "mock_cognition".into(),
            state: match state {
                TaskState::Completed => "completed",
                TaskState::Cancelled => "cancelled",
                _ => "failed",
            }
            .into(),
            started_at,
            finished_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            summary: Some("Diagnóstico cognitivo LR-5".into()),
            error_code: if state == TaskState::Failed {
                Some(error_code.unwrap_or("task_failed").into())
            } else {
                None
            },
        };
        let write = tauri::async_runtime::spawn_blocking(move || {
            let conn = db.open()?;
            task_history::insert(&conn, &record)
        })
        .await;
        if !matches!(write, Ok(Ok(()))) {
            eprintln!(
                "[Luna Core] task_history code=write_failed task_id={}",
                id.0
            );
        }
    });
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_policy_is_snapshot_for_each_request() {
        use crate::cognition::policy::{CognitiveRole, RoutingMode, ThinkingLevel};
        use crate::cognition::types::ContextMetadata;
        let identity: crate::persistence::identity::IdentityInput = serde_json::from_value(serde_json::json!({
      "version":"test","canonicalName":"Luna","presentation":"neutral","primaryLanguage":"pt-BR",
      "concept":"test","traits":{},"behavioralInvariants":[],"modes":{},
      "relationship":{"primaryPersonName":"","relationModes":[],
        "affectionStyle":{"warm":false,"provocative":false,"playfulJealousy":false,"playfulTerritoriality":false,"coercion":false,"isolation":false,"emotionalBlackmail":false},
        "interactionPreferences":{"wantsRealDisagreement":false,"wantsLunaToProposeDirectionsDuringStructuring":false,"prefersLinearFlowDuringImplementation":false}},
      "memoryPolicy":{"retrieval":"none","history":"none","continuity":"none","storePrivateChainOfThought":false},
      "provenance":"test","effectiveFrom":"2026-01-01"
    })).unwrap();
        let context = || ContextBundle {
            identity: identity.clone(),
            relevant_memories: vec![],
            recent_messages: vec![],
            metadata: ContextMetadata {
                identity_version: "test".into(),
                memory_count: 0,
                recent_message_count: 0,
            },
        };
        let mut policy = CognitiveRolePolicy {
            role: CognitiveRole::Conversation,
            routing_mode: RoutingMode::Fixed,
            targets: vec![
                crate::cognition::policy::CognitiveTargetPolicy {
                    provider_id: "gemini".into(),
                    model: "gemini-custom".into(),
                    thinking_level: Some(ThinkingLevel::High),
                },
                crate::cognition::policy::CognitiveTargetPolicy {
                    provider_id: "groq".into(),
                    model: "openai/gpt-oss-20b".into(),
                    thinking_level: Some(ThinkingLevel::Low),
                },
            ],
            max_output_tokens: Some(8192),
            max_provider_calls: 3,
            retry_enabled: true,
            max_retries: 1,
            retry_backoff_ms: 1500,
            history_max_messages: 8,
            history_max_bytes: 12288,
            summary_input_max_bytes: 32768,
            context_max_bytes: 32768,
        };
        let dormant = policy.targets.pop().unwrap();
        let first_timeouts = crate::persistence::gemini_settings::GeminiTimeouts::default();
        let groq_timeouts = crate::cognition::types::ProviderTimeouts {
            request_timeout_ms: 30_000,
            stream_idle_timeout_ms: 12_000,
        };
        let first_configs = HashMap::from([
            ("gemini".into(), first_timeouts.into()),
            ("groq".into(), groq_timeouts),
        ]);
        let (first_budget, first) =
            chat_budget_and_request(42, "Oi".into(), vec![], context(), &policy, &first_configs)
                .unwrap();
        policy.targets[0].model = "gemini-new".into();
        policy.targets[0].thinking_level = None;
        policy.max_output_tokens = None;
        policy.max_provider_calls = 2;
        policy.routing_mode = RoutingMode::Preferred;
        policy.targets.push(dormant);
        let next_timeouts = crate::persistence::gemini_settings::GeminiTimeouts {
            request_timeout_ms: 60_000,
            stream_idle_timeout_ms: 20_000,
        };
        let next_configs = HashMap::from([
            ("gemini".into(), next_timeouts.into()),
            ("groq".into(), groq_timeouts),
        ]);
        let (next_budget, next) =
            chat_budget_and_request(42, "Oi".into(), vec![], context(), &policy, &next_configs)
                .unwrap();
        assert_eq!(first.selection, ProviderSelection::Fixed("gemini".into()));
        assert_eq!(
            (
                first.targets[0].invocation.model.as_str(),
                first.targets[0].invocation.thinking_level,
                first.max_output_tokens,
                first_budget.max_provider_calls
            ),
            ("gemini-custom", Some(ThinkingLevel::High), Some(8192), 3)
        );
        assert_eq!(next.selection, ProviderSelection::Preferred);
        assert_eq!(next.targets.len(), 2);
        assert_eq!(
            (
                next.targets[0].provider_id.as_str(),
                next.targets[0].invocation.model.as_str(),
                next.targets[0].invocation.thinking_level
            ),
            ("gemini", "gemini-new", None)
        );
        assert_eq!(
            (
                next.targets[1].provider_id.as_str(),
                next.targets[1].invocation.model.as_str(),
                next.targets[1].invocation.thinking_level
            ),
            ("groq", "openai/gpt-oss-20b", Some(ThinkingLevel::Low))
        );
        assert_eq!(next.max_output_tokens, None);
        assert_eq!(next_budget.max_provider_calls, 2);
        assert_eq!(
            first.targets[0].invocation.timeouts,
            Some(first_timeouts.into())
        );
        assert_eq!(
            next.targets[0].invocation.timeouts,
            Some(next_timeouts.into())
        );
        assert_eq!(next.targets[1].invocation.timeouts, Some(groq_timeouts));
        policy.targets[0].provider_id = "groq".into();
        policy.targets[0].model = "groq-primary".into();
        policy.targets[0].thinking_level = Some(ThinkingLevel::Medium);
        policy.targets[1].provider_id = "gemini".into();
        policy.targets[1].model = "gemini-fallback".into();
        policy.targets[1].thinking_level = Some(ThinkingLevel::High);
        let (_, reversed) =
            chat_budget_and_request(42, "Oi".into(), vec![], context(), &policy, &next_configs)
                .unwrap();
        assert_eq!(reversed.selection, ProviderSelection::Preferred);
        assert_eq!(reversed.targets[0].invocation.timeouts, Some(groq_timeouts));
        assert_eq!(
            reversed.targets[1].invocation.timeouts,
            Some(next_timeouts.into())
        );
        assert_eq!(
            (
                &reversed.targets[0].invocation.model,
                reversed.targets[0].invocation.thinking_level
            ),
            (&"groq-primary".to_string(), Some(ThinkingLevel::Medium))
        );
        assert_eq!(
            (
                &reversed.targets[1].invocation.model,
                reversed.targets[1].invocation.thinking_level
            ),
            (&"gemini-fallback".to_string(), Some(ThinkingLevel::High))
        );
        use crate::cognition::{
            mock::{MockProvider, MockScenario},
            registry::ProviderRegistry,
            types::ProviderConfig,
        };
        let mut providers = ProviderRegistry::default();
        for (id, priority) in [("gemini", 1), ("groq", 2)] {
            providers
                .register(
                    ProviderConfig {
                        id: id.into(),
                        enabled: true,
                        priority,
                        capabilities: ProviderCapabilities::text_stream(),
                    },
                    Arc::new(MockProvider::new(MockScenario::Normal)),
                )
                .unwrap();
        }
        let scheduler = crate::cognition::scheduler::Scheduler::new(providers);
        let cancelled = AtomicBool::new(false);
        policy.routing_mode = RoutingMode::Fixed;
        let second = policy.targets.pop().unwrap();
        let (budget, request) = chat_budget_and_request(
            42,
            "input".into(),
            vec![],
            context(),
            &policy,
            &next_configs,
        )
        .unwrap();
        assert_eq!(request.affinity_key.as_deref(), Some("conversation:42"));
        assert_eq!(request.estimated_context_bytes, 5);
        tauri::async_runtime::block_on(scheduler.run(request, budget, &cancelled, &mut |_| Ok(())))
            .unwrap();
        policy.routing_mode = RoutingMode::Auto;
        policy.targets.insert(0, second);
        let history = vec![ProviderMessage {
            role: ProviderRole::Assistant,
            content: "á".repeat(2048),
        }];
        for (session, expected) in [(42, "groq"), (43, "gemini")] {
            let (budget, request) = chat_budget_and_request(
                session,
                "input".into(),
                history.clone(),
                context(),
                &policy,
                &next_configs,
            )
            .unwrap();
            assert_eq!(request.estimated_context_bytes, 4101);
            let result = tauri::async_runtime::block_on(scheduler.run(
                request,
                budget,
                &cancelled,
                &mut |_| Ok(()),
            ))
            .unwrap();
            assert_eq!(result.provider_id, expected);
        }
        let (_, request) = chat_budget_and_request(
            42,
            "input".into(),
            vec![ProviderMessage {
                role: ProviderRole::User,
                content: "x".repeat(2 * 1024 * 1024),
            }],
            context(),
            &policy,
            &next_configs,
        )
        .unwrap();
        assert_eq!(request.estimated_context_bytes, 1024 * 1024);
    }

    #[test]
    fn ids_are_monotonic_and_tasks_are_removed() {
        let registry = Arc::new(TaskRegistry::default());
        let (first, _) = registry.register().unwrap();
        let (second, _) = registry.register().unwrap();
        assert_eq!((first.0, second.0), (1, 2));
        registry.mark_running(first);
        assert_eq!(
            registry.active.lock().unwrap().get(&first).unwrap().state,
            TaskState::Running
        );
        drop(ActiveTask {
            registry: registry.clone(),
            id: first,
            session_id: None,
        });
        assert!(!registry.cancel(first));
        assert!(registry.cancel(second));
    }

    #[test]
    fn background_and_foreground_ids_share_one_monotonic_sequence() {
        let registry = TaskRegistry::default();
        registry.seed_next_id(40);
        let (chat, _) = registry.register().unwrap();
        let summary = registry.reserve_background_id().unwrap();
        let (mock, _) = registry.register().unwrap();
        assert_eq!((chat.0, summary.0, mock.0), (41, 42, 43));
        assert!(!registry.cancel(summary));
        assert!(registry.cancel(chat));
        assert!(registry.cancel(mock));
    }

    #[test]
    fn foreground_counter_is_raii_for_parallel_cancel_channel_close_and_panic() {
        let registry = Arc::new(TaskRegistry::default());
        let first = registry.foreground_guard_for_test(1);
        let first_id = TaskId(registry.next_id.load(Ordering::Relaxed));
        let second = registry.foreground_guard_for_test(1);
        let second_id = TaskId(registry.next_id.load(Ordering::Relaxed));
        let third = registry.foreground_guard_for_test(2);
        assert!(registry.has_foreground_provider_work());
        assert!(registry.cancel(first_id));
        drop(first);
        assert!(registry.has_foreground_provider_work_for_session(1));
        assert_eq!(registry.finish_channel_closed(second_id), TaskState::Failed);
        drop(second);
        assert!(!registry.has_foreground_provider_work_for_session(1));
        assert!(registry.has_foreground_provider_work());
        drop(third);
        assert!(!registry.has_foreground_provider_work());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = registry.foreground_guard_for_test(3);
            panic!("synthetic task panic");
        }));
        assert!(result.is_err());
        assert!(!registry.has_foreground_provider_work());
    }

    #[test]
    fn cancellation_is_controlled() {
        let registry = TaskRegistry::default();
        let (id, cancelled) = registry.register().unwrap();
        assert!(!registry.cancel(TaskId(id.0 + 1)));
        assert!(registry.cancel(id));
        assert!(cancelled.load(Ordering::Acquire));
        assert_eq!(
            registry.finish(id, TaskState::Completed),
            TaskState::Cancelled
        );
        assert!(!registry.cancel(id));

        let (completed, _) = registry.register().unwrap();
        assert_eq!(
            registry.finish(completed, TaskState::Completed),
            TaskState::Completed
        );
        assert!(!registry.cancel(completed));
    }

    #[test]
    fn channel_failure_is_failed_and_removes_active_task() {
        let registry = TaskRegistry::default();
        let (id, interrupted) = registry.register().unwrap();
        interrupted.store(true, Ordering::Release);
        assert_eq!(registry.finish_channel_closed(id), TaskState::Failed);
        assert!(!registry.cancel(id));
    }
}

#[cfg(test)]
#[path = "conversation_preflight_tests.rs"]
mod conversation_preflight_tests;
