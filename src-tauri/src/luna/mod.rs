pub mod runtime;
mod task;

use std::sync::Arc;

use tauri::{ipc::Channel, State};
use crate::persistence::database::Database;
#[cfg(debug_assertions)]
use crate::cognition::{CognitionRuntime, DiagnosticScenario, scheduler::ProviderStatus};

use runtime::TaskRegistry;
use task::{TaskEvent, TaskId};
use crate::security::{audit::{Action, AuditEvent, Outcome}, validation};

#[tauri::command]
pub fn start_mock_task(registry: State<'_, Arc<TaskRegistry>>, db: State<'_, Database>, channel: Channel<TaskEvent>) -> Result<TaskId, String> {
  AuditEvent::new(Action::CommandInvoked, Outcome::Allowed).with_detail("start_mock_task").emit();
  runtime::start(registry.inner().clone(), db.inner().clone(), channel).inspect_err(|_| {
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

#[cfg(debug_assertions)]
#[tauri::command]
pub fn start_mock_cognition_task(registry: State<'_, Arc<TaskRegistry>>, db: State<'_, Database>,
  cognition: State<'_, Arc<CognitionRuntime>>, scenario: DiagnosticScenario, channel: Channel<TaskEvent>) -> Result<TaskId, String> {
  AuditEvent::new(Action::CommandInvoked, Outcome::Allowed).with_detail("start_mock_cognition_task").emit();
  runtime::start_cognition(registry.inner().clone(), db.inner().clone(), cognition.inner().clone(), scenario, channel)
}

#[cfg(debug_assertions)]
#[tauri::command]
pub fn cognition_provider_status(cognition: State<'_, Arc<CognitionRuntime>>) -> Vec<ProviderStatus> {
  cognition.status()
}
