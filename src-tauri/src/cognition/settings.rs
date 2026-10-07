use super::{
    catalog,
    policy::{self, CognitiveRole, CognitiveRolePolicy},
    summary::SummaryWorker,
    ProviderRuntime, ProviderTimeoutHandles,
};
use crate::{
    persistence::{
        conversation,
        database::Database,
        general_settings::{self, GeneralSettings},
        provider_timeouts,
    },
    security::secrets::SecretStore,
};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

// Independent writes prevent an open settings window from overwriting layout changes.
#[tauri::command]
pub async fn get_shell_settings(db: State<'_, Database>, app: AppHandle) -> Result<crate::persistence::shell_settings::ShellSettings, String> {
    let db = db.inner().clone();
    let mut settings = tauri::async_runtime::spawn_blocking(move || {
        let conn = db.open().map_err(|e| e.code().to_owned())?;
        crate::persistence::shell_settings::load(&conn).map_err(|e| e.code().to_owned())
    }).await.map_err(|_| "worker_failed".to_owned())??;
    // Temporary control surface (Headless/attention) must hydrate from runtime, not legacy opt-in.
    let runtime = app.state::<crate::adaptive::AdaptivePresentationManager>().snapshot();
    settings.presentation_mode = if runtime.state == crate::adaptive::RuntimeState::Presence { crate::persistence::shell_settings::PresentationMode::Presence } else { crate::persistence::shell_settings::PresentationMode::Economy };
    Ok(settings)
}

#[tauri::command]
pub async fn update_presentation_mode(db: State<'_, Database>, app: AppHandle, mode: crate::persistence::shell_settings::PresentationMode) -> Result<(), String> {
    let _ = db;
    let policy = if mode == crate::persistence::shell_settings::PresentationMode::Presence { crate::adaptive::PresentationPolicy::Presence } else { crate::adaptive::PresentationPolicy::Economy };
    crate::adaptive::on_main(app, move |app| crate::adaptive::set_policy(app, policy)).await
}

#[tauri::command]
pub async fn update_shell_layout(db: State<'_, Database>, layout: crate::persistence::shell_settings::ShellLayout) -> Result<(), String> {
    let db = db.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let conn = db.open().map_err(|e| e.code().to_owned())?;
        crate::persistence::shell_settings::save_layout(&conn, layout).map_err(|e| e.code().to_owned())
    }).await.map_err(|_| "worker_failed".to_owned())?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneralSettingsUpdate {
    pub settings: GeneralSettings,
    pub always_on_top_requested: bool,
}

#[tauri::command]
pub async fn get_general_settings(db: State<'_, Database>) -> Result<GeneralSettings, String> {
    let db = db.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let conn = db.open().map_err(|e| e.code())?;
        general_settings::load(&conn).map_err(|e| e.code())
    })
    .await
    .map_err(|_| "worker_failed".to_string())?
    .map_err(str::to_owned)
}

