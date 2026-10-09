pub mod runtime;
pub(crate) mod events;
pub(crate) mod task;

use std::sync::Arc;

use crate::cognition::gemini_commands::CurrentRunSessions;
use crate::cognition::policy::{self, CognitiveRole, RoutingMode};
use crate::cognition::scheduler::ProviderStatus;
use crate::cognition::ProviderRuntime;
#[cfg(debug_assertions)]
use crate::cognition::{CognitionRuntime, DiagnosticScenario};
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

#[cfg(debug_assertions)]
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
pub async fn cancel_task(
    registry: State<'_, Arc<TaskRegistry>>,
    db: State<'_, Database>,
    task_id: u64,
) -> Result<bool, String> {
    AuditEvent::new(Action::CommandInvoked, Outcome::Allowed)
        .with_detail("cancel_task")
        .emit();
    let id = validation::task_id(task_id).map_err(|code| {
        AuditEvent::new(Action::SecurityError, Outcome::Denied)
            .with_detail(code)
            .emit();
        "TaskId inválido".to_string()
    })?;
    let accepted = cancel_task_core(registry.inner(), db.inner(), TaskId(id))
        .await
        .map_err(str::to_owned)?;
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

pub(crate) async fn cancel_task_core(
    registry: &TaskRegistry,
    db: &Database,
    root: TaskId,
) -> Result<bool, &'static str> {
    validation::task_id(root.0)?;
    let active = registry.cancel(root);
    let durable = crate::persistence::continuations::with_connection(db, move |conn| {
        crate::persistence::continuations::ContinuationRepository::cancel(conn, root.0)
    })
    .await?;
    Ok(active || durable)
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

#[tauri::command]
pub fn start_task_graph(
    registry: State<'_, Arc<TaskRegistry>>,
    db: State<'_, Database>,
    runtime: State<'_, Arc<ProviderRuntime>>,
    store: State<'_, Arc<SecretStore>>,
    objective: String,
    channel: Channel<TaskEvent>,
) -> Result<TaskId, String> {
    AuditEvent::new(Action::CommandInvoked, Outcome::Allowed)
        .with_detail("start_task_graph")
        .emit();
    crate::cognition::task_graph_runtime::start_task(
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
        routing_status_from_backend(&db, &statuses, &store)
    })
    .await
    .map_err(|_| "worker_failed".to_owned())?
}

fn routing_status_from_backend(
    db: &Database,
    statuses: &[ProviderStatus],
    store: &SecretStore,
) -> Result<ConversationRoutingStatus, String> {
    let conn = db.open().map_err(|e| e.code().to_owned())?;
    let policy =
        policy::load(&conn, CognitiveRole::Conversation).map_err(|e| e.code().to_owned())?;
    crate::cognition::catalog::validate_policy_registered(&policy, statuses)
        .map_err(str::to_owned)?;
    let ids: Vec<_> = policy
        .targets
        .iter()
        .map(|target| target.provider_id.as_str())
        .collect();
    // Informative UX only. The later task always revalidates its own backend snapshot.
    let configured = crate::cognition::catalog::configured_many(store, &ids).unwrap_or_default();
    Ok(ConversationRoutingStatus {
        routing_mode: policy.routing_mode,
        targets: policy
            .targets
            .iter()
            .map(|target| ConversationProviderState {
                provider_id: target.provider_id.clone(),
                display_name: crate::cognition::catalog::integration(&target.provider_id)
                    .map(|item| item.display_name)
                    .unwrap_or(&target.provider_id)
                    .to_owned(),
                configured: configured.get(&target.provider_id) == Some(&true),
                cooldown_ms: statuses
                    .iter()
                    .find(|status| status.id == target.provider_id)
                    .map(|status| status.cooldown_ms)
                    .unwrap_or(0),
            })
            .collect(),
    })
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
    runtime::start_conversation_with_policy(
        registry.inner().clone(),
        db.inner().clone(),
        runtime.inner().clone(),
        store.inner().clone(),
        sessions.inner().clone(),
        session_id,
        message,
        channel,
        events::TaskAttachmentPolicy::HeadlessSafe,
    )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionSnapshot {
    session_id: Option<i64>,
    task: Option<events::TaskObservation>,
}

#[tauri::command]
pub fn get_current_interaction(
    sessions: State<'_, CurrentRunSessions>,
    registry: State<'_, Arc<TaskRegistry>>,
) -> Result<InteractionSnapshot, String> {
    let session_id = sessions.selected()?;
    #[cfg(feature = "perf1c-probe")]
    crate::perf1c_probe::record("ui_get_current_interaction", serde_json::json!({"sessionId":session_id}));
    Ok(InteractionSnapshot { session_id, task: session_id.and_then(|id| registry.events.snapshot(id)) })
}

#[tauri::command]
pub fn attach_conversation_events(
    sessions: State<'_, CurrentRunSessions>,
    registry: State<'_, Arc<TaskRegistry>>,
    task_id: u64,
    session_id: i64,
    after_sequence: u32,
    channel: Channel<TaskEvent>,
) -> Result<events::TaskObservation, String> {
    if sessions.selected()? != Some(session_id) { return Err("session_invalid".into()); }
    let id = validation::task_id(task_id).map_err(str::to_owned)?;
    #[cfg(feature = "perf1c-probe")]
    crate::perf1c_probe::record("ui_attach", serde_json::json!({"taskId":id,"sessionId":session_id}));
    registry.events.attach(TaskId(id), session_id, after_sequence, channel)
}
