pub mod runtime;
mod task;

use std::sync::Arc;

use serde::Serialize;
use tauri::{ipc::Channel, State};
use crate::persistence::database::Database;
use crate::persistence::conversation;
use crate::cognition::ProviderRuntime;
use crate::cognition::policy::{self, CognitiveRole, RoutingMode};
use crate::security::secrets::{SecretKey, SecretStore};
use crate::cognition::gemini_commands::CurrentRunSessions;
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationProviderState {
  provider_id: String,
  configured: bool,
  cooldown_ms: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRoutingStatus {
  routing_mode: RoutingMode,
  primary: ConversationProviderState,
  fallback: Option<ConversationProviderState>,
}

fn provider_configured(store: &SecretStore, provider_id: &str) -> bool {
  let key = match provider_id {
    "gemini" => SecretKey::GeminiApiKey,
    "groq" => SecretKey::GroqApiKey,
    _ => return false,
  };
  store.get_secret(key).ok().flatten().is_some()
}

#[tauri::command]
pub fn conversation_routing_status(db: State<'_, Database>, runtime: State<'_, Arc<ProviderRuntime>>,
  store: State<'_, Arc<SecretStore>>) -> Result<ConversationRoutingStatus, String> {
  let conn = db.open().map_err(|e| e.code())?;
  let policy = policy::load(&conn, CognitiveRole::Conversation).map_err(|e| e.code())?;
  policy.validate().map_err(str::to_owned)?;
  let statuses = runtime.scheduler.status();
  let state = |provider_id: &str| ConversationProviderState {
    provider_id: provider_id.to_owned(),
    configured: provider_configured(store.inner().as_ref(), provider_id),
    cooldown_ms: statuses.iter().find(|status| status.id == provider_id).map(|status| status.cooldown_ms).unwrap_or(0),
  };
  Ok(ConversationRoutingStatus {
    routing_mode: policy.routing_mode,
    primary: state(&policy.provider_id),
    fallback: if policy.routing_mode == RoutingMode::Preferred {
      policy.fallback_provider_id.as_deref().map(state)
    } else { None },
  })
}

#[tauri::command]
pub fn start_conversation_task(registry: State<'_, Arc<TaskRegistry>>, db: State<'_, Database>,
  runtime: State<'_, Arc<ProviderRuntime>>, gemini: State<'_, Arc<crate::cognition::gemini::GeminiTimeoutState>>,
  groq: State<'_, Arc<crate::cognition::groq::GroqTimeoutState>>, sessions: State<'_, CurrentRunSessions>,
  session_id: i64, message: String, channel: Channel<TaskEvent>) -> Result<TaskId, String> {
  if message.trim().is_empty() || message.len() > 4096 { return Err("conversation_input_invalid".into()); }
  let current_run = sessions.0.lock().map_err(|_| "session_registry_failed")?;
  if session_id <= 0 || !current_run.contains(&session_id) { return Err("session_invalid".into()); }
  let conn = db.open().map_err(|e| e.code())?;
  if !conversation::is_active_session(&conn, session_id).map_err(|e| e.code())? { return Err("session_invalid".into()); }
  let policy = policy::load(&conn, CognitiveRole::Conversation).map_err(|e| e.code())?;
  runtime::start_conversation(registry.inner().clone(), db.inner().clone(), runtime.inner().clone(),
    gemini.inner().clone(), groq.inner().clone(), session_id, message, policy, channel)
}
