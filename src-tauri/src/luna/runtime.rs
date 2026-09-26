use std::{
  collections::HashMap,
  sync::{atomic::{AtomicBool, AtomicU64, Ordering}, Arc, Mutex},
  time::Duration,
};

use tauri::ipc::Channel;
use chrono::{SecondsFormat, Utc};
use crate::persistence::{database::Database, task_history::{self, TaskRecord}};

use super::task::{TaskEvent, TaskEventKind, TaskId, TaskState, TaskStep};

struct TaskControl {
  state: TaskState,
  cancelled: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct TaskRegistry {
  next_id: AtomicU64,
  active: Mutex<HashMap<TaskId, TaskControl>>,
}

impl TaskRegistry {
  pub fn seed_next_id(&self, last: u64) { self.next_id.store(last, Ordering::Relaxed); }
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
}

// Also removes the registration if the spawned future is dropped unexpectedly.
struct ActiveTask {
  registry: Arc<TaskRegistry>,
  id: TaskId,
}

impl Drop for ActiveTask {
  fn drop(&mut self) {
    self.registry.remove(self.id);
  }
}

fn emit(channel: &Channel<TaskEvent>, id: TaskId, sequence: &mut u32, state: TaskState, kind: TaskEventKind) -> Result<(), String> {
  *sequence += 1;
  channel.send(TaskEvent { task_id: id, sequence: *sequence, state, kind }).map_err(|error| error.to_string())
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
    let _active = ActiveTask { registry: registry.clone(), id };
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn ids_are_monotonic_and_tasks_are_removed() {
    let registry = Arc::new(TaskRegistry::default());
    let (first, _) = registry.register().unwrap();
    let (second, _) = registry.register().unwrap();
    assert_eq!((first.0, second.0), (1, 2));
    registry.mark_running(first);
    assert_eq!(registry.active.lock().unwrap().get(&first).unwrap().state, TaskState::Running);
    drop(ActiveTask { registry: registry.clone(), id: first });
    assert!(!registry.cancel(first));
    assert!(registry.cancel(second));
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
}
