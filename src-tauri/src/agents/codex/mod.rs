mod status;
mod app_server;

pub use status::CodexRuntimeStatus;

#[tauri::command]
pub async fn get_codex_runtime_status() -> Result<CodexRuntimeStatus, String> {
  status::get_codex_runtime_status().await
}

#[tauri::command]
pub async fn probe_codex_app_server() -> Result<app_server::CodexAppServerProbe, String> {
  app_server::probe_codex_app_server().await
}
