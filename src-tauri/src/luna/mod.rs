pub mod runtime;
pub(crate) mod task;

use std::sync::Arc;

use crate::cognition::gemini_commands::CurrentRunSessions;
use crate::cognition::policy::{self, CognitiveRole, RoutingMode};
use crate::cognition::ProviderRuntime;
#[cfg(debug_assertions)]
use crate::cognition::{scheduler::ProviderStatus, CognitionRuntime, DiagnosticScenario};
use crate::persistence::database::Database;
use crate::security::secrets::SecretStore;
use serde::Serialize;
use tauri::{ipc::Channel, State};

use crate::security::{
    audit::{Action, AuditEvent, Outcome},
    validation,
};
use runtime::TaskRegistry;
use task::{TaskEvent, TaskId};

#[tauri::command]
pub fn start_mock_task(
    registry: State<'_, Arc<TaskRegistry>>,
    db: State<'_, Database>,
    channel: Channel<TaskEvent>,
) -> Result<TaskId, String> {
    AuditEvent::new(Action::CommandInvoked, Outcome::Allowed)
        .with_detail("start_mock_task")
        .emit();
    runtime::start(registry.inner().clone(), db.inner().clone(), channel).inspect_err(|_| {
        AuditEvent::new(Action::SecurityError, Outcome::Failed)
            .with_detail("task_registration_failed")
            .emit();
    })
}

#[tauri::command]
pub fn cancel_task(registry: State<'_, Arc<TaskRegistry>>, task_id: u64) -> Result<bool, String> {
    AuditEvent::new(Action::CommandInvoked, Outcome::Allowed)
        .with_detail("cancel_task")
        .emit();
    let id = validation::task_id(task_id).map_err(|code| {
        AuditEvent::new(Action::SecurityError, Outcome::Denied)
            .with_detail(code)
            .emit();
        "TaskId inválido".to_string()
    })?;
    let accepted = registry.cancel(TaskId(id));
    AuditEvent::new(
        Action::TaskCancelRequested,
        if accepted {
            Outcome::Succeeded
        } else {
            Outcome::Denied
        },
    )
    .with_task_id(id)
    .emit();
    Ok(accepted)
}

#[tauri::command]
pub fn start_orchestrator_planning(
    registry: State<'_, Arc<TaskRegistry>>,
    db: State<'_, Database>,
    runtime: State<'_, Arc<ProviderRuntime>>,
    store: State<'_, Arc<SecretStore>>,
    objective: String,
    channel: Channel<TaskEvent>,
) -> Result<TaskId, String> {
    AuditEvent::new(Action::CommandInvoked, Outcome::Allowed)
        .with_detail("start_orchestrator_planning")
        .emit();
    crate::cognition::orchestrator::start_task(
        registry.inner().clone(),
        db.inner().clone(),
        runtime.inner().clone(),
        store.inner().clone(),
        objective,
        channel,
    )
}

#[cfg(debug_assertions)]
#[tauri::command]
pub fn start_mock_cognition_task(
    registry: State<'_, Arc<TaskRegistry>>,
    db: State<'_, Database>,
    cognition: State<'_, Arc<CognitionRuntime>>,
    scenario: DiagnosticScenario,
    channel: Channel<TaskEvent>,
) -> Result<TaskId, String> {
    AuditEvent::new(Action::CommandInvoked, Outcome::Allowed)
        .with_detail("start_mock_cognition_task")
        .emit();
    runtime::start_cognition(
        registry.inner().clone(),
        db.inner().clone(),
        cognition.inner().clone(),
        scenario,
        channel,
    )
}

#[cfg(debug_assertions)]
#[tauri::command]
pub fn cognition_provider_status(
    cognition: State<'_, Arc<CognitionRuntime>>,
) -> Vec<ProviderStatus> {
    cognition.status()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationProviderState {
    provider_id: String,
    display_name: String,
    configured: bool,
    cooldown_ms: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRoutingStatus {
    routing_mode: RoutingMode,
    targets: Vec<ConversationProviderState>,
}

#[tauri::command]
pub async fn conversation_routing_status(
    db: State<'_, Database>,
    runtime: State<'_, Arc<ProviderRuntime>>,
    store: State<'_, Arc<SecretStore>>,
) -> Result<ConversationRoutingStatus, String> {
    let db = db.inner().clone();
    let store = store.inner().clone();
    let statuses = runtime.scheduler.status();
    tauri::async_runtime::spawn_blocking(move || {
        let conn = db.open().map_err(|e| e.code().to_owned())?;
        let policy =
            policy::load(&conn, CognitiveRole::Conversation).map_err(|e| e.code().to_owned())?;
        crate::cognition::catalog::validate_policy_registered(&policy, &statuses)
            .map_err(str::to_owned)?;
        let state = |provider_id: &str| ConversationProviderState {
            provider_id: provider_id.to_owned(),
            display_name: crate::cognition::catalog::integration(provider_id)
                .map(|item| item.display_name)
                .unwrap_or(provider_id)
                .to_owned(),
            configured: crate::cognition::catalog::configured(store.as_ref(), provider_id),
            cooldown_ms: statuses
                .iter()
                .find(|status| status.id == provider_id)
                .map(|status| status.cooldown_ms)
                .unwrap_or(0),
        };
        Ok(ConversationRoutingStatus {
            routing_mode: policy.routing_mode,
            targets: policy
                .targets
                .iter()
                .map(|target| state(&target.provider_id))
                .collect(),
        })
    })
    .await
    .map_err(|_| "worker_failed".to_owned())?
}

#[tauri::command]
pub fn start_conversation_task(
    registry: State<'_, Arc<TaskRegistry>>,
    db: State<'_, Database>,
    runtime: State<'_, Arc<ProviderRuntime>>,
    store: State<'_, Arc<SecretStore>>,
    sessions: State<'_, CurrentRunSessions>,
    session_id: i64,
    message: String,
    channel: Channel<TaskEvent>,
) -> Result<TaskId, String> {
    runtime::start_conversation(
        registry.inner().clone(),
        db.inner().clone(),
        runtime.inner().clone(),
        store.inner().clone(),
        sessions.inner().clone(),
        session_id,
        message,
        channel,
    )
}
