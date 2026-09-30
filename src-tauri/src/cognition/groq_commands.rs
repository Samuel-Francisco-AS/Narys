use serde::Serialize;
use std::sync::{atomic::AtomicBool, Arc};
use tauri::{ipc::Channel, State};

use super::{
    groq::MODEL,
    scheduler::SchedulerEvent,
    summary::SummaryWorker,
    types::{
        ContextBundle, ContextMetadata, ProviderCapabilities, ProviderInvocationConfig,
        ProviderSelection, ProviderTarget, ProviderTaskRequest, SchedulerError, TaskBudget,
        TaskResult,
    },
    ProviderRuntime, ProviderTimeoutHandles,
};
use crate::security::{
    audit::{Action, AuditEvent, Outcome},
    secrets::{SecretKey, SecretStore},
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroqStatus {
    pub configured: bool,
    pub enabled: bool,
    pub credential_store_available: bool,
    pub cooldown_ms: u64,
}
fn cooldown_ms(runtime: &ProviderRuntime) -> u64 {
    runtime
        .scheduler
        .status()
        .into_iter()
        .find(|status| status.id == "groq")
        .map(|status| status.cooldown_ms)
        .unwrap_or(0)
}

#[tauri::command]
pub async fn groq_status(
    store: State<'_, Arc<SecretStore>>,
    runtime: State<'_, Arc<ProviderRuntime>>,
) -> Result<GroqStatus, String> {
    let store = store.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        store
            .get_secret(SecretKey::GroqApiKey)
            .map(|value| value.is_some())
    })
    .await
    .map_err(|_| "groq_status_failed")?;
    Ok(GroqStatus {
        configured: result.as_ref().copied().unwrap_or(false),
        enabled: true,
        credential_store_available: result.is_ok(),
        cooldown_ms: cooldown_ms(&runtime),
    })
}

#[tauri::command]
pub async fn groq_set_api_key(
    store: State<'_, Arc<SecretStore>>,
    runtime: State<'_, Arc<ProviderRuntime>>,
    worker: State<'_, Arc<SummaryWorker>>,
    api_key: String,
) -> Result<GroqStatus, String> {
    let key = api_key.trim();
    if key.is_empty() || key.len() > 512 || key.bytes().any(|b| b.is_ascii_control()) {
        return Err("groq_key_invalid".into());
    }
    let key = key.as_bytes().to_vec();
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.set_secret(SecretKey::GroqApiKey, &key))
        .await
        .map_err(|_| "groq_key_store_failed")?
        .map_err(|error| error.code())?;
    AuditEvent::new(Action::CommandInvoked, Outcome::Succeeded)
        .with_detail("groq_key_configured")
        .emit();
    worker.kick();
    Ok(GroqStatus {
        configured: true,
        enabled: true,
        credential_store_available: true,
        cooldown_ms: cooldown_ms(&runtime),
    })
}

#[tauri::command]
pub async fn groq_delete_api_key(
    store: State<'_, Arc<SecretStore>>,
    runtime: State<'_, Arc<ProviderRuntime>>,
    worker: State<'_, Arc<SummaryWorker>>,
) -> Result<GroqStatus, String> {
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || store.delete_secret(SecretKey::GroqApiKey))
        .await
        .map_err(|_| "groq_key_delete_failed")?
        .map_err(|error| error.code())?;
    AuditEvent::new(Action::CommandInvoked, Outcome::Succeeded)
        .with_detail("groq_key_deleted")
        .emit();
    worker.kick();
    Ok(GroqStatus {
        configured: false,
        enabled: true,
        credential_store_available: true,
        cooldown_ms: cooldown_ms(&runtime),
    })
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GroqProbeEvent {
    Selected {
        #[serde(rename = "providerId")]
        provider_id: String,
        attempt: u32,
    },
    Chunk {
        text: String,
    },
}

#[tauri::command]
pub async fn groq_probe(
    runtime: State<'_, Arc<ProviderRuntime>>,
    handles: State<'_, Arc<ProviderTimeoutHandles>>,
    channel: Channel<GroqProbeEvent>,
) -> Result<TaskResult, String> {
    let identity = serde_json::from_value(serde_json::json!({
    "version":"groq-probe","canonicalName":"Luna","presentation":"neutral","primaryLanguage":"pt-BR",
    "concept":"provider diagnostic","traits":{},"behavioralInvariants":[],"modes":{},
    "relationship":{"primaryPersonName":"","relationModes":[],"affectionStyle":{"warm":false,"provocative":false,"playfulJealousy":false,"playfulTerritoriality":false,"coercion":false,"isolation":false,"emotionalBlackmail":false},
      "interactionPreferences":{"wantsRealDisagreement":false,"wantsLunaToProposeDirectionsDuringStructuring":false,"prefersLinearFlowDuringImplementation":false}},
    "memoryPolicy":{"retrieval":"none","history":"none","continuity":"none","storePrivateChainOfThought":false},
    "provenance":"internal","effectiveFrom":"2026-01-01"
  })).map_err(|_| "groq_probe_context_invalid")?;
    let context = ContextBundle {
        identity,
        relevant_memories: vec![],
        recent_messages: vec![],
        metadata: ContextMetadata {
            identity_version: "groq-probe".into(),
            memory_count: 0,
            recent_message_count: 0,
        },
    };
    let handle = handles.0.get("groq").ok_or("provider_unavailable")?;
    let timeouts = *handle.read().unwrap_or_else(|poison| poison.into_inner());
    let request = ProviderTaskRequest {
        input: "Responda em uma frase curta: conexão Groq confirmada.".into(),
        history: vec![],
        context: Arc::new(context),
        max_output_tokens: Some(96),
        selection: ProviderSelection::Fixed("groq".into()),
        targets: vec![ProviderTarget {
            provider_id: "groq".into(),
            invocation: ProviderInvocationConfig {
                model: MODEL.into(),
                thinking_level: Some(super::policy::ThinkingLevel::Low),
                timeouts: Some(timeouts),
            },
        }],
        required_capabilities: ProviderCapabilities::text_stream(),
    };
    let cancelled = AtomicBool::new(false);
    runtime
        .scheduler
        .run(
            request,
            TaskBudget {
                max_provider_calls: 1,
                max_output_tokens: Some(96),
            },
            &cancelled,
            &mut |event| {
                let outbound = match event {
                    SchedulerEvent::Selected {
                        provider_id,
                        attempt,
                    } => Some(GroqProbeEvent::Selected {
                        provider_id,
                        attempt,
                    }),
                    SchedulerEvent::Chunk { text, .. } => Some(GroqProbeEvent::Chunk { text }),
                    SchedulerEvent::Retry { .. } | SchedulerEvent::Fallback { .. } => None,
                };
                if let Some(event) = outbound {
                    channel
                        .send(event)
                        .map_err(|_| SchedulerError::EventSinkClosed)?;
                }
                Ok(())
            },
        )
        .await
        .map_err(|error| error.code().to_owned())
}
