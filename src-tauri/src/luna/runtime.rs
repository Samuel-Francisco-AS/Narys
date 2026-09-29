use std::{
  collections::HashMap,
  sync::{atomic::{AtomicBool, AtomicU64, Ordering}, Arc, Mutex, Weak},
  time::Duration,
};

use tauri::ipc::Channel;
use chrono::{SecondsFormat, Utc};
use crate::persistence::{database::Database, task_history::{self, TaskRecord}};
use crate::persistence::conversation;
use crate::cognition::{ProviderRuntime, context::{ContextBuilder, ContextRequest}, scheduler::SchedulerEvent,
  types::{ContextBundle, ProviderCapabilities, ProviderMessage, ProviderTaskRequest, ProviderTarget, ProviderInvocationConfig, ProviderRole, TaskBudget, SchedulerError, ProviderSelection}};
use crate::cognition::policy::{CognitiveRolePolicy, RoutingMode};
#[cfg(debug_assertions)]
use crate::cognition::{CognitionRuntime, DiagnosticScenario};

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
    self.foreground_provider_tasks.lock().unwrap_or_else(|poison| poison.into_inner()).contains_key(&session_id)
  }
  pub fn has_foreground_provider_work(&self) -> bool {
    !self.foreground_provider_tasks.lock().unwrap_or_else(|poison| poison.into_inner()).is_empty()
  }
  pub fn attach_summary_worker(&self, worker: &Arc<crate::cognition::summary::SummaryWorker>) {
    *self.summary_worker.lock().unwrap_or_else(|poison| poison.into_inner()) = Arc::downgrade(worker);
  }
  #[cfg(test)]
  pub fn mark_foreground_provider_work_for_test(&self, session_id: i64) {
    self.foreground_provider_tasks.lock().unwrap().insert(session_id, 1);
  }
  #[cfg(test)]
  pub fn foreground_provider_work_for_test_clear(&self, session_id: i64) {
    self.foreground_provider_tasks.lock().unwrap().remove(&session_id);
  }
  #[cfg(test)]
  pub fn foreground_guard_for_test(self: &Arc<Self>, session_id: i64) -> impl Drop {
    let (id, _) = self.register().unwrap();
    *self.foreground_provider_tasks.lock().unwrap().entry(session_id).or_default() += 1;
    ActiveTask { registry: self.clone(), id, session_id: Some(session_id) }
  }
  pub fn seed_next_id(&self, last: u64) { self.next_id.store(last, Ordering::Relaxed); }
  /// Reserve a monotonic ID for background work without making it cancelable in the conversation UI.
  pub fn reserve_background_id(&self) -> Result<TaskId, String> {
    let last = self.next_id.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |last|
      (last < 9_007_199_254_740_991).then_some(last + 1)).map_err(|_| "task_id_exhausted".to_string())?;
    Ok(TaskId(last + 1))
  }
  pub fn register(&self) -> Result<(TaskId, Arc<AtomicBool>), String> {
    // JavaScript numbers represent integers exactly only through 2^53 - 1.
    let id = self.next_id.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |last| {
      (last < 9_007_199_254_740_991).then_some(last + 1)
    }).map_err(|_| "Limite de identificadores de tarefa atingido".to_string())?;
    let id = TaskId(id + 1);
    let cancelled = Arc::new(AtomicBool::new(false));
    self.active.lock().unwrap_or_else(|poison| poison.into_inner()).insert(
      id,
      TaskControl { state: TaskState::Pending, cancelled: cancelled.clone() },
    );
    Ok((id, cancelled))
  }

  pub fn mark_running(&self, id: TaskId) {
    if let Some(task) = self.active.lock().unwrap_or_else(|poison| poison.into_inner()).get_mut(&id) {
      task.state = TaskState::Running;
    }
  }

  pub fn cancel(&self, id: TaskId) -> bool {
    let active = self.active.lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(task) = active.get(&id) {
      task.cancelled.store(true, Ordering::Release);
      true
    } else {
      false
    }
  }

  fn remove(&self, id: TaskId) {
    self.active.lock().unwrap_or_else(|poison| poison.into_inner()).remove(&id);
  }

  // Resolve cancellation and remove under the same lock. If cancel_task returns
  // true, the worker will publish TaskCancelled rather than TaskCompleted.
  fn finish(&self, id: TaskId, outcome: TaskState) -> TaskState {
    let mut active = self.active.lock().unwrap_or_else(|poison| poison.into_inner());
    let cancelled = active.get(&id).is_some_and(|task| task.cancelled.load(Ordering::Acquire));
    active.remove(&id);
    if cancelled { TaskState::Cancelled } else { outcome }
  }

  fn finish_channel_closed(&self, id: TaskId) -> TaskState {
    self.remove(id);
    TaskState::Failed
  }
}

