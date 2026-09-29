mod status;

pub use status::CodexRuntimeStatus;

#[tauri::command]
pub async fn get_codex_runtime_status() -> Result<CodexRuntimeStatus, String> {
  status::get_codex_runtime_status().await
}
