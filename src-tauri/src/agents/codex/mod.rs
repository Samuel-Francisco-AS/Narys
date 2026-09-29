mod status;
mod app_server;
pub mod backend;

pub use status::CodexRuntimeStatus;

#[tauri::command]
pub async fn get_codex_runtime_status() -> Result<CodexRuntimeStatus, String> {
  status::get_codex_runtime_status().await
}

#[tauri::command]
pub async fn probe_codex_app_server() -> Result<app_server::CodexAppServerProbe, String> {
  app_server::probe_codex_app_server().await
}

#[tauri::command]
pub async fn probe_codex_planner(
  registry: tauri::State<'_, std::sync::Arc<crate::agents::registry::AgentRegistry>>,
  objective: String,
) -> Result<crate::agents::planner::PlanV1, String> {
  planner_probe_response(backend::probe_planner(registry.inner(), objective).await)
}

fn planner_probe_response(result: Result<crate::agents::planner::PlanV1, backend::PlannerProbeError>)
  -> Result<crate::agents::planner::PlanV1, String> {
  result.map_err(|error| error.code().to_string())
}

#[tauri::command]
pub async fn probe_codex_planner_preflight() -> backend::PlannerPreflightProbe {
  backend::probe_preflight().await
}