// Also removes the registration if the spawned future is dropped unexpectedly.
struct ActiveTask {
  registry: Arc<TaskRegistry>,
  id: TaskId,
  session_id: Option<i64>,
}

impl Drop for ActiveTask {
  fn drop(&mut self) {
    self.registry.remove(self.id);
    if let Some(session_id) = self.session_id {
      let mut in_flight = self.registry.foreground_provider_tasks.lock().unwrap_or_else(|poison| poison.into_inner());
      if let Some(count) = in_flight.get_mut(&session_id) {
        *count -= 1;
        if *count == 0 { in_flight.remove(&session_id); }
      }
      let foreground_finished = in_flight.is_empty();
      drop(in_flight);
      if foreground_finished {
        if let Some(worker) = self.registry.summary_worker.lock().unwrap_or_else(|poison| poison.into_inner()).upgrade() {
          worker.kick();
        }
      }
    }
  }
}

fn emit(channel: &Channel<TaskEvent>, id: TaskId, sequence: &mut u32, state: TaskState, kind: TaskEventKind) -> Result<(), String> {
  *sequence += 1;
  channel.send(TaskEvent { task_id: id, sequence: *sequence, state, kind }).map_err(|error| error.to_string())
}

fn emit_cognitive(channel: &Channel<TaskEvent>, id: TaskId, sequence: &mut u32,
  kind: TaskEventKind, interrupted: &AtomicBool) -> Result<(), ()> {
  emit(channel, id, sequence, TaskState::Running, kind).map_err(|_| {
    interrupted.store(true, Ordering::Release);
  })
}

fn chat_budget_and_request(message: String, history: Vec<ProviderMessage>, context: ContextBundle,
  policy: &CognitiveRolePolicy, gemini_timeouts: crate::cognition::types::ProviderTimeouts,
  groq_timeouts: crate::cognition::types::ProviderTimeouts) -> (TaskBudget, ProviderTaskRequest) {
  let budget = TaskBudget { max_provider_calls: policy.max_provider_calls, max_output_tokens: policy.max_output_tokens };
  let mut targets = vec![ProviderTarget { provider_id: policy.provider_id.clone(), invocation: ProviderInvocationConfig {
    model: policy.model.clone(), thinking_level: policy.thinking_level, timeouts: Some(gemini_timeouts),
  } }];
  let selection = match policy.routing_mode {
    RoutingMode::Fixed => ProviderSelection::Fixed(policy.provider_id.clone()),
    RoutingMode::Preferred => {
      if let (Some(provider_id), Some(model)) = (&policy.fallback_provider_id, &policy.fallback_model) {
        targets.push(ProviderTarget { provider_id: provider_id.clone(), invocation: ProviderInvocationConfig {
          model: model.clone(), thinking_level: policy.fallback_thinking_level, timeouts: Some(groq_timeouts),
        } });
      }
      ProviderSelection::Preferred(policy.provider_id.clone())
    }
  };
  let request = ProviderTaskRequest { input: message, history, context: Arc::new(context), max_output_tokens: policy.max_output_tokens,
    selection, targets, required_capabilities: ProviderCapabilities::text_stream() };
  (budget, request)
}

