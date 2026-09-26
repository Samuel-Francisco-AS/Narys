pub mod runtime;
mod task;

use std::sync::Arc;

use tauri::{ipc::Channel, State};

use runtime::TaskRegistry;
use task::{TaskEvent, TaskId};
use crate::security::{audit::{Action, AuditEvent, Outcome}, validation};

#[tauri::command]
pub fn start_mock_task(registry: State<'_, Arc<TaskRegistry>>, channel: Channel<TaskEvent>) -> Result<TaskId, String> {
  AuditEvent::new(Action::CommandInvoked, Outcome::Allowed).with_detail("start_mock_task").emit();
  runtime::start(registry.inner().clone(), channel).inspect_err(|_| {
    AuditEvent::new(Action::SecurityError, Outcome::Failed).with_detail("task_registration_failed").emit();
  })
}

#[tauri::command]
pub fn cancel_task(registry: State<'_, Arc<TaskRegistry>>, task_id: u64) -> Result<bool, String> {
  AuditEvent::new(Action::CommandInvoked, Outcome::Allowed).with_detail("cancel_task").emit();
  let id = validation::task_id(task_id).map_err(|code| {
    AuditEvent::new(Action::SecurityError, Outcome::Denied).with_detail(code).emit();
    "TaskId inválido".to_string()
  })?;
  let accepted = registry.cancel(TaskId(id));
  AuditEvent::new(Action::TaskCancelRequested, if accepted { Outcome::Succeeded } else { Outcome::Denied })
    .with_task_id(id).emit();
  Ok(accepted)
}
