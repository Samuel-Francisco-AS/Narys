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
    telemetry: Vec<super::telemetry::ProviderTelemetrySnapshot>,
    providers: Vec<catalog::ProviderInfo>,
    roles: Vec<CognitiveRolePolicy>,
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
    tauri::async_runtime::spawn_blocking(move || {
        let conn = db.open().map_err(|e| e.code())?;
        let roles = vec![
            policy::load(&conn, CognitiveRole::Conversation).map_err(|e| e.code())?,
            policy::load(&conn, CognitiveRole::Summary).map_err(|e| e.code())?,
            policy::load(&conn, CognitiveRole::Orchestrator).map_err(|e| e.code())?,
            policy::load(&conn, CognitiveRole::Worker).map_err(|e| e.code())?,
        ];
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
            telemetry,
            providers: infos.providers,
            roles,
            credential_store_available: infos.credential_store_available,
            provider_timeouts: timeouts,
        })
    })
    .await
    .map_err(|_| "worker_failed".to_string())?
    .map_err(str::to_owned)
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
        let saved = policy::save(&mut conn, &policy).map_err(|e| e.code())?;
        if saved.role == CognitiveRole::Summary && saved.summary_input_max_bytes == 0 {
            conversation::disable_pending_summaries(&conn).map_err(|e| e.code())?;
        }
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