pub fn start_conversation(registry: Arc<TaskRegistry>, db: Database, runtime: Arc<ProviderRuntime>,
  gemini: Arc<crate::cognition::gemini::GeminiTimeoutState>, groq: Arc<crate::cognition::groq::GroqTimeoutState>,
  session_id: i64, message: String, policy: CognitiveRolePolicy, channel: Channel<TaskEvent>) -> Result<TaskId, String> {
  policy.validate().map_err(str::to_owned)?;
  let (id, cancelled) = registry.register()?;
  *registry.foreground_provider_tasks.lock().unwrap_or_else(|poison| poison.into_inner()).entry(session_id).or_default() += 1;
  let gemini_timeouts = *gemini.timeouts.read().unwrap_or_else(|poison| poison.into_inner());
  let groq_timeouts = *groq.timeouts.read().unwrap_or_else(|poison| poison.into_inner());
  let started_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
  tauri::async_runtime::spawn(async move {
    let _active = ActiveTask { registry: registry.clone(), id, session_id: Some(session_id) };
    let mut sequence = 0;
    registry.mark_running(id);
    let result = async {
      emit_cognitive(&channel,id,&mut sequence,TaskEventKind::TaskStarted,&cancelled).map_err(|_| "channel_closed")?;
      let db_context = db.clone();
      let (context, history) = tauri::async_runtime::spawn_blocking(move || {
        let conn = db_context.open().map_err(|e| e.code())?;
        let history = conversation::outbound_history(&conn, session_id, policy.history_max_messages as usize, policy.history_max_bytes as usize).map_err(|e| e.code())?;
        let context = ContextBuilder::build(&conn, ContextRequest { domain: None, kind: None, min_importance: 0,
          memory_limit: 0, include_recent_conversation: false }).map_err(|e| e.code())?;
        Ok::<_, &'static str>((context, history))
      }).await.map_err(|_| "worker_failed")??;
      emit_cognitive(&channel,id,&mut sequence,TaskEventKind::ContextBuilt { memory_count: 0, recent_message_count: 0 },&cancelled)
        .map_err(|_| "channel_closed")?;
      let history = history.into_iter().map(|turn| ProviderMessage {
        role: match turn.role { conversation::SessionRole::User => ProviderRole::User, conversation::SessionRole::Assistant => ProviderRole::Assistant },
        content: turn.content,
      }).collect();
      let (budget, request) = chat_budget_and_request(message.clone(), history, context, &policy, gemini_timeouts, groq_timeouts);
      let result = runtime.scheduler.run_with_retry(request,budget,policy.retry_policy(),&cancelled,&mut |event| {
        let kind = match event {
          SchedulerEvent::Selected { provider_id, attempt } => TaskEventKind::ProviderSelected { provider_id, attempt },
          SchedulerEvent::Chunk { provider_id, text } => TaskEventKind::ProviderChunk { provider_id, chunk: text },
          SchedulerEvent::Retry { provider_id, reason_code } => TaskEventKind::ProviderRetry { provider_id, reason_code: reason_code.into() },
          SchedulerEvent::Fallback { from, to, reason_code } => TaskEventKind::ProviderFallback { from_provider_id: from, to_provider_id: to, reason_code: reason_code.into() },
        };
        emit_cognitive(&channel,id,&mut sequence,kind,&cancelled).map_err(|_| SchedulerError::EventSinkClosed)
      }).await.map_err(|e| e.code())?;
      // From this point the provider call is finished and the local exchange is
      // entering its commit phase. A cancel accepted before this boundary wins;
      // a later cancel is rejected instead of leaving a cancelled task with a
      // persisted final assistant answer.
      if registry.finish(id, TaskState::Completed) == TaskState::Cancelled { return Err("cancelled"); }
      let db_write = db.clone();
      let user = message.clone(); let answer = result.text.clone();
      tauri::async_runtime::spawn_blocking(move || {
        let mut conn = db_write.open().map_err(|e| e.code())?;
        conversation::append_exchange_to_session(&mut conn,session_id,&user,&answer).map_err(|e| e.code())
      }).await.map_err(|_| "worker_failed")??;
      emit_cognitive(&channel,id,&mut sequence,TaskEventKind::TaskResultReady { result },&cancelled)
        .map_err(|_| "channel_closed")?;
      Ok::<(), &'static str>(())
    }.await;
    let (outcome, error_code) = match result {
      Ok(()) => (TaskState::Completed,None), Err("cancelled") => (TaskState::Cancelled,None),
      Err(code) => (TaskState::Failed,Some(code)),
    };
    let mut state = if error_code == Some("channel_closed") { registry.finish_channel_closed(id) } else { registry.finish(id,outcome) };
    let mut error_code = error_code;
    let record = TaskRecord { task_id:id.0, kind:"conversation".into(),
      state:match state {TaskState::Completed=>"completed",TaskState::Cancelled=>"cancelled",_=>"failed"}.into(),
      started_at, finished_at:Utc::now().to_rfc3339_opts(SecondsFormat::Millis,true), summary:Some("Conversa multi-provider LR-7C".into()),
      error_code:error_code.map(str::to_owned) };
    let db_record = db.clone();
    let write = tauri::async_runtime::spawn_blocking(move || { let conn=db_record.open()?; task_history::insert(&conn,&record) }).await;
    if !matches!(write,Ok(Ok(()))) {
      eprintln!("[Luna Core] task_history code=write_failed task_id={}",id.0);
      state = TaskState::Failed; error_code = Some("task_history_write_failed");
    }
    let terminal = match state { TaskState::Completed => TaskEventKind::TaskCompleted, TaskState::Cancelled => TaskEventKind::TaskCancelled,
      _ => TaskEventKind::TaskFailed { detail: error_code.unwrap_or("task_failed").into() } };
    if emit(&channel,id,&mut sequence,state,terminal).is_err() {
      cancelled.store(true,Ordering::Release); error_code = Some("channel_closed");
      let update = tauri::async_runtime::spawn_blocking(move || { let conn=db.open()?;
        task_history::mark_failed(&conn,id.0,"channel_closed") }).await;
      if !matches!(update,Ok(Ok(()))) { eprintln!("[Luna Core] task_history code=channel_update_failed task_id={}",id.0); }
    }
    if matches!(error_code,Some("provider_auth_failed" | "channel_closed")) {
      crate::security::audit::AuditEvent::new(crate::security::audit::Action::SecurityError,
        crate::security::audit::Outcome::Failed).with_detail(error_code.unwrap()).with_task_id(id.0).emit();
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
  emit(channel, id, sequence, TaskState::Running, TaskEventKind::TaskStarted)?;

  for step in [TaskStep::Prepare, TaskStep::Verify] {
    if cancelled.load(Ordering::Acquire) {
      return finish_and_emit(registry, channel, id, sequence, TaskState::Cancelled);
    }
    emit(channel, id, sequence, TaskState::Running, TaskEventKind::StepStarted { step })?;
    if wait_or_cancel(cancelled, Duration::from_millis(850)).await {
      return finish_and_emit(registry, channel, id, sequence, TaskState::Cancelled);
    }
    emit(channel, id, sequence, TaskState::Running, TaskEventKind::StepCompleted { step })?;
  }

  finish_and_emit(registry, channel, id, sequence, TaskState::Completed)
}

fn finish_and_emit(registry: &TaskRegistry, channel: &Channel<TaskEvent>, id: TaskId, sequence: &mut u32, outcome: TaskState) -> Result<TaskState, String> {
  let state = registry.finish(id, outcome);
  let kind = if state == TaskState::Cancelled { TaskEventKind::TaskCancelled } else { TaskEventKind::TaskCompleted };
  emit(channel, id, sequence, state, kind)?;
  Ok(state)
}

pub fn start(registry: Arc<TaskRegistry>, db: Database, channel: Channel<TaskEvent>) -> Result<TaskId, String> {
  let (id, cancelled) = registry.register()?;
  let started_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
  tauri::async_runtime::spawn(async move {
    let _active = ActiveTask { registry: registry.clone(), id, session_id: None };
    let mut sequence = 0;
    let outcome = match run_mock_task(&registry, id, &cancelled, &channel, &mut sequence).await {
      Ok(state) => state,
      Err(_) => {
        let state = registry.finish(id, TaskState::Failed);
        let kind = if state == TaskState::Cancelled { TaskEventKind::TaskCancelled } else { TaskEventKind::TaskFailed { detail: "Falha no canal ou worker".into() } };
        let _ = emit(&channel, id, &mut sequence, state, kind);
        eprintln!("[Luna Core] tarefa {} falhou; code=channel_or_worker_error", id.0);
        state
      }
    };
    let state = match outcome { TaskState::Completed => "completed", TaskState::Cancelled => "cancelled", _ => "failed" };
    let record = TaskRecord { task_id:id.0,kind:"mock".into(),state:state.into(),started_at,
      finished_at:Utc::now().to_rfc3339_opts(SecondsFormat::Millis,true),
      summary:Some("Tarefa mock LR-2".into()),error_code:if state == "failed" { Some("channel_or_worker_error".into()) } else { None } };
    let write = tauri::async_runtime::spawn_blocking(move || {
      let conn = db.open()?;
      task_history::insert(&conn, &record)
    }).await;
    if !matches!(write, Ok(Ok(()))) { eprintln!("[Luna Core] task_history code=write_failed task_id={}", id.0); }
  });
  Ok(id)
}

#[cfg(debug_assertions)]
pub fn start_cognition(registry: Arc<TaskRegistry>, db: Database, cognition: Arc<CognitionRuntime>,
  scenario: DiagnosticScenario, channel: Channel<TaskEvent>) -> Result<TaskId, String> {
  let (id, cancelled) = registry.register()?;
  let started_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
  tauri::async_runtime::spawn(async move {
    let _active = ActiveTask { registry: registry.clone(), id, session_id: None };
    let mut sequence = 0;
    registry.mark_running(id);
    let started = emit_cognitive(&channel, id, &mut sequence, TaskEventKind::TaskStarted, &cancelled);
    let db_for_context = db.clone();
    let context = if started.is_ok() { Some(tauri::async_runtime::spawn_blocking(move || {
      let conn = db_for_context.open().map_err(|e| e.code())?;
      ContextBuilder::build(&conn, ContextRequest { domain: None, kind: None, min_importance: 0,
        memory_limit: 3, include_recent_conversation: true }).map_err(|e| e.code())
    }).await) } else { None };
    let result = match context {
      None => Err("channel_closed"),
      Some(Ok(Ok(context))) => {
        let context_event = emit_cognitive(&channel, id, &mut sequence, TaskEventKind::ContextBuilt {
          memory_count: context.metadata.memory_count, recent_message_count: context.metadata.recent_message_count }, &cancelled);
        let budget = if scenario == DiagnosticScenario::BudgetExhausted {
          TaskBudget { max_provider_calls: 1, max_output_tokens: Some(32) }
        } else { TaskBudget { max_provider_calls: 3, max_output_tokens: Some(32) } };
        let request = ProviderTaskRequest { input: "Execute o diagnóstico cognitivo LR-5.".into(), history: vec![], context: Arc::new(context),
          max_output_tokens: budget.max_output_tokens, selection: ProviderSelection::Auto, targets: ["mock-primary", "mock-fallback"].into_iter().map(|id| ProviderTarget {
            provider_id: id.into(), invocation: ProviderInvocationConfig { model: "mock".into(), thinking_level: None, timeouts: None }
          }).collect(), required_capabilities: ProviderCapabilities::text_stream() };
        if context_event.is_err() { Err("channel_closed") } else { cognition.scheduler(scenario).run(request, budget, &cancelled, &mut |event| {
          let kind = match event {
            SchedulerEvent::Selected { provider_id, attempt } => TaskEventKind::ProviderSelected { provider_id, attempt },
            SchedulerEvent::Chunk { provider_id, text } => TaskEventKind::ProviderChunk { provider_id, chunk: text },
            SchedulerEvent::Retry { provider_id, reason_code } => TaskEventKind::ProviderRetry { provider_id, reason_code: reason_code.into() },
            SchedulerEvent::Fallback { from, to, reason_code } => TaskEventKind::ProviderFallback { from_provider_id: from, to_provider_id: to, reason_code: reason_code.into() },
          };
          emit_cognitive(&channel, id, &mut sequence, kind, &cancelled).map_err(|_| SchedulerError::EventSinkClosed)
        }).await.map_err(|e| e.code()) }
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
        if emit_cognitive(&channel, id, &mut sequence, TaskEventKind::TaskResultReady { result }, &cancelled).is_err() {
          outcome = TaskState::Failed;
          error_code = Some("channel_closed");
        }
      }
    }
    let mut state = if error_code == Some("channel_closed") { registry.finish_channel_closed(id) }
      else { registry.finish(id, outcome) };
    let terminal = match state {
      TaskState::Completed => TaskEventKind::TaskCompleted,
      TaskState::Cancelled => TaskEventKind::TaskCancelled,
      _ => TaskEventKind::TaskFailed { detail: error_code.unwrap_or("task_failed").into() },
    };
    if emit(&channel, id, &mut sequence, state, terminal).is_err() {
      cancelled.store(true, Ordering::Release);
      state = TaskState::Failed;
      error_code = Some("channel_closed");
    }
    if error_code == Some("channel_closed") {
      crate::security::audit::AuditEvent::new(crate::security::audit::Action::SecurityError,
        crate::security::audit::Outcome::Failed).with_task_id(id.0).with_detail("channel_closed").emit();
    }
    let record = TaskRecord { task_id: id.0, kind: "mock_cognition".into(),
      state: match state { TaskState::Completed => "completed", TaskState::Cancelled => "cancelled", _ => "failed" }.into(),
      started_at, finished_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
      summary: Some("Diagnóstico cognitivo LR-5".into()), error_code: if state == TaskState::Failed { Some(error_code.unwrap_or("task_failed").into()) } else { None } };
    let write = tauri::async_runtime::spawn_blocking(move || {
      let conn = db.open()?; task_history::insert(&conn, &record)
    }).await;
    if !matches!(write, Ok(Ok(()))) { eprintln!("[Luna Core] task_history code=write_failed task_id={}", id.0); }
  });
  Ok(id)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn chat_policy_is_snapshot_for_each_request() {
    use crate::cognition::policy::{CognitiveRole, ThinkingLevel};
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
    let context = || ContextBundle { identity: identity.clone(), relevant_memories: vec![], recent_messages: vec![],
      metadata: ContextMetadata { identity_version: "test".into(), memory_count: 0, recent_message_count: 0 } };
    let mut policy = CognitiveRolePolicy { role: CognitiveRole::Conversation, provider_id: "gemini".into(),
      model: "gemini-custom".into(), thinking_level: Some(ThinkingLevel::High), max_output_tokens: Some(8192), max_provider_calls: 3, retry_enabled: true, max_retries: 1, retry_backoff_ms: 1500, history_max_messages: 8, history_max_bytes: 12288, summary_input_max_bytes: 32768 };
    let first_timeouts = crate::persistence::gemini_settings::GeminiTimeouts::default();
    let (first_budget, first) = chat_budget_and_request("Oi".into(), vec![], context(), &policy, first_timeouts.into());
    policy.model = "gemini-new".into(); policy.thinking_level = None; policy.max_output_tokens = None; policy.max_provider_calls = 1;
    let next_timeouts = crate::persistence::gemini_settings::GeminiTimeouts { request_timeout_ms: 60_000, stream_idle_timeout_ms: 20_000 };
    let (next_budget, next) = chat_budget_and_request("Oi".into(), vec![], context(), &policy, next_timeouts.into());
    assert_eq!(first.selection, ProviderSelection::Fixed("gemini".into()));
    assert_eq!((first.targets[0].invocation.model.as_str(), first.targets[0].invocation.thinking_level, first.max_output_tokens, first_budget.max_provider_calls),
      ("gemini-custom", Some(ThinkingLevel::High), Some(8192), 3));
    assert_eq!((next.targets[0].invocation.model.as_str(), next.targets[0].invocation.thinking_level, next.max_output_tokens, next_budget.max_provider_calls),
      ("gemini-new", None, None, 1));
    assert_eq!(first.targets[0].invocation.timeouts, Some(first_timeouts.into()));
    assert_eq!(next.targets[0].invocation.timeouts, Some(next_timeouts.into()));
  }

  #[test]
  fn ids_are_monotonic_and_tasks_are_removed() {
    let registry = Arc::new(TaskRegistry::default());
    let (first, _) = registry.register().unwrap();
    let (second, _) = registry.register().unwrap();
    assert_eq!((first.0, second.0), (1, 2));
    registry.mark_running(first);
    assert_eq!(registry.active.lock().unwrap().get(&first).unwrap().state, TaskState::Running);
    drop(ActiveTask { registry: registry.clone(), id: first, session_id: None });
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
    assert_eq!(registry.finish(id, TaskState::Completed), TaskState::Cancelled);
    assert!(!registry.cancel(id));

    let (completed, _) = registry.register().unwrap();
    assert_eq!(registry.finish(completed, TaskState::Completed), TaskState::Completed);
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