#[tauri::command]
pub async fn update_general_settings(
    db: State<'_, Database>,
    app: AppHandle,
    settings: GeneralSettings,
) -> Result<GeneralSettingsUpdate, String> {
    settings.validate().map_err(str::to_owned)?;
    let db = db.inner().clone();
    let saved = tauri::async_runtime::spawn_blocking(move || {
        let conn = db.open().map_err(|e| e.code())?;
        general_settings::save(&conn, &settings).map_err(|e| e.code())?;
        Ok::<_, &'static str>(settings)
    })
    .await
    .map_err(|_| "worker_failed".to_string())?
    .map_err(str::to_owned)?;
    let main = app.get_webview_window("main");
    let always_on_top_requested = main
        .as_ref()
        .is_some_and(|window| window.set_always_on_top(saved.always_on_top).is_ok());
    app.emit_to("main", "general-settings-changed", &saved)
        .map_err(|_| "settings_event_failed")?;
    Ok(GeneralSettingsUpdate {
        settings: saved,
        always_on_top_requested,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSettings {
    resilience: Vec<super::resilience::ResilienceSnapshot>,
    rate: Vec<super::rate::RateSnapshot>,
    admission: Vec<super::admission::AdmissionSnapshot>,
    telemetry: Vec<super::telemetry::ProviderTelemetrySnapshot>,
    providers: Vec<catalog::ProviderInfo>,
    roles: Vec<CognitiveRolePolicy>,
    allocation_policies: Vec<super::allocation_policy::CognitiveRoleAllocationPolicy>,
    credential_store_available: bool,
    provider_timeouts: std::collections::HashMap<String, super::types::ProviderTimeouts>,
}

#[tauri::command]
pub async fn get_ai_settings(
    db: State<'_, Database>,
    store: State<'_, Arc<SecretStore>>,
    runtime: State<'_, Arc<ProviderRuntime>>,
) -> Result<AiSettings, String> {
    let db = db.inner().clone();
    let store = store.inner().clone();
    let statuses = runtime.scheduler.status();
    let telemetry = runtime.scheduler.telemetry_snapshot();
    let admission = runtime.scheduler.admission_snapshot();
    let rate = runtime.scheduler.rate_snapshot();
    let resilience = runtime.scheduler.resilience_snapshot();
    tauri::async_runtime::spawn_blocking(move || {
        let conn = db.open().map_err(|e| e.code())?;
        let settings =
            super::allocation_policy::load_all_role_settings(&conn).map_err(|e| e.code())?;
        let roles = settings.iter().map(|s| s.policy.clone()).collect();
        let allocation_policies = settings.into_iter().map(|s| s.allocation_policy).collect();
        let mut timeouts = std::collections::HashMap::new();
        for status in statuses
            .iter()
            .filter(|status| catalog::integration(&status.id).is_some())
        {
            timeouts.insert(
                status.id.clone(),
                provider_timeouts::load(&conn, &status.id).map_err(|e| e.code())?,
            );
        }
        let infos = catalog::infos(&statuses, &store);
        Ok::<_, &'static str>(AiSettings {
            resilience,
            rate,
            admission,
            telemetry,
            providers: infos.providers,
            roles,
            allocation_policies,
            credential_store_available: infos.credential_store_available,
            provider_timeouts: timeouts,
        })
    })
    .await
    .map_err(|_| "worker_failed".to_string())?
    .map_err(str::to_owned)
}

/// Memory-only read. No Database, SecretStore, credential, filesystem or provider call.
#[tauri::command]
pub async fn get_provider_operational_snapshot(
    runtime: State<'_, Arc<ProviderRuntime>>,
) -> Result<super::operational::ProviderOperationalSnapshot, String> {
    let scheduler = runtime.scheduler.clone();
    // Authority locks may briefly wait for concurrent mutations. Keep that wait
    // off the WebView/main thread; the worker performs only the memory read.
    tauri::async_runtime::spawn_blocking(move || scheduler.operational_snapshot())
        .await
        .map_err(|_| "worker_failed".to_owned())
}

#[tauri::command]
pub async fn update_provider_rate_policy(
    runtime: State<'_, Arc<ProviderRuntime>>,
    provider_id: String,
    policy: super::rate::RatePolicy,
) -> Result<(), String> {
    let rate = runtime.scheduler.rate.clone();
    tauri::async_runtime::spawn_blocking(move || rate.set_policy(&provider_id, policy))
        .await
        .map_err(|_| "worker_failed".to_owned())?
        .map_err(|e| e.code().to_owned())
}

#[tauri::command]
pub async fn update_provider_timeouts(
    db: State<'_, Database>,
    handles: State<'_, Arc<ProviderTimeoutHandles>>,
    provider_id: String,
    timeouts: super::types::ProviderTimeouts,
) -> Result<super::types::ProviderTimeouts, String> {
    let handle = handles
        .0
        .get(&provider_id)
        .ok_or("provider_unavailable")?
        .clone();
    let value = timeouts;
    let db = db.inner().clone();
    let id = provider_id;
    let saved = tauri::async_runtime::spawn_blocking(move || {
        let conn = db.open().map_err(|e| e.code())?;
        provider_timeouts::save(&conn, &id, value).map_err(|e| e.code())?;
        Ok::<_, &'static str>(value)
    })
    .await
    .map_err(|_| "worker_failed".to_string())?
    .map_err(str::to_owned)?;
    *handle.write().unwrap_or_else(|p| p.into_inner()) = saved;
    Ok(saved)
}

#[tauri::command]
pub async fn update_cognitive_role_policy(
    db: State<'_, Database>,
    worker: State<'_, Arc<SummaryWorker>>,
    runtime: State<'_, Arc<ProviderRuntime>>,
    store: State<'_, Arc<SecretStore>>,
    policy: CognitiveRolePolicy,
) -> Result<CognitiveRolePolicy, String> {
    policy.validate().map_err(str::to_owned)?;
    let summary_disabled =
        policy.role == CognitiveRole::Summary && policy.summary_input_max_bytes == 0;
    if !summary_disabled {
        catalog::validate_policy(&policy, &runtime.scheduler.status(), &store)
            .map_err(str::to_owned)?;
    }
    let db = db.inner().clone();
    let saved = tauri::async_runtime::spawn_blocking(move || {
        let mut conn = db.open().map_err(|e| e.code())?;
        // Preserve compatibility while keeping disabled Summary cleanup atomic.
        let tx = conn.transaction().map_err(|_| "write_failed")?;
        policy::write_in_transaction(&tx, &policy).map_err(|e| e.code())?;
        if policy.role == CognitiveRole::Summary && policy.summary_input_max_bytes == 0 {
            conversation::disable_pending_summaries(&tx).map_err(|e| e.code())?;
        }
        let saved = policy::load(&tx, policy.role).map_err(|e| e.code())?;
        tx.commit().map_err(|_| "write_failed")?;
        Ok::<_, &'static str>(saved)
    })
    .await
    .map_err(|_| "worker_failed".to_string())?
    .map_err(str::to_owned)?;
    if saved.role == CognitiveRole::Summary {
        worker.kick();
    }
    Ok(saved)
}

/// One user action, one validated write transaction, including Summary disable.
#[tauri::command]
pub async fn update_cognitive_role_settings(
    db: State<'_, Database>,
    worker: State<'_, Arc<SummaryWorker>>,
    runtime: State<'_, Arc<ProviderRuntime>>,
    store: State<'_, Arc<SecretStore>>,
    policy: CognitiveRolePolicy,
    allocation_policy: super::allocation_policy::CognitiveRoleAllocationPolicy,
) -> Result<super::allocation_policy::CognitiveRoleSettings, String> {
    super::allocation_policy::validate_role_settings(&policy, &allocation_policy)
        .map_err(str::to_owned)?;
    let db = db.inner().clone();
    let store = store.inner().clone();
    let statuses = runtime.scheduler.status();
    let saved = tauri::async_runtime::spawn_blocking(move || {
        if !(policy.role == CognitiveRole::Summary && policy.summary_input_max_bytes == 0) {
            catalog::validate_policy(&policy, &statuses, &store)?;
        }
        let mut conn = db.open().map_err(|e| e.code())?;
        super::allocation_policy::save_role_settings(&mut conn, &policy, &allocation_policy)
            .map_err(|e| e.code())
    })
    .await
    .map_err(|_| "worker_failed".to_owned())?
    .map_err(str::to_owned)?;
    if saved.policy.role == CognitiveRole::Summary {
        worker.kick();
    }
    Ok(saved)
}

fn open_window(
    app: &AppHandle,
    label: &str,
    title: &str,
    width: f64,
    height: f64,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(label) {
        window.show().map_err(|_| "settings_window_failed")?;
        window.set_focus().map_err(|_| "settings_window_failed")?;
        return Ok(());
    }
    WebviewWindowBuilder::new(
        app,
        label,
        WebviewUrl::App(format!("index.html?surface={label}").into()),
    )
    .title(title)
    .inner_size(width, height)
    .min_inner_size(460.0, 400.0)
    .resizable(true)
    .decorations(true)
    .transparent(false)
    .build()
    .map_err(|_| "settings_window_failed")?;
    Ok(())
}
#[tauri::command]
pub fn open_general_settings_window(app: AppHandle) -> Result<(), String> {
    open_window(
        &app,
        "settings-general",
        "Configurações da Luna",
        650.0,
        560.0,
    )
}
#[tauri::command]
pub fn open_ai_settings_window(app: AppHandle) -> Result<(), String> {
    open_window(&app, "settings-ai", "IA e modelos", 760.0, 750.0)
}
