use super::{
    policy::{self, CognitiveRole, CognitiveRolePolicy},
    summary::SummaryWorker,
};
use crate::{
    persistence::database::Database,
    security::secrets::{SecretKey, SecretStore},
};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProviderInfo {
    id: &'static str,
    display_name: &'static str,
    configured: bool,
    supported_thinking_levels: [&'static str; 3],
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSettings {
    providers: Vec<AiProviderInfo>,
    roles: Vec<CognitiveRolePolicy>,
    credential_store_available: bool,
}

#[tauri::command]
pub async fn get_ai_settings(
    db: State<'_, Database>,
    store: State<'_, Arc<SecretStore>>,
) -> Result<AiSettings, String> {
    let db = db.inner().clone();
    let store = store.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let conn = db.open().map_err(|e| e.code())?;
        let roles = vec![
            policy::load(&conn, CognitiveRole::Conversation).map_err(|e| e.code())?,
            policy::load(&conn, CognitiveRole::Summary).map_err(|e| e.code())?,
        ];
        let credential = store.get_secret(SecretKey::GeminiApiKey);
        Ok::<_, &'static str>(AiSettings {
            providers: vec![AiProviderInfo {
                id: "gemini",
                display_name: "Gemini",
                configured: credential.as_ref().is_ok_and(|value| value.is_some()),
                supported_thinking_levels: ["low", "medium", "high"],
            }],
            roles,
            credential_store_available: credential.is_ok(),
        })
    })
    .await
    .map_err(|_| "worker_failed".to_string())?
    .map_err(str::to_owned)
}

#[tauri::command]
pub async fn update_cognitive_role_policy(
    db: State<'_, Database>,
    worker: State<'_, Arc<SummaryWorker>>,
    policy: CognitiveRolePolicy,
) -> Result<CognitiveRolePolicy, String> {
    policy.validate().map_err(str::to_owned)?;
    let db = db.inner().clone();
    let saved = tauri::async_runtime::spawn_blocking(move || {
        let mut conn = db.open().map_err(|e| e.code())?;
        policy::save(&mut conn, &policy).map_err(|e| e.code())
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
